use super::{get, required};
use crate::store::Store;
use anyhow::{anyhow, bail, Result};
use rusqlite::params;
use serde_json::{json, Value};

fn checked(store: &Store, p: &Value) -> Result<(String, String)> {
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
    Ok((
        id.to_string(),
        current["status"].as_str().unwrap_or("").to_string(),
    ))
}

pub fn pause(store: &mut Store, p: &Value) -> Result<Value> {
    let (id, status) = checked(store, p)?;
    if status == "paused" {
        return Ok(json!({"id":id,"status":"paused","duplicate":true}));
    }
    if !["planning", "running", "stalled"].contains(&status.as_str()) {
        bail!("run cannot pause in this state");
    }
    let now = crate::daemon::now();
    let tx = store.conn.transaction()?;
    tx.execute(
        "UPDATE swarm_runs SET status='paused',updated_ms=?2 WHERE id=?1",
        params![id, now],
    )?;
    tx.execute("INSERT OR IGNORE INTO swarm_messages(run_id,message_id,job_id,attempt_id,sender,recipient,kind,revision,payload,phase,created_ms,updated_ms) SELECT a.run_id,'pause-'||a.id,a.job_id,a.id,'control',a.id,'checkpoint',a.revision,'{}','queued',?2,?2 FROM swarm_attempts a WHERE a.run_id=?1 AND a.status='registered'",params![id,now])?;
    tx.commit()?;
    Ok(json!({"id":id,"status":"paused","duplicate":false}))
}

pub fn resume(store: &mut Store, p: &Value) -> Result<Value> {
    let (id, status) = checked(store, p)?;
    if status == "running" {
        return Ok(json!({"id":id,"status":"running","duplicate":true}));
    }
    if status != "paused" {
        bail!("run is not paused");
    }
    store.conn.execute(
        "UPDATE swarm_runs SET status='running',updated_ms=?2 WHERE id=?1",
        params![id, crate::daemon::now()],
    )?;
    Ok(json!({"id":id,"status":"running","duplicate":false}))
}

pub fn off(store: &mut Store, p: &Value) -> Result<Value> {
    let (id, status) = checked(store, p)?;
    if status == "draining" {
        return Ok(json!({"id":id,"status":"draining","duplicate":true}));
    }
    if !["planning", "running", "paused", "stalled"].contains(&status.as_str()) {
        bail!("run cannot drain in this state");
    }
    let now = crate::daemon::now();
    let tx = store.conn.transaction()?;
    tx.execute(
        "UPDATE swarm_runs SET status='draining',updated_ms=?2 WHERE id=?1",
        params![id, now],
    )?;
    tx.execute("UPDATE swarm_jobs SET status='cancelled',updated_ms=?2 WHERE run_id=?1 AND status IN ('planned','ready')",params![id,now])?;
    tx.commit()?;
    Ok(json!({"id":id,"status":"draining","duplicate":false}))
}
