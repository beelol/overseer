use super::required;
use crate::store::Store;
use anyhow::{anyhow, bail, Result};
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Map, Value};
use std::collections::HashSet;

fn defaults() -> Map<String, Value> {
    json!({
        "max_workers":8,
        "growth_per_wave":4,
        "growth_interval_ms":5000,
        "deadline_ms":3600000,
        "run_allocation_percent":10,
        "finishing_reserve_percent":20,
        "max_attempts":2,
        "review_hold":8,
        "review_resume":4,
        "director_batch_count":20,
        "director_batch_bytes":32768,
        "director_batch_delay_ms":5000,
        "worker_brief_max_bytes":32768,
        "backlog_max":1000,
        "ready_materialized_max":100,
        "allow_estimated_quota":false
    })
    .as_object()
    .unwrap()
    .clone()
}

fn validate_policy(v: &Value) -> Result<&Map<String, Value>> {
    let object = v
        .as_object()
        .ok_or_else(|| anyhow!("policy must be an object"))?;
    let built = defaults();
    for (key, value) in object {
        let Some(default) = built.get(key) else {
            bail!("unknown swarm policy setting {key}");
        };
        if default.is_boolean() {
            if !value.is_boolean() {
                bail!("policy {key} must be boolean");
            }
        } else if ["run_allocation_percent", "finishing_reserve_percent"].contains(&key.as_str()) {
            if value.as_i64().is_none_or(|n| !(1..=100).contains(&n)) {
                bail!("policy {key} must be an integer percentage from 1 to 100");
            }
        } else if value.as_i64().is_none_or(|n| n <= 0 || n > 86_400_000) {
            bail!("policy {key} must be a positive bounded integer");
        }
    }
    Ok(object)
}

fn validate_targets(v: &Value) -> Result<Vec<String>> {
    let arr = v
        .as_array()
        .ok_or_else(|| anyhow!("allowed_targets must be a string array"))?;
    if arr.len() > 100 {
        bail!("too many allowed targets");
    }
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    for item in arr {
        let id = item
            .as_str()
            .ok_or_else(|| anyhow!("allowed target must be a string"))?;
        if id.is_empty() || id.len() > 200 || id.chars().any(char::is_control) || !seen.insert(id) {
            bail!("invalid or duplicate allowed target");
        }
        result.push(id.to_string());
    }
    Ok(result)
}

fn saved(store: &Store, scope: &str, key: &str) -> Result<Option<(Value, Value)>> {
    let raw: Option<(String, String)> = store
        .conn
        .query_row(
            "SELECT policy,allowed_targets FROM swarm_policy_settings WHERE scope=?1 AND scope_key=?2",
            params![scope, key],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    raw.map(|(policy, targets)| {
        let mut policy: Value = serde_json::from_str(&policy)?;
        // Existing fixture databases may contain the former per-Swarm total
        // ceiling. The app setting supersedes it; retain all other settings.
        if let Some(values) = policy.as_object_mut() {
            values.remove("max_executing");
        }
        Ok((
            policy,
            serde_json::from_str(&targets)?,
        ))
    })
    .transpose()
}

pub fn set_policy(store: &mut Store, p: &Value) -> Result<Value> {
    let scope = required(p, "scope")?;
    let key = match scope {
        "application" => "*".to_string(),
        "category" => {
            let category = required(p, "category")?.trim();
            if category.is_empty() || category.len() > 160 {
                bail!("invalid category");
            }
            category.to_lowercase()
        }
        _ => bail!("scope must be application or category"),
    };
    let previous = saved(store, scope, &key)?;
    let policy = p
        .get("policy")
        .cloned()
        .unwrap_or_else(|| previous.as_ref().map(|v| v.0.clone()).unwrap_or(json!({})));
    validate_policy(&policy)?;
    let targets = p.get("allowed_targets").cloned().unwrap_or_else(|| {
        previous
            .as_ref()
            .map(|v| v.1.clone())
            .unwrap_or(Value::Null)
    });
    if !targets.is_null() {
        validate_targets(&targets)?;
    }
    store.conn.execute("INSERT INTO swarm_policy_settings(scope,scope_key,policy,allowed_targets,updated_ms) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(scope,scope_key) DO UPDATE SET policy=excluded.policy,allowed_targets=excluded.allowed_targets,updated_ms=excluded.updated_ms",
        params![scope,key,policy.to_string(),targets.to_string(),crate::daemon::now()])?;
    Ok(json!({"scope":scope,"scope_key":key,"policy":policy,"allowed_targets":targets}))
}

pub fn resolve(
    store: &Store,
    category: &str,
    run_policy: Option<&Value>,
    run_targets: Option<&Value>,
) -> Result<(Value, Value)> {
    let app = saved(store, "application", "*")?;
    let category_saved = saved(store, "category", &category.to_lowercase())?;
    let mut effective = defaults();
    let mut sources: Map<String, Value> = effective
        .keys()
        .map(|k| (k.clone(), json!("built_in")))
        .collect();
    for (name, layer) in [
        ("application", app.as_ref().map(|v| &v.0)),
        ("category", category_saved.as_ref().map(|v| &v.0)),
        ("run", run_policy),
    ] {
        if let Some(value) = layer {
            for (key, item) in validate_policy(value)? {
                effective.insert(key.clone(), item.clone());
                sources.insert(key.clone(), json!(name));
            }
        }
    }
    if effective["review_resume"].as_i64().unwrap() >= effective["review_hold"].as_i64().unwrap() {
        bail!("review_resume must be below review_hold");
    }
    let (targets, source) = if let Some(value) = run_targets {
        (validate_targets(value)?, "run")
    } else if let Some((_, value)) = category_saved.as_ref().filter(|(_, v)| !v.is_null()) {
        (validate_targets(value)?, "category")
    } else if let Some((_, value)) = app.as_ref().filter(|(_, v)| !v.is_null()) {
        (validate_targets(value)?, "application")
    } else {
        (Vec::new(), "built_in")
    };
    Ok((
        json!({"effective":effective,"sources":sources,"allowed_targets_source":source}),
        json!(targets),
    ))
}
