//! Overseer with Continuity (AC-197, and AC-183's handed-off run): an agent that is handed off
//! stays one agent to Overseer, a redirect sent while an agent waits for a connection arrives
//! once, Overseer's own run and a watcher follow Continuity keeping their role and their
//! read-only launch, and with every provider failing and no local model the daemon's half keeps
//! working. Everything outside the daemon is SYNTHETIC: the network is a file, Codex and Claude
//! Code are fixtures that act out a script and fail on command
//! (fixtures/fake-harness/continuity-harness.js), no model runs and no account is used.

mod common;
#[path = "common/ollama.rs"]
mod ollama;
#[path = "common/world.rs"]
mod world;

use common::*;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};
use world::*;

const ONLINE: &str = r#"{"system": "connected", "baseline": {"by_name": true, "by_ip": true}, "providers": {"openai": true, "anthropic": true}}"#;
const OFFLINE: &str = r#"{"system": "none"}"#;
const OPENAI_DOWN: &str = r#"{"system": "connected", "baseline": {"by_name": true, "by_ip": true}, "providers": {"openai": "connect", "anthropic": true}}"#;

struct Lab {
    w: World,
    d: Daemon,
    _r: tempfile::TempDir,
    repo: PathBuf,
}

/// No Ollama answers and OpenCode is not installed: no local model can take any work.
fn lab() -> Lab {
    let w = World::new();
    let harness = repo_root().join("fixtures/fake-harness/continuity-harness.js");
    let (control, log) = (w.file("harness.json"), w.file("harness.log"));
    std::fs::write(&control, "{}").unwrap();
    let system = w.file("system-home");
    std::fs::create_dir_all(system.join(".codex")).unwrap();
    let env: Vec<(&str, &str)> = vec![
        ("OVERSEER_TEST_SYSTEM_HOME", system.to_str().unwrap()),
        ("OVERSEER_CODEX_PATH", harness.to_str().unwrap()),
        ("OVERSEER_CLAUDE_PATH", harness.to_str().unwrap()),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "CONTINUITY_FIXTURE,CONTINUITY_LOG"),
        ("CONTINUITY_FIXTURE", control.to_str().unwrap()),
        ("CONTINUITY_LOG", log.to_str().unwrap()),
        ("OVERSEER_TEST_RETRY_BASE_MS", "100"),
        ("OVERSEER_TEST_RETRY_CAP_MS", "400"),
        ("OVERSEER_TEST_STALL_MS", "60000"),
    ];
    let d = w.start(&no_ollama(), &env);
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    Lab { w, d, _r: r, repo }
}

impl Lab {
    fn net(&self, text: &str) {
        self.w.net(serde_json::from_str(text).unwrap());
    }
    fn online(&self) {
        self.net(ONLINE);
        wait_conn(&self.d, "online", |s| s["state"] == "online" && s["providers"]["openai"]["reachable"] == true);
    }
    fn offline(&self) {
        self.net(OFFLINE);
        wait_conn(&self.d, "offline", |s| s["state"] == "offline");
    }
    fn openai_down(&self) {
        self.net(OPENAI_DOWN);
        wait_conn(&self.d, "OpenAI unreachable", is("degraded", "OpenAI unreachable"));
    }
    fn behave(&self, codex: &str, claude: &str) {
        write_whole(&self.w.file("harness.json"), &json!({"codex": codex, "claude": claude}).to_string());
    }
    fn continuity(&self, on: bool) {
        self.d.call("settings.set", json!({"values": {"enabled": on}}));
    }
    fn start(&self, harness: &str, prompt: &str) -> String {
        let c = self.d.call("task.create", json!({"repo": self.repo, "harness": harness, "prompt": prompt, "title": prompt}));
        assert_eq!(c["launch_error"], Value::Null, "{c}");
        run_id(&c)
    }
    fn until(&self, what: &str, secs: u64, mut pred: impl FnMut() -> bool) {
        let at = Instant::now();
        while !pred() {
            assert!(at.elapsed() < Duration::from_secs(secs), "never {what}");
            std::thread::sleep(Duration::from_millis(40));
        }
    }
    /// A run, Overseer's own (hidden from the agents lists) included.
    fn run(&self, id: &str) -> Value {
        self.d.call("state", json!({"include_hidden": true}))["runs"].as_array().unwrap().iter().find(|r| r["id"] == id).cloned().unwrap_or_else(|| panic!("no run {id}"))
    }
    fn status(&self, run: &str, status: &str) -> Value {
        self.until(&format!("{run} {status} (it is {})", self.run(run)["status"]), 30, || self.run(run)["status"] == status);
        self.run(run)
    }
    fn handoffs(&self) -> Vec<Value> {
        self.d.call("continuity.handoffs", json!({}))["handoffs"].as_array().unwrap().clone()
    }
    fn successor(&self, run: &str) -> String {
        let mut found = None;
        self.until(&format!("{run} handed off"), 30, || {
            found = self.handoffs().iter().find(|h| h["predecessor"] == run).map(|h| h["successor"].as_str().unwrap().to_string());
            found.is_some()
        });
        found.unwrap()
    }
    /// Every start of the fixture as `who` (codex or claude), from its own log.
    fn starts(&self, who: &str) -> Vec<Value> {
        std::fs::read_to_string(self.w.file("harness.log")).unwrap_or_default().lines().map(|l| serde_json::from_str::<Value>(l).unwrap()).filter(|s| s["who"] == who).collect()
    }
    fn turns(&self, run: &str) -> Vec<Value> {
        self.d.call("run.turns", json!({"run_id": run})).as_array().unwrap().clone()
    }
    fn kinds(&self, run: &str, kind: &str) -> Vec<Value> {
        self.d.events(run).into_iter().filter(|e| e["kind"] == kind).map(|e| e["payload"].clone()).collect()
    }
    fn digest(&self, run: &str) -> Value {
        self.d.call("agent.digest", json!({"run_id": run}))
    }
    fn session(&self) -> Value {
        self.d.call("overseer.session", json!({}))
    }
    fn sql(&self, statement: &str) {
        let db = self.d.home.path().join("overseer.sqlite");
        let out = Command::new("sqlite3").args(["-cmd", ".timeout 5000"]).arg(&db).arg(statement).output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    }
}

fn args_of(start: &Value) -> Vec<String> {
    start["args"].as_array().unwrap().iter().map(|a| a.as_str().unwrap().to_string()).collect()
}

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).map(|i| args[i + 1].clone())
}

/// The token a Claude Code launch was given for the daemon's tools, from its MCP configuration.
fn mcp_token(args: &[String]) -> String {
    let config: Value = serde_json::from_str(&std::fs::read_to_string(flag(args, "--mcp-config").expect("an MCP configuration")).unwrap()).unwrap();
    config["mcpServers"]["overseer"]["env"]["OVERSEER_MCP_TOKEN"].as_str().unwrap().to_string()
}

/// A handoff of a held, watched agent with an area and a guardrail: the successor is held,
/// watched, owns the area and keeps the guardrail; the watcher, handed off in turn, stays a
/// read-only watcher of the same subject; the digest and roster read one agent.
#[test]
fn ac197_a_handed_off_agent_stays_one_agent_to_overseer() {
    let l = lab();
    l.d.call("overseer.session", json!({}));
    l.d.call("agent.cadence", json!({"cadence": "off", "by": "owner"}));
    // OpenAI cannot be reached and Continuity is off: the Codex agent waits with its message.
    l.continuity(false);
    l.openai_down();
    l.behave("network", "ok");
    let prompt = "write moved.txt carried on; say done";
    let a = l.start("codex", prompt);
    l.status(&a, "waiting_for_connection");
    // Held, watched (by a Codex watcher), with an area and a guardrail.
    l.d.call("agent.area", json!({"run_id": a, "paths": ["src"], "by": "owner"}));
    l.d.call("agent.hold", json!({"run_id": a, "reason": "wait for the review", "by": "owner"}));
    l.d.call("agent.guardrail", json!({"run_id": a, "words": "Leave the secrets alone.", "deny": ["secrets"], "by": "owner"}));
    let watch = l.d.call("watch.start", json!({"subject": a, "brief": "look for edits outside src", "harness": "codex", "by": "owner"}));
    let watch_id = watch["id"].as_str().unwrap().to_string();

    // Continuity on: the work moves to Claude Code, and Overseer's view moves with it.
    l.continuity(true);
    let b = l.successor(&a);
    let done = l.status(&b, "completed");
    assert_eq!(done["harness"], "claude");
    assert_eq!(std::fs::read_to_string(PathBuf::from(l.digest(&b)["digest"]["worktree"].as_str().unwrap()).join("moved.txt")).unwrap(), "carried on\n");
    let holds = l.d.call("agent.holds", json!({}))["holds"].as_array().unwrap().clone();
    assert_eq!(holds.iter().map(|h| h["run_id"].as_str().unwrap()).collect::<Vec<_>>(), [b.as_str()], "the successor is held, the predecessor is not");
    assert_eq!(holds[0]["reason"], "wait for the review");
    let guardrails = l.d.call("agent.guardrails", json!({"run_id": b}))["guardrails"].as_array().unwrap().clone();
    assert_eq!(guardrails.len(), 1);
    assert!(l.d.call("agent.guardrails", json!({"run_id": a}))["guardrails"].as_array().unwrap().is_empty());
    let w = l.d.call("watch.list", json!({"open_only": true}))["watches"].as_array().unwrap().iter().find(|w| w["id"] == watch_id.as_str()).cloned().expect("the watch is still open");
    assert_eq!(w["subject"], b.as_str(), "the successor is watched");
    let state = l.d.call("state", json!({}));
    let o = &state["oversight"][&b];
    assert_eq!((o["held"].as_bool(), o["watched"].as_bool(), o["area"].clone()), (Some(true), Some(true), json!(["src"])), "{o}");
    assert!(state["oversight"][&a].is_null(), "nothing is left on the predecessor: {}", state["oversight"]);
    let moved = l.kinds(&b, "oversight_moved");
    assert_eq!(moved.len(), 1);
    assert_eq!(moved[0]["from"], a.as_str());
    // The successor's first turn carried the guardrail: its words, and Claude Code's deny rules.
    let first = l.starts("claude").into_iter().find(|s| s["prompt"].as_str().unwrap().contains("You are continuing a task")).expect("the successor's start");
    assert!(first["prompt"].as_str().unwrap().starts_with("[Guardrails from Overseer: Leave the secrets alone. Do not change: secrets.]"), "{}", first["prompt"]);
    assert!(flag(&args_of(&first), "--disallowedTools").unwrap().contains("Edit(secrets/**)"), "{:?}", args_of(&first));

    // Held: a message waits behind the hold, and goes after the release.
    assert_eq!(l.d.call("run.queue", json!({"run_id": b, "text": "write after.txt later; say done", "source": "owner"}))["delivery"], "queued");
    std::thread::sleep(Duration::from_millis(800));
    assert_eq!(l.turns(&b).len(), 1, "a held successor starts no turn from a queued message");
    l.d.call("agent.release", json!({"run_id": b, "by": "owner"}));
    l.until("the queued message delivered after the release", 30, || l.turns(&b).len() == 2);
    l.status(&b, "completed");
    assert_eq!(l.d.call("run.queued", json!({"run_id": b}))["queued"].as_array().unwrap().len(), 0);

    // One agent in the digest and the roster (AC-183: a handed-off run carried on by its successor).
    let db = l.digest(&b)["digest"].clone();
    assert_eq!(db["continued_from"], json!([a]));
    assert_eq!(db["area"], json!(["src"]));
    assert_eq!(db["guardrails"].as_array().unwrap().len(), 1);
    assert_eq!(db["guardrails"][0]["deny"], json!(["secrets"]));
    assert_eq!(db["asked"][0]["text"], prompt, "what was asked starts with the task the predecessor was given: {}", db["asked"]);
    assert_eq!(db["asked"][1]["source"], "handoff");
    assert!(db["last_messages"].as_array().unwrap().iter().any(|m| m.as_str().unwrap().starts_with("claude fixture: done")), "{}", db["last_messages"]);
    assert!(db["watches"].as_array().unwrap().iter().any(|w| w["id"] == watch_id.as_str()));
    let text = l.digest(&b)["text"].as_str().unwrap().to_string();
    assert!(text.contains(&format!("continued from: {a}")) && text.contains("area: src") && text.contains("guardrail (enforced): Leave the secrets alone."), "{text}");
    assert_eq!(l.digest(&a)["digest"]["handed_off_to"], b.as_str());
    let roster: Vec<String> = l.d.call("agents.roster", json!({}))["roster"].as_array().unwrap().iter().map(|r| r["id"].as_str().unwrap().to_string()).collect();
    assert!(roster.contains(&b) && !roster.contains(&a), "one line for the agent, its successor's: {roster:?}");

    // The watcher woke on the successor's turn; on Codex it could not reach OpenAI either, so it
    // was handed off to Claude Code: still the watch's, still a watcher, still read-only.
    let mut watcher = String::new();
    l.until("the watcher handed off to Claude Code", 40, || {
        let w = l.d.call("watch.list", json!({}))["watches"].as_array().unwrap().iter().find(|w| w["id"] == watch_id.as_str()).cloned().unwrap();
        watcher = w["watcher"].as_str().unwrap_or("").to_string();
        !watcher.is_empty() && l.run(&watcher)["harness"] == "claude"
    });
    let before = l.handoffs().into_iter().find(|h| h["successor"] == watcher.as_str()).expect("the watcher's handoff")["predecessor"].as_str().unwrap().to_string();
    assert_eq!(l.run(&before)["harness"], "codex");
    let role_moved = l.kinds(&watcher, "role_moved");
    assert_eq!((role_moved[0]["role"].as_str(), role_moved[0]["read_only"].as_bool()), (Some("watcher"), Some(true)), "{role_moved:?}");
    assert_eq!(l.d.call("state", json!({}))["oversight"][&watcher]["role"], "watcher");
    // The successor exists before its process has started: wait for Claude Code's own start.
    let watcher_start = || l.starts("claude").into_iter().find(|s| s["prompt"].as_str().unwrap().contains("[Watch "));
    l.until("the watcher's start on Claude Code", 30, || watcher_start().is_some());
    let start = watcher_start().unwrap();
    let args = args_of(&start);
    let denied = flag(&args, "--disallowedTools").expect("read-only");
    for tool in ["Bash", "Edit", "Write", "WebFetch"] {
        assert!(denied.split(',').any(|t| t == tool), "{tool} is refused to the watcher: {denied}");
    }
    assert!(flag(&args, "--allowedTools").unwrap().contains("mcp__overseer__finding"));
    // Its new token speaks for the watcher's successor and reads only the subject; the old one is gone.
    let token = mcp_token(&args);
    let tools = l.d.call("overseer.tools", json!({"token": token}))["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap().to_string()).collect::<Vec<_>>();
    assert!(tools.contains(&"finding".to_string()) && !tools.contains(&"propose".to_string()), "{tools:?}");
    let read = l.d.call("overseer.tool", json!({"token": token, "name": "agent", "arguments": {"id": b}}));
    assert_eq!(read["is_error"], false, "{read}");
    let refused = l.d.call("overseer.tool", json!({"token": token, "name": "file", "arguments": {"id": before, "path": "moved.txt"}}));
    assert!(refused["text"].as_str().unwrap().starts_with("refused: a watcher reads only its subject"), "{refused}");
}

/// A redirect sent while an agent waits for a connection takes the place of the message it kept:
/// nothing is launched while it waits, and the direction arrives once when the connection
/// returns (or, when the work moves, as the successor's pending message). A plain message waits
/// behind it and arrives once too.
#[test]
fn ac197_a_redirect_sent_while_waiting_arrives_once() {
    let l = lab();
    l.d.call("overseer.session", json!({}));
    l.d.call("agent.cadence", json!({"cadence": "off", "by": "owner"}));
    l.continuity(false);
    l.offline();
    l.behave("network", "ok");
    let a = l.start("codex", "write first.txt the old direction; say done");
    l.status(&a, "waiting_for_connection");
    let launched = l.starts("codex").len();
    let r = l.d.call("run.redirect", json!({"run_id": a, "text": "write second.txt the new direction; say done"}));
    assert_eq!(r["delivery"], "when the agent can run again", "{r}");
    assert_eq!(l.d.call("run.queue", json!({"run_id": a, "text": "write third.txt afterwards; say done"}))["delivery"], "queued");
    std::thread::sleep(Duration::from_millis(900));
    assert_eq!(l.starts("codex").len(), launched, "nothing is launched while the agent waits");
    assert_eq!(l.run(&a)["status"], "waiting_for_connection");
    let redirect = &l.kinds(&a, "redirect")[0];
    assert_eq!((redirect["waiting"].as_bool(), redirect["stopped"].as_bool()), (Some(true), Some(false)));
    let turns = l.turns(&a);
    assert_eq!(turns.iter().map(|t| t["status"].as_str().unwrap()).collect::<Vec<_>>(), ["interrupted", "waiting"], "the direction took the place of the kept message");
    assert_eq!(l.d.call("continuity.waits", json!({}))["waits"][0]["turn_id"], turns[1]["id"]);

    // The connection returns: the direction goes once, then the message behind it, once.
    l.behave("ok", "ok");
    l.online();
    l.until("both delivered", 30, || l.turns(&a).len() == 3 && l.run(&a)["status"] == "completed");
    let prompts: Vec<String> = l.starts("codex")[launched..].iter().map(|s| s["prompt"].as_str().unwrap().to_string()).collect();
    assert_eq!(prompts.iter().filter(|p| p.contains("second.txt")).count(), 1, "{prompts:?}");
    assert_eq!(prompts.iter().filter(|p| p.contains("third.txt")).count(), 1, "{prompts:?}");
    assert!(!prompts.iter().any(|p| p.contains("first.txt")), "the replaced message is never sent: {prompts:?}");
    let ws = PathBuf::from(l.digest(&a)["digest"]["worktree"].as_str().unwrap());
    assert!(ws.join("second.txt").exists() && ws.join("third.txt").exists() && !ws.join("first.txt").exists());
    assert_eq!(l.kinds(&a, "retry").iter().filter(|r| r["sending"] == true).count(), 1);
    std::thread::sleep(Duration::from_millis(600));
    assert_eq!(l.turns(&a).len(), 3, "and nothing is sent again");

    // Redirected while waiting, then handed off: the successor gets the direction, once.
    l.continuity(false);
    l.openai_down();
    l.behave("network", "ok");
    let c = l.start("codex", "write fifth.txt the old plan; say done");
    l.status(&c, "waiting_for_connection");
    l.d.call("run.redirect", json!({"run_id": c, "text": "write fourth.txt redirected; say done"}));
    l.continuity(true);
    let next = l.successor(&c);
    l.status(&next, "completed");
    let ws = PathBuf::from(l.digest(&next)["digest"]["worktree"].as_str().unwrap());
    assert!(ws.join("fourth.txt").exists() && !ws.join("fifth.txt").exists());
    let claude: Vec<Value> = l.starts("claude").into_iter().filter(|s| s["prompt"].as_str().unwrap().contains("fourth.txt")).collect();
    assert_eq!(claude.len(), 1);
    assert!(claude[0]["prompt"].as_str().unwrap().contains("The user's last message, not yet answered: write fourth.txt redirected; say done"), "{}", claude[0]["prompt"]);
}

/// Overseer's own run follows Continuity like any run: on Codex while OpenAI cannot be reached it
/// is handed off to Claude Code and stays Overseer's run (hidden from the agents, the
/// conversation's, read-only with the daemon's tools), and answers there.
#[test]
fn ac197_overseers_own_run_follows_continuity() {
    let l = lab();
    l.d.call("overseer.session", json!({}));
    l.d.call("agent.cadence", json!({"cadence": "off", "by": "owner"}));
    l.openai_down();
    l.behave("network", "ok");
    l.d.call("overseer.send", json!({"text": "What is everyone doing?", "surface": "ctl", "harness": "codex"}));
    let first = l.session()["run_id"].as_str().unwrap().to_string();
    let next = l.successor(&first);
    l.status(&next, "completed");
    let s = l.session();
    assert_eq!(s["run_id"], next.as_str(), "the conversation follows the successor");
    assert_eq!(l.run(&next)["harness"], "claude");
    assert!(s["messages"].as_array().unwrap().iter().any(|m| m["source"] == "overseer" && m["text"].as_str().unwrap().starts_with("claude fixture: done")), "Overseer answered from its successor: {}", s["messages"]);
    // Hidden like Overseer's first run: in no agents list and not in the roster.
    let state = l.d.call("state", json!({}));
    assert!(!state["runs"].as_array().unwrap().iter().any(|r| r["id"] == next.as_str() || r["id"] == first.as_str()), "{}", state["runs"]);
    let roster = l.d.call("agents.roster", json!({}))["text"].as_str().unwrap().to_string();
    assert!(!roster.contains(&next) && !roster.contains(&first), "{roster}");
    // Read-only with the daemon's tools, on a token of its own; the old token speaks for nobody.
    let start = l.starts("claude").into_iter().find(|s| s["prompt"].as_str().unwrap().contains("You are continuing a task")).unwrap();
    let args = args_of(&start);
    assert!(flag(&args, "--disallowedTools").unwrap().split(',').any(|t| t == "Bash"));
    assert!(flag(&args, "--allowedTools").unwrap().contains("mcp__overseer__propose"));
    let token = mcp_token(&args);
    let tools = l.d.call("overseer.tools", json!({"token": token}))["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap().to_string()).collect::<Vec<_>>();
    assert!(tools.contains(&"propose".to_string()), "{tools:?}");
    let old = l.starts("codex").into_iter().find(|s| s["prompt"].as_str().unwrap().contains("What is everyone doing?")).unwrap();
    let old_args = args_of(&old);
    let old_token = old_args.iter().find_map(|a| a.strip_prefix("mcp_servers.overseer.env={ OVERSEER_MCP_TOKEN = \"").map(|t| t.trim_end_matches("\" }").to_string())).expect("Codex was given a token");
    assert!(l.d.try_call("overseer.tools", json!({"token": old_token})).unwrap_err().contains("unknown token"));
    // The next message is Overseer's next turn on the successor.
    l.behave("ok", "ok");
    let n = l.turns(&next).len();
    l.d.call("overseer.send", json!({"text": "And now?", "surface": "ctl"}));
    l.until("the next turn on the successor", 30, || l.turns(&next).len() == n + 1 && l.run(&next)["status"] == "completed");
    assert_eq!(l.session()["run_id"], next.as_str());
}

/// With every provider failing and no local model: Overseer's run waits and the conversation says
/// why; a conflict is still found and resolved from its card; hold and release work; stop
/// everyone stops four agents from one proposal.
#[test]
fn ac197_every_provider_failing_and_no_local_model() {
    let l = lab();
    std::fs::write(l.repo.join("shared.txt"), "one\ntwo\nthree\n").unwrap();
    git(&l.repo, &["add", "shared.txt"]);
    git(&l.repo, &["commit", "-qm", "shared"]);
    l.d.call("overseer.session", json!({}));
    l.d.call("agent.cadence", json!({"cadence": "off", "by": "owner"}));
    l.offline();
    l.behave("network", "network");
    l.d.call("overseer.send", json!({"text": "What is everyone doing?", "surface": "ctl", "harness": "claude"}));
    let mut said = Value::Null;
    l.until("the conversation gives the reason", 20, || {
        said = l.session()["messages"].as_array().unwrap().iter().find(|m| m["card"]["kind"] == "cannot_answer").cloned().unwrap_or(Value::Null);
        !said.is_null()
    });
    let text = said["text"].as_str().unwrap();
    assert!(text.starts_with("Overseer cannot answer right now: ") && text.contains("the connection to Claude failed") && text.contains("keeps working"), "{text}");
    assert_eq!(said["card"]["waiting"], true);
    let s = l.session();
    assert_eq!(s["run_status"], "waiting_for_connection");
    assert!(l.handoffs().is_empty(), "no local model: nothing to hand off to");
    // A conflict is found and resolved from its card.
    let a = run_id(&l.d.generic(&l.repo, "worktree", "/bin/sh", &["-c", "printf 'one\\nA\\nthree\\n' > shared.txt; sleep 60"]));
    let b = run_id(&l.d.generic(&l.repo, "worktree", "/bin/sh", &["-c", "printf 'one\\nB\\nthree\\n' > shared.txt; sleep 60"]));
    let mut conflict = Value::Null;
    l.until("a same-lines conflict", 20, || {
        conflict = l.d.call("conflicts.list", json!({}))["conflicts"].as_array().unwrap().iter().find(|c| c["kind"] == "same_lines").cloned().unwrap_or(Value::Null);
        !conflict.is_null()
    });
    let resolved = l.d.call("conflict.resolve", json!({"id": conflict["id"], "how": "assign", "keeper": a, "by": "owner"}));
    assert_eq!(resolved["state"], "resolved", "{resolved}");
    assert!(!l.d.call("agent.guardrails", json!({"run_id": b}))["guardrails"].as_array().unwrap().is_empty());
    l.d.call("agent.hold", json!({"run_id": a, "reason": "wait", "by": "owner"}));
    assert!(l.d.call("agent.holds", json!({}))["holds"].as_array().unwrap().iter().any(|h| h["run_id"] == a.as_str()));
    l.d.call("agent.release", json!({"run_id": a, "by": "owner"}));
    // Stop everyone: four agents, one proposal, one yes.
    let mut four = vec![a.clone(), b.clone()];
    for _ in 0..2 {
        four.push(run_id(&l.d.generic(&l.repo, "worktree", "/bin/sh", &["-c", "sleep 60"])));
    }
    for id in &four {
        l.d.wait_status(id, |s| s == "running", 20);
    }
    l.sql("UPDATE overseer_sessions SET last_cause='owner';");
    let stops: Vec<Value> = four.iter().map(|id| json!({"action": "stop", "agent": id})).collect();
    let p = l.d.call("overseer.propose", json!({"actions": stops, "source": "ctl"}));
    l.d.call("overseer.answer", json!({"id": p["proposal"], "yes": true, "surface": "ctl", "by": "owner"}));
    for id in &four {
        l.d.wait_status(id, |s| s == "interrupted", 10);
    }
    // Overseer's run still waits, with the owner's words kept.
    assert_eq!(l.session()["run_status"], "waiting_for_connection");
}
