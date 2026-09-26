//! Provider-neutral eligibility and task-aware selection for Auto Mode.

use crate::auto_quota::{QuotaSnapshot, QuotaState};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityTier {
    General,
    Frontier,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Sandbox {
    ReadOnly,
    WorkspaceWrite,
    FullAccess,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Allowance {
    ObservedNonExhausted,
    Exhausted,
    Unknown,
    NotApplicable,
}

/// Normalized native account observations are the route's only allowance
/// claim. Expiry and reset fall back to unknown until a bounded refresh.
pub fn observed_allowance(snapshot: Option<&QuotaSnapshot>, model: &str, now_ms: i64) -> Allowance {
    match snapshot.map(|s| s.state_for(model, now_ms)) {
        Some(QuotaState::ObservedNonExhausted) => Allowance::ObservedNonExhausted,
        Some(QuotaState::Exhausted) => Allowance::Exhausted,
        Some(QuotaState::Unknown) | None => Allowance::Unknown,
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Health {
    Healthy,
    Degraded,
    Unavailable,
    Unknown,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Fit {
    Fits,
    Unknown,
    Unaffordable,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DrawSource {
    ProviderReported,
    AttributedActualWork,
}

/// A positive, uncertainty-bounded draw in the *same* quota window. This is
/// deliberately not a token count, API price, or cross-provider score.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct WindowDraw {
    pub bucket_id: String,
    pub window: String,
    pub upper_percent: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AllowanceEstimate {
    pub pool_id: String,
    pub model: String,
    pub effort: String,
    pub source: DrawSource,
    pub observed_ms: i64,
    pub windows: Vec<WindowDraw>,
}

/// Missing, stale, reset, or mismatched evidence never establishes fit.
/// `in_flight` must be the complete set of locally admitted work in this pool;
/// durable reservation and external-work uncertainty are separate admission
/// requirements before this can authorize a launch.
pub fn assess_fit(
    snapshot: &QuotaSnapshot,
    route: &Route,
    estimate: Option<&AllowanceEstimate>,
    in_flight: &[WindowDraw],
    now_ms: i64,
) -> Fit {
    match snapshot.state_for(&route.model, now_ms) {
        QuotaState::Exhausted => return Fit::Unaffordable,
        QuotaState::Unknown => return Fit::Unknown,
        QuotaState::ObservedNonExhausted => {}
    }
    let Some(estimate) = estimate else {
        return Fit::Unknown;
    };
    const MAX_ESTIMATE_AGE_MS: i64 = 30 * 86_400_000;
    if estimate.pool_id != route.pool_id
        || estimate.model != route.model
        || estimate.effort != route.effort
        || now_ms < estimate.observed_ms
        || now_ms.saturating_sub(estimate.observed_ms) >= MAX_ESTIMATE_AGE_MS
    {
        return Fit::Unknown;
    }
    let applicable = snapshot.applicable_to(&route.model);
    if applicable.is_empty() {
        return Fit::Unknown;
    }
    let mut unaffordable = false;
    let mut unknown = false;
    for window in applicable {
        let matching: Vec<_> = estimate
            .windows
            .iter()
            .filter(|d| d.bucket_id == window.bucket_id && d.window == window.window)
            .collect();
        if matching.len() != 1
            || !matching[0].upper_percent.is_finite()
            || matching[0].upper_percent <= 0.0
            || matching[0].upper_percent > 100.0
        {
            unknown = true;
            continue;
        }
        let mut committed = 0.0;
        let mut commitment_unknown = false;
        for draw in in_flight
            .iter()
            .filter(|d| d.bucket_id == window.bucket_id && d.window == window.window)
        {
            if !draw.upper_percent.is_finite()
                || draw.upper_percent <= 0.0
                || draw.upper_percent > 100.0
            {
                commitment_unknown = true;
                continue;
            }
            committed += draw.upper_percent;
        }
        if matching[0].upper_percent > 100.0 - window.used_percent {
            unaffordable = true;
        }
        if commitment_unknown {
            unknown = true;
        } else if matching[0].upper_percent + committed > 100.0 - window.used_percent {
            unaffordable = true;
        }
    }
    if unaffordable {
        Fit::Unaffordable
    } else if unknown {
        Fit::Unknown
    } else {
        Fit::Fits
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Route {
    pub id: String,
    pub harness: String,
    pub provider: String,
    pub endpoint: String,
    pub profile_id: String,
    pub pool_id: String,
    pub model: String,
    pub effort: String,
    pub tier: CapabilityTier,
    pub tools: BTreeSet<String>,
    pub context_limit: Option<u64>,
    pub supports_approvals: bool,
    pub sandbox: Sandbox,
    pub recommended_default: bool,
    pub quota: Allowance,
    pub fit: Fit,
    pub health: Health,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct WorkUnit {
    pub id: String,
    pub min_tier: CapabilityTier,
    pub required_tools: BTreeSet<String>,
    pub context_needed: u64,
    pub requires_approvals: bool,
    pub min_sandbox: Sandbox,
    pub max_sandbox: Sandbox,
    pub allowed_profiles: BTreeSet<String>,
    pub pinned_route: Option<String>,
    pub preferred_harness: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Exclusion {
    pub route_id: String,
    pub reason: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Decision {
    pub work_unit_id: String,
    pub selected: Option<String>,
    pub exclusions: Vec<Exclusion>,
    pub reason: String,
}

fn excluded(work: &WorkUnit, route: &Route, exhausted_pools: &BTreeSet<&str>) -> Option<&'static str> {
    if work
        .pinned_route
        .as_deref()
        .is_some_and(|pin| pin != route.id)
    {
        return Some("pinned_elsewhere");
    }
    if !work.allowed_profiles.contains(&route.profile_id) {
        return Some("account_not_allowed");
    }
    if route.health == Health::Unavailable {
        return Some("route_unavailable");
    }
    if route.quota == Allowance::Exhausted || exhausted_pools.contains(route.pool_id.as_str()) {
        return Some("quota_exhausted");
    }
    if route.fit == Fit::Unaffordable {
        return Some("estimated_draw_exceeds_allowance");
    }
    if route.tier < work.min_tier {
        return Some("insufficient_capability");
    }
    if !work.required_tools.is_subset(&route.tools) {
        return Some("missing_tool");
    }
    if work.context_needed > 0 && route.context_limit.is_none_or(|limit| limit < work.context_needed) {
        return Some("context_unavailable");
    }
    if work.requires_approvals && !route.supports_approvals {
        return Some("approvals_unsupported");
    }
    if route.sandbox < work.min_sandbox || route.sandbox > work.max_sandbox {
        return Some("sandbox_incompatible");
    }
    None
}

/// Deterministic eligibility comes before ranking. Unknown allowance remains
/// eligible only as a disclosed cold-start possibility, never as free capacity.
pub fn select(work: &WorkUnit, routes: &[Route]) -> Decision {
    select_with_pool_blocks(work, routes, true)
}

/// Replay only: decisions recorded before shared-pool block propagation keep
/// their original selector semantics rather than changing under a new build.
pub fn select_legacy_v1(work: &WorkUnit, routes: &[Route]) -> Decision {
    select_with_pool_blocks(work, routes, false)
}

fn select_with_pool_blocks(work: &WorkUnit, routes: &[Route], propagate_pool_blocks: bool) -> Decision {
    let mut exclusions = Vec::new();
    let mut eligible = Vec::new();
    let exhausted_pools: BTreeSet<&str> = routes.iter()
        .filter(|route| propagate_pool_blocks && route.quota == Allowance::Exhausted && !route.pool_id.is_empty())
        .map(|route| route.pool_id.as_str()).collect();
    for route in routes {
        if let Some(reason) = excluded(work, route, &exhausted_pools) {
            exclusions.push(Exclusion {
                route_id: route.id.clone(),
                reason: reason.into(),
            });
        } else {
            eligible.push(route);
        }
    }
    eligible.sort_by_key(|route| {
        (
            route.tier,
            match route.health {
                Health::Healthy => 0,
                Health::Degraded => 1,
                Health::Unknown => 2,
                Health::Unavailable => 3,
            },
            match route.quota {
                Allowance::ObservedNonExhausted | Allowance::NotApplicable => 0,
                Allowance::Unknown => 1,
                Allowance::Exhausted => 2,
            },
            route.fit != Fit::Fits,
            !route.recommended_default,
            work.preferred_harness.as_deref() != Some(route.harness.as_str()),
            route.id.as_str(),
        )
    });
    let selected = eligible.first().map(|r| r.id.clone());
    let reason = match eligible.first() {
        None => "no_eligible_route",
        Some(route) if route.quota == Allowance::Unknown => "cold_start_allowance_unknown",
        Some(route) if route.fit == Fit::Unknown => "cold_start_consumption_unknown",
        Some(_) => "eligible_task_suitable_default",
    };
    Decision {
        work_unit_id: work.id.clone(),
        selected,
        exclusions,
        reason: reason.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn route(
        id: &str,
        harness: &str,
        pool: &str,
        tier: CapabilityTier,
        effort: &str,
        tools: &[&str],
    ) -> Route {
        Route {
            id: id.into(),
            harness: harness.into(),
            provider: "fixture-provider".into(),
            endpoint: "fixture-endpoint".into(),
            profile_id: pool.into(),
            pool_id: pool.into(),
            model: id.into(),
            effort: effort.into(),
            tier,
            tools: tools.iter().map(|s| s.to_string()).collect(),
            context_limit: Some(100_000),
            supports_approvals: true,
            sandbox: Sandbox::WorkspaceWrite,
            recommended_default: tier == CapabilityTier::General,
            quota: Allowance::ObservedNonExhausted,
            fit: Fit::Unknown,
            health: Health::Healthy,
        }
    }

    fn unit(tier: CapabilityTier, tools: &[&str]) -> WorkUnit {
        WorkUnit {
            id: "unit-1".into(),
            min_tier: tier,
            required_tools: tools.iter().map(|s| s.to_string()).collect(),
            context_needed: 1000,
            requires_approvals: false,
            min_sandbox: Sandbox::ReadOnly,
            max_sandbox: Sandbox::WorkspaceWrite,
            allowed_profiles: ["pool-a", "pool-b"].iter().map(|s| s.to_string()).collect(),
            pinned_route: None,
            preferred_harness: None,
        }
    }

    #[test]
    fn healthy_browser_and_diagnosis_choose_different_suitable_models() {
        let sol = route(
            "sol-medium",
            "codex",
            "pool-a",
            CapabilityTier::General,
            "medium",
            &["browser"],
        );
        let astra = route(
            "astra-high",
            "codex",
            "pool-a",
            CapabilityTier::Frontier,
            "high",
            &["browser"],
        );
        let browser = select(
            &unit(CapabilityTier::General, &["browser"]),
            &[astra.clone(), sol.clone()],
        );
        assert_eq!(browser.selected.as_deref(), Some("sol-medium"));
        let diagnosis = select(&unit(CapabilityTier::Frontier, &["browser"]), &[astra, sol]);
        assert_eq!(diagnosis.selected.as_deref(), Some("astra-high"));
    }

    #[test]
    fn three_provider_routes_use_one_eligibility_path_without_a_harness_chain() {
        let codex = route("a", "codex", "pool-a", CapabilityTier::General, "medium", &[]);
        let claude = route("b", "claude", "pool-a", CapabilityTier::General, "medium", &[]);
        let opencode = route("c", "opencode", "pool-b", CapabilityTier::General, "medium", &[]);
        let mut work = unit(CapabilityTier::General, &["browser"]);
        let blocked = select(&work, &[opencode.clone(), codex.clone(), claude.clone()]);
        assert!(blocked.selected.is_none());
        assert_eq!(blocked.exclusions.len(), 3);
        assert!(blocked.exclusions.iter().all(|entry| entry.reason == "missing_tool"));

        work.required_tools.clear();
        assert_eq!(select(&work, &[opencode.clone(), claude.clone(), codex.clone()])
            .selected.as_deref(), Some("a"), "input order does not become provider policy");
        work.preferred_harness = Some("opencode".into());
        assert_eq!(select(&work, &[codex, opencode, claude]).selected.as_deref(), Some("c"));
    }

    #[test]
    fn hard_pin_and_missing_tools_override_harness_preference() {
        let preferred = route(
            "preferred",
            "claude",
            "pool-a",
            CapabilityTier::General,
            "medium",
            &[],
        );
        let capable = route(
            "capable",
            "codex",
            "pool-b",
            CapabilityTier::General,
            "medium",
            &["browser"],
        );
        let mut work = unit(CapabilityTier::General, &["browser"]);
        work.preferred_harness = Some("claude".into());
        assert_eq!(
            select(&work, &[preferred.clone(), capable.clone()])
                .selected
                .as_deref(),
            Some("capable")
        );
        work.pinned_route = Some("preferred".into());
        let stopped = select(&work, &[preferred, capable]);
        assert!(stopped.selected.is_none());
        assert!(stopped
            .exclusions
            .iter()
            .any(|x| x.reason == "missing_tool"));
    }

    #[test]
    fn shared_pool_exhaustion_blocks_both_harnesses_but_not_an_independent_route() {
        let mut codex = route(
            "codex",
            "codex",
            "pool-a",
            CapabilityTier::General,
            "medium",
            &["browser"],
        );
        let mut app = route(
            "codex-app",
            "codex-app",
            "pool-a",
            CapabilityTier::General,
            "medium",
            &["browser"],
        );
        let mut local = route(
            "local",
            "opencode",
            "pool-b",
            CapabilityTier::General,
            "medium",
            &["browser"],
        );
        codex.quota = Allowance::Exhausted;
        app.quota = Allowance::Exhausted;
        local.quota = Allowance::NotApplicable;
        let chosen = select(
            &unit(CapabilityTier::General, &["browser"]),
            &[codex, app, local],
        );
        assert_eq!(chosen.selected.as_deref(), Some("local"));
        assert_eq!(
            chosen
                .exclusions
                .iter()
                .filter(|x| x.reason == "quota_exhausted")
                .count(),
            2
        );
    }

    #[test]
    fn one_authoritative_pool_block_overrides_another_harnesses_stale_capacity() {
        let mut exhausted = route("codex", "codex", "pool-a", CapabilityTier::General,
            "medium", &["browser"]);
        let alternate = route("codex-app", "codex-app", "pool-a", CapabilityTier::General,
            "medium", &["browser"]);
        let independent = route("local", "opencode", "pool-b", CapabilityTier::General,
            "medium", &["browser"]);
        exhausted.quota = Allowance::Exhausted;
        let choice = select(&unit(CapabilityTier::General, &["browser"]),
            &[exhausted.clone(), alternate.clone(), independent.clone()]);
        assert_eq!(choice.selected.as_deref(), Some("local"));
        assert_eq!(choice.exclusions.iter().filter(|e| e.reason == "quota_exhausted").count(), 2);
        assert_eq!(select_legacy_v1(&unit(CapabilityTier::General, &["browser"]),
            &[exhausted, alternate, independent]).selected.as_deref(),
            Some("codex-app"));
    }

    #[test]
    fn observed_quota_state_feeds_routes_without_inventing_capacity() {
        use crate::auto_quota::parse_codex_rate_limits;
        use serde_json::json;
        let now = 1_800_000_000_000_i64;
        let quota = parse_codex_rate_limits(
            &json!({"rateLimits":{"primary":{"usedPercent":100,"resetsAt":1800003600}}}),
            "pool-a",
            now,
        )
        .unwrap();
        assert_eq!(observed_allowance(None, "sol", now), Allowance::Unknown);
        assert_eq!(
            observed_allowance(Some(&quota), "sol", now),
            Allowance::Exhausted
        );
        assert_eq!(
            observed_allowance(Some(&quota), "sol", now + 60_000),
            Allowance::Unknown
        );
    }

    #[test]
    fn scoped_allowance_fit_checks_every_window_and_in_flight_commitment() {
        use crate::auto_quota::parse_codex_rate_limits;
        use serde_json::json;
        let now = 1_800_000_000_000_i64;
        let quota = parse_codex_rate_limits(&json!({
            "ordinaryUsageAllowed": true,
            "rateLimitsByLimitId": {
                "shared": {"limitId":"shared", "primary":{"usedPercent":90,"resetsAt":1800003600}},
                "model": {"limitId":"model", "normalModelSlug":"sol", "primary":{"usedPercent":50,"resetsAt":1800003600}}
            }
        }), "pool-a", now).unwrap();
        let route = route(
            "sol",
            "codex",
            "pool-a",
            CapabilityTier::General,
            "medium",
            &["browser"],
        );
        let mut estimate = AllowanceEstimate {
            pool_id: "pool-a".into(),
            model: "sol".into(),
            effort: "medium".into(),
            source: DrawSource::AttributedActualWork,
            observed_ms: now,
            windows: vec![
                WindowDraw {
                    bucket_id: "shared".into(),
                    window: "primary".into(),
                    upper_percent: 15.0,
                },
                WindowDraw {
                    bucket_id: "model".into(),
                    window: "primary".into(),
                    upper_percent: 10.0,
                },
            ],
        };
        assert_eq!(
            assess_fit(&quota, &route, Some(&estimate), &[], now),
            Fit::Unaffordable
        );
        let mut partial = estimate.clone();
        partial.windows.pop();
        assert_eq!(
            assess_fit(&quota, &route, Some(&partial), &[], now),
            Fit::Unaffordable
        );
        partial.windows.clear();
        partial.windows.push(WindowDraw {
            bucket_id: "model".into(),
            window: "primary".into(),
            upper_percent: 60.0,
        });
        assert_eq!(
            assess_fit(&quota, &route, Some(&partial), &[], now),
            Fit::Unaffordable
        );
        partial.windows[0].upper_percent = 45.0;
        let prior = [WindowDraw {
            bucket_id: "model".into(),
            window: "primary".into(),
            upper_percent: 10.0,
        }];
        assert_eq!(
            assess_fit(&quota, &route, Some(&partial), &prior, now),
            Fit::Unaffordable
        );
        estimate.windows[0].upper_percent = 5.0;
        assert_eq!(
            assess_fit(&quota, &route, Some(&estimate), &[], now),
            Fit::Fits
        );
        let in_flight = [WindowDraw {
            bucket_id: "shared".into(),
            window: "primary".into(),
            upper_percent: 6.0,
        }];
        assert_eq!(
            assess_fit(&quota, &route, Some(&estimate), &in_flight, now),
            Fit::Unaffordable
        );
    }

    #[test]
    fn absent_or_invalid_draw_and_reset_boundary_remain_unknown() {
        use crate::auto_quota::parse_codex_rate_limits;
        use serde_json::json;
        let now = 1_800_000_000_000_i64;
        let quota = parse_codex_rate_limits(
            &json!({"rateLimits":{"primary":{"usedPercent":50,"resetsAt":1800000010}}}),
            "pool-a",
            now,
        )
        .unwrap();
        let route = route(
            "sol",
            "codex",
            "pool-a",
            CapabilityTier::General,
            "medium",
            &["browser"],
        );
        assert_eq!(assess_fit(&quota, &route, None, &[], now), Fit::Unknown);
        let mut estimate = AllowanceEstimate {
            pool_id: "pool-a".into(),
            model: "sol".into(),
            effort: "medium".into(),
            source: DrawSource::ProviderReported,
            observed_ms: now,
            windows: vec![WindowDraw {
                bucket_id: "codex".into(),
                window: "primary".into(),
                upper_percent: 0.0,
            }],
        };
        assert_eq!(
            assess_fit(&quota, &route, Some(&estimate), &[], now),
            Fit::Unknown
        );
        estimate.windows[0].upper_percent = 5.0;
        assert_eq!(
            assess_fit(&quota, &route, Some(&estimate), &[], now + 10_000),
            Fit::Unknown
        );
        estimate.pool_id = "other-account".into();
        assert_eq!(
            assess_fit(&quota, &route, Some(&estimate), &[], now),
            Fit::Unknown
        );
    }

    #[test]
    fn unaffordable_route_is_excluded_before_task_aware_ranking() {
        let mut costly = route(
            "costly",
            "codex",
            "pool-a",
            CapabilityTier::General,
            "medium",
            &["browser"],
        );
        let efficient = route(
            "efficient",
            "claude",
            "pool-b",
            CapabilityTier::General,
            "medium",
            &["browser"],
        );
        costly.fit = Fit::Unaffordable;
        let decision = select(
            &unit(CapabilityTier::General, &["browser"]),
            &[costly, efficient],
        );
        assert_eq!(decision.selected.as_deref(), Some("efficient"));
        assert!(decision
            .exclusions
            .iter()
            .any(|x| x.reason == "estimated_draw_exceeds_allowance"));
    }

    #[test]
    fn unknown_allowance_is_not_treated_as_free_and_no_suitable_route_pauses() {
        let mut unknown = route(
            "unknown",
            "claude",
            "pool-a",
            CapabilityTier::General,
            "medium",
            &["browser"],
        );
        let known = route(
            "known",
            "codex",
            "pool-b",
            CapabilityTier::General,
            "medium",
            &["browser"],
        );
        unknown.quota = Allowance::Unknown;
        assert_eq!(
            select(
                &unit(CapabilityTier::General, &["browser"]),
                &[unknown.clone(), known]
            )
            .selected
            .as_deref(),
            Some("known")
        );
        unknown.tools.clear();
        let paused = select(&unit(CapabilityTier::General, &["browser"]), &[unknown]);
        assert!(paused.selected.is_none());
        assert_eq!(paused.exclusions[0].reason, "missing_tool");
    }
}
