//! Read-only integration of local allowance evidence with Auto route fit.

use crate::{
    auto_consumption::EstimateKey,
    auto_quota::QuotaSnapshot,
    auto_select::{self, Allowance, Fit, Route, TaskSignature, WorkUnit},
    store::Store,
};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

enum SnapshotEvidence {
    Available(QuotaSnapshot),
    Unavailable,
    SharedPoolUnreconciled,
}

/// Apply only same-account, same-plan, same-task, resolved-version estimates.
/// Learning read failures downgrade fit to unknown rather than stopping work.
/// Unknown-draw pool claims remain the separate durable admission boundary.
pub fn apply_scoped_fit(
    store: &Store,
    work: &WorkUnit,
    routes: &mut [Route],
    account_generations: &BTreeMap<String, i64>,
    now_ms: i64,
) -> Vec<Value> {
    let mut quota_by_profile = BTreeMap::<String, SnapshotEvidence>::new();
    let started = Instant::now();
    let mut learning_unavailable = false;
    let mut evidence = Vec::with_capacity(routes.len());
    for route in routes {
        route.fit = Fit::Unknown;
        let mut reason = "allowance_not_observed_nonexhausted";
        let mut source = None;
        let mut observed_ms = None;
        let mut estimated_windows = Vec::new();
        if route.quota == Allowance::ObservedNonExhausted {
            let snapshot_evidence = quota_by_profile
                .entry(route.profile_id.clone())
                .or_insert_with(|| {
                    match store.auto_account_quota_observations(&route.profile_id) {
                        Ok(observations) if observations.len() > 1 => {
                            SnapshotEvidence::SharedPoolUnreconciled
                        }
                        Ok(observations) => observations
                            .into_iter()
                            .find(|reading| reading.pool_id == route.profile_id)
                            .map(|reading| SnapshotEvidence::Available(reading.snapshot))
                            .unwrap_or(SnapshotEvidence::Unavailable),
                        Err(_) => SnapshotEvidence::Unavailable,
                    }
                });
            let shared_pool_unreconciled =
                matches!(snapshot_evidence, SnapshotEvidence::SharedPoolUnreconciled);
            let snapshot = match snapshot_evidence {
                SnapshotEvidence::Available(snapshot) => Some(&*snapshot),
                _ => None,
            };
            let generation = account_generations.get(&route.profile_id);
            let version = route
                .resolved_model_version
                .as_deref()
                .filter(|value| !value.is_empty());
            let plan = snapshot.and_then(QuotaSnapshot::reported_plan_type);
            let generation_changed = version.is_some()
                && generation.is_some_and(|expected| {
                    store
                        .auto_account_generation(&route.profile_id)
                        .ok()
                        .flatten()
                        != Some(*expected)
                });
            reason = if version.is_none() {
                "model_version_unverified"
            } else if generation.is_none() {
                "account_generation_unavailable"
            } else if generation_changed {
                "account_generation_changed"
            } else if snapshot.is_none() {
                if shared_pool_unreconciled {
                    "shared_pool_meter_unreconciled"
                } else {
                    "quota_snapshot_unavailable"
                }
            } else if plan.is_none() {
                "account_plan_unknown"
            } else {
                "no_comparable_estimate"
            };
            if let (Some(snapshot), Some(generation), Some(version), Some(plan)) =
                (snapshot.as_ref(), generation, version, plan)
            {
                if !generation_changed {
                    let key = EstimateKey {
                        pool_id: route.pool_id.clone(),
                        model: route.model.clone(),
                        effort: route.effort.clone(),
                        model_version: version.into(),
                        task_signature: TaskSignature::from(&*work),
                        plan_type: plan.into(),
                    };
                    if learning_unavailable {
                        reason = "learning_unavailable";
                    } else if started.elapsed() >= Duration::from_millis(500) {
                        reason = "fit_read_budget_elapsed";
                    } else {
                        match store.auto_allowance_estimate(
                            &route.profile_id,
                            *generation,
                            &key,
                            now_ms,
                        ) {
                            Ok(Some(estimate)) => {
                                source = Some(estimate.source);
                                observed_ms = Some(estimate.observed_ms);
                                estimated_windows = estimate.windows.clone();
                                route.fit = auto_select::assess_fit(
                                    snapshot,
                                    work,
                                    route,
                                    Some(&estimate),
                                    &[],
                                    now_ms,
                                );
                                reason = match route.fit {
                                    Fit::Fits => "bounded_estimate_fits",
                                    Fit::Unaffordable => "bounded_estimate_exceeds_allowance",
                                    Fit::Unknown => "estimate_or_quota_not_comparable",
                                };
                            }
                            Ok(None) => {
                                reason = "no_comparable_estimate";
                            }
                            Err(_) => {
                                learning_unavailable = true;
                                reason = "learning_unavailable";
                            }
                        }
                    }
                }
            }
        }
        evidence.push(json!({"route_id":route.id,"fit":route.fit,"reason":reason,
            "source":source,"observed_ms":observed_ms,"expected_windows":estimated_windows}));
    }
    evidence
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        auto_quota::parse_codex_rate_limits,
        auto_select::{select, Allowance, CapabilityTier, Fit, Health, Route, Sandbox, WorkUnit},
        store::Store,
    };
    use serde_json::json;
    use std::{collections::BTreeMap, path::Path};

    fn work() -> WorkUnit {
        WorkUnit {
            id: "browser-unit".into(),
            min_tier: CapabilityTier::General,
            required_tools: ["browser".into()].into(),
            context_needed: 1000,
            requires_approvals: false,
            min_sandbox: Sandbox::ReadOnly,
            max_sandbox: Sandbox::WorkspaceWrite,
            allowed_profiles: ["profile".into()].into(),
            pinned_route: None,
            preferred_harness: None,
        }
    }

    fn route(id: &str, pool: &str) -> Route {
        Route {
            id: id.into(),
            harness: "codex-app".into(),
            provider: "openai".into(),
            endpoint: "codex".into(),
            profile_id: "profile".into(),
            pool_id: pool.into(),
            model: id.into(),
            resolved_model_version: Some(format!("{id}-v1")),
            effort: "medium".into(),
            tier: CapabilityTier::General,
            tools: ["browser".into()].into(),
            context_limit: Some(100_000),
            supports_approvals: true,
            sandbox: Sandbox::WorkspaceWrite,
            recommended_default: id == "costly",
            quota: Allowance::ObservedNonExhausted,
            quota_blocks: Vec::new(),
            fit: Fit::Unknown,
            health: Health::Healthy,
            unresolved_quota_pool_identity: false,
            in_flight_pool_claim: false,
        }
    }

    fn seeded(now: i64) -> (Store, String) {
        let store = Store::open(Path::new(":memory:")).unwrap();
        store
            .record_auto_account_identity("profile", &"a".repeat(64))
            .unwrap();
        let pool = store.auto_account_pool_id("profile").unwrap().unwrap();
        let snapshot = parse_codex_rate_limits(
            &json!({"rateLimits":{
                "limitId":"codex","planType":"pro",
                "primary":{"usedPercent":95,"resetsAt":1_800_003_600}
            }}),
            "profile",
            now,
        )
        .unwrap();
        let event = store
            .insert_event(now, None, None, "quota", "fixture", "reported", &json!({}))
            .unwrap();
        store
            .insert_auto_quota(event.seq, "profile", "fixture", &snapshot)
            .unwrap();
        (store, pool)
    }

    fn estimate(
        pool: &str,
        model: &str,
        upper_percent: f64,
        now: i64,
    ) -> crate::auto_select::AllowanceEstimate {
        serde_json::from_value(json!({"pool_id":pool,"model":model,
            "model_version":format!("{model}-v1"),"effort":"medium","plan_type":"pro",
            "task_signature":{"min_tier":"general","required_tools":["browser"],
                "context_needed":1000,"requires_approvals":false,
                "min_sandbox":"read_only","max_sandbox":"workspace_write"},
            "source":"attributed_actual_work","observed_ms":now,
            "windows":[{"bucket_id":"codex","window":"primary",
                "upper_percent":upper_percent}]}))
        .unwrap()
    }

    #[test]
    fn bounded_fit_excludes_costly_route_and_selects_suitable_efficient_route() {
        let now = 1_800_000_000_000_i64;
        let (store, pool) = seeded(now);
        for (model, draw) in [("costly", 8.0), ("efficient", 3.0)] {
            store
                .put_auto_allowance_estimate("profile", 1, &estimate(&pool, model, draw, now), now)
                .unwrap();
        }
        let mut routes = vec![route("costly", &pool), route("efficient", &pool)];
        let generations = BTreeMap::from([("profile".into(), 1)]);
        let evidence = apply_scoped_fit(&store, &work(), &mut routes, &generations, now);
        assert_eq!(
            routes[0].fit,
            Fit::Unaffordable,
            "8% exceeds the 5% remaining window"
        );
        assert_eq!(routes[1].fit, Fit::Fits, "3% fits the same window");
        assert_eq!(
            select(&work(), &routes).selected.as_deref(),
            Some("efficient")
        );
        let saved_inputs = json!({"work":work(), "routes":routes});
        let replay_work: WorkUnit = serde_json::from_value(saved_inputs["work"].clone()).unwrap();
        let replay_routes: Vec<Route> =
            serde_json::from_value(saved_inputs["routes"].clone()).unwrap();
        assert_eq!(
            select(&replay_work, &replay_routes).selected.as_deref(),
            Some("efficient"),
            "recorded normalized inputs reproduce the decision"
        );
        assert_eq!(evidence.len(), 2);
    }

    #[test]
    fn missing_version_or_changed_account_leaves_fit_unknown() {
        let now = 1_800_000_000_000_i64;
        let (store, pool) = seeded(now);
        store
            .put_auto_allowance_estimate("profile", 1, &estimate(&pool, "costly", 8.0, now), now)
            .unwrap();
        let mut routes = vec![route("costly", &pool)];
        routes[0].resolved_model_version = None;
        let generations = BTreeMap::from([("profile".into(), 1)]);
        let missing_version = apply_scoped_fit(&store, &work(), &mut routes, &generations, now);
        assert_eq!(routes[0].fit, Fit::Unknown);
        assert_eq!(missing_version[0]["reason"], "model_version_unverified");
        routes[0].resolved_model_version = Some("costly-v1".into());
        store
            .record_auto_account_identity("profile", &"b".repeat(64))
            .unwrap();
        let changed_account = apply_scoped_fit(&store, &work(), &mut routes, &generations, now);
        assert_eq!(routes[0].fit, Fit::Unknown);
        assert_eq!(changed_account[0]["reason"], "account_generation_changed");
    }

    #[test]
    fn two_profiles_on_one_account_cannot_ignore_a_tighter_second_meter() {
        let now = 1_800_000_000_000_i64;
        let (store, pool) = seeded(now);
        store
            .put_auto_allowance_estimate("profile", 1, &estimate(&pool, "efficient", 3.0, now), now)
            .unwrap();
        store
            .record_auto_account_identity("other-profile", &"a".repeat(64))
            .unwrap();
        let tighter = parse_codex_rate_limits(
            &json!({"rateLimits":{"limitId":"codex","planType":"pro",
                "primary":{"usedPercent":99,"resetsAt":1_800_003_600}}}),
            "other-profile",
            now,
        )
        .unwrap();
        let event = store
            .insert_event(now, None, None, "quota", "fixture", "reported", &json!({}))
            .unwrap();
        store
            .insert_auto_quota(event.seq, "other-profile", "fixture", &tighter)
            .unwrap();
        let mut routes = vec![route("efficient", &pool)];
        let generations = BTreeMap::from([("profile".into(), 1)]);
        apply_scoped_fit(&store, &work(), &mut routes, &generations, now);
        assert_eq!(
            routes[0].fit,
            Fit::Unknown,
            "one of two same-account meters leaves only 1% room for a 3% estimate"
        );
    }

    #[test]
    fn locked_learning_cannot_multiply_one_busy_wait_by_hundred_routes() {
        let now = 1_800_000_000_000_i64;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.sqlite");
        let store = Store::open(&path).unwrap();
        store
            .record_auto_account_identity("profile", &"a".repeat(64))
            .unwrap();
        let pool = store.auto_account_pool_id("profile").unwrap().unwrap();
        let snapshot = parse_codex_rate_limits(
            &json!({"rateLimits":{
                "limitId":"codex","planType":"pro",
                "primary":{"usedPercent":50,"resetsAt":1_800_003_600}
            }}),
            "profile",
            now,
        )
        .unwrap();
        let event = store
            .insert_event(now, None, None, "quota", "fixture", "reported", &json!({}))
            .unwrap();
        store
            .insert_auto_quota(event.seq, "profile", "fixture", &snapshot)
            .unwrap();
        let learning =
            rusqlite::Connection::open(dir.path().join("state.sqlite.learning")).unwrap();
        learning.execute_batch("BEGIN EXCLUSIVE").unwrap();
        let mut routes = (0..100)
            .map(|index| route(&format!("model-{index}"), &pool))
            .collect::<Vec<_>>();
        let generations = BTreeMap::from([("profile".into(), 1)]);
        let started = std::time::Instant::now();
        let evidence = apply_scoped_fit(&store, &work(), &mut routes, &generations, now);
        let elapsed = started.elapsed();
        learning.execute_batch("COMMIT").unwrap();
        assert!(
            elapsed < std::time::Duration::from_secs(1),
            "learning lock multiplied across 100 routes: {elapsed:?}"
        );
        assert!(routes.iter().all(|route| route.fit == Fit::Unknown));
        assert_eq!(evidence.len(), 100);
    }
}
