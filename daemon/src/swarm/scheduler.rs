//! Fixture-only round-robin selection across eligible category backlogs.
//! Target ranking remains Auto Mode's responsibility; this consumes one injected target.

use super::{admission, required};
use crate::store::Store;
use anyhow::{bail, Result};
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

pub fn next(store: &mut Store, p: &Value) -> Result<Value> {
    let request_id = required(p, "request_id")?;
    let target = required(p, "target_id")?;
    if request_id.is_empty() || request_id.len() > 128 {
        bail!("invalid scheduler request id");
    }
    let digest = format!("{:x}", Sha256::digest(p.to_string().as_bytes()));
    let previous: Option<(String, String, String, String, String)> = store
        .conn
        .query_row(
            "SELECT request_sha256,run_id,job_id,attempt_id,target_id
         FROM swarm_scheduler_admissions WHERE request_id=?1",
            params![request_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .optional()?;
    if let Some((old, run, job, attempt, target_id)) = previous {
        if old != digest {
            bail!("scheduler request id reused with different input");
        }
        return Ok(
            json!({"status":"already_admitted","run_id":run,"job_id":job,
            "attempt_id":attempt,"target_id":target_id}),
        );
    }
    let last: String = store
        .conn
        .query_row(
            "SELECT last_category_key FROM swarm_scheduler_cursor WHERE id=1",
            [],
            |r| r.get(0),
        )
        .optional()?
        .unwrap_or_default();
    let mut stmt = store.conn.prepare(
        "SELECT id,category_key,generation,revision,allowed_targets FROM swarm_runs
         WHERE status IN ('planning','running')
         AND EXISTS(SELECT 1 FROM swarm_jobs WHERE run_id=swarm_runs.id AND status='ready')
         ORDER BY category_key,id",
    )?;
    let runs = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, String>(4)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(stmt);
    if runs.is_empty() {
        return Ok(json!({"status":"blocked","reason":"no_ready_category"}));
    }
    let start = runs.iter().position(|r| r.1 > last).unwrap_or(0);
    let mut blocked = Vec::new();
    for n in 0..runs.len() {
        let (run, key, generation, revision, allowed) = &runs[(start + n) % runs.len()];
        let allowed: Value = serde_json::from_str(allowed)?;
        if !allowed
            .as_array()
            .is_some_and(|a| a.iter().any(|v| v == target))
        {
            continue;
        }
        let job: String = store.conn.query_row(
            "SELECT id FROM swarm_jobs WHERE run_id=?1 AND status='ready' ORDER BY id LIMIT 1",
            params![run],
            |r| r.get(0),
        )?;
        let mut attempt = p.clone();
        attempt["run_id"] = json!(run);
        attempt["job_id"] = json!(job);
        attempt["generation"] = json!(generation);
        attempt["revision"] = json!(revision);
        let result = admission::admit_scheduled(
            store,
            &attempt,
            admission::ScheduledCommit {
                request_id,
                request_sha256: &digest,
                category_key: key,
            },
        )?;
        if result["status"] == "admitted" {
            let mut result = result;
            result["run_id"] = json!(run);
            result["job_id"] = json!(job);
            return Ok(result);
        }
        if blocked.len() < 20 {
            blocked.push(json!({"run_id":run,"reason":result["reason"]}));
        }
    }
    Ok(json!({"status":"blocked","reason":"all_categories_blocked","candidates":blocked}))
}
