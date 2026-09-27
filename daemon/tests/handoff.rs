//! Keeping the work going (Continuity, AC-84, AC-91 to AC-93, AC-96, AC-138 and the valve of
//! AC-140) against the real daemon, its real bridge and supervisors. Everything outside the
//! daemon is SYNTHETIC: the network and the memory are files, Ollama is a loopback server, and
//! Codex, Claude Code and OpenCode are fixtures that act out a script and fail on command
//! (fixtures/fake-harness/continuity-harness.js, opencode-serve-fixture.js). No model runs, no
//! account is used, and time is shortened: the first look at a waiting run is 100 ms away and
//! the backoff stops at 400 ms.

mod common;
#[path = "common/ollama.rs"]
mod ollama;
#[path = "common/world.rs"]
mod world;

use common::*;
use ollama::Ollama;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use world::*;

struct Lab {
    w: World,
    o: Ollama,
    d: Daemon,
    _r: tempfile::TempDir,
    repo: PathBuf,
}

const ONLINE: &str = r#"{"system": "connected", "baseline": {"by_name": true, "by_ip": true}, "providers": {"openai": true, "anthropic": true}}"#;
const OFFLINE: &str = r#"{"system": "none"}"#;
const OPENAI_DOWN: &str = r#"{"system": "connected", "baseline": {"by_name": true, "by_ip": true}, "providers": {"openai": "connect", "anthropic": true}}"#;
const CLAUDE_DOWN: &str = r#"{"system": "connected", "baseline": {"by_name": true, "by_ip": true}, "providers": {"openai": true, "anthropic": "connect"}}"#;
const BOTH_DOWN: &str = r#"{"system": "connected", "baseline": {"by_name": true, "by_ip": true}, "providers": {"openai": "connect", "anthropic": "timeout"}}"#;

fn lab_with(models: Vec<(Value, Value)>) -> Lab {
    lab_in(models, &[])
}

fn lab_in(models: Vec<(Value, Value)>, more: &[(&str, &str)]) -> Lab {
    let o = Ollama::start();
    for m in models {
        o.install(m);
    }
    o.state.lock().unwrap().loaded_size.insert("qwen3-coder:30b-64k".into(), 25_411_736_042);
    let w = World::new();
    let harness = repo_root().join("fixtures/fake-harness/continuity-harness.js");
    let opencode = repo_root().join("fixtures/fake-harness/opencode-serve-fixture.js");
    let (control, log) = (w.file("harness.json"), w.file("harness.log"));
    std::fs::write(&control, "{}").unwrap();
    // The logins the desktop apps share are a folder of the test, never the user's own.
    let system = w.file("system-home");
    std::fs::create_dir_all(system.join(".codex")).unwrap();
    let mut env: Vec<(&str, &str)> = vec![("OVERSEER_TEST_SYSTEM_HOME", system.to_str().unwrap())];
    env.extend_from_slice(
        &[
            ("OVERSEER_CODEX_PATH", harness.to_str().unwrap()),
            ("OVERSEER_CLAUDE_PATH", harness.to_str().unwrap()),
            ("OVERSEER_OPENCODE_PATH", opencode.to_str().unwrap()),
            ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "CONTINUITY_FIXTURE,CONTINUITY_LOG"),
            ("CONTINUITY_FIXTURE", control.to_str().unwrap()),
            ("CONTINUITY_LOG", log.to_str().unwrap()),
            ("OVERSEER_TEST_RETRY_BASE_MS", "100"),
            ("OVERSEER_TEST_RETRY_CAP_MS", "400"),
            // Silence counts as a stall only after a minute, so that a slow start on a busy
            // machine is not taken for one; the stall test shortens it.
            ("OVERSEER_TEST_STALL_MS", "60000"),
        ],
    );
    env.retain(|(name, _)| !more.iter().any(|(k, _)| k == name));
    env.extend_from_slice(more);
    let d = w.start(&o.url(), &env);
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    Lab { w, o, d, _r: r, repo }
}

fn lab() -> Lab {
    lab_with(vec![ollama::qwen3_coder_30b(), ollama::qwen3_coder_30b_64k(), ollama::qwen25_coder_14b()])
}

impl Lab {
    fn net(&self, text: &str) {
        self.w.net(serde_json::from_str(text).unwrap());
    }
    fn offline(&self) {
        self.net(OFFLINE);
        wait_conn(&self.d, "offline", |s| s["state"] == "offline");
    }
    fn online(&self) {
        self.net(ONLINE);
        wait_conn(&self.d, "online", |s| s["state"] == "online" && s["providers"]["openai"]["reachable"] == true);
    }
    /// How Codex and Claude Code behave from now on: ok, network, outage or stall.
    fn behave(&self, codex: &str, claude: &str) {
        write_whole(&self.w.file("harness.json"), &json!({"codex": codex, "claude": claude}).to_string());
    }
    fn start(&self, harness: &str, prompt: &str, mode: Option<&str>) -> Value {
        let mut p = json!({"repo": self.repo, "harness": harness, "prompt": prompt, "title": prompt});
        if let Some(m) = mode {
            p["permission_mode"] = json!(m);
        }
        let c = self.d.call("task.create", p);
        assert_eq!(c["launch_error"], Value::Null, "{c}");
        c
    }
    fn until(&self, run: &str, what: &str, pred: impl Fn(&Value) -> bool) -> Value {
        let at = Instant::now();
        loop {
            let r = self.d.run(run);
            if pred(&r) {
                return r;
            }
            assert!(at.elapsed() < Duration::from_secs(30), "run {run} never was {what}: it is {} ({})", r["status"], r["exit_reason"]);
            std::thread::sleep(Duration::from_millis(30));
        }
    }
    fn status(&self, run: &str, status: &'static str) -> Value {
        self.until(run, status, move |r| r["status"] == status)
    }
    /// Every start of Codex or Claude Code, from the fixture's own log.
    fn starts(&self, who: &str) -> Vec<Value> {
        std::fs::read_to_string(self.w.file("harness.log")).unwrap_or_default().lines().map(|l| serde_json::from_str::<Value>(l).unwrap()).filter(|s| s["who"] == who).collect()
    }
    fn asked_of_opencode(&self) -> Vec<Value> {
        let log = self.d.home.path().join("profiles/local-ollama/data/opencode/fixture-requests.jsonl");
        std::fs::read_to_string(log).unwrap_or_default().lines().map(|l| serde_json::from_str(l).unwrap()).collect()
    }
    fn kinds(&self, run: &str, kind: &str) -> Vec<Value> {
        self.d.events(run).into_iter().filter(|e| e["kind"] == kind).map(|e| e["payload"].clone()).collect()
    }
    fn said(&self, run: &str, role: &str) -> Vec<String> {
        self.kinds(run, "output").iter().filter(|p| p["role"] == role).map(|p| p["text"].as_str().unwrap().to_string()).collect()
    }
    fn turns(&self, run: &str) -> Vec<Value> {
        self.d.call("run.turns", json!({"run_id": run})).as_array().unwrap().clone()
    }
    fn handoffs(&self) -> Vec<Value> {
        self.d.call("continuity.handoffs", json!({}))["handoffs"].as_array().unwrap().clone()
    }
    fn successor(&self, run: &str) -> String {
        let at = Instant::now();
        loop {
            if let Some(h) = self.handoffs().iter().find(|h| h["predecessor"] == run) {
                return h["successor"].as_str().unwrap().to_string();
            }
            assert!(at.elapsed() < Duration::from_secs(30), "run {run} was never handed off: {}", self.d.run(run));
            std::thread::sleep(Duration::from_millis(30));
        }
    }
    fn workspace(&self, created: &Value) -> PathBuf {
        ws_path(&self.d, created)
    }
    fn degraded(&self, net: &str, reason: &'static str) {
        self.net(net);
        wait_conn(&self.d, reason, is("degraded", reason));
    }
    /// Waits for something in the run's events.
    fn event(&self, run: &str, kind: &str, what: &str, pred: impl Fn(&Value) -> bool) -> Value {
        let at = Instant::now();
        loop {
            if let Some(e) = self.kinds(run, kind).into_iter().find(|e| pred(e)) {
                return e;
            }
            assert!(at.elapsed() < Duration::from_secs(20), "run {run} never had {what}: {:?}", self.kinds(run, kind));
            std::thread::sleep(Duration::from_millis(30));
        }
    }
    /// What the waiting run was offered instead of being moved.
    fn offers(&self, run: &str) -> Vec<Value> {
        self.event(run, "retry", "an offer", |r| r["offers"].as_array().is_some_and(|o| !o.is_empty()))["offers"].as_array().unwrap().clone()
    }
    fn set(&self, values: Value) {
        self.d.call("settings.set", json!({"values": values}));
    }
    /// A local agent in Auto, on the picked model or a named one.
    fn local(&self, prompt: &str, model: Option<&str>) -> Value {
        let mut p = json!({"repo": self.repo, "harness": "opencode-serve", "prompt": prompt, "title": prompt, "permission_mode": "auto"});
        if let Some(m) = model {
            p["model"] = json!(m);
        }
        self.d.call("task.create", p)
    }
    /// Sends a message and waits for its turn to end.
    fn follow_up(&self, run: &str, prompt: &str) -> Value {
        let n = self.turns(run).len();
        self.d.call("run.follow_up", json!({"run_id": run, "prompt": prompt}));
        self.until(run, "done with its next turn", |_| self.turns(run).len() > n);
        self.until(run, "done with its next turn", |r| !["queued", "starting", "running"].contains(&r["status"].as_str().unwrap()))
    }
    /// Loads Ollama was asked for (unloads are not counted).
    fn loads(&self) -> Vec<String> {
        self.o.asked("/api/generate").into_iter().filter(|b| b["keep_alive"] != 0).map(|b| b["model"].as_str().unwrap().to_string()).collect()
    }
    /// What Codex reported about an account's limits, as its session log keeps it.
    fn quota(&self, codex_home: &std::path::Path, used_percent: f64, limited: bool) {
        let day = codex_home.join("sessions/2026/09/26");
        std::fs::create_dir_all(&day).unwrap();
        let mut limits = json!({"primary": {"used_percent": used_percent, "window_minutes": 300, "resets_at": 1790413619}, "plan_type": "team"});
        if limited {
            limits["rate_limit_reached_type"] = json!("primary");
        }
        write_whole(&day.join("rollout-fixture.jsonl"), &format!("{}\n", json!({"type": "event_msg", "payload": {"type": "token_count", "rate_limits": limits}})));
    }
}

// ---------------------------------------------------------------- AC-92

#[test]
fn ac92_wait_and_retry_never_fail() {
    let l = lab();
    l.d.call("settings.set", json!({"values": {"enabled": false}}));
    l.offline();
    l.behave("network", "ok");
    let prompt = "write retried.txt hello after the wait; say done";
    let c = l.start("codex", prompt, None);
    let run = run_id(&c);

    // The turn failed on the connection: the run waits, it does not fail, and its message is kept.
    let r = l.status(&run, "waiting_for_connection");
    assert!(r["exit_reason"].as_str().unwrap().starts_with("the connection to OpenAI failed: stream disconnected"), "{r}");
    assert_eq!(r["ended_ms"], Value::Null);
    let turns = l.turns(&run);
    assert_eq!((turns.len(), turns[0]["status"].as_str(), turns[0]["prompt"].as_str()), (1, Some("waiting"), Some(prompt)));
    let parked = l.kinds(&run, "status").into_iter().find(|s| s["status"] == "waiting_for_connection").unwrap();
    assert_eq!(parked["message_kept"], true);
    let session = r["native_id"].as_str().unwrap().to_string();

    // It is looked at with a backoff, and nothing is sent while the connection is gone.
    std::thread::sleep(Duration::from_millis(2200));
    let checks: Vec<Value> = l.kinds(&run, "retry");
    assert!(checks.len() >= 4 && checks.iter().all(|c| c["sending"] == false && c["continuity"] == false), "{checks:?}");
    for (i, c) in checks.iter().enumerate() {
        let attempt = (i + 1) as u32;
        assert_eq!(c["attempt"], attempt);
        let plain = (100u64 << attempt).min(400) as f64;
        let next = c["next_in_ms"].as_f64().unwrap();
        assert!((plain * 0.8 - 1.0..=plain * 1.2 + 1.0).contains(&next), "attempt {attempt}: {next} ms is not {plain} ms within a fifth");
        assert_eq!(c["gives_up_after_hours"], 36);
    }
    assert_eq!(l.starts("codex").len(), 1, "Codex was not started again while offline");
    assert_eq!(l.d.call("continuity.waits", json!({}))["waits"][0]["run_id"], run.as_str());

    // The connection returns: the same turn is sent once, through the harness's own resume.
    l.behave("ok", "ok");
    l.net(ONLINE);
    let done = l.status(&run, "completed");
    assert_eq!(done["native_id"], session.as_str());
    let starts = l.starts("codex");
    assert_eq!(starts.len(), 2, "exactly one more start");
    assert_eq!(starts[1]["prompt"], prompt, "the message that was kept");
    let args: Vec<&str> = starts[1]["args"].as_array().unwrap().iter().map(|a| a.as_str().unwrap()).collect();
    assert_eq!(&args[..3], ["exec", "resume", session.as_str()], "through Codex's own resume");
    let turns = l.turns(&run);
    assert_eq!((turns.len(), turns[0]["status"].as_str()), (1, Some("completed")), "no second turn");
    assert_eq!(l.kinds(&run, "turn_started").len(), 1);
    assert_eq!(l.kinds(&run, "retry").iter().filter(|c| c["sending"] == true).count(), 1);
    assert_eq!(std::fs::read_to_string(l.workspace(&c).join("retried.txt")).unwrap(), "hello after the wait\n");
    assert_eq!(l.said(&run, "assistant"), ["codex fixture: done (retried.txt)"]);
    assert!(l.d.call("continuity.waits", json!({}))["waits"].as_array().unwrap().is_empty());

    // A retry that fails again goes back to waiting, with the next delay.
    l.behave("network", "ok");
    let c2 = l.start("codex", "say again", None);
    let run2 = run_id(&c2);
    l.status(&run2, "waiting_for_connection");
    let at = Instant::now();
    while l.starts("codex").iter().filter(|s| s["prompt"] == "say again").count() < 3 {
        assert!(at.elapsed() < Duration::from_secs(10), "while online a failed turn is tried again: {:?}", l.kinds(&run2, "retry"));
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(l.turns(&run2).len(), 1);
    // Stop ends the wait; nothing is started afterwards.
    l.status(&run2, "waiting_for_connection");
    l.d.call("run.interrupt", json!({"run_id": run2}));
    let stopped = l.d.run(&run2);
    assert_eq!((stopped["status"].as_str(), stopped["exit_reason"].as_str()), (Some("interrupted"), Some("stopped by the user while waiting")));
    let n = l.starts("codex").len();
    std::thread::sleep(Duration::from_millis(900));
    assert_eq!(l.starts("codex").len(), n);

    // After 36 hours without a connection the run fails with the reason, keeps its message, and
    // offers to try again.
    l.offline();
    let c3 = l.start("codex", "write late.txt better late; say done", None);
    let run3 = run_id(&c3);
    l.status(&run3, "waiting_for_connection");
    l.d.call("continuity.test_age", json!({"run_id": run3, "by_ms": 36 * 3_600_000 - 5_000}));
    assert_eq!(l.d.run(&run3)["status"], "waiting_for_connection", "five seconds before the limit it still waits");
    let aged = l.d.call("continuity.test_age", json!({"run_id": run3, "by_ms": 10_000}))["run"].clone();
    assert_eq!((aged["status"].as_str(), aged["exit_reason"].as_str()), (Some("failed"), Some("no connection for 36 hours")));
    assert_eq!(aged["attention"], json!({"kind": "connection", "reason": "no connection for 36 hours", "message_kept": true, "turn": l.turns(&run3)[0]["id"], "actions": ["retry_now", "use_local"]}));
    l.behave("ok", "ok");
    l.online();
    l.d.call("run.retry_now", json!({"run_id": run3}));
    assert_eq!(l.status(&run3, "completed")["attention"], Value::Null);
    assert!(l.workspace(&c3).join("late.txt").exists());
    assert_eq!(l.turns(&run3).len(), 1);

    // "Use a local model now": with Continuity off the move is offered, and made at the user's word.
    l.offline();
    l.behave("network", "ok");
    let c4 = l.start("codex", "write now.txt local at once; say done", None);
    let run4 = run_id(&c4);
    l.status(&run4, "waiting_for_connection");
    assert_eq!(l.offers(&run4), vec![json!({"to": "local", "label": "qwen3-coder:30b", "account": null, "mode": "acceptEdits", "difference": null})]);
    assert!(l.handoffs().is_empty() && l.loads().is_empty(), "nothing moved and nothing was loaded on its own");
    let moved = l.d.call("run.handoff", json!({"run_id": run4, "to": "local"}));
    let next = moved["successor"]["id"].as_str().unwrap().to_string();
    assert_eq!(l.status(&next, "completed")["harness"], "opencode-serve");
    assert_eq!(std::fs::read_to_string(l.workspace(&c4).join("now.txt")).unwrap(), "local at once\n");
    assert_eq!(l.said(&run4, "system").last().unwrap(), "Moving to **qwen3-coder:30b** (local, Ollama) as you asked. Work continues in the same worktree.");
    assert_eq!(l.d.run(&run4)["status"], "handed_off");
    // New agents started offline are local ones.
    assert_eq!(l.d.call("continuity.status", json!({}))["new_agents"]["local_only"], true);
}

// ---------------------------------------------------------------- AC-91

#[test]
fn ac91_transition_to_local_when_offline() {
    let l = lab();
    l.offline();
    l.behave("network", "ok");
    let prompt = "write handoff.txt hello from local; say done";
    let at = Instant::now();
    let c = l.start("codex", prompt, None);
    let first = run_id(&c);
    let next = l.successor(&first);
    let took = at.elapsed();
    assert!(took < Duration::from_secs(30), "the local run started {took:?} after the turn failed");

    // The successor is a local run in the same task and the same worktree.
    let done = l.status(&next, "completed");
    let before = l.d.run(&first);
    assert_eq!((done["harness"].as_str(), done["model"].as_str(), done["profile_id"].as_str()), (Some("opencode-serve"), Some("ollama/qwen3-coder:30b-64k"), Some("local-ollama")));
    assert_eq!((done["task_id"].clone(), done["workspace_id"].clone()), (before["task_id"].clone(), before["workspace_id"].clone()));
    assert_eq!(std::fs::read_to_string(l.workspace(&c).join("handoff.txt")).unwrap(), "hello from local\n");
    // The predecessor was handed off; it did not fail.
    assert_eq!((before["status"].as_str(), before["exit_reason"].as_str()), (Some("handed_off"), Some(format!("handed off to {next} (offline)").as_str())));
    assert!(before["ended_ms"].is_i64());
    assert!(!l.kinds(&first, "status").iter().any(|s| s["status"] == "failed"));
    assert_eq!(l.handoffs(), vec![json!({"predecessor": first, "successor": next, "reason": "offline", "at_ms": l.handoffs()[0]["at_ms"], "stay": false, "offered_ms": null})]);

    // Both chats say what happened, in the owner's words.
    assert_eq!(l.said(&first, "system").last().unwrap(), "Transitioning to **qwen3-coder:30b** (local, Ollama) because you've disconnected. Work continues in the same worktree.");
    let opened = l.said(&next, "system");
    assert!(opened[0].starts_with(&format!("Continued from \"{prompt}\" after the connection was lost at ")) && opened[0].ends_with('.'), "{opened:?}");
    let h = &l.kinds(&next, "handoff")[0];
    assert_eq!((h["predecessor"].as_str(), h["reason"].as_str(), h["target"]["to"].as_str(), h["connection"]["state"].as_str()), (Some(first.as_str()), Some("offline"), Some("local"), Some("offline")));
    assert_eq!(l.kinds(&first, "handoff")[0]["successor"], next.as_str());
    assert_eq!(l.said(&next, "assistant"), ["done"]);

    // The handoff prompt is built from the record.
    let sent = l.asked_of_opencode().into_iter().find(|r| r["path"].as_str().is_some_and(|p| p.ends_with("/prompt_async"))).unwrap();
    let text = sent["body"]["parts"][0]["text"].as_str().unwrap();
    assert!(text.starts_with(&format!("You are continuing a task another agent started; its model became unreachable.\nTask: {prompt}\n")), "{text}");
    assert!(text.contains(&format!("working tree {}", l.workspace(&c).display())) && text.contains("Do not change branches."));
    assert!(text.contains("The previous agent had not reported anything yet.") && text.ends_with("do not redo finished work."));
    // Codex ran in its sandbox, which edits without asking: the local run edits without asking
    // and would ask before a command. Never looser.
    let session = l.asked_of_opencode().into_iter().find(|r| r["method"] == "POST" && r["path"] == "/session").unwrap();
    let rule = |p: &str| session["body"]["permission"].as_array().unwrap().iter().find(|r| r["permission"] == p).map(|r| r["action"].clone());
    assert_eq!((rule("edit"), rule("bash"), rule("question")), (Some(json!("allow")), Some(json!("ask")), Some(json!("deny"))));
    // One writer at a time: Codex had ended before the local run was launched.
    assert_eq!(l.starts("codex").len(), 1);
    let ws = l.d.call("state", json!({}))["workspaces"].as_array().unwrap().iter().find(|w| w["id"] == done["workspace_id"]).cloned().unwrap();
    assert_eq!(ws["owner_run_id"], Value::Null, "the worktree is free again when the work is done");
    // The model passed the guard and was loaded under the watchdog.
    assert_eq!(l.kinds(&next, "local_model")[0]["auto"], true);
    assert_eq!(l.o.asked("/api/generate").len(), 1);
    // A message to the agent that handed off goes to the agent that has the work now.
    let turn = l.d.call("run.follow_up", json!({"run_id": first, "prompt": "write again.txt more; say done"}));
    assert_eq!(turn["run_id"], next.as_str(), "{turn}");
    l.until(&next, "done with the message", |r| r["status"] == "completed" && l.turns(&next).len() == 2);
    assert_eq!(std::fs::read_to_string(l.workspace(&c).join("again.txt")).unwrap(), "more\n");
    assert_eq!(l.turns(&first).len(), 1, "the handed-off agent got no turn of its own");

    // A read-only Codex run becomes Plan only, and changes nothing.
    let c = l.start("codex", "write never.txt nope; say blocked", Some("read-only"));
    let next = l.successor(&run_id(&c));
    assert_eq!(l.status(&next, "completed")["harness"], "opencode-serve");
    assert!(!l.workspace(&c).join("never.txt").exists());
    assert_eq!(l.asked_of_opencode().iter().filter(|r| r["method"] == "POST" && r["path"] == "/session").next_back().unwrap()["body"]["agent"], "plan");

    // A Claude Code run in Ask first keeps asking on the local model (AC-138).
    l.behave("ok", "network");
    let c = l.start("claude", "write asked.txt hello; say done", Some("manual"));
    let next = l.successor(&run_id(&c));
    let ask = l.status(&next, "waiting_for_user")["attention"].clone();
    assert_eq!(ask["tool"], "edit: asked.txt");
    assert!(!l.workspace(&c).join("asked.txt").exists());
    l.d.call("run.permission", json!({"run_id": next, "request_id": ask["request_id"], "allow": true}));
    l.status(&next, "completed");
    assert!(l.workspace(&c).join("asked.txt").exists());
    assert_eq!(l.said(&run_id(&c), "system").last().unwrap(), "Transitioning to **qwen3-coder:30b** (local, Ollama) because you've disconnected. Work continues in the same worktree.");
}

#[test]
fn ac91_without_a_local_model_it_says_why_and_waits() {
    // Only a model that failed its check is installed.
    let l = lab_with(vec![ollama::qwen25_coder_14b()]);
    l.offline();
    l.behave("network", "ok");
    let c = l.start("codex", "write later.txt hello; say done", None);
    let run = run_id(&c);
    l.status(&run, "waiting_for_connection");
    let at = Instant::now();
    let note = loop {
        if let Some(n) = l.kinds(&run, "retry").iter().find_map(|r| r["note"].as_str().map(str::to_string)) {
            break n;
        }
        assert!(at.elapsed() < Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(50));
    };
    assert_eq!(note, "no local model is installed that is verified and fits the memory budget of 51.2 GiB (qwen3-coder:30b: not installed, and downloads are off or the registry cannot be reached); downloads need a connection");
    assert!(l.handoffs().is_empty() && l.asked_of_opencode().is_empty() && l.o.asked("/api/generate").is_empty());
    assert_eq!(l.d.run(&run)["status"], "waiting_for_connection");
    // The model arrives (it was installed by other means): the next look hands the work off.
    l.o.install(ollama::qwen3_coder_30b()).install(ollama::qwen3_coder_30b_64k());
    let next = l.successor(&run);
    l.status(&next, "completed");
    assert!(l.workspace(&c).join("later.txt").exists());
}

#[test]
fn ac91_a_turn_that_stalls_while_offline_is_interrupted_and_handed_off() {
    let l = lab_in(vec![ollama::qwen3_coder_30b(), ollama::qwen3_coder_30b_64k(), ollama::qwen25_coder_14b()], &[("OVERSEER_TEST_STALL_MS", "500")]);
    l.behave("stall", "ok");
    let c = l.start("codex", "write stalled.txt hello; say done", None);
    let run = run_id(&c);
    l.status(&run, "running");
    std::thread::sleep(Duration::from_millis(900));
    assert_eq!(l.d.run(&run)["status"], "running", "silence while online is not a stall");
    assert!(l.kinds(&run, "stall").is_empty());
    l.net(OFFLINE);
    let next = l.successor(&run);
    let stall = &l.kinds(&run, "stall")[0];
    assert_eq!((stall["action"].as_str(), stall["limit_ms"].as_i64(), stall["connection"].as_str()), (Some("interrupted by Overseer"), Some(500), Some("no network (system)")));
    assert!(stall["silent_ms"].as_i64().unwrap() >= 500);
    assert!(l.kinds(&run, "interrupt_requested").is_empty(), "it is Overseer's action, recorded as such, not the user's");
    let parked = l.kinds(&run, "status").into_iter().find(|s| s["status"] == "waiting_for_connection").unwrap();
    assert_eq!(parked["reason"], "no answer while offline; the turn was interrupted by Overseer");
    l.status(&next, "completed");
    assert_eq!(l.d.run(&run)["status"], "handed_off");
    assert!(l.workspace(&c).join("stalled.txt").exists());
}

// ---------------------------------------------------------------- AC-84

#[test]
fn ac84_fail_over_to_the_best_working_provider() {
    let l = lab();
    l.degraded(OPENAI_DOWN, "OpenAI unreachable");
    l.behave("network", "ok");
    let prompt = "write failover.txt hello from the other provider; say done";
    let at = Instant::now();
    let c = l.start("codex", prompt, None);
    let first = run_id(&c);
    let next = l.successor(&first);
    assert!(at.elapsed() < Duration::from_secs(30), "the successor started {:?} after the turn failed", at.elapsed());

    // The successor runs on the other provider, in the same task and the same worktree.
    let done = l.status(&next, "completed");
    let before = l.d.run(&first);
    assert_eq!((done["harness"].as_str(), done["profile_id"].as_str()), (Some("claude"), Some("system-claude")));
    assert_eq!((done["task_id"].clone(), done["workspace_id"].clone()), (before["task_id"].clone(), before["workspace_id"].clone()));
    assert_eq!(std::fs::read_to_string(l.workspace(&c).join("failover.txt")).unwrap(), "hello from the other provider\n");
    let since_start = option(&l.d, &next, "task_start", None);
    assert_eq!(diff_paths(&l.d, &c, since_start["base"].as_str().unwrap()), vec![("A".to_string(), "failover.txt".to_string())], "the review of the task shows the successor's work in the same worktree");
    // The predecessor was handed off; it did not fail.
    assert_eq!((before["status"].as_str(), before["exit_reason"].as_str()), (Some("handed_off"), Some(format!("handed off to {next} (provider_unreachable:openai)").as_str())));
    assert!(!l.kinds(&first, "status").iter().any(|s| s["status"] == "failed"));
    // Both chats say what happened and why.
    assert_eq!(l.said(&first, "system").last().unwrap(), "OpenAI is unreachable; continuing with **Claude Code** (account \"claude (existing login)\") because it is the best working option.");
    assert!(l.said(&next, "system")[0].starts_with(&format!("Continued from \"{prompt}\" after the connection was lost at ")));
    let h = &l.kinds(&next, "handoff")[0];
    assert_eq!((h["reason"].as_str(), h["target"]["to"].as_str(), h["connection"]["state"].as_str(), h["connection"]["reason"].as_str()), (Some("provider_unreachable:openai"), Some("anthropic"), Some("degraded"), Some("OpenAI unreachable")));
    // Codex edits in its sandbox without asking; Claude Code edits without asking and asks before
    // a command. Stricter, never looser.
    let started = l.starts("claude");
    let args: Vec<&str> = started[0]["args"].as_array().unwrap().iter().map(|a| a.as_str().unwrap()).collect();
    assert!(args.windows(2).any(|w| w == ["--permission-mode", "acceptEdits"]), "{args:?}");
    assert!(started[0]["prompt"].as_str().unwrap().starts_with("You are continuing a task another agent started; its model became unreachable.\n"));
    // Local is used only when no online provider works: nothing was loaded, OpenCode never ran.
    assert!(l.loads().is_empty() && l.asked_of_opencode().is_empty());

    // Every provider failing: no failover, and the offline policy applies.
    l.degraded(BOTH_DOWN, "Claude and OpenAI unreachable");
    l.behave("network", "network");
    let c = l.start("codex", "write both.txt local took over; say done", None);
    let first = run_id(&c);
    let next = l.successor(&first);
    assert_eq!(l.status(&next, "completed")["harness"], "opencode-serve");
    assert_eq!(l.said(&first, "system").last().unwrap(), "Transitioning to **qwen3-coder:30b** (local, Ollama) because no provider can be reached. Work continues in the same worktree.");
    assert!(!l.starts("claude").iter().any(|s| s["prompt"].as_str().unwrap().contains("both.txt")), "Claude Code was not tried while it cannot be reached");
    let local_run = next;

    // Claude Code in Ask first cannot be kept by Codex, which does not ask: the move is offered
    // with the difference, and nothing moves on its own.
    l.degraded(CLAUDE_DOWN, "Claude unreachable");
    l.behave("ok", "network");
    let c = l.start("claude", "write strict.txt only when asked; say done", Some("manual"));
    let run = run_id(&c);
    l.status(&run, "waiting_for_connection");
    assert_eq!(l.offers(&run), vec![json!({"to": "openai", "label": "Codex", "account": "codex (existing login)", "mode": "workspace-write", "difference": "Codex edits files and runs commands in its sandbox without asking first"})]);
    std::thread::sleep(Duration::from_millis(700));
    assert!(!l.handoffs().iter().any(|h| h["predecessor"] == run.as_str()));
    assert_eq!(l.d.run(&run)["status"], "waiting_for_connection");
    let refused = l.d.try_call("run.handoff", json!({"run_id": run, "to": "openai"})).unwrap_err();
    assert!(refused.contains("Codex edits files and runs commands in its sandbox without asking first; to continue, accept the mode workspace-write"), "{refused}");
    let moved = l.d.call("run.handoff", json!({"run_id": run, "to": "openai", "accept_mode": "workspace-write"}));
    let next = moved["successor"]["id"].as_str().unwrap().to_string();
    assert_eq!(l.status(&next, "completed")["harness"], "codex");
    assert_eq!(l.d.run(&run)["exit_reason"], format!("handed off to {next} (user)").as_str());
    assert!(l.workspace(&c).join("strict.txt").exists());
    // Plan only can be kept (read only), so that run moves on its own.
    let c = l.start("claude", "say what you would do", Some("plan"));
    let next = l.successor(&run_id(&c));
    l.status(&next, "completed");
    let started = l.starts("codex").into_iter().next_back().unwrap();
    assert!(started["args"].as_array().unwrap().iter().any(|a| a.as_str().unwrap().contains("read-only")), "{started}");

    // With two accounts of the target provider the one with the most quota left is taken; an
    // account at its limit is not; with the same quota, the one used last.
    let a = l.d.call("profile.create", json!({"name": "Work", "harness": "codex"}));
    let b = l.d.call("profile.create", json!({"name": "Personal", "harness": "codex"}));
    let home = |p: &Value| PathBuf::from(p["home"].as_str().unwrap()).join("codex");
    l.quota(&l.w.file("system-home").join(".codex"), 90.0, false);
    l.quota(&home(&a), 20.0, false);
    l.quota(&home(&b), 70.0, false);
    let moved_to = |prompt: &str| {
        let c = l.start("claude", prompt, Some("auto"));
        let first = run_id(&c);
        let next = l.successor(&first);
        let done = l.status(&next, "completed");
        (done["profile_id"].as_str().unwrap().to_string(), l.said(&first, "system").last().unwrap().clone())
    };
    assert_eq!(moved_to("write q1.txt one; say done"), (a["id"].as_str().unwrap().to_string(), "Claude is unreachable; continuing with **Codex** (account \"Work\") because it is the best working option.".to_string()));
    l.quota(&home(&a), 100.0, true);
    assert_eq!(moved_to("write q2.txt two; say done").0, b["id"].as_str().unwrap());
    l.quota(&home(&a), 50.0, false);
    l.quota(&home(&b), 50.0, false);
    l.quota(&l.w.file("system-home").join(".codex"), 50.0, false);
    assert_eq!(moved_to("write q3.txt three; say done").0, b["id"].as_str().unwrap(), "the same quota left: the account used last");

    // The order is the owner's when both alternatives work.
    l.behave("ok", "ok");
    l.online();
    let order = |l: &Lab| -> Vec<String> { l.d.call("run.targets", json!({"run_id": local_run}))["online"].as_array().unwrap().iter().map(|t| t["to"].as_str().unwrap().to_string()).collect() };
    assert_eq!(order(&l), ["openai", "anthropic"]);
    l.set(json!({"providerOrder": ["anthropic", "openai"]}));
    assert_eq!(order(&l), ["anthropic", "openai"]);
    l.set(json!({"providerOrder": ["openai", "anthropic"]}));
    // The user may still take the second one.
    let moved = l.d.call("run.handoff", json!({"run_id": local_run, "to": "anthropic"}));
    assert_eq!(l.status(moved["successor"]["id"].as_str().unwrap(), "completed")["harness"], "claude");

    // With Continuity off the run waits (AC-92) and is offered the other provider.
    l.set(json!({"enabled": false}));
    l.degraded(OPENAI_DOWN, "OpenAI unreachable");
    l.behave("network", "ok");
    let c = l.start("codex", "write offered.txt by request; say done", None);
    let run = run_id(&c);
    l.status(&run, "waiting_for_connection");
    let offers = l.offers(&run);
    assert_eq!((offers.len(), offers[0]["to"].as_str(), offers[0]["label"].as_str(), offers[0]["difference"].clone()), (1, Some("anthropic"), Some("Claude Code"), Value::Null));
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(l.d.run(&run)["status"], "waiting_for_connection");
    assert!(!l.handoffs().iter().any(|h| h["predecessor"] == run.as_str()));
}

// ---------------------------------------------------------------- AC-93

#[test]
fn ac93_back_online() {
    let l = lab();
    l.offline();
    l.behave("network", "ok");
    let c = l.start("codex", "write local.txt made offline; say done", None);
    let first = run_id(&c);
    let session = l.status(&first, "handed_off")["native_id"].as_str().unwrap().to_string();
    let local = l.successor(&first);
    l.status(&local, "completed");
    // Offline, a new agent can only be a local one, and the reason is given.
    let new = l.d.call("continuity.status", json!({}))["new_agents"].clone();
    assert_eq!((new["local_only"].clone(), new["default"]["harness"].as_str(), new["default"]["why"].as_str()), (json!(true), Some("opencode-serve"), Some("offline: no network (system)")));
    assert_eq!(new["harnesses"][0], json!({"harness": "codex", "provider": "openai", "usable": false, "why": "offline: no network (system)"}));

    // The connection returns: the way back is offered once, and nothing moves on its own.
    l.behave("ok", "ok");
    l.online();
    let offer = l.event(&local, "back_online", "the offer", |_| true);
    assert_eq!(offer, json!({"return_online": "offer", "back_to": {"harness": "codex", "label": "Codex", "run": first}, "offer": true, "at_next_turn": false}));
    assert_eq!(l.said(&local, "system").last().unwrap(), "Back online. This agent is still on a local model.");
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(l.kinds(&local, "back_online").len(), 1, "said once");
    assert_eq!(l.d.run(&local)["harness"], "opencode-serve");
    // New agents default to the online harness the user last chose.
    let new = l.d.call("continuity.status", json!({}))["new_agents"].clone();
    assert_eq!((new["local_only"].clone(), new["default"]["harness"].as_str(), new["default"]["local"].clone(), new["default"]["profile_id"].clone()), (json!(false), Some("codex"), json!(false), l.d.run(&first)["profile_id"].clone()));

    // Stay local keeps working locally.
    l.d.call("run.stay", json!({"run_id": local}));
    let after = l.follow_up(&local, "write more.txt still local; say done");
    assert_eq!((after["status"].as_str(), after["harness"].as_str()), (Some("completed"), Some("opencode-serve")));
    assert!(l.workspace(&c).join("more.txt").exists());
    assert_eq!(l.handoffs().len(), 1);

    // Switch back continues in the original harness: the same worktree and its own session.
    let moved = l.d.call("run.handoff", json!({"run_id": local, "to": "back"}));
    let back = moved["successor"]["id"].as_str().unwrap().to_string();
    let done = l.status(&back, "completed");
    assert_eq!((done["harness"].as_str(), done["native_id"].as_str(), done["workspace_id"].clone(), done["profile_id"].clone()), (Some("codex"), Some(session.as_str()), l.d.run(&first)["workspace_id"].clone(), l.d.run(&first)["profile_id"].clone()));
    let started = l.starts("codex").into_iter().next_back().unwrap();
    let args: Vec<&str> = started["args"].as_array().unwrap().iter().map(|a| a.as_str().unwrap()).collect();
    assert_eq!(&args[..3], ["exec", "resume", session.as_str()]);
    let text = started["prompt"].as_str().unwrap();
    assert!(text.starts_with("You are continuing a task another agent started; the connection is back and the work returns to its first agent.\n"), "{text}");
    assert!(text.contains("Done so far (the previous agent's last messages, newest last):\n- done\n- done\n") && text.contains("local.txt") && text.contains("more.txt"), "what was done locally is told: {text}");
    assert_eq!(l.said(&local, "system").last().unwrap(), "Back online. Continuing with **Codex**.");
    assert!(l.said(&back, "system")[0].starts_with("Continued from \"write local.txt made offline; say done\" now that the connection is back."));
    let left = l.d.run(&local);
    assert_eq!((left["status"].as_str(), left["exit_reason"].as_str()), (Some("handed_off"), Some(format!("handed off to {back} (back_online)").as_str())));

    // With returnOnline set to auto the next message goes back on its own, and says so. The
    // first agent had no session yet, so the local work is summarised to a new one.
    l.set(json!({"returnOnline": "auto"}));
    l.offline();
    l.behave("early", "ok");
    let c = l.start("codex", "write auto.txt first part; say done", None);
    let first = run_id(&c);
    let local = l.successor(&first);
    l.status(&local, "completed");
    assert_eq!(l.d.run(&first)["native_id"], Value::Null);
    l.behave("ok", "ok");
    l.online();
    let offer = l.event(&local, "back_online", "the way back", |_| true);
    assert_eq!((offer["offer"].clone(), offer["at_next_turn"].clone()), (json!(false), json!(true)));
    assert_eq!(l.d.run(&local)["harness"], "opencode-serve", "the turn that ended is left alone");
    let turn = l.d.call("run.follow_up", json!({"run_id": local, "prompt": "write auto2.txt second part; say done"}));
    let back = turn["run_id"].as_str().unwrap().to_string();
    assert_ne!(back, local);
    let done = l.status(&back, "completed");
    assert_eq!((done["harness"].as_str(), done["workspace_id"].clone()), (Some("codex"), l.d.run(&first)["workspace_id"].clone()));
    assert_eq!(std::fs::read_to_string(l.workspace(&c).join("auto2.txt")).unwrap(), "second part\n");
    assert_eq!(l.said(&local, "system").last().unwrap(), "Back online. Continuing with **Codex**.");
    let started = l.starts("codex").into_iter().next_back().unwrap();
    assert_eq!(started["args"][1], "--json", "a new session: {started}");
    let text = started["prompt"].as_str().unwrap();
    assert!(text.contains("Done so far") && text.contains("Files changed since the task started") && text.contains("auto.txt"));
    assert!(text.contains("The user's last message, not yet answered: write auto2.txt second part; say done\n"));
    assert_eq!(l.d.run(&local)["status"], "handed_off");
    // A run that chose to stay is not moved by auto.
    l.offline();
    l.behave("network", "ok");
    let c = l.start("codex", "write stay.txt one; say done", None);
    let local = l.successor(&run_id(&c));
    l.status(&local, "completed");
    l.behave("ok", "ok");
    l.online();
    l.event(&local, "back_online", "the way back", |_| true);
    l.d.call("run.stay", json!({"run_id": local}));
    let after = l.follow_up(&local, "write stay2.txt two; say done");
    assert_eq!((after["status"].as_str(), after["harness"].as_str()), (Some("completed"), Some("opencode-serve")));
}

// ---------------------------------------------------------------- AC-96

#[test]
fn ac96_several_local_agents_share_one_model() {
    let l = lab_with(vec![ollama::qwen3_coder_30b(), ollama::qwen3_coder_30b_64k(), ollama::qwen25_coder_14b(), ollama::qwen25_coder_32b()]);
    l.o.machine(&l.w.file("memory.json"), 128.0, 12.8);
    // Each agent works until the test lets it finish.
    let go = l.w.file("go");
    let started: Vec<Value> = ["a", "b", "c"].iter().map(|n| l.local(&format!("until {}; write {n}.txt {n}; say done", go.display()), None)).collect();
    let runs: Vec<String> = started.iter().map(|c| { assert_eq!(c["launch_error"], Value::Null, "{c}"); run_id(c) }).collect();
    for r in &runs {
        let now = l.d.run(r);
        assert!(["starting", "running"].contains(&now["status"].as_str().unwrap()), "all three work at once: {} ({})", now["status"], now["exit_reason"]);
    }
    // One copy of the model is loaded, once, and the agents that wait for it say so.
    assert_eq!(l.o.loaded(), ["qwen3-coder:30b-64k"]);
    assert_eq!(l.loads(), ["qwen3-coder:30b-64k"]);
    let queued = |r: &str| -> Vec<String> { l.said(r, "system").into_iter().filter(|t| t.starts_with("Queued")).collect() };
    assert!(queued(&runs[0]).is_empty());
    assert_eq!(queued(&runs[1]), ["Queued behind 1 local agent: they share one loaded model."]);
    assert_eq!(queued(&runs[2]), ["Queued behind 2 local agents: they share one loaded model."]);
    assert_eq!(l.kinds(&runs[2], "local_queue"), vec![json!({"behind": 2, "model": "ollama/qwen3-coder:30b-64k"})]);
    let models: Vec<(Value, Value)> = runs.iter().map(|r| { let m = l.kinds(r, "local_model")[0].clone(); (m["model"].clone(), m["already_loaded"].clone()) }).collect();
    assert_eq!(models, vec![(json!("ollama/qwen3-coder:30b-64k"), json!(false)), (json!("ollama/qwen3-coder:30b-64k"), json!(true)), (json!("ollama/qwen3-coder:30b-64k"), json!(true))]);

    // A second, different model loads only when both fit the share of memory.
    l.set(json!({"ramCeilingPercent": 30}));
    let refused = l.local("write big.txt no", Some("ollama/qwen2.5-coder:32b"));
    assert_eq!(refused["launch_error"], "qwen2.5-coder:32b does not fit beside qwen3-coder:30b-64k: 23.5 GiB and 23.7 GiB together are over 30% of 128 GiB (38.4 GiB)");
    assert_eq!(l.d.run(refused["run"]["id"].as_str().unwrap())["status"], "failed");
    assert_eq!(l.o.loaded(), ["qwen3-coder:30b-64k"]);
    let beside = l.local("write small.txt yes; say done", Some("ollama/qwen2.5-coder:14b"));
    assert_eq!(beside["launch_error"], Value::Null, "{beside}");
    let small = run_id(&beside);
    let m = l.kinds(&small, "local_model")[0].clone();
    assert_eq!((m["model"].as_str(), m["context"].as_u64()), (Some("ollama/overseer/qwen2.5-coder-14b:16k"), Some(16384)), "at 32k the two together would be over the share");
    assert_eq!(l.o.loaded(), ["qwen3-coder:30b-64k", "overseer/qwen2.5-coder-14b:16k"]);
    assert!(queued(&small).is_empty(), "it has its own model");
    std::fs::write(&go, "").unwrap();
    for ((r, c), name) in runs.iter().zip(&started).zip(["a", "b", "c"]) {
        assert_eq!(l.status(r, "completed")["status"], "completed");
        assert_eq!(std::fs::read_to_string(l.workspace(c).join(format!("{name}.txt"))).unwrap(), format!("{name}\n"));
    }
    l.status(&small, "completed");
    assert_eq!(l.loads(), ["qwen3-coder:30b-64k", "overseer/qwen2.5-coder-14b:16k"], "nothing was loaded twice");
}

#[test]
fn ac96_a_memory_squeeze_shrinks_the_next_pick_and_leaves_the_turn_alone() {
    let l = lab();
    l.o.state.lock().unwrap().loaded_size.insert("overseer/qwen3-coder-30b:32k".into(), 22_000_000_000);
    l.o.machine(&l.w.file("memory.json"), 128.0, 12.8);
    let c = l.local("write one.txt 1; sleep 2; write two.txt 2; say done", None);
    assert_eq!(c["launch_error"], Value::Null, "{c}");
    let run = run_id(&c);
    l.until(&run, "at work", |_| l.workspace(&c).join("one.txt").exists());
    // Everything else on the machine takes more: 11.3 GiB are left, under the headroom of 12.8.
    l.o.others(93.0, "normal");
    let done = l.status(&run, "completed");
    assert_eq!(done["model"], "ollama/qwen3-coder:30b-64k");
    assert!(l.workspace(&c).join("two.txt").exists(), "the turn that was running finished");
    assert!(l.kinds(&run, "memory_valve").is_empty() && l.kinds(&run, "stall").is_empty() && l.turns(&run).len() == 1);
    assert_eq!(l.o.loaded(), ["qwen3-coder:30b-64k"], "nothing is unloaded under a working turn");

    // The next turn: the same model at a shorter context, the old copy unloaded first, one note.
    let next = l.follow_up(&run, "write three.txt 3; say done");
    assert_eq!((next["status"].as_str(), next["model"].as_str()), (Some("completed"), Some("ollama/overseer/qwen3-coder-30b:32k")));
    assert!(l.workspace(&c).join("three.txt").exists());
    assert_eq!(l.o.loaded(), ["overseer/qwen3-coder-30b:32k"]);
    let picked = l.kinds(&run, "local_model");
    assert_eq!((picked[1]["context"].as_u64(), picked[1]["replaced"].clone(), picked[1]["auto"].clone()), (Some(32768), json!(["qwen3-coder:30b-64k"]), json!(true)));
    let asked: Vec<Value> = l.o.asked("/api/generate");
    let order: Vec<(String, bool)> = asked.iter().map(|b| (b["model"].as_str().unwrap().to_string(), b["keep_alive"] == 0)).collect();
    assert_eq!(order, [("qwen3-coder:30b-64k".to_string(), false), ("qwen3-coder:30b-64k".to_string(), true), ("overseer/qwen3-coder-30b:32k".to_string(), false)], "unloaded before the smaller copy was loaded");
    let notes = |l: &Lab| -> Vec<String> { l.said(&run, "system").into_iter().filter(|t| t.starts_with("Memory is tighter")).collect() };
    assert_eq!(notes(&l), ["Memory is tighter now (11.3 GiB available). This turn uses qwen3-coder:30b at a 32k context instead of a 64k context."]);
    // The same memory at the turn after that: the same copy, and no second note.
    let third = l.follow_up(&run, "write four.txt 4; say done");
    assert_eq!((third["status"].as_str(), third["model"].as_str()), (Some("completed"), Some("ollama/overseer/qwen3-coder-30b:32k")));
    assert_eq!((notes(&l).len(), l.loads().len()), (1, 2));

    // A copy the user loaded in their own Ollama is never unloaded to make room.
    let l = lab();
    l.o.machine(&l.w.file("memory.json"), 128.0, 12.8);
    l.o.set_loaded("qwen3-coder:30b-64k", 25_411_736_042, 65536);
    l.o.others(93.0, "normal");
    let c = l.local("write shared.txt 1; say done", None);
    assert_eq!(c["launch_error"], Value::Null, "{c}");
    let run = run_id(&c);
    assert_eq!(l.status(&run, "completed")["model"], "ollama/qwen3-coder:30b-64k");
    assert!(l.loads().is_empty() && l.o.asked("/api/generate").is_empty(), "it is shared as it is: nothing loaded, nothing unloaded");
    assert_eq!(l.kinds(&run, "local_model")[0]["already_loaded"], true);
}

// ---------------------------------------------------------------- AC-140 (the valve)

#[test]
fn ac140_critical_pressure_pauses_local_runs_and_resumes_them() {
    let l = lab();
    l.o.machine(&l.w.file("memory.json"), 128.0, 12.8);
    let prompt = "write before.txt one; sleep 3; write after.txt two; say done";
    let c = l.local(prompt, None);
    assert_eq!(c["launch_error"], Value::Null, "{c}");
    let run = run_id(&c);
    l.until(&run, "at work", |_| l.workspace(&c).join("before.txt").exists());
    l.o.others(12.8, "critical");
    let parked = l.status(&run, "waiting_for_memory");
    assert_eq!(parked["exit_reason"], "the system reported critical memory pressure; the local model was unloaded");
    assert_eq!(parked["ended_ms"], Value::Null);
    assert!(l.o.loaded().is_empty(), "/api/ps shows nothing loaded");
    assert_eq!(l.kinds(&run, "memory_valve"), vec![json!({"pressure": "critical", "action": "paused by Overseer; the message is kept", "model": "ollama/qwen3-coder:30b-64k"})]);
    assert!(l.kinds(&run, "interrupt_requested").is_empty(), "Overseer's action, not the user's");
    let turns = l.turns(&run);
    assert_eq!((turns.len(), turns[0]["status"].as_str(), turns[0]["prompt"].as_str()), (1, Some("waiting"), Some(prompt)));
    assert!(!l.workspace(&c).join("after.txt").exists());
    // While the pressure is critical nothing is loaded and nothing is sent.
    std::thread::sleep(Duration::from_millis(900));
    let checks = l.kinds(&run, "retry");
    assert!(!checks.is_empty() && checks.iter().all(|c| c["sending"] == false && c["note"] == "memory pressure is still critical"), "{checks:?}");
    assert_eq!(l.loads().len(), 1);
    let refused = l.local("write other.txt no", None);
    assert!(refused["launch_error"].as_str().unwrap().contains("critical memory pressure"), "{refused}");
    // The pressure is normal again: the turn is sent once, in the same session, and finishes.
    l.o.others(12.8, "normal");
    let done = l.status(&run, "completed");
    assert_eq!(done["native_id"], parked["native_id"]);
    assert!(l.workspace(&c).join("after.txt").exists());
    let turns = l.turns(&run);
    assert_eq!((turns.len(), turns[0]["status"].as_str()), (1, Some("completed")));
    assert_eq!(l.kinds(&run, "retry").iter().filter(|c| c["sending"] == true).count(), 1);
    assert_eq!(l.kinds(&run, "turn_started").len(), 1);
    let sent: Vec<Value> = l.asked_of_opencode().into_iter().filter(|r| r["path"].as_str().is_some_and(|p| p.ends_with("/prompt_async"))).collect();
    assert_eq!(sent.len(), 2, "once before the pause and once after it");
    assert_eq!(sent[0]["path"], sent[1]["path"], "the same session");
    assert_eq!(l.loads(), ["qwen3-coder:30b-64k", "qwen3-coder:30b-64k"], "loaded again through the guard");
    assert_eq!(l.o.loaded(), ["qwen3-coder:30b-64k"]);
}

// ---------------------------------------------------------------- AC-138 (an OpenCode without a server)

#[test]
fn ac138_without_the_server_a_local_agent_cannot_ask_so_the_move_is_offered() {
    let l = lab_in(vec![ollama::qwen3_coder_30b(), ollama::qwen3_coder_30b_64k()], &[("OVERSEER_TEST_OPENCODE_NO_SERVE", "1")]);
    l.offline();
    l.behave("ok", "network");
    // Ask first cannot be kept by an agent that cannot ask: offered with the difference, not done.
    let c = l.start("claude", "write careful.txt hello; say done", Some("manual"));
    let run = run_id(&c);
    l.status(&run, "waiting_for_connection");
    assert_eq!(l.offers(&run), vec![json!({"to": "local", "label": "qwen3-coder:30b", "account": null, "mode": "auto", "difference": "this OpenCode has no server, so the local agent cannot ask before it edits or runs a command"})]);
    std::thread::sleep(Duration::from_millis(600));
    assert!(l.handoffs().is_empty() && l.loads().is_empty());
    let refused = l.d.try_call("run.handoff", json!({"run_id": run, "to": "local"})).unwrap_err();
    assert!(refused.contains("cannot ask before it edits or runs a command; to continue, accept the mode auto"), "{refused}");
    let moved = l.d.call("run.handoff", json!({"run_id": run, "to": "local", "accept_mode": "auto"}));
    let next = moved["successor"]["id"].as_str().unwrap().to_string();
    let done = l.status(&next, "completed");
    assert_eq!((done["harness"].as_str(), done["profile_id"].as_str(), done["model"].as_str()), (Some("opencode"), Some("local-ollama"), Some("ollama/qwen3-coder:30b-64k")));
    assert_eq!(std::fs::read_to_string(l.workspace(&c).join("careful.txt")).unwrap(), "hello\n");
    assert_eq!(l.loads(), ["qwen3-coder:30b-64k"], "through the guard, as every local run");
    // A run that was in Auto loses nothing, so it moves on its own.
    let c = l.start("claude", "write free.txt hello; say done", Some("auto"));
    let next = l.successor(&run_id(&c));
    assert_eq!(l.status(&next, "completed")["harness"], "opencode");
    assert!(l.workspace(&c).join("free.txt").exists());
}

#[test]
fn ac84_an_agent_that_only_keeps_reconnecting_is_moved() {
    // The real Codex never fails its turn while its hosts are unreachable: it says "Reconnecting..."
    // for ever. With its provider unreachable and no progress, Overseer interrupts and moves the work.
    let l = lab_in(vec![ollama::qwen3_coder_30b(), ollama::qwen3_coder_30b_64k()], &[("OVERSEER_TEST_RECONNECT_MS", "800")]);
    l.behave("reconnect", "ok");
    let c = l.start("codex", "write reconnect.txt hello; say done", None);
    let run = run_id(&c);
    l.status(&run, "running");
    std::thread::sleep(Duration::from_millis(1500));
    assert_eq!(l.d.run(&run)["status"], "running", "reconnecting while the provider answers the probe is left alone");
    l.degraded(OPENAI_DOWN, "OpenAI unreachable");
    let next = l.successor(&run);
    let stall = &l.kinds(&run, "stall")[0];
    assert_eq!((stall["reason"].as_str(), stall["limit_ms"].as_i64()), (Some("no progress while OpenAI could not be reached; the turn was interrupted by Overseer"), Some(800)));
    assert!(stall["reconnecting_ms"].as_i64().unwrap() >= 800);
    let parked = l.kinds(&run, "status").into_iter().find(|s| s["status"] == "waiting_for_connection").unwrap();
    assert_eq!(parked["reason"], "no progress while OpenAI could not be reached; the turn was interrupted by Overseer");
    let done = l.status(&next, "completed");
    assert_eq!(done["harness"], "claude", "the work moved to the provider that can be reached");
    assert!(l.workspace(&c).join("reconnect.txt").exists());
    assert_eq!(l.d.run(&run)["status"], "handed_off");
}
