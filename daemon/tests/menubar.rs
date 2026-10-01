//! AC-262: what the Mac's menu-bar item reads (`menubar.snapshot`) and sends back
//! (`run.permission` with Always allow, `review.seen`), against the Claude fixture's menubar mode.
mod common;
use common::*;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

fn menubar_daemon() -> Daemon {
    let fixture = repo_root().join("fixtures/fake-harness/claude-fixture.js").display().to_string();
    Daemon::start(&[("OVERSEER_CLAUDE_PATH", &fixture), ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE"), ("FIXTURE_MODE", "menubar")])
}

fn wait_for(d: &Daemon, what: &str, secs: u64, pred: impl Fn(&Value) -> bool) -> Value {
    let until = std::time::Instant::now() + Duration::from_secs(secs);
    loop {
        let snap = d.call("menubar.snapshot", json!({}));
        if pred(&snap) { return snap; }
        assert!(std::time::Instant::now() < until, "{what}: {snap:#}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn ac262_snapshot_answers_and_review_marks() {
    let r = tmp();
    let site = repo(&r.path().join("site"));
    let notes = repo(&r.path().join("notes"));
    let d = menubar_daemon();
    let ask = |repo: &std::path::Path, title: &str, prompt: &str| run_id(&d.call("task.create", json!({"repo": repo, "harness": "claude", "prompt": prompt, "title": title})));
    let quick = ask(&notes, "Weekly notes digest", "write the digest");
    d.wait_done(&quick, 20);
    let busy = ask(&notes, "Tag search", "busy: 60");
    let test = ask(&site, "Checkout page redesign", "ask: npm test");
    d.wait_status(&test, |s| s == "waiting_for_user", 20);
    let push = ask(&site, "Blog RSS feed", "ask: git push origin rss");
    d.wait_status(&push, |s| s == "waiting_for_user", 20);
    d.wait_status(&busy, |s| s == "running", 20);

    let snap = wait_for(&d, "two waiting", 10, |s| s["waiting"].as_array().map_or(0, |w| w.len()) == 2);
    assert_eq!(snap["dev"], false);
    assert_eq!(snap["summary"], "3 working · 1 to review", "{snap:#}");
    // Needs you: newest first, as questions, with the harness's session rule to offer.
    assert_eq!(snap["waiting"][0]["title"], "Blog RSS feed");
    assert_eq!(snap["waiting"][0]["question"], "Run git push origin rss?");
    assert_eq!(snap["waiting"][1]["question"], "Run npm test?");
    assert_eq!(snap["waiting"][1]["always"], "Bash(npm test:*) · this session");
    assert_eq!(snap["waiting"][1]["repo"], "site");
    // Repositories: most recent first, a waiting one marked.
    let repos: Vec<(String, u64, bool)> = snap["repos"].as_array().unwrap().iter().map(|r| (r["name"].as_str().unwrap().to_string(), r["count"].as_u64().unwrap(), r["waiting"].as_bool().unwrap())).collect();
    assert_eq!(repos, [("site".to_string(), 2, true), ("notes".to_string(), 2, false)]);
    let notes_agents = &snap["repos"][1]["agents"];
    assert_eq!(notes_agents[0]["title"], "Tag search");
    assert!(notes_agents[0]["status"].as_str().unwrap().starts_with("Working · "), "{notes_agents}");
    assert_eq!(notes_agents[1]["status"], "To review");

    // Always allow: Claude Code gets the rule back as updatedPermissions.
    let request = snap["waiting"][1]["request_id"].as_str().unwrap().to_string();
    d.call("run.permission", json!({"run_id": test, "request_id": request, "allow": true, "always": true}));
    assert_eq!(d.wait_done(&test, 20)["status"], "completed");
    let answer = answer_of(&d, &test);
    assert_eq!(answer["behavior"], "allow");
    assert_eq!(answer["updatedPermissions"][0]["rules"][0]["ruleContent"], "npm test:*", "{answer}");
    let answered = d.events(&test).into_iter().find(|e| e["kind"] == "permission_answered").unwrap();
    assert_eq!(answered["payload"]["always"], "Bash(npm test:*) · this session");

    // Deny: the plain answer, nothing remembered.
    let request = snap["waiting"][0]["request_id"].as_str().unwrap().to_string();
    d.call("run.permission", json!({"run_id": push, "request_id": request, "allow": false}));
    d.wait_done(&push, 20);
    let answer = answer_of(&d, &push);
    assert_eq!(answer["behavior"], "deny");
    assert!(answer.get("updatedPermissions").is_none());

    // Looked at in VS Code: no longer to review.
    let snap = wait_for(&d, "nothing waiting", 10, |s| s["waiting"].as_array().is_some_and(|w| w.is_empty()));
    assert_eq!(snap["summary"], "1 working · 3 to review", "{snap:#}");
    let ended = d.run(&test)["ended_ms"].as_i64().unwrap();
    d.call("review.seen", json!({"marks": {test.clone(): ended}}));
    assert_eq!(d.call("menubar.snapshot", json!({}))["summary"], "1 working · 2 to review · 1 idle");

    // The item is not a VS Code window: it never counts as one.
    let mut conn = UnixStream::connect(d.socket()).unwrap();
    conn.write_all(format!("{}\n", json!({"id": 1, "method": "hello", "params": {"client": "menubar"}})).as_bytes()).unwrap();
    let mut line = String::new();
    BufReader::new(conn.try_clone().unwrap()).read_line(&mut line).unwrap();
    assert!(line.contains("\"protocol\""), "{line}");
    assert_eq!(d.call("daemon.clients", json!({}))["ui"], 0);
    drop(conn);
    d.call("run.interrupt", json!({"run_id": busy}));
    d.wait_done(&busy, 20);
}

/// The answer the fixture wrote in its worktree.
fn answer_of(d: &Daemon, run: &str) -> Value {
    let task = d.run(run)["task_id"].as_str().unwrap().to_string();
    let state = d.call("state", json!({}));
    let task = state["tasks"].as_array().unwrap().iter().find(|t| t["id"] == task.as_str()).unwrap().clone();
    let ws = state["workspaces"].as_array().unwrap().iter().find(|w| w["id"] == task["workspace_id"]).unwrap().clone();
    let path = std::path::PathBuf::from(ws["path"].as_str().unwrap()).join("answer.json");
    serde_json::from_str(&std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))).unwrap()
}

#[test]
fn ac262_always_allow_is_refused_when_nothing_is_offered() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let fixture = repo_root().join("fixtures/fake-harness/claude-fixture.js").display().to_string();
    let d = Daemon::start(&[("OVERSEER_CLAUDE_PATH", &fixture), ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE"), ("FIXTURE_MODE", "permission")]);
    let run = run_id(&d.call("task.create", json!({"repo": repo, "harness": "claude", "prompt": "write perm.txt", "title": "perm"})));
    let waiting = d.wait_status(&run, |s| s == "waiting_for_user", 15);
    let request = waiting["attention"]["request_id"].as_str().unwrap().to_string();
    assert!(waiting["attention"]["always"].is_null());
    let err = d.try_call("run.permission", json!({"run_id": run, "request_id": request, "allow": true, "always": true})).unwrap_err();
    assert!(err.contains("offers no Always allow"), "{err}");
    assert_eq!(d.run(&run)["status"], "waiting_for_user", "a refused Always allow answers nothing");
    d.call("run.permission", json!({"run_id": run, "request_id": request, "allow": true}));
    assert_eq!(d.wait_done(&run, 15)["status"], "completed");
}
