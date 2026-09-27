use super::{get, required};
use crate::store::Store;
use anyhow::{anyhow, bail, Result};
use rusqlite::{params, OptionalExtension};
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
    let status = super::finalize_control_if_idle(&tx,&id,now)?;
    tx.commit()?;
    Ok(json!({"id":id,"status":status,"duplicate":false}))
}

/// An owner-initiated time extension changes the run deadline only. The run's
/// frozen account allocations and any admitted job deadlines are untouched.
pub fn extend_deadline(store: &mut Store, p: &Value) -> Result<Value> {
    let run = required(p, "run_id")?;
    let request_id = required(p, "request_id")?;
    if request_id.is_empty() || request_id.len() > 128 || request_id.chars().any(char::is_control) {
        bail!("invalid deadline extension request id");
    }
    let expected = p["expected_deadline_at_ms"].as_i64()
        .ok_or_else(|| anyhow!("missing expected deadline"))?;
    let additional = p["additional_ms"].as_i64()
        .ok_or_else(|| anyhow!("missing extension duration"))?;
    if !(1..=86_400_000).contains(&additional) {
        bail!("extension duration must be between 1 millisecond and 24 hours");
    }
    let tx = store.conn.transaction()?;
    let replay: Option<(i64,i64,i64)> = tx.query_row(
        "SELECT expected_deadline_at_ms,additional_ms,new_deadline_at_ms
         FROM swarm_deadline_extensions WHERE run_id=?1 AND request_id=?2",
        params![run,request_id], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?)),
    ).optional()?;
    if let Some((old_expected,old_additional,new_deadline)) = replay {
        if old_expected != expected || old_additional != additional {
            bail!("deadline extension request id reused with different input");
        }
        return Ok(json!({"run_id":run,"deadline_at_ms":new_deadline,"duplicate":true}));
    }
    let (status,created,raw_policy): (String,i64,String) = tx.query_row(
        "SELECT status,created_ms,policy FROM swarm_runs WHERE id=?1",[run],
        |r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)),
    ).optional()?.ok_or_else(||anyhow!("unknown swarm run"))?;
    if !["planning","running","paused","stalled","draining"].contains(&status.as_str()) {
        bail!("swarm run cannot extend its deadline in this state");
    }
    let mut policy: Value = serde_json::from_str(&raw_policy)?;
    let duration = policy["effective"]["deadline_ms"].as_i64()
        .ok_or_else(||anyhow!("run is missing its deadline"))?;
    let old_deadline = created.checked_add(duration)
        .ok_or_else(||anyhow!("invalid current deadline"))?;
    if expected != old_deadline { bail!("stale deadline"); }
    let now = crate::daemon::now();
    if now >= old_deadline { bail!("run deadline already expired"); }
    let new_duration = duration.checked_add(additional)
        .ok_or_else(||anyhow!("deadline extension overflow"))?;
    let new_deadline = created.checked_add(new_duration)
        .ok_or_else(||anyhow!("deadline extension overflow"))?;
    policy["effective"]["deadline_ms"] = json!(new_duration);
    policy["sources"]["deadline_ms"] = json!("run_extension");
    tx.execute("UPDATE swarm_runs SET policy=?2,updated_ms=?3 WHERE id=?1",
        params![run,policy.to_string(),now])?;
    tx.execute("INSERT INTO swarm_deadline_extensions(run_id,request_id,
        expected_deadline_at_ms,additional_ms,old_deadline_at_ms,new_deadline_at_ms,created_ms)
        VALUES(?1,?2,?3,?4,?5,?6,?7)",
        params![run,request_id,expected,additional,old_deadline,new_deadline,now])?;
    tx.commit()?;
    Ok(json!({"run_id":run,"previous_deadline_at_ms":old_deadline,
        "deadline_at_ms":new_deadline,"duplicate":false}))
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
        let mut director = store.conn.prepare(
            "SELECT r.id FROM swarm_attempts a JOIN runs r ON r.id=a.executor_run_id
             WHERE a.run_id=?1 AND a.job_id=?2 AND a.status='registered'
               AND a.executor='director'
               AND r.status IN ('queued','starting','running','waiting_for_user')",
        )?;
        workers.extend(director.query_map(params![run,job], |row| row.get::<_,String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?);
    }
    workers.sort();
    workers.dedup();
    Ok(workers)
}

/// A received redirect is not an applied assignment. After 30 seconds, hold
/// its job and ask the worker to checkpoint before interrupting a linked run.
/// The message phase remains delivered so a late applied receipt is observable.
pub fn expire_redirects_due(store: &mut Store, now: i64) -> Result<Value> {
    let mut stmt = store.conn.prepare(
        "SELECT m.seq,m.run_id,m.job_id,m.attempt_id,m.revision,j.stop_reason,
                CASE WHEN r.status IN ('queued','starting','running','waiting_for_user','disconnected')
                     THEN l.overseer_run_id ELSE NULL END
         FROM swarm_messages m
         JOIN swarm_attempts a ON a.id=m.attempt_id AND a.run_id=m.run_id
         JOIN swarm_jobs j ON j.run_id=m.run_id AND j.id=m.job_id
         JOIN swarm_runs s ON s.id=m.run_id
         LEFT JOIN swarm_worker_launches l ON l.attempt_id=a.id
         LEFT JOIN runs r ON r.id=l.overseer_run_id
         WHERE m.kind='redirect' AND m.sender='director' AND m.phase='delivered'
           AND m.updated_ms<=?1 AND a.status='registered'
           AND s.status IN ('planning','running','paused','stalled','draining')
           AND j.status IN ('ready','reserved','launching','running','submitted','cancel_requested')
           AND (j.stop_reason IS NULL OR j.stop_reason='redirect_ack_timeout')
         ORDER BY m.updated_ms,m.seq LIMIT 100",
    )?;
    let due = stmt.query_map([now.saturating_sub(30_000)], |row| Ok((
        row.get::<_,i64>(0)?, row.get::<_,String>(1)?, row.get::<_,String>(2)?,
        row.get::<_,String>(3)?, row.get::<_,i64>(4)?,
        row.get::<_,Option<String>>(5)?, row.get::<_,Option<String>>(6)?,
    )))?.collect::<rusqlite::Result<Vec<_>>>()?;
    drop(stmt);
    let mut timed_out = 0;
    let mut workers = std::collections::BTreeSet::new();
    for (seq, run, job, attempt, revision, reason, worker) in due {
        if reason.as_deref() != Some("redirect_ack_timeout") {
            let tx = store.conn.transaction()?;
            let changed = tx.execute(
                "UPDATE swarm_jobs SET status='cancel_requested',stop_reason='redirect_ack_timeout',updated_ms=?3
                 WHERE run_id=?1 AND id=?2 AND status IN
                   ('ready','reserved','launching','running','submitted','cancel_requested')
                   AND stop_reason IS NULL",
                params![run,job,now],
            )?;
            if changed != 0 {
                tx.execute(
                    "INSERT OR IGNORE INTO swarm_messages
                     (run_id,message_id,job_id,attempt_id,sender,recipient,kind,revision,payload,phase,created_ms,updated_ms)
                     VALUES(?1,?2,?3,?4,'control',?4,'checkpoint',?5,?6,'queued',?7,?7)",
                    params![run,format!("redirect-timeout-{seq}"),job,attempt,revision,
                        json!({"reason":"redirect_ack_timeout","redirect_seq":seq}).to_string(),now],
                )?;
                timed_out += 1;
            }
            tx.commit()?;
        }
        if let Some(worker) = worker {
            workers.insert(worker);
        }
    }
    Ok(json!({"timed_out":timed_out,"interrupt_pending":workers}))
}
