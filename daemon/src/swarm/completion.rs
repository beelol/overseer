use super::required;
use crate::store::Store;
use anyhow::{anyhow, bail, Result};
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::HashSet;

pub fn get(store: &Store, run: &str) -> Result<Value> {
    let saved: Option<(i64, i64, String, String, String, i64)> = store
        .conn
        .query_row(
            "SELECT generation,revision,summary,verification,checks,created_ms FROM swarm_completions WHERE run_id=?1",
            params![run],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
        )
        .optional()?;
    match saved {
        Some((generation, revision, summary, verification, checks, created_ms)) => Ok(json!({
            "generation":generation,"revision":revision,"summary":summary,
            "verification":verification,"checks":serde_json::from_str::<Value>(&checks)?,
            "created_ms":created_ms
        })),
        None => Ok(Value::Null),
    }
}

pub fn complete(store: &mut Store, p: &Value) -> Result<Value> {
    let run = required(p, "run_id")?;
    let request_id = required(p, "request_id")?;
    let generation = p["generation"]
        .as_i64()
        .ok_or_else(|| anyhow!("missing generation"))?;
    let revision = p["revision"]
        .as_i64()
        .ok_or_else(|| anyhow!("missing revision"))?;
    let summary = required(p, "summary")?.trim();
    let verification = required(p, "verification")?.trim();
    if request_id.is_empty()
        || request_id.len() > 128
        || summary.is_empty()
        || summary.len() > 32 * 1024
        || verification.is_empty()
        || verification.len() > 8 * 1024
    {
        bail!("invalid completion request");
    }
    let checks = p["checks"]
        .as_array()
        .ok_or_else(|| anyhow!("checks must be an array"))?;
    if checks.is_empty() || checks.len() > 1000 {
        bail!("completion requires bounded job checks");
    }
    let mut covered = HashSet::new();
    for check in checks {
        let job = required(check, "job_id")?;
        if job.is_empty() || job.len() > 128 || !covered.insert(job) || check["outcome"] != "passed"
        {
            bail!("completion requires one passed check per job");
        }
        let evidence = check["evidence"]
            .as_array()
            .ok_or_else(|| anyhow!("check evidence must be an array"))?;
        if evidence.is_empty() || evidence.len() > 100 {
            bail!("completion check requires bounded evidence");
        }
        let mut unique = HashSet::new();
        for item in evidence {
            let id = item
                .as_str()
                .ok_or_else(|| anyhow!("evidence id must be a string"))?;
            if id.is_empty() || id.len() > 128 || !unique.insert(id) {
                bail!("completion check has invalid or duplicate evidence");
            }
        }
    }
    let request_sha256 = format!("{:x}", Sha256::digest(p.to_string().as_bytes()));
    let tx = store.conn.transaction()?;
    let old: Option<String> = tx
        .query_row(
            "SELECT request_sha256 FROM swarm_completions WHERE run_id=?1",
            params![run],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(old_hash) = old {
        if old_hash != request_sha256 {
            bail!("completion replay changes the accepted request");
        }
        return Ok(json!({"run_id":run,"status":"completed","duplicate":true}));
    }
    let current: (i64, i64, String) = tx
        .query_row(
            "SELECT generation,revision,status FROM swarm_runs WHERE id=?1",
            params![run],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?
        .ok_or_else(|| anyhow!("unknown swarm run"))?;
    if generation != current.0 || revision != current.1 {
        bail!("stale completion generation or revision");
    }
    if current.2 != "running" {
        bail!("swarm run is not running");
    }
    let mut stmt = tx.prepare("SELECT id,plan_revision,status FROM swarm_jobs WHERE run_id=?1")?;
    let jobs = stmt
        .query_map(params![run], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(stmt);
    if jobs.is_empty()
        || jobs.len() != covered.len()
        || jobs
            .iter()
            .any(|(id, _, status)| status != "accepted" || !covered.contains(id.as_str()))
    {
        bail!("completion requires every planned job to be accepted and checked");
    }
    let active_workers: i64 = tx.query_row(
        "SELECT COUNT(*) FROM swarm_attempts WHERE run_id=?1 AND status='registered'",
        params![run],
        |r| r.get(0),
    )?;
    let active_director: i64 = tx.query_row(
        "SELECT COUNT(*) FROM swarm_director_turns WHERE run_id=?1 AND status='active'",
        params![run],
        |r| r.get(0),
    )?;
    let pending_inbox: i64 = tx.query_row(
        "SELECT COUNT(*) FROM swarm_messages WHERE run_id=?1 AND recipient='director' AND phase!='applied'",
        params![run], |r| r.get(0),
    )?;
    if active_workers > 0 || active_director > 0 || pending_inbox > 0 {
        bail!("completion requires confirmed worker exits and an applied director inbox");
    }
    for (job, job_revision, _) in jobs {
        let check = checks
            .iter()
            .find(|c| c["job_id"] == job)
            .ok_or_else(|| anyhow!("missing job check"))?;
        let mut claimed: Vec<String> = check["evidence"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_owned())
            .collect();
        claimed.sort();
        let decision: Option<(String, String, i64)> = tx.query_row(
            "SELECT attempt_id,evidence,reviewed_message_seq FROM swarm_decisions WHERE run_id=?1 AND job_id=?2 AND decision='accept' ORDER BY id DESC LIMIT 1",
            params![run,job], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?)),
        ).optional()?;
        let (attempt, accepted_evidence, reviewed_message_seq) =
            decision.ok_or_else(|| anyhow!("accepted job lacks a review decision"))?;
        let mut results = tx.prepare(
            "SELECT payload FROM swarm_messages WHERE run_id=?1 AND job_id=?2 AND attempt_id=?3 AND revision=?4 AND kind='result'",
        )?;
        let payloads = results
            .query_map(params![run, job, attempt, job_revision], |r| {
                r.get::<_, String>(0)
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        drop(results);
        if payloads.iter().any(|raw| {
            serde_json::from_str::<Value>(raw)
                .ok()
                .is_some_and(|payload| payload["audit_outcome"] == "environment_failure")
        }) {
            bail!("environment failure cannot become a passed completion check");
        }
        let late_results: i64 = tx.query_row(
            "SELECT COUNT(*) FROM swarm_messages WHERE run_id=?1 AND job_id=?2 AND attempt_id=?3
             AND revision=?4 AND kind IN ('result','submit') AND seq>?5",
            params![run, job, attempt, job_revision, reviewed_message_seq],
            |r| r.get(0),
        )?;
        if late_results > 0 {
            bail!("new result after review requires a fresh decision");
        }
        let attempt_revision: i64 = tx
            .query_row(
                "SELECT revision FROM swarm_attempts WHERE id=?1 AND run_id=?2 AND job_id=?3",
                params![attempt, run, job],
                |r| r.get(0),
            )
            .optional()?
            .ok_or_else(|| anyhow!("accepted attempt is missing"))?;
        if attempt_revision != job_revision {
            bail!("accepted decision has stale source revision");
        }
        let mut reviewed: Vec<String> = serde_json::from_str(&accepted_evidence)?;
        reviewed.sort();
        if reviewed != claimed {
            bail!("completion evidence differs from accepted review");
        }
        if super::artifacts::pending_patch_integration(&tx,run,&job)? {
            bail!("unintegrated patch blocks completion");
        }
        for id in claimed {
            let artifact: Option<(String, i64, String, String)> = tx.query_row(
                "SELECT attempt_id,source_revision,content,sha256 FROM swarm_artifacts WHERE run_id=?1 AND job_id=?2 AND id=?3",
                params![run,job,id], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)),
            ).optional()?;
            let (source_attempt, source_revision, content, digest) =
                artifact.ok_or_else(|| anyhow!("completion evidence artifact is missing"))?;
            if source_attempt != attempt || source_revision != job_revision {
                bail!("completion evidence provenance changed");
            }
            if format!("{:x}", Sha256::digest(content.as_bytes())) != digest {
                bail!("completion evidence artifact failed integrity check");
            }
        }
    }
    let now = crate::daemon::now();
    tx.execute(
        "INSERT INTO swarm_completions(run_id,request_sha256,generation,revision,summary,verification,checks,created_ms) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
        params![run,request_sha256,generation,revision,summary,verification,p["checks"].to_string(),now],
    )?;
    tx.execute(
        "UPDATE swarm_runs SET status='completed',updated_ms=?2 WHERE id=?1 AND status='running'",
        params![run, now],
    )?;
    tx.commit()?;
    Ok(json!({"run_id":run,"status":"completed","duplicate":false}))
}
