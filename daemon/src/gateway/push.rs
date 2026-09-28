//! Needs-you notifications to paired phones (AC-129). The daemon builds each notification and
//! hands it to a delivery route; there is no Overseer server. A notification names the agent by
//! its harness and repository and says what kind of moment it is. It carries no prompt, code or
//! file content unless the owner turned that on for the device.

use super::devices::Device;
use super::remote::notification_settings;
use crate::daemon::Daemon;
use crate::store::Event;
use serde_json::{json, Value};
use std::sync::Arc;

pub const CATEGORY_PERMISSION: &str = "OVERSEER_PERMISSION";
pub const CATEGORY_AGENT: &str = "OVERSEER_AGENT";
/// Everything a notification may hold. A test compares every payload with this list.
pub const ALLOWED_FIELDS: &[&str] = &[
    "aps.alert.title", "aps.alert.body", "aps.category", "aps.thread-id", "aps.sound", "aps.interruption-level",
    "overseer.v", "overseer.kind", "overseer.run_id", "overseer.task_id", "overseer.request_id", "overseer.device",
    "Simulator Target Bundle",
];

/// What happened, as a phone is told: `permission`, `question`, `failure` or `finished`.
fn kind_of(event: &Event) -> Option<&'static str> {
    match event.kind.as_str() {
        "permission" => Some(if event.payload["kind"] == "question" { "question" } else { "permission" }),
        "status" => match event.payload["status"].as_str() {
            Some("failed") => Some("failure"),
            Some("completed") => Some("finished"),
            _ => None,
        },
        _ => None,
    }
}

fn sentence(kind: &str) -> &'static str {
    match kind {
        "permission" => "Needs your permission",
        "question" => "Has a question for you",
        "failure" => "Stopped with an error",
        _ => "Finished",
    }
}

fn harness_name(harness: &str) -> &str {
    match harness {
        "claude" => "Claude",
        "codex" | "codex-app" => "Codex",
        "opencode" => "OpenCode",
        _ => "Agent",
    }
}

/// The notification for one device. `show_text` adds the agent's title and its last line.
pub fn payload(kind: &str, run: &crate::store::Run, repo: &str, request_id: Option<&str>, device: &Device, settings: &Value, last_line: Option<&str>) -> Value {
    let repo_name = std::path::Path::new(repo).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let show_text = settings["show_text"].as_bool() == Some(true);
    // Text the owner asked for still goes through the daemon's redaction of secrets.
    let title = if show_text && !run.title.is_empty() { crate::redact::redact(&run.title).chars().take(80).collect::<String>() } else { format!("{} · {repo_name}", harness_name(&run.harness)) };
    let body = match (show_text, last_line) {
        (true, Some(line)) if !line.trim().is_empty() => format!("{}: {}", sentence(kind), crate::redact::redact(line.trim()).chars().take(140).collect::<String>()),
        _ => sentence(kind).to_string(),
    };
    let mut overseer = json!({"v": 1, "kind": kind, "run_id": run.id, "task_id": run.task_id, "device": device.id});
    if let Some(r) = request_id {
        overseer["request_id"] = json!(r);
    }
    json!({
        "aps": {
            "alert": {"title": title, "body": body},
            "category": if kind == "permission" { CATEGORY_PERMISSION } else { CATEGORY_AGENT },
            "thread-id": run.id,
            "sound": "default",
            "interruption-level": if kind == "finished" { "active" } else { "time-sensitive" },
        },
        "overseer": overseer,
    })
}

/// Every leaf of a payload as a dotted path, for the allowed-fields check.
pub fn fields(value: &Value, prefix: &str, out: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            for (k, v) in map {
                let path = if prefix.is_empty() { k.clone() } else { format!("{prefix}.{k}") };
                fields(v, &path, out);
            }
        }
        _ => out.push(prefix.to_string()),
    }
}

enum Route {
    /// Test-only: payloads are written to this directory instead of being sent.
    Files(std::path::PathBuf),
    /// The iOS simulator, through its own tool.
    Simulator { target: String, bundle: String },
    /// Apple's push service. Needs Overseer's own push key, which is a step for the owner.
    Apple,
    /// Android in this gate: the app shows notifications itself while it is open.
    InApp,
    Nothing(&'static str),
}

fn route(device: &Device, settings: &Value) -> Route {
    if let Some(dir) = std::env::var_os("OVERSEER_TEST_PUSH_DIR") {
        return Route::Files(dir.into());
    }
    if device.platform == "android" {
        return Route::InApp;
    }
    let Some(token) = settings["token"].as_str().filter(|t| !t.is_empty()) else { return Route::Nothing("this phone has not given a push token") };
    match settings["environment"].as_str() {
        Some("simulator") => Route::Simulator { target: if token.len() == 36 && token.contains('-') { token.to_string() } else { "booted".into() }, bundle: settings["bundle"].as_str().unwrap_or("com.beelol.overseer.phone").to_string() },
        _ => Route::Apple,
    }
}

fn deliver(route: &Route, payload: &Value, device: &Device) -> (&'static str, &'static str, String) {
    match route {
        Route::Files(dir) => {
            let _ = std::fs::create_dir_all(dir);
            let name = format!("{}-{}.json", crate::shim::now_ms(), uuid::Uuid::new_v4().simple());
            match std::fs::write(dir.join(name), serde_json::to_vec_pretty(&json!({"device": device.id, "payload": payload})).unwrap_or_default()) {
                Ok(_) => ("files", "sent", String::new()),
                Err(e) => ("files", "not_sent", e.to_string()),
            }
        }
        Route::Simulator { target, bundle } => {
            let mut body = payload.clone();
            body["Simulator Target Bundle"] = json!(bundle);
            let dir = crate::paths::data_dir().join("tmp");
            let _ = crate::paths::ensure_private_dir(&dir);
            let file = dir.join(format!("push-{}.apns", uuid::Uuid::new_v4().simple()));
            if let Err(e) = std::fs::write(&file, body.to_string()) {
                return ("simulator", "not_sent", e.to_string());
            }
            let out = std::process::Command::new("/usr/bin/xcrun").args(["simctl", "push", target, bundle]).arg(&file).output();
            let _ = std::fs::remove_file(&file);
            match out {
                Ok(o) if o.status.success() => ("simulator", "sent", String::new()),
                Ok(o) => ("simulator", "not_sent", String::from_utf8_lossy(&o.stderr).trim().chars().take(200).collect()),
                Err(e) => ("simulator", "not_sent", e.to_string()),
            }
        }
        Route::Apple => ("apple", "not_sent", "this Mac has no push key for Overseer yet (see the steps for the owner)".into()),
        Route::InApp => ("in_app", "not_sent", "Android shows notifications in the app in this gate".into()),
        Route::Nothing(why) => ("none", "not_sent", (*why).to_string()),
    }
}

/// True while a window on the Mac is focused on this agent: the owner is looking at it already.
fn watched_on_the_mac(d: &Daemon, run_id: &str) -> bool {
    d.gateway.focus.lock().unwrap().values().any(|focused| focused == run_id)
}

fn notify(d: &Arc<Daemon>, event: &Event, kind: &'static str) {
    let Some(run_id) = event.run_id.as_deref() else { return };
    let Ok(run) = d.run(run_id) else { return };
    if run.parent_run_id.is_some() {
        return; // a child's moments belong to its parent's conversation
    }
    let devices = match d.store.lock().unwrap().devices() {
        Ok(list) => list,
        Err(_) => return,
    };
    let devices: Vec<Device> = devices.into_iter().filter(|dev| dev.revoked_ms.is_none()).collect();
    if devices.is_empty() {
        return;
    }
    let repo = d.store.lock().unwrap().tasks().ok().and_then(|t| t.into_iter().find(|t| t.id == run.task_id)).map(|t| t.repo_root).unwrap_or_default();
    let request_id = if kind == "permission" || kind == "question" { event.payload["request_id"].as_str() } else { None };
    let mac_on = super::setting(d, "notifications").as_deref() != Some("0");
    let watched = watched_on_the_mac(d, run_id);
    for device in devices {
        let settings = notification_settings(&device.notifications);
        let why_not = if !mac_on {
            Some("notifications to phones are off on the Mac")
        } else if settings["enabled"].as_bool() != Some(true) {
            Some("notifications are off on this phone")
        } else if settings["kinds"][kind].as_bool() != Some(true) {
            Some("this kind of notification is off on this phone")
        } else if watched {
            Some("the owner is looking at this agent on the Mac")
        } else {
            None
        };
        let record = |route: &str, outcome: &str, why: &str, sent_fields: Vec<String>| {
            let _ = d.emit(Some(&run.task_id), Some(&run.id), "push", "gateway", "exact", json!({"device": device.id, "name": device.name, "kind": kind, "route": route, "outcome": outcome, "why": why, "fields": sent_fields}));
        };
        if let Some(why) = why_not {
            record("none", "not_sent", why, Vec::new());
            continue;
        }
        let last_line = if settings["show_text"].as_bool() == Some(true) { d.last_line(&run.id) } else { None };
        let body = payload(kind, &run, &repo, request_id, &device, &settings, last_line.as_deref());
        let mut sent_fields = Vec::new();
        fields(&body, "", &mut sent_fields);
        let (route_name, outcome, why) = deliver(&route(&device, &settings), &body, &device);
        record(route_name, outcome, &why, sent_fields);
    }
}

/// Follows the daemon's events and tells the phones when an agent needs the owner.
pub fn watch(d: Arc<Daemon>) {
    let mut events = d.events.subscribe();
    tokio::spawn(async move {
        loop {
            match events.recv().await {
                Ok(event) => {
                    if let Some(kind) = kind_of(&event) {
                        let d = d.clone();
                        let _ = tokio::task::spawn_blocking(move || notify(&d, &event, kind)).await;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => return,
            }
        }
    });
}

impl Daemon {
    /// The agent's last line of text, for a notification whose owner asked for text.
    pub fn last_line(&self, run_id: &str) -> Option<String> {
        let store = self.store.lock().unwrap();
        store
            .conn
            .query_row("SELECT payload FROM events WHERE run_id=?1 AND kind='output' ORDER BY seq DESC LIMIT 1", rusqlite::params![run_id], |r| r.get::<_, String>(0))
            .ok()
            .and_then(|p| serde_json::from_str::<Value>(&p).ok())
            .and_then(|v| v["text"].as_str().map(|t| t.lines().last().unwrap_or_default().to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(title: &str) -> crate::store::Run {
        crate::store::Run {
            id: "r-1".into(), task_id: "t-1".into(), parent_run_id: None, harness: "claude".into(), harness_version: None, profile_id: None, model: None, effort: None,
            workspace_id: "w-1".into(), native_id: None, status: "waiting_for_user".into(), exit_reason: None, created_ms: 0, ended_ms: None, title: title.into(),
            relation_source: None, relation_confidence: None, capabilities: Value::Null, process_generation: 1, attention: None,
        }
    }

    fn device() -> Device {
        Device { id: "d-1".into(), name: "Phone".into(), platform: "ios".into(), public_key: "00".into(), scope: "full".into(), paired_ms: 0, last_seen_ms: None, last_addr: None, last_counter: 0, revoked_ms: None, notifications: Value::Null, app: None }
    }

    #[test]
    fn a_notification_holds_only_the_allowed_fields_and_no_text_of_the_work() {
        let secret = "Refactor the billing module and rotate the API key sk-live";
        let body = payload("permission", &run(secret), "/Users/someone/projects/shop", Some("req-1"), &device(), &json!({"show_text": false}), Some("writing src/billing.ts"));
        let mut got = Vec::new();
        fields(&body, "", &mut got);
        for f in &got {
            assert!(ALLOWED_FIELDS.contains(&f.as_str()), "{f} is not an allowed field");
        }
        let text = body.to_string();
        assert!(!text.contains("billing") && !text.contains("Refactor") && !text.contains("sk-live") && !text.contains("/Users/"), "{text}");
        assert_eq!(body["aps"]["alert"]["title"], "Claude · shop");
        assert_eq!(body["aps"]["alert"]["body"], "Needs your permission");
        assert_eq!(body["aps"]["category"], CATEGORY_PERMISSION);
        assert_eq!(body["overseer"]["request_id"], "req-1");
        assert!(text.len() < 4096, "within the size a push allows");
        // The owner turned text on for this phone.
        let shown = payload("failure", &run(secret), "/x/shop", None, &device(), &json!({"show_text": true}), Some("error: tests failed"));
        assert_eq!(shown["aps"]["alert"]["body"], "Stopped with an error: error: tests failed");
        assert!(shown["aps"]["alert"]["title"].as_str().unwrap().starts_with("Refactor"));
        assert!(shown["overseer"].get("request_id").is_none());
    }
}
