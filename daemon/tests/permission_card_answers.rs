//! AC-230: the unsolicited permission card answers the native request on either button, and
//! answering from any surface advances the spoken permission queue. Fixtures only: no mic or
//! provider calls, with each daemon and listener confined to a temporary home.
mod common;

use common::*;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

fn listener_bin() -> PathBuf {
    static BIN: OnceLock<PathBuf> = OnceLock::new();
    BIN.get_or_init(|| {
        let root = repo_root();
        let target = std::env::var_os("CARGO_TARGET_DIR").map(PathBuf::from)
            .unwrap_or_else(|| root.join("target"));
        let bin = target.join("debug/overseer-listener");
        let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
        let ok = std::process::Command::new(cargo).args(["build", "-q", "-p", "overseer-listener"])
            .current_dir(&root).status().map(|s| s.success()).unwrap_or(false);
        assert!(ok || bin.exists(), "could not build overseer-listener");
        bin
    }).clone()
}

fn permission_daemon(voice: bool) -> Daemon {
    permission_daemon_with(voice, &[])
}

fn permission_daemon_with(voice: bool, extra: &[(&str, &str)]) -> Daemon {
    let fixture = repo_root().join("fixtures/fake-harness/claude-fixture.js").display().to_string();
    let listener = if voice { listener_bin().display().to_string() } else { String::new() };
    let mut env = vec![
        ("OVERSEER_CLAUDE_PATH", fixture.as_str()),
        ("CLAUDE_FIXTURE_MODE", "permission"),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "CLAUDE_FIXTURE_MODE"),
        ("OVERSEER_TEST_AUTO_DISABLED", "1"),
    ];
    if voice {
        env.extend([
            ("OVERSEER_VOICE_SIMULATE", "1"),
            ("OVERSEER_LISTENER", listener.as_str()),
            ("OVERSEER_LISTENER_TEST_VOICE", "1"),
        ]);
    }
    env.extend_from_slice(extra);
    let d = Daemon::start(&env);
    d.call("agent.cadence", json!({"cadence": "off", "by": "owner"}));
    d.call("overseer.session", json!({}));
    d
}

fn waiting_agent(d: &Daemon, repository: &Path, title: &str) -> Value {
    let created = d.call("task.create", json!({"repo": repository, "harness": "claude", "prompt": "write perm.txt", "title": title}));
    let id = run_id(&created);
    d.wait_status(&id, |s| s == "waiting_for_user", 30);
    created
}

fn surfaced_card(d: &Daemon, run: &str) -> Value {
    let until = Instant::now() + Duration::from_secs(15);
    loop {
        let session = d.call("overseer.session", json!({}));
        if let Some(p) = session["proposals"].as_array().unwrap().iter().find(|p|
            p["cause"] == "needs" && p["actions"].as_array().is_some_and(|a|
                a.iter().any(|a| a["action"] == "permission" && a["agent"] == run))) {
            return p.clone();
        }
        assert!(Instant::now() < until, "no surfaced permission card: {session}");
        std::thread::sleep(Duration::from_millis(25));
    }
}

fn decisions(d: &Daemon, run: &str) -> Vec<Value> {
    d.events(run).into_iter().filter(|e| e["kind"] == "permission_answered")
        .map(|e| e["payload"].clone()).collect()
}

#[test]
fn no_on_a_surfaced_permission_denies_the_native_request_once() {
    let scratch = tmp();
    let repository = repo(&scratch.path().join("site"));
    let d = permission_daemon(false);
    let created = waiting_agent(&d, &repository, "Sessions");
    let run = run_id(&created);
    let card = surfaced_card(&d, &run);
    let answer = d.call("overseer.answer", json!({"id": card["id"], "yes": false, "surface": "vscode", "by": "owner"}));
    assert_eq!(answer["state"], "no");
    assert_eq!(decisions(&d, &run).len(), 1, "No must reach the native request");
    assert_eq!(decisions(&d, &run)[0]["allow"], false);
    d.wait_done(&run, 15);
    assert!(!ws_path(&d, &created).join("perm.txt").exists(), "the refused write must not happen");
    assert!(d.try_call("overseer.answer", json!({"id": card["id"], "yes": false, "surface": "vscode", "by": "owner"})).unwrap_err().contains("already_answered"));
    assert_eq!(decisions(&d, &run).len(), 1);
}

#[test]
fn a_typed_no_reports_the_native_denial_in_the_conversation() {
    let scratch = tmp();
    let repository = repo(&scratch.path().join("site"));
    let d = permission_daemon(false);
    let run = run_id(&waiting_agent(&d, &repository, "Sessions"));
    surfaced_card(&d, &run);
    let answer = d.call("overseer.send", json!({"text": "no", "surface": "vscode"}));
    assert!(answer["reply"].as_str().unwrap().contains("denied Sessions"), "{answer}");
    assert_eq!(decisions(&d, &run).len(), 1);
    assert_eq!(decisions(&d, &run)[0]["allow"], false);
}

#[test]
fn declining_an_ordinary_proposal_keeps_the_native_request_waiting() {
    let scratch = tmp();
    let repository = repo(&scratch.path().join("site"));
    let d = permission_daemon(false);
    let created = waiting_agent(&d, &repository, "Sessions");
    let run = run_id(&created);
    let pending = d.run(&run)["attention"]["request_id"].clone();
    let proposal = d.call("overseer.propose", json!({"actions": [{"action": "permission", "agent": run, "request": pending, "allow_request": true}], "source": "ctl"}));
    d.call("overseer.answer", json!({"id": proposal["proposal"], "yes": false, "surface": "vscode", "by": "owner"}));
    assert_eq!(d.run(&run)["status"], "waiting_for_user");
    assert!(decisions(&d, &run).is_empty(), "a proposal decline still performs no action");
}

#[test]
fn no_on_an_already_answered_card_leaves_another_request_waiting() {
    let scratch = tmp();
    let repository = repo(&scratch.path().join("site"));
    let d = permission_daemon(false);
    let first = run_id(&waiting_agent(&d, &repository, "Sessions"));
    let card = surfaced_card(&d, &first);
    let request = d.run(&first)["attention"]["request_id"].clone();
    d.call("run.permission", json!({"run_id": first, "request_id": request, "allow": true}));
    let second = run_id(&waiting_agent(&d, &repository, "Gateway"));
    assert!(d.try_call("overseer.answer", json!({"id": card["id"], "yes": false, "surface": "vscode", "by": "owner"})).unwrap_err().contains("already_answered"));
    assert_eq!(decisions(&d, &first).len(), 1);
    assert_eq!(decisions(&d, &first)[0]["allow"], true);
    assert_eq!(d.run(&second)["status"], "waiting_for_user");
    assert!(decisions(&d, &second).is_empty());
}

#[test]
fn a_mismatched_surfaced_card_cannot_deny_the_pending_request() {
    let scratch = tmp();
    let repository = repo(&scratch.path().join("site"));
    let d = permission_daemon(false);
    let created = waiting_agent(&d, &repository, "Sessions");
    let run = run_id(&created);
    let card = surfaced_card(&d, &run);
    // Simulate the original card outliving its native request while the agent waits on another.
    let conn = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    conn.busy_timeout(Duration::from_secs(10)).unwrap();
    let mut attention = d.run(&run)["attention"].clone();
    attention["request_id"] = json!("different-request");
    conn.execute("UPDATE runs SET attention=?2 WHERE id=?1", rusqlite::params![run, attention.to_string()]).unwrap();
    let answer = d.call("overseer.answer", json!({"id": card["id"], "yes": false, "surface": "vscode", "by": "owner"}));
    assert!(answer["result"].as_str().unwrap().contains("another one now"), "{answer}");
    assert!(decisions(&d, &run).is_empty());
    assert_eq!(d.run(&run)["attention"]["request_id"], "different-request");
}

struct Live(Arc<Mutex<Vec<Value>>>);

impl Live {
    fn open(d: &Daemon) -> Self {
        let mut stream = UnixStream::connect(d.socket()).unwrap();
        writeln!(stream, "{}", json!({"id": 1, "method": "voice.subscribe", "params": {}})).unwrap();
        let messages: Arc<Mutex<Vec<Value>>> = Default::default();
        let sink = messages.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(stream).lines() {
                let Ok(line) = line else { break };
                if let Ok(value) = serde_json::from_str::<Value>(&line) {
                    if value["method"] == "voice" { sink.lock().unwrap().push(value["params"].clone()); }
                }
            }
        });
        Self(messages)
    }

    fn wait(&self, pred: impl Fn(&Value) -> bool) {
        let until = Instant::now() + Duration::from_secs(15);
        loop {
            if self.0.lock().unwrap().iter().any(&pred) { return; }
            assert!(Instant::now() < until, "no expected voice event; got {:?}", self.0.lock().unwrap());
            std::thread::sleep(Duration::from_millis(25));
        }
    }
}

#[test]
fn clicking_after_the_spoken_answer_window_expires_advances_the_queue() {
    let scratch = tmp();
    let repository = repo(&scratch.path().join("site"));
    let d = permission_daemon_with(true, &[("OVERSEER_VOICE_CONFIRM_S", "2")]);
    let live = Live::open(&d);
    d.call("voice.set", json!({"enabled": true}));
    live.wait(|v| v["kind"] == "state" && v["state"] == "listening");
    let first = run_id(&waiting_agent(&d, &repository, "Sessions"));
    live.wait(|v| v["kind"] == "read_back" && v["agent"] == first);
    let second = run_id(&waiting_agent(&d, &repository, "Gateway"));
    live.wait(|v| v["kind"] == "read_back" && v["lapsed"] == true);
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(live.0.lock().unwrap().iter().filter(|v| v["kind"] == "read_back" && v["agent"] == first).count(), 1, "silence must not replay the unanswered request");
    assert!(!live.0.lock().unwrap().iter().any(|v| v["kind"] == "read_back" && v["agent"] == second));
    let card = surfaced_card(&d, &first);
    d.call("overseer.answer", json!({"id": card["id"], "yes": true, "surface": "vscode", "by": "owner"}));
    live.wait(|v| v["kind"] == "read_back" && v["agent"] == second);
    let no = d.call("voice.say", json!({"text": "no, deny it"}));
    assert_eq!(no["allow"], false);
    d.wait_done(&second, 15);
    assert_eq!(decisions(&d, &second).len(), 1);
    assert_eq!(decisions(&d, &second)[0]["allow"], false);
    assert_eq!(live.0.lock().unwrap().iter().filter(|v| v["kind"] == "read_back" && v["agent"] == second).count(), 1);
}

#[test]
fn answering_from_a_click_or_native_surface_reads_the_next_permission_once() {
    for surface in ["vscode", "native"] {
        let scratch = tmp();
        let repository = repo(&scratch.path().join("site"));
        let d = permission_daemon(true);
        let live = Live::open(&d);
        d.call("voice.set", json!({"enabled": true}));
        live.wait(|v| v["kind"] == "state" && v["state"] == "listening");
        let first = run_id(&waiting_agent(&d, &repository, "Sessions"));
        live.wait(|v| v["kind"] == "read_back" && v["agent"] == first);
        let second = run_id(&waiting_agent(&d, &repository, "Gateway"));
        assert!(!live.0.lock().unwrap().iter().any(|v| v["kind"] == "read_back" && v["agent"] == second), "the second must wait its turn");
        if surface == "vscode" {
            let card = surfaced_card(&d, &first);
            d.call("overseer.answer", json!({"id": card["id"], "yes": true, "surface": surface, "by": "owner"}));
        } else {
            let request = d.run(&first)["attention"]["request_id"].clone();
            d.call("run.permission", json!({"run_id": first, "request_id": request, "allow": true}));
        }
        live.wait(|v| v["kind"] == "read_back" && v["agent"] == second);
        let resolved = live.0.lock().unwrap().iter().find(|v| v["kind"] == "read_back" && v["resolved"] == true).cloned().expect("clearing the first request must stop its asking indicator");
        assert_eq!(resolved["lapsed"], true, "existing UI surfaces use lapsed to clear asking");
        assert!(resolved["agent"].is_null(), "a resolved notification is not another read-back");
        std::thread::sleep(Duration::from_millis(800));
        assert_eq!(live.0.lock().unwrap().iter().filter(|v| v["kind"] == "read_back" && v["agent"] == second).count(), 1, "next request is read exactly once after {surface}");
        let no = d.call("voice.say", json!({"text": "no, deny it"}));
        assert_eq!(no["allow"], false, "the next spoken answer belongs to Gateway: {no}");
        d.wait_done(&second, 15);
        assert_eq!(decisions(&d, &second).len(), 1);
        assert_eq!(decisions(&d, &second)[0]["allow"], false);
    }
}
