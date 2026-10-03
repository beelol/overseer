//! AC274 Slice1 boundary qualification. Frozen native-shaped scripts pass through
//! the actual fixture stdout, shim, daemon socket and public event/raw projections.
//! These authored tests are unexecuted until the coordinator grants Cargo.
mod common;

use common::*;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const PRIVATE: &str = "fixtureOpaquePrivate63827";
const ONE: &str = "native-pending-batch-one";
const TWO: &str = "native-pending-batch-two";

fn vector(name: &str) -> Value {
    let vectors: Vec<Value> = serde_json::from_str(include_str!(
        "../../fixtures/transcripts/ac274/native-vectors.json"
    ))
    .unwrap();
    vectors.iter().find(|v| v["name"] == name).unwrap()["request"].clone()
}

fn command(id: Value, approval: &str) -> Value {
    let mut frame = vector("command_command_accept");
    frame["id"] = id;
    frame["params"]["threadId"] = json!("$THREAD");
    frame["params"]["turnId"] = json!("$TURN");
    frame["params"]["itemId"] = json!("shared-item");
    frame["params"]["approvalId"] = json!(approval);
    frame["params"]["cwd"] = json!("$CWD");
    frame["params"]["command"] = json!("touch protected-action.txt");
    frame
}

fn emit(frame: Value) -> Value {
    json!({"emit": frame})
}
fn mark(name: &str) -> Value {
    json!({"mark": name})
}
fn gate(path: &Path) -> Value {
    json!({"gate": path})
}

struct Script {
    // Stop fixture processes before deleting their script/repository/log folder.
    daemon: Daemon,
    scratch: tempfile::TempDir,
    repo: PathBuf,
    created: Value,
}
impl Script {
    fn start(harness: &str, version: &str, steps: Vec<Value>) -> Self {
        let scratch = tmp();
        let repo = repo(&scratch.path().join("repo"));
        let file = scratch.path().join("script.json");
        std::fs::write(&file, json!({"steps": steps}).to_string()).unwrap();
        let version_file = scratch.path().join("version");
        std::fs::write(&version_file, version).unwrap();
        let stdin = scratch.path().join("stdin");
        std::fs::create_dir_all(&stdin).unwrap();
        let codex = repo_root().join("fixtures/fake-harness/codex-app-fixture.js");
        let claude = repo_root().join("fixtures/fake-harness/claude-fixture.js");
        let daemon = Daemon::start(&[
            ("OVERSEER_CODEX_PATH", codex.to_str().unwrap()),
            ("OVERSEER_CLAUDE_PATH", claude.to_str().unwrap()),
            ("FIXTURE_MODE", "native-pending"),
            ("FIXTURE_NATIVE_REQUESTS_FILE", file.to_str().unwrap()),
            ("FIXTURE_VERSION_FILE", version_file.to_str().unwrap()),
            ("CLAUDE_FIXTURE_VERSION", version),
            ("FIXTURE_STDIN_LOG_DIR", stdin.to_str().unwrap()),
            ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE,FIXTURE_NATIVE_REQUESTS_FILE,FIXTURE_VERSION_FILE,CLAUDE_FIXTURE_VERSION,FIXTURE_STDIN_LOG_DIR"),
            ("OVERSEER_TEST_AUTO_DISABLED", "1"),
        ]);
        daemon.call("agent.cadence", json!({"cadence": "off", "by": "owner"}));
        let created = daemon.call(
            "task.create",
            json!({
                "repo": repo, "harness": harness, "prompt": "synthetic pending boundary",
                "title": "Pending fixture", "approval_policy": "untrusted"
            }),
        );
        Self {
            daemon,
            scratch,
            repo,
            created,
        }
    }
    fn run(&self) -> String {
        run_id(&self.created)
    }
    fn marker(&self, marker: &str, count: usize) {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let found = self
                .daemon
                .events(&self.run())
                .iter()
                .filter(|e| e["kind"] == "output" && e["payload"]["text"] == marker)
                .count();
            if found >= count {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "fixture output was not processed: {marker}"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    fn collection(&self) -> Value {
        let result = self
            .daemon
            .call("run.requests", json!({"run_id": self.run()}));
        assert!(
            result["cursor"].as_i64().is_some(),
            "collection has a replay cursor: {result}"
        );
        result["requests"]
            .as_array()
            .expect("typed request collection");
        result
    }
    fn requests(&self) -> Vec<Value> {
        self.collection()["requests"].as_array().unwrap().clone()
    }
    fn no_protected_action(&self) {
        assert!(!ws_path(&self.daemon, &self.created)
            .join("protected-action.txt")
            .exists());
        assert!(!self.repo.join("protected-action.txt").exists());
    }
}

fn assert_identity(r: &Value, run: &str) {
    assert!(
        r["key"].as_str().is_some_and(|s| s.starts_with("req-")),
        "opaque public key: {r}"
    );
    assert!(
        r["revision"].as_i64().is_some_and(|n| n > 0),
        "revision: {r}"
    );
    assert_eq!(r["process_run_id"], run);
    assert!(
        r.get("native_id").is_none() && r.get("envelope").is_none() && r.get("input").is_none()
    );
}

#[test]
fn two_native_requests_preserve_order_and_tagged_ids() {
    let s = Script::start(
        "codex-app",
        "codex-cli 0.158.0",
        vec![
            emit(command(json!(7), "integer-offer")),
            emit(command(json!("7"), "string-offer")),
            mark(ONE),
        ],
    );
    s.marker(ONE, 1);
    let pending = s.requests();
    assert_eq!(
        pending.len(),
        2,
        "neither native request may overwrite the other"
    );
    for r in &pending {
        assert_identity(r, &s.run());
        assert_eq!(r["family"], "command");
        assert_eq!(r["lifecycle"], "pending");
    }
    assert_ne!(
        pending[0]["key"], pending[1]["key"],
        "integer7 is distinct from string7"
    );
    assert_eq!(
        s.daemon.run(&s.run())["attention"]["request_id"],
        pending[0]["key"],
        "legacy navigation projects the first compatible item"
    );
    assert_eq!(
        s.requests(),
        pending,
        "collection order is stable across reads"
    );
    assert_eq!(
        s.daemon.run(&s.run())["pending_requests"],
        json!(pending),
        "state and collection expose the same ordered identities"
    );
    s.no_protected_action();
}

#[test]
fn same_item_distinct_approval_requests_do_not_overwrite() {
    let s = Script::start(
        "codex-app",
        "codex-cli 0.158.0",
        vec![
            emit(command(json!(7), "first-approval")),
            emit(command(json!(8), "second-approval")),
            mark(ONE),
        ],
    );
    s.marker(ONE, 1);
    let pending = s.requests();
    assert_eq!(
        pending.len(),
        2,
        "itemId and displayed command are equal; approval identities are not"
    );
    assert_ne!(pending[0]["key"], pending[1]["key"]);
    s.no_protected_action();
}

#[test]
fn replayed_frame_preserves_key_and_cursor() {
    let barrier = tmp();
    let release = barrier.path().join("release");
    let frame = command(json!(7), "replayed-offer");
    let mut s = Script::start(
        "codex-app",
        "codex-cli 0.158.0",
        vec![
            emit(frame.clone()),
            mark(ONE),
            gate(&release),
            emit(frame),
            mark(TWO),
        ],
    );
    s.marker(ONE, 1);
    let before = s.collection();
    assert_eq!(before["requests"].as_array().unwrap().len(), 1);
    s.daemon.kill9();
    s.daemon.spawn();
    assert_eq!(
        s.collection()["cursor"],
        before["cursor"],
        "daemon restart cannot advance the pending-collection cursor"
    );
    assert_eq!(
        s.collection()["requests"],
        before["requests"],
        "pending survives daemon crash without a new identity"
    );
    std::fs::write(release, "release").unwrap();
    s.marker(TWO, 1);
    assert_eq!(
        s.collection()["cursor"],
        before["cursor"],
        "duplicate native frame cannot advance the pending-collection cursor"
    );
    assert_eq!(
        s.collection()["requests"],
        before["requests"],
        "replayed frame cannot duplicate or revise authority"
    );
    assert_eq!(
        s.daemon
            .events(&s.run())
            .iter()
            .filter(|e| e["kind"] == "pending_request")
            .count(),
        1,
        "source replay cannot emit a second pending creation event"
    );
    s.no_protected_action();
}

#[test]
fn live_duplicate_id_changed_offer_conflicts_without_authority() {
    let original = command(json!(7), "original");
    let mut changed = original.clone();
    changed["params"]["command"] = json!("touch different-protected-action.txt");
    let s = Script::start(
        "codex-app",
        "codex-cli 0.158.0",
        vec![emit(original), emit(changed), mark(ONE)],
    );
    s.marker(ONE, 1);
    let requests = s.requests();
    assert_eq!(
        requests.len(),
        1,
        "ambiguous native ID must not acquire a second answerable offer"
    );
    assert!(
        s.daemon
            .events(&s.run())
            .iter()
            .any(|e| e["payload"]["reason_code"] == "native_identity_conflict"),
        "conflict must be observable rather than overwrite the original"
    );
    s.no_protected_action();
    assert!(!ws_path(&s.daemon, &s.created)
        .join("different-protected-action.txt")
        .exists());
}

#[test]
fn same_raw_id_after_generation_restart_refuses_old_key() {
    let s = Script::start(
        "codex-app",
        "codex-cli 0.158.0",
        vec![emit(command(json!(7), "same-id")), mark(ONE)],
    );
    s.marker(ONE, 1);
    let old = s.requests();
    assert_eq!(old.len(), 1);
    let generation = s.daemon.run(&s.run())["process_generation"].clone();
    s.daemon.call("run.interrupt", json!({"run_id": s.run()}));
    s.daemon.wait_done(&s.run(), 20);
    s.daemon.call(
        "run.follow_up",
        json!({"run_id": s.run(), "prompt": "explicit next fixture generation"}),
    );
    s.daemon
        .call("run.resume_queue", json!({"run_id": s.run()}));
    s.marker(ONE, 2);
    assert_ne!(s.daemon.run(&s.run())["process_generation"], generation);
    let new = s.requests();
    let current: Vec<_> = new.iter().filter(|r| r["lifecycle"] == "pending").collect();
    assert_eq!(current.len(), 1);
    assert_ne!(current[0]["key"], old[0]["key"]);
    let response = s.daemon.raw(
        format!(
            "{}\n",
            json!({"id":1,"method":"run.request.answer","params":{
                "run_id":s.run(),"request_key":old[0]["key"],"revision":old[0]["revision"],
                "answer":{"kind":"decision","decision":"accept"}
            }})
        )
        .as_bytes(),
    );
    assert!(
        serde_json::from_str::<Value>(&response)
            .unwrap()
            .get("error")
            .is_some(),
        "old key never addresses the successor pipe: {response}"
    );
    assert_eq!(s.requests(), new);
    s.no_protected_action();
}

#[test]
fn native_child_pending_identity_keeps_owning_parent_transport() {
    let mut child = command(json!(7), "child-approval");
    child["params"]["threadId"] = json!("thr-child");
    child["params"]["turnId"] = json!("child-turn");
    let s = Script::start(
        "codex-app",
        "codex-cli 0.158.0",
        vec![
            emit(
                json!({"method":"item/completed","params":{"threadId":"$THREAD","turnId":"$TURN","item":{
                    "type":"collabAgentToolCall","id":"spawn-child","tool":"spawnAgent","status":"completed", "senderThreadId":"$THREAD",
                    "receiverThreadIds":["thr-child"],"prompt":"child fixture","agentsStates":{"thr-child":{"status":"running"}}
                }}}),
            ),
            emit(child),
            mark(ONE),
        ],
    );
    s.marker(ONE, 1);
    let native_child = s
        .daemon
        .runs()
        .into_iter()
        .find(|r| r["native_id"] == "thr-child")
        .unwrap();
    let pending = s.requests();
    assert_eq!(pending.len(), 1);
    assert_identity(&pending[0], &s.run());
    assert_eq!(pending[0]["run_id"], native_child["id"]);
    s.no_protected_action(); // Actual child response routing remains a required Slice2 capture.
}

fn public_replay(d: &Daemon, run: &str) -> String {
    let mut socket = UnixStream::connect(d.socket()).unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(20)))
        .unwrap();
    writeln!(
        socket,
        "{}",
        json!({"id":1,"method":"events.subscribe","params":{"run_id":run,"after":0}})
    )
    .unwrap();
    let mut reader = BufReader::new(socket);
    let mut all = String::new();
    loop {
        let mut line = String::new();
        assert!(reader.read_line(&mut line).unwrap() > 0);
        let frame: Value = serde_json::from_str(&line).unwrap();
        all.push_str(&line);
        if frame["method"] == "replayed" {
            return all;
        }
    }
}

fn assert_private(s: &Script) {
    let state = s.daemon.call("state", json!({})).to_string();
    let events = s.daemon.events(&s.run());
    let raw = s
        .daemon
        .call("run.raw_output", json!({"run_id":s.run()}))
        .to_string();
    let replay = public_replay(&s.daemon, &s.run());
    for (surface, text) in [
        ("state", state),
        ("events", json!(events).to_string()),
        ("raw_output", raw),
        ("subscribe", replay),
    ] {
        assert!(
            !text.contains(PRIVATE),
            "private native envelope escaped through {surface}: {text}"
        );
    }
}

#[test]
fn secret_question_native_identity_is_private_and_not_a_bool_permission() {
    let mut question = vector("codex_question_free_text");
    question["id"] = json!(PRIVATE);
    question["params"]["threadId"] = json!("$THREAD");
    question["params"]["turnId"] = json!("$TURN");
    question["params"]["questions"][0]["id"] = json!(PRIVATE);
    let s = Script::start(
        "codex-app",
        "codex-cli 0.158.0",
        vec![emit(question), mark(ONE)],
    );
    s.marker(ONE, 1);
    assert_private(&s);
    let pending = s.requests();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0]["family"], "questions");
    assert!(pending[0].to_string().contains("secret"));
    assert_ne!(
        s.daemon.run(&s.run())["attention"]["kind"],
        "permission",
        "question cannot acquire legacy yes/no authority"
    );
    s.no_protected_action();
}

#[test]
fn unknown_protocol_version_secret_question_is_quarantined_before_legacy_parse() {
    let mut question = vector("claude_question_text");
    question["request"]["input"]["answers"] = json!({"Which option?": PRIVATE});
    let s = Script::start("claude", "99.0.0", vec![emit(question), mark(ONE)]);
    s.marker(ONE, 1);
    assert_private(&s);
    assert_ne!(
        s.daemon.run(&s.run())["attention"]["kind"],
        "permission",
        "unknown version cannot fall back to bool permission"
    );
    assert!(s
        .daemon
        .events(&s.run())
        .iter()
        .any(|e| e["payload"]["reason_code"] == "unqualified_native_version"));
    s.no_protected_action();
}

#[test]
fn future_native_control_secret_payload_is_quarantined_and_observable() {
    let frame = json!({"type":"control_request","request_id":PRIVATE,"request":{
        "subtype":"future_native_dialog","payload":{"opaque":PRIVATE}
    }});
    let s = Script::start("claude", "2.1.288", vec![emit(frame), mark(ONE)]);
    s.marker(ONE, 1);
    assert_private(&s);
    assert!(
        s.daemon
            .events(&s.run())
            .iter()
            .any(|e| e["payload"]["reason_code"] == "unsupported_native_kind"),
        "future request must have a safe actionable diagnostic"
    );
    assert_ne!(s.daemon.run(&s.run())["attention"]["kind"], "permission");
    s.no_protected_action();
}

#[test]
fn machine_callbacks_are_private_and_never_owner_cards() {
    let mut refresh = vector("machine_account_chatgptAuthTokens_refresh");
    refresh["params"]["previousAccountId"] = json!(PRIVATE);
    let s = Script::start(
        "codex-app",
        "codex-cli 0.158.0",
        vec![emit(refresh), mark(ONE)],
    );
    s.marker(ONE, 1);
    assert_private(&s);
    assert!(
        s.requests().is_empty(),
        "auth refresh belongs to an authenticated provider, never owner consent"
    );
    assert!(s
        .daemon
        .events(&s.run())
        .iter()
        .any(|e| e["payload"]["reason_code"] == "machine_capability_unavailable"));
    s.no_protected_action();
}

#[test]
fn unknown_dialog_does_not_fabricate_cancel() {
    let mut dialog = vector("claude_unknown_dialog_no_cancel");
    dialog["request"]["payload"]["opaque"] = json!(PRIVATE);
    let s = Script::start("claude", "2.1.288", vec![emit(dialog), mark(ONE)]);
    s.marker(ONE, 1);
    assert_private(&s);
    assert!(s
        .daemon
        .events(&s.run())
        .iter()
        .any(|e| e["payload"]["reason_code"] == "unsupported_native_kind"));
    let input: String = std::fs::read_dir(s.scratch.path().join("stdin"))
        .unwrap()
        .flatten()
        .map(|f| std::fs::read_to_string(f.path()).unwrap())
        .collect();
    assert!(
        !input.contains("control_response"),
        "undeclared dialog kind must not receive fake native success/cancellation: {input}"
    );
    s.no_protected_action();
}
