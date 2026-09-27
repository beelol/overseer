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
    #[serde(default = "default_fixture_harness")]
    harness: String,
    #[serde(default)]
    endpoint_id: Option<String>,
    #[serde(default = "default_health_scope")]
    health_scope: String,
    #[serde(default)]
    profile_id: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    effort: Option<String>,
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
fn default_fixture_harness() -> String { "generic".to_string() }
fn default_health_scope() -> String { "target".to_string() }

fn health_reason(health: &str) -> &'static str {
    match health {
        "rate_limited" => "rate_limited",
        "local_unavailable" => "local_harness_unavailable",
        _ => "target_unhealthy",
    }
}

fn scoped_failure<'a>(targets: &'a [Target], candidate: &Target) -> Option<&'a Target> {
    let mut matching: Vec<&Target> = targets.iter().filter(|source| source.health != "up"
        && source.id != candidate.id && match source.health_scope.as_str() {
            "account" => source.account_id == candidate.account_id,
            "endpoint" => source.endpoint_id.is_some() && source.endpoint_id == candidate.endpoint_id,
            "harness" => source.harness == candidate.harness,
            _ => false,
        }).collect();
    // A mixed failure snapshot must produce the same reason regardless of row order.
    matching.sort_by_key(|source| (match source.health.as_str() {
        "local_unavailable" => 0, "rate_limited" => 1, _ => 2,
    }, source.id.as_str()));
    matching.into_iter().next()
}

fn safe_identity(value: &str) -> bool {
    !value.is_empty() && value.len() <= 200 && !value.chars().any(char::is_control)
        && crate::redact::redact(value) == value
}

pub fn preview(p: &Value) -> Result<Value> {
    let snapshot: Snapshot = serde_json::from_value(p["snapshot"].clone())
        .map_err(|e| anyhow!("invalid target snapshot: {e}"))?;
    let request: Request = serde_json::from_value(p["request"].clone())
        .map_err(|e| anyhow!("invalid policy request: {e}"))?;
    if snapshot.version == 0 || snapshot.observed_ms > request.now_ms {
        bail!("invalid snapshot version or observation time");
    }
    if request.purpose != "worker" && request.purpose != "finishing"
        && request.purpose != "director_self" {
        bail!("purpose must be worker, director_self or finishing");
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
        if !safe_identity(&pool.id) || pools.insert(pool.id.as_str(), pool).is_some() {
            bail!("duplicate or invalid quota pool id");
        }
        let mut seen = HashSet::new();
        for window in &pool.windows {
            if !seen.insert(&window.id) || !safe_identity(&window.id)
                || !safe_identity(&window.unit) {
                bail!("duplicate or invalid quota window");
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
    let mut revoked_accounts = HashSet::new();
    let mut auth_failed_accounts = HashSet::new();
    for target in &snapshot.targets {
        if !safe_identity(&target.id)
            || !safe_identity(&target.account_id)
            || target.pool_ids.iter().any(|id| !safe_identity(id))
            || !target_ids.insert(target.id.as_str())
        {
            bail!("duplicate or invalid target identity");
        }
        if !["generic", "codex", "codex-app", "claude", "opencode"]
            .contains(&target.harness.as_str())
        {
            bail!("invalid target harness");
        }
        if !["target", "account", "endpoint", "harness"].contains(&target.health_scope.as_str())
            || (target.health_scope == "endpoint" && target.endpoint_id.is_none())
            || target.endpoint_id.as_deref().is_some_and(|id| !safe_identity(id)) {
            bail!("invalid target health scope or endpoint identity");
        }
        let route_values = [&target.profile_id, &target.model, &target.effort];
        if route_values.iter().filter_map(|value| value.as_deref()).any(|value| value.is_empty()
            || value.len() > 128 || value.chars().any(char::is_control)
            || crate::redact::redact(value) != value)
            || (target.harness == "generic" && route_values.iter().any(|value| value.is_some()))
            || (target.harness != "generic"
                && (target.profile_id.is_none() || target.model.is_none()))
        {
            bail!("incomplete or invalid target route");
        }
        if target.auth == "revoked" {
            revoked_accounts.insert(target.account_id.as_str());
        }
        if target.auth != "ok" {
            auth_failed_accounts.insert(target.account_id.as_str());
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
        } else if revoked_accounts.contains(target.account_id.as_str()) {
            reason = Some("auth_unavailable");
        } else if target.health != "up" {
            reason = Some(health_reason(&target.health));
        } else if auth_failed_accounts.contains(target.account_id.as_str()) {
            reason = Some("auth_unavailable");
        } else if let Some(source) = scoped_failure(&snapshot.targets, target) {
            reason = Some(health_reason(&source.health));
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
                    if usable == 0 {
                        reason = Some("quota_exhausted");
                        break;
                    }
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
            json!({"eligible":reason.is_none(),"reason":reason,"account_id":target.account_id,
                "harness":target.harness,"profile_id":target.profile_id,
                "model":target.model,"effort":target.effort,"windows":windows}),
        );
    }
    let mut target_json = Map::new();
    for (id, result) in results {
        target_json.insert(id, result);
    }
    Ok(json!({
        "snapshot_version":snapshot.version,
        "defaults":{"max_workers":8,"growth_per_wave":4,
            "growth_interval_ms":5000,"deadline_ms":3600000,"run_allocation_percent":10,
            "minimum_finishing_reserve_percent":20},
        "applied_percentages":{"run_allocation_percent":request.allocation_percent,
            "finishing_reserve_percent":request.finishing_reserve_percent},
        "targets":target_json,
        "authoritative_reservation":false
    }))
}
