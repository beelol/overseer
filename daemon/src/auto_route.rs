//! Allowlisted harness model discovery. A catalog reports availability and
//! supported efforts; it does not establish capability tier or allowance cost.

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

use crate::auto_quota::QuotaSnapshot;
use crate::auto_select::{self, Allowance, CapabilityTier, Fit, Health, Route, Sandbox};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct DiscoveredModel {
    pub model: String,
    pub efforts: Vec<String>,
    pub default_effort: Option<String>,
    pub is_default: bool,
    pub modalities: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ModelCatalog {
    pub observed_ms: i64,
    pub expires_ms: i64,
    pub models: Vec<DiscoveredModel>,
}

/// Exact MCP tool names observed in a connected Codex runtime for one cwd.
/// This is discovery evidence, not proof that a future child has permission
/// to call a tool; dispatch must recheck effective policy at launch.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ToolCatalog {
    pub observed_ms: i64,
    pub expires_ms: i64,
    pub tools: BTreeSet<String>,
}

/// Versioned product capability priors, not subscription-price estimates or a
/// user routing file. Unknown or effort-incompatible models remain manual until
/// there is evidence for their task suitability. Selection itself is generic.
const CODEX_CAPABILITY_PRIORS_V1: &[(&str, CapabilityTier, &str, bool)] = &[
    ("gpt-6-sol", CapabilityTier::General, "medium", true),
    ("gpt-6-astra", CapabilityTier::Frontier, "high", false),
];

pub fn codex_auto_routes(
    catalog: &ModelCatalog,
    tools: &ToolCatalog,
    quota: Option<&QuotaSnapshot>,
    profile_id: &str,
    now_ms: i64,
) -> Vec<Route> {
    if now_ms < catalog.observed_ms || now_ms >= catalog.expires_ms
        || now_ms < tools.observed_ms || now_ms >= tools.expires_ms {
        return Vec::new();
    }
    let mut routes = Vec::new();
    for model in &catalog.models {
        let Some((_, tier, effort, recommended)) = CODEX_CAPABILITY_PRIORS_V1.iter()
            .find(|(name, _, _, _)| *name == model.model) else { continue };
        if !model.efforts.iter().any(|available| available == effort) { continue; }
        routes.push(Route {
            id: format!("{profile_id}/{}/{}", model.model, effort),
            harness: "codex-app".into(), provider: "openai".into(), endpoint: "codex".into(),
            profile_id: profile_id.into(), pool_id: profile_id.into(),
            model: model.model.clone(), resolved_model_version: None,
            effort: (*effort).into(), tier: *tier,
            tools: tools.tools.clone(), context_limit: None, supports_approvals: true,
            sandbox: Sandbox::WorkspaceWrite, recommended_default: *recommended,
            quota: auto_select::observed_allowance(quota, &model.model, now_ms),
            quota_blocks: quota.map(|value| value.blocking_scopes(&model.model, now_ms)).unwrap_or_default(),
            fit: Fit::Unknown, health: Health::Unknown,
            unresolved_quota_pool_identity:false, in_flight_pool_claim:false,
        });
    }
    routes
}

/// Claude's installed CLI documents the moving `sonnet` and `opus` aliases
/// and `medium`/`high` efforts. These versioned capability priors are not
/// model-version identities or consumption estimates. Alias changes require
/// later actual-model attribution before learning can pool observations.
pub fn claude_auto_routes(
    auth: &crate::auto_collect::ClaudeAuth,
    quota: Option<&QuotaSnapshot>,
    profile_id: &str,
    now_ms: i64,
) -> Vec<Route> {
    if now_ms < auth.observed_ms || now_ms >= auth.observed_ms.saturating_add(60_000) {
        return Vec::new();
    }
    [("sonnet", "medium", CapabilityTier::General, true),
     ("opus", "high", CapabilityTier::Frontier, false)].into_iter()
        .map(|(model, effort, tier, recommended_default)| Route {
            id:format!("{profile_id}/{model}/{effort}"), harness:"claude".into(),
            provider:"anthropic".into(), endpoint:"claude-code".into(),
            profile_id:profile_id.into(), pool_id:profile_id.into(),
            model:model.into(), resolved_model_version:None, effort:effort.into(), tier,
            tools:BTreeSet::new(), context_limit:None, supports_approvals:true,
            sandbox:Sandbox::WorkspaceWrite, recommended_default,
            quota:auto_select::observed_allowance(quota, model, now_ms),
            quota_blocks:quota.map(|value| value.blocking_scopes(model, now_ms)).unwrap_or_default(),
            fit:Fit::Unknown, health:Health::Unknown,
            unresolved_quota_pool_identity:false, in_flight_pool_claim:false,
        }).collect()
}

/// A versioned task-capability prior, not a claim that any configured proxy
/// actually serves these weights or has free subscription allowance. Unknown
/// local model identities remain manual until supported by stronger evidence.
/// OpenAI documents gpt-oss-120b as an open-weight reasoning model with tool
/// calling: https://developers.openai.com/api/docs/models/gpt-oss-120b
pub fn opencode_local_routes(catalog: &crate::auto_opencode::LocalCatalog,
    profile_id: &str, now_ms: i64) -> Vec<Route> {
    if now_ms < catalog.observed_ms || now_ms >= catalog.expires_ms { return Vec::new(); }
    catalog.models.iter().filter_map(|model| {
        let port = crate::auto_opencode::verified_loopback_port(&model.endpoint)?;
        let model_id = model.model.strip_prefix(&format!("{}/", model.provider_id))?;
        if model_id != "gpt-oss-120b" || !model.toolcall || !model.reasoning { return None; }
        let mut pool = Sha256::new();
        pool.update(b"overseer:auto:opencode-local-socket:v2\0");
        pool.update(port.to_be_bytes());
        Some(Route {
            id:format!("{profile_id}/{}/default", model.model), harness:"opencode".into(),
            provider:model.provider_id.clone(), endpoint:model.endpoint.clone(),
            profile_id:profile_id.into(), pool_id:format!("local-endpoint/{:x}", pool.finalize()),
            model:model.model.clone(), resolved_model_version:None,
            effort:"default".into(), tier:CapabilityTier::General,
            tools:BTreeSet::new(), context_limit:model.context_limit,
            // The guarded inline config exposes read-only agent tools. This
            // is not evidence of an OS-level filesystem sandbox.
            supports_approvals:false, sandbox:Sandbox::ReadOnly,
            recommended_default:model.is_default, quota:Allowance::Unknown,
            quota_blocks:Vec::new(),
            fit:Fit::Unknown, health:Health::Unknown,
            unresolved_quota_pool_identity:false, in_flight_pool_claim:false,
        })
    }).collect()
}

fn identifier(value: &Value) -> Result<String> {
    let s = value
        .as_str()
        .ok_or_else(|| anyhow!("model catalog identifier is missing"))?;
    if s.is_empty()
        || s.len() > 120
        || !s
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
    {
        return Err(anyhow!("unsupported model catalog identifier"));
    }
    Ok(s.to_string())
}

/// Parse a complete Codex `model/list` result. Only allowlisted, bounded
/// machine-readable fields survive; descriptions and price-like values do not.
/// Partial pagination and schema drift are unknown rather than partial routes.
pub fn parse_codex_catalog(value: &Value, observed_ms: i64) -> Result<ModelCatalog> {
    if !value.get("nextCursor").is_none_or(Value::is_null) {
        return Err(anyhow!("model catalog pagination is incomplete"));
    }
    let data = value
        .get("data")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("model catalog data is unavailable"))?;
    if data.len() > 128 {
        return Err(anyhow!("model catalog exceeded its bound"));
    }
    let mut seen = BTreeSet::new();
    let mut models = Vec::new();
    for item in data {
        let hidden = match item.get("hidden") {
            None => false,
            Some(Value::Bool(v)) => *v,
            _ => return Err(anyhow!("invalid hidden model flag")),
        };
        if hidden {
            continue;
        }
        let model = identifier(item.get("model").unwrap_or(&Value::Null))?;
        if !seen.insert(model.clone()) {
            return Err(anyhow!("duplicate model catalog entry"));
        }
        let values = item
            .get("supportedReasoningEfforts")
            .and_then(Value::as_array)
            .ok_or_else(|| anyhow!("supported efforts are unavailable"))?;
        if values.len() > 16 {
            return Err(anyhow!("too many model efforts"));
        }
        let mut efforts = Vec::new();
        let mut effort_set = BTreeSet::new();
        for effort in values {
            let name = identifier(effort.get("reasoningEffort").unwrap_or(&Value::Null))?;
            if !effort_set.insert(name.clone()) {
                return Err(anyhow!("duplicate model effort"));
            }
            efforts.push(name);
        }
        let default_effort = match item.get("defaultReasoningEffort") {
            None | Some(Value::Null) => None,
            Some(value) => Some(identifier(value)?),
        };
        if default_effort
            .as_ref()
            .is_some_and(|e| !effort_set.contains(e))
        {
            return Err(anyhow!("default effort is not supported"));
        }
        let is_default = match item.get("isDefault") {
            None => false,
            Some(Value::Bool(v)) => *v,
            _ => return Err(anyhow!("invalid default model flag")),
        };
        let mut modalities = Vec::new();
        if let Some(values) = item.get("inputModalities") {
            let values = values
                .as_array()
                .ok_or_else(|| anyhow!("invalid input modalities"))?;
            if values.len() > 16 {
                return Err(anyhow!("too many input modalities"));
            }
            for value in values {
                modalities.push(identifier(value)?);
            }
        }
        models.push(DiscoveredModel {
            model,
            efforts,
            default_effort,
            is_default,
            modalities,
        });
    }
    Ok(ModelCatalog {
        observed_ms,
        expires_ms: observed_ms.saturating_add(300_000),
        models,
    })
}

/// `mcpServerStatus/list` returns descriptions and arbitrary schemas. Retain
/// only bounded server/tool identities for connected, error-free servers.
pub fn parse_codex_tools(value: &Value, observed_ms: i64) -> Result<ToolCatalog> {
    if !value.get("nextCursor").is_none_or(Value::is_null) {
        return Err(anyhow!("tool inventory pagination is incomplete"));
    }
    let servers = value.get("data").and_then(Value::as_array)
        .ok_or_else(|| anyhow!("tool inventory data is unavailable"))?;
    if servers.len() > 64 {
        return Err(anyhow!("tool inventory exceeded its server bound"));
    }
    let mut names = BTreeSet::new();
    let mut tools = BTreeSet::new();
    for server in servers {
        let name = identifier(server.get("name").unwrap_or(&Value::Null))?;
        if !names.insert(name.clone()) {
            return Err(anyhow!("duplicate tool server"));
        }
        let status = server.get("runtimeStatus").and_then(Value::as_str);
        if !matches!(status, Some("connected" | "notStarted" | "starting" | "authenticationRequired" | "failed" | "cancelled" | "disabled") | None) {
            return Err(anyhow!("unsupported tool server status"));
        }
        let auth = server.get("authStatus").and_then(Value::as_str);
        if !matches!(auth, Some("unknown" | "unsupported" | "notLoggedIn" | "bearerToken" | "oAuth")) {
            return Err(anyhow!("unsupported tool server auth status"));
        }
        if status != Some("connected")
            || !matches!(auth, Some("unsupported" | "bearerToken" | "oAuth"))
            || !server.get("toolsError").is_none_or(Value::is_null) {
            continue;
        }
        let entries = server.get("tools").and_then(Value::as_object)
            .ok_or_else(|| anyhow!("connected tool server has no catalog"))?;
        if entries.len() > 128 || tools.len() + entries.len() > 256 {
            return Err(anyhow!("tool inventory exceeded its tool bound"));
        }
        for (key, tool) in entries {
            let canonical = identifier(&Value::String(key.clone()))?;
            if tool.get("name").and_then(Value::as_str) != Some(key.as_str()) {
                return Err(anyhow!("tool name disagrees with its catalog key"));
            }
            tools.insert(format!("{name}/{canonical}"));
        }
    }
    Ok(ToolCatalog { observed_ms, expires_ms: observed_ms.saturating_add(60_000), tools })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn claude_alias_routes_keep_family_quota_and_do_not_claim_tools_or_model_charges() {
        let now = 1_800_000_000_000_i64;
        let auth = crate::auto_collect::ClaudeAuth { fingerprint:"f".repeat(64), observed_ms:now };
        let quota = crate::auto_quota::parse_claude_rate_limit_event(&json!({"type":"rate_limit_event",
            "rate_limit_info":{"status":"rejected","rateLimitType":"seven_day_opus",
                "resetsAt":1800003600,"unifiedWindows":{"five_hour":{"utilization":0.3,"resetsAt":1800003600}}}}),
            "system-claude", now).unwrap();
        let routes = claude_auto_routes(&auth, Some(&quota), "system-claude", now);
        assert_eq!(routes.len(), 2);
        assert_eq!(routes[0].model, "sonnet");
        assert_eq!(routes[0].quota, crate::auto_select::Allowance::ObservedNonExhausted);
        assert!(routes[0].tools.is_empty());
        assert_eq!(routes[1].model, "opus");
        assert_eq!(routes[1].quota, crate::auto_select::Allowance::Exhausted);
        assert!(claude_auto_routes(&auth, Some(&quota), "system-claude", now + 60_000).is_empty());
    }

    #[test]
    fn local_opencode_prior_preserves_unknown_allowance_and_excludes_unknown_models() {
        let now = 1_800_000_000_000_i64;
        let local = |provider: &str, model: &str, toolcall, reasoning| crate::auto_opencode::LocalModel {
            provider_id:provider.into(), model:format!("{provider}/{model}"),
            endpoint:format!("http://127.0.0.1:{}/v1", if provider == "a" { 47811 } else { 47812 }),
            context_limit:Some(32_000), toolcall, reasoning,
            variants:vec![], is_default:provider == "a",
        };
        let catalog = crate::auto_opencode::LocalCatalog { observed_ms:now,
            expires_ms:now + 60_000, models:vec![
                local("a", "gpt-oss-120b", true, true),
                local("b", "gpt-oss-120b", true, true),
                local("b", "unknown-model", true, true),
                local("b", "gpt-oss-20b", true, true),
                local("b", "gpt-oss-120b", false, true),
            ] };
        let routes = opencode_local_routes(&catalog, "p-local", now);
        assert_eq!(routes.len(), 2);
        assert_ne!(routes[0].endpoint, routes[1].endpoint);
        assert_eq!(routes[0].quota, Allowance::Unknown);
        assert_eq!(routes[0].fit, Fit::Unknown);
        assert_eq!(routes[0].sandbox, Sandbox::ReadOnly);
        assert!(!routes[0].supports_approvals);
        assert_eq!(routes[0].tier, CapabilityTier::General);
        assert_eq!(routes[0].effort, "default");
        assert!(opencode_local_routes(&catalog, "p-local", now + 60_000).is_empty());
    }

    #[test]
    fn local_opencode_profiles_share_only_the_same_verified_endpoint_pool() {
        let now = 1_800_000_000_000_i64;
        let model = |endpoint: &str| crate::auto_opencode::LocalModel {
            provider_id:"local".into(), model:"local/gpt-oss-120b".into(),
            endpoint:endpoint.into(), context_limit:Some(32_000), toolcall:true,
            reasoning:true, variants:vec![], is_default:true,
        };
        let catalog = |endpoint: &str| crate::auto_opencode::LocalCatalog {
            observed_ms:now, expires_ms:now + 60_000, models:vec![model(endpoint)],
        };
        let a = opencode_local_routes(&catalog("http://127.0.0.1:47811/v1"), "profile-a", now);
        let b = opencode_local_routes(&catalog("http://127.0.0.1:47811/v1"), "profile-b", now);
        let alias = opencode_local_routes(&catalog("http://127.0.0.1:47811/other"), "profile-c", now);
        let numeric = opencode_local_routes(&catalog("http://127.0.0.1:8080/v1"), "profile-d", now);
        let padded = opencode_local_routes(&catalog("http://127.0.0.1:08080/v1"), "profile-e", now);
        let c = opencode_local_routes(&catalog("http://127.0.0.1:47812/v1"), "profile-b", now);
        assert_eq!(a[0].pool_id, b[0].pool_id);
        assert_eq!(a[0].pool_id, alias[0].pool_id);
        assert_eq!(numeric[0].pool_id, padded[0].pool_id,
            "equivalent numeric ports must share one unknown-draw pool");
        assert_ne!(a[0].pool_id, c[0].pool_id);
        assert!(!a[0].pool_id.contains("127.0.0.1"), "durable pool keys should not copy endpoint URLs");
    }

    #[test]
    fn codex_catalog_preserves_reported_models_and_efforts_without_cost_or_tier() {
        let payload = json!({"data":[
            {"model":"gpt-6-astra","isDefault":true,"hidden":false,
             "defaultReasoningEffort":"medium","supportedReasoningEfforts":[{"reasoningEffort":"low"},{"reasoningEffort":"medium"}],
             "inputModalities":["text","image"],"description":"secret-description-sentinel","cost":999},
            {"model":"gpt-6-sol","isDefault":false,"hidden":false,
             "defaultReasoningEffort":"medium","supportedReasoningEfforts":[{"reasoningEffort":"medium"}],
             "inputModalities":["text"]},
            {"model":"hidden-model","isDefault":false,"hidden":true,
             "defaultReasoningEffort":"medium","supportedReasoningEfforts":[{"reasoningEffort":"medium"}]}
        ],"nextCursor":null});
        let catalog = parse_codex_catalog(&payload, 1000).unwrap();
        assert_eq!(catalog.models.len(), 2);
        assert_eq!(catalog.models[0].model, "gpt-6-astra");
        assert_eq!(catalog.models[0].efforts, vec!["low", "medium"]);
        assert_eq!(catalog.models[0].default_effort.as_deref(), Some("medium"));
        assert!(catalog.models[0].is_default);
        assert_eq!(catalog.models[0].modalities, vec!["text", "image"]);
        assert!(!serde_json::to_string(&catalog)
            .unwrap()
            .contains("secret-description-sentinel"));
        assert!(!serde_json::to_string(&catalog).unwrap().contains("999"));
    }

    #[test]
    fn malformed_or_unsupported_catalog_cannot_create_a_route() {
        let invalid = [
            json!({"data":{}}),
            json!({"data":[{"model":"good","hidden":false,"supportedReasoningEfforts":"medium"}]}),
            json!({"data":[{"model":"good","hidden":false,"defaultReasoningEffort":"ultra","supportedReasoningEfforts":[{"reasoningEffort":"low"}]}]}),
            json!({"data":[{"model":"bad;command","hidden":false,"supportedReasoningEfforts":[{"reasoningEffort":"low"}]}]}),
            json!({"data":[{"model":"dup","hidden":false,"supportedReasoningEfforts":[{"reasoningEffort":"low"}]},{"model":"dup","hidden":false,"supportedReasoningEfforts":[{"reasoningEffort":"low"}]}]}),
        ];
        for value in invalid {
            assert!(parse_codex_catalog(&value, 1000).is_err(), "{value}");
        }
    }

    #[test]
    fn codex_tool_inventory_retains_only_connected_exact_tools_without_descriptions() {
        let raw = json!({"data":[
            {"name":"browser","runtimeStatus":"connected","toolsError":null,
             "tools":{"navigate":{"name":"navigate","description":"secret-tool-sentinel","inputSchema":{}}},
             "authStatus":"unsupported"},
            {"name":"offline","runtimeStatus":"failed","toolsError":"failed",
             "tools":{"navigate":{"name":"navigate","inputSchema":{}}},"authStatus":"unknown"},
            {"name":"loggedout","runtimeStatus":"connected","toolsError":null,
             "tools":{"navigate":{"name":"navigate","inputSchema":{}}},"authStatus":"notLoggedIn"}
        ],"nextCursor":null});
        let catalog = parse_codex_tools(&raw, 1_000).unwrap();
        assert_eq!(catalog.tools, BTreeSet::from(["browser/navigate".to_string()]));
        assert_eq!(catalog.expires_ms, 61_000);
        assert!(!serde_json::to_string(&catalog).unwrap().contains("secret-tool-sentinel"));
    }

    #[test]
    fn codex_tool_inventory_rejects_partial_or_malformed_claims() {
        for raw in [
            json!({"data":[],"nextCursor":"more"}),
            json!({"data":[{"name":"browser","runtimeStatus":"connected","toolsError":null,"tools":{"bad;name":{"name":"bad;name"}}}],"nextCursor":null}),
            json!({"data":[{"name":"browser","runtimeStatus":"connected","toolsError":null,"tools":{"navigate":{"name":"other"}}}],"nextCursor":null}),
        ] {
            assert!(parse_codex_tools(&raw, 1_000).is_err(), "{raw}");
        }
    }

    #[test]
    fn discovered_codex_models_become_task_selectable_routes_without_invented_context_or_cost() {
        use crate::auto_select::{select, Allowance, CapabilityTier, Sandbox, WorkUnit};
        let catalog = parse_codex_catalog(&json!({"data":[
            {"model":"gpt-6-astra","hidden":false,"isDefault":true,
             "defaultReasoningEffort":"medium","supportedReasoningEfforts":[{"reasoningEffort":"medium"},{"reasoningEffort":"high"}]},
            {"model":"gpt-6-sol","hidden":false,"isDefault":false,
             "defaultReasoningEffort":"medium","supportedReasoningEfforts":[{"reasoningEffort":"low"},{"reasoningEffort":"medium"}]},
            {"model":"new-unknown-model","hidden":false,"isDefault":false,
             "defaultReasoningEffort":"medium","supportedReasoningEfforts":[{"reasoningEffort":"medium"}]}
        ],"nextCursor":null}), 1_000).unwrap();
        let tools = parse_codex_tools(&json!({"data":[{"name":"browser","runtimeStatus":"connected",
            "authStatus":"unsupported","toolsError":null,"tools":{"navigate":{"name":"navigate"}}}],"nextCursor":null}), 1_000).unwrap();
        let routes = codex_auto_routes(&catalog, &tools, None, "system-codex", 1_000);
        assert_eq!(routes.len(), 2, "unknown models cannot gain invented capability");
        assert!(routes.iter().all(|r| r.context_limit.is_none() && r.fit == crate::auto_select::Fit::Unknown
            && r.quota == Allowance::Unknown));
        let mut work = WorkUnit { id:"browser-unit".into(), min_tier:CapabilityTier::General,
            required_tools:BTreeSet::from(["browser/navigate".into()]), context_needed:0,
            requires_approvals:false, min_sandbox:Sandbox::WorkspaceWrite,
            max_sandbox:Sandbox::WorkspaceWrite, allowed_profiles:BTreeSet::from(["system-codex".into()]),
            pinned_route:None, preferred_harness:None,
            task_class:None, execution_budget_ms:None };
        let browser = select(&work, &routes);
        assert_eq!(browser.selected.as_deref(), Some("system-codex/gpt-6-sol/medium"));
        work.id = "diagnosis-unit".into();
        work.min_tier = CapabilityTier::Frontier;
        let diagnosis = select(&work, &routes);
        assert_eq!(diagnosis.selected.as_deref(), Some("system-codex/gpt-6-astra/high"));
        work.context_needed = 1;
        assert!(select(&work, &routes).selected.is_none(), "unknown context limit must not satisfy a positive requirement");
    }
}
