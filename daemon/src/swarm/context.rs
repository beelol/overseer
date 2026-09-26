//! Bounded, scoped context for fixture workers and director turns.
//! Live cross-account context transfer needs an explicit destination ACL.

use super::{broker, get, required};
use crate::daemon::Daemon;
use crate::store::Store;
use anyhow::{anyhow, bail, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::sync::Arc;

const MAX_INLINE: usize = 32 * 1024;

pub(super) fn revoked_dependency(conn: &Connection, run: &str, job: &str, target: &str) -> Result<bool> {
    Ok(conn.prepare(
        "SELECT 1 FROM swarm_artifact_revocations v
         JOIN swarm_artifacts a ON a.run_id=v.run_id AND a.id=v.artifact_id
         JOIN swarm_jobs j ON j.run_id=v.run_id AND j.id=?2
         WHERE v.run_id=?1 AND v.target_id=?3
         AND EXISTS (SELECT 1 FROM json_each(j.deps) dep WHERE dep.value=a.job_id)
         LIMIT 1",
    )?.exists(params![run,job,target])?)
}

pub fn grant_artifact(d: &Arc<Daemon>, p: &Value) -> Result<Value> {
    let _serial = d.swarm_launch_lock.lock().unwrap();
    let run = required(p, "run_id")?;
    let artifact = required(p, "artifact_id")?;
    let target = required(p, "target_id")?;
    let generation = p["generation"].as_i64().ok_or_else(|| anyhow!("missing generation"))?;
    let revision = p["revision"].as_i64().ok_or_else(|| anyhow!("missing revision"))?;
    let store = d.store.lock().unwrap();
    let current = get(&store, run)?;
    if current["generation"] != generation || current["revision"] != revision {
        bail!("stale director generation or plan revision");
    }
    if current["status"] != "running" {
        bail!("swarm run is not granting artifact access");
    }
    if !current["allowed_targets"].as_array()
        .is_some_and(|allowed| allowed.iter().any(|value| value == target)) {
        bail!("unknown or disallowed destination");
    }
    let revoked = store.conn.prepare(
        "SELECT 1 FROM swarm_artifact_revocations WHERE run_id=?1 AND artifact_id=?2 AND target_id=?3",
    )?.exists(params![run,artifact,target])?;
    if revoked {
        bail!("artifact destination access was revoked");
    }
    let row: Option<(String,String)> = store.conn.query_row(
        "SELECT a.content,a.sha256 FROM swarm_artifacts a
         JOIN swarm_jobs source ON source.run_id=a.run_id AND source.id=a.job_id
         WHERE a.run_id=?1 AND a.id=?2
           AND source.status='accepted' AND source.plan_revision=a.source_revision
           AND EXISTS (SELECT 1 FROM swarm_decisions d, json_each(d.evidence) accepted
                       WHERE d.run_id=a.run_id AND d.job_id=a.job_id
                         AND d.attempt_id=a.attempt_id AND d.revision=a.source_revision
                         AND d.decision='accept' AND accepted.value=a.id
                         AND NOT EXISTS (SELECT 1 FROM swarm_messages later
                                         WHERE later.run_id=d.run_id AND later.job_id=d.job_id
                                           AND later.attempt_id=d.attempt_id
                                           AND later.kind IN ('result','submit')
                                           AND later.seq>d.reviewed_message_seq))
           AND EXISTS (SELECT 1 FROM swarm_admissions s
                       JOIN swarm_jobs consumer ON consumer.run_id=s.run_id AND consumer.id=s.job_id
                       JOIN swarm_attempts t ON t.id=s.attempt_id AND t.status='registered'
                       JOIN json_each(consumer.deps) dep ON dep.value=a.job_id
                       WHERE s.run_id=a.run_id AND s.target_id=?3
                         AND consumer.plan_revision=t.revision
                         AND consumer.status IN ('reserved','launching','running','submitted'))",
        params![run,artifact,target], |r| Ok((r.get(0)?,r.get(1)?)),
    ).optional()?;
    let (content,digest) = row.ok_or_else(|| anyhow!("no current dependent assignment for destination"))?;
    if format!("{:x}", Sha256::digest(content.as_bytes())) != digest {
        bail!("artifact integrity check failed");
    }
    let inserted = store.conn.execute(
        "INSERT OR IGNORE INTO swarm_artifact_grants(run_id,artifact_id,target_id,created_ms)
         VALUES(?1,?2,?3,?4)",
        params![run,artifact,target,crate::daemon::now()],
    )?;
    Ok(json!({"status":"granted","artifact_id":artifact,"target_id":target,
        "duplicate":inserted==0}))
}

pub fn revoke_artifact(d: &Arc<Daemon>, p: &Value) -> Result<Value> {
    let _serial = d.swarm_launch_lock.lock().unwrap();
    let run = required(p, "run_id")?;
    let artifact = required(p, "artifact_id")?;
    let target = required(p, "target_id")?;
    let generation = p["generation"].as_i64().ok_or_else(|| anyhow!("missing generation"))?;
    let revision = p["revision"].as_i64().ok_or_else(|| anyhow!("missing revision"))?;
    let (duplicate, workers, affected_jobs) = {
        let mut store = d.store.lock().unwrap();
        let current = get(&store, run)?;
        if current["generation"] != generation || current["revision"] != revision {
            bail!("stale director generation or plan revision");
        }
        if current["status"] == "completed" || current["status"] == "stopped" {
            bail!("swarm run is terminal");
        }
        if !current["allowed_targets"].as_array()
            .is_some_and(|allowed| allowed.iter().any(|value| value == target)) {
            bail!("unknown or disallowed destination");
        }
        let source_job: String = store.conn.query_row(
            "SELECT job_id FROM swarm_artifacts WHERE run_id=?1 AND id=?2",
            params![run,artifact], |r| r.get(0),
        ).optional()?.ok_or_else(|| anyhow!("unknown artifact"))?;
        let tx = store.conn.transaction()?;
        let inserted = tx.execute(
            "INSERT OR IGNORE INTO swarm_artifact_revocations(run_id,artifact_id,target_id,created_ms)
             VALUES(?1,?2,?3,?4)",
            params![run,artifact,target,crate::daemon::now()],
        )?;
        let mut jobs_stmt = tx.prepare(
            "SELECT DISTINCT a.job_id FROM swarm_attempts a
             JOIN swarm_admissions s ON s.attempt_id=a.id AND s.run_id=a.run_id
             JOIN swarm_jobs j ON j.run_id=a.run_id AND j.id=a.job_id
             WHERE a.run_id=?1 AND s.target_id=?2
             AND EXISTS (SELECT 1 FROM json_each(j.deps) dep WHERE dep.value=?3)",
        )?;
        let affected_jobs = jobs_stmt.query_map(params![run,target,source_job], |r| r.get::<_,String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        drop(jobs_stmt);
        let mut worker_stmt = tx.prepare(
            "SELECT DISTINCT l.overseer_run_id FROM swarm_worker_launches l
             JOIN swarm_attempts a ON a.id=l.attempt_id AND a.status='registered'
             JOIN swarm_admissions s ON s.attempt_id=a.id AND s.target_id=?2
             JOIN swarm_jobs j ON j.run_id=a.run_id AND j.id=a.job_id
             JOIN runs r ON r.id=l.overseer_run_id
               AND r.status IN ('queued','starting','running','waiting_for_user')
             WHERE a.run_id=?1
             AND EXISTS (SELECT 1 FROM json_each(j.deps) dep WHERE dep.value=?3)",
        )?;
        let workers = worker_stmt.query_map(params![run,target,source_job], |r| r.get::<_,String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        drop(worker_stmt);
        for job in &affected_jobs {
            tx.execute(
                "UPDATE swarm_jobs SET status='blocked',stop_reason='artifact_permission_revoked',updated_ms=?3
                 WHERE run_id=?1 AND id=?2 AND status IN ('reserved','launching','running','submitted','accepted')",
                params![run,job,crate::daemon::now()],
            )?;
        }
        tx.commit()?;
        (inserted == 0, workers, affected_jobs)
    };
    let mut interrupt_requested = Vec::new();
    let mut unconfirmed = Vec::new();
    for worker in workers {
        if d.interrupt(&worker).is_ok() {
            interrupt_requested.push(worker);
        } else {
            unconfirmed.push(worker);
        }
    }
    Ok(json!({"status":"revoked","artifact_id":artifact,"target_id":target,
        "duplicate":duplicate,"affected_jobs":affected_jobs,
        "interrupt_requested":interrupt_requested,"unconfirmed":unconfirmed}))
}

fn inline_limit(p: &Value) -> Result<usize> {
    let limit = p["max_inline_bytes"].as_u64().unwrap_or(MAX_INLINE as u64);
    if !(1024..=MAX_INLINE as u64).contains(&limit) {
        bail!("invalid inline context byte limit");
    }
    Ok(limit as usize)
}

fn attempt_target(store: &Store, p: &Value) -> Result<(String, i64)> {
    let run = required(p, "run_id")?;
    let job = required(p, "job_id")?;
    let attempt = required(p, "attempt_id")?;
    let token = required(p, "token")?;
    let revision = broker::check_attempt(store, run, job, attempt, token)?;
    let target: String = store
        .conn
        .query_row(
            "SELECT s.target_id FROM swarm_admissions s
         JOIN swarm_attempts a ON a.id=s.attempt_id AND a.status='registered'
         JOIN swarm_jobs j ON j.run_id=s.run_id AND j.id=s.job_id
         WHERE s.run_id=?1 AND s.job_id=?2 AND s.attempt_id=?3
           AND j.plan_revision=a.revision AND j.status NOT IN ('cancelled','superseded')",
            params![run, job, attempt],
            |r| r.get(0),
        )
        .optional()?
        .ok_or_else(|| anyhow!("attempt has no admitted target"))?;
    let current = get(store, run)?;
    if !current["allowed_targets"]
        .as_array()
        .is_some_and(|a| a.iter().any(|v| v == &target))
    {
        bail!("attempt destination is no longer allowed");
    }
    if revoked_dependency(&store.conn, run, job, &target)? {
        bail!("attempt destination lost required artifact access");
    }
    if current["status"] == "stopped" || current["status"] == "stopping" {
        bail!("swarm run no longer permits context delivery");
    }
    Ok((target, revision))
}

fn visible_artifact(
    store: &Store,
    run: &str,
    job: &str,
    target: &str,
    id: &str,
) -> Result<(String, String, String, String, i64, String)> {
    let row: (String, String, String, String, i64, String) = store
        .conn
        .query_row(
            "SELECT a.job_id,a.kind,a.content,a.sha256,a.source_revision,s.target_id
         FROM swarm_artifacts a
         JOIN swarm_jobs j ON j.run_id=a.run_id AND j.id=a.job_id
         JOIN swarm_admissions s ON s.run_id=a.run_id AND s.attempt_id=a.attempt_id
         WHERE a.run_id=?1 AND a.id=?2
         AND (s.target_id=?3 OR EXISTS
              (SELECT 1 FROM swarm_artifact_grants g WHERE g.run_id=a.run_id
               AND g.artifact_id=a.id AND g.target_id=?3))
         AND NOT EXISTS (SELECT 1 FROM swarm_artifact_revocations v
                         WHERE v.run_id=a.run_id AND v.artifact_id=a.id AND v.target_id=?3)
         AND (a.job_id=?4 OR (j.status='accepted' AND j.plan_revision=a.source_revision
             AND EXISTS (SELECT 1 FROM swarm_decisions d, json_each(d.evidence) accepted
                         WHERE d.run_id=a.run_id AND d.job_id=a.job_id
                           AND d.attempt_id=a.attempt_id AND d.revision=a.source_revision
                           AND d.decision='accept' AND accepted.value=a.id
                           AND NOT EXISTS (SELECT 1 FROM swarm_messages later
                                           WHERE later.run_id=d.run_id AND later.job_id=d.job_id
                                             AND later.attempt_id=d.attempt_id
                                             AND later.kind IN ('result','submit')
                                             AND later.seq>d.reviewed_message_seq))
             AND EXISTS (SELECT 1 FROM swarm_jobs consumer, json_each(consumer.deps) dep
                         WHERE consumer.run_id=a.run_id AND consumer.id=?4
                           AND dep.value=a.job_id)))",
            params![run, id, target, job],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                ))
            },
        )
        .optional()?
        .ok_or_else(|| anyhow!("artifact is unavailable to this destination"))?;
    if format!("{:x}", Sha256::digest(row.2.as_bytes())) != row.3 {
        bail!("artifact integrity check failed");
    }
    Ok(row)
}

pub fn worker_brief(store: &Store, p: &Value) -> Result<Value> {
    let limit = inline_limit(p)?;
    let run = required(p, "run_id")?;
    let job = required(p, "job_id")?;
    let (target, attempt_revision) = attempt_target(store, p)?;
    let current = get(store, run)?;
    let (title,acceptance,deps,status,job_revision): (String,String,String,String,i64) = store.conn.query_row(
        "SELECT title,acceptance,deps,status,plan_revision FROM swarm_jobs WHERE run_id=?1 AND id=?2",
        params![run,job], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?)),
    )?;
    if attempt_revision != job_revision || status == "superseded" || status == "cancelled" {
        bail!("stale worker assignment");
    }
    let deps: Value = serde_json::from_str(&deps)?;
    let mut result = json!({"run_id":run,"category":current["category"],"objective":current["objective"],
        "plan_revision":job_revision,"target_id":target,"job":{"id":job,"title":title,
        "acceptance":acceptance,"deps":deps},"artifacts":[]});
    if result.to_string().len() > limit {
        bail!("required worker brief exceeds inline context limit");
    }
    let mut stmt = store.conn.prepare(
        "SELECT a.id FROM swarm_artifacts a
         JOIN swarm_jobs j ON j.run_id=a.run_id AND j.id=a.job_id
         JOIN swarm_admissions s ON s.run_id=a.run_id AND s.attempt_id=a.attempt_id
         WHERE a.run_id=?1
         AND (s.target_id=?2 OR EXISTS
              (SELECT 1 FROM swarm_artifact_grants g WHERE g.run_id=a.run_id
               AND g.artifact_id=a.id AND g.target_id=?2))
         AND NOT EXISTS (SELECT 1 FROM swarm_artifact_revocations v
                         WHERE v.run_id=a.run_id AND v.artifact_id=a.id AND v.target_id=?2)
         AND (a.job_id=?3 OR (j.status='accepted' AND j.plan_revision=a.source_revision
             AND EXISTS (SELECT 1 FROM swarm_decisions d, json_each(d.evidence) accepted
                         WHERE d.run_id=a.run_id AND d.job_id=a.job_id
                           AND d.attempt_id=a.attempt_id AND d.revision=a.source_revision
                           AND d.decision='accept' AND accepted.value=a.id
                           AND NOT EXISTS (SELECT 1 FROM swarm_messages later
                                           WHERE later.run_id=d.run_id AND later.job_id=d.job_id
                                             AND later.attempt_id=d.attempt_id
                                             AND later.kind IN ('result','submit')
                                             AND later.seq>d.reviewed_message_seq))
             AND EXISTS (SELECT 1 FROM swarm_jobs consumer, json_each(consumer.deps) dep
                         WHERE consumer.run_id=a.run_id AND consumer.id=?3
                           AND dep.value=a.job_id))) ORDER BY a.id",
    )?;
    let ids = stmt
        .query_map(params![run, target, job], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for id in ids {
        let (source, kind, content, sha, revision, _) =
            visible_artifact(store, run, job, &target, &id)?;
        let reference = json!({"id":id,"job_id":source,"kind":kind,"sha256":sha,
            "source_revision":revision,"size_bytes":content.len()});
        result["artifacts"].as_array_mut().unwrap().push(reference);
        if result.to_string().len() > limit {
            bail!("worker artifact references exceed inline context limit");
        }
    }
    Ok(result)
}

pub fn artifact_chunk(store: &Store, p: &Value) -> Result<Value> {
    let run = required(p, "run_id")?;
    let job = required(p, "job_id")?;
    let id = required(p, "artifact_id")?;
    let (target, _) = attempt_target(store, p)?;
    let (source, kind, content, sha, revision, _) = visible_artifact(store, run, job, &target, id)?;
    let offset = p["offset_bytes"].as_u64().unwrap_or(0) as usize;
    let max = p["max_bytes"].as_u64().unwrap_or(8192);
    if !(256..=16384).contains(&max) || offset > content.len() || !content.is_char_boundary(offset)
    {
        bail!("invalid artifact byte range");
    }
    let mut end = (offset + max as usize).min(content.len());
    while !content.is_char_boundary(end) {
        end -= 1;
    }
    let next = if end < content.len() {
        json!(end)
    } else {
        Value::Null
    };
    Ok(json!({"id":id,"job_id":source,"kind":kind,"sha256":sha,
        "source_revision":revision,"size_bytes":content.len(),
        "offset_bytes":offset,"next_offset_bytes":next,"content":&content[offset..end]}))
}

pub fn director_summary(store: &Store, p: &Value) -> Result<Value> {
    let limit = inline_limit(p)?;
    let run = required(p, "run_id")?;
    let current = get(store, run)?;
    if p["generation"] != current["generation"] || p["revision"] != current["revision"] {
        bail!("stale director generation or plan revision");
    }
    let total: i64 = store.conn.query_row(
        "SELECT COUNT(*) FROM swarm_jobs WHERE run_id=?1",
        params![run],
        |r| r.get(0),
    )?;
    let mut counts = json!({"total":total});
    let mut grouped = store
        .conn
        .prepare("SELECT status,COUNT(*) FROM swarm_jobs WHERE run_id=?1 GROUP BY status")?;
    for row in grouped.query_map(params![run], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
    })? {
        let (status, count) = row?;
        counts[status] = json!(count);
    }
    let cursor = p["cursor"].as_str().unwrap_or("");
    let mut result = json!({"run_id":run,"generation":current["generation"],
        "revision":current["revision"],"status":current["status"],
        "counts":counts,"jobs":[],"next_cursor":Value::Null});
    if result.to_string().len() > limit {
        bail!("required director summary exceeds inline context limit");
    }
    let mut stmt = store
        .conn
        .prepare("SELECT id,title,status FROM swarm_jobs WHERE run_id=?1 AND id>?2 ORDER BY id")?;
    let rows = stmt
        .query_map(params![run, cursor], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for (id, title, status) in rows {
        result["jobs"]
            .as_array_mut()
            .unwrap()
            .push(json!({"id":id,"title":title,"status":status}));
        result["next_cursor"] = json!(id);
        if result.to_string().len() > limit {
            result["jobs"].as_array_mut().unwrap().pop();
            if result["jobs"].as_array().unwrap().is_empty() {
                bail!("one director summary job exceeds inline context limit");
            }
            let last = result["jobs"].as_array().unwrap().last().unwrap()["id"].clone();
            result["next_cursor"] = last;
            return Ok(result);
        }
    }
    result["next_cursor"] = Value::Null;
    Ok(result)
}
