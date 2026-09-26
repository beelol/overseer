//! Deterministic, read-only admission preview over an injected Auto Mode snapshot.
//! This never launches work or reserves usage; the shared transaction is a separate step.

use anyhow::{anyhow, bail, Result};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, HashMap, HashSet};

#[derive(Deserialize)]
struct Snapshot {
    version: u64,
    observed_ms: i64,
    expires_ms: i64,
    targets: Vec<Target>,
    pools: Vec<Pool>,
}

#[derive(Deserialize)]
struct Target {
    id: String,
    account_id: String,
    pool_ids: Vec<String>,
    capabilities: Vec<String>,
    health: String,
    auth: String,
}

#[derive(Deserialize)]
struct Pool {
    id: String,
    windows: Vec<Window>,
}

#[derive(Deserialize)]
struct Window {
    id: String,
    unit: String,
    remaining_milli: Option<i64>,
    protected_milli: i64,
    reserved_milli: i64,
    confidence: String,
    expires_ms: i64,
}

#[derive(Deserialize)]
struct Request {
    now_ms: i64,
    allowed_targets: Vec<String>,
    required_capabilities: Vec<String>,
    purpose: String,
    estimate_milli: HashMap<String, i64>,
    #[serde(default)]
    finishing_estimate_milli: HashMap<String, i64>,
    #[serde(default)]
    allow_estimated: bool,
    #[serde(default = "default_allocation_percent")]
    allocation_percent: i64,
    #[serde(default = "default_finishing_reserve_percent")]
    finishing_reserve_percent: i64,
}

fn default_allocation_percent() -> i64 { 10 }
fn default_finishing_reserve_percent() -> i64 { 20 }

pub fn preview(p: &Value) -> Result<Value> {
    let snapshot: Snapshot = serde_json::from_value(p["snapshot"].clone())
        .map_err(|e| anyhow!("invalid target snapshot: {e}"))?;
    let request: Request = serde_json::from_value(p["request"].clone())
        .map_err(|e| anyhow!("invalid policy request: {e}"))?;
    if snapshot.version == 0 || snapshot.observed_ms > request.now_ms {
        bail!("invalid snapshot version or observation time");
    }
    if request.purpose != "worker" && request.purpose != "finishing" {
        bail!("purpose must be worker or finishing");
    }
    if !(1..=100).contains(&request.allocation_percent)
        || !(1..=100).contains(&request.finishing_reserve_percent)
    {
        bail!("allocation and finishing reserve percentages must be 1-100");
    }
    if request.estimate_milli.values().any(|v| *v < 0)
        || request.finishing_estimate_milli.values().any(|v| *v < 0)
    {
        bail!("negative usage estimate");
    }
    let mut pools = HashMap::new();
    for pool in &snapshot.pools {
        if pool.id.is_empty() || pools.insert(pool.id.as_str(), pool).is_some() {
            bail!("duplicate or empty quota pool id");
        }
        let mut seen = HashSet::new();
        for window in &pool.windows {
            if !seen.insert(&window.id) || window.id.is_empty() || window.unit.is_empty() {
                bail!("duplicate or empty quota window");
            }
            if window.protected_milli < 0
                || window.reserved_milli < 0
                || window.remaining_milli.is_some_and(|v| v < 0)
            {
                bail!("negative quota amount");
            }
            if !["exact", "estimated", "unknown"].contains(&window.confidence.as_str()) {
                bail!("invalid quota confidence");
            }
        }
    }
    let mut target_ids = HashSet::new();
    let mut account_pools: HashMap<&str, (usize, HashSet<&str>)> = HashMap::new();
    for target in &snapshot.targets {
        if target.id.is_empty()
            || target.account_id.is_empty()
            || !target_ids.insert(target.id.as_str())
        {
            bail!("duplicate or missing target identity");
        }
        let declared: HashSet<&str> = target.pool_ids.iter().map(String::as_str).collect();
        if let Some((count, common)) = account_pools.get_mut(target.account_id.as_str()) {
            *count += 1;
            common.retain(|pool| declared.contains(pool));
        } else {
            account_pools.insert(target.account_id.as_str(), (1, declared));
        }
    }
    let allowed: HashSet<&str> = request.allowed_targets.iter().map(String::as_str).collect();
    let required: HashSet<&str> = request
        .required_capabilities
        .iter()
        .map(String::as_str)
        .collect();
    let mut results = BTreeMap::new();
    for target in &snapshot.targets {
        let mut windows = Vec::new();
        let mut reason: Option<&str> = None;
        if !allowed.contains(target.id.as_str()) {
            reason = Some("not_allowed");
        } else if snapshot.expires_ms <= request.now_ms {
            reason = Some("stale_snapshot");
        } else if target.health != "up" {
            reason = Some("target_unhealthy");
        } else if target.auth != "ok" {
            reason = Some("auth_unavailable");
        } else if !required
            .iter()
            .all(|cap| target.capabilities.iter().any(|c| c == cap))
        {
            reason = Some("missing_capability");
        } else if account_pools
            .get(target.account_id.as_str())
            .is_some_and(|(count, common)| *count > 1 && common.is_empty())
        {
            reason = Some("account_pool_conflict");
        } else if target.pool_ids.is_empty() {
            reason = Some("unknown_quota_pool");
        } else {
            let mut pool_ids = target.pool_ids.clone();
            pool_ids.sort();
            pool_ids.dedup();
            for pool_id in pool_ids {
                let Some(pool) = pools.get(pool_id.as_str()) else {
                    reason = Some("unknown_quota_pool");
                    break;
                };
                if pool.windows.is_empty() {
                    reason = Some("unknown_quota");
                    break;
                }
                let mut sorted: Vec<&Window> = pool.windows.iter().collect();
                sorted.sort_by_key(|w| &w.id);
                for window in sorted {
                    if window.expires_ms <= request.now_ms {
                        reason = Some("stale_snapshot");
                        break;
                    }
                    let Some(remaining) = window.remaining_milli else {
                        reason = Some("unknown_quota");
                        break;
                    };
                    if window.confidence == "unknown" {
                        reason = Some("unknown_quota");
                        break;
                    }
                    if window.confidence == "estimated" && !request.allow_estimated {
                        reason = Some("estimated_quota_requires_permission");
                        break;
                    }
                    let Some(estimate) = request.estimate_milli.get(&window.unit) else {
                        reason = Some("missing_estimate");
                        break;
                    };
                    if *estimate == 0 {
                        reason = Some("uncalibrated_estimate");
                        break;
                    }
                    let usable = remaining
                        .saturating_sub(window.protected_milli)
                        .saturating_sub(window.reserved_milli)
                        .max(0);
                    let allocation = usable.saturating_mul(request.allocation_percent) / 100;
                    let minimum_reserve = allocation
                        .saturating_mul(request.finishing_reserve_percent) / 100;
                    let finishing = request
                        .finishing_estimate_milli
                        .get(&window.unit)
                        .copied()
                        .unwrap_or(0);
                    let reserve = minimum_reserve.max(finishing);
                    let available = if request.purpose == "finishing" {
                        allocation
                    } else {
                        allocation.saturating_sub(reserve)
                    };
                    windows.push(json!({"pool_id":pool_id,"window_id":window.id,"unit":window.unit,
                        "allocation_milli":allocation,"finishing_reserve_milli":reserve,
                        "available_milli":available,"usable_milli":usable,
                        "estimate_milli":estimate,"confidence":window.confidence}));
                    if *estimate > available {
                        reason = Some("finishing_reserve");
                        break;
                    }
                }
                if reason.is_some() {
                    break;
                }
            }
        }
        results.insert(
            target.id.clone(),
            json!({"eligible":reason.is_none(),"reason":reason,"account_id":target.account_id,"windows":windows}),
        );
    }
    let mut target_json = Map::new();
    for (id, result) in results {
        target_json.insert(id, result);
    }
    Ok(json!({
        "snapshot_version":snapshot.version,
        "defaults":{"max_workers":8,"max_executing":9,"growth_per_wave":4,
            "growth_interval_ms":5000,"deadline_ms":3600000,"run_allocation_percent":10,
            "minimum_finishing_reserve_percent":20},
        "applied_percentages":{"run_allocation_percent":request.allocation_percent,
            "finishing_reserve_percent":request.finishing_reserve_percent},
        "targets":target_json,
        "authoritative_reservation":false
    }))
}
