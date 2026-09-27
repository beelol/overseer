//! Durable eligibility observation over the shared Auto Mode snapshot shape.
//! The server exposes this only to fixtures until Auto Mode owns the live feed.

use super::{get as get_run, policy, required};
use crate::store::Store;
use anyhow::{anyhow, bail, Result};
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, HashSet};

const WINDOW_PREVIEW_LIMIT: usize = 100;

pub fn get(store: &Store, run: &str) -> Result<Value> {
    let row: Option<(String, Option<String>, String, String, String, i64, i64, i64, i64)> = store
        .conn
        .query_row(
            "SELECT state,reason,eligible_targets,purpose,allowance_windows,
                    allowance_window_count,observed_ms,expires_ms,wake_count
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
                    r.get(7)?,
                    r.get(8)?,
                ))
            },
        )
        .optional()?;
    Ok(match row {
        Some((state, reason, targets, purpose, windows, window_count, observed, expires, wakes)) => json!({
            "state":state,"reason":reason,
            "eligible_targets":serde_json::from_str::<Value>(&targets)?,
            "allowance_windows":serde_json::from_str::<Value>(&windows)?,
            "allowance_window_count":window_count,
            "allowance_windows_truncated":window_count>WINDOW_PREVIEW_LIMIT as i64,
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
    let snapshot_sha256 = format!("{:x}", Sha256::digest(p["snapshot"].to_string().as_bytes()));
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
    let revoked_accounts: HashSet<&str> = p["snapshot"]["targets"].as_array()
        .into_iter().flatten()
        .filter(|target| target["auth"]=="revoked")
        .filter_map(|target| target["account_id"].as_str()).collect();
    let revoked_targets: Vec<String> = allowed.iter().filter_map(|id| {
        let id=id.as_str()?;
        p["snapshot"]["targets"].as_array()?.iter()
            .any(|target| target["id"]==id && target["account_id"].as_str()
                .is_some_and(|account| revoked_accounts.contains(account)))
            .then(|| id.to_string())
    }).collect();
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
    let prior_hashes: Option<(String, String)> = store
        .conn
        .query_row(
            "SELECT request_sha256,snapshot_sha256 FROM swarm_availability WHERE run_id=?1",
            params![run],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    if prior_hashes
        .as_ref()
        .is_some_and(|(hash, _)| hash != &request_sha256)
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
        || old["purpose"] != purpose
        || prior_hashes
            .as_ref()
            .is_some_and(|(_, hash)| hash != &snapshot_sha256);
    if !old.is_null() && observed == old["observed_ms"].as_i64().unwrap_or(-1) && changed {
        bail!("conflicting availability observation at the same time");
    }
    let woken = !old.is_null() && old["state"] == "blocked" && state == "eligible";
    let wakes = old["wake_count"].as_i64().unwrap_or(0) + i64::from(woken);
    let (allowance_windows,allowance_window_count)=if !changed && !old.is_null() {
        (old["allowance_windows"].clone(),old["allowance_window_count"].as_i64().unwrap_or(0))
    } else {
        selected_allowance_windows(&p["snapshot"],allowed,&old)
    };
    let tx = store.conn.transaction()?;
    tx.execute(
        "INSERT INTO swarm_availability(run_id,state,reason,eligible_targets,purpose,request_sha256,snapshot_sha256,allowance_windows,allowance_window_count,observed_ms,expires_ms,wake_count,updated_ms)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)
         ON CONFLICT(run_id) DO UPDATE SET state=excluded.state,reason=excluded.reason,
         eligible_targets=excluded.eligible_targets,purpose=excluded.purpose,
         snapshot_sha256=excluded.snapshot_sha256,allowance_windows=excluded.allowance_windows,
         allowance_window_count=excluded.allowance_window_count,
         observed_ms=excluded.observed_ms,expires_ms=excluded.expires_ms,
         wake_count=excluded.wake_count,updated_ms=excluded.updated_ms",
        params![run,state,reason,json!(eligible).to_string(),purpose,request_sha256,snapshot_sha256,
            allowance_windows.to_string(),allowance_window_count,observed,expires,wakes,now],
    )?;
    let mut revoked_jobs=Vec::new();
    for target in &revoked_targets {
        let mut stmt=tx.prepare(
            "SELECT DISTINCT j.id FROM swarm_jobs j
             JOIN swarm_attempts a ON a.run_id=j.run_id AND a.job_id=j.id AND a.status='registered'
             JOIN swarm_admissions s ON s.attempt_id=a.id AND s.target_id=?2
             WHERE j.run_id=?1 AND j.status IN ('reserved','launching','running','submitted','accepted')
             ORDER BY j.id")?;
        let jobs=stmt.query_map(params![run,target],|r|r.get::<_,String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        drop(stmt);
        for job in jobs {
            tx.execute("UPDATE swarm_jobs SET status='cancel_requested',
                stop_reason='account_identity_revoked',updated_ms=?3
                WHERE run_id=?1 AND id=?2",
                params![run,job,now])?;
            revoked_jobs.push(job);
        }
    }
    revoked_jobs.sort();
    revoked_jobs.dedup();
    if !revoked_jobs.is_empty() {
        super::record_operation(&tx, run, "revoke")?;
        tx.execute(
            "INSERT INTO swarm_messages(run_id,message_id,job_id,attempt_id,sender,recipient,kind,revision,payload,phase,created_ms,updated_ms)
             VALUES(?1,?2,NULL,NULL,'control','director','availability',?3,?4,'queued',?5,?5)",
            params![run,format!("identity-revoked-{observed}"),
                current["revision"].as_i64().unwrap_or(0),
                json!({"reason":"account_identity_revoked","targets":revoked_targets,
                    "affected_jobs":revoked_jobs}).to_string(),now],
        )?;
    }
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
        "changed":changed,"woken":woken,"wake_count":wakes,
        "allowance_windows":allowance_windows,
        "allowance_window_count":allowance_window_count,
        "allowance_windows_truncated":allowance_window_count>WINDOW_PREVIEW_LIMIT as i64,
        "revoked_jobs":revoked_jobs}),
    )
}

fn selected_allowance_windows(snapshot: &Value, allowed: &[Value], old: &Value) -> (Value,i64) {
    let allowed_ids: HashSet<&str>=allowed.iter().filter_map(Value::as_str).collect();
    let mut pool_ids=BTreeSet::new();
    for target in snapshot["targets"].as_array().into_iter().flatten() {
        if target["id"].as_str().is_some_and(|id|allowed_ids.contains(id)) {
            for pool in target["pool_ids"].as_array().into_iter().flatten() {
                if let Some(id)=pool.as_str() { pool_ids.insert(id); }
            }
        }
    }
    let mut windows=Vec::new();
    let mut count=0_i64;
    for pool_id in pool_ids {
        let Some(pool)=snapshot["pools"].as_array().into_iter().flatten()
            .find(|pool|pool["id"]==pool_id) else {continue};
        let mut current:Vec<&Value>=pool["windows"].as_array().into_iter().flatten().collect();
        current.sort_by_key(|window|window["id"].as_str().unwrap_or("").to_owned());
        for window in current {
            count+=1;
            if windows.len()>=WINDOW_PREVIEW_LIMIT {continue;}
            let window_id=window["id"].as_str().unwrap_or("");
            let unit=window["unit"].as_str().unwrap_or("");
            let previous=old["allowance_windows"].as_array().into_iter().flatten()
                .find(|prior| prior["pool_id"]==pool_id && prior["window_id"]==window_id
                    && prior["unit"]==unit)
                .and_then(|prior|prior["remaining_milli"].as_i64());
            let remaining=window["remaining_milli"].as_i64();
            let protected=window["protected_milli"].as_i64().unwrap_or(0);
            let reserved=window["reserved_milli"].as_i64().unwrap_or(0);
            let usable=remaining.map(|v|v.saturating_sub(protected).saturating_sub(reserved).max(0));
            let change=remaining.zip(previous).map(|(now,before)|now.saturating_sub(before));
            windows.push(json!({"pool_id":pool_id,"window_id":window_id,"unit":unit,
                "remaining_milli":remaining,"protected_milli":protected,"reserved_milli":reserved,
                "observed_usable_milli":usable,"confidence":window["confidence"],
                "window_expires_ms":window["expires_ms"],
                "previous_remaining_milli":previous,"change_milli":change}));
        }
    }
    (json!(windows),count)
}
