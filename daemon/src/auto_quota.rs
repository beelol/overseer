//! Provider-scoped allowance observations for Auto Mode.

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

const FRESH_MS: i64 = 60_000;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct QuotaWindow {
    pub pool_id: String,
    pub bucket_id: String,
    pub window: String,
    pub model: Option<String>,
    #[serde(default)]
    pub model_family: Option<String>,
    #[serde(default)]
    pub plan_type: Option<String>,
    pub used_percent: f64,
    pub reset_ms: Option<i64>,
    pub duration_mins: Option<i64>,
    pub observed_ms: i64,
    pub expires_ms: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct QuotaSnapshot {
    pub ordinary_usage_allowed: Option<bool>,
    pub observed_ms: i64,
    pub expires_ms: i64,
    pub windows: Vec<QuotaWindow>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_uncertain_until_ms: Option<i64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct StoredQuotaObservation {
    pub event_seq: i64,
    pub pool_id: String,
    pub source: String,
    pub snapshot: QuotaSnapshot,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuotaState {
    ObservedNonExhausted,
    Exhausted,
    Unknown,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "scope", content = "value", rename_all = "snake_case")]
pub enum QuotaBlockScope {
    Account,
    Model(String),
    ModelFamily(String),
}

impl QuotaSnapshot {
    /// A retained observation can authorize capacity only until its own or
    /// any bucket's expiry/reset. Clock rollback also requires a new read.
    /// The caller decides whether its source supports a bounded refresh.
    pub fn needs_refresh(&self, now_ms: i64) -> bool {
        now_ms < self.observed_ms || now_ms >= self.expires_ms
            || self.windows.iter().any(|window| now_ms < window.observed_ms
                || now_ms >= window.expires_ms
                || window.reset_ms.is_some_and(|reset| reset <= now_ms))
    }

    /// Only a single, reported plan across every retained bucket can scope a
    /// cumulative thread sample. Missing or conflicting plans remain unknown.
    pub fn reported_plan_type(&self) -> Option<&str> {
        let first = self.windows.first()?.plan_type.as_deref()?;
        self.windows.iter().all(|window| window.plan_type.as_deref() == Some(first))
            .then_some(first)
    }

    /// Native notifications have arrival times but no proven provider order.
    /// A lower reading for the same window/reset, or a backward local clock,
    /// cannot establish fresh capacity. Keep explicit active blocks; a
    /// separate account-scoped metadata read may establish a real correction.
    pub fn reconcile_unordered_native_evidence(&mut self, prior: &QuotaSnapshot, now_ms: i64) {
        let decreased_until = prior.windows.iter().filter_map(|previous| {
            previous.reset_ms.is_none_or(|reset| reset > now_ms)
                .then_some(())
                .filter(|_| self.windows.iter().any(|current| {
                    current.pool_id == previous.pool_id
                        && current.bucket_id == previous.bucket_id
                        && current.window == previous.window
                        && current.model == previous.model
                        && current.model_family == previous.model_family
                        && current.plan_type == previous.plan_type
                        && current.reset_ms == previous.reset_ms
                        && current.used_percent < previous.used_percent
                }))
                .map(|_| previous.reset_ms.unwrap_or(i64::MAX))
        }).max();
        let backwards_until = (now_ms < prior.observed_ms).then(||
            prior.windows.iter().filter_map(|window| window.reset_ms)
                .filter(|reset| *reset > now_ms).max().unwrap_or(i64::MAX));
        self.native_uncertain_until_ms = prior.native_uncertain_until_ms
            .filter(|until| now_ms < *until)
            .into_iter().chain(decreased_until).chain(backwards_until).max();
        let has_current_meter = !self.windows.is_empty();
        if self.native_uncertain_until_ms.is_some() {
            self.windows.retain(|window| window.used_percent >= 100.0);
            if self.ordinary_usage_allowed == Some(true) {
                self.ordinary_usage_allowed = None;
            }
        }
        if prior.ordinary_usage_allowed == Some(false) && self.ordinary_usage_allowed != Some(true) {
            self.ordinary_usage_allowed = Some(false);
        }
        for window in &prior.windows {
            if window.used_percent >= 100.0 && window.reset_ms.is_some_and(|reset| reset <= now_ms) {
                continue;
            }
            let refreshed = self.windows.iter().any(|current| current.pool_id == window.pool_id
                && current.bucket_id == window.bucket_id && current.window == window.window
                && current.model == window.model && current.model_family == window.model_family);
            // A valid partial native frame may update one bucket while leaving
            // another untouched. Carry the omitted bucket with its original
            // freshness; an empty/malformed frame instead keeps only blocks.
            if !refreshed && (window.used_percent >= 100.0 || has_current_meter) {
                self.windows.push(window.clone());
            }
        }
    }

    pub fn applicable_to(&self, model: &str) -> Vec<&QuotaWindow> {
        self.windows
            .iter()
            .filter(|w| w.model.as_deref().is_none_or(|m| m == model)
                && w.model_family.as_deref().is_none_or(|family| model.split('-').any(|part| part == family)))
            .collect()
    }

    pub fn blocking_window(&self, model: &str) -> Option<&QuotaWindow> {
        self.applicable_to(model)
            .into_iter()
            .find(|w| w.used_percent >= 100.0)
    }

    pub fn allowance_unknown(&self, model: &str) -> bool {
        self.applicable_to(model).is_empty()
    }

    /// Report the scope of each active authoritative block. An account-wide
    /// denial supersedes narrower windows; model and family limits must not
    /// exclude sibling models in the same account pool.
    pub fn blocking_scopes(&self, model: &str, now_ms: i64) -> Vec<QuotaBlockScope> {
        if self.state_for(model, now_ms) != QuotaState::Exhausted { return Vec::new(); }
        if self.ordinary_usage_allowed == Some(false) { return vec![QuotaBlockScope::Account]; }
        let mut scopes: Vec<_> = self.applicable_to(model).into_iter()
            .filter(|window| window.used_percent >= 100.0
                && window.reset_ms.is_none_or(|reset| reset > now_ms))
            .map(|window| match (&window.model, &window.model_family) {
                (Some(model), _) => QuotaBlockScope::Model(model.clone()),
                (None, Some(family)) => QuotaBlockScope::ModelFamily(family.clone()),
                (None, None) => QuotaBlockScope::Account,
            }).collect();
        scopes.sort();
        scopes.dedup();
        scopes
    }

    pub fn state_for(&self, model: &str, now_ms: i64) -> QuotaState {
        if self.ordinary_usage_allowed == Some(false) {
            return QuotaState::Exhausted;
        }
        let applicable = self.applicable_to(model);
        if applicable
            .iter()
            .any(|w| w.used_percent >= 100.0 && w.reset_ms.is_none_or(|reset| reset > now_ms))
        {
            return QuotaState::Exhausted;
        }
        if self.native_uncertain_until_ms.is_some_and(|until| now_ms < until) {
            return QuotaState::Unknown;
        }
        if now_ms < self.observed_ms {
            return QuotaState::Unknown;
        }
        if now_ms >= self.expires_ms {
            return QuotaState::Unknown;
        }
        if applicable.is_empty()
            || applicable
                .iter()
                .any(|w| now_ms < w.observed_ms || now_ms >= w.expires_ms
                    || w.reset_ms.is_some_and(|reset| reset <= now_ms))
        {
            QuotaState::Unknown
        } else {
            QuotaState::ObservedNonExhausted
        }
    }
}

/// Bound and digest provider identity before it reaches durable state or the protocol.
/// Set by the run supervisor in place of the raw Codex account id before a
/// reply is recorded (`shim::redact_account_reply`).
pub const ACCOUNT_FINGERPRINT_FIELD: &str = "overseerAccountFingerprint";

pub fn account_fingerprint(value: &Value) -> Result<String> {
    if value.get("accountId").is_none() {
        if let Some(fingerprint) = value.get(ACCOUNT_FINGERPRINT_FIELD).and_then(Value::as_str)
            .filter(|f| f.len() == 64 && f.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())) {
            return Ok(fingerprint.to_string());
        }
    }
    let account = value
        .get("accountId")
        .and_then(Value::as_str)
        .filter(|id| {
            !id.is_empty()
                && id.len() <= 256
                && id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
        })
        .ok_or_else(|| anyhow!("Codex account identity unavailable"))?;
    let mut digest = Sha256::new();
    digest.update(b"overseer:auto:codex-account:v1\0");
    digest.update(account.as_bytes());
    Ok(format!("{:x}", digest.finalize()))
}

fn identifier(value: &str) -> Result<String> {
    if value.is_empty()
        || value.len() > 120
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'/' | b':'))
    {
        return Err(anyhow!("unsupported quota identifier"));
    }
    Ok(value.to_string())
}

fn windows(
    snapshot: &Value,
    fallback: &str,
    pool: &str,
    observed_ms: i64,
    out: &mut Vec<QuotaWindow>,
) -> Result<()> {
    let object = snapshot
        .as_object()
        .ok_or_else(|| anyhow!("quota bucket must be an object"))?;
    let bucket_id = identifier(
        object
            .get("limitId")
            .and_then(Value::as_str)
            .unwrap_or(fallback),
    )?;
    // Plan changes invalidate learned draw rates. Retain only a bounded
    // provider identifier; malformed/free-text values become unknown.
    let plan_type = object.get("planType").and_then(Value::as_str)
        .filter(|value| !value.is_empty() && value.len() <= 40
            && value.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')))
        .map(str::to_string);
    let model = match object.get("normalModelSlug") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => Some(identifier(s)?),
        _ => return Err(anyhow!("unsupported quota model scope")),
    };
    for window in ["primary", "secondary"] {
        let Some(data) = object.get(window).filter(|v| !v.is_null()) else {
            continue;
        };
        let data = data
            .as_object()
            .ok_or_else(|| anyhow!("quota window must be an object"))?;
        let used_percent = data
            .get("usedPercent")
            .and_then(Value::as_f64)
            .filter(|p| p.is_finite() && (0.0..=100.0).contains(p))
            .ok_or_else(|| anyhow!("invalid quota percentage"))?;
        let reset_ms = match data.get("resetsAt") {
            None | Some(Value::Null) => None,
            Some(v) => Some(
                v.as_i64()
                    .and_then(|n| n.checked_mul(1000))
                    .filter(|n| *n > 0)
                    .ok_or_else(|| anyhow!("invalid quota reset"))?,
            ),
        };
        if reset_ms.is_some_and(|n| n <= observed_ms) {
            continue;
        }
        let duration_mins = match data.get("windowDurationMins") {
            None | Some(Value::Null) => None,
            Some(v) => Some(
                v.as_i64()
                    .filter(|n| *n > 0 && *n <= 525_600)
                    .ok_or_else(|| anyhow!("invalid quota duration"))?,
            ),
        };
        out.push(QuotaWindow {
            pool_id: pool.to_string(),
            bucket_id: bucket_id.clone(),
            window: window.into(),
            model: model.clone(),
            model_family: None,
            plan_type: plan_type.clone(),
            used_percent,
            reset_ms,
            duration_mins,
            observed_ms,
            expires_ms: observed_ms.saturating_add(FRESH_MS),
        });
    }
    Ok(())
}

fn claude_window(name: &str) -> Option<Option<&'static str>> {
    match name {
        "five_hour" | "seven_day" => Some(None),
        "seven_day_opus" => Some(Some("opus")),
        "seven_day_sonnet" => Some(Some("sonnet")),
        // Paid overage meters are not subscription capacity.
        _ => None,
    }
}

fn claude_reset(value: Option<&Value>, observed_ms: i64) -> Result<Option<i64>> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(value) => {
            let reset = value.as_i64().and_then(|seconds| seconds.checked_mul(1000))
                .filter(|reset| *reset > observed_ms && *reset <= observed_ms.saturating_add(370 * 86_400_000))
                .ok_or_else(|| anyhow!("invalid Claude rate-limit reset"))?;
            Ok(Some(reset))
        }
    }
}

fn claude_meter(name: &str, value: &Value, pool: &str, observed_ms: i64) -> Result<Option<QuotaWindow>> {
    let Some(model_family) = claude_window(name) else { return Ok(None) };
    let object = value.as_object().ok_or_else(|| anyhow!("Claude rate-limit meter must be an object"))?;
    let Some(utilization) = object.get("utilization") else { return Ok(None) };
    let used_percent = utilization.as_f64()
        .filter(|number| number.is_finite() && (0.0..=1.0).contains(number))
        .ok_or_else(|| anyhow!("invalid Claude rate-limit utilization"))? * 100.0;
    Ok(Some(QuotaWindow { pool_id:pool.into(), bucket_id:name.into(), window:name.into(),
        model:None, model_family:model_family.map(str::to_string), plan_type:None, used_percent,
        reset_ms:claude_reset(object.get("resetsAt"), observed_ms)?, duration_mins:None,
        observed_ms, expires_ms:observed_ms.saturating_add(FRESH_MS) }))
}

/// Native Claude Code `rate_limit_event` only. An allowed event is an explicit
/// allowance for the reading's term; a rejected, unrecognized scope
/// conservatively blocks this account; absent or malformed meters never become
/// an invented balance. The caller must retain only this normalized snapshot.
pub fn parse_claude_rate_limit_event(value: &Value, pool_id: &str, observed_ms: i64) -> Result<QuotaSnapshot> {
    if value.get("type").and_then(Value::as_str) != Some("rate_limit_event") {
        return Err(anyhow!("not a Claude rate-limit event"));
    }
    let pool = identifier(pool_id)?;
    let info = value.get("rate_limit_info").and_then(Value::as_object)
        .ok_or_else(|| anyhow!("Claude rate-limit info is unavailable"))?;
    let rejected = match info.get("status").and_then(Value::as_str) {
        Some("rejected") => true,
        Some("allowed" | "allowed_warning") => false,
        _ => return Err(anyhow!("unsupported Claude rate-limit status")),
    };
    let current = info.get("rateLimitType").and_then(Value::as_str);
    let mut out = Vec::new();
    if let Some(unified) = info.get("unifiedWindows") {
        let windows = unified.as_object().filter(|windows| windows.len() <= 16)
            .ok_or_else(|| anyhow!("unsupported Claude unified window map"))?;
        for (name, meter) in windows {
            if let Some(window) = claude_meter(name, meter, &pool, observed_ms)? { out.push(window); }
        }
    }
    if let Some(name) = current.filter(|name| claude_window(name).is_some()) {
        let top = serde_json::json!({"utilization":info.get("utilization"), "resetsAt":info.get("resetsAt")});
        let top_has_meter = info.get("utilization").is_some();
        if let Some(existing) = out.iter().find(|window| window.bucket_id == name) {
            if top_has_meter {
                let parsed = claude_meter(name, &top, &pool, observed_ms)?
                    .ok_or_else(|| anyhow!("Claude top-level meter disagrees with unified window"))?;
                if parsed.used_percent != existing.used_percent
                    || parsed.reset_ms.is_some_and(|reset| existing.reset_ms != Some(reset)) {
                    return Err(anyhow!("conflicting Claude rate-limit meters"));
                }
            }
        } else if top_has_meter {
            if let Some(window) = claude_meter(name, &top, &pool, observed_ms)? { out.push(window); }
        }
        if rejected {
            if let Some(window) = out.iter_mut().find(|window| window.bucket_id == name) {
                window.used_percent = 100.0;
            } else {
                out.push(QuotaWindow { pool_id:pool.clone(), bucket_id:name.into(), window:name.into(),
                    model:None, model_family:claude_window(name).flatten().map(str::to_string),
                    plan_type:None, used_percent:100.0,
                    reset_ms:claude_reset(info.get("resetsAt"), observed_ms)?,
                    duration_mins:None, observed_ms, expires_ms:observed_ms.saturating_add(FRESH_MS) });
            }
        }
    }
    // An `allowed`/`allowed_warning` event is an explicit allowance for the
    // reading's own term (the owner's decision of 2026-09-28); a rejection of
    // an unrecognized scope denies the account; a scoped rejection blocks its
    // window and leaves the account-wide allowance unknown.
    let ordinary_usage_allowed = if !rejected { Some(true) }
        else if current.and_then(claude_window).is_none() { Some(false) }
        else { None };
    Ok(QuotaSnapshot { ordinary_usage_allowed,
        observed_ms, expires_ms:observed_ms.saturating_add(FRESH_MS), windows:out,
        native_uncertain_until_ms:None })
}

/// Parse an account-scoped Codex app-server response. Token totals and credits
/// never become an invented subscription-token budget.
pub fn parse_codex_rate_limits(
    value: &Value,
    pool_id: &str,
    observed_ms: i64,
) -> Result<QuotaSnapshot> {
    let object = value
        .as_object()
        .ok_or_else(|| anyhow!("Codex rate limits must be an object"))?;
    let pool = identifier(pool_id)?;
    let ordinary_usage_allowed = match object.get("ordinaryUsageAllowed") {
        None | Some(Value::Null) => None,
        Some(Value::Bool(value)) => Some(*value),
        _ => return Err(anyhow!("invalid ordinary usage flag")),
    };
    let mut out = Vec::new();
    match object.get("rateLimitsByLimitId") {
        Some(Value::Object(buckets)) => {
            if buckets.len() > 64 {
                return Err(anyhow!("too many quota buckets"));
            }
            for (name, bucket) in buckets {
                windows(bucket, name, &pool, observed_ms, &mut out)?;
            }
        }
        Some(Value::Null) | None => {
            if let Some(bucket) = object.get("rateLimits").filter(|v| !v.is_null()) {
                windows(bucket, "codex", &pool, observed_ms, &mut out)?;
            }
        }
        _ => return Err(anyhow!("unsupported quota bucket schema")),
    }
    Ok(QuotaSnapshot {
        ordinary_usage_allowed,
        observed_ms,
        expires_ms: observed_ms.saturating_add(FRESH_MS),
        windows: out,
        native_uncertain_until_ms: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn claude_native_windows_keep_account_and_model_family_scope() {
        let now = 1_800_000_000_000_i64;
        let allowed = parse_claude_rate_limit_event(&json!({"type":"rate_limit_event",
            "rate_limit_info":{"status":"allowed","rateLimitType":"five_hour",
                "utilization":0.2,"resetsAt":1800003600,
                "unifiedWindows":{"seven_day":{"utilization":0.3,"resetsAt":1800500000},
                    "seven_day_opus":{"utilization":0.9,"resetsAt":1800500000}}}}),
            "system-claude", now).unwrap();
        assert_eq!(allowed.windows.len(), 3);
        assert_eq!(allowed.applicable_to("claude-sonnet-4-5").len(), 2);
        assert_eq!(allowed.applicable_to("claude-opus-4-5").len(), 3);
        assert_eq!(allowed.state_for("claude-sonnet-4-5", now), QuotaState::ObservedNonExhausted);
        let rejected = parse_claude_rate_limit_event(&json!({"type":"rate_limit_event",
            "rate_limit_info":{"status":"rejected","rateLimitType":"seven_day_opus",
                "resetsAt":1800500000,"unifiedWindows":{"seven_day":{"utilization":0.3,"resetsAt":1800500000}}}}),
            "system-claude", now).unwrap();
        assert_eq!(rejected.state_for("claude-opus-4-5", now), QuotaState::Exhausted);
        assert_eq!(rejected.state_for("claude-sonnet-4-5", now), QuotaState::ObservedNonExhausted);
    }

    /// The owner's decision of 2026-09-28: an `allowed` or `allowed_warning`
    /// event is an explicit allowance for its freshness period, as Codex's
    /// `ordinaryUsageAllowed` is. A scoped rejection is not an account-wide
    /// allowance, and an unscoped one is an account-wide denial.
    #[test]
    fn a_claude_allowed_event_is_an_explicit_allowance() {
        let now = 1_800_000_000_000_i64;
        let event = |status: &str, scope: &str| parse_claude_rate_limit_event(&json!({"type":"rate_limit_event",
            "rate_limit_info":{"status":status,"rateLimitType":scope,"resetsAt":1800003600,
                "unifiedWindows":{"five_hour":{"utilization":0.2,"resetsAt":1800003600}}}}), "pool", now).unwrap();
        assert_eq!(event("allowed", "five_hour").ordinary_usage_allowed, Some(true));
        assert_eq!(event("allowed_warning", "five_hour").ordinary_usage_allowed, Some(true));
        assert_eq!(event("rejected", "seven_day_opus").ordinary_usage_allowed, None, "a scoped block");
        assert_eq!(event("rejected", "new_window").ordinary_usage_allowed, Some(false));
        // The allowance lasts only as long as the reading: after it, unknown.
        let allowed = event("allowed", "five_hour");
        assert_eq!(allowed.state_for("claude-sonnet-4-5", now + 59_999), QuotaState::ObservedNonExhausted);
        assert_eq!(allowed.state_for("claude-sonnet-4-5", now + 60_000), QuotaState::Unknown);
    }

    #[test]
    fn claude_missing_or_invalid_meter_never_invents_remaining_allowance() {
        let now = 1_800_000_000_000_i64;
        let info = |details: Value| json!({"type":"rate_limit_event","rate_limit_info":details});
        let empty = parse_claude_rate_limit_event(&info(json!({"status":"allowed"})), "pool", now).unwrap();
        assert_eq!(empty.state_for("claude-sonnet", now), QuotaState::Unknown);
        for utilization in [json!(-0.1),json!(1.1),json!("0.5"),json!(null)] {
            assert!(parse_claude_rate_limit_event(&info(json!({"status":"allowed",
                "rateLimitType":"five_hour","utilization":utilization})), "pool", now).is_err());
        }
        let unknown_rejection = parse_claude_rate_limit_event(&info(json!({"status":"rejected",
            "rateLimitType":"new_window"})), "pool", now).unwrap();
        assert_eq!(unknown_rejection.state_for("claude-sonnet", now), QuotaState::Exhausted);
        assert_eq!(unknown_rejection.state_for("claude-opus", now), QuotaState::Exhausted);
    }

    #[test]
    fn known_claude_rejection_survives_malformed_followup_until_its_reset() {
        let now = 1_800_000_000_000_i64;
        let blocked = parse_claude_rate_limit_event(&json!({"type":"rate_limit_event",
            "rate_limit_info":{"status":"rejected","rateLimitType":"seven_day_opus",
                "resetsAt":1800003600}}), "pool", now).unwrap();
        assert_eq!(blocked.state_for("claude-opus-4-5", now + 120_000), QuotaState::Exhausted,
            "a known rejection persists beyond the 60-second freshness of nonblocking meters");
        let mut unknown = parse_claude_rate_limit_event(&json!({"type":"rate_limit_event",
            "rate_limit_info":{"status":"allowed"}}), "pool", now + 120_000).unwrap();
        unknown.reconcile_unordered_native_evidence(&blocked, now + 120_000);
        assert_eq!(unknown.state_for("claude-opus-4-5", now + 120_000), QuotaState::Exhausted);
        assert_eq!(unknown.state_for("claude-sonnet-4-5", now + 120_000), QuotaState::Unknown);
        assert_eq!(unknown.state_for("claude-opus-4-5", now + 3_600_000), QuotaState::Unknown);
    }

    #[test]
    fn unordered_native_meter_decrease_is_unknown_until_a_new_window() {
        let at = 1_800_000_000_000_i64;
        let frame = |used: f64, reset: i64, observed: i64| {
            parse_claude_rate_limit_event(&json!({"type":"rate_limit_event",
                "rate_limit_info":{"status":"allowed","rateLimitType":"five_hour",
                    "utilization":used,"resetsAt":reset}}), "pool", observed).unwrap()
        };
        let prior = frame(0.8, 1_800_003_600, at);
        let mut late = frame(0.2, 1_800_003_600, at + 10);
        late.reconcile_unordered_native_evidence(&prior, at + 10);
        assert_eq!(late.state_for("claude-sonnet", at + 10), QuotaState::Unknown);

        let mut following = frame(0.25, 1_800_003_600, at + 20);
        following.reconcile_unordered_native_evidence(&late, at + 20);
        assert_eq!(following.state_for("claude-sonnet", at + 20), QuotaState::Unknown,
            "another same-reset notification cannot erase the uncertain ordering");

        let mut reset = frame(0.2, 1_800_007_200, at + 3_600_010);
        reset.reconcile_unordered_native_evidence(&following, at + 3_600_010);
        assert_eq!(reset.state_for("claude-sonnet", at + 3_600_010),
            QuotaState::ObservedNonExhausted);

        let mut clock_back = frame(0.9, 1_800_003_600, at - 10);
        clock_back.reconcile_unordered_native_evidence(&prior, at - 10);
        assert_eq!(clock_back.state_for("claude-sonnet", at - 10), QuotaState::Unknown);
    }

    #[test]
    fn partial_native_update_does_not_refresh_an_omitted_window() {
        let at = 1_800_000_000_000_i64;
        let frame = |used: f64, observed: i64, weekly: Option<f64>| {
            let mut info = json!({"status":"allowed","rateLimitType":"five_hour",
                "utilization":used,"resetsAt":1_800_003_600});
            if let Some(weekly) = weekly {
                info["unifiedWindows"] = json!({"seven_day":{
                    "utilization":weekly,"resetsAt":1_800_500_000}});
            }
            parse_claude_rate_limit_event(&json!({"type":"rate_limit_event",
                "rate_limit_info":info}), "pool", observed).unwrap()
        };
        let full = frame(0.2, at, Some(0.4));
        let mut partial = frame(0.3, at + 20, None);
        partial.reconcile_unordered_native_evidence(&full, at + 20);
        assert_eq!(partial.state_for("claude-sonnet", at + 20),
            QuotaState::ObservedNonExhausted);
        assert_eq!(partial.state_for("claude-sonnet", at + 60_005), QuotaState::Unknown,
            "a fresh five-hour update cannot refresh an omitted seven-day meter");

        let mut later_partial = frame(0.4, at + 60_010, None);
        later_partial.reconcile_unordered_native_evidence(&partial, at + 60_010);
        assert_eq!(later_partial.state_for("claude-sonnet", at + 60_010), QuotaState::Unknown);

        let mut later_full = frame(0.45, at + 60_015, Some(0.5));
        later_full.reconcile_unordered_native_evidence(&later_partial, at + 60_015);
        assert_eq!(later_full.state_for("claude-sonnet", at + 60_015),
            QuotaState::ObservedNonExhausted);
    }

    #[test]
    fn reordered_native_meter_keeps_other_windows_unknown_after_its_reset() {
        let at = 1_800_000_000_000_i64;
        let mut first = parse_claude_rate_limit_event(&json!({"type":"rate_limit_event",
            "rate_limit_info":{"status":"allowed","rateLimitType":"five_hour",
                "utilization":0.8,"resetsAt":1_800_003_600,
                "unifiedWindows":{"seven_day":{"utilization":0.4,
                    "resetsAt":1_800_500_000}}}}), "pool", at).unwrap();
        let mut regressed = parse_claude_rate_limit_event(&json!({"type":"rate_limit_event",
            "rate_limit_info":{"status":"allowed","rateLimitType":"five_hour",
                "utilization":0.2,"resetsAt":1_800_003_600}}), "pool", at + 10).unwrap();
        regressed.reconcile_unordered_native_evidence(&first, at + 10);
        assert_eq!(regressed.state_for("claude-sonnet", at + 10), QuotaState::Unknown);
        first = parse_claude_rate_limit_event(&json!({"type":"rate_limit_event",
            "rate_limit_info":{"status":"allowed","rateLimitType":"five_hour",
                "utilization":0.1,"resetsAt":1_800_007_200}}),
            "pool", at + 3_600_010).unwrap();
        first.reconcile_unordered_native_evidence(&regressed, at + 3_600_010);
        assert_eq!(first.state_for("claude-sonnet", at + 3_600_010), QuotaState::Unknown,
            "a new five-hour window does not establish the omitted weekly window");
    }

    #[test]
    fn codex_preserves_each_bucket_and_blocks_an_exhausted_secondary_window() {
        let now = 1_800_000_000_000_i64;
        let payload = json!({
            "ordinaryUsageAllowed": true,
            "rateLimitsByLimitId": {
                "codex": {"limitId":"codex", "normalModelSlug":null,
                    "primary":{"usedPercent":40,"windowDurationMins":300,"resetsAt":1800003600},
                    "secondary":{"usedPercent":100,"windowDurationMins":10080,"resetsAt":1800500000}},
                "base_model_inference":{"limitId":"base_model_inference","normalModelSlug":"gpt-5.6-luna",
                    "primary":{"usedPercent":25,"windowDurationMins":10080,"resetsAt":1800500000},"secondary":null}
            }
        });
        let snapshot = parse_codex_rate_limits(&payload, "pool-1", now).unwrap();
        assert_eq!(snapshot.windows.len(), 3);
        assert_eq!(snapshot.applicable_to("gpt-6-sol").len(), 2);
        assert_eq!(snapshot.applicable_to("gpt-5.6-luna").len(), 3);
        assert_eq!(
            snapshot
                .blocking_window("gpt-6-sol")
                .map(|w| w.window.as_str()),
            Some("secondary")
        );
        assert_eq!(snapshot.windows[0].observed_ms, now);
        assert_eq!(snapshot.windows[0].expires_ms, now + 60_000);
    }

    #[test]
    fn codex_plan_scope_is_retained_without_echoing_invalid_free_text() {
        let now = 1_800_000_000_000_i64;
        let reported = parse_codex_rate_limits(&json!({"rateLimits":{
            "limitId":"codex","planType":"pro",
            "primary":{"usedPercent":35,"resetsAt":1800003600}
        }}), "pool-1", now).unwrap();
        assert_eq!(serde_json::to_value(&reported.windows[0]).unwrap()["plan_type"], "pro");
        let malformed = parse_codex_rate_limits(&json!({"rateLimits":{
            "limitId":"codex","planType":"private-user@example.com",
            "primary":{"usedPercent":35,"resetsAt":1800003600}
        }}), "pool-1", now).unwrap();
        assert!(serde_json::to_value(&malformed.windows[0]).unwrap()["plan_type"].is_null());
        assert_eq!(malformed.state_for("gpt-6-sol", now), QuotaState::ObservedNonExhausted);
    }

    #[test]
    fn quota_expiry_and_reset_require_refresh_before_recovery() {
        let now = 1_800_000_000_000_i64;
        let payload = json!({"ordinaryUsageAllowed":true,"rateLimits":{
            "primary":{"usedPercent":100,"resetsAt":1800000060,"windowDurationMins":300}}});
        let snapshot = parse_codex_rate_limits(&payload, "pool-1", now).unwrap();
        assert_eq!(snapshot.state_for("gpt-6-sol", now), QuotaState::Exhausted);
        assert!(!snapshot.needs_refresh(now + 59_999));
        assert_eq!(
            snapshot.state_for("gpt-6-sol", now + 59_999),
            QuotaState::Exhausted
        );
        assert_eq!(
            snapshot.state_for("gpt-6-sol", now + 60_000),
            QuotaState::Unknown
        );
        assert!(snapshot.needs_refresh(now + 60_000));
        let early_reset = parse_codex_rate_limits(
            &json!({"ordinaryUsageAllowed":true,"rateLimits":{
            "primary":{"usedPercent":100,"resetsAt":1800000030,"windowDurationMins":300}}}),
            "pool-1",
            now,
        )
        .unwrap();
        assert_eq!(
            early_reset.state_for("gpt-6-sol", now + 30_000),
            QuotaState::Unknown
        );
        assert!(early_reset.needs_refresh(now + 30_000));
        let denied = parse_codex_rate_limits(
            &json!({"ordinaryUsageAllowed":false,"rateLimits":null}),
            "pool-1",
            now,
        )
        .unwrap();
        assert_eq!(denied.state_for("gpt-6-sol", now), QuotaState::Exhausted);
        assert_eq!(
            denied.state_for("gpt-6-sol", now + 60_000),
            QuotaState::Exhausted,
            "an explicit account denial without a reset cannot expire into new capacity"
        );
    }

    #[test]
    fn backward_clock_skew_does_not_clear_a_known_quota_block() {
        let observed = 1_800_000_000_000_i64;
        let exhausted = parse_codex_rate_limits(&json!({"rateLimits":{
            "primary":{"usedPercent":100,"resetsAt":1800003600}}}), "pool-1", observed).unwrap();
        assert_eq!(exhausted.state_for("gpt-6-sol", observed - 1), QuotaState::Exhausted,
            "clock rollback must not turn a known block into unknown eligibility");
        let available = parse_codex_rate_limits(&json!({"rateLimits":{
            "primary":{"usedPercent":30,"resetsAt":1800003600}}}), "pool-1", observed).unwrap();
        assert_eq!(available.state_for("gpt-6-sol", observed - 1), QuotaState::Unknown,
            "clock rollback must not carry a capacity claim into the past");
        assert!(available.needs_refresh(observed - 1));
    }

    #[test]
    fn codex_missing_or_malformed_windows_never_fabricate_capacity() {
        let now = 1_800_000_000_000_i64;
        let missing = parse_codex_rate_limits(
            &json!({"ordinaryUsageAllowed":true,"rateLimits":null}),
            "pool-1",
            now,
        )
        .unwrap();
        assert!(missing.windows.is_empty());
        assert!(missing.allowance_unknown("gpt-6-sol"));
        for bad in [json!(-1), json!(101), json!("20")].iter() {
            let value = json!({"rateLimits":{"primary":{"usedPercent":bad,"resetsAt":1800003600}}});
            assert!(parse_codex_rate_limits(&value, "pool-1", now).is_err());
        }
        assert!(
            parse_codex_rate_limits(&json!({"rateLimitsByLimitId":[]}), "pool-1", now).is_err()
        );
    }

    #[test]
    fn ambiguous_cli_text_and_percentage_labels_never_become_allowance() {
        let now = 1_800_000_000_000_i64;
        for text in [
            "\u{1b}[32m5-hour remaining: 80%\u{1b}[0m",
            "5 heures restantes : 80 %",
            "used: 20; remaining: 80",
        ] {
            assert!(parse_codex_rate_limits(&json!(text), "pool", now).is_err());
            assert!(parse_claude_rate_limit_event(&json!(text), "pool", now).is_err());
        }
        for ambiguous in [
            json!({"remainingPercent":80}),
            json!({"used":20}),
            json!({"usedPercent":"20%"}),
            json!({"usedPercent":-1}),
            json!({"usedPercent":101}),
        ] {
            let result = parse_codex_rate_limits(
                &json!({"rateLimits":{"primary":ambiguous}}), "pool", now,
            );
            assert!(result.is_err(), "ambiguous or invalid meter must not become capacity");
        }
        assert!(parse_claude_rate_limit_event(&json!({"type":"rate_limit_event",
            "rate_limit_info":{"status":"autorisé","rateLimitType":"five_hour",
                "utilization":0.2}}), "pool", now).is_err());
    }

    #[test]
    fn reset_time_zone_and_missing_units_need_structured_source_contract() {
        let now = 1_800_000_000_000_i64;
        for reset in [json!("2027-01-15T12:00:00Z"), json!("2027-01-15T04:00:00-08:00"),
            json!(-1), json!(null)] {
            let codex = json!({"rateLimits":{"primary":{"usedPercent":40,
                "resetsAt":reset,"windowDurationMins":300}}});
            if reset.is_null() {
                let snapshot = parse_codex_rate_limits(&codex, "pool", now).unwrap();
                assert_eq!(snapshot.windows[0].reset_ms, None);
            } else {
                assert!(parse_codex_rate_limits(&codex, "pool", now).is_err());
            }
        }
        for reset in ["2027-01-15T12:00:00Z", "2027-01-15T04:00:00-08:00"] {
            assert!(parse_claude_rate_limit_event(&json!({"type":"rate_limit_event",
                "rate_limit_info":{"status":"allowed","rateLimitType":"five_hour",
                    "utilization":0.2,"resetsAt":reset}}), "pool", now).is_err());
        }
        let missing_units = parse_codex_rate_limits(
            &json!({"rateLimits":{"primary":{"remaining":80}}}), "pool", now,
        );
        assert!(missing_units.is_err(), "a bare number has no known percentage semantics");
    }
}
