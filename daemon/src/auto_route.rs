//! Allowlisted harness model discovery. A catalog reports availability and
//! supported efforts; it does not establish capability tier or allowance cost.

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;

use crate::auto_quota::QuotaSnapshot;
use crate::auto_select::{self, CapabilityTier, Fit, Health, Route, Sandbox};

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
            model: model.model.clone(), effort: (*effort).into(), tier: *tier,
            tools: tools.tools.clone(), context_limit: None, supports_approvals: true,
            sandbox: Sandbox::WorkspaceWrite, recommended_default: *recommended,
            quota: auto_select::observed_allowance(quota, &model.model, now_ms),
            fit: Fit::Unknown, health: Health::Unknown,
        });
    }
    routes
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
            pinned_route:None, preferred_harness:None };
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
