//! Authenticated native metadata only. Local management APIs remain separate.
use super::{bindings, delivery, error, library, revision};
use crate::daemon::Daemon;
use anyhow::Result;
use rusqlite::OptionalExtension;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeSet;

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Operation {
    List,
    Why,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    operation: Operation,
    run_id: Option<String>,
}

/// The caller and role come only from the authenticated token. Authorize, resolve
/// and project under one Store lock so a watch change cannot widen a read.
pub(crate) fn call(d: &Daemon, caller: &str, role: &str, params: &Value) -> Result<Value> {
    let request: Request = serde_json::from_value(params.clone()).map_err(|_| {
        error(
            "invalid_mod",
            "Mods reads accept only list or why with its run_id",
        )
    })?;
    let requested = match request.operation {
        Operation::List => {
            if params.get("run_id").is_some() {
                return Err(error("invalid_mod", "list does not accept a run target"));
            }
            None
        }
        Operation::Why => Some(
            request
                .run_id
                .as_deref()
                .filter(|id| !id.is_empty() && id.len() <= 512 && !id.chars().any(char::is_control))
                .ok_or_else(|| error("invalid_mod", "why needs a valid run_id"))?,
        ),
    };
    let store = d.store.lock().unwrap();
    let constrained = if role == "overseer" {
        None
    } else {
        let subject: Option<String> = store.conn.query_row(
            "SELECT subject FROM watches WHERE watcher=?1 AND ended_ms IS NULL ORDER BY created_ms, rowid LIMIT 1",
            [caller], |r| r.get(0),
        ).optional()?;
        match (role, subject) {
            ("agent", Some(subject)) | ("watcher", Some(subject)) => Some(subject),
            ("agent", None) => Some(caller.to_string()),
            ("watcher", None) => {
                return Err(error(
                    "invalid_mod",
                    "Mods reads need an active watch subject",
                ))
            }
            _ => return Err(error("invalid_mod", "This role has no native Mods reads")),
        }
    };
    if let (Some(allowed), Some(requested)) = (&constrained, requested) {
        if allowed != requested {
            return Err(error(
                "invalid_mod",
                "Mods reads are limited to your current task or watch subject",
            ));
        }
    }
    let target = requested.or(constrained.as_deref());
    let context = target.map(|id| bindings::context(&store, id)).transpose()?;
    let visible: Vec<_> = bindings::stored(&store)?
        .into_iter()
        .filter(|binding| {
            context
                .as_ref()
                .is_none_or(|context| binding.scope.matches(context))
        })
        .collect();
    let ids: BTreeSet<_> = visible.iter().map(|binding| binding.id.as_str()).collect();
    let result = match request.operation {
        Operation::List => {
            let fingerprints: BTreeSet<_> = visible
                .iter()
                .map(|binding| binding.fingerprint.as_str())
                .collect();
            let installed: Vec<_> = library::versions(&store)?
                .into_iter()
                .filter(|version| {
                    constrained.is_none() || fingerprints.contains(version.fingerprint.as_str())
                })
                .map(|version| metadata(&version.public()))
                .collect();
            json!({"revision":revision(&store)?,"installed":installed,"bindings":visible,
                "support":delivery::support(),"unavailable":[{"id":"less-tool-noise","reason":"Planned; external transformers are not implemented"}],
                "notice":"Native reads show metadata, not private text or source files. Installation and binding changes require the shared proposal controls."})
        }
        Operation::Why => {
            let mut applied = delivery::applied_from_store(&store, requested.unwrap())?;
            // The resolver's full decision ledger includes out-of-scope bindings.
            // Keep only the authorized target's decisions before serialization.
            if let Some(decisions) = applied["desired"]["decisions"].as_array_mut() {
                decisions.retain(|decision| {
                    decision["binding_id"]
                        .as_str()
                        .is_some_and(|id| ids.contains(id))
                });
            }
            if let Some(decisions) = applied["last_turn"]["plan"]["decisions"].as_array_mut() {
                // Historical scope decisions describe the target at that turn,
                // including bindings since removed. Do not disclose other scopes.
                decisions.retain(|decision| decision["status"] != "not_in_scope");
            }
            metadata(&applied)
        }
    };
    Ok(crate::daemon::redact_value(result))
}

/// Snapshot metadata remains useful (pins, state, transport, reason and pending),
/// while private bodies and source/file details never enter the native reply.
fn metadata(value: &Value) -> Value {
    match value {
        Value::Object(fields)
            if fields.contains_key("manifest") && fields.contains_key("fingerprint") =>
        {
            json!({"id":value["id"],"version":value["version"],"fingerprint":value["fingerprint"],"name":value["manifest"]["name"]})
        }
        Value::Object(fields) => Value::Object(
            fields
                .iter()
                .filter(|(key, _)| {
                    ![
                        "text",
                        "rules_text",
                        "style_text",
                        "contents",
                        "binding_snapshot",
                        "files",
                        "source",
                        "manifest",
                    ]
                    .contains(&key.as_str())
                })
                .map(|(key, value)| (key.clone(), metadata(value)))
                .collect(),
        ),
        Value::Array(values) => Value::Array(values.iter().map(metadata).collect()),
        _ => value.clone(),
    }
}
