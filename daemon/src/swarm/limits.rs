//! User-requested per-run worker ceiling changes. This revision is separate
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
    let max_workers = p["max_workers"]
        .as_i64()
        .ok_or_else(|| anyhow!("missing worker ceiling"))?;
    if !(1..=10_000).contains(&max_workers) {
        bail!("worker ceiling must be an integer from 1 to 10000");
    }
    let tx = store.conn.transaction()?;
    let replay: Option<(i64, i64, i64)> = tx
        .query_row(
            "SELECT expected_revision,new_max_workers,limit_revision
             FROM swarm_limit_events WHERE run_id=?1 AND request_id=?2",
            params![run, request_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    if let Some((old_expected, old_max, revision)) = replay {
        if old_expected != expected || old_max != max_workers {
            bail!("limit request id reused with different input");
        }
        return Ok(json!({"run_id":run,"limit_revision":revision,
            "max_workers":max_workers,"duplicate":true}));
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
    policy["effective"]["max_workers"] = json!(max_workers);
    policy["sources"]["max_workers"] = json!("run_update");
    let next = revision + 1;
    let now = crate::daemon::now();
    tx.execute(
        "UPDATE swarm_runs SET policy=?2,limit_revision=?3,updated_ms=?4
         WHERE id=?1 AND limit_revision=?5",
        params![run, policy.to_string(), next, now, revision],
    )?;
    tx.execute(
        "INSERT INTO swarm_limit_events(run_id,request_id,expected_revision,limit_revision,
            old_max_workers,new_max_workers,created_ms) VALUES(?1,?2,?3,?4,?5,?6,?7)",
        params![run, request_id, revision, next, old_max, max_workers, now],
    )?;
    tx.commit()?;
    Ok(json!({"run_id":run,"limit_revision":next,
        "max_workers":max_workers,"previous_max_workers":old_max,"duplicate":false}))
}
