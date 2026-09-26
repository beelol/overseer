//! Scripted worker launch bridge. This remains fixture-only until Auto Mode supplies a
//! daemon-owned target and each live adapter proves directive/descendant control.

use super::{broker, get, required};
use crate::daemon::Daemon;
use anyhow::{anyhow, bail, Result};
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::sync::Arc;

pub fn launch_worker(d: &Arc<Daemon>, p: &Value) -> Result<Value> {
    let _serial = d.swarm_launch_lock.lock().unwrap();
    let run = required(p, "run_id")?;
    let job = required(p, "job_id")?;
    let attempt = required(p, "attempt_id")?;
    let token = required(p, "token")?;
    let repo = required(p, "repo")?;
    let program = required(p, "program")?;
    let prompt = required(p, "prompt")?;
    let title = required(p, "title")?;
    let args = p["args"]
        .as_array()
        .ok_or_else(|| anyhow!("args must be an array"))?;
    if program.is_empty()
        || !program.starts_with('/')
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
    {
        let store = d.store.lock().unwrap();
        let attempt_revision = broker::check_attempt(&store, run, job, attempt, token)?;
        let current = get(&store, run)?;
        let prior: Option<(String,Option<String>)> = store.conn.query_row(
            "SELECT request_sha256,overseer_run_id FROM swarm_worker_launches WHERE attempt_id=?1 AND run_id=?2 AND job_id=?3",
            params![attempt,run,job], |row| Ok((row.get(0)?,row.get(1)?)),
        ).optional()?;
        let new_intent = prior.is_none();
        if let Some((old_digest, linked)) = prior {
            if old_digest != digest {
                bail!("worker launch replay changes request");
            }
            if let Some(overseer_run_id) = linked {
                return Ok(
                    json!({"status":"linked","overseer_run_id":overseer_run_id,"duplicate":true}),
                );
            }
        }
        if current["status"] != "running" && current["status"] != "planning" {
            bail!("swarm run is not launching workers");
        }
        let eligible: bool = store.conn.prepare(
            "SELECT 1 FROM swarm_admissions a JOIN swarm_jobs j ON j.run_id=a.run_id AND j.id=a.job_id
             JOIN swarm_attempts t ON t.id=a.attempt_id AND t.status='registered'
             WHERE a.attempt_id=?1 AND a.run_id=?2 AND a.job_id=?3
             AND j.status='reserved' AND j.plan_revision=?4"
        )?.exists(params![attempt,run,job,attempt_revision])?;
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
                "INSERT INTO swarm_worker_launches(attempt_id,run_id,job_id,request_sha256,created_ms)
                 VALUES(?1,?2,?3,?4,?5)",
                params![attempt,run,job,digest,crate::daemon::now()],
            )?;
        }
    }
    let task = d.create_task_for_swarm(
        &json!({
            "repo":repo,"harness":"generic","workspace_mode":"worktree",
            "program":program,"args":args,"prompt":assigned_prompt,"title":title,
        }),
        attempt,
    )?;
    let overseer_run_id = task["run"]["id"]
        .as_str()
        .ok_or_else(|| anyhow!("worker run was not recorded"))?;
    if !task["launch_error"].is_null() {
        return Ok(
            json!({"status":"launch_failed","overseer_run_id":overseer_run_id,
            "error":task["launch_error"],"duplicate":false}),
        );
    }
    Ok(json!({"status":"launched","overseer_run_id":overseer_run_id,"duplicate":false}))
}

pub fn interrupt_workers(d: &Arc<Daemon>, run: &str) -> Result<Value> {
    let linked = {
        let store = d.store.lock().unwrap();
        let mut stmt = store.conn.prepare(
            "SELECT r.id FROM swarm_worker_launches l JOIN runs r ON r.id=l.overseer_run_id
             WHERE l.run_id=?1 AND r.status IN ('queued','starting','running','waiting_for_user')",
        )?;
        let ids = stmt
            .query_map(params![run], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        ids
    };
    let mut requested = Vec::new();
    let mut unconfirmed = Vec::new();
    for worker in linked {
        if d.interrupt(&worker).is_ok() {
            requested.push(worker);
        } else {
            unconfirmed.push(worker);
        }
    }
    Ok(json!({"interrupt_requested":requested,"unconfirmed":unconfirmed}))
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
    if crate::daemon::ACTIVE.contains(&worker_status.as_str()) || ended.is_none() {
        return Ok(json!({"status":"active","overseer_run_id":overseer_run_id}));
    }
    let message_id = format!("terminal-{attempt}");
    let payload = json!({"overseer_run_id":overseer_run_id,"run_status":worker_status});
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
