//! Findings and independent reproduction (SWARM-45).
//!
//! A finding is the director's record of one defect: a title, the discovery it rests on, and
//! evidence entries, each a submitted artifact of one job about one endpoint. Recording again
//! only adds evidence; merging a duplicate finding moves its evidence into the one kept, with
//! every endpoint's entries intact, and leaves the duplicate as `merged` for provenance.
//!
//! Agreement is not proof. An endpoint is confirmed only by accepted reproduction evidence
//! (an artifact of kind `reproduction` in an accepted review), and once the director has
//! created an independent reproducer for the finding, only that reproducer's accepted
//! reproduction confirms it. How many workers agree is shown, never counted.
//!
//! A reproducer is an explicit job the director creates for one finding, as a plan revision,
//! with its own budget in one unit. It may not depend on, or write where, the work it
//! reproduces does. One finding has at most one reproducer, and one discovery message can be
//! the source of at most one: repeated or duplicate discoveries return the reproducer that
//! exists. Its admissions draw on the run's allocation like any job and are also bounded by
//! its own budget.

use super::{get, plan, required};
use crate::store::Store;
use anyhow::{anyhow, bail, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};

fn bounded_id(value: &str, what: &str) -> Result<()> {
    if value.is_empty() || value.len() > 100
        || !value.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b)) {
        bail!("invalid {what}");
    }
    Ok(())
}

fn bounded_text(value: &str, max: usize, what: &str) -> Result<()> {
    if value.trim().is_empty() || value.len() > max || value.chars().any(char::is_control)
        || crate::redact::redact(value) != value {
        bail!("invalid {what}");
    }
    Ok(())
}

/// The director's current generation and revision, in a state that may change findings.
fn current(store: &Store, p: &Value) -> Result<(String, i64)> {
    let run = required(p, "run_id")?;
    super::owner::require(store, run, p)?;
    let now = get(store, run)?;
    if now["generation"] != p["generation"] || now["revision"] != p["revision"] {
        bail!("stale director generation or plan revision");
    }
    if !["planning", "running", "paused"].contains(&now["status"].as_str().unwrap_or("")) {
        bail!("run cannot change findings in this state");
    }
    Ok((run.to_string(), now["revision"].as_i64().unwrap_or(0)))
}

/// A submitted artifact of this job: stored intact and named by one of its attempt's results.
fn submitted(conn: &Connection, run: &str, job: &str, artifact: &str) -> Result<()> {
    let found: Option<(String, String, String)> = conn.query_row(
        "SELECT attempt_id,content,sha256 FROM swarm_artifacts WHERE run_id=?1 AND job_id=?2 AND id=?3",
        params![run, job, artifact], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).optional()?;
    let (attempt, content, digest) = found.ok_or_else(|| anyhow!("finding evidence {job}/{artifact} is not an artifact of that job"))?;
    if format!("{:x}", Sha256::digest(content.as_bytes())) != digest {
        bail!("finding evidence {job}/{artifact} changed after submission");
    }
    let mut stmt = conn.prepare("SELECT payload FROM swarm_messages WHERE run_id=?1 AND job_id=?2 AND attempt_id=?3
        AND sender=?3 AND kind IN ('result','submit')")?;
    let payloads = stmt.query_map(params![run, job, attempt], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if !payloads.iter().any(|raw| serde_json::from_str::<Value>(raw).ok()
        .is_some_and(|v| v["artifact_ids"].as_array().is_some_and(|ids| ids.iter().any(|a| a == artifact)))) {
        bail!("finding evidence {job}/{artifact} was never submitted in a result");
    }
    Ok(())
}

fn merged_into(conn: &Connection, run: &str, finding: &str) -> Result<Option<Option<String>>> {
    Ok(conn.query_row("SELECT merged_into FROM swarm_findings WHERE run_id=?1 AND finding_id=?2",
        params![run, finding], |r| r.get::<_, Option<String>>(0)).optional()?)
}

pub fn record(store: &mut Store, p: &Value) -> Result<Value> {
    let (run, revision) = current(store, p)?;
    let finding = required(p, "finding_id")?;
    bounded_id(finding, "finding id")?;
    let title = required(p, "title")?;
    bounded_text(title, 200, "finding title")?;
    let root = p["root_cause"].as_str();
    if let Some(root) = root {
        bounded_text(root, 128, "finding root cause")?;
    }
    let evidence = p["evidence"].as_array().ok_or_else(|| anyhow!("a finding needs evidence"))?;
    if evidence.is_empty() || evidence.len() > 100 {
        bail!("a finding needs 1-100 evidence entries");
    }
    let mut entries = Vec::new();
    for e in evidence {
        let (job, artifact, endpoint) = (required(e, "job_id")?, required(e, "artifact_id")?, required(e, "endpoint")?);
        bounded_text(endpoint, 200, "finding endpoint")?;
        submitted(&store.conn, &run, job, artifact)?;
        entries.push((job.to_string(), artifact.to_string(), endpoint.trim().to_string()));
    }
    let tx = store.conn.transaction()?;
    match merged_into(&tx, &run, finding)? {
        Some(Some(into)) => bail!("finding {finding} was merged into {into}; record its evidence there"),
        Some(None) => {
            let old: String = tx.query_row("SELECT title FROM swarm_findings WHERE run_id=?1 AND finding_id=?2",
                params![run, finding], |r| r.get(0))?;
            if old != title {
                bail!("finding id reused with a different title");
            }
        }
        None => {
            tx.execute("INSERT INTO swarm_findings(run_id,finding_id,title,root_cause,created_revision,created_ms)
                VALUES(?1,?2,?3,?4,?5,?6)", params![run, finding, title, root, revision, crate::daemon::now()])?;
        }
    }
    for (job, artifact, endpoint) in &entries {
        tx.execute("INSERT OR IGNORE INTO swarm_finding_evidence(run_id,finding_id,job_id,artifact_id,endpoint,created_ms)
            VALUES(?1,?2,?3,?4,?5,?6)", params![run, finding, job, artifact, endpoint, crate::daemon::now()])?;
    }
    tx.commit()?;
    readout(&store.conn, &run, finding)
}

pub fn merge(store: &mut Store, p: &Value) -> Result<Value> {
    let (run, _) = current(store, p)?;
    let into = required(p, "into")?;
    let from: Vec<String> = p["from"].as_array().ok_or_else(|| anyhow!("merge needs the findings to merge (from)"))?
        .iter().map(|v| v.as_str().map(str::to_string).ok_or_else(|| anyhow!("invalid finding id"))).collect::<Result<_>>()?;
    if from.is_empty() || from.len() > 50 {
        bail!("merge needs 1-50 findings");
    }
    let tx = store.conn.transaction()?;
    if merged_into(&tx, &run, into)? != Some(None) {
        bail!("finding {into} is unknown or already merged");
    }
    let reproducer = |f: &str| -> Result<Option<String>> {
        Ok(tx.query_row("SELECT job_id FROM swarm_reproductions WHERE run_id=?1 AND finding_id=?2",
            params![run, f], |r| r.get(0)).optional()?)
    };
    let mut kept = reproducer(into)?;
    for f in &from {
        if f == into {
            bail!("a finding cannot be merged into itself");
        }
        match merged_into(&tx, &run, f)? {
            None => bail!("unknown finding {f}"),
            Some(Some(other)) if other == into => continue,
            Some(Some(other)) => bail!("finding {f} was already merged into {other}"),
            Some(None) => {}
        }
        if let Some(job) = reproducer(f)? {
            if kept.is_some() {
                bail!("both findings have an independent reproducer; keep one before merging");
            }
            tx.execute("UPDATE swarm_reproductions SET finding_id=?3 WHERE run_id=?1 AND finding_id=?2", params![run, f, into])?;
            tx.execute("UPDATE swarm_reproduction_sources SET finding_id=?3 WHERE run_id=?1 AND finding_id=?2", params![run, f, into])?;
            kept = Some(job);
        }
        tx.execute("INSERT OR IGNORE INTO swarm_finding_evidence(run_id,finding_id,job_id,artifact_id,endpoint,created_ms)
            SELECT run_id,?3,job_id,artifact_id,endpoint,created_ms FROM swarm_finding_evidence
            WHERE run_id=?1 AND finding_id=?2", params![run, f, into])?;
        tx.execute("UPDATE swarm_findings SET merged_into=?3 WHERE run_id=?1 AND finding_id=?2", params![run, f, into])?;
    }
    tx.commit()?;
    readout(&store.conn, &run, into)
}

/// Every job that depends, directly or not, on any of `roots`, and the roots themselves.
fn downstream(conn: &Connection, run: &str, roots: &HashSet<String>) -> Result<HashSet<String>> {
    let mut stmt = conn.prepare("SELECT id,deps FROM swarm_jobs WHERE run_id=?1")?;
    let jobs = stmt.query_map([run], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut out = roots.clone();
    loop {
        let before = out.len();
        for (id, raw) in &jobs {
            let deps: Vec<String> = serde_json::from_str(raw).unwrap_or_default();
            if deps.iter().any(|d| out.contains(d)) {
                out.insert(id.clone());
            }
        }
        if out.len() == before {
            return Ok(out);
        }
    }
}

pub fn reproduce(store: &mut Store, p: &Value) -> Result<Value> {
    let (run, revision) = current(store, p)?;
    let finding = required(p, "finding_id")?;
    let source = required(p, "source_message_id")?;
    // One reproducer per finding, and a discovery reproduced once: repeats and duplicate
    // discoveries get the reproducer that exists.
    let existing: Option<(String, String)> = store.conn.query_row(
        "SELECT r.job_id,r.finding_id FROM swarm_reproductions r WHERE r.run_id=?1 AND (r.finding_id=?2
           OR r.finding_id=(SELECT finding_id FROM swarm_reproduction_sources WHERE run_id=?1 AND message_id=?3))
         ORDER BY r.finding_id=?2 DESC LIMIT 1",
        params![run, finding, source], |r| Ok((r.get(0)?, r.get(1)?))).optional()?;
    let source_ok: bool = store.conn.prepare(
        "SELECT 1 FROM swarm_messages WHERE run_id=?1 AND message_id=?2 AND recipient='director'
         AND kind IN ('discovery','result','submit')")?.exists(params![run, source])?;
    if !source_ok {
        bail!("a reproducer needs a discovery or result in the director's inbox as its source");
    }
    if let Some((job, of)) = existing {
        store.conn.execute("INSERT OR IGNORE INTO swarm_reproduction_sources(run_id,message_id,finding_id,created_ms)
            VALUES(?1,?2,?3,?4)", params![run, source, of, crate::daemon::now()])?;
        return Ok(json!({"run_id":run,"finding_id":of,"job_id":job,"duplicate":true,"revision":revision}));
    }
    match merged_into(&store.conn, &run, finding)? {
        None => bail!("record the finding before creating its reproducer"),
        Some(Some(into)) => bail!("finding {finding} was merged into {into}"),
        Some(None) => {}
    }
    let mut spec: plan::JobSpec = serde_json::from_value(p["job"].clone()).map_err(|e| anyhow!("invalid reproducer job: {e}"))?;
    let deps = std::mem::take(&mut spec.deps);
    plan::validate(std::slice::from_ref(&spec), 1)?;
    spec.deps = deps;
    if spec.budget_role != "worker" {
        bail!("a reproducer is a worker job");
    }
    let budget = p["budget_milli"].as_object().ok_or_else(|| anyhow!("a reproducer needs its own budget"))?;
    if budget.len() != 1 {
        bail!("a reproducer's budget is in exactly one unit");
    }
    let (unit, amount) = budget.iter().next().unwrap();
    let amount = amount.as_i64().filter(|a| (1..=1_000_000_000_000).contains(a))
        .ok_or_else(|| anyhow!("a reproducer's budget must be a positive bounded amount"))?;
    bounded_id(unit, "budget unit")?;
    // Independence: not built on the reproduced work, and not writing where it writes.
    let mut stmt = store.conn.prepare("SELECT DISTINCT job_id FROM swarm_finding_evidence WHERE run_id=?1 AND finding_id=?2")?;
    let reproduced: HashSet<String> = stmt.query_map(params![run, finding], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    drop(stmt);
    let tainted = downstream(&store.conn, &run, &reproduced)?;
    for dep in &spec.deps {
        let known: bool = store.conn.prepare("SELECT 1 FROM swarm_jobs WHERE run_id=?1 AND id=?2")?.exists(params![run, dep])?;
        if !known {
            bail!("unknown dependency {dep}");
        }
        if tainted.contains(dep) {
            bail!("a reproducer must be independent of the work it reproduces (it depends on {dep})");
        }
    }
    for job in &reproduced {
        let raw: String = store.conn.query_row("SELECT resource_claims FROM swarm_jobs WHERE run_id=?1 AND id=?2",
            params![run, job], |r| r.get(0))?;
        let theirs: Vec<plan::ResourceClaim> = serde_json::from_str(&raw)?;
        for mine in &spec.resource_claims {
            if theirs.iter().any(|t| t.resource == mine.resource && (t.mode == "write" || mine.mode == "write")) {
                bail!("a reproducer must be independent of the work it reproduces (it shares {} with {job})", mine.resource);
            }
        }
    }
    let taken: bool = store.conn.prepare("SELECT 1 FROM swarm_jobs WHERE run_id=?1 AND id=?2")?.exists(params![run, spec.id])?;
    if taken {
        bail!("job id {} already exists", spec.id);
    }
    let now = crate::daemon::now();
    let next = revision + 1;
    let tx = store.conn.transaction()?;
    let mut ready = true;
    for dep in &spec.deps {
        ready &= super::artifacts::dep_satisfied(&tx, &run, dep)?;
    }
    tx.execute("INSERT INTO swarm_jobs(run_id,id,plan_revision,title,acceptance,deps,resource_claims,required_capabilities,budget_role,status,created_ms,updated_ms)
        VALUES(?1,?2,?3,?4,?5,?6,?7,?8,'worker',?9,?10,?10)",
        params![run, spec.id, next, spec.title, spec.acceptance, serde_json::to_string(&spec.deps)?,
            serde_json::to_string(&spec.resource_claims)?, serde_json::to_string(&spec.required_capabilities)?,
            if ready { "ready" } else { "planned" }, now])?;
    tx.execute("UPDATE swarm_runs SET revision=?2,failed_planning_turns=0,no_progress_turns=0,updated_ms=?3 WHERE id=?1",
        params![run, next, now])?;
    tx.execute("INSERT INTO swarm_reproductions(run_id,finding_id,job_id,budget_unit,budget_milli,created_revision,created_ms)
        VALUES(?1,?2,?3,?4,?5,?6,?7)", params![run, finding, spec.id, unit, amount, next, now])?;
    tx.execute("INSERT OR IGNORE INTO swarm_reproduction_sources(run_id,message_id,finding_id,created_ms) VALUES(?1,?2,?3,?4)",
        params![run, source, finding, now])?;
    super::materialize_ready(&tx, &run, now)?;
    tx.commit()?;
    Ok(json!({"run_id":run,"finding_id":finding,"job_id":spec.id,"duplicate":false,"revision":next,
        "budget_milli":{unit.as_str(): amount}}))
}

/// Admission's budget check for a reproducer job: `None` when the job is no reproducer or the
/// estimate fits what is left of its own budget.
pub(super) fn over_budget(conn: &Connection, run: &str, job: &str, p: &Value) -> Result<Option<&'static str>> {
    let budget: Option<(String, i64)> = conn.query_row(
        "SELECT budget_unit,budget_milli FROM swarm_reproductions WHERE run_id=?1 AND job_id=?2",
        params![run, job], |r| Ok((r.get(0)?, r.get(1)?))).optional()?;
    let Some((unit, amount)) = budget else { return Ok(None) };
    let Some(estimate) = p["estimate_milli"][unit.as_str()].as_i64() else { return Ok(Some("reproduction_budget_unknown")) };
    let spent: i64 = conn.query_row("SELECT COALESCE(SUM(amount_milli),0) FROM swarm_reproduction_charges WHERE run_id=?1 AND job_id=?2",
        params![run, job], |r| r.get(0))?;
    Ok((estimate < 0 || spent.saturating_add(estimate) > amount).then_some("reproduction_budget"))
}

/// Charge an admitted reproducer attempt's estimate to its own budget.
pub(super) fn charge(conn: &Connection, attempt: &str, run: &str, job: &str, p: &Value) -> Result<()> {
    let unit: Option<String> = conn.query_row("SELECT budget_unit FROM swarm_reproductions WHERE run_id=?1 AND job_id=?2",
        params![run, job], |r| r.get(0)).optional()?;
    if let Some(unit) = unit {
        let amount = p["estimate_milli"][unit.as_str()].as_i64().unwrap_or(0);
        conn.execute("INSERT OR IGNORE INTO swarm_reproduction_charges(attempt_id,run_id,job_id,unit,amount_milli,created_ms)
            VALUES(?1,?2,?3,?4,?5,?6)", params![attempt, run, job, unit, amount, crate::daemon::now()])?;
    }
    Ok(())
}

/// Whether this evidence entry is accepted reproduction evidence.
fn confirming(conn: &Connection, run: &str, job: &str, artifact: &str) -> Result<bool> {
    let row: Option<(String, String, String)> = conn.query_row(
        "SELECT a.kind,a.attempt_id,j.status FROM swarm_artifacts a JOIN swarm_jobs j ON j.run_id=a.run_id AND j.id=a.job_id
         WHERE a.run_id=?1 AND a.job_id=?2 AND a.id=?3", params![run, job, artifact], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).optional()?;
    let Some((kind, attempt, status)) = row else { return Ok(false) };
    if kind != "reproduction" || status != "accepted" {
        return Ok(false);
    }
    let accepted: Option<String> = conn.query_row(
        "SELECT evidence FROM swarm_decisions WHERE run_id=?1 AND job_id=?2 AND attempt_id=?3 AND decision='accept'
         ORDER BY id DESC LIMIT 1", params![run, job, attempt], |r| r.get(0)).optional()?;
    Ok(accepted.and_then(|raw| serde_json::from_str::<Vec<String>>(&raw).ok()).is_some_and(|ids| ids.iter().any(|a| a == artifact)))
}

fn readout(conn: &Connection, run: &str, finding: &str) -> Result<Value> {
    let (title, root, merged, created_revision): (String, Option<String>, Option<String>, i64) = conn.query_row(
        "SELECT title,root_cause,merged_into,created_revision FROM swarm_findings WHERE run_id=?1 AND finding_id=?2",
        params![run, finding], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?;
    let reproducer: Option<(String, String, i64)> = conn.query_row(
        "SELECT job_id,budget_unit,budget_milli FROM swarm_reproductions WHERE run_id=?1 AND finding_id=?2",
        params![run, finding], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).optional()?;
    let mut stmt = conn.prepare("SELECT job_id,artifact_id,endpoint FROM swarm_finding_evidence WHERE run_id=?1 AND finding_id=?2
        ORDER BY endpoint,job_id,artifact_id")?;
    let entries = stmt.query_map(params![run, finding], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut by_endpoint: BTreeMap<String, (Vec<Value>, bool)> = BTreeMap::new();
    let mut jobs = HashSet::new();
    for (job, artifact, endpoint) in entries {
        let proof = confirming(conn, run, &job, &artifact)?
            && reproducer.as_ref().is_none_or(|(repro, _, _)| repro == &job);
        jobs.insert(job.clone());
        let slot = by_endpoint.entry(endpoint).or_insert((Vec::new(), false));
        slot.0.push(json!({"job_id":job,"artifact_id":artifact,"confirming":proof}));
        slot.1 |= proof;
    }
    let confirmed = by_endpoint.values().filter(|(_, c)| *c).count();
    let status = if merged.is_some() { "merged" }
        else if !by_endpoint.is_empty() && confirmed == by_endpoint.len() { "confirmed" }
        else if confirmed > 0 { "partially_confirmed" } else { "candidate" };
    let reproducer = match reproducer {
        Some((job, unit, amount)) => {
            let spent: i64 = conn.query_row("SELECT COALESCE(SUM(amount_milli),0) FROM swarm_reproduction_charges WHERE run_id=?1 AND job_id=?2",
                params![run, job], |r| r.get(0))?;
            let state: Option<String> = conn.query_row("SELECT status FROM swarm_jobs WHERE run_id=?1 AND id=?2", params![run, job], |r| r.get(0)).optional()?;
            let mut sources_stmt = conn.prepare("SELECT message_id FROM swarm_reproduction_sources WHERE run_id=?1 AND finding_id=?2 ORDER BY created_ms,message_id")?;
            let sources: Vec<String> = sources_stmt.query_map(params![run, finding], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
            json!({"job_id":job,"job_status":state,"budget_milli":{unit: amount},"spent_milli":spent,"sources":sources})
        }
        None => Value::Null,
    };
    Ok(json!({"run_id":run,"finding_id":finding,"title":title,"root_cause":root,"merged_into":merged,
        "created_revision":created_revision,"status":status,"agreeing_jobs":jobs.len(),
        "endpoints":by_endpoint.into_iter().map(|(endpoint,(evidence,confirmed))| json!({"endpoint":endpoint,
            "confirmed":confirmed,"evidence":evidence})).collect::<Vec<_>>(),
        "reproducer":reproducer}))
}

pub fn list(store: &Store, p: &Value) -> Result<Value> {
    let run = required(p, "run_id")?;
    get(store, run)?;
    let mut stmt = store.conn.prepare("SELECT finding_id FROM swarm_findings WHERE run_id=?1 ORDER BY created_ms,finding_id LIMIT 1000")?;
    let ids: Vec<String> = stmt.query_map([run], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
    let findings = ids.iter().map(|id| readout(&store.conn, run, id)).collect::<Result<Vec<_>>>()?;
    Ok(json!({"run_id":run,"findings":findings}))
}
