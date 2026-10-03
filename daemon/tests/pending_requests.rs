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
        let s = Script::start(harness,if harness=="claude" {"2.1.288 (Claude Code)"} else {"codex-cli 0.158.0"},vec![emit(frame),mark(ONE)]);
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
            }}})),emit(child),mark(ONE)]);
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
