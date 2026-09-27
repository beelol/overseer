//! User-requested per-run worker and backlog ceiling changes. This revision is separate
//! from the director's plan revision so in-flight reports retain their contract.

use super::required;
use crate::store::Store;
use anyhow::{anyhow, bail, Result};
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};

pub fn set(store: &mut Store, p: &Value) -> Result<Value> {
    let run = required(p, "run_id")?;
    let request_id = required(p, "request_id")?;
    if request_id.is_empty() || request_id.len() > 128 {
        bail!("invalid limit request id");
    }
    let expected = p["expected_limit_revision"]
        .as_i64()
        .ok_or_else(|| anyhow!("missing expected limit revision"))?;
    if expected < 0 {
        bail!("invalid expected limit revision");
    }
    let requested_workers = p.get("max_workers").map(|v| v.as_i64()
        .ok_or_else(|| anyhow!("worker ceiling must be an integer from 1 to 10000"))).transpose()?;
    let requested_backlog = p.get("backlog_max").map(|v| v.as_i64()
        .ok_or_else(|| anyhow!("backlog ceiling must be an integer from 1 to 10000"))).transpose()?;
    if requested_workers.is_none() && requested_backlog.is_none() {
        bail!("missing worker or backlog ceiling");
    }
    if requested_workers.is_some_and(|n| !(1..=10_000).contains(&n)) {
        bail!("worker ceiling must be an integer from 1 to 10000");
    }
    if requested_backlog.is_some_and(|n| !(1..=10_000).contains(&n)) {
        bail!("backlog ceiling must be an integer from 1 to 10000");
    }
    let tx = store.conn.transaction()?;
    let replay: Option<(i64, i64, i64, Option<i64>, Option<i64>, Option<i64>)> = tx
        .query_row(
            "SELECT expected_revision,new_max_workers,limit_revision,
                    request_max_workers,request_backlog_max,new_backlog_max
             FROM swarm_limit_events WHERE run_id=?1 AND request_id=?2",
            params![run, request_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?,
                row.get(3)?, row.get(4)?, row.get(5)?)),
        )
        .optional()?;
    if let Some((old_expected, old_max, revision, old_request_workers,
        old_request_backlog, old_backlog)) = replay {
        // Pre-migration rows only stored worker changes.
        let old_request_workers = if old_request_workers.is_none() && old_backlog.is_none() {
            Some(old_max)
        } else { old_request_workers };
        if old_expected != expected || old_request_workers != requested_workers
            || old_request_backlog != requested_backlog {
            bail!("limit request id reused with different input");
        }
        return Ok(json!({"run_id":run,"limit_revision":revision,
            "max_workers":old_max,"backlog_max":old_backlog,"duplicate":true}));
    }
    let (status, raw_policy, revision): (String, String, i64) = tx
        .query_row(
            "SELECT status,policy,limit_revision FROM swarm_runs WHERE id=?1",
            params![run],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?
        .ok_or_else(|| anyhow!("unknown swarm run"))?;
    if !["planning", "running", "paused", "stalled", "draining"].contains(&status.as_str()) {
        bail!("swarm run cannot change its worker ceiling in this state");
    }
    if revision != expected {
        bail!("stale limit revision");
    }
    let mut policy: Value = serde_json::from_str(&raw_policy)?;
    let old_max = policy["effective"]["max_workers"]
        .as_i64()
        .ok_or_else(|| anyhow!("run is missing its worker ceiling"))?;
    let old_backlog = policy["effective"]["backlog_max"]
        .as_i64().ok_or_else(|| anyhow!("run is missing its backlog ceiling"))?;
    let max_workers = requested_workers.unwrap_or(old_max);
    let backlog_max = requested_backlog.unwrap_or(old_backlog);
    if requested_backlog.is_some() {
        let nonterminal: i64 = tx.query_row(
            "SELECT COUNT(*) FROM swarm_jobs WHERE run_id=?1 AND status NOT IN ('accepted','failed','cancelled')",
            [run], |row| row.get(0))?;
        if nonterminal > backlog_max {
            bail!("backlog ceiling is below existing nonterminal jobs");
        }
        policy["effective"]["backlog_max"] = json!(backlog_max);
        policy["sources"]["backlog_max"] = json!("run_update");
    }
    if requested_workers.is_some() {
        policy["effective"]["max_workers"] = json!(max_workers);
        policy["sources"]["max_workers"] = json!("run_update");
    }
    let next = revision + 1;
    let now = crate::daemon::now();
    tx.execute(
        "UPDATE swarm_runs SET policy=?2,limit_revision=?3,updated_ms=?4
         WHERE id=?1 AND limit_revision=?5",
        params![run, policy.to_string(), next, now, revision],
    )?;
    tx.execute(
        "INSERT INTO swarm_limit_events(run_id,request_id,expected_revision,limit_revision,
            old_max_workers,new_max_workers,request_max_workers,request_backlog_max,
            old_backlog_max,new_backlog_max,created_ms)
            VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
        params![run, request_id, revision, next, old_max, max_workers,
            requested_workers, requested_backlog, old_backlog, backlog_max, now],
    )?;
    tx.commit()?;
    Ok(json!({"run_id":run,"limit_revision":next,
        "max_workers":max_workers,"previous_max_workers":old_max,
        "backlog_max":backlog_max,"previous_backlog_max":old_backlog,"duplicate":false}))
}
