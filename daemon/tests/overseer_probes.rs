//! Gate S's long waits and its OpenCode probe, kept out of the everyday suite (`#[ignore]`):
//! run with `cargo test -p overseerd --test overseer_probes -- --ignored`.
//!
//! - AC-193: an idle subject causes no wake in ten minutes (literally waited).
//! - AC-198: an hour of nine idle agents causes no Overseer turn (literally waited).
//! - AC-187: what OpenCode refuses: a guardrail on an OpenCode agent reads *watched*, and the real
//!   OpenCode (1.15.x, found on PATH or in `OVERSEER_OPENCODE_PROBE`) writes the denied path when
//!   told to; the daemon reports it within 2 s and holds the agent. The model is the repository's
//!   deterministic mock (fixtures/mock-openai/server.js): no account and no paid turn.
//!
//! Everything else is the Claude fixture and generic programs.

mod common;

use common::*;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

fn claude_fixture() -> String {
    repo_root().join("fixtures/fake-harness/claude-fixture.js").display().to_string()
}

fn overseer_daemon(mode_file: &Path) -> Daemon {
    std::fs::write(mode_file, "overseer").unwrap();
    Daemon::start(&[("OVERSEER_CLAUDE_PATH", &claude_fixture()), ("CLAUDE_FIXTURE_MODE_FILE", &mode_file.display().to_string()), ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "CLAUDE_FIXTURE_MODE_FILE")])
}

fn claude_task(d: &Daemon, repo: &Path, mode_file: &Path, mode: &str, title: &str, prompt: &str) -> String {
    std::fs::write(mode_file, mode).unwrap();
    run_id(&d.call("task.create", json!({"repo": repo, "harness": "claude", "prompt": prompt, "title": title})))
}

fn overseer_turns(d: &Daemon) -> usize {
    let s = d.call("overseer.session", json!({}));
    s["run_id"].as_str().map(|r| d.call("run.turns", json!({"run_id": r})).as_array().unwrap().len()).unwrap_or(0)
}

fn wait_overseer_idle(d: &Daemon) {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let s = d.call("overseer.session", json!({}));
        if !s["run_id"].is_null() && !["queued", "starting", "running"].contains(&s["run_status"].as_str().unwrap_or("")) {
            return;
        }
        assert!(Instant::now() < deadline, "Overseer stayed busy: {s}");
        std::thread::sleep(Duration::from_millis(200));
    }
}

fn minutes(var: &str, default: u64) -> Duration {
    Duration::from_secs(60 * std::env::var(var).ok().and_then(|v| v.parse().ok()).unwrap_or(default))
}

/// AC-193: a watched subject that is working but says nothing causes no wake in ten minutes;
/// no watcher is started and the watch stays open.
#[test]
#[ignore]
fn ac193_an_idle_subject_causes_no_wake_in_ten_minutes() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let mode_file = r.path().join("mode");
    let d = overseer_daemon(&mode_file);
    d.call("overseer.session", json!({}));
    d.call("agent.cadence", json!({"cadence": "off", "by": "owner"}));
    let subject = run_id(&d.generic(&repo, "worktree", "/bin/sh", &["-c", "echo started; sleep 3600"]));
    d.wait_status(&subject, |s| s == "running", 20);
    std::thread::sleep(Duration::from_secs(2));
    let watch = d.call("watch.start", json!({"subject": subject, "brief": "anything at all", "harness": "claude", "by": "owner"}));
    let started = Instant::now();
    let wait = minutes("PROBE_IDLE_MINUTES", 10);
    while started.elapsed() < wait {
        std::thread::sleep(Duration::from_secs(30));
        let w = d.call("watch.list", json!({}))["watches"][0].clone();
        assert_eq!((w["wakes"].as_i64(), w["watcher"].as_str().unwrap_or(""), w["open"].as_bool()), (Some(0), "", Some(true)), "after {:?}: {w}", started.elapsed());
    }
    assert_eq!(d.run(&subject)["status"], "running");
    assert!(!d.events(&subject).iter().any(|e| e["kind"] == "watch_wake"));
    assert_eq!(d.call("state", json!({}))["runs"].as_array().unwrap().len(), 1, "no watcher run was started");
    println!("PROBE ac193: watch {} on a working, silent subject: 0 wakes in {:?}", watch["id"], started.elapsed());
}

/// AC-198: an hour of nine idle fixture agents, with check-ins on, causes no Overseer turn.
#[test]
#[ignore]
fn ac198_an_hour_of_nine_idle_agents_causes_no_turn() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let mode_file = r.path().join("mode");
    let d = overseer_daemon(&mode_file);
    d.call("overseer.session", json!({}));
    d.call("agent.cadence", json!({"cadence": "off", "by": "owner"}));
    d.call("overseer.send", json!({"text": "What is everyone doing?", "surface": "ctl", "harness": "claude"}));
    wait_overseer_idle(&d);
    let mut agents = Vec::new();
    for i in 0..9 {
        let id = claude_task(&d, &repo, &mode_file, "echo", &format!("Idle {i}"), "hello");
        d.wait_done(&id, 30);
        agents.push(id);
    }
    std::fs::write(&mode_file, "overseer").unwrap();
    d.call("agent.cadence", json!({"cadence": "every:3", "by": "owner"}));
    std::thread::sleep(Duration::from_secs(10));
    let before = overseer_turns(&d);
    let started = Instant::now();
    let wait = minutes("PROBE_HOUR_MINUTES", 60);
    while started.elapsed() < wait {
        std::thread::sleep(Duration::from_secs(60));
        assert_eq!(overseer_turns(&d), before, "a turn after {:?}", started.elapsed());
    }
    for a in &agents {
        assert_eq!(d.run(a)["status"], "completed");
    }
    assert_eq!(d.call("overseer.cap", json!({}))["self_started_today"].as_i64(), Some(0));
    println!("PROBE ac198: nine idle agents, check-ins every third turn: {} Overseer turns in {:?} (the owner's one before)", overseer_turns(&d) - before, started.elapsed());
}

fn opencode() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("OVERSEER_OPENCODE_PROBE") {
        return Some(PathBuf::from(p));
    }
    let home = std::env::var("HOME").ok()?;
    [format!("{home}/.opencode/bin/opencode"), "/opt/homebrew/bin/opencode".into(), "/usr/local/bin/opencode".into()].into_iter().map(PathBuf::from).find(|p| p.exists())
}

/// AC-187: OpenCode has no per-path refusal, so its guardrail reads watched; told to write inside
/// the forbidden path it does, and the daemon reports the write within 2 s and holds the agent.
#[test]
#[ignore]
fn ac187_opencode_probe_its_guardrail_is_watched() {
    let Some(bin) = opencode() else {
        panic!("OpenCode is not installed; set OVERSEER_OPENCODE_PROBE");
    };
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    // The deterministic mock model.
    let port_file = r.path().join("mock.port");
    let mut mock = std::process::Command::new("node")
        .arg(repo_root().join("fixtures/mock-openai/server.js"))
        .env("MOCK_PORT", "0")
        .env("MOCK_PORT_FILE", &port_file)
        .env("MOCK_LOG", r.path().join("mock.log"))
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !port_file.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    let port = std::fs::read_to_string(&port_file).unwrap().trim().to_string();
    let d = Daemon::start(&[("OVERSEER_OPENCODE_PATH", bin.to_str().unwrap())]);
    let profile = d.call("profile.create", json!({"name": "OpenCode mock", "harness": "opencode"}));
    let config = PathBuf::from(profile["home"].as_str().unwrap()).join("config/opencode");
    std::fs::create_dir_all(&config).unwrap();
    let body = json!({"$schema": "https://opencode.ai/config.json", "provider": {"mock": {"npm": "@ai-sdk/openai-compatible", "name": "Mock (deterministic fixture)", "options": {"baseURL": format!("http://127.0.0.1:{port}/v1")}, "models": {"mock-coder": {"name": "Mock Coder", "tool_call": true}}}}, "model": "mock/mock-coder", "small_model": "mock/mock-coder", "autoupdate": false, "share": "disabled"});
    std::fs::write(config.join("opencode.json"), serde_json::to_vec_pretty(&body).unwrap()).unwrap();
    let created = d.call("task.create", json!({"repo": repo, "harness": "opencode", "profile_id": profile["id"], "model": "mock/mock-coder", "prompt": "say hello", "title": "OpenCode probe"}));
    assert_eq!(created["launch_error"], Value::Null, "{created}");
    let run = run_id(&created);
    let first = d.wait_done(&run, 90);
    assert_eq!(first["status"], "completed", "{first}");
    let g = d.call("agent.guardrail", json!({"run_id": run, "words": "Do not touch src.", "deny": ["src"], "hold_on_cross": true, "by": "owner"}));
    assert_eq!(g["enforcement"], "watched", "OpenCode has no per-path refusal: {g}");
    d.call("run.follow_up", json!({"run_id": run, "prompt": "write src/probe.txt"}));
    let ws = ws_path(&d, &created);
    let deadline = Instant::now() + Duration::from_secs(90);
    while !ws.join("src/probe.txt").exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    let written = Instant::now();
    assert!(ws.join("src/probe.txt").exists(), "OpenCode did not write; events: {:?}", d.events(&run).iter().map(|e| e["kind"].clone()).collect::<Vec<_>>());
    let deadline = Instant::now() + Duration::from_secs(10);
    let crossed = loop {
        if let Some(e) = d.events(&run).into_iter().find(|e| e["kind"] == "guardrail_crossed") {
            break e;
        }
        assert!(Instant::now() < deadline, "no guardrail_crossed");
        std::thread::sleep(Duration::from_millis(50));
    };
    let took = written.elapsed();
    assert!(took < Duration::from_secs(2) + Duration::from_millis(500), "reported {took:?} after the write was seen");
    assert!(d.call("agent.holds", json!({}))["holds"].as_array().unwrap().iter().any(|h| h["run_id"] == run.as_str()), "held");
    println!("PROBE ac187: OpenCode {} wrote src/probe.txt (not refused); label watched; guardrail_crossed {} ; reported within {:?} of the file appearing", String::from_utf8_lossy(&std::process::Command::new(&bin).arg("--version").output().unwrap().stdout).trim(), crossed["payload"], took);
    let _ = mock.kill();
}
