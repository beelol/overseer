//! A spoken yes belongs only to the last question the owner heard. Fixtures only.
mod common;

use common::*;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
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
            .current_dir(root).status().map(|s| s.success()).unwrap_or(false);
        assert!(ok || bin.exists(), "could not build overseer-listener");
        bin
    }).clone()
}

fn daemon(mode: &str) -> Daemon {
    let fixture = repo_root().join("fixtures/fake-harness/claude-fixture.js").display().to_string();
    let listener = listener_bin().display().to_string();
    let d = Daemon::start(&[
        ("OVERSEER_CLAUDE_PATH", &fixture),
        ("CLAUDE_FIXTURE_MODE", mode),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "CLAUDE_FIXTURE_MODE"),
        ("OVERSEER_VOICE_SIMULATE", "1"),
        ("OVERSEER_LISTENER", &listener),
        ("OVERSEER_LISTENER_TEST_VOICE", "1"),
        ("OVERSEER_VOICE_CONFIRM_S", "60"),
        ("OVERSEER_TEST_AUTO_DISABLED", "1"),
    ]);
    d.call("agent.cadence", json!({"cadence": "off", "by": "owner"}));
    d.call("overseer.session", json!({}));
    d
}

struct Live(Arc<Mutex<Vec<Value>>>);
impl Live {
    fn listen(d: &Daemon) -> Self {
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
        let live = Self(messages);
        d.call("voice.set", json!({"enabled": true}));
        live.wait(|v| v["kind"] == "state" && v["state"] == "listening");
        live
    }

    fn wait(&self, pred: impl Fn(&Value) -> bool) -> Value {
        let until = Instant::now() + Duration::from_secs(30);
        loop {
            if let Some(value) = self.0.lock().unwrap().iter().find(|v| pred(v)).cloned() { return value; }
            assert!(Instant::now() < until, "no expected voice event; got {:?}", self.0.lock().unwrap());
            std::thread::sleep(Duration::from_millis(25));
        }
    }
}

fn confirm(d: &Daemon, live: &Live, text: &str) -> Value {
    let spoken = d.call("voice.say", json!({"text": text}));
    live.wait(|v| v["kind"] == "request" && v["request"]["id"] == spoken["request"] && v["request"]["state"] == "waiting")["request"].clone()
}

#[test]
fn a_yes_answers_only_the_latest_confirm_and_never_revives_an_older_card() {
    for method in ["voice.say", "voice.answer"] {
        let scratch = tmp();
        let repository = repo(&scratch.path().join("site"));
        let d = daemon("overseer");
        for title in ["Phone", "Gateway"] {
            d.call("task.create", json!({"repo": repository, "harness": "generic", "workspace_mode": "worktree", "program": "/bin/sleep", "args": ["600"], "prompt": "", "title": title}));
        }
        let live = Live::listen(&d);
        let old = confirm(&d, &live, "Archive Phone.");
        let newest = confirm(&d, &live, "Archive Gateway.");
        let yes = if method == "voice.say" { json!({"text": "yes"}) } else { json!({"yes": true}) };
        let answer = d.call(method, yes.clone());
        assert_eq!(answer["request"], newest["id"], "{method} must answer the latest spoken question: {answer}");
        let session = d.call("overseer.session", json!({}));
        assert!(session["proposals"].as_array().unwrap().iter().any(|p| p["id"] == old["proposal"] && p["state"] == "open"), "older proposal stays available on its card: {session}");
        let again = d.try_call(method, yes);
        assert!(again.as_ref().map(|v| v["answered"] != true).unwrap_or(true), "a second yes must not approve the older card: {again:?}");
        assert!(d.call("overseer.session", json!({}))["proposals"].as_array().unwrap().iter().any(|p| p["id"] == old["proposal"] && p["state"] == "open"));
    }
}

#[test]
fn asking_which_permission_clears_the_previous_spoken_answer_target() {
    let scratch = tmp();
    let repository = repo(&scratch.path().join("site"));
    let d = daemon("permission");
    let live = Live::listen(&d);
    let first = d.call("task.create", json!({"repo": repository, "harness": "claude", "prompt": "write perm.txt", "title": "Sessions"}));
    let first = run_id(&first);
    d.wait_status(&first, |s| s == "waiting_for_user", 30);
    live.wait(|v| v["kind"] == "read_back" && v["agent"] == first);
    let second = d.call("task.create", json!({"repo": repository, "harness": "claude", "prompt": "write perm.txt", "title": "Gateway"}));
    let second = run_id(&second);
    d.wait_status(&second, |s| s == "waiting_for_user", 30);
    let handled = d.call("voice.say", json!({"text": "handle what needs me"}));
    assert!(handled["said"].as_str().unwrap().contains("Which one"), "{handled}");
    let answer = d.try_call("voice.answer", json!({"yes": true}));
    assert!(answer.is_err(), "the latest question asked which agent, so a bare yes cannot grant Sessions: {answer:?}");
    // Waiting past the usual settle window must not automatically restore the hidden target.
    std::thread::sleep(Duration::from_secs(4));
    assert!(d.try_call("voice.answer", json!({"yes": true})).is_err());
    for run in [&first, &second] {
        assert_eq!(d.run(run)["status"], "waiting_for_user");
        assert!(!d.events(run).iter().any(|e| e["kind"] == "permission_answered"));
    }
}
