//! Scripted worker launch bridge. This remains fixture-only until Auto Mode supplies a
//! daemon-owned target and each live adapter proves directive/descendant control.

use super::{broker, get, required};
use crate::daemon::{Daemon, SwarmDirectorIdentity, SwarmWorkerIdentity};
use crate::store::Store;
use anyhow::{anyhow, bail, Result};
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

/// Extend a supervised director lease only after its linked local supervisor
/// answers a control ping. An unlinked, exited, disconnected, or already
/// expired owner remains uncertain and must go through reconciliation.
pub fn refresh_linked_director_owners(d: &Arc<Daemon>) -> Result<usize> {
    let now = crate::daemon::now();
    let due = {
        let store = d.store.lock().unwrap();
        let mut stmt = store.conn.prepare(
            "SELECT o.run_id,o.generation,o.overseer_run_id FROM swarm_director_owners o
             JOIN swarm_runs s ON s.id=o.run_id AND s.generation=o.generation
             JOIN runs r ON r.id=o.overseer_run_id
             WHERE o.status='active' AND o.supervised_launch=1 AND o.launch_phase='spawn_requested'
             AND o.lease_expires_ms>?1 AND o.lease_expires_ms<=?2
             AND s.status IN ('planning','running','paused','draining')
             AND r.run_dir IS NOT NULL AND r.ended_ms IS NULL
             AND r.status IN ('queued','starting','running','waiting_for_user')
             ORDER BY o.lease_expires_ms LIMIT 4")?;
        let rows = stmt.query_map(params![now,now+15_000], |r| {
            Ok((r.get::<_,String>(0)?,r.get::<_,i64>(1)?,r.get::<_,String>(2)?))
        })?.collect::<rusqlite::Result<Vec<_>>>()?;
        rows
    };
    let mut renewed = 0;
    for (run, generation, process) in due {
        let director = d.run(&process)?;
        if !["queued","starting","running","waiting_for_user"].contains(&director.status.as_str()) {
            continue;
        }
        let reachable = d.control_socket(&director).ok()
            .and_then(|socket| crate::shim::control_with_timeout(
                &socket, &json!({"op":"ping"}), Duration::from_millis(250)).ok())
            .is_some_and(|reply| reply["ok"] == true);
        if !reachable {
            continue;
        }
        let now = crate::daemon::now();
        renewed += d.store.lock().unwrap().conn.execute(
            "UPDATE swarm_director_owners SET renewed_ms=?4,lease_expires_ms=?5
             WHERE run_id=?1 AND generation=?2 AND overseer_run_id=?3
             AND status='active' AND supervised_launch=1 AND launch_phase='spawn_requested'
             AND lease_expires_ms>?4
             AND EXISTS (SELECT 1 FROM swarm_runs s WHERE s.id=?1 AND s.generation=?2
                 AND s.status IN ('planning','running','paused','draining'))
             AND EXISTS (SELECT 1 FROM runs r WHERE r.id=?3 AND r.run_dir IS NOT NULL
                 AND r.ended_ms IS NULL AND r.status IN ('queued','starting','running','waiting_for_user'))",
            params![run,generation,process,now,now+30_000])?;
    }
    Ok(renewed)
}

const UNKNOWN_AFTER_MS: i64 = 60_000;
const SAMPLE_INTERVAL_MS: i64 = 15_000;
const SUSPECT_RETRY_MS: i64 = 1_000;
const STOP_RETRY_MS: i64 = 5_000;

/// Bind the owner before starting a supervised scripted process. The returned
/// result contains only the linked process ID; the credential stays in the
/// private 0600 launch file and process environment.
pub fn launch_director(d: &Arc<Daemon>, p: &Value) -> Result<Value> {
    let _serial = d.swarm_launch_lock.lock().unwrap();
    if d.swarm_storage_blocked.load(Ordering::SeqCst) {
        bail!("swarm storage is blocked; recover write capacity before launching new work");
    }
    let run = required(p,"run_id")?;
    let generation = p["generation"].as_i64().ok_or_else(|| anyhow!("missing generation"))?;
    let repo = required(p,"repo")?;
    let program = required(p,"program")?;
    let prompt = required(p,"prompt")?;
    let title = required(p,"title")?;
    let args = p["args"].as_array().ok_or_else(|| anyhow!("args must be an array"))?;
    if program.is_empty() || !program.starts_with('/') || program.len()>1024
        || prompt.is_empty() || prompt.len()>8000 || title.is_empty() || title.len()>200
        || args.len()>32 || args.iter().any(|v| v.as_str().is_none_or(|s| s.len()>4096)) {
        bail!("invalid scripted director launch");
    }
    crate::git::toplevel(std::path::Path::new(repo))?;
    let owner = super::owner::begin(&mut d.store.lock().unwrap(),
        &json!({"run_id":run,"generation":generation,"supervised_launch":true}))?;
    let token = owner["owner_token"].as_str().unwrap();
    let task = d.create_task_for_swarm_director(&json!({
        "repo":repo,"harness":"generic","workspace_mode":"worktree",
        "program":program,"args":args,"prompt":prompt,"title":title,
    }), &SwarmDirectorIdentity { run_id:run.to_string(),generation,token:token.to_string() })?;
    let process = task["run"]["id"].as_str()
        .ok_or_else(|| anyhow!("director run was not recorded"))?;
    if !task["launch_error"].is_null() {
        let status = if task["launch_uncertain"] == true { "launch_uncertain" }
            else { "launch_failed" };
        return Ok(json!({"status":status,"overseer_run_id":process,
            "error":task["launch_error"]}));
    }
    Ok(json!({"status":"launched","overseer_run_id":process}))
}

fn sample_interval(state: &str) -> i64 {
    if state == "suspect" { SUSPECT_RETRY_MS } else { SAMPLE_INTERVAL_MS }
}

pub fn liveness(store: &Store, p: &Value) -> Result<Value> {
    let run = required(p, "run_id")?;
    let job = required(p, "job_id")?;
    let attempt = required(p, "attempt_id")?;
    let row: Option<(String, Option<i64>, i64)> = store.conn.query_row(
        "SELECT l.state,l.unreachable_since_ms,l.last_sample_ms FROM swarm_worker_liveness l
         JOIN swarm_attempts a ON a.id=l.attempt_id
         WHERE l.attempt_id=?1 AND a.run_id=?2 AND a.job_id=?3",
        params![attempt,run,job],
        |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?)),
    ).optional()?;
    Ok(match row {
        Some((state,since,last)) => json!({"state":state,"unreachable_since_ms":since,
            "last_sample_ms":last,"sample_interval_ms":sample_interval(&state),
            "unknown_after_ms":UNKNOWN_AFTER_MS}),
        None => json!({"state":"unobserved","unreachable_since_ms":null,
            "last_sample_ms":null,"sample_interval_ms":SAMPLE_INTERVAL_MS,
            "unknown_after_ms":UNKNOWN_AFTER_MS}),
    })
}

/// One deterministic reachability observation. Only a daemon-owned sampler may call this
/// in normal operation; the protocol exposes it solely behind fixture opt-in.
pub fn sample_liveness(store: &mut Store, p: &Value) -> Result<Value> {
    let run = required(p, "run_id")?;
    let job = required(p, "job_id")?;
    let attempt = required(p, "attempt_id")?;
    let now = p["now_ms"].as_i64().ok_or_else(|| anyhow!("missing sample time"))?;
    let reachable = p["reachable"].as_bool().ok_or_else(|| anyhow!("missing reachability"))?;
    if now < 0 {
        bail!("invalid sample time");
    }
    let linked = store.conn.prepare(
        "SELECT 1 FROM swarm_attempts a JOIN swarm_worker_launches l ON l.attempt_id=a.id
         WHERE a.id=?1 AND a.run_id=?2 AND a.job_id=?3 AND a.status='registered'
         AND l.overseer_run_id IS NOT NULL"
    )?.exists(params![attempt,run,job])?;
    if !linked {
        bail!("unknown or inactive linked worker attempt");
    }
    let prior: Option<(Option<i64>,i64)> = store.conn.query_row(
        "SELECT unreachable_since_ms,last_sample_ms FROM swarm_worker_liveness WHERE attempt_id=?1",
        params![attempt], |r| Ok((r.get(0)?,r.get(1)?)),
    ).optional()?;
    if prior.as_ref().is_some_and(|(_,last)| now <= *last) {
        bail!("out-of-order liveness sample");
    }
    let since = if reachable { None } else { Some(prior.and_then(|(since,_)| since).unwrap_or(now)) };
    let state = if reachable { "reachable" }
        else if now.saturating_sub(since.unwrap()) >= UNKNOWN_AFTER_MS { "unknown" }
        else { "suspect" };
    store.conn.execute(
        "INSERT INTO swarm_worker_liveness(attempt_id,state,unreachable_since_ms,last_sample_ms)
         VALUES(?1,?2,?3,?4) ON CONFLICT(attempt_id) DO UPDATE SET
         state=excluded.state,unreachable_since_ms=excluded.unreachable_since_ms,
         last_sample_ms=excluded.last_sample_ms",
        params![attempt,state,since,now],
    )?;
    Ok(json!({"state":state,"unreachable_since_ms":since,"last_sample_ms":now,
        "sample_interval_ms":sample_interval(state),"unknown_after_ms":UNKNOWN_AFTER_MS}))
}

/// Probe a bounded number of due local worker control sockets. This reads supervisor
/// reachability only; it never interprets model output or confirms process exit.
pub fn sample_due_workers(d: &Arc<Daemon>, now: i64) -> Result<usize> {
    let due = {
        let store = d.store.lock().unwrap();
        let mut stmt = store.conn.prepare(
            "SELECT l.run_id,l.job_id,l.attempt_id,l.overseer_run_id FROM swarm_worker_launches l
             JOIN swarm_attempts a ON a.id=l.attempt_id AND a.status='registered'
             JOIN runs r ON r.id=l.overseer_run_id
             LEFT JOIN swarm_worker_liveness v ON v.attempt_id=l.attempt_id
             WHERE r.status IN ('queued','starting','running','waiting_for_user','disconnected')
             AND (v.last_sample_ms IS NULL OR v.last_sample_ms<=?1
                  OR (v.state='suspect' AND v.last_sample_ms<=?2))
             ORDER BY COALESCE(v.last_sample_ms,0),l.created_ms LIMIT 4",
        )?;
        let rows = stmt.query_map(params![now.saturating_sub(SAMPLE_INTERVAL_MS),
            now.saturating_sub(SUSPECT_RETRY_MS)], |r| {
            Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,
                r.get::<_,String>(2)?,r.get::<_,String>(3)?))
        })?.collect::<rusqlite::Result<Vec<_>>>()?;
        rows
    };
    let mut sampled = 0;
    for (run, job, attempt, overseer_run) in due {
        let worker = d.run(&overseer_run)?;
        if !crate::daemon::ACTIVE.contains(&worker.status.as_str()) && worker.status != "disconnected" {
            continue;
        }
        let reachable = worker.status != "disconnected" && d.control_socket(&worker).ok()
            .and_then(|socket| crate::shim::control_with_timeout(
                &socket, &json!({"op":"ping"}), Duration::from_millis(250)).ok())
            .is_some_and(|reply| reply["ok"] == true);
        sample_liveness(&mut d.store.lock().unwrap(), &json!({
            "run_id":run,"job_id":job,"attempt_id":attempt,
            "now_ms":now,"reachable":reachable}))?;
        sampled += 1;
    }
    Ok(sampled)
}

pub fn launch_worker(d: &Arc<Daemon>, p: &Value) -> Result<Value> {
    let _serial = d.swarm_launch_lock.lock().unwrap();
    if d.swarm_storage_blocked.load(Ordering::SeqCst) {
        bail!("swarm storage is blocked; recover write capacity before launching new work");
    }
    launch_worker_locked(d,p)
}

pub(super) fn launch_worker_locked(d: &Arc<Daemon>, p: &Value) -> Result<Value> {
    let run = required(p, "run_id")?;
    let job = required(p, "job_id")?;
    let attempt = required(p, "attempt_id")?;
    let token = required(p, "token")?;
    let repo = required(p, "repo")?;
    let harness = p["harness"].as_str().unwrap_or("generic");
    if !["generic", "codex", "codex-app", "claude", "opencode"].contains(&harness) {
        bail!("unknown fixture worker harness");
    }
    let program = if harness == "generic" { required(p, "program")? }
        else { p["program"].as_str().unwrap_or("") };
    let prompt = required(p, "prompt")?;
    let title = required(p, "title")?;
    let args = p["args"]
        .as_array()
        .ok_or_else(|| anyhow!("args must be an array"))?;
    if (harness == "generic" && (program.is_empty() || !program.starts_with('/')))
        || (harness != "generic" && (!program.is_empty() || !args.is_empty()))
        || program.len() > 1024
        || prompt.len() > 8000
        || title.is_empty()
        || title.len() > 200
        || args.len() > 32
        || args
            .iter()
            .any(|arg| arg.as_str().is_none_or(|s| s.len() > 4096))
    {
        bail!("invalid scripted worker launch");
    }
    let digest = format!("{:x}", Sha256::digest(p.to_string().as_bytes()));
    let assigned_prompt;
    let assigned_route;
    let attempt_revision;
    {
        let store = d.store.lock().unwrap();
        attempt_revision = broker::check_attempt(&store, run, job, attempt, token)?;
        let executor: String = store.conn.query_row(
            "SELECT executor FROM swarm_attempts WHERE id=?1 AND run_id=?2 AND job_id=?3",
            params![attempt,run,job], |r| r.get(0))?;
        if executor != "worker" {
            bail!("director-executed attempt cannot launch a worker");
        }
        let current = get(&store, run)?;
        let prior: Option<(String,Option<String>,Option<String>)> = store.conn.query_row(
            "SELECT l.request_sha256,l.overseer_run_id,
              CASE WHEN r.status='failed' THEN 'launch_failed'
                   WHEN r.run_dir IS NULL AND
                     (l.launch_phase='spawn_requested' OR l.launch_phase IS NULL) THEN 'launch_uncertain'
                   ELSE 'linked' END
             FROM swarm_worker_launches l LEFT JOIN runs r ON r.id=l.overseer_run_id
             WHERE l.attempt_id=?1 AND l.run_id=?2 AND l.job_id=?3",
            params![attempt,run,job], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?)),
        ).optional()?;
        let new_intent = prior.is_none();
        if let Some((old_digest, linked, status)) = prior {
            if old_digest != digest {
                bail!("worker launch replay changes request");
            }
            if let Some(overseer_run_id) = linked {
                return Ok(
                    json!({"status":status.unwrap_or_else(||"linked".into()),
                        "overseer_run_id":overseer_run_id,"duplicate":true}),
                );
            }
        }
        if current["status"] != "running" && current["status"] != "planning" {
            bail!("swarm run is not launching workers");
        }
        let admitted_route: Option<(Option<String>,Option<String>,Option<String>,Option<String>)> = store.conn.query_row(
            "SELECT target_harness,target_profile_id,target_model,target_effort FROM swarm_admissions
             WHERE attempt_id=?1 AND run_id=?2 AND job_id=?3",
            params![attempt,run,job], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?)),
        ).optional()?;
        let Some((admitted_harness, profile_id, model, effort)) = admitted_route else {
            bail!("attempt has no admitted target route");
        };
        if admitted_harness.as_deref() != Some(harness) {
            bail!("worker harness does not match admitted target harness");
        }
        if (harness != "generic" && (profile_id.is_none() || model.is_none()))
            || [ ("profile_id",profile_id.as_deref()), ("model",model.as_deref()),
                ("effort",effort.as_deref()) ].iter().any(|(key,selected)|
                    p.get(*key).is_some() && p[*key].as_str() != *selected)
        {
            bail!("worker launch changes admitted target route");
        }
        let eligible: bool = store.conn.prepare(
            "SELECT 1 FROM swarm_admissions a JOIN swarm_jobs j ON j.run_id=a.run_id AND j.id=a.job_id
             JOIN swarm_attempts t ON t.id=a.attempt_id AND t.status='registered'
             WHERE a.attempt_id=?1 AND a.run_id=?2 AND a.job_id=?3
             AND j.status='reserved' AND j.plan_revision=?4
             AND j.deadline_at_ms>?5"
        )?.exists(params![attempt,run,job,attempt_revision,crate::daemon::now()])?;
        if !eligible {
            bail!("attempt has no current admitted job");
        }
        let brief = super::worker_brief(&store, p)?;
        assigned_prompt = format!("{prompt}\n\nSwarm assignment and evidence references:\n{brief}");
        if assigned_prompt.len() > 32 * 1024 {
            bail!("scripted worker prompt exceeds inline context limit");
        }
        if new_intent {
            store.conn.execute(
                "INSERT INTO swarm_worker_launches(attempt_id,run_id,job_id,request_sha256,created_ms,launch_phase)
                 VALUES(?1,?2,?3,?4,?5,'reserved')",
                params![attempt,run,job,digest,crate::daemon::now()],
            )?;
        }
        // The admitted route is the source of profile and model choices; the
        // caller's launch payload cannot replace them between selection and spawn.
        assigned_route = (profile_id, model, effort);
    }
    let program_override = if harness == "generic" { json!(program) } else { Value::Null };
    let task = d.create_task_for_swarm(
        &json!({
            "repo":repo,"harness":harness,"workspace_mode":"worktree",
            "profile_id":assigned_route.0,"model":assigned_route.1,"effort":assigned_route.2,
            "program":program_override,"args":args,"prompt":assigned_prompt,"title":title,
        }),
        &SwarmWorkerIdentity {
            run_id: run.to_string(),
            job_id: job.to_string(),
            attempt_id: attempt.to_string(),
            token: token.to_string(),
            revision: attempt_revision,
        },
    )?;
    let overseer_run_id = task["run"]["id"]
        .as_str()
        .ok_or_else(|| anyhow!("worker run was not recorded"))?;
    if !task["launch_error"].is_null() {
        let status = if task["launch_uncertain"] == true { "launch_uncertain" }
            else { "launch_failed" };
        return Ok(
            json!({"status":status,"overseer_run_id":overseer_run_id,
            "error":task["launch_error"],"duplicate":false}),
        );
    }
    Ok(json!({"status":"launched","overseer_run_id":overseer_run_id,"duplicate":false}))
}

pub fn interrupt_workers(d: &Arc<Daemon>, run: &str) -> Result<Value> {
    interrupt_workers_with_fault(d, run, false)
}

pub fn interrupt_workers_with_fault(d: &Arc<Daemon>, run: &str, fail_first: bool) -> Result<Value> {
    let now = crate::daemon::now();
    let linked = {
        let store = d.store.lock().unwrap();
        let mut stmt = store.conn.prepare(
            "SELECT r.id FROM swarm_worker_launches l JOIN runs r ON r.id=l.overseer_run_id
             LEFT JOIN swarm_stop_signals s ON s.run_id=l.run_id AND s.overseer_run_id=r.id
             WHERE l.run_id=?1 AND r.status IN ('queued','starting','running','waiting_for_user')
             AND (s.overseer_run_id IS NULL OR s.last_attempt_ms<=?2)
             ORDER BY r.id",
        )?;
        let ids = stmt
            .query_map(params![run,now-STOP_RETRY_MS], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        ids
    };
    let mut requested = Vec::new();
    let mut unconfirmed = Vec::new();
    for worker in linked {
        let result = if fail_first {
            Err(anyhow!("fixture simulated unreachable worker control socket"))
        } else {
            d.interrupt(&worker).map(|_| ())
        };
        let outcome = if result.is_ok() { "requested" } else { "unconfirmed" };
        d.store.lock().unwrap().conn.execute(
            "INSERT INTO swarm_stop_signals(run_id,overseer_run_id,attempts,last_attempt_ms,last_outcome)
             VALUES(?1,?2,1,?3,?4) ON CONFLICT(run_id,overseer_run_id) DO UPDATE SET
             attempts=attempts+1,last_attempt_ms=excluded.last_attempt_ms,
             last_outcome=excluded.last_outcome",
            params![run,worker,now,outcome],
        )?;
        if result.is_ok() {
            requested.push(worker);
        } else {
            unconfirmed.push(worker);
        }
    }
    Ok(json!({"interrupt_requested":requested,"unconfirmed":unconfirmed}))
}

/// A committed Stop survives a failed first signal or daemon crash. Retry only linked,
/// still-active workers; process exit is reconciled separately before releasing an attempt.
pub fn retry_stopping_interrupts(d: &Arc<Daemon>) -> Result<usize> {
    let due = {
        let store = d.store.lock().unwrap();
        let mut stmt = store.conn.prepare(
            "SELECT DISTINCT l.run_id FROM swarm_worker_launches l
             JOIN swarm_runs s ON s.id=l.run_id AND s.status='stopping'
             JOIN runs r ON r.id=l.overseer_run_id
               AND r.status IN ('queued','starting','running','waiting_for_user')
             LEFT JOIN swarm_stop_signals i ON i.run_id=l.run_id AND i.overseer_run_id=r.id
             WHERE i.overseer_run_id IS NULL OR i.last_attempt_ms<=?1
             ORDER BY l.run_id LIMIT 100",
        )?;
        let rows = stmt.query_map(params![crate::daemon::now()-STOP_RETRY_MS], |row| row.get::<_,String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows
    };
    let mut retried = 0;
    for run in due {
        let result = interrupt_workers(d, &run)?;
        retried += result["interrupt_requested"].as_array().map_or(0, Vec::len)
            + result["unconfirmed"].as_array().map_or(0, Vec::len);
    }
    Ok(retried)
}

/// Stop only workers whose own job was quarantined by a resource conflict or
/// selected-account revocation. Restart retries a missed signal from durable state.
pub fn retry_targeted_interrupts(d: &Arc<Daemon>) -> Result<Value> {
    let now = crate::daemon::now();
    let linked = {
        let store = d.store.lock().unwrap();
        let mut stmt = store.conn.prepare(
            "SELECT l.run_id,r.id FROM swarm_worker_launches l
             JOIN swarm_jobs j ON j.run_id=l.run_id AND j.id=l.job_id
             JOIN runs r ON r.id=l.overseer_run_id
             LEFT JOIN swarm_stop_signals s ON s.run_id=l.run_id AND s.overseer_run_id=r.id
             WHERE j.stop_reason IN ('resource_contamination','account_identity_revoked')
               AND j.status IN ('cancel_requested','blocked')
               AND r.status IN ('queued','starting','running','waiting_for_user')
               AND (s.overseer_run_id IS NULL OR s.last_attempt_ms<=?1)
             ORDER BY l.run_id,r.id LIMIT 100",
        )?;
        let rows = stmt.query_map(params![now-STOP_RETRY_MS], |row| {
            Ok((row.get::<_,String>(0)?,row.get::<_,String>(1)?))
        })?.collect::<rusqlite::Result<Vec<_>>>()?;
        rows
    };
    let mut requested = Vec::new();
    let mut unconfirmed = Vec::new();
    for (run,worker) in linked {
        let result = d.interrupt(&worker);
        let outcome = if result.is_ok() { "requested" } else { "unconfirmed" };
        d.store.lock().unwrap().conn.execute(
            "INSERT INTO swarm_stop_signals(run_id,overseer_run_id,attempts,last_attempt_ms,last_outcome)
             VALUES(?1,?2,1,?3,?4) ON CONFLICT(run_id,overseer_run_id) DO UPDATE SET
             attempts=attempts+1,last_attempt_ms=excluded.last_attempt_ms,
             last_outcome=excluded.last_outcome",
            params![run,worker,now,outcome],
        )?;
        if result.is_ok() { requested.push(worker); } else { unconfirmed.push(worker); }
    }
    Ok(json!({"interrupt_requested":requested,"unconfirmed":unconfirmed}))
}

/// Known native descendants lacking a confirmed terminal receipt, and confirmed
/// failures among them. This walks grandchildren as well as direct children.
pub(super) fn descendant_receipts(conn: &rusqlite::Connection, root: &str) -> Result<(i64,i64)> {
    Ok(conn.query_row(
        "WITH RECURSIVE descendants(id) AS (
             SELECT id FROM runs WHERE parent_run_id=?1
             UNION SELECT child.id FROM runs child JOIN descendants d ON child.parent_run_id=d.id
         )
         SELECT COALESCE(SUM(CASE WHEN child.ended_ms IS NULL
                   OR child.status NOT IN ('completed','failed','interrupted')
                   THEN 1 ELSE 0 END),0),
                COALESCE(SUM(CASE WHEN child.status='failed' THEN 1 ELSE 0 END),0)
         FROM runs child JOIN descendants d ON child.id=d.id",
        params![root], |row| Ok((row.get(0)?,row.get(1)?)),
    )?)
}

/// Copy a supervised worker's terminal state into the durable director inbox.
/// A process exit is only lifecycle evidence; the director still has to assess
/// a separate result/artifact before the job can be accepted.
pub fn reconcile_worker(d: &Arc<Daemon>, p: &Value) -> Result<Value> {
    let run = required(p, "run_id")?;
    let job = required(p, "job_id")?;
    let attempt = required(p, "attempt_id")?;
    let generation = p["generation"]
        .as_i64()
        .ok_or_else(|| anyhow!("missing generation"))?;
    let revision = p["revision"]
        .as_i64()
        .ok_or_else(|| anyhow!("missing revision"))?;
    let mut store = d.store.lock().unwrap();
    let current = get(&store, run)?;
    if current["generation"] != generation || current["revision"] != revision {
        bail!("stale director generation or plan revision");
    }
    let linked: Option<(String, Option<String>, Option<i64>, i64)> = store
        .conn
        .query_row(
            "SELECT l.overseer_run_id,r.status,r.ended_ms,a.revision
         FROM swarm_worker_launches l
         JOIN swarm_attempts a ON a.id=l.attempt_id AND a.run_id=l.run_id AND a.job_id=l.job_id
         LEFT JOIN runs r ON r.id=l.overseer_run_id
         WHERE l.run_id=?1 AND l.job_id=?2 AND l.attempt_id=?3 AND l.overseer_run_id IS NOT NULL",
            params![run, job, attempt],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()?;
    let Some((overseer_run_id, worker_status, ended, attempt_revision)) = linked else {
        return Ok(json!({"status":"unlinked"}));
    };
    let Some(worker_status) = worker_status else {
        bail!("linked worker run is missing");
    };
    // Overseer records a lost supervisor as `disconnected` even when the harness
    // child may still be alive. Its ended timestamp is not process-exit evidence.
    if worker_status == "disconnected" {
        return Ok(json!({"status":"unknown","overseer_run_id":overseer_run_id,
            "worker_status":worker_status}));
    }
    let reachability: Option<String> = store.conn.query_row(
        "SELECT state FROM swarm_worker_liveness WHERE attempt_id=?1",
        params![attempt], |r| r.get(0),
    ).optional()?;
    if crate::daemon::ACTIVE.contains(&worker_status.as_str())
        && matches!(reachability.as_deref(), Some("suspect" | "unknown"))
    {
        return Ok(json!({"status":reachability.unwrap(),
            "overseer_run_id":overseer_run_id,"worker_status":worker_status}));
    }
    if crate::daemon::ACTIVE.contains(&worker_status.as_str()) || ended.is_none() {
        return Ok(json!({"status":"active","overseer_run_id":overseer_run_id}));
    }
    // A harness may report its parent process as exited before its native children
    // finish. Overseer changes children without a terminal receipt to `unknown`;
    // that inferred state cannot close the logical Swarm attempt or its reservation.
    let (unconfirmed_descendants, failed_descendants) = descendant_receipts(&store.conn, &overseer_run_id)?;
    if unconfirmed_descendants > 0 {
        return Ok(json!({"status":"descendants_unconfirmed",
            "overseer_run_id":overseer_run_id,"unconfirmed_descendants":unconfirmed_descendants}));
    }
    let message_id = format!("terminal-{attempt}");
    let payload = json!({"overseer_run_id":overseer_run_id,"run_status":worker_status,
        "native_descendant_failures":failed_descendants});
    let previous: Option<String> = store
        .conn
        .query_row(
            "SELECT payload FROM swarm_messages WHERE run_id=?1 AND message_id=?2",
            params![run, message_id],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(old) = &previous {
        if serde_json::from_str::<Value>(old)? != payload {
            bail!("terminal event replay changed payload");
        }
    } else {
        let now = crate::daemon::now();
        store.conn.execute(
            "INSERT INTO swarm_messages(run_id,message_id,job_id,attempt_id,sender,recipient,kind,revision,payload,phase,created_ms,updated_ms)
             VALUES(?1,?2,?3,?4,'runtime','director','terminal',?5,?6,'queued',?7,?7)",
            params![run,message_id,job,attempt,attempt_revision,payload.to_string(),now],
        )?;
    }
    super::artifacts::confirm_exit(&mut store, p)?;
    Ok(
        json!({"status":"terminal","overseer_run_id":overseer_run_id,
        "worker_status":worker_status,"duplicate":previous.is_some()}),
    )
}

/// Observe completed linked processes without requiring a caller to poll each worker.
/// The bounded scan resumes after a crash because unfinished attempts remain registered.
pub fn reconcile_terminal_workers(d: &Arc<Daemon>) -> Result<usize> {
    let due = {
        let store = d.store.lock().unwrap();
        let mut stmt = store.conn.prepare(
            "SELECT l.run_id,l.job_id,l.attempt_id,s.generation,s.revision
             FROM swarm_worker_launches l
             JOIN swarm_attempts a ON a.id=l.attempt_id AND a.status='registered'
             JOIN swarm_runs s ON s.id=l.run_id
             JOIN runs r ON r.id=l.overseer_run_id AND r.ended_ms IS NOT NULL
                AND r.status!='disconnected'
             AND NOT EXISTS (
                 WITH RECURSIVE descendants(id) AS (
                     SELECT id FROM runs WHERE parent_run_id=r.id
                     UNION SELECT child.id FROM runs child
                         JOIN descendants d ON child.parent_run_id=d.id
                 )
                 SELECT 1 FROM runs child JOIN descendants d ON child.id=d.id
                 WHERE child.ended_ms IS NULL
                    OR child.status NOT IN ('completed','failed','interrupted')
             )
             ORDER BY r.ended_ms LIMIT 100",
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows
    };
    let mut reconciled = 0;
    for (run, job, attempt, generation, revision) in due {
        match reconcile_worker(
            d,
            &json!({"run_id":run,"job_id":job,"attempt_id":attempt,
            "generation":generation,"revision":revision}),
        ) {
            Ok(result) if result["status"] == "terminal" => reconciled += 1,
            Ok(_) => {}
            Err(error) => crate::log(&format!(
                "swarm worker {attempt} reconciliation failed: {error}"
            )),
        }
    }
    Ok(reconciled)
}
