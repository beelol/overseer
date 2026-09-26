use super::{get, plan, record_planning_failure, required};
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
    resource_claims: Vec<plan::ResourceClaim>,
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
    let prior = get(store, id)?;
    if prior["generation"] != generation {
        bail!("stale director generation");
    }
    if prior["revision"] != expected {
        bail!("stale plan revision");
    }
    if !["planning", "running", "paused"].contains(&prior["status"].as_str().unwrap_or("")) {
        bail!("swarm run cannot be revised in this state");
    }
    let parsed: Result<Vec<plan::JobSpec>> = (|| {
        let jobs: Vec<plan::JobSpec> =
            serde_json::from_value(p["jobs"].clone()).map_err(|e| anyhow!("invalid jobs: {e}"))?;
        plan::validate(&jobs)?;
        Ok(jobs)
    })();
    let jobs = match parsed {
        Ok(jobs) => jobs,
        Err(error) => {
            record_planning_failure(store, id)?;
            return Err(error);
        }
    };
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
        "SELECT id,title,acceptance,deps,resource_claims,status,attempt_count FROM swarm_jobs WHERE run_id=?1",
    )?;
    let rows = stmt
        .query_map(params![id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, i64>(6)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(stmt);
    let mut old = HashMap::new();
    for (job_id, title, acceptance, raw_deps, raw_claims, status, attempts) in rows {
        old.insert(
            job_id,
            OldJob {
                title,
                acceptance,
                deps: serde_json::from_str(&raw_deps)?,
                resource_claims: serde_json::from_str(&raw_claims)?,
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
                o.title != j.title
                    || o.acceptance != j.acceptance
                    || o.deps != j.deps
                    || o.resource_claims != j.resource_claims
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
    if affected.is_empty() && new_ids.len() == old.len() {
        return Ok(json!({"id":id,"generation":generation,"revision":expected,
            "affected":0,"redirected":0,"unchanged":true}));
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
            tx.execute("INSERT INTO swarm_jobs(run_id,id,plan_revision,title,acceptance,deps,resource_claims,status,created_ms,updated_ms) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?9)",
                params![id,job.id,revision,job.title,job.acceptance,serde_json::to_string(&job.deps)?,serde_json::to_string(&job.resource_claims)?,state,now])?;
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
        let unsafe_effects: i64 = tx.query_row(
            "SELECT COUNT(*) FROM swarm_effects WHERE run_id=?1 AND job_id=?2 AND outcome IN ('unknown','applied')",
            params![id,job.id], |r| r.get(0),
        )?;
        let state = if !live.is_empty() {
            "cancel_requested"
        } else if unsafe_effects > 0 {
            "blocked"
        } else if previous.attempts >= 2 {
            "failed"
        } else if deps_satisfied {
            "ready"
        } else {
            "planned"
        };
        tx.execute("UPDATE swarm_jobs SET plan_revision=?3,title=?4,acceptance=?5,deps=?6,resource_claims=?7,status=?8,
            deadline_at_ms=CASE WHEN ?10=1 THEN NULL ELSE deadline_at_ms END,
            stop_reason=CASE WHEN ?10=1 THEN NULL ELSE stop_reason END,updated_ms=?9
            WHERE run_id=?1 AND id=?2",
            params![id,job.id,revision,job.title,job.acceptance,serde_json::to_string(&job.deps)?,serde_json::to_string(&job.resource_claims)?,state,now,i64::from(live.is_empty())])?;
        if live.is_empty() && unsafe_effects == 0 {
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
        "UPDATE swarm_runs SET revision=?2,failed_planning_turns=0,no_progress_turns=0,updated_ms=?3 WHERE id=?1",
        params![id, revision, now],
    )?;
    tx.commit()?;
    Ok(
        json!({"id":id,"generation":generation,"revision":revision,"affected":affected.len(),"redirected":redirected}),
    )
}
