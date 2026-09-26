//! Allowlisted provider estimates from an already executed Codex thread.
//! Credits are provider-estimated thread activity, not subscription-window percent.

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

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
        estimated_credits_micros: total,
        groups,
    }))
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
