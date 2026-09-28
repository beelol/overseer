//! A director-declared evidence disagreement remains visible and blocks review
//! until a separate, accepted reproduction resolves it or it is marked unresolved.

use super::{get, required};
use crate::store::Store;
use anyhow::{anyhow, bail, Result};
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

fn current_revision<'a>(store: &Store, p: &'a Value) -> Result<&'a str> {
    let run = required(p, "run_id")?;
    super::owner::require(store, run, p)?;
    let current = get(store, run)?;
    if current["generation"] != p["generation"] || current["revision"] != p["revision"] {
        bail!("stale conflict director generation or plan revision");
    }
    if current["status"] != "running" && current["status"] != "planning" {
        bail!("run cannot change conflicts in this state");
    }
    Ok(run)
}

fn artifact_chain(store: &Store, run: &str, job: &str, id: &str) -> Result<(String, String, String)> {
    let found: Option<(String, i64, String, String, String, i64, String)> = store.conn.query_row(
        "SELECT a.attempt_id,a.source_revision,a.kind,a.content,a.sha256,j.plan_revision,j.status
         FROM swarm_artifacts a JOIN swarm_jobs j ON j.run_id=a.run_id AND j.id=a.job_id
         WHERE a.run_id=?1 AND a.job_id=?2 AND a.id=?3",
        params![run,job,id], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,
            r.get(4)?,r.get(5)?,r.get(6)?)),
    ).optional()?;
    let (attempt, source_revision, kind, content, digest, job_revision, state) =
        found.ok_or_else(|| anyhow!("conflict evidence artifact is missing"))?;
    if source_revision != job_revision || format!("{:x}", Sha256::digest(content.as_bytes())) != digest {
        bail!("conflict evidence revision or integrity changed");
    }
    let mut stmt=store.conn.prepare(
        "SELECT payload FROM swarm_messages WHERE run_id=?1 AND job_id=?2 AND attempt_id=?3
         AND revision=?4 AND kind IN ('result','submit')")?;
    let payloads=stmt.query_map(params![run,job,attempt,job_revision],|r|r.get::<_,String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if !payloads.iter().any(|raw| serde_json::from_str::<Value>(raw).ok()
        .is_some_and(|v| v["artifact_ids"].as_array().is_some_and(|ids| ids.iter().any(|a| a==id)))) {
        bail!("conflict artifact has no submitted result");
    }
    Ok((attempt,kind,state))
}

pub fn open(store: &mut Store, p: &Value) -> Result<Value> {
    let run=current_revision(store,p)?.to_string();
    let id=required(p,"conflict_id")?;
    let left_job=required(p,"left_job_id")?;
    let left_artifact=required(p,"left_artifact_id")?;
    let right_job=required(p,"right_job_id")?;
    let right_artifact=required(p,"right_artifact_id")?;
    let reason=required(p,"reason")?.trim();
    if id.is_empty() || id.len()>128 || left_job==right_job || reason.is_empty()
        || reason.len()>1000 || [left_job,left_artifact,right_job,right_artifact]
            .iter().any(|v| v.is_empty() || v.len()>128) {
        bail!("invalid bounded conflict");
    }
    let reason=crate::redact::redact(reason);
    let prior: Option<(i64,i64,String,String,String,String,String,String)> = store.conn.query_row(
        "SELECT generation,revision,left_job_id,left_artifact_id,right_job_id,
                right_artifact_id,reason,status FROM swarm_conflicts
         WHERE run_id=?1 AND conflict_id=?2",
        params![run,id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,
            r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?)),
    ).optional()?;
    if let Some((generation,revision,lj,la,rj,ra,old_reason,status))=prior {
        if generation!=p["generation"].as_i64().unwrap_or(-1)
            || revision!=p["revision"].as_i64().unwrap_or(-1)
            || lj!=left_job || la!=left_artifact || rj!=right_job
            || ra!=right_artifact || old_reason!=reason {
            bail!("conflict id reused with different input");
        }
        return Ok(json!({"conflict_id":id,"status":status,"duplicate":true}));
    }
    let (left_attempt,_,left_state)=artifact_chain(store,&run,left_job,left_artifact)?;
    let (right_attempt,_,right_state)=artifact_chain(store,&run,right_job,right_artifact)?;
    if left_attempt==right_attempt || [left_state.as_str(),right_state.as_str()]
        .iter().any(|state| !["submitted","reserved","accepted"].contains(state)) {
        bail!("conflict requires separate submitted or accepted attempts");
    }
    let now=crate::daemon::now();
    let tx=store.conn.transaction()?;
    tx.execute(
        "INSERT INTO swarm_conflicts(run_id,conflict_id,generation,revision,left_job_id,
          left_artifact_id,right_job_id,right_artifact_id,reason,status,created_ms,updated_ms)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,'open',?10,?10)",
        params![run,id,p["generation"].as_i64(),p["revision"].as_i64(),
            left_job,left_artifact,right_job,right_artifact,reason,now],
    )?;
    let mut stmt=tx.prepare("SELECT id,deps FROM swarm_jobs WHERE run_id=?1 AND status='ready'")?;
    let ready=stmt.query_map([&run],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(stmt);
    for (job,raw_deps) in ready {
        let deps: Vec<String>=serde_json::from_str(&raw_deps)?;
        if deps.iter().try_fold(false,|blocked,dep|
            Ok::<bool,anyhow::Error>(blocked || !super::artifacts::dep_satisfied(&tx,&run,dep)?))? {
            tx.execute("UPDATE swarm_jobs SET status='planned',updated_ms=?3
                WHERE run_id=?1 AND id=?2 AND status='ready'",params![run,job,now])?;
        }
    }
    // A dependent already admitted before the contradiction must stop as well.
    // Keep the stop intent and checkpoint envelope atomic with the conflict so
    // restart/replay cannot leave it working from evidence now in dispute.
    let mut stmt=tx.prepare(
        "WITH RECURSIVE downstream(job_id) AS (
             SELECT ?2 UNION SELECT ?3
             UNION
             SELECT j.id FROM swarm_jobs j
             JOIN json_each(j.deps) dep
             JOIN downstream d ON dep.value=d.job_id
             WHERE j.run_id=?1
         )
         SELECT a.id,a.job_id,a.revision FROM downstream d
         JOIN swarm_attempts a ON a.run_id=?1 AND a.job_id=d.job_id
         JOIN swarm_jobs j ON j.run_id=a.run_id AND j.id=a.job_id
         WHERE a.status='registered' AND a.job_id NOT IN (?2,?3)
           AND j.status IN ('reserved','launching','running','submitted','accepted')
           AND j.stop_reason IS NULL")?;
    let active=stmt.query_map(params![run,left_job,right_job],|r|
        Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,i64>(2)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(stmt);
    for (attempt,job,revision) in active {
        let changed=tx.execute(
            "UPDATE swarm_jobs SET status='cancel_requested',stop_reason='evidence_conflict',
             updated_ms=?3 WHERE run_id=?1 AND id=?2 AND stop_reason IS NULL",
            params![run,job,now])?;
        if changed>0 {
            tx.execute(
                "INSERT OR IGNORE INTO swarm_messages(run_id,message_id,job_id,attempt_id,
                 sender,recipient,kind,revision,payload,phase,created_ms,updated_ms)
                 VALUES(?1,?2,?3,?4,'control',?4,'checkpoint',?5,?6,'queued',?7,?7)",
                params![run,format!("conflict-checkpoint-{id}-{attempt}"),job,attempt,
                    revision,json!({"reason":"evidence_conflict","conflict_id":id}).to_string(),now])?;
        }
    }
    // A completed dependent may already have an accepted review or a submitted
    // result. Retain its historical decision and artifacts, but remove its
    // current authority: resolving the ancestor does not re-accept the child.
    let mut stmt=tx.prepare(
        "WITH RECURSIVE downstream(job_id) AS (
             SELECT ?2 UNION SELECT ?3
             UNION
             SELECT j.id FROM swarm_jobs j
             JOIN json_each(j.deps) dep
             JOIN downstream d ON dep.value=d.job_id
             WHERE j.run_id=?1
         )
         SELECT j.id FROM downstream d
         JOIN swarm_jobs j ON j.run_id=?1 AND j.id=d.job_id
         WHERE j.id NOT IN (?2,?3) AND j.status IN ('accepted','submitted')
           AND j.stop_reason IS NULL
           AND NOT EXISTS (SELECT 1 FROM swarm_attempts a
               WHERE a.run_id=j.run_id AND a.job_id=j.id AND a.status='registered')")?;
    let finished=stmt.query_map(params![run,left_job,right_job],|r|r.get::<_,String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(stmt);
    for job in finished {
        tx.execute("UPDATE swarm_jobs SET status='blocked',stop_reason='evidence_conflict',
            updated_ms=?3 WHERE run_id=?1 AND id=?2 AND stop_reason IS NULL",
            params![run,job,now])?;
    }
    tx.commit()?;
    Ok(json!({"conflict_id":id,"status":"open","duplicate":false}))
}

pub fn resolve(store: &mut Store, p: &Value) -> Result<Value> {
    let run=current_revision(store,p)?.to_string();
    let id=required(p,"conflict_id")?;
    let outcome=required(p,"outcome")?;
    if !["supports_left","supports_right","unresolved"].contains(&outcome) {
        bail!("invalid conflict resolution outcome");
    }
    let repro_job=p["reproduction_job_id"].as_str();
    let repro_artifact=p["reproduction_artifact_id"].as_str();
    if outcome=="unresolved" {
        if repro_job.is_some() || repro_artifact.is_some() {
            bail!("unresolved conflict cannot claim reproduction evidence");
        }
    } else if repro_job.is_none() || repro_artifact.is_none() {
        bail!("resolved conflict requires independent reproduction evidence");
    }
    let prior: Option<(String,String,String,String,Option<String>,Option<String>)>=store.conn.query_row(
        "SELECT left_job_id,right_job_id,status,COALESCE(outcome,''),
                reproduction_job_id,reproduction_artifact_id
         FROM swarm_conflicts WHERE run_id=?1 AND conflict_id=?2",
        params![run,id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?)),
    ).optional()?;
    let (left,right,status,old_outcome,old_job,old_artifact)=
        prior.ok_or_else(||anyhow!("unknown conflict"))?;
    if status!="open" {
        if old_outcome!=outcome || old_job.as_deref()!=repro_job
            || old_artifact.as_deref()!=repro_artifact {
            bail!("conflict resolution replay changes input");
        }
        return Ok(json!({"conflict_id":id,"status":status,"duplicate":true}));
    }
    if let (Some(job),Some(artifact))=(repro_job,repro_artifact) {
        if job==left || job==right {
            bail!("conflict requires independent reproduction job");
        }
        let (attempt,kind,state)=artifact_chain(store,&run,job,artifact)?;
        if kind!="reproduction" || state!="accepted" {
            bail!("conflict reproduction must be accepted evidence");
        }
        let accepted: Option<String>=store.conn.query_row(
            "SELECT evidence FROM swarm_decisions WHERE run_id=?1 AND job_id=?2
             AND attempt_id=?3 AND decision='accept' ORDER BY id DESC LIMIT 1",
            params![run,job,attempt],|r|r.get(0),
        ).optional()?;
        if !accepted.as_deref().and_then(|raw|serde_json::from_str::<Vec<String>>(raw).ok())
            .is_some_and(|ids|ids.iter().any(|a|a==artifact)) {
            bail!("conflict reproduction lacks accepted provenance");
        }
        let active: i64=store.conn.query_row(
            "SELECT COUNT(*) FROM swarm_attempts WHERE run_id=?1 AND job_id=?2 AND status='registered'",
            params![run,job],|r|r.get(0))?;
        if active>0 { bail!("conflict reproduction worker exit is unconfirmed"); }
    }
    // A prior accepted review of the contradicted side cannot become valid merely
    // because an independent reproduction supports the other side. Re-plan that
    // job so its old evidence cannot satisfy the current acceptance check.
    if outcome!="unresolved" {
        let contradicted = if outcome=="supports_left" { &right } else { &left };
        let status: String = store.conn.query_row(
            "SELECT status FROM swarm_jobs WHERE run_id=?1 AND id=?2",
            params![run,contradicted], |r| r.get(0))?;
        if status=="accepted" {
            bail!("accepted contradictory review requires plan revision before resolution");
        }
    }
    let new_status=if outcome=="unresolved" {"unresolved"} else {"resolved"};
    let now=crate::daemon::now();
    let tx=store.conn.transaction()?;
    tx.execute(
        "UPDATE swarm_conflicts SET status=?3,outcome=?4,reproduction_job_id=?5,
         reproduction_artifact_id=?6,updated_ms=?7 WHERE run_id=?1 AND conflict_id=?2 AND status='open'",
        params![run,id,new_status,outcome,repro_job,repro_artifact,now],
    )?;
    if new_status=="resolved" {
        super::materialize_ready(&tx,&run,now)?;
    }
    tx.commit()?;
    Ok(json!({"conflict_id":id,"status":new_status,"outcome":outcome,"duplicate":false}))
}

pub fn list(store: &Store, p: &Value) -> Result<Value> {
    let run=required(p,"run_id")?;
    let mut stmt=store.conn.prepare(
        "SELECT conflict_id,left_job_id,left_artifact_id,right_job_id,right_artifact_id,
                reason,status,outcome,reproduction_job_id,reproduction_artifact_id
         FROM swarm_conflicts WHERE run_id=?1 ORDER BY created_ms,conflict_id LIMIT 1001")?;
    let rows=stmt.query_map([run],|r|Ok(json!({"conflict_id":r.get::<_,String>(0)?,
        "left_job_id":r.get::<_,String>(1)?,"left_artifact_id":r.get::<_,String>(2)?,
        "right_job_id":r.get::<_,String>(3)?,"right_artifact_id":r.get::<_,String>(4)?,
        "reason":r.get::<_,String>(5)?,"status":r.get::<_,String>(6)?,
        "outcome":r.get::<_,Option<String>>(7)?,
        "reproduction_job_id":r.get::<_,Option<String>>(8)?,
        "reproduction_artifact_id":r.get::<_,Option<String>>(9)?})))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if rows.len()>1000 { bail!("conflict list exceeds bounded readout"); }
    Ok(json!({"run_id":run,"conflicts":rows}))
}

/// Keep the terminal coverage report available even when a long-running
/// category has more conflict history than the full conflict-list endpoint
/// can return in one response. The count makes a truncated preview explicit.
pub(super) fn unresolved_preview(store: &Store, run: &str) -> Result<(i64, Vec<Value>)> {
    let count: i64=store.conn.query_row(
        "SELECT COUNT(*) FROM swarm_conflicts WHERE run_id=?1 AND status!='resolved'",
        [run],|r|r.get(0))?;
    let mut stmt=store.conn.prepare(
        "SELECT conflict_id,left_job_id,left_artifact_id,right_job_id,right_artifact_id,
                reason,status,outcome,reproduction_job_id,reproduction_artifact_id
         FROM swarm_conflicts WHERE run_id=?1 AND status!='resolved'
         ORDER BY created_ms,conflict_id LIMIT 100")?;
    let rows=stmt.query_map([run],|r|Ok(json!({"conflict_id":r.get::<_,String>(0)?,
        "left_job_id":r.get::<_,String>(1)?,"left_artifact_id":r.get::<_,String>(2)?,
        "right_job_id":r.get::<_,String>(3)?,"right_artifact_id":r.get::<_,String>(4)?,
        "reason":r.get::<_,String>(5)?,"status":r.get::<_,String>(6)?,
        "outcome":r.get::<_,Option<String>>(7)?,
        "reproduction_job_id":r.get::<_,Option<String>>(8)?,
        "reproduction_artifact_id":r.get::<_,Option<String>>(9)?})))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok((count,rows))
}
