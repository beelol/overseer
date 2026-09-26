//! Fixture admission transaction. A live call must use daemon-owned Auto Mode telemetry.

use super::{get, policy, required};
use crate::store::Store;
use anyhow::{anyhow, bail, Result};
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

fn hash(raw: &str) -> String {
    format!("{:x}", Sha256::digest(raw.as_bytes()))
}

fn blocked(reason: &str) -> Value {
    json!({"status":"blocked","reason":reason})
}

pub fn admit(store: &mut Store, p: &Value) -> Result<Value> {
    let run = required(p, "run_id")?;
    let job = required(p, "job_id")?;
    let target = required(p, "target_id")?;
    let request_id = required(p, "request_id")?;
    if request_id.is_empty() || request_id.len() > 128 {
        bail!("invalid admission request id");
    }
    let generation = p["generation"]
        .as_i64()
        .ok_or_else(|| anyhow!("missing generation"))?;
    let revision = p["revision"]
        .as_i64()
        .ok_or_else(|| anyhow!("missing revision"))?;
    let now = p["now_ms"]
        .as_i64()
        .ok_or_else(|| anyhow!("missing admission time"))?;
    let request_hash = hash(&p.to_string());
    let current = get(store, run)?;
    if current["generation"] != generation {
        bail!("stale director generation");
    }
    if current["revision"] != revision {
        bail!("stale plan revision");
    }
    let previous: Option<(String,String)> = store.conn.query_row(
        "SELECT request_sha256,attempt_id FROM swarm_admissions WHERE run_id=?1 AND request_id=?2",
        params![run,request_id], |r|Ok((r.get(0)?,r.get(1)?)),
    ).optional()?;
    if let Some((old_hash, attempt_id)) = previous {
        if old_hash != request_hash {
            bail!("admission request id reused with different input");
        }
        return Ok(json!({"status":"already_admitted","attempt_id":attempt_id}));
    }
    let deadline = current["policy"]["effective"]["deadline_ms"]
        .as_i64()
        .unwrap_or(3_600_000);
    let created = current["created_ms"].as_i64().unwrap_or(now);
    if now >= created.saturating_add(deadline) {
        if current["status"] != "stopping"
            && current["status"] != "stopped"
            && current["status"] != "completed"
        {
            super::stop(
                store,
                &json!({"run_id":run,"generation":generation,"revision":revision}),
            )?;
        }
        return Ok(blocked("run_deadline"));
    }
    let tx = store.conn.transaction()?;
    let replay: Option<(String, String)> = tx
        .query_row(
            "SELECT request_sha256,attempt_id FROM swarm_admissions WHERE run_id=?1 AND request_id=?2",
            params![run, request_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    if let Some((old_hash, attempt_id)) = replay {
        if old_hash != request_hash {
            bail!("admission request id reused with different input");
        }
        return Ok(json!({"status":"already_admitted","attempt_id":attempt_id}));
    }
    if current["status"] != "planning" && current["status"] != "running" {
        return Ok(blocked("run_not_admitting"));
    }
    let effective = &current["policy"]["effective"];
    let request = json!({
        "now_ms":now,
        "allowed_targets":current["allowed_targets"],
        "required_capabilities":p["required_capabilities"],
        "purpose":p["purpose"],
        "estimate_milli":p["estimate_milli"],
        "finishing_estimate_milli":p.get("finishing_estimate_milli").cloned().unwrap_or(json!({})),
        "allow_estimated":effective["allow_estimated_quota"].as_bool().unwrap_or(false),
    });
    let preview = policy::preview(&json!({"snapshot":p["snapshot"],"request":request}))?;
    let candidate = &preview["targets"][target];
    if candidate.is_null() {
        return Ok(blocked("unknown_target"));
    }
    if candidate["eligible"] != true {
        return Ok(blocked(
            candidate["reason"].as_str().unwrap_or("ineligible_target"),
        ));
    }
    let job_info: Option<(String, i64, i64)> = tx
        .query_row(
            "SELECT status,plan_revision,attempt_count FROM swarm_jobs WHERE run_id=?1 AND id=?2",
            params![run, job],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    let Some((job_status, job_revision, attempts)) = job_info else {
        return Ok(blocked("unknown_job"));
    };
    if job_status != "ready" {
        return Ok(blocked("job_not_ready"));
    }
    if attempts >= effective["max_attempts"].as_i64().unwrap_or(2).min(2) {
        return Ok(blocked("attempt_limit"));
    }
    let pending: i64 = tx.query_row(
        "SELECT COUNT(*) FROM swarm_jobs WHERE run_id=?1 AND status='submitted'",
        params![run],
        |r| r.get(0),
    )?;
    let was_held: i64 = tx
        .query_row(
            "SELECT held FROM swarm_review_gate WHERE run_id=?1",
            params![run],
            |r| r.get(0),
        )
        .optional()?
        .unwrap_or(0);
    let hold_at = effective["review_hold"].as_i64().unwrap_or(8);
    let resume_below = effective["review_resume"].as_i64().unwrap_or(4);
    let held = if pending >= hold_at {
        1
    } else if pending < resume_below {
        0
    } else {
        was_held
    };
    tx.execute("INSERT INTO swarm_review_gate(run_id,held) VALUES(?1,?2) ON CONFLICT(run_id) DO UPDATE SET held=excluded.held",params![run,held])?;
    if held == 1 {
        tx.commit()?;
        return Ok(blocked("review_backlog"));
    }
    let workers: i64 = tx.query_row(
        "SELECT COUNT(*) FROM swarm_attempts WHERE run_id=?1 AND status='registered'",
        params![run],
        |r| r.get(0),
    )?;
    if workers >= effective["max_workers"].as_i64().unwrap_or(8) {
        return Ok(blocked("worker_limit"));
    }
    let global_workers: i64 = tx.query_row(
        "SELECT COUNT(*) FROM swarm_attempts WHERE status='registered'",
        [],
        |r| r.get(0),
    )?;
    let global_directors: i64 = tx.query_row(
        "SELECT COUNT(*) FROM swarm_runs WHERE status IN ('running','paused','stalled','stopping')",
        [],
        |r| r.get(0),
    )?;
    let new_director = if current["status"] == "planning" {
        1
    } else {
        0
    };
    if global_workers + global_directors + new_director
        >= effective["max_executing"].as_i64().unwrap_or(9)
    {
        return Ok(blocked("global_agent_limit"));
    }
    let growth: Option<(i64, i64)> = tx
        .query_row(
            "SELECT wave_start_ms,admitted_count FROM swarm_growth WHERE run_id=?1",
            params![run],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let interval = effective["growth_interval_ms"].as_i64().unwrap_or(5000);
    let (wave_start, count) = match growth {
        Some((start, count)) if now >= start && now - start < interval => (start, count),
        _ => (now, 0),
    };
    if count >= effective["growth_per_wave"].as_i64().unwrap_or(4) {
        return Ok(blocked("growth_wave_full"));
    }
    let windows = candidate["windows"]
        .as_array()
        .ok_or_else(|| anyhow!("qualified target has no quota windows"))?;
    if windows.is_empty() {
        return Ok(blocked("unknown_quota"));
    }
    let mut chosen = Vec::new();
    for window in windows {
        let pool = window["pool_id"]
            .as_str()
            .ok_or_else(|| anyhow!("missing pool"))?;
        let window_id = window["window_id"]
            .as_str()
            .ok_or_else(|| anyhow!("missing window"))?;
        let unit = window["unit"]
            .as_str()
            .ok_or_else(|| anyhow!("missing unit"))?;
        let usable = window["usable_milli"]
            .as_i64()
            .ok_or_else(|| anyhow!("missing usable quota"))?;
        let estimate = window["estimate_milli"]
            .as_i64()
            .ok_or_else(|| anyhow!("missing estimate"))?;
        let global_reserved: i64 = tx.query_row(
            "SELECT COALESCE(SUM(amount_milli),0) FROM swarm_reservations WHERE pool_id=?1 AND window_id=?2 AND status IN ('active','uncertain')",
            params![pool,window_id], |r|r.get(0),
        )?;
        if estimate > usable.saturating_sub(global_reserved) {
            return Ok(blocked("shared_pool_headroom"));
        }
        let frozen: Option<(String,i64,i64)> = tx.query_row(
            "SELECT unit,allocation_milli,reserve_milli FROM swarm_allocations WHERE run_id=?1 AND pool_id=?2 AND window_id=?3",
            params![run,pool,window_id], |r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)),
        ).optional()?;
        let (allocation, reserve) = if let Some((old_unit, allocation, reserve)) = frozen {
            if old_unit != unit {
                return Ok(blocked("quota_unit_changed"));
            }
            (allocation, reserve)
        } else {
            let allocation = usable.saturating_sub(global_reserved).max(0) / 10;
            let finish = p["finishing_estimate_milli"][unit].as_i64().unwrap_or(0);
            (allocation, (allocation / 5).max(finish))
        };
        let own_reserved: i64 = tx.query_row(
            "SELECT COALESCE(SUM(amount_milli),0) FROM swarm_reservations WHERE run_id=?1 AND pool_id=?2 AND window_id=?3 AND status IN ('active','uncertain')",
            params![run,pool,window_id], |r|r.get(0),
        )?;
        let available = if p["purpose"] == "finishing" {
            allocation.saturating_sub(own_reserved)
        } else {
            allocation
                .saturating_sub(own_reserved)
                .saturating_sub(reserve)
        };
        if estimate > available {
            return Ok(blocked("finishing_reserve"));
        }
        chosen.push((
            pool.to_string(),
            window_id.to_string(),
            unit.to_string(),
            allocation,
            reserve,
            estimate,
        ));
    }
    let attempt_id = format!("att-{}", &uuid::Uuid::new_v4().simple().to_string()[..16]);
    let token = uuid::Uuid::new_v4().simple().to_string();
    tx.execute("INSERT INTO swarm_attempts(id,run_id,job_id,revision,token_sha256,status,created_ms) VALUES(?1,?2,?3,?4,?5,'registered',?6)",
        params![attempt_id,run,job,job_revision,hash(&token),now])?;
    for (pool, window_id, unit, allocation, reserve, estimate) in &chosen {
        tx.execute("INSERT OR IGNORE INTO swarm_allocations(run_id,pool_id,window_id,unit,allocation_milli,reserve_milli,created_ms) VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![run,pool,window_id,unit,allocation,reserve,now])?;
        tx.execute("INSERT INTO swarm_reservations(attempt_id,run_id,pool_id,window_id,unit,amount_milli,status,created_ms) VALUES(?1,?2,?3,?4,?5,?6,'active',?7)",
            params![attempt_id,run,pool,window_id,unit,estimate,now])?;
    }
    tx.execute("INSERT INTO swarm_admissions(run_id,request_id,request_sha256,job_id,attempt_id,target_id,created_ms) VALUES(?1,?2,?3,?4,?5,?6,?7)",
        params![run,request_id,request_hash,job,attempt_id,target,now])?;
    tx.execute("UPDATE swarm_jobs SET attempt_count=attempt_count+1,status='reserved',updated_ms=?3 WHERE run_id=?1 AND id=?2",params![run,job,now])?;
    tx.execute(
        "UPDATE swarm_runs SET status='running',updated_ms=?2 WHERE id=?1 AND status='planning'",
        params![run, now],
    )?;
    tx.execute("INSERT INTO swarm_growth(run_id,wave_start_ms,admitted_count) VALUES(?1,?2,1) ON CONFLICT(run_id) DO UPDATE SET wave_start_ms=excluded.wave_start_ms,admitted_count=?3",
        params![run,wave_start,count+1])?;
    tx.commit()?;
    Ok(
        json!({"status":"admitted","attempt_id":attempt_id,"token":token,"target_id":target,
        "allocation_milli":chosen.first().map(|w|w.3),"reservation_windows":chosen.len()}),
    )
}
