//! Bounded, scoped context for fixture workers and director turns.
//! Live cross-account context transfer needs an explicit destination ACL.

use super::{broker, get, required};
use crate::store::Store;
use anyhow::{anyhow, bail, Result};
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

const MAX_INLINE: usize = 32 * 1024;

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
         WHERE a.run_id=?1 AND a.id=?2 AND s.target_id=?3
         AND (a.job_id=?4 OR (j.status='accepted' AND j.plan_revision=a.source_revision
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
         WHERE a.run_id=?1 AND s.target_id=?2
         AND (a.job_id=?3 OR (j.status='accepted' AND j.plan_revision=a.source_revision
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
