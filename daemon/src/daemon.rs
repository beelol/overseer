//! Daemon state and behavior: tasks, workspaces, snapshots, runs and their processes.

use crate::adapters::{self, InterruptPlan, LaunchReq, Norm};
use crate::git;
use crate::paths;
use crate::redact::redact;
use crate::shim::{self, ExitInfo, LaunchFile, ShimInfo};
use crate::store::{self, Event, Profile, Run, Snapshot, Store, Task, Turn, Workspace};
use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashSet};
use std::io::{Read, Seek, SeekFrom};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;

pub const ACTIVE: &[&str] = &["queued", "starting", "running", "waiting_for_user"];
const RAW_SEGMENTS_KEPT: u64 = 4;
pub const DEFAULT_AUTO_EXECUTION_BUDGET_MS: u64 = 300_000;

pub fn now() -> i64 {
    shim::now_ms() as i64
}

fn short_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()[..12].to_string()
}

fn codex_thread_request(app: &Value) -> Value {
    match app["resume"].as_str() {
        Some(thread) => json!({"id": "ovs-thread", "method": "thread/resume", "params": {
            "threadId": thread, "cwd": app["cwd"], "approvalPolicy": app["approval"], "sandbox": "workspace-write"}}),
        None => {
            let mut params = json!({"cwd": app["cwd"], "approvalPolicy": app["approval"], "sandbox": "workspace-write"});
            if let Some(model) = app["model"].as_str() {
                params["model"] = json!(model);
            }
            json!({"id": "ovs-thread", "method": "thread/start", "params": params})
        }
    }
}

fn codex_child_next_request(app: &Value) -> Value {
    if app["required_tools"].as_array().is_some_and(|tools| !tools.is_empty()) {
        json!({"id":"ovs-auto-tools","method":"mcpServerStatus/list",
            "params":{"detail":"toolsAndAuthOnly"}})
    } else {
        codex_thread_request(app)
    }
}

pub(crate) fn valid_required_tool(name: &str) -> bool {
    let Some((server, tool)) = name.split_once('/') else { return false };
    [server, tool].iter().all(|part| !part.is_empty() && part.len() <= 120
        && part.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.')))
}

pub struct Daemon {
    pub store: Mutex<Store>,
    profile_gates: Mutex<BTreeMap<String, Arc<Mutex<()>>>>,
    workspace_gates: Mutex<BTreeMap<String, Arc<Mutex<()>>>>,
    work_unit_gates: Mutex<BTreeMap<String, std::sync::Weak<Mutex<()>>>>,
    pub events: broadcast::Sender<Event>,
    tails: Mutex<HashSet<String>>,
    exe: PathBuf,
    pub started_ms: i64,
    pub learning_paused: std::sync::atomic::AtomicBool,
    /// Connected VS Code windows (connections that said hello as `client: "vscode"`).
    pub ui_clients: std::sync::atomic::AtomicUsize,
    /// Bumped on every UI connect/disconnect so a pending background notice can tell a reload
    /// (reconnect within the grace period) from VS Code really closing.
    pub ui_epoch: std::sync::atomic::AtomicU64,
}

fn pid_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    let rc = unsafe { libc::kill(pid as i32, 0) };
    rc == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

fn control_socket_path(run: &str, generation: i64) -> PathBuf {
    paths::short_socket(&format!("c-{}-{generation}.sock", &run[..run.len().min(14)]))
}

impl Daemon {
    pub fn open() -> Result<Arc<Self>> {
        paths::ensure_private_dir(&paths::data_dir())?;
        paths::ensure_private_dir(&paths::runtime_dir())?;
        paths::ensure_private_dir(&paths::runs_dir())?;
        let store = Store::open(&paths::db_path())?;
        let (tx, _) = broadcast::channel(4096);
        let exe = std::env::current_exe()?;
        let daemon = Arc::new(Self { store: Mutex::new(store), profile_gates: Mutex::new(BTreeMap::new()), workspace_gates: Mutex::new(BTreeMap::new()), work_unit_gates: Mutex::new(BTreeMap::new()), events: tx, tails: Mutex::new(HashSet::new()), exe, started_ms: now(), learning_paused: std::sync::atomic::AtomicBool::new(false),
            ui_clients: std::sync::atomic::AtomicUsize::new(0), ui_epoch: std::sync::atomic::AtomicU64::new(0) });
        daemon.ensure_system_profiles()?;
        Ok(daemon)
    }

    pub fn emit(&self, task: Option<&str>, run: Option<&str>, kind: &str, source: &str, confidence: &str, payload: Value) -> Result<Event> {
        let payload = redact_value(payload);
        let event = self.store.lock().unwrap().insert_event(now(), task, run, kind, source, confidence, &payload)?;
        let _ = self.events.send(event.clone());
        Ok(event)
    }

    // ------------------------------------------------------------------ profiles

    fn ensure_system_profiles(&self) -> Result<()> {
        let store = self.store.lock().unwrap();
        let existing = store.profiles()?;
        for harness in ["codex", "claude", "opencode"] {
            if !existing.iter().any(|p| p.is_system && p.harness == harness) {
                store.insert_profile(&Profile {
                    id: format!("system-{harness}"),
                    name: format!("{harness} (existing login)"),
                    harness: harness.into(),
                    home: None,
                    is_system: true,
                    created_ms: now(),
                })?;
            }
        }
        Ok(())
    }

    pub fn create_profile(&self, name: &str, harness: &str) -> Result<Profile> {
        if !["codex", "claude", "opencode"].contains(&harness) {
            bail!("profiles are only needed for account-based harnesses (codex, claude, opencode)");
        }
        let name = name.trim();
        if name.is_empty() || name.len() > 80 {
            bail!("profile name must be 1-80 characters");
        }
        let id = format!("p-{}", short_id());
        let home = paths::profiles_dir().join(&id);
        paths::ensure_private_dir(&home)?;
        let profile = Profile { id, name: name.into(), harness: harness.into(), home: Some(home.display().to_string()), is_system: false, created_ms: now() };
        // Create the harness credential folder now (0700), so a sign-in never starts without it.
        let _ = Self::profile_env(&profile);
        self.store.lock().unwrap().insert_profile(&profile)?;
        self.emit(None, None, "profile", "daemon", "exact", json!({"profile": profile}))?;
        Ok(profile)
    }

    pub fn profile_env(profile: &Profile) -> BTreeMap<String, String> {
        let mut env = BTreeMap::new();
        if profile.is_system {
            // Test-only: point the desktop-linked logins at a fixture home instead of ~/.codex, ~/.claude.
            if let Some(sys) = std::env::var_os("OVERSEER_TEST_SYSTEM_HOME").map(std::path::PathBuf::from) {
                match profile.harness.as_str() {
                    "codex" => { env.insert("CODEX_HOME".into(), sys.join(".codex").display().to_string()); }
                    "claude" => { env.insert("CLAUDE_CONFIG_DIR".into(), sys.join(".claude").display().to_string()); }
                    _ => {}
                }
            }
        }
        if let Some(home) = &profile.home {
            let home = Path::new(home);
            match profile.harness.as_str() {
                "codex" => {
                    let dir = home.join("codex");
                    let _ = paths::ensure_private_dir(&dir);
                    env.insert("CODEX_HOME".into(), dir.display().to_string());
                }
                "claude" => {
                    let dir = home.join("claude");
                    let _ = paths::ensure_private_dir(&dir);
                    env.insert("CLAUDE_CONFIG_DIR".into(), dir.display().to_string());
                }
                "opencode" => {
                    for (key, sub) in [("XDG_DATA_HOME", "data"), ("XDG_CONFIG_HOME", "config"), ("XDG_STATE_HOME", "state"), ("XDG_CACHE_HOME", "cache")] {
                        let dir = home.join(sub);
                        let _ = paths::ensure_private_dir(&dir);
                        env.insert(key.into(), dir.display().to_string());
                    }
                }
                _ => {}
            }
        }
        env
    }

    /// Serializes metadata probes with launches and follow-ups for one account profile.
    pub fn profile_gate(&self, id: &str) -> Arc<Mutex<()>> {
        self.profile_gates.lock().unwrap().entry(id.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(()))).clone()
    }

    /// Concurrent clients deciding the same unit share one decision/launch.
    /// Weak entries allow completed unit locks to be discarded on later calls.
    pub fn work_unit_gate(&self, id: &str) -> Arc<Mutex<()>> {
        let mut gates = self.work_unit_gates.lock().unwrap();
        gates.retain(|_, gate| gate.strong_count() > 0);
        if let Some(gate) = gates.get(id).and_then(std::sync::Weak::upgrade) {
            return gate;
        }
        let gate = Arc::new(Mutex::new(()));
        gates.insert(id.to_string(), Arc::downgrade(&gate));
        gate
    }

    fn workspace_gate(&self, id: &str) -> Arc<Mutex<()>> {
        self.workspace_gates.lock().unwrap().entry(id.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(()))).clone()
    }

    pub fn profile(&self, id: &str) -> Result<Profile> {
        self.store.lock().unwrap().profile(id)?.ok_or_else(|| anyhow!("unknown profile {id}"))
    }

    /// Command the UI runs in a terminal to sign in. Account login only; no API keys.
    pub fn login_command(&self, id: &str, device: bool) -> Result<Value> {
        let profile = self.profile(id)?;
        let program = adapters::resolve_program(&profile.harness).ok_or_else(|| anyhow!("{} not installed", profile.harness))?;
        let args: Vec<&str> = match profile.harness.as_str() {
            "codex" if device => vec!["login", "--device-auth"],
            "codex" => vec!["login"],
            "claude" => vec!["auth", "login"],
            "opencode" => vec!["auth", "login"],
            _ => bail!("no login flow"),
        };
        Ok(json!({"program": program, "args": args, "env": Self::profile_env(&profile), "profile": profile}))
    }

    pub fn logout(&self, id: &str) -> Result<Value> {
        let profile = self.profile(id)?;
        if profile.is_system {
            bail!("Overseer does not log out the existing system login; use the harness directly if you intend that");
        }
        let program = adapters::resolve_program(&profile.harness).ok_or_else(|| anyhow!("{} not installed", profile.harness))?;
        let args: Vec<&str> = match profile.harness.as_str() {
            "codex" => vec!["logout"],
            "claude" => vec!["auth", "logout"],
            _ => bail!("logout for {} is done with its own auth command", profile.harness),
        };
        let out = run_with_env(&program, &args, &Self::profile_env(&profile))?;
        self.emit(None, None, "profile", "daemon", "exact", json!({"profile_id": id, "action": "logout", "exit": out.0}))?;
        Ok(json!({"exit": out.0, "output": redact(&out.1)}))
    }

    pub fn profile_status(&self, id: &str) -> Result<Value> {
        let profile = self.profile(id)?;
        let env = Self::profile_env(&profile);
        let Some(program) = adapters::resolve_program(&profile.harness) else {
            return Ok(json!({"profile_id": id, "installed": false, "logged_in": false, "detail": format!("{} not installed", profile.harness)}));
        };
        let version = adapters::version_of(&program);
        let mut result = json!({"profile_id": id, "installed": true, "program": program, "version": version});
        match profile.harness.as_str() {
            "codex" => {
                let (code, out) = run_with_env(&program, &["login", "status"], &env)?;
                let logged = code == 0 && out.contains("Logged in");
                result["logged_in"] = json!(logged);
                result["method"] = json!(if out.contains("ChatGPT") { "chatgpt-account" } else if out.contains("API key") { "api-key (not allowed by Overseer)" } else { "none" });
                result["detail"] = json!(redact(out.trim()));
                let home = env.get("CODEX_HOME").cloned().unwrap_or_else(|| format!("{}/.codex", std::env::var("HOME").unwrap_or_default()));
                if let Some(identity) = codex_identity(Path::new(&home).join("auth.json").as_path()) {
                    result["identity"] = identity;
                }
                if out.contains("API key") {
                    result["logged_in"] = json!(false);
                    result["detail"] = json!("This profile uses an API key. Overseer requires ChatGPT account login.");
                }
            }
            "claude" => {
                let (_, out) = run_with_env(&program, &["auth", "status"], &env)?;
                let parsed: Value = serde_json::from_str(&out).unwrap_or(Value::Null);
                let logged = parsed["loggedIn"].as_bool().unwrap_or(false);
                result["logged_in"] = json!(logged);
                result["method"] = parsed["authMethod"].clone();
                let who = ["email", "emailAddress", "accountUuid", "orgId"].iter().filter_map(|k| parsed[*k].as_str()).collect::<Vec<_>>().join("|");
                if !who.is_empty() {
                    result["identity"] = json!({"fingerprint": fingerprint(&who), "plan": parsed["subscriptionType"].clone()});
                }
                if parsed["authMethod"].as_str().map(|m| m.contains("api")).unwrap_or(false) {
                    result["logged_in"] = json!(false);
                    result["detail"] = json!("This profile uses an API key. Overseer requires Claude account login.");
                }
            }
            "opencode" => {
                let (_, out) = run_with_env(&program, &["auth", "list"], &env)?;
                let clean = strip_ansi(&out);
                let count = clean.lines().find_map(|l| l.trim().trim_start_matches('└').trim().strip_suffix(" credentials").and_then(|n| n.trim().parse::<i64>().ok()));
                result["logged_in"] = json!(count.unwrap_or(0) > 0);
                result["credentials"] = json!(count);
                result["detail"] = json!(redact(clean.trim()));
            }
            _ => {}
        }
        Ok(result)
    }

    // ------------------------------------------------------------------ snapshots

    pub fn take_snapshot(&self, ws: &Workspace, kind: &str) -> Result<Snapshot> {
        let path = Path::new(&ws.path);
        let trees = git::capture_trees(path, &paths::data_dir().join("tmp"))?;
        let id = format!("s-{}", short_id());
        let msg = format!("overseer {kind} snapshot {id}");
        let commit = git::pin_tree(path, &trees.worktree_tree, trees.head.as_deref(), &msg, &format!("refs/overseer/snapshots/{id}"))?;
        let index_commit = git::pin_tree(path, &trees.index_tree, trees.head.as_deref(), &format!("{msg} (index)"), &format!("refs/overseer/snapshots/{id}-index"))?;
        let status = git::status(path)?;
        let snap = Snapshot {
            id,
            workspace_id: ws.id.clone(),
            kind: kind.into(),
            head: trees.head,
            index_tree: trees.index_tree,
            worktree_tree: trees.worktree_tree,
            commit_sha: commit,
            index_commit: Some(index_commit),
            created_ms: now(),
            dirty: serde_json::to_value(status)?,
        };
        self.store.lock().unwrap().insert_snapshot(&snap)?;
        Ok(snap)
    }

    // ------------------------------------------------------------------ tasks and runs

    pub fn workspace(&self, id: &str) -> Result<Workspace> {
        self.store.lock().unwrap().workspace(id)?.ok_or_else(|| anyhow!("unknown workspace {id}"))
    }

    pub fn run(&self, id: &str) -> Result<Run> {
        self.store.lock().unwrap().run(id)?.ok_or_else(|| anyhow!("unknown run {id}"))
    }

    pub fn task(&self, id: &str) -> Result<Task> {
        self.store.lock().unwrap().task(id)?.ok_or_else(|| anyhow!("unknown task {id}"))
    }

    fn active_writer(&self, path: &str) -> Result<Option<Run>> {
        let store = self.store.lock().unwrap();
        for ws in store.workspace_by_path(path)? {
            if let Some(owner) = &ws.owner_run_id {
                if let Some(run) = store.run(owner)? {
                    if ACTIVE.contains(&run.status.as_str()) {
                        return Ok(Some(run));
                    }
                }
            }
        }
        Ok(None)
    }

    pub fn create_task(self: &Arc<Self>, p: &Value) -> Result<Value> {
        let repo_in = p["repo"].as_str().ok_or_else(|| anyhow!("repo is required"))?;
        let harness = p["harness"].as_str().unwrap_or("codex");
        if !["codex", "codex-app", "claude", "opencode", "generic"].contains(&harness) {
            bail!("unknown harness {harness}");
        }
        let effort = match p.get("effort") {
            None | Some(Value::Null) => None,
            Some(Value::String(value)) if !value.is_empty() => Some(value.clone()),
            _ => bail!("effort must be a nonempty string"),
        };
        adapters::validate_effort(harness, effort.as_deref())?;
        let prompt = p["prompt"].as_str().unwrap_or_default().to_string();
        if prompt.is_empty() && harness != "generic" {
            bail!("prompt is required");
        }
        let title = p["title"].as_str().map(str::to_string).unwrap_or_else(|| prompt.chars().take(60).collect());
        let mode = p["workspace_mode"].as_str().unwrap_or("worktree");
        let repo = git::toplevel(Path::new(repo_in)).context("repository not found")?;
        let common = git::common_dir(&repo)?;
        let profile = match p["profile_id"].as_str() {
            Some(id) => Some(self.profile(id)?),
            None if harness != "generic" => Some(self.profile(&format!("system-{}", profile_harness(harness)))?),
            None => None,
        };
        if let Some(prof) = &profile {
            if prof.harness != profile_harness(harness) {
                bail!("profile {} belongs to {}, not {harness}", prof.name, prof.harness);
            }
        }
        let target_ref = p["target_ref"].as_str().filter(|s| !s.is_empty()).map(str::to_string);
        if let Some(t) = &target_ref {
            if git::rev_parse(&repo, t).is_none() {
                bail!("target ref {t} does not exist");
            }
        }
        let (ws, fork_commit, fork_prov) = match mode {
            "worktree" => {
                let start = target_ref.clone().unwrap_or_else(|| "HEAD".into());
                let start_sha = git::rev_parse(&repo, &start).ok_or_else(|| anyhow!("repository has no commit to branch from"))?;
                let repo_name = repo.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "repo".into());
                let hash = &fingerprint(&common.display().to_string())[..8];
                let parent = paths::worktrees_dir().join(format!("{repo_name}-{hash}"));
                let (path, branch) = git::worktree_add(&repo, &parent, &title, &start_sha)?;
                let ws = Workspace {
                    id: format!("w-{}", short_id()),
                    path: path.display().to_string(),
                    repo_root: repo.display().to_string(),
                    common_dir: common.display().to_string(),
                    kind: "worktree".into(),
                    branch: Some(branch.clone()),
                    owner_run_id: None,
                    initial_dirty: json!({"clean": true}),
                    created_ms: now(),
                    removed_ms: None,
                };
                (ws, Some(start_sha.clone()), Some(format!("recorded: worktree branch {branch} created from {start} at {start_sha}")))
            }
            "current" => {
                let path = repo.display().to_string();
                if let Some(run) = self.active_writer(&path)? {
                    bail!("the current checkout already has an active writer (run {} '{}'); refusing a second independent writer", run.id, run.title);
                }
                let status = git::status(&repo)?;
                let head = git::head(&repo);
                let (fork, prov) = match (git::default_branch(&repo), &head) {
                    (Some(base), Some(h)) => match git::merge_base(&repo, &base, h) {
                        Some(mb) => (Some(mb.clone()), Some(format!("detected candidate: merge-base of {base} and HEAD ({mb}); not recorded when the branch was created"))),
                        None => (None, Some(format!("unknown: {base} shares no history with HEAD"))),
                    },
                    _ => (None, Some("unknown: no integration branch or HEAD commit".into())),
                };
                let ws = Workspace {
                    id: format!("w-{}", short_id()),
                    path,
                    repo_root: repo.display().to_string(),
                    common_dir: common.display().to_string(),
                    kind: "current".into(),
                    branch: status.branch.clone(),
                    owner_run_id: None,
                    initial_dirty: {
                        let mut v = serde_json::to_value(&status)?;
                        v["unsaved_drafts"] = p["unsaved"].clone();
                        v
                    },
                    created_ms: now(),
                    removed_ms: None,
                };
                (ws, fork, prov)
            }
            other => bail!("unknown workspace mode {other}"),
        };
        self.store.lock().unwrap().insert_workspace(&ws)?;
        let task = Task {
            id: format!("t-{}", short_id()),
            title: title.clone(),
            prompt: prompt.clone(),
            repo_root: repo.display().to_string(),
            target_ref: target_ref.clone(),
            workspace_id: ws.id.clone(),
            start_snapshot: None,
            fork_commit,
            fork_provenance: fork_prov,
            created_ms: now(),
        };
        let start = self.take_snapshot(&ws, "task-start")?;
        let task = Task { start_snapshot: Some(start.id.clone()), ..task };
        let program = p["program"].as_str().map(str::to_string);
        let version = match harness {
            "generic" => None,
            h => adapters::resolve_program(h).and_then(|prog| adapters::version_of(&prog)),
        };
        let run = Run {
            id: format!("r-{}", short_id()),
            task_id: task.id.clone(),
            parent_run_id: None,
            harness: harness.into(),
            harness_version: version,
            profile_id: profile.as_ref().map(|p| p.id.clone()),
            model: p["model"].as_str().filter(|s| !s.is_empty()).map(str::to_string),
            effort,
            workspace_id: ws.id.clone(),
            native_id: None,
            status: "queued".into(),
            exit_reason: None,
            created_ms: now(),
            ended_ms: None,
            title: title.clone(),
            relation_source: None,
            relation_confidence: None,
            capabilities: adapters::capabilities(harness),
            process_generation: 0,
            attention: None,
        };
        {
            // Task and run appear together: a state snapshot never shows a task without its run.
            let store = self.store.lock().unwrap();
            store.insert_task(&task)?;
            store.insert_run(&run)?;
            store.set_workspace_owner(&ws.id, Some(&run.id))?;
        }
        let generic = json!({"program": program, "args": p["args"].clone(), "approval": p["approval_policy"].as_str().unwrap_or("on-request"), "extra_args": p["extra_args"].clone()});
        {
            let store = self.store.lock().unwrap();
            store.conn.execute("UPDATE runs SET launch=?2 WHERE id=?1", rusqlite::params![run.id, generic.to_string()])?;
        }
        self.emit(Some(&task.id), Some(&run.id), "task_created", "daemon", "exact", json!({"task": task, "workspace": ws, "run": run}))?;
        let started = self.start_turn(&run.id, &prompt, false);
        let run = self.run(&run.id)?;
        let task = self.task(&task.id)?;
        if let Err(e) = started {
            return Ok(json!({"task": task, "run": run, "workspace": ws, "launch_error": e.to_string()}));
        }
        Ok(json!({"task": task, "run": run, "workspace": ws}))
    }

    /// Execute a bounded work unit in a separately supervised workspace made
    /// from a settled parent's exact snapshot. Route selection happens before
    /// this execution boundary; this method never guesses a model or account.
    pub fn delegate_run(self: &Arc<Self>, p: &Value) -> Result<Value> {
        let work_unit_id = p["work_unit_id"].as_str().filter(|s| !s.is_empty() && s.len() <= 120
            && s.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_')))
            .ok_or_else(|| anyhow!("work_unit_id must be a stable 1-120 character identifier"))?;
        let parent_id = p["parent_run_id"].as_str().ok_or_else(|| anyhow!("parent_run_id is required"))?;
        let initial = self.run(parent_id)?;
        let parent_workspace_gate = self.workspace_gate(&initial.workspace_id);
        let parent_guard = parent_workspace_gate.lock().unwrap();
        let harness = p["harness"].as_str().ok_or_else(|| anyhow!("delegation harness is required"))?;
        if !["codex", "codex-app", "claude", "opencode"].contains(&harness) {
            bail!("delegation requires a supported account-based harness");
        }
        let prompt = p["prompt"].as_str().filter(|s| !s.is_empty() && s.len() <= 32_768)
            .ok_or_else(|| anyhow!("delegation prompt must be 1-32768 bytes"))?;
        let title = p["title"].as_str().unwrap_or("delegated work").trim();
        if title.is_empty() || title.len() > 80 {
            bail!("delegation title must be 1-80 bytes");
        }
        let model = p["model"].as_str().filter(|s| !s.is_empty() && s.len() <= 120
            && s.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'/')))
            .ok_or_else(|| anyhow!("delegation requires a valid model identifier"))?.to_string();
        let effort = p["effort"].as_str().ok_or_else(|| anyhow!("delegation requires a reasoning effort"))?.to_string();
        adapters::validate_effort(harness, Some(&effort))?;
        let required_tools = match p.get("required_tools") {
            None | Some(Value::Null) => Vec::new(),
            Some(Value::Array(values)) if values.len() <= 16 => {
                let mut tools = Vec::new();
                for value in values {
                    let tool = value.as_str().filter(|tool| valid_required_tool(tool))
                        .ok_or_else(|| anyhow!("invalid required tool identity"))?;
                    if tools.contains(&tool.to_string()) { bail!("duplicate required tool"); }
                    tools.push(tool.to_string());
                }
                tools.sort();
                tools
            }
            _ => bail!("required_tools must be a bounded list"),
        };
        if !required_tools.is_empty() && harness != "codex-app" {
            bail!("required-tool preflight is not supported for this harness");
        }
        let profile_id = p["profile_id"].as_str().map(str::to_string)
            .unwrap_or_else(|| format!("system-{}", profile_harness(harness)));
        let request = json!({"parent_run_id":parent_id,"harness":harness,"profile_id":profile_id,
            "model":model,"effort":effort,"prompt":prompt,"title":title,"required_tools":required_tools});
        let mut request = request;
        if p["auto_selected"] == true {
            let execution_budget_ms = match p.get("execution_budget_ms") {
                None => DEFAULT_AUTO_EXECUTION_BUDGET_MS,
                Some(value) => value.as_u64().ok_or_else(|| anyhow!("automatic execution budget must be an integer"))?,
            };
            if !(1_000..=1_800_000).contains(&execution_budget_ms) {
                bail!("automatic execution budget must be 1000-1800000 ms");
            }
            let requirements_hash = p["requirements_hash"].as_str().filter(|value| value.len() == 64
                && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
                .ok_or_else(|| anyhow!("automatic delegation requires a requirements hash"))?;
            request["auto_selected"] = json!(true);
            request["requirements_hash"] = json!(requirements_hash);
            request["execution_budget_ms"] = json!(execution_budget_ms);
            if harness == "opencode" {
                let endpoint = p["auto_local_endpoint"].as_str()
                    .ok_or_else(|| anyhow!("automatic local OpenCode requires a selected endpoint"))?;
                request["auto_local_endpoint"] = json!(endpoint);
            } else {
                let expected_generation = p["expected_account_generation"].as_i64().filter(|generation| *generation > 0)
                    .ok_or_else(|| anyhow!("automatic delegation requires an account generation"))?;
                request["expected_account_generation"] = json!(expected_generation);
            }
        }
        let request_hash = {
            use sha2::{Digest, Sha256};
            Sha256::digest(serde_json::to_vec(&request)?).iter().map(|byte| format!("{byte:02x}")).collect::<String>()
        };
        let saved_work_unit = { self.store.lock().unwrap().managed_work_unit(work_unit_id)? };
        if let Some((saved_parent, child_id, saved_hash)) = saved_work_unit {
            if saved_parent != parent_id || saved_hash != request_hash {
                bail!("work_unit_id was already used for different delegated work");
            }
            let child = self.run(&child_id)?;
            let workspace = self.workspace(&child.workspace_id)?;
            return Ok(json!({"work_unit_id":work_unit_id,"run":child,"workspace":workspace,"replayed":true}));
        }
        let parent = self.run(parent_id)?;
        if parent.parent_run_id.is_some() || parent.status != "completed" {
            bail!("delegation requires a completed top-level parent checkpoint");
        }
        let parent_ws = self.workspace(&parent.workspace_id)?;
        if parent_ws.removed_ms.is_some() || self.active_writer(&parent_ws.path)?.is_some() {
            bail!("parent workspace is unavailable or has an active writer");
        }
        let profile = self.profile(&profile_id)?;
        if profile.harness != profile_harness(harness) {
            bail!("delegation profile belongs to another harness");
        }
        if p["auto_selected"] == true && harness == "opencode" {
            let endpoint = request["auto_local_endpoint"].as_str().unwrap();
            crate::auto_opencode::auto_local_inline_config(&profile, Path::new(&parent_ws.path),
                &model, endpoint)?;
            if crate::auto_opencode::probe_local_endpoint(endpoint)
                != crate::auto_opencode::EndpointProbe::Reachable {
                bail!("selected local OpenCode endpoint is unavailable before child creation");
            }
        }
        let parent_approval = {
            let store = self.store.lock().unwrap();
            let launch: Option<String> = store.conn.query_row(
                "SELECT launch FROM runs WHERE id=?1", [&parent.id], |row| row.get(0))?;
            let launch: Value = launch.as_deref().and_then(|value| serde_json::from_str(value).ok())
                .unwrap_or(Value::Null);
            let generic = launch.get("generic").unwrap_or(&launch);
            generic["approval"].as_str()
                .ok_or_else(|| anyhow!("parent approval policy is unavailable"))?.to_string()
        };
        let snapshot = self.take_snapshot(&parent_ws, "managed-delegation")?;
        let repo = Path::new(&parent_ws.repo_root);
        let repo_name = repo.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "repo".into());
        let hash = &fingerprint(&parent_ws.common_dir)[..8];
        let worktrees = paths::worktrees_dir().join(format!("{repo_name}-{hash}"));
        let (path, branch) = git::worktree_add(repo, &worktrees, &format!("delegate-{}-{title}", parent.id), &snapshot.commit_sha)?;
        let ws = Workspace {
            id: format!("w-{}", short_id()), path: path.display().to_string(),
            repo_root: parent_ws.repo_root.clone(), common_dir: parent_ws.common_dir.clone(),
            kind: "worktree".into(), branch: Some(branch), owner_run_id: None,
            initial_dirty: json!({"clean":true,"parent_snapshot":snapshot.id}),
            created_ms: now(), removed_ms: None,
        };
        let run = Run {
            id: format!("r-{}", short_id()), task_id: parent.task_id.clone(),
            parent_run_id: Some(parent.id.clone()), harness: harness.into(),
            harness_version: adapters::resolve_program(harness).and_then(|program| adapters::version_of(&program)),
            profile_id: Some(profile.id.clone()), model: Some(model), effort: Some(effort),
            workspace_id: ws.id.clone(), native_id: None, status: "queued".into(),
            exit_reason: None, created_ms: now(), ended_ms: None, title: title.into(),
            relation_source: Some("managed-delegation".into()),
            relation_confidence: Some("exact (Overseer-created work unit)".into()),
            capabilities: adapters::capabilities(harness), process_generation: 0, attention: None,
        };
        let saved = (|| -> Result<()> {
            let store = self.store.lock().unwrap();
            store.conn.execute_batch("SAVEPOINT managed_child_create")?;
            let writes = (|| -> Result<()> {
                store.insert_workspace(&ws)?;
                store.insert_run(&run)?;
                store.insert_managed_work_unit(work_unit_id, &parent.id, &run.id, &request_hash)?;
                store.set_workspace_owner(&ws.id, Some(&run.id))?;
                store.conn.execute("UPDATE runs SET launch=?2 WHERE id=?1",
                    rusqlite::params![run.id, json!({"approval":parent_approval,"extra_args":[],"required_tools":required_tools,
                        "auto_selected":p["auto_selected"] == true,"expected_account_generation":p["expected_account_generation"],
                        "execution_budget_ms":request["execution_budget_ms"],
                        "auto_local_endpoint":request["auto_local_endpoint"],
                        "requirements_hash":p["requirements_hash"]}).to_string()])?;
                Ok(())
            })();
            match writes {
                Ok(()) => store.conn.execute_batch("RELEASE managed_child_create")?,
                Err(error) => {
                    let _ = store.conn.execute_batch("ROLLBACK TO managed_child_create; RELEASE managed_child_create");
                    return Err(error);
                }
            }
            Ok(())
        })();
        if let Err(error) = saved {
            let _ = git::worktree_remove(repo, &path);
            return Err(error);
        }
        drop(parent_guard);
        self.emit(Some(&run.task_id), Some(&run.id), "managed_child_created", "daemon", "exact",
            json!({"parent_run_id":parent.id,"run":run,"workspace":ws,"snapshot_id":snapshot.id}))?;
        if let Err(error) = self.start_turn(&run.id, prompt, false) {
            self.mark_ended(&run, "failed", &format!("delegated launch failed: {error}"))?;
            return Ok(json!({"work_unit_id":work_unit_id,"run":self.run(&run.id)?,"workspace":ws,"launch_error":error.to_string()}));
        }
        Ok(json!({"work_unit_id":work_unit_id,"run":self.run(&run.id)?,"workspace":ws,"snapshot_id":snapshot.id}))
    }

    /// A stable result handle for a settled managed child. The event sequence
    /// lets the eventual coordinator deduplicate delivery across reconnects.
    pub fn delegated_result(&self, run_id: &str) -> Result<Value> {
        let run = self.run(run_id)?;
        if run.relation_source.as_deref() != Some("managed-delegation") {
            bail!("run is not an Overseer-managed child");
        }
        if ACTIVE.contains(&run.status.as_str()) {
            return Ok(json!({"state":"pending","run_id":run.id,"parent_run_id":run.parent_run_id}));
        }
        if run.status != "completed" {
            return Ok(json!({"state":"not_completed","run_id":run.id,"parent_run_id":run.parent_run_id,"status":run.status,"reason":run.exit_reason}));
        }
        let events = self.store.lock().unwrap().events_after(0, Some(run_id), 5000)?;
        let output = events.iter().rev().find(|event| event.kind == "output" && event.payload["role"] == "assistant"
            && event.payload["text"].as_str().is_some());
        let Some(output) = output else {
            return Ok(json!({"state":"completed_without_text","run_id":run.id,"parent_run_id":run.parent_run_id}));
        };
        let text: String = output.payload["text"].as_str().unwrap().chars().take(8192).collect();
        Ok(json!({"state":"ready","run_id":run.id,"parent_run_id":run.parent_run_id,
            "event_seq":output.seq,"text":text,"workspace_id":run.workspace_id}))
    }

    /// Start a work turn: fresh run-start snapshot, then launch (or stdin for live generic processes).
    pub fn start_turn(self: &Arc<Self>, run_id: &str, prompt: &str, follow_up: bool) -> Result<Turn> {
        let initial = self.run(run_id)?;
        let profile_gate = initial.profile_id.as_deref().map(|id| self.profile_gate(id));
        let _profile_guard = profile_gate.as_ref().map(|gate| gate.lock().unwrap());
        let workspace_gate = self.workspace_gate(&initial.workspace_id);
        let _workspace_guard = workspace_gate.lock().unwrap();
        let run = self.run(run_id)?;
        if follow_up && run.relation_source.as_deref() == Some("managed-delegation") {
            bail!("a managed work unit has one result; delegate a new work unit instead");
        }
        if run.parent_run_id.is_some() && run.relation_source.as_deref() != Some("managed-delegation") {
            bail!("follow-ups go to the top-level run; native children are controlled by their parent harness");
        }
        let ws = self.workspace(&run.workspace_id)?;
        if ws.removed_ms.is_some() {
            bail!("workspace was removed");
        }
        if follow_up {
            if ACTIVE.contains(&run.status.as_str()) && adapters::follow_up_via_stdin(&run.harness, prompt).is_none() {
                bail!("run is still working; interrupt it or wait for it to finish before sending a follow-up");
            }
            if !ACTIVE.contains(&run.status.as_str()) {
                if let Some(other) = self.active_writer(&ws.path)? {
                    if other.id != run.id {
                        bail!("workspace has another active writer ({})", other.id);
                    }
                }
            }
        }
        if run.harness == "claude" && run.relation_source.as_deref() == Some("managed-delegation") {
            let launch: Option<String> = self.store.lock().unwrap().conn.query_row(
                "SELECT launch FROM runs WHERE id=?1", [run_id], |row| row.get(0))?;
            let launch: Value = launch.as_deref().and_then(|text| serde_json::from_str(text).ok())
                .unwrap_or(Value::Null);
            if launch["auto_selected"] == true {
                let profile_id = run.profile_id.as_deref().ok_or_else(|| anyhow!("automatic Claude profile unavailable"))?;
                let expected = launch["expected_account_generation"].as_i64()
                    .ok_or_else(|| anyhow!("automatic Claude account generation unavailable"))?;
                let program = adapters::resolve_program("claude")
                    .ok_or_else(|| anyhow!("Claude executable unavailable"))?;
                let profile = self.profile(profile_id)?;
                let auth = crate::auto_collect::claude_auth_status(&program, &Self::profile_env(&profile),
                    std::time::Duration::from_secs(5), now())?;
                let store = self.store.lock().unwrap();
                store.record_auto_account_identity(profile_id, &auth.fingerprint)?;
                if store.auto_account_generation(profile_id)? != Some(expected) {
                    bail!("automatic Claude account changed before the model turn");
                }
                if let Some(model) = run.model.as_deref() {
                    if let Some(observation) = store.latest_auto_quota(profile_id)? {
                        if observation.snapshot.state_for(model, now()) == crate::auto_quota::QuotaState::Exhausted {
                            bail!("automatic Claude allowance is exhausted");
                        }
                    }
                }
            }
        }
        let snap = self.take_snapshot(&ws, "run-start")?;
        let n = self.store.lock().unwrap().turns(run_id)?.len() as i64 + 1;
        let turn = Turn { id: format!("u-{}", short_id()), run_id: run_id.into(), n, prompt: prompt.into(), snapshot_id: Some(snap.id.clone()), started_ms: now(), ended_ms: None, status: "running".into() };
        self.store.lock().unwrap().insert_turn(&turn)?;
        self.emit(Some(&run.task_id), Some(run_id), "turn_started", "daemon", "exact", json!({"turn": turn, "snapshot": snap.commit_sha}))?;
        if follow_up && ACTIVE.contains(&run.status.as_str()) {
            if let Some(line) = adapters::follow_up_via_stdin(&run.harness, prompt) {
                self.send_stdin(&run, &line)?;
                return Ok(turn);
            }
        }
        let mut profile_env = match &run.profile_id {
            Some(id) => Self::profile_env(&self.profile(id)?),
            None => BTreeMap::new(),
        };
        let launch_meta: Value = {
            let store = self.store.lock().unwrap();
            store.conn.query_row("SELECT launch FROM runs WHERE id=?1", [run_id], |r| r.get::<_, Option<String>>(0))?.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(Value::Null)
        };
        let generic_meta = launch_meta.get("generic").cloned().unwrap_or(launch_meta.clone());
        if run.harness == "opencode" && generic_meta["auto_selected"] == true {
            let endpoint = generic_meta["auto_local_endpoint"].as_str()
                .ok_or_else(|| anyhow!("automatic local OpenCode endpoint is unavailable"))?;
            let profile = self.profile(run.profile_id.as_deref()
                .ok_or_else(|| anyhow!("automatic local OpenCode profile is unavailable"))?)?;
            let model = run.model.as_deref()
                .ok_or_else(|| anyhow!("automatic local OpenCode model is unavailable"))?;
            let inline = crate::auto_opencode::auto_local_inline_config(&profile,
                Path::new(&ws.path), model, endpoint)?;
            if crate::auto_opencode::probe_local_endpoint(endpoint)
                != crate::auto_opencode::EndpointProbe::Reachable {
                bail!("selected local OpenCode endpoint is unavailable before the model turn");
            }
            profile_env.insert("OPENCODE_CONFIG_CONTENT".into(), inline);
            profile_env.insert("OPENCODE_DISABLE_MODELS_FETCH".into(), "true".into());
            profile_env.insert("OPENCODE_DISABLE_DEFAULT_PLUGINS".into(), "true".into());
        }
        let args: Option<Vec<String>> = generic_meta["args"].as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect());
        let extra_args: Vec<String> = generic_meta["extra_args"].as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()).unwrap_or_default();
        let resume = if follow_up { run.native_id.clone() } else { None };
        if follow_up && resume.is_none() && run.harness != "generic" {
            bail!("no native session id was reported for this run, so it cannot be resumed");
        }
        let launch = adapters::launch(
            &run.harness,
            &LaunchReq {
                cwd: Path::new(&ws.path),
                prompt,
                model: run.model.as_deref(),
                effort: run.effort.as_deref(),
                profile_env,
                resume_session: resume.as_deref(),
                program_override: generic_meta["program"].as_str(),
                args_override: args.as_deref(),
                extra_args: &extra_args,
            },
        )?;
        self.store.lock().unwrap().set_workspace_owner(&ws.id, Some(run_id))?;
        let app = json!({"prompt": prompt, "cwd": ws.path, "model": run.model, "effort": run.effort, "resume": resume, "approval": generic_meta["approval"].as_str().unwrap_or("on-request"),
            "required_tools":generic_meta["required_tools"], "auto_selected":generic_meta["auto_selected"],
            "expected_account_generation":generic_meta["expected_account_generation"]});
        self.spawn_process(&run, &ws, launch, json!({"generic": generic_meta, "app": app}))?;
        Ok(turn)
    }

    fn spawn_process(self: &Arc<Self>, run: &Run, ws: &Workspace, launch: adapters::Launch, meta: Value) -> Result<()> {
        let generation = run.process_generation + 1;
        let run_dir = paths::runs_dir().join(&run.id).join(format!("p{generation}"));
        paths::ensure_private_dir(&run_dir)?;
        let control = control_socket_path(&run.id, generation);
        let file = LaunchFile {
            program: launch.program.clone(),
            args: launch.args.clone(),
            cwd: ws.path.clone(),
            env: launch.env.clone(),
            initial_stdin: launch.initial_stdin.clone(),
            close_stdin: launch.close_stdin,
            control_socket: control.display().to_string(),
        };
        std::fs::write(run_dir.join("launch.json"), serde_json::to_vec_pretty(&file)?)?;
        if run.relation_source.as_deref() == Some("managed-delegation") && run.harness == "codex-app" {
            std::fs::write(run_dir.join("auto-account-deadline"), now().saturating_add(5_000).to_string())?;
        }
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(run_dir.join("launch.json"), std::fs::Permissions::from_mode(0o600))?;
        }
        let err = std::fs::File::create(run_dir.join("shim.err"))?;
        let mut cmd = std::process::Command::new(&self.exe);
        cmd.arg("shim").arg(&run_dir).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(err);
        unsafe {
            cmd.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
        let mut child = cmd.spawn().context("starting run supervisor")?;
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        let mut meta = meta;
        meta["program"] = json!(launch.program);
        meta["args"] = json!(launch.args.iter().map(|a| redact(a)).collect::<Vec<_>>());
        meta["env_keys"] = json!(launch.env.keys().collect::<Vec<_>>());
        self.store.lock().unwrap().set_run_process(&run.id, &run_dir.display().to_string(), generation, &meta)?;
        self.store.lock().unwrap().update_run_status(&run.id, "starting", None, None)?;
        {
            let store = self.store.lock().unwrap();
            store.conn.execute("UPDATE runs SET exit_reason=NULL, ended_ms=NULL, attention=NULL WHERE id=?1", [&run.id])?;
        }
        self.emit(Some(&run.task_id), Some(&run.id), "status", "daemon", "exact", json!({"status": "starting", "generation": generation, "program": launch.program}))?;
        self.spawn_tail(&run.id);
        if run.relation_source.as_deref() == Some("managed-delegation") && run.harness == "codex-app" {
            self.watch_managed_codex_handshake(run.id.clone(), generation, run_dir);
        }
        Ok(())
    }

    /// Metadata calls must not leave an unstarted managed child holding its
    /// workspace indefinitely. The deadline lives beside the supervisor so a
    /// daemon restart cannot reset it or launch another child.
    fn watch_managed_codex_handshake(self: &Arc<Self>, run_id: String, generation: i64, dir: PathBuf) {
        let daemon = self.clone();
        tokio::spawn(async move {
            let deadline = std::fs::read_to_string(dir.join("auto-account-deadline"))
                .ok().and_then(|value| value.parse::<i64>().ok())
                .unwrap_or_else(|| now().saturating_add(5_000));
            let remaining = deadline.saturating_sub(now()).max(0) as u64;
            tokio::time::sleep(std::time::Duration::from_millis(remaining)).await;
            let stalled = daemon.run(&run_id).ok().is_some_and(|run| {
                run.process_generation == generation && run.native_id.is_none() && ACTIVE.contains(&run.status.as_str())
            });
            if !stalled { return; }
            let _ = std::fs::write(dir.join("auto-account-timeout"), b"deadline exceeded\n");
            if let Ok(run) = daemon.run(&run_id) {
                let _ = daemon.emit(Some(&run.task_id), Some(&run.id), "auto_account_timeout", "daemon", "exact",
                    json!({"reason":"managed Codex metadata handshake timed out before model launch"}));
            }
            if let Some(sock) = daemon.run(&run_id).ok().and_then(|run| daemon.control_socket(&run).ok()) {
                let _ = shim::control(&sock, &json!({"op":"signal","sig":libc::SIGTERM}));
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                let still_stalled = daemon.run(&run_id).ok().is_some_and(|run| {
                    run.process_generation == generation && run.native_id.is_none() && ACTIVE.contains(&run.status.as_str())
                });
                if still_stalled {
                    let _ = shim::control(&sock, &json!({"op":"signal","sig":libc::SIGKILL}));
                }
            }
        });
    }

    pub(crate) fn control_socket(&self, run: &Run) -> Result<PathBuf> {
        let (dir, _, _) = self.store.lock().unwrap().run_process(&run.id)?.ok_or_else(|| anyhow!("run has no process"))?;
        let launch: LaunchFile = serde_json::from_slice(&std::fs::read(Path::new(&dir).join("launch.json"))?)?;
        Ok(PathBuf::from(launch.control_socket))
    }

    fn send_stdin(&self, run: &Run, data: &str) -> Result<()> {
        let sock = self.control_socket(run)?;
        let reply = shim::control(&sock, &json!({"op": "stdin", "data": data}))?;
        if reply["ok"] != true {
            bail!("could not write to the harness: {}", reply["error"]);
        }
        Ok(())
    }

    pub fn interrupt(self: &Arc<Self>, run_id: &str) -> Result<Value> {
        self.interrupt_with_origin(run_id, None)
    }

    fn interrupt_with_origin(self: &Arc<Self>, run_id: &str, auto_budget_ms: Option<u64>) -> Result<Value> {
        let run = self.run(run_id)?;
        if run.parent_run_id.is_some() && run.relation_source.as_deref() != Some("managed-delegation") {
            bail!("native children are interrupted through their parent run");
        }
        if !ACTIVE.contains(&run.status.as_str()) {
            bail!("run is not active (status {})", run.status);
        }
        let mut child_interrupt_errors = Vec::new();
        if run.parent_run_id.is_none() {
            let children = self.store.lock().unwrap().children(run_id)?;
            for child in children.into_iter().filter(|child| child.relation_source.as_deref() == Some("managed-delegation") && ACTIVE.contains(&child.status.as_str())) {
                if let Err(error) = self.interrupt_with_origin(&child.id, None) {
                    child_interrupt_errors.push(json!({"run_id":child.id,"error":error.to_string()}));
                }
            }
        }
        let (dir, _, _) = self.store.lock().unwrap().run_process(run_id)?.ok_or_else(|| anyhow!("run has no process"))?;
        let budget_marker = Path::new(&dir).join("auto-budget.requested");
        let first_budget_stop = auto_budget_ms.is_some() && !budget_marker.exists();
        if let Some(ms) = auto_budget_ms {
            std::fs::write(&budget_marker, ms.to_string())?;
            if first_budget_stop {
                self.emit(Some(&run.task_id), Some(run_id), "auto_execution_budget_exhausted", "daemon", "exact",
                    json!({"execution_budget_ms":ms,"outcome":"stopping_existing_child"}))?;
            }
        }
        std::fs::write(Path::new(&dir).join("interrupt.requested"), now().to_string())?;
        if auto_budget_ms.is_none() || first_budget_stop {
            self.emit(Some(&run.task_id), Some(run_id), "interrupt_requested",
                if auto_budget_ms.is_some() { "daemon" } else { "user" }, "exact",
                if auto_budget_ms.is_some() { json!({"reason":"auto_execution_budget"}) } else { json!({}) })?;
        }
        let sock = self.control_socket(&run)?;
        let plan = if run.harness == "codex-app" {
            let turn = std::fs::read_to_string(Path::new(&dir).join("turn.id")).unwrap_or_default();
            match (&run.native_id, turn.is_empty()) {
                (Some(thread), false) => InterruptPlan::StdinThenSignal(format!("{}\n", json!({"id": "ovs-interrupt", "method": "turn/interrupt", "params": {"threadId": thread, "turnId": turn}}))),
                _ => InterruptPlan::Signal,
            }
        } else {
            adapters::interrupt_plan(&run.harness)
        };
        match plan {
            InterruptPlan::Signal => {
                shim::control(&sock, &json!({"op": "signal", "sig": libc::SIGINT}))?;
            }
            InterruptPlan::StdinThenSignal(msg) => {
                let _ = shim::control(&sock, &json!({"op": "stdin", "data": msg}));
                let daemon = self.clone();
                let run_id = run_id.to_string();
                tokio::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                    if let Ok(run) = daemon.run(&run_id) {
                        if ACTIVE.contains(&run.status.as_str()) {
                            let _ = shim::control(&sock, &json!({"op": "close_stdin"}));
                            let _ = shim::control(&sock, &json!({"op": "signal", "sig": libc::SIGINT}));
                        }
                    }
                });
            }
        }
        // Escalate if the harness ignores SIGINT.
        let daemon = self.clone();
        let run_id = run_id.to_string();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(10)).await;
            if let Ok(run) = daemon.run(&run_id) {
                if ACTIVE.contains(&run.status.as_str()) {
                    if let Ok(sock) = daemon.control_socket(&run) {
                        let _ = shim::control(&sock, &json!({"op": "signal", "sig": libc::SIGTERM}));
                    }
                }
            }
        });
        if auto_budget_ms.is_some() {
            let daemon = self.clone();
            let run_id = run.id.clone();
            let generation = run.process_generation;
            tokio::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_secs(15)).await;
                if let Ok(run) = daemon.run(&run_id) {
                    if run.process_generation == generation && ACTIVE.contains(&run.status.as_str()) {
                        if let Ok(sock) = daemon.control_socket(&run) {
                            let _ = shim::control(&sock, &json!({"op":"signal","sig":libc::SIGKILL}));
                        }
                    }
                }
            });
        }
        if !child_interrupt_errors.is_empty() {
            self.emit(Some(&run.task_id), Some(&run.id), "managed_child_interrupt_failed", "daemon", "exact",
                json!({"children":child_interrupt_errors}))?;
        }
        Ok(json!({"ok": true,"child_interrupt_errors":child_interrupt_errors}))
    }

    pub fn answer_permission(&self, run_id: &str, request_id: &str, allow: bool, message: &str) -> Result<Value> {
        let run = self.run(run_id)?;
        let attention = run.attention.clone().ok_or_else(|| anyhow!("run has no pending permission request"))?;
        if attention["request_id"].as_str() != Some(request_id) {
            bail!("permission request {request_id} is not pending");
        }
        let reply = adapters::permission_reply(&run.harness, request_id, allow, &attention["input"], if message.is_empty() { "Denied by user in Overseer" } else { message })
            .ok_or_else(|| anyhow!("{} does not support permission replies", run.harness))?;
        self.send_stdin(&run, &reply)?;
        {
            let store = self.store.lock().unwrap();
            store.set_run_attention(run_id, None)?;
            store.update_run_status(run_id, "running", None, None)?;
        }
        self.emit(Some(&run.task_id), Some(run_id), "permission_answered", "user", "exact", json!({"request_id": request_id, "allow": allow}))?;
        self.emit(Some(&run.task_id), Some(run_id), "status", "daemon", "exact", json!({"status": "running"}))?;
        Ok(json!({"ok": true}))
    }

    // ------------------------------------------------------------------ output tailing

    pub fn spawn_tail(self: &Arc<Self>, run_id: &str) {
        if !self.tails.lock().unwrap().insert(run_id.to_string()) {
            return;
        }
        let daemon = self.clone();
        let run_id = run_id.to_string();
        tokio::task::spawn_blocking(move || {
            if let Err(e) = daemon.tail_loop(&run_id) {
                let _ = daemon.emit(None, Some(&run_id), "daemon_error", "daemon", "exact", json!({"message": e.to_string()}));
            }
            daemon.tails.lock().unwrap().remove(&run_id);
        });
    }

    fn tail_loop(self: &Arc<Self>, run_id: &str) -> Result<()> {
        let mut last_liveness = std::time::Instant::now();
        let mut state = TailState::default();
        let mut announced = false;
        let auto_deadline = {
            let store = self.store.lock().unwrap();
            let launch: Option<String> = store.conn.query_row(
                "SELECT launch FROM runs WHERE id=?1", [run_id], |row| row.get(0))?;
            let launch: Value = launch.as_deref().and_then(|value| serde_json::from_str(value).ok())
                .unwrap_or(Value::Null);
            let generic = launch.get("generic").unwrap_or(&launch);
            if generic["auto_selected"] == true {
                let budget = generic["execution_budget_ms"].as_i64();
                let started = store.turns(run_id)?.first().map(|turn| turn.started_ms);
                budget.zip(started).map(|(budget, started)| (budget, started.saturating_add(budget)))
            } else { None }
        };
        let mut budget_stop_sent = false;
        loop {
            let run = self.run(run_id)?;
            if let Some((budget_ms, deadline_ms)) = auto_deadline {
                if !budget_stop_sent && now() >= deadline_ms && ACTIVE.contains(&run.status.as_str()) {
                    let process = self.store.lock().unwrap().run_process(run_id)?;
                    if let Some((dir, _, _)) = process {
                        if !Path::new(&dir).join("exit.json").exists() {
                            self.interrupt_with_origin(run_id, Some(budget_ms as u64))?;
                            budget_stop_sent = true;
                        }
                    }
                }
            }
            if !announced && run.status == "starting" {
                let process = self.store.lock().unwrap().run_process(run_id)?;
                if let Some((dir, _, _)) = process {
                    if Path::new(&dir).join("shim.json").exists() {
                        announced = true;
                        self.store.lock().unwrap().update_run_status(run_id, "running", None, None)?;
                        self.emit(Some(&run.task_id), Some(run_id), "status", "supervisor", "exact", json!({"status": "running", "why": "harness process started"}))?;
                        continue;
                    }
                }
            }
            let process = self.store.lock().unwrap().run_process(run_id)?;
            let Some((dir, mut seg, mut off)) = process else { return Ok(()) };
            let dir = PathBuf::from(dir);
            let path = shim::segment_path(&dir, seg as u64);
            let mut progressed = false;
            if let Ok(mut file) = std::fs::File::open(&path) {
                file.seek(SeekFrom::Start(off as u64))?;
                let mut buf = Vec::new();
                file.by_ref().take(1024 * 1024).read_to_end(&mut buf)?;
                if let Some(end) = buf.iter().rposition(|b| *b == b'\n') {
                    let chunk = &buf[..=end];
                    let mut lines = Vec::new();
                    for line in chunk.split(|b| *b == b'\n').filter(|l| !l.is_empty()) {
                        if let Ok(rec) = serde_json::from_slice::<Value>(line) {
                            lines.push(rec);
                        }
                    }
                    off += chunk.len() as i64;
                    self.apply_lines(&run, &lines, seg, off, &mut state)?;
                    progressed = true;
                }
            }
            if progressed {
                continue;
            }
            if run.harness == "opencode" && state.store_polled.elapsed().as_millis() > 1200 {
                state.store_polled = std::time::Instant::now();
                self.poll_opencode_store(&run, &mut state)?;
            }
            if shim::segment_path(&dir, seg as u64 + 1).exists() {
                seg += 1;
                off = 0;
                self.store.lock().unwrap().set_run_cursor(run_id, seg, off)?;
                self.enforce_raw_retention(&run, &dir, seg as u64)?;
                continue;
            }
            if let Ok(bytes) = std::fs::read(dir.join("exit.json")) {
                // Re-check for output written between our read and the exit record.
                let len = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                if len as i64 > off {
                    continue;
                }
                let exit: ExitInfo = serde_json::from_slice(&bytes)?;
                if run.harness == "opencode" {
                    self.poll_opencode_store(&run, &mut state)?;
                }
                self.finalize(&run, &dir, &exit, &state)?;
                return Ok(());
            }
            if last_liveness.elapsed().as_secs() >= 2 {
                last_liveness = std::time::Instant::now();
                if !self.supervisor_alive(&dir) {
                    std::thread::sleep(std::time::Duration::from_millis(300));
                    if dir.join("exit.json").exists() {
                        continue;
                    }
                    let spawned = dir.join("shim.json").exists();
                    let reason = if spawned { "supervisor process disappeared without recording an exit (killed externally?); harness state unknown" } else { "supervisor never started" };
                    self.mark_ended(&run, "disconnected", reason)?;
                    return Ok(());
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(60));
        }
    }

    fn supervisor_alive(&self, dir: &Path) -> bool {
        match std::fs::read(dir.join("shim.json")).ok().and_then(|b| serde_json::from_slice::<ShimInfo>(&b).ok()) {
            Some(info) => pid_alive(info.shim_pid),
            // Not written yet: allow a short startup window.
            None => std::fs::metadata(dir.join("launch.json")).and_then(|m| m.modified()).map(|t| t.elapsed().map(|e| e.as_secs() < 10).unwrap_or(true)).unwrap_or(false),
        }
    }

    fn enforce_raw_retention(&self, run: &Run, dir: &Path, current: u64) -> Result<()> {
        if current < RAW_SEGMENTS_KEPT {
            return Ok(());
        }
        let drop_through = current - RAW_SEGMENTS_KEPT;
        let mut removed = Vec::new();
        for n in 0..=drop_through {
            let p = shim::segment_path(dir, n);
            if p.exists() {
                std::fs::remove_file(&p)?;
                removed.push(n);
            }
        }
        if !removed.is_empty() {
            self.emit(Some(&run.task_id), Some(&run.id), "retention", "daemon", "exact", json!({"raw_segments_removed": removed, "note": "older raw output was discarded by the retention bound"}))?;
        }
        Ok(())
    }

    fn apply_lines(self: &Arc<Self>, run: &Run, lines: &[Value], seg: i64, off: i64, state: &mut TailState) -> Result<()> {
        let mut emitted = Vec::new();
        {
            let store = self.store.lock().unwrap();
            let tx = store.conn.unchecked_transaction()?;
            let root_native = if run.harness == "codex-app" { store.run(&run.id)?.and_then(|r| r.native_id) } else { None };
            for rec in lines {
                let stream = rec["s"].as_str().unwrap_or("o");
                let data = rec["d"].as_str().unwrap_or_default();
                let mut norms = adapters::parse(&run.harness, stream, data);
                if run.harness == "codex-app" && stream == "o" {
                    let thread = serde_json::from_str::<Value>(data).ok().and_then(|v| v["params"]["threadId"].as_str().map(str::to_string));
                    if let (Some(t), Some(root)) = (thread, &root_native) {
                        if &t != root {
                            norms = adapters::scope_codex_app_child(&t, norms);
                        }
                    }
                }
                for norm in norms {
                    self.apply_norm(&store, run, norm, state, &mut emitted)?;
                }
            }
            store.set_run_cursor(&run.id, seg, off)?;
            state.since_prune += emitted.len();
            if state.since_prune > 500 {
                state.since_prune = 0;
                if let Some(cutoff) = store.prune_run_events(&run.id, store::EVENTS_PER_RUN)? {
                    emitted.push(store.insert_event(now(), Some(&run.task_id), Some(&run.id), "retention", "daemon", "exact", &json!({"events_truncated_through_seq": cutoff}))?);
                }
            }
            tx.commit()?;
        }
        for e in emitted {
            let _ = self.events.send(e);
        }
        let sends = std::mem::take(&mut state.sends);
        if !sends.is_empty() {
            if let Ok(sock) = self.control_socket(run) {
                for text in sends {
                    let _ = shim::control(&sock, &json!({"op": "stdin", "data": text}));
                }
            }
        }
        if std::mem::take(&mut state.close_stdin) {
            if let Ok(sock) = self.control_socket(run) {
                let _ = shim::control(&sock, &json!({"op": "close_stdin"}));
            }
        }
        Ok(())
    }

    fn apply_norm(&self, store: &Store, run: &Run, norm: Norm, state: &mut TailState, out: &mut Vec<Event>) -> Result<()> {
        let task = Some(run.task_id.as_str());
        let rid = Some(run.id.as_str());
        let mut ev = |kind: &str, source: &str, conf: &str, payload: Value, run_override: Option<&str>| -> Result<()> {
            out.push(store.insert_event(now(), task, run_override.or(rid), kind, source, conf, &redact_value(payload))?);
            Ok(())
        };
        match norm {
            Norm::Session(id) => {
                if !id.is_empty() && state.session.as_deref() != Some(&id) {
                    state.session = Some(id.clone());
                    let current = store.run(&run.id)?.and_then(|r| r.native_id);
                    if current.is_none() {
                        store.set_run_native(&run.id, &id)?;
                        ev("session", "harness", "exact", json!({"native_id": id}), None)?;
                    }
                }
            }
            Norm::Running => {
                let status = store.run(&run.id)?.map(|r| r.status).unwrap_or_default();
                if status == "starting" || status == "queued" {
                    store.update_run_status(&run.id, "running", None, None)?;
                    ev("status", "harness", "exact", json!({"status": "running"}), None)?;
                }
            }
            Norm::Text { role, text } => ev("output", "harness", "exact", json!({"role": role, "text": text}), None)?,
            Norm::Tool { name, id, summary } => {
                let status = store.run(&run.id)?.map(|r| r.status).unwrap_or_default();
                if status == "starting" {
                    store.update_run_status(&run.id, "running", None, None)?;
                    ev("status", "harness", "inferred", json!({"status": "running", "why": "tool activity"}), None)?;
                }
                ev("tool", "harness", "exact", json!({"name": name, "id": id, "summary": summary}), None)?
            }
            Norm::ToolDetail { id, input, output, status, is_error } => {
                ev("tool_result", "harness", "exact", json!({"id": id, "input": input, "output": output, "status": status, "is_error": is_error}), None)?
            }
            Norm::FileChange { paths, kind, confidence } => {
                let ws = store.workspace(&run.workspace_id)?;
                let root = ws.map(|w| w.path).unwrap_or_default();
                let rel: Vec<String> = paths
                    .iter()
                    .map(|p| {
                        let path = Path::new(p);
                        let abs = if path.is_absolute() { path.to_path_buf() } else { Path::new(&root).join(path) };
                        let canon = std::fs::canonicalize(&abs).unwrap_or(abs);
                        canon.strip_prefix(&root).map(|r| r.display().to_string()).unwrap_or_else(|_| p.clone())
                    })
                    .collect();
                ev("file_activity", "harness", confidence, json!({"paths": rel, "kind": kind, "attribution": "reported by the agent harness"}), None)?
            }
            Norm::Child { native_id, parent_native, title, status, text, only_if_known, evidence } => {
                if native_id.is_empty() {
                    return Ok(());
                }
                let existing = find_in_tree(store, &run.id, &native_id)?;
                let child = match existing {
                    Some(c) => c,
                    None if only_if_known => return Ok(()),
                    None => {
                        let (parent_id, pending) = match &parent_native {
                            Some(p) => match find_in_tree(store, &run.id, p)? {
                                Some(r) => (r.id, None),
                                None => (run.id.clone(), Some(p.clone())),
                            },
                            None => (run.id.clone(), None),
                        };
                        let child = Run {
                            id: format!("r-{}", short_id()),
                            task_id: run.task_id.clone(),
                            parent_run_id: Some(parent_id),
                            harness: run.harness.clone(),
                            harness_version: run.harness_version.clone(),
                            profile_id: run.profile_id.clone(),
                            model: None,
                            effort: None,
                            workspace_id: run.workspace_id.clone(),
                            native_id: Some(native_id.clone()),
                            status: status.clone().unwrap_or_else(|| "running".into()),
                            exit_reason: None,
                            created_ms: now(),
                            ended_ms: None,
                            title: title.clone().unwrap_or_else(|| "native child".into()),
                            relation_source: Some(evidence.clone()),
                            relation_confidence: Some(match &pending {
                                Some(p) => format!("inferred: reported parent {p} not seen yet; attached to the root run provisionally"),
                                None => "exact (structured harness event)".into(),
                            }),
                            capabilities: json!({"control": "through parent harness only", "workspace": "shared with parent"}),
                            process_generation: 0,
                            attention: None,
                        };
                        store.insert_run(&child)?;
                        ev("child", "harness", if pending.is_some() { "inferred" } else { "exact" }, json!({"child": child, "evidence": evidence, "workspace": "shared with parent"}), None)?;
                        if let Some(p) = &pending {
                            store.conn.execute("UPDATE runs SET pending_parent_native=?2 WHERE id=?1", rusqlite::params![child.id, p])?;
                        }
                        // A delayed parent: adopt earlier-seen children that named this run as parent.
                        let orphans: Vec<String> = {
                            let mut stmt = store.conn.prepare("SELECT id FROM runs WHERE task_id=?1 AND pending_parent_native=?2")?;
                            let rows = stmt.query_map(rusqlite::params![run.task_id, native_id], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
                            rows
                        };
                        for orphan in orphans {
                            if is_ancestor_run(store, &orphan, &child.id)? {
                                continue; // never create a cycle
                            }
                            store.conn.execute(
                                "UPDATE runs SET parent_run_id=?2, pending_parent_native=NULL, relation_confidence='exact (structured harness event; parent reported later)' WHERE id=?1",
                                rusqlite::params![orphan, child.id],
                            )?;
                            ev("child_reparented", "harness", "exact", json!({"child_run_id": orphan, "parent_run_id": child.id}), Some(&orphan))?;
                        }
                        child
                    }
                };
                if let Some(st) = status {
                    if st != child.status {
                        let ended = if ACTIVE.contains(&st.as_str()) { None } else { Some(now()) };
                        store.update_run_status(&child.id, &st, None, ended)?;
                        ev("status", "harness", "exact", json!({"status": st, "evidence": evidence}), Some(&child.id))?;
                    }
                }
                if let Some(t) = text {
                    ev("output", "harness", "exact", json!({"role": "assistant", "text": t}), Some(&child.id))?;
                }
            }
            Norm::Usage(u) => {
                let event = store.insert_event(now(), task, rid, "usage", "harness", "exact", &redact_value(u))?;
                if run.harness == "codex-app" {
                    if let Some(limits) = event.payload.get("rate_limits") {
                        let pool_id = run.profile_id.as_deref().unwrap_or("system-codex");
                        let payload = json!({"rateLimits": limits});
                        if let Ok(snapshot) = crate::auto_quota::parse_codex_rate_limits(&payload, pool_id, event.ts) {
                            let _ = store.insert_auto_quota(event.seq, pool_id, "codex-app/native-update", &snapshot);
                        }
                    }
                }
                if let Some(measurement) = crate::auto_telemetry::from_usage_with_effort(
                    event.ts, &run.task_id, &run.id, &run.harness,
                    run.profile_id.as_deref(), run.model.as_deref(), run.effort.as_deref(), &event.payload,
                ) {
                    // Learning is best effort. The durable execution event remains authoritative.
                    let recorded = store.insert_auto_measurement(event.seq, &measurement);
                    self.learning_paused.store(recorded.is_err(), std::sync::atomic::Ordering::Relaxed);
                }
                out.push(event);
            }
            Norm::Quota(raw) => {
                if run.harness == "claude" {
                    if let Some(pool_id) = run.profile_id.as_deref() {
                        let observed_ms = now();
                        let parsed = crate::auto_quota::parse_claude_rate_limit_event(&raw, pool_id, observed_ms);
                        let mut snapshot = match parsed {
                            Ok(snapshot) => snapshot,
                            Err(_) if raw["rate_limit_info"]["status"] == "rejected" => {
                                // The provider's structured rejection is still a block when
                                // its meter or scope drifts beyond the supported schema.
                                crate::auto_quota::parse_claude_rate_limit_event(
                                    &json!({"type":"rate_limit_event","rate_limit_info":{"status":"rejected"}}),
                                    pool_id, observed_ms)?
                            }
                            Err(_) => crate::auto_quota::QuotaSnapshot {
                                ordinary_usage_allowed:None, observed_ms,
                                expires_ms:observed_ms.saturating_add(60_000), windows:Vec::new(),
                            },
                        };
                        if let Some(prior) = store.latest_auto_quota(pool_id)? {
                            snapshot.preserve_uncleared_blocks(&prior.snapshot, observed_ms);
                        }
                        let event = store.insert_event(observed_ms, task, rid, "auto_quota", "harness", "normalized",
                            &json!({"pool_id":pool_id,"snapshot":snapshot}))?;
                        let _ = store.insert_auto_quota(event.seq, pool_id, "claude/native-rate-limit-event", &snapshot);
                        out.push(event);
                    }
                }
            }
            Norm::Permission { request_id, tool, input } => {
                let attention = json!({"kind": "permission", "request_id": request_id, "tool": tool, "input": input});
                store.set_run_attention(&run.id, Some(&attention))?;
                store.update_run_status(&run.id, "waiting_for_user", None, None)?;
                ev("permission", "harness", "exact", attention, None)?;
                ev("status", "harness", "exact", json!({"status": "waiting_for_user"}), None)?;
            }
            Norm::Error { class, message } => {
                state.last_error = Some((class.clone(), message.clone()));
                ev("error", "harness", "exact", json!({"class": class, "message": message}), None)?
            }
            Norm::BackgroundTasks(n) => state.background = n,
            Norm::BackgroundLaunched(id) => {
                state.backgrounded.insert(id);
            }
            Norm::BackgroundNotified(id) => {
                if state.backgrounded.remove(&id) {
                    state.expected_turns += 1;
                }
            }
            Norm::TurnDone { ok, summary } => {
                if run.harness == "claude" {
                    state.expected_turns = state.expected_turns.saturating_sub(1);
                    let interrupted = store.run_process(&run.id)?.map(|(dir, _, _)| Path::new(&dir).join("interrupt.requested").exists()).unwrap_or(false);
                    if !interrupted && (state.background > 0 || state.expected_turns > 0) {
                        // Claude reports an interim result while background subagents run, and
                        // continues with another turn for each finished one (even one that
                        // finished before this result). The session must stay open so those
                        // turns' permission requests can be answered.
                        let why = if state.background > 0 { format!("{} background task(s) still running", state.background) } else { "Claude continues after a background task finished".to_string() };
                        ev("output", "harness", "exact", json!({"role": "system", "text": format!("interim result; {why}: {}", summary.unwrap_or_default())}), None)?;
                        return Ok(());
                    }
                }
                state.turn_done = Some(ok);
                store.finish_open_turns(&run.id, if ok { "completed" } else { "failed" }, now())?;
                ev("turn_done", "harness", "exact", json!({"ok": ok, "summary": summary}), None)?;
                if run.harness == "claude" || run.harness == "codex-app" {
                    // One turn per process: closing stdin lets the session end cleanly.
                    // Done after the store lock is released (see apply_lines).
                    state.close_stdin = true;
                }
            }
            Norm::Send(text) => state.sends.push(text),
            Norm::TurnId(id) => {
                if let Some((dir, _, _)) = store.run_process(&run.id)? {
                    let _ = std::fs::write(Path::new(&dir).join("turn.id"), &id);
                }
            }
            Norm::RpcResult { id, result, error } => {
                let meta: Value = store.conn.query_row("SELECT launch FROM runs WHERE id=?1", [&run.id], |r| r.get::<_, Option<String>>(0))?.and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Value::Null);
                let app = &meta["app"];
                if let Some(err) = error {
                    if id == "ovs-auto-quota" {
                        ev("auto_account_unknown", "harness", "exact", json!({"reason":"account quota metadata unavailable"}), None)?;
                        state.sends.push(format!("{}\n", codex_child_next_request(app)));
                        return Ok(());
                    }
                    if id == "ovs-auto-tools" {
                        let msg = "required tool verification unavailable";
                        state.last_error = Some(("capability".into(), msg.into()));
                        ev("error", "daemon", "exact", json!({"class":"capability","message":msg}), None)?;
                        state.turn_done = Some(false);
                        state.close_stdin = true;
                        return Ok(());
                    }
                    let msg = err["message"].as_str().map(str::to_string).unwrap_or_else(|| err.to_string());
                    state.last_error = Some((classify(&msg), msg.clone()));
                    ev("error", "harness", "exact", json!({"class": classify(&msg), "message": msg, "request": id}), None)?;
                    state.turn_done = Some(false);
                    state.close_stdin = true;
                    return Ok(());
                }
                match id.as_str() {
                    "ovs-init" => {
                        let msg = if run.relation_source.as_deref() == Some("managed-delegation") && run.harness == "codex-app" {
                            json!({"id":"ovs-account","method":"account/read","params":{"refreshToken":false}})
                        } else { codex_thread_request(app) };
                        state.sends.push(format!("{msg}\n"));
                    }
                    "ovs-account" => {
                        if result["requiresOpenaiAuth"] != true || result["account"]["type"] != "chatgpt" {
                            ev("error", "harness", "exact", json!({"class":"authentication","message":"managed Codex delegation requires ChatGPT account login"}), None)?;
                            state.turn_done = Some(false);
                            state.close_stdin = true;
                            return Ok(());
                        }
                        state.sends.push(format!("{}\n", json!({"id":"ovs-auto-quota","method":"account/rateLimits/read","params":{}})));
                    }
                    "ovs-auto-quota" => {
                        let recorded = (|| -> Result<bool> {
                            let fingerprint = crate::auto_quota::account_fingerprint(&result)?;
                            let profile_id = run.profile_id.as_deref().ok_or_else(|| anyhow!("run account profile unavailable"))?;
                            store.record_auto_account_identity(profile_id, &fingerprint)?;
                            let generation = store.auto_account_generation(profile_id)?.ok_or_else(|| anyhow!("account generation unavailable"))?;
                            store.record_auto_run_account(&run.id, profile_id, generation)?;
                            if app["auto_selected"] == true {
                                if app["expected_account_generation"].as_i64() != Some(generation) {
                                    return Ok(false);
                                }
                                if let Some(model) = run.model.as_deref() {
                                    let quota = crate::auto_quota::parse_codex_rate_limits(&result, profile_id, now()).ok();
                                    if matches!(quota.as_ref().map(|snapshot| snapshot.state_for(model, now())),
                                        Some(crate::auto_quota::QuotaState::Exhausted)) {
                                        return Ok(false);
                                    }
                                }
                            }
                            Ok(true)
                        })();
                        match recorded {
                            Ok(false) => {
                                let msg = "automatic child account changed or allowance is exhausted";
                                state.last_error = Some(("quota_or_account".into(), msg.into()));
                                ev("error", "daemon", "exact", json!({"class":"quota_or_account","message":msg}), None)?;
                                state.turn_done = Some(false);
                                state.close_stdin = true;
                                return Ok(());
                            }
                            Err(_) if app["auto_selected"] == true => {
                                let msg = "automatic child account evidence unavailable";
                                state.last_error = Some(("account".into(), msg.into()));
                                ev("error", "daemon", "exact", json!({"class":"account","message":msg}), None)?;
                                state.turn_done = Some(false);
                                state.close_stdin = true;
                                return Ok(());
                            }
                            Err(_) => {
                                self.learning_paused.store(true, std::sync::atomic::Ordering::Relaxed);
                                ev("auto_account_unknown", "daemon", "exact", json!({"reason":"run account evidence unavailable"}), None)?;
                            }
                            Ok(true) => {}
                        }
                        state.sends.push(format!("{}\n", codex_child_next_request(app)));
                    }
                    "ovs-auto-tools" => {
                        let expected = app["required_tools"].as_array().cloned().unwrap_or_default();
                        let observed = crate::auto_route::parse_codex_tools(&result, now());
                        let allowed = observed.ok().is_some_and(|catalog| expected.iter().all(|tool| tool.as_str()
                            .is_some_and(|name| catalog.tools.contains(name))));
                        if !allowed {
                            let msg = "required tool unavailable in managed child";
                            state.last_error = Some(("capability".into(), msg.into()));
                            ev("error", "daemon", "exact", json!({"class":"capability","message":msg}), None)?;
                            state.turn_done = Some(false);
                            state.close_stdin = true;
                            return Ok(());
                        }
                        state.sends.push(format!("{}\n", codex_thread_request(app)));
                    }
                    "ovs-thread" => {
                        let thread = result["thread"]["id"].as_str().unwrap_or_default().to_string();
                        if !thread.is_empty() && store.run(&run.id)?.and_then(|r| r.native_id).is_none() {
                            store.set_run_native(&run.id, &thread)?;
                            ev("session", "harness", "exact", json!({"native_id": thread}), None)?;
                        }
                        let mut params = json!({"threadId": thread, "input": [{"type": "text", "text": app["prompt"], "text_elements": []}]});
                        if let Some(model) = app["model"].as_str() {
                            params["model"] = json!(model);
                        }
                        if let Some(effort) = app["effort"].as_str() {
                            params["effort"] = json!(effort);
                        }
                        let turn = json!({"id": "ovs-turn", "method": "turn/start", "params": params});
                        state.sends.push(format!("{turn}\n"));
                    }
                    _ => {}
                }
            }
            Norm::Ignored => {}
            Norm::Unparsed(text) => ev("raw_unparsed", "harness", "unknown", json!({"text": text, "parser_version": adapters::PARSER_VERSION}), None)?,
        }
        Ok(())
    }

    fn opencode_db(&self, run: &Run) -> Option<PathBuf> {
        let env = match &run.profile_id {
            Some(id) => Self::profile_env(&self.profile(id).ok()?),
            None => BTreeMap::new(),
        };
        let data = env.get("XDG_DATA_HOME").map(PathBuf::from).or_else(|| std::env::var_os("XDG_DATA_HOME").map(PathBuf::from)).unwrap_or_else(|| PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".local/share"));
        Some(data.join("opencode/opencode.db")).filter(|p| p.exists())
    }

    fn poll_opencode_store(self: &Arc<Self>, run: &Run, state: &mut TailState) -> Result<()> {
        let Some(root) = self.run(&run.id)?.native_id else { return Ok(()) };
        let Some(db) = self.opencode_db(run) else { return Ok(()) };
        let norms = match adapters::opencode_store_children(&db, &root) {
            Ok(n) => n,
            Err(_) => return Ok(()), // store busy or schema changed: keep stream-derived children only
        };
        let mut fresh = Vec::new();
        for norm in norms {
            if let Norm::Child { native_id, status, text, .. } = &norm {
                let key = format!("{status:?}|{}", text.as_deref().map(|t| fingerprint(t)).unwrap_or_default());
                if state.store_seen.get(native_id) == Some(&key) {
                    continue;
                }
                state.store_seen.insert(native_id.clone(), key);
            }
            fresh.push(norm);
        }
        if fresh.is_empty() {
            return Ok(());
        }
        let mut emitted = Vec::new();
        {
            let store = self.store.lock().unwrap();
            let tx = store.conn.unchecked_transaction()?;
            for norm in fresh {
                self.apply_norm(&store, run, norm, state, &mut emitted)?;
            }
            tx.commit()?;
        }
        for e in emitted {
            let _ = self.events.send(e);
        }
        Ok(())
    }

    fn finalize(&self, run: &Run, dir: &Path, exit: &ExitInfo, state: &TailState) -> Result<()> {
        let interrupted = dir.join("interrupt.requested").exists();
        let auto_budget = dir.join("auto-budget.requested").exists();
        let durable_turn_done = if state.turn_done.is_none() && run.harness != "generic" {
            self.store.lock().unwrap().last_turn_completion(&run.id)?
        } else { None };
        let (status, reason) = if let Some(err) = &exit.spawn_error {
            ("failed", format!("could not start harness: {err}"))
        } else if auto_budget {
            ("failed", format!("automatic execution budget elapsed; existing child stopped (exit {})", describe_exit(exit)))
        } else if interrupted {
            ("interrupted", format!("interrupted by user (exit {})", describe_exit(exit)))
        } else if dir.join("auto-account-timeout").exists() {
            ("failed", "managed Codex account metadata handshake timed out".to_string())
        } else if let Some(sig) = exit.signal {
            ("failed", format!("killed by signal {sig} (not requested by Overseer)"))
        } else if exit.code == Some(0) {
            match (run.harness.as_str(), state.turn_done.or(durable_turn_done)) {
                ("generic", _) => ("completed", "exit 0".to_string()),
                (_, Some(true)) => ("completed", "turn completed; exit 0".to_string()),
                (_, Some(false)) => ("failed", format!("turn reported failure{}", error_suffix(state))),
                (_, None) => ("unknown", "process exited 0 without a turn-completion event".to_string()),
            }
        } else {
            ("failed", format!("exit {}{}", describe_exit(exit), error_suffix(state)))
        };
        self.mark_ended(run, status, &reason)
    }

    fn mark_ended(&self, run: &Run, status: &str, reason: &str) -> Result<()> {
        let mut emitted = Vec::new();
        {
            let store = self.store.lock().unwrap();
            store.conn.execute_batch("SAVEPOINT settle_run")?;
            let settled = (|| -> Result<()> {
                let ended = now();
                store.update_run_status(&run.id, status, Some(reason), Some(ended))?;
                store.set_run_attention(&run.id, None)?;
                let turn_status = if status == "completed" { "completed" } else { status };
                store.finish_open_turns(&run.id, turn_status, ended)?;
                emitted.push(store.insert_event(ended, Some(&run.task_id), Some(&run.id), "status", "daemon", "exact", &json!({"status": status, "reason": reason}))?);
                if status == "completed" && run.relation_source.as_deref() == Some("managed-delegation") {
                    if let Some(notice) = store.publish_managed_result_notice(run, ended)? {
                        emitted.push(notice);
                    }
                }
                // Children whose end was never reported are unknown, not completed.
                let mut stack = vec![run.id.clone()];
                while let Some(parent) = stack.pop() {
                    for child in store.children(&parent)? {
                        // A managed child has its own supervisor and isolated workspace.
                        // The parent's process exit is not evidence that it stopped.
                        if child.relation_source.as_deref() == Some("managed-delegation") {
                            continue;
                        }
                        stack.push(child.id.clone());
                        if ACTIVE.contains(&child.status.as_str()) {
                            let why = "parent process ended before the child's final status was reported";
                            store.update_run_status(&child.id, "unknown", Some(why), Some(ended))?;
                            emitted.push(store.insert_event(ended, Some(&run.task_id), Some(&child.id), "status", "daemon", "inferred", &json!({"status": "unknown", "reason": why}))?);
                        }
                    }
                }
                if let Some(ws) = store.workspace(&run.workspace_id)? {
                    if ws.owner_run_id.as_deref() == Some(&run.id) {
                        store.set_workspace_owner(&ws.id, None)?;
                    }
                }
                Ok(())
            })();
            match settled {
                Ok(()) => store.conn.execute_batch("RELEASE settle_run")?,
                Err(error) => {
                    let _ = store.conn.execute_batch("ROLLBACK TO settle_run; RELEASE settle_run");
                    return Err(error);
                }
            }
        }
        for e in emitted {
            let _ = self.events.send(e);
        }
        Ok(())
    }

    /// Called once at startup: reattach to surviving supervisors, finalize exited
    /// ones, and report lost sessions. Never relaunches work.
    pub fn reconcile(self: &Arc<Self>) -> Result<Value> {
        let runs = self.store.lock().unwrap().runs()?;
        let mut report = Vec::new();
        for run in runs.iter().filter(|r| (r.parent_run_id.is_none() || r.relation_source.as_deref() == Some("managed-delegation")) && (ACTIVE.contains(&r.status.as_str()) || r.status == "disconnected")) {
            let process = self.store.lock().unwrap().run_process(&run.id)?;
            let Some((dir, _, _)) = process else {
                if ACTIVE.contains(&run.status.as_str()) {
                    self.mark_ended(run, "failed", "daemon stopped before the run was launched")?;
                    report.push(json!({"run": run.id, "result": "never launched"}));
                }
                continue;
            };
            let dir = PathBuf::from(dir);
            if dir.join("exit.json").exists() {
                self.spawn_tail(&run.id);
                report.push(json!({"run": run.id, "result": "exited while daemon was down; replaying output"}));
            } else if self.supervisor_alive(&dir) {
                if run.status == "disconnected" {
                    self.store.lock().unwrap().update_run_status(&run.id, "running", None, None)?;
                }
                self.emit(Some(&run.task_id), Some(&run.id), "reattached", "daemon", "exact", json!({"note": "daemon restarted; supervisor still running"}))?;
                self.spawn_tail(&run.id);
                if run.relation_source.as_deref() == Some("managed-delegation") && run.harness == "codex-app" && run.native_id.is_none() {
                    self.watch_managed_codex_handshake(run.id.clone(), run.process_generation, dir.clone());
                }
                report.push(json!({"run": run.id, "result": "reattached"}));
            } else if run.status != "disconnected" {
                let child_alive = std::fs::read(dir.join("shim.json")).ok().and_then(|b| serde_json::from_slice::<ShimInfo>(&b).ok()).map(|i| pid_alive(i.child_pid)).unwrap_or(false);
                let reason = if child_alive { "supervisor lost; harness process still exists but its output is no longer observable" } else { "supervisor and harness are gone without an exit record (lost session)" };
                self.mark_ended(run, "disconnected", reason)?;
                report.push(json!({"run": run.id, "result": reason}));
            }
        }
        self.emit(None, None, "daemon_started", "daemon", "exact", json!({"reconcile": report, "pid": std::process::id()}))?;
        Ok(json!(report))
    }

    // ------------------------------------------------------------------ comparisons

    fn root_run(&self, run: &Run) -> Result<Run> {
        let mut current = run.clone();
        while let Some(parent) = current.parent_run_id.clone() {
            current = self.run(&parent)?;
        }
        Ok(current)
    }

    pub fn comparisons(&self, run_id: &str, branch: Option<&str>) -> Result<Value> {
        let run = self.run(run_id)?;
        let root = self.root_run(&run)?;
        let task = self.task(&run.task_id)?;
        let ws = self.workspace(&run.workspace_id)?;
        let path = Path::new(&ws.path);
        let head = git::head(path);
        let turns = self.store.lock().unwrap().turns(&root.id)?;
        let mut options = Vec::new();
        let snap_info = |id: &str| -> Option<Snapshot> { self.store.lock().unwrap().snapshot(id).ok().flatten() };
        match turns.last().and_then(|t| t.snapshot_id.as_deref().and_then(snap_info).map(|s| (t.clone(), s))) {
            Some((turn, snap)) => options.push(json!({
                "mode": "latest_run", "label": "Latest run", "base": snap.commit_sha, "available": true, "default": true,
                "detail": format!("run-start snapshot {} (turn {} of {}, {}), captured including dirty and untracked files", snap.id, turn.n, root.id, turn.started_ms),
                "provenance": "recorded", "snapshot": snap,
                "inherited": run.parent_run_id.is_some(),
            })),
            None => options.push(json!({"mode": "latest_run", "label": "Latest run", "available": false, "default": true, "detail": "no run-start snapshot recorded"})),
        }
        for turn in turns.iter().rev().skip(1) {
            if let Some(snap) = turn.snapshot_id.as_deref().and_then(snap_info) {
                options.push(json!({"mode": format!("turn:{}", turn.n), "label": format!("Since turn {}", turn.n), "base": snap.commit_sha, "available": true,
                    "detail": format!("earlier run-start snapshot {} (turn {})", snap.id, turn.n), "provenance": "recorded"}));
            }
        }
        match task.start_snapshot.as_deref().and_then(snap_info) {
            Some(snap) => options.push(json!({"mode": "task_start", "label": "Since task start", "base": snap.commit_sha, "available": true,
                "detail": format!("task-start snapshot {} (HEAD {} plus dirty contents at creation)", snap.id, snap.head.clone().unwrap_or_else(|| "none".into())), "provenance": "recorded"})),
            None => options.push(json!({"mode": "task_start", "label": "Since task start", "available": false, "detail": "task-start snapshot missing"})),
        }
        match &task.fork_commit {
            Some(fork) if git::rev_parse(path, fork).is_some() => {
                let recorded = task.fork_provenance.as_deref().map(|p| p.starts_with("recorded")).unwrap_or(false);
                options.push(json!({"mode": "fork", "label": if recorded { "Original fork" } else { "Original fork (detected candidate)" }, "base": fork, "available": true,
                    "detail": task.fork_provenance, "provenance": if recorded { "recorded" } else { "detected" }}))
            }
            _ => options.push(json!({"mode": "fork", "label": "Original fork", "available": false, "detail": task.fork_provenance.clone().unwrap_or_else(|| "unknown: no fork commit was recorded".into())})),
        }
        let target = branch.map(str::to_string).or(task.target_ref.clone()).or_else(|| git::default_branch(path));
        match (&target, &head) {
            (Some(t), Some(h)) => match git::rev_parse(path, t) {
                Some(tip) => {
                    match git::merge_base(path, &tip, h) {
                        Some(mb) => options.push(json!({"mode": "branch_merge_base", "branch": t, "label": format!("Merge-base with {t} (PR-style)"), "base": mb, "available": true,
                            "detail": format!("merge-base({t}@{}, HEAD@{}) = {}", &tip[..10], &h[..10], mb), "provenance": "computed now"})),
                        None => options.push(json!({"mode": "branch_merge_base", "branch": t, "label": format!("Merge-base with {t}"), "available": false, "detail": format!("{t} shares no history with HEAD")})),
                    }
                    options.push(json!({"mode": "branch_tip", "branch": t, "label": format!("Tip of {t} (direct)"), "base": tip, "available": true, "detail": format!("{t} at {tip}"), "provenance": "computed now"}));
                }
                None => options.push(json!({"mode": "branch_merge_base", "branch": t, "label": format!("Merge-base with {t}"), "available": false, "detail": format!("branch {t} not found")})),
            },
            (None, _) => options.push(json!({"mode": "branch_merge_base", "label": "Target branch", "available": false, "detail": "no target branch: none configured and no default branch detected"})),
            (_, None) => options.push(json!({"mode": "branch_merge_base", "label": "Target branch", "available": false, "detail": "workspace has no HEAD commit"})),
        }
        Ok(json!({"run_id": run_id, "workspace": ws, "head": head, "branch": git::head_branch(path), "options": options, "branches": git::branches(path)}))
    }

    pub fn workspace_diff(&self, workspace_id: &str, base: &str) -> Result<Value> {
        self.workspace_diff_opts(workspace_id, base, true)
    }

    pub fn workspace_diff_opts(&self, workspace_id: &str, base: &str, with_status: bool) -> Result<Value> {
        let ws = self.workspace(workspace_id)?;
        let path = Path::new(&ws.path);
        if git::rev_parse(path, base).is_none() {
            bail!("comparison base {base} is not available in this repository");
        }
        let trees = git::capture_trees(path, &paths::data_dir().join("tmp"))?;
        let changes = git::diff_trees(path, base, &trees.worktree_tree)?;
        let status = if with_status { serde_json::to_value(git::status(path)?)? } else { Value::Null };
        Ok(json!({"workspace_id": ws.id, "root": ws.path, "base": base, "current_tree": trees.worktree_tree, "index_tree": trees.index_tree, "head": trees.head, "changes": changes, "status": status}))
    }

    pub fn cleanup_plan(&self, workspace_id: &str) -> Result<Value> {
        let ws = self.workspace(workspace_id)?;
        let runs: Vec<Run> = self.store.lock().unwrap().runs()?.into_iter().filter(|r| r.workspace_id == ws.id && ACTIVE.contains(&r.status.as_str())).collect();
        let status = if Path::new(&ws.path).exists() { Some(git::status(Path::new(&ws.path))?) } else { None };
        let removable = ws.kind == "worktree" && runs.is_empty() && ws.removed_ms.is_none();
        let reason = if ws.kind != "worktree" {
            "the current checkout is never removed by Overseer".to_string()
        } else if !runs.is_empty() {
            format!("{} active run(s) still use this workspace", runs.len())
        } else if ws.removed_ms.is_some() {
            "already removed".to_string()
        } else {
            "removable".to_string()
        };
        Ok(json!({"workspace": ws, "active_runs": runs.iter().map(|r| json!({"id": r.id, "title": r.title, "status": r.status})).collect::<Vec<_>>(),
            "dirty": status, "removable": removable, "reason": reason}))
    }

    pub fn cleanup(&self, workspace_id: &str, discard_dirty: bool) -> Result<Value> {
        let plan = self.cleanup_plan(workspace_id)?;
        if plan["removable"] != true {
            bail!("refusing cleanup: {}", plan["reason"].as_str().unwrap_or("not removable"));
        }
        let ws = self.workspace(workspace_id)?;
        let dirty = plan["dirty"].as_object().map(|d| ["staged", "unstaged", "untracked", "conflicted"].iter().any(|k| d.get(*k).and_then(|v| v.as_array()).map(|a| !a.is_empty()).unwrap_or(false))).unwrap_or(false);
        if dirty && !discard_dirty {
            bail!("workspace has uncommitted work; review it and confirm discarding explicitly");
        }
        let repo = Path::new(&ws.repo_root);
        if dirty {
            git::git(repo, &["worktree", "remove", "--force", &ws.path])?;
        } else {
            git::worktree_remove(repo, Path::new(&ws.path))?;
        }
        self.store.lock().unwrap().mark_workspace_removed(&ws.id, now())?;
        self.emit(None, None, "workspace_removed", "user", "exact", json!({"workspace_id": ws.id, "path": ws.path, "branch_kept": ws.branch, "discarded_dirty": dirty}))?;
        Ok(json!({"ok": true, "branch_kept": ws.branch}))
    }

    pub fn state(&self) -> Result<Value> {
        let store = self.store.lock().unwrap();
        let runs = store.runs()?;
        let mut turns = serde_json::Map::new();
        for r in runs.iter().filter(|r| r.parent_run_id.is_none() || r.relation_source.as_deref() == Some("managed-delegation")) {
            turns.insert(r.id.clone(), serde_json::to_value(store.turns(&r.id)?)?);
        }
        Ok(json!({"cursor": store.max_seq()?, "tasks": store.tasks()?, "runs": runs, "workspaces": store.workspaces()?, "profiles": store.profiles()?, "turns": turns,
            "daemon": {"pid": std::process::id(), "started_ms": self.started_ms, "version": env!("CARGO_PKG_VERSION"), "parser_version": adapters::PARSER_VERSION}}))
    }

    pub fn raw_output(&self, run_id: &str, max_bytes: usize) -> Result<Value> {
        let process = self.store.lock().unwrap().run_process(run_id)?;
        let Some((dir, _, _)) = process else { return Ok(json!({"lines": [], "truncated": false})) };
        let dir = PathBuf::from(dir);
        let mut segments: Vec<u64> = (0..10_000).filter(|n| shim::segment_path(&dir, *n).exists()).collect();
        let dropped = segments.first().copied().unwrap_or(0) > 0;
        segments.reverse();
        let mut lines: Vec<Value> = Vec::new();
        let mut total = 0usize;
        let mut truncated = dropped;
        'outer: for n in segments {
            let text = std::fs::read_to_string(shim::segment_path(&dir, n)).unwrap_or_default();
            for line in text.lines().rev() {
                total += line.len();
                if total > max_bytes {
                    truncated = true;
                    break 'outer;
                }
                if let Ok(mut rec) = serde_json::from_str::<Value>(line) {
                    if let Some(d) = rec["d"].as_str() {
                        rec["d"] = json!(redact(d));
                    }
                    lines.push(rec);
                }
            }
        }
        lines.reverse();
        Ok(json!({"lines": lines, "truncated": truncated, "note": if truncated { "older raw output is not shown (retention/size bound)" } else { "" }}))
    }
}

struct TailState {
    session: Option<String>,
    turn_done: Option<bool>,
    last_error: Option<(String, String)>,
    since_prune: usize,
    store_polled: std::time::Instant,
    store_seen: std::collections::HashMap<String, String>,
    close_stdin: bool,
    sends: Vec<String>,
    background: usize,
    /// Claude turns still expected from this process: the user's turn plus one continuation per
    /// reported backgrounded task. The session closes only when all have produced a result.
    expected_turns: usize,
    backgrounded: std::collections::HashSet<String>,
}

impl Default for TailState {
    fn default() -> Self {
        Self { session: None, turn_done: None, last_error: None, since_prune: 0, store_polled: std::time::Instant::now(), store_seen: Default::default(), close_stdin: false, sends: Vec::new(), background: 0, expected_turns: 1, backgrounded: Default::default() }
    }
}

/// Is `candidate` an ancestor of (or equal to) `run`?
fn is_ancestor_run(store: &Store, candidate: &str, run: &str) -> Result<bool> {
    let mut current = Some(run.to_string());
    let mut seen = HashSet::new();
    while let Some(id) = current {
        if id == candidate {
            return Ok(true);
        }
        if !seen.insert(id.clone()) {
            return Ok(true); // existing cycle: refuse
        }
        current = store.run(&id)?.and_then(|r| r.parent_run_id);
    }
    Ok(false)
}

/// Find a descendant of `root` with the given native id (breadth-first, cycle-safe).
fn find_in_tree(store: &Store, root: &str, native: &str) -> Result<Option<Run>> {
    let mut queue = std::collections::VecDeque::from([root.to_string()]);
    let mut seen = HashSet::new();
    while let Some(id) = queue.pop_front() {
        if !seen.insert(id.clone()) {
            continue;
        }
        for child in store.children(&id)? {
            // Harness-native IDs are scoped to one supervisor. An independent
            // managed child may report the same native ID in another process.
            if child.relation_source.as_deref() == Some("managed-delegation") {
                continue;
            }
            if child.native_id.as_deref() == Some(native) {
                return Ok(Some(child));
            }
            queue.push_back(child.id.clone());
        }
    }
    Ok(None)
}

/// Account profiles belong to the harness family (codex-app shares Codex logins).
fn profile_harness(harness: &str) -> &str {
    if harness == "codex-app" { "codex" } else { harness }
}

fn classify(msg: &str) -> String {
    adapters::classify_error(msg).to_string()
}

fn describe_exit(exit: &ExitInfo) -> String {
    match (exit.code, exit.signal) {
        (Some(c), _) => format!("code {c}"),
        (None, Some(s)) => format!("signal {s}"),
        _ => "unknown".into(),
    }
}

fn error_suffix(state: &TailState) -> String {
    state.last_error.as_ref().map(|(c, m)| format!("; last error [{c}]: {}", m.chars().take(200).collect::<String>())).unwrap_or_default()
}

fn redact_value(v: Value) -> Value {
    match v {
        Value::String(s) => Value::String(redact(&s)),
        Value::Array(a) => Value::Array(a.into_iter().map(redact_value).collect()),
        Value::Object(o) => Value::Object(o.into_iter().map(|(k, v)| {
            let lower = k.to_ascii_lowercase();
            let secret = ["token", "access_token", "refresh_token", "id_token", "oauth_token", "api_key", "apikey", "authorization", "password", "secret", "client_secret", "cookie"];
            if secret.contains(&lower.as_str()) {
                (k, Value::String("[redacted]".into()))
            } else {
                (k, redact_value(v))
            }
        }).collect()),
        other => other,
    }
}

pub fn fingerprint(value: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(value.as_bytes());
    digest.iter().take(8).map(|b| format!("{b:02x}")).collect()
}

fn strip_ansi(s: &str) -> String {
    let re = regex::Regex::new(r"\x1b\[[0-9;]*[A-Za-z]").unwrap();
    re.replace_all(s, "").to_string()
}

fn run_with_env(program: &Path, args: &[&str], env: &BTreeMap<String, String>) -> Result<(i32, String)> {
    let mut base = adapters::base_env(&program.display().to_string());
    base.extend(env.clone());
    let out = std::process::Command::new(program).args(args).current_dir(adapters::neutral_dir()).env_clear().envs(&base).stdin(std::process::Stdio::null()).output()?;
    let mut text = String::from_utf8_lossy(&out.stdout).to_string();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    Ok((out.status.code().unwrap_or(-1), text))
}

/// Non-secret identity facts from a Codex auth.json: plan type and a one-way
/// fingerprint of the account/user ids. Tokens are never returned or stored.
pub fn codex_identity(auth: &Path) -> Option<Value> {
    use base64::Engine;
    let data: Value = serde_json::from_slice(&std::fs::read(auth).ok()?).ok()?;
    let token = data["tokens"]["id_token"].as_str()?;
    let payload = token.split('.').nth(1)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(payload.trim_end_matches('=')).ok()?;
    let claims: Value = serde_json::from_slice(&bytes).ok()?;
    let auth_claims = &claims["https://api.openai.com/auth"];
    let account = auth_claims["chatgpt_account_id"].as_str().unwrap_or_default();
    let user = auth_claims["chatgpt_user_id"].as_str().or(claims["sub"].as_str()).unwrap_or_default();
    Some(json!({
        "account_fingerprint": fingerprint(account),
        "user_fingerprint": fingerprint(user),
        "plan": auth_claims["chatgpt_plan_type"].clone(),
        "auth_mode": data["auth_mode"].clone(),
        "has_api_key": data["OPENAI_API_KEY"].as_str().map(|k| !k.is_empty()).unwrap_or(false),
    }))
}
