//! The owner's first Voice Mode session (2026-09-28), typed half: Overseer moves the owner around
//! VS Code with Look actions that need no yes (AC-226); "handle what needs me" and "tell it yes"
//! answer what waits, with no model turn (AC-227); a proposal that can no longer be done is closed
//! the moment its agent changes, so Needs you clears (AC-228). A real daemon with the Claude
//! fixture as Overseer's model and as the agents; no paid turn.
mod common;
use common::*;
use serde_json::{json, Value};
use std::path::Path;
use std::time::{Duration, Instant};

fn claude_fixture() -> String {
    repo_root().join("fixtures/fake-harness/claude-fixture.js").display().to_string()
}

/// A daemon whose Claude is the fixture, the mode chosen per task through a mode file.
fn fixture_daemon(mode_file: &Path) -> Daemon {
    std::fs::write(mode_file, "overseer").unwrap();
    Daemon::start(&[
        ("OVERSEER_CLAUDE_PATH", &claude_fixture()),
        ("CLAUDE_FIXTURE_MODE_FILE", &mode_file.display().to_string()),
        ("FIXTURE_WORKER_MS", "800"),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "CLAUDE_FIXTURE_MODE_FILE,FIXTURE_WORKER_MS"),
    ])
}

fn agent(d: &Daemon, repo: &Path, mode_file: &Path, mode: &str, title: &str) -> String {
    std::fs::write(mode_file, mode).unwrap();
    let id = run_id(&d.call("task.create", json!({"repo": repo, "harness": "claude", "prompt": "do the work", "title": title})));
    // The mode is read when the harness starts; wait for that before the next task changes it.
    d.wait_status(&id, |s| s != "queued" && s != "starting", 20);
    std::thread::sleep(Duration::from_millis(300));
    std::fs::write(mode_file, "overseer").unwrap();
    id
}

fn session(d: &Daemon) -> Value {
    d.call("overseer.session", json!({}))
}

fn wait_idle(d: &Daemon, secs: u64) -> Value {
    let deadline = Instant::now() + Duration::from_secs(secs);
    loop {
        let s = session(d);
        let status = s["run_status"].as_str().unwrap_or("");
        if !s["run_id"].is_null() && !["queued", "starting", "running", "waiting_for_user"].contains(&status) {
            return s;
        }
        assert!(Instant::now() < deadline, "Overseer's turn did not end: {s}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn say(d: &Daemon, text: &str) -> Value {
    d.call("overseer.send", json!({"text": text, "surface": "vscode", "harness": "claude"}))
}

/// The Look actions the daemon asked VS Code to show, on that agent.
fn looks(d: &Daemon, run: &str) -> Vec<Value> {
    d.events(run).into_iter().filter(|e| e["kind"] == "overseer_action" && ["focus", "show_work", "open_file", "open_review", "open_worktree"].contains(&e["payload"]["action"].as_str().unwrap_or(""))).map(|e| e["payload"].clone()).collect()
}

fn wait_looks(d: &Daemon, run: &str, n: usize, secs: u64) -> Vec<Value> {
    let deadline = Instant::now() + Duration::from_secs(secs);
    loop {
        let l = looks(d, run);
        if l.len() >= n || Instant::now() > deadline {
            return l;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// AC-226: typed "show me the draft agent", "what did it make?" and "open the file it made" each
/// become a Look action on the right agent, at once at the Ask first level (no proposal waits),
/// and the file is an absolute path inside the agent's worktree (never a relative one).
#[test]
fn ac226_overseer_shows_the_agent_its_work_and_the_file_it_made() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let mode_file = r.path().join("mode");
    let d = fixture_daemon(&mode_file);
    let other = agent(&d, &repo, &mode_file, "worker", "Notes helper");
    d.wait_done(&other, 30);
    let draft = agent(&d, &repo, &mode_file, "worker", "Draft agent");
    d.wait_done(&draft, 30);
    assert_eq!(session(&d)["level"], "ask_first");

    say(&d, "Show me the draft agent.");
    wait_idle(&d, 30);
    let l = wait_looks(&d, &draft, 1, 10);
    assert_eq!(l.len(), 1, "one Look action on the draft agent: {l:?}");
    assert_eq!(l[0]["action"], "focus");
    assert!(looks(&d, &other).is_empty(), "nothing shown of the other agent");
    let s = session(&d);
    assert!(s["proposals"].as_array().unwrap().is_empty(), "nothing waits for a yes: {}", s["proposals"]);
    let card = s["cards"].as_array().unwrap().last().unwrap().clone();
    assert_eq!(card["state"], "yes", "done at once by the level's own answer: {card}");

    say(&d, "What did it make?");
    wait_idle(&d, 30);
    let l = wait_looks(&d, &draft, 2, 10);
    assert_eq!(l[1]["action"], "show_work", "{l:?}");
    assert_eq!(l[1]["files"], 1, "its finished work: one file changed: {l:?}");

    say(&d, "Open the file it made.");
    wait_idle(&d, 30);
    let l = wait_looks(&d, &draft, 3, 10);
    assert_eq!(l[2]["action"], "open_file", "{l:?}");
    let path = l[2]["path"].as_str().unwrap();
    let worktree = std::fs::canonicalize(l[2]["worktree"].as_str().unwrap()).unwrap();
    assert!(Path::new(path).is_absolute() && path.ends_with("/draft.md") && Path::new(path).starts_with(&worktree), "an absolute path inside its worktree: {path} in {}", worktree.display());
    assert!(session(&d)["proposals"].as_array().unwrap().is_empty());
    // The Look actions are Look: they reach no method and change nothing about the agent.
    assert_eq!(d.run(&draft)["status"], "completed");
}

/// AC-227: "handle what needs me" asks the one question the waiting permission needs, and the
/// owner's "yes" answers it; "tell it yes" answers the next one at once. No model turn is used.
#[test]
fn ac227_handle_what_needs_me_answers_a_waiting_permission() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let mode_file = r.path().join("mode");
    let d = fixture_daemon(&mode_file);
    // Nothing waits: said so.
    let nothing = say(&d, "Handle what needs me.");
    assert_eq!(nothing["handled"], true, "{nothing}");
    assert_eq!(nothing["reply"], "Nothing needs you right now.");

    let asks = agent(&d, &repo, &mode_file, "permission", "Sessions");
    d.wait_status(&asks, |s| s == "waiting_for_user", 20);
    let handled = say(&d, "Handle what needs me.");
    assert_eq!(handled["handled"], true, "{handled}");
    let reply = handled["reply"].as_str().unwrap();
    assert!(reply.starts_with("Sessions wants to change perm.txt") && reply.ends_with("Allow it?"), "the one question it needs: {reply}");
    let s = session(&d);
    let open = s["proposals"].as_array().unwrap();
    assert_eq!(open.len(), 1, "the question waits for a yes: {s}");
    assert_eq!(open[0]["actions"][0]["action"], "permission");
    assert_eq!(d.run(&asks)["status"], "waiting_for_user", "nothing answered before the yes");

    let yes = say(&d, "yes");
    assert_eq!(yes["handled"], true, "{yes}");
    let run = d.wait_status(&asks, |s| s != "waiting_for_user", 5);
    assert_ne!(run["status"], "waiting_for_user");
    d.wait_done(&asks, 20);
    assert_eq!(d.run(&asks)["status"], "completed");

    let second = agent(&d, &repo, &mode_file, "permission", "Writer");
    d.wait_status(&second, |s| s == "waiting_for_user", 20);
    let told = say(&d, "Tell it yes.");
    assert_eq!(told["handled"], true, "{told}");
    assert!(told["reply"].as_str().unwrap().starts_with("Allowed Writer"), "{told}");
    d.wait_done(&second, 20);
    assert_eq!(d.run(&second)["status"], "completed");
    // All of it by the daemon: Overseer's model never ran.
    assert!(session(&d)["run_id"].is_null(), "no model turn for what needs you");
    let texts: Vec<String> = session(&d)["messages"].as_array().unwrap().iter().map(|m| m["text"].as_str().unwrap_or("").to_string()).collect();
    assert!(texts.iter().any(|t| t == "Handle what needs me.") && texts.iter().any(|t| t == "Tell it yes."), "the same conversation: {texts:?}");
}

/// AC-228: an open proposal about a permission is closed the moment that request is answered
/// elsewhere, and one about an agent that has since finished is closed when it finishes, so
/// Overseer's part of Needs you clears within a second.
#[test]
fn ac228_needs_you_clears_when_the_agent_no_longer_needs_the_owner() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let mode_file = r.path().join("mode");
    let d = fixture_daemon(&mode_file);
    let asks = agent(&d, &repo, &mode_file, "permission", "Sessions");
    d.wait_status(&asks, |s| s == "waiting_for_user", 20);
    let request = d.run(&asks)["attention"]["request_id"].as_str().unwrap().to_string();
    d.call("overseer.session", json!({}));
    let p = d.call("overseer.propose", json!({"actions": [{"action": "permission", "agent": asks, "allow_request": true}], "source": "test"}));
    assert_eq!(p["state"], "open", "{p}");
    let open = |d: &Daemon| d.call("state", json!({}))["overseer"]["open_proposals"].as_i64().unwrap_or(-1);
    assert_eq!(open(&d), 1);
    // The owner answers it in the agent's chat instead.
    let t0 = Instant::now();
    d.call("run.permission", json!({"run_id": asks, "request_id": request, "allow": true}));
    let answered = t0.elapsed();
    let deadline = Instant::now() + Duration::from_secs(1);
    while open(&d) != 0 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    let cleared = t0.elapsed();
    assert_eq!(open(&d), 0, "the proposal no one can carry out is closed");
    assert!(cleared <= Duration::from_millis(1000) + answered, "within a second: {cleared:?}");
    let card = d.call("overseer.card", json!({"id": p["proposal"]}));
    assert_eq!(card["state"], "stale", "{card}");
    assert!(card["result"].as_str().unwrap().starts_with("Not needed any more"), "{card}");
    assert_ne!(d.run(&asks)["status"], "waiting_for_user", "the agent itself no longer waits");

    // A message proposed while an agent ran is closed when the agent finishes.
    let (runner, _) = {
        let created = d.call("task.create", json!({"repo": repo, "harness": "generic", "workspace_mode": "worktree", "program": "/bin/sh", "args": ["-c", "sleep 2"], "prompt": "", "title": "Runner"}));
        let id = run_id(&created);
        d.wait_status(&id, |s| s == "running", 20);
        (id, ())
    };
    let p = d.call("overseer.propose", json!({"actions": [{"action": "redirect", "agent": runner, "text": "Change course"}], "source": "test"}));
    assert_eq!(p["state"], "open");
    d.wait_done(&runner, 20);
    let t0 = Instant::now();
    while open(&d) != 0 && t0.elapsed() < Duration::from_secs(1) {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(open(&d), 0, "closed when the agent finished");
}
