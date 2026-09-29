//! Local, content-free usage measurements for Auto Mode.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Measurement {
    pub observed_ms: i64,
    pub task_id: String,
    pub run_id: String,
    pub harness: String,
    pub profile_id: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub cached_input_tokens: Option<i64>,
    pub reasoning_output_tokens: Option<i64>,
    pub cost_usd: Option<f64>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct StoredMeasurement {
    pub event_seq: i64,
    #[serde(flatten)]
    pub measurement: Measurement,
}

const MAX_TOKEN_COUNT: u64 = 1_000_000_000_000;

fn token(value: &Value, keys: &[&str]) -> Result<Option<i64>, ()> {
    for key in keys {
        if let Some(value) = value.get(key) {
            if value.is_null() {
                continue;
            }
            return value
                .as_u64()
                .filter(|n| *n <= MAX_TOKEN_COUNT)
                .map(|n| Some(n as i64))
                .ok_or(());
        }
    }
    Ok(None)
}

fn cost(value: &Value) -> Result<Option<f64>, ()> {
    for key in ["total_cost_usd", "cost"] {
        if let Some(value) = value.get(key) {
            if value.is_null() {
                continue;
            }
            return value
                .as_f64()
                .filter(|n| n.is_finite() && *n >= 0.0 && *n <= 1_000_000.0)
                .map(Some)
                .ok_or(());
        }
    }
    Ok(None)
}

fn stable_id(value: &str) -> Option<String> {
    (value.len() <= 120
        && !value.is_empty()
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'/')))
    .then(|| value.to_string())
}

/// The storage boundary validates the same allowlisted shape even when a
/// caller constructs a measurement without using `from_usage`.
pub fn valid_for_store(m: &Measurement) -> bool {
    [m.task_id.as_str(), m.run_id.as_str(), m.harness.as_str()]
        .iter()
        .all(|s| stable_id(s).is_some())
        && m.profile_id
            .as_deref()
            .is_none_or(|s| stable_id(s).is_some())
        && m.model.as_deref().is_none_or(|s| stable_id(s).is_some())
        && m.effort.as_deref().is_none_or(|s| stable_id(s).is_some())
        && [
            m.input_tokens,
            m.output_tokens,
            m.cached_input_tokens,
            m.reasoning_output_tokens,
        ]
        .iter()
        .all(|value| value.is_none_or(|n| (0..=MAX_TOKEN_COUNT as i64).contains(&n)))
        && m.cost_usd
            .is_none_or(|n| n.is_finite() && (0.0..=1_000_000.0).contains(&n))
}

/// Normalize only numeric fields from an existing harness usage event. Raw payloads
/// and arbitrary strings never cross into the usage-learning store.
#[cfg(test)]
pub fn from_usage(
    observed_ms: i64,
    task_id: &str,
    run_id: &str,
    harness: &str,
    profile_id: Option<&str>,
    model: Option<&str>,
    payload: &Value,
) -> Option<Measurement> {
    from_usage_with_effort(
        observed_ms,
        task_id,
        run_id,
        harness,
        profile_id,
        model,
        None,
        payload,
    )
}

pub fn from_usage_with_effort(
    observed_ms: i64,
    task_id: &str,
    run_id: &str,
    harness: &str,
    profile_id: Option<&str>,
    model: Option<&str>,
    effort: Option<&str>,
    payload: &Value,
) -> Option<Measurement> {
    let values = payload
        .get("usage")
        .filter(|v| v.is_object())
        .or_else(|| payload.get("last").filter(|v| v.is_object()))
        .or_else(|| payload.get("tokens").filter(|v| v.is_object()))
        .unwrap_or(payload);
    let input_tokens = token(values, &["input_tokens", "inputTokens", "input"]).ok()?;
    let output_tokens = token(values, &["output_tokens", "outputTokens", "output"]).ok()?;
    let cached_input_tokens = token(
        values,
        &[
            "cached_input_tokens",
            "cachedInputTokens",
            "cache_read_input_tokens",
        ],
    )
    .ok()?;
    let reasoning_output_tokens = token(
        values,
        &[
            "reasoning_output_tokens",
            "reasoningOutputTokens",
            "reasoning",
        ],
    )
    .ok()?;
    let cost_usd = cost(payload).ok()?;
    if input_tokens.is_none()
        && output_tokens.is_none()
        && cached_input_tokens.is_none()
        && reasoning_output_tokens.is_none()
        && cost_usd.is_none()
    {
        return None;
    }
    Some(Measurement {
        observed_ms,
        task_id: stable_id(task_id)?,
        run_id: stable_id(run_id)?,
        harness: stable_id(harness)?,
        profile_id: profile_id.and_then(stable_id),
        model: model.and_then(stable_id),
        effort: effort.and_then(stable_id),
        input_tokens,
        output_tokens,
        cached_input_tokens,
        reasoning_output_tokens,
        cost_usd,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn usage_extraction_keeps_numbers_and_excludes_content() {
        let payload = json!({
            "input_tokens": 120,
            "output_tokens": 20,
            "cached_input_tokens": 15,
            "reasoning_output_tokens": 4,
            "prompt": "secret-prompt-sentinel",
            "token": "secret-token-sentinel"
        });
        let measurement = from_usage(
            1000,
            "t-1",
            "r-1",
            "codex",
            Some("p-1"),
            Some("model-1"),
            &payload,
        )
        .unwrap();
        assert_eq!(measurement.input_tokens, Some(120));
        assert_eq!(measurement.output_tokens, Some(20));
        assert_eq!(
            from_usage(
                1000,
                "t-1",
                "r-1",
                "codex",
                None,
                Some("gpt-5.6-sol"),
                &payload
            )
            .unwrap()
            .model
            .as_deref(),
            Some("gpt-5.6-sol")
        );
        let serialized = serde_json::to_string(&measurement).unwrap();
        assert!(!serialized.contains("secret-prompt-sentinel"));
        assert!(!serialized.contains("secret-token-sentinel"));
    }

    #[test]
    fn quota_only_and_invalid_usage_do_not_become_consumption() {
        assert!(from_usage(
            1000,
            "t",
            "r",
            "codex-app",
            None,
            None,
            &json!({"rate_limits":{"primary":{"usedPercent":80}}})
        )
        .is_none());
        assert!(from_usage(
            1000,
            "t",
            "r",
            "codex",
            None,
            None,
            &json!({"input_tokens":-1})
        )
        .is_none());
        assert!(from_usage(
            1000,
            "t",
            "r",
            "codex",
            None,
            None,
            &json!({"input_tokens":"100"})
        )
        .is_none());
    }
}
