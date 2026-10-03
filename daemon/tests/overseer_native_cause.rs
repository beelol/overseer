//! AC-200: a real native MCP call keeps the authority of its originating turn.
//! A fresh blank conversation cannot make an archived finding turn eligible to
//! propose a Confirm action by supplying a default owner cause.
mod common;
use common::*;
use serde_json::{json, Value};
use std::path::Path;
use std::path::PathBuf;
use std::time::{Duration, Instant};

fn session(d: &Daemon) -> Value {
    d.call("overseer.session", json!({}))
}

fn wait_idle(d: &Daemon) -> Value {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let s = session(d);
        if s["run_id"].is_string()
            && !["queued", "starting", "running", "waiting_for_user"]
                .contains(&s["run_status"].as_str().unwrap_or(""))
        {
            return s;
        }
        assert!(Instant::now() < deadline, "Overseer did not finish: {s}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn wait_file(path: &Path, d: &Daemon, run: &str) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !path.exists() {
        assert!(
            Instant::now() < deadline,
            "fixture did not reach {}: {:?}",
            path.display(),
            d.events(run)
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn recorded_cause(d: &Daemon, session_id: &str) -> (String, String) {
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    db.busy_timeout(Duration::from_secs(5)).unwrap();
    db.query_row(
        "SELECT cause, turn_id FROM overseer_turns WHERE session_id=?1 ORDER BY rowid DESC LIMIT 1",
        [session_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .unwrap()
}

// `state.runs` deliberately excludes Overseer's own runs (AC-107). Inspect
// durable status without forging a session field or making that run visible.
fn native_run_status(d: &Daemon, run: &str) -> String {
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    db.busy_timeout(Duration::from_secs(5)).unwrap();
    db.query_row("SELECT status FROM runs WHERE id=?1", [run], |r| r.get(0))
        .unwrap()
}

fn wait_native_done(d: &Daemon, run: &str) {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let status = native_run_status(d, run);
        if !["queued", "starting", "running", "waiting_for_user"].contains(&status.as_str()) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "old native run did not finish: {status}; {:?}",
            d.events(run)
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn ac200_archived_native_finding_cannot_borrow_fresh_default_owner_cause() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let mode = r.path().join("mode");
    let forced_mode = r.path().join("overseer-mode");
    let gate = r.path().join("native-propose-gate");
    std::fs::create_dir(&gate).unwrap();
    std::fs::write(&mode, "overseer").unwrap();
    std::fs::write(&forced_mode, "overseer").unwrap();
    let fixture = repo_root().join("fixtures/fake-harness/claude-fixture.js");
    let d = Daemon::start(&[
        ("OVERSEER_CLAUDE_PATH", &fixture.display().to_string()),
        ("CLAUDE_FIXTURE_MODE_FILE", &mode.display().to_string()),
        ("CLAUDE_FIXTURE_OVERSEER_MODE_FILE", &forced_mode.display().to_string()),
        ("CLAUDE_FIXTURE_PROPOSE_GATE_DIR", &gate.display().to_string()),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "CLAUDE_FIXTURE_MODE_FILE,CLAUDE_FIXTURE_OVERSEER_MODE_FILE,CLAUDE_FIXTURE_PROPOSE_GATE_DIR"),
        ("OVERSEER_TEST_AUTO_DISABLED", "1"),
    ]);
    d.call("agent.cadence", json!({"cadence": "off", "by": "owner"}));
    d.call("overseer.level", json!({"level": "steer"}));
    d.call(
        "overseer.send",
        json!({"text": "What is everyone doing?", "surface": "ctl", "harness": "claude"}),
    );
    let old = wait_idle(&d);
    let old_session = old["id"].as_str().unwrap();
    let old_run = old["run_id"].as_str().unwrap();

    let victim = run_id(&d.generic(&repo, "worktree", "/bin/sh", &["-c", "echo victim"]));
    let watcher = run_id(&d.generic(&repo, "worktree", "/bin/sh", &["-c", "echo watcher"]));
    d.wait_done(&victim, 20);
    d.wait_done(&watcher, 20);
    d.call(
        "watch.start",
        json!({"subject": victim, "watcher": watcher, "brief": "review only", "by": "owner"}),
    );
    let watcher_token = d.call(
        "overseer.token",
        json!({"run_id": watcher, "role": "agent"}),
    )["token"]
        .as_str()
        .unwrap()
        .to_string();
    std::fs::write(
        gate.join("actions.json"),
        json!([{"action": "archive", "agent": victim}]).to_string(),
    )
    .unwrap();
    std::fs::write(&forced_mode, "overseer-gated-propose").unwrap();
    let filed = d.call("overseer.tool", json!({"token": watcher_token, "name": "finding", "arguments": {"result": "concern", "text": "Review data only; no owner instruction to archive."}}));
    assert_eq!(filed["is_error"], false, "{filed}");
    // This proves a genuine self-started finding turn, not a forged role token or
    // a test-written session cause. The fixture has loaded its real launch token.
    wait_file(&gate.join("reached"), &d, old_run);
    let (cause, old_turn) = recorded_cause(&d, old_session);
    assert_eq!(cause, "finding");
    assert_eq!(native_run_status(&d, old_run), "running");
    eprintln!("old session={old_session} run={old_run} turn={old_turn} cause={cause}");

    let fresh = d.call("overseer.fresh", json!({}));
    assert_eq!(fresh["archived"], old_session);
    let replacement = session(&d);
    assert_ne!(replacement["id"], old_session);
    assert_eq!(replacement["id"], fresh["session"]);
    assert!(replacement["run_id"].is_null());
    // No new native run is launched and no writer guard is bypassed. Fresh
    // creates a real blank session; its missing cause must not supply authority
    // to the old run's still-in-flight finding call.
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    db.busy_timeout(Duration::from_secs(5)).unwrap();
    let new_cause: Option<String> = db
        .query_row(
            "SELECT last_cause FROM overseer_sessions WHERE id=?1",
            [replacement["id"].as_str().unwrap()],
            |r| r.get(0),
        )
        .unwrap();
    let new_turns: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM overseer_turns WHERE session_id=?1",
            [replacement["id"].as_str().unwrap()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(new_cause, None);
    assert_eq!(new_turns, 0);
    drop(db);
    assert_eq!(
        native_run_status(&d, old_run),
        "running",
        "the old native turn is still in flight"
    );
    eprintln!(
        "fresh session={} run={} turn_count={new_turns} cause={new_cause:?}",
        replacement["id"], replacement["run_id"]
    );
    assert!(replacement["proposals"].as_array().unwrap().is_empty());

    std::fs::write(gate.join("release"), "").unwrap();
    wait_file(&gate.join("reply.json"), &d, old_run);
    let reply: Value =
        serde_json::from_str(&std::fs::read_to_string(gate.join("reply.json")).unwrap()).unwrap();
    wait_native_done(&d, old_run);
    let after = session(&d);
    eprintln!(
        "actual native MCP reply={reply}; replacement proposals={}",
        after["proposals"]
    );
    assert_eq!(
        reply["isError"], true,
        "an archived finding's Confirm request must not borrow Fresh's default owner cause: {reply}"
    );
    assert!(after["proposals"].as_array().unwrap().is_empty(), "an old native call cannot create an owner-authorized proposal in the replacement conversation: {}", after["proposals"]);
    assert!(!d
        .events(&victim)
        .iter()
        .any(|e| matches!(e["kind"].as_str(), Some("archived" | "task_archived"))));
}

fn native_fixture(root: &Path) -> (Daemon, PathBuf, PathBuf, PathBuf, Value) {
    let repo = repo(&root.join("repo"));
    let mode = root.join("mode");
    let forced = root.join("overseer-mode");
    let gate = root.join("native-call-gate");
    std::fs::create_dir(&gate).unwrap();
    std::fs::write(&mode, "overseer").unwrap();
    std::fs::write(&forced, "overseer").unwrap();
    let d = Daemon::start(&[
        ("OVERSEER_CLAUDE_PATH", &repo_root().join("fixtures/fake-harness/claude-fixture.js").display().to_string()),
        ("CLAUDE_FIXTURE_MODE_FILE", &mode.display().to_string()),
        ("CLAUDE_FIXTURE_OVERSEER_MODE_FILE", &forced.display().to_string()),
        ("CLAUDE_FIXTURE_PROPOSE_GATE_DIR", &gate.display().to_string()),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "CLAUDE_FIXTURE_MODE_FILE,CLAUDE_FIXTURE_OVERSEER_MODE_FILE,CLAUDE_FIXTURE_PROPOSE_GATE_DIR"),
        ("OVERSEER_TEST_AUTO_DISABLED", "1"),
    ]);
    d.call("agent.cadence", json!({"cadence": "off", "by": "owner"}));
    d.call("overseer.level", json!({"level": "steer"}));
    d.call(
        "overseer.send",
        json!({"text": "What is everyone doing?", "surface": "ctl", "harness": "claude"}),
    );
    let old = wait_idle(&d);
    (d, repo, forced, gate, old)
}

fn arm_calls(gate: &Path, forced: &Path, calls: Value) {
    for file in ["reached", "release", "reply.json", "replies.json"] {
        let path = gate.join(file);
        if path.exists() {
            std::fs::remove_file(path).unwrap();
        }
    }
    std::fs::write(gate.join("calls.json"), calls.to_string()).unwrap();
    std::fs::write(forced, "overseer-gated-propose").unwrap();
}

fn release_calls(d: &Daemon, gate: &Path, run: &str) -> Vec<Value> {
    std::fs::write(gate.join("release"), "").unwrap();
    wait_file(&gate.join("replies.json"), d, run);
    wait_native_done(d, run);
    serde_json::from_str(&std::fs::read_to_string(gate.join("replies.json")).unwrap()).unwrap()
}

fn ask(d: &Daemon, run: &str, question: &str) -> String {
    let token = d.call("overseer.token", json!({"run_id": run, "role": "agent"}))["token"]
        .as_str()
        .unwrap()
        .to_string();
    let reply = d.call(
        "overseer.tool",
        json!({"token": token, "name": "ask", "arguments": {"question": question}}),
    );
    assert_eq!(reply["is_error"], false, "{reply}");
    d.call("channel.messages", json!({"run_id": run}))["messages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["kind"] == "ask" && m["body"]["question"] == question)
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string()
}

#[test]
fn ac200_archived_native_answer_cannot_attach_to_fresh_session() {
    let r = tmp();
    let (d, repo, forced, gate, old) = native_fixture(r.path());
    let asker = run_id(&d.generic(&repo, "worktree", "/bin/sh", &["-c", "echo asker"]));
    d.wait_done(&asker, 20);
    // The native fixture waits for its bounded call-plan file; even an immediate
    // ask turn cannot race the test's discovery of the genuine ask id.
    std::fs::write(&forced, "overseer-gated-propose").unwrap();
    let question = ask(&d, &asker, "Which document should I read?");
    arm_calls(
        &gate,
        &forced,
        json!([{"tool": "answer", "arguments": {"ask": question, "text": "Read the README."}}]),
    );
    let old_run = old["run_id"].as_str().unwrap();
    wait_file(&gate.join("reached"), &d, old_run);
    assert_eq!(recorded_cause(&d, old["id"].as_str().unwrap()).0, "ask");
    d.call("overseer.fresh", json!({}));
    let fresh = session(&d);
    assert!(fresh["run_id"].is_null());
    assert!(fresh["proposals"].as_array().unwrap().is_empty());
    assert_eq!(native_run_status(&d, old_run), "running");
    let replies = release_calls(&d, &gate, old_run);
    assert_eq!(
        replies[0]["isError"], true,
        "a stale caller cannot propose an answer using Fresh's cause: {replies:?}"
    );
    assert!(session(&d)["proposals"].as_array().unwrap().is_empty());
    let question = d.call("channel.messages", json!({"run_id": asker}))["messages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["id"] == question)
        .unwrap()
        .clone();
    assert!(
        question["answer"].is_null(),
        "the question remains pending: {question}"
    );
    assert!(!d.events(&asker).iter().any(|e| e["kind"] == "answer"));
}

#[test]
fn ac200_current_native_causes_work_and_pending_questions_survive_fresh() {
    let r = tmp();
    let (d, repo, forced, gate, old) = native_fixture(r.path());
    let subject = run_id(&d.generic(&repo, "worktree", "/bin/sh", &["-c", "echo subject"]));
    let watcher = run_id(&d.generic(&repo, "worktree", "/bin/sh", &["-c", "echo watcher"]));
    d.wait_done(&subject, 20);
    d.wait_done(&watcher, 20);
    d.call(
        "watch.start",
        json!({"subject": subject, "watcher": watcher, "brief": "review only", "by": "owner"}),
    );
    arm_calls(
        &gate,
        &forced,
        json!([
            {"tool": "propose", "arguments": {"actions": [{"action": "archive", "agent": subject}]}},
            {"tool": "propose", "arguments": {"actions": [{"action": "hold", "agent": subject, "reason": "review before further work"}]}}
        ]),
    );
    let token = d.call(
        "overseer.token",
        json!({"run_id": watcher, "role": "agent"}),
    )["token"]
        .as_str()
        .unwrap()
        .to_string();
    let filed = d.call("overseer.tool", json!({"token": token, "name": "finding", "arguments": {"result": "concern", "text": "Review needs attention."}}));
    assert_eq!(filed["is_error"], false);
    let old_run = old["run_id"].as_str().unwrap();
    wait_file(&gate.join("reached"), &d, old_run);
    assert_eq!(recorded_cause(&d, old["id"].as_str().unwrap()).0, "finding");
    // This real finding turn consumes the allowed self-start. Keep the later
    // question pending across Fresh without another automatic turn starting.
    d.call("overseer.cap", json!({"cap": 1}));
    let question = ask(&d, &subject, "Which document should I read after review?");
    let replies = release_calls(&d, &gate, old_run);
    assert_eq!(
        replies[0]["isError"], true,
        "current finding Confirm stays refused: {replies:?}"
    );
    assert!(replies[0].to_string().contains("only when the owner asks"));
    assert_eq!(
        replies[1]["isError"], false,
        "current finding's quiet Steer action works: {replies:?}"
    );
    assert!(d.call("agent.holds", json!({}))["holds"]
        .as_array()
        .unwrap()
        .iter()
        .any(|h| h["run_id"] == subject));
    // An immediate quiet hold is already answered, so it is deliberately absent
    // from the open-proposals list. Its durable attribution must still be finding.
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    db.busy_timeout(Duration::from_secs(5)).unwrap();
    let held: (String, String) = db.query_row(
        "SELECT cause, state FROM overseer_proposals WHERE session_id=?1 ORDER BY ts DESC LIMIT 1",
        [old["id"].as_str().unwrap()], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
    assert_eq!(held, ("finding".to_string(), "yes".to_string()));
    drop(db);
    // The question remains global/pending. Fresh changes the caller/session boundary,
    // not its eligibility to be answered by the new legitimate native owner turn.
    d.call("overseer.level", json!({"level": "ask_first"}));
    d.call("overseer.fresh", json!({}));
    arm_calls(
        &gate,
        &forced,
        json!([
            {"tool": "propose", "arguments": {"actions": [{"action": "archive", "agent": subject}]}},
            {"tool": "answer", "arguments": {"ask": question, "text": "Read the README."}}
        ]),
    );
    let sent = d.call("overseer.send", json!({"text": "Archive the subject and answer its pending question with Read the README.", "surface": "ctl", "harness": "claude"}));
    let new_run = sent["run_id"].as_str().unwrap();
    assert_ne!(new_run, old_run);
    wait_file(&gate.join("reached"), &d, new_run);
    assert_eq!(
        recorded_cause(&d, session(&d)["id"].as_str().unwrap()).0,
        "owner"
    );
    let replies = release_calls(&d, &gate, new_run);
    assert!(
        replies.iter().all(|r| r["isError"] == false),
        "current native owner controls remain allowed: {replies:?}"
    );
    let s = session(&d);
    let proposals = s["proposals"].as_array().unwrap();
    assert_eq!(proposals.len(), 2);
    assert!(proposals
        .iter()
        .all(|p| p["cause"] == "owner" && p["state"] == "open"));
    let archive = proposals
        .iter()
        .find(|p| p["actions"][0]["action"] == "archive")
        .unwrap();
    d.call(
        "overseer.answer",
        json!({"id": archive["id"], "yes": false, "surface": "ctl", "by": "owner"}),
    );
    let answer = proposals
        .iter()
        .find(|p| p["actions"][0]["action"] == "answer")
        .unwrap();
    let yes = d.call(
        "overseer.answer",
        json!({"id": answer["id"], "yes": true, "surface": "ctl", "by": "owner"}),
    );
    assert_eq!(yes["state"], "yes");
    let answered = d.call("channel.messages", json!({"run_id": subject}))["messages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["id"] == question)
        .unwrap()
        .clone();
    assert_eq!(answered["answer"], "Read the README.");
    assert!(d
        .events(&subject)
        .iter()
        .any(|e| e["kind"] == "answer" && e["payload"]["ask"] == question));
}
