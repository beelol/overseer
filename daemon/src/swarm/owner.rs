//! Durable director identity. The scripted process path receives its token privately;
//! live model harness authority remains unqualified.

use super::{get, required};
use crate::store::Store;
use anyhow::{anyhow, bail, Result};
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

const LEASE_MS: i64 = 30_000;

fn hash(token: &str) -> String {
    format!("{:x}", Sha256::digest(token.as_bytes()))
}

pub fn begin(store: &mut Store, p: &Value) -> Result<Value> {
    let run = required(p, "run_id")?;
    let generation = p["generation"].as_i64()
        .ok_or_else(|| anyhow!("missing director generation"))?;
    let current = get(store, run)?;
    if current["generation"] != generation {
        bail!("stale director generation");
    }
    let replacement_pending = current["status"] == "stalled"
        && current["stall_reason"] == "director_replacement_pending";
    if !replacement_pending
        && !["planning", "running", "paused"].contains(&current["status"].as_str().unwrap_or("")) {
        bail!("run cannot begin a director owner in this state");
    }
    let now = crate::daemon::now();
    let token = uuid::Uuid::new_v4().simple().to_string();
    let supervised = i64::from(p["supervised_launch"].as_bool().unwrap_or(false));
    let tx = store.conn.transaction()?;
    let existing: Option<(i64,String)> = tx.query_row(
        "SELECT generation,status FROM swarm_director_owners WHERE run_id=?1",
        [run], |row| Ok((row.get(0)?,row.get(1)?)),
    ).optional()?;
    if existing.as_ref().is_some_and(|(old,status)| *old >= generation || status != "released") {
        bail!("director owner already reserved; process reconciliation required");
    }
    let active_turn = tx.prepare(
        "SELECT 1 FROM swarm_director_turns WHERE run_id=?1 AND status='active'",
    )?.exists([run])?;
    if active_turn {
        bail!("active director turn requires reconciliation before owner binding");
    }
    tx.execute(
        "INSERT INTO swarm_director_owners(run_id,generation,token_sha256,status,created_ms,renewed_ms,lease_expires_ms,supervised_launch)
         VALUES(?1,?2,?3,'active',?4,?4,?5,?6)
         ON CONFLICT(run_id) DO UPDATE SET generation=excluded.generation,
             token_sha256=excluded.token_sha256,status='active',created_ms=excluded.created_ms,
             renewed_ms=excluded.renewed_ms,lease_expires_ms=excluded.lease_expires_ms,
             overseer_run_id=NULL,supervised_launch=excluded.supervised_launch",
        params![run,generation,hash(&token),now,now+LEASE_MS,supervised],
    )?;
    if replacement_pending {
        let restored = current["stalled_from"].as_str().unwrap_or("planning");
        if !["planning", "running", "paused", "draining"].contains(&restored) {
            bail!("invalid director replacement state");
        }
        tx.execute("UPDATE swarm_runs SET status=?2,stalled_from=NULL,stall_reason=NULL,updated_ms=?3
            WHERE id=?1 AND generation=?4 AND status='stalled' AND stall_reason='director_replacement_pending'",
            params![run,restored,now,generation])?;
    }
    tx.commit()?;
    Ok(json!({"run_id":run,"generation":generation,"owner_token":token,
        "lease_expires_ms":now+LEASE_MS}))
}

pub fn require(store: &Store, run: &str, p: &Value) -> Result<()> {
    let owner: Option<(i64,String,String,i64)> = store.conn.query_row(
        "SELECT generation,token_sha256,status,lease_expires_ms FROM swarm_director_owners WHERE run_id=?1",
        [run], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?)),
    ).optional()?;
    let Some((generation,digest,status,expires)) = owner else {
        // Scripted fixtures predate owner binding. Normal live director routes are
        // still closed by the server's fixture gate.
        return Ok(());
    };
    if status != "active" || p["generation"] != generation {
        bail!("stale or inactive director owner generation");
    }
    let token = required(p,"owner_token")?;
    if digest != hash(token) {
        bail!("invalid director owner identity");
    }
    if crate::daemon::now() >= expires {
        bail!("director owner lease expired; reconciliation required");
    }
    Ok(())
}

pub fn renew(store: &mut Store, p: &Value) -> Result<Value> {
    let run = required(p,"run_id")?;
    require(store,run,p)?;
    let now = crate::daemon::now();
    let updated = store.conn.execute(
        "UPDATE swarm_director_owners SET renewed_ms=?2,lease_expires_ms=?3
         WHERE run_id=?1 AND status='active' AND lease_expires_ms>?2",
        params![run,now,now+LEASE_MS],
    )?;
    if updated == 0 {
        bail!("director owner is not bound");
    }
    Ok(json!({"run_id":run,"generation":p["generation"],
        "lease_expires_ms":now+LEASE_MS}))
}

/// A lease timeout only makes director termination uncertain. It never releases
/// ownership or reservations; replacement still requires confirmed process death.
pub fn expire_due(store: &mut Store) -> Result<usize> {
    let now = crate::daemon::now();
    let stalled = store.conn.execute(
        "UPDATE swarm_runs SET stalled_from=status,status='stalled',
         stall_reason='director_termination_unknown',updated_ms=?1
         WHERE status IN ('planning','running','paused','draining')
         AND EXISTS(SELECT 1 FROM swarm_director_owners o WHERE o.run_id=swarm_runs.id
                    AND o.generation=swarm_runs.generation AND o.status='active'
                    AND o.lease_expires_ms<=?1)",
        [now],
    )?;
    Ok(stalled)
}
