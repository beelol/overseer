//! One claim ledger for every agent in the daemon (SWARM-44, Gate S's "Areas and conflicts":
//! "one ledger for swarm workers and every other agent").
//!
//! Gate S landed first with `areas` (an ordinary agent's paths); Swarm keeps its jobs'
//! claims in `swarm_claims`, which carry what an area has no room for (the job, read or write,
//! the plan revision). Both are rows of the one ledger: every writer of either table asks
//! [`write_holders`] inside the same store transaction before it writes, so a Swarm job and an
//! ordinary agent can never both own one path. The store's single connection and lock make the
//! check and the write one step.
//!
//! Across the two kinds only exclusive ownership conflicts: an ordinary agent's area counts as
//! a write claim, a Swarm `write` claim as exclusive, and a Swarm `read` claim (independent
//! read-only analysis of a pinned revision) never conflicts with an area. Between Swarm jobs
//! the Swarm rules still apply (read and write conflict on one resource); between ordinary
//! agents Gate S's still apply (overlapping areas are allowed and become conflicts it shows).
//!
//! A claim is refused, never queued or reassigned here: who owns a path inside a swarm is the
//! director's decision, and the owner or Overseer decides for an ordinary agent. Every refusal
//! is recorded once in `claim_refusals`, so both sides' coordinators can see it.

use crate::daemon::ACTIVE;
use anyhow::Result;
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// Someone who owns a path in the ledger.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Holder {
    /// `swarm` (a Swarm job's write claim) or `agent` (an ordinary agent's area).
    pub kind: &'static str,
    /// The swarm run, or the ordinary agent's run.
    pub run: String,
    /// The Swarm job; `None` for an ordinary agent.
    pub job: Option<String>,
    /// The path it holds, as it was claimed.
    pub resource: String,
}

impl Holder {
    pub fn json(&self) -> Value {
        json!({"kind": self.kind, "run_id": self.run, "job_id": self.job, "resource": self.resource})
    }

    pub fn describe(&self) -> String {
        match &self.job {
            Some(job) => format!("Swarm job {job} of swarm {}", self.run),
            None => format!("agent {}", self.run),
        }
    }
}

/// Two claims overlap when they name the same path or one is a directory holding the other.
pub fn overlaps(a: &str, b: &str) -> bool {
    let a = a.trim().trim_start_matches("./").trim_end_matches('/');
    let b = b.trim().trim_start_matches("./").trim_end_matches('/');
    !a.is_empty() && !b.is_empty()
        && (a == b || a.starts_with(&format!("{b}/")) || b.starts_with(&format!("{a}/")))
}

fn canonical(p: &str) -> PathBuf {
    std::fs::canonicalize(Path::new(p)).unwrap_or_else(|_| PathBuf::from(p))
}

/// The repositories (Git common directories) a swarm run is scoped to; `None` when the run has
/// no recorded scope (a fixture run), which is compared with every repository.
fn swarm_scope(conn: &Connection, swarm_run: &str) -> Result<Option<Vec<PathBuf>>> {
    let raw: Option<String> = conn
        .query_row("SELECT repository_scope FROM swarm_runs WHERE id=?1", [swarm_run], |r| r.get(0))
        .unwrap_or(None);
    let Some(raw) = raw else { return Ok(None) };
    let rows: Vec<Value> = serde_json::from_str(&raw).unwrap_or_default();
    Ok(Some(rows.iter().filter_map(|row| row["common_dir"].as_str()).map(canonical).collect()))
}

fn in_scope(scope: &Option<Vec<PathBuf>>, common_dir: &str) -> bool {
    match scope {
        None => true,
        Some(dirs) => dirs.contains(&canonical(common_dir)),
    }
}

/// Runs that belong to a swarm (its director and its workers): their paths are the swarm's
/// claims, never areas of their own.
fn is_swarm_run(conn: &Connection, run: &str) -> Result<bool> {
    Ok(conn.prepare(
        "SELECT 1 FROM swarm_worker_launches WHERE overseer_run_id=?1
         UNION SELECT 1 FROM swarm_director_owners WHERE overseer_run_id=?1",
    )?.exists([run])?)
}

/// Ordinary agents whose area overlaps `resource`, in a repository the swarm run may touch.
/// Only live agents hold an area: a finished agent's area is history.
pub fn agent_holders(conn: &Connection, swarm_run: &str, resource: &str) -> Result<Vec<Holder>> {
    let scope = swarm_scope(conn, swarm_run)?;
    let active = ACTIVE.iter().map(|s| format!("'{s}'")).collect::<Vec<_>>().join(",");
    let mut stmt = conn.prepare(&format!(
        "SELECT a.run_id, a.path, w.common_dir FROM areas a
         JOIN runs r ON r.id=a.run_id JOIN workspaces w ON w.id=r.workspace_id
         WHERE r.status IN ({active}) ORDER BY a.created_ms, a.run_id, a.path"))?;
    let rows = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut out = Vec::new();
    for (run, path, common) in rows {
        if overlaps(&path, resource) && in_scope(&scope, &common) && !is_swarm_run(conn, &run)? {
            out.push(Holder { kind: "agent", run, job: None, resource: path });
        }
    }
    Ok(out)
}

/// Swarm jobs whose active write claim overlaps `path`, among swarms that may touch the
/// ordinary agent's repository.
pub fn swarm_write_holders(conn: &Connection, agent_run: &str, path: &str) -> Result<Vec<Holder>> {
    let common: Option<String> = conn
        .query_row("SELECT w.common_dir FROM runs r JOIN workspaces w ON w.id=r.workspace_id WHERE r.id=?1",
            [agent_run], |r| r.get(0))
        .ok();
    let mut stmt = conn.prepare(
        "SELECT resource, run_id, job_id FROM swarm_claims WHERE status='active' AND mode='write'
         ORDER BY created_ms, run_id, job_id")?;
    let rows = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut out = Vec::new();
    for (resource, run, job) in rows {
        if !overlaps(&resource, path) {
            continue;
        }
        let scope = swarm_scope(conn, &run)?;
        if common.as_deref().is_none_or(|c| in_scope(&scope, c)) {
            out.push(Holder { kind: "swarm", run, job: Some(job), resource });
        }
    }
    Ok(out)
}

/// Record a refused claim once (who asked, for what, and who holds it) and, when a Swarm job
/// is on either side, tell its director in its durable inbox as a routine event from the
/// ledger. The envelope names no attempt and assigns nothing: the director decides whether its
/// job keeps, releases or narrows the path. Returns the refusal's id.
pub fn refuse(conn: &Connection, claimant: &Holder, holder: &Holder) -> Result<String> {
    use sha2::Digest as _;
    let key = format!("{}|{}|{:?}|{}|{}|{}|{:?}|{}", claimant.kind, claimant.run, claimant.job, claimant.resource,
        holder.kind, holder.run, holder.job, holder.resource);
    let id = format!("cr-{}", &format!("{:x}", sha2::Sha256::digest(key.as_bytes()))[..16]);
    let now = crate::daemon::now();
    conn.execute(
        "INSERT OR IGNORE INTO claim_refusals(id, resource, claimant_kind, claimant_run, claimant_job,
           holder_kind, holder_run, holder_job, holder_resource, created_ms)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
        params![id, claimant.resource, claimant.kind, claimant.run, claimant.job, holder.kind, holder.run,
            holder.job, holder.resource, now],
    )?;
    let (swarm, agent) = if claimant.kind == "swarm" { (claimant, holder) } else { (holder, claimant) };
    if swarm.kind != "swarm" || agent.kind != "agent" {
        return Ok(id);
    }
    let terminal: bool = conn
        .prepare("SELECT 1 FROM swarm_runs WHERE id=?1 AND status IN ('stopped','completed','invalidated')")?
        .exists([&swarm.run])?;
    if terminal {
        return Ok(id);
    }
    let revision: i64 = conn.query_row("SELECT revision FROM swarm_runs WHERE id=?1", [&swarm.run], |r| r.get(0))?;
    let payload = json!({"source":"ledger","refusal":id,"claimant":claimant.kind,
        "resource":claimant.resource,"held":holder.resource,"agent_run_id":agent.run,
        "swarm_job_id":swarm.job,
        "note": if claimant.kind == "agent" {
            "an ordinary agent asked for a path your job holds; it was refused and your job keeps it"
        } else {
            "your job asked for a path inside an ordinary agent's area; it was refused"
        }});
    conn.execute(
        "INSERT OR IGNORE INTO swarm_messages(run_id,message_id,job_id,attempt_id,sender,recipient,kind,revision,payload,phase,created_ms,updated_ms)
         VALUES(?1,?2,?3,NULL,'ledger','director','claim',?4,?5,'queued',?6,?6)",
        params![swarm.run, format!("ledger-{id}"), swarm.job, revision, payload.to_string(), now],
    )?;
    Ok(id)
}

/// The refusal message for a claimant, naming who holds the path and who decides.
pub fn refusal_text(holders: &[Holder]) -> String {
    let named: Vec<String> = holders.iter().map(|h| format!("{} ({})", h.resource, h.describe())).collect();
    if holders.iter().any(|h| h.kind == "swarm") {
        format!("claim conflict: held by {}; that swarm's director decides who owns it, so ask Overseer to raise it with the director", named.join(", "))
    } else {
        format!("claim conflict: owned by {}", named.join(", "))
    }
}

/// The whole ledger, for a readout: Swarm jobs' active claims and live ordinary agents' areas,
/// and the refusals it has made.
pub fn ledger(conn: &Connection) -> Result<Value> {
    let mut rows = Vec::new();
    {
        let mut stmt = conn.prepare(
            "SELECT resource, run_id, job_id, mode FROM swarm_claims WHERE status='active' ORDER BY created_ms, resource")?;
        for row in stmt.query_map([], |r| Ok(json!({"holder":"swarm","resource":r.get::<_,String>(0)?,
            "run_id":r.get::<_,String>(1)?,"job_id":r.get::<_,String>(2)?,"mode":r.get::<_,String>(3)?})))? {
            rows.push(row?);
        }
    }
    {
        let active = ACTIVE.iter().map(|s| format!("'{s}'")).collect::<Vec<_>>().join(",");
        let mut stmt = conn.prepare(&format!(
            "SELECT a.path, a.run_id, a.set_by FROM areas a JOIN runs r ON r.id=a.run_id
             WHERE r.status IN ({active}) ORDER BY a.created_ms, a.path"))?;
        let found = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for (path, run, by) in found {
            if !is_swarm_run(conn, &run)? {
                rows.push(json!({"holder":"agent","resource":path,"run_id":run,"job_id":null,"mode":"write","set_by":by}));
            }
        }
    }
    let mut stmt = conn.prepare(
        "SELECT id, resource, claimant_kind, claimant_run, claimant_job, holder_kind, holder_run, holder_job,
                holder_resource, created_ms FROM claim_refusals ORDER BY created_ms, id LIMIT 1000")?;
    let refusals = stmt.query_map([], |r| Ok(json!({"id":r.get::<_,String>(0)?,"resource":r.get::<_,String>(1)?,
        "claimant":{"kind":r.get::<_,String>(2)?,"run_id":r.get::<_,String>(3)?,"job_id":r.get::<_,Option<String>>(4)?},
        "holder":{"kind":r.get::<_,String>(5)?,"run_id":r.get::<_,String>(6)?,"job_id":r.get::<_,Option<String>>(7)?,
            "resource":r.get::<_,String>(8)?},"created_ms":r.get::<_,i64>(9)?})))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(json!({"claims": rows, "refusals": refusals}))
}

impl crate::daemon::Daemon {
    /// Show each new refusal on the ordinary agent's side: an event on the agent's run and a
    /// card in Overseer's conversation (Overseer is that agent's coordinator, as the director is
    /// the Swarm job's). Neither decides for the other side; nothing is reassigned.
    pub fn notify_claim_refusals(self: &std::sync::Arc<Self>) -> Result<()> {
        let pending: Vec<(String, String, String, String, Option<String>, String, String, Option<String>, String)> = {
            let store = self.store.lock().unwrap();
            let mut stmt = store.conn.prepare(
                "SELECT id, resource, claimant_kind, claimant_run, claimant_job, holder_kind, holder_run, holder_job,
                        holder_resource FROM claim_refusals WHERE notified_ms IS NULL ORDER BY created_ms, id")?;
            let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?,
                r.get(6)?, r.get(7)?, r.get(8)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
            rows
        };
        for (id, resource, claimant_kind, claimant_run, claimant_job, holder_kind, holder_run, holder_job, held) in pending {
            let agent_run = if claimant_kind == "agent" { &claimant_run } else { &holder_run };
            let swarm_side = if claimant_kind == "swarm" { Some((&claimant_run, &claimant_job)) }
                else if holder_kind == "swarm" { Some((&holder_run, &holder_job)) } else { None };
            let card = json!({"kind":"claim_refused","id":id,"resource":resource,"held":held,
                "claimant":{"kind":claimant_kind,"run_id":claimant_run,"job_id":claimant_job},
                "holder":{"kind":holder_kind,"run_id":holder_run,"job_id":holder_job}});
            if let Ok(run) = self.run(agent_run) {
                let text = match (claimant_kind.as_str(), swarm_side) {
                    ("agent", Some((swarm, job))) => format!("{}'s claim of {resource} was refused: Swarm job {} of swarm {swarm} holds {held}; its director decides.",
                        run.title, job.clone().unwrap_or_default()),
                    (_, Some((swarm, job))) => format!("Swarm job {} of swarm {swarm} asked for {resource}, inside {}'s area {held}; it was refused and its director was told.",
                        job.clone().unwrap_or_default(), run.title),
                    _ => format!("{}'s claim of {resource} was refused: {held} is held.", run.title),
                };
                self.emit(Some(&run.task_id), Some(agent_run), "claim_refused", "daemon", "exact", card.clone())?;
                if let Ok(session) = self.overseer_session() {
                    let sid = session["id"].as_str().unwrap_or_default().to_string();
                    self.append_session_message(&sid, "card", None, &text, Some(&card))?;
                }
            }
            self.store.lock().unwrap().conn.execute("UPDATE claim_refusals SET notified_ms=?2 WHERE id=?1",
                params![id, crate::daemon::now()])?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::overlaps;

    #[test]
    fn a_directory_overlaps_what_it_holds_and_nothing_else() {
        assert!(overlaps("routes", "routes/tasks.ts"));
        assert!(overlaps("routes/tasks.ts", "routes/"));
        assert!(overlaps("./routes/tasks.ts", "routes/tasks.ts"));
        assert!(!overlaps("routes", "routes-v2/tasks.ts"));
        assert!(!overlaps("atlas-db-j2", "atlas-db-j7"));
        assert!(!overlaps("", "routes"));
    }
}
