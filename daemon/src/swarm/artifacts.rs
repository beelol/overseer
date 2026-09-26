use super::{broker, get, required};
use crate::store::Store;
use anyhow::{anyhow, bail, Result};
use rusqlite::{params, OptionalExtension, Transaction};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

pub fn put(store: &mut Store, p: &Value) -> Result<Value> {
    let run = required(p, "run_id")?;
    let job = required(p, "job_id")?;
    let attempt = required(p, "attempt_id")?;
    let token = required(p, "token")?;
    let id = required(p, "artifact_id")?;
    let kind = required(p, "kind")?;
    let content = required(p, "content")?;
    let revision = p["source_revision"]
        .as_i64()
        .ok_or_else(|| anyhow!("missing source revision"))?;
    if id.is_empty()
        || id.len() > 128
        || kind.is_empty()
        || kind.len() > 80
        || content.len() > 256 * 1024
    {
        bail!("invalid or oversized artifact");
    }
    let attempt_revision = broker::check_attempt(store, run, job, attempt, token)?;
    if revision != attempt_revision {
        bail!("artifact source revision does not match attempt");
    }
    let safe = crate::redact::redact(content);
    let digest = format!("{:x}", Sha256::digest(safe.as_bytes()));
    let old: Option<(String, String, String, i64, String, String)> = store.conn.query_row(
        "SELECT job_id,attempt_id,kind,source_revision,content,sha256 FROM swarm_artifacts WHERE run_id=?1 AND id=?2",
        params![run,id], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?)),
    ).optional()?;
    if let Some((old_job, old_attempt, old_kind, old_rev, old_content, old_digest)) = old {
        if format!("{:x}", Sha256::digest(old_content.as_bytes())) != old_digest {
            bail!("artifact integrity check failed");
        }
        if (
            old_job.as_str(),
            old_attempt.as_str(),
            old_kind.as_str(),
            old_rev,
            old_digest.as_str(),
        ) != (job, attempt, kind, revision, digest.as_str())
        {
            bail!("artifact id reused with different content or provenance");
        }
        return Ok(json!({"id":id,"sha256":digest,"duplicate":true}));
    }
    store.conn.execute(
        "INSERT INTO swarm_artifacts(id,run_id,job_id,attempt_id,source_revision,kind,content,sha256,created_ms) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
        params![id,run,job,attempt,revision,kind,safe,digest,crate::daemon::now()],
    )?;
    Ok(json!({"id":id,"sha256":digest,"duplicate":false}))
}

pub fn decide(store: &mut Store, p: &Value) -> Result<Value> {
    let run = required(p, "run_id")?;
    let job = required(p, "job_id")?;
    let decision = required(p, "decision")?;
    let generation = p["generation"]
        .as_i64()
        .ok_or_else(|| anyhow!("missing generation"))?;
    let revision = p["revision"]
        .as_i64()
        .ok_or_else(|| anyhow!("missing revision"))?;
    if !["accept", "reject", "unresolved"].contains(&decision) {
        bail!("invalid decision");
    }
    let current = get(store, run)?;
    if current["generation"] != generation {
        bail!("stale director generation");
    }
    if current["revision"] != revision {
        bail!("stale plan revision");
    }
    if current["status"] == "stopping" || current["status"] == "stopped" || current["status"] == "stalled" {
        bail!("run cannot accept director decisions in this state");
    }
    let evidence = p["evidence"]
        .as_array()
        .ok_or_else(|| anyhow!("evidence must be an array"))?;
    if evidence.is_empty() || evidence.len() > 100 || evidence.iter().any(|v| v.as_str().is_none())
    {
        bail!("decision requires artifact evidence");
    }
    let job_revision: i64 = store
        .conn
        .query_row(
            "SELECT plan_revision FROM swarm_jobs WHERE run_id=?1 AND id=?2",
            params![run, job],
            |r| r.get(0),
        )
        .optional()?
        .ok_or_else(|| anyhow!("unknown job"))?;
    let mut attempt_id: Option<String> = None;
    let mut has_reproduction = false;
    for artifact in evidence {
        let id = artifact.as_str().unwrap();
        let found: Option<(String,i64,String,String,String)> = store.conn.query_row(
            "SELECT attempt_id,source_revision,content,sha256,kind FROM swarm_artifacts WHERE run_id=?1 AND job_id=?2 AND id=?3",
            params![run,job,id], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?)),
        ).optional()?;
        let (attempt, source_rev, content, digest, kind) =
            found.ok_or_else(|| anyhow!("missing artifact evidence {id}"))?;
        if source_rev != job_revision {
            bail!("stale artifact source revision");
        }
        if format!("{:x}", Sha256::digest(content.as_bytes())) != digest {
            bail!("artifact integrity check failed");
        }
        if attempt_id.as_deref().is_some_and(|a| a != attempt) {
            bail!("mixed attempt evidence requires separate review");
        }
        has_reproduction |= kind == "reproduction";
        attempt_id = Some(attempt);
    }
    let attempt = attempt_id.ok_or_else(|| anyhow!("missing attempt"))?;
    let mut stmt = store.conn.prepare(
        "SELECT payload FROM swarm_messages WHERE run_id=?1 AND job_id=?2 AND attempt_id=?3 AND kind IN ('result','submit') AND revision=?4"
    )?;
    let submitted = stmt
        .query_map(params![run, job, attempt, job_revision], |r| {
            r.get::<_, String>(0)
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(stmt);
    if decision == "accept" && submitted.iter().any(|raw| {
        serde_json::from_str::<Value>(raw).ok()
            .is_some_and(|payload| payload["audit_outcome"] == "environment_failure")
    }) {
        bail!("environment failure cannot be accepted as a passed check");
    }
    if decision == "accept" && !has_reproduction && submitted.iter().any(|raw| {
        serde_json::from_str::<Value>(raw).ok()
            .is_some_and(|payload| payload["audit_outcome"] == "confirmed_defect")
    }) {
        bail!("confirmed defect requires reproduction artifact evidence");
    }
    let linked = submitted.iter().any(|raw| {
        let payload: Value = serde_json::from_str(raw).unwrap_or(Value::Null);
        evidence.iter().all(|id| {
            payload["artifact_ids"]
                .as_array()
                .is_some_and(|arr| arr.contains(id))
        })
    });
    if !linked {
        bail!("artifact evidence was not submitted by this attempt");
    }
    let state: String = store
        .conn
        .query_row(
            "SELECT status FROM swarm_jobs WHERE run_id=?1 AND id=?2",
            params![run, job],
            |r| r.get(0),
        )
        .optional()?
        .ok_or_else(|| anyhow!("unknown job"))?;
    if ["cancelled", "cancel_requested", "superseded", "failed"].contains(&state.as_str()) {
        bail!("job cannot be decided in this state");
    }
    let evidence_text = Value::Array(evidence.to_vec()).to_string();
    let old: Option<String> = store.conn.query_row(
        "SELECT evidence FROM swarm_decisions WHERE run_id=?1 AND job_id=?2 AND attempt_id=?3 AND decision=?4",
        params![run,job,attempt,decision], |r| r.get(0),
    ).optional()?;
    if let Some(old_evidence) = old {
        if old_evidence != evidence_text {
            bail!("decision replay changes evidence");
        }
        return Ok(json!({"job_id":job,"decision":decision,"duplicate":true}));
    }
    let other_decision: Option<String> = store.conn.query_row(
        "SELECT decision FROM swarm_decisions WHERE run_id=?1 AND job_id=?2 AND attempt_id=?3 LIMIT 1",
        params![run,job,attempt], |r| r.get(0),
    ).optional()?;
    if other_decision.is_some() {
        bail!("attempt already has a different review decision");
    }
    if state == "accepted" {
        bail!("job already accepted");
    }
    if state != "reserved" && state != "submitted" {
        bail!("job is not awaiting review");
    }
    let now = crate::daemon::now();
    let tx = store.conn.transaction()?;
    tx.execute(
        "INSERT INTO swarm_decisions(run_id,job_id,attempt_id,revision,decision,evidence,created_ms) VALUES(?1,?2,?3,?4,?5,?6,?7)",
        params![run,job,attempt,revision,decision,evidence_text,now],
    )?;
    let next = match decision {
        "accept" => "accepted",
        "reject" => "rejected",
        _ => "blocked",
    };
    tx.execute(
        "UPDATE swarm_jobs SET status=?3,updated_ms=?4 WHERE run_id=?1 AND id=?2",
        params![run, job, next, now],
    )?;
    if next == "accepted" {
        release_and_unlock(&tx, run, job, now)?;
    }
    tx.commit()?;
    Ok(json!({"job_id":job,"decision":decision,"status":next,"duplicate":false}))
}

fn release_and_unlock(tx: &Transaction<'_>, run: &str, job: &str, now: i64) -> Result<()> {
    let active: i64 = tx.query_row(
        "SELECT COUNT(*) FROM swarm_attempts WHERE run_id=?1 AND job_id=?2 AND status='registered'",
        params![run, job],
        |r| r.get(0),
    )?;
    if active > 0 {
        return Ok(());
    }
    tx.execute("UPDATE swarm_claims SET status='released',updated_ms=?3 WHERE run_id=?1 AND job_id=?2 AND status='active'", params![run,job,now])?;
    let mut stmt =
        tx.prepare("SELECT id,deps FROM swarm_jobs WHERE run_id=?1 AND status='planned'")?;
    let planned = stmt
        .query_map(params![run], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(stmt);
    for (id, raw_deps) in planned {
        let deps: Vec<String> = serde_json::from_str(&raw_deps)?;
        if deps.is_empty() {
            continue;
        }
        let mut ready = true;
        for dep in deps {
            let (state,active): (String,i64) = tx.query_row(
                "SELECT j.status,(SELECT COUNT(*) FROM swarm_attempts a WHERE a.run_id=j.run_id AND a.job_id=j.id AND a.status='registered') FROM swarm_jobs j WHERE j.run_id=?1 AND j.id=?2",
                params![run,dep], |r| Ok((r.get(0)?,r.get(1)?)),
            )?;
            if state != "accepted" || active > 0 {
                ready = false;
                break;
            }
        }
        if ready {
            tx.execute(
                "UPDATE swarm_jobs SET status='ready',updated_ms=?3 WHERE run_id=?1 AND id=?2",
                params![run, id, now],
            )?;
        }
    }
    Ok(())
}

pub fn confirm_exit(store: &mut Store, p: &Value) -> Result<Value> {
    let run = required(p, "run_id")?;
    let job = required(p, "job_id")?;
    let attempt = required(p, "attempt_id")?;
    let generation = p["generation"]
        .as_i64()
        .ok_or_else(|| anyhow!("missing generation"))?;
    let revision = p["revision"]
        .as_i64()
        .ok_or_else(|| anyhow!("missing revision"))?;
    let current = get(store, run)?;
    if current["generation"] != generation {
        bail!("stale director generation");
    }
    if current["revision"] != revision {
        bail!("stale plan revision");
    }
    let (attempt_status, attempt_revision): (String, i64) = store
        .conn
        .query_row(
            "SELECT status,revision FROM swarm_attempts WHERE id=?1 AND run_id=?2 AND job_id=?3",
            params![attempt, run, job],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?
        .ok_or_else(|| anyhow!("unknown attempt"))?;
    if attempt_status == "finished" {
        return Ok(json!({"attempt_id":attempt,"status":"finished","duplicate":true}));
    }
    if attempt_status != "registered" {
        bail!("attempt cannot finish in this state");
    }
    let linked: Option<(Option<String>,Option<String>,Option<i64>)> = store.conn.query_row(
        "SELECT l.overseer_run_id,r.status,r.ended_ms FROM swarm_worker_launches l
         LEFT JOIN runs r ON r.id=l.overseer_run_id WHERE l.attempt_id=?1 AND l.run_id=?2",
        params![attempt,run], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?)),
    ).optional()?;
    if let Some((linked_run,status,ended)) = linked {
        if linked_run.is_none() || status.as_deref().is_none_or(|s| crate::daemon::ACTIVE.contains(&s)) || ended.is_none() {
            bail!("linked worker exit is not confirmed");
        }
    }
    let now = crate::daemon::now();
    let tx = store.conn.transaction()?;
    tx.execute(
        "UPDATE swarm_attempts SET status='finished' WHERE id=?1",
        params![attempt],
    )?;
    let (job_status, count, job_revision): (String, i64, i64) = tx.query_row(
        "SELECT status,attempt_count,plan_revision FROM swarm_jobs WHERE run_id=?1 AND id=?2",
        params![run, job],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    if job_status == "accepted" {
        release_and_unlock(&tx, run, job, now)?;
    } else if current["status"] == "stopping" {
        tx.execute(
            "UPDATE swarm_jobs SET status='cancelled',updated_ms=?3 WHERE run_id=?1 AND id=?2",
            params![run, job, now],
        )?;
        tx.execute("UPDATE swarm_claims SET status='released',updated_ms=?3 WHERE run_id=?1 AND job_id=?2 AND status='active'",params![run,job,now])?;
    } else if job_status == "rejected" {
        let next = if count < 2 { "ready" } else { "failed" };
        tx.execute(
            "UPDATE swarm_jobs SET status=?3,updated_ms=?4 WHERE run_id=?1 AND id=?2",
            params![run, job, next, now],
        )?;
        tx.execute("UPDATE swarm_claims SET status='released',updated_ms=?3 WHERE run_id=?1 AND job_id=?2 AND status='active'",params![run,job,now])?;
    } else if job_status == "cancel_requested" && job_revision > attempt_revision {
        let deps_raw: String = tx.query_row(
            "SELECT deps FROM swarm_jobs WHERE run_id=?1 AND id=?2",
            params![run, job],
            |r| r.get(0),
        )?;
        let deps: Vec<String> = serde_json::from_str(&deps_raw)?;
        let mut ready = deps.is_empty();
        if !deps.is_empty() {
            ready = true;
            for dep in deps {
                let dep_status: String = tx.query_row(
                    "SELECT status FROM swarm_jobs WHERE run_id=?1 AND id=?2",
                    params![run, dep],
                    |r| r.get(0),
                )?;
                if dep_status != "accepted" {
                    ready = false;
                    break;
                }
            }
        }
        let next = if count >= 2 {
            "failed"
        } else if ready {
            "ready"
        } else {
            "planned"
        };
        tx.execute(
            "UPDATE swarm_jobs SET status=?3,updated_ms=?4 WHERE run_id=?1 AND id=?2",
            params![run, job, next, now],
        )?;
        tx.execute("UPDATE swarm_claims SET status='released',updated_ms=?3 WHERE run_id=?1 AND job_id=?2 AND status='active'",params![run,job,now])?;
    }
    tx.commit()?;
    Ok(json!({"attempt_id":attempt,"status":"finished","duplicate":false}))
}
