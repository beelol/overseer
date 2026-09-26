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

pub fn now() -> i64 {
    shim::now_ms() as i64
}

fn short_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()[..12].to_string()
}

/// Per-turn choices (AC-60): model, reasoning effort, permission mode and images.
#[derive(Default, Clone, Debug)]
pub struct TurnOpts {
    pub model: Option<String>,
    pub effort: Option<String>,
    pub mode: Option<String>,
    /// (media type, bytes)
    pub images: Vec<(String, Vec<u8>)>,
}

impl TurnOpts {
    pub fn from_params(p: &Value) -> Result<Self> {
        let text = |k: &str| p[k].as_str().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
        let mut images = Vec::new();
        if let Some(list) = p["images"].as_array() {
            if list.len() > 4 {
                bail!("attach at most 4 images per message");
            }
            for img in list {
                use base64::Engine;
                let mime = img["mime"].as_str().unwrap_or_default().to_string();
                if !["image/png", "image/jpeg", "image/gif", "image/webp"].contains(&mime.as_str()) {
                    bail!("images must be PNG, JPEG, GIF or WebP");
                }
                let bytes = base64::engine::general_purpose::STANDARD.decode(img["data"].as_str().unwrap_or_default()).map_err(|_| anyhow!("image data is not base64"))?;
                if bytes.len() > 5 * 1024 * 1024 {
                    bail!("images must be 5 MB or smaller");
                }
                images.push((mime, bytes));
            }
        }
        Ok(Self { model: text("model"), effort: text("effort"), mode: text("permission_mode"), images })
    }
}

pub struct Daemon {
    pub store: Mutex<Store>,
    pub events: broadcast::Sender<Event>,
    tails: Mutex<HashSet<String>>,
    pub(crate) swarm_launch_lock: Mutex<()>,
    exe: PathBuf,
    pub started_ms: i64,
    /// Connected VS Code windows (connections that said hello as `client: "vscode"`).
    pub ui_clients: std::sync::atomic::AtomicUsize,
    /// Bumped on every UI connect/disconnect so a pending background notice can tell a reload
    /// (reconnect within the grace period) from VS Code really closing.
    pub ui_epoch: std::sync::atomic::AtomicU64,
    /// When VS Code windows went from none to some, and the runs the last background notice named:
    /// a brief reconnect after a notice (a probe, a crash-restart) does not repeat it.
    pub ui_session: Mutex<(Option<std::time::Instant>, Option<Vec<String>>)>,
}

pub(crate) fn pid_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    let rc = unsafe { libc::kill(pid as i32, 0) };
    rc == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

fn control_socket_path(run: &str, generation: i64) -> PathBuf {
    paths::short_socket(&format!("c-{}-{generation}.sock", &run[..run.len().min(14)]))
}

/// A daemon-issued identity for a supervised, fixture-only Swarm worker.
/// It is passed through the private launch file, never through the task prompt
/// or user-visible launch metadata.
pub(crate) struct SwarmWorkerIdentity {
    pub run_id: String,
    pub job_id: String,
    pub attempt_id: String,
    pub token: String,
    pub revision: i64,
}

impl Daemon {
    pub fn open() -> Result<Arc<Self>> {
        paths::ensure_private_dir(&paths::data_dir())?;
        paths::ensure_private_dir(&paths::runtime_dir())?;
        paths::ensure_private_dir(&paths::runs_dir())?;
        let store = Store::open(&paths::db_path())?;
        let (tx, _) = broadcast::channel(4096);
        let exe = std::env::current_exe()?;
        let daemon = Arc::new(Self { store: Mutex::new(store), events: tx, tails: Mutex::new(HashSet::new()), swarm_launch_lock: Mutex::new(()), exe, started_ms: now(),
            ui_clients: std::sync::atomic::AtomicUsize::new(0), ui_epoch: std::sync::atomic::AtomicU64::new(0), ui_session: Mutex::new((None, None)) });
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
        self.create_task_internal(p, None)
    }

    pub(crate) fn create_task_for_swarm(self: &Arc<Self>, p: &Value, identity: &SwarmWorkerIdentity) -> Result<Value> {
        self.create_task_internal(p, Some(identity))
    }

    fn create_task_internal(self: &Arc<Self>, p: &Value, swarm_identity: Option<&SwarmWorkerIdentity>) -> Result<Value> {
        let repo_in = p["repo"].as_str().ok_or_else(|| anyhow!("repo is required"))?;
        let harness = p["harness"].as_str().unwrap_or("codex");
        if !["codex", "codex-app", "claude", "opencode", "generic"].contains(&harness) {
            bail!("unknown harness {harness}");
        }
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
            archived_ms: None,
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
            store.insert_task_and_run(&task,&run,swarm_identity.map(|identity|identity.attempt_id.as_str()))?;
        }
        let generic = json!({"program": program, "args": p["args"].clone(), "approval": p["approval_policy"].as_str().unwrap_or("on-request"), "extra_args": p["extra_args"].clone()});
        let opts = TurnOpts { model: None, ..TurnOpts::from_params(p)? };
        {
            let store = self.store.lock().unwrap();
            store.conn.execute("UPDATE runs SET launch=?2 WHERE id=?1", rusqlite::params![run.id, generic.to_string()])?;
        }
        self.emit(Some(&task.id), Some(&run.id), "task_created", "daemon", "exact", json!({"task": task, "workspace": ws, "run": run}))?;
        let started = self.start_turn_internal(&run.id, &prompt, false, &opts, swarm_identity);
        if let Err(e) = started {
            let current = self.run(&run.id)?;
            // If no supervisor was recorded, a rejected initial turn must not
            // consume an active slot forever. A process with a recorded run
            // directory is left to normal exit/recovery reconciliation.
            if current.status == "queued"
                && self.store.lock().unwrap().run_process(&run.id)?.is_none()
            {
                self.mark_ended(&current, "failed", &format!("launch failed: {}", redact(&e.to_string())))?;
            }
            let run = self.run(&run.id)?;
            let task = self.task(&task.id)?;
            return Ok(json!({"task": task, "run": run, "workspace": ws, "launch_error": e.to_string()}));
        }
        let run = self.run(&run.id)?;
        let task = self.task(&task.id)?;
        Ok(json!({"task": task, "run": run, "workspace": ws}))
    }

    /// Start a work turn: fresh run-start snapshot, then launch (or stdin for live generic processes).
    pub fn start_turn(self: &Arc<Self>, run_id: &str, prompt: &str, follow_up: bool, opts: &TurnOpts) -> Result<Turn> {
        self.start_turn_internal(run_id, prompt, follow_up, opts, None)
    }

    fn start_turn_internal(self: &Arc<Self>, run_id: &str, prompt: &str, follow_up: bool, opts: &TurnOpts, swarm_identity: Option<&SwarmWorkerIdentity>) -> Result<Turn> {
        let mut run = self.run(run_id)?;
        if run.parent_run_id.is_some() {
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
        let launch_meta: Value = {
            let store = self.store.lock().unwrap();
            store.conn.query_row("SELECT launch FROM runs WHERE id=?1", [run_id], |r| r.get::<_, Option<String>>(0))?.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(Value::Null)
        };
        let mut generic_meta = launch_meta.get("generic").cloned().unwrap_or(launch_meta.clone());
        // Turn options: this turn's choices, else the run's last ones (a model change sticks).
        let effort = opts.effort.clone().or_else(|| generic_meta["opts"]["effort"].as_str().map(str::to_string));
        let mode = opts.mode.clone().or_else(|| generic_meta["opts"]["mode"].as_str().map(str::to_string));
        adapters::check_turn_options(&run.harness, effort.as_deref(), mode.as_deref(), opts.images.len())?;
        if let Some(m) = &opts.model {
            if run.model.as_deref() != Some(m.as_str()) {
                self.store.lock().unwrap().conn.execute("UPDATE runs SET model=?2 WHERE id=?1", rusqlite::params![run_id, m])?;
                run.model = Some(m.clone());
            }
        }
        if generic_meta.is_null() {
            generic_meta = json!({});
        }
        generic_meta["opts"] = json!({"effort": effort, "mode": mode});
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
        let profile_env = match &run.profile_id {
            Some(id) => Self::profile_env(&self.profile(id)?),
            None => BTreeMap::new(),
        };
        // Attachments are private files in the run folder, named by content.
        let mut images = Vec::new();
        if !opts.images.is_empty() {
            let dir = paths::runs_dir().join(run_id).join("attachments");
            paths::ensure_private_dir(&dir)?;
            for (mime, bytes) in &opts.images {
                use sha2::Digest;
                let ext = mime.trim_start_matches("image/").replace("jpeg", "jpg");
                let path = dir.join(format!("{:x}.{ext}", sha2::Sha256::digest(bytes)));
                std::fs::write(&path, bytes)?;
                {
                    use std::os::unix::fs::PermissionsExt;
                    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
                }
                images.push((mime.clone(), path));
            }
        }
        let args: Option<Vec<String>> = generic_meta["args"].as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect());
        let extra_args: Vec<String> = generic_meta["extra_args"].as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()).unwrap_or_default();
        let resume = if follow_up { run.native_id.clone() } else { None };
        if follow_up && resume.is_none() && run.harness != "generic" {
            bail!("no native session id was reported for this run, so it cannot be resumed");
        }
        let mut launch = adapters::launch(
            &run.harness,
            &LaunchReq {
                cwd: Path::new(&ws.path),
                prompt,
                model: run.model.as_deref(),
                profile_env,
                resume_session: resume.as_deref(),
                program_override: generic_meta["program"].as_str(),
                args_override: args.as_deref(),
                extra_args: &extra_args,
                effort: effort.as_deref(),
                permission_mode: mode.as_deref(),
                images: &images,
            },
        )?;
        if let Some(identity) = swarm_identity {
            if run.harness != "generic" || std::env::var("OVERSEER_SWARM_FIXTURE_API").as_deref() != Ok("1") {
                bail!("scripted Swarm worker identity requires fixture-only generic harness");
            }
            for (key, value) in [
                ("OVERSEER_SWARM_RUN_ID", identity.run_id.clone()),
                ("OVERSEER_SWARM_JOB_ID", identity.job_id.clone()),
                ("OVERSEER_SWARM_ATTEMPT_ID", identity.attempt_id.clone()),
                ("OVERSEER_SWARM_TOKEN", identity.token.clone()),
                ("OVERSEER_SWARM_REVISION", identity.revision.to_string()),
                ("OVERSEER_HOME", paths::data_dir().display().to_string()),
                ("OVERSEER_SOCKET", paths::socket_path().display().to_string()),
                ("OVERSEER_BIN", self.exe.display().to_string()),
            ] {
                launch.env.insert(key.to_string(), value);
            }
        }
        self.store.lock().unwrap().set_workspace_owner(&ws.id, Some(run_id))?;
        let app = json!({"prompt": prompt, "cwd": ws.path, "model": run.model, "resume": resume, "approval": generic_meta["approval"].as_str().unwrap_or("on-request")});
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
        Ok(())
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
        let run = self.run(run_id)?;
        if run.parent_run_id.is_some() {
            bail!("native children are interrupted through their parent run");
        }
        if !ACTIVE.contains(&run.status.as_str()) {
            bail!("run is not active (status {})", run.status);
        }
        let (dir, _, _) = self.store.lock().unwrap().run_process(run_id)?.ok_or_else(|| anyhow!("run has no process"))?;
        std::fs::write(Path::new(&dir).join("interrupt.requested"), now().to_string())?;
        self.emit(Some(&run.task_id), Some(run_id), "interrupt_requested", "user", "exact", json!({}))?;
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
        Ok(json!({"ok": true}))
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
        loop {
            let run = self.run(run_id)?;
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
            Norm::Usage(u) => ev("usage", "harness", "exact", u, None)?,
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
                let interrupted = store.run_process(&run.id)?.map(|(dir, _, _)| Path::new(&dir).join("interrupt.requested").exists()).unwrap_or(false);
                if run.harness == "claude" {
                    state.expected_turns = state.expected_turns.saturating_sub(1);
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
                // A turn that ends because the user interrupted it (Claude reports it as an error
                // result) is interrupted, not failed.
                store.finish_open_turns(&run.id, if ok { "completed" } else if interrupted { "interrupted" } else { "failed" }, now())?;
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
                if let Some(err) = error {
                    let msg = err["message"].as_str().map(str::to_string).unwrap_or_else(|| err.to_string());
                    state.last_error = Some((classify(&msg), msg.clone()));
                    ev("error", "harness", "exact", json!({"class": classify(&msg), "message": msg, "request": id}), None)?;
                    state.turn_done = Some(false);
                    state.close_stdin = true;
                    return Ok(());
                }
                let meta: Value = store.conn.query_row("SELECT launch FROM runs WHERE id=?1", [&run.id], |r| r.get::<_, Option<String>>(0))?.and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Value::Null);
                let app = &meta["app"];
                match id.as_str() {
                    "ovs-init" => {
                        let msg = match app["resume"].as_str() {
                            Some(thread) => json!({"id": "ovs-thread", "method": "thread/resume", "params": {"threadId": thread, "cwd": app["cwd"], "approvalPolicy": app["approval"], "sandbox": "workspace-write"}}),
                            None => {
                                let mut params = json!({"cwd": app["cwd"], "approvalPolicy": app["approval"], "sandbox": "workspace-write"});
                                if let Some(m) = app["model"].as_str() {
                                    params["model"] = json!(m);
                                }
                                json!({"id": "ovs-thread", "method": "thread/start", "params": params})
                            }
                        };
                        state.sends.push(format!("{msg}\n"));
                    }
                    "ovs-thread" => {
                        let thread = result["thread"]["id"].as_str().unwrap_or_default().to_string();
                        if !thread.is_empty() && store.run(&run.id)?.and_then(|r| r.native_id).is_none() {
                            store.set_run_native(&run.id, &thread)?;
                            ev("session", "harness", "exact", json!({"native_id": thread}), None)?;
                        }
                        let turn = json!({"id": "ovs-turn", "method": "turn/start", "params": {"threadId": thread, "input": [{"type": "text", "text": app["prompt"], "text_elements": []}]}});
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
        let (status, reason) = if let Some(err) = &exit.spawn_error {
            ("failed", format!("could not start harness: {err}"))
        } else if interrupted {
            ("interrupted", format!("interrupted by user (exit {})", describe_exit(exit)))
        } else if let Some(sig) = exit.signal {
            ("failed", format!("killed by signal {sig} (not requested by Overseer)"))
        } else if exit.code == Some(0) {
            match (run.harness.as_str(), state.turn_done) {
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
            let ended = now();
            store.update_run_status(&run.id, status, Some(reason), Some(ended))?;
            store.set_run_attention(&run.id, None)?;
            let turn_status = if status == "completed" { "completed" } else { status };
            store.finish_open_turns(&run.id, turn_status, ended)?;
            emitted.push(store.insert_event(ended, Some(&run.task_id), Some(&run.id), "status", "daemon", "exact", &json!({"status": status, "reason": reason}))?);
            // Children whose end was never reported are unknown, not completed.
            let mut stack = vec![run.id.clone()];
            while let Some(parent) = stack.pop() {
                for child in store.children(&parent)? {
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
        for run in runs.iter().filter(|r| r.parent_run_id.is_none() && (ACTIVE.contains(&r.status.as_str()) || r.status == "disconnected")) {
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

    /// Archives or restores a task (AC-63): hidden from the default list, never deleted.
    pub fn task_archive(&self, task_id: &str, archived: bool) -> Result<Value> {
        let when = if archived { Some(now()) } else { None };
        if !self.store.lock().unwrap().set_task_archived(task_id, when)? {
            bail!("unknown task {task_id}");
        }
        self.emit(Some(task_id), None, "task_archived", "user", "exact", json!({"archived": archived}))?;
        Ok(json!({"task_id": task_id, "archived_ms": when}))
    }

    /// Task ids whose task, runs, accounts or conversation match `query` (AC-63).
    pub fn search(&self, query: &str, limit: i64) -> Result<Value> {
        let q = query.trim();
        if q.is_empty() {
            return Ok(json!({"task_ids": []}));
        }
        let started = std::time::Instant::now();
        let ids = self.store.lock().unwrap().search(q, limit.clamp(1, 1000))?;
        Ok(json!({"task_ids": ids, "ms": started.elapsed().as_millis() as u64}))
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
        for r in runs.iter().filter(|r| r.parent_run_id.is_none()) {
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
