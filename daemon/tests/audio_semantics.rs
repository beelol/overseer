//! Canonical Audio Mode meanings at actual daemon boundaries (AC275–286).
//! These tests intentionally use the incumbent event-to-player test sink before
//! the shared manifest resolver lands. A key capture is not spoken-content or
//! complete pack qualification. No provider process or private audio is used.
mod common;
use common::*;
use serde_json::{json, Value};
use std::path::Path;
use std::time::{Duration, Instant};

fn keys(path: &Path) -> Vec<String> {
    std::fs::read_to_string(path).unwrap_or_default().lines()
        .map(|line| line.rsplit_once(':').expect("pack:key capture").1.to_owned()).collect()
}

fn wait_captures(path: &Path, count: usize) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while keys(path).len() < count {
        assert!(Instant::now() < deadline, "missing {count} captures: {:?}", keys(path));
        std::thread::sleep(Duration::from_millis(20));
    }
}

// Observe past the specified 800ms coalescing window, including its final
// delivery. This is a negative-observation window, not a readiness substitute.
fn settled_keys(path: &Path) -> Vec<String> {
    std::thread::sleep(Duration::from_millis(1100));
    keys(path)
}

fn enable(d: &Daemon) {
    assert_eq!(d.call("audio.set", json!({"enabled":true}))["enabled"], true);
}

#[test]
fn terminal_task_failure_has_its_specific_line_without_generic_attention() {
    let temp = tmp();
    let checkout = repo(&temp.path().join("repo"));
    let log = temp.path().join("audio.log");
    let d = Daemon::start(&[("OVERSEER_TEST_AUDIO_LOG", log.to_str().unwrap()),
        ("OVERSEER_TEST_AUTO_DISABLED", "1")]);
    enable(&d);
    let run = run_id(&d.generic(&checkout, "worktree", "/bin/sh", &["-c", "exit 3"]));
    assert_eq!(d.wait_done(&run, 10)["status"], "failed");
    wait_captures(&log, 2);
    assert_eq!(settled_keys(&log), ["agent_started", "agent_failed"]);
}

#[test]
fn ordinary_success_has_one_start_and_one_logical_completion() {
    let temp = tmp();
    let checkout = repo(&temp.path().join("repo"));
    let log = temp.path().join("audio.log");
    let d = Daemon::start(&[("OVERSEER_TEST_AUDIO_LOG", log.to_str().unwrap()),
        ("OVERSEER_TEST_AUTO_DISABLED", "1")]);
    enable(&d);
    let run = run_id(&d.generic(&checkout, "worktree", "/bin/sh", &["-c", "exit 0"]));
    assert_eq!(d.wait_done(&run, 10)["status"], "completed");
    wait_captures(&log, 2);
    assert_eq!(settled_keys(&log), ["agent_started", "agent_complete"]);
}

#[test]
fn legacy_permission_and_waiting_projection_announce_the_same_specific_need_once() {
    let temp = tmp();
    let checkout = repo(&temp.path().join("repo"));
    let log = temp.path().join("audio.log");
    let fixture = repo_root().join("fixtures/fake-harness/claude-fixture.js");
    let d = Daemon::start(&[("OVERSEER_TEST_AUDIO_LOG", log.to_str().unwrap()),
        ("OVERSEER_TEST_AUTO_DISABLED", "1"), ("OVERSEER_CLAUDE_PATH", fixture.to_str().unwrap()),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE"), ("FIXTURE_MODE", "permission")]);
    enable(&d);
    let run = run_id(&d.call("task.create", json!({"repo":checkout,"harness":"claude",
        "prompt":"write perm.txt","title":"one permission"})));
    let waiting = d.wait_status(&run, |s| s == "waiting_for_user", 15);
    assert_eq!(waiting["attention"]["request_id"], "req-1");
    wait_captures(&log, 2);
    assert_eq!(settled_keys(&log), ["agent_started", "agent_permission_required"]);
    d.call("run.permission", json!({"run_id":run,"request_id":"req-1","allow":true}));
    assert_eq!(d.wait_done(&run, 15)["status"], "completed");
    wait_captures(&log, 3);
    assert_eq!(settled_keys(&log), ["agent_started", "agent_permission_required", "agent_complete"]);
}

#[test]
fn expired_auth_without_a_permitted_fallback_announces_sign_in_not_failure() {
    let temp = tmp();
    let checkout = repo(&temp.path().join("repo"));
    let log = temp.path().join("audio.log");
    let fixture = repo_root().join("fixtures/fake-harness/claude-fixture.js");
    let d = Daemon::start(&[("OVERSEER_TEST_AUDIO_LOG", log.to_str().unwrap()),
        ("OVERSEER_TEST_AUTO_DISABLED", "1"), ("OVERSEER_CLAUDE_PATH", fixture.to_str().unwrap()),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE"), ("FIXTURE_MODE", "auth")]);
    // No automatic route is authorized; this is not a retry/fallback fixture.
    d.call("settings.set", json!({"values":{"enabled":false}}));
    enable(&d);
    let run = run_id(&d.call("task.create", json!({"repo":checkout,"harness":"claude",
        "prompt":"try","title":"expired synthetic authentication"})));
    assert_eq!(d.wait_done(&run, 15)["status"], "failed");
    wait_captures(&log, 2);
    assert_eq!(settled_keys(&log), ["agent_started", "agent_sign_in_required"]);
}

#[test]
fn unrecoverable_live_supervisor_loss_is_unexpected_stop_not_generic_attention() {
    let temp = tmp();
    let checkout = repo(&temp.path().join("repo"));
    let log = temp.path().join("audio.log");
    let d = Daemon::start(&[("OVERSEER_TEST_AUDIO_LOG", log.to_str().unwrap()),
        ("OVERSEER_TEST_AUTO_DISABLED", "1")]);
    d.call("settings.set", json!({"values":{"enabled":false}}));
    enable(&d);
    let run = run_id(&d.generic(&checkout, "worktree", "/bin/sh", &["-c", "sleep 30"]));
    d.wait_status(&run, |s| s == "running", 10);
    wait_captures(&log, 1);
    let (shim, _) = launch_info(&d, &run);
    signal(shim["shim_pid"].as_i64().unwrap(), 9);
    assert_eq!(d.wait_done(&run, 15)["status"], "disconnected");
    // Terminate the owned orphan before an assertion can panic. Daemon's Drop
    // also performs owned-process cleanup on all paths.
    signal(shim["child_pid"].as_i64().unwrap(), 9);
    wait_captures(&log, 2);
    assert_eq!(settled_keys(&log), ["agent_started", "agent_stopped_unexpectedly"]);
}

#[test]
fn owner_stop_is_silent_after_the_actual_start() {
    let temp = tmp();
    let checkout = repo(&temp.path().join("repo"));
    let log = temp.path().join("audio.log");
    let d = Daemon::start(&[("OVERSEER_TEST_AUDIO_LOG", log.to_str().unwrap()),
        ("OVERSEER_TEST_AUTO_DISABLED", "1")]);
    enable(&d);
    let run = run_id(&d.generic(&checkout, "worktree", "/bin/sh", &["-c", "sleep 30"]));
    d.wait_status(&run, |s| s == "running", 10);
    wait_captures(&log, 1);
    d.call("run.interrupt", json!({"run_id":run}));
    assert_eq!(d.wait_done(&run, 15)["status"], "interrupted");
    assert_eq!(settled_keys(&log), ["agent_started"]);
}

#[test]
fn two_distinct_live_permission_needs_use_plural_instead_of_individual_lines() {
    let temp = tmp();
    let checkout = repo(&temp.path().join("repo"));
    let log = temp.path().join("audio.log");
    let gate = temp.path().join("permission-gate");
    let fixture = repo_root().join("fixtures/fake-harness/claude-fixture.js");
    let d = Daemon::start(&[("OVERSEER_TEST_AUDIO_LOG", log.to_str().unwrap()),
        ("OVERSEER_TEST_AUTO_DISABLED", "1"), ("OVERSEER_CLAUDE_PATH", fixture.to_str().unwrap()),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE,FIXTURE_PERMISSION_BARRIER"),
        ("FIXTURE_MODE", "permission"), ("FIXTURE_PERMISSION_BARRIER", gate.to_str().unwrap())]);
    enable(&d);
    let one = run_id(&d.call("task.create", json!({"repo":checkout,"harness":"claude","prompt":"one","title":"one"})));
    let two = run_id(&d.call("task.create", json!({"repo":checkout,"harness":"claude","prompt":"two","title":"two"})));
    for run in [&one, &two] {
        d.wait_status(run, |s| s == "running", 10);
        assert!(d.run(run)["attention"].is_null());
    }
    std::fs::write(&gate, b"release both actual native requests").unwrap();
    for run in [&one, &two] { d.wait_status(run, |s| s == "waiting_for_user", 15); }
    assert_eq!(d.runs().iter().filter(|r| r["parent_run_id"].is_null()
        && r["status"] == "waiting_for_user").count(), 2);
    let deadline = Instant::now() + Duration::from_secs(5);
    while !keys(&log).iter().any(|key| key != "agent_started") {
        assert!(Instant::now() < deadline, "no need capture: {:?}", keys(&log));
        std::thread::sleep(Duration::from_millis(20));
    }
    let needs: Vec<_> = settled_keys(&log).into_iter().filter(|key| key != "agent_started").collect();
    assert_eq!(needs, ["agents_need_attention"]);
}

struct SwarmWorld {
    d: Daemon,
    checkout: std::path::PathBuf,
    trace: std::path::PathBuf,
    gate: std::path::PathBuf,
    log: std::path::PathBuf,
    _temp: tempfile::TempDir,
}

fn swarm_world() -> SwarmWorld {
    let temp = tmp();
    let checkout = repo(&temp.path().join("atlas"));
    let trace = temp.path().join("director-trace.jsonl");
    let gate = temp.path().join("gate");
    let log = temp.path().join("audio.log");
    let fixture = repo_root().join("fixtures/swarm/s0-start-v1/director.py");
    let config = json!({"program":"/usr/bin/python3","args":[fixture,trace,gate]}).to_string();
    let d = Daemon::start(&[("OVERSEER_SWARM_FIXTURE_DIRECTOR", &config),
        ("OVERSEER_TEST_AUDIO_LOG", log.to_str().unwrap()), ("OVERSEER_TEST_AUTO_DISABLED", "1")]);
    enable(&d);
    d.call("agents.limit.set", json!({"max_active":5}));
    d.call("swarm.policy.set", json!({"scope":"application","allowed_targets":["fixture-local","system-claude"]}));
    SwarmWorld { d, checkout, trace, gate, log, _temp:temp }
}

fn start_swarm(w: &SwarmWorld) -> (String, String) {
    let params = json!({"category":"Backend security",
        "objective":"Audit Atlas tenant isolation; report bugs, don't change application code",
        "repositories":[w.checkout]});
    let readback = w.d.call("swarm.start", params.clone());
    let mut confirm = params;
    confirm["request_id"] = json!("audio-s0-start");
    confirm["confirm_readback_sha256"] = readback["readback_sha256"].clone();
    let started = w.d.call("swarm.start", confirm);
    assert_eq!(started["status"], "started", "{started}");
    assert_eq!(started["director"]["status"], "launched", "{started}");
    (started["run"]["id"].as_str().unwrap().to_owned(),
        started["director"]["overseer_run_id"].as_str().unwrap().to_owned())
}

fn wait_dispatched(w: &SwarmWorld) {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let records: Vec<Value> = std::fs::read_to_string(&w.trace).unwrap_or_default().lines()
            .map(|line| serde_json::from_str(line).unwrap()).collect();
        if records.iter().any(|v| v["step"] == "dispatched") { return; }
        assert!(Instant::now() < deadline, "director not ready: {records:?}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn accepted_live_swarm_director_and_workers_have_one_swarm_start() {
    let w = swarm_world();
    let (swarm, director) = start_swarm(&w);
    wait_dispatched(&w);
    assert_eq!(w.d.run(&director)["status"], "running");
    assert_eq!(w.d.call("swarm.get", json!({"id":swarm}))["status"], "running");
    wait_captures(&w.log, 1);
    assert_eq!(settled_keys(&w.log), ["swarm_initiated"]);
}

#[test]
fn whole_swarm_completion_replaces_director_and_worker_completions() {
    let w = swarm_world();
    let (swarm, director) = start_swarm(&w);
    wait_dispatched(&w);
    std::fs::write(&w.gate, b"finish audited fixture objective").unwrap();
    assert_eq!(w.d.wait_done(&director, 60)["status"], "completed");
    assert_eq!(w.d.call("swarm.get", json!({"id":swarm}))["status"], "completed");
    wait_captures(&w.log, 2);
    assert_eq!(settled_keys(&w.log), ["swarm_initiated", "swarm_complete"]);
}
