//! Overseer on every surface (AC-199): a request typed and the same request spoken through Voice
//! Mode leave cards of the same form; the cue log: Overseer needing the owner (a proposal and a
//! conflict together) is one attention cue, and Overseer's own run and a watcher starting and
//! finishing make no sound. A real daemon with the simulated voice (`OVERSEER_VOICE_SIMULATE=1`),
//! the Claude fixture as Overseer and as the agents, and cues written to a log. No microphone,
//! no speaker, no paid turn.

mod common;

use common::*;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

/// The listener binary Voice Mode runs, built once for these tests.
fn listener_bin() -> PathBuf {
    static BIN: OnceLock<PathBuf> = OnceLock::new();
    BIN.get_or_init(|| {
        let root = repo_root();
        let target = std::env::var_os("CARGO_TARGET_DIR").map(PathBuf::from).unwrap_or_else(|| root.join("target"));
        let bin = target.join("debug/overseer-listener");
        let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
        let ok = std::process::Command::new(cargo).args(["build", "-q", "-p", "overseer-listener"]).current_dir(&root).status().map(|s| s.success()).unwrap_or(false);
        assert!(ok || bin.exists(), "could not build overseer-listener");
        bin
    })
    .clone()
}

struct Env {
    d: Daemon,
    dir: tempfile::TempDir,
}

impl Env {
    fn start(voice: bool) -> Env {
        let dir = tempfile::tempdir().unwrap();
        let mode = dir.path().join("mode");
        std::fs::write(&mode, "overseer").unwrap();
        let log = dir.path().join("cues.log").display().to_string();
        let bin = listener_bin().display().to_string();
        let fixture = repo_root().join("fixtures/fake-harness/claude-fixture.js").display().to_string();
        let mode_s = mode.display().to_string();
        let mut env: Vec<(&str, &str)> = vec![
            ("OVERSEER_TEST_AUDIO_LOG", &log),
            ("OVERSEER_CLAUDE_PATH", &fixture),
            ("CLAUDE_FIXTURE_MODE_FILE", &mode_s),
            ("FIXTURE_SLOW_MS", "2000"),
            ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "CLAUDE_FIXTURE_MODE_FILE,FIXTURE_SLOW_MS"),
        ];
        if voice {
            env.extend_from_slice(&[("OVERSEER_VOICE_SIMULATE", "1"), ("OVERSEER_LISTENER", &bin), ("OVERSEER_LISTENER_TEST_VOICE", "1")]);
        }
        Env { d: Daemon::start(&env), dir }
    }
    fn mode(&self, m: &str) {
        std::fs::write(self.dir.path().join("mode"), m).unwrap();
    }
    fn cues(&self) -> Vec<String> {
        std::fs::read_to_string(self.dir.path().join("cues.log")).unwrap_or_default().lines().map(String::from).collect()
    }
    fn until(&self, what: &str, secs: u64, mut f: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(secs);
        while !f() {
            assert!(Instant::now() < deadline, "never {what}");
            std::thread::sleep(Duration::from_millis(50));
        }
    }
    fn session(&self) -> Value {
        self.d.call("overseer.session", json!({}))
    }
    fn overseer_idle(&self) {
        self.until("Overseer idle", 60, || {
            let s = self.session();
            !s["run_id"].is_null() && !["queued", "starting", "running"].contains(&s["run_status"].as_str().unwrap_or(""))
        });
    }
}

fn sleeper(d: &Daemon, repo: &Path, title: &str) -> String {
    let created = d.call("task.create", json!({"repo": repo, "harness": "generic", "workspace_mode": "worktree", "program": "/bin/sleep", "args": ["60"], "prompt": "", "title": title}));
    created["run"]["id"].as_str().unwrap().to_string()
}

/// A card once its row for `run` went out.
fn sent_card(env: &Env, proposal: &str) -> Value {
    let mut card = Value::Null;
    env.until(&format!("proposal {proposal} carried out"), 60, || {
        card = env.d.call("overseer.card", json!({"id": proposal}));
        card["rows"].as_array().is_some_and(|r| !r.is_empty() && r.iter().all(|x| x["sent_ms"].is_i64()))
    });
    card
}

#[test]
fn ac199_a_typed_request_and_the_same_spoken_request_leave_cards_of_one_form() {
    let env = Env::start(true);
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let phone = sleeper(&env.d, &repo, "Phone");
    env.d.call("overseer.session", json!({}));
    env.d.call("agent.cadence", json!({"cadence": "off", "by": "owner"}));
    env.d.call("overseer.level", json!({"level": "steer"}));
    // Typed.
    env.d.call("overseer.send", json!({"text": "Tell Phone to use the new wire format.", "surface": "vscode", "harness": "claude"}));
    let mut typed_id = String::new();
    env.until("Overseer's proposal", 60, || {
        typed_id = env.session()["messages"].as_array().unwrap().iter().filter_map(|m| m["text"].as_str()).find_map(|t| t.split("(proposal ").nth(1).map(|x| x.trim_end_matches(')').to_string())).unwrap_or_default();
        !typed_id.is_empty()
    });
    let typed = sent_card(&env, &typed_id);
    env.overseer_idle();
    // Spoken.
    env.d.call("voice.set", json!({"enabled": true}));
    env.until("listening", 20, || env.d.call("voice.get", json!({}))["state"] == "listening");
    let said = env.d.call("voice.say", json!({"text": "Tell Phone to use the new wire format."}));
    assert_eq!(said["taken"], true, "{said}");
    let rq = said["request"].as_str().unwrap().to_string();
    let mut spoken_id = String::new();
    env.until("the spoken request's proposal", 60, || {
        spoken_id = env.d.call("voice.requests", json!({}))["requests"].as_array().unwrap().iter().find(|r| r["id"] == rq.as_str()).and_then(|r| r["proposal"].as_str().map(str::to_string)).unwrap_or_default();
        !spoken_id.is_empty()
    });
    let spoken = sent_card(&env, &spoken_id);
    // One form: the same fields, the same agent, action, delivery, reason, states and answer;
    // what differs is the source (the spoken text opens with the owner's words and the request,
    // as AC-169 asks) and the model's own confidence.
    let keys = |v: &Value| v.as_object().unwrap().keys().cloned().collect::<Vec<_>>();
    assert_eq!(keys(&typed), keys(&spoken));
    assert_eq!(keys(&typed["rows"][0]), keys(&spoken["rows"][0]));
    for field in ["state", "answered_by", "surface", "via", "session"] {
        assert_eq!(typed[field], spoken[field], "{field}");
    }
    let (t, v) = (&typed["rows"][0], &spoken["rows"][0]);
    for field in ["action", "run_id", "title", "delivery", "state", "why"] {
        assert_eq!(t[field], v[field], "row {field}");
    }
    let sent = t["message"].as_str().unwrap();
    let spoken_text = v["message"].as_str().unwrap();
    let source = format!("(voice, request {rq}) The owner said: “Tell Phone to use the new wire format.”\nFor you: ");
    assert_eq!(spoken_text.strip_prefix(source.as_str()), Some(sent), "the spoken text is the typed text after its source: {spoken_text:?}");
    let strip = |a: &Value| {
        let mut a = a.clone();
        a.as_object_mut().unwrap().remove("confidence");
        a.as_object_mut().unwrap().remove("why");
        a["text"] = json!(a["text"].as_str().unwrap().trim_start_matches(source.as_str()));
        a
    };
    assert_eq!(strip(&typed["actions"][0]), strip(&spoken["actions"][0]));
    assert_eq!(spoken["rows"][0]["why"], "named");
}

/// The cue log (AC-199, AC-143): Overseer's own run makes no sound; a proposal waiting for the
/// owner and a conflict that needs a decision arriving together are one attention cue; a watcher
/// starting and finishing makes none.
#[test]
fn ac199_the_cue_log_one_cue_per_need_and_none_for_overseer_s_own_runs() {
    let env = Env::start(false);
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    std::fs::write(repo.join("shared.txt"), "one\ntwo\nthree\n").unwrap();
    git(&repo, &["add", "shared.txt"]);
    git(&repo, &["commit", "-qm", "shared"]);
    env.d.call("audio.set", json!({"enabled": true}));
    env.d.call("overseer.session", json!({}));
    env.d.call("agent.cadence", json!({"cadence": "off", "by": "owner"}));
    // Overseer's own run starts and finishes: no sound.
    env.d.call("overseer.send", json!({"text": "What is everyone doing?", "surface": "ctl", "harness": "claude"}));
    env.overseer_idle();
    std::thread::sleep(Duration::from_millis(1200));
    assert!(env.cues().is_empty(), "Overseer's own run is silent: {:?}", env.cues());
    // Two agents writing the same lines; their starts are cues of their own.
    let a = run_id(&env.d.generic(&repo, "worktree", "/bin/sh", &["-c", "printf 'one\\nA\\nthree\\n' > shared.txt; sleep 60"]));
    let b = run_id(&env.d.generic(&repo, "worktree", "/bin/sh", &["-c", "printf 'one\\nB\\nthree\\n' > shared.txt; sleep 60"]));
    // The conflict and, at once, a proposal that waits for the owner (Ask first).
    let mut conflict = Value::Null;
    env.until("a same-lines conflict", 30, || {
        conflict = env.d.call("conflicts.list", json!({}))["conflicts"].as_array().unwrap().iter().find(|c| c["kind"] == "same_lines").cloned().unwrap_or(Value::Null);
        !conflict.is_null()
    });
    let p = env.d.call("overseer.propose", json!({"actions": [{"action": "hold", "agent": a, "reason": "the conflict"}], "source": "ctl"}));
    assert_eq!(p["state"], "open", "{p}");
    assert_eq!(conflict["needs_decision"], true, "{conflict}");
    std::thread::sleep(Duration::from_millis(2000));
    let needs = env.cues().iter().filter(|c| c.ends_with(":agent_needs_attention")).count();
    assert_eq!(needs, 1, "a proposal and a conflict together are one cue: {:?}", env.cues());
    env.d.call("overseer.answer", json!({"id": p["proposal"], "yes": false, "surface": "ctl", "by": "owner"}));
    for id in [&a, &b] {
        env.d.call("run.interrupt", json!({"run_id": id}));
    }
    std::thread::sleep(Duration::from_millis(1200));
    // A watcher starting and finishing: the subject's start and completion are its cues; the
    // watcher starts as the subject ends and finishes two seconds later (past the burst gate),
    // and makes none.
    let before = env.cues().len();
    env.mode("slow");
    let subject = run_id(&env.d.call("task.create", json!({"repo": repo, "harness": "claude", "prompt": "tidy", "title": "Subject"})));
    env.d.wait_status(&subject, |s| s == "running", 20);
    env.d.call("watch.start", json!({"subject": subject, "brief": "anything odd", "harness": "claude", "by": "owner"}));
    env.d.wait_done(&subject, 30);
    let mut watcher = String::new();
    env.until("the watcher started", 20, || {
        watcher = env.d.call("watch.list", json!({}))["watches"][0]["watcher"].as_str().unwrap_or("").to_string();
        !watcher.is_empty()
    });
    env.until("the watcher finished", 30, || {
        env.d.call("state", json!({}))["runs"].as_array().unwrap().iter().any(|r| r["id"] == watcher.as_str() && r["status"] == "completed")
    });
    std::thread::sleep(Duration::from_millis(1500));
    let cues = env.cues();
    assert_eq!(cues[before..].to_vec(), ["reactor:agent_started", "reactor:agent_complete"], "the subject's start and end, and nothing for its watcher: {cues:?}");
}
