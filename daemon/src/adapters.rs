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
    /// Input and/or result of a tool call, joined to its `Tool` by id in the conversation view.
    ToolDetail { id: String, input: Option<Value>, output: Option<String>, status: Option<String>, is_error: bool },
    FileChange { paths: Vec<String>, kind: String, confidence: &'static str },
    /// Native child run. `only_if_known` updates never create a child (e.g. a tool
    /// result whose tool_use id may not belong to a delegation).
    Child { native_id: String, parent_native: Option<String>, title: Option<String>, status: Option<String>, text: Option<String>, only_if_known: bool, evidence: String },
    Usage(Value),
    /// A native provider quota frame. The daemon validates and stores only
    /// normalized scoped fields, never this raw event payload.
    Quota(Value),
    /// `always`: what the harness offers to allow for the rest of the session, when it offers
    /// anything (AC-262's Always allow): `{"label", "suggestions"?}`.
    Permission { request_id: String, tool: String, input: Value, always: Option<Value> },
    Error { class: String, message: String },
    /// Only an explicit, bounded numeric native field can extend the
    /// transient health cooldown; prose is never parsed as Retry-After.
    ErrorRetryAfter { class: String, message: String, retry_after_ms: i64 },
    TurnDone { ok: bool, summary: Option<String> },
    /// JSON-RPC response to one of Overseer's requests (app-server transport).
    RpcResult { id: String, result: Value, error: Option<Value> },
    /// Text to write to the harness's stdin once the current batch is committed.
    Send(String),
    /// The harness reported its current turn id (needed to interrupt that turn).
    TurnId(String),
    /// Number of background tasks the harness reports as still running (Claude Code).
    BackgroundTasks(usize),
    /// A backgrounded task was launched (Claude Code `task_started` with `is_backgrounded`).
    BackgroundLaunched(String),
    /// A task finished and was reported (`task_notification`); for a backgrounded task Claude
    /// continues with another turn after the current result, even if it finished before it.
    BackgroundNotified(String),
    /// The main agent got a tool's result and calls the model again in the same turn (Claude
    /// Code): task notifications reported before this are read in that call.
    MainContinues,
    /// Line was recognized structurally but carries no user-visible content.
    Ignored,
    /// Line not understood by this parser version; retained as raw output.
    Unparsed(String),
}

pub struct LaunchReq<'a> {
    pub cwd: &'a Path,
    pub prompt: &'a str,
    /// The immutable text snapshot; never native flags, environment or tools.
    pub mods: Option<&'a crate::mods::delivery::TurnMods>,
    pub model: Option<&'a str>,
    /// Reasoning effort for this turn (AC-60), validated by `check_turn_options`.
    pub effort: Option<&'a str>,
    pub sandbox: Option<&'a str>,
    pub profile_env: BTreeMap<String, String>,
    pub resume_session: Option<&'a str>,
    pub program_override: Option<&'a str>,
    pub args_override: Option<&'a [String]>,
    /// Extra harness arguments chosen for the task (e.g. `-c agents.max_depth=2`).
    pub extra_args: &'a [String],
    /// Permission / sandbox mode for this turn (AC-60).
    pub permission_mode: Option<&'a str>,
    /// Images attached to this turn's prompt (private files in the run folder).
    pub images: &'a [(String, PathBuf)],
    /// Swarm workers must not start unaccounted native subagents. This is
    /// daemon-owned launch metadata, never a caller-supplied target capability.
    pub swarm_worker: bool,
    /// The daemon's per-run Swarm tools over MCP (the proposed native
    /// director and worker path, `swarm.native_director`): daemon-owned
    /// launch metadata, like `swarm_worker`, never caller-supplied arguments.
    pub swarm_tools: Option<SwarmTools<'a>>,
}

/// A Swarm member's MCP configuration (a private file in the run's folder
/// naming `overseerd mcp` and the member's token) and the tools it may call.
#[derive(Clone, Copy)]
pub struct SwarmTools<'a> {
    pub config: &'a Path,
    pub allowed: &'a [String],
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

/// This is a launch-time restriction, not live qualification under SWARM-17.
/// Generic is available only through the fixture API; Claude is launched with
/// Agent/Task denied. Other transports currently have no established way to
/// prevent an unaccounted native child in a Swarm worker.
pub fn swarm_worker_launch_supported(harness: &str) -> bool {
    matches!(harness, "generic" | "claude")
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

/// A login folder the daemon was given for its harnesses (`CLAUDE_CONFIG_DIR`, `CODEX_HOME` named in
/// OVERSEER_HARNESS_ENV_PASSTHROUGH and set), as a dev daemon with the owner's logins has (AC-221).
pub fn chosen_login_dir(key: &str) -> Option<String> {
    if key.is_empty() {
        return None;
    }
    let named = std::env::var("OVERSEER_HARNESS_ENV_PASSTHROUGH").ok()?.split(',').any(|k| k.trim() == key);
    std::env::var(key).ok().filter(|v| named && !v.is_empty())
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
    // opencode-serve is a transport of the OpenCode binary: it shares OVERSEER_OPENCODE_PATH.
    let family = match harness {
        "codex-app" => "codex",
        "opencode-serve" => "opencode",
        h => h,
    };
    let env_key = format!("OVERSEER_{}_PATH", family.to_ascii_uppercase());
    if let Ok(p) = std::env::var(&env_key) {
        // An explicit path wins, and never falls back to PATH; a missing file is "not installed".
        let p = PathBuf::from(p);
        return p.is_file().then_some(p);
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
        "opencode" | "opencode-serve" => which("opencode"),
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
            "model": "supported (-m, per turn)", "effort": "supported (model_reasoning_effort, per turn)", "permission_mode": "supported (sandbox: read-only or workspace-write)", "images": "supported (-i)",
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
            "model": "supported (at start)", "effort": "supported (turn/start effort, per turn)", "permission_mode": "supported (approval policy at start)", "images": "unsupported in Overseer's app-server transport",
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
            "model": "supported (--model, per turn)", "effort": "supported (--effort, per turn)", "permission_mode": "supported (--permission-mode: acceptEdits, plan, auto, manual)", "images": "supported (stream-json image blocks)",
            "account_login": "Claude.ai account via claude auth login (CLAUDE_CONFIG_DIR per profile)",
            "verification": "live-verified on macOS with a claude.ai account (Claude Code 2.1.246): edit, permissions, nested subagents, follow-up, interrupt"
        }),
        "opencode" => json!({
            "transport": "opencode run --format json (one process per turn)",
            "launch": "supported", "output": "supported", "follow_up": "supported (--session <id>)",
            "interrupt": "supported (SIGINT)", "resume": "supported",
            "approvals": "unknown", "file_activity": "supported (edit/write tool parts)",
            "children": "partial (task tool parts expose child session ids when present)", "usage": "supported (step_finish tokens)",
            "model": "supported (-m, per turn)", "effort": "unsupported", "permission_mode": "unsupported", "images": "unsupported",
            "quota": "unknown", "account_login": "opencode auth login (XDG_DATA_HOME per profile); login itself untested",
            "verification": "verified through the real OpenCode runtime with a mock provider and with local Ollama models; no account login verified"
        }),
        "opencode-serve" => json!({
            "transport": "opencode serve (HTTP and an event stream on loopback, through overseerd opencode-bridge; one process per turn)",
            "launch": "supported", "output": "supported", "follow_up": "supported (the same session, continued by a new server)",
            "interrupt": "supported (POST /session/{id}/abort, then SIGINT)", "resume": "supported",
            "approvals": "supported (permission.asked events answered Allow once or Deny in Overseer; never remembered)",
            "file_activity": "supported (edit/write tool parts)", "children": "supported (session.created with a parent; child output on the same stream)",
            "usage": "supported (tokens per assistant message; cost 0 for local models)", "quota": "not applicable (local model)",
            "model": "supported (a local Ollama tag, per turn)", "effort": "unsupported", "permission_mode": "supported (plan, manual, acceptEdits, auto as session rules)", "images": "unsupported",
            "account_login": "none: Overseer's own OpenCode profile with the local Ollama provider only",
            "verification": "transport verified by the AC-139 spike with OpenCode 1.15.13 and a local model"
        }),
        _ => json!({
            "transport": "generic process (stdin/stdout)", "launch": "supported", "output": "supported (raw lines)",
            "follow_up": "supported (writes a line to stdin)", "interrupt": "supported (SIGINT)", "resume": "unsupported",
            "approvals": "unknown", "file_activity": "unknown (filesystem only)", "children": "unknown", "usage": "unknown", "quota": "unknown",
            "model": "not applicable", "effort": "not applicable", "permission_mode": "not applicable", "images": "not applicable",
            "account_login": "not applicable",
            "verification": "protocol tests with fixture executables"
        }),
    }
}

pub fn validate_effort(harness: &str, effort: Option<&str>) -> Result<()> {
    let Some(value) = effort else { return Ok(()); };
    if harness == "generic" {
        bail!("generic harness does not expose a reasoning effort");
    }
    if value.is_empty() || value.len() > 32 || !value.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_') {
        bail!("effort must be a short lowercase variant name");
    }
    Ok(())
}

pub fn launch(harness: &str, req: &LaunchReq) -> Result<Launch> {
    if let Some(mods) = req.mods { mods.validate_prompt(req.prompt)?; }
    validate_effort(harness, req.effort)?;
    if req.swarm_worker {
        if !req.extra_args.is_empty() {
            bail!("Swarm worker cannot override native delegation controls");
        }
        if !swarm_worker_launch_supported(harness) {
            bail!("Swarm worker native delegation is not controlled for {harness}");
        }
    }
    if req.swarm_tools.is_some() {
        // Only Claude Code has the MCP configuration, tool allow-list and
        // delegation deny this path relies on (Gate S's spike, AC-180).
        if harness != "claude" {
            bail!("Swarm tools over MCP are not supported for {harness}");
        }
        if !req.extra_args.is_empty() {
            bail!("a Swarm member cannot override its tools with extra arguments");
        }
    }
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
    let effort = req.effort;
    for a in req.extra_args {
        let lower = a.to_ascii_lowercase();
        if lower.contains("api-key") || lower.contains("api_key") || lower.contains("access-token") {
            bail!("extra harness argument {a} is not allowed (no API keys or tokens)");
        }
    }
    let (mut args, initial_stdin, close_stdin) = match harness {
        "codex" => {
            let sandbox = match if req.permission_mode == Some("read-only") {
                "read-only"
            } else { req.sandbox.unwrap_or("workspace-write") } {
                "read-only" => "read-only",
                "workspace-write" => "workspace-write",
                _ => bail!("unsupported Codex CLI sandbox"),
            };
            let mut args = vec!["exec".to_string()];
            if let Some(session) = req.resume_session {
                args.extend(["resume".into(), session.into()]);
            }
            args.extend(["--json".into(), "--skip-git-repo-check".into()]);
            if req.resume_session.is_none() {
                args.extend(["-s".into(), sandbox.into(), "-C".into(), req.cwd.display().to_string()]);
            } else {
                // `exec resume` has no -s/-C; keep the same sandbox (cwd comes from the supervisor).
                args.extend(["-c".into(), format!("sandbox_mode=\"{sandbox}\"")]);
            }
            if let Some(m) = model {
                args.extend(["-m".into(), m.into()]);
            }
            if let Some(level) = effort {
                args.extend(["-c".into(), format!("model_reasoning_effort=\"{level}\"")]);
            }
            for (_, image) in req.images {
                args.extend(["-i".into(), image.display().to_string()]);
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
            if let Some(level) = effort {
                args.extend(["--effort".into(), level.into()]);
            }
            if let Some(session) = req.resume_session {
                args.extend(["--resume".into(), session.into()]);
            }
            if let Some(e) = req.effort {
                args.extend(["--effort".into(), e.into()]);
            }
            if let Some(mode) = req.permission_mode {
                args.extend(["--permission-mode".into(), mode.into()]);
            }
            if req.swarm_worker || req.swarm_tools.is_some() {
                // A Swarm member in Claude's read-only mode (an audit worker)
                // also has its file-editing tools denied (shell commands stay).
                let mut denied = vec!["Agent", "Task"];
                if req.permission_mode == Some(crate::swarm::audit::READ_ONLY_MODE) {
                    denied.extend(crate::swarm::audit::WRITE_TOOLS);
                }
                args.extend(["--disallowedTools".into(), denied.join(",")]);
            }
            if let Some(tools) = req.swarm_tools {
                args.extend(["--mcp-config".into(), tools.config.display().to_string(),
                    "--strict-mcp-config".into(), "--allowedTools".into(), tools.allowed.join(",")]);
            }
            let content = if req.images.is_empty() {
                json!(req.prompt)
            } else {
                use base64::Engine;
                let mut parts = vec![json!({"type": "text", "text": req.prompt})];
                for (mime, path) in req.images {
                    let data = base64::engine::general_purpose::STANDARD.encode(std::fs::read(path)?);
                    parts.push(json!({"type": "image", "source": {"type": "base64", "media_type": mime, "data": data}}));
                }
                json!(parts)
            };
            let msg = json!({"type": "user", "message": {"role": "user", "content": content}});
            (args, Some(format!("{msg}\n")), false)
        }
        "opencode" => {
            let mut args = vec!["run".to_string(), "--format".into(), "json".into()];
            if let Some(m) = model {
                args.extend(["-m".into(), m.into()]);
            }
            if let Some(level) = effort.filter(|level| *level != "default") {
                args.extend(["--variant".into(), level.into()]);
            }
            if let Some(session) = req.resume_session {
                args.extend(["--session".into(), session.into()]);
            }
            args.push("--".into());
            args.push(req.prompt.to_string());
            (args, None, true)
        }
        "opencode-serve" => {
            let title: String = req.prompt.lines().next().unwrap_or_default().chars().take(60).collect();
            let (_, args, start) = crate::opencode_bridge::launch_parts(&program, req.prompt, model, req.permission_mode, req.resume_session, &title)?;
            (args, Some(start), false)
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
    // The opencode-serve harness runs `overseerd opencode-bridge`, which starts OpenCode itself.
    let program_str = if harness == "opencode-serve" { std::env::current_exe()?.display().to_string() } else { program_str };
    Ok(Launch { program: program_str, args, env, initial_stdin, close_stdin })
}

/// Which turn options a harness accepts (AC-60); anything else is refused with a clear reason.
pub fn check_turn_options(harness: &str, effort: Option<&str>, mode: Option<&str>, images: usize) -> Result<()> {
    if harness == "codex-app" {
        validate_effort(harness, effort)?;
        if mode.is_some() { bail!("codex-app does not take a per-turn permission mode"); }
        if images > 0 { bail!("codex-app does not take image attachments"); }
        return Ok(());
    }
    let (efforts, modes, can_images): (&[&str], &[&str], bool) = match harness {
        "claude" => (&["low", "medium", "high", "xhigh", "max"], &["acceptEdits", "plan", "auto", "manual"], true),
        "codex" => (&["minimal", "low", "medium", "high", "xhigh"], &["read-only", "workspace-write"], true),
        "opencode-serve" => (&[], crate::opencode_bridge::MODES, false),
        _ => (&[], &[], false),
    };
    if let Some(e) = effort {
        if !efforts.contains(&e) {
            bail!("{harness} does not take reasoning effort {e:?}{}", if efforts.is_empty() { String::new() } else { format!(" (choose {})", efforts.join(", ")) });
        }
    }
    if let Some(m) = mode {
        if !modes.contains(&m) {
            bail!("{harness} does not take permission mode {m:?}{}", if modes.is_empty() { String::new() } else { format!(" (choose {})", modes.join(", ")) });
        }
    }
    if images > 0 && !can_images {
        bail!("{harness} does not take image attachments");
    }
    Ok(())
}

pub fn interrupt_plan(harness: &str) -> InterruptPlan {
    match harness {
        "opencode-serve" => InterruptPlan::StdinThenSignal(crate::opencode_bridge::abort_line()),
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

/// Claude Code's permission suggestions (`permission_suggestions` on `can_use_tool`) as an
/// Always allow offer: the rules it would add, named the way Claude Code writes them
/// ("Bash(npm test:*)"), and where they would be kept ("this session").
pub fn claude_always(suggestions: &Value) -> Option<Value> {
    let list = suggestions.as_array().filter(|l| !l.is_empty())?;
    let mut rules = Vec::new();
    let mut places = Vec::new();
    for s in list {
        for r in s["rules"].as_array().into_iter().flatten() {
            let tool = r["toolName"].as_str().unwrap_or_default();
            if tool.is_empty() { continue; }
            rules.push(match r["ruleContent"].as_str().filter(|c| !c.is_empty()) { Some(c) => format!("{tool}({c})"), None => tool.to_string() });
        }
        if s["type"] == "setMode" { if let Some(m) = s["mode"].as_str() { rules.push(format!("mode {m}")); } }
        if let Some(d) = s["destination"].as_str() { let d = match d { "session" => "this session", "localSettings" => "this project (local settings)", "projectSettings" => "this project", "userSettings" => "every project", other => other }; if !places.contains(&d) { places.push(d); } }
    }
    let what = if rules.is_empty() { "what Claude Code suggests".to_string() } else { rules.join(", ") };
    let label = if places.is_empty() { what } else { format!("{what} · {}", places.join(", ")) };
    Some(json!({"label": label, "suggestions": suggestions}))
}

/// Durable native grant identity. Labels are presentation, never permission authority.
/// The digest includes private daemon ownership and the exact native descriptor; neither
/// credentials in that descriptor nor native session IDs are copied into the public event.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeSessionGrant {
    pub v: u8,
    pub harness: String,
    pub family: String,
    pub scope: String,
    pub digest: String,
    pub host_replay_qualified: bool,
}

#[derive(Clone, Copy)]
pub struct NativeGrantContext<'a> {
    pub harness: &'a str,
    pub run_id: &'a str,
    pub native_id: Option<&'a str>,
    pub process_generation: i64,
}

/// Only a completely recognized, tool-wide session rule can be replayed by the host.
/// Patterned rules and other updates still go to the native harness on an owner answer;
/// their matching semantics are not qualified for host replay.
fn claude_session_rule_replay_qualified(tool: &str, suggestions: &Value) -> bool {
    let Some(list) = suggestions.as_array().filter(|list| !list.is_empty()) else { return false };
    !tool.is_empty() && list.iter().all(|suggestion| {
        let Some(object) = suggestion.as_object() else { return false };
        if object.len() != 4 || suggestion["type"] != "addRules"
            || suggestion["behavior"] != "allow" || suggestion["destination"] != "session" {
            return false;
        }
        suggestion["rules"].as_array().filter(|rules| !rules.is_empty()).is_some_and(|rules| {
            rules.iter().all(|rule| rule.as_object().is_some_and(|object| {
                object.len() == 1 && rule["toolName"].as_str() == Some(tool)
            }))
        })
    })
}

fn canonical_grant_value(value: &Value) -> Value {
    match value {
        Value::Object(object) => {
            let mut entries: Vec<_> = object.iter().collect();
            entries.sort_unstable_by(|(a, _), (b, _)| a.cmp(b));
            Value::Object(entries.into_iter().map(|(key, value)|
                (key.clone(), canonical_grant_value(value))).collect())
        }
        Value::Array(array) => Value::Array(array.iter().map(canonical_grant_value).collect()),
        other => other.clone(),
    }
}

/// Context comes from the daemon's stored Run, never request input or a surface parameter.
/// Codex's acceptForSession cache remains native-owned: another native ask is still pending
/// until that cache identity/scope is qualified, even if the command appears identical.
pub fn native_session_grant(context: &NativeGrantContext<'_>, tool: &str, input: &Value,
    offer: &Value) -> Option<NativeSessionGrant> {
    use sha2::{Digest, Sha256};
    let native_id = context.native_id.filter(|id| !id.is_empty())?;
    if context.run_id.is_empty() || context.process_generation <= 0 { return None }
    let (family, descriptor, host_replay_qualified) = match context.harness {
        "claude" => ("can_use_tool", offer["suggestions"].clone(),
            claude_session_rule_replay_qualified(tool, &offer["suggestions"])),
        "codex-app" => ("item/commandExecution/requestApproval", input.clone(), false),
        _ => return None,
    };
    let identity = canonical_grant_value(&json!({"v":1,"harness":context.harness,
        "family":family,"scope":"session","run_id":context.run_id,"native_id":native_id,
        "process_generation":context.process_generation,"tool":tool,"descriptor":descriptor}));
    let digest = Sha256::digest(serde_json::to_vec(&identity).ok()?);
    Some(NativeSessionGrant { v:1, harness:context.harness.into(), family:family.into(),
        scope:"session".into(), digest:digest.iter().map(|byte| format!("{byte:02x}")).collect(),
        host_replay_qualified })
}

/// The harness's answer line. `always` is the request's Always allow offer, when the owner chose it.
pub fn permission_reply(harness: &str, request_id: &str, allow: bool, input: &Value, message: &str) -> Option<String> {
    permission_reply_always(harness, request_id, allow, input, message, None)
}

pub fn permission_reply_always(harness: &str, request_id: &str, allow: bool, input: &Value, message: &str, always: Option<&Value>) -> Option<String> {
    let always = always.filter(|_| allow);
    match harness {
        "codex-app" => {
            let id: Value = serde_json::from_str(request_id).unwrap_or(Value::String(request_id.to_string()));
            let decision = if always.is_some() { "acceptForSession" } else if allow { "accept" } else { "decline" };
            Some(format!("{}\n", json!({"id": id, "result": {"decision": decision}})))
        }
        "opencode-serve" => Some(crate::opencode_bridge::permission_line(request_id, allow, message)),
        "claude" => {
            let response = match always {
                Some(offer) => json!({"behavior": "allow", "updatedInput": input, "updatedPermissions": offer["suggestions"]}),
                None if allow => json!({"behavior": "allow", "updatedInput": input}),
                None => json!({"behavior": "deny", "message": message}),
            };
            Some(format!("{}\n", json!({"type": "control_response", "response": {"subtype": "success", "request_id": request_id, "response": response}})))
        }
        _ => None,
    }
}

/// Classify a direct harness error without inferring account quota from HTTP throttling.
pub fn classify_error(message: &str) -> &'static str {
    let m = message.to_ascii_lowercase();
    if m.contains("usage limit") || m.contains("quota") || m.contains("exceeded your") || m.contains("out of credits") || m.contains("credit limit") || m.contains("insufficient_quota") {
        "quota"
    } else if m.contains("rate limit") || m.contains("rate_limit") || m.contains("429") || m.contains("too many requests") {
        "rate_limit"
    } else if m.contains("503 service unavailable") || m.contains("http status 503") {
        "service_unavailable"
    } else if m.contains("enetdown") || m.contains("network is down") {
        // A kernel network-down error is host-scoped. A destination-specific
        // unreachable error is not enough to declare every remote route down.
        "host_offline"
    } else if m.contains("authenticat") || m.contains("401") || m.contains("unauthorized") || m.contains("not logged in") || m.contains("log in") || m.contains("login") || m.contains("oauth") || m.contains("token expired") {
        "auth"
    } else if is_network_error(&m) {
        "network"
    } else {
        "other"
    }
}

fn structured_error(message: &str, source: &Value) -> Norm {
    structured_error_class(classify_error(message), message, source)
}

fn structured_error_class(class: &str, message: &str, source: &Value) -> Norm {
    let retry_after_ms = source.get("retryAfterMs").or_else(|| source.get("retry_after_ms"))
        .and_then(Value::as_i64)
        .filter(|ms| (1..=86_400_000).contains(ms));
    match (class, retry_after_ms) {
        ("rate_limit" | "service_unavailable", Some(retry_after_ms)) => Norm::ErrorRetryAfter {
            class: class.into(), message: truncate(message, 2000), retry_after_ms,
        },
        _ => Norm::Error { class: class.into(), message: truncate(message, 2000) },
    }
}

/// Connection failures and provider outages (Continuity, AC-83): the request never reached the
/// provider, or the provider answered with a failure of its own. `m` is lower-case.
fn is_network_error(m: &str) -> bool {
    const SIGNS: &[&str] = &[
        "econnrefused", "econnreset", "enotfound", "eai_again", "etimedout", "enetunreach", "ehostunreach", "epipe", "fetch failed", "getaddrinfo", "socket hang up",
        "network is unreachable", "network is down", "no route to host", "connection refused", "connection reset", "connection closed", "connection error", "unable to connect",
        "could not resolve host", "failed to lookup address", "dns error", "name or service not known", "nodename nor servname", "tls handshake", "handshake failed", "certificate",
        "request timed out", "connection timed out", "operation timed out", "stream disconnected", "error sending request", "network error",
        "502 bad gateway", "503 service unavailable", "504 gateway timeout", "status 502", "status 503", "status 504", "status 529", "status: 502", "status: 503", "status: 504", "status: 529",
        "api error: 5", "overloaded", "bad gateway", "service unavailable", "gateway timeout", "internal server error",
    ];
    SIGNS.iter().any(|s| m.contains(s))
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
        if class != "other" && harness != "opencode" && harness != "codex-app" && harness != "opencode-serve" {
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
        "opencode-serve" => crate::opencode_bridge::parse(&v),
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
            vec![structured_error(&msg, &v["error"]), Norm::TurnDone { ok: false, summary: Some(truncate(&msg, 300)) }]
        }
        "error" => {
            let msg = s(&v["message"]);
            vec![structured_error(&msg, v)]
        }
        "item.started" | "item.updated" | "item.completed" => {
            let item = &v["item"];
            let done = ty == "item.completed";
            let id = item["id"].as_str().map(str::to_string);
            match item["type"].as_str().unwrap_or_default() {
                "agent_message" if done => vec![Norm::Text { role: "assistant".into(), text: truncate(&s(&item["text"]), 16384) }],
                "reasoning" if done => vec![Norm::Text { role: "reasoning".into(), text: truncate(&s(&item["text"]), 4096) }],
                "command_execution" => {
                    let mut out = vec![Norm::Tool {
                        name: "shell".into(),
                        id: id.clone(),
                        summary: truncate(&format!("{} [{}{}]", s(&item["command"]), s(&item["status"]), item["exit_code"].as_i64().map(|c| format!(", exit {c}")).unwrap_or_default()), 2000),
                    }];
                    if let Some(id) = id {
                        out.push(tool_detail(id, Some(json!({"command": item["command"]})), item["aggregated_output"].as_str(), &s(&item["status"]), item["exit_code"].as_i64().map(|c| c != 0).unwrap_or(false)));
                    }
                    out
                }
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
                    vec![structured_error(&msg, item)]
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

/// Tool input (as JSON, capped at 8 KiB) and output (capped at 8 KiB) for the conversation view.
fn tool_detail(id: String, input: Option<Value>, output: Option<&str>, status: &str, is_error: bool) -> Norm {
    let input = input.filter(|v| !v.is_null()).map(|v| {
        let text = v.to_string();
        if text.len() > 8192 { json!({"truncated": truncate(&text, 8192)}) } else { v }
    });
    Norm::ToolDetail { id, input, output: output.map(|o| truncate(o, 8192)), status: if status.is_empty() { None } else { Some(status.to_string()) }, is_error }
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
            m @ ("item/commandExecution/requestApproval" | "execCommandApproval") => vec![Norm::Permission {
                request_id: id,
                tool: format!("command: {}", params["command"].as_str().map(str::to_string).unwrap_or_else(|| params["command"].to_string())),
                input: params.clone(),
                // The app server's v2 decision "acceptForSession" (docs/verification/AC-01.md).
                always: (m == "item/commandExecution/requestApproval").then(|| json!({"label": "this command for the rest of the session"})),
            }],
            "item/fileChange/requestApproval" | "applyPatchApproval" => vec![Norm::Permission { request_id: id, tool: "file change".into(), input: params.clone(), always: None }],
            "item/permissions/requestApproval" => vec![Norm::Permission { request_id: id, tool: "permissions".into(), input: params.clone(), always: None }],
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
                out.push(structured_error(msg, &turn["error"]));
            }
            out.push(Norm::TurnDone { ok: status == "completed", summary: Some(status) });
            out
        }
        "thread/tokenUsage/updated" => vec![Norm::Usage(params["tokenUsage"].clone())],
        "account/rateLimits/updated" => vec![Norm::Usage(json!({"rate_limits": params["rateLimits"]}))],
        "error" => {
            let msg = s(&params["error"]["message"]);
            let display = format!("{msg}{}", if params["willRetry"] == true { " (will retry)" } else { "" });
            vec![structured_error(&display, &params["error"])]
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
                "commandExecution" => {
                    let mut out = vec![Norm::Tool { name: "shell".into(), id: id.clone(), summary: truncate(&format!("{} [{}{}]", s(&item["command"]), s(&item["status"]), item["exitCode"].as_i64().map(|c| format!(", exit {c}")).unwrap_or_default()), 2000) }];
                    if let Some(id) = id {
                        out.push(tool_detail(id, Some(json!({"command": item["command"], "cwd": item["cwd"]})), item["aggregatedOutput"].as_str(), &s(&item["status"]), item["exitCode"].as_i64().map(|c| c != 0).unwrap_or(false) || item["status"] == "declined"));
                    }
                    out
                }
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
            Norm::TurnId(_) | Norm::Running | Norm::Session(_) | Norm::Usage(_) | Norm::Quota(_) | Norm::ToolDetail { .. } | Norm::BackgroundLaunched(_) | Norm::BackgroundNotified(_) | Norm::MainContinues => Norm::Ignored,
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
                // Claude 2.x does not stream a subagent's final reply; `task_notification` carries it as `summary`.
                let text = if sub == "task_notification" { v["summary"].as_str().filter(|t| !t.is_empty()).map(|t| truncate(t, 8192)) } else { None };
                let mut out = vec![Norm::Child { native_id: s(&v["tool_use_id"]), parent_native: None, title: v["description"].as_str().map(str::to_string), status, text, only_if_known: true, evidence: format!("claude system {sub}") }];
                // Only a background task the main agent launched (spawn depth 1) brings another
                // top-level turn; a subagent's own background child is reported back to that
                // subagent, so waiting for a turn on its account would hold the run open forever.
                let top_level = v["spawn_depth"].as_u64().map_or(true, |d| d <= 1);
                if sub == "task_started" && v["is_backgrounded"] == true && top_level {
                    out.push(Norm::BackgroundLaunched(s(&v["task_id"])));
                } else if sub == "task_notification" {
                    out.push(Norm::BackgroundNotified(s(&v["task_id"])));
                }
                out
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
                out.push(structured_error_class(&class, &text, v));
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
                            Some(p) => out.push(Norm::Child { native_id: p.clone(), parent_native: None, title: None, status: None, text: Some(if ty == "user" { format!("Prompt: {text}") } else { text }), only_if_known: true, evidence: "claude parent_tool_use_id".into() }),
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
                            None => {
                                out.push(Norm::Tool { name, id: Some(id.clone()), summary });
                                out.push(tool_detail(id, Some(input.clone()), None, "started", false));
                            }
                        }
                    }
                    "tool_result" => {
                        let id = s(&c["tool_use_id"]);
                        let failed = c["is_error"].as_bool().unwrap_or(false);
                        if parent.is_none() {
                            let text = match &c["content"] {
                                Value::String(t) => t.clone(),
                                Value::Array(a) => a.iter().filter_map(|x| x["text"].as_str()).collect::<Vec<_>>().join("\n"),
                                other => other.to_string(),
                            };
                            out.push(tool_detail(id.clone(), None, Some(&text), if failed { "failed" } else { "completed" }, failed));
                            out.push(Norm::MainContinues);
                        }
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
                out.push(structured_error(&result, v));
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
                vec![Norm::Permission { request_id: s(&v["request_id"]), tool: s(&req["tool_name"]), input: req["input"].clone(), always: if req["suppress_always_allow_rule"] == true { None } else { claude_always(&req["permission_suggestions"]) } }]
            } else {
                vec![Norm::Unparsed(truncate(&v.to_string(), 2000))]
            }
        }
        // Usage limits as Claude reports them (AC-62): the account's windows and reset times.
        "rate_limit_event" => {
            let mut out = vec![Norm::Quota(v.clone())];
            if let Some(info) = v.get("rate_limit_info") {
                // Account usage needs the known meter fields, not arbitrary
                // provider prose that may accompany the native event.
                let mut meter = serde_json::Map::new();
                for key in ["status", "rateLimitType"] {
                    if let Some(value) = info[key].as_str() {
                        meter.insert(key.into(), json!(value));
                    }
                }
                if let Some(value) = info["resetsAt"].as_i64() {
                    meter.insert("resetsAt".into(), json!(value));
                }
                let mut windows = serde_json::Map::new();
                for key in ["five_hour", "seven_day", "seven_day_opus"] {
                    let window = &info["unifiedWindows"][key];
                    let mut fields = serde_json::Map::new();
                    if let Some(value) = window["utilization"].as_f64() {
                        fields.insert("utilization".into(), json!(value));
                    }
                    if let Some(value) = window["resetsAt"].as_i64() {
                        fields.insert("resetsAt".into(), json!(value));
                    }
                    if !fields.is_empty() {
                        windows.insert(key.into(), Value::Object(fields));
                    }
                }
                meter.insert("unifiedWindows".into(), Value::Object(windows));
                out.push(Norm::Usage(json!({"rate_limits": {"claude_rate_limit": meter}})));
            }
            out
        },
        "control_response" | "stream_event" => vec![Norm::Ignored],
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
            if let Some(id) = part["callID"].as_str() {
                out.push(tool_detail(id.to_string(), Some(input.clone()), state["output"].as_str(), &status, status == "error"));
            }
        }
        "step_finish" => {
            out.push(Norm::Usage(json!({"tokens": part["tokens"], "cost": part["cost"], "reason": part["reason"]})));
            if part["reason"] == "stop" {
                out.push(Norm::TurnDone { ok: true, summary: None });
            }
        }
        "error" => {
            let msg = v["error"]["data"]["message"].as_str().or(v["error"]["message"].as_str()).map(str::to_string).unwrap_or_else(|| v["error"].to_string());
            out.push(structured_error(&msg, &v["error"]["data"]));
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
    fn structured_retry_after_is_bounded_and_never_read_from_error_prose() {
        let native = json!({"method":"error","params":{"error":{
            "message":"HTTP 429 Too Many Requests","retryAfterMs":300_000}}});
        assert_eq!(parse_codex_app(&native), vec![Norm::ErrorRetryAfter {
            class:"rate_limit".into(), message:"HTTP 429 Too Many Requests".into(),
            retry_after_ms:300_000,
        }]);
        let mut injected = native;
        injected["params"]["error"]["retryAfterMs"] = json!("300000");
        injected["params"]["error"]["message"] = json!("HTTP 429 Too Many Requests; retryAfterMs: 300000");
        assert!(matches!(&parse_codex_app(&injected)[0], Norm::Error { class, .. } if class == "rate_limit"));
        injected["params"]["error"]["retryAfterMs"] = json!(100_000_000);
        assert!(matches!(&parse_codex_app(&injected)[0], Norm::Error { class, .. } if class == "rate_limit"));
        injected["params"]["error"]["message"] = json!("usage limit reached");
        injected["params"]["error"]["retryAfterMs"] = json!(300_000);
        assert!(matches!(&parse_codex_app(&injected)[0], Norm::Error { class, .. } if class == "quota"),
            "Retry-After cannot recast a quota rejection as transient health");
    }

    #[test]
    fn classify_errors() {
        assert_eq!(classify_error("Failed to authenticate: OAuth session expired and could not be refreshed"), "auth");
        assert_eq!(classify_error("stream error: 429 Too Many Requests"), "rate_limit");
        assert_eq!(classify_error("HTTP 503 Service Unavailable"), "service_unavailable");
        assert_eq!(classify_error("connect ENETDOWN: Network is down"), "host_offline");
        assert_eq!(classify_error("connect ENETUNREACH: Network is unreachable"), "network");
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
    fn network_errors_are_their_own_class() {
        for m in [
            "stream disconnected before completion: error sending request for url (https://chatgpt.com/backend-api/codex/responses)",
            "getaddrinfo ENOTFOUND api.anthropic.com",
            "connect ECONNREFUSED 127.0.0.1:443",
            "TypeError: fetch failed",
            "Connection error.",
            "error: Network is unreachable (os error 51)",
            "API Error: 529 {\"type\":\"error\",\"error\":{\"type\":\"overloaded_error\",\"message\":\"Overloaded\"}}",
            "tls handshake eof",
        ] {
            assert_eq!(classify_error(m), "network", "{m}");
        }
        assert_eq!(classify_error("unexpected status 503 Service Unavailable"), "service_unavailable");
        // Account states are never network errors, whatever else the text says.
        assert_eq!(classify_error("exceeded retry limit, last status: 429 Too Many Requests"), "rate_limit");
        assert_eq!(classify_error("You've hit your usage limit."), "quota");
        assert_eq!(classify_error("401 Unauthorized"), "auth");
        assert_eq!(classify_error("the command timed out after 120 s"), "other", "a slow tool is not a connection problem");
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

    #[test]
    fn selected_effort_reaches_supported_cli_launches_as_one_argument() {
        for (harness, expected) in [
            ("codex", "model_reasoning_effort=\"medium\""),
            ("claude", "--effort"),
            ("opencode", "--variant"),
        ] {
            let launch = launch(harness, &LaunchReq {
                cwd: Path::new("/tmp"), prompt: "work", mods: None, model: Some("fixture-model"),
                effort: Some("medium"), sandbox: Some("workspace-write"), profile_env: BTreeMap::new(),
                resume_session: None, program_override: Some("/bin/true"),
                args_override: None, extra_args: &[], permission_mode: None, images: &[], swarm_worker: false, swarm_tools: None,
            }).unwrap();
            assert!(launch.args.iter().any(|arg| arg == expected), "{harness}: {:?}", launch.args);
            assert!(!launch.args.iter().any(|arg| arg.contains(";")));
        }
        assert!(validate_effort("codex", Some("medium;touch /tmp/x")).is_err());
        assert!(validate_effort("generic", Some("medium")).is_err());
    }

    #[test]
    fn codex_cli_keeps_selected_read_only_sandbox_on_start_and_resume() {
        for resume in [None, Some("session-1")] {
            let launch = launch("codex", &LaunchReq {
                cwd: Path::new("/tmp"), prompt: "inspect", mods: None, model: Some("fixture-model"),
                effort: Some("medium"), sandbox: Some("read-only"), profile_env: BTreeMap::new(),
                resume_session: resume, program_override: Some("/bin/true"),
                args_override: None, extra_args: &[], permission_mode: None, images: &[], swarm_worker: false, swarm_tools: None,
            }).unwrap();
            let args = &launch.args;
            if resume.is_some() {
                assert!(args.contains(&"sandbox_mode=\"read-only\"".to_string()), "{args:?}");
            } else {
                assert!(args.windows(2).any(|pair| pair == ["-s", "read-only"]), "{args:?}");
            }
        }
    }
}

#[cfg(test)]
mod turn_option_tests {
    use super::*;

    fn req<'a>(resume: Option<&'a str>, images: &'a [(String, PathBuf)]) -> LaunchReq<'a> {
        LaunchReq { cwd: Path::new("/tmp/w"), prompt: "do it", mods: None, model: Some("gpt-5.6-luna"), sandbox: Some("workspace-write"), profile_env: BTreeMap::new(), resume_session: resume, program_override: Some("/bin/echo"),
            args_override: None, extra_args: &[], effort: Some("high"), permission_mode: Some("read-only"), images, swarm_worker: false, swarm_tools: None }
    }

    #[test]
    fn mod_text_preserves_native_mcp_and_read_only_worker_controls() {
        use sha2::{Digest, Sha256};
        let config = PathBuf::from("/tmp/run/mcp-swarm.json");
        let allowed = vec!["mcp__overseer__swarm_result".to_string()];
        let guidance = "[Optional Overseer Mods: text guidance]\nUse complete sentences.\n";
        let snapshot = crate::mods::delivery::TurnMods { saved: json!({"text":guidance,
            "digest":format!("{:x}",Sha256::digest(guidance.as_bytes()))}) };
        let composed = format!("{guidance}do it");
        for session in [None, Some("session-1")] {
            let mut request = req(session, &[]);
            request.swarm_worker = true;
            request.swarm_tools = Some(SwarmTools { config: &config, allowed: &allowed });
            request.permission_mode = Some("plan");
            let baseline = launch("claude", &request).unwrap();
            request.prompt = &composed;
            request.mods = Some(&snapshot);
            let delivered = launch("claude", &request).unwrap();
            assert_eq!(delivered.program, baseline.program);
            assert_eq!(delivered.args, baseline.args);
            assert_eq!(delivered.env, baseline.env);
            assert_eq!(delivered.close_stdin, baseline.close_stdin);
            let mut sent: Value = serde_json::from_str(delivered.initial_stdin.as_deref().unwrap().trim()).unwrap();
            assert_eq!(sent["message"]["content"],composed);
            sent["message"]["content"] = json!("do it");
            assert_eq!(sent, serde_json::from_str::<Value>(baseline.initial_stdin.as_deref().unwrap().trim()).unwrap());
            request.prompt = "do it";
            assert!(launch("claude", &request).err().unwrap().to_string().contains("immutable turn prompt"));
        }
    }

    #[test]
    fn swarm_worker_disables_native_claude_delegation_on_initial_and_resumed_turns() {
        for session in [None, Some("session-1")] {
            let mut request = req(session, &[]);
            request.swarm_worker = true;
            let launch = launch("claude", &request).unwrap();
            assert!(launch.args.windows(2).any(|a| a == ["--disallowedTools", "Agent,Task"]),
                "native delegation was not disabled: {:?}", launch.args);
        }
    }

    #[test]
    fn swarm_worker_rejects_unqualified_native_delegation_transports() {
        let mut request = req(None, &[]);
        request.swarm_worker = true;
        for harness in ["codex", "codex-app", "opencode"] {
            assert!(launch(harness, &request).err().unwrap().to_string()
                .contains("native delegation is not controlled"), "{harness}");
        }
    }

    #[test]
    fn a_read_only_swarm_worker_has_its_write_tools_denied() {
        let config = PathBuf::from("/tmp/run/mcp-swarm.json");
        let allowed = vec!["mcp__overseer__swarm_result".to_string()];
        let mut request = req(None, &[]);
        request.swarm_worker = true;
        request.swarm_tools = Some(SwarmTools { config: &config, allowed: &allowed });
        request.permission_mode = Some("plan");
        let args = launch("claude", &request).unwrap().args;
        assert!(args.windows(2).any(|a| a == ["--permission-mode", "plan"]), "{args:?}");
        // Shell commands stay allowed (the owner's answer of 2026-09-28); the
        // post-attempt source check is the guard against a command that writes.
        assert!(args.windows(2).any(|a| a == ["--disallowedTools", "Agent,Task,Edit,Write,MultiEdit,NotebookEdit"]),
            "{args:?}");
        request.permission_mode = None;
        let args = launch("claude", &request).unwrap().args;
        assert!(args.windows(2).any(|a| a == ["--disallowedTools", "Agent,Task"]), "{args:?}");
    }

    #[test]
    fn swarm_tools_give_claude_its_mcp_tools_and_deny_native_delegation() {
        let config = PathBuf::from("/tmp/run/mcp-swarm.json");
        let allowed = vec!["mcp__overseer__swarm_plan".to_string(), "mcp__overseer__swarm_dispatch".to_string()];
        for session in [None, Some("session-1")] {
            let mut request = req(session, &[]);
            request.swarm_tools = Some(SwarmTools { config: &config, allowed: &allowed });
            let args = launch("claude", &request).unwrap().args;
            assert!(args.windows(2).any(|a| a == ["--disallowedTools", "Agent,Task"]), "{args:?}");
            assert!(args.windows(2).any(|a| a == ["--mcp-config", "/tmp/run/mcp-swarm.json"]), "{args:?}");
            assert!(args.iter().any(|a| a == "--strict-mcp-config"), "{args:?}");
            assert!(args.windows(2).any(|a| a == ["--allowedTools", "mcp__overseer__swarm_plan,mcp__overseer__swarm_dispatch"]), "{args:?}");
        }
        let mut request = req(None, &[]);
        request.swarm_tools = Some(SwarmTools { config: &config, allowed: &allowed });
        for harness in ["codex", "codex-app", "opencode"] {
            assert!(launch(harness, &request).err().unwrap().to_string().contains("not supported"), "{harness}");
        }
        let extra = ["--allowedTools".into(), "Agent".into()];
        request.extra_args = &extra;
        assert!(launch("claude", &request).err().unwrap().to_string().contains("cannot override its tools"));
    }

    #[test]
    fn swarm_worker_cannot_override_delegation_deny_with_extra_arguments() {
        let mut request = req(None, &[]);
        request.swarm_worker = true;
        let extra = ["--allowedTools".into(), "Agent".into()];
        request.extra_args = &extra;
        assert!(launch("claude", &request).err().unwrap().to_string()
            .contains("cannot override native delegation controls"));
    }

    #[test]
    fn codex_turn_options_become_cli_arguments() {
        let images = vec![("image/png".to_string(), PathBuf::from("/tmp/a.png"))];
        let a = launch("codex", &req(None, &images)).unwrap().args;
        let j = a.join(" ");
        assert!(j.contains("-s read-only -C /tmp/w"), "{j}");
        assert!(j.contains("-m gpt-5.6-luna -c model_reasoning_effort=\"high\""), "{j}");
        assert!(j.contains("-i /tmp/a.png -- do it"), "{j}");
        let r = launch("codex", &req(Some("thread-1"), &images)).unwrap().args.join(" ");
        assert!(r.starts_with("exec resume thread-1") && r.contains("sandbox_mode=\"read-only\"") && r.contains("-i /tmp/a.png"), "{r}");
    }

    #[test]
    fn turn_options_are_checked_per_harness() {
        assert!(check_turn_options("claude", Some("max"), Some("acceptEdits"), 1).is_ok());
        assert!(check_turn_options("codex", Some("high"), Some("read-only"), 2).is_ok());
        assert!(check_turn_options("claude", Some("ludicrous"), None, 0).is_err());
        assert!(check_turn_options("claude", None, Some("bypassPermissions"), 0).is_err(), "never bypass permissions");
        assert!(check_turn_options("opencode", Some("high"), None, 0).is_err());
        assert!(check_turn_options("generic", None, None, 1).is_err());
    }
}

#[cfg(test)]
mod always_allow_tests {
    use super::*;

    #[test]
    fn claude_suggestions_become_a_named_session_rule() {
        let offer = claude_always(&json!([{"type": "addRules", "rules": [{"toolName": "Bash", "ruleContent": "npm test:*"}], "behavior": "allow", "destination": "session"}])).unwrap();
        assert_eq!(offer["label"], "Bash(npm test:*) · this session");
        assert!(claude_always(&json!([])).is_none() && claude_always(&Value::Null).is_none());
        let line = permission_reply_always("claude", "req", true, &json!({"command": "npm test"}), "", Some(&offer)).unwrap();
        let v: Value = serde_json::from_str(line.trim()).unwrap();
        assert_eq!(v["response"]["response"]["updatedPermissions"][0]["rules"][0]["ruleContent"], "npm test:*");
        // Allow once and Deny are unchanged; a deny never carries the rule.
        let deny: Value = serde_json::from_str(permission_reply_always("claude", "req", false, &json!({}), "no", Some(&offer)).unwrap().trim()).unwrap();
        assert_eq!(deny["response"]["response"]["behavior"], "deny");
        assert!(deny["response"]["response"].get("updatedPermissions").is_none());
    }

    fn grant_context() -> NativeGrantContext<'static> {
        NativeGrantContext { harness:"claude", run_id:"private-owner-run",
            native_id:Some("private-native-session"), process_generation:1 }
    }

    fn write_offer() -> Value {
        claude_always(&json!([{"type":"addRules","rules":[{"toolName":"Write"}],
            "behavior":"allow","destination":"session"}])).unwrap()
    }

    #[test]
    fn ac274_replay_qualifies_complete_native_tool_wide_rules_only() {
        let offer = write_offer();
        assert!(native_session_grant(&grant_context(), "Write", &json!({}), &offer).unwrap().host_replay_qualified);
        for (field, value) in [("type",json!("replaceRules")), ("behavior",json!("deny")),
            ("destination",json!("userSettings")), ("unexpected",json!(true))] {
            let mut changed = offer.clone();
            changed["suggestions"][0][field] = value;
            assert!(!native_session_grant(&grant_context(), "Write", &json!({}), &changed).unwrap().host_replay_qualified, "{changed}");
        }
        for rules in [json!([]), json!([{"toolName":"Read"}]),
            json!([{"toolName":"Write","ruleContent":"docs/*"}]),
            json!([{"toolName":"Write","ruleContent":null}]),
            json!([{"toolName":"Write","unexpected":true}]),
            json!([{"toolName":"Write"},{"toolName":"Read"}])] {
            let mut changed = offer.clone(); changed["suggestions"][0]["rules"] = rules;
            assert!(!native_session_grant(&grant_context(), "Write", &json!({}), &changed).unwrap().host_replay_qualified, "{changed}");
        }
    }

    #[test]
    fn ac274_grant_digest_canonicalizes_objects_and_preserves_array_scope() {
        let context = grant_context();
        let a: Value = serde_json::from_str(r#"{"suggestions":[{"type":"addRules","rules":[{"toolName":"Write"}],"behavior":"allow","destination":"session"}]}"#).unwrap();
        let b: Value = serde_json::from_str(r#"{"suggestions":[{"destination":"session","behavior":"allow","rules":[{"toolName":"Write"}],"type":"addRules"}]}"#).unwrap();
        let first = native_session_grant(&context,"Write",&json!({"file_path":"one.txt"}),&a).unwrap();
        assert_eq!(first,native_session_grant(&context,"Write",&json!({"file_path":"two.txt"}),&b).unwrap(),"a native tool-wide rule covers different file inputs");
        let mut ordered = a.clone(); ordered["suggestions"][0]["rules"] = json!([{"toolName":"Write"},{"toolName":"Read"}]);
        let mut reversed = ordered.clone(); reversed["suggestions"][0]["rules"] = json!([{"toolName":"Read"},{"toolName":"Write"}]);
        assert_ne!(native_session_grant(&context,"Write",&Value::Null,&ordered).unwrap().digest,
            native_session_grant(&context,"Write",&Value::Null,&reversed).unwrap().digest,"array matching remains conservative");
        let mut changed = a; changed["suggestions"][0]["behavior"] = json!("deny");
        assert_ne!(first.digest,native_session_grant(&context,"Write",&Value::Null,&changed).unwrap().digest);
    }

    #[test]
    fn ac274_grant_identity_requires_actual_owner_session_and_generation() {
        let context = grant_context(); let offer = write_offer();
        let first = native_session_grant(&context,"Write",&json!({}),&offer).unwrap();
        for changed in [NativeGrantContext { run_id:"other-owner",..context },
            NativeGrantContext { native_id:Some("other-session"),..context },
            NativeGrantContext { process_generation:2,..context }] {
            let spoofed = json!({"run_id":context.run_id,"session_id":context.native_id,"process_generation":1});
            assert_ne!(first.digest,native_session_grant(&changed,"Write",&spoofed,&offer).unwrap().digest,"input cannot overwrite daemon context");
        }
        for invalid in [NativeGrantContext { run_id:"",..context },
            NativeGrantContext { native_id:None,..context },
            NativeGrantContext { native_id:Some(""),..context },
            NativeGrantContext { process_generation:0,..context }] {
            assert!(native_session_grant(&invalid,"Write",&json!({}),&offer).is_none());
        }
    }

    #[test]
    fn ac274_codex_records_only_a_digest_and_keeps_native_cache_authority() {
        let context = NativeGrantContext { harness:"codex-app",..grant_context() };
        let input = json!({"command":"touch approved.txt","token":"secret-grant-sentinel"});
        let grant = native_session_grant(&context,"command: touch approved.txt",&input,&json!({"label":"this command"})).unwrap();
        assert!(!grant.host_replay_qualified);
        assert_eq!(grant.digest.len(),64);
        let public = serde_json::to_value(&grant).unwrap().to_string();
        for private in ["secret-grant-sentinel","private-native-session","private-owner-run","touch approved.txt"] {
            assert!(!public.contains(private),"only nonsecret classification and digest persist: {public}");
        }
        let mut changed = input; changed["token"] = json!("different-secret");
        assert_ne!(grant.digest,native_session_grant(&context,"command: touch approved.txt",&changed,&json!({})).unwrap().digest);
    }

    #[test]
    fn ac274_native_suppression_hides_the_claude_always_offer() {
        let request = json!({"type":"control_request","request_id":"veto","request":{
            "subtype":"can_use_tool","tool_name":"Write","input":{},
            "permission_suggestions":[{"type":"addRules","rules":[{"toolName":"Write"}],"behavior":"allow","destination":"session"}],
            "suppress_always_allow_rule":true
        }});
        let norms = parse_claude(&request);
        let Some(Norm::Permission { always, .. }) = norms.first() else { panic!("{norms:?}") };
        assert!(always.is_none(), "native veto suppresses the host Always choice: {norms:?}");
        for flag in [Value::Null, json!(false)] {
            let mut ordinary = request.clone();
            if flag.is_null() { ordinary["request"].as_object_mut().unwrap().remove("suppress_always_allow_rule"); }
            else { ordinary["request"]["suppress_always_allow_rule"] = flag; }
            let ordinary = parse_claude(&ordinary);
            assert!(matches!(ordinary.first(), Some(Norm::Permission { always: Some(_), .. })), "an absent/false veto preserves the native offer: {ordinary:?}");
        }
    }

    #[test]
    fn codex_command_approvals_offer_accept_for_session() {
        let norms = parse_codex_app(&json!({"id": 7, "method": "item/commandExecution/requestApproval", "params": {"command": "npm test"}}));
        let Some(Norm::Permission { always: Some(offer), .. }) = norms.first() else { panic!("{norms:?}") };
        let line = permission_reply_always("codex-app", "7", true, &json!({}), "", Some(offer)).unwrap();
        assert!(line.contains("\"acceptForSession\""), "{line}");
        assert!(permission_reply("codex-app", "7", true, &json!({}), "").unwrap().contains("\"accept\""));
    }
}
