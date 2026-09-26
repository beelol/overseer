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

impl QuotaSnapshot {
    pub fn applicable_to(&self, model: &str) -> Vec<&QuotaWindow> {
        self.windows
            .iter()
            .filter(|w| w.model.as_deref().is_none_or(|m| m == model))
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

    pub fn state_for(&self, model: &str, now_ms: i64) -> QuotaState {
        if now_ms < self.observed_ms || now_ms >= self.expires_ms {
            return QuotaState::Unknown;
        }
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
        if applicable.is_empty()
            || applicable
                .iter()
                .any(|w| w.reset_ms.is_some_and(|reset| reset <= now_ms))
        {
            QuotaState::Unknown
        } else {
            QuotaState::ObservedNonExhausted
        }
    }
}

/// Bound and digest provider identity before it reaches durable state or the protocol.
pub fn account_fingerprint(value: &Value) -> Result<String> {
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
            used_percent,
            reset_ms,
            duration_mins,
            observed_ms,
            expires_ms: observed_ms.saturating_add(FRESH_MS),
        });
    }
    Ok(())
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
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
    fn quota_expiry_and_reset_require_refresh_before_recovery() {
        let now = 1_800_000_000_000_i64;
        let payload = json!({"ordinaryUsageAllowed":true,"rateLimits":{
            "primary":{"usedPercent":100,"resetsAt":1800000060,"windowDurationMins":300}}});
        let snapshot = parse_codex_rate_limits(&payload, "pool-1", now).unwrap();
        assert_eq!(snapshot.state_for("gpt-6-sol", now), QuotaState::Exhausted);
        assert_eq!(
            snapshot.state_for("gpt-6-sol", now + 59_999),
            QuotaState::Exhausted
        );
        assert_eq!(
            snapshot.state_for("gpt-6-sol", now + 60_000),
            QuotaState::Unknown
        );
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
        let denied = parse_codex_rate_limits(
            &json!({"ordinaryUsageAllowed":false,"rateLimits":null}),
            "pool-1",
            now,
        )
        .unwrap();
        assert_eq!(denied.state_for("gpt-6-sol", now), QuotaState::Exhausted);
        assert_eq!(
            denied.state_for("gpt-6-sol", now + 60_000),
            QuotaState::Unknown
        );
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
}
