//! The `opencode-serve` harness (Continuity, AC-138): local models through OpenCode's headless
//! server, with permission modes. The spike (AC-139) chose this transport: permission rules
//! travel with the session, requests arrive as events and are answered once or rejected, an
//! interrupt is a request, and a session survives a restart of the server.
//!
//! A run's supervisor starts `overseerd opencode-bridge`, one process per turn like every other
//! harness. The bridge starts `opencode serve` on a loopback port of its own behind a password
//! only it knows, follows the server's events, and speaks a small line protocol with the daemon:
//!
//! - standard input, one JSON object per line: `{"op":"start",…}` first, then
//!   `{"op":"permission","id":…,"reply":"once"|"reject"}` and `{"op":"abort"}`;
//! - standard output, one JSON object per line with a `b` field (`ready`, `session`, `text`,
//!   `tool`, `permission`, `child`, `usage`, `error`, `note`, `turn_done`), which `parse` turns
//!   into the daemon's normalized events.
//!
//! The user's own OpenCode configuration is never read or written: a local run always uses
//! Overseer's own profile, whose configuration names only the local provider.

use crate::adapters::Norm;
use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

pub const HARNESS: &str = "opencode-serve";
pub const PROVIDER: &str = "ollama";
pub const LOCAL_PROFILE: &str = "local-ollama";

// ------------------------------------------------------------------ permission modes

/// Overseer's permission modes, the same names Claude Code's adapter uses, so a handoff from it
/// keeps its mode as it is.
pub const MODES: &[&str] = &["plan", "manual", "acceptEdits", "auto"];
pub const DEFAULT_MODE: &str = "manual";

/// The OpenCode agent and the session's rules for a mode. Every mode denies the `question` tool:
/// a session would otherwise wait on a question Overseer has no card for.
pub fn rules(mode: &str) -> Result<(&'static str, Value)> {
    let rule = |permission: &str, action: &str| json!({"permission": permission, "pattern": "*", "action": action});
    let no_questions = rule("question", "deny");
    Ok(match mode {
        "plan" => ("plan", json!([no_questions])),
        "manual" => ("build", json!([rule("edit", "ask"), rule("bash", "ask"), no_questions])),
        "acceptEdits" => ("build", json!([rule("edit", "allow"), rule("bash", "ask"), no_questions])),
        "auto" => ("build", json!([no_questions])),
        other => bail!("{HARNESS} does not take permission mode {other:?} (choose {})", MODES.join(", ")),
    })
}

/// The mode of another harness, carried over without loosening it (AC-138). Codex's sandbox
/// modes have no exact twin: read-only becomes Plan only, and workspace-write becomes Accept
/// edits, which asks before a command where Codex's sandbox would not.
pub fn carried_mode(from_harness: &str, mode: Option<&str>) -> &'static str {
    match (from_harness, mode) {
        (_, Some("plan")) | (_, Some("read-only")) => "plan",
        (_, Some("acceptEdits")) | (_, Some("workspace-write")) => "acceptEdits",
        (_, Some("auto")) => "auto",
        (_, Some("manual")) => "manual",
        // Codex without a mode runs in its workspace-write sandbox.
        ("codex", None) | ("codex-app", None) => "acceptEdits",
        _ => DEFAULT_MODE,
    }
}

// ------------------------------------------------------------------ the profile's configuration

/// OpenCode's configuration for Overseer's local profile: the Ollama provider on loopback with
/// the given tags, and nothing else enabled, so a local run can never reach an online model, not
/// even to name a session.
pub fn config(tags: &[String], model: &str, ollama_url: &str) -> Value {
    let models: serde_json::Map<String, Value> = tags.iter().map(|t| (t.clone(), json!({"name": t, "tool_call": true}))).collect();
    json!({
        "$schema": "https://opencode.ai/config.json",
        "provider": {PROVIDER: {"npm": "@ai-sdk/openai-compatible", "name": "Ollama (local)", "options": {"baseURL": format!("{ollama_url}/v1")}, "models": models}},
        "enabled_providers": [PROVIDER],
        "model": format!("{PROVIDER}/{model}"),
        "small_model": format!("{PROVIDER}/{model}"),
        "autoupdate": false,
        "share": "disabled",
    })
}

pub fn write_config(config_home: &Path, tags: &[String], model: &str, ollama_url: &str) -> Result<PathBuf> {
    let dir = config_home.join("opencode");
    crate::paths::ensure_private_dir(&dir)?;
    let path = dir.join("opencode.json");
    std::fs::write(&path, serde_json::to_vec_pretty(&config(tags, model, ollama_url))?)?;
    Ok(path)
}

/// Does this OpenCode have the headless server? An older one does not; then a local run cannot
/// ask before it edits or runs a command, and is only ever offered, never started on its own.
/// It is asked once per program file, in Overseer's own profile (`env`), never in the user's.
pub fn server_available(opencode: &Path, env: &std::collections::BTreeMap<String, String>) -> bool {
    if std::env::var_os("OVERSEER_TEST_OPENCODE_NO_SERVE").is_some() {
        return false;
    }
    static KNOWN: std::sync::Mutex<Vec<(std::path::PathBuf, Option<std::time::SystemTime>, bool)>> = std::sync::Mutex::new(Vec::new());
    let changed = std::fs::metadata(opencode).and_then(|m| m.modified()).ok();
    if let Some((_, _, has)) = KNOWN.lock().unwrap_or_else(|e| e.into_inner()).iter().find(|(p, at, _)| p == opencode && *at == changed) {
        return *has;
    }
    let out = std::process::Command::new(opencode).args(["serve", "--help"]).envs(env).stdin(std::process::Stdio::null()).output();
    // The help of the command itself names it; the general help of a build without it does not.
    let has = out.is_ok_and(|o| o.status.success() && serves(&format!("{}\n{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))));
    let mut known = KNOWN.lock().unwrap_or_else(|e| e.into_inner());
    known.retain(|(p, _, _)| p != opencode);
    known.push((opencode.to_path_buf(), changed, has));
    has
}

/// Is this the help of `opencode serve`? (OpenCode 1.15 prints it on the error stream.)
fn serves(help: &str) -> bool {
    help.lines().any(|l| l.trim() == "opencode serve") && help.contains("--port")
}

/// `ollama/<tag>` or a bare tag, as the tag.
pub fn tag_of(model: &str) -> &str {
    model.strip_prefix("ollama/").unwrap_or(model)
}

// ------------------------------------------------------------------ what the daemon sends

pub fn start_line(prompt: &str, model: &str, mode: &str, session: Option<&str>, title: &str) -> String {
    format!("{}\n", json!({"op": "start", "prompt": prompt, "model": tag_of(model), "mode": mode, "session": session, "title": title}))
}

pub fn permission_line(request_id: &str, allow: bool, message: &str) -> String {
    // Only `once` and `reject` are ever sent: nothing is remembered for the rest of the session.
    format!("{}\n", json!({"op": "permission", "id": request_id, "reply": if allow { "once" } else { "reject" }, "message": message}))
}

pub fn abort_line() -> String {
    format!("{}\n", json!({"op": "abort"}))
}

// ------------------------------------------------------------------ bridge lines to normalized events

fn text(v: &Value) -> String {
    v.as_str().unwrap_or_default().to_string()
}

fn clip(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}… [truncated {} bytes]", &s[..end], s.len() - end)
}

/// A short, readable name for what a permission request asks.
pub fn permission_title(permission: &str, patterns: &Value, metadata: &Value) -> String {
    let first = patterns.as_array().and_then(|a| a.first()).and_then(|p| p.as_str()).unwrap_or_default();
    match permission {
        "edit" => format!("edit: {}", metadata["filepath"].as_str().map(|p| p.rsplit('/').next().unwrap_or(p)).filter(|p| !p.is_empty()).unwrap_or(first)),
        "bash" => format!("command: {first}"),
        "external_directory" => format!("outside the worktree: {first}"),
        other => format!("{other}: {first}"),
    }
}

/// One line of the bridge's output as the daemon's normalized events.
pub fn parse(v: &Value) -> Vec<Norm> {
    let child = v["child"].as_str().map(str::to_string);
    let evidence = || format!("opencode server event for session {}", child.clone().unwrap_or_default());
    let in_child = |text: String| Norm::Child { native_id: child.clone().unwrap_or_default(), parent_native: None, title: None, status: None, text: Some(text), only_if_known: true, evidence: evidence() };
    match v["b"].as_str().unwrap_or_default() {
        "ready" => vec![Norm::Ignored],
        "session" => vec![Norm::Session(text(&v["id"]))],
        "running" => vec![Norm::Running],
        "text" => {
            let (role, body) = (text(&v["role"]), clip(&text(&v["text"]), 16384));
            match child {
                Some(_) => vec![in_child(if role == "assistant" { body } else { format!("[{role}] {body}") })],
                None => vec![Norm::Text { role, text: body }],
            }
        }
        "tool" => {
            let (name, status, id) = (text(&v["name"]), text(&v["status"]), v["id"].as_str().map(str::to_string));
            let summary = clip(&format!("{} [{}]", v["input"], status), 1000);
            if child.is_some() {
                return if status == "completed" || status == "error" { vec![in_child(format!("[tool {name}] {summary}"))] } else { vec![Norm::Ignored] };
            }
            let mut out = Vec::new();
            if status == "completed" && ["edit", "write", "patch", "multiedit"].contains(&name.as_str()) {
                if let Some(p) = v["input"]["filePath"].as_str().or(v["input"]["file_path"].as_str()) {
                    out.push(Norm::FileChange { paths: vec![p.to_string()], kind: name.clone(), confidence: "tool-input" });
                }
            }
            out.push(Norm::Tool { name, id: id.clone(), summary });
            if let Some(id) = id {
                let input = Some(v["input"].clone()).filter(|i| !i.is_null());
                let output = v["output"].as_str().or(v["error"].as_str()).map(|o| clip(o, 8192));
                out.push(Norm::ToolDetail { id, input, output, status: Some(status.clone()).filter(|s| !s.is_empty()), is_error: status == "error" });
            }
            out
        }
        "permission" => vec![Norm::Permission {
            request_id: text(&v["id"]),
            tool: permission_title(&text(&v["permission"]), &v["patterns"], &v["metadata"]),
            input: json!({"permission": v["permission"], "patterns": v["patterns"], "path": v["metadata"]["filepath"], "diff": v["metadata"]["diff"].as_str().map(|d| clip(d, 8192)), "call": v["call"], "child": v["child"]}),
        }],
        "child" => vec![Norm::Child {
            native_id: text(&v["id"]),
            parent_native: v["parent"].as_str().map(str::to_string),
            title: v["title"].as_str().map(str::to_string),
            status: v["status"].as_str().map(str::to_string),
            text: None,
            only_if_known: false,
            evidence: format!("opencode server: session.created for {} with parent {}", text(&v["id"]), v["parent"].as_str().unwrap_or("the root session")),
        }],
        "usage" => vec![Norm::Usage(json!({"tokens": v["tokens"], "cost": v["cost"], "model": v["model"], "agent": v["agent"], "local": true}))],
        "error" => vec![Norm::Error { class: text(&v["class"]), message: clip(&text(&v["message"]), 2000) }],
        "note" => vec![Norm::Text { role: "system".into(), text: clip(&text(&v["text"]), 2000) }],
        "turn_done" => vec![Norm::TurnDone { ok: v["ok"] == true, summary: v["summary"].as_str().map(|s| clip(s, 300)) }],
        _ => vec![Norm::Unparsed(clip(&v.to_string(), 4000))],
    }
}

// ------------------------------------------------------------------ server events to bridge lines

/// A reply that is a tool call written out as text: nothing ran. Two shapes have been seen, one
/// per Qwen coder generation.
pub fn is_tool_call_as_text(reply: &str) -> bool {
    let t = reply.trim();
    if t.contains("<function=") || t.contains("</tool_call>") || t.contains("<tool_call>") {
        return true;
    }
    let body = t.trim_start_matches("```json").trim_start_matches("```").trim_end_matches("```").trim();
    serde_json::from_str::<Value>(body).is_ok_and(|v| v["name"].is_string() && v["arguments"].is_object())
}

/// What the bridge remembers of a turn while it follows the server's events.
#[derive(Default)]
pub struct Turn {
    pub root: String,
    /// Child sessions of the root (and theirs), with the parent of each.
    children: HashMap<String, String>,
    /// message id → role, so the user's own prompt is not repeated as output.
    roles: HashMap<String, String>,
    /// Assistant messages whose usage has been reported.
    reported: HashSet<String>,
    /// Tool parts already reported in their final state.
    finished_tools: HashSet<String>,
    started_tools: HashSet<String>,
    pub busy_seen: bool,
    pub idle: bool,
    /// Tools the root called since the last prompt, and its last finished reply.
    pub tools_called: usize,
    pub last_reply: String,
    pub failed: Option<(String, String)>,
    pub model: String,
}

impl Turn {
    pub fn new(root: &str, model: &str) -> Self {
        Self { root: root.to_string(), model: model.to_string(), ..Default::default() }
    }

    /// Clears what belongs to one prompt, keeping what is known of the session.
    pub fn next_prompt(&mut self) {
        self.busy_seen = false;
        self.idle = false;
        self.tools_called = 0;
        self.last_reply.clear();
        self.failed = None;
    }

    fn scope(&self, session: &str) -> Option<Option<String>> {
        if session == self.root {
            Some(None)
        } else if self.children.contains_key(session) {
            Some(Some(session.to_string()))
        } else {
            None // another session on the same server: not this run's
        }
    }

    /// One server event as zero or more lines for the daemon.
    pub fn lines(&mut self, event: &Value) -> Vec<Value> {
        let p = &event["properties"];
        let session = p["sessionID"].as_str().or(p["info"]["id"].as_str()).or(p["part"]["sessionID"].as_str()).unwrap_or_default().to_string();
        let kind = event["type"].as_str().unwrap_or_default();
        if kind == "session.created" {
            let info = &p["info"];
            let (id, parent) = (text(&info["id"]), text(&info["parentID"]));
            if !parent.is_empty() && (parent == self.root || self.children.contains_key(&parent)) {
                self.children.insert(id.clone(), parent.clone());
                return vec![json!({"b": "child", "id": id, "parent": if parent == self.root { Value::Null } else { json!(parent) }, "title": info["title"], "status": "running"})];
            }
            return Vec::new();
        }
        let Some(child) = self.scope(&session) else { return Vec::new() };
        let with_child = |mut line: Value| {
            if let Some(c) = &child {
                line["child"] = json!(c);
            }
            line
        };
        match kind {
            "session.status" => {
                let status = p["status"]["type"].as_str().unwrap_or_default();
                if child.is_none() && status == "busy" && !self.busy_seen {
                    self.busy_seen = true;
                    return vec![json!({"b": "running"})];
                }
                Vec::new()
            }
            "session.idle" => match &child {
                None => {
                    // Idle before the prompt was taken up is the session's resting state, not the end.
                    if self.busy_seen {
                        self.idle = true;
                    }
                    Vec::new()
                }
                Some(c) => vec![json!({"b": "child", "id": c, "parent": self.parent_line(c), "status": "completed"})],
            },
            "session.error" => {
                let (class, message) = describe_error(&p["error"]);
                if child.is_none() {
                    self.failed = Some((class.clone(), message.clone()));
                }
                vec![with_child(json!({"b": "error", "class": class, "message": message}))]
            }
            "message.updated" => {
                let info = &p["info"];
                let id = text(&info["id"]);
                self.roles.insert(id.clone(), text(&info["role"]));
                let mut out = Vec::new();
                if info["role"] == "assistant" && info["time"]["completed"].is_number() && self.reported.insert(id) {
                    if child.is_none() {
                        out.push(json!({"b": "usage", "tokens": info["tokens"], "cost": info["cost"], "model": format!("{}/{}", text(&info["providerID"]), text(&info["modelID"])), "agent": info["agent"]}));
                        if !info["error"].is_null() && self.failed.is_none() {
                            self.failed = Some(describe_error(&info["error"]));
                        }
                    }
                }
                out
            }
            "message.part.updated" => {
                let part = &p["part"];
                let role = self.roles.get(part["messageID"].as_str().unwrap_or_default()).cloned().unwrap_or_default();
                match part["type"].as_str().unwrap_or_default() {
                    "text" | "reasoning" => {
                        let body = text(&part["text"]);
                        if role != "assistant" || !part["time"]["end"].is_number() || body.trim().is_empty() || part["synthetic"] == true {
                            return Vec::new();
                        }
                        let kind = if part["type"] == "reasoning" { "reasoning" } else { "assistant" };
                        if child.is_none() && kind == "assistant" {
                            self.last_reply = body.clone();
                        }
                        vec![with_child(json!({"b": "text", "role": kind, "text": body.trim()}))]
                    }
                    "tool" => {
                        let state = &part["state"];
                        let status = text(&state["status"]);
                        let id = text(&part["callID"]);
                        let done = status == "completed" || status == "error";
                        // One line when a tool starts and one when it ends; repeats are dropped.
                        if done {
                            if !self.finished_tools.insert(id.clone()) {
                                return Vec::new();
                            }
                        } else if status != "running" || !self.started_tools.insert(id.clone()) {
                            return Vec::new();
                        }
                        if child.is_none() && done {
                            self.tools_called += 1;
                        }
                        vec![with_child(json!({"b": "tool", "id": id, "name": part["tool"], "status": status, "input": state["input"], "output": state["output"], "error": state["error"], "title": state["title"]}))]
                    }
                    _ => Vec::new(),
                }
            }
            "permission.asked" => vec![with_child(json!({"b": "permission", "id": p["id"], "permission": p["permission"], "patterns": p["patterns"], "metadata": p["metadata"], "call": p["tool"]["callID"]}))],
            _ => Vec::new(),
        }
    }

    fn parent_line(&self, child: &str) -> Value {
        match self.children.get(child) {
            Some(p) if p != &self.root => json!(p),
            _ => Value::Null,
        }
    }
}

/// The class and the text of an error the server reports.
pub fn describe_error(error: &Value) -> (String, String) {
    let name = error["name"].as_str().unwrap_or("UnknownError");
    let message = error["data"]["message"].as_str().or(error["message"].as_str()).unwrap_or(name).to_string();
    let class = match name {
        "MessageAbortedError" => "aborted".to_string(),
        "ProviderAuthError" => "auth".to_string(),
        _ => match crate::adapters::classify_error(&message) {
            // The provider is this machine's own Ollama: a connection failure is a local one.
            "network" => "local_model".to_string(),
            other => other.to_string(),
        },
    };
    (class, format!("{name}: {message}"))
}

// ------------------------------------------------------------------ the bridge process

static STOP: AtomicBool = AtomicBool::new(false);

extern "C" fn on_signal(_: libc::c_int) {
    STOP.store(true, Ordering::SeqCst);
}

enum Msg {
    Event(Value),
    Op(Value),
    StdinClosed,
    StreamClosed(String),
}

fn say(line: &Value) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{line}");
    let _ = out.flush();
}

struct Server {
    base: String,
    auth: String,
    child: std::process::Child,
    version: String,
}

impl Server {
    fn agent(seconds: Option<u64>) -> ureq::Agent {
        let b = ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(3));
        match seconds {
            Some(s) => b.timeout(Duration::from_secs(s)).build(),
            None => b.build(),
        }
    }

    fn call(&self, method: &str, path: &str, body: Option<&Value>) -> Result<Value> {
        let req = Self::agent(Some(60)).request(method, &format!("{}{path}", self.base)).set("authorization", &self.auth);
        let res = match body {
            Some(b) => req.set("content-type", "application/json").send_string(&b.to_string()),
            None => req.call(),
        };
        match res {
            Ok(r) => Ok(serde_json::from_str(&r.into_string().unwrap_or_default()).unwrap_or(Value::Null)),
            Err(ureq::Error::Status(code, r)) => bail!("the OpenCode server answered {code} to {method} {path}: {}", clip(&r.into_string().unwrap_or_default(), 300)),
            Err(e) => bail!("the OpenCode server did not answer {method} {path}: {e}"),
        }
    }

    fn stop(&mut self) {
        unsafe {
            libc::kill(self.child.id() as i32, libc::SIGTERM);
        }
        let until = Instant::now() + Duration::from_secs(3);
        while Instant::now() < until {
            if matches!(self.child.try_wait(), Ok(Some(_))) {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn start_server(opencode: &str) -> Result<Server> {
    let port = std::net::TcpListener::bind("127.0.0.1:0")?.local_addr()?.port();
    let secret = uuid::Uuid::new_v4().simple().to_string();
    let mut child = std::process::Command::new(opencode)
        .args(["serve", "--hostname", "127.0.0.1", "--port", &port.to_string()])
        .env("OPENCODE_SERVER_PASSWORD", &secret)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .with_context(|| format!("starting {opencode} serve"))?;
    let (tx, rx) = mpsc::channel::<String>();
    for pipe in [child.stdout.take().map(|p| Box::new(p) as Box<dyn std::io::Read + Send>), child.stderr.take().map(|p| Box::new(p) as Box<dyn std::io::Read + Send>)].into_iter().flatten() {
        let tx = tx.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(pipe).lines().map_while(|l| l.ok()) {
                if tx.send(line.clone()).is_err() {
                    // After start-up the server's own output is the bridge's diagnostics.
                    eprintln!("opencode: {line}");
                }
            }
        });
    }
    let until = Instant::now() + Duration::from_secs(45);
    let mut seen = Vec::new();
    let base = loop {
        if let Ok(Some(status)) = child.try_wait() {
            bail!("opencode serve ended at once ({status}): {}", seen.join(" | "));
        }
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(line) => {
                if let Some(at) = line.find("http://127.0.0.1:") {
                    break line[at..].split_whitespace().next().unwrap_or_default().trim_end_matches('/').to_string();
                }
                seen.push(clip(&line, 200));
            }
            Err(_) if Instant::now() > until => {
                let _ = child.kill();
                bail!("opencode serve did not start listening within 45 s: {}", seen.join(" | "));
            }
            Err(_) => {}
        }
    };
    drop(rx);
    use base64::Engine;
    let auth = format!("Basic {}", base64::engine::general_purpose::STANDARD.encode(format!("opencode:{secret}")));
    let mut server = Server { base, auth, child, version: String::new() };
    let health = server.call("GET", "/global/health", None)?;
    server.version = text(&health["version"]);
    Ok(server)
}

fn follow_events(server: &Server, tx: mpsc::Sender<Msg>) -> Result<()> {
    let res = Server::agent(None).get(&format!("{}/event", server.base)).set("authorization", &server.auth).set("accept", "text/event-stream").call().map_err(|e| anyhow!("the OpenCode server's event stream did not open: {e}"))?;
    std::thread::spawn(move || {
        let mut data = String::new();
        for line in BufReader::new(res.into_reader()).lines() {
            let Ok(line) = line else { break };
            if let Some(rest) = line.strip_prefix("data:") {
                data.push_str(rest.trim_start());
            } else if line.is_empty() && !data.is_empty() {
                if let Ok(v) = serde_json::from_str::<Value>(&data) {
                    if tx.send(Msg::Event(v)).is_err() {
                        return;
                    }
                }
                data.clear();
            }
        }
        let _ = tx.send(Msg::StreamClosed("the server closed its event stream".into()));
    });
    Ok(())
}

fn follow_stdin(tx: mpsc::Sender<Msg>, first: mpsc::Sender<Value>) {
    std::thread::spawn(move || {
        let mut started = false;
        for line in std::io::stdin().lock().lines().map_while(|l| l.ok()) {
            let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
            if !started && v["op"] == "start" {
                started = true;
                let _ = first.send(v);
            } else if tx.send(Msg::Op(v)).is_err() {
                return;
            }
        }
        let _ = tx.send(Msg::StdinClosed);
    });
}

const NUDGE: &str = "Your last reply wrote a tool call out as text, so nothing ran. Call the tool itself now.";

/// `overseerd opencode-bridge --opencode <path>`. Exit code 0 when the turn ended (well or
/// badly) and was reported; 1 when the bridge itself could not do its work.
pub fn main(args: &[String]) -> i32 {
    match bridge(args) {
        Ok(()) => 0,
        Err(e) => {
            say(&json!({"b": "error", "class": "local_model", "message": format!("{e:#}")}));
            say(&json!({"b": "turn_done", "ok": false, "summary": format!("{e}")}));
            1
        }
    }
}

fn bridge(args: &[String]) -> Result<()> {
    let opencode = args.iter().position(|a| a == "--opencode").and_then(|i| args.get(i + 1)).ok_or_else(|| anyhow!("usage: overseerd opencode-bridge --opencode <path>"))?;
    unsafe {
        libc::signal(libc::SIGINT, on_signal as libc::sighandler_t);
        libc::signal(libc::SIGTERM, on_signal as libc::sighandler_t);
    }
    let (tx, rx) = mpsc::channel::<Msg>();
    let (first_tx, first_rx) = mpsc::channel::<Value>();
    follow_stdin(tx.clone(), first_tx);
    let start = first_rx.recv_timeout(Duration::from_secs(30)).map_err(|_| anyhow!("no start request arrived on standard input"))?;
    let prompt = text(&start["prompt"]);
    let model = text(&start["model"]);
    let mode = start["mode"].as_str().unwrap_or(DEFAULT_MODE).to_string();
    let (agent, session_rules) = rules(&mode)?;
    if prompt.is_empty() || model.is_empty() {
        bail!("the start request needs a prompt and a model");
    }
    let mut server = start_server(opencode)?;
    say(&json!({"b": "ready", "version": server.version, "mode": mode, "agent": agent, "model": format!("{PROVIDER}/{model}")}));
    let outcome = turn(&mut server, &rx, tx, &start, &prompt, &model, agent, &session_rules);
    server.stop();
    outcome
}

#[allow(clippy::too_many_arguments)]
fn turn(server: &mut Server, rx: &mpsc::Receiver<Msg>, tx: mpsc::Sender<Msg>, start: &Value, prompt: &str, model: &str, agent: &str, session_rules: &Value) -> Result<()> {
    follow_events(server, tx)?;
    let session = match start["session"].as_str().filter(|s| !s.is_empty()) {
        Some(id) => {
            server.call("GET", &format!("/session/{id}"), None).with_context(|| format!("session {id} is not in this profile any more"))?;
            // The mode may have changed since the last turn: the rules are set again.
            server.call("PATCH", &format!("/session/{id}"), Some(&json!({"permission": session_rules})))?;
            id.to_string()
        }
        None => {
            let s = server.call("POST", "/session", Some(&json!({"title": start["title"].as_str().unwrap_or("Overseer"), "agent": agent, "permission": session_rules})))?;
            text(&s["id"])
        }
    };
    if session.is_empty() {
        bail!("the OpenCode server did not give a session id");
    }
    say(&json!({"b": "session", "id": session}));
    let send = |server: &Server, body: &str| server.call("POST", &format!("/session/{session}/prompt_async"), Some(&json!({"model": {"providerID": PROVIDER, "modelID": model}, "agent": agent, "parts": [{"type": "text", "text": body}]})));
    send(server, prompt)?;
    let mut state = Turn::new(&session, model);
    let mut nudged = false;
    let mut aborted = false;
    let mut quiet_since = Instant::now();
    loop {
        if STOP.load(Ordering::SeqCst) && !aborted {
            aborted = true;
            let _ = server.call("POST", &format!("/session/{session}/abort"), None);
            quiet_since = Instant::now();
        }
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(Msg::Event(ev)) => {
                quiet_since = Instant::now();
                for line in state.lines(&ev) {
                    say(&line);
                }
            }
            Ok(Msg::Op(op)) => match op["op"].as_str().unwrap_or_default() {
                "permission" => {
                    let id = text(&op["id"]);
                    let reply = if op["reply"] == "once" { "once" } else { "reject" };
                    match server.call("POST", &format!("/permission/{id}/reply"), Some(&json!({"reply": reply, "message": op["message"]}))) {
                        Ok(_) => say(&json!({"b": "note", "text": format!("permission {id}: {}", if reply == "once" { "allowed once" } else { "denied" })})),
                        Err(e) => say(&json!({"b": "error", "class": "other", "message": format!("the answer to permission {id} did not reach OpenCode: {e}")})),
                    }
                }
                "abort" => {
                    aborted = true;
                    let _ = server.call("POST", &format!("/session/{session}/abort"), None);
                }
                _ => {}
            },
            // The daemon closing standard input does not end a turn; the turn ends by itself.
            Ok(Msg::StdinClosed) => {}
            Ok(Msg::StreamClosed(why)) => {
                say(&json!({"b": "error", "class": "local_model", "message": why}));
                say(&json!({"b": "turn_done", "ok": false, "summary": "the OpenCode server stopped"}));
                return Ok(());
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if let Ok(Some(status)) = server.child.try_wait() {
                    say(&json!({"b": "error", "class": "local_model", "message": format!("the OpenCode server ended ({status})")}));
                    say(&json!({"b": "turn_done", "ok": false, "summary": "the OpenCode server ended"}));
                    return Ok(());
                }
                // An abort that the server never confirms still ends the turn.
                if aborted && quiet_since.elapsed() > Duration::from_secs(5) {
                    state.idle = true;
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => bail!("the bridge lost its own channels"),
        }
        if !state.idle {
            continue;
        }
        if aborted {
            say(&json!({"b": "turn_done", "ok": false, "summary": "interrupted"}));
            return Ok(());
        }
        if let Some((_, message)) = &state.failed {
            say(&json!({"b": "turn_done", "ok": false, "summary": message}));
            return Ok(());
        }
        if state.tools_called == 0 && is_tool_call_as_text(&state.last_reply) {
            if !nudged {
                nudged = true;
                say(&json!({"b": "note", "text": "The model wrote a tool call as text, so nothing ran. Asking it once more to call the tool."}));
                state.next_prompt();
                send(server, NUDGE)?;
                continue;
            }
            say(&json!({"b": "error", "class": "tool_call_as_text", "message": format!("{model} wrote its tool call as text twice; nothing ran")}));
            say(&json!({"b": "turn_done", "ok": false, "summary": "the model wrote its tool call as text"}));
            return Ok(());
        }
        say(&json!({"b": "turn_done", "ok": true, "summary": Value::Null}));
        return Ok(());
    }
}

// ------------------------------------------------------------------ what the daemon does before a local turn

/// Overseer's own OpenCode profile for local models, made on first use. Local runs never use the
/// user's own OpenCode configuration or sessions.
pub fn local_profile(d: &crate::daemon::Daemon) -> Result<crate::store::Profile> {
    if let Some(p) = d.store.lock().unwrap().profile(LOCAL_PROFILE)? {
        return Ok(p);
    }
    let home = crate::paths::profiles_dir().join(LOCAL_PROFILE);
    crate::paths::ensure_private_dir(&home)?;
    let profile = crate::store::Profile { id: LOCAL_PROFILE.into(), name: "Local models".into(), harness: "opencode".into(), home: Some(home.display().to_string()), is_system: false, created_ms: crate::daemon::now(), account: None };
    d.store.lock().unwrap().insert_profile(&profile)?;
    Ok(profile)
}

/// The pieces of a launch that differ from other harnesses: the program is `overseerd` itself.
pub fn launch_parts(opencode: &Path, prompt: &str, model: Option<&str>, mode: Option<&str>, session: Option<&str>, title: &str) -> Result<(String, Vec<String>, String)> {
    let model = model.filter(|m| !m.is_empty()).ok_or_else(|| anyhow!("a local run needs a model"))?;
    let mode = mode.unwrap_or(DEFAULT_MODE);
    rules(mode)?;
    let exe = std::env::current_exe()?.display().to_string();
    Ok((exe, vec!["opencode-bridge".into(), "--opencode".into(), opencode.display().to_string()], start_line(prompt, model, mode, session, title)))
}

pub fn env_for(profile_env: &BTreeMap<String, String>) -> Result<&String> {
    profile_env.get("XDG_CONFIG_HOME").ok_or_else(|| anyhow!("local runs use Overseer's own OpenCode profile, never your own OpenCode configuration; this account has no folder of its own"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_server_is_recognised_from_its_own_help() {
        // OpenCode 1.15.13, `opencode serve --help`, as printed on the error stream.
        let help = "opencode serve\n\nstarts a headless opencode server\n\nOptions:\n  -h, --help         show help  [boolean]\n      --port         port to listen on  [number] [default: 0]\n      --hostname     hostname to listen on  [string] [default: \"127.0.0.1\"]\n";
        assert!(serves(help));
        // The general help of a build without the command lists other commands only.
        assert!(!serves("opencode [project]\n\nCommands:\n  opencode run [message..]  run opencode with a message\n  opencode auth             manage credentials\n\nOptions:\n  --port  port\n"));
        assert!(!serves(""));
    }

    fn fixture(name: &str) -> Vec<Value> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().join("fixtures/transcripts").join(format!("opencode-1.15.13-serve-{name}-local.jsonl"));
        std::fs::read_to_string(path).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect()
    }

    /// Replays a recorded server transcript through the bridge and the parser, as the daemon
    /// would see it.
    fn replay(name: &str) -> (Turn, Vec<Value>, Vec<Norm>) {
        let events = fixture(name);
        let root = events.iter().find(|e| e["type"] == "session.created" && e["properties"]["info"]["parentID"].is_null()).map(|e| text(&e["properties"]["info"]["id"])).unwrap();
        let mut turn = Turn::new(&root, "qwen3-coder:30b-64k");
        let lines: Vec<Value> = events.iter().flat_map(|e| turn.lines(e)).collect();
        let norms = lines.iter().flat_map(parse).collect();
        (turn, lines, norms)
    }

    fn kinds(lines: &[Value]) -> Vec<String> {
        lines.iter().map(|l| match l["b"].as_str().unwrap() {
            "tool" => format!("tool {} {}", l["name"].as_str().unwrap(), l["status"].as_str().unwrap()),
            "permission" => format!("permission {}", l["permission"].as_str().unwrap()),
            "text" => format!("text {}", l["text"].as_str().unwrap().trim()),
            "child" => format!("child {}", l["status"].as_str().unwrap()),
            other => other.to_string(),
        }).collect()
    }

    #[test]
    fn ac139_the_allow_fixture_replays_as_a_conversation() {
        let (turn, lines, norms) = replay("allow");
        assert_eq!(
            kinds(&lines),
            ["running", "tool write running", "permission edit", "tool write completed", "tool bash running", "permission bash", "tool bash completed", "usage", "text done", "usage"],
            "the user's own prompt, repeats and other sessions are left out"
        );
        assert!(turn.idle && turn.failed.is_none());
        assert_eq!((turn.tools_called, turn.last_reply.as_str()), (2, "done"));
        let asks: Vec<&Norm> = norms.iter().filter(|n| matches!(n, Norm::Permission { .. })).collect();
        let Norm::Permission { request_id, tool, input } = asks[0] else { unreachable!() };
        assert!(request_id.starts_with("per_"));
        assert_eq!(tool, "edit: allowed.txt");
        assert_eq!(input["path"], "/WORKSPACE/allowed.txt");
        assert!(input["diff"].as_str().unwrap().contains("+hello from spike"), "the card can show what would be written");
        let Norm::Permission { tool, .. } = asks[1] else { unreachable!() };
        assert_eq!(tool, "command: ls -la");
        assert!(norms.contains(&Norm::FileChange { paths: vec!["/WORKSPACE/allowed.txt".into()], kind: "write".into(), confidence: "tool-input" }));
        assert!(norms.contains(&Norm::Text { role: "assistant".into(), text: "done".into() }));
        assert!(norms.iter().any(|n| matches!(n, Norm::Usage(u) if u["local"] == true && u["cost"] == 0 && u["tokens"]["total"].as_u64().unwrap() > 10_000 && u["model"] == "ollama/qwen3-coder:30b-64k")));
        assert!(norms.iter().any(|n| matches!(n, Norm::ToolDetail { status: Some(s), is_error: false, output: Some(o), .. } if s == "completed" && o.contains("Wrote file"))));
    }

    #[test]
    fn ac139_the_deny_fixture_shows_the_refused_write() {
        let (turn, lines, norms) = replay("deny");
        let k = kinds(&lines);
        assert_eq!(&k[..4], ["running", "tool write running", "permission edit", "tool write error"]);
        assert!(turn.idle);
        assert!(!norms.iter().any(|n| matches!(n, Norm::FileChange { .. })), "a denied write is not file activity");
        assert!(norms.iter().any(|n| matches!(n, Norm::ToolDetail { is_error: true, .. })));
    }

    #[test]
    fn ac139_the_interrupt_fixture_ends_as_aborted() {
        let (turn, lines, norms) = replay("interrupt");
        assert!(kinds(&lines).contains(&"tool bash running".to_string()));
        assert_eq!(turn.failed.as_ref().map(|f| f.0.as_str()), Some("aborted"));
        assert!(norms.iter().any(|n| matches!(n, Norm::Error { class, message } if class == "aborted" && message.contains("MessageAbortedError"))));
    }

    #[test]
    fn ac139_the_children_fixture_gives_a_child_run_with_its_output() {
        let (turn, lines, norms) = replay("children");
        let k = kinds(&lines);
        assert!(k.contains(&"child running".to_string()) && k.contains(&"child completed".to_string()), "{k:?}");
        assert!(turn.idle);
        let child = lines.iter().find(|l| l["b"] == "child").unwrap();
        assert_eq!((child["parent"].clone(), child["title"].as_str()), (Value::Null, Some("say hi (@general subagent)")));
        let id = child["id"].as_str().unwrap();
        assert!(norms.iter().any(|n| matches!(n, Norm::Child { native_id, only_if_known: false, status: Some(s), .. } if native_id == id && s == "running")));
        assert!(norms.iter().any(|n| matches!(n, Norm::Child { native_id, text: Some(t), .. } if native_id == id && t == "hi from child")), "the child's reply belongs to the child");
        assert!(norms.contains(&Norm::Text { role: "assistant".into(), text: "hi from child".into() }), "and the root's own reply to the root");
        // The fixture also holds events of an unrelated session on the same server: none leak in.
        assert!(!lines.iter().any(|l| l.to_string().contains("mCoJXo")));
        assert_eq!(norms.iter().filter(|n| matches!(n, Norm::Usage(_))).count(), 2, "usage of the root's two messages only");
    }

    #[test]
    fn modes_become_an_agent_and_rules() {
        let ask = |mode: &str, permission: &str| -> String {
            let (_, r) = rules(mode).unwrap();
            r.as_array().unwrap().iter().find(|x| x["permission"] == permission).map(|x| text(&x["action"])).unwrap_or_else(|| "agent default".into())
        };
        assert_eq!(rules("plan").unwrap().0, "plan");
        assert_eq!((rules("manual").unwrap().0, ask("manual", "edit"), ask("manual", "bash")), ("build", "ask".to_string(), "ask".to_string()));
        assert_eq!((ask("acceptEdits", "edit"), ask("acceptEdits", "bash")), ("allow".to_string(), "ask".to_string()));
        assert_eq!((ask("auto", "edit"), ask("auto", "bash")), ("agent default".to_string(), "agent default".to_string()));
        for mode in MODES {
            assert_eq!(ask(mode, "question"), "deny", "{mode}: a session never waits on a question");
        }
        assert!(rules("bypassPermissions").unwrap_err().to_string().contains("choose plan, manual, acceptEdits, auto"));
        // Carried over from other harnesses, never looser.
        assert_eq!(carried_mode("claude", Some("manual")), "manual");
        assert_eq!(carried_mode("claude", Some("plan")), "plan");
        assert_eq!(carried_mode("claude", None), "manual", "Claude Code asks unless told otherwise");
        assert_eq!(carried_mode("codex", Some("read-only")), "plan");
        assert_eq!(carried_mode("codex", Some("workspace-write")), "acceptEdits");
        assert_eq!(carried_mode("codex", None), "acceptEdits");
    }

    #[test]
    fn the_profile_names_only_the_local_provider() {
        let c = config(&["qwen3-coder:30b-64k".into(), "overseer/qwen2.5-coder-14b:16k".into()], "qwen3-coder:30b-64k", "http://127.0.0.1:11434");
        assert_eq!(c["enabled_providers"], json!(["ollama"]));
        assert_eq!((c["model"].as_str(), c["small_model"].as_str()), (Some("ollama/qwen3-coder:30b-64k"), Some("ollama/qwen3-coder:30b-64k")));
        assert_eq!(c["provider"]["ollama"]["options"]["baseURL"], "http://127.0.0.1:11434/v1");
        assert_eq!(c["provider"]["ollama"]["models"].as_object().unwrap().len(), 2);
        assert_eq!((c["autoupdate"].clone(), c["share"].clone()), (json!(false), json!("disabled")));
        assert!(c.get("permission").is_none(), "rules travel with the session, not with the file");
        let dir = tempfile::tempdir().unwrap();
        let path = write_config(dir.path(), &["a:1".into()], "a:1", "http://127.0.0.1:11434").unwrap();
        assert_eq!(path, dir.path().join("opencode/opencode.json"));
        assert_eq!(tag_of("ollama/qwen3-coder:30b-64k"), "qwen3-coder:30b-64k");
    }

    #[test]
    fn a_tool_call_written_as_text_is_recognised() {
        assert!(is_tool_call_as_text("<function=bash>\n<parameter=command>\nsleep 45\n</parameter>\n</function>\n</tool_call>\n.done"));
        assert!(is_tool_call_as_text("{\n  \"name\": \"write\",\n  \"arguments\": {\n    \"content\": \"hello from qwen\",\n    \"filePath\": \"/tmp/x/ollama.txt\"\n  }\n}"));
        assert!(is_tool_call_as_text("```json\n{\"name\": \"bash\", \"arguments\": {\"command\": \"ls\"}}\n```"));
        assert!(!is_tool_call_as_text("done"));
        assert!(!is_tool_call_as_text("The config is {\"name\": \"x\"} as you asked."));
        assert!(!is_tool_call_as_text("{\"name\": \"package\", \"version\": \"1.0.0\"}"));
    }

    #[test]
    fn what_the_daemon_sends_is_one_line_each() {
        let s: Value = serde_json::from_str(start_line("do it", "ollama/qwen3-coder:30b-64k", "plan", None, "t").trim()).unwrap();
        assert_eq!(s, json!({"op": "start", "prompt": "do it", "model": "qwen3-coder:30b-64k", "mode": "plan", "session": null, "title": "t"}));
        let p: Value = serde_json::from_str(permission_line("per_1", true, "").trim()).unwrap();
        assert_eq!((p["reply"].as_str(), p["id"].as_str()), (Some("once"), Some("per_1")));
        assert_eq!(serde_json::from_str::<Value>(permission_line("per_1", false, "no").trim()).unwrap()["reply"], "reject");
        assert!(start_line("two\nlines", "m", "auto", Some("ses_1"), "t").trim_end().lines().count() == 1);
    }

    #[test]
    fn errors_from_the_server_are_classified() {
        assert_eq!(describe_error(&json!({"name": "MessageAbortedError", "data": {"message": "Aborted"}})), ("aborted".to_string(), "MessageAbortedError: Aborted".to_string()));
        assert_eq!(describe_error(&json!({"name": "APIError", "data": {"message": "connect ECONNREFUSED 127.0.0.1:11434"}})).0, "local_model");
        assert_eq!(describe_error(&json!({"name": "UnknownError", "data": {"message": "model 'x' not found"}})).0, "other");
    }
}
