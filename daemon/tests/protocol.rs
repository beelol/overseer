//! Protocol-level regression tests against the real daemon binary and real Git.
//! Test names carry the acceptance criterion they support. Fixture/synthetic harnesses
//! are labeled; they never substitute for live account or native-child evidence.

mod common;
use common::*;
use serde_json::json;
use std::path::Path;
use std::time::Duration;

fn sh(d: &Daemon, repo: &Path, mode: &str, script: &str) -> serde_json::Value {
    d.generic(repo, mode, "/bin/sh", &["-c", script])
}

fn fixture(name: &str) -> String {
    repo_root().join("fixtures").join(name).display().to_string()
}

#[test]
fn auto_disabled_preserves_manual_create_follow_up_and_interrupt() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let d = Daemon::start(&[("OVERSEER_TEST_AUTO_DISABLED", "1")]);
    assert_eq!(d.call("auto.mode.get", json!({}))["enabled"], false);
    let created = d.generic(&repo, "worktree", "/bin/sh", &["-c", "cat >> input.txt"]);
    let run = run_id(&created);
    d.wait_status(&run, |status| status == "running", 10);
    d.call("run.follow_up", json!({"run_id":run,"prompt":"manual continuation"}));
    assert_eq!(d.call("run.turns", json!({"run_id":run})).as_array().unwrap().len(), 2);
    d.call("run.interrupt", json!({"run_id":run}));
    assert_eq!(d.wait_done(&run, 15)["status"], "interrupted");
    assert_eq!(std::fs::read_to_string(ws_path(&d, &created).join("input.txt")).unwrap(),
        "manual continuation\n");
    assert_eq!(d.runs().len(), 1, "manual routing must not create an Auto child");
    assert!(!d.events(&run).iter().any(|event| event["kind"] == "auto_decision"));
}

#[test]
fn auto_mode_defaults_off_and_only_explicit_enable_allows_new_dispatch() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let mut d = Daemon::start(&[("OVERSEER_TEST_AUTO_DISABLED", "1"),
        ("OVERSEER_CODEX_PATH", &fixture("fake-harness/codex-app-fixture.js")),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE"),
        ("FIXTURE_MODE", "managed-models")]);
    let parent = run_id(&d.call("task.create", json!({"repo":repo,"harness":"codex-app",
        "model":"gpt-6-astra","effort":"high","prompt":"manual parent"})));
    assert_eq!(d.wait_done(&parent, 15)["status"], "completed");
    let request = json!({"work_unit_id":"disabled-auto-1","parent_run_id":parent,
        "min_tier":"general","required_tools":[],"prompt":"bounded browser check"});
    assert!(d.try_call("auto.dispatch", request.clone()).is_err(),
        "the default-off daemon must refuse new Auto work");
    assert_eq!(d.runs().len(), 1);
    assert!(!d.events(&parent).iter().any(|event| event["kind"] == "auto_decision"));
    assert_eq!(d.call("auto.mode.get", json!({}))["enabled"], false);

    assert_eq!(d.call("auto.mode.set", json!({"enabled":true}))["enabled"], true);
    assert!(d.try_call("auto.mode.set", json!({"enabled":"yes"})).is_err());
    let selected = d.call("auto.dispatch", request.clone());
    assert_eq!(selected["state"], "dispatched", "{selected}");
    let child = run_id(&selected);
    assert_eq!(d.call("auto.mode.set", json!({"enabled":false}))["enabled"], false);
    assert!(d.try_call("auto.dispatch", json!({"work_unit_id":"disabled-auto-2",
        "parent_run_id":parent,"min_tier":"general","required_tools":[],
        "prompt":"another child"})).is_err());
    assert_eq!(d.wait_done(&child, 15)["status"], "completed",
        "turning Auto off must not interrupt an existing child");
    let replay = d.call("auto.dispatch", request);
    assert_eq!(run_id(&replay), child);
    assert_eq!(replay["replayed"], true);
    d.kill9();
    d.spawn();
    assert_eq!(d.call("auto.mode.get", json!({}))["enabled"], false,
        "the off state must survive daemon restart");
    assert_eq!(d.runs().len(), 2);
}


#[test]
fn disabling_auto_during_collection_prevents_new_admission() {
    use std::time::Instant;
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let trace = r.path().join("disable-during-collection-trace.txt");
    let d = Daemon::start(&[("OVERSEER_CODEX_PATH", &fixture("fake-harness/codex-app-fixture.js")),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE,FIXTURE_MODEL_DELAY_MS,FIXTURE_TRACE_FILE"),
        ("FIXTURE_MODE", "managed-models"), ("FIXTURE_MODEL_DELAY_MS", "1500"),
        ("FIXTURE_TRACE_FILE", trace.to_str().unwrap())]);
    let parent = run_id(&d.call("task.create", json!({"repo":repo,"harness":"codex-app",
        "model":"gpt-6-astra","effort":"high","prompt":"manual parent"})));
    assert_eq!(d.wait_done(&parent, 15)["status"], "completed");
    std::fs::write(&trace, "").unwrap();
    let request = json!({"work_unit_id":"disable-during-collect-1","parent_run_id":parent,
        "min_tier":"general","required_tools":[],"prompt":"bounded browser check"});
    let outcome = std::thread::scope(|scope| {
        let pending = scope.spawn(|| d.try_call("auto.dispatch", request));
        let deadline = Instant::now() + Duration::from_secs(5);
        while !std::fs::read_to_string(&trace).unwrap_or_default().contains("model_read") {
            assert!(Instant::now() < deadline, "Auto metadata collection did not begin");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(d.call("auto.mode.set", json!({"enabled":false}))["enabled"], false);
        pending.join().unwrap()
    });
    assert!(outcome.is_err(), "new work was admitted after Auto was disabled: {outcome:?}");
    assert_eq!(d.runs().len(), 1, "no child may launch after the disable");
    assert!(!d.events(&parent).iter().any(|event| event["kind"] == "auto_decision"),
        "disabled work must not persist an admission decision");
}

// ---------------------------------------------------------------- AC-05 / AC-07

#[test]
fn ac05_ac07_state_survives_daemon_crash_and_reattaches() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let mut d = Daemon::start(&[]);
    let created = sh(&d, &repo, "worktree", "echo started; sleep 4; echo x > out.txt; echo finished");
    let run = run_id(&created);
    d.wait_status(&run, |s| s == "running", 10);
    std::thread::sleep(Duration::from_millis(500));
    let before = d.call("state", json!({}));
    let events_before = d.events(&run).len();
    d.kill9(); // forced crash while the harness keeps working
    std::thread::sleep(Duration::from_millis(300));
    d.spawn();
    let after = d.call("state", json!({}));
    assert_eq!(before["tasks"], after["tasks"], "tasks identical after restart");
    assert_eq!(after["runs"].as_array().unwrap().len(), 1, "no duplicate runs");
    let reattached = d.events(&run).iter().any(|e| e["kind"] == "reattached");
    assert!(reattached, "daemon reported reattachment");
    let done = d.wait_done(&run, 20);
    assert_eq!(done["status"], "completed");
    assert_eq!(done["process_generation"], 1, "no replacement launch");
    let events = d.events(&run);
    assert!(events.len() > events_before);
    let texts: Vec<String> = events.iter().filter(|e| e["kind"] == "output").map(|e| e["payload"]["text"].as_str().unwrap().to_string()).collect();
    assert_eq!(texts.iter().filter(|t| *t == "started").count(), 1, "output not duplicated: {texts:?}");
    assert!(texts.contains(&"finished".to_string()), "output produced while daemon was down was recovered");
    let seqs: Vec<i64> = events.iter().map(|e| e["seq"].as_i64().unwrap()).collect();
    assert!(seqs.windows(2).all(|w| w[0] < w[1]), "event order preserved");
}

#[test]
fn ac07_lost_session_is_reported_not_running() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let mut d = Daemon::start(&[]);
    let created = sh(&d, &repo, "worktree", "sleep 30");
    let run = run_id(&created);
    d.wait_status(&run, |s| s == "running", 10);
    let (shim, _) = launch_info(&d, &run);
    d.kill9();
    signal(shim["child_pid"].as_i64().unwrap(), 9);
    signal(shim["shim_pid"].as_i64().unwrap(), 9);
    std::thread::sleep(Duration::from_millis(300));
    d.spawn();
    let run_now = d.run(&run);
    assert_eq!(run_now["status"], "disconnected", "{run_now}");
    assert!(run_now["exit_reason"].as_str().unwrap().contains("lost"), "{run_now}");
    assert_eq!(d.runs().len(), 1);
}

// ---------------------------------------------------------------- AC-06

#[test]
fn ac06_lifecycle_states_follow_real_signals() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let d = Daemon::start(&[]);
    let ok = run_id(&sh(&d, &repo, "worktree", "exit 0"));
    let fail = run_id(&sh(&d, &repo, "worktree", "echo boom >&2; exit 3"));
    let slow = run_id(&sh(&d, &repo, "worktree", "sleep 30"));
    let killed = run_id(&sh(&d, &repo, "worktree", "sleep 30"));
    let sup = run_id(&sh(&d, &repo, "worktree", "sleep 30"));
    assert_eq!(d.wait_done(&ok, 10)["status"], "completed");
    let f = d.wait_done(&fail, 10);
    assert_eq!(f["status"], "failed");
    assert!(f["exit_reason"].as_str().unwrap().contains("exit code 3"));
    d.wait_status(&slow, |s| s == "running", 10);
    d.call("run.interrupt", json!({"run_id": slow}));
    let i = d.wait_done(&slow, 15);
    assert_eq!(i["status"], "interrupted", "{i}");
    d.wait_status(&killed, |s| s == "running", 10);
    let (shim, _) = launch_info(&d, &killed);
    signal(shim["child_pid"].as_i64().unwrap(), 9); // external kill, not requested
    let k = d.wait_done(&killed, 10);
    assert_eq!(k["status"], "failed");
    assert!(k["exit_reason"].as_str().unwrap().contains("signal 9"), "{k}");
    d.wait_status(&sup, |s| s == "running", 10);
    let (shim2, _) = launch_info(&d, &sup);
    signal(shim2["shim_pid"].as_i64().unwrap(), 9); // supervisor lost
    let s = d.wait_done(&sup, 15);
    assert_eq!(s["status"], "disconnected", "{s}");
    signal(shim2["child_pid"].as_i64().unwrap(), 9);
}

#[test]
fn ac06_structured_harness_silence_is_not_completion() {
    // Fixture: a Codex transcript cut before turn.completed, exit 0.
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let cut = r.path().join("cut.jsonl");
    let full = std::fs::read_to_string(fixture("transcripts/codex-0.155-exec-subagent-live.jsonl")).unwrap();
    std::fs::write(&cut, full.lines().take(5).collect::<Vec<_>>().join("\n")).unwrap();
    let d = Daemon::start(&[("OVERSEER_CODEX_PATH", &fixture("fake-harness/replay.js")), ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "REPLAY_FILE,REPLAY_DELAY_MS"), ("REPLAY_FILE", cut.to_str().unwrap()), ("REPLAY_DELAY_MS", "10")]);
    let created = d.call("task.create", json!({"repo": repo, "harness": "codex", "prompt": "x", "title": "cut"}));
    let run = d.wait_done(&run_id(&created), 20);
    assert_eq!(run["status"], "unknown", "{run}");
    let kids: Vec<_> = d.runs().into_iter().filter(|x| x["parent_run_id"] == run["id"]).collect();
    assert!(kids.iter().all(|k| k["status"] == "unknown"), "child end never reported: {kids:?}");
}

// ---------------------------------------------------------------- AC-08

#[test]
fn ac08_socket_is_owner_only_and_requests_are_not_shell() {
    use std::os::unix::fs::PermissionsExt;
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let d = Daemon::start(&[]);
    let sock = d.socket();
    assert_eq!(std::fs::metadata(&sock).unwrap().permissions().mode() & 0o777, 0o600);
    assert_eq!(std::fs::metadata(sock.parent().unwrap()).unwrap().permissions().mode() & 0o777, 0o700);
    assert!(d.raw(b"{not json\n").contains("parse_error"));
    assert!(d.raw(b"{\"id\":1,\"method\":\"state\",\"params\":[1]}\n").contains("invalid_params"));
    assert!(d.raw(b"{\"id\":1,\"method\":7}\n").contains("invalid_request"));
    let big = format!("{{\"id\":1,\"method\":\"state\",\"params\":{{\"x\":\"{}\"}}}}\n", "a".repeat(1_100_000));
    assert!(d.raw(big.as_bytes()).contains("request_too_large"));
    let marker = r.path().join("pwned");
    let evil = format!("{}; touch {}", repo.display(), marker.display());
    assert!(d.try_call("task.create", json!({"repo": evil, "harness": "generic", "program": "/bin/echo", "args": []})).is_err());
    assert!(d.try_call("repo.inspect", json!({"path": format!("$(touch {})", marker.display())})).is_err());
    // Arguments are passed as argv, never through a shell.
    let created = d.generic(&repo, "worktree", "/bin/echo", &[&format!("$(touch {})", marker.display()), "; rm -rf /"]);
    d.wait_done(&run_id(&created), 10);
    assert!(!marker.exists(), "no shell fragment executed");
    let out: Vec<String> = d.events(&run_id(&created)).iter().filter(|e| e["kind"] == "output").map(|e| e["payload"]["text"].as_str().unwrap().to_string()).collect();
    assert!(out.iter().any(|t| t.contains("$(touch")), "{out:?}");
    assert!(d.try_call("no.such.method", json!({})).is_err());
}

// ---------------------------------------------------------------- AC-10

#[test]
fn ac10_replay_cursor_burst_and_retention() {
    use std::io::{BufRead, BufReader, Write};
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let d = Daemon::start(&[]);
    let created = sh(&d, &repo, "worktree", "i=0; while [ $i -lt 12000 ]; do echo line$i; i=$((i+1)); done");
    let run = run_id(&created);
    d.wait_done(&run, 60);
    let mut all = Vec::new();
    let mut after = 0;
    loop {
        let page = d.call("events.list", json!({"run_id": run, "after": after, "limit": 5000}))["events"].as_array().unwrap().clone();
        if page.is_empty() {
            break;
        }
        after = page.last().unwrap()["seq"].as_i64().unwrap();
        all.extend(page);
    }
    let events = &all;
    let retention = events.iter().find(|e| e["kind"] == "retention").expect("retention marker present");
    assert!(retention["payload"]["events_truncated_through_seq"].as_i64().unwrap() > 0);
    let first_output = events.iter().find(|e| e["kind"] == "output").unwrap()["payload"]["text"].as_str().unwrap().to_string();
    assert_ne!(first_output, "line0", "oldest events pruned, not silently kept");
    let last = events.iter().rev().find(|e| e["kind"] == "output").unwrap()["payload"]["text"].as_str().unwrap().to_string();
    assert_eq!(last, "line11999");
    // Reconnect from a mid-stream cursor: exactly the retained events after it, no duplicates.
    let mid = events[events.len() / 2]["seq"].as_i64().unwrap();
    let mut conn = std::os::unix::net::UnixStream::connect(d.socket()).unwrap();
    conn.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    conn.write_all(format!("{}\n", json!({"id": 1, "method": "events.subscribe", "params": {"after": mid, "run_id": run}})).as_bytes()).unwrap();
    let mut seqs = Vec::new();
    for line in BufReader::new(conn).lines() {
        let msg: serde_json::Value = serde_json::from_str(&line.unwrap()).unwrap();
        if msg["method"] == "replayed" {
            break;
        }
        if msg["method"] == "event" {
            seqs.push(msg["params"]["seq"].as_i64().unwrap());
        }
    }
    let expected: Vec<i64> = events.iter().map(|e| e["seq"].as_i64().unwrap()).filter(|s| *s > mid).collect();
    assert_eq!(seqs, expected);
    // Raw output stays inspectable and redacted.
    let raw = d.call("run.raw_output", json!({"run_id": run, "max_bytes": 4096}));
    assert_eq!(raw["truncated"], true);
}

#[test]
fn ac10_redaction_of_secrets_in_output() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let d = Daemon::start(&[]);
    let created = sh(&d, &repo, "worktree", "echo token sk-abcdefghijklmnopqrstuvwxyz0123; echo '{\"access_token\": \"abc\"}'");
    let run = run_id(&created);
    d.wait_done(&run, 10);
    let text = serde_json::to_string(&d.events(&run)).unwrap() + &d.call("run.raw_output", json!({"run_id": run})).to_string();
    assert!(!text.contains("sk-abcdefghijklmnopqrstuvwxyz0123"));
    assert!(text.contains("[redacted]"));
}

// ---------------------------------------------------------------- AC-15

#[test]
fn ac15_generic_harness_paths_with_spaces_failures_and_unknown_capabilities() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let dir = r.path().join("my tools");
    std::fs::create_dir_all(&dir).unwrap();
    let script = dir.join("fake agent.sh");
    std::fs::write(&script, "#!/bin/sh\nprintf 'arg:%s\\n' \"$@\"\nread line\necho \"got:$line\"\nexit ${EXIT_CODE:-0}\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    let d = Daemon::start(&[]);
    let created = d.call("task.create", json!({"repo": repo, "harness": "generic", "program": script, "args": ["two words", "x y z"], "prompt": "hello there", "title": "spaces"}));
    let run = run_id(&created);
    let done = d.wait_done(&run, 10);
    assert_eq!(done["status"], "completed");
    let out: Vec<String> = d.events(&run).iter().filter(|e| e["kind"] == "output").map(|e| e["payload"]["text"].as_str().unwrap().to_string()).collect();
    assert_eq!(out, vec!["arg:two words", "arg:x y z", "got:hello there"]);
    let caps = &done["capabilities"];
    for key in ["children", "usage", "quota", "approvals"] {
        assert!(caps[key].as_str().unwrap().starts_with("unknown"), "{key}: {}", caps[key]);
    }
    let failing = sh(&d, &repo, "worktree", "exit 7");
    let f = d.wait_done(&run_id(&failing), 10);
    assert_eq!(f["status"], "failed");
    let missing = d.call("task.create", json!({"repo": repo, "harness": "generic", "program": "/no/such binary", "args": [], "prompt": ""}));
    let m = d.wait_done(&run_id(&missing), 10);
    assert_eq!(m["status"], "failed");
    assert!(m["exit_reason"].as_str().unwrap().contains("could not start"), "{m}");
    let slow = sh(&d, &repo, "worktree", "trap 'echo got-int; exit 130' INT; sleep 30 & wait");
    d.wait_status(&run_id(&slow), |s| s == "running", 10);
    std::thread::sleep(Duration::from_millis(300));
    d.call("run.interrupt", json!({"run_id": run_id(&slow)}));
    assert_eq!(d.wait_done(&run_id(&slow), 15)["status"], "interrupted");
}

// ---------------------------------------------------------------- AC-16 (fixture only)

fn claude_daemon(mode: &str) -> Daemon {
    Daemon::start(&[("OVERSEER_CLAUDE_PATH", &fixture("fake-harness/claude-fixture.js")), ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE"), ("FIXTURE_MODE", mode)])
}

#[test]
fn ac16_fixture_permission_allow_deny_and_interrupt_waiting_run() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    for (answer, expect_file) in [(Some(true), true), (Some(false), false), (None, false)] {
        let d = claude_daemon("permission");
        let created = d.call("task.create", json!({"repo": repo, "harness": "claude", "prompt": "write perm.txt", "title": "perm"}));
        let run = run_id(&created);
        let waiting = d.wait_status(&run, |s| s == "waiting_for_user", 15);
        let req = waiting["attention"]["request_id"].as_str().unwrap().to_string();
        std::thread::sleep(Duration::from_millis(500));
        assert_eq!(d.run(&run)["status"], "waiting_for_user", "never auto-approved");
        match answer {
            Some(allow) => {
                d.call("run.permission", json!({"run_id": run, "request_id": req, "allow": allow}));
                assert_eq!(d.wait_done(&run, 15)["status"], "completed");
            }
            None => {
                d.call("run.interrupt", json!({"run_id": run}));
                assert_eq!(d.wait_done(&run, 20)["status"], "interrupted");
            }
        }
        assert_eq!(ws_path(&d, &created).join("perm.txt").exists(), expect_file, "answer {answer:?}");
    }
}

#[test]
fn ac16_fixture_error_classes_stay_distinct() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    for (mode, class) in [("auth", "auth"), ("ratelimit", "rate_limit"), ("quota", "quota")] {
        let d = claude_daemon(mode);
        let created = d.call("task.create", json!({"repo": repo, "harness": "claude", "prompt": "x", "title": mode}));
        let run = run_id(&created);
        let done = d.wait_done(&run, 15);
        assert_eq!(done["status"], "failed", "{mode}");
        let classes: Vec<String> = d.events(&run).iter().filter(|e| e["kind"] == "error").map(|e| e["payload"]["class"].as_str().unwrap().to_string()).collect();
        assert!(classes.contains(&class.to_string()), "{mode}: {classes:?}");
        assert!(done["exit_reason"].as_str().unwrap().contains(class), "{done}");
    }
}

// ---------------------------------------------------------------- AC-18 / AC-20

#[test]
fn ac18_fixture_recursive_tree_duplicates_and_delayed_parent() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let mut d = claude_daemon("nested");
    let created = d.call("task.create", json!({"repo": repo, "harness": "claude", "prompt": "nest", "title": "nest"}));
    let root = run_id(&created);
    assert_eq!(d.wait_done(&root, 15)["status"], "completed");
    let check = |d: &Daemon| {
        let runs = d.runs();
        assert_eq!(runs.len(), 3, "root + child + grandchild, no duplicates: {runs:?}");
        let child = runs.iter().find(|r| r["native_id"] == "toolu_child").unwrap();
        let grand = runs.iter().find(|r| r["native_id"] == "toolu_grand").unwrap();
        assert_eq!(child["parent_run_id"], root.as_str());
        assert_eq!(grand["parent_run_id"], child["id"], "delayed parent adopted its child");
        assert!(grand["relation_confidence"].as_str().unwrap().starts_with("exact"));
        assert_eq!(grand["status"], "completed");
        assert_eq!(child["status"], "completed");
        assert_eq!(grand["workspace_id"], child["workspace_id"], "native children share the workspace");
        for run in &runs {
            let mut seen = std::collections::HashSet::new();
            let mut cur = Some(run["id"].as_str().unwrap().to_string());
            while let Some(id) = cur {
                assert!(seen.insert(id.clone()), "cycle at {id}");
                cur = runs.iter().find(|x| x["id"] == id.as_str()).unwrap()["parent_run_id"].as_str().map(str::to_string);
            }
        }
    };
    check(&d);
    let child = d.runs().into_iter().find(|r| r["native_id"] == "toolu_child").unwrap();
    let err = d.try_call("run.follow_up", json!({"run_id": child["id"], "prompt": "x"})).unwrap_err();
    assert!(err.contains("top-level run"), "{err}");
    assert!(d.try_call("run.interrupt", json!({"run_id": child["id"]})).unwrap_err().contains("parent"));
    d.kill9();
    d.spawn();
    check(&d);
}

#[test]
fn ac20_prose_is_not_a_child_and_unknown_events_are_visible() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let d = claude_daemon("prose");
    let created = d.call("task.create", json!({"repo": repo, "harness": "claude", "prompt": "x", "title": "prose"}));
    d.wait_done(&run_id(&created), 15);
    assert_eq!(d.runs().len(), 1, "text claiming delegation created no child");
    // Parser-version mismatch: an unknown Codex event type is retained as unparsed with the parser version.
    let weird = r.path().join("weird.jsonl");
    std::fs::write(&weird, "{\"type\":\"thread.started\",\"thread_id\":\"t1\"}\n{\"type\":\"future.event\",\"x\":1}\n{\"type\":\"turn.completed\",\"usage\":{}}\n").unwrap();
    let d2 = Daemon::start(&[("OVERSEER_CODEX_PATH", &fixture("fake-harness/replay.js")), ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "REPLAY_FILE,REPLAY_DELAY_MS"), ("REPLAY_FILE", weird.to_str().unwrap()), ("REPLAY_DELAY_MS", "10")]);
    let c2 = d2.call("task.create", json!({"repo": repo, "harness": "codex", "prompt": "x", "title": "weird"}));
    d2.wait_done(&run_id(&c2), 15);
    let unparsed: Vec<_> = d2.events(&run_id(&c2)).into_iter().filter(|e| e["kind"] == "raw_unparsed").collect();
    assert_eq!(unparsed.len(), 1);
    assert_eq!(unparsed[0]["confidence"], "unknown");
    assert!(unparsed[0]["payload"]["parser_version"].is_string());
}

#[test]
fn ac19_fixture_codex_live_transcript_children() {
    // Replays the recorded LIVE Codex 0.155 transcript; the live capture itself is AC-19 evidence.
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let d = Daemon::start(&[("OVERSEER_CODEX_PATH", &fixture("fake-harness/replay.js")), ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "REPLAY_FILE,REPLAY_DELAY_MS"), ("REPLAY_FILE", &fixture("transcripts/codex-0.155-exec-subagent-live.jsonl")), ("REPLAY_DELAY_MS", "10")]);
    let created = d.call("task.create", json!({"repo": repo, "harness": "codex", "prompt": "x", "title": "codex"}));
    let root = run_id(&created);
    assert_eq!(d.wait_done(&root, 15)["status"], "completed");
    let kids: Vec<_> = d.runs().into_iter().filter(|x| x["parent_run_id"] == root.as_str()).collect();
    assert_eq!(kids.len(), 1);
    assert_eq!(kids[0]["native_id"], "01a0d6e3-e523-7be1-a56d-243f00cb399c");
    assert_eq!(kids[0]["status"], "completed");
    let child_out: Vec<_> = d.events(kids[0]["id"].as_str().unwrap()).into_iter().filter(|e| e["kind"] == "output").collect();
    assert!(child_out.iter().any(|e| e["payload"]["text"] == "hi"));
}

// ---------------------------------------------------------------- AC-21 / AC-23 / AC-24

#[test]
fn ac21_parallel_worktrees_are_independent_and_collisions_are_safe() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    std::fs::write(repo.join("a.txt"), "dirty source\n").unwrap();
    let source_before = fingerprint(&repo);
    let d = Daemon::start(&[]);
    // Pre-existing branch with the name the first task would get.
    git(&repo, &["branch", "overseer/same-name"]);
    let one = d.call("task.create", json!({"repo": repo, "harness": "generic", "program": "/bin/sh", "args": ["-c", "sleep 1; echo one > shared.txt"], "prompt": "", "title": "same name"}));
    let two = d.call("task.create", json!({"repo": repo, "harness": "generic", "program": "/bin/sh", "args": ["-c", "sleep 1; echo two > shared.txt"], "prompt": "", "title": "same name"}));
    d.wait_done(&run_id(&one), 10);
    d.wait_done(&run_id(&two), 10);
    let (p1, p2) = (ws_path(&d, &one), ws_path(&d, &two));
    assert_ne!(p1, p2);
    assert_eq!(std::fs::read_to_string(p1.join("shared.txt")).unwrap(), "one\n");
    assert_eq!(std::fs::read_to_string(p2.join("shared.txt")).unwrap(), "two\n");
    assert_eq!(one["workspace"]["branch"], "overseer/same-name-2");
    assert_eq!(two["workspace"]["branch"], "overseer/same-name-3");
    assert_eq!(git(&repo, &["rev-parse", "overseer/same-name"]), git(&repo, &["rev-parse", "main"]), "existing branch untouched");
    assert_eq!(fingerprint(&repo), source_before, "source checkout unchanged");
}

#[test]
fn ac23_unrelated_writer_rejected_on_current_checkout() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let d = Daemon::start(&[]);
    let first = sh(&d, &repo, "current", "sleep 3");
    let err = d.try_call("task.create", json!({"repo": repo, "harness": "generic", "workspace_mode": "current", "program": "/bin/echo", "args": []}));
    assert!(err.unwrap_err().contains("already has an active writer"));
    d.wait_done(&run_id(&first), 10);
    let second = sh(&d, &repo, "current", "true");
    assert_eq!(d.wait_done(&run_id(&second), 10)["status"], "completed", "allowed once the first writer ended");
}

#[test]
fn ac24_cleanup_reports_and_preserves_until_confirmed() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let d = Daemon::start(&[]);
    let current = sh(&d, &repo, "current", "true");
    d.wait_done(&run_id(&current), 10);
    let err = d.try_call("workspace.cleanup", json!({"workspace_id": current["workspace"]["id"], "discard_dirty": true})).unwrap_err();
    assert!(err.contains("never removed"), "{err}");
    let active = sh(&d, &repo, "worktree", "sleep 5");
    let plan = d.call("workspace.cleanup_plan", json!({"workspace_id": active["workspace"]["id"]}));
    assert_eq!(plan["removable"], false);
    assert_eq!(plan["active_runs"].as_array().unwrap().len(), 1);
    assert!(d.try_call("workspace.cleanup", json!({"workspace_id": active["workspace"]["id"], "discard_dirty": true})).is_err());
    let interrupted = sh(&d, &repo, "worktree", "echo work > untracked.txt; sleep 30");
    let run = run_id(&interrupted);
    d.wait_status(&run, |s| s == "running", 10);
    std::thread::sleep(Duration::from_millis(400));
    d.call("run.interrupt", json!({"run_id": run}));
    assert_eq!(d.wait_done(&run, 15)["status"], "interrupted");
    let path = ws_path(&d, &interrupted);
    assert!(path.join("untracked.txt").exists(), "interrupted work retained");
    let plan = d.call("workspace.cleanup_plan", json!({"workspace_id": interrupted["workspace"]["id"]}));
    assert_eq!(plan["dirty"]["untracked"][0], "untracked.txt");
    assert!(d.try_call("workspace.cleanup", json!({"workspace_id": interrupted["workspace"]["id"]})).unwrap_err().contains("uncommitted"));
    assert!(path.join("untracked.txt").exists());
    let res = d.call("workspace.cleanup", json!({"workspace_id": interrupted["workspace"]["id"], "discard_dirty": true}));
    assert!(!path.exists());
    assert!(git(&repo, &["branch", "--list", res["branch_kept"].as_str().unwrap()]).contains("overseer/"), "branch kept");
    d.wait_done(&run_id(&active), 10);
}

// ---------------------------------------------------------------- AC-22

#[test]
fn ac22_current_checkout_preserves_preexisting_work() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    std::fs::write(repo.join("a.txt"), "a staged\n").unwrap();
    git(&repo, &["add", "a.txt"]);
    std::fs::write(repo.join("a.txt"), "a staged then unstaged\n").unwrap();
    std::fs::write(repo.join("b.txt"), "b unstaged\n").unwrap();
    std::fs::write(repo.join("notes.txt"), "untracked\n").unwrap();
    let index_before = git(&repo, &["diff", "--cached"]);
    let d = Daemon::start(&[]);
    let created = d.call("task.create", json!({"repo": repo, "harness": "generic", "workspace_mode": "current", "program": "/bin/sh",
        "args": ["-c", "echo agent > agent.txt; echo agent-edit >> b.txt; sleep 30"], "prompt": "", "title": "current", "unsaved": ["draft.txt"]}));
    let run = run_id(&created);
    d.wait_status(&run, |s| s == "running", 10);
    std::thread::sleep(Duration::from_millis(600));
    d.call("run.interrupt", json!({"run_id": run}));
    assert_eq!(d.wait_done(&run, 15)["status"], "interrupted");
    assert_eq!(git(&repo, &["diff", "--cached"]), index_before, "staged work intact");
    assert_eq!(std::fs::read_to_string(repo.join("a.txt")).unwrap(), "a staged then unstaged\n");
    assert_eq!(std::fs::read_to_string(repo.join("b.txt")).unwrap(), "b unstaged\nagent-edit\n");
    assert_eq!(std::fs::read_to_string(repo.join("notes.txt")).unwrap(), "untracked\n");
    assert!(git(&repo, &["stash", "list"]).is_empty(), "nothing stashed");
    let ws = &created["workspace"];
    assert_eq!(ws["kind"], "current");
    assert_eq!(ws["initial_dirty"]["staged"][0]["path"], "a.txt");
    assert_eq!(ws["initial_dirty"]["untracked"][0], "notes.txt");
    assert_eq!(ws["initial_dirty"]["unsaved_drafts"][0], "draft.txt");
    let latest = option(&d, &run, "latest_run", None);
    assert_eq!(diff_paths(&d, &created, latest["base"].as_str().unwrap()), vec![("A".into(), "agent.txt".into()), ("M".into(), "b.txt".into())], "only observed run changes");
    let fork = option(&d, &run, "fork", None);
    assert!(fork["label"].as_str().unwrap().contains("detected"), "current-checkout fork is a labeled candidate: {fork}");
}

// ---------------------------------------------------------------- AC-26

#[test]
fn ac26_snapshots_and_selectable_bases() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    // Stacked branches with pre-existing feature commits.
    git(&repo, &["switch", "-q", "-c", "feature-a"]);
    std::fs::write(repo.join("feature.txt"), "feature a\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "feature a"]);
    git(&repo, &["switch", "-q", "-c", "feature-b"]);
    std::fs::write(repo.join("b.txt"), "b on feature-b\n").unwrap();
    git(&repo, &["commit", "-q", "-am", "feature b"]);
    // Dirty starting state: staged, unstaged, untracked.
    std::fs::write(repo.join("a.txt"), "a staged\n").unwrap();
    git(&repo, &["add", "a.txt"]);
    std::fs::write(repo.join("b.txt"), "b unstaged\n").unwrap();
    std::fs::write(repo.join("pre.txt"), "pre-existing untracked\n").unwrap();
    let before = fingerprint(&repo);
    let mut d = Daemon::start(&[]);
    let created = d.call("task.create", json!({"repo": repo, "harness": "generic", "workspace_mode": "current", "program": "/bin/sh", "args": ["-c", "echo run1 >> pre.txt"], "prompt": "", "title": "snap"}));
    let run = run_id(&created);
    d.wait_done(&run, 10);
    assert_eq!(fingerprint(&repo).replace("pre-existing untracked\nrun1", "pre-existing untracked"), before, "snapshots did not mutate, stage or stash anything");
    let latest1 = option(&d, &run, "latest_run", None);
    assert_eq!(latest1["default"], true);
    assert_eq!(diff_paths(&d, &created, latest1["base"].as_str().unwrap()), vec![("M".into(), "pre.txt".into())], "baseline included dirty+untracked contents");
    // A user edit between runs, then turn 2 (follow-up) gets its own baseline.
    std::fs::write(repo.join("between.txt"), "user edit between runs\n").unwrap();
    d.call("run.follow_up", json!({"run_id": run, "prompt": ""}));
    d.wait_done(&run, 10);
    let latest2 = option(&d, &run, "latest_run", None);
    assert_ne!(latest1["base"], latest2["base"]);
    assert_eq!(diff_paths(&d, &created, latest2["base"].as_str().unwrap()), vec![("M".into(), "pre.txt".into())], "turn-2 diff excludes the between-runs user edit");
    let turn1 = option(&d, &run, "turn:1", None);
    assert_eq!(turn1["base"], latest1["base"], "prior baseline addressable");
    let start = option(&d, &run, "task_start", None);
    let since_start = diff_paths(&d, &created, start["base"].as_str().unwrap());
    assert!(since_start.contains(&("A".into(), "between.txt".into())) && since_start.contains(&("M".into(), "pre.txt".into())));
    assert!(!since_start.iter().any(|(_, p)| p == "a.txt" || p == "b.txt"), "task start preserves dirty starting contents, not just HEAD: {since_start:?}");
    // Branch comparisons: merge-base vs tip, stacked parent, target advance, missing target.
    let mb = option(&d, &run, "branch_merge_base", Some("feature-a"));
    assert_eq!(mb["base"], git(&repo, &["rev-parse", "feature-a"]).as_str());
    let tip_main = option(&d, &run, "branch_tip", Some("main"));
    assert_eq!(tip_main["base"], git(&repo, &["rev-parse", "main"]).as_str());
    let branch_diff = diff_paths(&d, &created, mb["base"].as_str().unwrap());
    assert!(branch_diff.contains(&("M".into(), "b.txt".into())), "branch mode includes committed + dirty: {branch_diff:?}");
    git(&repo, &["stash", "list"]);
    let wt = r.path().join("adv");
    git(&repo, &["worktree", "add", "-q", wt.to_str().unwrap(), "main"]);
    std::fs::write(wt.join("main-only.txt"), "advance\n").unwrap();
    git(&wt, &["add", "."]);
    git(&wt, &["commit", "-q", "-m", "main advances"]);
    let mb_main = option(&d, &run, "branch_merge_base", Some("main"));
    let tip_main2 = option(&d, &run, "branch_tip", Some("main"));
    assert_ne!(mb_main["base"], tip_main2["base"], "target advance distinguishes merge-base from tip");
    let missing = d.call("comparison.options", json!({"run_id": run, "branch": "no-such-branch"}));
    let miss = missing["options"].as_array().unwrap().iter().find(|o| o["mode"] == "branch_merge_base").unwrap();
    assert_eq!(miss["available"], false);
    assert!(miss["detail"].as_str().unwrap().contains("not found"));
    assert!(d.try_call("workspace.diff", json!({"workspace_id": created["workspace"]["id"], "base": "no-such-branch"})).is_err(), "unresolved base is an error, not an empty diff");
    // Rebase cannot rewrite recorded snapshots; they survive a daemon restart.
    git(&repo, &["stash", "push", "-q", "-u", "-m", "test-only"]);
    git(&repo, &["rebase", "-q", "main"]);
    git(&repo, &["stash", "pop", "-q"]);
    d.kill9();
    d.spawn();
    let again = option(&d, &run, "turn:1", None);
    assert_eq!(again["base"], latest1["base"]);
    assert!(git(&repo, &["cat-file", "-t", latest1["base"].as_str().unwrap()]) == "commit");
    let fork = option(&d, &run, "fork", None);
    assert!(fork["available"] == true || fork["detail"].as_str().unwrap().starts_with("unknown"));
}

#[test]
fn ac26_unknown_fork_is_reported_unavailable() {
    let r = tmp();
    let dir = r.path().join("orphan");
    std::fs::create_dir_all(&dir).unwrap();
    git(&dir, &["init", "-q", "-b", "work"]);
    git(&dir, &["config", "user.email", "t@e"]);
    git(&dir, &["config", "user.name", "t"]);
    std::fs::write(dir.join("x"), "x").unwrap();
    git(&dir, &["add", "."]);
    git(&dir, &["commit", "-q", "-m", "x"]);
    let d = Daemon::start(&[]);
    let created = d.generic(&dir, "current", "/usr/bin/true", &[]);
    let fork = option(&d, &run_id(&created), "fork", None);
    assert_eq!(fork["available"], false, "{fork}");
    assert!(fork["detail"].as_str().unwrap().starts_with("unknown"));
}

// ---------------------------------------------------------------- AC-27 / AC-28

#[test]
fn ac27_complete_change_and_dirty_views() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let d = Daemon::start(&[]);
    // Clean main/main.
    let clean = d.generic(&repo, "current", "/usr/bin/true", &[]);
    let run = run_id(&clean);
    d.wait_done(&run, 10);
    let head = git(&repo, &["rev-parse", "HEAD"]);
    assert!(diff_paths(&d, &clean, &head).is_empty(), "clean main/main is empty");
    let st = d.call("workspace.status", json!({"workspace_id": clean["workspace"]["id"]}));
    assert!(st["staged"].as_array().unwrap().is_empty() && st["unstaged"].as_array().unwrap().is_empty() && st["untracked"].as_array().unwrap().is_empty());
    // Dirty main/main: staged, unstaged, rename, delete, untracked, binary, oversized, ignored.
    std::fs::write(repo.join("a.txt"), "a changed\n").unwrap();
    git(&repo, &["add", "a.txt"]);
    git(&repo, &["mv", "b.txt", "b-renamed.txt"]);
    std::fs::write(repo.join("new.txt"), "untracked\n").unwrap();
    std::fs::write(repo.join("bin.dat"), [0u8, 1, 2, 0, 255]).unwrap();
    std::fs::write(repo.join("big.txt"), "x".repeat(3 * 1024 * 1024)).unwrap();
    std::fs::create_dir_all(repo.join("ignored")).unwrap();
    for i in 0..200 {
        std::fs::write(repo.join(format!("ignored/f{i}")), "i").unwrap();
    }
    std::fs::write(repo.join("debug.log"), "log").unwrap();
    std::fs::remove_file(repo.join(".gitignore")).unwrap();
    git(&repo, &["checkout", "--", ".gitignore"]);
    let changes = diff_paths(&d, &clean, &head);
    assert!(changes.contains(&("M".into(), "a.txt".into())));
    assert!(changes.contains(&("R".into(), "b-renamed.txt".into())));
    for f in ["new.txt", "bin.dat", "big.txt"] {
        assert!(changes.contains(&("A".into(), f.into())), "{f} listed: {changes:?}");
    }
    assert!(!changes.iter().any(|(_, p)| p.starts_with("ignored/") || p == "debug.log"), "ignored files not listed");
    let st = d.call("workspace.status", json!({"workspace_id": clean["workspace"]["id"]}));
    let staged: Vec<&str> = st["staged"].as_array().unwrap().iter().map(|c| c["path"].as_str().unwrap()).collect();
    assert!(staged.contains(&"a.txt") && staged.contains(&"b-renamed.txt") && staged.contains(&"b.txt"), "{staged:?}");
    // A fresh run has an empty run diff but the dirty view still shows the work.
    let fresh = d.generic(&repo, "current", "/usr/bin/true", &[]);
    d.wait_done(&run_id(&fresh), 10);
    let latest = option(&d, &run_id(&fresh), "latest_run", None);
    assert!(diff_paths(&d, &fresh, latest["base"].as_str().unwrap()).is_empty());
    let st2 = d.call("workspace.status", json!({"workspace_id": fresh["workspace"]["id"]}));
    assert!(!st2["staged"].as_array().unwrap().is_empty() && !st2["untracked"].as_array().unwrap().is_empty());
    // Deletion shows as D in branch mode.
    std::fs::remove_file(repo.join("a.txt")).unwrap();
    assert!(diff_paths(&d, &clean, &head).contains(&("D".into(), "a.txt".into())));
}

#[test]
fn ac28_opposing_layers_remain_inspectable() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let d = Daemon::start(&[]);
    let created = d.generic(&repo, "current", "/usr/bin/true", &[]);
    d.wait_done(&run_id(&created), 10);
    let head = git(&repo, &["rev-parse", "HEAD"]);
    std::fs::write(repo.join("a.txt"), "B\n").unwrap();
    git(&repo, &["add", "a.txt"]);
    std::fs::write(repo.join("a.txt"), "a\n").unwrap(); // restored without staging
    assert!(diff_paths(&d, &created, &head).is_empty(), "net diff cancels out");
    let st = d.call("workspace.status", json!({"workspace_id": created["workspace"]["id"]}));
    assert_eq!(st["staged"][0]["path"], "a.txt");
    assert_eq!(st["unstaged"][0]["path"], "a.txt");
    // Staged deletion followed by untracked recreation.
    git(&repo, &["rm", "-q", "b.txt"]);
    std::fs::write(repo.join("b.txt"), "recreated\n").unwrap();
    let st = d.call("workspace.status", json!({"workspace_id": created["workspace"]["id"]}));
    assert!(st["staged"].as_array().unwrap().iter().any(|c| c["path"] == "b.txt" && c["status"] == "D"), "{st}");
    assert!(st["untracked"].as_array().unwrap().iter().any(|p| p == "b.txt"), "{st}");
    assert!(diff_paths(&d, &created, &head).contains(&("M".into(), "b.txt".into())));
}

// ---------------------------------------------------------------- AC-11 / AC-13 (non-login parts)

#[test]
fn ac13_isolated_profiles_have_separate_homes_and_no_keys() {
    let d = Daemon::start(&[]);
    let a = d.call("profile.create", json!({"name": "Codex A", "harness": "codex"}));
    let b = d.call("profile.create", json!({"name": "Codex B", "harness": "codex"}));
    assert_ne!(a["home"], b["home"]);
    let la = d.call("profile.login_command", json!({"id": a["id"]}));
    let lb = d.call("profile.login_command", json!({"id": b["id"]}));
    assert_ne!(la["env"]["CODEX_HOME"], lb["env"]["CODEX_HOME"]);
    assert_eq!(la["args"], json!(["login"]));
    let err = d.try_call("profile.logout", json!({"id": "system-codex"})).unwrap_err();
    assert!(err.contains("does not log out"), "{err}");
    d.call("profile.rename", json!({"id": a["id"], "name": "Codex Alpha"}));
    let names: Vec<String> = d.call("profile.list", json!({})).as_array().unwrap().iter().map(|p| p["name"].as_str().unwrap().to_string()).collect();
    assert!(names.contains(&"Codex Alpha".to_string()));
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(std::fs::metadata(a["home"].as_str().unwrap()).unwrap().permissions().mode() & 0o777, 0o700);
}

#[test]
fn ac34_merge_conflicts_do_not_break_snapshots_or_diffs() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    git(&repo, &["switch", "-q", "-c", "other"]);
    std::fs::write(repo.join("a.txt"), "theirs\n").unwrap();
    git(&repo, &["commit", "-q", "-am", "theirs"]);
    git(&repo, &["switch", "-q", "main"]);
    std::fs::write(repo.join("a.txt"), "ours\n").unwrap();
    git(&repo, &["commit", "-q", "-am", "ours"]);
    let _ = std::process::Command::new("git").current_dir(&repo).args(["merge", "other"]).output();
    let index_before = std::fs::read(repo.join(".git/index")).unwrap();
    let d = Daemon::start(&[]);
    let created = d.generic(&repo, "current", "/usr/bin/true", &[]);
    assert_eq!(d.wait_done(&run_id(&created), 10)["status"], "completed", "snapshot succeeded during a conflict");
    let st = d.call("workspace.status", json!({"workspace_id": created["workspace"]["id"]}));
    assert_eq!(st["conflicted"][0], "a.txt");
    let head = git(&repo, &["rev-parse", "HEAD"]);
    assert!(diff_paths(&d, &created, &head).contains(&("M".into(), "a.txt".into())));
    assert_eq!(std::fs::read(repo.join(".git/index")).unwrap(), index_before, "real index (with conflict stages) untouched");
}

#[test]
fn ac09_follow_up_reaches_only_the_selected_run() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let d = Daemon::start(&[]);
    let a = d.generic(&repo, "worktree", "/bin/sh", &["-c", "cat >> input.txt"]);
    let b = d.generic(&repo, "worktree", "/bin/sh", &["-c", "cat >> input.txt"]);
    let (ra, rb) = (run_id(&a), run_id(&b));
    d.wait_status(&ra, |s| s == "running", 10);
    d.wait_status(&rb, |s| s == "running", 10);
    d.call("run.follow_up", json!({"run_id": ra, "prompt": "only for A"}));
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(d.call("run.turns", json!({"run_id": ra})).as_array().unwrap().len(), 2);
    assert_eq!(d.call("run.turns", json!({"run_id": rb})).as_array().unwrap().len(), 1);
    assert_eq!(std::fs::read_to_string(ws_path(&d, &a).join("input.txt")).unwrap_or_default(), "only for A\n");
    assert_eq!(std::fs::read_to_string(ws_path(&d, &b).join("input.txt")).unwrap_or_default(), "");
    d.call("run.interrupt", json!({"run_id": ra}));
    d.call("run.interrupt", json!({"run_id": rb}));
    // Children cannot receive follow-ups or interrupts directly.
    let err = d.try_call("run.follow_up", json!({"run_id": "r-missing", "prompt": "x"})).unwrap_err();
    assert!(err.contains("unknown run"));
}

#[test]
fn ac16_fixture_codex_app_server_approvals_interrupt_and_unsupported_requests() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let d = Daemon::start(&[("OVERSEER_CODEX_PATH", &fixture("fake-harness/codex-app-fixture.js"))]);
    for mode in ["allow", "deny", "interrupt"] {
        let created = d.call("task.create", json!({"repo": repo, "harness": "codex-app", "prompt": "touch approved.txt", "title": mode, "approval_policy": "untrusted"}));
        let run = run_id(&created);
        let waiting = d.wait_status(&run, |s| s == "waiting_for_user", 15);
        assert!(waiting["attention"]["tool"].as_str().unwrap().contains("touch approved.txt"));
        std::thread::sleep(Duration::from_millis(400));
        assert_eq!(d.run(&run)["status"], "waiting_for_user", "never auto-approved");
        let req = waiting["attention"]["request_id"].as_str().unwrap().to_string();
        match mode {
            "allow" => { d.call("run.permission", json!({"run_id": run, "request_id": req, "allow": true})); }
            "deny" => { d.call("run.permission", json!({"run_id": run, "request_id": req, "allow": false})); }
            _ => { d.call("run.interrupt", json!({"run_id": run})); }
        }
        let done = d.wait_done(&run, 20);
        let expect = if mode == "interrupt" { "interrupted" } else { "completed" };
        assert_eq!(done["status"], expect, "{mode}: {done}");
        assert_eq!(ws_path(&d, &created).join("approved.txt").exists(), mode == "allow", "{mode}");
        let text: Vec<String> = d.events(&run).iter().filter(|e| e["kind"] == "output").map(|e| e["payload"]["text"].as_str().unwrap_or_default().to_string()).collect();
        if mode != "interrupt" {
            assert!(text.iter().any(|t| t.contains("unsupported request was refused")), "{text:?}");
        }
        assert_eq!(done["native_id"], "thr-fixture-1");
    }
}

#[test]
fn ac14_fixture_claude_background_subagent_keeps_session_open_for_permissions() {
    // Regression for a live Claude 2.1.246 run: an interim `result` arrives while a background
    // subagent runs; closing stdin then made later permission requests fail ("Stream closed").
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let d = claude_daemon("background");
    let created = d.call("task.create", json!({"repo": repo, "harness": "claude", "prompt": "bg", "title": "bg"}));
    let run = run_id(&created);
    let waiting = d.wait_status(&run, |s| s == "waiting_for_user", 15);
    d.call("run.permission", json!({"run_id": run, "request_id": waiting["attention"]["request_id"], "allow": true}));
    assert_eq!(d.wait_done(&run, 15)["status"], "completed");
    assert!(ws_path(&d, &created).join("bg.txt").exists());
    let kids: Vec<_> = d.runs().into_iter().filter(|x| x["parent_run_id"] == run.as_str()).collect();
    assert_eq!(kids.len(), 1);
    assert_eq!(kids[0]["status"], "completed");
}

#[test]
fn ac14_fixture_claude_background_task_finishing_before_the_interim_result_still_keeps_the_session_open() {
    // Regression for a live Claude run (2026-09-25, AC-43 live scenario): the background agent
    // finished and was reported before the interim `result`, so no task was "still running";
    // Claude then continued with another turn whose Write permission failed with "Stream closed".
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let d = claude_daemon("background-early");
    let created = d.call("task.create", json!({"repo": repo, "harness": "claude", "prompt": "bg", "title": "bg"}));
    let run = run_id(&created);
    let waiting = d.wait_status(&run, |s| s == "waiting_for_user", 15);
    assert_eq!(waiting["attention"]["tool"], "Write");
    d.call("run.permission", json!({"run_id": run, "request_id": waiting["attention"]["request_id"], "allow": true}));
    assert_eq!(d.wait_done(&run, 15)["status"], "completed");
    assert!(ws_path(&d, &created).join("bg.txt").exists(), "the continuation turn's Write was allowed and ran");
    let dones = d.events(&run).into_iter().filter(|e| e["kind"] == "turn_done").count();
    assert_eq!(dones, 1, "the interim result is not a finished turn");
}

#[test]
fn ac19_fixture_codex_app_child_threads_nest_and_do_not_end_the_parent() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let d = Daemon::start(&[("OVERSEER_CODEX_PATH", &fixture("fake-harness/codex-app-fixture.js")), ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE"), ("FIXTURE_MODE", "tree")]);
    let created = d.call("task.create", json!({"repo": repo, "harness": "codex-app", "prompt": "tree", "title": "tree", "approval_policy": "untrusted", "extra_args": ["-c", "agents.max_depth=2"]}));
    let root = run_id(&created);
    // The child's turn/completed must not end the root turn: the root still reaches its approval.
    let waiting = d.wait_status(&root, |s| s == "waiting_for_user", 15);
    d.call("run.permission", json!({"run_id": root, "request_id": waiting["attention"]["request_id"], "allow": true}));
    assert_eq!(d.wait_done(&root, 15)["status"], "completed");
    let runs = d.runs();
    let child = runs.iter().find(|x| x["native_id"] == "thr-child").expect("child");
    let grand = runs.iter().find(|x| x["native_id"] == "thr-grand").expect("grandchild");
    assert_eq!(child["parent_run_id"], root.as_str());
    assert_eq!(grand["parent_run_id"], child["id"]);
    assert_eq!(child["status"], "completed");
    let child_out: Vec<_> = d.events(child["id"].as_str().unwrap()).into_iter().filter(|e| e["kind"] == "output").map(|e| e["payload"]["text"].as_str().unwrap_or_default().to_string()).collect();
    assert!(child_out.contains(&"child output".to_string()), "{child_out:?}");
    let root_out: Vec<_> = d.events(&root).into_iter().filter(|e| e["kind"] == "output").map(|e| e["payload"]["text"].as_str().unwrap_or_default().to_string()).collect();
    assert!(!root_out.contains(&"child output".to_string()), "child text not attributed to the root");
    let launch = std::fs::read_to_string(d.home.path().join("runs").join(&root).join("p1/launch.json")).unwrap();
    assert!(launch.contains("agents.max_depth=2"), "extra args passed to the harness");
}

// ---------------------------------------------------------------- AC-45 visible background agents

/// A persistent connection that identifies itself as a VS Code window.
fn vscode_window(d: &Daemon) -> std::os::unix::net::UnixStream {
    use std::io::{BufRead, BufReader, Write};
    let mut conn = std::os::unix::net::UnixStream::connect(d.socket()).unwrap();
    conn.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    conn.write_all(format!("{}\n", json!({"id": 1, "method": "hello", "params": {"client": "vscode"}})).as_bytes()).unwrap();
    let mut line = String::new();
    BufReader::new(conn.try_clone().unwrap()).read_line(&mut line).unwrap();
    assert!(line.contains("\"protocol\""), "{line}");
    conn
}

fn notifier(dir: &Path) -> (String, std::path::PathBuf) {
    let log = dir.join("notices.log");
    let script = dir.join("notify.sh");
    std::fs::write(&script, format!("#!/bin/sh\nprintf '%s|%s\\n' \"$1\" \"$2\" >> '{}'\n", log.display())).unwrap();
    std::process::Command::new("chmod").arg("+x").arg(&script).status().unwrap();
    (script.display().to_string(), log)
}

fn notices(d: &Daemon) -> Vec<serde_json::Value> {
    d.call("events.list", json!({"after": 0, "limit": 5000}))["events"].as_array().unwrap().iter().filter(|e| e["kind"] == "background_notice").cloned().collect()
}

#[test]
fn ac45_last_vscode_window_closing_with_active_runs_posts_a_notice_but_a_reload_does_not() {
    let t = tmp();
    let (cmd, log) = notifier(t.path());
    let d = Daemon::start(&[("OVERSEER_BACKGROUND_NOTICE_MS", "600"), ("OVERSEER_NOTIFY_COMMAND", &cmd)]);
    let repo = repo(&t.path().join("r"));
    let created = sh(&d, &repo, "worktree", "sleep 30");
    let run = run_id(&created);
    d.wait_status(&run, |s| s == "running", 20);
    let w1 = vscode_window(&d);
    let w2 = vscode_window(&d);
    assert_eq!(d.call("daemon.clients", json!({}))["vscode"], 2);
    // One of two windows closes: not the last, no notice.
    drop(w1);
    std::thread::sleep(Duration::from_millis(1200));
    assert!(!log.exists(), "closing one of two windows must not notify");
    // Reload: the last window disconnects and reconnects within the grace period.
    drop(w2);
    std::thread::sleep(Duration::from_millis(150));
    let w3 = vscode_window(&d);
    std::thread::sleep(Duration::from_millis(1200));
    assert!(!log.exists(), "a window reload must not notify");
    // VS Code really closes: exactly one notice naming the running agent and how to stop it.
    drop(w3);
    for _ in 0..60 {
        if log.exists() { break; }
        std::thread::sleep(Duration::from_millis(100));
    }
    std::thread::sleep(Duration::from_millis(300));
    let text = std::fs::read_to_string(&log).expect("notice sent");
    assert_eq!(text.lines().count(), 1, "{text}");
    assert!(text.starts_with("Overseer: 1 agent still running|generic: /bin/sh -c sleep 30"), "{text}");
    assert!(text.contains("Stop Agents and Daemon"), "{text}");
    let n = notices(&d);
    assert_eq!(n.len(), 1);
    assert_eq!(n[0]["payload"]["runs"][0]["id"], run.as_str());
    // The agent keeps running (unchanged behavior).
    assert_eq!(d.run(&run)["status"], "running");
    d.call("run.interrupt", json!({"run_id": run}));
    d.wait_done(&run, 20);
}

#[test]
fn ac45_no_notice_when_nothing_is_running() {
    let t = tmp();
    let (cmd, log) = notifier(t.path());
    let d = Daemon::start(&[("OVERSEER_BACKGROUND_NOTICE_MS", "300"), ("OVERSEER_NOTIFY_COMMAND", &cmd)]);
    let repo = repo(&t.path().join("r"));
    let done = run_id(&sh(&d, &repo, "worktree", "echo finished"));
    d.wait_done(&done, 20);
    drop(vscode_window(&d));
    std::thread::sleep(Duration::from_millis(1200));
    assert!(!log.exists(), "no notice when nothing is running");
    assert!(notices(&d).is_empty());
    assert!(std::fs::read_to_string(d.home.path().join("overseerd.log")).unwrap_or_default().contains("no active agents, no notice"));
}

#[test]
fn ac45_stop_all_interrupts_runs_forces_stragglers_and_exits_the_daemon() {
    use std::io::{BufRead, BufReader, Write};
    let t = tmp();
    let mut d = Daemon::start(&[]);
    let repo = repo(&t.path().join("r"));
    let polite = run_id(&sh(&d, &repo, "worktree", "sleep 60"));
    // Ignores SIGINT, so interrupt alone cannot stop it.
    let stubborn = run_id(&sh(&d, &repo, "worktree", "trap '' INT; while true; do sleep 1; done"));
    for r in [&polite, &stubborn] {
        d.wait_status(r, |s| s == "running", 20);
    }
    let pids: Vec<i64> = [&polite, &stubborn].iter().flat_map(|r| { let (s, _) = launch_info(&d, r); vec![s["shim_pid"].as_i64().unwrap(), s["child_pid"].as_i64().unwrap()] }).collect();
    // Another window is subscribed; it must learn that the stop was deliberate (so it does not respawn).
    let mut sub = std::os::unix::net::UnixStream::connect(d.socket()).unwrap();
    sub.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
    sub.write_all(format!("{}\n", json!({"id": 1, "method": "events.subscribe", "params": {"after": 0}})).as_bytes()).unwrap();
    let reader = std::thread::spawn(move || {
        let mut seen = Vec::new();
        for line in BufReader::new(sub).lines().map_while(Result::ok) {
            if line.contains("daemon_stopping") { seen.push(line); break; }
        }
        seen
    });
    let result = d.call("daemon.stop_all", json!({}));
    assert_eq!(result["stopped"].as_array().unwrap().len(), 2, "{result}");
    assert!(result["forced"].as_array().unwrap().iter().any(|x| x == stubborn.as_str()), "{result}");
    assert!(result["remaining"].as_array().unwrap().is_empty(), "{result}");
    let exited = (0..50).any(|_| { std::thread::sleep(Duration::from_millis(100)); d.child.as_mut().unwrap().try_wait().unwrap().is_some() });
    assert!(exited, "daemon exits after stop_all");
    for pid in pids {
        assert!(!pid_alive(pid), "process {pid} still alive");
    }
    assert_eq!(reader.join().unwrap().len(), 1, "subscribers get daemon_stopping");
    // Restarting finds both runs stopped, not reattached.
    d.child = None;
    d.spawn();
    for r in [&polite, &stubborn] {
        let st = d.run(r)["status"].as_str().unwrap().to_string();
        assert!(["interrupted", "failed"].contains(&st.as_str()), "{r} {st}");
    }
}

// ---------------------------------------------------------------- AC-43 conversation data

#[test]
fn ac43_fixture_tool_calls_carry_inputs_and_results_for_the_conversation_view() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    // Claude: tool_use input and tool_result output, joined by the tool id; the Agent tool id is the child's native id.
    let d = claude_daemon("nested");
    let created = d.call("task.create", json!({"repo": repo, "harness": "claude", "prompt": "delegate", "title": "nested"}));
    let root = run_id(&created);
    assert_eq!(d.wait_done(&root, 15)["status"], "completed");
    let evs = d.events(&root);
    let tool = evs.iter().find(|e| e["kind"] == "tool" && e["payload"]["id"] == "toolu_child").expect("Agent tool event");
    assert_eq!(tool["payload"]["name"], "Agent");
    let details: Vec<_> = evs.iter().filter(|e| e["kind"] == "tool_result" && e["payload"]["id"] == "toolu_child").collect();
    assert!(details.iter().any(|e| e["payload"]["input"]["description"] == "child task"), "{details:?}");
    assert!(details.iter().any(|e| e["payload"]["output"] == "done" && e["payload"]["status"] == "completed"), "{details:?}");
    let child = d.runs().into_iter().find(|x| x["parent_run_id"] == root.as_str()).unwrap();
    assert_eq!(child["native_id"], "toolu_child", "the conversation nests the child under the tool with this id");
    // Codex app-server: the command's input and final status arrive as tool_result.
    let d = Daemon::start(&[("OVERSEER_CODEX_PATH", &fixture("fake-harness/codex-app-fixture.js"))]);
    let created = d.call("task.create", json!({"repo": repo, "harness": "codex-app", "prompt": "touch", "title": "app"}));
    let run = run_id(&created);
    let waiting = d.wait_status(&run, |s| s == "waiting_for_user", 20);
    d.call("run.permission", json!({"run_id": run, "request_id": waiting["attention"]["request_id"], "allow": true}));
    d.wait_done(&run, 20);
    let evs = d.events(&run);
    let cmd: Vec<_> = evs.iter().filter(|e| e["kind"] == "tool_result" && e["payload"]["id"] == "cmd1").collect();
    assert!(cmd.iter().any(|e| e["payload"]["input"]["command"] == "touch approved.txt"), "{cmd:?}");
    assert!(cmd.iter().any(|e| e["payload"]["status"] == "completed"), "{cmd:?}");
    assert!(evs.iter().any(|e| e["kind"] == "permission_answered"));
}

// ---------------------------------------------------------------- AC-44 merge back

fn ws_id(created: &serde_json::Value) -> String {
    created["workspace"]["id"].as_str().unwrap().to_string()
}

#[test]
fn ac44_clean_merge_back_commits_the_worktree_and_merges_only_on_request() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let d = Daemon::start(&[]);
    let created = sh(&d, &repo, "worktree", "printf 'a\\nagent line\\n' > a.txt; printf 'new\\n' > new.txt");
    let run = run_id(&created);
    d.wait_done(&run, 20);
    let main_before = git(&repo, &["rev-parse", "main"]);
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(git(&repo, &["rev-parse", "main"]), main_before, "never merges automatically");
    let plan = d.call("workspace.merge_plan", json!({"workspace_id": ws_id(&created)}));
    assert_eq!(plan["ok"], true, "{plan}");
    assert_eq!(plan["state"], "idle");
    assert_eq!(plan["target"], "main");
    assert_eq!(plan["worktree_uncommitted"].as_array().unwrap().len(), 2, "{plan}");
    let prep = d.call("workspace.merge_prepare", json!({"workspace_id": ws_id(&created)}));
    assert_eq!(prep["state"], "ready", "{prep}");
    assert_eq!(git(&repo, &["rev-parse", "main"]), main_before, "prepare never touches the target");
    let plan = d.call("workspace.merge_plan", json!({"workspace_id": ws_id(&created)}));
    assert_eq!(plan["can_complete"], true, "{plan}");
    let done = d.call("workspace.merge_complete", json!({"workspace_id": ws_id(&created)}));
    assert_eq!(done["merged"], true);
    assert_eq!(std::fs::read_to_string(repo.join("a.txt")).unwrap(), "a\nagent line\n");
    assert!(repo.join("new.txt").exists());
    assert_eq!(git(&repo, &["status", "--porcelain"]), "");
    assert!(git(&repo, &["log", "-1", "--format=%s"]).contains("Overseer merge back"));
    assert!(d.events(&run).iter().any(|e| e["kind"] == "merge_back" && e["payload"]["state"] == "merged"));
    // Afterwards there is nothing left to merge.
    let again = d.call("workspace.merge_plan", json!({"workspace_id": ws_id(&created)}));
    assert_eq!(again["ok"], false);
    assert!(again["reason"].as_str().unwrap().contains("Nothing to merge"), "{again}");
}

#[test]
fn ac44_conflicts_are_resolved_in_the_worktree_before_the_target_changes() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let d = Daemon::start(&[]);
    let created = sh(&d, &repo, "worktree", "printf 'agent version\\n' > a.txt");
    d.wait_done(&run_id(&created), 20);
    // The target moved on meanwhile, touching the same line.
    std::fs::write(repo.join("a.txt"), "main version\n").unwrap();
    git(&repo, &["commit", "-qam", "main edit"]);
    let main_before = git(&repo, &["rev-parse", "main"]);
    let id = ws_id(&created);
    let prep = d.call("workspace.merge_prepare", json!({"workspace_id": id, "handoff": true}));
    assert_eq!(prep["state"], "conflicts", "{prep}");
    assert_eq!(prep["files"], json!(["a.txt"]));
    assert_eq!(prep["handoff"]["sent"], false, "generic runs cannot take follow-ups: {prep}");
    assert_eq!(git(&repo, &["rev-parse", "main"]), main_before);
    assert_eq!(d.call("workspace.merge_plan", json!({"workspace_id": id}))["state"], "resolving");
    // Still conflicted: refuses to finish.
    let still = d.call("workspace.merge_resolved", json!({"workspace_id": id}));
    assert_eq!(still["state"], "resolving");
    assert!(d.try_call("workspace.merge_complete", json!({"workspace_id": id})).is_err());
    // The agent (here: the test) resolves the file; Overseer stages it and finishes the worktree merge.
    let ws = ws_path(&d, &created);
    std::fs::write(ws.join("a.txt"), "main version\nagent version\n").unwrap();
    assert_eq!(d.call("workspace.merge_resolved", json!({"workspace_id": id}))["state"], "ready");
    let done = d.call("workspace.merge_complete", json!({"workspace_id": id}));
    assert_eq!(done["merged"], true);
    assert_eq!(std::fs::read_to_string(repo.join("a.txt")).unwrap(), "main version\nagent version\n");
    assert_eq!(git(&repo, &["status", "--porcelain"]), "");
}

#[test]
fn ac44_refuses_dirty_target_active_runs_and_current_checkout_tasks() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let d = Daemon::start(&[]);
    let created = sh(&d, &repo, "worktree", "printf 'agent\\n' > b.txt");
    d.wait_done(&run_id(&created), 20);
    let id = ws_id(&created);
    // Dirty target checkout: refused, explained, untouched.
    std::fs::write(repo.join("a.txt"), "user's unsaved work\n").unwrap();
    std::fs::write(repo.join("scratch.txt"), "untracked\n").unwrap();
    let before = fingerprint(&repo);
    assert_eq!(d.call("workspace.merge_prepare", json!({"workspace_id": id}))["state"], "ready");
    let plan = d.call("workspace.merge_plan", json!({"workspace_id": id}));
    assert_eq!(plan["can_complete"], false);
    assert!(plan["blockers"][0].as_str().unwrap().contains("uncommitted changes (a.txt)"), "{plan}");
    let err = d.try_call("workspace.merge_complete", json!({"workspace_id": id})).unwrap_err();
    assert!(err.contains("never disturbs"), "{err}");
    assert_eq!(fingerprint(&repo), before, "dirty target left untouched");
    // Target checkout on another branch: refused with an explanation.
    git(&repo, &["checkout", "-q", "--", "a.txt"]);
    git(&repo, &["switch", "-q", "-c", "elsewhere"]);
    let err = d.try_call("workspace.merge_complete", json!({"workspace_id": id})).unwrap_err();
    assert!(err.contains("switch it to main"), "{err}");
    git(&repo, &["switch", "-q", "main"]);
    // Active run: refused.
    let busy = sh(&d, &repo, "worktree", "sleep 30");
    d.wait_status(&run_id(&busy), |s| s == "running", 20);
    let plan = d.call("workspace.merge_plan", json!({"workspace_id": ws_id(&busy)}));
    assert_eq!(plan["ok"], false);
    assert!(plan["reason"].as_str().unwrap().contains("still running"), "{plan}");
    d.call("run.interrupt", json!({"run_id": run_id(&busy)}));
    // Current-checkout task: nothing to merge back.
    let cur = sh(&d, &repo, "current", "true");
    d.wait_done(&run_id(&cur), 20);
    let plan = d.call("workspace.merge_plan", json!({"workspace_id": ws_id(&cur)}));
    assert_eq!(plan["ok"], false);
    assert!(plan["reason"].as_str().unwrap().contains("current checkout"), "{plan}");
}

// ---------------------------------------------------------------- AC-46 / AC-11 accounts (fixture CLI)

struct AccountLab { d: Daemon, _t: tempfile::TempDir, sys: std::path::PathBuf, next: std::path::PathBuf }

fn account_lab() -> AccountLab {
    let t = tmp();
    let sys = t.path().join("desktop-home");
    std::fs::create_dir_all(&sys).unwrap();
    let next = t.path().join("next-login");
    let cli = fixture("fake-harness/account-cli.js");
    let d = Daemon::start(&[("OVERSEER_CODEX_PATH", &cli), ("OVERSEER_CLAUDE_PATH", &cli), ("OVERSEER_TEST_SYSTEM_HOME", sys.to_str().unwrap()),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_LOGIN_ACCOUNT_FILE,OVERSEER_TEST_SYSTEM_HOME"), ("FIXTURE_LOGIN_ACCOUNT_FILE", next.to_str().unwrap())]);
    AccountLab { d, _t: t, sys, next }
}

impl AccountLab {
    /// Runs the account's own sign-in command the way the UI's terminal does, as `who`.
    fn sign_in(&self, id: &str, who: &str) {
        std::fs::write(&self.next, who).unwrap();
        let cmd = self.d.call("profile.login_command", json!({"id": id}));
        let mut c = std::process::Command::new(cmd["program"].as_str().unwrap());
        c.args(cmd["args"].as_array().unwrap().iter().map(|a| a.as_str().unwrap())).env("FIXTURE_LOGIN_ACCOUNT_FILE", &self.next);
        for (k, v) in cmd["env"].as_object().unwrap() { c.env(k, v.as_str().unwrap()); }
        assert!(c.status().unwrap().success());
    }
    fn fp(&self, id: &str) -> (bool, String, String) {
        let st = self.d.call("profile.status", json!({"id": id}));
        let idn = &st["identity"];
        let fp = idn["account_fingerprint"].as_str().or(idn["fingerprint"].as_str()).unwrap_or("").to_string();
        (st["logged_in"] == true, fp, idn["plan"].as_str().unwrap_or("").to_string())
    }
}

#[test]
fn ac46_accounts_by_provider_fixed_vs_desktop_linked_and_isolated_resign_in_and_removal() {
    let lab = account_lab();
    let d = &lab.d;
    let list = d.call("account.list", json!({}));
    let providers: Vec<_> = list["providers"].as_array().unwrap().iter().map(|p| (p["id"].as_str().unwrap().to_string(), p["available"] == true)).collect();
    assert_eq!(providers.iter().map(|p| p.0.as_str()).collect::<Vec<_>>(), ["openai", "anthropic", "local", "devin"]);
    assert!(!providers[3].1, "Devin has no account login");
    assert!(d.try_call("account.create", json!({"provider": "devin", "name": "x"})).is_err());
    let sys_codex = list["accounts"].as_array().unwrap().iter().find(|a| a["id"] == "system-codex").unwrap().clone();
    assert_eq!(sys_codex["kind"], "follows-app");
    assert_eq!(sys_codex["harnesses"], json!(["codex", "codex-app"]));
    // Add one fixed account per available provider; their folders exist immediately (AC-11).
    let work = d.call("account.create", json!({"provider": "openai", "name": "Work ChatGPT"}))["account"].clone();
    let claude = d.call("account.create", json!({"provider": "anthropic", "name": "Claude fixed"}))["account"].clone();
    let (work_id, claude_id) = (work["id"].as_str().unwrap().to_string(), claude["id"].as_str().unwrap().to_string());
    use std::os::unix::fs::PermissionsExt;
    for (acct, sub) in [(&work, "codex"), (&claude, "claude")] {
        let dir = std::path::Path::new(acct["home"].as_str().unwrap()).join(sub);
        assert!(dir.is_dir(), "{} created at creation time", dir.display());
        assert_eq!(std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777, 0o700);
    }
    assert_eq!(lab.fp(&work_id).0, false, "missing login reported");
    lab.sign_in(&work_id, "work:team");
    lab.sign_in(&claude_id, "claudia:max");
    lab.sign_in("system-codex", "desk1:pro"); // the desktop app's own login
    let (w_ok, w_fp, w_plan) = lab.fp(&work_id);
    let (c_ok, c_fp, c_plan) = lab.fp(&claude_id);
    let (_, d1, _) = lab.fp("system-codex");
    assert!(w_ok && c_ok && w_plan == "team" && c_plan == "max" && !w_fp.is_empty() && !c_fp.is_empty());
    assert_ne!(w_fp, d1);
    // The desktop app switches accounts: the linked account follows, the fixed one does not.
    lab.sign_in("system-codex", "desk2:plus");
    let (_, d2, _) = lab.fp("system-codex");
    assert_ne!(d1, d2, "desktop-linked account follows the app");
    assert_eq!(lab.fp(&work_id).1, w_fp, "fixed account unchanged by the desktop switch");
    // Re-sign-in affects only that account.
    d.call("profile.logout", json!({"id": work_id}));
    assert_eq!(lab.fp(&work_id).0, false);
    assert_eq!(lab.fp(&claude_id), (true, c_fp.clone(), c_plan.clone()));
    assert_eq!(lab.fp("system-codex").1, d2);
    lab.sign_in(&work_id, "work2:plus");
    let (_, w2, _) = lab.fp(&work_id);
    assert_ne!(w2, w_fp);
    assert_eq!(lab.fp("system-codex").1, d2);
    // Removal affects only that account; desktop logins cannot be removed or signed out.
    let claude_home = std::path::PathBuf::from(claude["home"].as_str().unwrap());
    d.call("account.remove", json!({"id": claude_id}));
    assert!(!claude_home.exists());
    assert_eq!(lab.fp(&work_id).1, w2);
    assert_eq!(lab.fp("system-codex").1, d2);
    assert!(lab.sys.join(".codex/auth.json").exists());
    assert!(d.try_call("account.remove", json!({"id": "system-codex"})).unwrap_err().contains("never removes"));
    assert!(d.try_call("profile.logout", json!({"id": "system-codex"})).is_err());
    let ids: Vec<String> = d.call("account.list", json!({}))["accounts"].as_array().unwrap().iter().map(|a| a["id"].as_str().unwrap().to_string()).collect();
    assert!(!ids.contains(&claude_id) && ids.contains(&work_id));
    // Credentials never enter the database or events.
    let db = std::fs::read(d.home.path().join("overseer.sqlite")).unwrap();
    assert!(!String::from_utf8_lossy(&db).contains("\"access_token\""));
}

// ---------------------------------------------------------------- AC-08 foreign peers

#[test]
fn ac08_connections_from_a_foreign_uid_are_rejected_and_logged() {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::fs::PermissionsExt;
    // Test-only override: the daemon treats uid 4242424 as its owner, so this test process
    // (the real owner) is a foreign peer. The override can only reject more, never admit more.
    let home = tempfile::Builder::new().prefix("ovs-t").tempdir_in("/tmp").unwrap();
    let mut child = std::process::Command::new(BIN).arg("serve").env("OVERSEER_HOME", home.path()).env("OVERSEER_TEST_EXPECT_UID", "4242424")
        .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn().unwrap();
    let sock = std::path::PathBuf::from(String::from_utf8(std::process::Command::new(BIN).arg("socket-path").env("OVERSEER_HOME", home.path()).output().unwrap().stdout).unwrap().trim());
    for _ in 0..100 { if sock.exists() { break; } std::thread::sleep(Duration::from_millis(50)); }
    assert_eq!(std::fs::metadata(&sock).unwrap().permissions().mode() & 0o777, 0o600, "socket is owner-only");
    assert_eq!(std::fs::metadata(sock.parent().unwrap()).unwrap().permissions().mode() & 0o777, 0o700, "socket directory is owner-only");
    let mut conn = std::os::unix::net::UnixStream::connect(&sock).unwrap();
    conn.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let _ = conn.write_all(format!("{}\n", json!({"id": 1, "method": "task.create", "params": {"repo": "/tmp", "harness": "generic", "program": "/usr/bin/touch", "args": ["/tmp/ovs-ac08-should-not-exist"]}})).as_bytes());
    let mut line = String::new();
    let n = BufReader::new(conn).read_line(&mut line).unwrap_or(0);
    assert_eq!(n, 0, "no reply to a foreign peer: {line}");
    assert!(!std::path::Path::new("/tmp/ovs-ac08-should-not-exist").exists(), "nothing executed");
    let uid = unsafe { libc_getuid() };
    let log = std::fs::read_to_string(home.path().join("overseerd.log")).unwrap_or_default();
    assert!(log.contains(&format!("rejected connection from uid Some({uid})")), "{log}");
    let _ = child.kill();
    let _ = child.wait();
}

extern "C" {
    #[link_name = "getuid"]
    fn libc_getuid() -> u32;
}

// ---------------------------------------------------------------- AC-51 worktree file tree

#[test]
fn ac51_worktree_tree_lists_one_directory_marks_changes_and_stays_inside() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    std::fs::create_dir_all(repo.join("src/deep")).unwrap();
    std::fs::write(repo.join("src/deep/x.txt"), "x\n").unwrap();
    std::fs::write(repo.join("src/keep.txt"), "k\n").unwrap();
    std::fs::write(repo.join("gone.txt"), "g\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-qm", "more"]);
    let d = Daemon::start(&[]);
    let created = sh(&d, &repo, "worktree", "printf 'changed\\n' > src/deep/x.txt; printf 'new\\n' > added.txt; rm gone.txt");
    d.wait_done(&run_id(&created), 20);
    let id = ws_id(&created);
    let root = d.call("workspace.tree", json!({"workspace_id": id}));
    let names: Vec<&str> = root["entries"].as_array().unwrap().iter().map(|e| e["name"].as_str().unwrap()).collect();
    assert_eq!(names[0], "src", "directories first: {names:?}");
    assert!(!names.contains(&".git"));
    let find = |v: &serde_json::Value, n: &str| v["entries"].as_array().unwrap().iter().find(|e| e["name"] == n).cloned().unwrap();
    assert_eq!(find(&root, "src")["changes_inside"], 1);
    assert_eq!(find(&root, "added.txt")["status"], "A");
    assert_eq!(find(&root, "gone.txt")["status"], "D");
    assert_eq!(find(&root, "gone.txt")["deleted"], true);
    assert!(find(&root, "a.txt")["status"].is_null());
    let deep = d.call("workspace.tree", json!({"workspace_id": id, "dir": "src/deep"}));
    assert_eq!(find(&deep, "x.txt")["status"], "M");
    assert_eq!(find(&deep, "x.txt")["path"], "src/deep/x.txt");
    for bad in ["..", "../..", "/etc", ".git", "src/../../x"] {
        assert!(d.try_call("workspace.tree", json!({"workspace_id": id, "dir": bad})).is_err(), "{bad} must be refused");
    }
    // Large directory: capped, counted, and fast.
    let big = ws_path(&d, &created).join("big");
    std::fs::create_dir_all(&big).unwrap();
    for i in 0..6000 { std::fs::write(big.join(format!("f{i:05}.txt")), "").unwrap(); }
    let t0 = std::time::Instant::now();
    let listing = d.call("workspace.tree", json!({"workspace_id": id, "dir": "big"}));
    assert!(t0.elapsed() < Duration::from_secs(2), "{:?}", t0.elapsed());
    assert_eq!(listing["total"], 6000);
    assert_eq!(listing["truncated"], true);
    assert_eq!(listing["entries"].as_array().unwrap().len(), 5000);
}

// ---------------------------------------------------------------- AC-52 native notifications (fake helper)

/// A fake `Overseer Notifier.app` whose executable logs its arguments and exits with `code`.
fn fake_notifier(dir: &Path, code: i32) -> (std::path::PathBuf, std::path::PathBuf) {
    let app = dir.join(format!("Fake{code}.app"));
    let bin = app.join("Contents/MacOS");
    std::fs::create_dir_all(&bin).unwrap();
    let log = dir.join(format!("notifier-{code}.log"));
    std::fs::write(bin.join("notifier"), format!("#!/bin/sh\nprintf '%s\\n' \"$@\" >> '{}'\nexit {code}\n", log.display())).unwrap();
    std::process::Command::new("chmod").arg("+x").arg(bin.join("notifier")).status().unwrap();
    (app, log)
}

#[test]
fn ac52_notifications_use_the_overseer_helper_and_fall_back_when_denied_or_missing() {
    let t = tmp();
    let (fallback, fallback_log) = notifier(t.path()); // records fallback deliveries instead of osascript
    // Helper present and allowed.
    let (ok_app, ok_log) = fake_notifier(t.path(), 0);
    let d = Daemon::start(&[("OVERSEER_TEST_NOTIFIER_DIRECT", "1"), ("OVERSEER_NOTIFIER_APP", ok_app.to_str().unwrap()), ("OVERSEER_NOTIFY_FALLBACK", &fallback)]);
    assert_eq!(d.call("daemon.test_notice", json!({}))["delivered_via"], "overseer-notifier (ok)");
    let args = std::fs::read_to_string(&ok_log).unwrap();
    assert!(args.contains("--title\nOverseer notifications are on\n--body\n"), "{args}");
    assert!(args.contains("--open\nvscode://beelol.overseer/open-center"), "clicks open the Overseer view: {args}");
    assert!(!fallback_log.exists(), "no fallback when the helper posted");
    drop(d);
    // Helper present but notifications denied: fall back, and say so.
    let (denied_app, _) = fake_notifier(t.path(), 3);
    let d = Daemon::start(&[("OVERSEER_TEST_NOTIFIER_DIRECT", "1"), ("OVERSEER_NOTIFIER_APP", denied_app.to_str().unwrap()), ("OVERSEER_NOTIFY_FALLBACK", &fallback)]);
    let via = d.call("daemon.test_notice", json!({}))["delivered_via"].as_str().unwrap().to_string();
    assert_eq!(via, format!("overseer-notifier (denied); fell back to {fallback} (ok)"));
    assert!(std::fs::read_to_string(&fallback_log).unwrap().contains("Overseer notifications are on"));
    drop(d);
    // No permission answer yet (exit 5) and helper missing: both fall back.
    let (pending_app, _) = fake_notifier(t.path(), 5);
    let d = Daemon::start(&[("OVERSEER_TEST_NOTIFIER_DIRECT", "1"), ("OVERSEER_NOTIFIER_APP", pending_app.to_str().unwrap()), ("OVERSEER_NOTIFY_FALLBACK", &fallback)]);
    assert!(d.call("daemon.test_notice", json!({}))["delivered_via"].as_str().unwrap().starts_with("overseer-notifier (permission not answered yet); fell back to"));
    drop(d);
    let d = Daemon::start(&[("OVERSEER_TEST_NOTIFIER_DIRECT", "1"), ("OVERSEER_NOTIFIER_APP", "/nonexistent/Overseer Notifier.app"), ("OVERSEER_NOTIFY_FALLBACK", &fallback)]);
    assert!(d.call("daemon.test_notice", json!({}))["delivered_via"].as_str().unwrap().starts_with("overseer-notifier (not installed); fell back to"));
}

#[test]
fn ac52_background_notice_is_delivered_by_the_helper() {
    let t = tmp();
    let (fallback, _) = notifier(t.path());
    let (app, log) = fake_notifier(t.path(), 0);
    let d = Daemon::start(&[("OVERSEER_TEST_NOTIFIER_DIRECT", "1"), ("OVERSEER_NOTIFIER_APP", app.to_str().unwrap()), ("OVERSEER_NOTIFY_FALLBACK", &fallback), ("OVERSEER_BACKGROUND_NOTICE_MS", "300")]);
    let repo = repo(&t.path().join("r"));
    let run = run_id(&sh(&d, &repo, "worktree", "sleep 30"));
    d.wait_status(&run, |s| s == "running", 20);
    drop(vscode_window(&d));
    let mut n = vec![];
    for _ in 0..50 { n = notices(&d); if !n.is_empty() { break; } std::thread::sleep(Duration::from_millis(100)); }
    assert_eq!(n.len(), 1);
    assert_eq!(n[0]["payload"]["delivered_via"], "overseer-notifier (ok)");
    assert!(std::fs::read_to_string(&log).unwrap().contains("Overseer: 1 agent still running"));
    d.call("run.interrupt", json!({"run_id": run}));
    d.wait_done(&run, 20);
}

#[test]
fn ac52_the_daemon_finds_the_notifier_app_next_to_its_own_binary() {
    // The installed layout: bin/overseerd-<platform> and bin/Overseer Notifier.app side by side.
    let t = tmp();
    let bin = t.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let daemon = bin.join("overseerd-test");
    std::fs::copy(BIN, &daemon).unwrap();
    let (fake, log) = fake_notifier(t.path(), 0);
    std::fs::rename(&fake, bin.join("Overseer Notifier.app")).unwrap();
    let home = t.path().join("home");
    let mut child = std::process::Command::new(&daemon).arg("serve").env("OVERSEER_HOME", &home).env("OVERSEER_TEST_NOTIFIER_DIRECT", "1").env_remove("OVERSEER_NOTIFIER_APP")
        .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn().unwrap();
    let ctl = |m: &str| String::from_utf8(std::process::Command::new(&daemon).args(["ctl", m, "{}"]).env("OVERSEER_HOME", &home).output().unwrap().stdout).unwrap();
    let mut out = String::new();
    for _ in 0..50 { out = ctl("daemon.test_notice"); if out.contains("delivered_via") { break; } std::thread::sleep(Duration::from_millis(100)); }
    assert!(out.contains("overseer-notifier (ok)"), "{out}");
    assert!(std::fs::read_to_string(&log).unwrap().contains("--open\nvscode://beelol.overseer/open-center"));
    let _ = child.kill();
    let _ = child.wait();
}

// ---------------------------------------------------------------- AC-50 open a pull request

#[test]
fn ac50_pr_plan_explains_refusals_and_prepares_a_github_branch_without_merging() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let d = Daemon::start(&[]);
    let created = sh(&d, &repo, "worktree", "printf 'agent line\\n' >> a.txt");
    d.wait_done(&run_id(&created), 20);
    let id = ws_id(&created);
    // No remote.
    let plan = d.call("workspace.pr_plan", json!({"workspace_id": id}));
    assert_eq!(plan["ok"], false);
    assert!(plan["reason"].as_str().unwrap().contains("has no Git remote"), "{plan}");
    // A non-GitHub remote.
    git(&repo, &["remote", "add", "origin", "https://gitlab.example.invalid/a/b.git"]);
    let plan = d.call("workspace.pr_plan", json!({"workspace_id": id}));
    assert!(plan["reason"].as_str().unwrap().contains("not on GitHub"), "{plan}");
    // A GitHub remote whose pushes go to a local bare repository (insteadOf), as in the UI test.
    let bare = r.path().join("remote.git");
    std::process::Command::new("git").args(["init", "-q", "--bare", bare.to_str().unwrap()]).status().unwrap();
    git(&repo, &["remote", "set-url", "origin", "https://github.com/test-owner/test-repo.git"]);
    git(&repo, &["config", &format!("url.{}.insteadOf", bare.display()), "https://github.com/test-owner/test-repo.git"]);
    let plan = d.call("workspace.pr_plan", json!({"workspace_id": id}));
    assert_eq!(plan["ok"], true, "{plan}");
    assert_eq!((plan["owner"].as_str(), plan["repo"].as_str(), plan["target"].as_str()), (Some("test-owner"), Some("test-repo"), Some("main")));
    assert_eq!(plan["uncommitted"], json!(["a.txt"]));
    let main_before = git(&repo, &["rev-parse", "main"]);
    let prep = d.call("workspace.pr_prepare", json!({"workspace_id": id}));
    assert_eq!(prep["committed"], true);
    assert_eq!(prep["files"][0]["path"], "a.txt");
    assert!(prep["commits"][0].as_str().unwrap().starts_with("Overseer: "), "{prep}");
    assert_eq!(git(&repo, &["rev-parse", "main"]), main_before, "nothing is merged");
    d.call("workspace.pr_opened", json!({"workspace_id": id, "url": "https://github.com/test-owner/test-repo/pull/7", "number": 7}));
    assert!(d.events(&run_id(&created)).iter().any(|e| e["kind"] == "pull_request" && e["payload"]["number"] == 7));
    assert!(d.try_call("workspace.pr_opened", json!({"workspace_id": id, "url": "javascript:alert(1)", "number": 1})).is_err());
    // Active run and current checkout: refused with an explanation.
    let busy = sh(&d, &repo, "worktree", "sleep 30");
    d.wait_status(&run_id(&busy), |s| s == "running", 20);
    assert!(d.call("workspace.pr_plan", json!({"workspace_id": ws_id(&busy)}))["reason"].as_str().unwrap().contains("still running"));
    d.call("run.interrupt", json!({"run_id": run_id(&busy)}));
    let cur = sh(&d, &repo, "current", "true");
    d.wait_done(&run_id(&cur), 20);
    assert!(d.call("workspace.pr_plan", json!({"workspace_id": ws_id(&cur)}))["reason"].as_str().unwrap().contains("current checkout"));
}

#[test]
fn ac50_pr_plan_targets_the_branch_name_in_a_fresh_clone() {
    // Found live: in a fresh clone the default branch is the remote-tracking `origin/master`, and the
    // plan passed that to GitHub as the base, which GitHub rejects. The target must be the branch name.
    let r = tmp();
    let src = repo(&r.path().join("src"));
    let bare = r.path().join("remote.git");
    std::process::Command::new("git").args(["clone", "-q", "--bare", src.to_str().unwrap(), bare.to_str().unwrap()]).status().unwrap();
    let clone = r.path().join("clone");
    std::process::Command::new("git").args(["clone", "-q", bare.to_str().unwrap(), clone.to_str().unwrap()]).status().unwrap();
    git(&clone, &["remote", "set-url", "origin", "https://github.com/test-owner/test-repo.git"]);
    git(&clone, &["config", &format!("url.{}.insteadOf", bare.display()), "https://github.com/test-owner/test-repo.git"]);
    assert_eq!(git(&clone, &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"]), "origin/main");
    let d = Daemon::start(&[]);
    let created = sh(&d, &clone, "worktree", "printf 'agent line\\n' >> a.txt");
    d.wait_done(&run_id(&created), 20);
    let plan = d.call("workspace.pr_plan", json!({"workspace_id": ws_id(&created)}));
    assert_eq!(plan["ok"], true, "{plan}");
    assert_eq!(plan["target"], "main", "{plan}");
    let prep = d.call("workspace.pr_prepare", json!({"workspace_id": ws_id(&created)}));
    assert_eq!(prep["files"][0]["path"], "a.txt", "{prep}");
    assert_eq!(prep["commits"].as_array().map(Vec::len), Some(1), "only the run's commit, compared against main: {prep}");
}

#[test]
fn auto_usage_is_local_bounded_and_clear_does_not_erase_run_history() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let replay = r.path().join("auto-usage.jsonl");
    std::fs::write(&replay, concat!(
        "{\"type\":\"thread.started\",\"thread_id\":\"auto-usage-thread\"}\n",
        "{\"type\":\"turn.started\"}\n",
        "{\"type\":\"turn.completed\",\"usage\":{\"input_tokens\":42,\"output_tokens\":7,\"prompt\":\"secret-prompt-sentinel\"}}\n"
    )).unwrap();
    let d = Daemon::start(&[("OVERSEER_CODEX_PATH", &fixture("fake-harness/replay.js")),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "REPLAY_FILE,REPLAY_DELAY_MS"),
        ("REPLAY_FILE", replay.to_str().unwrap()), ("REPLAY_DELAY_MS", "10")]);
    let created = d.call("task.create", json!({"repo": repo, "harness": "codex", "model":"gpt-6-sol", "effort":"medium", "prompt": "x", "title": "usage"}));
    let run = run_id(&created);
    assert_eq!(d.wait_done(&run, 15)["status"], "completed");
    let result = d.call("auto.usage.list", json!({"limit": 10}));
    let rows = result["measurements"].as_array().unwrap();
    assert_eq!(rows.len(), 1, "{result}");
    assert_eq!(rows[0]["input_tokens"], 42);
    assert_eq!(rows[0]["output_tokens"], 7);
    assert_eq!(rows[0]["effort"], "medium");
    assert!(!result.to_string().contains("secret-prompt-sentinel"));
    let summary = d.call("auto.usage.summary", json!({"limit": 10}));
    assert_eq!(summary["aggregates"].as_array().unwrap().len(), 1);
    assert_eq!(summary["aggregates"][0]["samples"], 1);
    assert_eq!(summary["aggregates"][0]["input_tokens"], 42);
    assert_eq!(summary["aggregates"][0]["effort"], "medium");
    assert!(!summary.to_string().contains("secret-prompt-sentinel"));
    let export_path = r.path().join("auto-usage-export.json");
    let exported = d.call("auto.usage.export", json!({"path": export_path}));
    assert_eq!(exported["count"], 1);
    let exported_text = std::fs::read_to_string(&export_path).unwrap();
    assert!(exported_text.contains("\"input_tokens\": 42"));
    assert!(exported_text.contains("\"aggregates\""));
    assert!(!exported_text.contains("secret-prompt-sentinel"));
    assert_eq!(d.call("auto.usage.clear", json!({}))["deleted"], 1);
    assert!(d.call("auto.usage.list", json!({}))["measurements"].as_array().unwrap().is_empty());
    assert!(d.call("auto.usage.summary", json!({}))["aggregates"].as_array().unwrap().is_empty());
    assert_eq!(d.run(&run)["status"], "completed");
    assert!(d.events(&run).iter().any(|e| e["kind"] == "usage"));
}

#[test]
fn auto_usage_inspection_expires_idle_history_without_restarting_daemon() {
    use rusqlite::params;
    let d = Daemon::start(&[]);
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64;
    const DAY: i64 = 86_400_000;
    let learning = rusqlite::Connection::open(d.home.path().join("overseer.sqlite.learning")).unwrap();
    for (id, observed) in [(1, now - 31 * DAY), (2, now - 29 * DAY)] {
        learning.execute("INSERT INTO auto_measurements(event_seq,observed_ms,task_id,run_id,harness) VALUES(?1,?2,'task','run','codex')",
            params![id, observed]).unwrap();
        learning.execute("INSERT INTO auto_thread_usage_observations(run_id,profile_id,read_account_generation,attribution,observed_ms,source,estimate) VALUES('run','profile',1,'unverified',?1,'fixture','{}')",
            params![observed]).unwrap();
    }
    for (day, observed) in [(1, now - 91 * DAY), (2, now - 89 * DAY)] {
        learning.execute("INSERT INTO auto_daily_aggregates(day_ms,harness,profile_id,model,effort,last_observed_ms,samples,input_observations,input_tokens,output_observations,output_tokens,cached_input_observations,cached_input_tokens,reasoning_output_observations,reasoning_output_tokens,cost_observations,cost_usd) VALUES(?1,'codex','','','',?2,1,0,0,0,0,0,0,0,0,0,0)",
            params![day, observed]).unwrap();
    }

    let detail = d.call("auto.usage.list", json!({}));
    assert_eq!(detail["measurements"].as_array().unwrap().len(), 1, "{detail}");
    let summary = d.call("auto.usage.summary", json!({}));
    assert_eq!(summary["aggregates"].as_array().unwrap().len(), 1, "{summary}");
    for table in ["auto_measurements", "auto_daily_aggregates", "auto_thread_usage_observations"] {
        let count: i64 = learning.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| row.get(0)).unwrap();
        assert_eq!(count, 1, "idle inspection must remove expired {table} rows from disk");
    }
}

#[test]
fn auto_usage_inspection_reports_paused_when_retention_cleanup_fails_then_recovers() {
    use rusqlite::params;
    let d = Daemon::start(&[]);
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64;
    let learning = rusqlite::Connection::open(d.home.path().join("overseer.sqlite.learning")).unwrap();
    learning.execute("INSERT INTO auto_measurements(event_seq,observed_ms,task_id,run_id,harness) VALUES(1,?1,'task','run','codex')",
        params![now - 31 * 86_400_000_i64]).unwrap();
    learning.execute_batch("CREATE TRIGGER reject_idle_cleanup BEFORE DELETE ON auto_measurements
        BEGIN SELECT RAISE(FAIL,'injected retention cleanup failure'); END;").unwrap();

    let paused = d.call("auto.usage.list", json!({}));
    assert_eq!(paused["learning_paused"], true, "{paused}");
    assert!(paused["measurements"].as_array().unwrap().is_empty(),
        "expired learning must not leak when maintenance fails: {paused}");
    for (method, field) in [
        ("auto.usage.summary", "aggregates"),
        ("auto.usage.thread.list", "observations"),
        ("auto.usage.work.list", "work_units"),
    ] {
        let response = d.call(method, json!({}));
        assert_eq!(response["learning_paused"], true, "{method}: {response}");
        assert!(response[field].as_array().unwrap().is_empty(), "{method}: {response}");
    }
    let export = d.home.path().join("expired-learning-export.json");
    assert!(d.try_call("auto.usage.export", json!({"path":export})).is_err());
    assert!(!export.exists(), "failed maintenance must not export expired history");

    learning.execute_batch("DROP TRIGGER reject_idle_cleanup").unwrap();
    let recovered = d.call("auto.usage.list", json!({}));
    assert_eq!(recovered["learning_paused"], false, "{recovered}");
    assert!(recovered["measurements"].as_array().unwrap().is_empty());
    assert_eq!(learning.query_row("SELECT COUNT(*) FROM auto_measurements", [], |row| row.get::<_, i64>(0)).unwrap(), 0);
}

#[test]
fn auto_clearing_learning_during_an_active_child_preserves_its_execution() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let trace = r.path().join("learning-clear-trace.txt");
    let mut d = Daemon::start(&[("OVERSEER_CODEX_PATH", &fixture("fake-harness/codex-app-fixture.js")),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH",
            "FIXTURE_MODE,FIXTURE_TRACE_FILE,FIXTURE_EMIT_USAGE,FIXTURE_TURN_DELAY_MS"),
        ("FIXTURE_MODE", "managed-models"), ("FIXTURE_TRACE_FILE", trace.to_str().unwrap()),
        ("FIXTURE_EMIT_USAGE", "1"), ("FIXTURE_TURN_DELAY_MS", "6000")]);
    let parent = run_id(&d.call("task.create", json!({"repo":repo,"harness":"codex-app",
        "model":"gpt-6-astra","effort":"high","prompt":"seed context"})));
    assert_eq!(d.wait_done(&parent, 15)["status"], "completed");
    assert_eq!(d.call("auto.usage.list", json!({}))["measurements"].as_array().unwrap().len(), 1);

    let selected = d.call("auto.dispatch", json!({"work_unit_id":"clear-while-child-runs",
        "parent_run_id":parent,"min_tier":"general","required_tools":[],
        "prompt":"browser check"}));
    assert_eq!(selected["state"], "dispatched", "{selected}");
    let child = run_id(&selected);
    d.wait_status(&child, |status| status == "running", 10);
    let workspace_id = selected["workspace"]["id"].as_str().unwrap();
    let before = d.call("state", json!({}));
    assert_eq!(before["workspaces"].as_array().unwrap().iter()
        .find(|workspace| workspace["id"] == workspace_id).unwrap()["owner_run_id"], child);

    assert_eq!(d.call("auto.usage.clear", json!({}))["deleted"], 1);
    assert!(d.call("auto.usage.list", json!({}))["measurements"].as_array().unwrap().is_empty());
    assert!(d.call("auto.usage.summary", json!({}))["aggregates"].as_array().unwrap().is_empty());
    assert_eq!(d.run(&child)["status"], "running");
    let during = d.call("state", json!({}));
    assert_eq!(during["workspaces"].as_array().unwrap().iter()
        .find(|workspace| workspace["id"] == workspace_id).unwrap()["owner_run_id"], child);

    assert_eq!(d.wait_done(&child, 15)["status"], "completed");
    assert_eq!(d.call("run.result", json!({"run_id":child}))["state"], "ready");
    let new_samples = d.call("auto.usage.list", json!({}));
    assert_eq!(new_samples["measurements"].as_array().unwrap().len(), 1, "{new_samples}");
    assert_eq!(new_samples["measurements"][0]["run_id"], child);
    assert_eq!(d.call("auto.usage.summary", json!({}))["aggregates"][0]["samples"], 1);
    assert_eq!(d.events(&parent).into_iter().filter(|event|
        event["kind"] == "managed_child_result_available").count(), 1);
    assert_eq!(std::fs::read_to_string(&trace).unwrap().matches("turn_model:gpt-6-sol").count(), 1);
    d.kill9();
    d.spawn();
    assert_eq!(d.call("auto.usage.list", json!({}))["measurements"].as_array().unwrap().len(), 1,
        "reopening must not regenerate the cleared parent sample from old events");
    assert_eq!(d.run(&parent)["status"], "completed");
    assert_eq!(d.run(&child)["status"], "completed");
}

#[cfg(target_os = "macos")]
#[test]
fn auto_local_usage_records_and_aggregates_with_egress_denied() {
    let profile = "(version 1) (allow default) (deny network-outbound)";
    let denied = std::process::Command::new("/usr/bin/sandbox-exec")
        .args(["-p", profile, "python3", "-c",
            "import socket; s=socket.socket(); print(s.connect_ex(('127.0.0.1', 9)))"])
        .output().unwrap();
    assert!(denied.status.success(), "sandbox preflight failed: {}",
        String::from_utf8_lossy(&denied.stderr));
    assert_eq!(String::from_utf8_lossy(&denied.stdout).trim(), "1",
        "the sandbox must reject outbound connections before this test can prove anything");

    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let replay = r.path().join("local-usage.jsonl");
    std::fs::write(&replay, concat!(
        "{\"type\":\"thread.started\",\"thread_id\":\"egress-denied-thread\"}\n",
        "{\"type\":\"turn.started\"}\n",
        "{\"type\":\"turn.completed\",\"usage\":{\"input_tokens\":21,\"output_tokens\":3}}\n"
    )).unwrap();
    let d = Daemon::start(&[("OVERSEER_CODEX_PATH", &fixture("fake-harness/replay.js")),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "REPLAY_FILE,REPLAY_DELAY_MS"),
        ("REPLAY_FILE", replay.to_str().unwrap()), ("REPLAY_DELAY_MS", "1000"),
        ("OVERSEER_TEST_DENY_EGRESS", "1")]);
    let pid = d.child.as_ref().unwrap().id().to_string();
    let inspected = std::process::Command::new("/bin/ps")
        .args(["-o", "pid=,comm=", "-p", &pid]).output().unwrap();
    assert!(inspected.status.success(), "daemon process disappeared");
    assert!(!String::from_utf8_lossy(&inspected.stdout).trim().is_empty());
    let sockets = std::process::Command::new("/usr/sbin/lsof")
        .args(["-nP", "-a", "-p", &pid, "-i"]).output().unwrap();
    assert!(sockets.stdout.is_empty(), "unexpected daemon Internet socket: {}",
        String::from_utf8_lossy(&sockets.stdout));

    let created = d.call("task.create", json!({"repo":repo,"harness":"codex",
        "model":"gpt-6-sol","effort":"medium","prompt":"record local usage"}));
    let run = run_id(&created);
    d.wait_status(&run, |status| status == "running", 10);
    let (shim, _) = launch_info(&d, &run);
    let harness_pid = shim["child_pid"].as_i64().unwrap().to_string();
    let harness_process = std::process::Command::new("/bin/ps")
        .args(["-o", "pid=,comm=", "-p", &harness_pid]).output().unwrap();
    assert!(harness_process.status.success(), "harness process disappeared before inspection");
    for inspected_pid in [&pid, &harness_pid] {
        let live_sockets = std::process::Command::new("/usr/sbin/lsof")
            .args(["-nP", "-a", "-p", inspected_pid, "-i"]).output().unwrap();
        assert!(live_sockets.stdout.is_empty(), "unexpected Internet socket during task: {}",
            String::from_utf8_lossy(&live_sockets.stdout));
    }
    assert_eq!(d.wait_done(&run, 15)["status"], "completed");
    let usage = d.call("auto.usage.list", json!({"limit":10}));
    let summary = d.call("auto.usage.summary", json!({"limit":10}));
    assert_eq!(usage["measurements"][0]["input_tokens"], 21, "{usage}");
    assert_eq!(usage["measurements"][0]["output_tokens"], 3, "{usage}");
    assert_eq!(summary["aggregates"][0]["samples"], 1, "{summary}");
    assert_eq!(summary["aggregates"][0]["input_tokens"], 21, "{summary}");
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite.learning")).unwrap();
    let details: i64 = db.query_row("SELECT COUNT(*) FROM auto_measurements", [], |row| row.get(0)).unwrap();
    let aggregates: i64 = db.query_row("SELECT COUNT(*) FROM auto_daily_aggregates", [], |row| row.get(0)).unwrap();
    assert_eq!((details, aggregates), (1, 1), "usage must remain in the local learning database");
    let sockets_after = std::process::Command::new("/usr/sbin/lsof")
        .args(["-nP", "-a", "-p", &pid, "-i"]).output().unwrap();
    assert!(sockets_after.stdout.is_empty(), "unexpected daemon Internet socket after measurement");
}

#[test]
fn auto_learning_samples_live_in_a_separate_capped_local_file() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let replay = r.path().join("separate-learning.jsonl");
    std::fs::write(&replay, concat!(
        "{\"type\":\"thread.started\",\"thread_id\":\"learning-thread\"}\n",
        "{\"type\":\"turn.started\"}\n",
        "{\"type\":\"turn.completed\",\"usage\":{\"input_tokens\":42}}\n"
    )).unwrap();
    let d = Daemon::start(&[("OVERSEER_CODEX_PATH", &fixture("fake-harness/replay.js")),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "REPLAY_FILE,REPLAY_DELAY_MS"),
        ("REPLAY_FILE", replay.to_str().unwrap()), ("REPLAY_DELAY_MS", "10")]);
    let created = d.call("task.create", json!({"repo":repo,"harness":"codex","prompt":"x"}));
    let run = run_id(&created);
    assert_eq!(d.wait_done(&run, 15)["status"], "completed");
    let main = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let learning_path = d.home.path().join("overseer.sqlite.learning");
    assert!(learning_path.is_file(), "learning must not grow the execution database");
    let learning = rusqlite::Connection::open(&learning_path).unwrap();
    let samples: i64 = learning.query_row("SELECT COUNT(*) FROM auto_measurements", [], |row| row.get(0)).unwrap();
    let legacy: i64 = main.query_row("SELECT COUNT(*) FROM auto_measurements", [], |row| row.get(0)).unwrap();
    let events: i64 = main.query_row("SELECT COUNT(*) FROM events WHERE run_id=?1 AND kind='usage'",
        [&run], |row| row.get(0)).unwrap();
    assert_eq!((samples, legacy, events), (1, 0, 1));
    let page_size: i64 = learning.pragma_query_value(None, "page_size", |row| row.get(0)).unwrap();
    learning.pragma_update(None, "max_page_count", 128 * 1024 * 1024 / page_size).unwrap();
    learning.execute_batch("CREATE TABLE pressure(payload BLOB);
        CREATE TRIGGER pressure_learning BEFORE INSERT ON auto_measurements
        BEGIN INSERT INTO pressure(payload) VALUES(zeroblob(1048576)); END;").unwrap();
    let mut filled = 0;
    while learning.execute("INSERT INTO pressure(payload) VALUES(zeroblob(1048576))", []).is_ok() {
        filled += 1;
        assert!(filled < 130, "learning file exceeded its page cap");
    }
    assert!(filled > 0);
    let pressured = d.call("task.create", json!({"repo":repo,"harness":"codex","prompt":"pressure"}));
    let pressured_run = run_id(&pressured);
    assert_eq!(d.wait_done(&pressured_run, 15)["status"], "completed");
    assert_eq!(d.call("auto.usage.list", json!({}))["learning_paused"], true);
    assert!(std::fs::metadata(&learning_path).unwrap().len() <= 128 * 1024 * 1024);
    assert_eq!(d.call("auto.usage.list", json!({}))["measurements"].as_array().unwrap().len(), 1);
    assert_eq!(main.query_row("SELECT COUNT(*) FROM events WHERE run_id=?1 AND kind='usage'",
        [&pressured_run], |row| row.get::<_, i64>(0)).unwrap(), 1);
    learning.execute_batch("DROP TRIGGER pressure_learning; DELETE FROM pressure;").unwrap();
    let recovered = d.call("task.create", json!({"repo":repo,"harness":"codex","prompt":"recovered"}));
    assert_eq!(d.wait_done(&run_id(&recovered), 15)["status"], "completed");
    assert_eq!(d.call("auto.usage.list", json!({}))["learning_paused"], false);
    assert_eq!(d.call("auto.usage.list", json!({}))["measurements"].as_array().unwrap().len(), 2);
}

#[test]
fn auto_selection_and_result_survive_physical_learning_file_capacity() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let d = Daemon::start(&[("OVERSEER_CODEX_PATH", &fixture("fake-harness/codex-app-fixture.js")),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE,FIXTURE_EMIT_USAGE"),
        ("FIXTURE_MODE", "managed-models"), ("FIXTURE_EMIT_USAGE", "1")]);
    let parent = run_id(&d.call("task.create", json!({"repo":repo,"harness":"codex-app",
        "model":"gpt-6-astra","effort":"high","prompt":"seed context"})));
    assert_eq!(d.wait_done(&parent, 15)["status"], "completed");
    let learning_path = d.home.path().join("overseer.sqlite.learning");
    let learning = rusqlite::Connection::open(&learning_path).unwrap();
    let page_size: i64 = learning.pragma_query_value(None, "page_size", |row| row.get(0)).unwrap();
    learning.pragma_update(None, "max_page_count", 128 * 1024 * 1024 / page_size).unwrap();
    learning.execute_batch("CREATE TABLE physical_pressure(payload BLOB);
        CREATE TRIGGER pressure_work_summary BEFORE INSERT ON auto_work_observations
        BEGIN INSERT INTO physical_pressure(payload) VALUES(zeroblob(1048576)); END;").unwrap();
    let mut filled = 0;
    while learning.execute("INSERT INTO physical_pressure(payload) VALUES(zeroblob(1048576))", []).is_ok() {
        filled += 1;
        assert!(filled < 130, "learning database exceeded its 128 MiB page cap");
    }
    assert!(filled > 0);
    let mut children = Vec::new();
    for unit in ["physical-pressure-1", "physical-pressure-while-paused-2"] {
        let selected = d.call("auto.dispatch", json!({"work_unit_id":unit,"parent_run_id":parent,
            "min_tier":"general","required_tools":[],"prompt":"bounded work"}));
        assert_eq!(selected["state"], "dispatched", "{selected}");
        assert_eq!(selected["decision"]["selected"], "system-codex/gpt-6-sol/medium");
        let child = run_id(&selected);
        assert_eq!(d.wait_done(&child, 15)["status"], "completed");
        assert_eq!(d.call("run.result", json!({"run_id":child}))["state"], "ready");
        assert_eq!(d.call("auto.usage.list", json!({}))["learning_paused"], true);
        children.push(child);
    }
    assert!(std::fs::metadata(&learning_path).unwrap().len() <= 128 * 1024 * 1024);
    assert!(d.call("auto.usage.work.list", json!({}))["work_units"].as_array().unwrap().is_empty());
    learning.execute_batch("DROP TRIGGER pressure_work_summary; DELETE FROM physical_pressure;").unwrap();
    let recovered = d.call("auto.dispatch", json!({"work_unit_id":"physical-pressure-recovered-3",
        "parent_run_id":parent,"min_tier":"general","required_tools":[],"prompt":"new bounded work"}));
    assert_eq!(recovered["state"], "dispatched", "{recovered}");
    let recovered_child = run_id(&recovered);
    assert_eq!(d.wait_done(&recovered_child, 15)["status"], "completed");
    assert_eq!(d.call("auto.usage.list", json!({}))["learning_paused"], false);
    let rows = d.call("auto.usage.work.list", json!({}));
    assert_eq!(rows["work_units"].as_array().unwrap().len(), 1, "{rows}");
    assert_eq!(rows["work_units"][0]["run_id"], recovered_child);
    for child in children.into_iter().chain(std::iter::once(recovered_child)) {
        assert_eq!(d.events(&child).iter().filter(|event| event["kind"] == "usage").count(), 1,
            "learning pressure must not replay a child turn");
    }
}

#[test]
fn auto_telemetry_failure_pauses_learning_without_restarting_or_replaying_work() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let replay = r.path().join("auto-usage-failure.jsonl");
    std::fs::write(&replay, concat!(
        "{\"type\":\"thread.started\",\"thread_id\":\"auto-usage-thread\"}\n",
        "{\"type\":\"turn.started\"}\n",
        "{\"type\":\"turn.completed\",\"usage\":{\"input_tokens\":42}}\n"
    )).unwrap();
    let d = Daemon::start(&[("OVERSEER_CODEX_PATH", &fixture("fake-harness/replay.js")),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "REPLAY_FILE,REPLAY_DELAY_MS"),
        ("REPLAY_FILE", replay.to_str().unwrap()), ("REPLAY_DELAY_MS", "10")]);
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite.learning")).unwrap();
    db.execute_batch("CREATE TRIGGER auto_fail BEFORE INSERT ON auto_measurements BEGIN SELECT RAISE(FAIL, 'telemetry failure'); END;").unwrap();
    let first = d.call("task.create", json!({"repo": repo, "harness": "codex", "prompt": "x", "title": "first"}));
    let first_run = run_id(&first);
    assert_eq!(d.wait_done(&first_run, 15)["status"], "completed");
    let paused = d.call("auto.usage.list", json!({}));
    assert_eq!(paused["learning_paused"], true);
    assert!(paused["measurements"].as_array().unwrap().is_empty());
    db.execute_batch("DROP TRIGGER auto_fail;").unwrap();
    let second = d.call("task.create", json!({"repo": repo, "harness": "codex", "prompt": "x", "title": "second"}));
    let second_run = run_id(&second);
    assert_eq!(d.wait_done(&second_run, 15)["status"], "completed");
    let recovered = d.call("auto.usage.list", json!({}));
    assert_eq!(recovered["learning_paused"], false);
    assert_eq!(recovered["measurements"].as_array().unwrap().len(), 1);
    assert_eq!(d.events(&first_run).iter().filter(|e| e["kind"] == "usage").count(), 1);
    assert_eq!(d.events(&second_run).iter().filter(|e| e["kind"] == "usage").count(), 1);
}


#[test]
fn auto_aggregate_failure_rolls_back_learning_without_replaying_the_task() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let replay = r.path().join("aggregate-failure.jsonl");
    std::fs::write(&replay, concat!(
        "{\"type\":\"thread.started\",\"thread_id\":\"aggregate-fixture\"}\n",
        "{\"type\":\"turn.started\"}\n",
        "{\"type\":\"turn.completed\",\"usage\":{\"input_tokens\":42}}\n"
    )).unwrap();
    let d = Daemon::start(&[("OVERSEER_CODEX_PATH", &fixture("fake-harness/replay.js")),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "REPLAY_FILE,REPLAY_DELAY_MS"),
        ("REPLAY_FILE", replay.to_str().unwrap()), ("REPLAY_DELAY_MS", "10")]);
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite.learning")).unwrap();
    db.execute_batch("CREATE TRIGGER aggregate_fail BEFORE INSERT ON auto_daily_aggregates BEGIN SELECT RAISE(FAIL, 'aggregate failure'); END;").unwrap();
    let first = d.call("task.create", json!({"repo": repo, "harness": "codex", "prompt": "x", "title": "aggregate first"}));
    let first_run = run_id(&first);
    assert_eq!(d.wait_done(&first_run, 15)["status"], "completed");
    assert_eq!(d.call("auto.usage.list", json!({}))["learning_paused"], true);
    assert!(d.call("auto.usage.list", json!({}))["measurements"].as_array().unwrap().is_empty());
    assert!(d.call("auto.usage.summary", json!({}))["aggregates"].as_array().unwrap().is_empty());
    db.execute_batch("DROP TRIGGER aggregate_fail;").unwrap();
    let second = d.call("task.create", json!({"repo": repo, "harness": "codex", "prompt": "x", "title": "aggregate second"}));
    let second_run = run_id(&second);
    assert_eq!(d.wait_done(&second_run, 15)["status"], "completed");
    assert_eq!(d.call("auto.usage.list", json!({}))["learning_paused"], false);
    assert_eq!(d.call("auto.usage.summary", json!({}))["aggregates"][0]["samples"], 1);
    for run in [first_run, second_run] {
        assert_eq!(d.events(&run).iter().filter(|e| e["kind"] == "usage").count(), 1);
    }
}

#[test]
fn auto_dispatch_continues_during_local_learning_write_pressure_and_recovers() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let trace = r.path().join("auto-learning-pressure-trace.txt");
    let d = Daemon::start(&[("OVERSEER_CODEX_PATH", &fixture("fake-harness/codex-app-fixture.js")),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE,FIXTURE_TRACE_FILE"),
        ("FIXTURE_MODE", "managed-models"), ("FIXTURE_TRACE_FILE", trace.to_str().unwrap())]);
    let parent = run_id(&d.call("task.create", json!({"repo":repo,"harness":"codex-app",
        "model":"gpt-6-astra","effort":"high","prompt":"seed context"})));
    assert_eq!(d.wait_done(&parent, 15)["status"], "completed");
    assert!(d.call("auto.usage.list", json!({}))["measurements"].as_array().unwrap().is_empty());
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite.learning")).unwrap();
    db.execute_batch("CREATE TRIGGER learning_capacity_full BEFORE INSERT ON auto_measurements
        BEGIN SELECT RAISE(FAIL, 'simulated local learning capacity full'); END;
        CREATE TRIGGER work_capacity_full BEFORE INSERT ON auto_work_observations
        BEGIN SELECT RAISE(FAIL, 'simulated work learning capacity full'); END;").unwrap();
    let first = d.call("auto.dispatch", json!({"work_unit_id":"learning-pressure-1",
        "parent_run_id":parent,"min_tier":"general","required_tools":[],"prompt":"browser check"}));
    assert_eq!(first["state"], "dispatched", "{first}");
    let first_child = run_id(&first);
    assert_eq!(d.wait_done(&first_child, 15)["status"], "completed");
    assert_eq!(d.call("run.result", json!({"run_id":first_child}))["state"], "ready");
    let paused = d.call("auto.usage.list", json!({}));
    assert_eq!(paused["learning_paused"], true);
    assert_eq!(paused["measurements"].as_array().unwrap().len(), 0,
        "failed learning write must not leave a partial sample");
    assert!(d.call("auto.usage.work.list", json!({}))["work_units"].as_array().unwrap().is_empty(),
        "failed work learning must not leave a partial row");
    let while_paused = d.call("auto.dispatch", json!({"work_unit_id":"learning-pressure-while-paused",
        "parent_run_id":parent,"min_tier":"general","required_tools":[],"prompt":"work despite unavailable learning"}));
    assert_eq!(while_paused["state"], "dispatched", "{while_paused}");
    assert_eq!(while_paused["decision"]["selected"], "system-codex/gpt-6-sol/medium");
    let paused_child = run_id(&while_paused);
    assert_eq!(d.wait_done(&paused_child, 15)["status"], "completed");
    assert_eq!(d.call("run.result", json!({"run_id":paused_child}))["state"], "ready");
    assert_eq!(d.call("auto.usage.list", json!({}))["learning_paused"], true);
    assert!(d.call("auto.usage.work.list", json!({}))["work_units"].as_array().unwrap().is_empty());
    db.execute_batch("DROP TRIGGER learning_capacity_full; DROP TRIGGER work_capacity_full;").unwrap();
    let second = d.call("auto.dispatch", json!({"work_unit_id":"learning-pressure-2",
        "parent_run_id":parent,"min_tier":"general","required_tools":[],"prompt":"another bounded child"}));
    assert_eq!(second["state"], "dispatched", "{second}");
    let second_child = run_id(&second);
    assert_eq!(d.wait_done(&second_child, 15)["status"], "completed");
    let recovered = d.call("auto.usage.list", json!({}));
    assert_eq!(recovered["learning_paused"], false);
    assert_eq!(recovered["measurements"].as_array().unwrap().len(), 0,
        "recovery must be observed from the work write alone");
    let work = d.call("auto.usage.work.list", json!({}));
    assert_eq!(work["work_units"].as_array().unwrap().len(), 1);
    assert_eq!(work["work_units"][0]["work_unit_id"], "learning-pressure-2");
    assert_eq!(d.runs().len(), 4);
    assert_eq!(d.events(&parent).into_iter().filter(|event| event["kind"] == "auto_decision").count(), 3);
    assert_eq!(std::fs::read_to_string(&trace).unwrap().matches("turn_model:gpt-6-sol").count(), 3,
        "learning recovery must not replay any child turn");
}

#[test]
fn auto_successful_work_summary_does_not_hide_failed_usage_recording() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let d = Daemon::start(&[("OVERSEER_CODEX_PATH", &fixture("fake-harness/codex-app-fixture.js")),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE,FIXTURE_EMIT_USAGE"),
        ("FIXTURE_MODE", "managed-models"), ("FIXTURE_EMIT_USAGE", "1")]);
    let parent = run_id(&d.call("task.create", json!({"repo":repo,"harness":"codex-app",
        "model":"gpt-6-astra","effort":"high","prompt":"seed context"})));
    assert_eq!(d.wait_done(&parent, 15)["status"], "completed");
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite.learning")).unwrap();
    db.execute_batch("CREATE TRIGGER usage_capacity_full BEFORE INSERT ON auto_measurements
        BEGIN SELECT RAISE(FAIL, 'simulated usage capacity full'); END;").unwrap();
    let first = d.call("auto.dispatch", json!({"work_unit_id":"mixed-learning-failure-1",
        "parent_run_id":parent,"min_tier":"general","required_tools":[],"prompt":"bounded child"}));
    assert_eq!(first["state"], "dispatched", "{first}");
    let first_child = run_id(&first);
    assert_eq!(d.wait_done(&first_child, 15)["status"], "completed");
    assert_eq!(d.call("auto.usage.work.list", json!({}))["work_units"].as_array().unwrap().len(), 1,
        "the work summary can succeed even when usage recording fails");
    assert_eq!(d.call("auto.usage.list", json!({}))["learning_paused"], true,
        "a successful work summary must not hide the failed usage lane");
    db.execute_batch("DROP TRIGGER usage_capacity_full;").unwrap();
    let second = d.call("auto.dispatch", json!({"work_unit_id":"mixed-learning-recovered-2",
        "parent_run_id":parent,"min_tier":"general","required_tools":[],"prompt":"next bounded child"}));
    assert_eq!(second["state"], "dispatched", "{second}");
    assert_eq!(d.wait_done(&run_id(&second), 15)["status"], "completed");
    assert_eq!(d.call("auto.usage.list", json!({}))["learning_paused"], false);
    assert_eq!(d.call("auto.usage.work.list", json!({}))["work_units"].as_array().unwrap().len(), 2);
    assert_eq!(d.call("auto.usage.list", json!({}))["measurements"].as_array().unwrap().len(), 2,
        "the missing first child sample must not be reconstructed or duplicated");
}

#[test]
fn auto_thread_and_work_refresh_failures_report_pause_until_their_own_recovery() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let d = Daemon::start(&[("OVERSEER_CODEX_PATH", &fixture("fake-harness/codex-app-fixture.js")),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE"),
        ("FIXTURE_MODE", "managed-delegation")]);
    let parent = run_id(&d.call("task.create", json!({"repo":repo,"harness":"codex-app",
        "model":"gpt-6-astra","effort":"high","prompt":"seed context"})));
    assert_eq!(d.wait_done(&parent, 15)["status"], "completed");
    let child = run_id(&d.call("run.delegate", json!({"work_unit_id":"learning-refresh-child",
        "parent_run_id":parent,"harness":"codex-app","model":"gpt-6-sol",
        "effort":"medium","prompt":"bounded work"})));
    assert_eq!(d.wait_done(&child, 15)["status"], "completed");
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite.learning")).unwrap();
    db.execute_batch("CREATE TRIGGER reject_thread_learning BEFORE INSERT ON auto_thread_usage_observations
        BEGIN SELECT RAISE(FAIL, 'thread learning unavailable'); END;").unwrap();
    assert!(d.try_call("auto.usage.thread.refresh", json!({"run_id":child})).is_err());
    assert_eq!(d.call("auto.usage.list", json!({}))["learning_paused"], true,
        "a failed thread-credit learning write must surface paused state");
    db.execute_batch("DROP TRIGGER reject_thread_learning;").unwrap();
    assert_eq!(d.call("auto.usage.thread.refresh", json!({"run_id":child}))["state"], "estimated");
    assert_eq!(d.call("auto.usage.list", json!({}))["learning_paused"], false);

    db.execute_batch("CREATE TRIGGER reject_work_refresh BEFORE UPDATE ON auto_work_observations
        BEGIN SELECT RAISE(FAIL, 'work refresh unavailable'); END;").unwrap();
    assert_eq!(d.call("auto.usage.thread.refresh", json!({"run_id":child}))["state"], "estimated");
    assert_eq!(d.call("auto.usage.list", json!({}))["learning_paused"], true,
        "a failed work-summary correction must surface paused state");
    db.execute_batch("DROP TRIGGER reject_work_refresh;").unwrap();
    assert_eq!(d.call("auto.usage.thread.refresh", json!({"run_id":child}))["state"], "estimated");
    assert_eq!(d.call("auto.usage.list", json!({}))["learning_paused"], false,
        "a successful correction must clear only its own failed lane");
    assert_eq!(d.call("auto.usage.thread.list", json!({}))["observations"].as_array().unwrap().len(), 1);
    assert_eq!(d.call("auto.usage.work.list", json!({}))["work_units"].as_array().unwrap().len(), 1);
    assert_eq!(d.run(&child)["status"], "completed");
}

#[test]
fn auto_transient_account_evidence_failure_recovers_without_hiding_other_lanes() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let d = Daemon::start(&[("OVERSEER_CODEX_PATH", &fixture("fake-harness/codex-app-fixture.js")),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE"),
        ("FIXTURE_MODE", "managed-models")]);
    let parent = run_id(&d.call("task.create", json!({"repo":repo,"harness":"codex-app",
        "model":"gpt-6-astra","effort":"high","prompt":"seed context"})));
    assert_eq!(d.wait_done(&parent, 15)["status"], "completed");
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    db.execute_batch("CREATE TRIGGER reject_account_evidence BEFORE INSERT ON auto_account_identity
        BEGIN SELECT RAISE(FAIL, 'account evidence unavailable'); END;").unwrap();
    let first = run_id(&d.call("run.delegate", json!({"work_unit_id":"account-evidence-fail-1",
        "parent_run_id":parent,"harness":"codex-app","model":"gpt-6-sol",
        "effort":"medium","prompt":"first turn"})));
    assert_eq!(d.wait_done(&first, 15)["status"], "completed");
    assert_eq!(d.call("auto.usage.list", json!({}))["learning_paused"], true);
    db.execute_batch("DROP TRIGGER reject_account_evidence;").unwrap();
    let second = run_id(&d.call("run.delegate", json!({"work_unit_id":"account-evidence-recovered-2",
        "parent_run_id":parent,"harness":"codex-app","model":"gpt-6-sol",
        "effort":"medium","prompt":"second turn"})));
    assert_eq!(d.wait_done(&second, 15)["status"], "completed");
    assert_eq!(d.call("auto.usage.list", json!({}))["learning_paused"], false,
        "a successful later account read must clear the attribution failure");
    assert_eq!(d.runs().len(), 3);
}

#[test]
fn auto_codex_thread_credit_estimate_is_metadata_only_and_separate_from_quota() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let trace = r.path().join("usage-read-trace.txt");
    let d = Daemon::start(&[("OVERSEER_CODEX_PATH", &fixture("fake-harness/codex-app-fixture.js")),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE,FIXTURE_TRACE_FILE"),
        ("FIXTURE_MODE", "metadata-usage"), ("FIXTURE_TRACE_FILE", trace.to_str().unwrap())]);
    let created = d.call("task.create", json!({"repo":repo,"harness":"codex-app","model":"gpt-6-sol","effort":"medium","prompt":"requested work","title":"credit fixture"}));
    let run = run_id(&created);
    assert_eq!(created["run"]["effort"], "medium");
    assert_eq!(d.wait_done(&run, 15)["status"], "completed");
    let observed = d.call("auto.usage.thread.refresh", json!({"run_id":run}));
    assert_eq!(observed["state"], "estimated", "{observed}");
    assert_eq!(observed["observation"]["estimate"]["estimated_credits_micros"], 2_500_000);
    assert_eq!(observed["observation"]["estimate"]["groups"][0]["effort"], "medium");
    assert_eq!(observed["observation"]["attribution"], "unverified_run_account");
    assert!(observed["observation"]["read_account_generation"].as_i64().unwrap() > 0);
    assert!(!observed.to_string().contains("private-credit-sentinel"));
    assert!(!observed.to_string().contains("secret-prompt-sentinel"));
    let repeated = d.call("auto.usage.thread.refresh", json!({"run_id":run}));
    assert_eq!(repeated["observation"]["id"], observed["observation"]["id"],
        "re-reading one unchanged cumulative thread estimate must not create a second sample");
    let rows = d.call("auto.usage.thread.list", json!({"limit":10}));
    assert_eq!(rows["observations"].as_array().unwrap().len(), 1);
    assert_eq!(d.runs().len(), 1, "metadata read must not start an agent run");
    assert_eq!(std::fs::read_to_string(&trace).unwrap().lines().filter(|line| *line == "thread_usage_read").count(), 2);
    assert!(std::fs::read_to_string(&trace).unwrap().lines().any(|line| line == "turn_effort:medium"));
    let export = r.path().join("usage-export.json");
    d.call("auto.usage.export", json!({"path":export}));
    let exported = std::fs::read_to_string(&export).unwrap();
    assert!(exported.contains("estimated_credits_micros"));
    assert!(!exported.contains("private-credit-sentinel"));
    assert!(!exported.contains("secret-prompt-sentinel"));
    d.call("auto.usage.clear", json!({}));
    assert!(d.call("auto.usage.thread.list", json!({}))["observations"].as_array().unwrap().is_empty());
    assert_eq!(d.runs().len(), 1);
    assert!(d.try_call("task.create", json!({"repo":repo,"harness":"codex-app","model":"gpt-6-sol","effort":"medium;bad","prompt":"x"})).is_err());
    assert_eq!(d.runs().len(), 1, "invalid effort must fail before creating a run");
}

#[test]
fn auto_thread_credit_read_loses_plan_attribution_when_the_account_plan_changes() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let plan_file = r.path().join("plan-type.txt");
    std::fs::write(&plan_file, "pro").unwrap();
    let d = Daemon::start(&[("OVERSEER_CODEX_PATH", &fixture("fake-harness/codex-app-fixture.js")),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE,FIXTURE_PLAN_TYPE_FILE"),
        ("FIXTURE_MODE", "managed-delegation"),
        ("FIXTURE_PLAN_TYPE_FILE", plan_file.to_str().unwrap())]);
    let parent = run_id(&d.call("task.create", json!({"repo":repo,"harness":"codex-app",
        "model":"gpt-6-astra","effort":"high","prompt":"seed context"})));
    assert_eq!(d.wait_done(&parent, 15)["status"], "completed");
    let child = run_id(&d.call("run.delegate", json!({"work_unit_id":"plan-scope-child-1",
        "parent_run_id":parent,"harness":"codex-app","model":"gpt-6-sol",
        "effort":"medium","prompt":"bounded work"})));
    assert_eq!(d.wait_done(&child, 15)["status"], "completed");
    let original = d.call("auto.usage.thread.refresh", json!({"run_id":child}));
    assert_eq!(original["observation"]["attribution"],
        "same_account_generation_and_reported_plan", "{original}");
    assert_eq!(original["observation"]["estimate"]["plan_type"], "pro");
    std::fs::write(&plan_file, "plus").unwrap();
    let changed = d.call("auto.usage.thread.refresh", json!({"run_id":child}));
    assert_eq!(changed["observation"]["id"], original["observation"]["id"],
        "a later read is a correction to the same cumulative sample");
    assert_eq!(changed["observation"]["attribution"], "unverified_plan_scope", "{changed}");
    assert_eq!(changed["allowance_delta"]["state"], "unverified");
    assert!(changed["allowance_delta"]["reasons"].as_array().unwrap().iter().any(|reason|
        reason == "account_plan_changed"), "{changed}");
    assert_eq!(changed["observation"]["estimate"]["plan_type"], "plus");
    assert_eq!(changed["observation"]["subscription_window_relation"], "unverified");
    assert_eq!(d.runs().len(), 2, "metadata-only correction must not launch work");
}

#[test]
fn auto_managed_codex_quota_reads_are_linked_to_the_child_without_raw_response() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let d = Daemon::start(&[("OVERSEER_CODEX_PATH", &fixture("fake-harness/codex-app-fixture.js")),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE"),
        ("FIXTURE_MODE", "managed-delegation")]);
    let parent = run_id(&d.call("task.create", json!({"repo":repo,"harness":"codex-app",
        "model":"gpt-6-astra","effort":"high","prompt":"seed context"})));
    assert_eq!(d.wait_done(&parent, 15)["status"], "completed");
    let child = run_id(&d.call("run.delegate", json!({"work_unit_id":"quota-pair-child-1",
        "parent_run_id":parent,"harness":"codex-app","model":"gpt-6-sol",
        "effort":"medium","prompt":"bounded work"})));
    assert_eq!(d.wait_done(&child, 15)["status"], "completed");
    let refreshed = d.call("auto.usage.thread.refresh", json!({"run_id":child}));
    assert_eq!(refreshed["allowance_delta"]["state"], "unverified", "{refreshed}");
    assert!(refreshed["allowance_delta"]["windows"].as_array().unwrap().is_empty());
    let reasons = refreshed["allowance_delta"]["reasons"].as_array().unwrap();
    for reason in ["external_usage_unexcluded", "reporting_not_settled", "meter_precision_unknown"] {
        assert!(reasons.iter().any(|item| item == reason), "{refreshed}");
    }
    let observations = d.events(&child).into_iter().filter(|event|
        event["kind"] == "auto_quota" || event["kind"] == "quota").collect::<Vec<_>>();
    assert_eq!(observations.len(), 2, "before and after allowance reads must belong to the same child: {observations:?}");
    assert_eq!(observations[0]["source"], "codex-app/managed-pre-turn");
    assert_eq!(observations[1]["source"], "codex-app/metadata-read");
    assert_eq!(observations[0]["payload"]["snapshot"]["windows"][0]["plan_type"], "pro");
    assert_eq!(observations[1]["payload"]["snapshot"]["windows"][0]["plan_type"], "pro");
    assert!(observations[0]["payload"]["snapshot"]["observed_ms"].as_i64().unwrap()
        <= observations[1]["payload"]["snapshot"]["observed_ms"].as_i64().unwrap());
    assert!(!serde_json::to_string(&observations).unwrap().contains("secret-credit-sentinel"));
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let linked: i64 = db.query_row("SELECT COUNT(*) FROM auto_quota_observations q JOIN events e ON e.seq=q.event_seq WHERE e.run_id=?1", [&child], |row| row.get(0)).unwrap();
    assert_eq!(linked, 2, "normalized quota snapshots must be durable and run-scoped");
}

#[test]
fn auto_work_history_links_actual_usage_outcome_and_quota_without_content() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let mut d = Daemon::start(&[("OVERSEER_CODEX_PATH", &fixture("fake-harness/codex-app-fixture.js")),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE,FIXTURE_EMIT_USAGE"),
        ("FIXTURE_MODE", "managed-models"), ("FIXTURE_EMIT_USAGE", "1")]);
    let parent = run_id(&d.call("task.create", json!({"repo":repo,"harness":"codex-app",
        "model":"gpt-6-astra","effort":"high","prompt":"private-parent-sentinel"})));
    assert_eq!(d.wait_done(&parent, 15)["status"], "completed");
    let selected = d.call("auto.dispatch", json!({"work_unit_id":"measured-browser-unit",
        "parent_run_id":parent,"min_tier":"general","required_tools":["browser/navigate"],
        "prompt":"private-browser-sentinel"}));
    assert_eq!(selected["state"], "dispatched", "{selected}");
    let child = run_id(&selected);
    assert_eq!(d.wait_done(&child, 15)["status"], "completed");
    let _ = d.call("auto.usage.thread.refresh", json!({"run_id":child}));
    let history = d.call("auto.usage.work.list", json!({"limit":10}));
    let rows = history["work_units"].as_array().unwrap();
    assert_eq!(rows.len(), 1, "{history}");
    let work = &rows[0];
    assert_eq!(work["work_unit_id"], "measured-browser-unit");
    assert_eq!(work["run_id"], child);
    assert_eq!(work["model"], "gpt-6-sol");
    assert_eq!(work["effort"], "medium");
    assert_eq!(work["status"], "completed");
    assert_eq!(work["usage"]["input_tokens"], 42);
    assert_eq!(work["usage"]["output_tokens"], 7);
    assert_eq!(work["quota_before"]["source"], "codex-app/managed-pre-turn");
    assert_eq!(work["quota_after"]["source"], "codex-app/metadata-read");
    assert_eq!(work["subscription_window_draw"], "unverified");
    assert!(work["execution_ms"].as_i64().unwrap() >= 0);
    assert!(work["launch_overhead_ms"].as_i64().unwrap() >= 0);
    let serialized = history.to_string();
    assert!(!serialized.contains("private-parent-sentinel"));
    assert!(!serialized.contains("private-browser-sentinel"));
    assert!(!serialized.contains("secret-credit-sentinel"));
    let export_path = r.path().join("work-observations.json");
    d.call("auto.usage.export", json!({"path":export_path}));
    let exported = std::fs::read_to_string(&export_path).unwrap();
    assert!(exported.contains("\"work_units\""));
    assert!(exported.contains("measured-browser-unit"));
    assert!(!exported.contains("private-browser-sentinel"));
    assert!(!exported.contains("private-parent-sentinel"));
    d.call("auto.usage.clear", json!({}));
    assert!(d.call("auto.usage.work.list", json!({}))["work_units"].as_array().unwrap().is_empty());
    d.kill9();
    d.spawn();
    assert!(d.call("auto.usage.work.list", json!({}))["work_units"].as_array().unwrap().is_empty(),
        "cleared work learning must not regenerate from retained execution history");
    assert_eq!(d.run(&child)["status"], "completed");
}

#[test]
fn auto_work_history_preserves_missing_usage_as_unknown() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let d = Daemon::start(&[("OVERSEER_CODEX_PATH", &fixture("fake-harness/codex-app-fixture.js")),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE"),
        ("FIXTURE_MODE", "managed-models")]);
    let parent = run_id(&d.call("task.create", json!({"repo":repo,"harness":"codex-app",
        "model":"gpt-6-astra","effort":"high","prompt":"seed context"})));
    assert_eq!(d.wait_done(&parent, 15)["status"], "completed");
    let child = run_id(&d.call("auto.dispatch", json!({"work_unit_id":"no-usage-unit",
        "parent_run_id":parent,"min_tier":"general","required_tools":[],
        "prompt":"bounded work"})));
    assert_eq!(d.wait_done(&child, 15)["status"], "completed");
    let rows = d.call("auto.usage.work.list", json!({}));
    assert_eq!(rows["work_units"].as_array().unwrap().len(), 1, "{rows}");
    assert_eq!(rows["work_units"][0]["usage_observations"], 0);
    assert!(rows["work_units"][0]["usage"].is_null());
    assert_eq!(rows["work_units"][0]["subscription_window_draw"], "unverified");
}

#[test]
fn auto_work_history_does_not_reappear_after_account_changes_during_child() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let account_file = r.path().join("account-id.txt");
    std::fs::write(&account_file, "account-A").unwrap();
    let d = Daemon::start(&[("OVERSEER_CODEX_PATH", &fixture("fake-harness/codex-app-fixture.js")),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE,FIXTURE_ACCOUNT_ID_FILE,FIXTURE_TURN_DELAY_MS"),
        ("FIXTURE_MODE", "managed-models"), ("FIXTURE_ACCOUNT_ID_FILE", account_file.to_str().unwrap()),
        ("FIXTURE_TURN_DELAY_MS", "6000")]);
    let parent = run_id(&d.call("task.create", json!({"repo":repo,"harness":"codex-app",
        "model":"gpt-6-astra","effort":"high","prompt":"seed context"})));
    assert_eq!(d.wait_done(&parent, 15)["status"], "completed");
    let selected = d.call("auto.dispatch", json!({"work_unit_id":"account-changes-mid-child",
        "parent_run_id":parent,"min_tier":"general","required_tools":[],"prompt":"browser check"}));
    assert_eq!(selected["state"], "dispatched", "{selected}");
    let child = run_id(&selected);
    d.wait_status(&child, |status| status == "running", 10);
    let profile_id = selected["run"]["profile_id"].as_str().unwrap();
    let workspace_id = selected["workspace"]["id"].as_str().unwrap();
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let generation = || -> i64 { db.query_row(
        "SELECT generation FROM auto_account_identity WHERE profile_id=?1",
        [profile_id], |row| row.get(0)).unwrap() };
    let prior_generation = generation();
    std::fs::write(&account_file, "account-B").unwrap();
    d.call("auto.tools.inspect", json!({"profile_id":profile_id,"workspace_id":workspace_id}));
    assert!(generation() > prior_generation, "metadata read must observe the new login while the child is active");
    assert_eq!(d.wait_done(&child, 15)["status"], "completed");
    assert!(d.call("auto.usage.work.list", json!({}))["work_units"].as_array().unwrap().is_empty(),
        "a settled child must not recreate prior-account learning");
}

#[test]
fn auto_managed_child_has_an_isolated_parent_snapshot_and_returnable_result() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let trace = r.path().join("managed-trace.txt");
    let account_file = r.path().join("account-id.txt");
    std::fs::write(&account_file, "account-A").unwrap();
    let d = Daemon::start(&[("OVERSEER_CODEX_PATH", &fixture("fake-harness/codex-app-fixture.js")),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE,FIXTURE_TRACE_FILE,FIXTURE_ACCOUNT_ID_FILE"),
        ("FIXTURE_MODE", "managed-delegation"), ("FIXTURE_TRACE_FILE", trace.to_str().unwrap()),
        ("FIXTURE_ACCOUNT_ID_FILE", account_file.to_str().unwrap())]);
    let created = d.call("task.create", json!({"repo":repo,"harness":"codex-app","model":"gpt-6-astra","effort":"high","prompt":"seed context","title":"parent","approval_policy":"never"}));
    let parent = run_id(&created);
    assert_eq!(d.wait_done(&parent, 15)["status"], "completed");
    let request = json!({"work_unit_id":"browser-unit-1","parent_run_id":parent,"harness":"codex-app","model":"gpt-6-sol","effort":"medium","prompt":"browser check","title":"browser check"});
    let delegated = d.call("run.delegate", request.clone());
    let child = delegated["run"]["id"].as_str().unwrap().to_string();
    let repeated = d.call("run.delegate", request.clone());
    assert_eq!(repeated["run"]["id"], child, "a repeated work unit must not launch a second child");
    assert_eq!(repeated["replayed"], true);
    let mut changed = request;
    changed["model"] = json!("gpt-6-astra");
    assert!(d.try_call("run.delegate", changed).is_err(), "a reused key cannot silently change its route");
    assert_eq!(d.runs().len(), 2);
    assert_eq!(delegated["run"]["parent_run_id"], parent);
    assert_eq!(delegated["run"]["model"], "gpt-6-sol");
    assert_eq!(delegated["run"]["effort"], "medium");
    assert_ne!(delegated["workspace"]["path"], created["workspace"]["path"]);
    let child_path = delegated["workspace"]["path"].as_str().unwrap();
    assert_eq!(std::fs::read_to_string(Path::new(child_path).join("parent-context.txt")).unwrap(), "from parent\n");
    assert_eq!(d.wait_done(&child, 15)["status"], "completed");
    assert!(Path::new(child_path).join("browser-report.txt").exists());
    assert!(!Path::new(created["workspace"]["path"].as_str().unwrap()).join("browser-report.txt").exists());
    let result = d.call("run.result", json!({"run_id":child}));
    assert_eq!(result["state"], "ready", "{result}");
    assert_eq!(result["text"], "browser result: parent context found");
    assert_eq!(result["parent_run_id"], parent);
    assert_eq!(d.call("run.result", json!({"run_id":child}))["event_seq"], result["event_seq"]);
    let notices = d.events(&parent).into_iter().filter(|event|
        event["kind"] == "managed_child_result_available").collect::<Vec<_>>();
    assert_eq!(notices.len(), 1, "one durable parent notice per settled child");
    assert_eq!(notices[0]["payload"]["work_unit_id"], "browser-unit-1");
    assert_eq!(notices[0]["payload"]["child_run_id"], child);
    assert_eq!(notices[0]["payload"]["source_event_seq"], result["event_seq"]);
    assert_eq!(notices[0]["payload"]["state"], "ready");
    assert!(notices[0]["payload"].get("text").is_none(), "the parent notice is a handle, not a copy of child output");
    assert!(d.try_call("run.follow_up", json!({"run_id":child,"prompt":"repeat browser check"})).is_err(),
        "a settled work unit cannot produce a second, unreported child result");
    let usage = d.call("auto.usage.thread.refresh", json!({"run_id":child}));
    assert_eq!(usage["observation"]["attribution"], "same_account_generation_and_reported_plan", "{usage}");
    assert_eq!(usage["observation"]["subscription_window_relation"], "unverified");
    assert!(!serde_json::to_string(&d.events(&child)).unwrap().contains("account-A"), "raw account identity must not enter run history");
    d.call("run.follow_up", json!({"run_id":parent,"prompt":format!("Use {}", result["text"].as_str().unwrap())}));
    assert_eq!(d.wait_done(&parent, 15)["status"], "completed");
    assert!(d.events(&parent).iter().any(|e| e["kind"] == "output" && e["payload"]["text"] == "continued with browser result"));
    let trace = std::fs::read_to_string(trace).unwrap();
    assert!(trace.contains("turn_model:gpt-6-sol"));
    assert!(trace.contains("turn_effort:medium"));
    let approvals = trace.lines().filter(|line| line.starts_with("thread_approval:")).collect::<Vec<_>>();
    assert!(approvals.len() >= 2 && approvals.iter().all(|approval| *approval == "thread_approval:never"),
        "the managed child must inherit the parent's approval ceiling: {approvals:?}");
    assert_eq!(d.runs().len(), 2);
    assert_eq!(d.call("auto.usage.work.list", json!({}))["work_units"].as_array().unwrap().len(), 1);
    std::fs::write(&account_file, "account-B").unwrap();
    let switched = d.call("auto.usage.thread.refresh", json!({"run_id":child}));
    assert_eq!(switched["observation"]["attribution"], "unverified_run_account");
    assert!(d.call("auto.usage.work.list", json!({}))["work_units"].as_array().unwrap().is_empty(),
        "a changed login must invalidate prior account-scoped work learning");
    assert_eq!(d.call("auto.usage.thread.list", json!({}))["observations"].as_array().unwrap().len(), 1,
        "an account switch must remove the old account's estimate");
    d.call("auto.usage.clear", json!({}));
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let account_evidence: i64 = db.query_row("SELECT COUNT(*) FROM auto_run_account_evidence", [], |row| row.get(0)).unwrap();
    assert_eq!(account_evidence, 0, "clearing learning must remove retained account-attribution evidence");
    assert_eq!(d.runs().len(), 2, "clearing learning must preserve execution history");
}

#[test]
fn completed_run_handoff_reuses_workspace_with_a_new_native_session() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let trace = r.path().join("handoff-trace.txt");
    let mut d = Daemon::start(&[("OVERSEER_CODEX_PATH", &fixture("fake-harness/codex-app-fixture.js")),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE,FIXTURE_TRACE_FILE,FIXTURE_TURN_DELAY_MS"),
        ("FIXTURE_MODE", "managed-models"), ("FIXTURE_TRACE_FILE", trace.to_str().unwrap()),
        ("FIXTURE_TURN_DELAY_MS", "1000")]);
    let created = d.call("task.create", json!({"repo":repo,"harness":"codex-app",
        "model":"gpt-6-astra","effort":"high","prompt":"seed context",
        "approval_policy":"never"}));
    let source = run_id(&created);
    assert_eq!(d.wait_done(&source, 15)["status"], "completed");
    let request = json!({"source_run_id":source,"harness":"codex-app",
        "model":"gpt-6-sol","effort":"medium",
        "handoff":{"corrections":["keep the original file"],
            "completed":["created parent-context.txt"],"remaining":["check the file"],
            "tests":["seed step passed"],"limitations":["browser unavailable"],
            "unresolved_actions":[]}});
    let mut unresolved = request.clone();
    unresolved["handoff"]["unresolved_actions"] = json!(["external action outcome unknown"]);
    assert!(d.try_call("run.handoff", unresolved).is_err());
    assert_eq!(d.runs().len(), 1);
    let switched = d.call("run.handoff", request.clone());
    let next = run_id(&switched);
    assert_ne!(next, source);
    assert_eq!(switched["run"]["parent_run_id"], source);
    assert_eq!(switched["workspace"]["id"], created["workspace"]["id"]);
    assert_eq!(switched["run"]["native_id"], serde_json::Value::Null);
    let checkpoint = switched["snapshot_id"].as_str().unwrap();
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let snapshot_workspace: String = db.query_row("SELECT workspace_id FROM snapshots WHERE id=?1",
        [checkpoint], |row| row.get(0)).unwrap();
    assert_eq!(snapshot_workspace, created["workspace"]["id"],
        "the handoff checkpoint must belong to the original workspace");
    let saved_launch: String = db.query_row("SELECT launch FROM runs WHERE id=?1",
        [&next], |row| row.get(0)).unwrap();
    let saved_launch: serde_json::Value = serde_json::from_str(&saved_launch).unwrap();
    let saved_handoff = saved_launch.get("generic").unwrap_or(&saved_launch);
    assert_eq!(saved_handoff["snapshot_id"], checkpoint);
    assert_eq!(saved_handoff["handoff"]["corrections"][0], "keep the original file");
    assert!(d.try_call("task.create", json!({"repo":switched["workspace"]["path"],
        "workspace_mode":"current","harness":"generic","program":"/bin/sh",
        "args":["-c","exit 0"],"title":"competing writer"})).is_err());
    let replay = d.call("run.handoff", request.clone());
    assert_eq!(run_id(&replay), next);
    assert_eq!(replay["replayed"], true);
    let mut changed = request;
    changed["model"] = json!("gpt-6-astra");
    assert!(d.try_call("run.handoff", changed).is_err());
    assert_eq!(d.wait_done(&next, 15)["status"], "completed");
    assert_eq!(d.runs().len(), 2);
    assert_ne!(d.run(&source)["native_id"], d.run(&next)["native_id"]);
    assert!(ws_path(&d, &created).join("parent-context.txt").exists());
    let turns = d.call("run.turns", json!({"run_id":next}));
    let prompt = turns[0]["prompt"].as_str().unwrap();
    for expected in ["seed context", "keep the original file", "created parent-context.txt",
        "check the file", "seed step passed", "browser unavailable"] {
        assert!(prompt.contains(expected), "missing {expected}: {prompt}");
    }
    let trace = std::fs::read_to_string(trace).unwrap();
    assert_eq!(trace.matches("thread_started").count(), 2);
    assert!(trace.lines().filter(|line| line.starts_with("thread_approval:")).all(|line| line == "thread_approval:never"));
    d.kill9();
    d.spawn();
    let replay_after_restart = d.call("run.handoff", json!({"source_run_id":source,"harness":"codex-app",
        "model":"gpt-6-sol","effort":"medium",
        "handoff":{"corrections":["keep the original file"],
            "completed":["created parent-context.txt"],"remaining":["check the file"],
            "tests":["seed step passed"],"limitations":["browser unavailable"],
            "unresolved_actions":[]}}));
    assert_eq!(run_id(&replay_after_restart), next);
    assert_eq!(replay_after_restart["replayed"], true);
    assert_eq!(d.runs().len(), 2);
    let delegated = d.call("auto.dispatch", json!({"work_unit_id":"after-handoff-unit",
        "parent_run_id":next,"min_tier":"general","required_tools":[],
        "prompt":"browser check","title":"browser check"}));
    assert_eq!(delegated["state"], "dispatched", "{delegated}");
    let child = run_id(&delegated);
    assert_eq!(d.wait_done(&child, 15)["status"], "completed");
    assert_eq!(d.call("run.result", json!({"run_id":child}))["state"], "ready");
}

#[test]
fn handoff_refuses_a_failed_or_unresolved_source_without_starting_another_run() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let d = Daemon::start(&[("OVERSEER_CODEX_PATH", &fixture("fake-harness/codex-app-fixture.js")),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE"),
        ("FIXTURE_MODE", "managed-delegation")]);
    let created = d.call("task.create", json!({"repo":repo,"harness":"codex-app",
        "model":"gpt-6-sol","effort":"medium","prompt":"edit then 503"}));
    let source = run_id(&created);
    assert_eq!(d.wait_done(&source, 15)["status"], "failed");
    assert_eq!(std::fs::read_to_string(ws_path(&d, &created).join("partial-edit.txt")).unwrap(),
        "written before failure\n");
    let mut request = json!({"source_run_id":source,"harness":"codex-app",
        "model":"gpt-6-sol","effort":"medium",
        "handoff":{"corrections":[],"completed":[],"remaining":["retry"],
            "tests":[],"limitations":[],"unresolved_actions":[]}});
    assert!(d.try_call("run.handoff", request.clone()).is_err());
    request["handoff"]["unresolved_actions"] = json!(["unknown external write"]);
    assert!(d.try_call("run.handoff", request).is_err());
    assert_eq!(d.runs().len(), 1);
    assert_eq!(std::fs::read_to_string(ws_path(&d, &created).join("partial-edit.txt")).unwrap(),
        "written before failure\n");
}

#[test]
fn handoff_replay_does_not_launch_a_committed_but_unstarted_continuation() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let trace = r.path().join("unstarted-handoff-trace.txt");
    let mut d = Daemon::start(&[("OVERSEER_CODEX_PATH", &fixture("fake-harness/codex-app-fixture.js")),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE,FIXTURE_TRACE_FILE"),
        ("FIXTURE_MODE", "managed-models"), ("FIXTURE_TRACE_FILE", trace.to_str().unwrap())]);
    let source = run_id(&d.call("task.create", json!({"repo":repo,"harness":"codex-app",
        "model":"gpt-6-astra","effort":"high","prompt":"seed context"})));
    assert_eq!(d.wait_done(&source, 15)["status"], "completed");
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    db.execute_batch("CREATE TRIGGER reject_handoff_notice BEFORE INSERT ON events
        WHEN NEW.kind='handoff_created'
        BEGIN SELECT RAISE(FAIL, 'injected handoff notice failure'); END;").unwrap();
    let request = json!({"source_run_id":source,"harness":"codex-app",
        "model":"gpt-6-sol","effort":"medium",
        "handoff":{"corrections":[],"completed":["seeded context"],
            "remaining":["review files"],"tests":[],"limitations":[],"unresolved_actions":[]}});
    assert!(d.try_call("run.handoff", request.clone()).is_err());
    let continuations = d.runs().into_iter().filter(|run| run["parent_run_id"] == source)
        .collect::<Vec<_>>();
    assert_eq!(continuations.len(), 1);
    let next = continuations[0]["id"].as_str().unwrap().to_string();
    assert_eq!(continuations[0]["process_generation"], 0);
    assert_eq!(d.call("run.handoff", request.clone())["state"], "launch_uncertain");
    d.kill9();
    db.execute_batch("DROP TRIGGER reject_handoff_notice;").unwrap();
    d.spawn();
    let replay = d.call("run.handoff", request);
    assert_eq!(replay["state"], "launch_uncertain", "{replay}");
    assert_eq!(run_id(&replay), next);
    assert_eq!(d.runs().len(), 2);
    assert_eq!(std::fs::read_to_string(trace).unwrap().matches("turn_model:gpt-6-sol").count(), 0,
        "recovery must never start an uncertain handoff turn");
}

#[test]
fn auto_opencode_metadata_keeps_real_local_endpoints_separate_without_cloud_routes() {
    let Some(program) = std::env::var_os("PATH").and_then(|path| std::env::split_paths(&path)
        .map(|dir| dir.join("opencode")).find(|candidate| candidate.is_file())) else { return };
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let local = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = local.local_addr().unwrap().port();
    std::fs::write(repo.join("opencode.json"), json!({"$schema":"https://opencode.ai/config.json","provider":{
        "local_a":{"npm":"@ai-sdk/openai-compatible","options":{"baseURL":format!("http://127.0.0.1:{port}/v1")},
            "models":{"fixture-a":{"name":"Fixture A","tool_call":true}}},
        "local_b":{"npm":"@ai-sdk/openai-compatible","options":{"baseURL":"http://127.0.0.1:1/v1"},
            "models":{"fixture-b":{"name":"Fixture B","tool_call":true}}}
    },"model":"local_a/fixture-a","small_model":"local_a/fixture-a","autoupdate":false,"share":"disabled"}).to_string()).unwrap();
    let config_home = r.path().join("config");
    let data_home = r.path().join("data");
    let cache_home = r.path().join("cache");
    for path in [&config_home,&data_home,&cache_home] { std::fs::create_dir(path).unwrap(); }
    let d = Daemon::start(&[("OVERSEER_OPENCODE_PATH",program.to_str().unwrap()),
        ("XDG_CONFIG_HOME",config_home.to_str().unwrap()),
        ("XDG_DATA_HOME",data_home.to_str().unwrap()),
        ("XDG_CACHE_HOME",cache_home.to_str().unwrap())]);
    let created = d.call("task.create", json!({"repo":repo,"workspace_mode":"current",
        "harness":"generic","program":"/bin/true","prompt":"metadata workspace"}));
    let workspace_id = created["workspace"]["id"].as_str().unwrap();
    let inspected = d.call("auto.opencode.local.inspect",
        json!({"profile_id":"system-opencode","workspace_id":workspace_id}));
    let models = inspected["catalog"]["models"].as_array().unwrap();
    assert_eq!(models.len(), 2, "{inspected}");
    assert_eq!(models[0]["model"], "local_a/fixture-a");
    assert_eq!(models[1]["model"], "local_b/fixture-b");
    assert_eq!(inspected["endpoint_health"]["local_a"], "reachable");
    assert_eq!(inspected["endpoint_health"]["local_b"], "unavailable");
    assert_eq!(inspected["allowance"], "unknown",
        "a loopback address alone does not prove the server is free local inference rather than a cloud proxy");
    assert_eq!(inspected["local_execution_config_verified"]["local_a/fixture-a"], false,
        "a system profile may have uninspected cloud credentials");
    assert!(!inspected.to_string().contains("openai/"));
    let isolated = d.call("profile.create", json!({"name":"Isolated local provider",
        "harness":"opencode"}));
    let profile_id = isolated["id"].as_str().unwrap();
    let isolated_read = d.call("auto.opencode.local.inspect", json!({"profile_id":profile_id,
        "workspace_id":workspace_id}));
    assert_eq!(isolated_read["local_execution_config_verified"]["local_a/fixture-a"], true,
        "credential-free isolated local provider can be considered for a future Auto route: {isolated_read}");
    assert_eq!(isolated_read["local_execution_config_verified"]["local_b/fixture-b"], true,
        "the process-scoped override can select a second verified local route");
    assert_eq!(isolated_read["endpoint_health"]["local_b"], "unavailable");
    let auth_file = Path::new(isolated["home"].as_str().unwrap()).join("data/opencode/auth.json");
    std::fs::create_dir_all(auth_file.parent().unwrap()).unwrap();
    std::fs::write(&auth_file, "secret-auth-sentinel").unwrap();
    let credentialed = d.try_call("auto.opencode.local.inspect", json!({"profile_id":profile_id,
        "workspace_id":workspace_id}));
    match credentialed {
        Ok(value) => {
            assert_eq!(value["local_execution_config_verified"]["local_a/fixture-a"], false);
            assert!(!value.to_string().contains("secret-auth-sentinel"));
        }
        Err(reason) => assert!(!reason.contains("secret-auth-sentinel"), "{reason}"),
    }
}

#[test]
fn auto_opencode_isolated_local_provider_executes_real_harness_against_a_mock_endpoint() {
    let Some(program) = std::env::var_os("PATH").and_then(|path| std::env::split_paths(&path)
        .map(|dir| dir.join("opencode")).find(|candidate| candidate.is_file())) else { return };
    struct MockServer(std::process::Child);
    impl Drop for MockServer {
        fn drop(&mut self) { let _ = self.0.kill(); let _ = self.0.wait(); }
    }
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let port_file = r.path().join("mock-port");
    let mock_log = r.path().join("mock-log.jsonl");
    let _mock = MockServer(std::process::Command::new("node")
        .arg(fixture("mock-openai/server.js"))
        .env("MOCK_PORT_FILE", &port_file).env("MOCK_LOG", &mock_log)
        .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null())
        .spawn().unwrap());
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !port_file.exists() {
        assert!(std::time::Instant::now() < deadline, "local mock provider did not start");
        std::thread::sleep(Duration::from_millis(10));
    }
    let port = std::fs::read_to_string(&port_file).unwrap();
    std::fs::write(repo.join("opencode.json"), json!({"provider":{
        "local_a":{"npm":"@ai-sdk/openai-compatible",
            "options":{"baseURL":format!("http://127.0.0.1:{port}/v1")},
            "models":{"fixture-a":{"name":"Fixture A","tool_call":true}}}
    },"model":"local_a/fixture-a","small_model":"local_a/fixture-a",
        "autoupdate":false,"share":"disabled"}).to_string()).unwrap();
    git(&repo, &["add", "opencode.json"]);
    git(&repo, &["commit", "-q", "-m", "local provider fixture"]);
    let d = Daemon::start(&[("OVERSEER_OPENCODE_PATH",program.to_str().unwrap())]);
    let profile = d.call("profile.create", json!({"name":"Mock local","harness":"opencode"}));
    let created = d.call("task.create", json!({"repo":repo,"harness":"opencode",
        "profile_id":profile["id"],"model":"local_a/fixture-a","prompt":"reply hello"}));
    let run = run_id(&created);
    let done = d.wait_done(&run, 60);
    assert_eq!(done["status"], "completed", "{done} events: {:?}", d.events(&run));
    assert!(d.events(&run).iter().any(|event| event["kind"] == "output"
        && event["payload"]["text"].as_str().is_some_and(|text| text.contains("hello from mock"))));
    assert!(std::fs::read_to_string(&mock_log).unwrap().contains("/v1/chat/completions"));
}

#[test]
fn auto_selected_opencode_child_uses_guarded_alternate_local_endpoint() {
    let Some(program) = std::env::var_os("PATH").and_then(|path| std::env::split_paths(&path)
        .map(|dir| dir.join("opencode")).find(|candidate| candidate.is_file())) else { return };
    struct MockServer(std::process::Child);
    impl Drop for MockServer {
        fn drop(&mut self) { let _ = self.0.kill(); let _ = self.0.wait(); }
    }
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let port_file = r.path().join("mock-port");
    let mock_log = r.path().join("mock-log.jsonl");
    let _mock = MockServer(std::process::Command::new("node")
        .arg(fixture("mock-openai/server.js"))
        .env("MOCK_PORT_FILE", &port_file).env("MOCK_LOG", &mock_log)
        .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null())
        .spawn().unwrap());
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !port_file.exists() {
        assert!(std::time::Instant::now() < deadline, "local mock provider did not start");
        std::thread::sleep(Duration::from_millis(10));
    }
    let alternate = format!("http://127.0.0.1:{}/v1", std::fs::read_to_string(&port_file).unwrap());
    let config = json!({"$schema":"https://opencode.ai/config.json","provider":{
        "local_a":{"npm":"@ai-sdk/openai-compatible","options":{"baseURL":"http://127.0.0.1:1/v1"},
            "models":{"fixture-a":{"name":"Fixture A","tool_call":true}}},
        "local_b":{"npm":"@ai-sdk/openai-compatible","options":{"baseURL":alternate},
            "models":{"fixture-b":{"name":"Fixture B","tool_call":true}}}
    },"model":"local_a/fixture-a","small_model":"local_a/fixture-a",
        "autoupdate":false,"share":"disabled"});
    std::fs::write(repo.join("opencode.json"), config.to_string()).unwrap();
    git(&repo, &["add", "opencode.json"]);
    git(&repo, &["commit", "-q", "-m", "local providers"]);
    let d = Daemon::start(&[("OVERSEER_OPENCODE_PATH", program.to_str().unwrap())]);
    let profile = d.call("profile.create", json!({"name":"Isolated local","harness":"opencode"}));
    let parent = run_id(&sh(&d, &repo, "current", "echo ready"));
    assert_eq!(d.wait_done(&parent, 10)["status"], "completed");
    let delegated = d.call("run.delegate", json!({"work_unit_id":"alternate-local-1",
        "parent_run_id":parent,"harness":"opencode","profile_id":profile["id"],
        "model":"local_b/fixture-b","effort":"default","prompt":"reply hello",
        "title":"local child","auto_selected":true,
        "requirements_hash":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "auto_local_endpoint":alternate}));
    assert!(delegated.get("launch_error").is_none(), "{delegated}");
    let child_id = run_id(&delegated);
    let done = d.wait_done(&child_id, 60);
    assert_eq!(done["status"], "completed", "{done} events: {:?}", d.events(&child_id));
    assert!(std::fs::read_to_string(&mock_log).unwrap().contains("/v1/chat/completions"));
    assert!(!Path::new(delegated["workspace"]["path"].as_str().unwrap()).join("forbidden.txt").exists());
    let child_config: serde_json::Value = serde_json::from_slice(&std::fs::read(
        Path::new(delegated["workspace"]["path"].as_str().unwrap()).join("opencode.json")).unwrap()).unwrap();
    assert_eq!(child_config, config, "Auto must not mutate the project provider configuration");
    // The first real OpenCode run may create profile config asynchronously.
    // Test endpoint refusal through a fresh profile so that the endpoint guard,
    // rather than the independent profile-config guard, is the failing boundary.
    let refused_profile = d.call("profile.create", json!({"name":"Unreachable local","harness":"opencode"}));
    let refused = d.try_call("run.delegate", json!({"work_unit_id":"refused-local-2",
        "parent_run_id":parent,"harness":"opencode","profile_id":refused_profile["id"],
        "model":"local_a/fixture-a","effort":"default","prompt":"reply hello",
        "title":"unreachable local child","auto_selected":true,
        "requirements_hash":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        "auto_local_endpoint":"http://127.0.0.1:1/v1"}));
    let refused = refused.unwrap_err();
    assert!(refused.contains("unavailable before child creation"), "{refused}");
    let auth_file = Path::new(profile["home"].as_str().unwrap()).join("data/opencode/auth.json");
    std::fs::create_dir_all(auth_file.parent().unwrap()).unwrap();
    std::fs::write(&auth_file, "secret-auth-sentinel").unwrap();
    let credentialed = d.try_call("run.delegate", json!({"work_unit_id":"credentialed-local-3",
        "parent_run_id":parent,"harness":"opencode","profile_id":profile["id"],
        "model":"local_b/fixture-b","effort":"default","prompt":"reply hello",
        "title":"credentialed local child","auto_selected":true,
        "requirements_hash":"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
        "auto_local_endpoint":alternate}));
    let credentialed = credentialed.unwrap_err();
    assert!(credentialed.contains("contains credentials"), "{credentialed}");
    assert!(!credentialed.contains("secret-auth-sentinel"));
    assert_eq!(d.runs().len(), 2, "failed preflights must not create managed children");
}

#[test]
fn auto_dispatch_selects_reachable_local_opencode_provider_after_another_fails() {
    let Some(program) = std::env::var_os("PATH").and_then(|path| std::env::split_paths(&path)
        .map(|dir| dir.join("opencode")).find(|candidate| candidate.is_file())) else { return };
    struct MockServer(std::process::Child);
    impl Drop for MockServer {
        fn drop(&mut self) { let _ = self.0.kill(); let _ = self.0.wait(); }
    }
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let port_file = r.path().join("mock-port");
    let mock_log = r.path().join("mock-log.jsonl");
    let _mock = MockServer(std::process::Command::new("node")
        .arg(fixture("mock-openai/server.js"))
        .env("MOCK_PORT_FILE", &port_file).env("MOCK_LOG", &mock_log)
        .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null())
        .spawn().unwrap());
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !port_file.exists() {
        assert!(std::time::Instant::now() < deadline, "local mock provider did not start");
        std::thread::sleep(Duration::from_millis(10));
    }
    let reachable = format!("http://127.0.0.1:{}/v1", std::fs::read_to_string(&port_file).unwrap());
    let quota_mode = r.path().join("quota-mode.txt");
    std::fs::write(&quota_mode, "available").unwrap();
    std::fs::write(repo.join("opencode.json"), json!({
        "$schema":"https://opencode.ai/config.json","provider":{
            "local_a":{"npm":"@ai-sdk/openai-compatible",
                "options":{"baseURL":"http://127.0.0.1:1/v1"},
                "models":{"gpt-oss-120b":{"name":"Local A","tool_call":true,"reasoning":true}}},
            "local_b":{"npm":"@ai-sdk/openai-compatible",
                "options":{"baseURL":reachable},
                "models":{"gpt-oss-120b":{"name":"Local B","tool_call":true,"reasoning":true}}}
        },"model":"local_a/gpt-oss-120b","small_model":"local_a/gpt-oss-120b",
        "autoupdate":false,"share":"disabled"}).to_string()).unwrap();
    git(&repo, &["add", "opencode.json"]);
    git(&repo, &["commit", "-q", "-m", "local route candidates"]);
    let d = Daemon::start(&[("OVERSEER_CODEX_PATH", &fixture("fake-harness/codex-app-fixture.js")),
        ("OVERSEER_OPENCODE_PATH", program.to_str().unwrap()),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE,FIXTURE_QUOTA_MODE_FILE"),
        ("FIXTURE_MODE", "managed-models"),
        ("FIXTURE_QUOTA_MODE_FILE", quota_mode.to_str().unwrap())]);
    let local = d.call("profile.create", json!({"name":"Local Auto","harness":"opencode"}));
    let parent = run_id(&d.call("task.create", json!({"repo":repo,"harness":"codex-app",
        "model":"gpt-6-astra","effort":"high","prompt":"parent checkpoint",
        "approval_policy":"never"})));
    assert_eq!(d.wait_done(&parent, 15)["status"], "completed");
    let request = json!({"work_unit_id":"local-auto-route-1","parent_run_id":parent,
        "min_tier":"general","required_tools":[],"sandbox":"read_only",
        "allowed_profiles":[local["id"]],"preferred_harness":"opencode",
        "prompt":"reply hello","title":"local summary"});
    let selected = d.call("auto.dispatch", request.clone());
    assert_eq!(selected["state"], "dispatched", "{selected}");
    assert_eq!(selected["run"]["harness"], "opencode");
    assert_eq!(selected["decision"]["selected"],
        format!("{}/local_b/gpt-oss-120b/default", local["id"].as_str().unwrap()));
    let excluded = selected["decision"]["exclusions"].as_array().unwrap();
    assert!(excluded.iter().any(|item| item["reason"] == "route_unavailable"
        && item["route_id"].as_str().unwrap().contains("local_a")), "{selected}");
    let child_id = run_id(&selected);
    assert_eq!(d.wait_done(&child_id, 60)["status"], "completed");
    assert!(std::fs::read_to_string(&mock_log).unwrap().contains("/v1/chat/completions"));
    let isolated_home = Path::new(local["home"].as_str().unwrap());
    let stats = std::process::Command::new(&program).args(["stats", "--pure", "--models", "5"])
        .current_dir(&repo)
        .env("XDG_DATA_HOME", isolated_home.join("data"))
        .env("XDG_CONFIG_HOME", isolated_home.join("config"))
        .env("XDG_STATE_HOME", isolated_home.join("state"))
        .env("XDG_CACHE_HOME", isolated_home.join("cache"))
        .env("OPENCODE_DISABLE_AUTOUPDATE", "1")
        .env("OPENCODE_DISABLE_MODELS_FETCH", "1")
        .output().unwrap();
    assert!(stats.status.success(), "OpenCode stats failed: {}", String::from_utf8_lossy(&stats.stderr));
    let stats_text = String::from_utf8_lossy(&stats.stdout);
    assert!(stats_text.contains("gpt-oss-120b"), "controlled task did not appear in OpenCode stats: {stats_text}");
    let after_stats = d.call("auto.opencode.local.inspect", json!({"profile_id":local["id"],
        "workspace_id":d.run(&parent)["workspace_id"]}));
    assert_eq!(after_stats["allowance"], "unknown",
        "token/cost statistics cannot become an invented subscription balance");
    let decision_event = d.events(&parent).into_iter()
        .find(|event| event["kind"] == "auto_decision").unwrap();
    assert_eq!(decision_event["payload"]["candidates"].as_array().unwrap().iter()
        .find(|item| item["id"] == selected["decision"]["selected"]).unwrap()["quota"], "unknown");
    assert_eq!(d.call("auto.decision.replay", json!({"event_seq":decision_event["seq"]}))["matches_recorded"], true);
    assert_eq!(d.call("auto.dispatch", request)["run"]["id"], child_id);
    let write_request = d.call("auto.dispatch", json!({"work_unit_id":"local-auto-write-2",
        "parent_run_id":parent,"min_tier":"general","required_tools":[],
        "allowed_profiles":[local["id"]],"prompt":"write a file","title":"write"}));
    assert_eq!(write_request["state"], "paused", "{write_request}");
    assert!(write_request["decision"]["exclusions"].as_array().unwrap().iter()
        .any(|item| item["reason"] == "sandbox_incompatible"));
    std::fs::write(&quota_mode, "exhausted").unwrap();
    let unresolved = d.call("auto.dispatch", json!({"work_unit_id":"local-auto-quota-3",
        "parent_run_id":parent,"min_tier":"general","required_tools":[],
        "sandbox":"read_only","allowed_profiles":["system-codex",local["id"]],
        "prompt":"reply hello","title":"quota identity"}));
    assert_eq!(unresolved["state"], "paused", "{unresolved}");
    assert!(unresolved["decision"]["exclusions"].as_array().unwrap().iter()
        .any(|item| item["reason"] == "unresolved_quota_pool_identity"
            && item["route_id"].as_str().unwrap().contains("local_b")), "{unresolved}");
    assert_eq!(d.runs().len(), 2, "neither rejected unit may create a child");
}

#[test]
fn auto_opencode_silent_503_budget_stops_while_daemon_is_down_without_duplicate() {
    let Some(program) = std::env::var_os("PATH").and_then(|path| std::env::split_paths(&path)
        .map(|dir| dir.join("opencode")).find(|candidate| candidate.is_file())) else { return };
    struct MockServer(std::process::Child);
    impl Drop for MockServer {
        fn drop(&mut self) { let _ = self.0.kill(); let _ = self.0.wait(); }
    }
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let start = |name: &str, status: Option<&str>| {
        let port_file = r.path().join(format!("{name}-port"));
        let log_file = r.path().join(format!("{name}-log"));
        let mut command = std::process::Command::new("node");
        command.arg(fixture("mock-openai/server.js"))
            .env("MOCK_PORT_FILE", &port_file).env("MOCK_LOG", &log_file)
            .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
        if let Some(status) = status { command.env("MOCK_HTTP_STATUS", status); }
        let server = MockServer(command.spawn().unwrap());
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !port_file.exists() {
            assert!(std::time::Instant::now() < deadline, "{name} mock provider did not start");
            std::thread::sleep(Duration::from_millis(10));
        }
        let endpoint = format!("http://127.0.0.1:{}/v1",
            std::fs::read_to_string(&port_file).unwrap());
        (server, endpoint, log_file)
    };
    let (_failing, failed_endpoint, failed_log) = start("failing", Some("503"));
    let (_healthy, healthy_endpoint, healthy_log) = start("healthy", None);
    std::fs::write(repo.join("opencode.json"), json!({
        "$schema":"https://opencode.ai/config.json","provider":{
            "local_a":{"npm":"@ai-sdk/openai-compatible","options":{"baseURL":failed_endpoint},
                "models":{"gpt-oss-120b":{"name":"Local A","tool_call":true,"reasoning":true}}},
            "local_b":{"npm":"@ai-sdk/openai-compatible","options":{"baseURL":healthy_endpoint},
                "models":{"gpt-oss-120b":{"name":"Local B","tool_call":true,"reasoning":true}}}
        },"model":"local_a/gpt-oss-120b","small_model":"local_a/gpt-oss-120b",
        "autoupdate":false,"share":"disabled"}).to_string()).unwrap();
    git(&repo, &["add", "opencode.json"]);
    git(&repo, &["commit", "-q", "-m", "two reachable local routes"]);
    let mut d = Daemon::start(&[("OVERSEER_CODEX_PATH", &fixture("fake-harness/codex-app-fixture.js")),
        ("OVERSEER_OPENCODE_PATH", program.to_str().unwrap()),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE"),
        ("FIXTURE_MODE", "managed-models")]);
    let profile = d.call("profile.create", json!({"name":"Local 503","harness":"opencode"}));
    let parent = run_id(&d.call("task.create", json!({"repo":repo,"harness":"codex-app",
        "model":"gpt-6-astra","effort":"high","prompt":"parent checkpoint",
        "approval_policy":"never"})));
    assert_eq!(d.wait_done(&parent, 15)["status"], "completed");
    let request = |unit: &str| json!({"work_unit_id":unit,"parent_run_id":parent,
        "min_tier":"general","required_tools":[],"sandbox":"read_only",
        "allowed_profiles":[profile["id"]],"prompt":"reply hello","title":"local check",
        "execution_budget_ms":5000});
    for invalid in [json!(0), json!("5000"), json!(1_800_001)] {
        let mut rejected = request("local-503-invalid-budget");
        rejected["execution_budget_ms"] = invalid;
        assert!(d.try_call("auto.dispatch", rejected).is_err());
    }
    // A cold installed CLI can time out during metadata discovery in the
    // serial suite. That must pause without a child; a fresh unit may retry.
    let mut first = None;
    let mut first_unit = String::new();
    for attempt in 0..3 {
        let unit = format!("local-503-first-{attempt}");
        let result = d.call("auto.dispatch", request(&unit));
        if result["state"] == "dispatched" {
            first = Some(result);
            first_unit = unit;
            break;
        }
        assert_eq!(result["state"], "paused", "{result}");
        assert!(result["discovery_failures"].as_array().is_some_and(|failures|
            failures.iter().any(|failure| failure["reason"] == "metadata_or_auth_unavailable")),
            "only transient metadata discovery may be retried: {result}");
        assert_eq!(d.runs().len(), 1, "metadata timeout cannot launch a child");
    }
    let first = first.expect("installed OpenCode metadata did not recover in three bounded attempts");
    assert!(first["decision"]["selected"].as_str().unwrap().contains("local_a"));
    let first_id = run_id(&first);
    let request_deadline = std::time::Instant::now() + Duration::from_secs(8);
    loop {
        if std::fs::read_to_string(&failed_log).unwrap_or_default()
            .contains("/v1/chat/completions") { break; }
        assert!(std::time::Instant::now() < request_deadline,
            "OpenCode never requested the failing local endpoint");
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(d.events(&first_id).iter().all(|event| event["kind"] != "error"),
        "the installed OpenCode CLI has not surfaced its retry as a classified error yet");
    // The installed CLI retries a 503 without emitting an error promptly.
    // The supervisor must enforce the original deadline without a daemon.
    d.kill9();
    let process_dir = d.home.path().join("runs").join(&first_id).join("p1");
    let offline_deadline = std::time::Instant::now() + Duration::from_secs(12);
    while !process_dir.join("exit.json").exists() {
        assert!(std::time::Instant::now() < offline_deadline,
            "the child outlived its budget while the daemon was down");
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(process_dir.join("auto-budget.requested").exists());
    d.spawn();
    let ended = d.wait_done(&first_id, 20);
    assert_eq!(ended["status"], "failed", "{ended}");
    assert!(ended["exit_reason"].as_str().unwrap_or_default().contains("execution budget"));
    assert_eq!(d.events(&first_id).iter().filter(|event|
        event["kind"] == "auto_execution_budget_exhausted").count(), 1,
        "reattachment should publish one budget outcome");
    let replay = d.call("auto.dispatch", request(&first_unit));
    assert_eq!(replay["state"], "paused", "{replay}");
    assert_eq!(replay["run"]["id"], first_id);
    let mut changed = request(&first_unit);
    changed["execution_budget_ms"] = json!(6000);
    assert!(d.try_call("auto.dispatch", changed).is_err(),
        "replay must not change the persisted execution deadline");
    assert!(!healthy_log.exists(), "an uncertain stopped unit cannot silently try another endpoint");
    assert_eq!(d.runs().len(), 2, "expired work must not create a second child");
}

#[test]
fn auto_claude_native_quota_keeps_model_scope_and_drops_raw_provider_text() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let d = claude_daemon("native-quota");
    let created = d.call("task.create", json!({"repo":repo,"harness":"claude",
        "model":"claude-opus-4-5","prompt":"inspect native quota","title":"quota event"}));
    let run = run_id(&created);
    assert_eq!(d.wait_done(&run, 15)["status"], "completed");
    let opus = d.call("auto.quota.state", json!({"profile_id":"system-claude",
        "harness":"claude","model":"claude-opus-4-5"}));
    let sonnet = d.call("auto.quota.state", json!({"profile_id":"system-claude",
        "harness":"claude","model":"claude-sonnet-4-5"}));
    assert_eq!(opus["state"], "exhausted", "{opus}");
    assert_eq!(sonnet["state"], "observed_non_exhausted", "{sonnet}");
    assert_eq!(opus["observation"]["source"], "claude/native-rate-limit-event");
    assert_eq!(opus["observation"]["snapshot"]["windows"].as_array().unwrap().len(), 3);
    assert!(!opus.to_string().contains("secret-quota-sentinel"));
    assert!(!serde_json::to_string(&d.events(&run)).unwrap().contains("secret-quota-sentinel"));
}

#[test]
fn auto_claude_malformed_new_meter_supersedes_older_capacity_with_unknown() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let d = claude_daemon("native-quota-invalid");
    let created = d.call("task.create", json!({"repo":repo,"harness":"claude",
        "prompt":"inspect invalid meter","title":"quota drift"}));
    let run = run_id(&created);
    assert_eq!(d.wait_done(&run, 15)["status"], "completed");
    let state = d.call("auto.quota.state", json!({"profile_id":"system-claude",
        "harness":"claude","model":"claude-sonnet-4-5"}));
    assert_eq!(state["state"], "unknown", "{state}");
    assert!(state["observation"]["snapshot"]["windows"].as_array().unwrap().is_empty());
    assert!(!state.to_string().contains("secret-invalid-meter"));
}

#[test]
fn auto_claude_malformed_followup_does_not_clear_a_native_rejection() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let d = claude_daemon("native-quota-block-invalid");
    let created = d.call("task.create", json!({"repo":repo,"harness":"claude",
        "prompt":"inspect rejected meter","title":"quota block"}));
    let run = run_id(&created);
    assert_eq!(d.wait_done(&run, 15)["status"], "completed");
    let opus = d.call("auto.quota.state", json!({"profile_id":"system-claude",
        "harness":"claude","model":"claude-opus-4-5"}));
    let sonnet = d.call("auto.quota.state", json!({"profile_id":"system-claude",
        "harness":"claude","model":"claude-sonnet-4-5"}));
    assert_eq!(opus["state"], "exhausted", "{opus}");
    assert_eq!(sonnet["state"], "unknown", "{sonnet}");
}

#[test]
fn auto_dispatch_selects_managed_children_for_different_healthy_work_units_and_pauses_on_exhaustion() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let trace = r.path().join("auto-dispatch-trace.txt");
    let quota = r.path().join("quota-mode.txt");
    std::fs::write(&quota, "available").unwrap();
    let mut d = Daemon::start(&[("OVERSEER_CODEX_PATH", &fixture("fake-harness/codex-app-fixture.js")),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE,FIXTURE_TRACE_FILE,FIXTURE_QUOTA_MODE_FILE"),
        ("FIXTURE_MODE", "managed-models"), ("FIXTURE_TRACE_FILE", trace.to_str().unwrap()),
        ("FIXTURE_QUOTA_MODE_FILE", quota.to_str().unwrap())]);
    let parent = run_id(&d.call("task.create", json!({"repo":repo,"harness":"codex-app",
        "model":"gpt-6-astra","effort":"high","prompt":"seed context","approval_policy":"never"})));
    assert_eq!(d.wait_done(&parent, 15)["status"], "completed");
    for (unit, extra) in [
        ("bad-context", json!({"context_needed":"unknown"})),
        ("bad-approvals", json!({"requires_approvals":"yes"})),
        ("bad-pin", json!({"pinned_route":17})),
        ("unsupported-profile", json!({"profile_id":"another-account"})),
        ("unsupported-pool", json!({"allowed_profiles":["another-account"]})),
        ("unsupported-model", json!({"model":"gpt-6-astra"})),
    ] {
        let mut invalid = json!({"work_unit_id":unit,"parent_run_id":parent,
            "min_tier":"general","required_tools":[],"prompt":"browser check"});
        invalid.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        assert!(d.try_call("auto.dispatch", invalid).is_err(), "{unit} must not discard a constraint");
    }
    assert_eq!(d.runs().len(), 1, "invalid constraints must fail before launching a child");
    let browser_request = json!({"work_unit_id":"auto-browser-1","parent_run_id":parent,
        "min_tier":"general","required_tools":["browser/navigate"],"prompt":"browser check","title":"browser check"});
    let browser = d.call("auto.dispatch", browser_request.clone());
    assert_eq!(browser["state"], "dispatched", "{browser}");
    assert_eq!(browser["decision"]["selected"], "system-codex/gpt-6-sol/medium");
    let decision_event = d.events(&parent).into_iter()
        .find(|event| event["kind"] == "auto_decision").unwrap();
    let replay = d.call("auto.decision.replay", json!({"event_seq":decision_event["seq"]}));
    assert_eq!(replay["matches_recorded"], true, "{replay}");
    assert_eq!(replay["decision"]["selected"], browser["decision"]["selected"]);
    let recorded = &decision_event["payload"];
    assert_eq!(recorded["selected_route"]["harness"], "codex-app");
    assert_eq!(recorded["selected_route"]["model"], "gpt-6-sol");
    assert_eq!(recorded["selected_route"]["effort"], "medium");
    assert_eq!(recorded["selected_route"]["fit"], "unknown");
    assert_eq!(recorded["estimator"]["state"], "unavailable");
    assert!(recorded["estimator"]["version"].is_null());
    assert_eq!(recorded["inference"]["state"], "not_used");
    assert!(recorded["inference"]["output"].is_null());
    assert!(!decision_event["payload"].to_string().contains("browser check"),
        "the decision trace must not store the work prompt or title");
    let browser_id = run_id(&browser);
    assert_eq!(d.wait_done(&browser_id, 15)["status"], "completed");
    assert_eq!(d.call("run.result", json!({"run_id":browser_id}))["state"], "ready");
    d.kill9();
    d.spawn();
    let repeated = d.call("auto.dispatch", browser_request);
    assert_eq!(repeated["run"]["id"], browser_id);
    assert_eq!(repeated["replayed"], true);
    let diagnosis = d.call("auto.dispatch", json!({"work_unit_id":"auto-diagnosis-2","parent_run_id":parent,
        "min_tier":"frontier","required_tools":[],"prompt":"diagnose the result","title":"diagnosis"}));
    assert_eq!(diagnosis["decision"]["selected"], "system-codex/gpt-6-astra/high");
    let diagnosis_id = run_id(&diagnosis);
    assert_eq!(d.wait_done(&diagnosis_id, 15)["status"], "completed");
    std::fs::write(&quota, "exhausted").unwrap();
    let paused = d.call("auto.dispatch", json!({"work_unit_id":"auto-paused-3","parent_run_id":parent,
        "min_tier":"general","required_tools":[],"prompt":"next work","title":"next"}));
    assert_eq!(paused["state"], "paused", "{paused}");
    assert!(paused["decision"]["selected"].is_null());
    let paused_event = d.events(&parent).into_iter().rev()
        .find(|event| event["kind"] == "auto_decision").unwrap();
    assert!(paused_event["payload"]["selected_route"].is_null(),
        "a paused decision must not claim a chosen model");
    assert_eq!(d.runs().len(), 3, "exhaustion must not start another child");
    let trace = std::fs::read_to_string(trace).unwrap();
    assert_eq!(trace.matches("turn_model:gpt-6-sol").count(), 1);
    assert_eq!(trace.matches("turn_model:gpt-6-astra").count(), 2);
}

#[test]
fn auto_cold_start_discloses_unknown_allowance_and_rejects_invented_inference() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let quota = r.path().join("quota-mode.txt");
    let tools = r.path().join("tool-mode.txt");
    std::fs::write(&quota, "unknown").unwrap();
    std::fs::write(&tools, "available").unwrap();
    let d = Daemon::start(&[("OVERSEER_CODEX_PATH", &fixture("fake-harness/codex-app-fixture.js")),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE,FIXTURE_QUOTA_MODE_FILE,FIXTURE_TOOL_MODE_FILE"),
        ("FIXTURE_MODE", "managed-models"), ("FIXTURE_QUOTA_MODE_FILE", quota.to_str().unwrap()),
        ("FIXTURE_TOOL_MODE_FILE", tools.to_str().unwrap())]);
    let parent = run_id(&d.call("task.create", json!({"repo":repo,"harness":"codex-app",
        "model":"gpt-6-astra","effort":"high","prompt":"seed context"})));
    assert_eq!(d.wait_done(&parent, 15)["status"], "completed");
    assert!(d.call("auto.usage.list", json!({}))["measurements"].as_array().unwrap().is_empty(),
        "the cold-start decision must have no learned usage history");

    let request = json!({"work_unit_id":"cold-start-browser","parent_run_id":parent,
        "min_tier":"general","required_tools":["browser/navigate"],"prompt":"browser check"});
    let mut invented = request.clone();
    invented["inference_result"] = json!({"allowance":"free","model":"gpt-6-astra"});
    assert!(d.try_call("auto.dispatch", invented).is_err(),
        "caller-supplied inference cannot invent an account allowance");
    assert_eq!(d.runs().len(), 1);

    let selected = d.call("auto.dispatch", request);
    assert_eq!(selected["state"], "dispatched", "{selected}");
    assert_eq!(selected["decision"]["reason"], "cold_start_allowance_unknown", "{selected}");
    let event = d.events(&parent).into_iter().find(|event| event["kind"] == "auto_decision").unwrap();
    assert_eq!(event["payload"]["selected_route"]["quota"], "unknown");
    assert_eq!(event["payload"]["selected_route"]["fit"], "unknown");
    assert_eq!(event["payload"]["inference"]["state"], "not_used");
    assert_eq!(d.wait_done(&run_id(&selected), 15)["status"], "completed");

    std::fs::write(&tools, "missing").unwrap();
    let paused = d.call("auto.dispatch", json!({"work_unit_id":"cold-start-no-browser",
        "parent_run_id":parent,"min_tier":"general","required_tools":["browser/navigate"],
        "prompt":"another browser check"}));
    assert_eq!(paused["state"], "paused", "{paused}");
    assert!(paused["decision"]["selected"].is_null());
    assert_eq!(d.runs().len(), 2, "unsupported tools must not launch a second child");
}

#[test]
fn auto_ordinary_child_failure_stays_failed_until_the_user_requests_new_work() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let claude_mode = r.path().join("claude-mode.txt");
    std::fs::write(&claude_mode, "prose").unwrap();
    let d = Daemon::start(&[("OVERSEER_CODEX_PATH", &fixture("fake-harness/codex-app-fixture.js")),
        ("OVERSEER_CLAUDE_PATH", &fixture("fake-harness/claude-fixture.js")),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE,CLAUDE_FIXTURE_MODE_FILE"),
        ("FIXTURE_MODE", "managed-models"),
        ("CLAUDE_FIXTURE_MODE_FILE", claude_mode.to_str().unwrap())]);
    let parent = run_id(&d.call("task.create", json!({"repo":repo,"harness":"codex-app",
        "model":"gpt-6-astra","effort":"high","prompt":"seed context"})));
    assert_eq!(d.wait_done(&parent, 15)["status"], "completed");
    let warmup = run_id(&d.call("task.create", json!({"repo":repo,"harness":"claude",
        "model":"sonnet","effort":"medium","prompt":"independent completed work"})));
    assert_eq!(d.wait_done(&warmup, 15)["status"], "completed");
    std::fs::write(&claude_mode, "ordinary-failure").unwrap();
    let request = json!({"work_unit_id":"ordinary-failure-child-1","parent_run_id":parent,
        "min_tier":"general","required_tools":[],"allowed_profiles":["system-claude","system-codex"],
        "preferred_harness":"claude","prompt":"perform a bounded check"});
    let dispatched = d.call("auto.dispatch", request.clone());
    assert_eq!(dispatched["state"], "dispatched", "{dispatched}");
    assert_eq!(dispatched["run"]["harness"], "claude");
    let child = run_id(&dispatched);
    assert_eq!(d.wait_done(&child, 15)["status"], "failed");
    let observations = d.call("auto.usage.work.list", json!({}));
    let failed = observations["work_units"].as_array().unwrap().iter()
        .find(|row| row["run_id"] == child).unwrap();
    assert_eq!(failed["status"], "failed", "failed work must remain in local outcome learning");
    assert_eq!(failed["subscription_window_draw"], "unverified",
        "failed work's token activity is not a subscription charge");
    assert_eq!(d.call("run.result", json!({"run_id":child}))["state"], "not_completed");
    assert!(d.events(&child).iter().any(|event|
        event["kind"] == "error" && event["payload"]["class"] == "other"));
    let replay = d.call("auto.dispatch", request);
    assert_eq!(replay["state"], "paused", "{replay}");
    assert_eq!(replay["run"]["id"], child);
    assert_eq!(d.runs().len(), 3, "an ordinary task failure must not launch the eligible alternate");
    assert!(d.events(&parent).iter().all(|event|
        event["kind"] != "managed_child_result_available" || event["payload"]["child_run_id"] != child));

    let next = d.call("auto.dispatch", json!({"work_unit_id":"ordinary-failure-next-2",
        "parent_run_id":parent,"min_tier":"general","required_tools":[],
        "allowed_profiles":["system-codex"],"prompt":"newly requested independent work"}));
    assert_eq!(next["state"], "dispatched", "{next}");
    let next_child = run_id(&next);
    assert_eq!(d.wait_done(&next_child, 15)["status"], "completed");
    assert_eq!(d.runs().len(), 4);
}

#[test]
fn auto_dispatch_can_choose_an_explicitly_allowed_claude_child_and_reject_missing_browser_tools() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let mut d = Daemon::start(&[("OVERSEER_CODEX_PATH", &fixture("fake-harness/codex-app-fixture.js")),
        ("OVERSEER_CLAUDE_PATH", &fixture("fake-harness/claude-fixture.js")),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE,CLAUDE_FIXTURE_MODE"),
        ("FIXTURE_MODE", "managed-models"), ("CLAUDE_FIXTURE_MODE", "prose")]);
    let parent = run_id(&d.call("task.create", json!({"repo":repo,"harness":"codex-app",
        "model":"gpt-6-astra","effort":"high","prompt":"seed context","approval_policy":"never"})));
    assert_eq!(d.wait_done(&parent, 15)["status"], "completed");
    let request = json!({"work_unit_id":"cross-claude-1","parent_run_id":parent,
        "min_tier":"general","required_tools":[],"allowed_profiles":["system-claude"],
        "preferred_harness":"claude","prompt":"summarize the bounded result","title":"summary"});
    let child = d.call("auto.dispatch", request.clone());
    assert_eq!(child["state"], "dispatched", "{child}");
    assert_eq!(child["decision"]["selected"], "system-claude/sonnet/medium");
    assert_eq!(child["run"]["harness"], "claude");
    let child_id = run_id(&child);
    assert_eq!(d.wait_done(&child_id, 15)["status"], "completed");
    assert_eq!(d.call("run.result", json!({"run_id":child_id}))["state"], "ready");
    assert!(!serde_json::to_string(&d.events(&parent)).unwrap().contains("fixture@example.test"));
    assert!(!serde_json::to_string(&d.events(&child_id)).unwrap().contains("fixture@example.test"));
    let decision_event = d.events(&parent).into_iter().find(|event| event["kind"] == "auto_decision").unwrap();
    assert_eq!(d.call("auto.decision.replay", json!({"event_seq":decision_event["seq"]}))["matches_recorded"], true);
    d.kill9();
    d.spawn();
    assert_eq!(d.call("auto.dispatch", request)["run"]["id"], child_id);
    let browser = d.call("auto.dispatch", json!({"work_unit_id":"cross-claude-browser-2",
        "parent_run_id":parent,"min_tier":"general","required_tools":["browser/navigate"],
        "allowed_profiles":["system-claude"],"prompt":"visit a page"}));
    assert_eq!(browser["state"], "paused", "{browser}");
    assert_eq!(browser["decision"]["exclusions"][0]["reason"], "missing_tool");
    assert_eq!(d.runs().len(), 2, "tool-incompatible Claude route must not launch");
}

#[test]
fn auto_claude_account_change_before_child_turn_pauses_without_a_model_call() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let counter = r.path().join("claude-auth-count");
    let mut d = Daemon::start(&[("OVERSEER_CODEX_PATH", &fixture("fake-harness/codex-app-fixture.js")),
        ("OVERSEER_CLAUDE_PATH", &fixture("fake-harness/claude-fixture.js")),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE,CLAUDE_FIXTURE_MODE,CLAUDE_FIXTURE_AUTH_COUNTER_FILE,CLAUDE_FIXTURE_AUTH_SWITCH_AFTER"),
        ("FIXTURE_MODE", "managed-models"), ("CLAUDE_FIXTURE_MODE", "prose"),
        ("CLAUDE_FIXTURE_AUTH_COUNTER_FILE", counter.to_str().unwrap()),
        ("CLAUDE_FIXTURE_AUTH_SWITCH_AFTER", "2")]);
    let parent = run_id(&d.call("task.create", json!({"repo":repo,"harness":"codex-app",
        "model":"gpt-6-astra","effort":"high","prompt":"seed context","approval_policy":"never"})));
    assert_eq!(d.wait_done(&parent, 15)["status"], "completed");
    let request = json!({"work_unit_id":"cross-claude-switch-1",
        "parent_run_id":parent,"min_tier":"general","required_tools":[],
        "allowed_profiles":["system-claude"],"prompt":"safe bounded result"});
    let outcome = d.call("auto.dispatch", request.clone());
    assert_eq!(outcome["state"], "paused", "{outcome}");
    assert_eq!(outcome["run"]["status"], "failed");
    assert_eq!(std::fs::read_to_string(&counter).unwrap(), "3");
    let child_id = run_id(&outcome);
    assert!(!d.events(&child_id).iter().any(|event| event["kind"] == "turn_started"),
        "a changed account must be caught before Claude starts a model turn");
    d.kill9();
    d.spawn();
    let replay = d.call("auto.dispatch", request);
    assert_eq!(replay["state"], "paused");
    assert_eq!(replay["run"]["id"], child_id);
    assert!(replay["actions"].as_array().is_some_and(|actions| actions.contains(&json!("refresh"))));
    assert_eq!(d.runs().len(), 2, "reconnecting must not create a second child");
}

#[test]
fn auto_pre_effect_account_rejection_selects_an_allowed_independent_route() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let counter = r.path().join("claude-auth-count");
    let quota = r.path().join("codex-quota-mode");
    std::fs::write(&quota, "unknown").unwrap();
    let d = Daemon::start(&[("OVERSEER_CODEX_PATH", &fixture("fake-harness/codex-app-fixture.js")),
        ("OVERSEER_CLAUDE_PATH", &fixture("fake-harness/claude-fixture.js")),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE,CLAUDE_FIXTURE_MODE,CLAUDE_FIXTURE_AUTH_COUNTER_FILE,FIXTURE_QUOTA_MODE_FILE"),
        ("FIXTURE_MODE", "managed-models"), ("CLAUDE_FIXTURE_MODE", "prose"),
        ("CLAUDE_FIXTURE_AUTH_COUNTER_FILE", counter.to_str().unwrap()),
        ("FIXTURE_QUOTA_MODE_FILE", quota.to_str().unwrap())]);
    let parent = run_id(&d.call("task.create", json!({"repo":repo,"harness":"codex-app",
        "model":"gpt-6-astra","effort":"high","prompt":"seed context","approval_policy":"never"})));
    assert_eq!(d.wait_done(&parent, 15)["status"], "completed");
    let request = json!({"work_unit_id":"pre-effect-account-reject-1",
        "parent_run_id":parent,"min_tier":"general","required_tools":[],
        "allowed_profiles":["system-claude","system-codex"],"preferred_harness":"claude",
        "prompt":"safe bounded result"});
    let outcome = d.call("auto.dispatch", request.clone());
    assert_eq!(outcome["state"], "dispatched", "{outcome}");
    assert_eq!(outcome["run"]["harness"], "codex-app", "{outcome}");
    assert_eq!(d.wait_done(&run_id(&outcome), 15)["status"], "completed");
    assert_eq!(std::fs::read_to_string(&counter).unwrap(), "2",
        "Claude must be rechecked once before its model turn, then excluded");
    assert_eq!(outcome["pre_effect_failures"].as_array().unwrap().len(), 1);
    assert_eq!(outcome["decision"]["exclusions"].as_array().unwrap().iter()
        .filter(|entry| entry["reason"] == "route_unavailable").count(), 2);
    assert_eq!(d.runs().len(), 2, "the rejected route must not create a child");
    assert_eq!(d.call("auto.dispatch", request)["run"]["id"], outcome["run"]["id"],
        "the same work unit must replay its successful alternate");
    let decisions = d.events(&parent).into_iter().filter(|e| e["kind"] == "auto_decision")
        .collect::<Vec<_>>();
    assert_eq!(decisions.len(), 1, "one durable decision per work unit");
    assert_eq!(d.call("auto.decision.replay", json!({"event_seq":decisions[0]["seq"]}))["matches_recorded"], true);
}

#[test]
fn auto_pre_effect_rejections_stop_after_three_distinct_routes_without_a_child() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let quota = r.path().join("codex-quota-mode");
    std::fs::write(&quota, "unknown").unwrap();
    let d = Daemon::start(&[("OVERSEER_CODEX_PATH", &fixture("fake-harness/codex-app-fixture.js")),
        ("OVERSEER_CLAUDE_PATH", &fixture("fake-harness/claude-fixture.js")),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE,CLAUDE_FIXTURE_MODE,CLAUDE_FIXTURE_AUTH_PER_PROFILE,FIXTURE_QUOTA_MODE_FILE"),
        ("FIXTURE_MODE", "managed-models"), ("CLAUDE_FIXTURE_MODE", "prose"),
        ("CLAUDE_FIXTURE_AUTH_PER_PROFILE", "1"),
        ("FIXTURE_QUOTA_MODE_FILE", quota.to_str().unwrap())]);
    let parent = run_id(&d.call("task.create", json!({"repo":repo,"harness":"codex-app",
        "model":"gpt-6-astra","effort":"high","prompt":"seed context","approval_policy":"never"})));
    assert_eq!(d.wait_done(&parent, 15)["status"], "completed");
    let claude_profiles = (0..3).map(|n| d.call("profile.create", json!({"name":format!("Reject {n}"),
        "harness":"claude"}))["id"].as_str().unwrap().to_string()).collect::<Vec<_>>();
    let mut allowed = claude_profiles;
    allowed.push("system-codex".into());
    let outcome = d.call("auto.dispatch", json!({"work_unit_id":"pre-effect-three-limit-1",
        "parent_run_id":parent,"min_tier":"general","required_tools":[],
        "allowed_profiles":allowed,"preferred_harness":"claude","prompt":"bounded result"}));
    assert_eq!(outcome["state"], "paused", "{outcome}");
    assert_eq!(outcome["decision"]["reason"], "pre_effect_attempt_limit");
    assert_eq!(outcome["pre_effect_failures"].as_array().unwrap().len(), 3);
    assert_eq!(d.runs().len(), 1, "no child may be created after three rejected preflights");
    let decision = d.events(&parent).into_iter().find(|e| e["kind"] == "auto_decision").unwrap();
    assert_eq!(d.call("auto.decision.replay", json!({"event_seq":decision["seq"]}))["matches_recorded"], true);
}

#[test]
fn auto_slow_preflights_share_the_ten_second_decision_deadline() {
    use std::time::Instant;
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let quota = r.path().join("codex-quota-mode");
    std::fs::write(&quota, "unknown").unwrap();
    let d = Daemon::start(&[("OVERSEER_CODEX_PATH", &fixture("fake-harness/codex-app-fixture.js")),
        ("OVERSEER_CLAUDE_PATH", &fixture("fake-harness/claude-fixture.js")),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE,CLAUDE_FIXTURE_MODE,CLAUDE_FIXTURE_AUTH_PER_PROFILE,CLAUDE_FIXTURE_AUTH_INITIAL_DELAY_MS,CLAUDE_FIXTURE_AUTH_PREFLIGHT_DELAY_MS,FIXTURE_QUOTA_MODE_FILE"),
        ("FIXTURE_MODE", "managed-models"), ("CLAUDE_FIXTURE_MODE", "prose"),
        ("CLAUDE_FIXTURE_AUTH_PER_PROFILE", "1"),
        ("CLAUDE_FIXTURE_AUTH_INITIAL_DELAY_MS", "3500"),
        ("CLAUDE_FIXTURE_AUTH_PREFLIGHT_DELAY_MS", "4000"),
        ("FIXTURE_QUOTA_MODE_FILE", quota.to_str().unwrap())]);
    let parent = run_id(&d.call("task.create", json!({"repo":repo,"harness":"codex-app",
        "model":"gpt-6-astra","effort":"high","prompt":"seed context","approval_policy":"never"})));
    assert_eq!(d.wait_done(&parent, 15)["status"], "completed");
    let mut allowed = (0..7).map(|n| d.call("profile.create", json!({"name":format!("Slow {n}"),
        "harness":"claude"}))["id"].as_str().unwrap().to_string()).collect::<Vec<_>>();
    allowed.push("system-codex".into());
    let started = Instant::now();
    let outcome = d.call("auto.dispatch", json!({"work_unit_id":"slow-preflights-1",
        "parent_run_id":parent,"min_tier":"general","required_tools":[],
        "allowed_profiles":allowed,"preferred_harness":"claude","prompt":"bounded result"}));
    let elapsed = started.elapsed();
    assert_eq!(outcome["state"], "paused", "{outcome}");
    assert_eq!(outcome["decision"]["reason"], "collection_deadline_elapsed");
    assert!(elapsed <= Duration::from_secs(11), "discovery and preflight took {elapsed:?}");
    assert_eq!(d.runs().len(), 1, "no child starts after the decision deadline");
    let event = d.events(&parent).into_iter().find(|e| e["kind"] == "auto_decision").unwrap();
    assert_eq!(d.call("auto.decision.replay", json!({"event_seq":event["seq"]}))["matches_recorded"], true);
}

#[test]
fn auto_dispatch_from_claude_exhausted_frontier_pool_uses_allowed_codex_child() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let d = Daemon::start(&[("OVERSEER_CODEX_PATH", &fixture("fake-harness/codex-app-fixture.js")),
        ("OVERSEER_CLAUDE_PATH", &fixture("fake-harness/claude-fixture.js")),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE,CLAUDE_FIXTURE_MODE"),
        ("FIXTURE_MODE", "managed-models"), ("CLAUDE_FIXTURE_MODE", "native-quota")]);
    let parent = run_id(&d.call("task.create", json!({"repo":repo,"harness":"claude",
        "model":"opus","effort":"high","prompt":"identify difficult diagnosis","approval_policy":"never"})));
    assert_eq!(d.wait_done(&parent, 15)["status"], "completed");
    let blocked = d.call("auto.quota.state", json!({"profile_id":"system-claude",
        "harness":"claude","model":"opus"}));
    assert_eq!(blocked["state"], "exhausted", "{blocked}");
    let next = d.call("auto.dispatch", json!({"work_unit_id":"claude-to-codex-1",
        "parent_run_id":parent,"min_tier":"frontier","required_tools":[],
        "allowed_profiles":["system-claude","system-codex"],
        "prompt":"finish the difficult diagnosis"}));
    assert_eq!(next["state"], "dispatched", "{next}");
    assert_eq!(next["decision"]["selected"], "system-codex/gpt-6-astra/high");
    assert_eq!(next["run"]["harness"], "codex-app");
    assert!(next["decision"]["exclusions"].as_array().unwrap().iter()
        .any(|entry| entry["route_id"] == "system-claude/opus/high" && entry["reason"] == "quota_exhausted"));
    assert_eq!(d.wait_done(&run_id(&next), 15)["status"], "completed");
}

#[test]
fn auto_dispatch_blocks_two_profiles_on_one_codex_account_but_uses_an_independent_account() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let account_dir = r.path().join("account-ids");
    let quota_dir = r.path().join("quota-modes");
    std::fs::create_dir_all(&account_dir).unwrap();
    std::fs::create_dir_all(&quota_dir).unwrap();
    let d = Daemon::start(&[("OVERSEER_CODEX_PATH", &fixture("fake-harness/codex-app-fixture.js")),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE,FIXTURE_ACCOUNT_IDS_DIR,FIXTURE_QUOTA_MODES_DIR"),
        ("FIXTURE_MODE", "managed-models"),
        ("FIXTURE_ACCOUNT_IDS_DIR", account_dir.to_str().unwrap()),
        ("FIXTURE_QUOTA_MODES_DIR", quota_dir.to_str().unwrap())]);
    let profiles: Vec<_> = ["Exhausted", "Stale alternate", "Independent"].into_iter()
        .map(|name| d.call("profile.create", json!({"name":name,"harness":"codex"}))).collect();
    for (index, profile) in profiles.iter().enumerate() {
        let id = profile["id"].as_str().unwrap();
        std::fs::write(account_dir.join(id), if index < 2 { "private-shared-identity-007" } else { "private-independent-identity-008" }).unwrap();
        std::fs::write(quota_dir.join(id), ["exhausted", "available", "unknown"][index]).unwrap();
    }
    let parent = run_id(&d.call("task.create", json!({"repo":repo,"harness":"codex-app",
        "model":"gpt-6-astra","effort":"high","prompt":"seed context","approval_policy":"never"})));
    assert_eq!(d.wait_done(&parent, 15)["status"], "completed");
    let exhausted = profiles[0]["id"].as_str().unwrap();
    let independent = profiles[2]["id"].as_str().unwrap();
    let primed = d.call("auto.dispatch", json!({"work_unit_id":"shared-account-prime-0",
        "parent_run_id":parent,"min_tier":"general","required_tools":[],
        "allowed_profiles":[exhausted],"prompt":"check exhausted account"}));
    assert_eq!(primed["state"], "paused", "{primed}");
    let remaining = [profiles[1]["id"].as_str().unwrap(), independent];
    let hidden = d.call("auto.dispatch", json!({"work_unit_id":"shared-account-hidden-block-2",
        "parent_run_id":parent,"min_tier":"general","required_tools":[],
        "allowed_profiles":remaining,"prompt":"summarize another result"}));
    assert_eq!(hidden["state"], "dispatched", "{hidden}");
    assert_eq!(hidden["run"]["profile_id"], independent,
        "a previously observed account block must survive candidate filtering: {hidden}");
    assert_eq!(d.wait_done(&run_id(&hidden), 15)["status"], "completed");
    let allowed: Vec<_> = profiles.iter().map(|profile| profile["id"].as_str().unwrap()).collect();
    let outcome = d.call("auto.dispatch", json!({"work_unit_id":"shared-account-pool-1",
        "parent_run_id":parent,"min_tier":"general","required_tools":[],
        "allowed_profiles":allowed,"prompt":"summarize the result"}));
    assert_eq!(outcome["state"], "dispatched", "{outcome}");
    assert_eq!(outcome["run"]["profile_id"], independent, "{outcome}");
    for profile in &profiles[..2] {
        let id = profile["id"].as_str().unwrap();
        assert!(outcome["decision"]["exclusions"].as_array().unwrap().iter().any(|entry|
            entry["route_id"].as_str().unwrap().starts_with(&format!("{id}/"))
                && entry["reason"] == "quota_exhausted"), "{outcome}");
    }
    let event = d.events(&parent).into_iter().find(|item| item["kind"] == "auto_decision"
        && item["payload"]["decision"]["work_unit_id"] == "shared-account-pool-1").unwrap();
    let routes = event["payload"]["selection_input"]["routes"].as_array().unwrap();
    let pool_for = |id: &str| routes.iter().find(|route| route["profile_id"] == id).unwrap()["pool_id"].clone();
    assert_eq!(pool_for(profiles[0]["id"].as_str().unwrap()), pool_for(profiles[1]["id"].as_str().unwrap()));
    assert_ne!(pool_for(profiles[0]["id"].as_str().unwrap()), pool_for(independent));
    assert!(!event["payload"].to_string().contains("private-shared-identity-007"));
    assert!(!event["payload"].to_string().contains("private-independent-identity-008"));
    assert_eq!(d.wait_done(&run_id(&outcome), 15)["status"], "completed");
}

#[test]
fn auto_unknown_draw_claim_blocks_another_profile_on_the_same_account_until_settlement() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let quota = r.path().join("quota-mode.txt");
    let accounts = r.path().join("account-ids");
    std::fs::create_dir_all(&accounts).unwrap();
    std::fs::write(&quota, "unknown").unwrap();
    let d = Daemon::start(&[("OVERSEER_CODEX_PATH", &fixture("fake-harness/codex-app-fixture.js")),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE,FIXTURE_QUOTA_MODE_FILE,FIXTURE_TURN_DELAY_MS,FIXTURE_ACCOUNT_IDS_DIR"),
        ("FIXTURE_MODE", "managed-models"), ("FIXTURE_QUOTA_MODE_FILE", quota.to_str().unwrap()),
        ("FIXTURE_TURN_DELAY_MS", "8000"), ("FIXTURE_ACCOUNT_IDS_DIR", accounts.to_str().unwrap())]);
    let profiles: Vec<_> = ["First", "Same account", "Independent account"].into_iter().map(|name|
        d.call("profile.create", json!({"name":name,"harness":"codex"}))["id"]
            .as_str().unwrap().to_string()).collect();
    for (index, profile) in profiles.iter().enumerate() {
        std::fs::write(accounts.join(profile), if index < 2 { "shared-account" } else { "independent-account" }).unwrap();
    }
    let parent = run_id(&d.call("task.create", json!({"repo":repo,"harness":"codex-app",
        "model":"gpt-6-astra","effort":"high","prompt":"seed context"})));
    assert_eq!(d.wait_done(&parent, 15)["status"], "completed");

    let first = d.call("auto.dispatch", json!({"work_unit_id":"unknown-claim-first",
        "parent_run_id":parent,"min_tier":"general","required_tools":[],
        "allowed_profiles":[profiles[0]],"prompt":"browser check"}));
    assert_eq!(first["state"], "dispatched", "{first}");
    let first_child = run_id(&first);
    d.wait_status(&first_child, |status| status == "running", 10);
    let second = d.call("auto.dispatch", json!({"work_unit_id":"unknown-claim-second",
        "parent_run_id":parent,"min_tier":"general","required_tools":[],
        "allowed_profiles":[profiles[1]],"prompt":"second bounded unit"}));
    assert_eq!(second["state"], "paused", "{second}");
    assert!(second["decision"]["exclusions"].as_array().unwrap().iter().any(|entry|
        entry["reason"] == "pool_in_flight_unknown_draw"), "{second}");
    assert_eq!(d.runs().len(), 2, "one unknown-draw account may have only one admitted child");
    let event = d.events(&parent).into_iter().find(|event|
        event["kind"] == "auto_decision" && event["payload"]["decision"]["work_unit_id"] == "unknown-claim-second").unwrap();
    assert_eq!(d.call("auto.decision.replay", json!({"event_seq":event["seq"]}))["matches_recorded"], true);

    let independent = d.call("auto.dispatch", json!({"work_unit_id":"unknown-claim-independent",
        "parent_run_id":parent,"min_tier":"general","required_tools":[],
        "allowed_profiles":[profiles[2]],"prompt":"independent bounded unit"}));
    assert_eq!(independent["state"], "dispatched", "{independent}");
    assert_eq!(independent["run"]["profile_id"], profiles[2]);
    assert_eq!(d.run(&first_child)["status"], "running",
        "the independent pool must be admitted while the first account is still active");
    assert_eq!(d.wait_done(&run_id(&independent), 15)["status"], "completed");

    assert_eq!(d.wait_done(&first_child, 15)["status"], "completed");
    let after = d.call("auto.dispatch", json!({"work_unit_id":"unknown-claim-after-settlement",
        "parent_run_id":parent,"min_tier":"general","required_tools":[],
        "allowed_profiles":[profiles[1]],"prompt":"next bounded unit"}));
    assert_eq!(after["state"], "dispatched", "{after}");
    assert_eq!(after["run"]["profile_id"], profiles[1]);
    assert_eq!(d.wait_done(&run_id(&after), 15)["status"], "completed");
}

#[test]
fn auto_simultaneous_unknown_draw_units_admit_only_one_shared_account_child() {
    use std::sync::{Arc, Barrier};
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let quota = r.path().join("quota-mode.txt");
    std::fs::write(&quota, "unknown").unwrap();
    let d = Daemon::start(&[("OVERSEER_CODEX_PATH", &fixture("fake-harness/codex-app-fixture.js")),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE,FIXTURE_QUOTA_MODE_FILE,FIXTURE_TURN_DELAY_MS"),
        ("FIXTURE_MODE", "managed-models"), ("FIXTURE_QUOTA_MODE_FILE", quota.to_str().unwrap()),
        ("FIXTURE_TURN_DELAY_MS", "6000")]);
    let profiles: Vec<_> = ["First concurrent", "Second concurrent"].into_iter().map(|name|
        d.call("profile.create", json!({"name":name,"harness":"codex"}))["id"]
            .as_str().unwrap().to_string()).collect();
    let parent = run_id(&d.call("task.create", json!({"repo":repo,"harness":"codex-app",
        "model":"gpt-6-astra","effort":"high","prompt":"seed context"})));
    assert_eq!(d.wait_done(&parent, 15)["status"], "completed");
    let gate = Arc::new(Barrier::new(3));
    let results = std::thread::scope(|scope| {
        let handles = (0..2).map(|index| {
            let gate = gate.clone();
            let profile = profiles[index].clone();
            let parent = parent.clone();
            let d = &d;
            scope.spawn(move || {
                gate.wait();
                d.call("auto.dispatch", json!({"work_unit_id":format!("simultaneous-unknown-{index}"),
                    "parent_run_id":parent,"min_tier":"general","required_tools":[],
                    "allowed_profiles":[profile],"prompt":"browser check"}))
            })
        }).collect::<Vec<_>>();
        gate.wait();
        handles.into_iter().map(|handle| handle.join().unwrap()).collect::<Vec<_>>()
    });
    assert_eq!(results.iter().filter(|result| result["state"] == "dispatched").count(), 1,
        "exactly one shared account child may be admitted: {results:?}");
    assert_eq!(results.iter().filter(|result| result["state"] == "paused").count(), 1,
        "the contending unit must pause with an explicit reason: {results:?}");
    assert!(results.iter().find(|result| result["state"] == "paused").unwrap()
        ["decision"]["exclusions"].as_array().unwrap().iter().any(|entry|
            entry["reason"] == "pool_in_flight_unknown_draw"));
    assert_eq!(d.runs().len(), 2);
    let launched = results.iter().find(|result| result["state"] == "dispatched").unwrap();
    assert_eq!(d.wait_done(&run_id(launched), 15)["status"], "completed");
}

#[test]
fn auto_child_does_not_start_while_a_manual_run_uses_its_profile() {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::net::UnixStream;
    use std::time::Instant;
    let r = tmp();
    let parent_repo = repo(&r.path().join("repo"));
    let manual_repo = repo(&r.path().join("manual-repo"));
    let bin = r.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    let marker = r.path().join("auto-git-stalled");
    let release = r.path().join("release-auto-git");
    let trace = r.path().join("manual-race-trace.txt");
    let wrapper = bin.join("git");
    std::fs::write(&wrapper, format!("#!/bin/sh\nif [ \"$1\" = worktree ] && [ \"$2\" = add ] && [ \"$3\" = -b ]; then\n  case \"$4\" in overseer/delegate-*|overseer/auto-*)\n    printf x > '{}'\n    while [ ! -e '{}' ]; do /bin/sleep 0.05; done;;\n  esac\nfi\nexec /usr/bin/git \"$@\"\n", marker.display(), release.display())).unwrap();
    std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o700)).unwrap();
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap());
    let d = Daemon::start(&[("PATH", &path),
        ("OVERSEER_CODEX_PATH", &fixture("fake-harness/codex-app-fixture.js")),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE,FIXTURE_TURN_DELAY_MS,FIXTURE_TRACE_FILE"),
        ("FIXTURE_MODE", "managed-models"), ("FIXTURE_TURN_DELAY_MS", "6000"),
        ("FIXTURE_TRACE_FILE", trace.to_str().unwrap())]);
    let parent = run_id(&d.call("task.create", json!({"repo":parent_repo,"harness":"codex-app",
        "model":"gpt-6-astra","effort":"high","prompt":"seed context"})));
    assert_eq!(d.wait_done(&parent, 15)["status"], "completed");
    let request = json!({"work_unit_id":"manual-race-auto-1","parent_run_id":parent,
        "min_tier":"general","required_tools":[],"prompt":"browser check"});
    let socket = d.socket();
    let auto = std::thread::spawn(move || {
        let mut conn = UnixStream::connect(socket).unwrap();
        conn.set_read_timeout(Some(Duration::from_secs(15))).unwrap();
        conn.write_all(format!("{}\n", json!({"id":1,"method":"auto.dispatch","params":request})).as_bytes()).unwrap();
        let mut line = String::new();
        BufReader::new(conn).read_line(&mut line).unwrap();
        let reply: serde_json::Value = serde_json::from_str(&line).unwrap();
        reply["result"].clone()
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    while !marker.exists() {
        assert!(Instant::now() < deadline, "Auto did not reach Git preparation");
        std::thread::sleep(Duration::from_millis(20));
    }
    let manual = run_id(&d.call("task.create", json!({"repo":manual_repo,"harness":"codex-app",
        "model":"gpt-6-astra","effort":"high","prompt":"browser check"})));
    d.wait_status(&manual, |status| status == "running", 10);
    std::fs::write(&release, "go").unwrap();
    let response = auto.join().unwrap();
    assert_eq!(response["state"], "paused", "Auto must not overlap the active manual profile run: {response}");
    assert_eq!(std::fs::read_to_string(&trace).unwrap().matches("turn_model:gpt-6-sol").count(), 0,
        "the Auto child must not start a model turn while manual work is active");
    assert_eq!(d.wait_done(&manual, 15)["status"], "completed");
}

#[test]
fn auto_recent_429_excludes_only_its_route_without_inventing_quota_exhaustion() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let d = Daemon::start(&[("OVERSEER_CODEX_PATH", &fixture("fake-harness/codex-app-fixture.js")),
        ("OVERSEER_CLAUDE_PATH", &fixture("fake-harness/claude-fixture.js")),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE,CLAUDE_FIXTURE_MODE"),
        ("FIXTURE_MODE", "managed-models"), ("CLAUDE_FIXTURE_MODE", "prose")]);
    let parent = run_id(&d.call("task.create", json!({"repo":repo,"harness":"codex-app",
        "model":"gpt-6-astra","effort":"high","prompt":"seed context","approval_policy":"never"})));
    assert_eq!(d.wait_done(&parent, 15)["status"], "completed");
    let failing = run_id(&d.call("task.create", json!({"repo":repo,"harness":"codex-app",
        "model":"gpt-6-sol","effort":"medium","prompt":"simulate direct 429","approval_policy":"never"})));
    assert_eq!(d.wait_done(&failing, 15)["status"], "failed");
    assert!(d.events(&failing).iter().any(|event| event["kind"] == "error"
        && event["payload"]["class"] == "rate_limit"));
    let result = d.call("auto.dispatch", json!({"work_unit_id":"recent-429-1",
        "parent_run_id":parent,"min_tier":"general","required_tools":[],
        "allowed_profiles":["system-codex","system-claude"],
        "preferred_harness":"codex-app","prompt":"continue on an available route"}));
    assert_eq!(result["state"], "dispatched", "{result}");
    assert_eq!(result["decision"]["selected"], "system-claude/sonnet/medium", "{result}");
    assert!(result["decision"]["exclusions"].as_array().unwrap().iter()
        .any(|entry| entry["reason"] == "route_unavailable"
            && entry["route_id"].as_str().unwrap().starts_with("system-codex/")));
    assert_ne!(d.call("auto.quota.state", json!({"profile_id":"system-codex",
        "harness":"codex-app","model":"gpt-6-sol"}))["state"], "exhausted");
    let decision_event = d.events(&parent).into_iter()
        .find(|event| event["kind"] == "auto_decision").unwrap();
    assert_eq!(d.call("auto.decision.replay", json!({"event_seq":decision_event["seq"]}))["matches_recorded"], true);
    assert_eq!(d.wait_done(&run_id(&result), 15)["status"], "completed");
}

#[test]
fn auto_recent_503_blocks_the_failed_endpoint_before_an_independent_child() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let d = Daemon::start(&[("OVERSEER_CODEX_PATH", &fixture("fake-harness/codex-app-fixture.js")),
        ("OVERSEER_CLAUDE_PATH", &fixture("fake-harness/claude-fixture.js")),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE,CLAUDE_FIXTURE_MODE"),
        ("FIXTURE_MODE", "managed-models"), ("CLAUDE_FIXTURE_MODE", "prose")]);
    let parent = run_id(&d.call("task.create", json!({"repo":repo,"harness":"codex-app",
        "model":"gpt-6-astra","effort":"high","prompt":"seed context","approval_policy":"never"})));
    assert_eq!(d.wait_done(&parent, 15)["status"], "completed");
    let failing = run_id(&d.call("task.create", json!({"repo":repo,"harness":"codex-app",
        "model":"gpt-6-sol","effort":"medium","prompt":"simulate direct 503","approval_policy":"never"})));
    assert_eq!(d.wait_done(&failing, 15)["status"], "failed");
    assert!(d.events(&failing).iter().any(|event| event["kind"] == "error"
        && event["payload"]["class"] == "service_unavailable"));
    let result = d.call("auto.dispatch", json!({"work_unit_id":"recent-503-1",
        "parent_run_id":parent,"min_tier":"general","required_tools":[],
        "allowed_profiles":["system-codex","system-claude"],
        "preferred_harness":"codex-app","prompt":"continue on an independent route"}));
    assert_eq!(result["state"], "dispatched", "{result}");
    assert_eq!(result["decision"]["selected"], "system-claude/sonnet/medium", "{result}");
    assert!(result["decision"]["exclusions"].as_array().unwrap().iter()
        .any(|entry| entry["reason"] == "route_unavailable"
            && entry["route_id"].as_str().unwrap().starts_with("system-codex/")));
    assert_eq!(d.wait_done(&run_id(&result), 15)["status"], "completed");
}

#[test]
fn auto_four_slow_profile_collectors_finish_within_the_decision_deadline() {
    use std::time::Instant;
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let trace = r.path().join("slow-collectors-trace.txt");
    let d = Daemon::start(&[("OVERSEER_CODEX_PATH", &fixture("fake-harness/codex-app-fixture.js")),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE,FIXTURE_MODEL_DELAY_MS,FIXTURE_TRACE_FILE"),
        ("FIXTURE_MODE", "managed-models"), ("FIXTURE_MODEL_DELAY_MS", "3500"),
        ("FIXTURE_TRACE_FILE", trace.to_str().unwrap())]);
    let parent = run_id(&d.call("task.create", json!({"repo":repo,"harness":"codex-app",
        "model":"gpt-6-astra","effort":"high","prompt":"seed context","approval_policy":"never"})));
    assert_eq!(d.wait_done(&parent, 15)["status"], "completed");
    let profiles = (0..4).map(|n| d.call("profile.create", json!({"name":format!("Collector {n}"),
        "harness":"codex"}))["id"].as_str().unwrap().to_string()).collect::<Vec<_>>();
    let start = Instant::now();
    let request = json!({"work_unit_id":"four-slow-collectors-1",
        "parent_run_id":parent,"min_tier":"general","required_tools":["absent/tool"],
        "allowed_profiles":profiles,"prompt":"bounded metadata only"});
    let outcome = std::thread::scope(|scope| {
        let pending = scope.spawn(|| d.call("auto.dispatch", request));
        let probe_deadline = Instant::now() + Duration::from_secs(2);
        while !std::fs::read_to_string(&trace).unwrap_or_default().contains("model_read") {
            assert!(Instant::now() < probe_deadline, "slow metadata collection did not begin");
            std::thread::sleep(Duration::from_millis(10));
        }
        let ui_start = Instant::now();
        assert!(d.call("hello", json!({}))["protocol"].is_number());
        assert!(ui_start.elapsed() < Duration::from_secs(2),
            "another client waited behind the slow Auto decision");
        pending.join().unwrap()
    });
    let elapsed = start.elapsed();
    assert_eq!(outcome["state"], "paused", "{outcome}");
    assert!(elapsed <= Duration::from_secs(11), "Auto decision took {elapsed:?}, exceeding the 10+1 second bound");
    assert_eq!(d.runs().len(), 1, "no model child should launch when the tool is absent");
}

#[test]
fn auto_hundred_local_routes_with_stalled_account_read_pause_before_deadline_and_keep_ui_responsive() {
    use std::collections::BTreeSet;
    use std::time::Instant;
    let Some(program) = std::env::var_os("PATH").and_then(|path| std::env::split_paths(&path)
        .map(|dir| dir.join("opencode")).find(|candidate| candidate.is_file())) else { return };
    struct MockServer(std::process::Child);
    impl Drop for MockServer {
        fn drop(&mut self) { let _ = self.0.kill(); let _ = self.0.wait(); }
    }
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let port_file = r.path().join("mock-port");
    let mock_log = r.path().join("mock-log.jsonl");
    let _mock = MockServer(std::process::Command::new("node")
        .arg(fixture("mock-openai/server.js"))
        .env("MOCK_PORT_FILE", &port_file).env("MOCK_LOG", &mock_log)
        .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null())
        .spawn().unwrap());
    let ready_deadline = Instant::now() + Duration::from_secs(5);
    while !port_file.exists() {
        assert!(Instant::now() < ready_deadline, "local mock provider did not start");
        std::thread::sleep(Duration::from_millis(10));
    }
    let endpoint = format!("http://127.0.0.1:{}/v1", std::fs::read_to_string(&port_file).unwrap());
    let providers = (0..100).map(|n| (format!("local_{n:03}"), json!({
        "npm":"@ai-sdk/openai-compatible","options":{"baseURL":endpoint},
        "models":{"gpt-oss-120b":{"name":format!("Local {n}"),"tool_call":true,"reasoning":true}}
    }))).collect::<serde_json::Map<String, serde_json::Value>>();
    std::fs::write(repo.join("opencode.json"), json!({
        "$schema":"https://opencode.ai/config.json","provider":providers,
        "model":"local_000/gpt-oss-120b","small_model":"local_000/gpt-oss-120b",
        "autoupdate":false,"share":"disabled"
    }).to_string()).unwrap();
    git(&repo, &["add", "opencode.json"]);
    git(&repo, &["commit", "-q", "-m", "hundred local routes"]);
    let trace = r.path().join("stalled-account-trace.txt");
    let d = Daemon::start(&[("OVERSEER_CODEX_PATH", &fixture("fake-harness/codex-app-fixture.js")),
        ("OVERSEER_OPENCODE_PATH", program.to_str().unwrap()),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE,FIXTURE_MODEL_DELAY_MS,FIXTURE_TRACE_FILE"),
        ("FIXTURE_MODE", "managed-models"), ("FIXTURE_MODEL_DELAY_MS", "12000"),
        ("FIXTURE_TRACE_FILE", trace.to_str().unwrap())]);
    let local = d.call("profile.create", json!({"name":"Hundred local routes","harness":"opencode"}));
    let parent = run_id(&d.call("task.create", json!({"repo":repo,"harness":"codex-app",
        "model":"gpt-6-astra","effort":"high","prompt":"seed context","approval_policy":"never"})));
    assert_eq!(d.wait_done(&parent, 15)["status"], "completed");
    let request = json!({"work_unit_id":"hundred-local-routes-1","parent_run_id":parent,
        "min_tier":"general","required_tools":["browser/navigate"],
        "sandbox":"read_only","allowed_profiles":[local["id"],"system-codex"],
        "prompt":"bounded browser audit"});
    let start = Instant::now();
    let outcome = std::thread::scope(|scope| {
        let pending = scope.spawn(|| d.call("auto.dispatch", request));
        let probe_deadline = Instant::now() + Duration::from_secs(3);
        while !std::fs::read_to_string(&trace).unwrap_or_default().contains("model_read") {
            assert!(Instant::now() < probe_deadline, "stalled account collector did not begin");
            std::thread::sleep(Duration::from_millis(10));
        }
        let ui_start = Instant::now();
        assert!(d.call("hello", json!({}))["protocol"].is_number());
        assert!(ui_start.elapsed() < Duration::from_secs(2),
            "another UI client waited behind the stalled hundred-route decision");
        pending.join().unwrap()
    });
    assert_eq!(outcome["state"], "paused", "{outcome}");
    assert!(start.elapsed() <= Duration::from_secs(11),
        "hundred-route decision exceeded the 10+1 second tolerance");
    let decision = d.events(&parent).into_iter().find(|event| event["kind"] == "auto_decision").unwrap();
    assert!(decision["payload"]["discovery_failures"].as_array().unwrap().iter()
        .any(|failure| failure["profile_id"] == "system-codex"
            && failure["reason"] == "metadata_or_auth_unavailable"),
        "the stalled account read must be excluded before the decision: {decision}");
    let routes = decision["payload"]["selection_input"]["routes"].as_array().unwrap();
    assert_eq!(routes.len(), 100, "all bounded local candidates reached the selector");
    let pools = routes.iter().map(|route| route["pool_id"].as_str().unwrap()).collect::<BTreeSet<_>>();
    assert_eq!(pools.len(), 1, "one endpoint must retain one shared pool identity");
    assert_eq!(d.runs().len(), 1, "metadata reads and ineligible routes must not start a child");
    assert_eq!(std::fs::read_to_string(&trace).unwrap().matches("model_read").count(), 1);
    assert!(!std::fs::read_to_string(&mock_log).unwrap_or_default().contains("/v1/chat/completions"),
        "catalog discovery must not send a paid or mock model request");
}

#[test]
fn auto_claude_api_key_auth_is_excluded_without_a_child() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let d = Daemon::start(&[("OVERSEER_CODEX_PATH", &fixture("fake-harness/codex-app-fixture.js")),
        ("OVERSEER_CLAUDE_PATH", &fixture("fake-harness/claude-fixture.js")),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE,CLAUDE_FIXTURE_AUTH_MODE"),
        ("FIXTURE_MODE", "managed-models"), ("CLAUDE_FIXTURE_AUTH_MODE", "api-key")]);
    let parent = run_id(&d.call("task.create", json!({"repo":repo,"harness":"codex-app",
        "model":"gpt-6-astra","effort":"high","prompt":"seed context","approval_policy":"never"})));
    assert_eq!(d.wait_done(&parent, 15)["status"], "completed");
    let outcome = d.call("auto.dispatch", json!({"work_unit_id":"api-key-claude-1",
        "parent_run_id":parent,"min_tier":"general","required_tools":[],
        "allowed_profiles":["system-claude"],"prompt":"use the subscription"}));
    assert_eq!(outcome["state"], "paused", "{outcome}");
    assert_eq!(outcome["discovery_failures"][0]["reason"], "metadata_or_auth_unavailable");
    assert_eq!(d.runs().len(), 1);
}

#[test]
fn auto_dispatch_rechecks_quota_and_account_generation_inside_the_selected_child() {
    for action in ["exhaust_quota", "switch_account"] {
        let r = tmp();
        let repo = repo(&r.path().join("repo"));
        let trace = r.path().join("dispatch-race-trace.txt");
        let quota = r.path().join("quota-mode.txt");
        let account = r.path().join("account-id.txt");
        std::fs::write(&quota, "available").unwrap();
        std::fs::write(&account, "account-A").unwrap();
        let d = Daemon::start(&[("OVERSEER_CODEX_PATH", &fixture("fake-harness/codex-app-fixture.js")),
            ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE,FIXTURE_TRACE_FILE,FIXTURE_QUOTA_MODE_FILE,FIXTURE_ACCOUNT_ID_FILE,FIXTURE_TOOL_READ_ACTION"),
            ("FIXTURE_MODE", "managed-models"), ("FIXTURE_TRACE_FILE", trace.to_str().unwrap()),
            ("FIXTURE_QUOTA_MODE_FILE", quota.to_str().unwrap()),
            ("FIXTURE_ACCOUNT_ID_FILE", account.to_str().unwrap()), ("FIXTURE_TOOL_READ_ACTION", action)]);
        let parent = run_id(&d.call("task.create", json!({"repo":repo,"harness":"codex-app",
            "model":"gpt-6-astra","effort":"high","prompt":"seed context"})));
        assert_eq!(d.wait_done(&parent, 15)["status"], "completed");
        let dispatched = d.call("auto.dispatch", json!({"work_unit_id":"race-unit","parent_run_id":parent,
            "min_tier":"general","required_tools":["browser/navigate"],"prompt":"browser check"}));
        assert_eq!(dispatched["decision"]["selected"], "system-codex/gpt-6-sol/medium", "{action}: {dispatched}");
        let child = run_id(&dispatched);
        assert_eq!(d.wait_done(&child, 15)["status"], "failed", "{action}");
        let trace = std::fs::read_to_string(trace).unwrap();
        assert_eq!(trace.matches("turn_model:gpt-6-sol").count(), 0,
            "{action} must stop before the selected model turn");
        let history = serde_json::to_string(&d.events(&child)).unwrap();
        assert!(!history.contains("account-A") && !history.contains("account-B"));
        assert_eq!(d.runs().len(), 2);
    }
}

#[test]
fn auto_dispatch_concurrent_clients_share_one_selection_and_one_child() {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixStream;
    use std::sync::{Arc, Barrier};
    use std::time::Instant;
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let d = Daemon::start(&[("OVERSEER_CODEX_PATH", &fixture("fake-harness/codex-app-fixture.js")),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE,FIXTURE_MODEL_DELAY_MS"),
        ("FIXTURE_MODE", "managed-models"), ("FIXTURE_MODEL_DELAY_MS", "300")]);
    let parent = run_id(&d.call("task.create", json!({"repo":repo,"harness":"codex-app","prompt":"seed context"})));
    assert_eq!(d.wait_done(&parent, 15)["status"], "completed");
    let params = json!({"work_unit_id":"same-auto-unit","parent_run_id":parent,
        "min_tier":"general","required_tools":[],"prompt":"browser check"});
    let barrier = Arc::new(Barrier::new(3));
    let socket = d.socket();
    let handles = (0..2).map(|_| {
        let barrier = barrier.clone();
        let socket = socket.clone();
        let params = params.clone();
        std::thread::spawn(move || -> Result<serde_json::Value, String> {
            barrier.wait();
            let mut conn = UnixStream::connect(socket).map_err(|error| error.to_string())?;
            conn.set_read_timeout(Some(Duration::from_secs(15))).map_err(|error| error.to_string())?;
            conn.write_all(format!("{}\n", json!({"id":1,"method":"auto.dispatch","params":params})).as_bytes())
                .map_err(|error| error.to_string())?;
            let mut line = String::new();
            BufReader::new(conn).read_line(&mut line).map_err(|error| error.to_string())?;
            let reply: serde_json::Value = serde_json::from_str(&line).map_err(|error| error.to_string())?;
            if let Some(error) = reply.get("error") { return Err(error.to_string()); }
            Ok(reply["result"].clone())
        })
    }).collect::<Vec<_>>();
    barrier.wait();
    let results = handles.into_iter().map(|handle| handle.join().unwrap().unwrap()).collect::<Vec<_>>();
    for result in &results {
        assert_eq!(result["work_unit_id"], "same-auto-unit", "{result}");
        assert!(result["state"] == "launch_pending" || result["state"] == "dispatched", "{result}");
    }
    let deadline = Instant::now() + Duration::from_secs(15);
    let resolved = loop {
        let replay = d.call("auto.dispatch", params.clone());
        if replay["run"]["id"].is_string() { break replay; }
        assert_eq!(replay["state"], "launch_pending", "{replay}");
        assert!(Instant::now() < deadline, "one admitted launch never resolved");
        std::thread::sleep(Duration::from_millis(20));
    };
    let child = run_id(&resolved);
    for result in &results {
        if let Some(initial_child) = result["run"]["id"].as_str() {
            assert_eq!(initial_child, child, "clients cannot receive different children");
        }
    }
    assert_eq!(d.wait_done(&child, 15)["status"], "completed");
    let decisions = d.events(&parent).into_iter().filter(|event| event["kind"] == "auto_decision").count();
    assert_eq!(decisions, 1, "concurrent requests should reuse one recorded selection");
    assert_eq!(d.runs().len(), 2);
}

#[test]
fn auto_decision_and_launch_intent_commit_or_fail_together() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let d = Daemon::start(&[("OVERSEER_CODEX_PATH", &fixture("fake-harness/codex-app-fixture.js")),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE"),
        ("FIXTURE_MODE", "managed-models")]);
    let parent = run_id(&d.call("task.create", json!({"repo":repo,"harness":"codex-app",
        "model":"gpt-6-astra","effort":"high","prompt":"seed context"})));
    assert_eq!(d.wait_done(&parent, 15)["status"], "completed");
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    db.execute_batch("CREATE TRIGGER reject_auto_intent BEFORE INSERT ON auto_launch_intents
        BEGIN SELECT RAISE(FAIL, 'injected intent failure'); END;").unwrap();
    let request = json!({"work_unit_id":"atomic-intent-1","parent_run_id":parent,
        "min_tier":"general","required_tools":[],"prompt":"bounded child task"});
    assert!(d.try_call("auto.dispatch", request.clone()).is_err());
    let decisions = d.events(&parent).into_iter().filter(|event| event["kind"] == "auto_decision").count();
    assert_eq!(decisions, 0, "failed admission must not leave a selected decision without its intent");
    assert_eq!(d.runs().len(), 1);
    db.execute_batch("DROP TRIGGER reject_auto_intent;").unwrap();
    db.execute_batch("CREATE TRIGGER reject_auto_decision BEFORE INSERT ON events
        WHEN NEW.kind='auto_decision' BEGIN SELECT RAISE(FAIL, 'injected decision failure'); END;").unwrap();
    assert!(d.try_call("auto.dispatch", request.clone()).is_err());
    let intents: i64 = db.query_row("SELECT COUNT(*) FROM auto_launch_intents WHERE work_unit_id='atomic-intent-1'",
        [], |row| row.get(0)).unwrap();
    assert_eq!(intents, 0, "failed decision persistence must roll back the launch intent");
    db.execute_batch("DROP TRIGGER reject_auto_decision;").unwrap();
    let dispatched = d.call("auto.dispatch", request);
    assert_eq!(dispatched["state"], "dispatched", "{dispatched}");
    assert_eq!(d.events(&parent).into_iter().filter(|event| event["kind"] == "auto_decision").count(), 1);
    assert_eq!(d.runs().len(), 2);
}

#[test]
fn auto_child_rows_and_launch_phase_commit_or_fail_together() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let trace = r.path().join("child-trace.txt");
    let d = Daemon::start(&[("OVERSEER_CODEX_PATH", &fixture("fake-harness/codex-app-fixture.js")),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE,FIXTURE_TRACE_FILE"),
        ("FIXTURE_MODE", "managed-models"), ("FIXTURE_TRACE_FILE", trace.to_str().unwrap())]);
    let parent = run_id(&d.call("task.create", json!({"repo":repo,"harness":"codex-app",
        "model":"gpt-6-astra","effort":"high","prompt":"seed context"})));
    assert_eq!(d.wait_done(&parent, 15)["status"], "completed");
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    db.execute_batch("CREATE TRIGGER reject_child_phase BEFORE UPDATE OF phase ON auto_launch_intents
        WHEN NEW.phase='child_created' BEGIN SELECT RAISE(FAIL, 'injected phase failure'); END;").unwrap();
    let request = json!({"work_unit_id":"atomic-child-1","parent_run_id":parent,
        "min_tier":"general","required_tools":[],"prompt":"bounded child task"});
    let first = d.call("auto.dispatch", request.clone());
    assert_eq!(first["state"], "paused", "{first}");
    assert_eq!(first["pause_reason"], "launch_effects_uncertain");
    assert_eq!(d.runs().len(), 1, "a rejected phase transition cannot leave a committed child");
    assert_eq!(std::fs::read_to_string(&trace).unwrap().matches("turn_model:gpt-6-sol").count(), 0,
        "a rejected phase transition cannot start the child's model turn");
    let replay = d.call("auto.dispatch", request);
    assert_eq!(replay["state"], "paused", "{replay}");
    assert_eq!(d.events(&parent).into_iter().filter(|event| event["kind"] == "auto_decision").count(), 1);
    db.execute_batch("DROP TRIGGER reject_child_phase;").unwrap();
    let next = d.call("auto.dispatch", json!({"work_unit_id":"atomic-child-2",
        "parent_run_id":parent,"min_tier":"general","required_tools":[],
        "prompt":"another bounded child task"}));
    assert_eq!(next["state"], "dispatched", "{next}");
    let phase: String = db.query_row("SELECT phase FROM auto_launch_intents WHERE work_unit_id='atomic-child-2'",
        [], |row| row.get(0)).unwrap();
    assert_eq!(phase, "child_created");
}

#[test]
fn auto_distinct_concurrent_units_share_one_profile_metadata_read() {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixStream;
    use std::sync::{Arc, Barrier};
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let trace = r.path().join("shared-metadata-trace.txt");
    let d = Daemon::start(&[("OVERSEER_CODEX_PATH", &fixture("fake-harness/codex-app-fixture.js")),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE,FIXTURE_MODEL_DELAY_MS,FIXTURE_TRACE_FILE"),
        ("FIXTURE_MODE", "managed-models"), ("FIXTURE_MODEL_DELAY_MS", "750"),
        ("FIXTURE_TRACE_FILE", trace.to_str().unwrap())]);
    let parent = run_id(&d.call("task.create", json!({"repo":repo,"harness":"codex-app",
        "model":"gpt-6-astra","effort":"high","prompt":"seed context"})));
    assert_eq!(d.wait_done(&parent, 15)["status"], "completed");
    std::fs::write(&trace, "").unwrap();
    let barrier = Arc::new(Barrier::new(3));
    let socket = d.socket();
    let handles = (0..2).map(|n| {
        let barrier = barrier.clone();
        let socket = socket.clone();
        let params = json!({"work_unit_id":format!("shared-profile-{n}"),
            "parent_run_id":parent,"min_tier":"general","required_tools":["absent/tool"],
            "prompt":"check unavailable tool"});
        std::thread::spawn(move || -> Result<serde_json::Value, String> {
            barrier.wait();
            let mut conn = UnixStream::connect(socket).map_err(|error| error.to_string())?;
            conn.set_read_timeout(Some(Duration::from_secs(15))).map_err(|error| error.to_string())?;
            conn.write_all(format!("{}\n", json!({"id":1,"method":"auto.dispatch","params":params})).as_bytes())
                .map_err(|error| error.to_string())?;
            let mut line = String::new();
            BufReader::new(conn).read_line(&mut line).map_err(|error| error.to_string())?;
            let reply: serde_json::Value = serde_json::from_str(&line).map_err(|error| error.to_string())?;
            if let Some(error) = reply.get("error") { return Err(error.to_string()); }
            Ok(reply["result"].clone())
        })
    }).collect::<Vec<_>>();
    barrier.wait();
    let results = handles.into_iter().map(|handle| handle.join().unwrap().unwrap()).collect::<Vec<_>>();
    assert!(results.iter().all(|result| result["state"] == "paused"), "{results:?}");
    assert_eq!(d.runs().len(), 1, "neither request should start a child");
    let reads = std::fs::read_to_string(&trace).unwrap().matches("model_read").count();
    assert_eq!(reads, 2, "one paginated model catalog should serve both decisions");
    let later = d.call("auto.dispatch", json!({"work_unit_id":"shared-profile-later",
        "parent_run_id":parent,"min_tier":"general","required_tools":["absent/tool"],
        "prompt":"check unavailable tool again"}));
    assert_eq!(later["state"], "paused");
    let reads = std::fs::read_to_string(&trace).unwrap().matches("model_read").count();
    assert_eq!(reads, 4, "a later decision must collect fresh profile evidence");
}

#[test]
fn auto_different_workspaces_share_account_read_but_check_tools_separately() {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixStream;
    use std::sync::{Arc, Barrier};
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let trace = r.path().join("shared-account-trace.txt");
    let d = Daemon::start(&[("OVERSEER_CODEX_PATH", &fixture("fake-harness/codex-app-fixture.js")),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE,FIXTURE_MODEL_DELAY_MS,FIXTURE_TRACE_FILE"),
        ("FIXTURE_MODE", "managed-models"), ("FIXTURE_MODEL_DELAY_MS", "750"),
        ("FIXTURE_TRACE_FILE", trace.to_str().unwrap())]);
    let parents = (0..2).map(|n| {
        let parent = run_id(&d.call("task.create", json!({"repo":repo,"harness":"codex-app",
            "model":"gpt-6-astra","effort":"high","prompt":format!("seed context {n}")})));
        assert_eq!(d.wait_done(&parent, 15)["status"], "completed");
        parent
    }).collect::<Vec<_>>();
    std::fs::write(&trace, "").unwrap();
    let barrier = Arc::new(Barrier::new(3));
    let socket = d.socket();
    let handles = parents.iter().enumerate().map(|(n, parent)| {
        let barrier = barrier.clone();
        let socket = socket.clone();
        let params = json!({"work_unit_id":format!("cross-workspace-{n}"),
            "parent_run_id":parent,"min_tier":"general","required_tools":["absent/tool"],
            "prompt":"check unavailable tool"});
        std::thread::spawn(move || -> Result<serde_json::Value, String> {
            barrier.wait();
            let mut conn = UnixStream::connect(socket).map_err(|error| error.to_string())?;
            conn.set_read_timeout(Some(Duration::from_secs(15))).map_err(|error| error.to_string())?;
            conn.write_all(format!("{}\n", json!({"id":1,"method":"auto.dispatch","params":params})).as_bytes())
                .map_err(|error| error.to_string())?;
            let mut line = String::new();
            BufReader::new(conn).read_line(&mut line).map_err(|error| error.to_string())?;
            let reply: serde_json::Value = serde_json::from_str(&line).map_err(|error| error.to_string())?;
            if let Some(error) = reply.get("error") { return Err(error.to_string()); }
            Ok(reply["result"].clone())
        })
    }).collect::<Vec<_>>();
    barrier.wait();
    let results = handles.into_iter().map(|handle| handle.join().unwrap().unwrap()).collect::<Vec<_>>();
    assert!(results.iter().all(|result| result["state"] == "paused"), "{results:?}");
    let trace_text = std::fs::read_to_string(&trace).unwrap();
    assert_eq!(trace_text.matches("model_read").count(), 2,
        "the shared account model catalog should be read once");
    let tool_paths = trace_text.lines().filter_map(|line| line.strip_prefix("tool_cwd:"))
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(tool_paths.len(), 2, "tool availability must be checked in each workspace");
    assert_eq!(d.runs().len(), 2, "neither unavailable-tool unit should start a child");
    let later = d.call("auto.dispatch", json!({"work_unit_id":"cross-workspace-later",
        "parent_run_id":parents[0],"min_tier":"general","required_tools":["absent/tool"],
        "prompt":"check unavailable tool again"}));
    assert_eq!(later["state"], "paused");
    assert_eq!(std::fs::read_to_string(&trace).unwrap().matches("model_read").count(), 4,
        "the shared account read must expire when the overlapping collection ends");
}

#[test]
fn auto_claude_concurrent_workspaces_share_one_auth_read() {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixStream;
    use std::sync::{Arc, Barrier};
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let count_file = r.path().join("claude-auth-count.txt");
    let d = Daemon::start(&[("OVERSEER_CLAUDE_PATH", &fixture("fake-harness/claude-fixture.js")),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "CLAUDE_FIXTURE_MODE,CLAUDE_FIXTURE_AUTH_COUNTER_FILE,CLAUDE_FIXTURE_AUTH_INITIAL_DELAY_MS,CLAUDE_FIXTURE_AUTH_SWITCH_AFTER"),
        ("CLAUDE_FIXTURE_MODE", "prose"),
        ("CLAUDE_FIXTURE_AUTH_COUNTER_FILE", count_file.to_str().unwrap()),
        ("CLAUDE_FIXTURE_AUTH_INITIAL_DELAY_MS", "750"),
        ("CLAUDE_FIXTURE_AUTH_SWITCH_AFTER", "99")]);
    let parents = (0..2).map(|n| {
        let parent = run_id(&d.call("task.create", json!({"repo":repo,"harness":"claude",
            "model":"sonnet","prompt":format!("seed context {n}")})));
        assert_eq!(d.wait_done(&parent, 15)["status"], "completed");
        parent
    }).collect::<Vec<_>>();
    let barrier = Arc::new(Barrier::new(3));
    let socket = d.socket();
    let handles = parents.iter().enumerate().map(|(n, parent)| {
        let barrier = barrier.clone();
        let socket = socket.clone();
        let params = json!({"work_unit_id":format!("shared-claude-auth-{n}"),
            "parent_run_id":parent,"min_tier":"general","required_tools":["browser/navigate"],
            "prompt":"browser check"});
        std::thread::spawn(move || -> Result<serde_json::Value, String> {
            barrier.wait();
            let mut conn = UnixStream::connect(socket).map_err(|error| error.to_string())?;
            conn.set_read_timeout(Some(Duration::from_secs(15))).map_err(|error| error.to_string())?;
            conn.write_all(format!("{}\n", json!({"id":1,"method":"auto.dispatch","params":params})).as_bytes())
                .map_err(|error| error.to_string())?;
            let mut line = String::new();
            BufReader::new(conn).read_line(&mut line).map_err(|error| error.to_string())?;
            let reply: serde_json::Value = serde_json::from_str(&line).map_err(|error| error.to_string())?;
            if let Some(error) = reply.get("error") { return Err(error.to_string()); }
            Ok(reply["result"].clone())
        })
    }).collect::<Vec<_>>();
    barrier.wait();
    let results = handles.into_iter().map(|handle| handle.join().unwrap().unwrap()).collect::<Vec<_>>();
    assert!(results.iter().all(|result| result["state"] == "paused"), "{results:?}");
    assert_eq!(std::fs::read_to_string(&count_file).unwrap(), "1",
        "overlapping decisions should use one Claude auth process");
    let later = d.call("auto.dispatch", json!({"work_unit_id":"shared-claude-auth-later",
        "parent_run_id":parents[0],"min_tier":"general","required_tools":["browser/navigate"],
        "prompt":"browser check again"}));
    assert_eq!(later["state"], "paused");
    assert_eq!(std::fs::read_to_string(&count_file).unwrap(), "2",
        "later work must refresh Claude account identity");
    assert_eq!(d.runs().len(), 2);
}

#[test]
fn auto_managed_child_checks_required_tools_before_starting_a_turn() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let trace = r.path().join("required-tools-trace.txt");
    let mode = r.path().join("tool-mode.txt");
    std::fs::write(&mode, "available").unwrap();
    let d = Daemon::start(&[("OVERSEER_CODEX_PATH", &fixture("fake-harness/codex-app-fixture.js")),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE,FIXTURE_TRACE_FILE,FIXTURE_TOOL_MODE_FILE"),
        ("FIXTURE_MODE", "managed-delegation"), ("FIXTURE_TRACE_FILE", trace.to_str().unwrap()),
        ("FIXTURE_TOOL_MODE_FILE", mode.to_str().unwrap())]);
    let parent = run_id(&d.call("task.create", json!({"repo":repo,"harness":"codex-app","prompt":"seed context"})));
    assert_eq!(d.wait_done(&parent, 15)["status"], "completed");
    let allowed_result = d.call("run.delegate", json!({"work_unit_id":"tool-allowed","parent_run_id":parent,
        "harness":"codex-app","model":"gpt-6-sol","effort":"medium","prompt":"browser check",
        "required_tools":["browser/navigate"]}));
    let allowed = run_id(&allowed_result);
    assert_eq!(d.wait_done(&allowed, 15)["status"], "completed");
    assert_eq!(d.call("run.result", json!({"run_id":allowed}))["state"], "ready");
    std::fs::write(&mode, "missing").unwrap();
    let denied = run_id(&d.call("run.delegate", json!({"work_unit_id":"tool-denied","parent_run_id":parent,
        "harness":"codex-app","model":"gpt-6-sol","effort":"medium","prompt":"browser check",
        "required_tools":["browser/navigate"]})));
    assert_eq!(d.wait_done(&denied, 15)["status"], "failed");
    for (mode_name, unit) in [("logged-out", "tool-logged-out"), ("error", "tool-metadata-error")] {
        std::fs::write(&mode, mode_name).unwrap();
        let run = run_id(&d.call("run.delegate", json!({"work_unit_id":unit,"parent_run_id":parent,
            "harness":"codex-app","model":"gpt-6-sol","effort":"medium","prompt":"browser check",
            "required_tools":["browser/navigate"]})));
        assert_eq!(d.wait_done(&run, 15)["status"], "failed", "{mode_name}");
        assert!(!serde_json::to_string(&d.events(&run)).unwrap().contains("private-tool-error-sentinel"));
    }
    let trace = std::fs::read_to_string(trace).unwrap();
    assert_eq!(trace.matches("tool_preflight:available").count(), 1);
    assert_eq!(trace.matches("tool_preflight:missing").count(), 1);
    assert_eq!(trace.matches("tool_preflight:logged-out").count(), 1);
    assert_eq!(trace.matches("tool_preflight:error").count(), 1);
    assert!(trace.contains(&format!("tool_cwd:{}", allowed_result["workspace"]["path"].as_str().unwrap())),
        "the child must inspect tools in its own workspace");
    assert_eq!(trace.matches("turn_model:gpt-6-sol").count(), 1,
        "the denied child must stop before its model turn");
    assert!(d.try_call("run.delegate", json!({"work_unit_id":"invalid-tool","parent_run_id":parent,
        "harness":"codex-app","model":"gpt-6-sol","effort":"medium","prompt":"browser check",
        "required_tools":["browser/navigate;unsafe"]})).is_err());
    assert_eq!(d.runs().len(), 5, "invalid tool names must fail before creating a run");
}

#[test]
fn auto_managed_child_unknown_quota_metadata_still_runs_without_account_attribution() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let d = Daemon::start(&[("OVERSEER_CODEX_PATH", &fixture("fake-harness/codex-app-fixture.js")),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE"), ("FIXTURE_MODE", "managed-no-quota")]);
    let parent = run_id(&d.call("task.create", json!({"repo":repo,"harness":"codex-app","prompt":"seed context"})));
    assert_eq!(d.wait_done(&parent, 15)["status"], "completed");
    let child = run_id(&d.call("run.delegate", json!({"work_unit_id":"unknown-quota-unit","parent_run_id":parent,"harness":"codex-app","model":"gpt-6-sol","effort":"medium","prompt":"browser check"})));
    assert_eq!(d.wait_done(&child, 15)["status"], "completed");
    assert_eq!(d.call("run.result", json!({"run_id":child}))["state"], "ready");
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let evidence: i64 = db.query_row("SELECT COUNT(*) FROM auto_run_account_evidence WHERE run_id=?1", [&child], |row| row.get(0)).unwrap();
    assert_eq!(evidence, 0, "unsupported metadata cannot become inferred account evidence");
}

#[test]
fn auto_managed_child_rejects_api_key_login_before_turn() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let auth_file = r.path().join("auth-kind.txt");
    let trace = r.path().join("key-trace.txt");
    std::fs::write(&auth_file, "key").unwrap();
    let d = Daemon::start(&[("OVERSEER_CODEX_PATH", &fixture("fake-harness/codex-app-fixture.js")),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE,FIXTURE_AUTH_FILE,FIXTURE_TRACE_FILE"),
        ("FIXTURE_MODE", "managed-delegation"), ("FIXTURE_AUTH_FILE", auth_file.to_str().unwrap()),
        ("FIXTURE_TRACE_FILE", trace.to_str().unwrap())]);
    let parent = run_id(&d.call("task.create", json!({"repo":repo,"harness":"codex-app","prompt":"seed context"})));
    assert_eq!(d.wait_done(&parent, 15)["status"], "completed");
    let child = run_id(&d.call("run.delegate", json!({"work_unit_id":"key-login-unit","parent_run_id":parent,"harness":"codex-app","model":"gpt-6-sol","effort":"medium","prompt":"browser check"})));
    assert_eq!(d.wait_done(&child, 15)["status"], "failed");
    let trace = std::fs::read_to_string(trace).unwrap();
    assert_eq!(trace.lines().filter(|line| *line == "thread_started").count(), 1,
        "the child must not start a model thread under API-key auth");
}

#[test]
fn auto_managed_child_silent_account_metadata_times_out_without_a_model_turn() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let trace = r.path().join("silent-metadata-trace.txt");
    let d = Daemon::start(&[("OVERSEER_CODEX_PATH", &fixture("fake-harness/codex-app-fixture.js")),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE,FIXTURE_TRACE_FILE"),
        ("FIXTURE_MODE", "managed-silent-metadata"), ("FIXTURE_TRACE_FILE", trace.to_str().unwrap())]);
    let parent = run_id(&d.call("task.create", json!({"repo":repo,"harness":"codex-app","prompt":"seed context"})));
    assert_eq!(d.wait_done(&parent, 15)["status"], "completed");
    let child = run_id(&d.call("run.delegate", json!({"work_unit_id":"silent-account-unit","parent_run_id":parent,"harness":"codex-app","model":"gpt-6-sol","effort":"medium","prompt":"browser check"})));
    let done = d.wait_done(&child, 12);
    assert_eq!(done["status"], "failed", "{done}");
    assert!(done["exit_reason"].as_str().unwrap().contains("metadata handshake timed out"));
    let trace = std::fs::read_to_string(trace).unwrap();
    assert_eq!(trace.lines().filter(|line| *line == "thread_started").count(), 1);
    assert_eq!(trace.lines().filter(|line| *line == "metadata_silent").count(), 1);
}

#[test]
fn auto_managed_child_metadata_deadline_survives_daemon_restart() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let mut d = Daemon::start(&[("OVERSEER_CODEX_PATH", &fixture("fake-harness/codex-app-fixture.js")),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE"),
        ("FIXTURE_MODE", "managed-silent-metadata")]);
    let parent = run_id(&d.call("task.create", json!({"repo":repo,"harness":"codex-app","prompt":"seed context"})));
    assert_eq!(d.wait_done(&parent, 15)["status"], "completed");
    let child = run_id(&d.call("run.delegate", json!({"work_unit_id":"restart-silent-unit","parent_run_id":parent,"harness":"codex-app","model":"gpt-6-sol","effort":"medium","prompt":"browser check"})));
    d.kill9();
    std::thread::sleep(Duration::from_secs(6));
    d.spawn();
    let done = d.wait_done(&child, 5);
    assert_eq!(done["status"], "failed", "{done}");
    assert!(done["exit_reason"].as_str().unwrap().contains("metadata handshake timed out"));
    assert_eq!(done["process_generation"], 1, "reconciliation must not relaunch the child");
}

#[test]
fn auto_managed_child_reattaches_after_daemon_restart_without_relaunch() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let mut d = Daemon::start(&[("OVERSEER_CODEX_PATH", &fixture("fake-harness/codex-app-fixture.js")),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE"), ("FIXTURE_MODE", "managed-delay")]);
    let parent = run_id(&d.call("task.create", json!({"repo":repo,"harness":"codex-app","prompt":"seed context","title":"parent"})));
    assert_eq!(d.wait_done(&parent, 15)["status"], "completed");
    let request = json!({"work_unit_id":"browser-restart-unit","parent_run_id":parent,"harness":"codex-app","model":"gpt-6-sol","effort":"medium","prompt":"browser check"});
    let child = run_id(&d.call("run.delegate", request.clone()));
    d.wait_status(&child, |status| status == "running", 10);
    d.kill9();
    d.spawn();
    let repeated = d.call("run.delegate", request);
    assert_eq!(repeated["run"]["id"], child);
    assert_eq!(repeated["replayed"], true);
    assert_eq!(d.runs().len(), 2);
    assert_eq!(d.wait_done(&child, 15)["status"], "completed");
    assert_eq!(d.run(&child)["process_generation"], 1);
    assert_eq!(d.call("run.result", json!({"run_id":child}))["text"], "browser result: parent context found");
    assert_eq!(d.events(&child).iter().filter(|event| event["kind"] == "output" && event["payload"]["role"] == "assistant").count(), 1);
    let notices = d.events(&parent).into_iter().filter(|event|
        event["kind"] == "managed_child_result_available").collect::<Vec<_>>();
    assert_eq!(notices.len(), 1, "restart must neither lose nor duplicate the parent result notice");
    assert_eq!(notices[0]["payload"]["child_run_id"], child);
}

#[test]
fn auto_running_child_survives_lost_dispatch_response_and_two_client_reconnect() {
    use std::io::Write;
    use std::os::unix::net::UnixStream;
    use std::time::Instant;
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let trace = r.path().join("post-spawn-trace.txt");
    let mut d = Daemon::start(&[("OVERSEER_CODEX_PATH", &fixture("fake-harness/codex-app-fixture.js")),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE,FIXTURE_TRACE_FILE,FIXTURE_TURN_DELAY_MS"),
        ("FIXTURE_MODE", "managed-models"), ("FIXTURE_TURN_DELAY_MS", "6000"),
        ("FIXTURE_TRACE_FILE", trace.to_str().unwrap())]);
    let parent = run_id(&d.call("task.create", json!({"repo":repo,"harness":"codex-app",
        "model":"gpt-6-astra","effort":"high","prompt":"seed context"})));
    assert_eq!(d.wait_done(&parent, 15)["status"], "completed");
    let request = json!({"work_unit_id":"auto-post-spawn-1","parent_run_id":parent,
        "min_tier":"general","required_tools":[],"prompt":"browser check"});
    let mut lost_client = UnixStream::connect(d.socket()).unwrap();
    lost_client.write_all(format!("{}\n", json!({"id":1,"method":"auto.dispatch",
        "params":request})).as_bytes()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let child = loop {
        let runs = d.runs();
        if let Some(run) = runs.iter().find(|run| run["parent_run_id"] == parent) {
            let child = run["id"].as_str().unwrap().to_string();
            if d.events(&child).iter().any(|event| event["kind"] == "turn_started") { break child; }
        }
        assert!(Instant::now() < deadline, "Auto child never reached the post-spawn crash window");
        std::thread::sleep(Duration::from_millis(20));
    };
    d.wait_status(&child, |status| status == "running", 10);
    d.kill9();
    drop(lost_client);
    d.spawn();
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let claim_state = || db.query_row("SELECT state FROM auto_pool_claims WHERE work_unit_id='auto-post-spawn-1'",
        [], |row| row.get::<_, String>(0)).unwrap();
    assert_eq!(claim_state(), "active", "restart must keep the claim for a live child");
    let first = d.call("auto.dispatch", request.clone());
    let second = d.call("auto.dispatch", request);
    for replay in [&first, &second] {
        assert_eq!(replay["run"]["id"], child);
        assert_eq!(replay["replayed"], true);
    }
    assert_eq!(d.wait_done(&child, 15)["status"], "completed");
    assert_eq!(claim_state(), "released", "confirmed child settlement releases the claim once");
    assert_eq!(d.run(&child)["process_generation"], 1);
    assert_eq!(d.runs().len(), 2);
    assert_eq!(d.events(&parent).into_iter().filter(|event| event["kind"] == "auto_decision").count(), 1);
    assert_eq!(d.events(&parent).into_iter().filter(|event|
        event["kind"] == "managed_child_result_available").count(), 1);
    assert_eq!(std::fs::read_to_string(&trace).unwrap().matches("turn_model:gpt-6-sol").count(), 1,
        "lost dispatch response must not start a second model turn");
}

#[test]
fn auto_post_spawn_write_failure_keeps_unknown_draw_claim_until_supervisor_settles() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let trace = r.path().join("post-spawn-write-trace.txt");
    let mut d = Daemon::start(&[("OVERSEER_CODEX_PATH", &fixture("fake-harness/codex-app-fixture.js")),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE,FIXTURE_TRACE_FILE,FIXTURE_TURN_DELAY_MS"),
        ("FIXTURE_MODE", "managed-models"), ("FIXTURE_TURN_DELAY_MS", "6000"),
        ("FIXTURE_TRACE_FILE", trace.to_str().unwrap())]);
    let parent = run_id(&d.call("task.create", json!({"repo":repo,"harness":"codex-app",
        "model":"gpt-6-astra","effort":"high","prompt":"seed context"})));
    assert_eq!(d.wait_done(&parent, 15)["status"], "completed");
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    db.execute_batch("CREATE TRIGGER reject_child_start_status BEFORE UPDATE OF status ON runs
        WHEN NEW.status='starting' AND NEW.parent_run_id IS NOT NULL
        BEGIN SELECT RAISE(FAIL, 'injected post-spawn status failure'); END;").unwrap();
    let first = d.call("auto.dispatch", json!({"work_unit_id":"post-spawn-write-1",
        "parent_run_id":parent,"min_tier":"general","required_tools":[],
        "prompt":"browser check"}));
    assert_eq!(first["state"], "paused", "{first}");
    let child = d.runs().into_iter().find(|run| run["parent_run_id"] == parent).unwrap();
    assert_eq!(child["process_generation"], 1, "failure occurred after a supervisor was spawned");
    let claim: String = db.query_row("SELECT state FROM auto_pool_claims WHERE work_unit_id='post-spawn-write-1'",
        [], |row| row.get(0)).unwrap();
    assert_ne!(claim, "released", "the supervisor may still consume allowance after a status-write failure");
    db.execute_batch("DROP TRIGGER reject_child_start_status;").unwrap();
    d.kill9();
    d.spawn();
    let child_id = child["id"].as_str().unwrap();
    assert_eq!(d.wait_done(child_id, 15)["status"], "completed");
    let settled_claim: String = db.query_row("SELECT state FROM auto_pool_claims WHERE work_unit_id='post-spawn-write-1'",
        [], |row| row.get(0)).unwrap();
    assert_eq!(settled_claim, "released");
}

#[test]
fn auto_post_spawn_write_failure_reattaches_without_a_daemon_restart() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let d = Daemon::start(&[("OVERSEER_CODEX_PATH", &fixture("fake-harness/codex-app-fixture.js")),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE,FIXTURE_TURN_DELAY_MS"),
        ("FIXTURE_MODE", "managed-models"), ("FIXTURE_TURN_DELAY_MS", "1000")]);
    let parent = run_id(&d.call("task.create", json!({"repo":repo,"harness":"codex-app",
        "model":"gpt-6-astra","effort":"high","prompt":"seed context"})));
    assert_eq!(d.wait_done(&parent, 15)["status"], "completed");
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    db.execute_batch("CREATE TRIGGER reject_child_start_status BEFORE UPDATE OF status ON runs
        WHEN NEW.status='starting' AND NEW.parent_run_id IS NOT NULL
        BEGIN SELECT RAISE(FAIL, 'injected post-spawn status failure'); END;").unwrap();
    let response = d.call("auto.dispatch", json!({"work_unit_id":"post-spawn-recover-local",
        "parent_run_id":parent,"min_tier":"general","required_tools":[],
        "prompt":"browser check"}));
    assert_eq!(response["state"], "paused", "{response}");
    let child = d.runs().into_iter().find(|run| run["parent_run_id"] == parent).unwrap();
    let child_id = child["id"].as_str().unwrap();
    db.execute_batch("DROP TRIGGER reject_child_start_status;").unwrap();
    assert_eq!(d.wait_done(child_id, 10)["status"], "completed",
        "the original supervisor should settle without requiring a daemon restart");
    let claim: String = db.query_row("SELECT state FROM auto_pool_claims WHERE work_unit_id='post-spawn-recover-local'",
        [], |row| row.get(0)).unwrap();
    assert_eq!(claim, "released");
}

#[test]
fn auto_supervisor_is_not_spawned_before_its_durable_identity_is_written() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let trace = r.path().join("supervisor-order-trace.txt");
    let d = Daemon::start(&[("OVERSEER_CODEX_PATH", &fixture("fake-harness/codex-app-fixture.js")),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE,FIXTURE_TRACE_FILE"),
        ("FIXTURE_MODE", "managed-models"), ("FIXTURE_TRACE_FILE", trace.to_str().unwrap())]);
    let parent = run_id(&d.call("task.create", json!({"repo":repo,"harness":"codex-app",
        "model":"gpt-6-astra","effort":"high","prompt":"seed context"})));
    assert_eq!(d.wait_done(&parent, 15)["status"], "completed");
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    db.execute_batch("CREATE TRIGGER reject_child_process_identity BEFORE UPDATE OF run_dir ON runs
        WHEN NEW.parent_run_id IS NOT NULL AND NEW.run_dir IS NOT NULL
        BEGIN SELECT RAISE(FAIL, 'injected supervisor identity failure'); END;").unwrap();
    let response = d.call("auto.dispatch", json!({"work_unit_id":"supervisor-order-1",
        "parent_run_id":parent,"min_tier":"general","required_tools":[],
        "prompt":"browser check"}));
    assert_eq!(response["state"], "paused", "{response}");
    let child = d.runs().into_iter().find(|run| run["parent_run_id"] == parent).unwrap();
    let child_id = child["id"].as_str().unwrap();
    std::thread::sleep(Duration::from_millis(1200));
    assert!(!d.home.path().join("runs").join(child_id).join("p1/shim.json").exists(),
        "the actual supervisor must not start before its identity can be committed");
    assert_eq!(std::fs::read_to_string(&trace).unwrap().matches("turn_model:gpt-6-sol").count(), 0,
        "no harness turn may start without a durable supervisor identity");
    let claim: String = db.query_row("SELECT state FROM auto_pool_claims WHERE work_unit_id='supervisor-order-1'",
        [], |row| row.get(0)).unwrap();
    assert_eq!(claim, "released", "a rejected pre-spawn identity write consumed no allowance");
}

#[test]
fn auto_replay_does_not_claim_a_committed_but_unstarted_child_was_dispatched() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let trace = r.path().join("unstarted-child-trace.txt");
    let mut d = Daemon::start(&[("OVERSEER_CODEX_PATH", &fixture("fake-harness/codex-app-fixture.js")),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE,FIXTURE_TRACE_FILE"),
        ("FIXTURE_MODE", "managed-models"), ("FIXTURE_TRACE_FILE", trace.to_str().unwrap())]);
    let parent = run_id(&d.call("task.create", json!({"repo":repo,"harness":"codex-app",
        "model":"gpt-6-astra","effort":"high","prompt":"seed context"})));
    assert_eq!(d.wait_done(&parent, 15)["status"], "completed");
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    db.execute_batch("CREATE TRIGGER reject_child_created_notice BEFORE INSERT ON events
        WHEN NEW.kind='managed_child_created'
        BEGIN SELECT RAISE(FAIL, 'injected child notice failure'); END;").unwrap();
    let request = json!({"work_unit_id":"unstarted-auto-child-1","parent_run_id":parent,
        "min_tier":"general","required_tools":[],"prompt":"bounded child task"});
    let first = d.call("auto.dispatch", request.clone());
    assert_eq!(first["state"], "paused", "{first}");
    let children = d.runs().into_iter().filter(|run| run["parent_run_id"] == parent)
        .collect::<Vec<_>>();
    assert_eq!(children.len(), 1, "child identity was committed before the notice failed");
    let child = children[0]["id"].as_str().unwrap().to_string();
    assert_eq!(children[0]["status"], "failed",
        "the stopped pre-turn child must settle without entering profile activity checks");
    assert_eq!(children[0]["process_generation"], 0);
    let before_restart = d.call("auto.dispatch", request.clone());
    assert_eq!(before_restart["state"], "paused",
        "unstarted child cannot be reported as dispatched: {before_restart}");
    assert_eq!(before_restart["pause_reason"], "launch_effects_uncertain");
    assert_eq!(before_restart["run"]["id"], child);
    let claim: String = db.query_row("SELECT state FROM auto_pool_claims WHERE work_unit_id='unstarted-auto-child-1'",
        [], |row| row.get(0)).unwrap();
    assert_eq!(claim, "released", "a stopped worker before the child turn cannot hold subscription allowance");
    db.execute_batch("DROP TRIGGER reject_child_created_notice;").unwrap();
    let second = d.call("auto.dispatch", json!({"work_unit_id":"unstarted-auto-child-2",
        "parent_run_id":parent,"min_tier":"general","required_tools":[],
        "prompt":"second bounded child task"}));
    assert_eq!(second["state"], "dispatched", "a separate unit should use the released allowance: {second}");
    let second_child = second["run"]["id"].as_str().unwrap().to_string();
    assert_eq!(d.wait_done(&second_child, 15)["status"], "completed");
    d.kill9();
    d.spawn();
    let replay = d.call("auto.dispatch", request);
    assert_eq!(replay["state"], "paused", "unstarted child cannot be reported as dispatched: {replay}");
    assert_eq!(replay["pause_reason"], "launch_effects_uncertain");
    assert_eq!(replay["run"]["id"], child);
    assert_eq!(d.runs().len(), 3);
    assert_eq!(std::fs::read_to_string(&trace).unwrap().matches("turn_model:gpt-6-sol").count(), 1,
        "replay must not start the first, unstarted child automatically");
}

#[test]
fn auto_managed_result_notice_failure_rolls_back_settlement_and_recovers_once() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let mut d = Daemon::start(&[("OVERSEER_CODEX_PATH", &fixture("fake-harness/codex-app-fixture.js")),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE"), ("FIXTURE_MODE", "managed-delay")]);
    let parent = run_id(&d.call("task.create", json!({"repo":repo,"harness":"codex-app",
        "prompt":"seed context","title":"parent"})));
    assert_eq!(d.wait_done(&parent, 15)["status"], "completed");
    let child = run_id(&d.call("run.delegate", json!({"work_unit_id":"browser-notice-fault",
        "parent_run_id":parent,"harness":"codex-app","model":"gpt-6-sol",
        "effort":"medium","prompt":"browser check"})));
    d.wait_status(&child, |status| status == "running", 10);
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    db.execute_batch("CREATE TRIGGER fail_result_notice BEFORE INSERT ON events
        WHEN NEW.kind='managed_child_result_available'
        BEGIN SELECT RAISE(FAIL, 'fixture notice failure'); END;").unwrap();
    std::thread::sleep(Duration::from_secs(7));
    assert_eq!(d.run(&child)["status"], "running", "failed notice must not leave a completed child without delivery");
    let current = d.call("state", json!({}));
    assert_eq!(current["turns"][&child].as_array().unwrap().last().unwrap()["status"], "completed",
        "the turn-completion signal was durable before settlement failed");
    assert!(d.events(&parent).iter().all(|event| event["kind"] != "managed_child_result_available"));
    db.execute_batch("DROP TRIGGER fail_result_notice;").unwrap();
    d.kill9();
    d.spawn();
    assert_eq!(d.wait_done(&child, 15)["status"], "completed");
    assert_eq!(d.run(&child)["process_generation"], 1, "recovery must not launch a second child");
    let notices = d.events(&parent).into_iter().filter(|event|
        event["kind"] == "managed_child_result_available").collect::<Vec<_>>();
    assert_eq!(notices.len(), 1);
    assert_eq!(notices[0]["payload"]["child_run_id"], child);
}

#[test]
fn auto_parent_interrupt_propagates_to_active_managed_child() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let d = Daemon::start(&[("OVERSEER_CODEX_PATH", &fixture("fake-harness/codex-app-fixture.js")),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE"), ("FIXTURE_MODE", "managed-delay")]);
    let parent = run_id(&d.call("task.create", json!({"repo":repo,"harness":"codex-app","prompt":"seed context","title":"parent"})));
    assert_eq!(d.wait_done(&parent, 15)["status"], "completed");
    let child = run_id(&d.call("run.delegate", json!({"work_unit_id":"browser-cancel-unit","parent_run_id":parent,"harness":"codex-app","model":"gpt-6-sol","effort":"medium","prompt":"browser check"})));
    d.wait_status(&child, |status| status == "running", 10);
    d.call("run.follow_up", json!({"run_id":parent,"prompt":"hold parent"}));
    d.wait_status(&parent, |status| status == "running", 10);
    d.call("run.interrupt", json!({"run_id":parent}));
    assert_eq!(d.wait_done(&parent, 15)["status"], "interrupted");
    assert_eq!(d.wait_done(&child, 15)["status"], "interrupted");
    assert_ne!(d.call("run.result", json!({"run_id":child}))["state"], "ready");
}

#[test]
fn auto_managed_child_launch_failure_releases_its_workspace() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let d = Daemon::start(&[("OVERSEER_CODEX_PATH", &fixture("fake-harness/codex-app-fixture.js")),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE"), ("FIXTURE_MODE", "managed-delegation")]);
    let parent = run_id(&d.call("task.create", json!({"repo":repo,"harness":"codex-app","prompt":"seed context","title":"parent"})));
    assert_eq!(d.wait_done(&parent, 15)["status"], "completed");
    let delegated = d.call("run.delegate", json!({"work_unit_id":"failed-launch-unit","parent_run_id":parent,"harness":"claude","model":"claude-sonnet","effort":"medium","prompt":"browser check"}));
    let child = delegated["run"]["id"].as_str().unwrap();
    assert_eq!(d.wait_done(child, 15)["status"], "failed");
    assert_eq!(d.call("run.result", json!({"run_id":child}))["state"], "not_completed");
    let workspace_id = delegated["workspace"]["id"].as_str().unwrap();
    let state = d.call("state", json!({}));
    let workspace = state["workspaces"].as_array().unwrap().iter().find(|w| w["id"] == workspace_id).unwrap();
    assert!(workspace["owner_run_id"].is_null());
    assert_eq!(d.runs().len(), 2);
}

#[test]
fn auto_failed_worktree_launch_keeps_one_intent_across_restart() {
    use std::os::unix::fs::PermissionsExt;
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let bin = r.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    let attempts = r.path().join("worktree-attempts");
    let wrapper = bin.join("git");
    std::fs::write(&wrapper, format!("#!/bin/sh\nif [ \"$1\" = worktree ] && [ \"$2\" = add ] && [ \"$3\" = -b ]; then\n  case \"$4\" in overseer/delegate-*) printf 'x\\n' >> '{}'; exit 43;; esac\nfi\nexec /usr/bin/git \"$@\"\n", attempts.display())).unwrap();
    std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o700)).unwrap();
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap());
    let mut d = Daemon::start(&[("PATH", &path),
        ("OVERSEER_CODEX_PATH", &fixture("fake-harness/codex-app-fixture.js")),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE"),
        ("FIXTURE_MODE", "managed-models")]);
    let parent = run_id(&d.call("task.create", json!({"repo":repo,"harness":"codex-app",
        "model":"gpt-6-astra","effort":"high","prompt":"seed context"})));
    assert_eq!(d.wait_done(&parent, 15)["status"], "completed");
    let request = json!({"work_unit_id":"intent-failed-worktree-1",
        "parent_run_id":parent,"min_tier":"general","required_tools":[],
        "prompt":"continue after the checkpoint"});
    let first = d.call("auto.dispatch", request.clone());
    assert_eq!(first["state"], "paused", "{first}");
    assert_eq!(first["pause_reason"], "launch_effects_uncertain");
    assert_eq!(std::fs::read_to_string(&attempts).unwrap().lines().count(), 1);
    assert_eq!(d.runs().len(), 1, "no child was recorded after the Git failure");
    d.kill9();
    d.env.iter_mut().find(|(key, _)| key == "FIXTURE_MODE").unwrap().1 = "managed-delay".into();
    d.spawn();
    d.call("run.follow_up", json!({"run_id":parent,"prompt":"continue parent work"}));
    d.wait_status(&parent, |status| status == "running", 10);
    let replay = d.call("auto.dispatch", request);
    assert_eq!(replay["state"], "paused", "{replay}");
    assert_eq!(replay["pause_reason"], "launch_effects_uncertain");
    assert_eq!(std::fs::read_to_string(&attempts).unwrap().lines().count(), 1,
        "replay must not attempt the Git mutation again");
    assert_eq!(d.runs().len(), 1, "replay must not create a second child");
    assert!(d.try_call("run.delegate", json!({"work_unit_id":"intent-failed-worktree-1",
        "parent_run_id":parent,"harness":"codex-app","model":"gpt-6-sol",
        "effort":"medium","prompt":"manual retry with the same identity"})).is_err());
    assert_eq!(std::fs::read_to_string(&attempts).unwrap().lines().count(), 1,
        "manual delegation cannot reuse an unsettled Auto work-unit identity");
}

#[test]
fn auto_crash_after_git_worktree_effect_reports_planned_resource_without_retry() {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::net::UnixStream;
    use std::time::Instant;
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let bin = r.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    let marker = r.path().join("git-effect");
    let attempts = r.path().join("git-attempts");
    let wrapper = bin.join("git");
    std::fs::write(&wrapper, format!("#!/bin/sh\nif [ \"$1\" = worktree ] && [ \"$2\" = add ] && [ \"$3\" = -b ]; then\n  case \"$4\" in overseer/delegate-*|overseer/auto-*)\n    printf 'x\\n' >> '{}'\n    /usr/bin/git \"$@\" || exit $?\n    printf '%s\\n%s\\n%s\\n' \"$$\" \"$4\" \"$5\" > '{}'\n    exec /bin/sleep 30;;\n  esac\nfi\nexec /usr/bin/git \"$@\"\n", attempts.display(), marker.display())).unwrap();
    std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o700)).unwrap();
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap());
    let mut d = Daemon::start(&[("PATH", &path),
        ("OVERSEER_CODEX_PATH", &fixture("fake-harness/codex-app-fixture.js")),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE"),
        ("FIXTURE_MODE", "managed-models")]);
    let parent = run_id(&d.call("task.create", json!({"repo":repo,"harness":"codex-app",
        "model":"gpt-6-astra","effort":"high","prompt":"seed context"})));
    assert_eq!(d.wait_done(&parent, 15)["status"], "completed");
    let request = json!({"work_unit_id":"git-effect-crash-1",
        "parent_run_id":parent,"min_tier":"general","required_tools":[],
        "prompt":"continue after the checkpoint"});
    let socket = d.socket();
    let started = Instant::now();
    let sent = request.clone();
    let pending = std::thread::spawn(move || {
        let mut conn = UnixStream::connect(socket).unwrap();
        conn.set_read_timeout(Some(Duration::from_secs(12))).unwrap();
        conn.write_all(format!("{}\n", json!({"id":1,"method":"auto.dispatch","params":sent})).as_bytes()).unwrap();
        let mut line = String::new();
        BufReader::new(conn).read_line(&mut line).unwrap();
        serde_json::from_str::<serde_json::Value>(&line).unwrap()
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    while !marker.exists() {
        assert!(Instant::now() < deadline, "Git worktree creation did not reach the crash window");
        std::thread::sleep(Duration::from_millis(10));
    }
    let effect = std::fs::read_to_string(&marker).unwrap();
    let mut parts = effect.lines();
    let wrapper_pid: i64 = parts.next().unwrap().parse().unwrap();
    let branch = parts.next().unwrap().to_string();
    let worktree = parts.next().unwrap().to_string();
    assert!(Path::new(&worktree).exists(), "the Git effect must have happened before the crash");
    let second_started = Instant::now();
    let during = d.call("auto.dispatch", request.clone());
    let second_elapsed = second_started.elapsed();
    let first = pending.join().unwrap();
    assert!(second_elapsed < Duration::from_secs(2),
        "a second client must read the admitted pending launch without waiting for the first response");
    assert!(started.elapsed() < Duration::from_secs(11), "selected launch must respond before its deadline");
    assert_eq!(first["result"]["state"], "launch_pending", "{first}");
    assert_eq!(first["result"]["work_unit_id"], "git-effect-crash-1");
    assert_eq!(during["state"], "launch_pending", "{during}");
    assert!(during["replayed"] == true);
    let decisions = d.events(&parent).into_iter().filter(|event| event["kind"] == "auto_decision").count();
    assert_eq!(decisions, 1, "two clients must share one recorded decision");
    d.kill9();
    signal(wrapper_pid, 9);
    d.spawn();
    let replay = d.call("auto.dispatch", request);
    assert_eq!(replay["state"], "paused", "{replay}");
    assert_eq!(replay["pause_reason"], "launch_effects_uncertain");
    assert_eq!(replay["launch_resources"]["branch"], branch);
    assert_eq!(replay["launch_resources"]["path"], worktree);
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let claim: String = db.query_row("SELECT state FROM auto_pool_claims WHERE work_unit_id='git-effect-crash-1'",
        [], |row| row.get(0)).unwrap();
    assert_eq!(claim, "released", "no committed child can have spent model allowance; uncertain Git resources remain separately journaled");
    assert_eq!(std::fs::read_to_string(&attempts).unwrap().lines().count(), 1,
        "a crash after the Git effect must not attempt another worktree add");
    assert_eq!(d.runs().len(), 1, "no child was committed before the crash");
}

#[test]
fn auto_stalled_git_worktree_add_stops_and_pauses_one_launch() {
    use std::os::unix::fs::PermissionsExt;
    use std::time::Instant;
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let bin = r.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    let marker = r.path().join("stalled-git");
    let attempts = r.path().join("git-attempts");
    let wrapper = bin.join("git");
    std::fs::write(&wrapper, format!("#!/bin/sh\nif [ \"$1\" = worktree ] && [ \"$2\" = add ] && [ \"$3\" = -b ]; then\n  case \"$4\" in overseer/delegate-*|overseer/auto-*)\n    printf 'x\\n' >> '{}'\n    printf '%s\\n' \"$$\" > '{}'\n    exec /bin/sleep 60;;\n  esac\nfi\nexec /usr/bin/git \"$@\"\n", attempts.display(), marker.display())).unwrap();
    std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o700)).unwrap();
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap());
    let d = Daemon::start(&[("PATH", &path),
        ("OVERSEER_CODEX_PATH", &fixture("fake-harness/codex-app-fixture.js")),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE"),
        ("FIXTURE_MODE", "managed-models")]);
    let parent = run_id(&d.call("task.create", json!({"repo":repo,"harness":"codex-app",
        "model":"gpt-6-astra","effort":"high","prompt":"seed context"})));
    assert_eq!(d.wait_done(&parent, 15)["status"], "completed");
    let request = json!({"work_unit_id":"stalled-git-1", "parent_run_id":parent,
        "min_tier":"general","required_tools":[],"prompt":"continue after the checkpoint"});
    let start = Instant::now();
    let first = d.call("auto.dispatch", request.clone());
    assert_eq!(first["state"], "launch_pending", "{first}");
    assert!(start.elapsed() < Duration::from_secs(11));
    let ui_start = Instant::now();
    assert_eq!(d.call("state", json!({}))["runs"].as_array().unwrap().len(), 1);
    assert!(ui_start.elapsed() < Duration::from_secs(2),
        "a pending Git launch must not block an ordinary UI state read");
    let deadline = Instant::now() + Duration::from_secs(23);
    let last = loop {
        let replay = d.call("auto.dispatch", request.clone());
        if replay["state"] == "paused" { break replay; }
        if Instant::now() >= deadline { break replay; }
        std::thread::sleep(Duration::from_millis(100));
    };
    let pid: i32 = std::fs::read_to_string(&marker).unwrap().trim().parse().unwrap();
    let still_running = unsafe { libc::kill(pid, 0) } == 0;
    if still_running { signal(pid as i64, 9); }
    assert_eq!(last["state"], "paused", "stalled Git never settled: {last}");
    assert!(!still_running, "timed-out Git process remained alive");
    assert_eq!(last["pause_reason"], "launch_effects_uncertain");
    assert_eq!(std::fs::read_to_string(&attempts).unwrap().lines().count(), 1);
    assert_eq!(d.runs().len(), 1, "a stalled Git operation cannot create a child");
}

#[test]
fn auto_codex_null_thread_usage_stays_unavailable_without_token_based_guess() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let d = Daemon::start(&[("OVERSEER_CODEX_PATH", &fixture("fake-harness/codex-app-fixture.js")),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE"), ("FIXTURE_MODE", "metadata-usage-null")]);
    let created = d.call("task.create", json!({"repo":repo,"harness":"codex-app","prompt":"requested work","title":"null credit fixture"}));
    let run = run_id(&created);
    assert_eq!(d.wait_done(&run, 15)["status"], "completed");
    let observed = d.call("auto.usage.thread.refresh", json!({"run_id":run}));
    assert_eq!(observed["state"], "unavailable", "{observed}");
    assert!(d.call("auto.usage.thread.list", json!({}))["observations"].as_array().unwrap().is_empty());
}

#[test]
fn auto_failed_thread_usage_read_invalidates_stale_account_capacity() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let auth_file = r.path().join("auth-state.txt");
    std::fs::write(&auth_file, "chatgpt").unwrap();
    let d = Daemon::start(&[("OVERSEER_CODEX_PATH", &fixture("fake-harness/codex-app-fixture.js")),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE,FIXTURE_AUTH_FILE"),
        ("FIXTURE_MODE", "metadata-usage"), ("FIXTURE_AUTH_FILE", auth_file.to_str().unwrap())]);
    let created = d.call("task.create", json!({"repo":repo,"harness":"codex-app","prompt":"requested work","title":"account changed"}));
    let run = run_id(&created);
    assert_eq!(d.wait_done(&run, 15)["status"], "completed");
    d.call("auto.quota.refresh", json!({"profile_id":"system-codex"}));
    assert_eq!(d.call("auto.quota.list", json!({}))["observations"].as_array().unwrap().len(), 1);
    std::fs::write(&auth_file, "key").unwrap();
    assert!(d.try_call("auto.usage.thread.refresh", json!({"run_id":run})).is_err());
    assert!(d.call("auto.quota.list", json!({}))["observations"].as_array().unwrap().is_empty());
    assert!(d.call("auto.usage.thread.list", json!({}))["observations"].as_array().unwrap().is_empty());
}

#[test]
fn auto_thread_usage_read_does_not_extend_earlier_quota_freshness() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let d = Daemon::start(&[("OVERSEER_CODEX_PATH", &fixture("fake-harness/codex-app-fixture.js")),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE,FIXTURE_USAGE_DELAY_MS"),
        ("FIXTURE_MODE", "metadata-usage"), ("FIXTURE_USAGE_DELAY_MS", "1200")]);
    let created = d.call("task.create", json!({"repo":repo,"harness":"codex-app",
        "prompt":"requested work","title":"quota timestamp"}));
    let run = run_id(&created);
    assert_eq!(d.wait_done(&run, 15)["status"], "completed");
    let usage = d.call("auto.usage.thread.refresh", json!({"run_id":run}));
    let quota = d.call("auto.quota.state", json!({"profile_id":"system-codex",
        "harness":"codex-app", "model":"gpt-6-sol"}));
    let usage_ms = usage["observation"]["estimate"]["observed_ms"].as_i64().unwrap();
    let quota_ms = quota["observation"]["snapshot"]["observed_ms"].as_i64().unwrap();
    assert!(usage_ms - quota_ms >= 1_000,
        "the later usage response must not extend quota freshness: usage={usage}, quota={quota}");
}

#[test]
fn auto_codex_native_allowance_update_is_scoped_and_not_usage() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let d = Daemon::start(&[("OVERSEER_CODEX_PATH", &fixture("fake-harness/codex-app-fixture.js")),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE"), ("FIXTURE_MODE", "quota")]);
    let created = d.call("task.create", json!({"repo": repo, "harness": "codex-app", "prompt": "x", "title": "quota"}));
    let run = run_id(&created);
    assert_eq!(d.wait_done(&run, 15)["status"], "completed");
    let result = d.call("auto.quota.list", json!({"limit": 10}));
    let rows = result["observations"].as_array().unwrap();
    assert_eq!(rows.len(), 1, "{result}");
    assert_eq!(rows[0]["pool_id"], "system-codex");
    assert_eq!(rows[0]["snapshot"]["windows"].as_array().unwrap().len(), 2);
    assert_eq!(rows[0]["snapshot"]["windows"][1]["used_percent"], 100.0);
    for harness in ["codex", "codex-app"] {
        let state = d.call("auto.quota.state", json!({"profile_id":"system-codex","harness":harness,"model":"gpt-6-sol"}));
        assert_eq!(state["state"], "exhausted", "{state}");
        assert_eq!(state["pool_id"], "system-codex");
    }
    let other = d.call("auto.quota.state", json!({"profile_id":"system-claude","harness":"claude","model":"claude-sonnet"}));
    assert_eq!(other["state"], "unknown");
    assert!(!result.to_string().contains("secret-credit-sentinel"));
    assert!(d.call("auto.usage.list", json!({}))["measurements"].as_array().unwrap().is_empty());
}


#[test]
fn auto_codex_metadata_refresh_reads_account_quota_without_a_model_turn() {
    let d = Daemon::start(&[("OVERSEER_CODEX_PATH", &fixture("fake-harness/codex-app-fixture.js")),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE"), ("FIXTURE_MODE", "metadata")]);
    let result = d.call("auto.quota.refresh", json!({"profile_id":"system-codex"}));
    assert_eq!(result["state"], "observed_non_exhausted", "{result}");
    assert_eq!(result["pool_id"], "system-codex");
    assert_eq!(result["snapshot"]["windows"].as_array().unwrap().len(), 1);
    assert!(!result.to_string().contains("private-account-id"));
    assert!(!result.to_string().contains("private@example.invalid"));
    assert!(!result.to_string().contains("secret-credit-sentinel"));
    assert!(d.runs().is_empty(), "metadata refresh must not create a task run");
}

#[test]
fn auto_codex_metadata_refresh_refuses_api_key_auth() {
    let d = Daemon::start(&[("OVERSEER_CODEX_PATH", &fixture("fake-harness/codex-app-fixture.js")),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE"), ("FIXTURE_MODE", "metadata-key")]);
    assert!(d.try_call("auto.quota.refresh", json!({"profile_id":"system-codex"})).is_err());
    assert!(d.call("auto.quota.list", json!({}))["observations"].as_array().unwrap().is_empty());
}


#[test]
fn auto_codex_metadata_account_switch_invalidates_prior_quota_without_exposing_identity() {
    let r = tmp();
    let account_file = r.path().join("account-id.txt");
    std::fs::write(&account_file, "first-private-account").unwrap();
    let d = Daemon::start(&[("OVERSEER_CODEX_PATH", &fixture("fake-harness/codex-app-fixture.js")),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE,FIXTURE_ACCOUNT_ID_FILE"),
        ("FIXTURE_MODE", "metadata"), ("FIXTURE_ACCOUNT_ID_FILE", account_file.to_str().unwrap())]);
    d.call("auto.quota.refresh", json!({"profile_id":"system-codex"}));
    d.call("auto.quota.refresh", json!({"profile_id":"system-codex"}));
    assert_eq!(d.call("auto.quota.list", json!({}))["observations"].as_array().unwrap().len(), 2);
    std::fs::write(&account_file, "second-private-account").unwrap();
    let changed = d.call("auto.quota.refresh", json!({"profile_id":"system-codex"}));
    let rows = d.call("auto.quota.list", json!({}));
    assert_eq!(rows["observations"].as_array().unwrap().len(), 1);
    assert!(!changed.to_string().contains("second-private-account"));
    assert!(!rows.to_string().contains("first-private-account"));
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let fingerprint: String = db.query_row("SELECT fingerprint FROM auto_account_identity WHERE profile_id='system-codex'", [], |row| row.get(0)).unwrap();
    assert_eq!(fingerprint.len(), 64);
    assert!(!fingerprint.contains("private-account"));
}


#[test]
fn auto_codex_model_catalog_is_paginated_allowlisted_and_account_scoped() {
    let r = tmp();
    let account_file = r.path().join("account-id.txt");
    std::fs::write(&account_file, "first-private-account").unwrap();
    let d = Daemon::start(&[("OVERSEER_CODEX_PATH", &fixture("fake-harness/codex-app-fixture.js")),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE,FIXTURE_ACCOUNT_ID_FILE"),
        ("FIXTURE_MODE", "metadata-models"), ("FIXTURE_ACCOUNT_ID_FILE", account_file.to_str().unwrap())]);
    let refreshed = d.call("auto.models.refresh", json!({"profile_id":"system-codex"}));
    let models = refreshed["catalog"]["models"].as_array().unwrap();
    assert_eq!(models.len(), 2, "{refreshed}");
    assert_eq!(models[1]["model"], "gpt-6-sol");
    assert_eq!(models[1]["efforts"], json!(["low", "medium"]));
    assert!(!refreshed.to_string().contains("secret-model-sentinel"));
    assert!(!refreshed.to_string().contains("first-private-account"));
    assert!(d.runs().is_empty(), "catalog read must not create a model turn");
    let cached = d.call("auto.models.list", json!({"profile_id":"system-codex"}));
    assert_eq!(cached["catalog"]["models"].as_array().unwrap().len(), 2);
    assert!(!cached.to_string().contains("secret-model-sentinel"));
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let stored: String = db.query_row("SELECT catalog FROM auto_model_catalogs WHERE profile_id='system-codex'", [], |row| row.get(0)).unwrap();
    assert!(!stored.contains("secret-model-sentinel"));
    std::fs::write(&account_file, "second-private-account").unwrap();
    d.call("auto.quota.refresh", json!({"profile_id":"system-codex"}));
    assert!(d.call("auto.models.list", json!({"profile_id":"system-codex"}))["catalog"].is_null());
}

#[test]
fn auto_codex_quota_freshness_starts_when_its_metadata_arrives() {
    let d = Daemon::start(&[("OVERSEER_CODEX_PATH", &fixture("fake-harness/codex-app-fixture.js")),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE,FIXTURE_MODEL_DELAY_MS"),
        ("FIXTURE_MODE", "metadata-models"), ("FIXTURE_MODEL_DELAY_MS", "1200")]);
    let models = d.call("auto.models.refresh", json!({"profile_id":"system-codex"}));
    let quota = d.call("auto.quota.state", json!({"profile_id":"system-codex",
        "harness":"codex-app", "model":"gpt-6-sol"}));
    let model_ms = models["catalog"]["observed_ms"].as_i64().unwrap();
    let quota_ms = quota["observation"]["snapshot"]["observed_ms"].as_i64().unwrap();
    assert!(model_ms - quota_ms >= 1_000,
        "the earlier quota response must not inherit a delayed catalog's freshness: models={models}, quota={quota}");
}

#[test]
fn auto_codex_tool_inventory_is_project_scoped_bounded_and_content_free() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let trace = r.path().join("tool-inventory-trace.txt");
    let d = Daemon::start(&[("OVERSEER_CODEX_PATH", &fixture("fake-harness/codex-app-fixture.js")),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE,FIXTURE_TRACE_FILE"),
        ("FIXTURE_MODE", "metadata-models"), ("FIXTURE_TRACE_FILE", trace.to_str().unwrap())]);
    let workspace = ws_id(&sh(&d, &repo, "worktree", "true"));
    let inspected = d.call("auto.tools.inspect", json!({"profile_id":"system-codex","workspace_id":workspace}));
    assert_eq!(inspected["catalog"]["tools"], json!(["browser/navigate","browser/snapshot"]), "{inspected}");
    assert_eq!(inspected["source"], "codex-app/mcpServerStatus-list");
    assert!(!inspected.to_string().contains("secret-tool-sentinel"));
    let trace = std::fs::read_to_string(trace).unwrap();
    assert_eq!(trace.lines().filter(|line| *line == "tool_read").count(), 2);
    assert!(trace.contains(&format!("tool_cwd:{}", d.call("state", json!({}))["workspaces"].as_array().unwrap()
        .iter().find(|item| item["id"] == workspace).unwrap()["path"].as_str().unwrap())));
    assert_eq!(d.runs().len(), 1, "tool inventory may not start a model run");
}

#[test]
fn auto_failed_account_read_does_not_leave_fresh_model_or_allowance_evidence() {
    let r = tmp();
    let auth_file = r.path().join("auth-state.txt");
    std::fs::write(&auth_file, "chatgpt").unwrap();
    let d = Daemon::start(&[("OVERSEER_CODEX_PATH", &fixture("fake-harness/codex-app-fixture.js")),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE,FIXTURE_AUTH_FILE"),
        ("FIXTURE_MODE", "metadata-models"), ("FIXTURE_AUTH_FILE", auth_file.to_str().unwrap())]);
    d.call("auto.models.refresh", json!({"profile_id":"system-codex"}));
    assert_eq!(d.call("auto.models.list", json!({"profile_id":"system-codex"}))["fresh"], true);
    assert_eq!(d.call("auto.quota.list", json!({}))["observations"].as_array().unwrap().len(), 1);
    std::fs::write(&auth_file, "key").unwrap();
    assert!(d.try_call("auto.models.refresh", json!({"profile_id":"system-codex"})).is_err());
    let catalog = d.call("auto.models.list", json!({"profile_id":"system-codex"}));
    assert_eq!(catalog["fresh"], false, "{catalog}");
    assert!(catalog["catalog"].is_null(), "{catalog}");
    assert!(d.call("auto.quota.list", json!({}))["observations"].as_array().unwrap().is_empty());
}

#[test]
fn auto_codex_metadata_read_and_new_run_do_not_overlap_same_profile() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let trace = r.path().join("metadata-trace.txt");
    let d = Daemon::start(&[("OVERSEER_CODEX_PATH", &fixture("fake-harness/codex-app-fixture.js")),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE,FIXTURE_TRACE_FILE"),
        ("FIXTURE_MODE", "metadata-delay"), ("FIXTURE_TRACE_FILE", trace.to_str().unwrap())]);
    std::thread::scope(|scope| {
        let refresh = scope.spawn(|| d.call("auto.quota.refresh", json!({"profile_id":"system-codex"})));
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !std::fs::read_to_string(&trace).unwrap_or_default().contains("metadata_started") {
            assert!(std::time::Instant::now() < deadline, "metadata probe did not start");
            std::thread::sleep(Duration::from_millis(10));
        }
        let created = d.call("task.create", json!({"repo":repo,"harness":"codex-app","prompt":"x","title":"after metadata"}));
        assert!(created["run"]["id"].is_string());
        refresh.join().unwrap();
    });
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        let lines = std::fs::read_to_string(&trace).unwrap_or_default();
        if lines.contains("thread_started") {
            assert!(lines.find("metadata_done").unwrap() < lines.find("thread_started").unwrap(), "{lines}");
            break;
        }
        assert!(std::time::Instant::now() < deadline, "run did not launch after metadata probe");
        std::thread::sleep(Duration::from_millis(10));
    }
}
