//! Read-only integration of local allowance evidence with Auto route fit.

use crate::{
    auto_consumption::EstimateKey,
    auto_quota::QuotaSnapshot,
    auto_select::{
        self, Allowance, AllowanceEstimate, DrawSource, Fit, Route, TaskSignature, WindowDraw,
        WorkUnit,
    },
    store::Store,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

enum SnapshotEvidence {
    Available(Vec<QuotaSnapshot>),
    Unavailable,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub(crate) enum FitEvidenceInput {
    Unavailable {
        reason: String,
    },
    Observed {
        snapshots: Vec<QuotaSnapshot>,
        estimate: Option<AllowanceEstimate>,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct FitEvidenceResult {
    pub fit: Fit,
    pub reason: String,
    pub source: Option<DrawSource>,
    pub observed_ms: Option<i64>,
    pub expected_windows: Vec<WindowDraw>,
}

pub(crate) fn evaluate_fit(
    work: &WorkUnit,
    route: &Route,
    input: &FitEvidenceInput,
    now_ms: i64,
) -> FitEvidenceResult {
    let mut result = FitEvidenceResult {
        fit: Fit::Unknown,
        reason: "no_comparable_estimate".into(),
        source: None,
        observed_ms: None,
        expected_windows: Vec::new(),
    };
    let (snapshots, estimate) = match input {
        FitEvidenceInput::Unavailable { reason } => {
            result.reason = reason.clone();
            return result;
        }
        FitEvidenceInput::Observed {
            snapshots,
            estimate,
        } => (snapshots, estimate),
    };
    let Some(estimate) = estimate else {
        return result;
    };
    result.source = Some(estimate.source);
    result.observed_ms = Some(estimate.observed_ms);
    result.expected_windows = estimate.windows.clone();
    if snapshots.is_empty() {
        result.reason = "quota_snapshot_unavailable".into();
        return result;
    }
    let mut all_fit = true;
    for snapshot in snapshots {
        match auto_select::assess_fit(snapshot, work, route, Some(estimate), &[], now_ms) {
            Fit::Unaffordable => {
                result.fit = Fit::Unaffordable;
                break;
            }
            Fit::Unknown => all_fit = false,
            Fit::Fits => {}
        }
    }
    if result.fit != Fit::Unaffordable && all_fit {
        result.fit = Fit::Fits;
    }
    result.reason = match result.fit {
        Fit::Fits => "bounded_estimate_fits",
        Fit::Unaffordable => "bounded_estimate_exceeds_allowance",
        Fit::Unknown => "estimate_or_quota_not_comparable",
    }
    .into();
    result
}

pub(crate) fn bound_fit_inputs(inputs: &mut [FitEvidenceInput]) {
    const MAX_TRACE_BYTES: usize = 128 * 1024;
    const MAX_SNAPSHOTS: usize = 16;
    const MAX_WINDOWS: usize = 16;
    let too_large = inputs.len() > 128
        || inputs.iter().any(|input| match input {
            FitEvidenceInput::Unavailable { reason } => reason.len() > 80,
            FitEvidenceInput::Observed {
                snapshots,
                estimate,
            } => {
                snapshots.len() > MAX_SNAPSHOTS
                    || snapshots
                        .iter()
                        .any(|snapshot| snapshot.windows.len() > MAX_WINDOWS)
                    || estimate
                        .as_ref()
                        .is_some_and(|estimate| estimate.windows.len() > MAX_WINDOWS)
            }
        })
        || serde_json::to_vec(inputs).map_or(true, |bytes| bytes.len() > MAX_TRACE_BYTES);
    if too_large {
        for input in inputs {
            *input = FitEvidenceInput::Unavailable {
                reason: "fit_trace_budget_exceeded".into(),
            };
        }
    }
}

/// Apply only same-account, same-plan, same-task, resolved-version evidence.
/// Unknown-draw pool claims remain the separate durable admission boundary.
pub fn apply_scoped_fit(
    store: &Store,
    work: &WorkUnit,
    routes: &mut [Route],
    account_generations: &BTreeMap<String, i64>,
    now_ms: i64,
) -> Vec<Value> {
    apply_scoped_fit_with_inputs(store, work, routes, account_generations, now_ms).1
}

fn unavailable(reason: &str) -> FitEvidenceInput {
    FitEvidenceInput::Unavailable {
        reason: reason.into(),
    }
}

/// Capture the bounded normalized evidence before selecting. The returned
/// inputs are the only source from which this decision's fit is calculated.
pub(crate) fn apply_scoped_fit_with_inputs(
    store: &Store,
    work: &WorkUnit,
    routes: &mut [Route],
    account_generations: &BTreeMap<String, i64>,
    now_ms: i64,
) -> (Vec<FitEvidenceInput>, Vec<Value>) {
    let mut quota_by_profile = BTreeMap::<String, SnapshotEvidence>::new();
    let started = Instant::now();
    let mut learning_unavailable = false;
    let mut inputs = Vec::with_capacity(routes.len());
    for route in routes.iter() {
        if route.quota != Allowance::ObservedNonExhausted {
            inputs.push(unavailable("allowance_not_observed_nonexhausted"));
            continue;
        }
        let Some(version) = route
            .resolved_model_version
            .as_deref()
            .filter(|v| !v.is_empty())
        else {
            inputs.push(unavailable("model_version_unverified"));
            continue;
        };
        if work.task_class.as_deref().is_none_or(str::is_empty)
            || work.execution_budget_ms.is_none_or(|budget| budget == 0)
        {
            inputs.push(unavailable("predictive_task_class_unavailable"));
            continue;
        }
        let Some(generation) = account_generations.get(&route.profile_id) else {
            inputs.push(unavailable("account_generation_unavailable"));
            continue;
        };
        if store
            .auto_account_generation(&route.profile_id)
            .ok()
            .flatten()
            != Some(*generation)
        {
            inputs.push(unavailable("account_generation_changed"));
            continue;
        }
        let snapshot_evidence = quota_by_profile
            .entry(route.profile_id.clone())
            .or_insert_with(
                || match store.auto_account_quota_observations(&route.profile_id) {
                    Ok(observations) if !observations.is_empty() => SnapshotEvidence::Available(
                        observations
                            .into_iter()
                            .map(|reading| reading.snapshot)
                            .collect(),
                    ),
                    _ => SnapshotEvidence::Unavailable,
                },
            );
        let SnapshotEvidence::Available(snapshots) = snapshot_evidence else {
            inputs.push(unavailable("quota_snapshot_unavailable"));
            continue;
        };
        let plan = snapshots
            .first()
            .and_then(QuotaSnapshot::reported_plan_type);
        let Some(plan) = plan.filter(|first| {
            snapshots
                .iter()
                .all(|snapshot| snapshot.reported_plan_type() == Some(*first))
        }) else {
            inputs.push(unavailable("account_plan_unreconciled"));
            continue;
        };
        if learning_unavailable {
            inputs.push(unavailable("learning_unavailable"));
            continue;
        }
        if started.elapsed() >= Duration::from_millis(500) {
            inputs.push(unavailable("fit_read_budget_elapsed"));
            continue;
        }
        let key = EstimateKey {
            pool_id: route.pool_id.clone(),
            model: route.model.clone(),
            effort: route.effort.clone(),
            model_version: version.into(),
            task_signature: TaskSignature::from(work),
            plan_type: plan.into(),
        };
        match store.auto_allowance_estimate(&route.profile_id, *generation, &key, now_ms) {
            Ok(estimate) => inputs.push(FitEvidenceInput::Observed {
                snapshots: snapshots.clone(),
                estimate,
            }),
            Err(_) => {
                learning_unavailable = true;
                inputs.push(unavailable("learning_unavailable"));
            }
        }
    }
    bound_fit_inputs(&mut inputs);
    let mut evidence = Vec::with_capacity(routes.len());
    for (route, input) in routes.iter_mut().zip(&inputs) {
        let result = evaluate_fit(work, route, input, now_ms);
        route.fit = result.fit;
        evidence.push(
            json!({"route_id":route.id,"fit":result.fit,"reason":result.reason,
            "source":result.source,"observed_ms":result.observed_ms,
            "expected_windows":result.expected_windows}),
        );
    }
    (inputs, evidence)
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
            task_class: Some("fixture_browser".into()),
            execution_budget_ms: Some(10_000),
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
                "min_sandbox":"read_only","max_sandbox":"workspace_write",
                "task_class":"fixture_browser","execution_budget_ms":10000},
            "prediction_basis":{"task_class":"fixture_browser",
                "max_execution_budget_ms":10000,"sample_count":5,
                "method":"bounded_class_upper_v1"},
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
            Fit::Unaffordable,
            "the tighter same-account meter leaves only 1% room for a 3% bound"
        );
        assert_eq!(select(&work(), &routes).selected, None);
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

    #[test]
    fn serialized_meter_inputs_recompute_fit_and_detect_a_changed_window() {
        let now = 1_800_000_000_000_i64;
        let (store, pool) = seeded(now);
        store
            .put_auto_allowance_estimate("profile", 1, &estimate(&pool, "efficient", 3.0, now), now)
            .unwrap();
        store
            .record_auto_account_identity("other-profile", &"a".repeat(64))
            .unwrap();
        let tighter = parse_codex_rate_limits(
            &json!({"rateLimits":{
                "limitId":"codex","planType":"pro",
                "primary":{"usedPercent":99,"resetsAt":1_800_003_600}
            }}),
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
        let (inputs, _) =
            apply_scoped_fit_with_inputs(&store, &work(), &mut routes, &generations, now);
        assert_eq!(routes[0].fit, Fit::Unaffordable);
        let saved = serde_json::to_value(&inputs).unwrap();
        let mut restored: Vec<FitEvidenceInput> = serde_json::from_value(saved).unwrap();
        assert_eq!(
            evaluate_fit(&work(), &routes[0], &restored[0], now).fit,
            Fit::Unaffordable
        );
        if let FitEvidenceInput::Observed { snapshots, .. } = &mut restored[0] {
            let tight = snapshots
                .iter_mut()
                .find(|snapshot| {
                    snapshot
                        .windows
                        .iter()
                        .any(|window| window.used_percent == 99.0)
                })
                .unwrap();
            tight.windows[0].used_percent = 90.0;
        } else {
            panic!("expected normalized meter inputs");
        }
        assert_eq!(
            evaluate_fit(&work(), &routes[0], &restored[0], now).fit,
            Fit::Fits
        );
    }

    #[test]
    fn oversized_evidence_never_retains_a_numeric_fit_without_its_inputs() {
        let now = 1_800_000_000_000_i64;
        let (store, pool) = seeded(now);
        let snapshot = store
            .latest_auto_quota("profile")
            .unwrap()
            .unwrap()
            .snapshot;
        let mut inputs = vec![FitEvidenceInput::Observed {
            snapshots: vec![snapshot; 17],
            estimate: Some(estimate(&pool, "efficient", 3.0, now)),
        }];
        bound_fit_inputs(&mut inputs);
        let route = route("efficient", &pool);
        assert_eq!(
            evaluate_fit(&work(), &route, &inputs[0], now).fit,
            Fit::Unknown
        );
        assert!(
            matches!(&inputs[0], FitEvidenceInput::Unavailable { reason }
            if reason == "fit_trace_budget_exceeded")
        );
    }
}
