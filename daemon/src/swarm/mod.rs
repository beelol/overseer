mod artifacts;
mod admission;
mod availability;
mod benefit;
mod broker;
mod completion;
mod conflicts;
mod coverage;
mod control;
mod context;
mod director;
mod owner;
mod dispatch;
mod effects;
mod integration;
mod limits;
mod verification;
mod plan;
mod policy;
mod revision;
mod runtime;
mod scheduler;
mod settings;
pub mod schema;
pub use artifacts::{confirm_exit, decide, put};
pub use admission::admit;
pub use availability::observe as observe_availability;
pub use benefit::preview as preview_benefit;
pub use benefit::commit as commit_benefit;
pub use broker::{ack, direct, messages, register, report};
pub use completion::complete;
pub use conflicts::{list as list_conflicts, open as open_conflict, resolve as resolve_conflict};
pub use coverage::report as coverage_report;
pub use control::{expire_due, expire_jobs_due, expire_redirects_due, off, pause, resume};
pub use context::{artifact_chunk, director_summary, worker_brief};
pub use context::{grant_artifact, retry_revoked_interrupts, revoke_artifact};
pub use director::{claim_batch, complete_batch, recover};
pub use owner::{begin as begin_director_owner, renew as renew_director_owner};
pub use owner::expire_due as expire_director_owners;
pub use owner::mark_uncertain_spawn as mark_uncertain_director_spawn;
pub use dispatch::next as dispatch_next;
pub use dispatch::recover_pending as recover_pending_dispatches;
pub use effects::{begin as begin_effect, reconcile as reconcile_effect};
pub use integration::integrate;
pub use limits::set as set_run_limit;
pub use verification::{prepare as prepare_verification, run as run_verification,
    record as record_verification, reconcile_control_verifications, PreparedVerification};
pub use policy::preview;
pub use settings::set_policy;
pub use revision::revise;
pub use runtime::{interrupt_workers, launch_worker, liveness, reconcile_terminal_workers,
    reconcile_worker, retry_contaminated_interrupts, retry_stopping_interrupts,
    sample_due_workers, sample_liveness};
pub use runtime::launch_director;
pub use runtime::refresh_linked_director_owners;
pub use runtime::interrupt_workers_with_fault;
pub use scheduler::next as schedule_next;

use crate::store::Store;
use anyhow::{anyhow, bail, Result};
use plan::JobSpec;
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};
use std::collections::BTreeMap;

fn required<'a>(p: &'a Value, key: &str) -> Result<&'a str> {
    p[key]
        .as_str()
        .ok_or_else(|| anyhow!("missing string parameter {key}"))
}

fn record_operation(conn: &rusqlite::Connection, run: &str, kind: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO swarm_operation_order(run_id,kind,created_ms) VALUES(?1,?2,?3)",
        params![run, kind, crate::daemon::now()],
    )?;
    Ok(())
}

/// Keep a bounded ready set while retaining the remaining eligible jobs durably as planned.
/// Called in the same transaction as every transition that opens or closes a ready slot.
fn materialize_ready(conn: &rusqlite::Connection, run: &str, now: i64) -> Result<()> {
    let (status, raw_policy): (String, String) = conn.query_row(
        "SELECT status,policy FROM swarm_runs WHERE id=?1", [run],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    if !["planning", "running", "paused"].contains(&status.as_str()) {
        return Ok(());
    }
    let policy: Value = serde_json::from_str(&raw_policy)?;
    let cap = policy["effective"]["ready_materialized_max"]
        .as_i64().unwrap_or(100).min(policy["effective"]["backlog_max"]
            .as_i64().unwrap_or(1000)).clamp(1, 10_000) as usize;
    let mut stmt = conn.prepare(
        "SELECT id FROM swarm_jobs WHERE run_id=?1 AND status='ready' ORDER BY id",
    )?;
    let ready = stmt.query_map([run], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(stmt);
    for id in ready.iter().skip(cap) {
        conn.execute("UPDATE swarm_jobs SET status='planned',updated_ms=?3 WHERE run_id=?1 AND id=?2",
            params![run,id,now])?;
    }
    let mut room = cap.saturating_sub(ready.len().min(cap));
    if room == 0 { return Ok(()); }
    let mut stmt = conn.prepare(
        "SELECT id,deps FROM swarm_jobs WHERE run_id=?1 AND status='planned' ORDER BY id",
    )?;
    let planned = stmt.query_map([run], |r| Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(stmt);
    for (id, raw_deps) in planned {
        if room == 0 { break; }
        let deps: Vec<String> = serde_json::from_str(&raw_deps)?;
        let mut eligible = true;
        for dep in deps {
            if !artifacts::dep_satisfied(conn, run, &dep)? {
                eligible = false;
                break;
            }
        }
        if eligible {
            conn.execute("UPDATE swarm_jobs SET status='ready',updated_ms=?3 WHERE run_id=?1 AND id=?2",
                params![run,id,now])?;
            room -= 1;
        }
    }
    Ok(())
}

/// A control request is terminal only after every linked process has a
/// confirmed exit and every admitted attempt is finished. Draining also waits
/// for submitted work to receive a director decision; neither path asserts a
/// successful audit without the separate evidence-gated completion call.
fn finalize_control_if_idle(conn: &rusqlite::Connection, run: &str, now: i64) -> Result<String> {
    let status: String = conn.query_row("SELECT status FROM swarm_runs WHERE id=?1", [run], |r| r.get(0))?;
    if status != "stopping" && status != "draining" {
        return Ok(status);
    }
    // Queued jobs cannot produce more work once control cancels them. Free
    // their claims even if a different job in this run is still draining.
    // Keep any claim whose attempt or external effect remains unresolved.
    conn.execute(
        "UPDATE swarm_claims SET status='released',updated_ms=?2
         WHERE run_id=?1 AND status='active'
           AND EXISTS (SELECT 1 FROM swarm_jobs j WHERE j.run_id=swarm_claims.run_id
                       AND j.id=swarm_claims.job_id AND j.status='cancelled')
           AND NOT EXISTS (SELECT 1 FROM swarm_attempts a WHERE a.run_id=swarm_claims.run_id
                           AND a.job_id=swarm_claims.job_id AND a.status='registered')
           AND NOT EXISTS (SELECT 1 FROM swarm_worker_launches l JOIN runs r ON r.id=l.overseer_run_id
                           WHERE l.run_id=swarm_claims.run_id AND l.job_id=swarm_claims.job_id
                           AND (r.ended_ms IS NULL OR r.status='disconnected'))
           AND NOT EXISTS (SELECT 1 FROM swarm_effects e WHERE e.run_id=swarm_claims.run_id
                           AND e.job_id=swarm_claims.job_id AND e.outcome IN ('unknown','applied'))",
        params![run, now],
    )?;
    let registered: i64 = conn.query_row(
        "SELECT COUNT(*) FROM swarm_attempts WHERE run_id=?1 AND status='registered'",
        [run], |r| r.get(0))?;
    let unconfirmed: i64 = conn.query_row(
        "SELECT COUNT(*) FROM swarm_worker_launches l JOIN runs r ON r.id=l.overseer_run_id
         WHERE l.run_id=?1 AND (r.ended_ms IS NULL OR r.status='disconnected')",
        [run], |r| r.get(0))?;
    let checking: i64 = conn.query_row(
        "SELECT COUNT(*) FROM swarm_verifications WHERE run_id=?1 AND status='running'",
        [run], |r| r.get(0))?;
    if registered != 0 || unconfirmed != 0 || checking != 0 {
        return Ok(status);
    }
    if status == "draining" {
        let pending: i64 = conn.query_row(
            "SELECT COUNT(*) FROM swarm_jobs WHERE run_id=?1
             AND status IN ('planned','ready','reserved','launching','running','submitted','cancel_requested')",
            [run], |r| r.get(0))?;
        if pending != 0 {
            return Ok(status);
        }
    }
    conn.execute("UPDATE swarm_runs SET status='stopped',updated_ms=?2 WHERE id=?1 AND status=?3",
        params![run,now,status])?;
    Ok("stopped".into())
}

fn row_run(row: &rusqlite::Row<'_>) -> rusqlite::Result<Value> {
    let targets: String = row.get("allowed_targets")?;
    let policy: String = row.get("policy")?;
    Ok(json!({
        "id": row.get::<_, String>("id")?,
        "category": row.get::<_, String>("category")?,
        "objective": row.get::<_, String>("objective")?,
        "source_change_permission": row.get::<_, String>("source_change_permission")?,
        "status": row.get::<_, String>("status")?,
        "stop_reason": row.get::<_, Option<String>>("stop_reason")?,
        "stalled_from": row.get::<_, Option<String>>("stalled_from")?,
        "stall_reason": row.get::<_, Option<String>>("stall_reason")?,
        "no_progress_turns": row.get::<_, i64>("no_progress_turns")?,
        "failed_planning_turns": row.get::<_, i64>("failed_planning_turns")?,
        "generation": row.get::<_, i64>("generation")?,
        "revision": row.get::<_, i64>("revision")?,
        "limit_revision": row.get::<_, i64>("limit_revision")?,
        "allowed_targets": serde_json::from_str::<Value>(&targets).unwrap_or(Value::Null),
        "needs_account_selection": serde_json::from_str::<Value>(&targets).ok().and_then(|v|v.as_array().map(|a|a.is_empty())).unwrap_or(true),
        "policy": serde_json::from_str::<Value>(&policy).unwrap_or(Value::Null),
        "created_ms": row.get::<_, i64>("created_ms")?,
        "updated_ms": row.get::<_, i64>("updated_ms")?,
    }))
}

pub fn create(store: &mut Store, p: &Value) -> Result<Value> {
    let category = required(p, "category")?.trim();
    let objective = required(p, "objective")?.trim();
    if category.is_empty() || category.len() > 160 || objective.is_empty() || objective.len() > 8000
    {
        bail!("category or objective is empty or too long");
    }
    if crate::redact::redact(category) != category || crate::redact::redact(objective) != objective {
        bail!("category or objective contains sensitive text");
    }
    let source_change_permission = p.get("source_change_permission")
        .map(Value::as_str)
        .unwrap_or(Some("none"))
        .ok_or_else(|| anyhow!("invalid source change permission"))?;
    if !["none", "isolated"].contains(&source_change_permission) {
        bail!("invalid source change permission");
    }
    let key = category.to_lowercase();
    let occupied: bool = store.conn.query_row(
        "SELECT 1 FROM swarm_runs WHERE category_key=?1 AND status IN ('planning','running','paused','stalled','draining','stopping') LIMIT 1",
        params![key], |_| Ok(()),
    ).optional()?.is_some();
    if occupied {
        bail!("category already has an active swarm run");
    }
    let (policy,targets) = settings::resolve(store,category,p.get("policy"),p.get("allowed_targets"))?;
    let id = format!("sw-{}", &uuid::Uuid::new_v4().simple().to_string()[..12]);
    let now = crate::daemon::now();
    store.conn.execute(
        "INSERT INTO swarm_runs(id,category,category_key,objective,source_change_permission,status,generation,revision,allowed_targets,policy,created_ms,updated_ms) VALUES(?1,?2,?3,?4,?5,'planning',1,0,?6,?7,?8,?8)",
        params![id,category,key,objective,source_change_permission,targets.to_string(),policy.to_string(),now],
    )?;
    get(store, &id)
}

pub fn get(store: &Store, id: &str) -> Result<Value> {
    let mut run = store
        .conn
        .query_row("SELECT * FROM swarm_runs WHERE id=?1", params![id], row_run)
        .optional()?
        .ok_or_else(|| anyhow!("unknown swarm run {id}"))?;
    run["completion"] = completion::get(store, id)?;
    run["availability"] = availability::get(store, id)?;
    run["benefit"] = benefit::get_state(store, id, run["revision"].as_i64().unwrap_or(0))?;
    let mut job_counts = BTreeMap::<String, i64>::new();
    let mut stmt = store.conn.prepare(
        "SELECT status,COUNT(*) FROM swarm_jobs WHERE run_id=?1 GROUP BY status",
    )?;
    for row in stmt.query_map([id], |row| Ok((row.get::<_,String>(0)?,row.get::<_,i64>(1)?)))? {
        let (status, count) = row?;
        job_counts.insert(status, count);
    }
    run["job_counts"] = json!({"total":job_counts.values().sum::<i64>(),
        "by_status":job_counts});
    run["registered_attempts"] = json!(store.conn.query_row(
        "SELECT COUNT(*) FROM swarm_attempts WHERE run_id=?1 AND status='registered'",
        [id], |row| row.get::<_,i64>(0))?);
    // A job marked running or an attempt still registered does not prove a
    // supervised process is running. Count only linked runs reported running
    // by the daemon, separately from job and attempt states.
    run["active_worker_processes"] = json!(store.conn.query_row(
        "SELECT COUNT(*) FROM swarm_worker_launches l JOIN runs r ON r.id=l.overseer_run_id
         WHERE l.run_id=?1 AND r.ended_ms IS NULL AND r.status IN
         ('running','waiting_for_user')",
        [id], |row| row.get::<_,i64>(0))?);
    run["director"] = store.conn.query_row(
        "SELECT o.status,o.overseer_run_id,r.status FROM swarm_director_owners o
         LEFT JOIN runs r ON r.id=o.overseer_run_id WHERE o.run_id=?1",
        [id], |row| Ok(json!({"owner_status":row.get::<_,String>(0)?,
            "overseer_run_id":row.get::<_,Option<String>>(1)?,
            "process_status":row.get::<_,Option<String>>(2)?})),
    ).optional()?.unwrap_or(Value::Null);
    if run["status"] == "stopping" {
        let count: i64 = store.conn.query_row(
            "SELECT COUNT(*) FROM swarm_worker_launches l JOIN runs r ON r.id=l.overseer_run_id
             WHERE l.run_id=?1 AND (r.ended_ms IS NULL OR r.status='disconnected')",
            params![id], |row| row.get(0))?;
        let mut stmt = store.conn.prepare(
            "SELECT l.job_id,l.attempt_id,l.overseer_run_id,r.status,
                    COALESCE(s.last_outcome,'not_attempted'),COALESCE(s.attempts,0),s.last_attempt_ms
             FROM swarm_worker_launches l JOIN runs r ON r.id=l.overseer_run_id
             LEFT JOIN swarm_stop_signals s ON s.run_id=l.run_id AND s.overseer_run_id=r.id
             WHERE l.run_id=?1 AND (r.ended_ms IS NULL OR r.status='disconnected')
             ORDER BY l.created_ms,l.attempt_id LIMIT 100")?;
        let exits = stmt.query_map(params![id], |row| Ok(json!({
            "job_id":row.get::<_,String>(0)?,"attempt_id":row.get::<_,String>(1)?,
            "overseer_run_id":row.get::<_,String>(2)?,"worker_status":row.get::<_,String>(3)?,
            "last_signal_outcome":row.get::<_,String>(4)?,"signal_attempts":row.get::<_,i64>(5)?,
            "last_signal_ms":row.get::<_,Option<i64>>(6)?
        })))?.collect::<rusqlite::Result<Vec<_>>>()?;
        run["unconfirmed_exit_count"] = json!(count);
        run["unconfirmed_exits_truncated"] = json!(count > exits.len() as i64);
        run["unconfirmed_exits"] = json!(exits);
    } else {
        run["unconfirmed_exit_count"] = json!(0);
        run["unconfirmed_exits_truncated"] = json!(false);
        run["unconfirmed_exits"] = json!([]);
    }
    Ok(run)
}

pub fn plan(store: &mut Store, p: &Value) -> Result<Value> {
    let id = required(p, "id")?;
    owner::require(store,id,p)?;
    let generation = p["generation"]
        .as_i64()
        .ok_or_else(|| anyhow!("missing generation"))?;
    let revision = p["revision"]
        .as_i64()
        .ok_or_else(|| anyhow!("missing revision"))?;
    let prior = get(store, id)?;
    if prior["generation"] != generation {
        bail!("stale director generation");
    }
    if prior["revision"] != revision {
        bail!("stale plan revision");
    }
    if !["planning", "running", "paused"].contains(&prior["status"].as_str().unwrap_or("")) {
        bail!("swarm run is not plannable");
    }
    let backlog_limit = prior["policy"]["effective"]["backlog_max"]
        .as_u64().unwrap_or(1000).min(10_000) as usize;
    let parsed: Result<(Vec<JobSpec>, Vec<Value>)> = (|| {
        let (jobs, rejected): (Vec<JobSpec>, Vec<Value>) = if p["allow_partial"] == true {
            plan::select_valid(&p["jobs"], backlog_limit)?
        } else {
            (serde_json::from_value(p["jobs"].clone())
                .map_err(|e| anyhow!("invalid jobs: {e}"))?, Vec::new())
        };
        plan::validate(&jobs, backlog_limit)?;
        Ok((jobs, rejected))
    })();
    let (jobs, rejected) = match parsed {
        Ok(valid) => valid,
        Err(error) => {
            record_planning_failure(store, id)?;
            return Err(error);
        }
    };
    let tx = store.conn.transaction()?;
    let current: (i64, i64, String) = tx
        .query_row(
            "SELECT generation,revision,status FROM swarm_runs WHERE id=?1",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?
        .ok_or_else(|| anyhow!("unknown swarm run {id}"))?;
    if generation != current.0 {
        bail!("stale director generation");
    }
    if revision != current.1 {
        bail!("stale plan revision");
    }
    if !["planning", "running", "paused"].contains(&current.2.as_str()) {
        bail!("swarm run is not plannable");
    }
    let progressed: bool = tx.query_row(
        "SELECT 1 FROM swarm_jobs WHERE run_id=?1 AND status NOT IN ('planned','ready') LIMIT 1",
        params![id], |_| Ok(()),
    ).optional()?.is_some();
    if progressed {
        bail!("cannot replace a plan with active or completed jobs; use a revision transition");
    }
    let mut stmt = tx.prepare("SELECT id,title,acceptance,deps,resource_claims FROM swarm_jobs WHERE run_id=?1 ORDER BY id")?;
    let rows = stmt.query_map(params![id], |r| {
        Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,
            r.get::<_,String>(2)?,r.get::<_,String>(3)?,r.get::<_,String>(4)?))
    })?.collect::<rusqlite::Result<Vec<_>>>()?;
    drop(stmt);
    let existing = rows.into_iter().map(|(id,title,acceptance,deps,resource_claims)| {
        Ok(JobSpec { id,title,acceptance,deps:serde_json::from_str(&deps)?,
            resource_claims:serde_json::from_str(&resource_claims)? })
    }).collect::<Result<Vec<_>>>()?;
    let mut proposed = jobs.clone();
    proposed.sort_by(|a,b| a.id.cmp(&b.id));
    if revision > 0 && existing == proposed {
        return Ok(json!({"id":id,"generation":generation,"revision":revision,
            "job_count":jobs.len(),"rejected":rejected,"unchanged":true}));
    }
    tx.execute("DELETE FROM swarm_jobs WHERE run_id=?1", params![id])?;
    let now = crate::daemon::now();
    for job in &jobs {
        let status = "planned";
        tx.execute(
            "INSERT INTO swarm_jobs(run_id,id,plan_revision,title,acceptance,deps,resource_claims,status,created_ms,updated_ms) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?9)",
            params![id,job.id,revision+1,job.title,job.acceptance,serde_json::to_string(&job.deps)?,serde_json::to_string(&job.resource_claims)?,status,now],
        )?;
    }
    tx.execute(
        "UPDATE swarm_runs SET revision=?2,failed_planning_turns=0,no_progress_turns=0,updated_ms=?3 WHERE id=?1",
        params![id, revision + 1, now],
    )?;
    materialize_ready(&tx, id, now)?;
    tx.commit()?;
    Ok(json!({"id":id,"generation":generation,"revision":revision+1,"job_count":jobs.len(),"rejected":rejected}))
}

pub(super) fn record_planning_failure(store: &mut Store, id: &str) -> Result<()> {
    let now = crate::daemon::now();
    let tx = store.conn.transaction()?;
    tx.execute("UPDATE swarm_runs SET failed_planning_turns=failed_planning_turns+1,
        updated_ms=?2 WHERE id=?1",params![id,now])?;
    tx.execute("UPDATE swarm_runs SET stalled_from=status,status='stalled',
        stall_reason='planning_failed',updated_ms=?2 WHERE id=?1 AND failed_planning_turns>=2",
        params![id,now])?;
    tx.commit()?;
    Ok(())
}

pub fn jobs(store: &Store, p: &Value) -> Result<Value> {
    let id = required(p, "id")?;
    get(store, id)?;
    let cursor = p["cursor"].as_str().unwrap_or("");
    let limit = p["limit"].as_i64().unwrap_or(50).clamp(1, 100);
    let status = match p.get("status") {
        None | Some(Value::Null) => None,
        Some(Value::String(status)) if ["planned","ready","reserved","launching",
            "running","submitted","blocked","accepted","rejected","failed","cancel_requested",
            "cancelled"].contains(&status.as_str()) => Some(status.as_str()),
        _ => bail!("invalid job status filter"),
    };
    let mut stmt = store
        .conn
        .prepare("SELECT * FROM swarm_jobs WHERE run_id=?1 AND id>?2
            AND (?3 IS NULL OR status=?3) ORDER BY id LIMIT ?4")?;
    let rows = stmt.query_map(params![id,cursor,status,limit+1], |r| {
        let deps: String = r.get("deps")?;
        let resource_claims: String = r.get("resource_claims")?;
        Ok(json!({
            "id":r.get::<_,String>("id")?,"run_id":r.get::<_,String>("run_id")?,
            "plan_revision":r.get::<_,i64>("plan_revision")?,
            "title":r.get::<_,String>("title")?,"acceptance":r.get::<_,String>("acceptance")?,
            "deps":serde_json::from_str::<Value>(&deps).unwrap_or(Value::Null),
            "resource_claims":serde_json::from_str::<Value>(&resource_claims).unwrap_or(Value::Null),
            "status":r.get::<_,String>("status")?,"attempt_count":r.get::<_,i64>("attempt_count")?,
            "deadline_at_ms":r.get::<_,Option<i64>>("deadline_at_ms")?,
            "stop_reason":r.get::<_,Option<String>>("stop_reason")?,
        }))
    })?.collect::<rusqlite::Result<Vec<_>>>()?;
    let has_more = rows.len() as i64 > limit;
    let page: Vec<Value> = rows.into_iter().take(limit as usize).collect();
    let next_cursor = if has_more {
        page.last().and_then(|j| j["id"].as_str())
    } else {
        None
    };
    Ok(json!({"jobs":page,"next_cursor":next_cursor}))
}

pub fn stop(store: &mut Store, p: &Value) -> Result<Value> {
    stop_with_reason(store, p, "requested", false)
}

pub(super) fn stop_for_deadline(store: &mut Store, p: &Value) -> Result<Value> {
    stop_with_reason(store, p, "deadline", true)
}

fn stop_with_reason(store: &mut Store, p: &Value, reason: &str, require_version: bool) -> Result<Value> {
    let id = required(p, "run_id")?;
    let current = get(store, id)?;
    // Stop is a user safety control. A stale view must not prevent it; only
    // internal deadline transitions carry a director-version precondition.
    if require_version {
        let generation = p["generation"]
            .as_i64()
            .ok_or_else(|| anyhow!("missing generation"))?;
        let revision = p["revision"]
            .as_i64()
            .ok_or_else(|| anyhow!("missing revision"))?;
        if current["generation"] != generation {
            bail!("stale director generation");
        }
        if current["revision"] != revision {
            bail!("stale plan revision");
        }
    }
    if current["status"] == "stopping" {
        return Ok(json!({"id":id,"status":"stopping","duplicate":true}));
    }
    if current["status"] == "stopped" && !current["stop_reason"].is_null() {
        return Ok(json!({"id":id,"status":"stopped","duplicate":true}));
    }
    if ["stopped","completed","invalidated"].contains(&current["status"].as_str().unwrap_or("")) {
        bail!("swarm run is terminal");
    }
    let now = crate::daemon::now();
    let tx = store.conn.transaction()?;
    tx.execute(
        "UPDATE swarm_runs SET status='stopping',stop_reason=?3,updated_ms=?2 WHERE id=?1",
        params![id, now, reason],
    )?;
    tx.execute("UPDATE swarm_jobs SET status='cancelled',updated_ms=?2 WHERE run_id=?1 AND status IN ('planned','ready')", params![id,now])?;
    tx.execute("UPDATE swarm_jobs SET status='cancel_requested',updated_ms=?2 WHERE run_id=?1 AND status IN ('reserved','launching','running')", params![id,now])?;
    if reason == "deadline" {
        // Keep the checkpoint request and Stop in the same durable control
        // transition. A worker may be interrupted before it can answer, but
        // the request and any resulting partial evidence remain reviewable.
        tx.execute(
            "INSERT OR IGNORE INTO swarm_messages(run_id,message_id,job_id,attempt_id,sender,recipient,kind,revision,payload,phase,created_ms,updated_ms)
             SELECT a.run_id,'deadline-checkpoint-'||a.id,a.job_id,a.id,'control',a.id,
                    'checkpoint',a.revision,'{\"reason\":\"run_deadline\"}','queued',?2,?2
             FROM swarm_attempts a WHERE a.run_id=?1 AND a.status='registered'",
            params![id,now],
        )?;
    }
    tx.execute(
        "INSERT OR IGNORE INTO swarm_messages(run_id,message_id,job_id,attempt_id,sender,recipient,kind,revision,payload,phase,created_ms,updated_ms) SELECT a.run_id,'stop-'||a.id,a.job_id,a.id,'control',a.id,'stop',a.revision,'{}','queued',?2,?2 FROM swarm_attempts a WHERE a.run_id=?1 AND a.status='registered'",
        params![id,now],
    )?;
    record_operation(&tx, id, "stop")?;
    let status = finalize_control_if_idle(&tx,id,now)?;
    tx.commit()?;
    Ok(json!({"id":id,"status":status,"stop_reason":reason,"duplicate":false}))
}

pub fn claim(store: &mut Store, p: &Value) -> Result<Value> {
    let run = required(p, "run_id")?;
    owner::require(store,run,p)?;
    let job = required(p, "job_id")?;
    let resource = required(p, "resource")?.trim();
    let mode = required(p, "mode")?;
    let generation = p["generation"]
        .as_i64()
        .ok_or_else(|| anyhow!("missing generation"))?;
    let revision = p["revision"]
        .as_i64()
        .ok_or_else(|| anyhow!("missing revision"))?;
    if resource.is_empty() || resource.len() > 512 || resource.chars().any(char::is_control) {
        bail!("invalid resource claim");
    }
    if mode != "read" && mode != "write" {
        bail!("claim mode must be read or write");
    }
    let current = get(store, run)?;
    if current["generation"] != generation {
        bail!("stale director generation");
    }
    if current["revision"] != revision {
        bail!("stale plan revision");
    }
    if current["status"] != "planning" && current["status"] != "running" {
        bail!("swarm run does not permit claims");
    }
    let after_use = p["after_use"] == true;
    let observed_attempt = if after_use {
        let attempt = required(p, "attempt_id")?;
        let token = required(p, "token")?;
        broker::check_attempt(store, run, job, attempt, token)?;
        let previous: i64 = store.conn.query_row(
            "SELECT COUNT(*) FROM swarm_resource_contamination WHERE run_id=?1 AND job_id=?2
             AND attempt_id=?3 AND resource=?4",
            params![run,job,attempt,resource], |r| r.get(0))?;
        if previous > 0 {
            return Ok(json!({"resource":resource,"mode":mode,"status":"contaminated","duplicate":true}));
        }
        let active = store.conn.prepare("SELECT 1 FROM swarm_attempts WHERE id=?1 AND status='registered'")?
            .exists([attempt])?;
        if !active {
            bail!("late resource observation requires an active attempt");
        }
        Some(attempt)
    } else { None };
    let status: String = store
        .conn
        .query_row(
            "SELECT status FROM swarm_jobs WHERE run_id=?1 AND id=?2",
            params![run, job],
            |r| r.get(0),
        )
        .optional()?
        .ok_or_else(|| anyhow!("unknown job"))?;
    if [
        "cancelled",
        "cancel_requested",
        "superseded",
        "failed",
    ]
    .contains(&status.as_str())
        || (status == "accepted" && !after_use)
    {
        bail!("job cannot claim a resource in this state");
    }
    let tx = store.conn.transaction()?;
    let mut stmt = tx.prepare(
        "SELECT run_id,job_id,mode FROM swarm_claims WHERE resource=?1 AND status='active'",
    )?;
    let existing = stmt
        .query_map(params![resource], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(stmt);
    let mut conflicts = Vec::new();
    let mut unconfirmed_owners = Vec::new();
    let mut own_mode = None;
    for (owner_run, owner_job, owner_mode) in &existing {
        if owner_run == run && owner_job == job {
            own_mode = Some(owner_mode.as_str());
            continue;
        }
        if mode == "write" || owner_mode == "write" {
            if !after_use {
                bail!("claim conflict on {resource}: owned by {owner_run}/{owner_job}");
            }
            let peer_attempt: Option<String> = tx.query_row(
                "SELECT id FROM swarm_attempts WHERE run_id=?1 AND job_id=?2 AND status='registered'
                 ORDER BY created_ms DESC LIMIT 1",
                params![owner_run,owner_job], |r| r.get(0)).optional()?;
            if let Some(peer_attempt) = peer_attempt {
                conflicts.push((owner_run.clone(),owner_job.clone(),peer_attempt));
            } else {
                unconfirmed_owners.push((owner_run.clone(),owner_job.clone()));
            }
        }
    }
    if after_use {
        // A mutable database can retain another worker's writes after that worker exits
        // and its admission claim is released. Keep observed use across that boundary.
        let mut observed = tx.prepare(
            "SELECT run_id,job_id,attempt_id,mode FROM swarm_resource_observations
             WHERE resource=?1 AND NOT (run_id=?2 AND job_id=?3)"
        )?;
        let peers = observed.query_map(params![resource,run,job], |r| {
            Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,
                r.get::<_,String>(2)?,r.get::<_,String>(3)?))
        })?.collect::<rusqlite::Result<Vec<_>>>()?;
        drop(observed);
        for (peer_run,peer_job,peer_attempt,peer_mode) in peers {
            if (mode == "write" || peer_mode == "write")
                && !conflicts.iter().any(|(r,j,a)| r == &peer_run && j == &peer_job && a == &peer_attempt) {
                conflicts.push((peer_run,peer_job,peer_attempt));
            }
        }
    }
    if own_mode.is_some_and(|held| held != mode) && !after_use {
        bail!("claim conflict: mode change requires release and revalidation");
    }
    let now = crate::daemon::now();
    if let Some(attempt) = observed_attempt {
        tx.execute("INSERT INTO swarm_resource_observations
            (run_id,job_id,attempt_id,resource,mode,created_ms,updated_ms)
            VALUES(?1,?2,?3,?4,?5,?6,?6)
            ON CONFLICT(attempt_id,resource) DO UPDATE SET
              mode=CASE WHEN mode='write' OR excluded.mode='write' THEN 'write' ELSE 'read' END,
              updated_ms=excluded.updated_ms",
            params![run,job,attempt,resource,mode,now])?;
        if !conflicts.is_empty() || !unconfirmed_owners.is_empty() {
            for (peer_run,peer_job,peer_attempt) in &conflicts {
                for (affected_run,affected_job,affected_attempt,other_run,other_job) in [
                    (run,job,attempt,peer_run.as_str(),peer_job.as_str()),
                    (peer_run.as_str(),peer_job.as_str(),peer_attempt.as_str(),run,job),
                ] {
                    tx.execute("INSERT OR IGNORE INTO swarm_resource_contamination
                        (run_id,job_id,attempt_id,resource,peer_run_id,peer_job_id,created_ms)
                        VALUES(?1,?2,?3,?4,?5,?6,?7)",
                        params![affected_run,affected_job,affected_attempt,resource,other_run,other_job,now])?;
                    tx.execute("UPDATE swarm_jobs SET status='cancel_requested',
                        stop_reason='resource_contamination',updated_ms=?3
                        WHERE run_id=?1 AND id=?2 AND status IN ('reserved','launching','running','submitted')",
                        params![affected_run,affected_job,now])?;
                    tx.execute("UPDATE swarm_jobs SET status='blocked',
                        stop_reason='resource_contamination',updated_ms=?3
                        WHERE run_id=?1 AND id=?2 AND status='accepted'",
                        params![affected_run,affected_job,now])?;
                    tx.execute("INSERT OR IGNORE INTO swarm_completion_invalidations
                        (run_id,reason,resource,created_ms)
                        SELECT run_id,'resource_contamination',?2,?3
                        FROM swarm_completions WHERE run_id=?1",
                        params![affected_run,resource,now])?;
                    tx.execute("UPDATE swarm_runs SET status='invalidated',
                        stop_reason='resource_contamination',updated_ms=?2
                        WHERE id=?1 AND status='completed'",
                        params![affected_run,now])?;
                    tx.execute("INSERT OR IGNORE INTO swarm_messages
                        (run_id,message_id,job_id,attempt_id,sender,recipient,kind,revision,payload,phase,created_ms,updated_ms)
                        SELECT a.run_id,'contamination-'||a.id,a.job_id,a.id,'control',a.id,'stop',a.revision,?2,'queued',?3,?3
                        FROM swarm_attempts a WHERE a.id=?1 AND a.status='registered'",
                        params![affected_attempt,json!({"resource":resource,"reason":"resource_contamination"}).to_string(),now])?;
                }
            }
            // A planned owner with no registered attempt cannot prove that it used the
            // resource. Keep the observation and quarantine the reporting attempt;
            // hold the owner before it can launch until a director resolves the claim.
            for (peer_run,peer_job) in &unconfirmed_owners {
                tx.execute("INSERT OR IGNORE INTO swarm_resource_contamination
                    (run_id,job_id,attempt_id,resource,peer_run_id,peer_job_id,created_ms)
                    VALUES(?1,?2,?3,?4,?5,?6,?7)",
                    params![run,job,attempt,resource,peer_run,peer_job,now])?;
                tx.execute("UPDATE swarm_jobs SET status='cancel_requested',
                    stop_reason='resource_contamination',updated_ms=?3
                    WHERE run_id=?1 AND id=?2 AND status IN ('reserved','launching','running','submitted')",
                    params![run,job,now])?;
                tx.execute("UPDATE swarm_jobs SET status='blocked',
                    stop_reason='resource_contamination',updated_ms=?3
                    WHERE run_id=?1 AND id=?2 AND status='accepted'",
                    params![run,job,now])?;
                tx.execute("UPDATE swarm_jobs SET status='blocked',
                    stop_reason='resource_overlap_unconfirmed',updated_ms=?3
                    WHERE run_id=?1 AND id=?2 AND status IN ('planned','ready','reserved','launching','running','submitted','accepted')",
                    params![peer_run,peer_job,now])?;
                tx.execute("INSERT OR IGNORE INTO swarm_messages
                    (run_id,message_id,job_id,attempt_id,sender,recipient,kind,revision,payload,phase,created_ms,updated_ms)
                    SELECT a.run_id,'contamination-'||a.id,a.job_id,a.id,'control',a.id,'stop',a.revision,?2,'queued',?3,?3
                    FROM swarm_attempts a WHERE a.id=?1 AND a.status='registered'",
                    params![attempt,json!({"resource":resource,"reason":"resource_overlap_unconfirmed"}).to_string(),now])?;
            }
            tx.commit()?;
            return Ok(json!({"resource":resource,"mode":mode,"status":"contaminated",
                "affected":conflicts.len()+unconfirmed_owners.len()+1,
                "unconfirmed_planned_owners":unconfirmed_owners.len(),"duplicate":false}));
        }
    }
    if let Some(held) = own_mode {
        if held == mode || held == "write" {
            tx.commit()?;
            return Ok(json!({"resource":resource,"mode":held,"duplicate":true}));
        }
        tx.execute("UPDATE swarm_claims SET mode='write',updated_ms=?4
            WHERE resource=?1 AND run_id=?2 AND job_id=?3 AND status='active'",
            params![resource,run,job,now])?;
    } else {
        tx.execute("INSERT INTO swarm_claims(resource,run_id,job_id,mode,status,revision,created_ms,updated_ms)
            VALUES(?1,?2,?3,?4,'active',?5,?6,?6)
            ON CONFLICT(resource,run_id,job_id) DO UPDATE SET
                mode=excluded.mode,status='active',revision=excluded.revision,updated_ms=excluded.updated_ms",
            params![resource,run,job,mode,revision,now])?;
    }
    tx.commit()?;
    Ok(json!({"resource":resource,"mode":mode,"duplicate":own_mode.is_some_and(|held| held==mode)}))
}
