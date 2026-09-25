//! Harness adapters: how to launch each harness and how to turn its output into
//! normalized events. Parsing is deterministic and versioned (`PARSER_VERSION`).
//! Unknown lines are kept as raw output rather than guessed at.

use anyhow::{bail, Result};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const PARSER_VERSION: &str = "2026-09-24.1";

#[derive(Debug, Clone, PartialEq)]
pub enum Norm {
    Session(String),
    Running,
    Text { role: String, text: String },
    Tool { name: String, id: Option<String>, summary: String },
    FileChange { paths: Vec<String>, kind: String, confidence: &'static str },
    /// Native child run. `only_if_known` updates never create a child (e.g. a tool
    /// result whose tool_use id may not belong to a delegation).
    Child { native_id: String, parent_native: Option<String>, title: Option<String>, status: Option<String>, text: Option<String>, only_if_known: bool, evidence: String },
    Usage(Value),
    Permission { request_id: String, tool: String, input: Value },
    Error { class: String, message: String },
    TurnDone { ok: bool, summary: Option<String> },
    /// JSON-RPC response to one of Overseer's requests (app-server transport).
    RpcResult { id: String, result: Value, error: Option<Value> },
    /// Text to write to the harness's stdin once the current batch is committed.
    Send(String),
    /// The harness reported its current turn id (needed to interrupt that turn).
    TurnId(String),
    /// Number of background tasks the harness reports as still running (Claude Code).
    BackgroundTasks(usize),
    /// Line was recognized structurally but carries no user-visible content.
    Ignored,
    /// Line not understood by this parser version; retained as raw output.
    Unparsed(String),
}

pub struct LaunchReq<'a> {
    pub cwd: &'a Path,
    pub prompt: &'a str,
    pub model: Option<&'a str>,
    pub profile_env: BTreeMap<String, String>,
    pub resume_session: Option<&'a str>,
    pub program_override: Option<&'a str>,
    pub args_override: Option<&'a [String]>,
    /// Extra harness arguments chosen for the task (e.g. `-c agents.max_depth=2`).
    pub extra_args: &'a [String],
}

pub struct Launch {
    pub program: String,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub initial_stdin: Option<String>,
    pub close_stdin: bool,
}

pub enum InterruptPlan {
    Signal,
    StdinThenSignal(String),
}

/// Environment variables forwarded to harnesses. Everything else (notably API keys
/// and the parent agent's own session variables) is dropped.
const ENV_ALLOW: &[&str] = &["HOME", "USER", "LOGNAME", "SHELL", "PATH", "LANG", "LC_ALL", "LC_CTYPE", "TMPDIR", "XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_STATE_HOME", "XDG_CACHE_HOME", "XDG_RUNTIME_DIR"];

pub fn base_env(program: &str) -> BTreeMap<String, String> {
    let mut env = BTreeMap::new();
    for key in ENV_ALLOW {
        if let Ok(v) = std::env::var(key) {
            env.insert(key.to_string(), v);
        }
    }
    // Explicit passthrough for fixture harness configuration (never API keys).
    if let Ok(names) = std::env::var("OVERSEER_HARNESS_ENV_PASSTHROUGH") {
        for key in names.split(',').map(str::trim).filter(|k| !k.is_empty() && !forbidden_env(k)) {
            if let Ok(v) = std::env::var(key) {
                env.insert(key.to_string(), v);
            }
        }
    }
    let home = env.get("HOME").cloned().unwrap_or_default();
    let mut path: Vec<String> = Vec::new();
    if let Some(dir) = Path::new(program).parent().filter(|p| !p.as_os_str().is_empty()) {
        path.push(dir.display().to_string());
    }
    for extra in [format!("{home}/.local/bin"), format!("{home}/.opencode/bin"), "/opt/homebrew/bin".into(), "/usr/local/bin".into()] {
        path.push(extra);
    }
    if let Some(existing) = env.get("PATH") {
        path.extend(existing.split(':').map(str::to_string));
    }
    for base in ["/usr/bin", "/bin", "/usr/sbin", "/sbin"] {
        path.push(base.into());
    }
    let mut seen = std::collections::HashSet::new();
    path.retain(|p| seen.insert(p.clone()));
    env.insert("PATH".into(), path.join(":"));
    env.insert("TERM".into(), "dumb".into());
    env.insert("NO_COLOR".into(), "1".into());
    env
}

/// Names that must never reach a harness even if a profile tries to set them.
pub fn forbidden_env(key: &str) -> bool {
    let k = key.to_ascii_uppercase();
    k.ends_with("_API_KEY") || k.starts_with("ANTHROPIC_") || k.starts_with("OPENAI_") || k == "CLAUDE_CODE_OAUTH_TOKEN" || k.ends_with("_ACCESS_TOKEN")
}

fn which(name: &str) -> Option<PathBuf> {
    let path = base_env("").get("PATH").cloned().unwrap_or_default();
    path.split(':').map(|d| Path::new(d).join(name)).find(|p| p.is_file())
}

/// Resolve the harness executable. Explicit overrides win; for Codex prefer the
/// ChatGPT app bundle because stale package-manager installs are common.
pub fn resolve_program(harness: &str) -> Option<PathBuf> {
    // codex-app is a transport of the Codex binary: it shares OVERSEER_CODEX_PATH.
    let family = if harness == "codex-app" { "codex" } else { harness };
    let env_key = format!("OVERSEER_{}_PATH", family.to_ascii_uppercase());
    if let Ok(p) = std::env::var(&env_key) {
        return Some(PathBuf::from(p));
    }
    match harness {
        "codex" | "codex-app" => {
            let bundled = PathBuf::from("/Applications/ChatGPT.app/Contents/Resources/codex");
            if bundled.is_file() {
                return Some(bundled);
            }
            which("codex")
        }
        "claude" => which("claude"),
        "opencode" => which("opencode"),
        _ => None,
    }
}

/// Probes (versions, login status) never run inside a user's repository.
pub fn neutral_dir() -> PathBuf {
    let dir = crate::paths::data_dir().join("tmp");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

pub fn version_of(program: &Path) -> Option<String> {
    let out = std::process::Command::new(program).arg("--version").current_dir(neutral_dir()).env_clear().envs(base_env(&program.display().to_string())).stdin(std::process::Stdio::null()).output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if text.is_empty() { None } else { Some(text.lines().next().unwrap_or_default().to_string()) }
}

pub fn capabilities(harness: &str) -> Value {
    match harness {
        "codex" => json!({
            "transport": "codex exec --json (one process per turn)",
            "launch": "supported", "output": "supported", "follow_up": "supported (codex exec resume <thread>)",
            "interrupt": "supported (SIGINT to process group)", "resume": "supported",
            "approvals": "unsupported in exec transport (sandbox policy decides); use the codex-app transport for approvals",
            "file_activity": "supported (file_change items)", "children": "supported (collab_tool_call spawn_agent/wait; child output limited to final message)",
            "usage": "supported (turn.completed usage)", "quota": "unknown (error text classification only)",
            "account_login": "ChatGPT account via codex login (CODEX_HOME per profile)",
            "verification": "live-verified on macOS with one ChatGPT account (codex 0.155); two simultaneous accounts not yet verified"
        }),
        "codex-app" => json!({
            "transport": "codex app-server (JSON-RPC over stdio)",
            "launch": "supported", "output": "supported", "follow_up": "supported (thread/resume + turn/start)",
            "interrupt": "supported (turn/interrupt, then SIGINT)", "resume": "supported",
            "approvals": "supported (command/file-change approval requests answered Allow/Deny in Overseer; never auto-approved)",
            "file_activity": "supported (fileChange items)", "children": "supported (collabAgentToolCall spawnAgent/wait)",
            "usage": "supported (thread/tokenUsage/updated)", "quota": "partial (account/rateLimits/updated when the server sends it)",
            "account_login": "ChatGPT account via codex login (CODEX_HOME per profile)",
            "verification": "live-verified on macOS: approvals Allow/Deny/Interrupt with one ChatGPT account (codex 0.155)"
        }),
        "claude" => json!({
            "transport": "claude -p stream-json (stdin/stdout)",
            "launch": "supported", "output": "supported", "follow_up": "supported (--resume <session>)",
            "interrupt": "supported (control_request interrupt, then SIGINT)", "resume": "supported",
            "approvals": "supported (--permission-prompt-tool stdio)", "file_activity": "supported (Write/Edit tool inputs)",
            "children": "supported (Agent/Task tool_use ids, parent_tool_use_id nesting, system task_* events)",
            "usage": "supported (result usage)", "quota": "unknown (error text classification only)",
            "account_login": "Claude.ai account via claude auth login (CLAUDE_CONFIG_DIR per profile)",
            "verification": "fixture-verified only (synthetic stream-json); live account run not yet verified"
        }),
        "opencode" => json!({
            "transport": "opencode run --format json (one process per turn)",
            "launch": "supported", "output": "supported", "follow_up": "supported (--session <id>)",
            "interrupt": "supported (SIGINT)", "resume": "supported",
            "approvals": "unknown", "file_activity": "supported (edit/write tool parts)",
            "children": "partial (task tool parts expose child session ids when present)", "usage": "supported (step_finish tokens)",
            "quota": "unknown", "account_login": "opencode auth login (XDG_DATA_HOME per profile); login itself untested",
            "verification": "verified with a local mock model provider through the real OpenCode runtime; no account run verified"
        }),
        _ => json!({
            "transport": "generic process (stdin/stdout)", "launch": "supported", "output": "supported (raw lines)",
            "follow_up": "supported (writes a line to stdin)", "interrupt": "supported (SIGINT)", "resume": "unsupported",
            "approvals": "unknown", "file_activity": "unknown (filesystem only)", "children": "unknown", "usage": "unknown", "quota": "unknown",
            "account_login": "not applicable",
            "verification": "protocol tests with fixture executables"
        }),
    }
}

pub fn launch(harness: &str, req: &LaunchReq) -> Result<Launch> {
    let program = match req.program_override {
        Some(p) => PathBuf::from(p),
        None => resolve_program(harness).ok_or_else(|| anyhow::anyhow!("{harness} executable not found"))?,
    };
    let program_str = program.display().to_string();
    let mut env = base_env(&program_str);
    for (k, v) in &req.profile_env {
        if forbidden_env(k) {
            bail!("profile environment may not set {k}");
        }
        env.insert(k.clone(), v.clone());
    }
    let model = req.model.filter(|m| !m.is_empty());
    for a in req.extra_args {
        let lower = a.to_ascii_lowercase();
        if lower.contains("api-key") || lower.contains("api_key") || lower.contains("access-token") {
            bail!("extra harness argument {a} is not allowed (no API keys or tokens)");
        }
    }
    let (mut args, initial_stdin, close_stdin) = match harness {
        "codex" => {
            let mut args = vec!["exec".to_string()];
            if let Some(session) = req.resume_session {
                args.extend(["resume".into(), session.into()]);
            }
            args.extend(["--json".into(), "--skip-git-repo-check".into()]);
            if req.resume_session.is_none() {
                args.extend(["-s".into(), "workspace-write".into(), "-C".into(), req.cwd.display().to_string()]);
            } else {
                // `exec resume` has no -s/-C; keep the same sandbox (cwd comes from the supervisor).
                args.extend(["-c".into(), "sandbox_mode=\"workspace-write\"".into()]);
            }
            if let Some(m) = model {
                args.extend(["-m".into(), m.into()]);
            }
            args.push("--".into());
            args.push(req.prompt.to_string());
            (args, None, true)
        }
        "codex-app" => {
            let init = json!({"id": "ovs-init", "method": "initialize", "params": {"clientInfo": {"name": "overseer", "title": "Overseer", "version": env!("CARGO_PKG_VERSION")}, "capabilities": null}});
            let initialized = json!({"method": "initialized"});
            (vec!["app-server".to_string()], Some(format!("{init}\n{initialized}\n")), false)
        }
        "claude" => {
            let mut args: Vec<String> = ["-p", "--output-format", "stream-json", "--input-format", "stream-json", "--verbose", "--permission-prompt-tool", "stdio"]
                .iter()
                .map(|s| s.to_string())
                .collect();
            if let Some(m) = model {
                args.extend(["--model".into(), m.into()]);
            }
            if let Some(session) = req.resume_session {
                args.extend(["--resume".into(), session.into()]);
            }
            let msg = json!({"type": "user", "message": {"role": "user", "content": req.prompt}});
            (args, Some(format!("{msg}\n")), false)
        }
        "opencode" => {
            let mut args = vec!["run".to_string(), "--format".into(), "json".into()];
            if let Some(m) = model {
                args.extend(["-m".into(), m.into()]);
            }
            if let Some(session) = req.resume_session {
                args.extend(["--session".into(), session.into()]);
            }
            args.push("--".into());
            args.push(req.prompt.to_string());
            (args, None, true)
        }
        "generic" => {
            let args = req.args_override.map(|a| a.to_vec()).unwrap_or_default();
            let stdin = if req.prompt.is_empty() { None } else { Some(format!("{}\n", req.prompt)) };
            (args, stdin, false)
        }
        other => bail!("unknown harness {other}"),
    };
    if !req.extra_args.is_empty() && harness != "generic" {
        // Options go before the positional prompt separator where one exists.
        let at = args.iter().position(|a| a == "--").unwrap_or(args.len());
        let extra: Vec<String> = req.extra_args.to_vec();
        args.splice(at..at, extra);
    }
    Ok(Launch { program: program_str, args, env, initial_stdin, close_stdin })
}

pub fn interrupt_plan(harness: &str) -> InterruptPlan {
    match harness {
        "claude" => InterruptPlan::StdinThenSignal(format!("{}\n", json!({"type": "control_request", "request_id": format!("overseer-int-{}", uuid::Uuid::new_v4().simple()), "request": {"subtype": "interrupt"}}))),
        _ => InterruptPlan::Signal,
    }
}

/// Follow-ups either go to a live process's stdin or start a resumed process.
pub fn follow_up_via_stdin(harness: &str, prompt: &str) -> Option<String> {
    match harness {
        "generic" => Some(format!("{prompt}\n")),
        _ => None,
    }
}

pub fn permission_reply(harness: &str, request_id: &str, allow: bool, input: &Value, message: &str) -> Option<String> {
    match harness {
        "codex-app" => {
            let id: Value = serde_json::from_str(request_id).unwrap_or(Value::String(request_id.to_string()));
            let decision = if allow { "accept" } else { "decline" };
            Some(format!("{}\n", json!({"id": id, "result": {"decision": decision}})))
        }
        "claude" => {
            let response = if allow { json!({"behavior": "allow", "updatedInput": input}) } else { json!({"behavior": "deny", "message": message}) };
            Some(format!("{}\n", json!({"type": "control_response", "response": {"subtype": "success", "request_id": request_id, "response": response}})))
        }
        _ => None,
    }
}

/// Classify an error message into auth / rate_limit / quota / other.
pub fn classify_error(message: &str) -> &'static str {
    let m = message.to_ascii_lowercase();
    if m.contains("usage limit") || m.contains("quota") || m.contains("exceeded your") || m.contains("out of credits") || m.contains("credit limit") || m.contains("insufficient_quota") {
        "quota"
    } else if m.contains("rate limit") || m.contains("rate_limit") || m.contains("429") || m.contains("too many requests") {
        "rate_limit"
    } else if m.contains("authenticat") || m.contains("401") || m.contains("unauthorized") || m.contains("not logged in") || m.contains("log in") || m.contains("login") || m.contains("oauth") || m.contains("token expired") {
        "auth"
    } else {
        "other"
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}… [truncated {} bytes]", &s[..end], s.len() - end)
}

pub fn parse(harness: &str, stream: &str, line: &str) -> Vec<Norm> {
    if stream == "i" || stream == "x" {
        return vec![Norm::Ignored];
    }
    if harness == "generic" {
        return vec![Norm::Text { role: if stream == "e" { "stderr".into() } else { "stdout".into() }, text: truncate(line, 8192) }];
    }
    if stream == "e" {
        let class = classify_error(line);
        if class != "other" && harness != "opencode" && harness != "codex-app" {
            return vec![Norm::Error { class: class.into(), message: truncate(line, 2000) }];
        }
        return vec![Norm::Text { role: "stderr".into(), text: truncate(line, 8192) }];
    }
    let Ok(v) = serde_json::from_str::<Value>(line) else {
        return vec![Norm::Unparsed(truncate(line, 8192))];
    };
    match harness {
        "codex" => parse_codex(&v),
        "codex-app" => parse_codex_app(&v),
        "claude" => parse_claude(&v),
        "opencode" => parse_opencode(&v),
        _ => vec![Norm::Unparsed(truncate(line, 8192))],
    }
}

fn s(v: &Value) -> String {
    v.as_str().map(str::to_string).unwrap_or_default()
}

pub fn parse_codex(v: &Value) -> Vec<Norm> {
    let ty = v["type"].as_str().unwrap_or_default();
    match ty {
        "thread.started" => vec![Norm::Session(s(&v["thread_id"]))],
        "turn.started" => vec![Norm::Running],
        "turn.completed" => vec![Norm::Usage(v["usage"].clone()), Norm::TurnDone { ok: true, summary: None }],
        "turn.failed" => {
            let msg = s(&v["error"]["message"]);
            vec![Norm::Error { class: classify_error(&msg).into(), message: truncate(&msg, 2000) }, Norm::TurnDone { ok: false, summary: Some(truncate(&msg, 300)) }]
        }
        "error" => {
            let msg = s(&v["message"]);
            vec![Norm::Error { class: classify_error(&msg).into(), message: truncate(&msg, 2000) }]
        }
        "item.started" | "item.updated" | "item.completed" => {
            let item = &v["item"];
            let done = ty == "item.completed";
            let id = item["id"].as_str().map(str::to_string);
            match item["type"].as_str().unwrap_or_default() {
                "agent_message" if done => vec![Norm::Text { role: "assistant".into(), text: truncate(&s(&item["text"]), 16384) }],
                "reasoning" if done => vec![Norm::Text { role: "reasoning".into(), text: truncate(&s(&item["text"]), 4096) }],
                "command_execution" => vec![Norm::Tool {
                    name: "shell".into(),
                    id,
                    summary: truncate(&format!("{} [{}{}]", s(&item["command"]), s(&item["status"]), item["exit_code"].as_i64().map(|c| format!(", exit {c}")).unwrap_or_default()), 2000),
                }],
                "file_change" => {
                    let paths: Vec<String> = item["changes"].as_array().map(|a| a.iter().map(|c| s(&c["path"])).collect()).unwrap_or_default();
                    let kind = item["changes"].as_array().and_then(|a| a.first()).map(|c| s(&c["kind"])).unwrap_or_default();
                    if done {
                        vec![Norm::FileChange { paths, kind, confidence: "reported" }]
                    } else {
                        vec![Norm::Tool { name: "apply_patch".into(), id, summary: truncate(&paths.join(", "), 2000) }]
                    }
                }
                "mcp_tool_call" | "web_search" => vec![Norm::Tool { name: s(&item["type"]), id, summary: truncate(&item.to_string(), 1000) }],
                "todo_list" if done => vec![Norm::Text { role: "plan".into(), text: truncate(&item["items"].to_string(), 4000) }],
                "error" => {
                    let msg = s(&item["message"]);
                    vec![Norm::Error { class: classify_error(&msg).into(), message: truncate(&msg, 2000) }]
                }
                "collab_tool_call" => parse_codex_collab(item, done),
                _ if done => vec![Norm::Unparsed(truncate(&v.to_string(), 4000))],
                _ => vec![Norm::Ignored],
            }
        }
        _ => vec![Norm::Unparsed(truncate(&v.to_string(), 4000))],
    }
}

fn codex_child_status(state: &str) -> Option<String> {
    Some(
        match state {
            "pending_init" => "queued",
            "running" | "in_progress" => "running",
            "completed" | "shutdown" => "completed",
            "errored" | "failed" => "failed",
            "interrupted" => "interrupted",
            "not_found" => "unknown",
            "" => return None,
            other => other,
        }
        .to_string(),
    )
}

fn parse_codex_collab(item: &Value, done: bool) -> Vec<Norm> {
    let tool = s(&item["tool"]);
    let mut out = vec![Norm::Tool { name: format!("collab:{tool}"), id: item["id"].as_str().map(str::to_string), summary: truncate(&s(&item["prompt"]), 500) }];
    let receivers: Vec<String> = item["receiver_thread_ids"].as_array().map(|a| a.iter().map(s).collect()).unwrap_or_default();
    let states = item["agents_states"].as_object().cloned().unwrap_or_default();
    for id in receivers.iter().chain(states.keys().filter(|k| !receivers.contains(k))) {
        let state = &states.get(id).cloned().unwrap_or(Value::Null);
        let status = codex_child_status(state["status"].as_str().unwrap_or_default());
        let text = state["message"].as_str().map(|m| truncate(m, 8192));
        out.push(Norm::Child {
            native_id: id.clone(),
            parent_native: None,
            title: if tool == "spawn_agent" { Some(truncate(&s(&item["prompt"]), 200)) } else { None },
            status: if done { status } else { None },
            text,
            only_if_known: tool != "spawn_agent",
            evidence: format!("codex collab_tool_call {tool} {}", s(&item["id"])),
        });
    }
    out
}

/// Codex app-server (JSON-RPC 2.0 over stdio). Responses to Overseer's own requests are
/// surfaced as `RpcResult`; server requests for approval become `Permission`; any other
/// server request is answered with a JSON-RPC error so the server never waits silently.
pub fn parse_codex_app(v: &Value) -> Vec<Norm> {
    let method = v["method"].as_str();
    let has_id = v.get("id").map(|i| !i.is_null()).unwrap_or(false);
    if has_id && method.is_none() {
        return vec![Norm::RpcResult { id: v["id"].as_str().map(str::to_string).unwrap_or_else(|| v["id"].to_string()), result: v["result"].clone(), error: v.get("error").cloned().filter(|e| !e.is_null()) }];
    }
    let params = &v["params"];
    if has_id {
        let id = v["id"].to_string();
        return match method.unwrap_or_default() {
            "item/commandExecution/requestApproval" | "execCommandApproval" => vec![Norm::Permission {
                request_id: id,
                tool: format!("command: {}", params["command"].as_str().map(str::to_string).unwrap_or_else(|| params["command"].to_string())),
                input: params.clone(),
            }],
            "item/fileChange/requestApproval" | "applyPatchApproval" => vec![Norm::Permission { request_id: id, tool: "file change".into(), input: params.clone() }],
            "item/permissions/requestApproval" => vec![Norm::Permission { request_id: id, tool: "permissions".into(), input: params.clone() }],
            other => vec![
                Norm::Text { role: "system".into(), text: format!("harness request {other} is not supported by Overseer; declined") },
                Norm::Send(format!("{}\n", json!({"id": v["id"], "error": {"code": -32601, "message": format!("{other} not supported by Overseer")}}))),
            ],
        };
    }
    match method.unwrap_or_default() {
        "thread/started" => {
            let t = &params["thread"];
            if t["parentThreadId"].is_string() { vec![Norm::Ignored] } else { vec![Norm::Session(s(&t["id"]))] }
        }
        "turn/started" => vec![Norm::TurnId(s(&params["turn"]["id"])), Norm::Running],
        "turn/completed" => {
            let turn = &params["turn"];
            let status = s(&turn["status"]);
            let mut out = Vec::new();
            if let Some(msg) = turn["error"]["message"].as_str() {
                out.push(Norm::Error { class: classify_error(msg).into(), message: truncate(msg, 2000) });
            }
            out.push(Norm::TurnDone { ok: status == "completed", summary: Some(status) });
            out
        }
        "thread/tokenUsage/updated" => vec![Norm::Usage(params["tokenUsage"].clone())],
        "account/rateLimits/updated" => vec![Norm::Usage(json!({"rate_limits": params["rateLimits"]}))],
        "error" => {
            let msg = s(&params["error"]["message"]);
            vec![Norm::Error { class: classify_error(&msg).into(), message: truncate(&format!("{msg}{}", if params["willRetry"] == true { " (will retry)" } else { "" }), 2000) }]
        }
        "item/started" | "item/completed" => {
            let item = &params["item"];
            let done = method == Some("item/completed");
            let id = item["id"].as_str().map(str::to_string);
            match item["type"].as_str().unwrap_or_default() {
                "agentMessage" if done => vec![Norm::Text { role: "assistant".into(), text: truncate(&s(&item["text"]), 16384) }],
                "reasoning" if done => {
                    let text = item["summary"].as_array().map(|a| a.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join("\n")).unwrap_or_default();
                    if text.is_empty() { vec![Norm::Ignored] } else { vec![Norm::Text { role: "reasoning".into(), text: truncate(&text, 4096) }] }
                }
                "commandExecution" => vec![Norm::Tool { name: "shell".into(), id, summary: truncate(&format!("{} [{}{}]", s(&item["command"]), s(&item["status"]), item["exitCode"].as_i64().map(|c| format!(", exit {c}")).unwrap_or_default()), 2000) }],
                "fileChange" => {
                    let paths: Vec<String> = item["changes"].as_array().map(|a| a.iter().map(|c| s(&c["path"])).collect()).unwrap_or_default();
                    if done && item["status"] == "completed" {
                        vec![Norm::FileChange { paths, kind: "patch".into(), confidence: "reported" }]
                    } else {
                        vec![Norm::Tool { name: "apply_patch".into(), id, summary: truncate(&format!("{} [{}]", paths.join(", "), s(&item["status"])), 2000) }]
                    }
                }
                "collabAgentToolCall" => {
                    let mut legacy = item.clone();
                    legacy["receiver_thread_ids"] = item["receiverThreadIds"].clone();
                    legacy["agents_states"] = item["agentsStates"].clone();
                    legacy["tool"] = json!(match item["tool"].as_str().unwrap_or_default() { "spawnAgent" => "spawn_agent", "wait" => "wait", "sendInput" => "send_input", "closeAgent" => "close_agent", o => o });
                    parse_codex_collab(&legacy, done)
                }
                "mcpToolCall" | "dynamicToolCall" | "webSearch" => vec![Norm::Tool { name: s(&item["type"]), id, summary: truncate(&item.to_string(), 1000) }],
                _ => vec![Norm::Ignored],
            }
        }
        _ => vec![Norm::Ignored],
    }
}

/// Re-scope app-server norms that belong to a child thread (`params.threadId` differs from
/// the root thread): their text/tools become that child's output, their turn completion
/// becomes the child's status, and their spawns nest under the child.
pub fn scope_codex_app_child(thread: &str, norms: Vec<Norm>) -> Vec<Norm> {
    let evidence = format!("codex app-server notification for child thread {thread}");
    norms
        .into_iter()
        .map(|n| match n {
            Norm::Text { role, text } => Norm::Child { native_id: thread.into(), parent_native: None, title: None, status: None, text: Some(if role == "assistant" { text } else { format!("[{role}] {text}") }), only_if_known: true, evidence: evidence.clone() },
            Norm::Tool { name, summary, .. } => Norm::Child { native_id: thread.into(), parent_native: None, title: None, status: None, text: Some(format!("[tool {name}] {summary}")), only_if_known: true, evidence: evidence.clone() },
            Norm::TurnDone { ok, .. } => Norm::Child { native_id: thread.into(), parent_native: None, title: None, status: Some(if ok { "completed" } else { "failed" }.into()), text: None, only_if_known: true, evidence: evidence.clone() },
            Norm::Child { native_id, parent_native: None, title, status, text, only_if_known, evidence } => Norm::Child { native_id, parent_native: Some(thread.into()), title, status, text, only_if_known, evidence },
            Norm::TurnId(_) | Norm::Running | Norm::Session(_) | Norm::Usage(_) => Norm::Ignored,
            other => other,
        })
        .collect()
}

pub fn parse_claude(v: &Value) -> Vec<Norm> {
    let ty = v["type"].as_str().unwrap_or_default();
    let parent = v["parent_tool_use_id"].as_str().map(str::to_string);
    match ty {
        "system" => match v["subtype"].as_str() {
            Some("init") => vec![Norm::Session(s(&v["session_id"])), Norm::Running, Norm::Text { role: "system".into(), text: format!("session started (model {})", s(&v["model"])) }],
            Some("background_tasks_changed") => vec![Norm::BackgroundTasks(v["tasks"].as_array().map(|a| a.len()).unwrap_or(0))],
            Some(sub) if sub.starts_with("task_") && v["tool_use_id"].is_string() => {
                let status = match v["status"].as_str() {
                    Some("completed") => Some("completed".to_string()),
                    Some("failed") | Some("error") => Some("failed".to_string()),
                    Some("killed") | Some("stopped") => Some("interrupted".to_string()),
                    Some("running") => Some("running".to_string()),
                    _ if sub == "task_started" => Some("running".to_string()),
                    _ => None,
                };
                vec![Norm::Child { native_id: s(&v["tool_use_id"]), parent_native: None, title: v["description"].as_str().map(str::to_string), status, text: None, only_if_known: true, evidence: format!("claude system {sub}") }]
            }
            _ => vec![Norm::Ignored],
        },
        "assistant" | "user" => {
            let mut out = Vec::new();
            if let Some(err) = v["error"].as_str() {
                let text = v["message"]["content"][0]["text"].as_str().unwrap_or(err).to_string();
                let class = match err {
                    "authentication_failed" | "oauth_org_not_allowed" => "auth".to_string(),
                    "rate_limit" => "rate_limit".to_string(),
                    "billing_error" => "quota".to_string(),
                    _ => classify_error(&text).to_string(),
                };
                out.push(Norm::Error { class, message: truncate(&text, 2000) });
                return out;
            }
            let content = &v["message"]["content"];
            let items: Vec<Value> = match content {
                Value::String(t) => vec![json!({"type": "text", "text": t})],
                Value::Array(a) => a.clone(),
                _ => vec![],
            };
            for c in items {
                match c["type"].as_str().unwrap_or_default() {
                    "text" => {
                        let text = truncate(&s(&c["text"]), 16384);
                        match &parent {
                            Some(p) => out.push(Norm::Child { native_id: p.clone(), parent_native: None, title: None, status: None, text: Some(text), only_if_known: true, evidence: "claude parent_tool_use_id".into() }),
                            None if ty == "assistant" => out.push(Norm::Text { role: "assistant".into(), text }),
                            None => {}
                        }
                    }
                    "tool_use" => {
                        let name = s(&c["name"]);
                        let id = s(&c["id"]);
                        let input = &c["input"];
                        if name == "Task" || name == "Agent" {
                            let title = input["description"].as_str().or(input["subagent_type"].as_str()).unwrap_or("subagent").to_string();
                            out.push(Norm::Child { native_id: id.clone(), parent_native: parent.clone(), title: Some(title), status: Some("running".into()), text: None, only_if_known: false, evidence: format!("claude tool_use {name} {id}{}", parent.as_ref().map(|p| format!(" inside {p}")).unwrap_or_default()) });
                        }
                        if let Some(path) = input["file_path"].as_str().or(input["notebook_path"].as_str()) {
                            if ["Write", "Edit", "MultiEdit", "NotebookEdit"].contains(&name.as_str()) {
                                out.push(Norm::FileChange { paths: vec![path.to_string()], kind: name.to_lowercase(), confidence: "tool-input" });
                            }
                        }
                        let summary = truncate(&input.to_string(), 1000);
                        match &parent {
                            Some(p) => out.push(Norm::Child { native_id: p.clone(), parent_native: None, title: None, status: None, text: Some(format!("[tool {name}] {summary}")), only_if_known: true, evidence: "claude parent_tool_use_id".into() }),
                            None => out.push(Norm::Tool { name, id: Some(id), summary }),
                        }
                    }
                    "tool_result" => {
                        let id = s(&c["tool_use_id"]);
                        let failed = c["is_error"].as_bool().unwrap_or(false);
                        out.push(Norm::Child { native_id: id, parent_native: parent.clone(), title: None, status: Some(if failed { "failed" } else { "completed" }.into()), text: None, only_if_known: true, evidence: "claude tool_result".into() });
                    }
                    _ => {}
                }
            }
            if out.is_empty() { vec![Norm::Ignored] } else { out }
        }
        "result" => {
            let is_error = v["is_error"].as_bool().unwrap_or(false);
            let result = s(&v["result"]);
            let mut out = vec![Norm::Usage(json!({"usage": v["usage"], "total_cost_usd": v["total_cost_usd"], "num_turns": v["num_turns"]}))];
            if is_error {
                out.push(Norm::Error { class: classify_error(&result).into(), message: truncate(&result, 2000) });
            }
            if let Some(denials) = v["permission_denials"].as_array().filter(|d| !d.is_empty()) {
                let tools: Vec<String> = denials.iter().map(|d| s(&d["tool_name"])).collect();
                out.push(Norm::Error { class: "permission_denied".into(), message: format!("{} tool request(s) were denied during this turn: {}", denials.len(), tools.join(", ")) });
            }
            out.push(Norm::TurnDone { ok: !is_error, summary: Some(truncate(&result, 300)) });
            out
        }
        "control_request" => {
            let req = &v["request"];
            if req["subtype"] == "can_use_tool" {
                vec![Norm::Permission { request_id: s(&v["request_id"]), tool: s(&req["tool_name"]), input: req["input"].clone() }]
            } else {
                vec![Norm::Unparsed(truncate(&v.to_string(), 2000))]
            }
        }
        "control_response" | "stream_event" | "rate_limit_event" => vec![Norm::Ignored],
        _ => vec![Norm::Unparsed(truncate(&v.to_string(), 4000))],
    }
}

pub fn parse_opencode(v: &Value) -> Vec<Norm> {
    let ty = v["type"].as_str().unwrap_or_default();
    let part = &v["part"];
    let mut out = Vec::new();
    if let Some(sid) = v["sessionID"].as_str().or(part["sessionID"].as_str()) {
        out.push(Norm::Session(sid.to_string()));
    }
    match ty {
        "step_start" => out.push(Norm::Running),
        "text" => out.push(Norm::Text { role: "assistant".into(), text: truncate(&s(&part["text"]), 16384) }),
        "reasoning" => out.push(Norm::Text { role: "reasoning".into(), text: truncate(&s(&part["text"]), 4096) }),
        "tool_use" | "tool" => {
            let tool = s(&part["tool"]);
            let state = &part["state"];
            let input = &state["input"];
            let status = s(&state["status"]);
            if tool == "task" {
                // OpenCode's task tool runs a child session; its id appears in metadata when available.
                let child = state["metadata"]["sessionId"].as_str().or(state["metadata"]["sessionID"].as_str()).map(str::to_string);
                let native = child.clone().unwrap_or_else(|| s(&part["callID"]));
                out.push(Norm::Child {
                    native_id: native,
                    parent_native: None,
                    title: input["description"].as_str().map(str::to_string),
                    status: Some(match status.as_str() { "completed" => "completed", "error" => "failed", _ => "running" }.into()),
                    text: state["output"].as_str().map(|t| truncate(t, 8192)),
                    only_if_known: false,
                    evidence: format!("opencode task tool call {} ({})", s(&part["callID"]), if child.is_some() { "child session id reported" } else { "child session id not reported; call id used" }),
                });
            }
            if ["edit", "write", "patch", "multiedit"].contains(&tool.as_str()) && status == "completed" {
                if let Some(p) = input["filePath"].as_str().or(input["file_path"].as_str()) {
                    out.push(Norm::FileChange { paths: vec![p.to_string()], kind: tool.clone(), confidence: "tool-input" });
                }
            }
            out.push(Norm::Tool { name: tool, id: part["callID"].as_str().map(str::to_string), summary: truncate(&format!("{} [{}]", input, status), 1000) });
        }
        "step_finish" => {
            out.push(Norm::Usage(json!({"tokens": part["tokens"], "cost": part["cost"], "reason": part["reason"]})));
            if part["reason"] == "stop" {
                out.push(Norm::TurnDone { ok: true, summary: None });
            }
        }
        "error" => {
            let msg = v["error"]["data"]["message"].as_str().or(v["error"]["message"].as_str()).map(str::to_string).unwrap_or_else(|| v["error"].to_string());
            out.push(Norm::Error { class: classify_error(&msg).into(), message: truncate(&msg, 2000) });
            out.push(Norm::TurnDone { ok: false, summary: Some(truncate(&msg, 300)) });
        }
        _ => out.push(Norm::Unparsed(truncate(&v.to_string(), 4000))),
    }
    out
}

/// OpenCode's `run --format json` stream only covers the root session. Descendant
/// sessions (children and grandchildren) are read from OpenCode's own session store,
/// read-only: `session.parent_id` gives the tree and the parent's `task` tool part gives
/// status. Returns child upserts ordered parents-first.
pub fn opencode_store_children(db: &Path, root_session: &str) -> Result<Vec<Norm>> {
    use rusqlite::{Connection, OpenFlags};
    let conn = Connection::open_with_flags(db, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)?;
    conn.busy_timeout(std::time::Duration::from_millis(500))?;
    let mut stmt = conn.prepare(
        "WITH RECURSIVE tree(id, parent_id, title, depth) AS (
           SELECT id, parent_id, title, 1 FROM session WHERE parent_id = ?1
           UNION ALL SELECT s.id, s.parent_id, s.title, t.depth + 1 FROM session s JOIN tree t ON s.parent_id = t.id WHERE t.depth < 16)
         SELECT tree.id, tree.parent_id, tree.title,
           (SELECT json_extract(p.data, '$.state.status') FROM part p WHERE p.session_id = tree.parent_id
              AND json_extract(p.data, '$.tool') = 'task' AND json_extract(p.data, '$.state.metadata.sessionId') = tree.id
              ORDER BY p.time_updated DESC LIMIT 1),
           (SELECT json_extract(p.data, '$.text') FROM part p WHERE p.session_id = tree.id AND json_extract(p.data, '$.type') = 'text'
              ORDER BY p.time_updated DESC LIMIT 1)
         FROM tree ORDER BY tree.depth, tree.id",
    )?;
    let rows = stmt.query_map([root_session], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?, r.get::<_, String>(2)?, r.get::<_, Option<String>>(3)?, r.get::<_, Option<String>>(4)?))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (id, parent, title, status, text) = row?;
        let status = match status.as_deref() {
            Some("completed") => "completed",
            Some("error") => "failed",
            Some("running") | Some("pending") => "running",
            _ => "running",
        };
        out.push(Norm::Child {
            native_id: id.clone(),
            parent_native: parent.filter(|p| p != root_session),
            title: Some(title),
            status: Some(status.into()),
            text: text.map(|t| truncate(&t, 8192)),
            only_if_known: false,
            evidence: format!("opencode session store: session {id} parent_id link"),
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_errors() {
        assert_eq!(classify_error("Failed to authenticate: OAuth session expired and could not be refreshed"), "auth");
        assert_eq!(classify_error("stream error: 429 Too Many Requests"), "rate_limit");
        assert_eq!(classify_error("You've hit your usage limit. Upgrade to Pro"), "quota");
        assert_eq!(classify_error("file not found"), "other");
        // Real message formats found in the pinned codex 0.155 binary (strings probe).
        assert_eq!(classify_error("Usage limit reached"), "quota");
        assert_eq!(classify_error("You've reached your workspace credit limit"), "quota");
        assert_eq!(classify_error("Your workspace is out of credits. Ask your workspace owner to add more."), "quota");
        assert_eq!(classify_error("exceeded retry limit, last status: 429 Too Many Requests"), "rate_limit");
        // Real Claude Code 2.1.246 message captured live.
        assert_eq!(classify_error("Failed to authenticate: OAuth session expired and could not be refreshed"), "auth");
    }

    #[test]
    fn forbidden_keys() {
        assert!(forbidden_env("OPENAI_API_KEY"));
        assert!(forbidden_env("anthropic_api_key"));
        assert!(forbidden_env("CLAUDE_CODE_OAUTH_TOKEN"));
        assert!(!forbidden_env("CODEX_HOME"));
    }

    #[test]
    fn base_env_drops_keys() {
        std::env::set_var("OPENAI_API_KEY", "sk-test");
        std::env::set_var("CLAUDECODE", "1");
        let env = base_env("/x/codex");
        assert!(!env.contains_key("OPENAI_API_KEY"));
        assert!(!env.contains_key("CLAUDECODE"));
        assert!(env["PATH"].starts_with("/x:"));
    }
}
