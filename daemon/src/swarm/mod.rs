mod artifacts;
mod admission;
mod availability;
mod benefit;
mod broker;
mod completion;
mod coverage;
mod control;
mod context;
mod director;
mod dispatch;
mod effects;
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
pub use coverage::report as coverage_report;
pub use control::{expire_due, expire_jobs_due, off, pause, resume};
pub use context::{artifact_chunk, director_summary, worker_brief};
pub use context::{grant_artifact, retry_revoked_interrupts, revoke_artifact};
pub use director::{claim_batch, complete_batch, recover};
pub use dispatch::next as dispatch_next;
pub use dispatch::recover_pending as recover_pending_dispatches;
pub use effects::{begin as begin_effect, reconcile as reconcile_effect};
pub use policy::preview;
pub use settings::set_policy;
pub use revision::revise;
pub use runtime::{interrupt_workers, launch_worker, liveness, reconcile_terminal_workers,
    reconcile_worker, retry_stopping_interrupts, sample_due_workers, sample_liveness};
pub use runtime::interrupt_workers_with_fault;
pub use scheduler::next as schedule_next;

use crate::store::Store;
use anyhow::{anyhow, bail, Result};
use plan::JobSpec;
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};

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

fn row_run(row: &rusqlite::Row<'_>) -> rusqlite::Result<Value> {
    let targets: String = row.get("allowed_targets")?;
    let policy: String = row.get("policy")?;
    Ok(json!({
        "id": row.get::<_, String>("id")?,
        "category": row.get::<_, String>("category")?,
        "objective": row.get::<_, String>("objective")?,
        "status": row.get::<_, String>("status")?,
        "stop_reason": row.get::<_, Option<String>>("stop_reason")?,
        "stalled_from": row.get::<_, Option<String>>("stalled_from")?,
        "stall_reason": row.get::<_, Option<String>>("stall_reason")?,
        "no_progress_turns": row.get::<_, i64>("no_progress_turns")?,
        "failed_planning_turns": row.get::<_, i64>("failed_planning_turns")?,
        "generation": row.get::<_, i64>("generation")?,
        "revision": row.get::<_, i64>("revision")?,
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
        "INSERT INTO swarm_runs(id,category,category_key,objective,status,generation,revision,allowed_targets,policy,created_ms,updated_ms) VALUES(?1,?2,?3,?4,'planning',1,0,?5,?6,?7,?7)",
        params![id,category,key,objective,targets.to_string(),policy.to_string(),now],
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
    let parsed: Result<(Vec<JobSpec>, Vec<Value>)> = (|| {
        let (jobs, rejected): (Vec<JobSpec>, Vec<Value>) = if p["allow_partial"] == true {
            plan::select_valid(&p["jobs"])?
        } else {
            (serde_json::from_value(p["jobs"].clone())
                .map_err(|e| anyhow!("invalid jobs: {e}"))?, Vec::new())
        };
        plan::validate(&jobs)?;
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
        let status = if job.deps.is_empty() {
            "ready"
        } else {
            "planned"
        };
        tx.execute(
            "INSERT INTO swarm_jobs(run_id,id,plan_revision,title,acceptance,deps,resource_claims,status,created_ms,updated_ms) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?9)",
            params![id,job.id,revision+1,job.title,job.acceptance,serde_json::to_string(&job.deps)?,serde_json::to_string(&job.resource_claims)?,status,now],
        )?;
    }
    tx.execute(
        "UPDATE swarm_runs SET revision=?2,failed_planning_turns=0,no_progress_turns=0,updated_ms=?3 WHERE id=?1",
        params![id, revision + 1, now],
    )?;
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
    let mut stmt = store
        .conn
        .prepare("SELECT * FROM swarm_jobs WHERE run_id=?1 AND id>?2 ORDER BY id LIMIT ?3")?;
    let rows = stmt.query_map(params![id,cursor,limit+1], |r| {
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
    stop_with_reason(store, p, "requested")
}

pub(super) fn stop_for_deadline(store: &mut Store, p: &Value) -> Result<Value> {
    stop_with_reason(store, p, "deadline")
}

fn stop_with_reason(store: &mut Store, p: &Value, reason: &str) -> Result<Value> {
    let id = required(p, "run_id")?;
    let generation = p["generation"]
        .as_i64()
        .ok_or_else(|| anyhow!("missing generation"))?;
    let revision = p["revision"]
        .as_i64()
        .ok_or_else(|| anyhow!("missing revision"))?;
    let current = get(store, id)?;
    if current["generation"] != generation {
        bail!("stale director generation");
    }
    if current["revision"] != revision {
        bail!("stale plan revision");
    }
    if current["status"] == "stopping" {
        return Ok(json!({"id":id,"status":"stopping","duplicate":true}));
    }
    if current["status"] == "stopped" || current["status"] == "completed" {
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
    tx.execute(
        "INSERT OR IGNORE INTO swarm_messages(run_id,message_id,job_id,attempt_id,sender,recipient,kind,revision,payload,phase,created_ms,updated_ms) SELECT a.run_id,'stop-'||a.id,a.job_id,a.id,'control',a.id,'stop',a.revision,'{}','queued',?2,?2 FROM swarm_attempts a WHERE a.run_id=?1 AND a.status='registered'",
        params![id,now],
    )?;
    record_operation(&tx, id, "stop")?;
    tx.commit()?;
    Ok(json!({"id":id,"status":"stopping","stop_reason":reason,"duplicate":false}))
}

pub fn claim(store: &mut Store, p: &Value) -> Result<Value> {
    let run = required(p, "run_id")?;
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
        "accepted",
    ]
    .contains(&status.as_str())
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
    for (owner_run, owner_job, owner_mode) in &existing {
        if owner_run == run && owner_job == job {
            if owner_mode == mode {
                return Ok(json!({"resource":resource,"mode":mode,"duplicate":true}));
            }
            bail!("claim conflict: mode change requires release and revalidation");
        }
        if mode == "write" || owner_mode == "write" {
            bail!("claim conflict on {resource}: owned by {owner_run}/{owner_job}");
        }
    }
    let now = crate::daemon::now();
    tx.execute("INSERT INTO swarm_claims(resource,run_id,job_id,mode,status,revision,created_ms,updated_ms) VALUES(?1,?2,?3,?4,'active',?5,?6,?6)",
        params![resource,run,job,mode,revision,now])?;
    tx.commit()?;
    Ok(json!({"resource":resource,"mode":mode,"duplicate":false}))
}
