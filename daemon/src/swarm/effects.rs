//! Durable side-effect intent. A lost receipt is uncertain until an external outcome
//! probe is reviewed; repeating the same intent never grants execution again.

use super::{broker, get, required};
use crate::store::Store;
use anyhow::{anyhow, bail, Result};
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

fn valid_id(value: &str) -> bool {
    !value.is_empty() && value.len() <= 128 && !value.chars().any(char::is_control)
}

pub fn begin(store: &mut Store, p: &Value) -> Result<Value> {
    let run = required(p, "run_id")?;
    let job = required(p, "job_id")?;
    let attempt = required(p, "attempt_id")?;
    let token = required(p, "token")?;
    let effect = required(p, "effect_id")?;
    let operation = required(p, "operation_id")?;
    let revision = p["revision"]
        .as_i64()
        .ok_or_else(|| anyhow!("missing revision"))?;
    if !valid_id(effect) || !valid_id(operation) {
        bail!("invalid effect or operation id");
    }
    let attempt_revision = broker::check_attempt(store, run, job, attempt, token)?;
    if attempt_revision != revision {
        bail!("stale effect assignment revision");
    }
    let hash = format!("{:x}", Sha256::digest(operation.as_bytes()));
    let old: Option<(String,String,String,String)> = store.conn.query_row(
        "SELECT job_id,attempt_id,operation_sha256,outcome FROM swarm_effects WHERE run_id=?1 AND effect_id=?2",
        params![run,effect], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?)),
    ).optional()?;
    if let Some((old_job, old_attempt, old_hash, outcome)) = old {
        if old_job != job || old_hash != hash {
            bail!("effect id reused with different job or operation");
        }
        return Ok(json!({"effect_id":effect,"origin_attempt_id":old_attempt,
            "outcome":outcome,"may_execute":false,"duplicate":true}));
    }
    let existing_operation: Option<String> = store
        .conn
        .query_row(
            "SELECT effect_id FROM swarm_effects WHERE run_id=?1 AND operation_sha256=?2",
            params![run, hash],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(existing) = existing_operation {
        bail!("operation already journaled as effect {existing}");
    }
    let current = get(store, run)?;
    if current["status"] != "planning" && current["status"] != "running" {
        bail!("swarm run does not permit a new side effect");
    }
    let (job_status, job_revision): (String, i64) = store
        .conn
        .query_row(
            "SELECT status,plan_revision FROM swarm_jobs WHERE run_id=?1 AND id=?2",
            params![run, job],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?
        .ok_or_else(|| anyhow!("unknown job"))?;
    let active: bool = store.conn.prepare(
        "SELECT 1 FROM swarm_attempts WHERE id=?1 AND run_id=?2 AND job_id=?3 AND status='registered'"
    )?.exists(params![attempt,run,job])?;
    if !active
        || job_revision != revision
        || !["reserved", "running"].contains(&job_status.as_str())
    {
        bail!("job is not executing the current assignment");
    }
    let now = crate::daemon::now();
    store.conn.execute(
        "INSERT INTO swarm_effects(run_id,effect_id,job_id,attempt_id,revision,operation_sha256,outcome,created_ms,updated_ms)
         VALUES(?1,?2,?3,?4,?5,?6,'unknown',?7,?7)",
        params![run,effect,job,attempt,revision,hash,now],
    )?;
    Ok(json!({"effect_id":effect,"origin_attempt_id":attempt,
        "outcome":"unknown","may_execute":true,"duplicate":false}))
}

pub fn reconcile(store: &mut Store, p: &Value) -> Result<Value> {
    let run = required(p, "run_id")?;
    let job = required(p, "job_id")?;
    let effect = required(p, "effect_id")?;
    let outcome = required(p, "outcome")?;
    let proof = required(p, "proof_artifact_id")?;
    let generation = p["generation"]
        .as_i64()
        .ok_or_else(|| anyhow!("missing generation"))?;
    let revision = p["revision"]
        .as_i64()
        .ok_or_else(|| anyhow!("missing revision"))?;
    // A worker-supplied artifact cannot prove that an external effect is absent.
    // Until a qualified outcome probe exists, only the conservative applied state
    // can be recorded; it never unlocks an automatic retry.
    if outcome != "applied" || !valid_id(proof) {
        bail!("invalid effect outcome or proof id");
    }
    let current = get(store, run)?;
    if current["generation"] != generation || current["revision"] != revision {
        bail!("stale director authority");
    }
    if !["planning", "running", "paused"].contains(&current["status"].as_str().unwrap_or("")) {
        bail!("run cannot reconcile effects in this state");
    }
    let (effect_job,attempt,source_revision,prior,prior_proof):(String,String,i64,String,Option<String>) =
        store.conn.query_row(
            "SELECT job_id,attempt_id,revision,outcome,proof_artifact_id FROM swarm_effects WHERE run_id=?1 AND effect_id=?2",
            params![run,effect], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?)),
        ).optional()?.ok_or_else(|| anyhow!("unknown effect"))?;
    if effect_job != job {
        bail!("effect belongs to a different job");
    }
    if prior != "unknown" {
        if prior == outcome && prior_proof.as_deref() == Some(proof) {
            return Ok(
                json!({"effect_id":effect,"outcome":outcome,"may_execute":false,"duplicate":true}),
            );
        }
        bail!("effect outcome already reconciled differently");
    }
    let artifact: Option<(String, String, i64, String, String)> = store
        .conn
        .query_row(
            "SELECT attempt_id,kind,source_revision,content,sha256 FROM swarm_artifacts
         WHERE run_id=?1 AND job_id=?2 AND id=?3",
            params![run, job, proof],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .optional()?;
    let (artifact_attempt, kind, artifact_revision, content, digest) =
        artifact.ok_or_else(|| anyhow!("missing effect probe artifact"))?;
    if artifact_attempt != attempt
        || kind != "effect_probe"
        || artifact_revision != source_revision
        || format!("{:x}", Sha256::digest(content.as_bytes())) != digest
    {
        bail!("effect probe provenance or integrity failed");
    }
    let mut stmt = store.conn.prepare(
        "SELECT payload FROM swarm_messages WHERE run_id=?1 AND job_id=?2 AND attempt_id=?3
         AND sender=?3 AND recipient='director' AND revision=?4",
    )?;
    let reports = stmt
        .query_map(params![run, job, attempt, source_revision], |row| {
            row.get::<_, String>(0)
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(stmt);
    let submitted = reports.iter().any(|raw| {
        serde_json::from_str::<Value>(raw)
            .ok()
            .is_some_and(|payload| {
                payload["artifact_ids"]
                    .as_array()
                    .is_some_and(|ids| ids.iter().any(|id| id.as_str() == Some(proof)))
            })
    });
    if !submitted {
        bail!("effect probe was not submitted to the director");
    }
    let now = crate::daemon::now();
    let tx = store.conn.transaction()?;
    tx.execute(
        "UPDATE swarm_effects SET outcome=?3,proof_artifact_id=?4,updated_ms=?5
        WHERE run_id=?1 AND effect_id=?2 AND outcome='unknown'",
        params![run, effect, outcome, proof, now],
    )?;
    tx.commit()?;
    Ok(json!({"effect_id":effect,"outcome":outcome,"may_execute":false,"duplicate":false}))
}
