//! AC-200: first native tools wait for actual run and durable origin publication.
//! One real launch-token MCP call per case; no forged transport or authority.
mod common;
use common::*;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

// Release both scheduler gates before joining even when an assertion unwinds.
// The peer has bounded socket reads/writes, so its join cannot wait indefinitely.
struct OwnerSend {
    releases: [PathBuf; 2],
    reply: mpsc::Receiver<Result<Value, String>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl OwnerSend {
    fn start(socket: PathBuf, text: String, publication: &Path, native: &Path) -> Self {
        let (tx, reply) = mpsc::channel();
        let thread = std::thread::spawn(move || {
            let result = (|| -> Result<Value, String> {
                let mut peer = UnixStream::connect(socket).map_err(|e| e.to_string())?;
                peer.set_read_timeout(Some(Duration::from_secs(30)))
                    .map_err(|e| e.to_string())?;
                peer.set_write_timeout(Some(Duration::from_secs(5)))
                    .map_err(|e| e.to_string())?;
                writeln!(
                    peer,
                    "{}",
                    json!({"id": 1, "method": "overseer.send", "params": {
                        "text": text, "surface": "ctl", "harness": "claude"
                    }})
                )
                .map_err(|e| e.to_string())?;
                let mut line = String::new();
                BufReader::new(peer)
                    .read_line(&mut line)
                    .map_err(|e| e.to_string())?;
                let wire: Value = serde_json::from_str(&line).map_err(|e| e.to_string())?;
                if !wire["error"].is_null() {
                    return Err(wire["error"].to_string());
                }
                Ok(wire["result"].clone())
            })();
            let _ = tx.send(result);
        });
        Self {
            releases: [publication.join("release"), native.join("release")],
            reply,
            thread: Some(thread),
        }
    }

    fn release(&self) {
        for path in &self.releases {
            let _ = std::fs::write(path, "");
        }
    }

    fn finish(&mut self) -> Value {
        self.release();
        let reply = self
            .reply
            .recv_timeout(Duration::from_secs(35))
            .expect("setup: owner send did not finish after publication release");
        self.thread
            .take()
            .unwrap()
            .join()
            .expect("owner send peer panicked");
        reply.expect("setup: owner send failed")
    }
}

impl Drop for OwnerSend {
    fn drop(&mut self) {
        self.release();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn wait_for(path: &Path, seconds: u64) {
    let deadline = Instant::now() + Duration::from_secs(seconds);
    while !path.exists() {
        assert!(
            Instant::now() < deadline,
            "setup: no acknowledgment at {}",
            path.display()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn first_native_call(stage: &str) {
    let root = tmp();
    let repository = repo(&root.path().join("repo"));
    let native = root.path().join("native");
    let publication = root.path().join("publication");
    std::fs::create_dir(&native).unwrap();
    std::fs::create_dir(&publication).unwrap();
    let forced = root.path().join("overseer-mode");
    std::fs::write(&forced, "overseer-gated-propose").unwrap();
    let net = root.path().join("net.json");
    std::fs::write(
        &net,
        json!({"system": "connected", "providers": {}}).to_string(),
    )
    .unwrap();
    let d = Daemon::start(&[
        (
            "OVERSEER_CLAUDE_PATH",
            &repo_root()
                .join("fixtures/fake-harness/claude-fixture.js")
                .display()
                .to_string(),
        ),
        (
            "CLAUDE_FIXTURE_OVERSEER_MODE_FILE",
            &forced.display().to_string(),
        ),
        (
            "CLAUDE_FIXTURE_PROPOSE_GATE_DIR",
            &native.display().to_string(),
        ),
        (
            "OVERSEER_HARNESS_ENV_PASSTHROUGH",
            "CLAUDE_FIXTURE_OVERSEER_MODE_FILE,CLAUDE_FIXTURE_PROPOSE_GATE_DIR",
        ),
        ("OVERSEER_TEST_NET", &net.display().to_string()),
        ("OVERSEER_CONTINUITY_PROBES", "off"),
        (
            "OVERSEER_TEST_NATIVE_PUBLICATION_GATE",
            &publication.display().to_string(),
        ),
        ("OVERSEER_TEST_NATIVE_PUBLICATION_STAGE", stage),
        ("OVERSEER_TEST_AUTO_DISABLED", "1"),
    ]);
    d.call("agent.cadence", json!({"cadence": "off", "by": "owner"}));
    d.call("overseer.level", json!({"level": "steer"}));
    let subject = run_id(&d.generic(&repository, "worktree", "/bin/sh", &["-c", "echo subject"]));
    d.wait_done(&subject, 20);
    std::fs::write(
        native.join("calls.json"),
        json!([{
            "tool": "propose", "arguments": {"actions": [{"action": "archive", "agent": subject}]}
        }])
        .to_string(),
    )
    .unwrap();
    let initial = d.call("overseer.session", json!({}));
    assert!(
        initial["run_id"].is_null(),
        "first real native launch only: {initial}"
    );
    let sid = initial["id"].as_str().unwrap();
    let mut send = OwnerSend::start(
        d.socket(),
        format!("Archive agent {subject}."),
        &publication,
        &native,
    );
    wait_for(&publication.join("reached"), 15);
    assert_eq!(
        std::fs::read_to_string(publication.join("reached")).unwrap(),
        stage
    );
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    db.busy_timeout(Duration::from_secs(5)).unwrap();
    let (run, turn): (String, String) = db.query_row(
        "SELECT r.id,t.id FROM runs r JOIN run_roles rr ON rr.run_id=r.id JOIN turns t ON t.run_id=r.id WHERE rr.role='overseer' ORDER BY t.n DESC LIMIT 1",
        [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
    let bound: Option<String> = db
        .query_row(
            "SELECT run_id FROM overseer_sessions WHERE id=?1",
            [sid],
            |r| r.get(0),
        )
        .unwrap();
    let token_run: String = db
        .query_row(
            "SELECT run_id FROM overseer_tokens WHERE role='overseer'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let origins: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM overseer_turns WHERE session_id=?1 AND turn_id=?2",
            rusqlite::params![sid, turn],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(origins, 0, "real publication window must be open");
    let token_turn: String = db.query_row("SELECT native_turn_id FROM overseer_tokens WHERE run_id=?1 AND role='overseer' AND revoked_ms IS NULL", [&run], |r| r.get(0)).unwrap();
    assert_eq!(
        token_turn, turn,
        "initial launch has immutable actual turn binding"
    );
    match stage {
        "before_bind" => {
            assert_eq!(bound, None);
            // The capability is bound to the already-created actual run/turn
            // before launch, while session/origin publication remains pending.
            assert_eq!(token_run, run);
        }
        "before_origin" => {
            assert_eq!(bound.as_deref(), Some(run.as_str()));
            assert_eq!(token_run, run);
        }
        _ => panic!("unexpected publication stage"),
    }
    drop(db);
    wait_for(&native.join("reached"), 10);
    assert!(std::fs::read_to_string(native.join("reached"))
        .unwrap()
        .contains("allowed native tool"));
    std::fs::write(native.join("release"), "").unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let observed = loop {
        if native.join("reply.json").exists() {
            break "actual early wire reply";
        }
        // Fixed source will acknowledge the actual contested TURN_START mutex,
        // immediately before its real lock wait; baseline has no such wait.
        if publication.join("native-waiting").exists() {
            break "actual turn-start mutex wait";
        }
        assert!(
            Instant::now() < deadline,
            "setup: native request produced neither a real reply nor mutex-wait acknowledgment"
        );
        std::thread::sleep(Duration::from_millis(10));
    };
    let sent = send.finish();
    assert_eq!(sent["run_id"], run);
    assert_eq!(sent["turn"], turn);
    assert_eq!(sent["queued"], false);
    wait_for(&native.join("reply.json"), 10);
    let reply: Value =
        serde_json::from_str(&std::fs::read_to_string(native.join("reply.json")).unwrap()).unwrap();
    wait_for(&native.join("replies.json"), 10);
    let replies: Value =
        serde_json::from_str(&std::fs::read_to_string(native.join("replies.json")).unwrap())
            .unwrap();
    assert_eq!(
        replies.as_array().unwrap().len(),
        1,
        "one input attempt only"
    );
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let s = d.call("overseer.session", json!({}));
        if !["queued", "starting", "running", "waiting_for_user"]
            .contains(&s["run_status"].as_str().unwrap_or(""))
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "setup: native fixture did not complete: {s}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let after = d.call("overseer.session", json!({}));
    assert_eq!(after["id"], sid);
    assert_eq!(after["run_id"], run);
    assert!(
        !d.events(&subject)
            .iter()
            .any(|e| matches!(e["kind"].as_str(), Some("archived" | "task_archived"))),
        "no archive approval or effect"
    );
    eprintln!("stage={stage}; observation={observed}; actual run={run} turn={turn}; actual native reply={reply}; proposals={}", after["proposals"]);
    // Cleanup/turn completion precede the intended baseline support assertion.
    assert_eq!(
        reply["isError"], false,
        "a genuine first owner native call must succeed after publication: {reply}"
    );
    assert_eq!(
        observed, "actual turn-start mutex wait",
        "the real call must await the production mutex, not depend on elapsed time"
    );
    let proposals = after["proposals"].as_array().unwrap();
    assert_eq!(proposals.len(), 1);
    assert_eq!(proposals[0]["state"], "open");
    assert_eq!(proposals[0]["cause"], "owner");
    assert_eq!(proposals[0]["actions"][0]["action"], "archive");
    let events = d.events(&run);
    let proposal = events.iter().find(|e| e["kind"] == "proposal").unwrap();
    assert_eq!(proposal["payload"]["confirm"], true);
    assert_eq!(proposal["payload"]["turn"]["id"], turn);
    assert_eq!(
        events
            .iter()
            .filter(|e| e["kind"] == "overseer_tool_call" && e["payload"]["name"] == "propose")
            .count(),
        1,
        "tool telemetry belongs to the resolved actual run"
    );
}

#[test]
fn ac200_first_native_call_waits_for_actual_run_binding() {
    first_native_call("before_bind");
}

#[test]
fn ac200_first_native_call_waits_for_durable_origin_publication() {
    first_native_call("before_origin");
}
