//! Fixture admission transaction. A live call must use daemon-owned Auto Mode telemetry.

use super::{get, policy, required};
use crate::store::Store;
use anyhow::{anyhow, bail, Result};
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, HashSet};

fn hash(raw: &str) -> String {
    format!("{:x}", Sha256::digest(raw.as_bytes()))
}

fn blocked(reason: &str) -> Value {
    json!({"status":"blocked","reason":reason})
}

/// Freeze every currently comparable approved pool with the first successful
/// admission. Later observations may reduce headroom, never create a new
/// allocation from a newly selected account.
pub(super) fn freeze_pool_caps(
    conn: &rusqlite::Connection, run: &str, snapshot: &Value,
    allowed: &Value, effective: &Value, now: i64,
) -> Result<()> {
    let existing:i64=conn.query_row("SELECT COUNT(*) FROM swarm_pool_caps WHERE run_id=?1",
        [run],|row|row.get(0))?;
    if existing>0 || snapshot["expires_ms"].as_i64().is_none_or(|expires|expires<=now) {
        return Ok(());
    }
    let allowed_ids:HashSet<&str>=allowed.as_array().into_iter().flatten()
        .filter_map(Value::as_str).collect();
    let mut pool_ids=BTreeSet::new();
    for target in snapshot["targets"].as_array().into_iter().flatten() {
        if target["id"].as_str().is_some_and(|id|allowed_ids.contains(id)) {
            for id in target["pool_ids"].as_array().into_iter().flatten()
                .filter_map(Value::as_str) { pool_ids.insert(id); }
        }
    }
    let percent=effective["run_allocation_percent"].as_i64().unwrap_or(10);
    let allow_estimated=effective["allow_estimated_quota"].as_bool().unwrap_or(false);
    for pool_id in pool_ids {
        let Some(pool)=snapshot["pools"].as_array().into_iter().flatten()
            .find(|pool|pool["id"]==pool_id) else {continue};
        for window in pool["windows"].as_array().into_iter().flatten() {
            let (Some(window_id),Some(unit),Some(remaining))=(window["id"].as_str(),
                window["unit"].as_str(),window["remaining_milli"].as_i64()) else {continue};
            if window["expires_ms"].as_i64().is_none_or(|expires|expires<=now)
                || window["confidence"]=="unknown"
                || (window["confidence"]=="estimated" && !allow_estimated) {continue;}
            let reserved:i64=conn.query_row("SELECT COALESCE(SUM(amount_milli),0)
                FROM swarm_reservations WHERE pool_id=?1 AND window_id=?2
                  AND status IN ('active','uncertain')",params![pool_id,window_id],
                |row|row.get(0))?;
            let usable=remaining.saturating_sub(window["protected_milli"].as_i64().unwrap_or(0))
                .saturating_sub(window["reserved_milli"].as_i64().unwrap_or(0))
                .saturating_sub(reserved).max(0);
            let cap=usable.saturating_mul(percent)/100;
            conn.execute("INSERT OR IGNORE INTO swarm_pool_caps
                (run_id,pool_id,window_id,unit,allocation_milli,created_ms)
                VALUES(?1,?2,?3,?4,?5,?6)",params![run,pool_id,window_id,unit,cap,now])?;
        }
    }
    Ok(())
}

fn record_observation(store: &Store, p: &Value, result: &Value) {
    let Some(status @ ("blocked" | "admitted")) = result["status"].as_str() else { return };
    let (Some(run),Some(job),Some(target)) =
        (p["run_id"].as_str(),p["job_id"].as_str(),p["target_id"].as_str()) else { return };
    if let Err(error)=store.conn.execute(
        "INSERT INTO swarm_admission_observations(run_id,job_id,target_id,status,reason,observed_ms)
         VALUES(?1,?2,?3,?4,?5,?6)
         ON CONFLICT(run_id) DO UPDATE SET job_id=excluded.job_id,target_id=excluded.target_id,
             status=excluded.status,reason=excluded.reason,observed_ms=excluded.observed_ms",
        params![run,job,target,status,result["reason"].as_str(),crate::daemon::now()],
    ) {
        // Admission may already be committed. An optional status readout must
        // never turn a successful reservation into an apparent launch failure.
        crate::log(&format!("swarm admission readout could not be recorded: {error}"));
    }
}

pub(super) struct ScheduledCommit<'a> {
    pub request_id: &'a str,
    pub request_sha256: &'a str,
    pub category_key: &'a str,
}

pub fn admit(store: &mut Store, p: &Value) -> Result<Value> {
    let run = required(p,"run_id")?;
    super::owner::require(store,run,p)?;
    let result=admit_inner(store, p, None)?;
    record_observation(store,p,&result);
    Ok(result)
}

pub(super) fn admit_scheduled(
    store: &mut Store,
    p: &Value,
    commit: ScheduledCommit<'_>,
) -> Result<Value> {
    let result=admit_inner(store, p, Some(commit))?;
    record_observation(store,p,&result);
    Ok(result)
}

fn admit_inner(
    store: &mut Store,
    p: &Value,
    scheduled: Option<ScheduledCommit<'_>>,
) -> Result<Value> {
    let run = required(p, "run_id")?;
    let job = required(p, "job_id")?;
    let target = required(p, "target_id")?;
    let director_self = p["purpose"] == "director_self";
    if director_self {
        // A director-executed job is the one-slot serial path, not a way for
        // the scheduler to create an uncounted worker process.
        if scheduled.is_some() {
            bail!("scheduled admission cannot execute inside the director");
        }
        super::owner::require(store,run,p)?;
    }
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
    let job_duration = match p.get("job_deadline_ms") {
        Some(value) => value
            .as_i64()
            .ok_or_else(|| anyhow!("invalid job deadline"))?,
        None => 900_000,
    };
    if !(1..=86_400_000).contains(&job_duration) {
        bail!("invalid job deadline");
    }
    let mut resource_claims = Vec::new();
    let mut seen_resources = HashSet::new();
    if let Some(value) = p.get("resource_claims") {
        let claims = value
            .as_array()
            .ok_or_else(|| anyhow!("resource_claims must be an array"))?;
        if claims.len() > 32 {
            bail!("too many resource claims");
        }
        for claim in claims {
            let resource = required(claim, "resource")?.trim();
            let mode = required(claim, "mode")?;
            if resource.is_empty()
                || resource.len() > 512
                || resource.chars().any(char::is_control)
                || !seen_resources.insert(resource)
            {
                bail!("invalid or duplicate resource claim");
            }
            if mode != "read" && mode != "write" {
                bail!("resource claim mode must be read or write");
            }
            resource_claims.push((resource.to_string(), mode.to_string()));
        }
    }
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
            super::stop_for_deadline(
                store,
                &json!({"run_id":run,"generation":generation,"revision":revision}),
            )?;
        }
        return Ok(blocked("run_deadline"));
    }
    // IMMEDIATE: the app-slot count and the shared booking are read and
    // written under one write lock, so no other admission can interleave.
    let tx = store.conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
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
    // The owner changed the requirements: nothing new starts under the old plan
    // until the director has revised it against them (SWARM-21).
    if super::requirements::pending(&tx, run)? {
        return Ok(blocked("requirements_pending"));
    }
    let observed_availability: Option<(String, String)> = tx
        .query_row(
            "SELECT state,snapshot_sha256 FROM swarm_availability WHERE run_id=?1",
            params![run],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    if let Some((state, snapshot_sha256)) = observed_availability {
        if state == "blocked" {
            return Ok(blocked("run_availability_blocked"));
        }
        if snapshot_sha256.is_empty() {
            return Ok(blocked("snapshot_unknown"));
        }
        if snapshot_sha256 != hash(&p["snapshot"].to_string()) {
            return Ok(blocked("snapshot_superseded"));
        }
    }
    let job_info: Option<(String, i64, i64, Option<i64>, String, String)> = tx
        .query_row(
            "SELECT status,plan_revision,attempt_count,deadline_at_ms,required_capabilities,budget_role FROM swarm_jobs WHERE run_id=?1 AND id=?2",
            params![run, job],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
        )
        .optional()?;
    let Some((job_status, job_revision, attempts, old_job_deadline, planned_capabilities, budget_role)) = job_info else {
        return Ok(blocked("unknown_job"));
    };
    if p["purpose"] == "finishing" && budget_role != "finishing" {
        return Ok(blocked("finishing_purpose_not_authorized"));
    }
    let budget_purpose = if budget_role == "finishing" { "finishing" }
        else if director_self { "director_self" } else { "worker" };
    let mut required_capabilities: Vec<String> = serde_json::from_value(p["required_capabilities"].clone())?;
    required_capabilities.extend(serde_json::from_str::<Vec<String>>(&planned_capabilities)?);
    required_capabilities.sort();
    required_capabilities.dedup();
    let effective = &current["policy"]["effective"];
    let request = json!({
        "now_ms":now,
        "allowed_targets":current["allowed_targets"],
        "required_capabilities":required_capabilities,
        "purpose":budget_purpose,
        "estimate_milli":p["estimate_milli"],
        "finishing_estimate_milli":p.get("finishing_estimate_milli").cloned().unwrap_or(json!({})),
        "allow_estimated":effective["allow_estimated_quota"].as_bool().unwrap_or(false),
        "allocation_percent":effective["run_allocation_percent"],
        "finishing_reserve_percent":effective["finishing_reserve_percent"],
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
    if !director_self && !crate::adapters::swarm_worker_launch_supported(
        candidate["harness"].as_str().unwrap_or(""),
    ) {
        return Ok(blocked("uncontrolled_native_delegation"));
    }
    // Fixture processes are isolated test inputs. No native Swarm execution
    // path has yet proved an audit-only source-write boundary, even when it can
    // disable native subagents. Do not reserve quota for one in an audit run.
    if current["source_change_permission"] == "none" && candidate["harness"] != "generic"
    {
        return Ok(blocked("audit_source_boundary_unqualified"));
    }
    if super::context::revoked_dependency(&tx, run, job, target)? {
        return Ok(blocked("artifact_permission_revoked"));
    }
    // A failed worker's checkpoint is useful only if this destination can read
    // it. Keep the job ready while the director grants access or chooses a
    // different allowed target; never launch a replacement with missing context.
    let checkpoint_needs_grant = tx
        .prepare(
            "SELECT 1 FROM swarm_artifacts a
         JOIN swarm_attempts prior ON prior.id=a.attempt_id AND prior.status='finished'
         JOIN swarm_admissions source ON source.run_id=a.run_id AND source.attempt_id=a.attempt_id
         WHERE a.run_id=?1 AND a.job_id=?2 AND a.kind='checkpoint'
           AND a.source_revision=?3 AND source.target_id<>?4
           AND NOT EXISTS (SELECT 1 FROM swarm_artifact_grants g
                           WHERE g.run_id=a.run_id AND g.artifact_id=a.id AND g.target_id=?4)
         LIMIT 1",
        )?
        .exists(params![run, job, job_revision, target])?;
    if checkpoint_needs_grant {
        return Ok(blocked("checkpoint_permission_required"));
    }
    let job_deadline = old_job_deadline.unwrap_or_else(|| {
        now.saturating_add(job_duration)
            .min(created.saturating_add(deadline))
    });
    if now >= job_deadline {
        return Ok(blocked("job_deadline"));
    }
    if job_status == "ready" || job_status == "planned" {
        let raw: String = tx.query_row(
            "SELECT deps FROM swarm_jobs WHERE run_id=?1 AND id=?2",
            params![run,job], |row| row.get(0),
        )?;
        let deps: Vec<String> = serde_json::from_str(&raw)?;
        let mut waiting_on = Vec::new();
        for dep in deps {
            if !super::artifacts::dep_satisfied(&tx,run,&dep)? {
                waiting_on.push(dep);
            }
        }
        if !waiting_on.is_empty() {
            return Ok(json!({"status":"blocked","reason":"dependency_pending",
                "waiting_on":waiting_on}));
        }
    }
    if job_status != "ready" {
        return Ok(blocked("job_not_ready"));
    }
    let planned_claims: String = tx.query_row(
        "SELECT resource_claims FROM swarm_jobs WHERE run_id=?1 AND id=?2",
        params![run, job],
        |r| r.get(0),
    )?;
    let planned_claims: Vec<super::plan::ResourceClaim> = serde_json::from_str(&planned_claims)?;
    for claim in planned_claims {
        if let Some((_, mode)) = resource_claims
            .iter()
            .find(|(resource, _)| resource == &claim.resource)
        {
            if mode != &claim.mode {
                bail!("admission cannot change a planned resource claim");
            }
        } else {
            resource_claims.push((claim.resource, claim.mode));
        }
    }
    if resource_claims.len() > 32 {
        bail!("too many resource claims");
    }
    let unsafe_effects: i64 = tx.query_row(
        "SELECT COUNT(*) FROM swarm_effects WHERE run_id=?1 AND job_id=?2 AND outcome IN ('unknown','applied')",
        params![run,job], |r| r.get(0),
    )?;
    if unsafe_effects > 0 {
        return Ok(blocked("side_effect_unreconciled"));
    }
    if attempts >= effective["max_attempts"].as_i64().unwrap_or(2).min(2) {
        return Ok(blocked("attempt_limit"));
    }
    // An independent reproducer has its own budget inside the run's allocation (SWARM-45).
    if let Some(reason) = super::findings::over_budget(&tx, run, job, p)? {
        return Ok(blocked(reason));
    }
    for (resource, mode) in &resource_claims {
        let mut stmt = tx.prepare(
            "SELECT run_id,job_id,mode FROM swarm_claims WHERE resource=?1 AND status='active'",
        )?;
        let owners = stmt
            .query_map(params![resource], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for (owner_run, owner_job, owner_mode) in owners {
            if owner_run == run && owner_job == job {
                if owner_mode != *mode {
                    return Ok(blocked("resource_conflict"));
                }
            } else if mode == "write" || owner_mode == "write" {
                return Ok(blocked("resource_conflict"));
            }
        }
        // The same ledger holds ordinary agents' areas (SWARM-44): a job whose exclusive
        // claim falls inside one waits; its director decides what to do about it.
        if mode == "write" {
            let held = crate::claims::agent_holders(&tx, run, resource)?;
            if !held.is_empty() {
                return Ok(json!({"status":"blocked","reason":"resource_conflict",
                    "holders":held.iter().map(crate::claims::Holder::json).collect::<Vec<_>>()}));
            }
        }
    }
    let queued_inbox: i64 = tx.query_row(
        "SELECT COUNT(*) FROM swarm_messages WHERE run_id=?1 AND recipient='director' AND phase='queued'",
        params![run],
        |r| r.get(0),
    )?;
    if queued_inbox >= 1000 {
        return Ok(blocked("director_inbox_full"));
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
    // A planning run reserves a director slot until its supervised director
    // exists. Once launched, that process already appears in app_active.
    let active_director_process = tx.prepare(
        "SELECT 1 FROM swarm_director_owners o JOIN runs r ON r.id=o.overseer_run_id
         WHERE o.run_id=?1 AND o.generation=?2 AND o.status='active'
         AND o.supervised_launch=1 AND r.parent_run_id IS NULL
         AND r.status IN ('queued','starting','running','waiting_for_user')",
    )?.exists(params![run,generation])?;
    let new_director = if current["status"] == "planning" && !active_director_process {
        1
    } else { 0 };
    // The one app-slot count (`account_booking::app_slots_in_use`), read in
    // this admission transaction: ordinary, Auto, booked and Swarm starts all
    // consult it, and in-flight ordinary starts hold durable rows in it.
    let now_ms = crate::daemon::now();
    crate::account_booking::release_orphaned_swarm_bookings_in_tx(&tx, now_ms)?;
    let app_active = crate::account_booking::app_slots_in_use(&tx)?;
    let app_limit = crate::account_booking::app_slot_limit(&tx)?;
    let director_process_id = if director_self {
        if app_limit != 1 || app_active != 1 {
            return Ok(blocked("director_self_requires_one_slot"));
        }
        let linked: Option<(String,String)> = tx.query_row(
            "SELECT r.id,r.harness FROM swarm_director_owners o JOIN runs r ON r.id=o.overseer_run_id
             WHERE o.run_id=?1 AND o.generation=?2 AND o.status='active'
             AND o.supervised_launch=1 AND r.run_dir IS NOT NULL
             AND r.status IN ('queued','starting','running','waiting_for_user')
             AND r.ended_ms IS NULL",
            params![run,generation], |r| Ok((r.get(0)?,r.get(1)?)),
        ).optional()?;
        if linked.as_ref().map(|(_,harness)| harness.as_str()) != candidate["harness"].as_str() {
            return Ok(blocked("director_route_not_linked"));
        }
        linked.map(|(id,_)|id)
    } else if app_active + new_director >= app_limit {
        return Ok(blocked("global_agent_limit"));
    } else { None };
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
    // An account target (a native harness on a profile) draws account
    // allowance: the shared booking is its only account decision, and Swarm
    // keeps its category allocation in the booking's own windows below. A
    // generic fixture target draws no account: its snapshot pools are
    // fixture policy, reserved in swarm_reservations as before.
    let account_target = !director_self && candidate["harness"] != "generic";
    let booking_input = if account_target { shared_booking_input(p)? } else { None };
    let empty = Vec::new();
    let windows = if account_target { &empty } else {
        freeze_pool_caps(&tx,run,&p["snapshot"],&current["allowed_targets"],effective,now)?;
        let windows = candidate["windows"]
            .as_array()
            .ok_or_else(|| anyhow!("qualified target has no quota windows"))?;
        if windows.is_empty() {
            return Ok(blocked("unknown_quota"));
        }
        windows
    };
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
        let cap:Option<(String,i64)>=tx.query_row(
            "SELECT unit,allocation_milli FROM swarm_pool_caps
             WHERE run_id=?1 AND pool_id=?2 AND window_id=?3",
            params![run,pool,window_id],|r|Ok((r.get(0)?,r.get(1)?)),
        ).optional()?;
        // A quota reset may rename the window. Carry forward this pool's
        // original ceiling, limited further by the new window's headroom.
        let cap=if cap.is_some() {cap} else {
            let prior:Option<i64>=tx.query_row(
                "SELECT MIN(allocation_milli) FROM swarm_pool_caps
                 WHERE run_id=?1 AND pool_id=?2 AND unit=?3",
                params![run,pool,unit],|r|r.get(0))?;
            prior.map(|ceiling| {
                let fresh=usable.saturating_sub(global_reserved).max(0)
                    .saturating_mul(effective["run_allocation_percent"].as_i64().unwrap_or(10))/100;
                (unit.to_string(),ceiling.min(fresh))
            })
        };
        let Some((cap_unit,run_cap))=cap else {return Ok(blocked("allocation_not_frozen"));};
        if cap_unit!=unit {return Ok(blocked("quota_unit_changed"));}
        let frozen: Option<(String,i64,i64)> = tx.query_row(
            "SELECT unit,allocation_milli,reserve_milli FROM swarm_allocations WHERE run_id=?1 AND pool_id=?2 AND window_id=?3",
            params![run,pool,window_id], |r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)),
        ).optional()?;
        let prior_cap: Option<i64> = tx.query_row(
            "SELECT MIN(allocation_milli) FROM swarm_allocations
             WHERE run_id=?1 AND pool_id=?2 AND unit=?3",
            params![run, pool, unit],
            |r| r.get(0),
        )?;
        let (allocation, reserve) = if let Some((old_unit, allocation, reserve)) = frozen {
            if old_unit != unit {
                return Ok(blocked("quota_unit_changed"));
            }
            (allocation, reserve)
        } else {
            let allocation = prior_cap.map_or(run_cap, |cap| run_cap.min(cap));
            let finish = p["finishing_estimate_milli"][unit].as_i64().unwrap_or(0);
            let minimum_reserve = allocation.saturating_mul(
                effective["finishing_reserve_percent"]
                    .as_i64()
                    .unwrap_or(20),
            ) / 100;
            (allocation, minimum_reserve.max(finish))
        };
        let own_reserved: i64 = tx.query_row(
            "SELECT COALESCE(SUM(amount_milli),0) FROM (
                SELECT attempt_id,MAX(amount_milli) AS amount_milli
                FROM swarm_reservations
                WHERE run_id=?1 AND pool_id=?2 AND unit=?3
                  AND status IN ('active','uncertain')
                GROUP BY attempt_id)",
            params![run, pool, unit],
            |r| r.get(0),
        )?;
        let available = if budget_role == "finishing" {
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
    let benefit: Option<(i64, String, i64, String, String)> = tx.query_row(
        "SELECT wave,decision,max_parallel_workers,job_ids,estimate_json FROM swarm_benefit_decisions
         WHERE run_id=?1 AND revision=?2 ORDER BY wave DESC LIMIT 1",
        params![run,revision],
        |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?)),
    ).optional()?;
    let mut benefit_assignment = None;
    if let Some((wave, decision, cap, jobs, estimate_json)) = benefit {
        let jobs: Vec<String> = serde_json::from_str(&jobs)?;
        if !jobs.iter().any(|id| id == job) {
            return Ok(blocked("benefit_job_not_estimated"));
        }
        if decision == "blocked" {
            return Ok(blocked("benefit_no_affordable_plan"));
        }
        let serial_in_director = director_self && decision == "serial"
            && cap == 0 && workers == 0;
        if workers >= cap && !serial_in_director {
            return Ok(blocked(if decision == "serial" {
                "benefit_serial"
            } else {
                "benefit_batch_full"
            }));
        }
        let estimate: Value = serde_json::from_str(&estimate_json)?;
        let worker = estimate[if decision == "parallel" {
            "parallel"
        } else {
            "serial"
        }]["workers"]
            .as_array()
            .and_then(|workers| workers.iter().find(|worker| worker["id"] == job))
            .ok_or_else(|| anyhow!("committed benefit job estimate missing"))?;
        benefit_assignment = Some((
            wave,
            worker["elapsed_ms"]
                .as_i64()
                .ok_or_else(|| anyhow!("committed elapsed estimate missing"))?,
            worker["usage_milli"].to_string(),
        ));
    } else if workers > 0 {
        return Ok(blocked("benefit_unproven"));
    }
    let attempt_id = format!("att-{}", &uuid::Uuid::new_v4().simple().to_string()[..16]);
    let token = uuid::Uuid::new_v4().simple().to_string();
    let mut booked_windows = Vec::new();
    if account_target {
        let profile = candidate["profile_id"].as_str()
            .ok_or_else(|| anyhow!("account target has no profile"))?;
        // The worker's upper draw: a fixture caller's, or the qualified draw
        // learned from this account's observations around isolated runs of
        // the same harness, model and effort (`upper_draw`). The qualified
        // draw cites the account's latest observation and current identity.
        let bucket = crate::upper_draw::DrawBucket::agent(
            candidate["harness"].as_str().unwrap_or_default(),
            candidate["model"].as_str(), candidate["effort"].as_str());
        let (quota_profile, account_generation, quota_event_seq, draw) = match &booking_input {
            Some(input) => (input.quota_profile_id.as_deref().unwrap_or(profile),
                input.account_generation, input.quota_event_seq,
                crate::account_booking::BookingDraw::Fixture(&input.upper_draw_milli)),
            None => {
                let generation: Option<i64> = tx.query_row(
                    "SELECT generation FROM auto_account_identity WHERE profile_id=?1",
                    [profile], |r| r.get(0)).optional()?;
                let latest: Option<i64> = tx.query_row(
                    "SELECT event_seq FROM auto_quota_observations WHERE pool_id=?1
                     ORDER BY observed_ms DESC,event_seq DESC LIMIT 1",
                    [profile], |r| r.get(0)).optional()?;
                let (Some(generation), Some(latest)) = (generation, latest) else {
                    return Ok(blocked("upper_draw_unknown"));
                };
                (profile, generation, latest, crate::account_booking::BookingDraw::Qualified(&bucket))
            }
        };
        // Swarm's category allocation and finishing reserve, in the cited
        // observation's windows, in thousandths of a reported percentage
        // point. Nothing is converted from tokens, credits or fixture units.
        let cited = crate::account_booking::cited_windows_in_tx(&tx, quota_profile, quota_event_seq)?;
        let draw_windows = match draw {
            crate::account_booking::BookingDraw::Fixture(draws) => Some(draws.len()),
            crate::account_booking::BookingDraw::Qualified(_) => cited.as_ref().map(Vec::len),
        };
        let allocation = match &cited {
            Some(windows) if Some(windows.len()) == draw_windows =>
                Some(category_allocation(&tx, run, windows, effective, budget_role == "finishing")?),
            _ => None,
        };
        let booking_id = crate::account_booking::swarm_attempt_booking_id(&attempt_id);
        let remaining: Option<Vec<i64>> = allocation.as_ref()
            .map(|windows| windows.iter().map(|w| w.remaining).collect());
        let account = crate::account_booking::AccountBookingRequest {
            id: &booking_id, request_hash: &request_hash, caller: "swarm",
            route_id: target, profile_id: profile, quota_profile_id: quota_profile,
            account_generation, quota_event_seq, now_ms, draw,
            allocation_remaining_milli: remaining.as_deref(),
        };
        let decision = crate::account_booking::book_shared_launch_in_tx(&tx,
            &crate::account_booking::LaunchBookingRequest {
                account: &account, workspace_path: None, consume_agent_slot: true,
                launch_hash: &request_hash,
            })?;
        match decision {
            crate::account_booking::LaunchBookingDecision::Booked(_) => {}
            crate::account_booking::LaunchBookingDecision::Blocked("upper_draw_unknown")
                if booking_input.is_none() => {
                // Show why: too few attributable samples, or no identity.
                let summary = crate::upper_draw::qualified_upper_draw_in_tx(
                    &tx, quota_profile, quota_event_seq, &bucket, now_ms)?.summary();
                return Ok(json!({"status":"blocked","reason":"upper_draw_unknown","draw":summary}));
            }
            crate::account_booking::LaunchBookingDecision::Blocked(reason) => return Ok(blocked(reason)),
            crate::account_booking::LaunchBookingDecision::Replayed(_) =>
                bail!("a new Swarm attempt cannot replay an earlier booking"),
        }
        booked_windows = allocation.unwrap_or_default();
    }
    tx.execute("INSERT INTO swarm_attempts(id,run_id,job_id,revision,token_sha256,status,executor,executor_run_id,created_ms) VALUES(?1,?2,?3,?4,?5,'registered',?6,?7,?8)",
        params![attempt_id,run,job,job_revision,hash(&token),
            if director_self { "director" } else { "worker" },director_process_id,now])?;
    super::findings::charge(&tx, &attempt_id, run, job, p)?;
    if let Some((wave, estimate_elapsed_ms, estimate_usage_milli)) = benefit_assignment {
        tx.execute("INSERT INTO swarm_benefit_attempt_outcomes(attempt_id,run_id,revision,wave,job_id,estimate_elapsed_ms,estimate_usage_milli)
            VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![attempt_id,run,revision,wave,job,estimate_elapsed_ms,estimate_usage_milli])?;
    }
    for (resource, mode) in &resource_claims {
        tx.execute(
            "INSERT INTO swarm_claims(resource,run_id,job_id,mode,status,revision,created_ms,updated_ms)
             VALUES(?1,?2,?3,?4,'active',?5,?6,?6)
             ON CONFLICT(resource,run_id,job_id) DO UPDATE SET
             mode=excluded.mode,status='active',revision=excluded.revision,updated_ms=excluded.updated_ms",
            params![resource, run, job, mode, job_revision, now],
        )?;
    }
    for window in &booked_windows {
        tx.execute("INSERT OR IGNORE INTO swarm_pool_caps(run_id,pool_id,window_id,unit,allocation_milli,created_ms)
            VALUES(?1,?2,?3,'percent_milli',?4,?5)",params![run,window.pool,window.key,window.cap,now])?;
        tx.execute("INSERT OR IGNORE INTO swarm_allocations(run_id,pool_id,window_id,unit,allocation_milli,reserve_milli,created_ms)
            VALUES(?1,?2,?3,'percent_milli',?4,?5,?6)",params![run,window.pool,window.key,window.cap,window.reserve,now])?;
    }
    for (pool, window_id, unit, allocation, reserve, estimate) in &chosen {
        tx.execute("INSERT OR IGNORE INTO swarm_pool_caps(run_id,pool_id,window_id,unit,allocation_milli,created_ms)
            VALUES(?1,?2,?3,?4,?5,?6)",params![run,pool,window_id,unit,allocation,now])?;
        tx.execute("INSERT OR IGNORE INTO swarm_allocations(run_id,pool_id,window_id,unit,allocation_milli,reserve_milli,created_ms) VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![run,pool,window_id,unit,allocation,reserve,now])?;
        tx.execute("INSERT INTO swarm_reservations(attempt_id,run_id,pool_id,window_id,unit,amount_milli,status,created_ms) VALUES(?1,?2,?3,?4,?5,?6,'active',?7)",
            params![attempt_id,run,pool,window_id,unit,estimate,now])?;
    }
    let target_harness = candidate["harness"]
        .as_str()
        .ok_or_else(|| anyhow!("qualified target has no harness"))?;
    tx.execute("INSERT INTO swarm_admissions(run_id,request_id,request_sha256,job_id,attempt_id,target_id,target_harness,target_profile_id,target_model,target_effort,created_ms) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
        params![run,request_id,request_hash,job,attempt_id,target,target_harness,
            candidate["profile_id"].as_str(),candidate["model"].as_str(),
            candidate["effort"].as_str(),now])?;
    if let Some(commit) = scheduled {
        tx.execute("INSERT INTO swarm_scheduler_admissions(request_id,request_sha256,run_id,job_id,attempt_id,target_id,created_ms)
            VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![commit.request_id,commit.request_sha256,run,job,attempt_id,target,now])?;
        tx.execute(
            "INSERT INTO swarm_scheduler_cursor(id,last_category_key) VALUES(1,?1)
            ON CONFLICT(id) DO UPDATE SET last_category_key=excluded.last_category_key",
            params![commit.category_key],
        )?;
    }
    tx.execute(
        "UPDATE swarm_jobs SET attempt_count=attempt_count+1,status='reserved',
        deadline_at_ms=COALESCE(deadline_at_ms,?4),updated_ms=?3 WHERE run_id=?1 AND id=?2",
        params![run, job, now, job_deadline],
    )?;
    super::materialize_ready(&tx, run, now)?;
    tx.execute(
        "UPDATE swarm_runs SET status='running',updated_ms=?2 WHERE id=?1 AND status='planning'",
        params![run, now],
    )?;
    tx.execute("INSERT INTO swarm_growth(run_id,wave_start_ms,admitted_count) VALUES(?1,?2,1) ON CONFLICT(run_id) DO UPDATE SET wave_start_ms=excluded.wave_start_ms,admitted_count=?3",
        params![run,wave_start,count+1])?;
    tx.commit()?;
    Ok(
        json!({"status":"admitted","attempt_id":attempt_id,"token":token,"target_id":target,
        "executor":if director_self { "director" } else { "worker" },
        "allocation_milli":chosen.first().map(|w|w.3).or(booked_windows.first().map(|w|w.cap)),
        "reservation_windows":chosen.len(),
        "shared_booking":account_target.then(||
            crate::account_booking::swarm_attempt_booking_id(&attempt_id)),
        "booked_windows":booked_windows.len()}),
    )
}

/// The caller-supplied booking inputs for an account target: a fixture
/// draw. Without them the booking uses the qualified draw (`upper_draw`).
struct SharedBookingInput {
    account_generation: i64,
    quota_event_seq: i64,
    quota_profile_id: Option<String>,
    upper_draw_milli: Vec<i64>,
}

fn shared_booking_input(p: &Value) -> Result<Option<SharedBookingInput>> {
    let booking = match p.get("shared_booking") {
        None | Some(Value::Null) => return Ok(None),
        Some(booking) => booking,
    };
    if std::env::var("OVERSEER_SHARED_BOOKING_FIXTURE_API").as_deref() != Ok("1") {
        bail!("shared launch booking is not available to this caller");
    }
    let upper_draw_milli = booking["upper_draw_milli"].as_array()
        .filter(|draws| !draws.is_empty() && draws.len() <= 32)
        .and_then(|draws| draws.iter().map(Value::as_i64).collect::<Option<Vec<_>>>())
        .ok_or_else(|| anyhow!("shared booking needs an upper draw for every window"))?;
    Ok(Some(SharedBookingInput {
        account_generation: booking["account_generation"].as_i64()
            .ok_or_else(|| anyhow!("shared booking needs the account generation"))?,
        quota_event_seq: booking["quota_event_seq"].as_i64()
            .ok_or_else(|| anyhow!("shared booking needs the cited quota observation"))?,
        quota_profile_id: booking["quota_profile_id"].as_str().map(str::to_string),
        upper_draw_milli,
    }))
}

pub(super) struct BookedWindow {
    pool: String,
    key: String,
    cap: i64,
    reserve: i64,
    pub(super) remaining: i64,
}

/// This category's remaining allocation in each cited account window. The
/// cap is frozen at the category's first booking in a window (a share of the
/// reported remaining allowance) and never grows; a window with a new key
/// (a reset) starts from the tighter of its fresh share and the category's
/// earlier caps in the same account pool. The category's own active and
/// uncertain bookings count against it, and a booking in a window that is
/// no longer reported (from before a reset) counts against every current
/// window. A non-finishing job also leaves the finishing reserve.
pub(super) fn category_allocation(
    conn: &rusqlite::Connection,
    run: &str,
    windows: &[(String, String, i64)],
    effective: &Value,
    finishing: bool,
) -> Result<Vec<BookedWindow>> {
    let percent = effective["run_allocation_percent"].as_i64().unwrap_or(10);
    let reserve_percent = effective["finishing_reserve_percent"].as_i64().unwrap_or(20);
    let keys: Vec<&str> = windows.iter().map(|(_, key, _)| key.as_str()).collect();
    let keys_json = serde_json::to_string(&keys)?;
    let mut result = Vec::with_capacity(windows.len());
    for (pool, key, remaining) in windows {
        let frozen: Option<i64> = conn.query_row(
            "SELECT allocation_milli FROM swarm_pool_caps
             WHERE run_id=?1 AND pool_id=?2 AND window_id=?3 AND unit='percent_milli'",
            params![run, pool, key], |row| row.get(0)).optional()?;
        let cap = match frozen {
            Some(cap) => cap,
            None => {
                let fresh = remaining.max(&0).saturating_mul(percent) / 100;
                let prior: Option<i64> = conn.query_row(
                    "SELECT MIN(allocation_milli) FROM swarm_pool_caps
                     WHERE run_id=?1 AND pool_id=?2 AND unit='percent_milli'",
                    params![run, pool], |row| row.get(0))?;
                prior.map_or(fresh, |prior| prior.min(fresh))
            }
        };
        let own: i64 = conn.query_row(
            "SELECT COALESCE(SUM(w.amount_milli),0) FROM shared_booking_windows w
             JOIN auto_pool_claims c ON c.work_unit_id=w.work_unit_id
             JOIN swarm_attempts a ON 'swarm/'||a.id=w.work_unit_id
             WHERE a.run_id=?1 AND w.pool_id=?2 AND c.state IN ('active','uncertain')
               AND (w.window_key=?3 OR w.window_key NOT IN (SELECT value FROM json_each(?4)))",
            params![run, pool, key, keys_json], |row| row.get(0))?;
        let reserve = if finishing { 0 } else { cap.saturating_mul(reserve_percent) / 100 };
        result.push(BookedWindow {
            pool: pool.clone(), key: key.clone(), cap, reserve,
            remaining: cap.saturating_sub(own).saturating_sub(reserve).max(0),
        });
    }
    Ok(result)
}
