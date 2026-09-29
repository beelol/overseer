//! Overseer's brain (the daemon side of the zero-friction goal): it starts agents on the right
//! harness, model and account (AC-237), checks finished work and offers the next step (AC-238),
//! brings stuck, failed and limited agents back (AC-239), never drops what it was told (AC-248)
//! and leads with what happened while the owner was away (AC-253).
mod common;
use common::*;
use serde_json::{json, Value};
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

fn claude_fixture() -> String {
    repo_root().join("fixtures/fake-harness/claude-fixture.js").display().to_string()
}

fn codex_app_fixture() -> String {
    repo_root().join("fixtures/fake-harness/codex-app-fixture.js").display().to_string()
}

/// The tests that spawn many fixture processes and assert on timing run one at a time.
fn heavy() -> std::sync::MutexGuard<'static, ()> {
    static HEAVY: std::sync::Mutex<()> = std::sync::Mutex::new(());
    HEAVY.lock().unwrap_or_else(|e| e.into_inner())
}

/// A daemon whose Claude is the fixture (Overseer mode unless a task's mode file says otherwise).
fn overseer_daemon(mode_file: &Path, extra: &[(&str, &str)]) -> Daemon {
    std::fs::write(mode_file, "overseer").unwrap();
    let mode = mode_file.display().to_string();
    let fixture = claude_fixture();
    let mut env: Vec<(&str, &str)> = vec![("OVERSEER_CLAUDE_PATH", &fixture), ("CLAUDE_FIXTURE_MODE_FILE", &mode), ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "CLAUDE_FIXTURE_MODE_FILE")];
    for (k, v) in extra {
        env.retain(|(key, _)| key != k);
        env.push((k, v));
    }
    Daemon::start(&env)
}

fn claude_task(d: &Daemon, repo: &Path, mode_file: &Path, mode: &str, title: &str, prompt: &str) -> String {
    std::fs::write(mode_file, mode).unwrap();
    let created = d.call("task.create", json!({"repo": repo, "harness": "claude", "prompt": prompt, "title": title}));
    run_id(&created)
}

fn session(d: &Daemon) -> Value {
    d.call("overseer.session", json!({}))
}

fn wait_overseer_idle(d: &Daemon, secs: u64) -> Value {
    let deadline = Instant::now() + Duration::from_secs(secs);
    loop {
        let s = session(d);
        let status = s["run_status"].as_str().unwrap_or("");
        if !s["run_id"].is_null() && !["queued", "starting", "running", "waiting_for_user"].contains(&status) {
            return s;
        }
        assert!(Instant::now() < deadline, "Overseer's run did not finish: {s}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Waits until the conversation has a message the predicate accepts.
fn wait_message(d: &Daemon, what: &str, secs: u64, pred: impl Fn(&Value) -> bool) -> Value {
    let deadline = Instant::now() + Duration::from_secs(secs);
    loop {
        let all = d.call("overseer.messages", json!({"after": 0, "limit": 500}))["messages"].as_array().cloned().unwrap_or_default();
        if let Some(m) = all.iter().find(|m| pred(m)) {
            return m.clone();
        }
        assert!(Instant::now() < deadline, "no message: {what}\n{}", all.iter().map(|m| format!("[{}] {}", m["source"], m["text"])).collect::<Vec<_>>().join("\n"));
        std::thread::sleep(Duration::from_millis(150));
    }
}

fn replies(d: &Daemon) -> Vec<String> {
    d.call("overseer.messages", json!({"after": 0, "limit": 500}))["messages"].as_array().cloned().unwrap_or_default().iter().filter(|m| m["source"] == "overseer").map(|m| m["text"].as_str().unwrap_or("").to_string()).collect()
}

/// The SQL the tests use to put the daemon's store into a state a client cannot ask for.
fn sql(d: &Daemon, statement: &str) {
    let db = d.home.path().join("overseer.sqlite");
    let out = Command::new("sqlite3").args(["-cmd", ".timeout 5000"]).arg(&db).arg(statement).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
}

fn turns(d: &Daemon, run: &str) -> Vec<Value> {
    d.call("run.turns", json!({"run_id": run})).as_array().cloned().unwrap_or_default()
}

/// What Overseer's own run said and hit, for a failing assertion.
fn overseer_trace(d: &Daemon) -> String {
    let s = session(d);
    let run = s["run_id"].as_str().unwrap_or("").to_string();
    d.events(&run).iter().filter(|e| ["output", "error", "tool", "tool_result", "turn_started", "overseer_tool_call"].contains(&e["kind"].as_str().unwrap_or(""))).map(|e| format!("{} {}", e["kind"], serde_json::to_string(&e["payload"]).unwrap_or_default().chars().take(900).collect::<String>())).collect::<Vec<_>>().join("\n")
}

// ---------------------------------------------------------------------- AC-248

/// AC-248: an owner's message whose turn cannot start is not lost. It is kept, the conversation
/// says so in plain words, and it is sent again, as its own turn, once a turn can start.
#[test]
fn ac248_a_failed_turn_start_is_retried() {
    let r = tmp();
    let mode_file = r.path().join("mode");
    let d = overseer_daemon(&mode_file, &[]);
    d.call("agent.cadence", json!({"cadence": "off", "by": "owner"}));
    d.call("overseer.send", json!({"text": "What is everyone doing?", "surface": "ctl", "harness": "claude"}));
    let s = wait_overseer_idle(&d, 30);
    let run = s["run_id"].as_str().unwrap().to_string();
    assert_eq!(replies(&d).len(), 1);
    // Overseer's scratch folder is taken away: its next turn cannot start.
    sql(&d, &format!("UPDATE workspaces SET removed_ms=1 WHERE id=(SELECT workspace_id FROM runs WHERE id='{run}')"));
    let sent = d.call("overseer.send", json!({"text": "Anything new?", "surface": "ctl"}));
    assert_eq!(sent["queued"], true, "the words are kept: {sent}");
    assert_eq!(session(&d)["pending"], 1);
    let note = wait_message(&d, "the words are kept", 10, |m| m["source"] == "system" && m["text"].as_str().unwrap_or("").contains("Your words are kept"));
    assert!(!note["text"].as_str().unwrap().contains("anyhow"), "{note}");
    // Still failing: tried again, and said once.
    std::thread::sleep(Duration::from_secs(5));
    let said = d.call("overseer.messages", json!({"after": 0, "limit": 500}))["messages"].as_array().unwrap().iter().filter(|m| m["source"] == "system").count();
    assert_eq!(said, 1, "the reason is said once while it keeps failing");
    // The folder is back: the kept message goes out as a turn of its own and is answered.
    sql(&d, &format!("UPDATE workspaces SET removed_ms=NULL WHERE id=(SELECT workspace_id FROM runs WHERE id='{run}')"));
    let deadline = Instant::now() + Duration::from_secs(40);
    while (replies(&d).len() < 2 || session(&d)["pending"] != 0) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(250));
    }
    assert_eq!(session(&d)["pending"], 0, "the kept message went out\n{}", overseer_trace(&d));
    assert_eq!(replies(&d).len(), 2, "and was answered\n{}", overseer_trace(&d));
    assert!(turns(&d, &run).iter().any(|t| t["prompt"].as_str().unwrap_or("").ends_with("Anything new?")));
}
