//! Voice Mode's live checks on the default Claude account (AC-165, AC-166, AC-168): Claude Haiku,
//! one attempt each, with the simulated voice (no microphone). Paid, so run by hand:
//!
//!     OVERSEER_VOICE_LIVE=1 cargo test -p overseerd --test voice_live -- --nocapture
//!
//! Writes `docs/verification/evidence/voice/live.json`. Without the variable it does nothing.

mod common;

use common::*;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn listener_bin() -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_BIN_EXE_overseerd"));
    dir.parent().unwrap().join("overseer-listener")
}

/// The live channel, with the time each message came.
struct Live {
    msgs: Arc<Mutex<Vec<(Instant, Value)>>>,
}

impl Live {
    fn open(d: &Daemon) -> Self {
        let mut conn = UnixStream::connect(d.socket()).unwrap();
        conn.write_all(
            format!(
                "{}\n",
                json!({"id": 1, "method": "voice.subscribe", "params": {}})
            )
            .as_bytes(),
        )
        .unwrap();
        let msgs: Arc<Mutex<Vec<(Instant, Value)>>> = Default::default();
        let sink = msgs.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(conn).lines() {
                let Ok(line) = line else { break };
                if let Ok(v) = serde_json::from_str::<Value>(&line) {
                    if v["method"] == "voice" {
                        sink.lock()
                            .unwrap()
                            .push((Instant::now(), v["params"].clone()));
                    }
                }
            }
        });
        std::thread::sleep(Duration::from_millis(100));
        Self { msgs }
    }
    fn find(&self, pred: impl Fn(&Value) -> bool) -> Option<(Instant, Value)> {
        self.msgs
            .lock()
            .unwrap()
            .iter()
            .find(|(_, v)| pred(v))
            .cloned()
    }
    fn wait(&self, secs: u64, pred: impl Fn(&Value) -> bool) -> Option<(Instant, Value)> {
        let end = Instant::now() + Duration::from_secs(secs);
        while Instant::now() < end {
            if let Some(x) = self.find(&pred) {
                return Some(x);
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        None
    }
    fn clear(&self) {
        self.msgs.lock().unwrap().clear();
    }
}

fn request(d: &Daemon, id: &str) -> Value {
    d.call("voice.requests", json!({}))["requests"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == id)
        .cloned()
        .unwrap_or(json!(null))
}

/// Waits for a request to close, up to `secs`; its row and its card's targets.
fn settled(d: &Daemon, id: &str, secs: u64) -> (Value, Vec<String>) {
    let end = Instant::now() + Duration::from_secs(secs);
    loop {
        let r = request(d, id);
        let st = r["state"].as_str().unwrap_or("");
        if [
            "sent",
            "partly_sent",
            "answered",
            "not_sent",
            "waiting",
            "not_for_overseer",
            "cancelled",
        ]
        .contains(&st)
            || Instant::now() > end
        {
            let targets = r["proposal"]
                .as_str()
                .and_then(|p| d.try_call("overseer.card", json!({"id": p})).ok())
                .map(|c| {
                    c["rows"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|x| x["title"].as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            return (r, targets);
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

#[test]
fn live_on_the_default_account() {
    if std::env::var("OVERSEER_VOICE_LIVE").ok().as_deref() != Some("1") {
        eprintln!("skipped: set OVERSEER_VOICE_LIVE=1 to run the paid live checks");
        return;
    }
    let bin = listener_bin().display().to_string();
    // The real Claude Code CLI (the tests disable real harnesses unless told where one is).
    let claude = std::env::var("OVERSEER_VOICE_LIVE_CLAUDE").unwrap_or_else(|_| {
        let out = std::process::Command::new("sh")
            .args(["-lc", "command -v claude"])
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    });
    assert!(
        !claude.is_empty(),
        "no claude CLI on PATH (set OVERSEER_VOICE_LIVE_CLAUDE)"
    );
    let d = Daemon::start(&[
        ("OVERSEER_VOICE_SIMULATE", "1"),
        ("OVERSEER_LISTENER", &bin),
        ("OVERSEER_LISTENER_TEST_VOICE", "1"),
        ("OVERSEER_CLAUDE_PATH", &claude),
    ]);
    let a = tmp();
    let b = tmp();
    let overseer_repo = repo(&a.path().join("overseer"));
    let website = repo(&b.path().join("website"));
    let sleeper = |r: &std::path::Path, title: &str| -> String {
        d.call("task.create", json!({"repo": r, "harness": "generic", "workspace_mode": "worktree", "program": "/bin/sleep", "args": ["1800"], "prompt": "", "title": title}))["run"]["id"].as_str().unwrap().to_string()
    };
    let agents: Vec<(String, String)> = [
        (&overseer_repo, "Phone app"),
        (&overseer_repo, "Continuity"),
        (&overseer_repo, "Swarm mode"),
        (&overseer_repo, "Auto routing"),
        (&website, "Landing page"),
        (&website, "Docs cleanup"),
    ]
    .iter()
    .map(|(r, t)| (sleeper(r, t), t.to_string()))
    .collect();
    // Overseer's session on Claude Haiku (one typed turn), then Voice Mode on.
    d.call("overseer.send", json!({"text": "Voice Mode live check starting. Reply with one short sentence.", "surface": "vscode", "harness": "claude", "model": "haiku"}));
    let started = Instant::now();
    while d.call("overseer.session", json!({}))["run_status"] != "completed"
        && started.elapsed() < Duration::from_secs(120)
    {
        std::thread::sleep(Duration::from_millis(500));
    }
    let session = d.call("overseer.session", json!({}));
    assert_eq!(
        session["run_status"], "completed",
        "Overseer's first turn on Haiku: {session}"
    );
    d.call("voice.set", json!({"enabled": true, "settle_seconds": 1, "start_defaults": {"harness": "claude", "model": "haiku", "workspace_mode": "worktree", "trusted": true}}));
    let live = Live::open(&d);
    // `OVERSEER_VOICE_LIVE_ONLY=ac168` runs the live start alone (one more attempt after a fix).
    let only = std::env::var("OVERSEER_VOICE_LIVE_ONLY").unwrap_or_default();
    live.wait(30, |v| v["kind"] == "state" && v["state"] == "listening");
    let mut out = json!({"account": "the default Claude login (system profile)", "model": "haiku", "attempts": "one each"});

    if only.is_empty() {
        // AC-165: one spoken request through the simulated listener, timed.
        live.clear();
        d.call(
            "voice.simulate",
            json!({"speechlike": 1.2, "words": "Tell Continuity to use the new wire format."}),
        );
        let end = live.wait(60, |v| v["kind"] == "heard" && v["final"] == true);
        let t = |x: Option<(Instant, Value)>| x.map(|(at, _)| at);
        let t_end = t(end.clone());
        let heard = t(live.wait(10, |v| v["kind"] == "heard_signal"));
        let on_it = t(live.wait(10, |v| v["kind"] == "spoke" && v["event"] == "start"));
        let taken = live
            .wait(20, |v| v["kind"] == "request")
            .map(|(_, v)| v["request"]["id"].as_str().unwrap_or("").to_string())
            .unwrap_or_default();
        let plan = t(live.wait(90, |v| {
            v["kind"] == "say"
                && v["text"] != "On it."
                && v["text"] != "Sent."
                && v["text"] != "Still working on it."
        }));
        let sent = t(live.wait(120, |v| v["kind"] == "say" && v["text"] == "Sent."));
        let ms = |x: Option<Instant>| match (t_end, x) {
            (Some(a), Some(b)) => json!(b.saturating_duration_since(a).as_millis() as u64),
            _ => json!(null),
        };
        let (row, targets) = settled(&d, &taken, 120);
        out["ac165"] = json!({"request": taken, "state": row["state"], "targets": targets, "heard_ms": ms(heard), "on_it_ms": ms(on_it), "plan_line_ms": ms(plan), "sent_ms": ms(sent), "answer": row["answer"]});

        // AC-166: a sample of ten, one attempt each; a message to a wrong agent fails the criterion.
        let all: Vec<String> = agents.iter().map(|(_, t)| t.clone()).collect();
        let sample: Vec<(&str, Vec<&str>)> = vec![
            (
                "Tell Continuity to use the new wire format.",
                vec!["Continuity"],
            ),
            (
                "The phone app should add tests for the gateway.",
                vec!["Phone app"],
            ),
            (
                "Ask auto routing for a short report on what it changed.",
                vec!["Auto routing"],
            ),
            (
                "The landing page needs a darker hero section.",
                vec!["Landing page"],
            ),
            (
                "Docs cleanup should also fix the intro page.",
                vec!["Docs cleanup"],
            ),
            (
                "Swarm mode should hold off on new branches for now.",
                vec!["Swarm mode"],
            ),
            (
                "Phone and Continuity should both rebase onto main.",
                vec!["Phone app", "Continuity"],
            ),
            (
                "Tell them also to update the ledger.",
                vec!["Phone app", "Continuity"],
            ),
            (
                "Everybody, pull main before you push.",
                all.iter().map(String::as_str).collect(),
            ),
            ("Please add tests.", vec![]),
        ];
        let mut results = Vec::new();
        let mut wrong = 0;
        for (text, expected) in &sample {
            let r = d.call("voice.say", json!({"text": text}));
            let id = r["request"].as_str().unwrap_or("").to_string();
            let (row, targets) = if id.is_empty() {
                (r.clone(), vec![])
            } else {
                settled(&d, &id, 150)
            };
            let extra: Vec<&String> = targets
                .iter()
                .filter(|t| !expected.contains(&t.as_str()))
                .collect();
            let missing: Vec<&&str> = expected
                .iter()
                .filter(|e| !targets.iter().any(|t| t == **e))
                .collect();
            if !extra.is_empty() {
                wrong += 1;
            }
            results.push(json!({"said": text, "expected": expected, "sent_to": targets, "state": row["state"], "answer": row["answer"], "match": extra.is_empty() && missing.is_empty(), "wrong_agent": !extra.is_empty()}));
        }
        out["ac166"] = json!({"sample": results, "messages_to_a_wrong_agent": wrong});
    }
    // AC-168: a tiny live start with the composer's choices (Claude, Haiku).
    let before = d.call("state", json!({}))["runs"].as_array().unwrap().len();
    let r = d.call(
        "voice.say",
        json!({"text": "Someone should write a one-line NOTES.md in the website repository that says hello."}),
    );
    let id = r["request"].as_str().unwrap_or("").to_string();
    let (row, targets) = settled(&d, &id, 150);
    let runs = d.call("state", json!({}))["runs"]
        .as_array()
        .unwrap()
        .clone();
    let new: Vec<&Value> = runs
        .iter()
        .filter(|x| x["title"] != "Talk to Overseer")
        .skip(agents.len())
        .collect();
    let started = new
        .first()
        .map(|x| x["id"].as_str().unwrap_or("").to_string());
    if let Some(run) = &started {
        d.wait_status(
            run,
            |s| !["queued", "starting", "running"].contains(&s),
            180,
        );
    }
    out["ac168"] = json!({"state": row["state"], "answer": row["answer"], "targets": targets, "runs_before": before, "runs_after": runs.len(),
        "started": started.as_ref().map(|r| { let x = d.run(r); json!({"harness": x["harness"], "model": x["model"], "status": x["status"], "title": x["title"]}) })});

    for (id, _) in &agents {
        let _ = d.try_call("run.interrupt", json!({"run_id": id}));
    }
    let name = if only.is_empty() {
        "live.json".to_string()
    } else {
        format!("live-{only}.json")
    };
    let path = repo_root()
        .join("docs/verification/evidence/voice")
        .join(name);
    std::fs::write(&path, serde_json::to_string_pretty(&out).unwrap()).unwrap();
    eprintln!("{}", serde_json::to_string_pretty(&out).unwrap());
}
