//! AC274 Slice1 boundary qualification. Frozen native-shaped scripts pass through
//! the actual fixture stdout, shim, daemon socket and public event/raw projections.
//! Baseline e4d8c7b: all 11 authored boundaries fail; see committed RED evidence.
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
        Self::start_with_env(harness, version, steps, &[])
    }
    fn start_with_env(harness: &str, version: &str, steps: Vec<Value>, extra: &[(&str, &str)]) -> Self {
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
        let mut env = vec![
            ("OVERSEER_CODEX_PATH", codex.to_str().unwrap()),
            ("OVERSEER_CLAUDE_PATH", claude.to_str().unwrap()),
            ("FIXTURE_MODE", "native-pending"),
            ("FIXTURE_NATIVE_REQUESTS_FILE", file.to_str().unwrap()),
            ("FIXTURE_VERSION_FILE", version_file.to_str().unwrap()),
            ("CLAUDE_FIXTURE_VERSION", version),
            ("FIXTURE_STDIN_LOG_DIR", stdin.to_str().unwrap()),
            ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE,FIXTURE_NATIVE_REQUESTS_FILE,FIXTURE_VERSION_FILE,CLAUDE_FIXTURE_VERSION,FIXTURE_STDIN_LOG_DIR"),
            ("OVERSEER_TEST_AUTO_DISABLED", "1"),
        ];
        env.extend_from_slice(extra);
        let daemon = Daemon::start(&env);
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
            emit(json!({"method":"turn/started","params":{"threadId":"thr-child","turn":{"id":"child-turn"}}})),
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

/// A private copy of the daemon executable supplies a real exec failure after
/// generation 1 is durably recorded. Never rename a shared Cargo/owner binary.
#[test]
fn failed_recorded_supervisor_launch_settles_child_and_releases_claim() {
    let scratch = tmp();
    let checkout = repo(&scratch.path().join("source"));
    let fixture = repo_root().join("fixtures/fake-harness/codex-app-fixture.js");
    let mut d = Daemon::start(&[
        ("OVERSEER_CODEX_PATH", fixture.to_str().unwrap()),
        ("FIXTURE_MODE", "managed-models"),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE"),
    ]);
    let parent = run_id(&d.call("task.create", json!({"repo":checkout,"harness":"codex-app",
        "model":"gpt-6-astra","effort":"high","prompt":"synthetic discovery"})));
    assert_eq!(d.wait_done(&parent,15)["status"],"completed");
    d.kill9();
    let executable = scratch.path().join("private-daemon-copy");
    std::fs::copy(BIN,&executable).unwrap();
    d.spawn_from(&executable);
    std::fs::rename(&executable,scratch.path().join("retired-private-daemon-copy")).unwrap();
    assert!(!executable.exists());
    let result = d.call("auto.dispatch",json!({"work_unit_id":"native-collection-launch-failure",
        "parent_run_id":parent,"min_tier":"general","required_tools":[],"prompt":"synthetic failed launch"}));
    assert_eq!(result["state"],"paused","launch failure is returned honestly: {result}");
    let child = d.runs().into_iter().find(|r|r["parent_run_id"] == parent).unwrap();
    assert_eq!(child["process_generation"],1,"the pre-spawn identity was durably advanced");
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let run_id = child["id"].as_str().unwrap();
    let dir: Option<String> = db.query_row("SELECT run_dir FROM runs WHERE id=?1",[run_id],|r|r.get(0)).unwrap();
    assert!(dir.is_none(),"failed exec has no recorded supervisor");
    assert!(!d.home.path().join("runs").join(run_id).join("p1/shim.json").exists());
    assert_eq!(child["status"],"failed","trusted launch failure must settle the advanced generation");
    assert!(child["ended_ms"].is_number());
    let claim: String = db.query_row("SELECT state FROM auto_pool_claims WHERE work_unit_id='native-collection-launch-failure'",[],|r|r.get(0)).unwrap();
    assert_eq!(claim,"released","no supervisor consumed the reserved allowance");
    assert!(d.events(run_id).iter().any(|e|e["kind"] == "status" && e["payload"]["status"] == "failed"));
}

/// Task2 Slice2 baseline fixtures: proposed socket/receipt boundary, not helper
/// implementation. UNEXECUTED until the coordinator grants a compiler slot.
mod slice2 {
    use super::*;
    use sha2::{Digest, Sha256};
    use std::sync::{Arc, Barrier};

    fn frozen(name: &str) -> Value {
        let vectors: Vec<Value> = serde_json::from_str(include_str!("../../fixtures/transcripts/ac274/native-vectors.json")).unwrap();
        vectors.into_iter().find(|v| v["name"] == name).unwrap()
    }
    fn answer_params(s: &Script, item: &Value, answer: Value) -> Value {
        json!({"run_id":s.run(),"request_key":item["key"],"revision":item["revision"],"answer":answer})
    }
    fn start_vector(name: &str) -> (Script, Value) {
        let v = frozen(name);
        let mut frame = v["request"].clone();
        if frame["params"].is_object() {
            frame["params"]["threadId"] = json!("$THREAD");
            if frame["params"]["turnId"].is_string() { frame["params"]["turnId"] = json!("$TURN"); }
        }
        let harness = v["harness"].as_str().unwrap();
        let s = Script::start(harness,if harness=="claude" {"2.1.288"} else {"codex-cli 0.158.0"},vec![emit(frame),mark(ONE)]);
        s.marker(ONE,1);
        assert_eq!(s.requests().len(),1,"frozen native frame reaches actual pending collection");
        (s,v)
    }
    fn native_replies(s: &Script) -> Vec<Value> {
        if s.daemon.run(&s.run())["harness"] == "claude" {
            std::fs::read_dir(s.scratch.path().join("stdin")).unwrap().flatten()
                .flat_map(|entry|std::fs::read_to_string(entry.path()).unwrap_or_default().lines()
                    .filter_map(|line|serde_json::from_str::<Value>(line).ok())
                    .filter(|v|v["type"]=="control_response").collect::<Vec<_>>()).collect()
        } else {
            std::fs::read_to_string(ws_path(&s.daemon,&s.created).join("native-pending-answers.jsonl"))
                .unwrap_or_default().lines().map(|line|serde_json::from_str(line).unwrap()).collect()
        }
    }
    fn wait_replies(s: &Script, count: usize) -> Vec<Value> {
        let deadline=Instant::now()+Duration::from_secs(10);
        loop {
            let replies=native_replies(s);
            if replies.len()>=count { return replies; }
            assert!(Instant::now()<deadline,"fixture did not receive {count} native replies");
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    fn response(socket: &Path, message: Value) -> Value {
        let mut conn=UnixStream::connect(socket).unwrap();
        conn.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        conn.set_write_timeout(Some(Duration::from_secs(5))).unwrap();
        writeln!(conn,"{message}").unwrap();
        let mut line=String::new(); BufReader::new(conn).read_line(&mut line).unwrap();
        serde_json::from_str(&line).unwrap()
    }
    fn reject(s: &Script, params: Value, code: &str) {
        let reply=response(&s.daemon.socket(),json!({"id":1,"method":"run.request.answer","params":params}));
        assert_eq!(reply["error"]["code"],code,"intended typed refusal, not an unrelated setup/internal failure: {reply}");
        assert!(native_replies(s).is_empty(),"rejected answer emits no native bytes");
        s.no_protected_action();
    }

    // These gates are startup-only fixture contracts, not method parameters.
    // Corresponding runtime seams remain unimplemented until their baseline is
    // reviewed; reaching a missing API must be reported before a hold timeout.
    struct AnswerGate {
        dir: tempfile::TempDir,
    }
    impl AnswerGate {
        fn new() -> Self { Self { dir: tmp() } }
        fn start(&self, steps: Vec<Value>) -> Script {
            Script::start_with_env("codex-app", "codex-cli 0.158.0", steps, &[
                ("OVERSEER_TEST_NET", "1"),
                ("OVERSEER_CONTINUITY_PROBES", "off"),
                ("OVERSEER_TEST_NATIVE_ANSWER_GATE", self.dir.path().to_str().unwrap()),
            ])
        }
        fn arm(&self, phase: &str, item: &Value) {
            std::fs::write(self.dir.path().join("config.json"),
                json!({"phase":phase,"request_key":item["key"]}).to_string()).unwrap();
        }
        fn reached(&self, phase: &str, item: &Value) -> bool {
            let marker = std::fs::read(self.dir.path().join("reached.json")).ok()
                .and_then(|b| serde_json::from_slice::<Value>(&b).ok());
            let Some(marker) = marker else { return false };
            assert_eq!(marker, json!({"phase":phase,"request_key":item["key"]}),
                "hold marker exposes only the daemon-issued selector and phase");
            true
        }
        fn release(&self) { std::fs::write(self.dir.path().join("release"), "release").unwrap(); }
    }
    impl Drop for AnswerGate {
        fn drop(&mut self) { let _ = std::fs::write(self.dir.path().join("release"), "release"); }
    }
    struct HeldAnswer {
        rx: std::sync::mpsc::Receiver<Value>,
        worker: Option<std::thread::JoinHandle<()>>,
        release: PathBuf,
    }
    impl HeldAnswer {
        fn start(s: &Script, hold: &AnswerGate, item: &Value, answer: Value) -> Self {
            Self::start_call(s,hold,"run.request.answer",answer_params(s,item,answer))
        }
        fn start_call(s: &Script, hold: &AnswerGate, method: &str, params: Value) -> Self {
            let socket = s.daemon.socket();
            let method = method.to_string();
            let (tx, rx) = std::sync::mpsc::channel();
            let worker = std::thread::spawn(move || {
                let mut conn = UnixStream::connect(socket).unwrap();
                conn.set_read_timeout(Some(Duration::from_secs(35))).unwrap();
                conn.set_write_timeout(Some(Duration::from_secs(5))).unwrap();
                writeln!(conn, "{}", json!({"id":1,"method":method,"params":params})).unwrap();
                let mut line = String::new();
                let value = match BufReader::new(conn).read_line(&mut line) {
                    Ok(0) | Err(_) => json!({"disconnected":true}),
                    Ok(_) => serde_json::from_str(&line).unwrap(),
                };
                let _ = tx.send(value);
            });
            Self { rx, worker: Some(worker), release: hold.dir.path().join("release") }
        }
        fn wait_reached(&self, hold: &AnswerGate, phase: &str, item: &Value) {
            let deadline = Instant::now() + Duration::from_secs(10);
            while !hold.reached(phase, item) {
                match self.rx.try_recv() {
                    Ok(reply) => panic!("answer returned before the intended {phase} boundary: {reply}"),
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => panic!("answer worker ended before {phase}"),
                    Err(std::sync::mpsc::TryRecvError::Empty) => {},
                }
                assert!(Instant::now() < deadline, "answer did not reach {phase}");
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        fn finish(&mut self) -> Value {
            let value = self.rx.recv_timeout(Duration::from_secs(10)).expect("released answer disposition");
            self.worker.take().unwrap().join().unwrap();
            value
        }
    }
    impl Drop for HeldAnswer {
        fn drop(&mut self) {
            // Release even while an assertion unwinds, then reap this owned
            // worker before the daemon/test directory can disappear.
            let _ = std::fs::write(&self.release, "release");
            if let Some(worker) = self.worker.take() { let _ = worker.join(); }
        }
    }
    fn assert_claim(s: &Script, item: &Value) -> String {
        let db = rusqlite::Connection::open(s.daemon.home.path().join("overseer.sqlite")).unwrap();
        let (token, digest, lifecycle): (String, String, String) = db.query_row(
            "SELECT delivery_token,answer_digest,lifecycle FROM native_pending_requests WHERE key=?1",
            [item["key"].as_str().unwrap()], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap();
        assert!(token.starts_with("delivery-"));
        assert_eq!(digest.len(), 64);
        assert_eq!(lifecycle, "claimed", "presend marker follows the committed immutable claim");
        token
    }

    #[test]
    fn native_resolution_after_claim_before_send_prevents_reply() {
        let hold = AnswerGate::new();
        let native = tmp(); let native_release = native.path().join("resolve");
        let mut resolved = vector("resolved_integer");
        resolved["params"]["threadId"] = json!("$THREAD");
        let s = hold.start(vec![emit(command(json!(7), "claimed-offer")),
            emit(command(json!(8), "other-offer")), mark(ONE), gate(&native_release), emit(resolved), mark(TWO)]);
        s.marker(ONE, 1); let items = s.requests(); assert_eq!(items.len(), 2);
        hold.arm("claimed_before_send", &items[0]);
        let mut worker = HeldAnswer::start(&s, &hold, &items[0], frozen("command_command_decline")["answer"].clone());
        worker.wait_reached(&hold, "claimed_before_send", &items[0]);
        assert_claim(&s, &items[0]); assert!(native_replies(&s).is_empty());
        std::fs::write(native_release, "release").unwrap(); s.marker(TWO, 1);
        let current = s.requests();
        assert_eq!(current[0]["lifecycle"], "native_resolved", "actual tail resolution wins while answer is held");
        assert_eq!(current[1], items[1], "unrelated pending request is unchanged");
        hold.release(); let reply = worker.finish();
        assert_eq!(reply["error"]["code"], "request_resolved", "{reply}");
        assert!(native_replies(&s).is_empty()); s.no_protected_action();
        assert_eq!(s.daemon.call("run.request.answer", answer_params(&s, &items[1],
            frozen("command_command_decline")["answer"].clone()))["delivery"], "written");
        let mut expected = frozen("command_command_decline")["response"].clone(); expected["id"] = json!(8);
        assert_eq!(wait_replies(&s, 1), vec![expected]);
    }

    struct LostAckProxy {
        socket: PathBuf,
        backing: PathBuf,
        stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
        thread: Option<std::thread::JoinHandle<()>>,
        receipt: std::sync::mpsc::Receiver<Value>,
    }
    impl LostAckProxy {
        fn start(s: &Script) -> Self {
            use std::os::unix::net::UnixListener;
            let db = rusqlite::Connection::open(s.daemon.home.path().join("overseer.sqlite")).unwrap();
            let dir: String = db.query_row("SELECT run_dir FROM runs WHERE id=?1",[s.run()],|r|r.get(0)).unwrap();
            let launch: Value = serde_json::from_slice(&std::fs::read(Path::new(&dir).join("launch.json")).unwrap()).unwrap();
            let socket = PathBuf::from(launch["control_socket"].as_str().unwrap());
            let backing = socket.with_file_name("native-backing.sock");
            std::fs::rename(&socket,&backing).unwrap();
            let listener = UnixListener::bind(&socket).unwrap(); listener.set_nonblocking(true).unwrap();
            let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let stopped = stop.clone(); let target = backing.clone();
            let (tx,receipt) = std::sync::mpsc::channel();
            let thread = std::thread::spawn(move || {
                let deadline = Instant::now()+Duration::from_secs(25); let mut lost = false;
                while !stopped.load(std::sync::atomic::Ordering::SeqCst) && Instant::now()<deadline {
                    let mut client = match listener.accept() {
                        Ok((client,_)) => client,
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {std::thread::sleep(Duration::from_millis(5));continue;},
                        Err(_) => break,
                    };
                    client.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
                    client.set_write_timeout(Some(Duration::from_secs(5))).unwrap();
                    let mut line = String::new();
                    if BufReader::new(client.try_clone().unwrap()).read_line(&mut line).is_err() {break;}
                    let request: Value = serde_json::from_str(&line).unwrap();
                    let reply = response(&target,request.clone());
                    if request["op"] == "request_reply" && !lost {
                        lost = true; let _=tx.send(reply); // Actual backend response is discarded, not fabricated.
                    } else { let _=writeln!(client,"{reply}"); }
                }
            });
            Self {socket,backing,stop,thread:Some(thread),receipt}
        }
        fn written(&self) -> Value {
            let receipt = self.receipt.recv_timeout(Duration::from_secs(10)).expect("proxy observed actual private backend receipt");
            assert_eq!(receipt["state"],"written", "{receipt}"); receipt
        }
    }
    impl Drop for LostAckProxy {
        fn drop(&mut self) {
            self.stop.store(true,std::sync::atomic::Ordering::SeqCst);
            if let Some(thread)=self.thread.take() {let _=thread.join();}
            let _=std::fs::remove_file(&self.socket); let _=std::fs::rename(&self.backing,&self.socket);
        }
    }
    fn denied_count(s: &Script) -> i64 {
        rusqlite::Connection::open(s.daemon.home.path().join("overseer.sqlite")).unwrap()
            .query_row("SELECT COUNT(*) FROM denied_permissions WHERE run_id=?1",[s.run()],|r|r.get(0)).unwrap()
    }

    #[test]
    fn active_claim_collection_reader_cannot_tombstone_held_owner_answer() {
        let hold=AnswerGate::new(); let s=hold.start(vec![emit(command(json!(7),"active-held-claim")),mark(ONE)]);
        s.marker(ONE,1); let item=s.requests()[0].clone(); hold.arm("claimed_before_send",&item);
        let mut worker=HeldAnswer::start(&s,&hold,&item,frozen("command_command_decline")["answer"].clone());
        worker.wait_reached(&hold,"claimed_before_send",&item); let token=assert_claim(&s,&item);
        let collection=s.collection(); assert_eq!(collection["requests"][0]["lifecycle"],"claimed");
        assert_eq!(s.collection(),collection,"a reader cannot settle an active owner's held claim");
        let db=rusqlite::Connection::open(s.daemon.home.path().join("overseer.sqlite")).unwrap();
        let dir:String=db.query_row("SELECT run_dir FROM runs WHERE id=?1",[s.run()],|r|r.get(0)).unwrap();
        assert!(!Path::new(&dir).join("native-receipts").join(format!("{token}.json")).exists(),"active claim read must not create a not-written tombstone");
        assert!(native_replies(&s).is_empty()); assert_eq!(denied_count(&s),0);
        hold.release(); let reply=worker.finish(); assert_eq!(reply["result"]["delivery"],"written","{reply}");
        assert_eq!(wait_replies(&s,1),vec![frozen("command_command_decline")["response"].clone()]);
        assert_eq!(denied_count(&s),1); s.no_protected_action();
    }

    #[test]
    fn lost_ack_reconciles_written_without_resend_or_rewriting_first_receipt() {
        let s = Script::start("codex-app","codex-cli 0.158.0",vec![emit(command(json!(7),"lost-ack-denial")),mark(ONE)]);
        s.marker(ONE,1); let item = s.requests()[0].clone(); let proxy = LostAckProxy::start(&s);
        let first = s.daemon.call("run.request.answer",answer_params(&s,&item,frozen("command_command_decline")["answer"].clone()));
        assert_eq!(first["delivery"],"uncertain","client lost acknowledgement despite an actual native write");
        let receipt = proxy.written(); assert_eq!(wait_replies(&s,1),vec![frozen("command_command_decline")["response"].clone()]);
        assert_eq!(denied_count(&s),0,"uncertain delivery alone is not a written owner denial");
        let db = rusqlite::Connection::open(s.daemon.home.path().join("overseer.sqlite")).unwrap();
        let original: String = db.query_row("SELECT result FROM native_answer_attempts WHERE delivery_token=?1",
            [receipt["delivery_token"].as_str().unwrap()],|r|r.get(0)).unwrap();
        assert_eq!(serde_json::from_str::<Value>(&original).unwrap(),first);
        let current = s.requests()[0].clone(); assert_eq!(current["lifecycle"],"answered_awaiting_native",
            "collection queries the real receipt without resending answer data");
        assert_eq!(denied_count(&s),1,"recovery-confirmed written denial is remembered exactly once");
        assert_eq!(s.requests()[0],current,"reconciliation is idempotent"); assert_eq!(denied_count(&s),1);
        let saved: String = db.query_row("SELECT result FROM native_answer_attempts WHERE delivery_token=?1",
            [receipt["delivery_token"].as_str().unwrap()],|r|r.get(0)).unwrap(); assert_eq!(saved,original,"first attempt result is immutable");
        assert_eq!(native_replies(&s).len(),1); drop(proxy); s.no_protected_action();
    }

    #[test]
    fn daemon_crash_after_claim_queries_tombstone_before_new_explicit_answer() {
        let hold = AnswerGate::new(); let mut s = hold.start(vec![emit(command(json!(7),"crashed-claim")),mark(ONE)]);
        s.marker(ONE,1); let item=s.requests()[0].clone(); let generation=s.daemon.run(&s.run())["process_generation"].clone();
        hold.arm("claimed_before_send",&item);
        let mut worker = HeldAnswer::start(&s,&hold,&item,frozen("command_command_decline")["answer"].clone());
        worker.wait_reached(&hold,"claimed_before_send",&item); let token=assert_claim(&s,&item);
        assert!(native_replies(&s).is_empty()); assert_eq!(denied_count(&s),0);
        s.daemon.kill9(); assert_eq!(worker.finish()["disconnected"],true); hold.release(); s.daemon.spawn();
        assert_eq!(s.daemon.run(&s.run())["process_generation"],generation,"surviving owned shim is reattached, not replayed");
        let current=s.requests()[0].clone(); assert_eq!(current["lifecycle"],"pending");
        assert_eq!(current["reason_code"],"definitely_unsent","authoritative missing receipt creates terminal not-written tombstone");
        assert_ne!(current["revision"],item["revision"]); assert_eq!(denied_count(&s),0);
        assert!(native_replies(&s).is_empty(),"recovery cannot reconstruct/replay private answer bytes");
        let db=rusqlite::Connection::open(s.daemon.home.path().join("overseer.sqlite")).unwrap();
        let dir:String=db.query_row("SELECT run_dir FROM runs WHERE id=?1",[s.run()],|r|r.get(0)).unwrap();
        let tombstone:Value=serde_json::from_slice(&std::fs::read(Path::new(&dir).join("native-receipts").join(format!("{token}.json"))).unwrap()).unwrap();
        assert_eq!(tombstone["state"],"not_written");
        let result=s.daemon.call("run.request.answer",answer_params(&s,&current,frozen("command_command_decline")["answer"].clone()));
        assert_eq!(result["delivery"],"written"); assert_eq!(wait_replies(&s,1),vec![frozen("command_command_decline")["response"].clone()]);
        assert_eq!(denied_count(&s),1); s.no_protected_action();
    }

    fn captured_needs_cannot_rebind(words: &str) {
        let hold = AnswerGate::new(); let native = tmp(); let native_release = native.path().join("replace-ask");
        let mut resolved = vector("resolved_integer"); resolved["params"]["threadId"] = json!("$THREAD");
        let s = hold.start(vec![emit(command(json!(7),"captured-A")),mark(ONE),gate(&native_release),
            emit(resolved),emit(command(json!(8),"replacement-B")),mark(TWO)]);
        s.marker(ONE,1); let old = s.requests()[0].clone(); hold.arm("needs_captured",&old);
        let mut worker = HeldAnswer::start_call(&s,&hold,"overseer.send",json!({"text":words,"surface":"vscode"}));
        worker.wait_reached(&hold,"needs_captured",&old);
        std::fs::write(native_release,"release").unwrap(); s.marker(TWO,1);
        let current = s.requests().into_iter().find(|r|r["lifecycle"] == "pending").unwrap();
        assert_ne!(current["key"],old["key"]); hold.release(); let reply = worker.finish();
        assert!(native_replies(&s).is_empty(),"old Needs words must not approve replacement B: {reply}");
        let reported = serde_json::to_string(&reply).unwrap();
        assert!(reported.contains("stale_request"),"captured Needs identity is refused explicitly, not silently recaptured: {reply}");
        assert_eq!(s.daemon.call("run.request.answer",answer_params(&s,&current,
            frozen("command_command_decline")["answer"].clone()))["delivery"],"written");
        let mut expected = frozen("command_command_decline")["response"].clone(); expected["id"] = json!(8);
        assert_eq!(wait_replies(&s,1),vec![expected]); s.no_protected_action();
    }
    #[test]
    fn captured_needs_yes_does_not_approve_replacement_native_request() { captured_needs_cannot_rebind("tell it yes"); }
    #[test]
    fn captured_needs_handle_does_not_propose_a_new_native_request() { captured_needs_cannot_rebind("handle what needs me"); }

    #[test]
    fn needs_handle_reports_stale_preparation_after_initial_capture_recheck() {
        let hold=AnswerGate::new(); let native=tmp(); let release_native=native.path().join("replace-after-check");
        let mut resolved=vector("resolved_integer"); resolved["params"]["threadId"]=json!("$THREAD");
        let s=hold.start(vec![emit(command(json!(7),"handle-captured-A")),mark(ONE),gate(&release_native),
            emit(resolved),emit(command(json!(8),"handle-replacement-B")),mark(TWO)]);
        s.marker(ONE,1); let old=s.requests()[0].clone();
        // Cancel only existing host proposals, leaving native A waiting so
        // Handle must prepare its checked proposal after the capture recheck.
        let session=s.daemon.call("overseer.session",json!({}));
        for proposal in session["proposals"].as_array().into_iter().flatten().filter(|p|p["state"] == "open") {
            s.daemon.call("overseer.cancel",json!({"id":proposal["id"],"by":"owner"}));
        }
        assert!(native_replies(&s).is_empty()); hold.arm("needs_preparing",&old);
        let mut worker=HeldAnswer::start_call(&s,&hold,"overseer.send",json!({"text":"handle what needs me","surface":"vscode"}));
        worker.wait_reached(&hold,"needs_preparing",&old);
        std::fs::write(release_native,"release").unwrap(); s.marker(TWO,1); hold.release();
        let reply=worker.finish(); assert!(serde_json::to_string(&reply).unwrap().contains("stale_request"),
            "preparation failure is visible, not old A Allow-it text with proposal:null: {reply}");
        assert!(native_replies(&s).is_empty(),"Handle is a question, not an approval");
        let current=s.requests().into_iter().find(|r|r["lifecycle"] == "pending").unwrap();
        assert_eq!(s.daemon.call("run.request.answer",answer_params(&s,&current,
            frozen("command_command_decline")["answer"].clone()))["delivery"],"written");
        let mut expected=frozen("command_command_decline")["response"].clone();expected["id"]=json!(8);
        assert_eq!(wait_replies(&s,1),vec![expected]);s.no_protected_action();
    }

    #[test]
    fn written_typed_native_declines_preserve_once_only_no_workaround_ledger() {
        for compatibility in [false,true] {
            let s = Script::start("codex-app","codex-cli 0.158.0",vec![emit(command(json!(7),"owner-denied-command")),mark(ONE)]);
            s.marker(ONE,1); let item = s.requests()[0].clone();
            let receipt = if compatibility {
                s.daemon.call("run.permission",json!({"run_id":s.run(),"request_id":item["key"],"revision":item["revision"],"allow":false}))
            } else { s.daemon.call("run.request.answer",answer_params(&s,&item,frozen("command_command_decline")["answer"].clone())) };
            assert_eq!(receipt["delivery"],"written"); assert_eq!(wait_replies(&s,1),vec![frozen("command_command_decline")["response"].clone()]);
            let db = rusqlite::Connection::open(s.daemon.home.path().join("overseer.sqlite")).unwrap();
            let count: i64 = db.query_row("SELECT COUNT(*) FROM denied_permissions WHERE run_id=?1 AND detail=?2",
                rusqlite::params![s.run(),"touch protected-action.txt"],|r|r.get(0)).unwrap();
            assert_eq!(count,1,"an actually written owner decline is remembered, exactly once");
            let other = s.daemon.call("task.create",json!({"repo":s.repo,"harness":"codex-app","prompt":"synthetic target","title":"Reroute target"}));
            let refused = s.daemon.try_call("overseer.propose",json!({"source":"test","actions":[{"action":"message","agent":run_id(&other),"text":"Please touch protected-action.txt","why":"attempted reroute"}]})).unwrap_err();
            assert!(refused.contains("owner denied"),"denied_match continues protecting a different agent: {refused}");
            reject(&s,answer_params(&s,&item,frozen("command_command_decline")["answer"].clone()),"already_answered");
            let count: i64 = db.query_row("SELECT COUNT(*) FROM denied_permissions WHERE run_id=?1 AND detail=?2",
                rusqlite::params![s.run(),"touch protected-action.txt"],|r|r.get(0)).unwrap();
            assert_eq!(count,1); s.no_protected_action();
        }
    }

    #[test]
    fn typed_bool_bridge_requires_frozen_revision_and_keeps_exact_native_decline() {
        let (s,v) = start_vector("command_command_decline"); let item = s.requests()[0].clone();
        let params = json!({"run_id":s.run(),"request_id":item["key"],"allow":false});
        let missing = response(&s.daemon.socket(), json!({"id":1,"method":"run.permission","params":params}));
        assert_eq!(missing["error"]["code"], "invalid_params", "typed compatibility needs a frozen revision: {missing}");
        assert!(native_replies(&s).is_empty());
        let mut stale = params.clone(); stale["revision"] = json!(item["revision"].as_i64().unwrap()+1);
        let rejected = response(&s.daemon.socket(), json!({"id":1,"method":"run.permission","params":stale}));
        assert_eq!(rejected["error"]["code"], "stale_request", "{rejected}");
        let mut valid = params; valid["revision"] = item["revision"].clone();
        let receipt = s.daemon.call("run.permission", valid);
        assert_eq!(receipt["delivery"], "written");
        assert_eq!(receipt["lifecycle"], "answered_awaiting_native", "transport acceptance is not native completion");
        assert_eq!(wait_replies(&s,1), vec![v["response"].clone()]); s.no_protected_action();
    }

    #[test]
    fn checked_overseer_confirmation_freezes_daemon_revision_and_exact_native_item() {
        let (s,v) = start_vector("command_command_decline"); let item = s.requests()[0].clone();
        let proposal = s.daemon.call("overseer.propose", json!({"source":"test","actions":[{
            "action":"permission","agent":s.run(),"request":"model-spoofed-key","revision":918273,
            "allow_request":false,"why":"synthetic owner confirmation"}]}));
        assert_eq!(proposal["state"], "open", "permission always requires owner confirmation");
        let db = rusqlite::Connection::open(s.daemon.home.path().join("overseer.sqlite")).unwrap();
        let stored: String = db.query_row("SELECT actions FROM overseer_proposals WHERE id=?1",
            [proposal["proposal"].as_str().unwrap()], |r|r.get(0)).unwrap();
        let stored: Value = serde_json::from_str(&stored).unwrap();
        assert_eq!(stored[0]["request"], item["key"], "model selectors are replaced with checked daemon identity");
        assert_eq!(stored[0]["revision"], item["revision"]);
        assert!(native_replies(&s).is_empty());
        let result = s.daemon.call("overseer.answer", json!({"id":proposal["proposal"],"yes":true,"surface":"spoofed voice","by":"spoofed actor"}));
        assert_eq!(result["state"], "yes", "{result}");
        assert_eq!(wait_replies(&s,1), vec![v["response"].clone()]);
        let current = s.requests()[0].clone();
        assert_eq!(current["actor"]["origin"], "confirmed_overseer");
        assert_eq!(current["actor"]["proposal"], proposal["proposal"]);
        assert_eq!(current["lifecycle"], "answered_awaiting_native"); s.no_protected_action();
    }

    #[test]
    fn voice_checked_readback_keeps_typed_revision_and_exact_native_decline() {
        let mut frame = frozen("command_command_decline")["request"].clone();
        frame["params"]["threadId"] = json!("$THREAD"); frame["params"]["turnId"] = json!("$TURN");
        let s = Script::start_with_env("codex-app", "codex-cli 0.158.0", vec![emit(frame),mark(ONE)],
            &[("OVERSEER_VOICE_SIMULATE","1")]);
        s.marker(ONE,1); let item = s.requests()[0].clone();
        s.daemon.call("voice.set", json!({"enabled":true,"permission_answers":true,"settle_seconds":1}));
        let read = s.daemon.call("voice.read_back", json!({}));
        assert_eq!(read["read_back"]["request"], item["key"]);
        assert_eq!(read["read_back"]["revision"], item["revision"], "readback freezes the exact typed offer");
        s.daemon.call("voice.answer", json!({"yes":false}));
        assert_eq!(wait_replies(&s,1), vec![frozen("command_command_decline")["response"].clone()]);
        let current = s.requests()[0].clone(); assert_eq!(current["actor"]["origin"], "confirmed_voice");
        assert_eq!(current["lifecycle"], "answered_awaiting_native"); s.no_protected_action();
    }

    #[test]
    fn native_turn_rollover_same_generation_retires_held_answer_and_keeps_current_control() {
        let hold = AnswerGate::new();
        let native = tmp(); let release_native = native.path().join("next-turn");
        let mut next = command(json!(8), "current-turn-offer");
        next["params"]["turnId"] = json!("fixture-native-turn-B");
        let s = hold.start(vec![emit(command(json!(7), "old-turn-offer")), mark(ONE),
            gate(&release_native),
            emit(json!({"method":"turn/started","params":{"threadId":"$THREAD","turn":{"id":"fixture-native-turn-B"}}})),
            emit(next), mark(TWO)]);
        s.marker(ONE, 1); let old = s.requests()[0].clone();
        let generation = s.daemon.run(&s.run())["process_generation"].clone();
        hold.arm("claimed_before_send", &old);
        let mut worker = HeldAnswer::start(&s, &hold, &old, frozen("command_command_decline")["answer"].clone());
        worker.wait_reached(&hold, "claimed_before_send", &old); assert_claim(&s, &old);
        std::fs::write(release_native, "release").unwrap(); s.marker(TWO, 1);
        assert_eq!(s.daemon.run(&s.run())["process_generation"], generation,
            "real native turn rollover occurs within the same process");
        let collection = s.requests();
        assert_eq!(collection[0]["lifecycle"], "native_resolved", "turn A becomes unanswerable on accepted turn B");
        let current = collection.iter().find(|r| r["lifecycle"] == "pending").unwrap().clone();
        assert_ne!(current["key"], old["key"]);
        hold.release(); let reply = worker.finish();
        assert_eq!(reply["error"]["code"], "request_resolved", "{reply}");
        assert!(native_replies(&s).is_empty(), "held A must not write under B");
        assert_eq!(s.daemon.call("run.request.answer", answer_params(&s, &current,
            frozen("command_command_decline")["answer"].clone()))["delivery"], "written");
        let mut expected = frozen("command_command_decline")["response"].clone(); expected["id"] = json!(8);
        assert_eq!(wait_replies(&s, 1), vec![expected]); s.no_protected_action();
    }

    #[test]
    fn native_turn_completion_retires_held_answer_before_process_settlement() {
        let hold = AnswerGate::new();
        let native = tmp(); let release_native = native.path().join("complete-turn");
        let s = hold.start(vec![emit(command(json!(7), "completed-turn-offer")), mark(ONE),
            gate(&release_native), emit(json!({"method":"turn/completed","params":{"threadId":"$THREAD",
                "turn":{"id":"$TURN","status":"completed","error":null}}}))]);
        s.marker(ONE, 1); let old = s.requests()[0].clone(); hold.arm("claimed_before_send", &old);
        let mut worker = HeldAnswer::start(&s, &hold, &old, frozen("command_command_decline")["answer"].clone());
        worker.wait_reached(&hold, "claimed_before_send", &old); assert_claim(&s, &old);
        std::fs::write(release_native, "release").unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        let events = loop {
            let events = s.daemon.events(&s.run());
            if events.iter().any(|e| e["kind"] == "turn_done") { break events; }
            assert!(Instant::now() < deadline, "actual native completion was not applied");
            std::thread::sleep(Duration::from_millis(10));
        };
        let changed = events.iter().find(|e| e["kind"] == "pending_request_changed"
            && e["payload"]["key"] == old["key"] && e["payload"]["lifecycle"] == "native_resolved")
            .expect("native completion retires authority independently of later supervisor exit");
        let done = events.iter().find(|e| e["kind"] == "turn_done").unwrap();
        assert!(changed["seq"].as_i64().unwrap() < done["seq"].as_i64().unwrap());
        hold.release(); let reply = worker.finish();
        assert_eq!(reply["error"]["code"], "request_resolved", "{reply}");
        assert!(native_replies(&s).is_empty()); s.no_protected_action();
    }

    #[test]
    fn replacement_generation_after_claim_before_send_rejects_old_attempt() {
        let hold = AnswerGate::new();
        let s = hold.start(vec![emit(command(json!(7), "old-offer")), mark(ONE)]);
        s.marker(ONE, 1); let old = s.requests()[0].clone();
        let generation = s.daemon.run(&s.run())["process_generation"].clone();
        hold.arm("claimed_before_send", &old);
        let mut worker = HeldAnswer::start(&s, &hold, &old, frozen("command_command_decline")["answer"].clone());
        worker.wait_reached(&hold, "claimed_before_send", &old); assert_claim(&s, &old);
        s.daemon.call("run.interrupt", json!({"run_id":s.run()})); s.daemon.wait_done(&s.run(), 20);
        let queued = s.daemon.call("run.follow_up", json!({"run_id":s.run(),"prompt":"explicit replacement"}));
        assert_eq!(queued["delivery"], "queued");
        assert_eq!(s.daemon.run(&s.run())["queue"]["paused"], true, "Stop cannot resume a held answer or followup");
        s.daemon.call("run.resume_queue", json!({"run_id":s.run()})); s.marker(ONE, 2);
        assert_ne!(s.daemon.run(&s.run())["process_generation"], generation);
        let current = s.requests().into_iter().find(|r| r["lifecycle"] == "pending").unwrap();
        assert_ne!(current["key"], old["key"]);
        hold.release(); let reply = worker.finish();
        assert_eq!(reply["error"]["code"], "stale_generation", "{reply}");
        assert!(native_replies(&s).is_empty()); s.no_protected_action();
        assert_eq!(s.daemon.call("run.request.answer", answer_params(&s, &current,
            frozen("command_command_decline")["answer"].clone()))["delivery"], "written");
        assert_eq!(wait_replies(&s, 1), vec![frozen("command_command_decline")["response"].clone()]);
    }

    #[tokio::test(flavor="multi_thread", worker_threads=2)]
    async fn full_device_downgrade_before_claim_cannot_send() {
        let hold = AnswerGate::new();
        let s = hold.start(vec![emit(command(json!(7), "device-offer")), mark(ONE)]);
        s.marker(ONE, 1); let item = s.requests()[0].clone(); hold.arm("validated_before_claim", &item);
        common::phone::enable(&s.daemon);
        let (mut phone, paired) = common::phone::pair(&s.daemon, "Held full fixture").await;
        let params = answer_params(&s, &item, frozen("command_command_decline")["answer"].clone());
        let mut work = tokio::spawn(async move { let reply = phone.act("run.request.answer", params).await; (phone, reply) });
        let deadline = Instant::now() + Duration::from_secs(10);
        while !hold.reached("validated_before_claim", &item) {
            if work.is_finished() {
                let (_, reply) = (&mut work).await.unwrap();
                panic!("full-device answer returned before validation hold: {reply}");
            }
            assert!(Instant::now() < deadline, "full-device answer did not reach validation hold");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(s.requests(), vec![item.clone()], "preclaim hold has not accepted or revised the item");
        let changed = s.daemon.call("gateway.device_scope", json!({"id":paired.device,"scope":"watch"}));
        assert_eq!(changed["scope"], "watch", "actual owner mutation acknowledged before release");
        hold.release(); let (_, reply) = work.await.unwrap();
        assert_eq!(common::phone::Phone::code(&reply), "watch_only", "{reply}");
        assert!(native_replies(&s).is_empty()); assert_eq!(s.requests(), vec![item.clone()]);
        assert_eq!(s.daemon.call("run.request.answer", answer_params(&s, &item,
            frozen("command_command_decline")["answer"].clone()))["delivery"], "written");
        assert_eq!(wait_replies(&s, 1), vec![frozen("command_command_decline")["response"].clone()]);
        s.no_protected_action();
    }

    async fn held_device_revocation(phase: &str) {
        let hold = AnswerGate::new();
        let s = hold.start(vec![emit(command(json!(7), "revoked-offer")), mark(ONE)]);
        s.marker(ONE, 1); let item = s.requests()[0].clone(); hold.arm(phase, &item);
        common::phone::enable(&s.daemon);
        let (mut phone, paired) = common::phone::pair(&s.daemon, "Revoked held fixture").await;
        let rid = common::phone::uuid();
        phone.send(&json!({"id":777,"method":"run.request.answer","request_id":rid,
            "params":answer_params(&s, &item, frozen("command_command_decline")["answer"].clone())})).await.unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !hold.reached(phase, &item) {
            if let Some(reply) = phone.next(Duration::from_millis(10)).await {
                if reply["id"] == 777 { panic!("phone answer returned before {phase}: {reply}"); }
                phone.inbox.push(reply);
            }
            assert!(Instant::now() < deadline, "phone answer did not reach {phase}");
        }
        if phase == "claimed_before_send" { assert_claim(&s, &item); }
        else { assert_eq!(s.requests(), vec![item.clone()]); }
        let revoked = s.daemon.call("gateway.device_revoke", json!({"id":paired.device}));
        assert_eq!(revoked["revoked"], true, "revocation acknowledged while the answer cannot send");
        hold.release(); assert!(phone.ends_within(Duration::from_secs(5)).await);
        // Revocation closes the real phone, so inspect its existing private
        // once-cache for the immutable refusal rather than fabricate a reply.
        let db = rusqlite::Connection::open(s.daemon.home.path().join("overseer.sqlite")).unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        let cached = loop {
            let value: Result<String, _> = db.query_row(
                "SELECT reply FROM remote_requests WHERE device_id=?1 AND request_id=?2",
                rusqlite::params![paired.device, rid], |r| r.get(0));
            if let Ok(value) = value { break serde_json::from_str::<Value>(&value).unwrap(); }
            assert!(Instant::now() < deadline, "revoked attempt has no final once-cache disposition");
            tokio::time::sleep(Duration::from_millis(10)).await;
        };
        assert_eq!(cached["error"]["code"], "revoked", "{cached}");
        assert!(native_replies(&s).is_empty()); s.no_protected_action();
        let pending = s.requests()[0].clone();
        assert_eq!(pending["key"], item["key"]); assert_eq!(pending["lifecycle"], "pending");
        if phase == "claimed_before_send" {
            assert!(pending["revision"].as_i64().unwrap() > item["revision"].as_i64().unwrap(),
                "a definitely-unsent restored item requires a fresh explicit owner action");
        }
        assert_eq!(s.daemon.call("run.request.answer", answer_params(&s, &pending,
            frozen("command_command_decline")["answer"].clone()))["delivery"], "written");
        assert_eq!(wait_replies(&s, 1), vec![frozen("command_command_decline")["response"].clone()]);
        let unchanged: String = db.query_row(
            "SELECT reply FROM remote_requests WHERE device_id=?1 AND request_id=?2",
            rusqlite::params![paired.device, rid], |r| r.get(0)).unwrap();
        assert_eq!(serde_json::from_str::<Value>(&unchanged).unwrap(), cached,
            "later local action does not rewrite the phone's first disposition");
    }

    #[tokio::test(flavor="multi_thread", worker_threads=2)]
    async fn revoked_full_device_before_claim_cannot_send() {
        held_device_revocation("validated_before_claim").await;
    }

    #[tokio::test(flavor="multi_thread", worker_threads=2)]
    async fn device_revocation_after_claim_before_send_prevents_held_reply() {
        held_device_revocation("claimed_before_send").await;
    }

    #[test]
    fn absent_receipt_status_tombstone_refuses_delayed_original_after_retry() {
        let (s,v) = start_vector("command_command_decline");
        let db = rusqlite::Connection::open(s.daemon.home.path().join("overseer.sqlite")).unwrap();
        let (dir,generation):(String,i64) = db.query_row(
            "SELECT run_dir,process_generation FROM runs WHERE id=?1",[s.run()],|r|Ok((r.get(0)?,r.get(1)?))).unwrap();
        let launch:Value = serde_json::from_slice(&std::fs::read(Path::new(&dir).join("launch.json")).unwrap()).unwrap();
        let socket = PathBuf::from(launch["control_socket"].as_str().unwrap());
        let data = format!("{}\n",v["response"]);
        let digest = format!("{:x}",Sha256::digest(data.as_bytes()));
        let old = format!("delivery-{}",uuid::Uuid::new_v4().simple());
        let original = json!({"op":"request_reply","generation":generation,
            "delivery_token":old,"answer_digest":digest,"data":data});
        // The original connection exists before the status query, but its first
        // request bytes are held by the test. This is an actual delayed client.
        let mut delayed = UnixStream::connect(&socket).unwrap();
        delayed.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        delayed.set_write_timeout(Some(Duration::from_secs(5))).unwrap();
        let status = response(&socket,json!({"op":"request_reply_status","generation":generation,
            "delivery_token":old,"answer_digest":digest}));
        assert_eq!(status["state"],"not_written","authoritative absent receipt disposition: {status}");
        assert!(native_replies(&s).is_empty());
        let retry = format!("delivery-{}",uuid::Uuid::new_v4().simple());
        let mut retried = original.clone(); retried["delivery_token"] = json!(retry);
        assert_eq!(response(&socket,retried)["state"],"written");
        assert_eq!(wait_replies(&s,1),vec![v["response"].clone()]);
        writeln!(delayed,"{original}").unwrap();
        let mut line=String::new(); BufReader::new(delayed).read_line(&mut line).unwrap();
        let late:Value=serde_json::from_str(&line).unwrap();
        assert_eq!(late,status,"query tombstone terminally prevents the original delayed token from writing");
        assert_eq!(native_replies(&s),vec![v["response"].clone()]);
        let durable:Value=serde_json::from_slice(&std::fs::read(Path::new(&dir)
            .join("native-receipts").join(format!("{old}.json"))).unwrap()).unwrap();
        assert_eq!(durable["state"],"not_written","no restoration authority without a durable token tombstone");
        s.no_protected_action();
    }

    #[test]
    fn native_token_tool_cannot_inherit_local_owner_transport_authority() {
        let probe=tmp();
        let s=Script::start_with_env("codex-app","codex-cli 0.158.0",
            vec![emit(command(json!(7),"native-tool-offer")),mark(ONE)], &[
                ("OVERSEER_TEST_NET","1"),("OVERSEER_CONTINUITY_PROBES","off"),
                ("OVERSEER_TEST_NATIVE_TOOL_AUTHORITY_PROBE",probe.path().to_str().unwrap()),
            ]);
        s.marker(ONE,1); let before=s.requests();
        let token=s.daemon.call("overseer.token",json!({"run_id":s.run(),"role":"overseer"}));
        let result=s.daemon.call("overseer.tool",json!({"token":token["token"],"name":"roster",
            "arguments":{"actor":"the Mac","surface":"owner","proposal":"accepted","request_key":before[0]["key"]}}));
        assert!(result["text"].is_string(),"the actual authenticated native tool executed");
        let observed:Value=serde_json::from_slice(&std::fs::read(probe.path().join("authority.json")).unwrap()).unwrap();
        assert_eq!(observed,json!({"authority":"none"}),
            "native token/model fields must not inherit transport LocalOwner at the tool entrance");
        assert_eq!(s.requests(),before); assert!(native_replies(&s).is_empty()); s.no_protected_action();
    }

    #[test]
    fn competing_answer_claims_emit_one_exact_native_response() {
        let (s,v)=start_vector("command_command_decline");
        let item=s.requests()[0].clone();
        let socket=s.daemon.socket();
        let barrier=Arc::new(Barrier::new(3));
        let mut threads=Vec::new();
        for decision in ["decline","cancel"] {
            let path=socket.clone(); let barrier=barrier.clone();
            let params=answer_params(&s,&item,json!({"kind":"decision","decision":decision}));
            threads.push(std::thread::spawn(move || {
                barrier.wait(); response(&path,json!({"id":1,"method":"run.request.answer","params":params}))
            }));
        }
        barrier.wait(); let results:Vec<Value>=threads.into_iter().map(|t|t.join().unwrap()).collect();
        assert_eq!(results.iter().filter(|r|r.get("result").is_some()).count(),1,"one accepted owner claim: {results:?}");
        assert_eq!(results.iter().filter(|r|r["error"]["code"]=="already_answered").count(),1);
        let replies=wait_replies(&s,1); assert_eq!(replies.len(),1);
        let cancel=frozen("command_command_cancel")["response"].clone();
        assert!(replies[0]==v["response"] || replies[0]==cancel,"exact frozen native ID/response");
        s.no_protected_action();
    }

    #[test]
    fn frozen_typed_family_answers_reach_exact_native_transport() {
        for name in ["file_decline","permissions_empty_deny","codex_question_id","codex_form_accept",
            "claude_deny","claude_question_text","claude_form_accept"] {
            let (s,v)=start_vector(name); let item=s.requests()[0].clone(); let mut answer=v["answer"].clone();
            if answer["kind"]=="questions" {
                let values=answer["answers"].as_object().unwrap().values().next().unwrap().clone();
                answer["answers"]=json!({"field-0":values});
            }
            let result=s.daemon.call("run.request.answer",answer_params(&s,&item,answer));
            assert_eq!(result["request_key"],item["key"]); assert_eq!(result["delivery"],"written");
            assert_eq!(result["lifecycle"],"answered_awaiting_native","write ack is not native resolution");
            assert_eq!(wait_replies(&s,1),vec![v["response"].clone()],"frozen response: {name}");
            assert_eq!(s.requests()[0]["lifecycle"],"answered_awaiting_native");
            s.no_protected_action();
        }
    }

    #[test]
    fn wrong_family_revision_and_caller_authority_emit_no_response() {
        let (s,_)=start_vector("command_command_decline"); let item=s.requests()[0].clone();
        reject(&s,answer_params(&s,&item,json!({"kind":"questions","answers":{"field-0":["A"]}})),"invalid_answer");
        let mut stale=answer_params(&s,&item,json!({"kind":"decision","decision":"decline"}));
        stale["revision"]=json!(item["revision"].as_i64().unwrap()+1); reject(&s,stale,"stale_request");
        for field in ["actor","surface","process_generation","native_id","protocol","offer_digest"] {
            let mut forged=answer_params(&s,&item,json!({"kind":"decision","decision":"decline"}));
            forged[field]=json!("owner"); reject(&s,forged,"invalid_params");
        }
        assert_eq!(s.requests(),vec![item.clone()],"no rejected answer changes authority or revision");
        let result=s.daemon.call("run.request.answer",answer_params(&s,&item,frozen("command_command_decline")["answer"].clone()));
        assert_eq!(result["delivery"],"written");
        assert_eq!(wait_replies(&s,1),vec![frozen("command_command_decline")["response"].clone()]);
    }

    #[test]
    fn permission_profile_widening_and_deny_removal_cannot_claim() {
        let (s,_)=start_vector("permissions_turn"); let item=s.requests()[0].clone();
        for name in ["permissions_drop_deny","permissions_widen_glob"] {
            reject(&s,answer_params(&s,&item,frozen(name)["answer"].clone()),"scope_enlarged");
        }
        assert_eq!(s.requests(),vec![item.clone()]);
        let valid=frozen("permissions_restricted_subset");
        assert_eq!(s.daemon.call("run.request.answer",answer_params(&s,&item,valid["answer"].clone()))["delivery"],"written");
        assert_eq!(wait_replies(&s,1),vec![valid["response"].clone()]);
    }

    #[test]
    fn persistent_native_veto_refuses_stale_always_but_allows_once() {
        let (s,v)=start_vector("claude_veto_allow_once"); let item=s.requests()[0].clone();
        assert_eq!(item["suppress_always"],true);
        reject(&s,answer_params(&s,&item,frozen("claude_session")["answer"].clone()),"native_veto");
        let result=s.daemon.call("run.request.answer",answer_params(&s,&item,v["answer"].clone()));
        assert_eq!(result["delivery"],"written"); assert_eq!(wait_replies(&s,1),vec![v["response"].clone()]);
    }

    #[test]
    fn structured_questions_and_forms_refuse_legacy_bool() {
        for name in ["codex_question_id","permissions_turn","claude_form_accept"] {
            let (s,v)=start_vector(name); let item=s.requests()[0].clone();
            let error=s.daemon.try_call("run.permission",json!({"run_id":s.run(),"request_id":item["key"],"allow":true})).unwrap_err();
            assert!(!error.contains("unknown method")); assert!(native_replies(&s).is_empty());
            // The actual typed path must also exist; a legacy refusal alone
            // cannot pass this new typed-answer qualification on Slice1.
            let answer=if v["answer"]["kind"]=="questions" {json!({"kind":"questions","answers":{"field-0":["A"]}})} else {v["answer"].clone()};
            assert_eq!(s.daemon.call("run.request.answer",answer_params(&s,&item,answer))["delivery"],"written");
        }
    }

    #[test]
    fn native_resolution_before_owner_answer_emits_no_response() {
        let mut resolved=vector("resolved_integer"); resolved["params"]["threadId"]=json!("$THREAD");
        let barrier=tmp(); let gate_path=barrier.path().join("resolve");
        let s=Script::start("codex-app","codex-cli 0.158.0",vec![emit(command(json!(7),"held")),mark(ONE),gate(&gate_path),emit(resolved),mark(TWO)]);
        s.marker(ONE,1); let item=s.requests()[0].clone();
        std::fs::write(&gate_path,"release").unwrap(); s.marker(TWO,1);
        let resolved=s.requests();
        assert_eq!(resolved.len(),1,"terminal projection is retained in its generation");
        assert_eq!(resolved[0]["key"],item["key"]);
        assert_eq!(resolved[0]["lifecycle"],"native_resolved","actual native resolution was applied before answer");
        let state=s.daemon.call("state",json!({}));
        let run=state["runs"].as_array().unwrap().iter().find(|r|r["id"]==s.run()).unwrap();
        assert_eq!(run["pending_count"],0); assert!(run["attention"].is_null());
        reject(&s,answer_params(&s,&item,json!({"kind":"decision","decision":"accept"})),"request_resolved");
    }

    #[tokio::test(flavor="multi_thread",worker_threads=2)]
    async fn authenticated_full_watch_and_revoked_devices_do_not_borrow_actor_text() {
        let (s,v)=start_vector("command_command_decline"); let item=s.requests()[0].clone();
        common::phone::enable(&s.daemon);
        let (mut full,full_pair)=common::phone::pair(&s.daemon,"Full fixture").await;
        let (mut watch,watch_pair)=common::phone::pair(&s.daemon,"Watch fixture").await;
        s.daemon.call("gateway.device_scope",json!({"id":watch_pair.device,"scope":"watch"}));
        let params=answer_params(&s,&item,v["answer"].clone());
        let refused=watch.act("run.request.answer",params.clone()).await;
        assert_eq!(common::phone::Phone::code(&refused),"watch_only");
        let mut forged=params.clone(); forged["actor"]=json!("the Mac"); forged["surface"]=json!("owner");
        let refused=full.act("run.request.answer",forged).await;
        assert_eq!(common::phone::Phone::code(&refused),"invalid_params");
        assert!(native_replies(&s).is_empty());
        let accepted=full.act("run.request.answer",params).await;
        assert_eq!(accepted["result"]["delivery"],"written"); assert_eq!(wait_replies(&s,1),vec![v["response"].clone()]);
        let events=s.daemon.events(&s.run());
        assert!(events.iter().any(|e|e["kind"]=="pending_request_changed" && e["payload"]["actor"]["device_id"]==full_pair.device));
        s.daemon.call("gateway.device_revoke",json!({"id":watch_pair.device}));
        assert!(watch.ends_within(Duration::from_secs(5)).await);
    }

    #[tokio::test(flavor="multi_thread",worker_threads=2)]
    async fn secret_typed_answer_is_private_in_results_history_and_device_once_cache() {
        let (s,_)=start_vector("codex_question_free_text"); let item=s.requests()[0].clone();
        common::phone::enable(&s.daemon);
        let (mut phone,paired)=common::phone::pair(&s.daemon,"Secret fixture").await;
        let rid=common::phone::uuid();
        let params=answer_params(&s,&item,json!({"kind":"questions","answers":{"field-0":[PRIVATE]}}));
        let accepted=phone.ask("run.request.answer",params.clone(),Some(&rid)).await.unwrap();
        assert!(accepted.get("error").is_none(),"actual full-device typed answer boundary: {accepted}");
        assert_eq!(accepted["result"]["delivery"],"written");
        assert_eq!(wait_replies(&s,1),vec![json!({"id":7,"result":{"answers":{"question-id":{"answers":[PRIVATE]}}}})]);
        assert_private(&s);
        assert!(!accepted.to_string().contains(PRIVATE) && !phone.transcript.contains(PRIVATE));
        let recorded_dir:String=rusqlite::Connection::open(s.daemon.home.path().join("overseer.sqlite")).unwrap()
            .query_row("SELECT run_dir FROM runs WHERE id=?1",[s.run()],|r|r.get(0)).unwrap();
        for entry in std::fs::read_dir(recorded_dir).unwrap().flatten() {
            if entry.file_name().to_string_lossy().starts_with("output-") {
                assert!(!std::fs::read_to_string(entry.path()).unwrap().contains(PRIVATE),"typed reply bytes are not retained in shim segments");
            }
        }
        let db=rusqlite::Connection::open(s.daemon.home.path().join("overseer.sqlite")).unwrap();
        let cached:String=db.query_row("SELECT reply FROM remote_requests WHERE device_id=?1 AND request_id=?2",rusqlite::params![paired.device,rid],|r|r.get(0)).unwrap();
        assert!(!cached.contains(PRIVATE),"device once-cache is a disposition, never the private answer");
        let replay=phone.ask("run.request.answer",params,Some(&rid)).await.unwrap();
        assert_eq!(replay["result"],accepted["result"]); assert_eq!(native_replies(&s).len(),1);
    }

    #[test]
    fn old_request_key_cannot_send_to_replacement_generation() {
        let (s,v)=start_vector("command_command_decline"); let old=s.requests()[0].clone();
        let generation=s.daemon.run(&s.run())["process_generation"].clone();
        s.daemon.call("run.interrupt",json!({"run_id":s.run()})); s.daemon.wait_done(&s.run(),20);
        s.daemon.call("run.follow_up",json!({"run_id":s.run(),"prompt":"explicit replacement fixture"}));
        s.daemon.call("run.resume_queue",json!({"run_id":s.run()})); s.marker(ONE,2);
        assert_ne!(s.daemon.run(&s.run())["process_generation"],generation);
        let current=s.requests().into_iter().find(|r|r["lifecycle"]=="pending").unwrap();
        reject(&s,answer_params(&s,&old,json!({"kind":"decision","decision":"accept"})),"stale_generation");
        assert_eq!(s.daemon.call("run.request.answer",answer_params(&s,&current,v["answer"].clone()))["delivery"],"written");
        assert_eq!(wait_replies(&s,1),vec![v["response"].clone()]); s.no_protected_action();
    }

    #[test]
    fn native_child_typed_answer_uses_parent_pipe_and_exact_native_id() {
        let mut child=command(json!("child-7"),"child-approval");
        child["params"]["threadId"]=json!("thr-child"); child["params"]["turnId"]=json!("child-turn");
        let s=Script::start("codex-app","codex-cli 0.158.0",vec![
            emit(json!({"method":"item/completed","params":{"threadId":"$THREAD","turnId":"$TURN","item":{
                "type":"collabAgentToolCall","id":"spawn-child","tool":"spawnAgent","status":"completed","senderThreadId":"$THREAD",
                "receiverThreadIds":["thr-child"],"prompt":"child fixture","agentsStates":{"thr-child":{"status":"running"}}
            }}})),emit(json!({"method":"turn/started","params":{"threadId":"thr-child","turn":{"id":"child-turn"}}})),emit(child),mark(ONE)]);
        s.marker(ONE,1); let item=s.requests()[0].clone();
        assert_ne!(item["run_id"],item["process_run_id"]); assert_eq!(item["process_run_id"],s.run());
        let mut params=answer_params(&s,&item,json!({"kind":"decision","decision":"decline"})); params["run_id"]=item["run_id"].clone();
        assert_eq!(s.daemon.call("run.request.answer",params)["delivery"],"written");
        assert_eq!(wait_replies(&s,1),vec![json!({"id":"child-7","result":{"decision":"decline"}})]); s.no_protected_action();
    }

    #[test]
    fn exact_receipt_token_writes_once_and_refuses_changed_digest_or_generation() {
        let (s,v)=start_vector("command_command_decline");
        let db=rusqlite::Connection::open(s.daemon.home.path().join("overseer.sqlite")).unwrap();
        let (dir,generation):(String,i64)=db.query_row("SELECT run_dir,process_generation FROM runs WHERE id=?1",[s.run()],|r|Ok((r.get(0)?,r.get(1)?))).unwrap();
        let launch:Value=serde_json::from_slice(&std::fs::read(Path::new(&dir).join("launch.json")).unwrap()).unwrap();
        let socket=PathBuf::from(launch["control_socket"].as_str().unwrap());
        let data=format!("{}\n",v["response"]); let digest=format!("{:x}",Sha256::digest(data.as_bytes()));
        let token=format!("delivery-{}",uuid::Uuid::new_v4().simple());
        let request=json!({"op":"request_reply","generation":generation,"delivery_token":token,"answer_digest":digest,"data":data});
        let first=response(&socket,request.clone()); assert_eq!(first["state"],"written", "typed private receipt operation: {first}");
        assert_eq!(response(&socket,request.clone()),first,"same token is not a second write");
        let alternate=format!("{}\n",frozen("command_command_cancel")["response"]);
        let mut changed=request.clone(); changed["data"]=json!(alternate);
        changed["answer_digest"]=json!(format!("{:x}",Sha256::digest(alternate.as_bytes())));
        let refused=response(&socket,changed); assert_eq!(refused["ok"],false); assert_eq!(refused["code"],"receipt_conflict");
        let mut stale=request; stale["generation"]=json!(generation+1);
        let refused=response(&socket,stale); assert_eq!(refused["ok"],false); assert_eq!(refused["code"],"stale_generation");
        assert_eq!(wait_replies(&s,1),vec![v["response"].clone()]);
        let status=response(&socket,json!({"op":"request_reply_status","generation":generation,"delivery_token":token,"answer_digest":digest}));
        assert_eq!(status["state"],"written");
        assert_eq!(native_replies(&s).len(),1); s.no_protected_action();
    }
}
