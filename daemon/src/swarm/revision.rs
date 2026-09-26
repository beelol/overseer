use super::{plan, required};
use crate::store::Store;
use anyhow::{anyhow, bail, Result};
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

#[derive(Clone)]
struct OldJob {
    title: String,
    acceptance: String,
    deps: Vec<String>,
    status: String,
    attempts: i64,
}

pub fn revise(store: &mut Store, p: &Value) -> Result<Value> {
    let id = required(p, "id")?;
    let generation = p["generation"]
        .as_i64()
        .ok_or_else(|| anyhow!("missing generation"))?;
    let expected = p["expected_revision"]
        .as_i64()
        .ok_or_else(|| anyhow!("missing expected revision"))?;
    let reason = required(p, "reason")?.trim();
    if reason.is_empty() || reason.len() > 2000 {
        bail!("revision requires a bounded reason");
    }
    let jobs: Vec<plan::JobSpec> =
        serde_json::from_value(p["jobs"].clone()).map_err(|e| anyhow!("invalid jobs: {e}"))?;
    plan::validate(&jobs)?;
    let tx = store.conn.transaction()?;
    let current: (i64, i64, String) = tx
        .query_row(
            "SELECT generation,revision,status FROM swarm_runs WHERE id=?1",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?
        .ok_or_else(|| anyhow!("unknown swarm run"))?;
    if generation != current.0 {
        bail!("stale director generation");
    }
    if expected != current.1 {
        bail!("stale plan revision");
    }
    if !["planning", "running", "paused"].contains(&current.2.as_str()) {
        bail!("swarm run cannot be revised in this state");
    }
    let mut stmt = tx.prepare(
        "SELECT id,title,acceptance,deps,status,attempt_count FROM swarm_jobs WHERE run_id=?1",
    )?;
    let rows = stmt
        .query_map(params![id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, i64>(5)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(stmt);
    let mut old = HashMap::new();
    for (job_id, title, acceptance, raw_deps, status, attempts) in rows {
        old.insert(
            job_id,
            OldJob {
                title,
                acceptance,
                deps: serde_json::from_str(&raw_deps)?,
                status,
                attempts,
            },
        );
    }
    let new_ids: HashSet<&str> = jobs.iter().map(|j| j.id.as_str()).collect();
    if old.keys().any(|k| !new_ids.contains(k.as_str())) {
        bail!("revision must retain existing job ids; removal needs a separate cancellation transition");
    }
    let mut affected: HashSet<String> = jobs
        .iter()
        .filter(|j| {
            old.get(&j.id).is_some_and(|o| {
                o.title != j.title || o.acceptance != j.acceptance || o.deps != j.deps
            })
        })
        .map(|j| j.id.clone())
        .collect();
    loop {
        let before = affected.len();
        for job in &jobs {
            if old.contains_key(&job.id) && job.deps.iter().any(|d| affected.contains(d)) {
                affected.insert(job.id.clone());
            }
        }
        if affected.len() == before {
            break;
        }
    }
    let now = crate::daemon::now();
    let revision = expected + 1;
    let safe_reason = crate::redact::redact(reason);
    let mut redirected = 0;
    for job in &jobs {
        let Some(previous) = old.get(&job.id) else {
            let ready = job.deps.iter().all(|dep| {
                old.get(dep)
                    .is_some_and(|o| o.status == "accepted" && !affected.contains(dep))
            });
            let state = if ready { "ready" } else { "planned" };
            tx.execute("INSERT INTO swarm_jobs(run_id,id,plan_revision,title,acceptance,deps,status,created_ms,updated_ms) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?8)",
                params![id,job.id,revision,job.title,job.acceptance,serde_json::to_string(&job.deps)?,state,now])?;
            continue;
        };
        if !affected.contains(&job.id) {
            continue;
        }
        let mut stmt = tx.prepare(
            "SELECT id FROM swarm_attempts WHERE run_id=?1 AND job_id=?2 AND status='registered'",
        )?;
        let live = stmt
            .query_map(params![id, job.id], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        drop(stmt);
        let deps_satisfied = job.deps.iter().all(|dep| {
            old.get(dep)
                .is_some_and(|o| o.status == "accepted" && !affected.contains(dep))
        });
        let state = if !live.is_empty() {
            "cancel_requested"
        } else if previous.attempts >= 2 {
            "failed"
        } else if deps_satisfied {
            "ready"
        } else {
            "planned"
        };
        tx.execute("UPDATE swarm_jobs SET plan_revision=?3,title=?4,acceptance=?5,deps=?6,status=?7,updated_ms=?8 WHERE run_id=?1 AND id=?2",
            params![id,job.id,revision,job.title,job.acceptance,serde_json::to_string(&job.deps)?,state,now])?;
        if live.is_empty() {
            tx.execute("UPDATE swarm_claims SET status='released',updated_ms=?3 WHERE run_id=?1 AND job_id=?2 AND status='active'",params![id,job.id,now])?;
        }
        for attempt in live {
            let message_id = format!("revision-{revision}-{attempt}");
            let payload = json!({"reason":safe_reason,"new_revision":revision,"action":"checkpoint-and-stop"}).to_string();
            tx.execute("INSERT INTO swarm_messages(run_id,message_id,job_id,attempt_id,sender,recipient,kind,revision,payload,phase,created_ms,updated_ms) VALUES(?1,?2,?3,?4,'director',?4,'redirect',?5,?6,'queued',?7,?7)",
                params![id,message_id,job.id,attempt,revision,payload,now])?;
            redirected += 1;
        }
    }
    tx.execute(
        "UPDATE swarm_runs SET revision=?2,updated_ms=?3 WHERE id=?1",
        params![id, revision, now],
    )?;
    tx.commit()?;
    Ok(
        json!({"id":id,"generation":generation,"revision":revision,"affected":affected.len(),"redirected":redirected}),
    )
}
