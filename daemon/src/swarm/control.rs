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
    if !["planning", "running"].contains(&status.as_str()) {
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
        "UPDATE swarm_runs SET status='draining',stalled_from=NULL,updated_ms=?2 WHERE id=?1",
        params![id, now],
    )?;
    tx.execute("UPDATE swarm_jobs SET status='cancelled',updated_ms=?2 WHERE run_id=?1 AND status IN ('planned','ready')",params![id,now])?;
    tx.commit()?;
    Ok(json!({"id":id,"status":"draining","duplicate":false}))
}

/// Expire runs independently of admission requests, including runs waiting for an account.
pub fn expire_due(store: &mut Store, now: i64) -> Result<Vec<String>> {
    let mut stmt = store.conn.prepare(
        "SELECT id,generation,revision,created_ms,policy FROM swarm_runs
         WHERE status IN ('planning','running','paused','stalled','draining')",
    )?;
    let active = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, String>(4)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(stmt);
    let mut expired = Vec::new();
    for (id, generation, revision, created, raw_policy) in active {
        let policy: Value = serde_json::from_str(&raw_policy)?;
        let deadline = policy["effective"]["deadline_ms"]
            .as_i64()
            .unwrap_or(3_600_000);
        if now >= created.saturating_add(deadline) {
            super::stop_for_deadline(
                store,
                &json!({"run_id":id,"generation":generation,"revision":revision}),
            )?;
            expired.push(id);
        }
    }
    Ok(expired)
}

/// Request a stop for jobs whose admission-time deadline expired. Return only
/// linked, still-active workers; their reservations remain held until exit is
/// confirmed. Repeated ticks retry interruption after a daemon restart.
pub fn expire_jobs_due(store: &mut Store, now: i64) -> Result<Vec<String>> {
    store.conn.execute(
        "UPDATE swarm_jobs SET status='failed',stop_reason='job_deadline',updated_ms=?1
         WHERE deadline_at_ms IS NOT NULL AND deadline_at_ms<=?1
           AND status IN ('planned','ready','submitted')
           AND NOT EXISTS (SELECT 1 FROM swarm_attempts a WHERE a.run_id=swarm_jobs.run_id
                           AND a.job_id=swarm_jobs.id AND a.status='registered')
           AND EXISTS (SELECT 1 FROM swarm_runs s WHERE s.id=swarm_jobs.run_id
                       AND s.status IN ('planning','running','paused','stalled','draining'))",
        params![now],
    )?;
    let mut stmt = store.conn.prepare(
        "SELECT j.run_id,j.id,j.deadline_at_ms,j.status FROM swarm_jobs j
         JOIN swarm_runs s ON s.id=j.run_id
         WHERE s.status IN ('planning','running','paused','stalled','draining')
           AND j.deadline_at_ms IS NOT NULL AND j.deadline_at_ms<=?1
           AND EXISTS (SELECT 1 FROM swarm_attempts a WHERE a.run_id=j.run_id
                       AND a.job_id=j.id AND a.status='registered')
           AND (j.status IN ('reserved','launching','running','submitted','accepted')
                OR (j.status='cancel_requested' AND j.stop_reason='job_deadline'))
         ORDER BY CASE WHEN j.status='cancel_requested' THEN 1 ELSE 0 END,
                  j.deadline_at_ms,j.run_id,j.id LIMIT 100",
    )?;
    let due = stmt.query_map(params![now], |row| Ok((
        row.get::<_,String>(0)?,row.get::<_,String>(1)?,
        row.get::<_,i64>(2)?,row.get::<_,String>(3)?
    )))?.collect::<rusqlite::Result<Vec<_>>>()?;
    drop(stmt);
    let mut workers = Vec::new();
    for (run, job, deadline, status) in due {
        if status != "cancel_requested" {
            let tx = store.conn.transaction()?;
            tx.execute("UPDATE swarm_jobs SET status='cancel_requested',stop_reason='job_deadline',updated_ms=?3
                WHERE run_id=?1 AND id=?2",params![run,job,now])?;
            tx.execute("INSERT OR IGNORE INTO swarm_messages(run_id,message_id,job_id,attempt_id,sender,recipient,kind,revision,payload,phase,created_ms,updated_ms)
                SELECT a.run_id,'job-deadline-'||a.id,a.job_id,a.id,'control',a.id,'stop',a.revision,?3,'queued',?4,?4
                FROM swarm_attempts a WHERE a.run_id=?1 AND a.job_id=?2 AND a.status='registered'",
                params![run,job,json!({"reason":"job_deadline","deadline_at_ms":deadline}).to_string(),now])?;
            tx.commit()?;
        }
        let mut linked = store.conn.prepare(
            "SELECT r.id FROM swarm_worker_launches l
             JOIN runs r ON r.id=l.overseer_run_id
             JOIN swarm_attempts a ON a.id=l.attempt_id AND a.status='registered'
             WHERE l.run_id=?1 AND l.job_id=?2
               AND r.status IN ('queued','starting','running','waiting_for_user')",
        )?;
        workers.extend(linked.query_map(params![run,job], |row| row.get::<_,String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?);
    }
    Ok(workers)
}
