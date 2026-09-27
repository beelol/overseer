use super::{get, plan, record_planning_failure, record_planning_failure_request, required};
use crate::store::Store;
use anyhow::{anyhow, bail, Result};
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};

#[derive(Clone)]
struct OldJob {
    title: String,
    acceptance: String,
    deps: Vec<String>,
    resource_claims: Vec<plan::ResourceClaim>,
    required_capabilities: Vec<String>,
    attempts: i64,
    status: String,
    stop_reason: Option<String>,
}

fn deps_satisfied(
    conn: &rusqlite::Connection,
    run: &str,
    deps: &[String],
    affected: &HashSet<String>,
) -> Result<bool> {
    for dep in deps {
        if affected.contains(dep) || !super::artifacts::dep_satisfied(conn, run, dep)? {
            return Ok(false);
        }
    }
    Ok(true)
}

pub fn revise(store: &mut Store, p: &Value) -> Result<Value> {
    let id = required(p, "id")?;
    super::owner::require(store,id,p)?;
    let request_id = p.get("request_id").map(|v| v.as_str()
        .ok_or_else(|| anyhow!("invalid revision request id"))).transpose()?;
    if request_id.is_some_and(|key| key.is_empty() || key.len() > 128
        || key.chars().any(char::is_control) || crate::redact::redact(key) != key) {
        bail!("invalid revision request id");
    }
    let mut effect = p.clone();
    if let Value::Object(fields) = &mut effect {
        fields.remove("request_id");
        fields.remove("owner_token");
    }
    let request_sha256 = format!("{:x}",Sha256::digest(effect.to_string().as_bytes()));
    if let Some(request_id) = request_id {
        let prior: Option<(String,String)> = store.conn.query_row(
            "SELECT request_sha256,result_json FROM swarm_revision_requests
             WHERE run_id=?1 AND request_id=?2",
            params![id,request_id],|row| Ok((row.get(0)?,row.get(1)?)),
        ).optional()?;
        if let Some((prior_sha256,result_json)) = prior {
            if prior_sha256 != request_sha256 {
                bail!("revision request id reused with different input");
            }
            let mut result: Value = serde_json::from_str(&result_json)?;
            if let Some(error) = result["error"].as_str() {
                bail!("{error}");
            }
            result["duplicate"] = json!(true);
            return Ok(result);
        }
    }
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
        plan::validate(&jobs, 10_000)?;
        Ok(jobs)
    })();
    let jobs = match parsed {
        Ok(jobs) => jobs,
        Err(error) => {
            if let Some(request_id) = request_id {
                let safe_error: String = crate::redact::redact(&error.to_string())
                    .chars().take(256).collect();
                record_planning_failure_request(store, id,
                    Some((request_id, request_sha256.as_str(), &safe_error)))?;
                bail!("{safe_error}");
            }
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
        "SELECT id,title,acceptance,deps,resource_claims,required_capabilities,attempt_count,status,stop_reason FROM swarm_jobs WHERE run_id=?1",
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
                r.get::<_, String>(7)?,
                r.get::<_, Option<String>>(8)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(stmt);
    let mut old = HashMap::new();
    for (job_id, title, acceptance, raw_deps, raw_claims, raw_capabilities, attempts, status, stop_reason) in rows {
        old.insert(
            job_id,
            OldJob {
                title,
                acceptance,
                deps: serde_json::from_str(&raw_deps)?,
                resource_claims: serde_json::from_str(&raw_claims)?,
                required_capabilities: serde_json::from_str(&raw_capabilities)?,
                attempts,
                status,
                stop_reason,
            },
        );
    }
    let new_ids: HashSet<&str> = jobs.iter().map(|j| j.id.as_str()).collect();
    if jobs.iter().any(|job| old.get(&job.id).is_some_and(|previous|
        previous.status == "superseded"
        || (previous.status == "cancel_requested" && previous.stop_reason.as_deref() == Some("scope_narrowed")))) {
        drop(tx);
        let error = "superseded job id cannot be reused in a later plan";
        record_planning_failure_request(store, id,
            request_id.map(|request_id| (request_id, request_sha256.as_str(), error)))?;
        bail!("{error}");
    }
    let mut omitted: Vec<String> = old.iter().filter_map(|(job, previous)| {
        (!new_ids.contains(job.as_str()) && previous.status != "superseded"
            && !(previous.status == "cancel_requested"
                && previous.stop_reason.as_deref() == Some("scope_narrowed")))
            .then_some(job.clone())
    }).collect();
    omitted.sort();
    let mut affected: HashSet<String> = jobs
        .iter()
        .filter(|j| {
            old.get(&j.id).is_some_and(|o| {
                o.title != j.title
                    || o.acceptance != j.acceptance
                    || o.deps != j.deps
                    || o.resource_claims != j.resource_claims
                    || o.required_capabilities != j.required_capabilities
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
    if affected.is_empty() && omitted.is_empty() && jobs.iter().all(|job| old.contains_key(&job.id)) {
        let mut result = json!({"id":id,"generation":generation,"revision":expected,
            "affected":0,"redirected":0,"unchanged":true});
        if let Some(request_id) = request_id {
            result["duplicate"] = json!(false);
            tx.execute("INSERT INTO swarm_revision_requests(run_id,request_id,request_sha256,
                result_json,created_ms) VALUES(?1,?2,?3,?4,?5)",
                params![id,request_id,request_sha256,result.to_string(),crate::daemon::now()])?;
            tx.commit()?;
        }
        return Ok(result);
    }
    let now = crate::daemon::now();
    let revision = expected + 1;
    let safe_reason = crate::redact::redact(reason);
    let mut redirected = 0;
    for job in &omitted {
        let integrated: bool = tx.prepare("SELECT 1 FROM swarm_integrated_artifacts
            WHERE run_id=?1 AND job_id=?2")?.exists(params![id,job])?;
        let integrating: bool = tx.prepare("SELECT 1 FROM swarm_integration_intents
            WHERE run_id=?1 AND job_id=?2")?.exists(params![id,job])?;
        if integrated || integrating {
            bail!("integrated or in-flight patch cannot be silently excluded from scope");
        }
        let unsafe_effects: bool = tx.prepare("SELECT 1 FROM swarm_effects
            WHERE run_id=?1 AND job_id=?2 AND outcome IN ('unknown','applied')")?
            .exists(params![id,job])?;
        if unsafe_effects {
            bail!("unreconciled side effect blocks scope narrowing");
        }
        let mut stmt = tx.prepare("SELECT id,revision FROM swarm_attempts
            WHERE run_id=?1 AND job_id=?2 AND status='registered'")?;
        let live = stmt.query_map(params![id,job], |row| Ok((row.get::<_,String>(0)?,row.get::<_,i64>(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        drop(stmt);
        let status = if live.is_empty() { "superseded" } else { "cancel_requested" };
        tx.execute("UPDATE swarm_jobs SET plan_revision=?3,status=?4,
            stop_reason='scope_narrowed',updated_ms=?5 WHERE run_id=?1 AND id=?2",
            params![id,job,revision,status,now])?;
        if live.is_empty() {
            tx.execute("UPDATE swarm_claims SET status='released',updated_ms=?3
                WHERE run_id=?1 AND job_id=?2 AND status='active'",params![id,job,now])?;
        }
        for (attempt,attempt_revision) in live {
            tx.execute("INSERT OR IGNORE INTO swarm_messages
                (run_id,message_id,job_id,attempt_id,sender,recipient,kind,revision,payload,phase,created_ms,updated_ms)
                VALUES(?1,?2,?3,?4,'control',?4,'stop',?5,?6,'queued',?7,?7)",
                params![id,format!("scope-stop-{revision}-{attempt}"),job,attempt,attempt_revision,
                    json!({"reason":"scope_narrowed","new_revision":revision}).to_string(),now])?;
        }
    }
    for job in &jobs {
        let Some(previous) = old.get(&job.id) else {
            let ready = deps_satisfied(&tx, id, &job.deps, &affected)?;
            let state = if ready { "ready" } else { "planned" };
            tx.execute("INSERT INTO swarm_jobs(run_id,id,plan_revision,title,acceptance,deps,resource_claims,required_capabilities,status,created_ms,updated_ms) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?10)",
                params![id,job.id,revision,job.title,job.acceptance,serde_json::to_string(&job.deps)?,serde_json::to_string(&job.resource_claims)?,serde_json::to_string(&job.required_capabilities)?,state,now])?;
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
        let deps_satisfied = deps_satisfied(&tx, id, &job.deps, &affected)?;
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
        tx.execute("UPDATE swarm_jobs SET plan_revision=?3,title=?4,acceptance=?5,deps=?6,resource_claims=?7,required_capabilities=?8,status=?9,
            deadline_at_ms=CASE WHEN ?11=1 THEN NULL ELSE deadline_at_ms END,
            stop_reason=CASE WHEN ?11=1 THEN NULL ELSE stop_reason END,updated_ms=?10
            WHERE run_id=?1 AND id=?2",
            params![id,job.id,revision,job.title,job.acceptance,serde_json::to_string(&job.deps)?,serde_json::to_string(&job.resource_claims)?,serde_json::to_string(&job.required_capabilities)?,state,now,i64::from(live.is_empty())])?;
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
    let backlog_limit = prior["policy"]["effective"]["backlog_max"]
        .as_i64().unwrap_or(1000).min(10_000);
    let nonterminal: i64 = tx.query_row(
        "SELECT COUNT(*) FROM swarm_jobs WHERE run_id=?1 AND status NOT IN ('accepted','failed','cancelled','superseded')",
        [id], |r| r.get(0),
    )?;
    if nonterminal > backlog_limit {
        drop(tx);
        record_planning_failure(store, id)?;
        bail!("job backlog exceeds configured backlog limit");
    }
    tx.execute(
        "UPDATE swarm_runs SET revision=?2,failed_planning_turns=0,no_progress_turns=0,updated_ms=?3 WHERE id=?1",
        params![id, revision, now],
    )?;
    super::materialize_ready(&tx, id, now)?;
    let mut result = json!({"id":id,"generation":generation,"revision":revision,
        "affected":affected.len(),"superseded":omitted.len(),"redirected":redirected});
    if let Some(request_id) = request_id {
        result["duplicate"] = json!(false);
        tx.execute("INSERT INTO swarm_revision_requests(run_id,request_id,request_sha256,
            result_json,created_ms) VALUES(?1,?2,?3,?4,?5)",
            params![id,request_id,request_sha256,result.to_string(),now])?;
    }
    tx.commit()?;
    Ok(result)
}
