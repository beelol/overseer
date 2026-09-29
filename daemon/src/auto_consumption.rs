//! Allowlisted provider estimates from an already executed Codex thread.
//! Credits are provider-estimated thread activity, not subscription-window percent.

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Comparable subscription-window estimate identity. This deliberately
/// excludes account-profile display names and never derives from tokens.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct EstimateKey {
    pub pool_id: String,
    pub model: String,
    pub effort: String,
    pub model_version: String,
    pub task_signature: crate::auto_select::TaskSignature,
    pub plan_type: String,
}

impl EstimateKey {
    pub fn from_estimate(estimate: &crate::auto_select::AllowanceEstimate) -> Option<Self> {
        let model_version = estimate.model_version.as_deref().filter(|value| !value.is_empty())?;
        let task_signature = estimate.task_signature.as_ref()?.clone();
        let plan_type = estimate.plan_type.as_deref().filter(|value| !value.is_empty())?;
        if estimate.pool_id.is_empty() || estimate.model.is_empty() || estimate.effort.is_empty() {
            return None;
        }
        Some(Self { pool_id:estimate.pool_id.clone(), model:estimate.model.clone(),
            effort:estimate.effort.clone(), model_version:model_version.into(),
            task_signature, plan_type:plan_type.into() })
    }
}

const MAX_CREDITS_MICROS: u64 = 1_000_000_000_000_000;
const MAX_TOKENS: u64 = 1_000_000_000_000;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CreditGroup {
    pub model: Option<String>,
    pub effort: Option<String>,
    pub estimated_credits_micros: u64,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cached_input_tokens: Option<u64>,
    pub net_new_input_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct StoredThreadUsageObservation {
    pub id: i64,
    pub run_id: String,
    pub profile_id: String,
    pub read_account_generation: i64,
    pub attribution: String,
    pub subscription_window_relation: String,
    pub source: String,
    pub estimate: ThreadUsageEstimate,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ThreadUsageEstimate {
    pub observed_ms: i64,
    #[serde(default)]
    pub plan_type: Option<String>,
    pub estimated_credits_micros: u64,
    pub groups: Vec<CreditGroup>,
}

fn identifier(value: Option<&Value>) -> Result<Option<String>> {
    let Some(value) = value.filter(|v| !v.is_null()) else {
        return Ok(None);
    };
    let s = value
        .as_str()
        .ok_or_else(|| anyhow!("invalid thread usage identifier"))?;
    if s.is_empty()
        || s.len() > 120
        || !s
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'/'))
    {
        return Err(anyhow!("unsupported thread usage identifier"));
    }
    Ok(Some(s.to_string()))
}

fn credits(value: Option<&Value>) -> Result<u64> {
    value
        .and_then(Value::as_u64)
        .filter(|n| *n <= MAX_CREDITS_MICROS)
        .ok_or_else(|| anyhow!("invalid estimated usage credits"))
}

fn tokens(value: Option<&Value>) -> Result<Option<u64>> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_u64()
            .filter(|n| *n <= MAX_TOKENS)
            .map(Some)
            .ok_or_else(|| anyhow!("invalid thread token count")),
    }
}

/// Read a billing-route estimate only for the requested thread. Account-wide
/// token activity is ignored. This is not a reported subscription-window draw;
/// a zero or absent estimate must not be interpreted as free work.
pub fn parse_codex_thread_usage(
    raw: &Value,
    expected_thread: &str,
    observed_ms: i64,
) -> Result<Option<ThreadUsageEstimate>> {
    let Some(value) = raw.get("threadUsage").filter(|v| !v.is_null()) else {
        return Ok(None);
    };
    let id = value
        .get("threadId")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("thread usage identity unavailable"))?;
    if id != expected_thread {
        return Err(anyhow!("thread usage belongs to another thread"));
    }
    let total = credits(value.get("estimatedUsageCreditsMicros"))?;
    let rows = value
        .get("groups")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("thread usage groups unavailable"))?;
    if rows.len() > 64 {
        return Err(anyhow!("too many thread usage groups"));
    }
    let mut groups = Vec::new();
    let mut sum = 0_u64;
    for row in rows {
        let charge = credits(row.get("estimatedUsageCreditsMicros"))?;
        sum = sum
            .checked_add(charge)
            .filter(|n| *n <= MAX_CREDITS_MICROS)
            .ok_or_else(|| anyhow!("thread usage sum overflow"))?;
        groups.push(CreditGroup {
            model: identifier(row.get("model"))?,
            effort: identifier(row.get("reasoningEffort"))?,
            estimated_credits_micros: charge,
            input_tokens: tokens(row.get("inputTokens"))?,
            output_tokens: tokens(row.get("outputTokens"))?,
            cached_input_tokens: tokens(row.get("cachedInputTokens"))?,
            net_new_input_tokens: tokens(row.get("netNewInputTokens"))?,
            total_tokens: tokens(row.get("totalTokens"))?,
        });
    }
    if sum != total {
        return Err(anyhow!("thread usage group total is inconsistent"));
    }
    Ok(Some(ThreadUsageEstimate {
        observed_ms,
        plan_type: None,
        estimated_credits_micros: total,
        groups,
    }))
}

/// Conditions for attributing a *window percentage* change to one completed
/// work unit. An adapter may set the error bound only after validating that
/// provider meter's precision. These flags are evidence, not user policy.
#[derive(Clone, Debug)]
pub struct DeltaContext<'a> {
    pub model: Option<&'a str>,
    pub effort: Option<&'a str>,
    pub resolved_model_version: Option<&'a str>,
    pub task_signature: Option<&'a crate::auto_select::TaskSignature>,
    pub same_account_generation: bool,
    pub model_version_stable: bool,
    pub local_overlap_excluded: bool,
    pub external_usage_excluded: bool,
    pub reporting_settled: bool,
    /// Maximum absolute error of each reported percentage, in percentage points.
    pub meter_error_percent: Option<f64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct WindowDeltaInterval {
    pub pool_id: String,
    pub bucket_id: String,
    pub window: String,
    pub model: Option<String>,
    pub model_family: Option<String>,
    pub lower_percent: f64,
    pub upper_percent: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct WindowDeltaAssessment {
    pub state: String,
    pub reasons: Vec<String>,
    pub windows: Vec<WindowDeltaInterval>,
}

/// Compare normalized readings without interpreting tokens or estimated thread
/// credits as a subscription charge. A visible zero is never proof of free work.
/// Every applicable window must have the same pool, scope, plan and reset.
pub fn assess_window_delta(
    before: Option<&crate::auto_quota::QuotaSnapshot>,
    after: &crate::auto_quota::QuotaSnapshot,
    context: &DeltaContext<'_>,
) -> WindowDeltaAssessment {
    let mut reasons = Vec::<String>::new();
    let mut add = |reason: &str| {
        if !reasons.iter().any(|existing| existing == reason) { reasons.push(reason.into()); }
    };
    if !context.same_account_generation { add("account_generation_unverified"); }
    if context.model.is_none() { add("model_unknown"); }
    if context.effort.is_none() { add("effort_unknown"); }
    if !context.model_version_stable { add("model_version_unverified"); }
    if !context.local_overlap_excluded { add("local_overlap_unexcluded"); }
    if !context.external_usage_excluded { add("external_usage_unexcluded"); }
    if !context.reporting_settled { add("reporting_not_settled"); }
    let error = context.meter_error_percent.filter(|value|
        value.is_finite() && (0.0..=100.0).contains(value));
    if error.is_none() { add("meter_precision_unknown"); }
    let Some(before) = before else {
        add("pre_turn_read_missing");
        return WindowDeltaAssessment { state:"unverified".into(), reasons, windows:Vec::new() };
    };
    if after.observed_ms <= before.observed_ms { add("observation_order_invalid"); }
    if before.ordinary_usage_allowed == Some(false) || after.ordinary_usage_allowed == Some(false) {
        add("account_denied");
    }
    match (before.reported_plan_type(), after.reported_plan_type()) {
        (Some(left), Some(right)) if left == right => {}
        (Some(_), Some(_)) => add("account_plan_changed"),
        _ => add("account_plan_unknown"),
    }
    let Some(model) = context.model else {
        return WindowDeltaAssessment { state:"unverified".into(), reasons, windows:Vec::new() };
    };
    let earlier = before.applicable_to(model);
    let later = after.applicable_to(model);
    if earlier.is_empty() || earlier.len() != later.len() { add("window_scope_changed"); }
    let mut intervals = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for old in earlier {
        let key = (&old.pool_id, &old.bucket_id, &old.window, &old.model, &old.model_family);
        if !seen.insert(key) { add("window_scope_duplicated"); continue; }
        let matches = later.iter().copied().filter(|new|
            new.pool_id == old.pool_id && new.bucket_id == old.bucket_id
                && new.window == old.window && new.model == old.model
                && new.model_family == old.model_family).collect::<Vec<_>>();
        if matches.len() != 1 { add("window_scope_changed"); continue; }
        let new = matches[0];
        match (&old.plan_type, &new.plan_type) {
            (Some(left), Some(right)) if left == right => {}
            (Some(_), Some(_)) => add("account_plan_changed"),
            _ => add("account_plan_unknown"),
        }
        if old.reset_ms.is_none() || new.reset_ms.is_none() {
            add("window_reset_unknown");
        } else if old.reset_ms != new.reset_ms
            || old.reset_ms.is_some_and(|reset| reset <= after.observed_ms) {
            add("window_reset_changed");
        }
        if !old.used_percent.is_finite() || !new.used_percent.is_finite()
            || !(0.0..=100.0).contains(&old.used_percent)
            || !(0.0..=100.0).contains(&new.used_percent) {
            add("meter_invalid");
            continue;
        }
        let visible = new.used_percent - old.used_percent;
        if visible < 0.0 { add("meter_decreased"); }
        if visible == 0.0 { add("zero_visible_delta"); }
        if let Some(error) = error {
            let lower = (visible - 2.0 * error).max(0.0);
            let upper = (visible + 2.0 * error).min(100.0);
            if lower == 0.0 && upper == 100.0 { add("meter_too_coarse"); }
            intervals.push(WindowDeltaInterval { pool_id:old.pool_id.clone(),
                bucket_id:old.bucket_id.clone(), window:old.window.clone(),
                model:old.model.clone(), model_family:old.model_family.clone(),
                lower_percent:lower, upper_percent:upper });
        }
    }
    if reasons.is_empty() {
        WindowDeltaAssessment { state:"bounded".into(), reasons, windows:intervals }
    } else {
        WindowDeltaAssessment { state:"unverified".into(), reasons, windows:Vec::new() }
    }
}

/// Promote only a fully bounded, attributed actual-work delta to the same
/// window units used by admission. The caller must establish the context's
/// non-overlap and settled-meter claims; absent proof leaves this unavailable.
/// This deliberately does not convert thread credits or token counts.
pub fn estimate_from_actual_window_delta(
    before: Option<&crate::auto_quota::QuotaSnapshot>,
    after: &crate::auto_quota::QuotaSnapshot,
    context: &DeltaContext<'_>,
) -> Option<crate::auto_select::AllowanceEstimate> {
    use crate::auto_select::{AllowanceEstimate, DrawSource, WindowDraw};
    let assessment = assess_window_delta(before, after, context);
    if assessment.state != "bounded" || assessment.windows.is_empty() {
        return None;
    }
    let model = context.model.filter(|value| !value.is_empty())?;
    let effort = context.effort.filter(|value| !value.is_empty())?;
    let model_version = context.resolved_model_version
        .filter(|value| !value.is_empty())?;
    let task_signature = context.task_signature?.clone();
    let plan_type = after.reported_plan_type()?.to_string();
    let pool_id = assessment.windows.first()?.pool_id.clone();
    if pool_id.is_empty() { return None; }
    let mut seen = std::collections::BTreeSet::new();
    let mut windows = Vec::with_capacity(assessment.windows.len());
    for window in assessment.windows {
        if window.pool_id != pool_id || window.bucket_id.is_empty() || window.window.is_empty()
            || !window.upper_percent.is_finite() || window.upper_percent <= 0.0
            || window.upper_percent > 100.0
            || !seen.insert((window.bucket_id.clone(), window.window.clone())) {
            return None;
        }
        windows.push(WindowDraw { bucket_id:window.bucket_id,
            window:window.window, upper_percent:window.upper_percent });
    }
    Some(AllowanceEstimate { pool_id, model:model.into(), effort:effort.into(),
        model_version:Some(model_version.into()), task_signature:Some(task_signature),
        plan_type:Some(plan_type), source:DrawSource::AttributedActualWork,
        observed_ms:after.observed_ms, windows, prediction_basis:None })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn thread_usage_keeps_scoped_model_effort_credit_estimates_only() {
        let raw = json!({"summary":{"lifetimeTokens":999999},"threadUsage":{
            "threadId":"thread-1","estimatedUsageCreditsMicros":2500000,
            "estimatedUsageUsdMicros":1234,"secret":"private-sentinel",
            "groups":[{"model":"gpt-6-sol","reasoningEffort":"medium","speed":"fast",
                "estimatedUsageCreditsMicros":2500000,"inputTokens":100,"outputTokens":20,
                "cachedInputTokens":10,"netNewInputTokens":90,"totalTokens":120,
                "prompt":"secret-prompt-sentinel"}]
        }});
        let observation = parse_codex_thread_usage(&raw, "thread-1", 1000)
            .unwrap()
            .unwrap();
        assert_eq!(observation.estimated_credits_micros, 2_500_000);
        assert_eq!(observation.groups[0].model.as_deref(), Some("gpt-6-sol"));
        assert_eq!(observation.groups[0].effort.as_deref(), Some("medium"));
        assert_eq!(observation.groups[0].input_tokens, Some(100));
        let output = serde_json::to_string(&observation).unwrap();
        assert!(!output.contains("private-sentinel"));
        assert!(!output.contains("secret-prompt-sentinel"));
        assert!(!output.contains("lifetimeTokens"));
        assert!(!output.contains("estimatedUsageUsdMicros"));
    }

    #[test]
    fn isolated_same_window_delta_is_a_bounded_interval_not_a_point_price() {
        use crate::auto_quota::parse_codex_rate_limits;
        let at = 1_800_000_000_000_i64;
        let reading = |used: f64, reset: i64, plan: &str, observed| {
            parse_codex_rate_limits(&json!({"rateLimits":{"limitId":"codex",
                "planType":plan,"primary":{"usedPercent":used,"resetsAt":reset}}}),
                "pool-1", observed).unwrap()
        };
        let before = reading(40.0, 1_800_003_600, "pro", at);
        let after = reading(42.0, 1_800_003_600, "pro", at + 20_000);
        let trusted = DeltaContext { model:Some("gpt-6-sol"), effort:Some("medium"),
            resolved_model_version:None, task_signature:None,
            same_account_generation:true, model_version_stable:true,
            local_overlap_excluded:true, external_usage_excluded:true,
            reporting_settled:true, meter_error_percent:Some(0.1) };
        let bounded = assess_window_delta(Some(&before), &after, &trusted);
        assert_eq!(bounded.state, "bounded");
        assert!(bounded.reasons.is_empty());
        assert_eq!(bounded.windows.len(), 1);
        assert!((bounded.windows[0].lower_percent - 1.8).abs() < 1e-9);
        assert!((bounded.windows[0].upper_percent - 2.2).abs() < 1e-9);
        let rounded_zero = reading(40.0, 1_800_003_600, "pro", at + 20_000);
        let uncertain = assess_window_delta(Some(&before), &rounded_zero, &trusted);
        assert_eq!(uncertain.state, "unverified");
        assert!(uncertain.reasons.contains(&"zero_visible_delta".to_string()));
        assert!(uncertain.windows.is_empty(), "zero visible change is not free work");
    }

    #[test]
    fn verified_actual_work_delta_becomes_a_scoped_positive_fit_estimate() {
        use crate::auto_quota::parse_codex_rate_limits;
        use crate::auto_select::{assess_fit, Allowance, CapabilityTier, Fit, Health, Route, Sandbox,
            TaskSignature, WorkUnit};
        use std::collections::BTreeSet;
        let at = 1_800_000_000_000_i64;
        let reading = |used: f64, observed| parse_codex_rate_limits(
            &json!({"rateLimits":{"limitId":"codex","planType":"pro",
                "primary":{"usedPercent":used,"resetsAt":1_800_003_600}}}),
            "pool-1", observed).unwrap();
        let before = reading(40.0, at);
        let after = reading(42.0, at + 20_000);
        let work = WorkUnit { id:"actual-unit".into(), min_tier:CapabilityTier::General,
            required_tools:BTreeSet::new(), context_needed:0, requires_approvals:false,
            min_sandbox:Sandbox::WorkspaceWrite, max_sandbox:Sandbox::WorkspaceWrite,
            allowed_profiles:["profile".into()].into(), pinned_route:None,
            preferred_harness:None, task_class:None, execution_budget_ms:None };
        let task_signature = TaskSignature::from(&work);
        let trusted = DeltaContext { model:Some("gpt-6-sol"), effort:Some("medium"),
            resolved_model_version:Some("gpt-6-sol-resolved-v1"),
            task_signature:Some(&task_signature),
            same_account_generation:true, model_version_stable:true,
            local_overlap_excluded:true, external_usage_excluded:true,
            reporting_settled:true, meter_error_percent:Some(0.1) };
        let estimate = estimate_from_actual_window_delta(Some(&before), &after, &trusted)
            .expect("credible actual work should yield a scoped estimate");
        assert_eq!(estimate.pool_id, "pool-1");
        assert_eq!(estimate.model, "gpt-6-sol");
        assert_eq!(estimate.effort, "medium");
        assert_eq!(estimate.plan_type.as_deref(), Some("pro"));
        assert_eq!(estimate.source, crate::auto_select::DrawSource::AttributedActualWork);
        assert!(estimate.prediction_basis.is_none(),
            "an attributed completed-work interval is not a future-work prediction");
        assert_eq!(estimate.windows.len(), 1);
        assert!((estimate.windows[0].upper_percent - 2.2).abs() < 1e-9);
        let route = Route { id:"sol".into(), harness:"codex-app".into(),
            provider:"openai".into(), endpoint:"codex".into(), profile_id:"profile".into(),
            pool_id:"pool-1".into(), model:"gpt-6-sol".into(),
            resolved_model_version:Some("gpt-6-sol-resolved-v1".into()),
            effort:"medium".into(),
            tier:CapabilityTier::General, tools:BTreeSet::new(), context_limit:None,
            supports_approvals:true, sandbox:Sandbox::WorkspaceWrite,
            supported_sandboxes:None,
            recommended_default:true, quota:Allowance::ObservedNonExhausted,
            quota_blocks:Vec::new(), fit:Fit::Unknown, health:Health::Healthy,
            unresolved_quota_pool_identity:false, in_flight_pool_claim:false, endpoint_recovery_in_flight:false };
        assert_eq!(assess_fit(&reading(96.0, at + 30_000), &work, &route,
            Some(&estimate), &[], at + 30_000), Fit::Unknown);
        assert_eq!(assess_fit(&reading(98.0, at + 30_000), &work, &route,
            Some(&estimate), &[], at + 30_000), Fit::Unknown);
    }

    #[test]
    fn bounded_delta_without_resolved_model_and_task_scope_is_not_a_route_estimate() {
        use crate::auto_quota::parse_codex_rate_limits;
        let at = 1_800_000_000_000_i64;
        let reading = |used: f64, observed| parse_codex_rate_limits(
            &json!({"rateLimits":{"limitId":"codex","planType":"pro",
                "primary":{"usedPercent":used,"resetsAt":1_800_003_600}}}),
            "pool-1", observed).unwrap();
        let context = DeltaContext { model:Some("gpt-6-sol"), effort:Some("medium"),
            resolved_model_version:None, task_signature:None,
            same_account_generation:true, model_version_stable:true,
            local_overlap_excluded:true, external_usage_excluded:true,
            reporting_settled:true, meter_error_percent:Some(0.1) };
        assert!(estimate_from_actual_window_delta(Some(&reading(40.0, at)),
            &reading(42.0, at + 20_000), &context).is_none());
    }

    #[test]
    fn actual_window_estimate_rejects_ambiguous_or_zero_draw() {
        use crate::auto_quota::parse_codex_rate_limits;
        let at = 1_800_000_000_000_i64;
        let reading = |used: f64, observed| parse_codex_rate_limits(
            &json!({"rateLimits":{"limitId":"codex","planType":"pro",
                "primary":{"usedPercent":used,"resetsAt":1_800_003_600}}}),
            "pool-1", observed).unwrap();
        let before = reading(40.0, at);
        let after = reading(42.0, at + 20_000);
        let mut context = DeltaContext { model:Some("gpt-6-sol"), effort:Some("medium"),
            resolved_model_version:None, task_signature:None,
            same_account_generation:true, model_version_stable:true,
            local_overlap_excluded:true, external_usage_excluded:true,
            reporting_settled:true, meter_error_percent:Some(0.1) };
        assert!(estimate_from_actual_window_delta(None, &after, &context).is_none());
        assert!(estimate_from_actual_window_delta(Some(&before),
            &reading(40.0, at + 20_000), &context).is_none());
        context.external_usage_excluded = false;
        assert!(estimate_from_actual_window_delta(Some(&before), &after, &context).is_none());
        context.external_usage_excluded = true;
        context.model_version_stable = false;
        assert!(estimate_from_actual_window_delta(Some(&before), &after, &context).is_none());
        context.model_version_stable = true;
        context.effort = None;
        assert!(estimate_from_actual_window_delta(Some(&before), &after, &context).is_none());
    }

    #[test]
    fn allowance_delta_rejects_reset_overlap_external_work_and_unverified_precision() {
        use crate::auto_quota::parse_codex_rate_limits;
        let at = 1_800_000_000_000_i64;
        let reading = |used: f64, reset: i64, plan: &str, observed| {
            parse_codex_rate_limits(&json!({"rateLimits":{"limitId":"codex",
                "planType":plan,"primary":{"usedPercent":used,"resetsAt":reset}}}),
                "pool-1", observed).unwrap()
        };
        let before = reading(40.0, 1_800_003_600, "pro", at);
        let after = reading(42.0, 1_800_003_600, "pro", at + 20_000);
        let mut context = DeltaContext { model:Some("gpt-6-sol"), effort:Some("medium"),
            resolved_model_version:None, task_signature:None,
            same_account_generation:true, model_version_stable:true,
            local_overlap_excluded:true, external_usage_excluded:true,
            reporting_settled:true, meter_error_percent:Some(0.1) };
        context.external_usage_excluded = false;
        context.local_overlap_excluded = false;
        context.reporting_settled = false;
        context.meter_error_percent = None;
        let uncertain = assess_window_delta(Some(&before), &after, &context);
        assert_eq!(uncertain.state, "unverified");
        for reason in ["local_overlap_unexcluded", "external_usage_unexcluded",
            "reporting_not_settled", "meter_precision_unknown"] {
            assert!(uncertain.reasons.contains(&reason.to_string()), "{uncertain:?}");
        }
        assert!(uncertain.windows.is_empty());
        context.local_overlap_excluded = true;
        context.external_usage_excluded = true;
        context.reporting_settled = true;
        context.meter_error_percent = Some(0.1);
        let reset = reading(42.0, 1_800_007_200, "pro", at + 20_000);
        assert!(assess_window_delta(Some(&before), &reset, &context).reasons.contains(&"window_reset_changed".to_string()));
        let plan = reading(42.0, 1_800_003_600, "plus", at + 20_000);
        assert!(assess_window_delta(Some(&before), &plan, &context).reasons.contains(&"account_plan_changed".to_string()));
        let denied = parse_codex_rate_limits(&json!({"ordinaryUsageAllowed":false,
            "rateLimits":{"limitId":"codex","planType":"pro",
            "primary":{"usedPercent":42.0,"resetsAt":1_800_003_600}}}),
            "pool-1", at + 20_000).unwrap();
        assert!(assess_window_delta(Some(&before), &denied, &context).reasons.contains(&"account_denied".to_string()));
        context.effort = None;
        assert!(assess_window_delta(Some(&before), &after, &context).reasons.contains(&"effort_unknown".to_string()));
        context.effort = Some("medium");
        context.model_version_stable = false;
        assert!(assess_window_delta(Some(&before), &after, &context).reasons.contains(&"model_version_unverified".to_string()));
        context.model_version_stable = true;
        context.meter_error_percent = Some(50.0);
        assert!(assess_window_delta(Some(&before), &after, &context).reasons.contains(&"meter_too_coarse".to_string()));
        context.meter_error_percent = Some(0.1);
        let out_of_order = reading(42.0, 1_800_003_600, "pro", at);
        assert!(assess_window_delta(Some(&before), &out_of_order, &context).reasons.contains(&"observation_order_invalid".to_string()));
        assert!(assess_window_delta(None, &after, &context).reasons.contains(&"pre_turn_read_missing".to_string()));
    }

    #[test]
    fn missing_route_and_malformed_or_mismatched_estimates_remain_unknown() {
        assert!(parse_codex_thread_usage(
            &json!({"summary":{"lifetimeTokens":100},"threadUsage":null}),
            "t",
            1000
        )
        .unwrap()
        .is_none());
        assert!(
            parse_codex_thread_usage(&json!({"summary":{"lifetimeTokens":100}}), "t", 1000)
                .unwrap()
                .is_none()
        );
        let valid = json!({"threadUsage":{"threadId":"t","estimatedUsageCreditsMicros":10,
            "groups":[{"model":"gpt-6-sol","reasoningEffort":"medium","estimatedUsageCreditsMicros":10}]}});
        assert!(parse_codex_thread_usage(&valid, "other", 1000).is_err());
        for bad in [json!(-1), json!("10"), json!(1.5)] {
            let mut value = valid.clone();
            value["threadUsage"]["estimatedUsageCreditsMicros"] = bad;
            assert!(parse_codex_thread_usage(&value, "t", 1000).is_err());
        }
        let mut mismatch = valid;
        mismatch["threadUsage"]["groups"][0]["estimatedUsageCreditsMicros"] = json!(9);
        assert!(parse_codex_thread_usage(&mismatch, "t", 1000).is_err());
    }
}
