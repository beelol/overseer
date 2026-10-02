//! AC-241: a waiting agent can always be answered. Deny with a note sends the note to the harness
//! as the reason (the fixture's stdin log); Allow for this session is not asked again for the
//! same tool, even by a harness that asks again; and Overseer's message to an agent blocked on a
//! permission says so ("blocked on your permission") instead of reporting it sent. A real daemon
//! with the Claude fixture; no paid turn.
mod common;
use common::*;
use serde_json::{json, Value};
use std::path::Path;
use std::time::Duration;

fn claude_fixture() -> String {
    repo_root().join("fixtures/fake-harness/claude-fixture.js").display().to_string()
}

fn daemon(mode_file: &Path, stdin_log: &Path) -> Daemon {
    std::fs::write(mode_file, "overseer").unwrap();
    std::fs::create_dir_all(stdin_log).unwrap();
    Daemon::start(&[
        ("OVERSEER_CLAUDE_PATH", &claude_fixture()),
        ("CLAUDE_FIXTURE_MODE_FILE", &mode_file.display().to_string()),
        ("FIXTURE_STDIN_LOG_DIR", &stdin_log.display().to_string()),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "CLAUDE_FIXTURE_MODE_FILE,FIXTURE_STDIN_LOG_DIR"),
    ])
}

fn agent(d: &Daemon, repo: &Path, mode_file: &Path, mode: &str, title: &str) -> String {
    std::fs::write(mode_file, mode).unwrap();
    let id = run_id(&d.call("task.create", json!({"repo": repo, "harness": "claude", "prompt": "do the work", "title": title})));
    d.wait_status(&id, |s| s != "queued" && s != "starting", 20);
    std::thread::sleep(Duration::from_millis(300));
    std::fs::write(mode_file, "overseer").unwrap();
    id
}

fn worktree(d: &Daemon, run: &str) -> std::path::PathBuf {
    let task = d.run(run)["task_id"].as_str().unwrap().to_string();
    let state = d.call("state", json!({}));
    let task = state["tasks"].as_array().unwrap().iter().find(|t| t["id"] == task.as_str()).unwrap().clone();
    let ws = state["workspaces"].as_array().unwrap().iter().find(|w| w["id"] == task["workspace_id"]).unwrap().clone();
    std::path::PathBuf::from(ws["path"].as_str().unwrap())
}

/// What Overseer sent the harness on standard input (FIXTURE_STDIN_LOG_DIR), as JSON lines.
fn stdin_lines(log: &Path, run_dir: &Path) -> Vec<Value> {
    let file = log.join(format!("{}.log", run_dir.file_name().unwrap().to_string_lossy()));
    std::fs::read_to_string(&file).unwrap_or_default().lines().filter_map(|l| serde_json::from_str(l).ok()).collect()
}

#[test]
fn ac241_deny_with_a_note_sends_the_note() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let (mode, log) = (r.path().join("mode"), r.path().join("stdin"));
    let d = daemon(&mode, &log);
    let run = agent(&d, &repo, &mode, "permission", "Docs");
    let waiting = d.wait_status(&run, |s| s == "waiting_for_user", 20);
    let request = waiting["attention"]["request_id"].as_str().unwrap().to_string();
    let note = "Don't write perm.txt; put it in docs/notes.md instead.";
    d.call("run.permission", json!({"run_id": run, "request_id": request, "allow": false, "message": note}));
    assert_eq!(d.wait_done(&run, 20)["status"], "completed");
    let sent = stdin_lines(&log, &worktree(&d, &run));
    let answer = sent.iter().find(|m| m["type"] == "control_response").unwrap_or_else(|| panic!("no answer reached the harness: {sent:#?}"));
    assert_eq!(answer["response"]["response"]["behavior"], "deny", "{answer}");
    assert_eq!(answer["response"]["response"]["message"], note, "the note is the reason the agent reads: {answer}");
    let said = d.events(&run).iter().any(|e| e["kind"] == "tool_result" && e["payload"].to_string().contains("docs/notes.md"));
    assert!(said, "the agent got the note as the tool's result");
}

#[test]
fn ac241_allow_for_this_session_is_not_asked_again() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let (mode, log) = (r.path().join("mode"), r.path().join("stdin"));
    let d = daemon(&mode, &log);
    let run = agent(&d, &repo, &mode, "permission-twice", "Writer");
    let waiting = d.wait_status(&run, |s| s == "waiting_for_user", 20);
    assert_eq!(waiting["attention"]["always"]["label"], "Write · this session", "{waiting}");
    let request = waiting["attention"]["request_id"].as_str().unwrap().to_string();
    d.call("run.permission", json!({"run_id": run, "request_id": request, "allow": true, "always": true}));
    // The fixture asks for Write again (two.txt); the daemon answers it from the session rule.
    let done = d.wait_done(&run, 20);
    assert_eq!(done["status"], "completed", "{done}");
    let asked: Vec<Value> = d.events(&run).into_iter().filter(|e| e["kind"] == "permission").collect();
    assert_eq!(asked.len(), 2, "{asked:#?}");
    assert!(asked[0]["payload"]["auto_allowed"].is_null(), "the first one is the owner's");
    assert_eq!(asked[1]["payload"]["auto_allowed"], "allowed for this session", "{:#}", asked[1]);
    let statuses: Vec<String> = d.events(&run).iter().filter(|e| e["kind"] == "status").map(|e| e["payload"]["status"].as_str().unwrap_or("").to_string()).collect();
    assert_eq!(statuses.iter().filter(|s| *s == "waiting_for_user").count(), 1, "the owner was asked once: {statuses:?}");
    let ws = worktree(&d, &run);
    let answers: Vec<Value> = std::fs::read_to_string(ws.join("answers.jsonl")).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    assert_eq!(answers.len(), 2);
    for a in &answers {
        assert_eq!(a["behavior"], "allow", "{a}");
        assert_eq!(a["updatedPermissions"][0]["rules"][0]["toolName"], "Write", "the session rule goes back each time: {a}");
    }
    assert!(ws.join("one.txt").exists() && ws.join("two.txt").exists());

    // Allow once is only once: the next request for the same tool asks the owner again.
    let again = agent(&d, &repo, &mode, "permission-twice", "Writer again");
    let waiting = d.wait_status(&again, |s| s == "waiting_for_user", 20);
    let first = waiting["attention"]["request_id"].as_str().unwrap().to_string();
    d.call("run.permission", json!({"run_id": again, "request_id": first, "allow": true}));
    let second = d.wait_status(&again, |s| s == "waiting_for_user", 20);
    assert_ne!(second["attention"]["request_id"].as_str().unwrap(), first, "a second request, asked of the owner");
    d.call("run.permission", json!({"run_id": again, "request_id": second["attention"]["request_id"], "allow": false, "message": "enough"}));
    d.wait_done(&again, 20);
}

#[test]
fn ac241_overseer_says_an_agent_is_blocked_on_your_permission() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let (mode, log) = (r.path().join("mode"), r.path().join("stdin"));
    let d = daemon(&mode, &log);
    let run = agent(&d, &repo, &mode, "permission", "Phone");
    d.wait_status(&run, |s| s == "waiting_for_user", 20);
    d.call("overseer.session", json!({}));
    let p = d.call("overseer.propose", json!({"actions": [{"action": "message", "agent": run, "text": "Also add tests."}], "source": "test"}));
    assert!(p["result"].as_str().unwrap().contains("Phone is blocked on your permission to use Write"), "what Overseer's turn reads: {p}");
    assert!(p["result"].as_str().unwrap().contains("ask whether to answer the permission"), "{p}");
    let card = d.call("overseer.card", json!({"id": p["proposal"]}));
    assert_eq!(card["actions"][0]["blocked_on"], "Phone is blocked on your permission to use Write", "{card:#}");
    let proposed = d.call("events.list", json!({"after": 0, "limit": 5000}))["events"].as_array().cloned().unwrap_or_default();
    let line = proposed.iter().filter(|e| e["kind"] == "proposal" && e["payload"]["id"] == p["proposal"]).find_map(|e| e["payload"]["lines"][0].as_str().map(str::to_string)).unwrap_or_default();
    assert!(line.contains("Phone is blocked on your permission to use Write") && line.contains("answer the permission first?"), "the proposal the owner reads: {line:?}");
    // Said yes anyway: the result says the message waits for the permission, not "sent".
    d.call("overseer.answer", json!({"id": p["proposal"], "yes": true}));
    let card = d.call("overseer.card", json!({"id": p["proposal"]}));
    let result = card["result"].as_str().unwrap_or_default();
    assert!(result.starts_with("Waiting on you: Phone is blocked on your permission") && result.contains("waits until you answer it") && !result.contains("Done") && !result.contains("sent \""), "{card:#}");
    let request = d.run(&run)["attention"]["request_id"].as_str().unwrap().to_string();
    d.call("run.permission", json!({"run_id": run, "request_id": request, "allow": false}));
    d.wait_done(&run, 20);
}
