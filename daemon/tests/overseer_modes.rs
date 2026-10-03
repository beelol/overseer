//! Permission modes by conversation (AC-230): typed conversations with Overseer (the Claude
//! fixture as its model) set each mode on a running agent and start one in Auto; Overseer setting
//! Auto by itself happens only within its level and the repositories the owner allows, and is
//! recorded with its reason; a waiting permission comes up by itself as a yes/no.
mod common;
use common::*;
use serde_json::{json, Value};
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

fn claude_fixture() -> String {
    repo_root().join("fixtures/fake-harness/claude-fixture.js").display().to_string()
}

/// A daemon whose Claude is the fixture: Overseer's run is always Overseer; each agent takes the
/// mode in the mode file when it starts. What each agent's process gets on its stdin is logged.
fn modes_daemon(mode_file: &Path, stdin_logs: &Path) -> Daemon {
    std::fs::write(mode_file, "overseer").unwrap();
    let (fixture, mode, logs) = (claude_fixture(), mode_file.display().to_string(), stdin_logs.display().to_string());
    let d = Daemon::start(&[
        ("OVERSEER_CLAUDE_PATH", &fixture),
        ("CLAUDE_FIXTURE_MODE_FILE", &mode),
        ("FIXTURE_STDIN_LOG_DIR", &logs),
        ("FIXTURE_SLOW_MS", "120000"),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "CLAUDE_FIXTURE_MODE_FILE,FIXTURE_STDIN_LOG_DIR,FIXTURE_SLOW_MS"),
        ("OVERSEER_TEST_AUTO_DISABLED", "1"),
    ]);
    // No check-ins: every Overseer turn here is one the test asked for.
    d.call("agent.cadence", json!({"cadence": "off", "by": "owner"}));
    d
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

/// Types to Overseer and returns the proposal its answer made (Ask first: it waits for a yes).
fn ask(d: &Daemon, text: &str) -> Value {
    let before = session(d)["proposals"].as_array().map(|p| p.len()).unwrap_or(0);
    d.call("overseer.send", json!({"text": text, "surface": "ctl", "harness": "claude"}));
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        std::thread::sleep(Duration::from_millis(200));
        let s = wait_overseer_idle(d, 60);
        let list = s["proposals"].as_array().cloned().unwrap_or_default();
        if list.len() > before {
            return list.last().cloned().unwrap();
        }
        assert!(Instant::now() < deadline, "no proposal for {text:?}: {}", s["messages"]);
    }
}

fn yes(d: &Daemon, p: &Value) -> Value {
    let a = d.call("overseer.answer", json!({"id": p["id"], "yes": true, "surface": "ctl", "by": "owner"}));
    assert_eq!(a["state"], "yes", "{a}");
    a
}

fn mode_events(d: &Daemon, run: &str) -> Vec<Value> {
    d.events(run).into_iter().filter(|e| e["kind"] == "overseer_action" && e["payload"]["action"] == "mode").map(|e| e["payload"].clone()).collect()
}

fn sql(d: &Daemon, statement: &str) {
    let db = d.home.path().join("overseer.sqlite");
    let out = Command::new("sqlite3").args(["-cmd", ".timeout 5000"]).arg(&db).arg(statement).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
}

/// What an echoing turn of the fixture reported: its arguments.
fn echoed_argv(d: &Daemon, run: &str) -> String {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Some(t) = d.events(run).iter().rev().find_map(|e| e["payload"]["text"].as_str().filter(|t| t.starts_with("ECHO ")).map(str::to_string)) {
            return t;
        }
        assert!(Instant::now() < deadline, "no echo from {run}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// AC-230: typed, each mode is set on a running agent (now, through its running turn, and for its
/// next turn), and an agent is started in Auto.
#[test]
fn ac230_typed_conversations_set_each_mode_and_start_one_in_auto() {
    let r = tmp();
    let site = repo(&r.path().join("site"));
    let mode_file = r.path().join("mode");
    let logs = r.path().join("stdin");
    std::fs::create_dir_all(&logs).unwrap();
    let d = modes_daemon(&mode_file, &logs);
    std::fs::write(&mode_file, "slow").unwrap();
    let created = d.call("task.create", json!({"repo": site, "harness": "claude", "prompt": "work on the site", "title": "Site"}));
    let agent = run_id(&created);
    d.wait_status(&agent, |s| s == "running", 30);
    let worktree = ws_path(&d, &created);
    let stdin_log = || std::fs::read_to_string(logs.join(format!("{}.log", worktree.file_name().unwrap().to_string_lossy()))).unwrap_or_default();
    for (words, mode, label) in [("Set Site to Accept edits.", "acceptEdits", "Accept edits"), ("Put Site in Ask first.", "manual", "Ask first"), ("Switch Site to Auto.", "auto", "Auto")] {
        let p = ask(&d, words);
        assert_eq!(p["actions"][0]["action"], "mode", "{p}");
        assert_eq!(p["lines"][0], format!("Set Site to {label}"), "{p}");
        let a = yes(&d, &p);
        assert!(a["result"].as_str().unwrap().contains(&format!("set Site to {label}")), "{a}");
        let set = mode_events(&d, &agent);
        let last = set.last().unwrap();
        assert_eq!(last["mode"], mode);
        assert_eq!(last["live"], true, "a running Claude turn takes it at once: {last}");
        assert_eq!(last["cause"], "owner");
        // The running turn got Claude's own control request for it.
        let sent = stdin_log();
        assert!(sent.lines().any(|l| l.contains("\"subtype\":\"set_permission_mode\"") && l.contains(&format!("\"mode\":\"{mode}\""))), "{sent}");
        let card = d.call("overseer.card", json!({"id": p["id"]}));
        assert_eq!(card["rows"][0]["delivery"], "mode", "{card}");
        assert_eq!(card["rows"][0]["message"], label, "{card}");
    }
    // Its next turn starts in the last mode set.
    d.call("run.interrupt", json!({"run_id": agent}));
    d.wait_status(&agent, |s| s == "interrupted", 30);
    std::fs::write(&mode_file, "echo").unwrap();
    d.call("run.follow_up", json!({"run_id": agent, "prompt": "go on"}));
    let argv = echoed_argv(&d, &agent);
    assert!(argv.contains("\"--permission-mode\",\"auto\""), "{argv}");
    // A mode its harness does not take is refused, with the reason.
    let generic = run_id(&d.call("task.create", json!({"repo": site, "harness": "generic", "workspace_mode": "worktree", "program": "/bin/sleep", "args": ["120"], "prompt": "", "title": "Plain"})));
    let refused = d.try_call("overseer.propose", json!({"actions": [{"action": "mode", "agent": generic, "mode": "Auto"}], "source": "ctl"})).unwrap_err();
    assert!(refused.contains("does not take Auto"), "{refused}");
    let refused = d.try_call("overseer.propose", json!({"actions": [{"action": "mode", "agent": agent, "mode": "bypassPermissions"}], "source": "ctl"})).unwrap_err();
    assert!(refused.contains("not a mode Overseer sets"), "{refused}");
    let _ = d.try_call("run.interrupt", json!({"run_id": generic}));
    // Started in Auto, typed.
    let p = ask(&d, "Start an agent in the site repo in Auto to write the release notes.");
    assert_eq!(p["actions"][0]["action"], "start", "{p}");
    assert_eq!(p["actions"][0]["permission_mode"], "auto", "{p}");
    let before: Vec<String> = d.runs().iter().map(|r| r["id"].as_str().unwrap().to_string()).collect();
    yes(&d, &p);
    let deadline = Instant::now() + Duration::from_secs(20);
    let new = loop {
        if let Some(r) = d.runs().into_iter().find(|r| !before.contains(&r["id"].as_str().unwrap().to_string())) {
            break r["id"].as_str().unwrap().to_string();
        }
        assert!(Instant::now() < deadline, "no new agent");
        std::thread::sleep(Duration::from_millis(100));
    };
    let argv = echoed_argv(&d, &new);
    assert!(argv.contains("\"--permission-mode\",\"auto\""), "started in Auto: {argv}");
}

/// AC-230: Overseer sets Auto by itself only within its level and the repositories the owner
/// allows, and each time the reason is recorded on the agent and its card.
#[test]
fn ac230_overseer_sets_auto_by_itself_only_where_allowed_and_says_why() {
    let r = tmp();
    let site = repo(&r.path().join("site"));
    let notes = repo(&r.path().join("notes"));
    let mode_file = r.path().join("mode");
    let logs = r.path().join("stdin");
    std::fs::create_dir_all(&logs).unwrap();
    let d = modes_daemon(&mode_file, &logs);
    std::fs::write(&mode_file, "slow").unwrap();
    let in_site = run_id(&d.call("task.create", json!({"repo": site, "harness": "claude", "prompt": "work", "title": "Site"})));
    let in_notes = run_id(&d.call("task.create", json!({"repo": notes, "harness": "claude", "prompt": "work", "title": "Notes"})));
    d.wait_status(&in_site, |s| s == "running", 30);
    d.wait_status(&in_notes, |s| s == "running", 30);
    session(&d);
    d.call("overseer.level", json!({"level": "auto"}));
    sql(&d, "UPDATE overseer_sessions SET last_cause='check_in';");
    let auto = |agent: &str, why: &str| d.try_call("overseer.propose", json!({"actions": [{"action": "mode", "agent": agent, "mode": "Auto", "why": why}], "source": "check_in"}));
    // No repository allowed yet: refused.
    let refused = auto(&in_site, "its edits are all inside its area").unwrap_err();
    assert!(refused.contains("only in the repositories the owner allows") && refused.contains("site"), "{refused}");
    // The owner allows the site repository (from the Mac).
    let set = d.call("overseer.auto_repos", json!({"repos": [site]}));
    assert_eq!(set["repos"].as_array().unwrap().len(), 1, "{set}");
    assert_eq!(d.call("overseer.auto_repos", json!({}))["repos"], set["repos"]);
    // With no reason: refused.
    let refused = auto(&in_site, "").unwrap_err();
    assert!(refused.contains("say why"), "{refused}");
    // In another repository: still refused.
    let refused = auto(&in_notes, "it asks for every edit").unwrap_err();
    assert!(refused.contains("notes is not one"), "{refused}");
    // Even an allowed repository at Auto waits for the owner's explicit yes.
    let done = auto(&in_site, "it has asked for 12 edits inside its own area").unwrap();
    assert_eq!(done["done"], false, "{done}");
    assert_eq!(done["state"], "open", "{done}");
    assert!(mode_events(&d, &in_site).is_empty(), "no mode change before a yes");
    std::thread::sleep(Duration::from_millis(2300));
    assert!(mode_events(&d, &in_site).is_empty(), "Auto must not send after the settle window");
    let proposal = session(&d)["proposals"].as_array().unwrap().iter()
        .find(|p| p["id"] == done["proposal"]).unwrap().clone();
    yes(&d, &proposal);
    let ev = mode_events(&d, &in_site);
    let last = ev.last().unwrap();
    assert_eq!(last["mode"], "auto");
    assert_eq!(last["why"], "it has asked for 12 edits inside its own area");
    assert_eq!(last["cause"], "check_in");
    let card = d.call("overseer.card", json!({"id": done["proposal"]}));
    assert_eq!(card["rows"][0]["why"], "it has asked for 12 edits inside its own area", "{card}");
    // Refusing the suggestion leaves the mode unchanged at every level.
    for level in ["auto", "steer", "ask_first"] {
        d.call("overseer.level", json!({"level": level}));
        let p = auto(&in_site, "it keeps asking for edits").unwrap();
        assert_eq!(p["state"], "open", "{level}: {p}");
        assert_eq!(p["done"], false, "{level}: {p}");
        let before = mode_events(&d, &in_site).len();
        d.call("overseer.answer", json!({"id": p["proposal"], "yes": false, "surface": "ctl", "by": "owner"}));
        assert_eq!(mode_events(&d, &in_site).len(), before, "{level}: no change after no");
    }
    // Starting a new agent in Auto has the same confirmation boundary.
    d.call("overseer.level", json!({"level": "auto"}));
    let before = d.runs().len();
    let start = d.call("overseer.propose", json!({"actions": [{
        "action": "start", "repo": site, "prompt": "write the notes", "title": "New notes",
        "harness": "claude", "permission_mode": "Auto", "why": "the work stays in its area"
    }], "source": "check_in"}));
    assert_eq!(start["state"], "open", "{start}");
    assert_eq!(start["done"], false, "{start}");
    assert_eq!(d.runs().len(), before, "no new Auto agent before a yes");
    d.call("overseer.answer", json!({"id": start["proposal"], "yes": false, "surface": "ctl", "by": "owner"}));
    assert_eq!(d.runs().len(), before, "no new Auto agent after no");
    // A turn triggered by the owner is not evidence that they asked for Auto.
    // Both typed and spoken requests still need a separate yes, at every level.
    for cause in ["owner", "voice"] {
        sql(&d, &format!("UPDATE overseer_sessions SET last_cause='{cause}';"));
        for level in ["auto", "steer", "ask_first"] {
            d.call("overseer.level", json!({"level": level}));
            for action in [
                json!({"action": "mode", "agent": in_notes, "mode": "Auto", "class": "steer"}),
                json!({"action": "start", "repo": notes, "prompt": "write notes", "title": "Notes", "harness": "claude", "permission_mode": "Auto", "class": "steer"}),
            ] {
                let p = d.call("overseer.propose", json!({"actions": [action], "source": cause}));
                assert_eq!(p["state"], "open", "{cause} at {level}: {p}");
                assert_eq!(p["done"], false, "{cause} at {level}: {p}");
                d.call("overseer.answer", json!({"id": p["proposal"], "yes": false, "surface": "ctl", "by": "owner"}));
            }
        }
    }
    for run in [&in_site, &in_notes] {
        let _ = d.try_call("run.interrupt", json!({"run_id": run}));
    }
}

/// AC-230: an agent's waiting permission comes up by itself in the conversation as the yes/no it
/// needs, without the owner asking; a click answers it, and Needs you counts it once.
#[test]
fn ac230_a_waiting_permission_comes_up_by_itself_and_a_click_answers_it() {
    let r = tmp();
    let site = repo(&r.path().join("site"));
    let mode_file = r.path().join("mode");
    let logs = r.path().join("stdin");
    std::fs::create_dir_all(&logs).unwrap();
    let d = modes_daemon(&mode_file, &logs);
    session(&d);
    std::fs::write(&mode_file, "permission").unwrap();
    let asks = run_id(&d.call("task.create", json!({"repo": site, "harness": "claude", "prompt": "write perm.txt", "title": "Sessions"})));
    d.wait_status(&asks, |s| s == "waiting_for_user", 30);
    std::fs::write(&mode_file, "overseer").unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let (card, proposal) = loop {
        let s = session(&d);
        let card = s["messages"].as_array().unwrap().iter().find(|m| m["card"]["kind"] == "needs" && m["card"]["agent"] == asks.as_str()).cloned();
        if let Some(c) = card {
            let id = c["card"]["proposal"].as_str().unwrap().to_string();
            let p = s["proposals"].as_array().unwrap().iter().find(|p| p["id"] == id.as_str()).cloned().unwrap();
            break (c, p);
        }
        assert!(Instant::now() < deadline, "the permission did not come up by itself: {}", s["messages"]);
        std::thread::sleep(Duration::from_millis(100));
    };
    assert_eq!(card["text"], "Sessions wants to change perm.txt. Allow it?", "{card}");
    assert_eq!(card["card"]["state"], "asked");
    assert_eq!(proposal["state"], "open", "a yes/no waiting for the owner: {proposal}");
    assert_eq!(proposal["actions"][0]["action"], "permission");
    // Counted once in Needs you: as the agent's own item, not again as Overseer's proposal.
    assert_eq!(d.call("state", json!({}))["overseer"]["open_proposals"], 0);
    // Nothing was asked of Overseer's model for it.
    assert!(session(&d)["messages"].as_array().unwrap().iter().all(|m| m["source"] != "owner"));
    // A click on Yes answers it.
    let a = d.call("overseer.answer", json!({"id": proposal["id"], "yes": true, "surface": "vscode", "by": "owner"}));
    assert_eq!(a["state"], "yes", "{a}");
    d.wait_done(&asks, 30);
    assert_eq!(d.run(&asks)["status"], "completed");
    // Once per request: no second card for it.
    let n = session(&d)["messages"].as_array().unwrap().iter().filter(|m| m["card"]["kind"] == "needs" && m["card"]["agent"] == asks.as_str()).count();
    assert_eq!(n, 1);
}
