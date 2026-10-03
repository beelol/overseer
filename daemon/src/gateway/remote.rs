//! What a device may ask, inside an authenticated session. Every request is checked against its
//! method's class and the device's scope before anything runs.

use super::classes::{self, Class, Scope};
use crate::daemon::Daemon;
use crate::server::{self, error_value, ProtoError};
use anyhow::Result;
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::sync::mpsc;

pub struct Ctx {
    pub device_id: String,
    pub device_name: String,
}

fn refusal(code: &str) -> &'static str {
    if code == "mac_only" {
        "this is available on the Mac only"
    } else if code == "watch_only" {
        "this phone may watch but not control; change its scope on the Mac"
    } else {
        "unknown method"
    }
}

fn kinds() -> [&'static str; 4] {
    ["permission", "question", "failure", "finished"]
}

/// A device's notification switches with every kind present.
pub fn notification_settings(stored: &Value) -> Value {
    let mut kinds_out = serde_json::Map::new();
    for k in kinds() {
        kinds_out.insert(k.into(), json!(stored["kinds"][k].as_bool().unwrap_or(true)));
    }
    json!({
        "enabled": stored["enabled"].as_bool().unwrap_or(false),
        "kinds": kinds_out,
        "show_text": stored["show_text"].as_bool().unwrap_or(false),
        "token": stored["token"].as_str(),
        "environment": stored["environment"].as_str().unwrap_or("development"),
        "bundle": stored["bundle"].as_str(),
    })
}

/// Methods that concern only the calling device. They never reach the daemon's dispatch.
fn own(d: &Arc<Daemon>, ctx: &Ctx, method: &str, p: &Value) -> Result<Value> {
    let device = d.store.lock().unwrap().device(&ctx.device_id)?.ok_or_else(|| ProtoError::new("revoked", "this phone is no longer paired"))?;
    Ok(match method {
        "hello" => json!({
            "protocol": server::PROTOCOL_VERSION, "version": env!("CARGO_PKG_VERSION"), "now_ms": super::now_ms(),
            "device": {"id": device.id, "name": device.name, "scope": device.scope, "platform": device.platform},
            "gateway": {"name": super::net::host_name(), "fingerprint": d.gateway.identity()?.fingerprint()},
            "notifications": notification_settings(&device.notifications),
            "mac_notifications": super::setting(d, "notifications").as_deref() != Some("0"),
        }),
        "ping" => json!({"now_ms": super::now_ms()}),
        "device.notifications" => {
            let mut next = notification_settings(&device.notifications);
            let mut changed = false;
            if let Some(v) = p["enabled"].as_bool() {
                next["enabled"] = json!(v);
                changed = true;
            }
            if let Some(v) = p["show_text"].as_bool() {
                next["show_text"] = json!(v);
                changed = true;
            }
            for k in kinds() {
                if let Some(v) = p["kinds"][k].as_bool() {
                    next["kinds"][k] = json!(v);
                    changed = true;
                }
            }
            for key in ["token", "environment", "bundle"] {
                if let Some(v) = p[key].as_str() {
                    if v.len() > 512 || v.chars().any(|c| c.is_control()) {
                        return Err(ProtoError::new("invalid_params", format!("{key} is not valid")).into());
                    }
                    next[key] = json!(v);
                    changed = true;
                } else if p[key].is_null() && p.get(key).is_some() {
                    next[key] = Value::Null;
                    changed = true;
                }
            }
            if changed {
                d.store.lock().unwrap().device_set_notifications(&ctx.device_id, &next)?;
            }
            next
        }
        other => return Err(ProtoError::new("unknown_method", format!("unknown method {other}")).into()),
    })
}

/// Limits that apply to a device and not to the Mac.
fn guard(d: &Arc<Daemon>, method: &str, p: &Value) -> Result<()> {
    let known = |path: &str| -> Result<()> {
        if d.repo_is_known(path)? {
            Ok(())
        } else {
            Err(ProtoError::new("mac_only", "from a phone, agents start in repositories Overseer already knows; use this repository on the Mac once first").into())
        }
    };
    match method {
        "task.create" => {
            // Parameters the protocol does not describe were refused before this; this is the
            // second check for the one that matters most.
            if p["harness"].as_str() == Some("generic") || ["program", "args", "extra_args", "approval_policy"].iter().any(|k| p.get(*k).is_some()) {
                return Err(ProtoError::new("mac_only", "starting a program is available on the Mac only").into());
            }
            known(p["repo"].as_str().unwrap_or_default())
        }
        "repo.inspect" => known(p["path"].as_str().unwrap_or_default()),
        "repo.files" => match p["repo"].as_str() {
            Some(repo) if p["workspace_id"].as_str().is_none() => known(repo),
            _ => Ok(()),
        },
        _ => Ok(()),
    }
}

async fn reply(tx: &mpsc::Sender<Value>, id: &Value, body: Value) {
    let mut message = serde_json::Map::new();
    message.insert("id".into(), id.clone());
    if let Value::Object(fields) = body {
        message.extend(fields);
    }
    let _ = tx.send(Value::Object(message)).await;
}

pub async fn handle(d: &Arc<Daemon>, ctx: &Ctx, bytes: Vec<u8>, tx: &mpsc::Sender<Value>) {
    let msg: Value = match serde_json::from_slice(&bytes) {
        Ok(v) => v,
        Err(e) => return reply(tx, &Value::Null, json!({"error": {"code": "parse_error", "message": e.to_string()}})).await,
    };
    let id = msg.get("id").cloned().unwrap_or(Value::Null);
    let Some(method) = msg.get("method").and_then(Value::as_str).map(str::to_string) else {
        return reply(tx, &id, json!({"error": {"code": "invalid_request", "message": "method must be a string"}})).await;
    };
    let params = msg.get("params").cloned().unwrap_or(json!({}));
    if !params.is_object() {
        return reply(tx, &id, json!({"error": {"code": "invalid_params", "message": "params must be an object"}})).await;
    }
    // The scope is read for every request, so a change on the Mac applies at once.
    let scope = d.store.lock().unwrap().device(&ctx.device_id).ok().flatten().filter(|dev| dev.revoked_ms.is_none()).and_then(|dev| Scope::parse(&dev.scope));
    let Some(scope) = scope else {
        return reply(tx, &id, json!({"error": {"code": "revoked", "message": "this phone is no longer paired"}})).await;
    };
    let class = match classes::allowed(&method, scope) {
        Ok(class) => class,
        Err(code) => {
            crate::log(&format!("gateway: \"{}\" was refused {method}: {code}", ctx.device_name));
            return reply(tx, &id, json!({"error": {"code": code, "message": refusal(code)}})).await;
        }
    };
    let extra = classes::undeclared_params(&method, &params);
    if !extra.is_empty() {
        crate::log(&format!("gateway: \"{}\" was refused {method}: parameters a phone may not send ({})", ctx.device_name, extra.join(", ")));
        return reply(tx, &id, json!({"error": {"code": "invalid_params", "message": format!("{method} from a phone does not take: {}", extra.join(", ")), "data": {"parameters": extra}}})).await;
    }
    if method == "events.subscribe" {
        return server::subscribe(d.clone(), id, params, tx.clone());
    }
    let request_id = msg.get("request_id").and_then(Value::as_str).map(str::to_string);
    if class == Class::Control {
        let ok = request_id.as_deref().is_some_and(|r| (8..=64).contains(&r.len()) && r.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'));
        if !ok {
            return reply(tx, &id, json!({"error": {"code": "request_id_required", "message": "a changing request from a phone carries a request id"}})).await;
        }
    }
    let daemon = d.clone();
    let tx = tx.clone();
    let device_id = ctx.device_id.clone();
    let actor = format!("phone:{}", ctx.device_name);
    let ctx = Ctx { device_id: ctx.device_id.clone(), device_name: ctx.device_name.clone() };
    tokio::spawn(async move {
        let work = tokio::task::spawn_blocking(move || {
            server::with_actor(Some(actor), || {
                let run = || -> Value {
                    let outcome = match class {
                        Class::Own => own(&daemon, &ctx, &method, &params),
                        _ => guard(&daemon, &method, &params).and_then(|_| {
                            if class == Class::Control {
                                let _ = daemon.emit(None, params["run_id"].as_str(), "remote_command", "user", "exact", json!({"method": method, "device": ctx.device_id, "request_id": request_id}));
                            }
                            server::with_native_authority(server::NativeAuthority::Device(ctx.device_id.clone()),
                                || server::dispatch(&daemon, &method, &params))
                        }),
                    };
                    match outcome {
                        Ok(v) => json!({"result": v}),
                        Err(e) => json!({"error": error_value(&e)}),
                    }
                };
                match (class, &request_id) {
                    (Class::Control, Some(rid)) => super::once(&daemon, &device_id, rid, &method, run),
                    _ => run(),
                }
            })
        })
        .await;
        let body = work.unwrap_or_else(|e| json!({"error": {"code": "internal", "message": e.to_string()}}));
        reply(&tx, &id, body).await;
    });
}
