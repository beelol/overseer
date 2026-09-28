#![allow(dead_code)]
//! The fixture world of the Continuity tests: the network and the machine's memory are JSON
//! files the daemon reads on every check, so a test changes them by rewriting the file.

use super::common::*;
use super::ollama::GIB;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::time::{Duration, Instant};

pub struct World {
    pub dir: tempfile::TempDir,
}

impl World {
    pub fn new() -> World {
        let w = World { dir: tmp() };
        w.net(json!({"system": "connected", "baseline": {"by_name": true, "by_ip": true}, "providers": {"openai": true, "anthropic": true}}));
        w.memory(128.0, 115.2, "normal");
        w
    }
    pub fn file(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }
    /// Rewrites the network the daemon sees.
    pub fn net(&self, v: Value) {
        write_whole(&self.file("net.json"), &v.to_string());
    }
    /// Rewrites the machine's memory, in GiB.
    pub fn memory(&self, total: f64, available: f64, pressure: &str) {
        write_whole(&self.file("memory.json"), &json!({"total": (total * GIB as f64) as u64, "available": (available * GIB as f64) as u64, "pressure": pressure}).to_string());
    }
    pub fn replay(&self, transcript: &str) {
        std::fs::copy(
            repo_root().join("fixtures/continuity").join(transcript),
            self.file("replay.jsonl"),
        )
        .unwrap();
    }
    pub fn start(&self, ollama_url: &str, extra: &[(&str, &str)]) -> Daemon {
        let (net, memory, replay) = (
            self.file("net.json"),
            self.file("memory.json"),
            self.file("replay.jsonl"),
        );
        let codex = repo_root().join("fixtures/fake-harness/replay.js");
        let mut env: Vec<(&str, &str)> = vec![
            ("OVERSEER_TEST_NET", net.to_str().unwrap()),
            ("OVERSEER_TEST_MEMORY", memory.to_str().unwrap()),
            ("OVERSEER_OLLAMA_URL", ollama_url),
            ("OVERSEER_OLLAMA_CANDIDATES", "/nonexistent/ollama"),
            ("OVERSEER_TEST_CONTINUITY_TICK_MS", "60"),
            ("OVERSEER_TEST_PROBE_MS", "60"),
            ("OVERSEER_TEST_PROBE_IDLE_MS", "60"),
            ("OVERSEER_TEST_WATCH_MS", "40"),
            ("OVERSEER_CODEX_PATH", codex.to_str().unwrap()),
            (
                "OVERSEER_HARNESS_ENV_PASSTHROUGH",
                "REPLAY_FILE,REPLAY_DELAY_MS",
            ),
            ("REPLAY_FILE", replay.to_str().unwrap()),
            ("REPLAY_DELAY_MS", "10"),
        ];
        for (k, v) in extra {
            env.retain(|(name, _)| name != k);
            env.push((k, v));
        }
        Daemon::start(&env)
    }
}

/// Written whole and then renamed, so the daemon never reads half a file.
pub fn write_whole(path: &std::path::Path, text: &str) {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, text).unwrap();
    std::fs::rename(&tmp, path).unwrap();
}

pub fn conn(d: &Daemon) -> Value {
    d.call("connection.status", json!({}))["status"].clone()
}

pub fn wait_conn(d: &Daemon, what: &str, pred: impl Fn(&Value) -> bool) -> (Value, Duration) {
    let started = Instant::now();
    loop {
        let s = conn(d);
        if pred(&s) {
            return (s, started.elapsed());
        }
        if started.elapsed() > Duration::from_secs(10) {
            panic!(
                "the connection never became {what}; it is {} ({})",
                s["state"], s["reason"]
            );
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

pub fn is(state: &'static str, reason: &'static str) -> impl Fn(&Value) -> bool {
    move |s| s["state"] == state && s["reason"] == reason
}

pub fn no_ollama() -> String {
    // A port nothing listens on.
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    format!("http://127.0.0.1:{}", l.local_addr().unwrap().port())
}

pub fn gib(v: &Value) -> f64 {
    (v.as_u64().unwrap() as f64 / GIB as f64 * 10.0).round() / 10.0
}

pub fn all_events(d: &Daemon, kind: &str) -> Vec<Value> {
    d.call("events.list", json!({"limit": 5000}))["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["kind"] == kind)
        .cloned()
        .collect()
}
