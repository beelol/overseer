//! Scripted fixture bridge from fair admission to a supervised worker.
//! Intents and admissions persist before launch so retry never admits a second attempt.

use super::{required, runtime, scheduler};
use crate::daemon::Daemon;
use anyhow::{anyhow, bail, Result};
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::Path;
use std::sync::Arc;

pub fn next(d: &Arc<Daemon>, p: &Value) -> Result<Value> {
    let _serial = d.swarm_launch_lock.lock().unwrap();
    let id = required(p, "request_id")?;
    let repo = required(p, "repo")?;
    let program = required(p, "program")?;
    let args = p["args"]
        .as_array()
        .ok_or_else(|| anyhow!("args must be an array"))?;
    if id.is_empty()
        || id.len() > 128
        || repo.is_empty()
        || !Path::new(repo).is_absolute()
        || !program.starts_with('/')
        || args.len() > 32
        || args
            .iter()
            .any(|a| a.as_str().is_none_or(|s| s.len() > 4096))
    {
        bail!("invalid scripted dispatch request");
    }
    let digest = format!("{:x}", Sha256::digest(p.to_string().as_bytes()));
    {
        let store = d.store.lock().unwrap();
        let prior: Option<String> = store
            .conn
            .query_row(
                "SELECT request_sha256 FROM swarm_dispatch_intents WHERE request_id=?1",
                params![id],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(old) = prior {
            if old != digest {
                bail!("dispatch request id reused with different input");
            }
        } else {
            verify_snapshot_fresh(p)?;
            crate::git::toplevel(Path::new(repo))?;
            if !Path::new(program).is_file() {
                bail!("scripted worker program is unavailable");
            }
            store.conn.execute("INSERT INTO swarm_dispatch_intents(request_id,request_sha256,request_json,created_ms)
                VALUES(?1,?2,?3,?4)",params![id,digest,p.to_string(),crate::daemon::now()])?;
        }
    }
    let scheduled = {
        let pending = d.pending_agent_slots.lock().unwrap();
        scheduler::next(&mut d.store.lock().unwrap(), p, *pending)?
    };
    if scheduled["status"] == "blocked" {
        return Ok(scheduled);
    }
    let run = scheduled["run_id"]
        .as_str()
        .ok_or_else(|| anyhow!("scheduler omitted run"))?;
    let job = scheduled["job_id"]
        .as_str()
        .ok_or_else(|| anyhow!("scheduler omitted job"))?;
    let attempt = scheduled["attempt_id"]
        .as_str()
        .ok_or_else(|| anyhow!("scheduler omitted attempt"))?;
    let linked: Option<String> = {
        let store = d.store.lock().unwrap();
        store.conn.query_row("SELECT overseer_run_id FROM swarm_worker_launches WHERE attempt_id=?1 AND overseer_run_id IS NOT NULL",
            params![attempt],|r|r.get(0)).optional()?
    };
    if let Some(worker) = linked {
        return Ok(json!({"status":"linked","run_id":run,"job_id":job,
            "attempt_id":attempt,"overseer_run_id":worker,"duplicate":true}));
    }
    verify_pending_launch(d, p, run)?;
    crate::git::toplevel(Path::new(repo))?;
    if !Path::new(program).is_file() {
        bail!("scripted worker program is unavailable");
    }
    if scheduled["status"] == "admitted" && p["inject_failure_after_admit_once"] == true {
        let store = d.store.lock().unwrap();
        let changed = store.conn.execute(
            "UPDATE swarm_dispatch_intents SET failure_injected=1
            WHERE request_id=?1 AND failure_injected=0",
            params![id],
        )?;
        if changed == 1 {
            bail!("injected failure after admission before worker launch");
        }
    }
    let token = if let Some(token) = scheduled["token"].as_str() {
        token.to_string()
    } else {
        // No run was linked before the crash: launch identity can be reissued safely.
        // A linked worker is handled above and is never assigned a new token.
        let token = uuid::Uuid::new_v4().simple().to_string();
        let store = d.store.lock().unwrap();
        let tx = store.conn.unchecked_transaction()?;
        let eligible: bool = tx
            .prepare(
                "SELECT 1 FROM swarm_attempts a
            JOIN swarm_admissions s ON s.attempt_id=a.id
            JOIN swarm_jobs j ON j.run_id=a.run_id AND j.id=a.job_id AND j.status='reserved'
            JOIN swarm_runs r ON r.id=a.run_id AND r.status IN ('planning','running')
            WHERE a.id=?1 AND a.run_id=?2 AND a.job_id=?3 AND a.status='registered'",
            )?
            .exists(params![attempt, run, job])?;
        if !eligible {
            bail!("admitted attempt is no longer launchable");
        }
        let has_link: bool=tx.prepare("SELECT 1 FROM swarm_worker_launches WHERE attempt_id=?1 AND overseer_run_id IS NOT NULL")?
            .exists(params![attempt])?;
        if has_link {
            bail!("worker linked during token recovery");
        }
        tx.execute(
            "DELETE FROM swarm_worker_launches WHERE attempt_id=?1 AND overseer_run_id IS NULL",
            params![attempt],
        )?;
        tx.execute(
            "UPDATE swarm_attempts SET token_sha256=?2 WHERE id=?1",
            params![attempt, format!("{:x}", Sha256::digest(token.as_bytes()))],
        )?;
        tx.commit()?;
        token
    };
    let title: String = {
        let store = d.store.lock().unwrap();
        store.conn.query_row(
            "SELECT title FROM swarm_jobs WHERE run_id=?1 AND id=?2",
            params![run, job],
            |r| r.get(0),
        )?
    };
    let launched = runtime::launch_worker_locked(
        d,
        &json!({"run_id":run,"job_id":job,
        "attempt_id":attempt,"token":token,"repo":repo,"program":program,"args":args,
        "prompt":"Complete the assigned Swarm job and report evidence to the director.","title":title}),
    )?;
    Ok(
        json!({"status":launched["status"],"run_id":run,"job_id":job,
        "attempt_id":attempt,"overseer_run_id":launched["overseer_run_id"],
        "duplicate":launched["duplicate"],"error":launched["error"]}),
    )
}

fn verify_pending_launch(d: &Arc<Daemon>, p: &Value, run: &str) -> Result<()> {
    verify_snapshot_fresh(p)?;
    let now = crate::daemon::now();
    let store = d.store.lock().unwrap();
    let current = super::get(&store, run)?;
    let target = required(p, "target_id")?;
    if !current["allowed_targets"]
        .as_array()
        .is_some_and(|allowed| allowed.iter().any(|id| id == target))
    {
        bail!("dispatch target permission revoked before launch");
    }
    let deadline = current["policy"]["effective"]["deadline_ms"]
        .as_i64()
        .unwrap_or(3_600_000);
    if now
        >= current["created_ms"]
            .as_i64()
            .unwrap_or(now)
            .saturating_add(deadline)
    {
        bail!("swarm deadline expired before dispatch launch");
    }
    Ok(())
}

fn verify_snapshot_fresh(p: &Value) -> Result<()> {
    let now = crate::daemon::now();
    let snapshot = &p["snapshot"];
    if snapshot["expires_ms"]
        .as_i64()
        .is_none_or(|expires| expires <= now)
    {
        bail!("dispatch target snapshot expired before launch");
    }
    let pools = snapshot["pools"]
        .as_array()
        .ok_or_else(|| anyhow!("dispatch target snapshot has no pools"))?;
    for pool in pools {
        let windows = pool["windows"]
            .as_array()
            .ok_or_else(|| anyhow!("dispatch quota pool has no windows"))?;
        for window in windows {
            if window["expires_ms"]
                .as_i64()
                .is_none_or(|expires| expires <= now)
            {
                bail!("dispatch quota snapshot expired before launch");
            }
        }
    }
    Ok(())
}

/// On daemon startup, finish only already-admitted fixture dispatch intents.
/// An invalid or stale intent stays reserved for explicit reconciliation; it is
/// never turned into another admission or silently retried every background tick.
pub fn recover_pending(d: &Arc<Daemon>) -> Result<Value> {
    if std::env::var("OVERSEER_SWARM_FIXTURE_API").as_deref() != Ok("1") {
        return Ok(json!({"status":"disabled"}));
    }
    let mut after = String::new();
    let mut launched = 0;
    let mut skipped = 0;
    loop {
        let row: Option<(String, String)> = {
            let store = d.store.lock().unwrap();
            store
                .conn
                .query_row(
                    "SELECT i.request_id,i.request_json FROM swarm_dispatch_intents i
                 JOIN swarm_scheduler_admissions s ON s.request_id=i.request_id
                 JOIN swarm_attempts a ON a.id=s.attempt_id AND a.status='registered'
                 JOIN swarm_jobs j ON j.run_id=s.run_id AND j.id=s.job_id AND j.status='reserved'
                 JOIN swarm_runs r ON r.id=s.run_id AND r.status IN ('planning','running')
                 LEFT JOIN swarm_worker_launches l ON l.attempt_id=s.attempt_id
                 WHERE i.request_id>?1 AND l.overseer_run_id IS NULL
                 ORDER BY i.request_id LIMIT 1",
                    params![after],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?
        };
        let Some((id, request_json)) = row else {
            break;
        };
        after = id.clone();
        let result = serde_json::from_str::<Value>(&request_json)
            .map_err(anyhow::Error::from)
            .and_then(|request| next(d, &request));
        match result {
            Ok(value) if value["status"] == "launched" || value["status"] == "linked" => {
                launched += 1;
            }
            Ok(value) => {
                skipped += 1;
                crate::log(&format!(
                    "swarm dispatch intent {id} remained pending: {value}"
                ));
            }
            Err(error) => {
                skipped += 1;
                crate::log(&format!(
                    "swarm dispatch intent {id} remained pending: {error}"
                ));
            }
        }
    }
    Ok(json!({"status":"reconciled","launched":launched,"skipped":skipped}))
}
