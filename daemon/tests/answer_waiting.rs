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


/// AC274: a native Codex session-cache decision is forwarded, never broadened by a display label.
fn codex_session_cache_remains_native(mode: &str) {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let fixture = repo_root().join("fixtures/fake-harness/codex-app-fixture.js").display().to_string();
    let d = Daemon::start(&[("OVERSEER_CODEX_PATH", &fixture), ("FIXTURE_MODE", mode), ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE")]);
    let run = run_id(&d.call("task.create", json!({"repo": repo, "harness": "codex-app", "prompt": "two commands"})));
    let first = d.wait_status(&run, |s| s == "waiting_for_user", 20);
    d.call("run.permission", json!({"run_id": run, "request_id": first["attention"]["request_id"], "allow": true, "always": true}));
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    let second = loop {
        let row = d.run(&run);
        if row["attention"]["request_id"] == "8" || row["status"] == "completed" { break row; }
        assert!(std::time::Instant::now() < deadline, "second native request never arrived: {row}");
        std::thread::sleep(Duration::from_millis(10));
    };
    assert_eq!(second["status"], "waiting_for_user", "a second native ask is not authorized by the first display label: {second}");
    let ws = worktree(&d, &run);
    assert!(ws.join("first-approved.txt").exists());
    assert!(!ws.join("second-approved.txt").exists(), "no second protected action before its answer");
    let answers: Vec<Value> = std::fs::read_to_string(ws.join("native-answers.jsonl")).unwrap().lines().map(|line| serde_json::from_str(line).unwrap()).collect();
    assert_eq!(answers.len(), 1);
    assert_eq!(answers[0], json!({"id": 7, "result": {"decision": "acceptForSession"}}), "native session decision is preserved exactly");
    d.call("run.permission", json!({"run_id": run, "request_id": second["attention"]["request_id"], "allow": false}));
    d.wait_done(&run, 20);
    let answers: Vec<Value> = std::fs::read_to_string(ws.join("native-answers.jsonl")).unwrap().lines().map(|line| serde_json::from_str(line).unwrap()).collect();
    assert_eq!(answers.len(), 2);
    assert_eq!(answers[1], json!({"id": 8, "result": {"decision": "decline"}}));
    assert!(!ws.join("second-approved.txt").exists(), "deny performs no protected action");
}

#[test]
fn ac274_changed_codex_command_does_not_borrow_a_session_label() { codex_session_cache_remains_native("session-two-commands"); }

#[test]
fn ac274_repeated_codex_command_still_uses_the_native_cache_authority() { codex_session_cache_remains_native("session-repeat-command"); }

fn changed_claude_grant_stays_pending(mode: &str) {
    let r = tmp(); let repo = repo(&r.path().join("repo"));
    let (mode_file, log) = (r.path().join("mode"), r.path().join("stdin"));
    let d = daemon(&mode_file, &log);
    let run = agent(&d, &repo, &mode_file, mode, "Writer");
    let first = d.wait_status(&run, |s| s == "waiting_for_user", 20);
    d.call("run.permission", json!({"run_id": run, "request_id": first["attention"]["request_id"], "allow": true, "always": true}));
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    let second = loop {
        let row = d.run(&run);
        if row["attention"]["request_id"] == "req-2" || row["status"] == "completed" { break row; }
        assert!(std::time::Instant::now() < deadline, "second Claude ask never arrived: {row}");
        std::thread::sleep(Duration::from_millis(10));
    };
    assert_eq!(second["status"], "waiting_for_user", "different native type/behavior is not the granted rule: {second}");
    assert_eq!(first["attention"]["always"]["label"], second["attention"]["always"]["label"], "the displayed label deliberately collides");
    d.call("run.permission", json!({"run_id": run, "request_id": "req-2", "allow": false}));
    d.wait_done(&run, 20);
    assert!(worktree(&d, &run).join("one.txt").exists());
    assert!(!worktree(&d, &run).join("two.txt").exists());
}

#[test]
fn ac274_changed_claude_rule_behavior_does_not_borrow_a_label() { changed_claude_grant_stays_pending("permission-changed-rule-behavior"); }

#[test]
fn ac274_changed_claude_rule_type_does_not_borrow_a_label() { changed_claude_grant_stays_pending("permission-changed-rule-type"); }

#[test]
fn ac274_changed_claude_rule_destination_does_not_borrow_a_label() { changed_claude_grant_stays_pending("permission-changed-rule-destination"); }

#[test]
fn ac274_a_new_process_generation_cannot_replay_an_old_session_grant() {
    let r = tmp(); let repo = repo(&r.path().join("repo"));
    let (mode, log) = (r.path().join("mode"), r.path().join("stdin"));
    let d = daemon(&mode, &log);
    let run = agent(&d, &repo, &mode, "permission-twice", "Writer");
    let first = d.wait_status(&run, |s| s == "waiting_for_user", 20);
    d.call("run.permission", json!({"run_id": run, "request_id": first["attention"]["request_id"], "allow": true, "always": true}));
    let done = d.wait_done(&run, 20);
    std::fs::write(&mode, "permission-twice").unwrap();
    d.call("run.follow_up", json!({"run_id": run, "prompt": "again"}));
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    let new = loop {
        let row = d.run(&run);
        if row["process_generation"] != done["process_generation"] && (row["status"] == "waiting_for_user" || row["status"] == "completed") { break row; }
        assert!(std::time::Instant::now() < deadline, "new generation never asked: {row}");
        std::thread::sleep(Duration::from_millis(10));
    };
    assert_eq!(new["status"], "waiting_for_user", "a resumed harness owns its own grant cache; old host memory cannot answer its new ask: {new}");
    d.call("run.permission", json!({"run_id": run, "request_id": new["attention"]["request_id"], "allow": false}));
    d.wait_done(&run, 20);
}


fn claude_grant_boundary_stays_pending(change_session: bool) {
    let r = tmp(); let repo = repo(&r.path().join("repo"));
    let mode = r.path().join("mode"); let gate = r.path().join("second.gate");
    std::fs::write(&mode, "permission-twice").unwrap();
    let d = Daemon::start(&[("OVERSEER_CLAUDE_PATH", &claude_fixture()),
        ("CLAUDE_FIXTURE_MODE_FILE", &mode.display().to_string()),
        ("FIXTURE_PERMISSION_SECOND_GATE", &gate.display().to_string()),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "CLAUDE_FIXTURE_MODE_FILE,FIXTURE_PERMISSION_SECOND_GATE")]);
    let run = run_id(&d.call("task.create", json!({"repo": repo, "harness": "claude", "prompt": "two writes"})));
    let first = d.wait_status(&run, |s| s == "waiting_for_user", 20);
    d.call("run.permission", json!({"run_id": run, "request_id": first["attention"]["request_id"], "allow": true, "always": true}));
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    db.busy_timeout(Duration::from_secs(10)).unwrap();
    if change_session {
        // Simulate an authoritative stored session migration while the response boundary is held.
        // Nothing in model request params chooses this ownership field.
        db.execute("UPDATE runs SET native_id='different-native-session' WHERE id=?1", [&run]).unwrap();
    } else {
        // Upgrade compatibility: an old event has only the display label, never typed authority.
        db.execute("UPDATE events SET payload=json_remove(payload,'$.grant') WHERE run_id=?1 AND kind='permission_answered'", [&run]).unwrap();
    }
    std::fs::write(&gate, "release").unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    let second = loop {
        let row = d.run(&run);
        if row["attention"]["request_id"] == "req-2" || row["status"] == "completed" { break row; }
        assert!(std::time::Instant::now() < deadline, "second ask missing: {row}");
        std::thread::sleep(Duration::from_millis(10));
    };
    assert_eq!(second["status"], "waiting_for_user", "changed session or old label-only record cannot authorize this request: {second}");
    d.call("run.permission", json!({"run_id": run, "request_id": "req-2", "allow": false}));
    d.wait_done(&run, 20);
    assert!(worktree(&d, &run).join("one.txt").exists());
    assert!(!worktree(&d, &run).join("two.txt").exists());
}

#[test]
fn ac274_changed_native_session_cannot_replay_an_old_grant() { claude_grant_boundary_stays_pending(true); }

#[test]
fn ac274_legacy_display_label_records_are_not_new_authority() { claude_grant_boundary_stays_pending(false); }


#[test]
fn ac274_native_suppression_removes_the_always_offer() {
    let r = tmp(); let repo = repo(&r.path().join("repo"));
    let (mode, log) = (r.path().join("mode"), r.path().join("stdin"));
    let d = daemon(&mode, &log);
    let run = agent(&d, &repo, &mode, "permission-suppress-always", "Native veto");
    let pending = d.wait_status(&run, |s| s == "waiting_for_user", 20);
    assert!(pending["attention"]["always"].is_null(), "native suppression hides the offer even when native suggestions exist: {pending}");
    d.call("run.permission", json!({"run_id": run, "request_id": pending["attention"]["request_id"], "allow": false}));
    d.wait_done(&run, 20);
    let ws = worktree(&d, &run);
    assert!(!ws.join("one.txt").exists() && !ws.join("two.txt").exists());
}

#[test]
fn ac274_a_stale_always_answer_is_refused_when_native_suppression_vetoes_it() {
    let r = tmp(); let repo = repo(&r.path().join("repo"));
    let (mode, log) = (r.path().join("mode"), r.path().join("stdin"));
    let d = daemon(&mode, &log);
    let run = agent(&d, &repo, &mode, "permission-suppress-always", "Stale Always answer");
    let pending = d.wait_status(&run, |s| s == "waiting_for_user", 20);
    let request = pending["attention"]["request_id"].clone();
    let reply: Value = serde_json::from_str(&d.raw(format!("{}\n", json!({"id": 1, "method": "run.permission", "params": {"run_id": run, "request_id": request, "allow": true, "always": true}})).as_bytes())).unwrap();
    assert_eq!(reply["error"]["code"], "no_always_allow", "a stale surface cannot override the native veto: {reply}");
    let still = d.run(&run);
    assert_eq!(still["status"], "waiting_for_user");
    assert_eq!(still["attention"]["request_id"], request);
    let ws = worktree(&d, &run);
    assert!(!ws.join("answers.jsonl").exists(), "the refused answer sends no native response");
    assert!(!ws.join("one.txt").exists());
    assert!(!d.events(&run).iter().any(|e| e["kind"] == "permission_answered"));
    d.call("run.permission", json!({"run_id": run, "request_id": request, "allow": false}));
    d.wait_done(&run, 20);
}

#[test]
fn ac274_native_suppression_prevents_replay_but_preserves_allow_once() {
    let r = tmp(); let repo = repo(&r.path().join("repo"));
    let (mode, log) = (r.path().join("mode"), r.path().join("stdin"));
    let d = daemon(&mode, &log);
    let run = agent(&d, &repo, &mode, "permission-suppress-second", "Session grant then veto");
    let first = d.wait_status(&run, |s| s == "waiting_for_user", 20);
    assert_eq!(first["attention"]["always"]["label"], "Write · this session");
    d.call("run.permission", json!({"run_id": run, "request_id": first["attention"]["request_id"], "allow": true, "always": true}));
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    let second = loop {
        let row = d.run(&run);
        if row["attention"]["request_id"] == "req-2" || row["status"] == "completed" { break row; }
        assert!(std::time::Instant::now() < deadline, "suppressed second ask missing: {row}");
        std::thread::sleep(Duration::from_millis(10));
    };
    assert_eq!(second["status"], "waiting_for_user", "a previous qualified grant cannot bypass the native veto: {second}");
    assert!(second["attention"]["always"].is_null());
    let ws = worktree(&d, &run);
    assert!(ws.join("one.txt").exists() && !ws.join("two.txt").exists());
    d.call("run.permission", json!({"run_id": run, "request_id": "req-2", "allow": true}));
    d.wait_done(&run, 20);
    let answers: Vec<Value> = std::fs::read_to_string(ws.join("answers.jsonl")).unwrap().lines().map(|line| serde_json::from_str(line).unwrap()).collect();
    assert_eq!(answers.len(), 2);
    assert!(answers[0].get("updatedPermissions").is_some());
    assert_eq!(answers[1]["behavior"], "allow");
    assert!(answers[1].get("updatedPermissions").is_none(), "Allow once carries no don't-ask-again update: {}", answers[1]);
    assert!(ws.join("two.txt").exists());
}
