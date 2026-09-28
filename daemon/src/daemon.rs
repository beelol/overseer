//! Daemon state and behavior: tasks, workspaces, snapshots, runs and their processes.

use crate::adapters::{self, InterruptPlan, LaunchReq, Norm};
use crate::git;
use crate::paths;
use crate::redact::redact;
use crate::shim::{self, ExitInfo, LaunchFile, ShimInfo};
use crate::store::{self, DirectorOwnerLink, Event, Profile, Run, Snapshot, Store, Task, Turn, Workspace};
use anyhow::{anyhow, bail, Context, Result};
use rusqlite::OptionalExtension;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashSet};
use std::io::{Read, Seek, SeekFrom};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;

/// `waiting_for_connection` and `waiting_for_memory` are Continuity's: the run has no process,
/// keeps its message and its worktree, and is started again by Overseer (daemon/src/handoff.rs).
pub const ACTIVE: &[&str] = &["queued", "starting", "running", "waiting_for_user", "waiting_for_connection", "waiting_for_memory"];
const RAW_SEGMENTS_KEPT: u64 = 4;
pub const DEFAULT_AUTO_EXECUTION_BUDGET_MS: u64 = 300_000;
pub const DEFAULT_AUTO_PARENT_BUDGET_MS: u64 = 1_800_000;
const MANUAL_POOL_CONFLICT: &str = "automatic account pool is in use; retry the manual turn after it settles";

#[derive(Debug)]
pub struct AgentLimitError {
    pub active: i64,
    pub limit: i64,
    pub running_agents: Vec<Value>,
}

impl std::fmt::Display for AgentLimitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "agent limit reached: {} of {} slots active", self.active, self.limit)
    }
}

impl std::error::Error for AgentLimitError {}

/// Admission found that the selected Auto unit's qualified upper draw no
/// longer fits its account's windows beside the live commitments. The unit
/// pauses: a known draw that cannot fit never falls back to launching on an
/// unknown-draw claim.
#[derive(Debug)]
pub struct AutoDrawExceedsAllowance;

impl std::fmt::Display for AutoDrawExceedsAllowance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "estimated draw exceeds the account's remaining allowance")
    }
}

impl std::error::Error for AutoDrawExceedsAllowance {}

/// The selected route's endpoint is past a failure's cooldown and another
/// unit is already its one shared recovery check.
#[derive(Debug)]
pub struct AutoEndpointRecoveryInFlight;

impl std::fmt::Display for AutoEndpointRecoveryInFlight {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "another unit is already checking this endpoint's recovery")
    }
}

impl std::error::Error for AutoEndpointRecoveryInFlight {}

/// A durable app-slot hold (`app_slot_holds`) for a start whose run row does
/// not exist yet. The hold is one occupant in the one app-slot count
/// (`account_booking::app_slots_in_use`); it is deleted in the commit that
/// inserts the run, or when the start stops first.
struct AgentSlotReservation<'a> {
    daemon: &'a Daemon,
    hold: Option<String>,
}

impl AgentSlotReservation<'_> {
    /// The run is inserted: its row now holds the slot.
    fn consumed(&mut self) {
        self.hold = None;
    }

    fn release_after_start(&mut self) {
        if let Some(hold) = self.hold.take() {
            if let Err(error) = self.daemon.store.lock().unwrap().release_app_slot_hold(&hold) {
                crate::log(&format!("app slot hold {hold} was not released: {error}"));
            }
        }
    }
}

impl Drop for AgentSlotReservation<'_> {
    fn drop(&mut self) {
        self.release_after_start();
    }
}

pub fn now() -> i64 {
    shim::now_ms() as i64
}

fn short_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()[..12].to_string()
}

fn codex_sandbox_name(mode: crate::auto_select::Sandbox) -> Result<&'static str> {
    match mode {
        crate::auto_select::Sandbox::ReadOnly => Ok("read-only"),
        crate::auto_select::Sandbox::WorkspaceWrite => Ok("workspace-write"),
        crate::auto_select::Sandbox::FullAccess => bail!("automatic full-access sandbox is unsupported"),
    }
}

fn codex_thread_request(app: &Value) -> Result<Value> {
    let sandbox = match app.get("sandbox") {
        None => "workspace-write", // existing manual runs predate the saved field
        Some(Value::String(value)) if value == "read-only" => "read-only",
        Some(Value::String(value)) if value == "workspace-write" => "workspace-write",
        _ => bail!("unsupported Codex sandbox for a saved run"),
    };
    match app["resume"].as_str() {
        Some(thread) => Ok(json!({"id": "ovs-thread", "method": "thread/resume", "params": {
            "threadId": thread, "cwd": app["cwd"], "approvalPolicy": app["approval"], "sandbox": sandbox}})),
        None => {
            let mut params = json!({"cwd": app["cwd"], "approvalPolicy": app["approval"], "sandbox": sandbox});
            if let Some(model) = app["model"].as_str() {
                params["model"] = json!(model);
            }
            Ok(json!({"id": "ovs-thread", "method": "thread/start", "params": params}))
        }
    }
}

fn codex_child_next_request(app: &Value) -> Result<Value> {
    if app["required_tools"].as_array().is_some_and(|tools| !tools.is_empty()) {
        Ok(json!({"id":"ovs-auto-tools","method":"mcpServerStatus/list",
            "params":{"detail":"toolsAndAuthOnly"}}))
    } else {
        codex_thread_request(app)
    }
}

pub(crate) fn valid_required_tool(name: &str) -> bool {
    let Some((server, tool)) = name.split_once('/') else { return false };
    [server, tool].iter().all(|part| !part.is_empty() && part.len() <= 120
        && part.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.')))
}

/// Per-turn choices (AC-60): model, reasoning effort, permission mode and images.
#[derive(Default, Clone, Debug)]
pub struct TurnOpts {
    pub model: Option<String>,
    pub effort: Option<String>,
    pub mode: Option<String>,
    /// (media type, bytes)
    pub images: Vec<(String, Vec<u8>)>,
    /// Continuity: the turn that is sent again after a wait, in place of a new one.
    pub retry_of: Option<String>,
    /// Continuity: the first turn of a successor run, which may continue an earlier session.
    pub handoff: bool,
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
        Ok(Self { model: text("model"), effort: text("effort"), mode: text("permission_mode"), images, retry_of: None, handoff: false })
    }
}

/// A shared launch booking for an ordinary start. A fixture caller may
/// supply its upper draw, account generation and cited observation (behind
/// `OVERSEER_SHARED_BOOKING_FIXTURE_API`). Any caller may instead ask for
/// the qualified draw (`"draw":"qualified"`): the daemon cites the account's
/// latest observation and current identity, and the booking computes the
/// draw from isolated runs of the same harness, model and effort, refusing
/// `upper_draw_unknown` until enough exist.
struct SharedStart {
    id: String,
    request_hash: String,
    route_id: String,
    profile_id: String,
    quota_profile_id: String,
    account_generation: i64,
    quota_event_seq: i64,
    /// None: the qualified draw for `bucket`.
    upper_draw_milli: Option<Vec<i64>>,
    bucket: crate::upper_draw::DrawBucket,
    consume_agent_slot: bool,
    launch_hash: String,
    /// Booked by the daemon for an ordinary start that asked for none.
    automatic: bool,
}

impl SharedStart {
    fn from_params(store: &Store, booking: &Value, p: &Value, harness: &str, profile_id: &str) -> Result<Self> {
        let text = |key: &str| booking[key].as_str().filter(|value| !value.is_empty()).map(str::to_string);
        let id = text("work_unit_id").ok_or_else(|| anyhow!("shared booking needs a work_unit_id"))?;
        let qualified = match booking.get("draw") {
            None | Some(Value::Null) => false,
            Some(Value::String(draw)) if draw == "qualified" => true,
            Some(_) => bail!("shared booking draw must be \"qualified\""),
        };
        let quota_profile_id = text("quota_profile_id").unwrap_or_else(|| profile_id.to_string());
        let (upper_draw_milli, account_generation, quota_event_seq) = if qualified {
            if booking.get("upper_draw_milli").is_some() {
                bail!("a qualified shared booking cannot also carry an upper draw");
            }
            // Cite what the daemon has recorded; the booking revalidates both
            // in its transaction. Missing evidence is refused there.
            (None, store.auto_account_generation(&quota_profile_id)?.unwrap_or(0).max(1),
                store.latest_auto_quota(&quota_profile_id)?.map_or(1, |q| q.event_seq))
        } else {
            if std::env::var("OVERSEER_SHARED_BOOKING_FIXTURE_API").as_deref() != Ok("1") {
                bail!("shared launch booking is not available to this caller");
            }
            let draws = booking["upper_draw_milli"].as_array()
                .filter(|draws| !draws.is_empty() && draws.len() <= 32)
                .and_then(|draws| draws.iter().map(Value::as_i64).collect::<Option<Vec<_>>>())
                .ok_or_else(|| anyhow!("shared booking needs an upper draw for every window"))?;
            (Some(draws),
                booking["account_generation"].as_i64()
                    .ok_or_else(|| anyhow!("shared booking needs the account generation"))?,
                booking["quota_event_seq"].as_i64()
                    .ok_or_else(|| anyhow!("shared booking needs the cited quota observation"))?)
        };
        use sha2::Digest;
        Ok(Self {
            request_hash: text("request_hash").unwrap_or_else(|| id.clone()),
            route_id: text("route_id").unwrap_or_else(||
                format!("{harness}/{}", p["model"].as_str().unwrap_or("default"))),
            profile_id: profile_id.to_string(),
            quota_profile_id,
            account_generation,
            quota_event_seq,
            upper_draw_milli,
            bucket: crate::upper_draw::DrawBucket::agent(harness, p["model"].as_str(), p["effort"].as_str()),
            consume_agent_slot: booking["consume_agent_slot"].as_bool().unwrap_or(true),
            // Every launch input is bound to the intent: a replay with any
            // changed field is refused rather than treated as the same start.
            launch_hash: format!("{:x}", sha2::Sha256::digest(p.to_string().as_bytes())),
            id,
            automatic: false,
        })
    }

    /// An ordinary start that asked for no booking books its account on the
    /// qualified draw when one exists (handover step 4). Its app slot stays
    /// the start's own durable hold, so the booking takes none; any refusal
    /// falls back to the unbooked start, which keeps today's behavior.
    fn automatic(store: &Store, p: &Value, harness: &str, profile_id: &str, task_class: &str) -> Result<Self> {
        use sha2::Digest;
        let id = format!("ordinary/{}", uuid::Uuid::new_v4().simple());
        Ok(Self {
            request_hash: id.clone(),
            route_id: format!("{harness}/{}", p["model"].as_str().unwrap_or("default")),
            profile_id: profile_id.to_string(),
            quota_profile_id: profile_id.to_string(),
            account_generation: store.auto_account_generation(profile_id)?.unwrap_or(0).max(1),
            quota_event_seq: store.latest_auto_quota(profile_id)?.map_or(1, |q| q.event_seq),
            upper_draw_milli: None,
            bucket: crate::upper_draw::DrawBucket {
                task_class: task_class.into(),
                ..crate::upper_draw::DrawBucket::agent(harness, p["model"].as_str(), p["effort"].as_str())
            },
            consume_agent_slot: false,
            launch_hash: format!("{:x}", sha2::Sha256::digest(p.to_string().as_bytes())),
            id,
            automatic: true,
        })
    }
}

#[derive(PartialEq)]
enum SharedLaunchStage { Booked, Claimed, Bound }

/// Held by the request that booked a shared launch until its run is bound.
/// Stopping while only booked is a confirmed pre-effect failure and releases
/// everything. Stopping after the effects claim, before any run is bound,
/// means no model process can have started: the slot and the account
/// commitment are released, the writer stays held for reconciliation.
struct UnboundSharedLaunch {
    daemon: Arc<Daemon>,
    id: String,
    stage: SharedLaunchStage,
    /// A Swarm worker's booking belongs to its durable admitted attempt, not
    /// to this request: stopping before the effects claim keeps it booked.
    keep_booked: bool,
}

impl UnboundSharedLaunch {
    /// Called immediately before the first external (Git) effect.
    fn claim_effects(&mut self) -> Result<()> {
        if !self.daemon.store.lock().unwrap().claim_shared_launch_effects(&self.id)? {
            self.stage = SharedLaunchStage::Bound;
            bail!("shared launch {} was claimed by another request", self.id);
        }
        self.stage = SharedLaunchStage::Claimed;
        shared_launch_test_crash("after_claim");
        Ok(())
    }
}

impl Drop for UnboundSharedLaunch {
    fn drop(&mut self) {
        let store = self.daemon.store.lock().unwrap();
        let released = match self.stage {
            SharedLaunchStage::Booked if self.keep_booked => return,
            SharedLaunchStage::Booked => store.release_shared_booking_pre_effect(&self.id),
            SharedLaunchStage::Claimed => store.release_unbound_shared_launch(&self.id),
            SharedLaunchStage::Bound => return,
        };
        if let Err(error) = released {
            crate::log(&format!("shared launch {} kept its holds: {error}", self.id));
        }
    }
}

/// Test-only crash points between booking, claiming and binding.
pub(crate) fn shared_launch_test_crash(point: &str) {
    if std::env::var("OVERSEER_TEST_SHARED_LAUNCH_CRASH").as_deref() == Ok(point) {
        unsafe { libc::kill(libc::getpid(), libc::SIGKILL); }
        // Delivery to this process is asynchronous: make no further progress.
        loop { std::thread::park(); }
    }
}

pub struct Daemon {
    pub store: Mutex<Store>,
    profile_gates: Mutex<BTreeMap<String, Arc<Mutex<()>>>>,
    workspace_gates: Mutex<BTreeMap<String, Arc<Mutex<()>>>>,
    work_unit_gates: Mutex<BTreeMap<String, std::sync::Weak<Mutex<()>>>>,
    pub events: broadcast::Sender<Event>,
    tails: Mutex<HashSet<String>>,
    pub(crate) swarm_launch_lock: Mutex<()>,
    pub(crate) swarm_integration_lock: Mutex<()>,
    /// Fail closed for Swarm launches after SQLite reports exhausted or unwritable storage.
    pub(crate) swarm_storage_blocked: std::sync::atomic::AtomicBool,
    pub(crate) exe: PathBuf,
    pub started_ms: i64,
    /// Storage unavailable at startup; this cannot recover without reopening the store.
    pub learning_paused: std::sync::atomic::AtomicBool,
    pub learning_usage_paused: std::sync::atomic::AtomicBool,
    pub learning_work_paused: std::sync::atomic::AtomicBool,
    pub learning_thread_paused: std::sync::atomic::AtomicBool,
    pub learning_account_paused: std::sync::atomic::AtomicBool,
    pub learning_maintenance_paused: std::sync::atomic::AtomicBool,
    /// Connected VS Code windows (connections that said hello as `client: "vscode"`).
    pub ui_clients: std::sync::atomic::AtomicUsize,
    /// Bumped on every UI connect/disconnect so a pending background notice can tell a reload
    /// (reconnect within the grace period) from VS Code really closing.
    pub ui_epoch: std::sync::atomic::AtomicU64,
    /// When VS Code windows went from none to some, and the runs the last background notice named:
    /// a brief reconnect after a notice (a probe, a crash-restart) does not repeat it.
    pub ui_session: Mutex<(Option<std::time::Instant>, Option<Vec<String>>)>,
    /// What Overseer coordinates with no model: pending conflict scans and their caches.
    pub coord: crate::overseer::conflicts::Coordination,
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

pub(crate) struct SwarmDirectorIdentity {
    pub run_id: String,
    pub generation: i64,
    pub token: String,
}

enum SwarmLaunchIdentity<'a> {
    Worker(&'a SwarmWorkerIdentity),
    Director(&'a SwarmDirectorIdentity),
}

impl Daemon {
    fn reserve_agent_slot(&self) -> Result<AgentSlotReservation<'_>> {
        let store = self.store.lock().unwrap();
        match store.hold_app_slot("start")? {
            Ok(hold) => Ok(AgentSlotReservation { daemon: self, hold: Some(hold) }),
            Err((active, limit)) =>
                Err(AgentLimitError { active, limit, running_agents: store.active_agents()? }.into()),
        }
    }

    pub fn open() -> Result<Arc<Self>> {
        paths::ensure_private_dir(&paths::data_dir())?;
        paths::ensure_private_dir(&paths::runtime_dir())?;
        paths::ensure_private_dir(&paths::runs_dir())?;
        let store = Store::open(&paths::db_path())?;
        let learning_paused = !store.learning_persistent;
        let (tx, _) = broadcast::channel(4096);
        let exe = std::env::current_exe()?;
        let daemon = Arc::new(Self { store: Mutex::new(store), profile_gates: Mutex::new(BTreeMap::new()), workspace_gates: Mutex::new(BTreeMap::new()), work_unit_gates: Mutex::new(BTreeMap::new()), events: tx, tails: Mutex::new(HashSet::new()), swarm_launch_lock: Mutex::new(()), swarm_integration_lock: Mutex::new(()), swarm_storage_blocked: std::sync::atomic::AtomicBool::new(false), exe, started_ms: now(), learning_paused: std::sync::atomic::AtomicBool::new(learning_paused),
            learning_usage_paused: std::sync::atomic::AtomicBool::new(false), learning_work_paused: std::sync::atomic::AtomicBool::new(false),
            learning_thread_paused: std::sync::atomic::AtomicBool::new(false), learning_account_paused: std::sync::atomic::AtomicBool::new(false),
            learning_maintenance_paused: std::sync::atomic::AtomicBool::new(false),
            ui_clients: std::sync::atomic::AtomicUsize::new(0), ui_epoch: std::sync::atomic::AtomicU64::new(0), ui_session: Mutex::new((None, None)),
            coord: crate::overseer::conflicts::Coordination::default() });
        daemon.ensure_system_profiles()?;
        {
            let mut store = daemon.store.lock().unwrap();
            // A SQLite page ceiling is connection-local. This fixture reapplies
            // it after restart so a still-full disk can be replayed reliably.
            if std::env::var("OVERSEER_SWARM_FIXTURE_API").as_deref() == Ok("1")
                && std::env::var("OVERSEER_TEST_SWARM_STORAGE_PAGE_LIMIT").as_deref() == Ok("current") {
                let pages: i64 = store.conn.pragma_query_value(None, "page_count", |r| r.get(0))?;
                store.conn.pragma_update(None, "max_page_count", pages)?;
            }
            if let Err(error) = store.probe_swarm_write_capacity() {
                daemon.swarm_storage_blocked.store(true, std::sync::atomic::Ordering::SeqCst);
                crate::log(&format!("swarm storage write probe failed at startup: {error}"));
            }
        }
        // Test settings (AC-201): the suites run with briefings and the channel, and check-ins,
        // off and on. OVERSEER_CHANNEL_DEFAULT is auto, on or off; OVERSEER_CHECK_INS is off,
        // done or every:N. Only set when the variable is present; the owner's settings otherwise.
        {
            let store = daemon.store.lock().unwrap();
            if let Ok(v) = std::env::var("OVERSEER_CHANNEL_DEFAULT") {
                if ["auto", "on", "off"].contains(&v.as_str()) {
                    store.conn.execute("INSERT OR REPLACE INTO meta(key, value) VALUES('overseer.channel', ?1)", [&v])?;
                }
            }
            if let Ok(v) = std::env::var("OVERSEER_CHECK_INS") {
                if crate::overseer::checkin::Cadence::parse(&v).is_ok() {
                    store.conn.execute("INSERT OR REPLACE INTO meta(key, value) VALUES('overseer.check_ins', ?1)", [&v])?;
                }
            }
        }
        Ok(daemon)
    }

    pub fn learning_is_paused(&self) -> bool {
        use std::sync::atomic::Ordering::Relaxed;
        self.learning_paused.load(Relaxed) || self.learning_usage_paused.load(Relaxed)
            || self.learning_work_paused.load(Relaxed) || self.learning_thread_paused.load(Relaxed)
            || self.learning_account_paused.load(Relaxed) || self.learning_maintenance_paused.load(Relaxed)
    }

    pub fn emit(&self, task: Option<&str>, run: Option<&str>, kind: &str, source: &str, confidence: &str, payload: Value) -> Result<Event> {
        let payload = redact_value(payload);
        let event = self.store.lock().unwrap().insert_event(now(), task, run, kind, source, confidence, &payload)?;
        let _ = self.events.send(event.clone());
        Ok(event)
    }

    pub fn record_auto_selected_decision(&self, work_unit_id: &str, parent: &Run,
        requirements_hash: &str, route_id: &str, pool_id: &str,
        account_generation: Option<i64>, execution_budget_ms: u64,
        trace: Value) -> Result<Option<Event>> {
        let store = self.store.lock().unwrap();
        if !store.auto_mode_enabled()? {
            return Err(anyhow!("Auto Mode was disabled before admission"));
        }
        let event = store.insert_auto_selected_decision(work_unit_id,
            parent, requirements_hash, route_id, pool_id, account_generation,
            execution_budget_ms, &redact_value(trace))?;
        drop(store);
        if let Some(event) = &event { let _ = self.events.send(event.clone()); }
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
                    // OpenCode also consults HOME-level configuration paths;
                    // XDG overrides alone do not isolate a managed profile.
                    env.insert("HOME".into(), home.display().to_string());
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
        self.create_task_internal(p, None)
    }

    pub(crate) fn create_task_for_swarm(self: &Arc<Self>, p: &Value, identity: &SwarmWorkerIdentity) -> Result<Value> {
        self.create_task_internal(p, Some(SwarmLaunchIdentity::Worker(identity)))
    }

    pub(crate) fn create_task_for_swarm_director(self: &Arc<Self>, p: &Value, identity: &SwarmDirectorIdentity) -> Result<Value> {
        self.create_task_internal(p, Some(SwarmLaunchIdentity::Director(identity)))
    }

    fn create_task_internal(self: &Arc<Self>, p: &Value, swarm_identity: Option<SwarmLaunchIdentity<'_>>) -> Result<Value> {
        let repo_in = p["repo"].as_str().ok_or_else(|| anyhow!("repo is required"))?;
        let harness = p["harness"].as_str().unwrap_or("codex");
        if !["codex", "codex-app", "claude", "opencode", "opencode-serve", "generic"].contains(&harness) {
            bail!("unknown harness {harness}");
        }
        let effort = match p.get("effort") {
            None | Some(Value::Null) => None,
            Some(Value::String(value)) if !value.is_empty() => Some(value.clone()),
            _ => bail!("effort must be a nonempty string"),
        };
        adapters::validate_effort(harness, effort.as_deref())?;
        let sandbox = match p.get("sandbox") {
            None => "workspace-write",
            Some(value) => {
                if !matches!(harness, "codex" | "codex-app") {
                    bail!("explicit task sandbox is unsupported for this harness");
                }
                codex_sandbox_name(serde_json::from_value(value.clone())?)?
            }
        };
        let prompt = p["prompt"].as_str().unwrap_or_default().to_string();
        if prompt.is_empty() && harness != "generic" {
            bail!("prompt is required");
        }
        // The proposed native path (`swarm.native_director`, default off): a
        // Claude director or worker gets the daemon's Swarm tools over MCP.
        let native_member = swarm_identity.is_some() && harness == "claude"
            && crate::swarm::native::enabled(&self.store.lock().unwrap())?;
        let native_director = native_member && matches!(swarm_identity, Some(SwarmLaunchIdentity::Director(_)));
        let mut slot = match swarm_identity.as_ref() {
            Some(SwarmLaunchIdentity::Worker(_)) => None,
            Some(SwarmLaunchIdentity::Director(identity)) => {
                // A running category already holds its director slot. Planning
                // categories have no reserved slot yet, so their first director
                // launch still competes with ordinary starts.
                let reserved = self.store.lock().unwrap().conn.query_row(
                    "SELECT status IN ('running','paused','stalled','stopping') FROM swarm_runs WHERE id=?1",
                    [&identity.run_id], |row| row.get::<_, bool>(0),
                )?;
                if reserved { None } else { Some(self.reserve_agent_slot()?) }
            }
            // A booked start takes its app slot in the shared booking (Auto's one authority),
            // which refuses a full limit with `global_agent_limit`; it is not counted here too.
            None if p.get("shared_booking").is_some_and(|b| !b.is_null()) => None,
            // Overseer's own coordinating run takes no app slot (Gate S, SWARM-07): a
            // full house never locks the owner out of it. Its turns are still metered.
            None if p["role"] == "overseer" => None,
            None => Some(self.reserve_agent_slot()?),
        };
        let title = p["title"].as_str().map(str::to_string).unwrap_or_else(|| prompt.chars().take(60).collect());
        let mode = p["workspace_mode"].as_str().unwrap_or("worktree");
        let repo = git::toplevel(Path::new(repo_in)).context("repository not found")?;
        let common = git::common_dir(&repo)?;
        let profile = match p["profile_id"].as_str() {
            // Local runs use Overseer's own OpenCode profile, never the user's own configuration; it is made on first use.
            Some(crate::opencode_bridge::LOCAL_PROFILE) | None if harness == "opencode-serve" => Some(crate::opencode_bridge::local_profile(self)?),
            Some(id) => Some(self.profile(id)?),
            None if harness != "generic" => Some(self.profile(&format!("system-{}", profile_harness(harness)))?),
            None => None,
        };
        if let Some(prof) = &profile {
            if prof.harness != profile_harness(harness) {
                bail!("profile {} belongs to {}, not {harness}", prof.name, prof.harness);
            }
        }
        let auto_routing = match p.get("auto_routing") {
            None => false,
            Some(Value::Bool(enabled)) => *enabled,
            _ => bail!("auto_routing must be a boolean"),
        };
        let auto_allowed_profiles = if auto_routing {
            if harness != "codex-app" {
                bail!("run-bound Auto delegation is not yet available for this harness");
            }
            if !self.store.lock().unwrap().auto_mode_enabled()? {
                bail!("Auto Mode is disabled");
            }
            let ids = match p.get("auto_allowed_profiles") {
                None => vec![profile.as_ref().ok_or_else(|| anyhow!("Auto parent has no profile"))?.id.clone()],
                Some(Value::Array(ids)) if !ids.is_empty() && ids.len() <= 8 => {
                    let mut checked = Vec::new();
                    for id in ids {
                        let id = id.as_str().filter(|id| !id.is_empty() && id.len() <= 120)
                            .ok_or_else(|| anyhow!("invalid Auto allowed profile"))?;
                        let candidate = self.profile(id)?;
                        if !matches!(candidate.harness.as_str(), "codex" | "claude" | "opencode")
                            || checked.iter().any(|existing| existing == id) {
                            bail!("invalid or duplicate Auto allowed profile");
                        }
                        checked.push(id.to_string());
                    }
                    checked
                }
                _ => bail!("auto_allowed_profiles must be a bounded nonempty list"),
            };
            Some(ids)
        } else {
            if p.get("auto_allowed_profiles").is_some() {
                bail!("auto_allowed_profiles requires auto_routing");
            }
            None
        };
        let auto_parent_budget_ms = if auto_routing {
            match p.get("auto_parent_budget_ms") {
                None => DEFAULT_AUTO_PARENT_BUDGET_MS,
                Some(value) => value.as_u64().filter(|ms| (1_000..=86_400_000).contains(ms))
                    .ok_or_else(|| anyhow!("auto_parent_budget_ms must be 1000-86400000"))?,
            }
        } else {
            if p.get("auto_parent_budget_ms").is_some() {
                bail!("auto_parent_budget_ms requires auto_routing");
            }
            0
        };
        let mut shared_start = match p.get("shared_booking") {
            None | Some(Value::Null) => None,
            Some(booking) => {
                if auto_routing {
                    bail!("an Auto parent cannot also carry a shared launch booking");
                }
                let profile = profile.as_ref()
                    .ok_or_else(|| anyhow!("a shared launch booking needs an account profile"))?;
                let store = self.store.lock().unwrap();
                Some(SharedStart::from_params(&store, booking, p, harness, &profile.id)?)
            }
        };
        // A booked Swarm worker: its admission booked the account windows and
        // the app slot (`swarm/<attempt>`); this launch claims the booking's
        // effects before the first Git effect and binds the run to it.
        let swarm_booking = match swarm_identity.as_ref() {
            Some(SwarmLaunchIdentity::Worker(identity)) => {
                let id = crate::account_booking::swarm_attempt_booking_id(&identity.attempt_id);
                match self.store.lock().unwrap().shared_launch_intent(&id)? {
                    None => None,
                    Some(intent) if intent.phase == "booked" && intent.effects_claimed_ms.is_none() => Some(id),
                    Some(intent) => bail!("Swarm worker booking {id} is not launchable ({}); it needs reconciliation",
                        intent.outcome.unwrap_or(intent.phase)),
                }
            }
            _ => None,
        };
        let unbooked_profile = match (&profile, &shared_start, &swarm_booking) {
            (Some(profile), None, None) => Some(profile.id.clone()),
            _ => None,
        };
        if let Some(profile_id) = &unbooked_profile {
            // An ordinary agent start (not Swarm, not an Auto parent, not the
            // daemon's own Overseer or watcher run) tries the one booking,
            // which itself counts live known-window bookings on the account.
            // A native director books its account like an ordinary start,
            // with its own class's draw (`swarm/director`).
            let can_book = (swarm_identity.is_none() || native_director) && !auto_routing && harness != "generic"
                && p.get("role").is_none_or(Value::is_null);
            if self.store.lock().unwrap().auto_claim_conflicts(profile_id, "", !can_book)? {
                bail!("{MANUAL_POOL_CONFLICT}");
            }
            if can_book {
                let store = self.store.lock().unwrap();
                let class = if native_director { "swarm/director" } else { crate::upper_draw::AGENT_CLASS };
                shared_start = Some(SharedStart::automatic(&store, p, harness, profile_id, class)?);
            }
        }
        let target_ref = p["target_ref"].as_str().filter(|s| !s.is_empty()).map(str::to_string);
        if let Some(t) = &target_ref {
            if git::rev_parse(&repo, t).is_none() {
                bail!("target ref {t} does not exist");
            }
        }
        let mut unbound_launch = swarm_booking.as_ref().map(|id| UnboundSharedLaunch {
            daemon: self.clone(), id: id.clone(), stage: SharedLaunchStage::Booked, keep_booked: true });
        if let Some(start) = &shared_start {
            let workspace_path = (mode == "current").then(|| repo.display().to_string());
            let account = crate::account_booking::AccountBookingRequest {
                id: &start.id, request_hash: &start.request_hash, caller: "ordinary",
                route_id: &start.route_id, profile_id: &start.profile_id,
                quota_profile_id: &start.quota_profile_id,
                account_generation: start.account_generation,
                quota_event_seq: start.quota_event_seq, now_ms: now(),
                draw: match &start.upper_draw_milli {
                    Some(draws) => crate::account_booking::BookingDraw::Fixture(draws),
                    None => crate::account_booking::BookingDraw::Qualified(&start.bucket),
                },
                allocation_remaining_milli: None,
            };
            let decision = self.store.lock().unwrap().book_shared_launch(
                &crate::account_booking::LaunchBookingRequest {
                    account: &account, workspace_path: workspace_path.as_deref(),
                    consume_agent_slot: start.consume_agent_slot, launch_hash: &start.launch_hash,
                });
            let booked = match decision {
                // Unknown draw, busy or short account, stale reading: the
                // ordinary start proceeds unbooked, exactly as before.
                Ok(crate::account_booking::LaunchBookingDecision::Blocked(_)) | Err(_)
                    if start.automatic => false,
                Err(error) => return Err(error),
                Ok(crate::account_booking::LaunchBookingDecision::Blocked(reason)) =>
                    bail!("shared launch booking blocked: {reason}"),
                Ok(crate::account_booking::LaunchBookingDecision::Replayed(_)) =>
                    return self.shared_start_replay(&start.id),
                Ok(crate::account_booking::LaunchBookingDecision::Booked(_)) => true,
            };
            if booked {
                unbound_launch = Some(UnboundSharedLaunch { daemon: self.clone(), id: start.id.clone(),
                    stage: SharedLaunchStage::Booked, keep_booked: false });
                shared_launch_test_crash("after_book");
            }
        }
        if shared_start.as_ref().is_some_and(|start| start.automatic) && unbound_launch.is_none() {
            // Unbooked, as before: a live booking on the account refuses it.
            shared_start = None;
            if let Some(profile_id) = &unbooked_profile {
                if self.store.lock().unwrap().auto_claim_conflicts_with_run(profile_id, "")? {
                    bail!("{MANUAL_POOL_CONFLICT}");
                }
            }
        }
        let (ws, fork_commit, fork_prov) = match mode {
            "worktree" => {
                let start = target_ref.clone().unwrap_or_else(|| "HEAD".into());
                let start_sha = git::rev_parse(&repo, &start).ok_or_else(|| anyhow!("repository has no commit to branch from"))?;
                let repo_name = repo.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "repo".into());
                let hash = &fingerprint(&common.display().to_string())[..8];
                let parent = paths::worktrees_dir().join(format!("{repo_name}-{hash}"));
                if let Some(launch) = unbound_launch.as_mut() {
                    launch.claim_effects()?;
                }
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
                let own = shared_start.as_ref().map(|start| start.id.as_str());
                if let Some(hold) = self.store.lock().unwrap().shared_writer_hold(&path, own, None)? {
                    bail!("the current checkout is held by shared launch {hold}; refusing a second independent writer");
                }
                if let Some(launch) = unbound_launch.as_mut() {
                    launch.claim_effects()?;
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
            effort,
            workspace_id: ws.id.clone(),
            // A Swarm worker continuing an earlier worker's session (SWARM-35); the launch
            // path has checked that the session is valid and related.
            native_id: match swarm_identity.as_ref() {
                Some(SwarmLaunchIdentity::Worker(_)) => p["resume_native_id"].as_str().filter(|s| !s.is_empty()).map(str::to_string),
                _ => None,
            },
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
        let mut generic = json!({"program": program, "args": p["args"].clone(), "approval": p["approval_policy"].as_str().unwrap_or("on-request"), "sandbox":sandbox, "extra_args": p["extra_args"].clone(),
            "auto_routing":auto_routing,"auto_allowed_profiles":auto_allowed_profiles,
            "auto_parent_budget_ms":auto_parent_budget_ms,
            "swarm_worker": matches!(swarm_identity.as_ref(), Some(SwarmLaunchIdentity::Worker(_))),
            "resume_first_turn": matches!(swarm_identity.as_ref(), Some(SwarmLaunchIdentity::Worker(_)))
                && p["resume_native_id"].as_str().is_some_and(|s| !s.is_empty())});
        // Capture this parent's own route metadata before its app-server turn
        // owns the profile. A second metadata session during that turn is not
        // safe; the child still verifies its own account and tools at launch.
        if auto_routing && profile.as_ref().is_some_and(|profile|
            generic["auto_allowed_profiles"].as_array().is_some_and(|allowed|
                allowed.iter().any(|id| id.as_str() == Some(profile.id.as_str())))) {
            let profile_id = profile.as_ref().unwrap().id.as_str();
            let preflight = (|| -> Result<Value> {
                let models = crate::server::dispatch(self, "auto.models.refresh",
                    &json!({"profile_id":profile_id,"timeout_ms":4000}))?;
                let first_generation = self.store.lock().unwrap().auto_account_generation(profile_id)?;
                let tools = crate::server::dispatch(self, "auto.tools.inspect",
                    &json!({"profile_id":profile_id,"workspace_id":ws.id,"timeout_ms":4000}))?;
                let generation = self.store.lock().unwrap().auto_account_generation(profile_id)?;
                if generation.is_none() || generation != first_generation {
                    bail!("parent Auto account changed during route preflight");
                }
                Ok(json!({"workspace_id":ws.id,"account_generation":generation,
                    "model_catalog":models["catalog"],"tool_catalog":tools["catalog"]}))
            })();
            if let Ok(catalogs) = preflight {
                generic["auto_parent_discovery"] = catalogs;
            }
        }
        {
            // Task and run appear together: a state snapshot never shows a task without its run.
            let director = match swarm_identity.as_ref() {
                Some(SwarmLaunchIdentity::Director(identity)) => Some(DirectorOwnerLink {
                    swarm_run_id: &identity.run_id,
                    generation: identity.generation,
                    token: &identity.token,
                }),
                _ => None,
            };
            let attempt = match swarm_identity.as_ref() {
                Some(SwarmLaunchIdentity::Worker(identity)) => Some(identity.attempt_id.as_str()), _ => None };
            // A booked start (ordinary or Swarm worker) holds its slot in the shared booking,
            // and its run is bound in the same commit, so the booking becomes the run's
            // occupancy. An unbooked start's durable slot hold is deleted in that commit.
            let bind = shared_start.as_ref().map(|start| start.id.as_str())
                .or(swarm_booking.as_deref()).map(|id| (id, now()));
            let hold = slot.as_ref().and_then(|reservation| reservation.hold.clone());
            self.store.lock().unwrap().insert_task_and_run_holding(&task, &run, attempt, director, bind, hold.as_deref())?;
            if let Some(reservation) = slot.as_mut() { reservation.consumed(); }
            // A run of the daemon's own (Overseer, a watcher) carries its role from the start.
            if let Some(role) = p["role"].as_str().filter(|r| ["overseer", "watcher"].contains(r)) {
                self.store.lock().unwrap().conn.execute("INSERT OR REPLACE INTO run_roles(run_id, role) VALUES(?1, ?2)", rusqlite::params![run.id, role])?;
            }
        }
        if let Some(launch) = unbound_launch.as_mut() {
            launch.stage = SharedLaunchStage::Bound;
        }
        let opts = TurnOpts { model: None, ..TurnOpts::from_params(p)? };
        if native_member {
            // The member's token goes only into its private MCP configuration.
            let (token, role) = match swarm_identity.as_ref() {
                Some(SwarmLaunchIdentity::Director(identity)) => (identity.token.as_str(), crate::swarm::native::DIRECTOR_ROLE),
                Some(SwarmLaunchIdentity::Worker(identity)) => (identity.token.as_str(), crate::swarm::native::WORKER_ROLE),
                None => unreachable!(),
            };
            let config = crate::swarm::native::write_config(&self.exe, &run.id, token)?;
            generic["swarm_tools"] = crate::swarm::native::launch_meta(&config, role);
        }
        {
            let store = self.store.lock().unwrap();
            store.conn.execute("UPDATE runs SET launch=?2 WHERE id=?1", rusqlite::params![run.id, generic.to_string()])?;
        }
        self.emit(Some(&task.id), Some(&run.id), "task_created", "daemon", "exact", json!({"task": task, "workspace": ws, "run": run}))?;
        let started = self.start_turn_internal(&run.id, &prompt, false, &opts, swarm_identity.as_ref());
        if let Err(e) = started {
            let current = self.run(&run.id)?;
            let launch_uncertain = {
                let store = self.store.lock().unwrap();
                crate::swarm::mark_uncertain_director_spawn(&store, &run.id)?
                    || store.mark_worker_spawn_uncertain(&run.id)?
            };
            // If no supervisor was recorded, a rejected initial turn must not
            // consume an active slot forever (nor keep a booked start's holds until
            // a restart). A process with a recorded run directory is left to normal
            // exit/recovery reconciliation.
            let settle = current.status == "queued"
                || ((shared_start.is_some() || swarm_booking.is_some()) && ACTIVE.contains(&current.status.as_str()));
            if !launch_uncertain && settle
                && self.store.lock().unwrap().run_process(&run.id)?.is_none()
            {
                self.mark_ended(&current, "failed", &format!("launch failed: {}", redact(&e.to_string())))?;
            }
            let run = self.run(&run.id)?;
            let task = self.task(&task.id)?;
            return Ok(json!({"task": task, "run": run, "workspace": ws,
                "launch_error": e.to_string(),"launch_uncertain":launch_uncertain}));
        }
        let run = self.run(&run.id)?;
        let task = self.task(&task.id)?;
        Ok(json!({"task": task, "run": run, "workspace": ws}))
    }

    /// A repeated or concurrent booked start returns the run its first
    /// request bound, and never claims or attempts the launch again.
    fn shared_start_replay(self: &Arc<Self>, id: &str) -> Result<Value> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            let intent = self.store.lock().unwrap().shared_launch_intent(id)?
                .ok_or_else(|| anyhow!("shared launch {id} is not recorded"))?;
            if let Some(run_id) = &intent.run_id {
                let run = self.run(run_id)?;
                let task = self.task(&run.task_id)?;
                let ws = self.workspace(&run.workspace_id)?;
                return Ok(json!({"task": task, "run": run, "workspace": ws, "replayed": true,
                    "shared_launch": intent}));
            }
            match intent.outcome.as_deref() {
                Some("released_unclaimed") =>
                    bail!("shared launch {id} was released before any effect; book a new attempt"),
                Some("effects_uncertain") =>
                    bail!("shared launch {id} stopped with uncertain effects; it is held for reconciliation and not retried"),
                _ if intent.phase == "released" => bail!("shared launch {id} was released"),
                _ => {}
            }
            if std::time::Instant::now() >= deadline {
                bail!("shared launch {id} is still pending");
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }

    /// Both the supervisor and the harness are confirmed gone, or no
    /// supervisor was ever recorded. A missing or unreadable shim record
    /// is not confirmation.
    fn run_processes_gone(&self, run_id: &str) -> bool {
        let process = self.store.lock().unwrap().run_process(run_id);
        match process {
            Ok(None) => true,
            Ok(Some((dir, _, _))) => std::fs::read(Path::new(&dir).join("shim.json")).ok()
                .and_then(|bytes| serde_json::from_slice::<ShimInfo>(&bytes).ok())
                .is_some_and(|info| !pid_alive(info.shim_pid) && !pid_alive(info.child_pid)),
            Err(_) => false,
        }
    }

    /// Execute a bounded work unit in a separately supervised workspace made
    /// from the parent's captured snapshot. Route selection happens before
    /// this execution boundary; this method never guesses a model or account.
    pub fn delegate_run(self: &Arc<Self>, p: &Value, auto_launch_claimed: bool) -> Result<Value> {
        if auto_launch_claimed && p["auto_selected"] != true {
            bail!("automatic launch requires an internally claimed work unit");
        }
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
        let selected_sandbox = match p.get("sandbox") {
            Some(value) => {
                let mode: crate::auto_select::Sandbox = serde_json::from_value(value.clone())?;
                let name = codex_sandbox_name(mode)?;
                if mode == crate::auto_select::Sandbox::ReadOnly
                    && !matches!(harness, "codex" | "codex-app" | "opencode") {
                    bail!("read-only delegation is unsupported for this harness");
                }
                Some(name)
            }
            None if p["auto_selected"] == true => bail!("automatic delegation requires a sandbox"),
            None => None,
        };
        let profile_id = p["profile_id"].as_str().map(str::to_string)
            .unwrap_or_else(|| format!("system-{}", profile_harness(harness)));
        let request = json!({"parent_run_id":parent_id,"harness":harness,"profile_id":profile_id,
            "model":model,"effort":effort,"prompt":prompt,"title":title,"required_tools":required_tools});
        let mut request = request;
        if let Some(mode) = selected_sandbox { request["sandbox"] = json!(mode); }
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
        let intent = self.store.lock().unwrap().auto_launch_intent(work_unit_id)?;
        if auto_launch_claimed {
            let (intent_parent, intent_hash, intent_route, intent_generation, intent_phase) = intent
                .ok_or_else(|| anyhow!("automatic launch intent is unavailable"))?;
            let route = format!("{profile_id}/{model}/{effort}");
            if p["auto_selected"] != true || intent_parent != parent_id
                || Some(intent_hash.as_str()) != p["requirements_hash"].as_str()
                || intent_route != route || intent_generation != p["expected_account_generation"].as_i64()
                || intent_phase != "preparing" {
                bail!("automatic launch intent does not match the selected route");
            }
        } else if intent.is_some() {
            bail!("work-unit identity is reserved by an automatic launch");
        }
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
        if (parent.parent_run_id.is_some() && parent.relation_source.as_deref() != Some("managed-continuation"))
            || (parent.status != "completed" && !(auto_launch_claimed && parent.status == "running")) {
            bail!("delegation requires a completed parent, or a running parent with an automatic launch claim");
        }
        let parent_ws = self.workspace(&parent.workspace_id)?;
        let active_writer = self.active_writer(&parent_ws.path)?;
        if parent_ws.removed_ms.is_some() || active_writer.as_ref().is_some_and(|writer|
            !(auto_launch_claimed && parent.status == "running" && writer.id == parent.id)) {
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
        let (parent_approval, parent_sandbox) = {
            let store = self.store.lock().unwrap();
            let launch: Option<String> = store.conn.query_row(
                "SELECT launch FROM runs WHERE id=?1", [&parent.id], |row| row.get(0))?;
            let launch: Value = launch.as_deref().and_then(|value| serde_json::from_str(value).ok())
                .unwrap_or(Value::Null);
            let generic = launch.get("generic").unwrap_or(&launch);
            let approval = generic["approval"].as_str()
                .ok_or_else(|| anyhow!("parent approval policy is unavailable"))?.to_string();
            let sandbox = generic["sandbox"].as_str().unwrap_or("workspace-write");
            if !matches!(sandbox, "read-only" | "workspace-write") {
                bail!("parent sandbox policy is unavailable");
            }
            (approval, sandbox.to_string())
        };
        let child_sandbox = selected_sandbox.unwrap_or(&parent_sandbox);
        if parent_sandbox == "read-only" && child_sandbox != "read-only" {
            bail!("delegation cannot widen the parent sandbox");
        }
        if child_sandbox == "read-only"
            && !matches!(harness, "codex" | "codex-app")
            && !(p["auto_selected"] == true && harness == "opencode") {
            bail!("read-only delegation is unsupported for this harness");
        }
        let snapshot = self.take_snapshot(&parent_ws, "managed-delegation")?;
        let repo = Path::new(&parent_ws.repo_root);
        let repo_name = repo.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "repo".into());
        let hash = &fingerprint(&parent_ws.common_dir)[..8];
        let worktrees = paths::worktrees_dir().join(format!("{repo_name}-{hash}"));
        let worktree_name = format!("delegate-{}-{title}", parent.id);
        let (path, branch) = if auto_launch_claimed {
            let (planned_path, planned_branch) = git::plan_worktree_add(repo, &worktrees, &worktree_name)?;
            let path_text = planned_path.to_str().ok_or_else(|| anyhow!("non-UTF-8 planned worktree path"))?;
            self.store.lock().unwrap().journal_auto_launch_worktree(work_unit_id,
                &planned_branch, path_text, &snapshot.id, &snapshot.commit_sha)?;
            git::worktree_add_planned_auto(repo, &planned_path, &planned_branch, &snapshot.commit_sha)?
        } else {
            git::worktree_add(repo, &worktrees, &worktree_name, &snapshot.commit_sha)?
        };
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
                    rusqlite::params![run.id, json!({"approval":parent_approval,"sandbox":child_sandbox,"extra_args":[],"required_tools":required_tools,
                        "auto_selected":p["auto_selected"] == true,"expected_account_generation":p["expected_account_generation"],
                        "execution_budget_ms":request["execution_budget_ms"],
                        "auto_local_endpoint":request["auto_local_endpoint"],
                        "requirements_hash":p["requirements_hash"]}).to_string()])?;
                if auto_launch_claimed {
                    store.mark_auto_child_created(work_unit_id)?;
                }
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
        if let Err(error) = self.emit(Some(&run.task_id), Some(&run.id), "managed_child_created", "daemon", "exact",
            json!({"parent_run_id":parent.id,"run":run,"workspace":ws,"snapshot_id":snapshot.id})) {
            // start_turn has not been called. The child and its Git resource
            // remain inspectable. Settling it also releases the pool claim
            // and keeps this unstarted run out of profile activity checks.
            self.mark_ended(&run, "failed", "delegated launch stopped before model turn")?;
            return Err(error);
        }
        if let Err(error) = self.start_turn(&run.id, prompt, false, &TurnOpts::default()) {
            // A supervisor recorded during start_turn may still be running
            // even though a later database/event write failed. Observe that
            // same supervisor now; only its eventual settlement releases
            // the pool claim. A pre-spawn failure has no process to observe.
            let process = self.store.lock().unwrap().run_process(&run.id)?;
            if let Some((dir, _, _)) = process {
                self.spawn_tail(&run.id);
                let current = self.run(&run.id)?;
                if current.harness == "codex-app" && current.native_id.is_none() {
                    self.watch_codex_account_handshake(run.id.clone(),
                        current.process_generation, PathBuf::from(dir));
                }
            } else {
                self.mark_ended(&run, "failed", &format!("delegated launch failed: {error}"))?;
            }
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

    /// A user-requested, same-harness continuation from a confirmed completed
    /// checkpoint. This is deliberately narrower than automatic outage handoff:
    /// failed/uncertain work and cross-provider context require separate proof.
    pub fn handoff_run(self: &Arc<Self>, p: &Value) -> Result<Value> {
        use sha2::{Digest, Sha256};
        let source_id = p["source_run_id"].as_str().ok_or_else(|| anyhow!("source_run_id is required"))?;
        let initial = self.run(source_id)?;
        let gate = self.workspace_gate(&initial.workspace_id);
        let guard = gate.lock().unwrap();
        let source = self.run(source_id)?;
        if source.parent_run_id.is_some() && source.relation_source.as_deref() != Some("managed-continuation") {
            bail!("only a task coordinator can be continued");
        }
        let harness = p["harness"].as_str().ok_or_else(|| anyhow!("handoff harness is required"))?;
        if harness != source.harness || !["codex", "codex-app", "claude", "opencode"].contains(&harness) {
            bail!("this handoff boundary supports only the source harness");
        }
        let profile_id = p["profile_id"].as_str().unwrap_or(source.profile_id.as_deref()
            .ok_or_else(|| anyhow!("source account profile is unavailable"))?);
        if Some(profile_id) != source.profile_id.as_deref() {
            bail!("this handoff boundary requires the source account profile");
        }
        let profile = self.profile(profile_id)?;
        if profile.harness != profile_harness(harness) {
            bail!("handoff profile belongs to another harness");
        }
        let model = p["model"].as_str().filter(|value| !value.is_empty() && value.len() <= 120
            && value.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'/')))
            .ok_or_else(|| anyhow!("handoff requires a valid model"))?;
        let effort = p["effort"].as_str().ok_or_else(|| anyhow!("handoff effort is required"))?;
        adapters::validate_effort(harness, Some(effort))?;
        let handoff = p["handoff"].as_object().ok_or_else(|| anyhow!("structured handoff is required"))?;
        if handoff.len() != 6 || handoff.keys().any(|key| !matches!(key.as_str(),
            "corrections" | "completed" | "remaining" | "tests" | "limitations" | "unresolved_actions")) {
            bail!("handoff requires corrections, completed, remaining, tests, limitations, and unresolved_actions");
        }
        let mut sections = Vec::new();
        for key in ["corrections", "completed", "remaining", "tests", "limitations", "unresolved_actions"] {
            let entries = handoff[key].as_array().filter(|items| items.len() <= 32)
                .ok_or_else(|| anyhow!("handoff {key} must be a bounded list"))?;
            let mut lines = Vec::new();
            for entry in entries {
                let line = entry.as_str().filter(|line| !line.is_empty() && line.len() <= 1024)
                    .ok_or_else(|| anyhow!("handoff {key} contains an invalid entry"))?;
                lines.push(format!("- {line}"));
            }
            if key == "unresolved_actions" && !lines.is_empty() {
                bail!("unresolved external actions require review before handoff");
            }
            sections.push(format!("{key}:\n{}", if lines.is_empty() { "- none".into() } else { lines.join("\n") }));
        }
        let task = self.task(&source.task_id)?;
        let prompt = format!("Continue the same task in a fresh session.\nOriginal goal:\n{}\n\n{}\n\nUse the existing workspace files; do not assume the previous native session or tool messages are portable.",
            task.prompt, sections.join("\n\n"));
        if prompt.len() > 32_768 { bail!("handoff context is too large for a safe continuation"); }
        let request = json!({"source_run_id":source_id,"harness":harness,"profile_id":profile_id,
            "model":model,"effort":effort,"handoff":p["handoff"]});
        let request_hash = Sha256::digest(serde_json::to_vec(&request)?).iter()
            .map(|byte| format!("{byte:02x}")).collect::<String>();
        let existing = { self.store.lock().unwrap().children(source_id)?.into_iter()
            .find(|child| child.relation_source.as_deref() == Some("managed-continuation")) };
        if let Some(existing) = existing {
            let saved: Option<String> = self.store.lock().unwrap().conn.query_row(
                "SELECT launch FROM runs WHERE id=?1", [&existing.id], |row| row.get(0))?;
            let launch: Value = saved.as_deref().and_then(|text| serde_json::from_str(text).ok()).unwrap_or(Value::Null);
            let generic = launch.get("generic").unwrap_or(&launch);
            if generic["handoff_hash"].as_str() != Some(&request_hash) {
                bail!("source run already has a different continuation");
            }
            let workspace = self.workspace(&existing.workspace_id)?;
            return Ok(json!({"run":existing,"workspace":workspace,"replayed":true,
                "state":if existing.process_generation == 0 { "launch_uncertain" } else { "existing" }}));
        }
        if source.status != "completed" || source.attention.is_some() || source.process_generation == 0 {
            bail!("handoff needs a completed, attention-free source checkpoint");
        }
        let process = self.store.lock().unwrap().run_process(source_id)?
            .ok_or_else(|| anyhow!("source supervisor outcome is unavailable"))?;
        if !Path::new(&process.0).join("exit.json").exists() {
            bail!("source supervisor exit is unconfirmed");
        }
        for child in self.store.lock().unwrap().children(source_id)? {
            if ACTIVE.contains(&child.status.as_str()) || matches!(child.status.as_str(), "unknown" | "disconnected") {
                bail!("source child {} has an unsettled outcome", child.id);
            }
        }
        let turns = self.store.lock().unwrap().turns(source_id)?;
        if turns.is_empty() || turns.iter().any(|turn| turn.ended_ms.is_none() || turn.status != "completed") {
            bail!("source turn outcome is unconfirmed");
        }
        let workspace = self.workspace(&source.workspace_id)?;
        if workspace.removed_ms.is_some() || workspace.owner_run_id.is_some()
            || self.active_writer(&workspace.path)?.is_some() {
            bail!("workspace ownership has not been released");
        }
        let (approval, sandbox) = {
            let saved: Option<String> = self.store.lock().unwrap().conn.query_row(
                "SELECT launch FROM runs WHERE id=?1", [source_id], |row| row.get(0))?;
            let launch: Value = saved.as_deref().and_then(|text| serde_json::from_str(text).ok()).unwrap_or(Value::Null);
            let generic = launch.get("generic").unwrap_or(&launch);
            if generic["extra_args"].as_array().is_some_and(|items| !items.is_empty())
                || generic["required_tools"].as_array().is_some_and(|items| !items.is_empty()) {
                bail!("source launch constraints cannot yet be transferred safely");
            }
            let approval = generic["approval"].as_str()
                .ok_or_else(|| anyhow!("source approval policy is unavailable"))?.to_string();
            let sandbox = generic["sandbox"].as_str().unwrap_or("workspace-write");
            if !matches!(sandbox, "read-only" | "workspace-write") {
                bail!("source sandbox policy is unavailable");
            }
            (approval, sandbox.to_string())
        };
        let snapshot = self.take_snapshot(&workspace, "handoff-checkpoint")?;
        let run = Run {
            id: format!("r-{}", short_id()), task_id: source.task_id.clone(),
            parent_run_id: Some(source.id.clone()), harness: harness.into(),
            harness_version: adapters::resolve_program(harness).and_then(|program| adapters::version_of(&program)),
            profile_id: Some(profile.id.clone()), model: Some(model.into()), effort: Some(effort.into()),
            workspace_id: workspace.id.clone(), native_id: None, status: "queued".into(),
            exit_reason: None, created_ms: now(), ended_ms: None, title: source.title.clone(),
            relation_source: Some("managed-continuation".into()),
            relation_confidence: Some("exact (completed checkpoint)".into()),
            capabilities: adapters::capabilities(harness), process_generation: 0, attention: None,
        };
        {
            let store = self.store.lock().unwrap();
            store.conn.execute_batch("SAVEPOINT handoff_create")?;
            let writes = (|| -> Result<()> {
                store.insert_run(&run)?;
                store.set_workspace_owner(&workspace.id, Some(&run.id))?;
                store.conn.execute("UPDATE runs SET launch=?2 WHERE id=?1",
                    rusqlite::params![run.id, json!({"approval":approval,"sandbox":sandbox,"extra_args":[],
                        "handoff_hash":request_hash,"source_run_id":source.id,"snapshot_id":snapshot.id,
                        "handoff":p["handoff"]}).to_string()])?;
                Ok(())
            })();
            match writes {
                Ok(()) => store.conn.execute_batch("RELEASE handoff_create")?,
                Err(error) => {
                    let _ = store.conn.execute_batch("ROLLBACK TO handoff_create; RELEASE handoff_create");
                    return Err(error);
                }
            }
        }
        drop(guard);
        self.emit(Some(&run.task_id), Some(&run.id), "handoff_created", "user", "exact",
            json!({"source_run_id":source.id,"checkpoint_snapshot_id":snapshot.id,
                "context_loss":"native session and tool messages not transferred"}))?;
        if let Err(error) = self.start_turn(&run.id, &prompt, false, &TurnOpts::default()) {
            self.mark_ended(&run, "failed", &format!("handoff launch failed: {error}"))?;
            return Ok(json!({"run":self.run(&run.id)?,"workspace":workspace,
                "snapshot_id":snapshot.id,"launch_error":error.to_string()}));
        }
        Ok(json!({"run":self.run(&run.id)?,"workspace":workspace,"snapshot_id":snapshot.id}))
    }

    /// Called while the profile gate is held and the prior turn has settled.
    /// This never opens a metadata session beside an active parent process.
    fn refresh_parent_discovery_between_turns(&self, profile_id: &str,
        workspace: &Workspace, expected_generation: i64) -> Result<Value> {
        use std::time::{Duration, Instant};
        let deadline = Instant::now() + Duration::from_secs(8);
        let remaining = || -> Result<Duration> {
            let left = deadline.saturating_duration_since(Instant::now());
            if left < Duration::from_millis(20) { bail!("parent Auto metadata deadline elapsed"); }
            Ok(left)
        };
        let profile = self.profile(profile_id)?;
        if profile.harness != "codex" { bail!("parent Auto profile is not Codex"); }
        if self.store.lock().unwrap().runs()?.iter().any(|other|
            other.profile_id.as_deref() == Some(profile_id)
                && ACTIVE.contains(&other.status.as_str())) {
            bail!("parent Auto profile has an active run");
        }
        let program = adapters::resolve_program("codex-app")
            .ok_or_else(|| anyhow!("Codex is not installed"))?;
        let mut env = adapters::base_env(&program.display().to_string());
        env.extend(Self::profile_env(&profile));
        let models = crate::auto_collect::codex_model_list(&program, &env,
            &adapters::neutral_dir(), remaining()?)?;
        let tools = crate::auto_collect::codex_tool_inventory(&program, &env,
            Path::new(&workspace.path), remaining()?)?;
        let model_identity = crate::auto_quota::account_fingerprint(&models.rate_limits)?;
        let tool_identity = crate::auto_quota::account_fingerprint(&tools.rate_limits)?;
        if model_identity != tool_identity {
            bail!("parent Auto account changed during metadata refresh");
        }
        let observed_ms = now();
        let catalog = crate::auto_route::parse_codex_catalog(&models.models, observed_ms)?;
        let tool_catalog = crate::auto_route::parse_codex_tools(&tools.tools, observed_ms)?;
        let snapshot = crate::auto_quota::parse_codex_rate_limits(&models.rate_limits,
            profile_id, models.rate_limits_observed_ms)?;
        let store = self.store.lock().unwrap();
        store.record_auto_account_identity(profile_id, &model_identity)?;
        if store.auto_account_generation(profile_id)? != Some(expected_generation) {
            bail!("parent Auto account changed between turns");
        }
        let event = store.insert_event(models.rate_limits_observed_ms, None, None,
            "quota", "codex-app/metadata-read", "reported",
            &json!({"profile_id":profile_id,"snapshot":snapshot}))?;
        store.insert_auto_quota(event.seq, profile_id, "codex-app/metadata-read", &snapshot)?;
        store.put_auto_model_catalog(profile_id, &catalog)?;
        Ok(json!({"workspace_id":workspace.id,"account_generation":expected_generation,
            "model_catalog":catalog,"tool_catalog":tool_catalog}))
    }

    /// Start a work turn: fresh run-start snapshot, then launch (or stdin for live generic processes).
    pub fn start_turn(self: &Arc<Self>, run_id: &str, prompt: &str, follow_up: bool, opts: &TurnOpts) -> Result<Turn> {
        self.start_turn_internal(run_id, prompt, follow_up, opts, None)
    }

    fn start_turn_internal(self: &Arc<Self>, run_id: &str, prompt: &str, follow_up: bool, opts: &TurnOpts, swarm_identity: Option<&SwarmLaunchIdentity<'_>>) -> Result<Turn> {
        let initial = self.run(run_id)?;
        let continuity = opts.retry_of.is_some() || opts.handoff;
        if follow_up && !continuity && initial.parent_run_id.is_none() {
            // Continuity may route this message to a successor, recursively
            // calling start_turn. Do that before taking either Auto admission
            // gate; neither gate is reentrant.
            if let Some(turn) = crate::handoff::before_follow_up(self, &initial, prompt, opts)? {
                return Ok(turn);
            }
        }
        let profile_gate = initial.profile_id.as_deref().map(|id| self.profile_gate(id));
        let _profile_guard = profile_gate.as_ref().map(|gate| gate.lock().unwrap());
        let workspace_gate = self.workspace_gate(&initial.workspace_id);
        let _workspace_guard = workspace_gate.lock().unwrap();
        let mut run = self.run(run_id)?;
        if run.relation_source.is_none() {
            if let Some(profile_id) = run.profile_id.as_deref() {
                let claim_conflict = {
                    let store = self.store.lock().unwrap();
                    store.auto_claim_conflicts_with_run(profile_id, &run.id)?
                };
                if claim_conflict {
                    if !follow_up {
                        // The task/worktree already exists. Settle its unstarted run
                        // so it does not remain a queued writer or a phantom pool user.
                        self.mark_ended(&run, "failed", MANUAL_POOL_CONFLICT)?;
                    }
                    bail!("{MANUAL_POOL_CONFLICT}");
                }
            }
        }
        if run.relation_source.as_deref() == Some("managed-delegation") {
            let store = self.store.lock().unwrap();
            let launch: Option<String> = store.conn.query_row(
                "SELECT launch FROM runs WHERE id=?1", [run_id], |row| row.get(0))?;
            let launch: Value = launch.as_deref().and_then(|text| serde_json::from_str(text).ok())
                .unwrap_or(Value::Null);
            if launch["auto_selected"] == true && store.runs()?.iter().any(|other|
                other.id != run.id && other.profile_id == run.profile_id
                    && ACTIVE.contains(&other.status.as_str())
                    && !(Some(other.id.as_str()) == run.parent_run_id.as_deref()
                        && store.run(&other.id).ok().flatten().is_some_and(|parent| {
                            let parent_launch: Option<String> = store.conn.query_row(
                                "SELECT launch FROM runs WHERE id=?1", [&parent.id], |row| row.get(0)).ok().flatten();
                            parent_launch.as_deref().and_then(|text| serde_json::from_str::<Value>(text).ok())
                                .is_some_and(|meta| meta["generic"]["auto_routing"] == true)
                        }))) {
                bail!("automatic child profile has another active run");
            }
            if launch["auto_selected"] == true {
                let auto_parent = run.parent_run_id.as_deref().filter(|parent_id| {
                    let saved: Option<String> = store.conn.query_row(
                        "SELECT launch FROM runs WHERE id=?1", [*parent_id], |row| row.get(0))
                        .ok().flatten();
                    saved.as_deref().and_then(|text| serde_json::from_str::<Value>(text).ok())
                        .is_some_and(|meta| meta["generic"]["auto_routing"] == true)
                });
                if let Some(pool_id) = run.profile_id.as_deref()
                    .map(|profile_id| store.auto_account_pool_id(profile_id)).transpose()?.flatten() {
                    if store.active_run_on_known_account_pool(&pool_id,
                        Some(&run.id), auto_parent)? {
                        bail!("automatic child account has another active run");
                    }
                }
            }
        }
        if follow_up && run.relation_source.as_deref() == Some("managed-delegation") {
            bail!("a managed work unit has one result; delegate a new work unit instead");
        }
        if run.parent_run_id.is_some() && !matches!(run.relation_source.as_deref(), Some("managed-delegation" | "managed-continuation")) {
            bail!("follow-ups go to the top-level run; native children are controlled by their parent harness");
        }
        if swarm_identity.is_none() {
            let store = self.store.lock().unwrap();
            if store.conn.prepare("SELECT 1 FROM swarm_worker_launches WHERE overseer_run_id=?1")?
                .exists([run_id])? {
                bail!("Swarm worker runs cannot receive ordinary follow-ups; continue through the Swarm director");
            }
            if store.conn.prepare("SELECT 1 FROM swarm_director_owners WHERE overseer_run_id=?1")?
                .exists([run_id])? {
                bail!("Swarm director runs cannot receive ordinary follow-ups; continue through Swarm coordination");
            }
        }
        let ws = self.workspace(&run.workspace_id)?;
        if ws.removed_ms.is_some() {
            bail!("workspace was removed");
        }
        if follow_up && !continuity && run.status == crate::handoff::HANDED_OFF {
            bail!("run handoff changed while preparing the follow-up; retry it");
        }
        if follow_up && !continuity {
            if ACTIVE.contains(&run.status.as_str()) && adapters::follow_up_via_stdin(&run.harness, prompt).is_none() {
                bail!("run is still working; interrupt it or wait for it to finish before sending a follow-up");
            }
            if !ACTIVE.contains(&run.status.as_str()) {
                if let Some(other) = self.active_writer(&ws.path)? {
                    if other.id != run.id {
                        bail!("workspace has another active writer ({})", other.id);
                    }
                }
                if let Some(hold) = self.store.lock().unwrap().shared_writer_hold(&ws.path, None, Some(&run.id))? {
                    bail!("workspace is held by shared launch {hold}");
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
                store.record_claude_identity(profile_id, &auth)?;
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
        let coordinating = self.store.lock().unwrap().conn.query_row(
            "SELECT 1 FROM run_roles WHERE run_id=?1 AND role='overseer'", [run_id], |_| Ok(())).is_ok();
        let mut resume_slot = if follow_up && !ACTIVE.contains(&run.status.as_str()) && !coordinating {
            Some(self.reserve_agent_slot()?)
        } else {
            None
        };
        let launch_meta: Value = {
            let store = self.store.lock().unwrap();
            store.conn.query_row("SELECT launch FROM runs WHERE id=?1", [run_id], |r| r.get::<_, Option<String>>(0))?.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(Value::Null)
        };
        let mut generic_meta = launch_meta.get("generic").cloned().unwrap_or(launch_meta.clone());
        if generic_meta["auto_routing"] == true {
            if !self.store.lock().unwrap().auto_mode_enabled()? {
                bail!("Auto Mode was disabled before the parent turn started");
            }
            if follow_up && run.harness == "codex-app" {
                if let Some(profile_id) = run.profile_id.as_deref().filter(|profile_id|
                    generic_meta["auto_allowed_profiles"].as_array().is_some_and(|allowed|
                        allowed.iter().any(|id| id.as_str() == Some(profile_id)))) {
                    let expected = generic_meta["auto_parent_discovery"]["account_generation"].as_i64()
                        .ok_or_else(|| anyhow!("parent Auto account preflight is unavailable"))?;
                    generic_meta["auto_parent_discovery"] = self
                        .refresh_parent_discovery_between_turns(profile_id, &ws, expected)?;
                }
            }
        }
        // Turn options: this turn's choices, else the run's last ones (a model change sticks).
        let effort = opts.effort.clone().or_else(|| generic_meta["opts"]["effort"].as_str().map(str::to_string));
        let mode = opts.mode.clone().or_else(|| generic_meta["opts"]["mode"].as_str().map(str::to_string));
        adapters::check_turn_options(&run.harness, effort.as_deref(), mode.as_deref(), opts.images.len())?;
        let saved_sandbox = generic_meta["sandbox"].as_str().unwrap_or("workspace-write");
        if !matches!(saved_sandbox, "read-only" | "workspace-write") {
            bail!("saved run sandbox is unsupported");
        }
        if run.harness == "codex" {
            if saved_sandbox == "read-only" && mode.as_deref() == Some("workspace-write") {
                bail!("turn cannot widen the saved read-only sandbox");
            }
            if mode.as_deref() == Some("read-only") {
                generic_meta["sandbox"] = json!("read-only");
            }
        }
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
        let turn = match &opts.retry_of {
            // A turn sent again after a wait is the same turn: no new record and no new snapshot.
            Some(id) => {
                let store = self.store.lock().unwrap();
                store.conn.execute("UPDATE turns SET status='running', ended_ms=NULL WHERE id=?1 AND run_id=?2", rusqlite::params![id, run_id])?;
                store.turns(run_id)?.into_iter().find(|t| &t.id == id).ok_or_else(|| anyhow!("turn {id} is not a turn of this run"))?
            }
            None => {
                let snap = self.take_snapshot(&ws, "run-start")?;
                let n = self.store.lock().unwrap().turns(run_id)?.len() as i64 + 1;
                // Guardrails are repeated on later turns; the briefing about the agents beside this
                // one goes with its task (AC-190).
                let preface = if follow_up { self.guardrail_preface(run_id) } else { self.briefing_preface(run_id) };
                let prompt_owned = if preface.is_empty() { prompt.to_string() } else { format!("{preface}\n\n{prompt}") };
                let turn = Turn { id: format!("u-{}", short_id()), run_id: run_id.into(), n, prompt: prompt_owned, snapshot_id: Some(snap.id.clone()), started_ms: now(), ended_ms: None, status: "running".into() };
                {
                    let store = self.store.lock().unwrap();
                    if let (None, Some(profile_id)) = (run.relation_source.as_deref(), run.profile_id.as_deref()) {
                        if !store.insert_turn_if_no_auto_claim(&turn, profile_id)? {
                            bail!("{MANUAL_POOL_CONFLICT}");
                        }
                    } else {
                        store.insert_turn(&turn)?;
                    }
                }
                self.emit(Some(&run.task_id), Some(run_id), "turn_started", "daemon", "exact", json!({"turn": turn, "snapshot": snap.commit_sha}))?;
                turn
            }
        };
        // What the harness receives: the turn's prompt, with its preface when it has one.
        let prompt_owned = turn.prompt.clone();
        let prompt = prompt_owned.as_str();
        let mut external_effect_attempted = false;
        let launch_result = (|| -> Result<()> {
        if follow_up && !continuity && ACTIVE.contains(&run.status.as_str()) {
            if let Some(line) = adapters::follow_up_via_stdin(&run.harness, prompt) {
                external_effect_attempted = true;
                self.send_stdin(&run, &line)?;
                return Ok(());
            }
        }
        let mut profile_env = match &run.profile_id {
            Some(id) => Self::profile_env(&self.profile(id)?),
            None => BTreeMap::new(),
        };
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
        let mut extra_args: Vec<String> = generic_meta["extra_args"].as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()).unwrap_or_default();
        if follow_up {
            extra_args.extend(self.guardrail_launch_args(run_id, &run.harness));
        }
        // The channel back to Overseer, on every turn of an agent that has one (AC-190).
        // A native Swarm member talks through its director instead (the Swarm/Auto contract).
        let swarm_tools_config = generic_meta["swarm_tools"]["config"].as_str().map(PathBuf::from);
        let swarm_tools_allowed = crate::swarm::native::allowed_tools(
            generic_meta["swarm_tools"]["role"].as_str().unwrap_or_default());
        if swarm_tools_config.is_none() {
            extra_args.extend(self.channel_launch_args(run_id, &run.harness)?);
        }
        let resume = if follow_up || generic_meta["resume_first_turn"] == true { run.native_id.clone() } else { None };
        if follow_up && resume.is_none() && run.harness != "generic" {
            bail!("no native session id was reported for this run, so it cannot be resumed");
        }
        if crate::continuity::is_local(&run) {
            // The guard, the context and the profile of a local run (Continuity, AC-138 and AC-140).
            if let Err(e) = crate::continuity::prepare_local_run(self, &mut run, &profile_env) {
                // A refused local run ends with its reason; it is never left waiting to launch.
                self.mark_ended(&run, "failed", &format!("not launched: {e}"))?;
                return Err(e);
            }
        }
        let sandbox = generic_meta["sandbox"].as_str().unwrap_or("workspace-write").to_string();
        if !matches!(sandbox.as_str(), "read-only" | "workspace-write") {
            bail!("saved run sandbox is unsupported");
        }
        let mut launch = adapters::launch(
            &run.harness,
            &LaunchReq {
                cwd: Path::new(&ws.path),
                prompt,
                model: run.model.as_deref(),
                effort: effort.as_deref().or(run.effort.as_deref()),
                sandbox: Some(&sandbox),
                profile_env,
                resume_session: resume.as_deref(),
                program_override: generic_meta["program"].as_str(),
                args_override: args.as_deref(),
                extra_args: &extra_args,
                permission_mode: mode.as_deref(),
                images: &images,
                swarm_worker: generic_meta["swarm_worker"] == true,
                swarm_tools: swarm_tools_config.as_deref().map(|config| adapters::SwarmTools {
                    config, allowed: &swarm_tools_allowed }),
            },
        )?;
        if generic_meta["auto_routing"] == true {
            let generation = run.process_generation + 1;
            let dir = paths::runs_dir().join(run_id).join(format!("p{generation}"));
            paths::ensure_private_dir(&dir)?;
            let capability = format!("{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple());
            let capability_path = dir.join("auto-bridge.cap");
            {
                use std::io::Write;
                use std::os::unix::fs::OpenOptionsExt;
                let mut file = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600)
                    .open(&capability_path)?;
                file.write_all(capability.as_bytes())?;
            }
            use sha2::Digest;
            generic_meta["auto_bridge_hash"] = json!(format!("{:x}", sha2::Sha256::digest(capability.as_bytes())));
            generic_meta["auto_bridge_generation"] = json!(generation);
            // Codex app-server accepts per-process MCP overrides. Keep the
            // bearer value in a private file, never in argv or launch.json.
            let command = serde_json::to_string(&self.exe.display().to_string())?;
            let args = serde_json::to_string(&vec!["auto-mcp".to_string(), run.id.clone(),
                capability_path.display().to_string(), paths::socket_path().display().to_string()])?;
            launch.args.extend(["-c".into(), format!("mcp_servers.overseer_auto.command={command}"),
                "-c".into(), format!("mcp_servers.overseer_auto.args={args}"),
                "-c".into(), "mcp_servers.overseer_auto.required=true".into()]);
        }
        if let Some(identity) = swarm_identity {
            // A native member (Claude with the daemon's Swarm tools) is the
            // proposed path behind `swarm.native_director`; anything else is
            // the fixture protocol.
            let native = swarm_tools_config.is_some() && run.harness == "claude";
            if !native && (std::env::var("OVERSEER_SWARM_FIXTURE_API").as_deref() != Ok("1")
                || (run.harness != "generic" && !matches!(identity, SwarmLaunchIdentity::Worker(_)))) {
                bail!("scripted Swarm identity requires a fixture worker or generic director");
            }
            // Synthetic provider streams exercise lifecycle parsing without giving a
            // model harness a worker command credential. Generic fixture scripts
            // keep their private broker environment for scripted report/ack calls.
            if run.harness == "generic" {
                let mut private_env = vec![
                    ("OVERSEER_HOME", paths::data_dir().display().to_string()),
                    ("OVERSEER_SOCKET", paths::socket_path().display().to_string()),
                    ("OVERSEER_BIN", self.exe.display().to_string()),
                ];
                match identity {
                    SwarmLaunchIdentity::Worker(identity) => private_env.extend([
                        ("OVERSEER_SWARM_RUN_ID", identity.run_id.clone()),
                        ("OVERSEER_SWARM_JOB_ID", identity.job_id.clone()),
                        ("OVERSEER_SWARM_ATTEMPT_ID", identity.attempt_id.clone()),
                        ("OVERSEER_SWARM_TOKEN", identity.token.clone()),
                        ("OVERSEER_SWARM_REVISION", identity.revision.to_string()),
                    ]),
                    SwarmLaunchIdentity::Director(identity) => private_env.extend([
                        ("OVERSEER_SWARM_RUN_ID", identity.run_id.clone()),
                        ("OVERSEER_SWARM_GENERATION", identity.generation.to_string()),
                        ("OVERSEER_SWARM_DIRECTOR_TOKEN", identity.token.clone()),
                    ]),
                }
                for (key, value) in private_env {
                    launch.env.insert(key.to_string(), value);
                }
            }
        }
        self.store.lock().unwrap().set_workspace_owner(&ws.id, Some(run_id))?;
        let app = json!({"prompt": prompt, "cwd": ws.path, "model": run.model, "effort": effort.as_deref().or(run.effort.as_deref()), "resume": resume, "approval": generic_meta["approval"].as_str().unwrap_or("on-request"),
            "sandbox":sandbox,
            "required_tools":generic_meta["required_tools"], "auto_selected":generic_meta["auto_selected"],
            "expected_account_generation":generic_meta["expected_account_generation"]});
        external_effect_attempted = true;
        self.spawn_process(&run, &ws, launch, json!({"generic": generic_meta, "app": app}))?;
        Ok(())
        })();
        if let Err(error) = launch_result {
            if !external_effect_attempted && opts.retry_of.is_none() {
                // Only a proven pre-effect failure may release this manual
                // follow-up's temporary account occupancy. Once stdin or a
                // supervisor launch was attempted, its outcome may be unknown.
                self.store.lock().unwrap().conn.execute(
                    "UPDATE turns SET status='failed',ended_ms=?3
                     WHERE id=?1 AND run_id=?2 AND ended_ms IS NULL",
                    rusqlite::params![turn.id, run.id, now()])?;
            }
            return Err(error);
        }
        if let Some(reservation) = &mut resume_slot {
            reservation.release_after_start();
        }
        Ok(turn)
    }

    fn spawn_process(self: &Arc<Self>, run: &Run, ws: &Workspace, launch: adapters::Launch, meta: Value) -> Result<()> {
        let generation = run.process_generation + 1;
        let run_dir = paths::runs_dir().join(&run.id).join(format!("p{generation}"));
        paths::ensure_private_dir(&run_dir)?;
        let control = control_socket_path(&run.id, generation);
        let auto_execution_deadline_ms = if meta["generic"]["auto_selected"] == true {
            let budget = meta["generic"]["execution_budget_ms"].as_u64()
                .filter(|ms| (1_000..=1_800_000).contains(ms))
                .ok_or_else(|| anyhow!("automatic child execution budget is unavailable"))?;
            let started = self.store.lock().unwrap().turns(&run.id)?.first()
                .map(|turn| turn.started_ms).filter(|ms| *ms > 0)
                .ok_or_else(|| anyhow!("automatic child turn start is unavailable"))? as u64;
            Some(started.saturating_add(budget))
        } else { None };
        let file = LaunchFile {
            program: launch.program.clone(),
            args: launch.args.clone(),
            cwd: ws.path.clone(),
            env: launch.env.clone(),
            initial_stdin: launch.initial_stdin.clone(),
            close_stdin: launch.close_stdin,
            control_socket: control.display().to_string(),
            auto_execution_deadline_ms,
        };
        std::fs::write(run_dir.join("launch.json"), serde_json::to_vec_pretty(&file)?)?;
        if run.harness == "codex-app" && (run.relation_source.as_deref() == Some("managed-delegation")
            || meta["generic"]["auto_selected"] == true) {
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
        let mut recorded_meta = meta;
        recorded_meta["program"] = json!(launch.program);
        recorded_meta["args"] = json!(launch.args.iter().map(|a| redact(a)).collect::<Vec<_>>());
        recorded_meta["env_keys"] = json!(launch.env.keys().collect::<Vec<_>>());
        let record_before_spawn = recorded_meta["generic"]["auto_selected"] == true
            || recorded_meta["generic"]["auto_routing"] == true
            || self.store.lock().unwrap().shared_launch_bound_unsettled(&run.id)?;
        if record_before_spawn {
            // A crash or write failure after cmd.spawn must not leave a live
            // Auto supervisor with no durable identity to reconcile.
            self.store.lock().unwrap().set_run_process(&run.id,
                &run_dir.display().to_string(), generation, &recorded_meta)?;
        }
        self.store.lock().unwrap().mark_director_spawn_requested(&run.id)?;
        self.store.lock().unwrap().mark_worker_spawn_requested(&run.id)?;
        let mut child = match cmd.spawn().context("starting run supervisor") {
            Ok(child) => child,
            Err(error) => {
                if record_before_spawn {
                    let store = self.store.lock().unwrap();
                    let cleared = store.conn.execute(
                        "UPDATE runs SET run_dir=NULL WHERE id=?1 AND run_dir=?2 AND process_generation=?3",
                        rusqlite::params![run.id, run_dir.display().to_string(), generation],
                    )?;
                    if cleared != 1 { bail!("pre-spawn supervisor identity changed"); }
                }
                return Err(error);
            }
        };
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        if !record_before_spawn {
            self.store.lock().unwrap().set_run_process(&run.id,
                &run_dir.display().to_string(), generation, &recorded_meta)?;
        }
        self.store.lock().unwrap().update_run_status(&run.id, "starting", None, None)?;
        {
            let store = self.store.lock().unwrap();
            store.conn.execute("UPDATE runs SET exit_reason=NULL, ended_ms=NULL, attention=NULL WHERE id=?1", [&run.id])?;
        }
        self.emit(Some(&run.task_id), Some(&run.id), "status", "daemon", "exact", json!({"status": "starting", "generation": generation, "program": launch.program}))?;
        self.spawn_tail(&run.id);
        if run.harness == "codex-app" && (run.relation_source.as_deref() == Some("managed-delegation")
            || recorded_meta["generic"]["auto_selected"] == true) {
            self.watch_codex_account_handshake(run.id.clone(), generation, run_dir);
        }
        Ok(())
    }

    /// Metadata calls must not leave an unstarted selected run holding its
    /// workspace indefinitely. The deadline lives beside the supervisor so a
    /// daemon restart cannot reset it or launch another child.
    fn watch_codex_account_handshake(self: &Arc<Self>, run_id: String, generation: i64, dir: PathBuf) {
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
                    json!({"reason":"Codex account metadata handshake timed out before model launch"}));
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
        if run.parent_run_id.is_some() && !matches!(run.relation_source.as_deref(), Some("managed-delegation" | "managed-continuation")) {
            bail!("native children are interrupted through their parent run");
        }
        if !ACTIVE.contains(&run.status.as_str()) {
            bail!("run is not active (status {})", run.status);
        }
        if run.status == crate::handoff::WAITING_FOR_CONNECTION || run.status == crate::handoff::WAITING_FOR_MEMORY {
            // Nothing is running: Stop ends the wait.
            return crate::handoff::stop_waiting(self, &run);
        }
        let mut child_interrupt_errors = Vec::new();
        if run.parent_run_id.is_none() || run.relation_source.as_deref() == Some("managed-continuation") {
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
        if !allow {
            // What the owner refused is remembered, so Overseer never has another agent do it (AC-196).
            let input = &attention["input"];
            let detail = input["command"].as_str().or(input["file_path"].as_str()).or(input["path"].as_str()).map(str::to_string).unwrap_or_else(|| input.to_string().chars().take(200).collect());
            self.store.lock().unwrap().conn.execute("INSERT INTO denied_permissions(run_id, tool, detail, ts) VALUES(?1, ?2, ?3, ?4)", rusqlite::params![run_id, attention["tool"].as_str().unwrap_or(""), detail, now()])?;
        }
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
        let mut pending_learning = Vec::new();
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
                    self.apply_norm(&store, run, norm, rec["t"].as_i64(), state,
                        &mut emitted, &mut pending_learning)?;
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
            let had_learning = !pending_learning.is_empty();
            let mut learning_failed = false;
            for (event_seq, measurement) in pending_learning {
                let recorded = store.insert_auto_measurement(event_seq, &measurement);
                learning_failed |= recorded.is_err();
            }
            if had_learning {
                self.learning_usage_paused.store(learning_failed, std::sync::atomic::Ordering::Relaxed);
            }
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

    fn apply_norm(&self, store: &Store, run: &Run, norm: Norm, captured_ms: Option<i64>, state: &mut TailState,
        out: &mut Vec<Event>, pending_learning: &mut Vec<(i64, crate::auto_telemetry::Measurement)>) -> Result<()> {
        let task = Some(run.task_id.as_str());
        let rid = Some(run.id.as_str());
        let mut ev = |kind: &str, source: &str, conf: &str, payload: Value, run_override: Option<&str>| -> Result<()> {
            out.push(store.insert_event(now(), task, run_override.or(rid), kind, source, conf, &redact_value(payload))?);
            Ok(())
        };
        if matches!(norm, Norm::Running | Norm::Tool { .. } | Norm::Text { .. }) {
            state.between_turns = false;
        }
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
                        // Supervisor line capture precedes daemon replay; a
                        // delayed batch must not refresh an older native meter.
                        let observed_ms = captured_ms.filter(|time| *time > 0).unwrap_or(event.ts);
                        if let Ok(mut snapshot) = crate::auto_quota::parse_codex_rate_limits(&payload, pool_id, observed_ms) {
                            if let Some(prior) = store.latest_auto_quota(pool_id)? {
                                snapshot.reconcile_unordered_native_evidence(&prior.snapshot, observed_ms);
                            }
                            let _ = store.insert_auto_quota(event.seq, pool_id, "codex-app/native-update", &snapshot);
                        }
                    }
                }
                if let Some(measurement) = crate::auto_telemetry::from_usage_with_effort(
                    event.ts, &run.task_id, &run.id, &run.harness,
                    run.profile_id.as_deref(), run.model.as_deref(), run.effort.as_deref(), &event.payload,
                ) {
                    // Only publish learning after the execution transaction
                    // commits; a rolled-back event must never leave a sample.
                    pending_learning.push((event.seq, measurement));
                }
                out.push(event);
            }
            Norm::Quota(raw) => {
                if run.harness == "claude" {
                    if let Some(pool_id) = run.profile_id.as_deref() {
                        let observed_ms = captured_ms.filter(|time| *time > 0).unwrap_or_else(now);
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
                                native_uncertain_until_ms:None,
                            },
                        };
                        // The event reports no plan: the latest identity read's
                        // plan stands for it (the owner's decision of 2026-09-28).
                        if let Some(plan) = store.auto_account_plan(pool_id)? {
                            for window in snapshot.windows.iter_mut().filter(|w| w.plan_type.is_none()) {
                                window.plan_type = Some(plan.clone());
                            }
                        }
                        if let Some(prior) = store.latest_auto_quota(pool_id)? {
                            snapshot.reconcile_unordered_native_evidence(&prior.snapshot, observed_ms);
                        }
                        let event = store.insert_event(observed_ms, task, rid, "auto_quota", "harness", "normalized",
                            &json!({"pool_id":pool_id,"snapshot":snapshot}))?;
                        let _ = store.insert_auto_quota(event.seq, pool_id, "claude/native-rate-limit-event", &snapshot);
                        out.push(event);
                    }
                }
            }
            Norm::Permission { request_id, tool, input } => {
                // The daemon's own tools (Overseer's reads, an agent's channel) are always allowed:
                // the daemon decides what each token may do.
                if tool.starts_with("mcp__overseer__") {
                    if let Some(reply) = adapters::permission_reply(&run.harness, &request_id, true, &input, "") {
                        state.sends.push(reply);
                    }
                    ev("permission", "daemon", "exact", json!({"kind": "permission", "request_id": request_id, "tool": tool, "auto_allowed": "Overseer's own tool"}), None)?;
                    return Ok(());
                }
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
            Norm::ErrorRetryAfter { class, message, retry_after_ms } => {
                state.last_error = Some((class.clone(), message.clone()));
                ev("error", "harness", "exact", json!({"class": class, "message": message,
                    "retry_after_ms": retry_after_ms}), None)?
            }
            Norm::BackgroundTasks(n) => state.background = n,
            Norm::BackgroundLaunched(id) => {
                state.backgrounded.insert(id);
            }
            Norm::BackgroundNotified(id) => {
                if state.backgrounded.remove(&id) {
                    // Between turns, the notice itself starts the next one.
                    if state.between_turns { state.expected_turns += 1 } else { state.unread_notices += 1 }
                }
            }
            // A notice reported mid-turn is read by the main agent's next model call in that
            // same turn; only one still unread when the turn ends brings another turn.
            Norm::MainContinues => state.unread_notices = 0,
            Norm::TurnDone { ok, summary } => {
                let interrupted = store.run_process(&run.id)?.map(|(dir, _, _)| Path::new(&dir).join("interrupt.requested").exists()).unwrap_or(false);
                if run.harness == "claude" {
                    state.expected_turns = state.expected_turns.saturating_sub(1) + std::mem::take(&mut state.unread_notices);
                    if !interrupted && (state.background > 0 || state.expected_turns > 0) {
                        // Claude reports an interim result while background subagents run, and
                        // continues with another turn for each finished one (even one that
                        // finished before this result). The session must stay open so those
                        // turns' permission requests can be answered.
                        state.between_turns = true;
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
                let meta: Value = store.conn.query_row("SELECT launch FROM runs WHERE id=?1", [&run.id], |r| r.get::<_, Option<String>>(0))?.and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Value::Null);
                let app = &meta["app"];
                if let Some(err) = error {
                    if id == "ovs-auto-quota" {
                        if app["auto_selected"] == true {
                            let msg = "selected Codex account evidence unavailable";
                            state.last_error = Some(("account".into(), msg.into()));
                            ev("error", "daemon", "exact", json!({"class":"account","message":msg}), None)?;
                            state.turn_done = Some(false);
                            state.close_stdin = true;
                            return Ok(());
                        }
                        ev("auto_account_unknown", "harness", "exact", json!({"reason":"account quota metadata unavailable"}), None)?;
                        state.sends.push(format!("{}\n", codex_child_next_request(app)?));
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
                        let msg = if run.harness == "codex-app" &&
                            (run.relation_source.as_deref() == Some("managed-delegation") || app["auto_selected"] == true) {
                            json!({"id":"ovs-account","method":"account/read","params":{"refreshToken":false}})
                        } else { codex_thread_request(app)? };
                        state.sends.push(format!("{msg}\n"));
                    }
                    "ovs-account" => {
                        if result["requiresOpenaiAuth"] != true || result["account"]["type"] != "chatgpt" {
                            ev("error", "harness", "exact", json!({"class":"authentication","message":"Codex account check requires ChatGPT login"}), None)?;
                            state.turn_done = Some(false);
                            state.close_stdin = true;
                            return Ok(());
                        }
                        state.sends.push(format!("{}\n", json!({"id":"ovs-auto-quota","method":"account/rateLimits/read","params":{}})));
                    }
                    "ovs-auto-quota" => {
                        let observed_ms = now();
                        let profile_id = run.profile_id.as_deref().ok_or_else(|| anyhow!("run account profile unavailable"))?;
                        let quota = crate::auto_quota::parse_codex_rate_limits(&result, profile_id, observed_ms).ok();
                        let recorded = (|| -> Result<bool> {
                            let fingerprint = crate::auto_quota::account_fingerprint(&result)?;
                            store.record_auto_account_identity(profile_id, &fingerprint)?;
                            let generation = store.auto_account_generation(profile_id)?.ok_or_else(|| anyhow!("account generation unavailable"))?;
                            store.record_auto_run_account(&run.id, profile_id, generation,
                                quota.as_ref().and_then(|snapshot| snapshot.reported_plan_type()))?;
                            if app["auto_selected"] == true {
                                if app["expected_account_generation"].as_i64() != Some(generation) {
                                    return Ok(false);
                                }
                                if let Some(model) = run.model.as_deref() {
                                    if matches!(quota.as_ref().map(|snapshot| snapshot.state_for(model, now())),
                                        Some(crate::auto_quota::QuotaState::Exhausted)) {
                                        return Ok(false);
                                    }
                                }
                            }
                            Ok(true)
                        })();
                        self.learning_account_paused.store(recorded.is_err(), std::sync::atomic::Ordering::Relaxed);
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
                                ev("auto_account_unknown", "daemon", "exact", json!({"reason":"run account evidence unavailable"}), None)?;
                            }
                            Ok(true) => {}
                        }
                        if let Some(snapshot) = quota {
                            // A normalized, run-scoped pre-turn observation can be
                            // paired with a later metadata read. It is not yet an
                            // attributable subscription charge.
                            let event = store.insert_event(observed_ms, task, rid, "auto_quota",
                                "codex-app/managed-pre-turn", "reported",
                                &json!({"pool_id":profile_id,"snapshot":snapshot}))?;
                            store.insert_auto_quota(event.seq, profile_id,
                                "codex-app/managed-pre-turn", &snapshot)?;
                            out.push(event);
                        }
                        state.sends.push(format!("{}\n", codex_child_next_request(app)?));
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
                        state.sends.push(format!("{}\n", codex_thread_request(app)?));
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
        let mut pending_learning = Vec::new();
        {
            let store = self.store.lock().unwrap();
            let tx = store.conn.unchecked_transaction()?;
            for norm in fresh {
                self.apply_norm(&store, run, norm, None, state,
                    &mut emitted, &mut pending_learning)?;
            }
            tx.commit()?;
            let had_learning = !pending_learning.is_empty();
            let mut learning_failed = false;
            for (event_seq, measurement) in pending_learning {
                let recorded = store.insert_auto_measurement(event_seq, &measurement);
                learning_failed |= recorded.is_err();
            }
            if had_learning {
                self.learning_usage_paused.store(learning_failed, std::sync::atomic::Ordering::Relaxed);
            }
        }
        for e in emitted {
            let _ = self.events.send(e);
        }
        Ok(())
    }

    fn finalize(&self, run: &Run, dir: &Path, exit: &ExitInfo, state: &TailState) -> Result<()> {
        let interrupted = dir.join("interrupt.requested").exists();
        let auto_budget = dir.join("auto-budget.requested").exists();
        if auto_budget {
            let reported: i64 = self.store.lock().unwrap().conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM events WHERE run_id=?1 AND kind='auto_execution_budget_exhausted')",
                [&run.id], |row| row.get(0))?;
            if reported == 0 {
                self.emit(Some(&run.task_id), Some(&run.id), "auto_execution_budget_exhausted",
                    "supervisor", "exact", json!({"outcome":"stopped_existing_child"}))?;
            }
        }
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
        // Continuity: a turn that failed on the connection parks its run instead of ending it.
        if crate::handoff::park(self, run, status, state.last_error.as_ref(), dir)? {
            return Ok(());
        }
        self.mark_ended(run, status, &reason)
    }

    pub(crate) fn mark_ended(&self, run: &Run, status: &str, reason: &str) -> Result<()> {
        let mut emitted = Vec::new();
        // Every other end status comes after an exit record or before any
        // supervisor; a lost supervisor may leave its harness running.
        let process_gone = status != "disconnected" || self.run_processes_gone(&run.id);
        {
            let store = self.store.lock().unwrap();
            store.conn.execute_batch("SAVEPOINT settle_run")?;
            let settled = (|| -> Result<()> {
                let ended = now();
                store.update_run_status(&run.id, status, Some(reason), Some(ended))?;
                store.set_run_attention(&run.id, None)?;
                let turn_status = if status == "completed" { "completed" } else { status };
                store.finish_open_turns(&run.id, turn_status, ended)?;
                store.release_settled_auto_pool_claim(&run.id)?;
                store.settle_shared_launch_run(&run.id, process_gone)?;
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
                        if matches!(child.relation_source.as_deref(), Some("managed-delegation" | "managed-continuation")) {
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
        if (run.relation_source.as_deref() == Some("managed-delegation")
            || run.parent_run_id.is_none())
            && matches!(status, "completed" | "failed" | "interrupted") {
            let recorded = self.store.lock().unwrap().record_auto_work_observation(&run.id);
            match recorded {
                Ok(true) => self.learning_work_paused.store(false, std::sync::atomic::Ordering::Relaxed),
                Err(_) => self.learning_work_paused.store(true, std::sync::atomic::Ordering::Relaxed),
                Ok(false) => {}
            }
        }
        Ok(())
    }

    /// Called once at startup: reattach to surviving supervisors, finalize exited
    /// ones, and report lost sessions. Never relaunches work.
    fn reattach_unrecorded_director(&self, run: &Run) -> Result<bool> {
        let owner: Option<(String, i64, String, i64)> = self.store.lock().unwrap().conn.query_row(
            "SELECT run_id,generation,token_sha256,lease_expires_ms FROM swarm_director_owners
             WHERE overseer_run_id=?1 AND status='active' AND supervised_launch=1
             AND launch_phase='spawn_requested'",
            [&run.id], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)),
        ).optional()?;
        let Some((swarm_run, generation, digest, lease_expires)) = owner else {
            return Ok(false);
        };
        let dir = paths::runs_dir().join(&run.id).join(format!("p{}",run.process_generation+1));
        if !std::fs::symlink_metadata(&dir).is_ok_and(|m| m.file_type().is_dir()) {
            return Ok(false);
        }
        let launch_path = dir.join("launch.json");
        use std::os::unix::fs::PermissionsExt;
        let Ok(metadata) = std::fs::symlink_metadata(&launch_path) else { return Ok(false) };
        if !metadata.file_type().is_file() || metadata.len() > 1_000_000
            || metadata.permissions().mode() & 0o077 != 0 {
            return Ok(false);
        }
        let Ok(raw) = std::fs::read(&launch_path) else { return Ok(false) };
        let Ok(launch) = serde_json::from_slice::<LaunchFile>(&raw) else { return Ok(false) };
        // A native director carries its token only in its MCP configuration;
        // the token's hash is then its whole identity.
        let native = !launch.env.contains_key("OVERSEER_SWARM_DIRECTOR_TOKEN");
        let Some(token) = launch.env.get("OVERSEER_SWARM_DIRECTOR_TOKEN").cloned()
            .or_else(|| crate::swarm::native::config_token(&launch.args)) else { return Ok(false) };
        let generation_text = generation.to_string();
        use sha2::{Digest, Sha256};
        if format!("{:x}",Sha256::digest(token.as_bytes())) != digest
            || (!native && launch.env.get("OVERSEER_SWARM_RUN_ID").map(String::as_str) != Some(swarm_run.as_str()))
            || (!native && launch.env.get("OVERSEER_SWARM_GENERATION").map(String::as_str) != Some(generation_text.as_str()))
            || launch.cwd != self.workspace(&run.workspace_id)?.path {
            return Ok(false);
        }
        let exited = std::fs::read(dir.join("exit.json")).ok()
            .and_then(|raw| serde_json::from_slice::<ExitInfo>(&raw).ok()).is_some();
        let live = std::fs::read(dir.join("shim.json")).ok()
            .and_then(|raw| serde_json::from_slice::<ShimInfo>(&raw).ok())
            .is_some_and(|info| pid_alive(info.shim_pid));
        if !exited && !live {
            return Ok(false);
        }
        let store = self.store.lock().unwrap();
        let prior: Option<String> = store.conn.query_row(
            "SELECT launch FROM runs WHERE id=?1", [&run.id], |r| r.get(0))?;
        let generic: Value = prior.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(Value::Null);
        let meta = json!({"generic":generic,"program":launch.program,
            "args":launch.args.iter().map(|arg|redact(arg)).collect::<Vec<_>>(),
            "env_keys":launch.env.keys().collect::<Vec<_>>(),"orphan_reconciled":true});
        if let Err(error) = store.set_run_process(
            &run.id,&dir.display().to_string(),run.process_generation+1,&meta) {
            crate::log(&format!("director orphan reattachment deferred: {}",redact(&error.to_string())));
            return Ok(false);
        }
        store.set_run_attention(&run.id,None)?;
        if live && !exited {
            store.update_run_status(&run.id,"running",None,None)?;
            if lease_expires > now() {
                store.conn.execute(
                    "UPDATE swarm_runs SET status=stalled_from,stalled_from=NULL,
                     stall_reason=NULL,updated_ms=?2 WHERE id=?1 AND status='stalled'
                     AND stall_reason='director_termination_unknown'
                     AND stalled_from IN ('planning','running','paused','draining')",
                    rusqlite::params![swarm_run,now()],
                )?;
            }
        }
        Ok(true)
    }

    fn reattach_unrecorded_worker(&self, run: &Run) -> Result<bool> {
        let identity: Option<(String,String,String,i64,String)> = self.store.lock().unwrap().conn.query_row(
            "SELECT l.run_id,l.job_id,l.attempt_id,a.revision,a.token_sha256
             FROM swarm_worker_launches l JOIN swarm_attempts a ON a.id=l.attempt_id
             WHERE l.overseer_run_id=?1
             AND (l.launch_phase='spawn_requested' OR l.launch_phase IS NULL)
             AND a.status='registered' AND a.run_id=l.run_id AND a.job_id=l.job_id",
            [&run.id], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?)),
        ).optional()?;
        let Some((swarm_run,job,attempt,revision,digest)) = identity else { return Ok(false) };
        let dir = paths::runs_dir().join(&run.id).join(format!("p{}",run.process_generation+1));
        if !std::fs::symlink_metadata(&dir).is_ok_and(|m| m.file_type().is_dir()) {
            return Ok(false);
        }
        let launch_path = dir.join("launch.json");
        use std::os::unix::fs::PermissionsExt;
        let Ok(metadata) = std::fs::symlink_metadata(&launch_path) else { return Ok(false) };
        if !metadata.file_type().is_file() || metadata.len() > 1_000_000
            || metadata.permissions().mode() & 0o077 != 0 {
            return Ok(false);
        }
        let Ok(raw) = std::fs::read(&launch_path) else { return Ok(false) };
        let Ok(launch) = serde_json::from_slice::<LaunchFile>(&raw) else { return Ok(false) };
        let native = !launch.env.contains_key("OVERSEER_SWARM_TOKEN");
        let Some(token) = launch.env.get("OVERSEER_SWARM_TOKEN").cloned()
            .or_else(|| crate::swarm::native::config_token(&launch.args)) else { return Ok(false) };
        let revision_text = revision.to_string();
        use sha2::{Digest, Sha256};
        if format!("{:x}",Sha256::digest(token.as_bytes())) != digest
            || (!native && (launch.env.get("OVERSEER_SWARM_RUN_ID").map(String::as_str) != Some(swarm_run.as_str())
            || launch.env.get("OVERSEER_SWARM_JOB_ID").map(String::as_str) != Some(job.as_str())
            || launch.env.get("OVERSEER_SWARM_ATTEMPT_ID").map(String::as_str) != Some(attempt.as_str())
            || launch.env.get("OVERSEER_SWARM_REVISION").map(String::as_str) != Some(revision_text.as_str())))
            || launch.cwd != self.workspace(&run.workspace_id)?.path {
            return Ok(false);
        }
        let exited = std::fs::read(dir.join("exit.json")).ok()
            .and_then(|raw| serde_json::from_slice::<ExitInfo>(&raw).ok()).is_some();
        let live = std::fs::read(dir.join("shim.json")).ok()
            .and_then(|raw| serde_json::from_slice::<ShimInfo>(&raw).ok())
            .is_some_and(|info| pid_alive(info.shim_pid));
        if !exited && !live { return Ok(false) }
        let store = self.store.lock().unwrap();
        let prior: Option<String> = store.conn.query_row(
            "SELECT launch FROM runs WHERE id=?1", [&run.id], |r| r.get(0))?;
        let generic: Value = prior.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(Value::Null);
        let meta = json!({"generic":generic,"program":launch.program,
            "args":launch.args.iter().map(|arg|redact(arg)).collect::<Vec<_>>(),
            "env_keys":launch.env.keys().collect::<Vec<_>>(),"orphan_reconciled":true});
        if let Err(error) = store.set_run_process(
            &run.id,&dir.display().to_string(),run.process_generation+1,&meta) {
            crate::log(&format!("worker orphan reattachment deferred: {}",redact(&error.to_string())));
            return Ok(false);
        }
        store.set_run_attention(&run.id,None)?;
        if live && !exited { store.update_run_status(&run.id,"running",None,None)?; }
        Ok(true)
    }

    pub fn reconcile(self: &Arc<Self>) -> Result<Value> {
        self.store.lock().unwrap().release_stale_unstarted_auto_pool_claims()?;
        let mut report = self.store.lock().unwrap().reconcile_shared_launches_on_start()?;
        self.reconcile_overseer()?;
        let runs = self.store.lock().unwrap().runs()?;
        for run in runs.iter().filter(|r| (r.parent_run_id.is_none() || matches!(r.relation_source.as_deref(), Some("managed-delegation" | "managed-continuation"))) && (ACTIVE.contains(&r.status.as_str()) || r.status == "disconnected")) {
            let mut process = self.store.lock().unwrap().run_process(&run.id)?;
            if process.is_none() {
                if self.reattach_unrecorded_director(run)? || self.reattach_unrecorded_worker(run)? {
                    process = self.store.lock().unwrap().run_process(&run.id)?;
                }
            }
            let Some((dir, _, _)) = process else {
                if ACTIVE.contains(&run.status.as_str()) {
                    let uncertain = {
                        let store = self.store.lock().unwrap();
                        if crate::swarm::mark_uncertain_director_spawn(&store, &run.id)? {
                            Some("director spawn uncertain")
                        } else if store.mark_worker_spawn_uncertain(&run.id)? {
                            Some("worker spawn uncertain")
                        } else { None }
                    };
                    if let Some(reason) = uncertain {
                        report.push(json!({"run": run.id, "result": reason}));
                    } else {
                        self.mark_ended(run, "failed", "daemon stopped before the run was launched")?;
                        report.push(json!({"run": run.id, "result": "never launched"}));
                    }
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
                if run.harness == "codex-app" && run.native_id.is_none()
                    && dir.join("auto-account-deadline").exists() {
                    self.watch_codex_account_handshake(run.id.clone(), run.process_generation, dir.clone());
                }
                report.push(json!({"run": run.id, "result": "reattached"}));
            } else if run.status == "disconnected" {
                // Lost by an earlier daemon: its bound holds are released only
                // once both of its processes are confirmed gone.
                if self.run_processes_gone(&run.id)
                    && self.store.lock().unwrap().settle_lost_shared_launch(&run.id)? {
                    report.push(json!({"run": run.id, "result": "lost shared launch settled"}));
                }
            } else {
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
        if let Some(snap) = self.redirect_snapshot(&root.workspace_id) {
            options.push(json!({"mode": "redirect", "label": "Since the change of direction", "base": snap.commit_sha, "available": true, "detail": format!("snapshot {} taken when Overseer redirected this agent", snap.id), "provenance": "recorded", "snapshot": snap}));
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
        self.state_for(false)
    }

    /// Overseer's own run (role `overseer`), its task and its workspace are listed in no agents
    /// list: a client that shows the conversation asks for them with `include_hidden`.
    pub fn state_for(&self, include_hidden: bool) -> Result<Value> {
        let store = self.store.lock().unwrap();
        let hidden: std::collections::HashSet<String> = if include_hidden {
            Default::default()
        } else {
            let mut stmt = store.conn.prepare("SELECT run_id FROM run_roles WHERE role='overseer'")?;
            let ids: std::collections::HashSet<String> = stmt.query_map([], |r| r.get::<_, String>(0))?.flatten().collect();
            ids
        };
        let all_runs = store.runs()?;
        let hidden_tasks: std::collections::HashSet<String> = all_runs.iter().filter(|r| hidden.contains(&r.id)).map(|r| r.task_id.clone()).collect();
        let hidden_ws: std::collections::HashSet<String> = all_runs.iter().filter(|r| hidden.contains(&r.id)).map(|r| r.workspace_id.clone()).collect();
        let runs: Vec<_> = all_runs.into_iter().filter(|r| !hidden_tasks.contains(&r.task_id)).collect();
        let tasks: Vec<_> = store.tasks()?.into_iter().filter(|t| !hidden_tasks.contains(&t.id)).collect();
        let workspaces: Vec<_> = store.workspaces()?.into_iter().filter(|w| !hidden_ws.contains(&w.id)).collect();
        let mut memberships = BTreeMap::new();
        let mut links = store.conn.prepare(
            "SELECT overseer_run_id,run_id,'worker',job_id FROM swarm_worker_launches
             WHERE overseer_run_id IS NOT NULL
             UNION ALL
             SELECT overseer_run_id,run_id,'director',NULL FROM swarm_director_owners
             WHERE overseer_run_id IS NOT NULL",
        )?;
        for row in links.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?,
            r.get::<_, String>(2)?, r.get::<_, Option<String>>(3)?)))? {
            let (process, run, role, job) = row?;
            memberships.insert(process, json!({"role":role,"run_id":run,"job_id":job}));
        }
        drop(links);
        let mut run_values = serde_json::to_value(&runs)?;
        for run in run_values.as_array_mut().expect("runs serialize as an array") {
            if let Some(link) = run["id"].as_str().and_then(|id| memberships.get(id)) {
                run["swarm_membership"] = link.clone();
            }
        }
        let mut turns = serde_json::Map::new();
        for r in runs.iter().filter(|r| r.parent_run_id.is_none() || matches!(r.relation_source.as_deref(), Some("managed-delegation" | "managed-continuation"))) {
            turns.insert(r.id.clone(), serde_json::to_value(store.turns(&r.id)?)?);
        }
        // Oversight per top-level run (held, watched, watching, open conflicts, area) and Overseer's
        // own summary (the level, what waits for the owner), so every surface shows the same thing
        // from one state (AC-199).
        let mut oversight = serde_json::Map::new();
        {
            let mut holds = store.conn.prepare("SELECT run_id, reason FROM holds")?;
            let held: std::collections::HashMap<String, String> = holds.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?.flatten().collect();
            let mut watches = store.conn.prepare("SELECT subject, watcher, mode FROM watches WHERE ended_ms IS NULL")?;
            let watching: Vec<(String, String, String)> = watches.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)))?.flatten().collect();
            let mut conflicts = store.conn.prepare("SELECT run_a, run_b, kind FROM conflicts WHERE state='open'")?;
            let open: Vec<(String, Option<String>, String)> = conflicts.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?, r.get::<_, String>(2)?)))?.flatten().collect();
            let mut areas = store.conn.prepare("SELECT run_id, path FROM areas ORDER BY path")?;
            let mut area_of: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
            for (run, path) in areas.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?.flatten() {
                area_of.entry(run).or_default().push(path);
            }
            let mut roles = store.conn.prepare("SELECT run_id, role FROM run_roles")?;
            let role_of: std::collections::HashMap<String, String> = roles.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?.flatten().collect();
            for r in runs.iter().filter(|r| r.parent_run_id.is_none()) {
                let mine: Vec<&(String, Option<String>, String)> = open.iter().filter(|c| c.0 == r.id || c.1.as_deref() == Some(r.id.as_str())).collect();
                let watched_by: Vec<&str> = watching.iter().filter(|w| w.0 == r.id).map(|w| w.1.as_str()).collect();
                let watching_whom: Vec<&str> = watching.iter().filter(|w| w.1 == r.id).map(|w| w.0.as_str()).collect();
                let held_reason = held.get(&r.id);
                if held_reason.is_none() && watched_by.is_empty() && watching_whom.is_empty() && mine.is_empty() && !area_of.contains_key(&r.id) && !role_of.contains_key(&r.id) {
                    continue;
                }
                oversight.insert(r.id.clone(), json!({
                    "held": held_reason.is_some(), "hold_reason": held_reason,
                    "watched": !watched_by.is_empty(), "watchers": watched_by, "watching": watching_whom,
                    "conflicts": mine.len(), "needs_decision": mine.iter().any(|c| c.2 == "same_lines" || c.2 == "area_crossed"),
                    "area": area_of.get(&r.id).cloned().unwrap_or_default(), "role": role_of.get(&r.id).cloned().unwrap_or_else(|| "agent".into()),
                }));
            }
        }
        let overseer = {
            use rusqlite::OptionalExtension;
            let session: Option<(String, String, Option<String>)> = store.conn.query_row("SELECT id, level, run_id FROM overseer_sessions WHERE archived_ms IS NULL ORDER BY started_ms DESC LIMIT 1", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).optional()?;
            match session {
                Some((id, level, run_id)) => {
                    let open: i64 = store.conn.query_row("SELECT COUNT(*) FROM overseer_proposals WHERE session_id=?1 AND state='open'", [&id], |r| r.get(0))?;
                    let decisions: i64 = store.conn.query_row("SELECT COUNT(*) FROM conflicts WHERE state='open' AND kind IN ('same_lines', 'area_crossed')", [], |r| r.get(0))?;
                    let last: Option<i64> = store.conn.query_row("SELECT MAX(ts) FROM overseer_messages WHERE session_id=?1", [&id], |r| r.get::<_, Option<i64>>(0)).ok().flatten();
                    json!({"session": id, "level": level, "run_id": run_id, "open_proposals": open, "conflicts_needing_decision": decisions, "last_message_ms": last})
                }
                None => json!({"session": Value::Null, "level": "ask_first", "open_proposals": 0, "conflicts_needing_decision": 0}),
            }
        };
        Ok(json!({"cursor": store.max_seq()?, "tasks": tasks, "runs": run_values, "workspaces": workspaces, "profiles": store.profiles()?, "turns": turns, "oversight": oversight, "overseer": overseer,
            "daemon": {"pid": std::process::id(), "started_ms": self.started_ms, "version": env!("CARGO_PKG_VERSION"), "parser_version": adapters::PARSER_VERSION,
                "swarm_storage": if self.swarm_storage_blocked.load(std::sync::atomic::Ordering::SeqCst) { "blocked" } else { "ready" }}}))
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
    /// Backgrounded tasks reported since the main agent last called the model.
    unread_notices: usize,
    /// After an interim result, before Claude's next turn starts.
    between_turns: bool,
    backgrounded: std::collections::HashSet<String>,
}

impl Default for TailState {
    fn default() -> Self {
        Self { session: None, turn_done: None, last_error: None, since_prune: 0, store_polled: std::time::Instant::now(), store_seen: Default::default(), close_stdin: false, sends: Vec::new(), background: 0, expected_turns: 1, unread_notices: 0, between_turns: false, backgrounded: Default::default() }
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
            if matches!(child.relation_source.as_deref(), Some("managed-delegation" | "managed-continuation")) {
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
    match harness {
        "codex-app" => "codex",
        "opencode-serve" => "opencode",
        h => h,
    }
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
