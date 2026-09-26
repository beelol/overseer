//! Opt-in, daemon-owned Reactor cues. No audio work occurs while disabled.
use crate::daemon::Daemon;
use crate::paths;
use crate::store::Event;
use anyhow::{anyhow, Result};
use rusqlite::params;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

const KEYS: &[&str] = &[
    "agent_queued",
    "agent_started",
    "agent_resumed",
    "agent_progress",
    "agent_complete",
    "agent_stopped",
    "agent_needs_attention",
    "agent_failed",
    "agent_unblocked",
    "review_ready",
    "delivery_ready",
    "verification_passed",
];
const DEFAULT_KEYS: &[&str] = &["agent_started", "agent_complete", "agent_needs_attention"];
const MANIFEST: &str = include_str!("../assets/reactor/manifest.json");

struct Cue {
    key: &'static str,
    preview: bool,
}

struct Runtime {
    enabled: AtomicBool,
    queue: mpsc::Sender<Cue>,
}
static RUNTIME: OnceLock<Runtime> = OnceLock::new();

fn available() -> bool {
    cfg!(target_os = "macos") && std::path::Path::new("/usr/bin/afplay").exists()
}

fn stored_enabled(d: &Arc<Daemon>) -> Result<bool> {
    let store = d.store.lock().unwrap();
    let value: Option<String> = store
        .conn
        .query_row(
            "SELECT value FROM meta WHERE key='audio.reactor.enabled'",
            [],
            |r| r.get(0),
        )
        .optional()?;
    Ok(value.as_deref() == Some("1"))
}

use rusqlite::OptionalExtension;

pub fn get(d: &Arc<Daemon>) -> Result<Value> {
    Ok(json!({
        "enabled": stored_enabled(d)?,
        "available": available(),
        "pack": "reactor",
        "keys": KEYS,
        "default_keys": DEFAULT_KEYS,
        "manifest": serde_json::from_str::<Value>(MANIFEST)?,
    }))
}

pub fn set(d: &Arc<Daemon>, p: &Value) -> Result<Value> {
    let enabled = p["enabled"]
        .as_bool()
        .ok_or_else(|| anyhow!("enabled must be a boolean"))?;
    if enabled && !available() {
        return Err(anyhow!("Reactor audio requires macOS afplay"));
    }
    d.store.lock().unwrap().conn.execute(
        "INSERT INTO meta(key,value) VALUES('audio.reactor.enabled',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        params![if enabled { "1" } else { "0" }],
    )?;
    if let Some(runtime) = RUNTIME.get() {
        runtime.enabled.store(enabled, Ordering::Relaxed);
    }
    Ok(json!({"enabled": enabled, "available": available(), "pack": "reactor"}))
}

pub fn preview(p: &Value) -> Result<Value> {
    let key = p["key"]
        .as_str()
        .ok_or_else(|| anyhow!("key must be a string"))?;
    let selected = KEYS
        .iter()
        .copied()
        .find(|candidate| *candidate == key)
        .ok_or_else(|| anyhow!("unknown Reactor cue"))?;
    if !available() {
        return Err(anyhow!("Reactor audio requires macOS afplay"));
    }
    enqueue(selected, true);
    Ok(json!({"queued": true, "key": selected}))
}

fn enqueue(key: &'static str, preview: bool) {
    if let Some(runtime) = RUNTIME.get() {
        // Bounded queue prevents a burst of agent events from growing memory or noise.
        let _ = runtime.queue.try_send(Cue { key, preview });
    }
}

/// Start once after reconciliation. Subscribe only to future live events: a daemon
/// restart must not replay old cues.
pub fn start(d: Arc<Daemon>) -> Result<()> {
    let enabled = stored_enabled(&d)?;
    let mut events = d.events.subscribe();
    let (tx, mut rx) = mpsc::channel::<Cue>(4);
    let _ = RUNTIME.set(Runtime {
        enabled: AtomicBool::new(enabled),
        queue: tx,
    });
    tokio::spawn(async move {
        while let Some(cue) = rx.recv().await {
            if !cue.preview
                && !RUNTIME
                    .get()
                    .is_some_and(|runtime| runtime.enabled.load(Ordering::Relaxed))
            {
                continue;
            }
            match tokio::task::spawn_blocking(move || play(cue.key)).await {
                Ok(Ok(())) => {}
                Ok(Err(e)) => crate::log(&format!("audio playback failed: {e}")),
                Err(e) => crate::log(&format!("audio worker ended: {e}")),
            }
        }
    });
    tokio::spawn(async move {
        let mut attention = HashSet::<String>::new();
        let mut last: Option<(&'static str, Instant)> = None;
        loop {
            let event = match events.recv().await {
                Ok(event) => event,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => break,
            };
            let Some(runtime) = RUNTIME.get() else { break };
            if !runtime.enabled.load(Ordering::Relaxed) {
                continue;
            }
            let Some(run_id) = event.run_id.as_deref() else {
                continue;
            };
            let is_root = d
                .store
                .lock()
                .unwrap()
                .run(run_id)
                .ok()
                .flatten()
                .is_some_and(|run| run.parent_run_id.is_none());
            if !is_root {
                continue;
            }
            let Some(key) = classify(&event, &mut attention) else {
                continue;
            };
            // Swarms can emit many identical transitions together. One cue conveys
            // the batch without playing a noisy series of overlapping sounds.
            if last.is_some_and(|(previous, at)| {
                previous == key && at.elapsed() < Duration::from_millis(800)
            }) {
                continue;
            }
            last = Some((key, Instant::now()));
            enqueue(key, false);
        }
    });
    Ok(())
}

fn classify(event: &Event, attention: &mut HashSet<String>) -> Option<&'static str> {
    let run_id = event.run_id.as_ref()?;
    if event.kind == "turn_started" && event.payload["turn"]["n"] == 1 {
        return Some("agent_started");
    }
    if event.kind != "status" {
        return None;
    }
    match event.payload["status"].as_str()? {
        "waiting_for_user" | "failed" | "disconnected" if attention.insert(run_id.clone()) => {
            Some("agent_needs_attention")
        }
        "completed" => {
            attention.remove(run_id);
            Some("agent_complete")
        }
        "running" | "starting" | "queued" | "interrupted" => {
            attention.remove(run_id);
            None
        }
        _ => None,
    }
}

fn bytes(key: &str) -> &'static [u8] {
    match key {
        "agent_queued" => include_bytes!("../assets/reactor/agent_queued.mp3"),
        "agent_started" => include_bytes!("../assets/reactor/agent_started.mp3"),
        "agent_resumed" => include_bytes!("../assets/reactor/agent_resumed.mp3"),
        "agent_progress" => include_bytes!("../assets/reactor/agent_progress.mp3"),
        "agent_complete" => include_bytes!("../assets/reactor/agent_complete.mp3"),
        "agent_stopped" => include_bytes!("../assets/reactor/agent_stopped.mp3"),
        "agent_needs_attention" => include_bytes!("../assets/reactor/agent_needs_attention.mp3"),
        "agent_failed" => include_bytes!("../assets/reactor/agent_failed.mp3"),
        "agent_unblocked" => include_bytes!("../assets/reactor/agent_unblocked.mp3"),
        "review_ready" => include_bytes!("../assets/reactor/review_ready.mp3"),
        "delivery_ready" => include_bytes!("../assets/reactor/delivery_ready.mp3"),
        "verification_passed" => include_bytes!("../assets/reactor/verification_passed.mp3"),
        _ => unreachable!("validated cue key"),
    }
}

fn play(key: &str) -> Result<()> {
    if !available() {
        return Ok(());
    }
    let dir = paths::data_dir().join("audio/reactor-v1");
    paths::ensure_private_dir(&dir)?;
    let path = dir.join(format!("{key}.mp3"));
    if std::fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0) != bytes(key).len() as u64 {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let tmp = dir.join(format!("{key}.tmp"));
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp)?;
        file.write_all(bytes(key))?;
        std::fs::rename(tmp, &path)?;
    }
    let status = std::process::Command::new("/usr/bin/afplay")
        .arg(&path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()?;
    if !status.success() {
        crate::log(&format!("afplay exited with {status}"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn event(kind: &str, status: Value) -> Event {
        Event {
            seq: 1,
            ts: 0,
            task_id: None,
            run_id: Some("root".into()),
            kind: kind.into(),
            source: "test".into(),
            confidence: "exact".into(),
            payload: status,
        }
    }
    #[test]
    fn core_transitions_are_broad_and_attention_is_deduped() {
        let mut attention = HashSet::new();
        assert_eq!(
            classify(
                &event("turn_started", json!({"turn":{"n":1}})),
                &mut attention
            ),
            Some("agent_started")
        );
        assert_eq!(
            classify(
                &event("turn_started", json!({"turn":{"n":2}})),
                &mut attention
            ),
            None
        );
        assert_eq!(
            classify(
                &event("permission", json!({"kind":"permission"})),
                &mut attention
            ),
            None
        );
        assert_eq!(
            classify(
                &event("status", json!({"status":"waiting_for_user"})),
                &mut attention
            ),
            Some("agent_needs_attention")
        );
        assert_eq!(
            classify(
                &event("status", json!({"status":"waiting_for_user"})),
                &mut attention
            ),
            None
        );
        assert_eq!(
            classify(
                &event("status", json!({"status":"running"})),
                &mut attention
            ),
            None
        );
        assert_eq!(
            classify(
                &event("status", json!({"status":"waiting_for_user"})),
                &mut attention
            ),
            Some("agent_needs_attention")
        );
        assert_eq!(
            classify(
                &event("status", json!({"status":"completed"})),
                &mut attention
            ),
            Some("agent_complete")
        );
        assert_eq!(
            classify(&event("status", json!({"status":"failed"})), &mut attention),
            Some("agent_needs_attention")
        );
        assert_eq!(
            classify(&event("status", json!({"status":"failed"})), &mut attention),
            None
        );
        assert_eq!(
            classify(
                &event("status", json!({"status":"disconnected"})),
                &mut attention
            ),
            None
        );
    }
    #[test]
    fn pack_has_twelve_short_original_cues() {
        let manifest: Value = serde_json::from_str(MANIFEST).unwrap();
        assert_eq!(manifest.as_array().unwrap().len(), KEYS.len());
        for cue in manifest.as_array().unwrap() {
            let key = cue["key"].as_str().unwrap();
            assert!(KEYS.contains(&key));
            assert!(cue["duration"].as_f64().unwrap() < 0.5);
            assert!(bytes(key).len() < 4_000);
            assert!(cue["provenance"].as_str().unwrap().contains("original"));
        }
    }
}
