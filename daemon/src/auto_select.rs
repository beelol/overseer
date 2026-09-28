//! Provider-neutral eligibility and task-aware selection for Auto Mode.

use crate::auto_quota::{QuotaBlockScope, QuotaSnapshot, QuotaState};
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
    /// The shared booking's per-window bound from this account's isolated
    /// runs of the same bucket (`upper_draw.rs`).
    QualifiedUpperDraw,
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
    /// A provider- or harness-resolved immutable model identity. A moving
    /// alias alone cannot make an old draw estimate comparable.
    #[serde(default)]
    pub model_version: Option<String>,
    #[serde(default)]
    pub task_signature: Option<TaskSignature>,
    #[serde(default)]
    pub plan_type: Option<String>,
    pub source: DrawSource,
    pub observed_ms: i64,
    pub windows: Vec<WindowDraw>,
    /// A validated predictive envelope for a bounded class of future work.
    /// A single completed-work delta has no such basis.
    #[serde(default)]
    pub prediction_basis: Option<PredictionBasis>,
}

/// Reserved contract for a future validated aggregate of comparable work.
/// The current completed-work delta producer leaves this absent, so its
/// samples cannot authorize a later launch. Count/method fields alone are
/// claims, not validation; the future producer must establish the bound.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PredictionBasis {
    pub task_class: String,
    pub max_execution_budget_ms: u64,
    pub sample_count: u32,
    pub method: String,
}

/// Missing, stale, reset, or mismatched evidence never establishes fit.
/// `in_flight` must be the complete set of locally admitted work in this pool;
/// durable reservation and external-work uncertainty are separate admission
/// requirements before this can authorize a launch.
pub fn assess_fit(
    snapshot: &QuotaSnapshot,
    work: &WorkUnit,
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
    let Some(basis) = estimate.prediction_basis.as_ref() else {
        return Fit::Unknown;
    };
    if !matches!(basis.method.as_str(), "bounded_class_upper_v1" | "bounded_class_total_upper_v1")
        || basis.sample_count < 5
        || work.task_class.as_deref() != Some(basis.task_class.as_str())
        || work.execution_budget_ms.is_none_or(|budget| budget == 0
            || budget > basis.max_execution_budget_ms)
    {
        return Fit::Unknown;
    }
    const MAX_ESTIMATE_AGE_MS: i64 = 30 * 86_400_000;
    if estimate.pool_id != route.pool_id
        || estimate.model != route.model
        || estimate.effort != route.effort
        || !matches!((&estimate.model_version, &route.resolved_model_version),
            (Some(left), Some(right)) if !left.is_empty() && left == right)
        || estimate.task_signature.as_ref() != Some(&TaskSignature::from(work))
        || now_ms < estimate.observed_ms
        || now_ms.saturating_sub(estimate.observed_ms) >= MAX_ESTIMATE_AGE_MS
    {
        return Fit::Unknown;
    }
    let applicable = snapshot.applicable_to(&route.model);
    if applicable.is_empty() || estimate.plan_type.is_none()
        || applicable.iter().any(|window| window.plan_type != estimate.plan_type) {
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
    #[serde(default)]
    pub resolved_model_version: Option<String>,
    pub effort: String,
    pub tier: CapabilityTier,
    pub tools: BTreeSet<String>,
    pub context_limit: Option<u64>,
    pub supports_approvals: bool,
    pub sandbox: Sandbox,
    /// Explicit modes supported by this harness route. Older recorded routes
    /// omit this field and retain their original single-mode interpretation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supported_sandboxes: Option<BTreeSet<Sandbox>>,
    pub recommended_default: bool,
    pub quota: Allowance,
    #[serde(default)]
    pub quota_blocks: Vec<QuotaBlockScope>,
    pub fit: Fit,
    pub health: Health,
    /// True only for this decision when an exhausted allowed account may be
    /// the upstream of a credential-free local proxy. Unknown identity is a
    /// conservative admission block, not a claim that this route is exhausted.
    #[serde(default)]
    pub unresolved_quota_pool_identity: bool,
    /// An admitted work unit with unknown draw is already using this pool.
    /// This is local admission evidence, not provider exhaustion.
    #[serde(default)]
    pub in_flight_pool_claim: bool,
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
    #[serde(default)]
    pub task_class: Option<String>,
    #[serde(default)]
    pub execution_budget_ms: Option<u64>,
}

/// Exact structured requirements for conservative comparison of completed
/// work. IDs, account allowlists, pins and harness preference are routing
/// choices, not a claim that two work units consume the same allowance.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TaskSignature {
    pub min_tier: CapabilityTier,
    pub required_tools: BTreeSet<String>,
    pub context_needed: u64,
    pub requires_approvals: bool,
    pub min_sandbox: Sandbox,
    pub max_sandbox: Sandbox,
    #[serde(default)]
    pub task_class: Option<String>,
    #[serde(default)]
    pub execution_budget_ms: Option<u64>,
}

impl From<&WorkUnit> for TaskSignature {
    fn from(work: &WorkUnit) -> Self {
        Self { min_tier:work.min_tier, required_tools:work.required_tools.clone(),
            context_needed:work.context_needed, requires_approvals:work.requires_approvals,
            min_sandbox:work.min_sandbox, max_sandbox:work.max_sandbox,
            task_class:work.task_class.clone(), execution_budget_ms:work.execution_budget_ms }
    }
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

/// A prediction of *complete* work-unit draw, including any context/cache
/// transfer, retry, and verification overhead. Only a separately validated
/// same-plan, same-pool producer may provide this for ranking. It is not an
/// API token price or a cross-account exchange rate.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CompleteCost {
    pub pool_id: String,
    pub plan_type: String,
    pub windows: Vec<WindowDraw>,
}

fn decisive_complete_savings(current: &CompleteCost, candidate: &CompleteCost) -> bool {
    if current.pool_id.is_empty() || current.pool_id != candidate.pool_id
        || current.plan_type.is_empty() || current.plan_type != candidate.plan_type
        || current.windows.is_empty() || current.windows.len() != candidate.windows.len()
        || current.windows.len() > 16 {
        return false;
    }
    let mut seen = BTreeSet::new();
    current.windows.iter().all(|window| {
        if !seen.insert((&window.bucket_id, &window.window))
            || !window.upper_percent.is_finite() || window.upper_percent <= 0.0 {
            return false;
        }
        let matches = candidate.windows.iter().filter(|other|
            other.bucket_id == window.bucket_id && other.window == window.window).collect::<Vec<_>>();
        matches.len() == 1 && window.upper_percent <= 100.0
            && matches[0].upper_percent.is_finite()
            && (0.0..=100.0).contains(&matches[0].upper_percent)
            && matches[0].upper_percent > 0.0
            && matches[0].upper_percent <= window.upper_percent * 0.9
    })
}

/// Preserve the ordinary task/health/allowance order unless two eligible,
/// equally capable routes have comparable complete-work predictions. The
/// ten-percent dominance margin prevents tiny estimate corrections from
/// changing a route; complete-work bounds already include switch overhead.
pub fn select_with_complete_costs(work: &WorkUnit, routes: &[Route],
    costs: &[Option<CompleteCost>]) -> Decision {
    let mut decision = select(work, routes);
    if costs.len() != routes.len() { return decision; }
    let Some(mut chosen_index) = decision.selected.as_deref()
        .and_then(|id| routes.iter().position(|route| route.id == id)) else {
        return decision;
    };
    if routes[chosen_index].fit != Fit::Fits || costs[chosen_index].as_ref()
        .is_none_or(|cost| cost.pool_id != routes[chosen_index].pool_id) {
        return decision;
    }
    let excluded = decision.exclusions.iter().map(|entry| entry.route_id.as_str())
        .collect::<BTreeSet<_>>();
    let mut candidate_indices = (0..routes.len()).collect::<Vec<_>>();
    candidate_indices.sort_by_key(|index| routes[*index].id.as_str());
    for index in candidate_indices {
        let route = &routes[index];
        let current = &routes[chosen_index];
        if excluded.contains(route.id.as_str()) || route.fit != Fit::Fits
            || route.tier != current.tier || route.health != current.health
            || route.quota != current.quota {
            continue;
        }
        if let (Some(baseline), Some(candidate)) = (&costs[chosen_index], &costs[index]) {
            if candidate.pool_id == route.pool_id
                && decisive_complete_savings(baseline, candidate) {
                chosen_index = index;
            }
        }
    }
    if decision.selected.as_deref() != Some(routes[chosen_index].id.as_str()) {
        decision.selected = Some(routes[chosen_index].id.clone());
        decision.reason = "comparable_complete_draw_lower".into();
    }
    decision
}

#[derive(Default)]
struct PoolBlocks<'a> {
    accounts: BTreeSet<&'a str>,
    models: BTreeSet<(&'a str, &'a str)>,
    families: BTreeSet<(&'a str, &'a str)>,
}

fn excluded(work: &WorkUnit, route: &Route, blocks: &PoolBlocks<'_>) -> Option<&'static str> {
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
    if route.unresolved_quota_pool_identity {
        return Some("unresolved_quota_pool_identity");
    }
    if route.quota == Allowance::Exhausted || blocks.accounts.contains(route.pool_id.as_str())
        || blocks.models.contains(&(route.pool_id.as_str(), route.model.as_str()))
        || route.model.split('-').any(|part| blocks.families.contains(&(route.pool_id.as_str(), part))) {
        return Some("quota_exhausted");
    }
    if route.in_flight_pool_claim {
        return Some("pool_in_flight_unknown_draw");
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
    let sandbox_supported = route.supported_sandboxes.as_ref()
        .map(|modes| modes.iter().any(|mode| *mode >= work.min_sandbox && *mode <= work.max_sandbox))
        .unwrap_or(route.sandbox >= work.min_sandbox && route.sandbox <= work.max_sandbox);
    if !sandbox_supported {
        return Some("sandbox_incompatible");
    }
    None
}

/// Deterministic eligibility comes before ranking. Unknown allowance remains
/// eligible only as a disclosed cold-start possibility, never as free capacity.
pub fn select(work: &WorkUnit, routes: &[Route]) -> Decision {
    select_with_pool_blocks(work, routes, true, false, true)
}

/// Replay only: decisions recorded before shared-pool block propagation keep
/// their original selector semantics rather than changing under a new build.
pub fn select_legacy_v1(work: &WorkUnit, routes: &[Route]) -> Decision {
    select_with_pool_blocks(work, routes, false, true, false)
}

/// Replay decisions written before public-status collection changed the
/// ranking of unknown versus degraded health. Pool blocks already existed.
pub fn select_pre_status_v1(work: &WorkUnit, routes: &[Route]) -> Decision {
    select_with_pool_blocks(work, routes, true, true, false)
}

/// Replay decisions written before quota-pool propagation used model scopes.
pub fn select_pre_scoped_pool_v1(work: &WorkUnit, routes: &[Route]) -> Decision {
    select_with_pool_blocks(work, routes, true, false, false)
}

fn select_with_pool_blocks(work: &WorkUnit, routes: &[Route], propagate_pool_blocks: bool,
    legacy_health_order: bool, scoped_blocks: bool) -> Decision {
    let mut exclusions = Vec::new();
    let mut eligible = Vec::new();
    let mut blocks = PoolBlocks::default();
    for route in routes.iter().filter(|route| propagate_pool_blocks
        && route.quota == Allowance::Exhausted && !route.pool_id.is_empty()) {
        if !scoped_blocks || route.quota_blocks.is_empty() {
            blocks.accounts.insert(route.pool_id.as_str());
            continue;
        }
        for scope in &route.quota_blocks {
            match scope {
                QuotaBlockScope::Account => { blocks.accounts.insert(route.pool_id.as_str()); }
                QuotaBlockScope::Model(model) => { blocks.models.insert((route.pool_id.as_str(), model)); }
                QuotaBlockScope::ModelFamily(family) => { blocks.families.insert((route.pool_id.as_str(), family)); }
            }
        }
    }
    for route in routes {
        if let Some(reason) = excluded(work, route, &blocks) {
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
            if legacy_health_order {
                match route.health { Health::Healthy => 0, Health::Degraded => 1,
                    Health::Unknown => 2, Health::Unavailable => 3 }
            } else {
                match route.health { Health::Healthy => 0, Health::Unknown => 1,
                    Health::Degraded => 2, Health::Unavailable => 3 }
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
            resolved_model_version: Some(format!("{id}-v1")),
            effort: effort.into(),
            tier,
            tools: tools.iter().map(|s| s.to_string()).collect(),
            context_limit: Some(100_000),
            supports_approvals: true,
            sandbox: Sandbox::WorkspaceWrite,
            supported_sandboxes:None,
            recommended_default: tier == CapabilityTier::General,
            quota: Allowance::ObservedNonExhausted,
            quota_blocks: Vec::new(),
            fit: Fit::Unknown,
            health: Health::Healthy,
            unresolved_quota_pool_identity: false,
            in_flight_pool_claim: false,
        }
    }

    #[test]
    fn legacy_replay_keeps_the_health_order_recorded_before_public_feeds() {
        let mut degraded = route("a", "codex-app", "pool-a", CapabilityTier::General,
            "medium", &[]);
        let mut unknown = route("b", "claude", "pool-b", CapabilityTier::General,
            "medium", &[]);
        degraded.health = Health::Degraded;
        unknown.health = Health::Unknown;
        let work = unit(CapabilityTier::General, &[]);
        assert_eq!(select_legacy_v1(&work, &[degraded.clone(), unknown.clone()]).selected.as_deref(), Some("a"));
        assert_eq!(select_pre_status_v1(&work, &[degraded.clone(), unknown.clone()]).selected.as_deref(), Some("a"));
        assert_eq!(select(&work, &[degraded, unknown]).selected.as_deref(), Some("b"));
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
            task_class: Some("fixture_browser".into()),
            execution_budget_ms: Some(10_000),
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
    fn complete_cost_ranking_respects_pool_pins_and_soft_harness_preference() {
        let mut baseline = route("baseline", "codex", "pool-a", CapabilityTier::General,
            "medium", &[]);
        baseline.fit = Fit::Fits;
        baseline.recommended_default = true;
        let mut alternate = route("alternate", "claude", "pool-a", CapabilityTier::General,
            "medium", &[]);
        alternate.fit = Fit::Fits;
        alternate.recommended_default = false;
        let cost = |pool: &str, upper_percent| Some(CompleteCost {
            pool_id:pool.into(), plan_type:"fixture-plan".into(),
            windows:vec![WindowDraw { bucket_id:"shared".into(),
                window:"primary".into(), upper_percent }],
        });
        let work = unit(CapabilityTier::General, &[]);
        assert_eq!(select(&work, &[baseline.clone(), alternate.clone()])
            .selected.as_deref(), Some("baseline"));
        let saved = select_with_complete_costs(&work, &[baseline.clone(), alternate.clone()],
            &[cost("pool-a", 8.0), cost("pool-a", 3.0)]);
        assert_eq!(saved.selected.as_deref(), Some("alternate"), "{saved:?}");

        let mut preferred = work.clone();
        preferred.preferred_harness = Some("codex".into());
        assert_eq!(select_with_complete_costs(&preferred,
            &[baseline.clone(), alternate.clone()], &[cost("pool-a", 8.0), cost("pool-a", 3.0)])
            .selected.as_deref(), Some("alternate"),
            "a soft harness preference cannot hide a decisive comparable saving");
        preferred.pinned_route = Some("baseline".into());
        assert_eq!(select_with_complete_costs(&preferred,
            &[baseline.clone(), alternate.clone()], &[cost("pool-a", 8.0), cost("pool-a", 3.0)])
            .selected.as_deref(), Some("baseline"),
            "the explicit pin remains a hard constraint");

        alternate.pool_id = "pool-b".into();
        alternate.profile_id = "pool-b".into();
        assert_eq!(select_with_complete_costs(&work, &[baseline, alternate],
            &[cost("pool-a", 8.0), cost("pool-b", 3.0)])
            .selected.as_deref(), Some("baseline"),
            "raw percentages from separate subscription pools are not prices");
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
    fn eligibility_reasons_do_not_depend_on_harness_identity() {
        let work = unit(CapabilityTier::General, &["browser"]);
        for harness in ["codex", "claude", "opencode", "synthetic-future-harness"] {
            let mut candidate = route("candidate", harness, "pool-a",
                CapabilityTier::General, "medium", &[]);
            let missing_tool = select(&work, &[candidate.clone()]);
            assert_eq!(missing_tool.selected, None, "{harness}");
            assert_eq!(missing_tool.exclusions[0].reason, "missing_tool", "{harness}");

            candidate.tools.insert("browser".into());
            candidate.quota = Allowance::Exhausted;
            let exhausted = select(&work, &[candidate.clone()]);
            assert_eq!(exhausted.exclusions[0].reason, "quota_exhausted", "{harness}");

            candidate.quota = Allowance::Unknown;
            candidate.health = Health::Healthy;
            let cold_start = select(&work, &[candidate]);
            assert_eq!(cold_start.selected.as_deref(), Some("candidate"),
                "unknown allowance stays a disclosed cold-start candidate for {harness}");
            assert_eq!(cold_start.reason, "cold_start_allowance_unknown", "{harness}");
        }
    }

    #[test]
    fn an_unknown_in_flight_pool_excludes_aliases_but_not_an_independent_pool() {
        let work = unit(CapabilityTier::General, &[]);
        let mut first = route("same-account-a", "codex", "pool-a", CapabilityTier::General, "medium", &[]);
        let mut alias = route("same-account-b", "claude", "pool-a", CapabilityTier::General, "medium", &[]);
        let independent = route("independent", "codex", "pool-b", CapabilityTier::General, "medium", &[]);
        first.in_flight_pool_claim = true;
        alias.in_flight_pool_claim = true;
        let decision = select(&work, &[first, alias, independent]);
        assert_eq!(decision.selected.as_deref(), Some("independent"));
        assert_eq!(decision.exclusions.iter().filter(|entry|
            entry.reason == "pool_in_flight_unknown_draw").count(), 2);
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

    /// AUTO-AC-11 in one place: a preferred harness is a soft tie-break that
    /// loses to capability (tools, tier, context); an explicit pin binds even
    /// against the preference; routes outside the allowed accounts, without
    /// the approvals the unit needs, or unable to run in its sandbox (a
    /// permission downgrade or widening) are excluded with their reasons.
    #[test]
    fn preference_is_soft_pins_bind_and_permission_mismatches_exclude() {
        let mut preferred = route("preferred", "claude", "pool-a", CapabilityTier::General, "medium", &[]);
        let capable = route("capable", "codex", "pool-b", CapabilityTier::General, "medium", &["browser"]);
        let frontier = route("frontier", "codex", "pool-b", CapabilityTier::Frontier, "high", &["browser"]);
        let reason = |decision: &Decision, id: &str| decision.exclusions.iter()
            .find(|x| x.route_id == id).map(|x| x.reason.clone());
        // Preference loses to missing tools, to tier and to context.
        let mut work = unit(CapabilityTier::General, &["browser"]);
        work.preferred_harness = Some("claude".into());
        let decision = select(&work, &[preferred.clone(), capable.clone()]);
        assert_eq!(decision.selected.as_deref(), Some("capable"));
        assert_eq!(reason(&decision, "preferred").as_deref(), Some("missing_tool"));
        preferred.tools = ["browser".to_string()].into();
        assert_eq!(select(&work, &[preferred.clone(), capable.clone()]).selected.as_deref(),
            Some("preferred"), "between equally capable routes the preference decides");
        let mut deep = work.clone();
        deep.min_tier = CapabilityTier::Frontier;
        let decision = select(&deep, &[preferred.clone(), frontier.clone()]);
        assert_eq!(decision.selected.as_deref(), Some("frontier"));
        assert_eq!(reason(&decision, "preferred").as_deref(), Some("insufficient_capability"));
        let mut long = work.clone();
        long.context_needed = 200_000;
        assert_eq!(reason(&select(&long, &[preferred.clone()]), "preferred").as_deref(),
            Some("context_unavailable"));
        // A pin binds against the preference; a pin elsewhere excludes.
        let mut pinned = work.clone();
        pinned.pinned_route = Some("capable".into());
        let decision = select(&pinned, &[preferred.clone(), capable.clone()]);
        assert_eq!(decision.selected.as_deref(), Some("capable"));
        assert_eq!(reason(&decision, "preferred").as_deref(), Some("pinned_elsewhere"));
        // Account boundary.
        let mut narrow = work.clone();
        narrow.allowed_profiles = ["pool-b".to_string()].into();
        assert_eq!(reason(&select(&narrow, &[preferred.clone()]), "preferred").as_deref(),
            Some("account_not_allowed"));
        // Approvals the unit requires cannot be dropped by the route.
        let mut no_approvals = capable.clone();
        no_approvals.supports_approvals = false;
        let mut approvals = work.clone();
        approvals.requires_approvals = true;
        assert_eq!(reason(&select(&approvals, &[no_approvals]), "capable").as_deref(),
            Some("approvals_unsupported"));
        // A read-only-only route cannot take write work; a write-only route
        // cannot take read-only work; a route offering both takes either.
        let mut read_only = capable.clone();
        read_only.sandbox = Sandbox::ReadOnly;
        let mut write = work.clone();
        write.min_sandbox = Sandbox::WorkspaceWrite;
        write.max_sandbox = Sandbox::WorkspaceWrite;
        assert_eq!(reason(&select(&write, &[read_only.clone()]), "capable").as_deref(),
            Some("sandbox_incompatible"));
        let mut read = work.clone();
        read.min_sandbox = Sandbox::ReadOnly;
        read.max_sandbox = Sandbox::ReadOnly;
        assert_eq!(reason(&select(&read, &[capable.clone()]), "capable").as_deref(),
            Some("sandbox_incompatible"));
        let mut both = capable.clone();
        both.supported_sandboxes = Some([Sandbox::ReadOnly, Sandbox::WorkspaceWrite].into());
        assert_eq!(select(&read, &[both.clone()]).selected.as_deref(), Some("capable"));
        assert_eq!(select(&write, &[both]).selected.as_deref(), Some("capable"));
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
    fn model_scoped_exhaustion_blocks_that_model_across_harnesses_but_leaves_sibling_eligible() {
        use crate::auto_quota::parse_codex_rate_limits;
        use serde_json::json;
        let now = 1_800_000_000_000_i64;
        let snapshot = parse_codex_rate_limits(&json!({"rateLimitsByLimitId":{
            "astra":{"limitId":"astra","normalModelSlug":"gpt-6-astra",
                "primary":{"usedPercent":100,"resetsAt":1800003600}}
        }}), "pool-a", now).unwrap();
        let mut blocked = route("astra-cli", "codex", "pool-a", CapabilityTier::General,
            "medium", &[]);
        blocked.model = "gpt-6-astra".into();
        blocked.quota = observed_allowance(Some(&snapshot), &blocked.model, now);
        blocked.quota_blocks = snapshot.blocking_scopes(&blocked.model, now);
        let mut stale = route("astra-app", "codex-app", "pool-a", CapabilityTier::General,
            "medium", &[]);
        stale.model = blocked.model.clone();
        let mut sibling = route("sol-app", "codex-app", "pool-a", CapabilityTier::General,
            "medium", &[]);
        sibling.model = "gpt-6-sol".into();
        let routes = [blocked, stale, sibling];
        let result = select(&unit(CapabilityTier::General, &[]), &routes);
        assert_eq!(result.selected.as_deref(), Some("sol-app"), "{result:?}");
        assert_eq!(result.exclusions.iter().filter(|e| e.reason == "quota_exhausted").count(), 2);
        assert!(select_pre_scoped_pool_v1(&unit(CapabilityTier::General, &[]), &routes)
            .selected.is_none(), "older traces keep account-wide pool propagation");
    }

    #[test]
    fn family_scoped_exhaustion_keeps_another_family_in_the_account_eligible() {
        let mut opus = route("opus-cli", "claude", "pool-a", CapabilityTier::General,
            "high", &[]);
        opus.model = "opus".into();
        opus.quota = Allowance::Exhausted;
        opus.quota_blocks = vec![QuotaBlockScope::ModelFamily("opus".into())];
        let mut stale_opus = route("opus-other", "claude", "pool-a", CapabilityTier::General,
            "high", &[]);
        stale_opus.model = "opus".into();
        let mut sonnet = route("sonnet", "claude", "pool-a", CapabilityTier::General,
            "medium", &[]);
        sonnet.model = "sonnet".into();
        let result = select(&unit(CapabilityTier::General, &[]), &[opus, stale_opus, sonnet]);
        assert_eq!(result.selected.as_deref(), Some("sonnet"), "{result:?}");
        assert_eq!(result.exclusions.iter().filter(|entry| entry.reason == "quota_exhausted").count(), 2);
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
            Allowance::Exhausted
        );
        assert_eq!(
            observed_allowance(Some(&quota), "sol", now + 3_600_000),
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
                "shared": {"limitId":"shared", "planType":"pro", "primary":{"usedPercent":90,"resetsAt":1800003600}},
                "model": {"limitId":"model", "planType":"pro", "normalModelSlug":"sol", "primary":{"usedPercent":50,"resetsAt":1800003600}}
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
            model_version: Some("sol-v1".into()),
            task_signature: Some(TaskSignature::from(&unit(CapabilityTier::General, &["browser"]))),
            plan_type: Some("pro".into()),
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
            prediction_basis: Some(PredictionBasis { task_class:"fixture_browser".into(),
                max_execution_budget_ms:10_000, sample_count:5,
                method:"bounded_class_upper_v1".into() }),
        };
        assert_eq!(
            assess_fit(&quota, &unit(CapabilityTier::General, &["browser"]), &route, Some(&estimate), &[], now),
            Fit::Unaffordable
        );
        let mut partial = estimate.clone();
        partial.windows.pop();
        assert_eq!(
            assess_fit(&quota, &unit(CapabilityTier::General, &["browser"]), &route, Some(&partial), &[], now),
            Fit::Unaffordable
        );
        partial.windows.clear();
        partial.windows.push(WindowDraw {
            bucket_id: "model".into(),
            window: "primary".into(),
            upper_percent: 60.0,
        });
        assert_eq!(
            assess_fit(&quota, &unit(CapabilityTier::General, &["browser"]), &route, Some(&partial), &[], now),
            Fit::Unaffordable
        );
        partial.windows[0].upper_percent = 45.0;
        let prior = [WindowDraw {
            bucket_id: "model".into(),
            window: "primary".into(),
            upper_percent: 10.0,
        }];
        assert_eq!(
            assess_fit(&quota, &unit(CapabilityTier::General, &["browser"]), &route, Some(&partial), &prior, now),
            Fit::Unaffordable
        );
        estimate.windows[0].upper_percent = 5.0;
        assert_eq!(
            assess_fit(&quota, &unit(CapabilityTier::General, &["browser"]), &route, Some(&estimate), &[], now),
            Fit::Fits
        );
        let in_flight = [WindowDraw {
            bucket_id: "shared".into(),
            window: "primary".into(),
            upper_percent: 6.0,
        }];
        assert_eq!(
            assess_fit(&quota, &unit(CapabilityTier::General, &["browser"]), &route, Some(&estimate), &in_flight, now),
            Fit::Unaffordable
        );
    }

    #[test]
    fn absent_or_invalid_draw_and_reset_boundary_remain_unknown() {
        use crate::auto_quota::parse_codex_rate_limits;
        use serde_json::json;
        let now = 1_800_000_000_000_i64;
        let quota = parse_codex_rate_limits(
            &json!({"rateLimits":{"planType":"pro","primary":{"usedPercent":50,"resetsAt":1800000010}}}),
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
        assert_eq!(assess_fit(&quota, &unit(CapabilityTier::General, &["browser"]), &route, None, &[], now), Fit::Unknown);
        let mut estimate = AllowanceEstimate {
            pool_id: "pool-a".into(),
            model: "sol".into(),
            effort: "medium".into(),
            model_version: Some("sol-v1".into()),
            task_signature: Some(TaskSignature::from(&unit(CapabilityTier::General, &["browser"]))),
            plan_type: Some("pro".into()),
            source: DrawSource::ProviderReported,
            observed_ms: now,
            windows: vec![WindowDraw {
                bucket_id: "codex".into(),
                window: "primary".into(),
                upper_percent: 0.0,
            }],
            prediction_basis: Some(PredictionBasis { task_class:"fixture_browser".into(),
                max_execution_budget_ms:10_000, sample_count:5,
                method:"bounded_class_upper_v1".into() }),
        };
        assert_eq!(
            assess_fit(&quota, &unit(CapabilityTier::General, &["browser"]), &route, Some(&estimate), &[], now),
            Fit::Unknown
        );
        estimate.windows[0].upper_percent = 5.0;
        assert_eq!(
            assess_fit(&quota, &unit(CapabilityTier::General, &["browser"]), &route, Some(&estimate), &[], now + 10_000),
            Fit::Unknown
        );
        estimate.pool_id = "other-account".into();
        assert_eq!(
            assess_fit(&quota, &unit(CapabilityTier::General, &["browser"]), &route, Some(&estimate), &[], now),
            Fit::Unknown
        );
    }

    #[test]
    fn allowance_fit_requires_the_same_known_account_plan() {
        use crate::auto_quota::parse_codex_rate_limits;
        use serde_json::json;
        let now = 1_800_000_000_000_i64;
        let quota = parse_codex_rate_limits(&json!({"rateLimits":{
            "limitId":"codex","planType":"pro",
            "primary":{"usedPercent":40,"resetsAt":1800003600}
        }}), "pool-a", now).unwrap();
        let route = route("sol", "codex", "pool-a", CapabilityTier::General,
            "medium", &["browser"]);
        let estimate = |plan: Option<&str>| -> AllowanceEstimate {
            serde_json::from_value(json!({"pool_id":"pool-a","model":"sol",
                "effort":"medium","source":"provider_reported","observed_ms":now,
                "model_version":"sol-v1",
                "task_signature":{"min_tier":"general","required_tools":["browser"],
                    "context_needed":1000,"requires_approvals":false,
                    "min_sandbox":"read_only","max_sandbox":"workspace_write",
                    "task_class":"fixture_browser","execution_budget_ms":10000},
                "prediction_basis":{"task_class":"fixture_browser",
                    "max_execution_budget_ms":10000,"sample_count":5,
                    "method":"bounded_class_upper_v1"},
                "plan_type":plan,
                "windows":[{"bucket_id":"codex","window":"primary","upper_percent":5.0}]
            })).unwrap()
        };
        let work = unit(CapabilityTier::General, &["browser"]);
        assert_eq!(assess_fit(&quota, &work, &route, Some(&estimate(Some("pro"))), &[], now), Fit::Fits);
        assert_eq!(assess_fit(&quota, &work, &route, Some(&estimate(Some("plus"))), &[], now), Fit::Unknown);
        assert_eq!(assess_fit(&quota, &work, &route, Some(&estimate(None)), &[], now), Fit::Unknown);
    }

    #[test]
    fn one_completed_unit_is_not_a_prediction_for_later_work() {
        use crate::auto_quota::parse_codex_rate_limits;
        use serde_json::json;
        let now = 1_800_000_000_000_i64;
        let quota = parse_codex_rate_limits(&json!({"rateLimits":{
            "limitId":"codex","planType":"pro",
            "primary":{"usedPercent":97,"resetsAt":1800003600}
        }}), "pool-a", now).unwrap();
        let work = unit(CapabilityTier::General, &["browser"]);
        let route = route("sol", "codex", "pool-a", CapabilityTier::General,
            "medium", &["browser"]);
        let short_sample: AllowanceEstimate = serde_json::from_value(json!({
            "pool_id":"pool-a","model":"sol","effort":"medium",
            "model_version":"sol-v1","task_signature":TaskSignature::from(&work),
            "plan_type":"pro","source":"attributed_actual_work","observed_ms":now,
            "windows":[{"bucket_id":"codex","window":"primary","upper_percent":2.0}]
        })).unwrap();
        assert_eq!(assess_fit(&quota, &work, &route, Some(&short_sample), &[], now),
            Fit::Unknown, "one observed 2% draw cannot certify another browser unit fits in 3%");
        let mut claimed = short_sample.clone();
        claimed.prediction_basis = Some(PredictionBasis {
            task_class:"fixture_browser".into(), max_execution_budget_ms:10_000,
            sample_count:1, method:"bounded_class_upper_v1".into(),
        });
        assert_eq!(assess_fit(&quota, &work, &route, Some(&claimed), &[], now),
            Fit::Unknown, "a one-sample claim is not a conservative prediction");
        claimed.prediction_basis.as_mut().unwrap().sample_count = 5;
        assert_eq!(assess_fit(&quota, &work, &route, Some(&claimed), &[], now), Fit::Fits);
        let mut longer = work.clone();
        longer.execution_budget_ms = Some(20_000);
        assert_eq!(assess_fit(&quota, &longer, &route, Some(&claimed), &[], now),
            Fit::Unknown, "a longer unit cannot inherit the short-budget envelope");
    }

    #[test]
    fn alias_without_resolved_model_version_cannot_reuse_numeric_draw() {
        use crate::auto_quota::parse_codex_rate_limits;
        use serde_json::json;
        let now = 1_800_000_000_000_i64;
        let quota = parse_codex_rate_limits(&json!({"rateLimits":{
            "limitId":"codex","planType":"pro",
            "primary":{"usedPercent":40,"resetsAt":1800003600}
        }}), "pool-a", now).unwrap();
        let mut route = route("sol", "codex", "pool-a", CapabilityTier::General,
            "medium", &["browser"]);
        route.resolved_model_version = None;
        let estimate: AllowanceEstimate = serde_json::from_value(json!({
            "pool_id":"pool-a","model":"sol","model_version":"sol-v1",
            "effort":"medium","task_signature":{
                "min_tier":"general","required_tools":["browser"],"context_needed":1000,
                "requires_approvals":false,"min_sandbox":"read_only",
                "max_sandbox":"workspace_write","task_class":"fixture_browser",
                "execution_budget_ms":10000},
            "prediction_basis":{"task_class":"fixture_browser",
                "max_execution_budget_ms":10000,"sample_count":5,
                "method":"bounded_class_upper_v1"},
            "source":"provider_reported","observed_ms":now,"plan_type":"pro",
            "windows":[{"bucket_id":"codex","window":"primary","upper_percent":5.0}]
        })).unwrap();
        let work = unit(CapabilityTier::General, &["browser"]);
        assert_eq!(assess_fit(&quota, &work, &route, Some(&estimate), &[], now), Fit::Unknown);
        route.resolved_model_version = Some("sol-v2".into());
        assert_eq!(assess_fit(&quota, &work, &route, Some(&estimate), &[], now), Fit::Unknown);
        route.resolved_model_version = Some("sol-v1".into());
        assert_eq!(assess_fit(&quota, &work, &route, Some(&estimate), &[], now), Fit::Fits);
    }

    #[test]
    fn a_browser_estimate_cannot_price_a_different_task() {
        use crate::auto_quota::parse_codex_rate_limits;
        use serde_json::json;
        let now = 1_800_000_000_000_i64;
        let quota = parse_codex_rate_limits(&json!({"rateLimits":{
            "limitId":"codex","planType":"pro",
            "primary":{"usedPercent":40,"resetsAt":1800003600}
        }}), "pool-a", now).unwrap();
        let route = route("sol", "codex", "pool-a", CapabilityTier::General,
            "medium", &["browser"]);
        let estimate: AllowanceEstimate = serde_json::from_value(json!({
            "pool_id":"pool-a","model":"sol","model_version":"sol-v1",
            "effort":"medium","source":"provider_reported","observed_ms":now,
            "plan_type":"pro","task_signature":{
                "min_tier":"general","required_tools":["browser"],"context_needed":1000,
                "requires_approvals":false,"min_sandbox":"read_only",
                "max_sandbox":"workspace_write","task_class":"fixture_browser",
                "execution_budget_ms":10000},
            "prediction_basis":{"task_class":"fixture_browser",
                "max_execution_budget_ms":10000,"sample_count":5,
                "method":"bounded_class_upper_v1"},
            "windows":[{"bucket_id":"codex","window":"primary","upper_percent":5.0}]
        })).unwrap();
        let browser = unit(CapabilityTier::General, &["browser"]);
        assert_eq!(assess_fit(&quota, &browser, &route, Some(&estimate), &[], now), Fit::Fits);
        let mut larger = browser.clone();
        larger.context_needed = 20_000;
        assert_eq!(assess_fit(&quota, &larger, &route, Some(&estimate), &[], now), Fit::Unknown);
        let mut different_tool = browser;
        different_tool.required_tools = ["terminal".into()].into();
        assert_eq!(assess_fit(&quota, &different_tool, &route, Some(&estimate), &[], now), Fit::Unknown);
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
