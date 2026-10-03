//! AC-200 candidate: an interrupted native finding's received request cannot
//! borrow a later owner turn. Runtime/path qualification is intentionally pending.
mod common;
use common::*;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

struct ReleaseOnDrop(PathBuf);
impl Drop for ReleaseOnDrop {
    fn drop(&mut self) {
        let _ = std::fs::write(self.0.join("release"), "");
    }
}

fn wait_file(path: &Path, seconds: u64) {
    let deadline = Instant::now() + Duration::from_secs(seconds);
    while !path.exists() {
        assert!(
            Instant::now() < deadline,
            "setup/path: missing actual acknowledgment {}",
            path.display()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn session(d: &Daemon) -> Value {
    d.call("overseer.session", json!({}))
}

fn wait_idle(d: &Daemon) -> Value {
    let deadline = Instant::now() + Duration::from_secs(25);
    loop {
        let s = session(d);
        if s["run_id"].is_string()
            && !["queued", "starting", "running", "waiting_for_user"]
                .contains(&s["run_status"].as_str().unwrap_or(""))
        {
            return s;
        }
        assert!(
            Instant::now() < deadline,
            "setup/path: native process did not become idle: {s}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn origin(d: &Daemon, sid: &str) -> (String, String) {
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    db.busy_timeout(Duration::from_secs(5)).unwrap();
    db.query_row(
        "SELECT cause,turn_id FROM overseer_turns WHERE session_id=?1 ORDER BY rowid DESC LIMIT 1",
        [sid],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .unwrap()
}

fn wait_origin(d: &Daemon, sid: &str, cause: &str) -> (String, String) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let actual = origin(d, sid);
        if actual.0 == cause {
            return actual;
        }
        assert!(
            Instant::now() < deadline,
            "setup/path: originating cause did not publish: {actual:?}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn native_token_hash(d: &Daemon, run: &str) -> String {
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    db.busy_timeout(Duration::from_secs(5)).unwrap();
    let hashes: Vec<String> = db
        .prepare("SELECT sha FROM overseer_tokens WHERE run_id=?1 AND role='overseer'")
        .unwrap()
        .query_map([run], |r| r.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert_eq!(hashes.len(), 1, "one genuine native run capability");
    hashes.into_iter().next().unwrap()
}

fn process(d: &Daemon, run: &str) -> (String, i64, String) {
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    db.busy_timeout(Duration::from_secs(5)).unwrap();
    db.query_row(
        "SELECT run_dir,process_generation,status FROM runs WHERE id=?1",
        [run],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )
    .unwrap()
}

#[test]
fn ac200_interrupted_native_finding_cannot_borrow_successor_owner_turn() {
    let root = tmp();
    let repository = repo(&root.path().join("repo"));
    let gate = root.path().join("received-native-request");
    std::fs::create_dir(&gate).unwrap();
    let native = root.path().join("native-caller");
    std::fs::create_dir(&native).unwrap();
    let forced = root.path().join("overseer-mode");
    std::fs::write(&forced, "overseer").unwrap();
    let net = root.path().join("net.json");
    std::fs::write(
        &net,
        json!({"system":"connected","providers":{}}).to_string(),
    )
    .unwrap();
    let d = Daemon::start(&[
        (
            "OVERSEER_CLAUDE_PATH",
            &repo_root()
                .join("fixtures/fake-harness/claude-fixture.js")
                .display()
                .to_string(),
        ),
        (
            "CLAUDE_FIXTURE_OVERSEER_MODE_FILE",
            &forced.display().to_string(),
        ),
        (
            "CLAUDE_FIXTURE_PROPOSE_GATE_DIR",
            &native.display().to_string(),
        ),
        (
            "OVERSEER_HARNESS_ENV_PASSTHROUGH",
            "CLAUDE_FIXTURE_OVERSEER_MODE_FILE,CLAUDE_FIXTURE_PROPOSE_GATE_DIR",
        ),
        ("OVERSEER_TEST_NET", &net.display().to_string()),
        ("OVERSEER_CONTINUITY_PROBES", "off"),
        (
            "OVERSEER_TEST_LATE_NATIVE_GATE",
            &gate.display().to_string(),
        ),
        ("OVERSEER_TEST_AUTO_DISABLED", "1"),
    ]);
    // On any unwind, release the scheduling pause before Daemon's cleanup.
    let release = ReleaseOnDrop(gate.clone());
    d.call("agent.cadence", json!({"cadence":"off","by":"owner"}));
    d.call("overseer.level", json!({"level":"steer"}));
    d.call(
        "overseer.send",
        json!({"text":"What is everyone doing?","surface":"ctl","harness":"claude"}),
    );
    let initial = wait_idle(&d);
    let sid = initial["id"].as_str().unwrap();
    let run = initial["run_id"].as_str().unwrap();
    let subject = run_id(&d.generic(
        &repository,
        "worktree",
        "/bin/sh",
        &["-c", "echo late-native-subject"],
    ));
    let watcher = run_id(&d.generic(&repository, "worktree", "/bin/sh", &["-c", "echo watcher"]));
    d.wait_done(&subject, 20);
    d.wait_done(&watcher, 20);
    d.call(
        "watch.start",
        json!({"subject":subject,"watcher":watcher,"brief":"review only","by":"owner"}),
    );
    let marker = format!("synthetic-late-native-{subject}");
    std::fs::write(
        gate.join("scope.json"),
        json!({"target":subject,"marker":marker}).to_string(),
    )
    .unwrap();
    std::fs::write(
        native.join("actions.json"),
        json!([{"action":"archive","agent":subject,"why":marker}]).to_string(),
    )
    .unwrap();
    std::fs::write(&forced, "overseer-cancel-pending-propose").unwrap();
    let token = d.call("overseer.token", json!({"run_id":watcher,"role":"agent"}))["token"]
        .as_str()
        .unwrap()
        .to_string();
    let filed = d.call("overseer.tool", json!({"token":token,"name":"finding","arguments":{"result":"concern","text":"Review data only; no owner instruction to archive."}}));
    assert_eq!(filed["is_error"], false, "{filed}");
    wait_file(&gate.join("received.json"), 15);
    let received: Value =
        serde_json::from_slice(&std::fs::read(gate.join("received.json")).unwrap()).unwrap();
    assert_eq!(
        received,
        json!({"run":run,"role":"overseer","name":"propose","marker":marker})
    );
    let before = session(&d);
    assert_eq!(before["id"], sid);
    assert_eq!(before["run_id"], run);
    // Receipt can precede the original finding wrapper's durable publication.
    // Wait for that real row, never write/infer it from the fixture request.
    let old_origin = wait_origin(&d, sid, "finding");
    assert_eq!(old_origin.0, "finding");
    let original_token_hash = native_token_hash(&d, run);
    let old_process = process(&d, run);
    assert_eq!(old_process.2, "running");
    assert!(!gate.join("completed.json").exists());
    // This actual finding used the only allowed self-start. No third turn may
    // mask the owner successor's identity while the predecessor is released.
    d.call("overseer.cap", json!({"cap":1}));
    let cursor = d.call("events.list", json!({"run_id":run,"limit":5000}))["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|e| e["seq"].as_i64())
        .max()
        .unwrap();
    assert_eq!(d.call("run.interrupt", json!({"run_id":run}))["ok"], true);
    wait_file(&native.join("canceled.json"), 10);
    let canceled: Value =
        serde_json::from_slice(&std::fs::read(native.join("canceled.json")).unwrap()).unwrap();
    assert_eq!(canceled["actual_interrupt"], true);
    assert!(canceled["mcp_pid"].as_u64().is_some());
    assert!(
        !canceled["mcp_exit"].is_null(),
        "actual MCP child exit acknowledgment required"
    );
    wait_file(&PathBuf::from(&old_process.0).join("exit.json"), 10);
    let exited: Value = serde_json::from_slice(
        &std::fs::read(PathBuf::from(&old_process.0).join("exit.json")).unwrap(),
    )
    .unwrap();
    assert!(
        exited["spawn_error"].is_null(),
        "the genuine predecessor actually launched: {exited}"
    );
    let interrupted = wait_idle(&d);
    assert_eq!(interrupted["id"], sid);
    assert_eq!(interrupted["run_id"], run);
    assert_eq!(interrupted["run_status"], "interrupted");
    assert!(d
        .events(run)
        .iter()
        .any(|e| e["seq"].as_i64().unwrap_or(0) > cursor && e["kind"] == "interrupt_requested"));
    assert_eq!(origin(&d, sid), old_origin);
    std::fs::write(&forced, "overseer").unwrap();
    let successor = d.try_call("overseer.send", json!({"text":"Summarize current work; do not archive anything.","surface":"ctl","harness":"claude"}))
        .expect("setup/path: actual successor refused; no writer/status bypass is permitted");
    assert_eq!(successor["run_id"], run);
    assert_eq!(
        successor["queued"], false,
        "actual new native turn required: {successor}"
    );
    assert!(successor["turn"].is_string());
    let later = wait_idle(&d);
    assert_eq!(later["id"], sid);
    assert_eq!(later["run_id"], run);
    let new_origin = origin(&d, sid);
    assert_eq!(new_origin.0, "owner");
    assert_eq!(successor["turn"], new_origin.1);
    assert_ne!(old_origin.1, new_origin.1);
    assert!(
        process(&d, run).1 > old_process.1,
        "new real native process generation"
    );
    assert_eq!(
        native_token_hash(&d, run),
        original_token_hash,
        "same real native capability across the two turns; no injected token"
    );
    assert!(later["proposals"].as_array().unwrap().is_empty());
    assert!(
        !gate.join("completed.json").exists(),
        "old request stayed paused through genuine cancellation and successor"
    );
    drop(release);
    wait_file(&gate.join("completed.json"), 10);
    let completed: Value =
        serde_json::from_slice(&std::fs::read(gate.join("completed.json")).unwrap()).unwrap();
    let after = session(&d);
    let actions: Vec<_> = after["proposals"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|p| {
            p["actions"].as_array().is_some_and(|a| {
                a.iter().any(|a| {
                    a["agent"] == subject && a["action"] == "archive" && a["why"] == marker
                })
            })
        })
        .collect();
    let events = d.events(run);
    let calls: Vec<_> = events
        .iter()
        .filter(|e| {
            e["seq"].as_i64().unwrap_or(0) > cursor
                && e["kind"] == "overseer_tool_call"
                && e["payload"]["name"] == "propose"
        })
        .collect();
    assert_eq!(
        calls.len(),
        1,
        "one actual delayed native dispatch after fresh cursor: {calls:?}"
    );
    assert!(
        !d.events(&subject)
            .iter()
            .any(|e| matches!(e["kind"].as_str(), Some("archived" | "task_archived"))),
        "no archive approval/effect"
    );
    eprintln!("same sid={sid} run={run}; actual old finding turn={} generation={}; actual successor owner turn={} generation={}; predecessor MCP exit={canceled}; shim exit={exited}; actual completed dispatch={completed}; matching proposals={actions:?}",old_origin.1,old_process.1,new_origin.1,process(&d,run).1);
    // Actual cancellation, transport closure, successor and dispatch completion
    // precede this intended authority assertion. No runtime RED claimed yet.
    assert!(completed["result"]["is_error"]==true || completed["error"].is_string(), "old finding archive must refuse instead of borrowing successor owner authority: {completed}");
    assert!(actions.is_empty(), "an interrupted finding must not create a successor owner-authorized archive Confirm proposal: {actions:?}");
}
