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
