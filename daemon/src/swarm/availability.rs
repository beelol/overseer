//! Durable eligibility observation over the shared Auto Mode snapshot shape.
//! The server exposes this only to fixtures until Auto Mode owns the live feed.

use super::{get as get_run, policy, required};
use crate::store::Store;
use anyhow::{anyhow, bail, Result};
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

pub fn get(store: &Store, run: &str) -> Result<Value> {
    let row: Option<(String, Option<String>, String, String, i64, i64, i64)> = store
        .conn
        .query_row(
            "SELECT state,reason,eligible_targets,purpose,observed_ms,expires_ms,wake_count
         FROM swarm_availability WHERE run_id=?1",
            params![run],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                ))
            },
        )
        .optional()?;
    Ok(match row {
        Some((state, reason, targets, purpose, observed, expires, wakes)) => json!({
            "state":state,"reason":reason,
            "eligible_targets":serde_json::from_str::<Value>(&targets)?,
            "purpose":purpose,"observed_ms":observed,"expires_ms":expires,
            "wake_count":wakes}),
        None => Value::Null,
    })
}

pub fn observe(store: &mut Store, p: &Value) -> Result<Value> {
    let run = required(p, "run_id")?;
    let now = p["now_ms"]
        .as_i64()
        .ok_or_else(|| anyhow!("missing observation time"))?;
    let current = get_run(store, run)?;
    if !["planning", "running", "paused"].contains(&current["status"].as_str().unwrap_or("")) {
        bail!("run cannot observe eligibility in this state");
    }
    let deadline = current["policy"]["effective"]["deadline_ms"]
        .as_i64()
        .unwrap_or(3_600_000);
    let created = current["created_ms"].as_i64().unwrap_or(now);
    if now >= created.saturating_add(deadline) {
        super::stop_for_deadline(
            store,
            &json!({"run_id":run,
            "generation":current["generation"],"revision":current["revision"]}),
        )?;
        return Ok(json!({"state":"blocked","reason":"run_deadline",
            "changed":false,"woken":false,
            "wake_count":current["availability"]["wake_count"].as_i64().unwrap_or(0)}));
    }
    let allowed = current["allowed_targets"]
        .as_array()
        .ok_or_else(|| anyhow!("invalid allowed target snapshot"))?;
    let effective = &current["policy"]["effective"];
    let request = json!({
        "now_ms":now,"allowed_targets":current["allowed_targets"],
        "required_capabilities":p["required_capabilities"],
        "purpose":p["purpose"],"estimate_milli":p["estimate_milli"],
        "finishing_estimate_milli":p.get("finishing_estimate_milli").cloned().unwrap_or(json!({})),
        "allow_estimated":effective["allow_estimated_quota"].as_bool().unwrap_or(false),
        "allocation_percent":effective["run_allocation_percent"],
        "finishing_reserve_percent":effective["finishing_reserve_percent"],
    });
    let mut stable_request = request.clone();
    stable_request.as_object_mut().unwrap().remove("now_ms");
    let request_sha256 = format!(
        "{:x}",
        Sha256::digest(stable_request.to_string().as_bytes())
    );
    let preview = policy::preview(&json!({"snapshot":p["snapshot"],"request":request}))?;
    let observed = p["snapshot"]["observed_ms"]
        .as_i64()
        .ok_or_else(|| anyhow!("missing snapshot observation time"))?;
    let expires = p["snapshot"]["expires_ms"]
        .as_i64()
        .ok_or_else(|| anyhow!("missing snapshot expiry"))?;
    let purpose = required(p, "purpose")?;
    let targets = &preview["targets"];
    let mut eligible = Vec::new();
    let mut reasons = Vec::new();
    let mut missing = 0;
    for id in allowed {
        let id = id
            .as_str()
            .ok_or_else(|| anyhow!("invalid allowed target"))?;
        let target = &targets[id];
        if target.is_null() {
            missing += 1;
        } else if target["eligible"] == true {
            eligible.push(id.to_string());
        } else {
            reasons.push(
                target["reason"]
                    .as_str()
                    .unwrap_or("ineligible_target")
                    .to_string(),
            );
        }
    }
    eligible.sort();
    let reason = if !eligible.is_empty() {
        None
    } else if allowed.is_empty() {
        Some("no_allowed_target".to_string())
    } else if missing == allowed.len() {
        Some("allowed_target_missing".to_string())
    } else if missing == 0 && reasons.iter().all(|r| r == &reasons[0]) {
        Some(reasons[0].clone())
    } else {
        Some("no_eligible_target".to_string())
    };
    let state = if reason.is_some() {
        "blocked"
    } else {
        "eligible"
    };
    let old = get(store, run)?;
    let prior_request_sha256: Option<String> = store
        .conn
        .query_row(
            "SELECT request_sha256 FROM swarm_availability WHERE run_id=?1",
            params![run],
            |r| r.get(0),
        )
        .optional()?;
    if prior_request_sha256
        .as_deref()
        .is_some_and(|hash| hash != request_sha256)
    {
        bail!("availability assessment changed");
    }
    if !old.is_null() && observed < old["observed_ms"].as_i64().unwrap_or(-1) {
        bail!("out-of-order availability observation");
    }
    let changed = old.is_null()
        || old["state"] != state
        || old["reason"] != reason.as_deref().map(Value::from).unwrap_or(Value::Null)
        || old["eligible_targets"] != json!(eligible)
        || old["purpose"] != purpose;
    if !old.is_null() && observed == old["observed_ms"].as_i64().unwrap_or(-1) && changed {
        bail!("conflicting availability observation at the same time");
    }
    let woken = !old.is_null() && old["state"] == "blocked" && state == "eligible";
    let wakes = old["wake_count"].as_i64().unwrap_or(0) + i64::from(woken);
    let tx = store.conn.transaction()?;
    tx.execute(
        "INSERT INTO swarm_availability(run_id,state,reason,eligible_targets,purpose,request_sha256,observed_ms,expires_ms,wake_count,updated_ms)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)
         ON CONFLICT(run_id) DO UPDATE SET state=excluded.state,reason=excluded.reason,
         eligible_targets=excluded.eligible_targets,purpose=excluded.purpose,
         observed_ms=excluded.observed_ms,expires_ms=excluded.expires_ms,
         wake_count=excluded.wake_count,updated_ms=excluded.updated_ms",
        params![run,state,reason,json!(eligible).to_string(),purpose,request_sha256,observed,expires,wakes,now],
    )?;
    if woken {
        let message_id = format!("availability-wake-{wakes}");
        tx.execute(
            "INSERT INTO swarm_messages(run_id,message_id,job_id,attempt_id,sender,recipient,kind,revision,payload,phase,created_ms,updated_ms)
             VALUES(?1,?2,NULL,NULL,'control','director','availability',?3,?4,'queued',?5,?5)",
            params![run,message_id,current["revision"].as_i64().unwrap_or(0),
                json!({"state":state,"eligible_targets":eligible}).to_string(),now],
        )?;
    }
    tx.commit()?;
    Ok(
        json!({"state":state,"reason":reason,"eligible_targets":eligible,
        "changed":changed,"woken":woken,"wake_count":wakes}),
    )
}
