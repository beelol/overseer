//! Managing phone access, from the Mac only: the local socket reaches these methods and no
//! device ever does (their class is `mac_only`, and a device's request is refused before dispatch).

use super::{classes::Scope, net, pairing};
use crate::daemon::Daemon;
use crate::server::ProtoError;
use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};
use std::sync::Arc;

fn text<'a>(p: &'a Value, key: &str) -> Result<&'a str> {
    p[key].as_str().ok_or_else(|| anyhow!("missing string parameter {key}"))
}

pub fn dispatch(d: &Arc<Daemon>, method: &str, p: &Value) -> Result<Value> {
    if crate::server::actor().is_some() {
        return Err(ProtoError::new("mac_only", "phone access is managed on the Mac").into());
    }
    Ok(match method {
        "gateway.status" => status(d)?,
        "gateway.enable" => {
            let port = match p["port"].as_u64() {
                Some(n) if n <= u16::MAX as u64 => Some(n as u16),
                Some(_) => bail!("port must be 0 to 65535"),
                None => None,
            };
            super::enable(d, port)?;
            status(d)?
        }
        "gateway.disable" => {
            let out = super::disable(d)?;
            let mut s = status(d)?;
            s["closed"] = out["closed"].clone();
            s
        }
        "gateway.settings" => {
            if let Some(on) = p["notifications"].as_bool() {
                super::set_setting(d, "notifications", if on { "1" } else { "0" })?;
            }
            if let Some(list) = p["allow"].as_array() {
                let mut ranges = Vec::new();
                for item in list {
                    let text = item.as_str().unwrap_or_default();
                    if net::Cidr::parse(text).is_none() {
                        bail!("{text} is not a range like 100.64.0.0/10");
                    }
                    ranges.push(text.trim().to_string());
                }
                super::set_setting(d, "allow", &ranges.join(","))?;
            }
            settings(d)
        }
        "gateway.pair_start" => pair_start(d)?,
        "gateway.pair_cancel" => {
            let was_open = d.gateway.pairing.lock().unwrap().take().is_some();
            if was_open {
                d.emit(None, None, "pairing_closed", "gateway", "exact", json!({"reason": "cancelled on the Mac"}))?;
            }
            json!({"open": false})
        }
        "gateway.pair_confirm" => {
            let request = text(p, "request")?;
            let accept = p["accept"].as_bool().unwrap_or(false);
            let pending = d.gateway.pairing.lock().unwrap().as_mut().and_then(|pairing| pairing.pending.remove(request));
            let Some(pending) = pending else { bail!("no phone is waiting to pair") };
            let name = pending.name.clone();
            pending.decide.send(accept).map_err(|_| anyhow!("the phone stopped waiting"))?;
            json!({"accepted": accept, "name": name})
        }
        "gateway.devices" => json!({"devices": devices(d)?}),
        "gateway.device_revoke" => {
            let id = text(p, "id")?;
            let authority = d.native_device_gate(id);
            let guard = authority.lock().unwrap();
            let device = d.store.lock().unwrap().device(id)?.ok_or_else(|| anyhow!("no such device"))?;
            let changed = d.store.lock().unwrap().device_revoke(id, super::now_ms())?;
            drop(guard); // Durable authority changes precede session closure.
            let closed = super::end_sessions(d, |s| s.device_id == id, "revoked");
            if changed {
                d.emit(None, None, "device_revoked", "gateway", "exact", json!({"device": id, "name": device.name, "closed": closed}))?;
            }
            json!({"revoked": true, "changed": changed, "closed": closed})
        }
        "gateway.device_scope" => {
            let id = text(p, "id")?;
            let scope = Scope::parse(text(p, "scope")?).ok_or_else(|| anyhow!("scope is full or watch"))?;
            let authority = d.native_device_gate(id);
            let guard = authority.lock().unwrap();
            if !d.store.lock().unwrap().device_set_scope(id, scope.as_str())? {
                bail!("no such device, or it was revoked");
            }
            drop(guard);
            d.emit(None, None, "device_scope", "gateway", "exact", json!({"device": id, "scope": scope.as_str()}))?;
            json!({"device": id, "scope": scope.as_str()})
        }
        "gateway.device_rename" => {
            let id = text(p, "id")?;
            let name: String = text(p, "name")?.chars().filter(|c| !c.is_control()).take(60).collect::<String>().trim().to_string();
            if name.is_empty() {
                bail!("a device needs a name");
            }
            if !d.store.lock().unwrap().device_rename(id, &name)? {
                bail!("no such device");
            }
            json!({"device": id, "name": name})
        }
        other => return Err(ProtoError::new("unknown_method", format!("unknown method {other}")).into()),
    })
}

fn settings(d: &Daemon) -> Value {
    json!({
        "notifications": super::setting(d, "notifications").as_deref() != Some("0"),
        "allow": super::setting(d, "allow").unwrap_or_default().split(',').filter(|s| !s.is_empty()).collect::<Vec<_>>(),
        "port": super::configured_port(d),
    })
}

/// Paired devices as the Mac lists them: no keys, and whether each is connected now.
fn devices(d: &Daemon) -> Result<Vec<Value>> {
    let connected: Vec<(String, String, i64)> = d.gateway.sessions.lock().unwrap().values().map(|s| (s.device_id.clone(), s.addr.clone(), s.since_ms)).collect();
    let mut out = Vec::new();
    for dev in d.store.lock().unwrap().devices()? {
        let live = connected.iter().find(|(id, _, _)| *id == dev.id);
        out.push(json!({
            "id": dev.id, "name": dev.name, "platform": dev.platform, "scope": dev.scope,
            "fingerprint": super::noise::unhex(&dev.public_key).map(|k| super::noise::fingerprint(&k)).unwrap_or_default(),
            "paired_ms": dev.paired_ms, "last_seen_ms": dev.last_seen_ms, "address": live.map(|l| l.1.clone()).or(dev.last_addr),
            "revoked_ms": dev.revoked_ms, "connected": live.is_some(), "connected_since_ms": live.map(|l| l.2), "app": dev.app,
            "notifications": dev.notifications.get("enabled").and_then(Value::as_bool).unwrap_or(false),
        }));
    }
    Ok(out)
}

pub fn status(d: &Arc<Daemon>) -> Result<Value> {
    let port = d.gateway.port();
    let enabled = port.is_some();
    let pairing = d.gateway.pairing.lock().unwrap().as_ref().filter(|p| !p.expired()).map(|p| {
        json!({"open": p.accepts_attempts(), "remaining_ms": p.remaining_ms(), "failures": p.failures,
               "waiting": p.pending.iter().map(|(id, w)| json!({"request": id, "name": w.name, "platform": w.platform, "address": w.addr, "fingerprint": w.fingerprint})).collect::<Vec<_>>()})
    });
    let fingerprint = if enabled || super::setting(d, "enabled").is_some() { d.gateway.identity().ok().map(|i| i.fingerprint()) } else { None };
    let all = devices(d)?;
    Ok(json!({
        "enabled": enabled, "port": port, "fingerprint": fingerprint, "name": net::host_name(),
        "addresses": if enabled { net::local_addresses() } else { Vec::new() },
        "sessions": d.gateway.session_count(),
        "paired": all.iter().filter(|dev| dev["revoked_ms"].is_null()).count(),
        "devices": all, "pairing": pairing, "settings": settings(d),
        "awake": d.gateway.power.is_held(),
    }))
}

fn pair_start(d: &Arc<Daemon>) -> Result<Value> {
    let Some(port) = d.gateway.port() else {
        return Err(ProtoError::new("phone_access_off", "turn phone access on before pairing a phone").into());
    };
    let identity = d.gateway.identity()?;
    let opened = pairing::Pairing::open()?;
    let addresses = net::local_addresses();
    let code = pairing::code(&identity.public, &opened.secret, port, &addresses);
    let remaining = opened.remaining_ms();
    *d.gateway.pairing.lock().unwrap() = Some(opened);
    d.emit(None, None, "pairing_opened", "gateway", "exact", json!({"valid_ms": remaining}))?;
    Ok(json!({"code": code, "valid_ms": remaining, "port": port, "addresses": addresses, "fingerprint": identity.fingerprint(), "name": net::host_name()}))
}
