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
            crate::git::toplevel(Path::new(repo))?;
            if !Path::new(program).is_file() {
                bail!("scripted worker program is unavailable");
            }
            store.conn.execute("INSERT INTO swarm_dispatch_intents(request_id,request_sha256,request_json,created_ms)
                VALUES(?1,?2,?3,?4)",params![id,digest,p.to_string(),crate::daemon::now()])?;
        }
    }
    let scheduled = scheduler::next(&mut d.store.lock().unwrap(), p)?;
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
    crate::git::toplevel(Path::new(repo))?;
    if !Path::new(program).is_file() {
        bail!("scripted worker program is unavailable");
    }
    if p["inject_failure_after_admit_once"] == true {
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
