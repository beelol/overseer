//! Continuity (Gate L): keep working when the connection drops.
//!
//! This module holds what the daemon knows and decides about the connection:
//! - the **settings** (`overseer.continuity.*`), owned and enforced here so they hold with VS Code
//!   closed (AC-88), and the one-time notice (AC-98);
//! - the **connection state**, online, degraded or offline, decided from the system's own answer,
//!   the probes and the agents' errors, so that an outage is never taken for being offline (AC-83);
//! - the protocol methods for all of it, and for the local inventory, pick and guard in `local.rs`.
//!
//! Design: docs/rfcs/offline-mode.md.

use crate::accounts::provider_of;
use crate::daemon::{now, Daemon, ACTIVE};
use crate::local::{self, PickOptions};
use crate::net::{self, Baseline, Probe, SystemAnswer, SystemNet};
use crate::sys::{self, GIB};
use anyhow::{anyhow, bail, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

// ------------------------------------------------------------------ settings

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct Settings {
    /// Continuity: fail over and go local on its own. Off: wait and retry.
    pub enabled: bool,
    /// Preference among working providers for failover. Providers with accounts that are not
    /// listed follow in the order their accounts were added; local is always last.
    pub provider_order: Vec<String>,
    pub allow_model_downloads: bool,
    pub allow_ollama_install: bool,
    /// Off until asked: keep the best-fitting eligible model downloaded while online.
    pub prefetch: bool,
    pub ram_ceiling_percent: u64,
    /// `None` is max(4 GiB, 10% of total).
    #[serde(rename = "ramHeadroomGiB")]
    pub ram_headroom_gib: Option<f64>,
    pub context_target: u64,
    pub context_floor: u64,
    pub preferred_models: Vec<String>,
    pub allow_unverified_models: bool,
    pub local_harness: String,
    pub return_online: String,
    pub retry_cap_seconds: u64,
    pub retry_for_hours: u64,
    pub stall_seconds: u64,
    pub probes: bool,
    pub ollama_idle_minutes: u64,
    pub registry: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: true,
            provider_order: vec!["openai".into(), "anthropic".into()],
            allow_model_downloads: false,
            allow_ollama_install: false,
            prefetch: false,
            ram_ceiling_percent: 40,
            ram_headroom_gib: None,
            context_target: 65536,
            context_floor: 16384,
            preferred_models: Vec::new(),
            allow_unverified_models: false,
            local_harness: "opencode".into(),
            return_online: "offer".into(),
            retry_cap_seconds: 120,
            retry_for_hours: 36,
            stall_seconds: 90,
            probes: true,
            ollama_idle_minutes: 30,
            registry: String::new(),
        }
    }
}

pub const KNOWN_PROVIDERS: &[&str] = &["openai", "anthropic"];

impl Settings {
    /// Every range the daemon enforces, with the reason a value is refused.
    pub fn validate(&self) -> Result<()> {
        let within = |name: &str, value: u64, low: u64, high: u64, unit: &str| -> Result<()> {
            if value < low || value > high {
                bail!("{name} must be between {low} and {high}{unit}; {value} was refused");
            }
            Ok(())
        };
        within("ramCeilingPercent", self.ram_ceiling_percent, 10, local::MAX_CEILING_PERCENT, " percent (one model never takes more than half of the memory)")?;
        if let Some(h) = self.ram_headroom_gib {
            if !(1.0..=1024.0).contains(&h) {
                bail!("ramHeadroomGiB must be between 1 and 1024; {h} was refused");
            }
        }
        within("contextFloor", self.context_floor, 8192, 262_144, " tokens")?;
        within("contextTarget", self.context_target, self.context_floor, 1_048_576, " tokens (it cannot be under the floor)")?;
        within("retryCapSeconds", self.retry_cap_seconds, 5, 3600, " seconds")?;
        within("retryForHours", self.retry_for_hours, 1, 36, " hours (the owner's limit is 36 hours)")?;
        within("stallSeconds", self.stall_seconds, 30, 3600, " seconds")?;
        within("ollamaIdleMinutes", self.ollama_idle_minutes, 1, 1440, " minutes")?;
        if !["opencode", "codex"].contains(&self.local_harness.as_str()) {
            bail!("localHarness must be opencode or codex; {:?} was refused", self.local_harness);
        }
        if !["offer", "auto", "stay"].contains(&self.return_online.as_str()) {
            bail!("returnOnline must be offer, auto or stay; {:?} was refused", self.return_online);
        }
        let mut seen = BTreeSet::new();
        for p in &self.provider_order {
            if p == "local" {
                bail!("providerOrder lists online providers; local is always last and is not listed");
            }
            if !p.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') || p.is_empty() {
                bail!("providerOrder has an entry that is not a provider id: {p:?}");
            }
            if !seen.insert(p) {
                bail!("providerOrder lists {p} twice");
            }
        }
        if !self.registry.is_empty() && !self.registry.starts_with("https://") {
            bail!("registry must be empty or an https address; {:?} was refused", self.registry);
        }
        for tag in &self.preferred_models {
            if tag.is_empty() || tag.len() > 120 || tag.contains(char::is_whitespace) {
                bail!("preferredModels has an entry that is not a model tag: {tag:?}");
            }
        }
        Ok(())
    }

    /// Lays `values` (the keys of `overseer.continuity.*`) over these settings and validates the
    /// result. An unknown key or an out-of-range value refuses the whole change.
    pub fn merged(&self, values: &Value) -> Result<Self> {
        let Some(obj) = values.as_object() else { bail!("settings must be an object of setting names and values") };
        let mut all = serde_json::to_value(self)?;
        for (k, v) in obj {
            if all.get(k).is_none() {
                bail!("{k} is not a Continuity setting");
            }
            all[k] = v.clone();
        }
        let next: Settings = serde_json::from_value(all).map_err(|e| anyhow!("a setting has the wrong type: {e}"))?;
        next.validate()?;
        Ok(next)
    }

    pub fn pick_options(&self, may_download: bool) -> PickOptions {
        PickOptions {
            ceiling_percent: self.ram_ceiling_percent,
            headroom: self.ram_headroom_gib.map(|g| (g * GIB as f64) as u64),
            context_target: self.context_target,
            context_floor: self.context_floor,
            preferred: self.preferred_models.clone(),
            allow_unverified: self.allow_unverified_models,
            harness: self.local_harness.clone(),
            may_download,
        }
    }
}

// ------------------------------------------------------------------ connection state

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Conn {
    Online,
    Degraded,
    Offline,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct Health {
    /// `None`: unknown (probes are off and no agent has reported).
    pub reachable: Option<bool>,
    pub reason: String,
    /// `probe`, `agents` or `unknown`.
    pub source: String,
}

/// What the agents themselves reported in the last two minutes.
#[derive(Default, Clone, Debug)]
pub struct Evidence {
    /// Providers that runs are using: active runs, and runs that reported a network error.
    pub in_use: BTreeSet<String>,
    /// Network-class errors per provider.
    pub network: BTreeMap<String, u64>,
    /// Of those, errors that say the provider itself is failing (5xx, 529, overloaded).
    pub outage: BTreeMap<String, u64>,
    /// Newest network-class error, to start a probe round at once.
    pub newest_ms: i64,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct Decision {
    pub state: Conn,
    pub reason: String,
    /// Providers that cannot be reached, in a stable order.
    pub unreachable: Vec<String>,
    /// Degraded with no provider left: the policy acts as if offline.
    pub acts_offline: bool,
    pub providers: BTreeMap<String, Health>,
}

pub fn provider_name(id: &str) -> String {
    match id {
        "openai" => "OpenAI".into(),
        "anthropic" => "Claude".into(),
        "local" => "Local".into(),
        other => other.to_string(),
    }
}

fn names(ids: &[String]) -> String {
    let mut n: Vec<String> = ids.iter().map(|i| provider_name(i)).collect();
    n.sort();
    match n.len() {
        0 => String::new(),
        1 => n.remove(0),
        _ => {
            let last = n.pop().unwrap();
            format!("{} and {last}", n.join(", "))
        }
    }
}

/// The decision rule of the RFC. Pure: the same inputs always give the same state.
pub fn decide(system: &SystemAnswer, baseline: Option<&Baseline>, probes: &BTreeMap<String, Probe>, probes_on: bool, agents: &Evidence) -> Decision {
    let mut providers: BTreeMap<String, Health> = BTreeMap::new();
    let unknown = || Health { reachable: None, reason: "not checked".into(), source: "unknown".into() };
    for id in KNOWN_PROVIDERS {
        providers.insert(id.to_string(), unknown());
    }
    if system.state == SystemNet::NoNetwork {
        return Decision { state: Conn::Offline, reason: "no network (system)".into(), unreachable: Vec::new(), acts_offline: true, providers };
    }
    if let Some(b) = baseline.filter(|b| !b.ok()) {
        return Decision { state: Conn::Offline, reason: format!("no working connection ({})", b.failure()), unreachable: Vec::new(), acts_offline: true, providers };
    }
    for id in KNOWN_PROVIDERS {
        let id = id.to_string();
        let outage = agents.outage.get(&id).copied().unwrap_or(0);
        let network = agents.network.get(&id).copied().unwrap_or(0);
        let health = match probes.get(&id).filter(|_| probes_on) {
            Some(p) if !p.ok => Health { reachable: Some(false), reason: p.reason.clone(), source: "probe".into() },
            // The hosts answer, but the agents keep getting 5xx or 529 from them: an outage.
            Some(_) if outage >= 2 => Health { reachable: Some(false), reason: "outage".into(), source: "agents".into() },
            Some(p) => Health { reachable: Some(true), reason: p.reason.clone(), source: "probe".into() },
            None if network >= 1 => Health { reachable: Some(false), reason: if outage >= 1 { "outage".into() } else { "connect".into() }, source: "agents".into() },
            None => unknown(),
        };
        providers.insert(id, health);
    }
    let unreachable: Vec<String> = providers.iter().filter(|(_, h)| h.reachable == Some(false)).map(|(k, _)| k.clone()).collect();
    if unreachable.is_empty() {
        return Decision { state: Conn::Online, reason: "connected".into(), unreachable, acts_offline: false, providers };
    }
    if !probes_on {
        // Without probes nothing confirms that the internet works, so every provider in use failing
        // on connection errors is read as the connection being gone.
        let connect_level = |id: &String| providers.get(id).is_some_and(|h| h.reachable == Some(false) && h.reason == "connect");
        if !agents.in_use.is_empty() && agents.in_use.iter().all(connect_level) {
            return Decision { state: Conn::Offline, reason: "all agents lost their connection".into(), unreachable, acts_offline: true, providers };
        }
    }
    let none_left = KNOWN_PROVIDERS.iter().all(|id| unreachable.iter().any(|u| u == id));
    Decision { state: Conn::Degraded, reason: format!("{} unreachable", names(&unreachable)), acts_offline: none_left, unreachable, providers }
}

#[derive(Serialize, Clone, Debug)]
pub struct Status {
    pub state: Conn,
    pub reason: String,
    pub unreachable: Vec<String>,
    pub acts_offline: bool,
    pub providers: BTreeMap<String, Health>,
    /// What the system itself said.
    pub system: SystemAnswer,
    pub baseline: Option<Baseline>,
    pub probes: bool,
    /// When this state began, and when it was last checked.
    pub since_ms: i64,
    pub checked_ms: i64,
    pub probed_ms: Option<i64>,
}

struct Shared {
    settings: Settings,
    status: Status,
    /// A decision seen once and waiting for its second reading.
    pending: Option<Decision>,
    baseline: Option<Baseline>,
    probes: BTreeMap<String, Probe>,
    last_probe: Option<Instant>,
    last_system: SystemNet,
    newest_error_ms: i64,
    force: bool,
}

static SHARED: OnceLock<Mutex<Shared>> = OnceLock::new();

fn shared() -> Result<std::sync::MutexGuard<'static, Shared>> {
    SHARED.get().ok_or_else(|| anyhow!("Continuity has not started"))?.lock().map_err(|_| anyhow!("Continuity state is poisoned"))
}

fn millis(var: &str, default: u64) -> Duration {
    Duration::from_millis(std::env::var(var).ok().and_then(|v| v.parse().ok()).unwrap_or(default))
}

// ------------------------------------------------------------------ store

fn meta_get(d: &Daemon, key: &str) -> Option<String> {
    use rusqlite::OptionalExtension;
    d.store.lock().unwrap().conn.query_row("SELECT value FROM meta WHERE key=?1", [key], |r| r.get::<_, String>(0)).optional().ok().flatten()
}

fn meta_set(d: &Daemon, key: &str, value: &str) -> Result<()> {
    d.store.lock().unwrap().conn.execute("INSERT INTO meta(key, value) VALUES(?1, ?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value", [key, value])?;
    Ok(())
}

fn load_settings(d: &Daemon) -> Settings {
    // A stored value that no longer validates (an older build's) falls back to the defaults.
    meta_get(d, "continuity.settings").and_then(|t| serde_json::from_str::<Settings>(&t).ok()).filter(|s| s.validate().is_ok()).unwrap_or_default()
}

pub fn settings() -> Settings {
    shared().map(|s| s.settings.clone()).unwrap_or_default()
}

pub fn status() -> Option<Status> {
    shared().ok().map(|s| s.status.clone())
}

/// What the agents reported in the last two minutes, and which providers are in use.
pub fn evidence(d: &Daemon) -> Evidence {
    let mut e = Evidence::default();
    let store = d.store.lock().unwrap();
    if let Ok(runs) = store.runs() {
        for r in runs.iter().filter(|r| r.parent_run_id.is_none() && ACTIVE.contains(&r.status.as_str())) {
            let p = provider_of(&r.harness);
            if KNOWN_PROVIDERS.contains(&p) {
                e.in_use.insert(p.to_string());
            }
        }
    }
    let since = now() - 120_000;
    if let Ok(mut stmt) = store.conn.prepare("SELECT r.harness, e.payload, e.ts FROM events e JOIN runs r ON r.id = e.run_id WHERE e.kind='error' AND e.ts > ?1 AND e.payload LIKE '%\"network\"%'") {
        let rows = stmt.query_map([since], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?)));
        for (harness, payload, ts) in rows.into_iter().flatten().flatten() {
            let v: Value = serde_json::from_str(&payload).unwrap_or(Value::Null);
            if v["class"] != "network" {
                continue;
            }
            let p = provider_of(&harness).to_string();
            // A provider whose run just failed was in use, even though the run has ended.
            if KNOWN_PROVIDERS.contains(&p.as_str()) {
                e.in_use.insert(p.clone());
            }
            *e.network.entry(p.clone()).or_default() += 1;
            if is_outage(v["message"].as_str().unwrap_or_default()) {
                *e.outage.entry(p).or_default() += 1;
            }
            e.newest_ms = e.newest_ms.max(ts);
        }
    }
    e
}

/// The provider answered, but with a failure of its own: 5xx, 529 or "overloaded".
pub fn is_outage(message: &str) -> bool {
    let m = message.to_ascii_lowercase();
    ["500 ", "502", "503", "504", "529", "overloaded", "internal server error", "bad gateway", "service unavailable", "gateway timeout"].iter().any(|s| m.contains(s))
}

// ------------------------------------------------------------------ the monitor

/// Probes can be switched off from outside (`OVERSEER_CONTINUITY_PROBES=off`), whatever the
/// setting says: the automated tests of the rest of the daemon never reach the network.
fn probes_allowed(settings: &Settings) -> bool {
    settings.probes && std::env::var("OVERSEER_CONTINUITY_PROBES").map(|v| v != "off").unwrap_or(true)
}

/// Starts Continuity: loads the settings, asks the system (which is quick and local), and watches
/// the connection for as long as the daemon runs. The probes, which take the network's time, run
/// from the watching thread, so the daemon never waits for them to start.
pub fn start(d: Arc<Daemon>) -> Result<()> {
    {
        let store = d.store.lock().unwrap();
        local::ensure_tables(&store.conn)?;
    }
    let settings = load_settings(&d);
    let system = net::system();
    let probes_on = probes_allowed(&settings);
    let first = decide(&system, None, &BTreeMap::new(), false, &evidence(&d));
    let at = now();
    let status = Status { state: first.state, reason: first.reason.clone(), unreachable: first.unreachable.clone(), acts_offline: first.acts_offline, providers: first.providers.clone(), system: system.clone(), baseline: None, probes: probes_on, since_ms: at, checked_ms: at, probed_ms: None };
    let shared = Shared { settings, status: status.clone(), pending: None, baseline: None, probes: BTreeMap::new(), last_probe: None, last_system: system.state, newest_error_ms: 0, force: true };
    if SHARED.set(Mutex::new(shared)).is_err() {
        bail!("Continuity was started twice");
    }
    crate::log(&format!("continuity: {:?} ({}); system: {}", status.state, status.reason, status.system.detail));
    d.emit(None, None, "connection", "daemon", "exact", json!({"status": status, "previous": Value::Null, "first": true}))?;
    std::thread::Builder::new().name("continuity".into()).spawn(move || {
        let mut first = true;
        loop {
            if !std::mem::take(&mut first) {
                std::thread::sleep(millis("OVERSEER_TEST_CONTINUITY_TICK_MS", 5000));
            }
            if let Err(e) = tick(&d) {
                crate::log(&format!("continuity: check failed: {e:#}"));
            }
        }
    })?;
    Ok(())
}

/// One check. The system is asked every time; the probes run when they are due: every 30 seconds
/// while runs are active or waiting, every 5 minutes otherwise, and at once when the system's
/// answer changes, an agent reports a network error, or a check is asked for.
pub fn tick(d: &Arc<Daemon>) -> Result<Status> {
    let system = net::system();
    let agents = evidence(d);
    let (probes_on, due) = {
        let s = shared()?;
        let busy = !agents.in_use.is_empty() || s.status.state != Conn::Online;
        let every = if busy { millis("OVERSEER_TEST_PROBE_MS", 30_000) } else { millis("OVERSEER_TEST_PROBE_IDLE_MS", 300_000) };
        let due = s.force || system.state != s.last_system || agents.newest_ms > s.newest_error_ms || s.last_probe.is_none_or(|t| t.elapsed() >= every);
        (probes_allowed(&s.settings), due)
    };
    let fresh = (probes_on && due && system.state != SystemNet::NoNetwork).then(net::probe_round);
    let mut s = shared()?;
    if let Some((baseline, probes)) = fresh {
        s.baseline = baseline;
        s.probes = probes;
        s.last_probe = Some(Instant::now());
        s.status.probed_ms = Some(now());
    }
    if !probes_on || system.state == SystemNet::NoNetwork {
        s.baseline = None;
        s.probes.clear();
        if system.state == SystemNet::NoNetwork {
            s.last_probe = None; // probe as soon as the system says connected again
        }
    }
    s.force = false;
    s.last_system = system.state;
    s.newest_error_ms = s.newest_error_ms.max(agents.newest_ms);
    let decision = decide(&system, s.baseline.as_ref(), &s.probes, probes_on, &agents);
    s.status.system = system.clone();
    s.status.baseline = s.baseline.clone();
    s.status.probes = probes_on;
    s.status.checked_ms = now();
    let same = s.status.state == decision.state && s.status.reason == decision.reason && s.status.unreachable == decision.unreachable;
    if same {
        s.pending = None;
        s.status.providers = decision.providers;
        return Ok(s.status.clone());
    }
    // A change needs two readings that agree, except the system's own "no network", which is
    // trusted at once.
    let immediate = system.state == SystemNet::NoNetwork;
    if !immediate && s.pending.as_ref() != Some(&decision) {
        s.pending = Some(decision);
        return Ok(s.status.clone());
    }
    let previous = json!({"state": s.status.state, "reason": s.status.reason, "since_ms": s.status.since_ms});
    s.pending = None;
    if s.status.state != decision.state {
        s.status.since_ms = now();
    }
    s.status.state = decision.state;
    s.status.reason = decision.reason;
    s.status.unreachable = decision.unreachable;
    s.status.acts_offline = decision.acts_offline;
    s.status.providers = decision.providers;
    let status = s.status.clone();
    drop(s);
    crate::log(&format!("continuity: {:?} ({}); system: {}", status.state, status.reason, status.system.detail));
    d.emit(None, None, "connection", "daemon", "exact", json!({"status": status, "previous": previous}))?;
    Ok(status)
}

// ------------------------------------------------------------------ local models

/// A model may be downloaded when the setting allows it, the registry can be reached, and Ollama
/// is there to receive it (running, or allowed to be installed).
fn may_download(settings: &Settings, ollama: &local::Ollama) -> bool {
    settings.allow_model_downloads && status().is_some_and(|s| s.state != Conn::Offline) && (ollama.running || settings.allow_ollama_install)
}

struct Local {
    inventory: local::Inventory,
    catalogue: Vec<local::Entry>,
    measured: local::Measured,
}

/// The inventory, the catalogue and the measured sizes; what Ollama reports for loaded models is
/// recorded on the way.
fn gather(d: &Daemon) -> Local {
    let inventory = local::inventory();
    let store = d.store.lock().unwrap();
    if let Ok(new) = local::record_measured(&store.conn, &inventory.models, &inventory.loaded, inventory.ollama.version.as_deref(), now()) {
        for (tag, ctx, bytes) in new {
            crate::log(&format!("continuity: measured {tag} at a {ctx}-token context: {} GiB", sys::gib(bytes)));
        }
    }
    let measured = local::measured(&store.conn);
    Local { inventory, catalogue: local::catalogue(), measured }
}

fn pick(d: &Daemon) -> Result<Value> {
    let settings = settings();
    let l = gather(d);
    let memory = l.inventory.memory.clone().ok_or_else(|| anyhow!("memory cannot be read ({}), so no model is picked", l.inventory.memory_error.clone().unwrap_or_default()))?;
    let opts = settings.pick_options(may_download(&settings, &l.inventory.ollama));
    let pick = local::pick(&memory, &l.inventory.models, &l.inventory.loaded, &l.catalogue, &l.measured, &opts);
    Ok(json!({"pick": pick, "ollama": l.inventory.ollama, "harness": settings.local_harness, "may_download": opts.may_download, "context_target": opts.context_target, "context_floor": opts.context_floor}))
}

/// The guard every load passes (AC-140): fresh memory, Ollama's own list of loaded models, no
/// override. Errors say exactly why a model may not be loaded.
pub fn approve(d: &Daemon, tag: &str, context: u64) -> Result<Value> {
    let settings = settings();
    let l = gather(d);
    if !l.inventory.ollama.running {
        bail!("{}; nothing can be loaded", l.inventory.ollama.detail);
    }
    local::approve(tag, context, &l.inventory.models, &l.inventory.loaded, &l.catalogue, &l.measured, &settings.pick_options(false))
}

/// Loads a model under the guard: approved on fresh numbers, its context set through a tag that
/// shares the weights, and watched while it loads. What Ollama then measures is recorded.
fn load(d: &Daemon, tag: &str, context: u64) -> Result<Value> {
    let settings = settings();
    let approved = approve(d, tag, context)?;
    let base = approved["base"].as_str().unwrap_or(tag).to_string();
    let inventory = local::inventory();
    let run_tag = local::ensure_tag(&inventory.models, tag, &base, context)?;
    let memory = sys::memory()?;
    let headroom = local::headroom(memory.total, settings.pick_options(false).headroom);
    let before = json!({"available": memory.available, "pressure": memory.pressure});
    let loaded = local::load_guarded(&run_tag, context, headroom, &format!("{}m", settings.ollama_idle_minutes));
    let after = sys::memory().ok().map(|m| json!({"available": m.available, "pressure": m.pressure}));
    let measured = gather(d).inventory.loaded.into_iter().find(|l| l.tag == run_tag);
    let outcome = json!({"tag": tag, "run_tag": run_tag, "context": context, "approved": approved, "memory_before": before, "memory_after": after, "measured": measured, "error": loaded.as_ref().err().map(|e| e.to_string())});
    d.emit(None, None, "local_load", "daemon", "exact", outcome.clone())?;
    loaded.map(|l| json!({"loaded": l, "detail": outcome}))
}

/// Before a local turn is launched: the model is chosen if none was, given a context, passed
/// through the guard, loaded under the watchdog, and named in Overseer's own OpenCode profile.
/// A refusal is an error with its reason, and nothing is launched.
pub fn prepare_local_run(d: &Daemon, run: &mut crate::store::Run, profile_env: &BTreeMap<String, String>) -> Result<()> {
    let config_home = std::path::PathBuf::from(crate::opencode_bridge::env_for(profile_env)?);
    let settings = settings();
    let l = gather(d);
    if !l.inventory.ollama.running {
        bail!("{}; a local model cannot run", l.inventory.ollama.detail);
    }
    let opts = settings.pick_options(false);
    // The model: the one asked for, or the pick.
    let (tag, base, context) = match run.model.as_deref().map(crate::opencode_bridge::tag_of).filter(|t| !t.is_empty()) {
        Some(tag) => {
            let model = l.inventory.models.iter().find(|m| m.tag == tag).ok_or_else(|| anyhow!("{tag} is not installed in Ollama"))?;
            let context = match model.configured_context {
                Some(c) => c,
                // A tag that sets no context gets the longest one that fits.
                None => local::context_steps(opts.context_target, opts.context_floor, model.max_context.unwrap_or(local::COMFORTABLE_CONTEXT))
                    .into_iter()
                    .find(|c| local::approve(tag, *c, &l.inventory.models, &l.inventory.loaded, &l.catalogue, &l.measured, &opts).is_ok())
                    .ok_or_else(|| local::approve(tag, opts.context_floor, &l.inventory.models, &l.inventory.loaded, &l.catalogue, &l.measured, &opts).unwrap_err())?,
            };
            (tag.to_string(), model.base.clone(), context)
        }
        None => {
            let memory = l.inventory.memory.clone().ok_or_else(|| anyhow!("memory cannot be read, so no model is picked"))?;
            let pick = local::pick(&memory, &l.inventory.models, &l.inventory.loaded, &l.catalogue, &l.measured, &opts);
            let c = pick.chosen.ok_or_else(|| anyhow!("no local model that is installed and verified fits the memory budget of {} GiB{}", sys::gib(pick.budget.budget), pick.rejected.first().map(|r| format!(" ({}: {})", r.tag, r.reason)).unwrap_or_default()))?;
            (c.tag.clone(), c.tag, c.context)
        }
    };
    let approved = local::approve(&tag, context, &l.inventory.models, &l.inventory.loaded, &l.catalogue, &l.measured, &opts)?;
    let run_tag = local::ensure_tag(&l.inventory.models, &tag, &base, context)?;
    let already = l.inventory.loaded.iter().any(|x| x.tag == run_tag && x.context.unwrap_or(0) >= context);
    if !already {
        let memory = sys::memory()?;
        let loaded = local::load_guarded(&run_tag, context, local::headroom(memory.total, opts.headroom), &format!("{}m", settings.ollama_idle_minutes));
        let after = gather(d);
        d.emit(Some(&run.task_id), Some(&run.id), "local_load", "daemon", "exact", json!({"tag": tag, "run_tag": run_tag, "context": context, "approved": approved, "memory_before": {"available": memory.available, "pressure": memory.pressure}, "memory_after": after.inventory.memory.as_ref().map(|m| json!({"available": m.available, "pressure": m.pressure})), "measured": after.inventory.loaded.iter().find(|x| x.tag == run_tag), "error": loaded.as_ref().err().map(|e| e.to_string())}))?;
        loaded?;
    }
    let mut tags: Vec<String> = l.inventory.models.iter().filter(|m| m.capabilities.iter().any(|c| c == "tools")).map(|m| m.tag.clone()).collect();
    if !tags.contains(&run_tag) {
        tags.push(run_tag.clone());
    }
    crate::opencode_bridge::write_config(&config_home, &tags, &run_tag, &local::ollama_url()?)?;
    let model = format!("ollama/{run_tag}");
    if run.model.as_deref() != Some(model.as_str()) {
        d.store.lock().unwrap().conn.execute("UPDATE runs SET model=?2 WHERE id=?1", rusqlite::params![run.id, model])?;
        run.model = Some(model.clone());
    }
    d.emit(Some(&run.task_id), Some(&run.id), "local_model", "daemon", "exact", json!({"model": model, "base": base, "context": context, "bytes": approved["bytes"], "measured": approved["measured"], "already_loaded": already, "budget": approved["budget"]}))?;
    Ok(())
}

// ------------------------------------------------------------------ protocol

pub fn handles(method: &str) -> bool {
    matches!(method, "connection.status" | "connection.check" | "continuity.status" | "continuity.notice" | "settings.get" | "settings.set" | "local.inventory" | "local.pick" | "local.approve" | "local.catalogue" | "local.load" | "local.unload")
}

pub fn dispatch(d: &Arc<Daemon>, method: &str, p: &Value) -> Result<Value> {
    Ok(match method {
        "connection.status" => json!({"status": status(), "enabled": settings().enabled}),
        "connection.check" => {
            shared()?.force = true;
            // Two readings, so a real change is applied before the answer is given.
            tick(d)?;
            json!({"status": tick(d)?, "enabled": settings().enabled})
        }
        "settings.get" => json!({"settings": settings(), "defaults": Settings::default()}),
        "settings.set" => {
            let next = settings().merged(&p["values"])?;
            meta_set(d, "continuity.settings", &serde_json::to_string(&next)?)?;
            let before = std::mem::replace(&mut shared()?.settings, next.clone());
            if before != next {
                shared()?.force = true;
                d.emit(None, None, "continuity_settings", "user", "exact", json!({"settings": next}))?;
            }
            json!({"settings": next})
        }
        "continuity.notice" => {
            if p["dismiss"].as_bool().unwrap_or(false) {
                meta_set(d, "continuity.notice_shown", &now().to_string())?;
            }
            let shown = meta_get(d, "continuity.notice_shown").and_then(|v| v.parse::<i64>().ok());
            let s = settings();
            json!({"show": s.enabled && shown.is_none(), "shown_ms": shown, "enabled": s.enabled, "allow_model_downloads": s.allow_model_downloads, "allow_ollama_install": s.allow_ollama_install})
        }
        "local.inventory" => serde_json::to_value(gather(d).inventory)?,
        "local.pick" => pick(d)?,
        "local.approve" => approve(d, p["tag"].as_str().ok_or_else(|| anyhow!("missing string parameter tag"))?, p["context"].as_u64().ok_or_else(|| anyhow!("missing number parameter context"))?)?,
        "local.catalogue" => json!({"models": local::catalogue(), "harness": settings().local_harness}),
        "local.load" => load(d, p["tag"].as_str().ok_or_else(|| anyhow!("missing string parameter tag"))?, p["context"].as_u64().ok_or_else(|| anyhow!("missing number parameter context"))?)?,
        "local.unload" => {
            let tag = p["tag"].as_str().ok_or_else(|| anyhow!("missing string parameter tag"))?;
            local::unload(tag)?;
            d.emit(None, None, "local_load", "daemon", "exact", json!({"tag": tag, "unloaded": true}))?;
            json!({"unloaded": tag})
        }
        "continuity.status" => {
            let s = settings();
            let shown = meta_get(d, "continuity.notice_shown").is_some();
            let memory = sys::memory();
            let budget = memory.as_ref().ok().map(|m| local::budget(m, &s.pick_options(false), 0));
            let picked = pick(d);
            json!({
                "connection": status(),
                "settings": s,
                "notice_shown": shown,
                "memory": memory.as_ref().ok(),
                "memory_error": memory.as_ref().err().map(|e| e.to_string()),
                "budget": budget,
                "pick": picked.as_ref().ok().map(|v| v["pick"]["chosen"].clone()),
                "pick_error": picked.as_ref().err().map(|e| e.to_string()),
                "rejected": picked.as_ref().ok().map(|v| v["pick"]["rejected"].clone()),
                "ollama": picked.as_ref().ok().map(|v| v["ollama"].clone()),
            })
        }
        other => bail!("unknown method {other}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sys(state: SystemNet) -> SystemAnswer {
        SystemAnswer { state, detail: "test".into() }
    }
    fn ok() -> Probe {
        Probe { ok: true, reason: "answered 200".into() }
    }
    fn fail(reason: &str) -> Probe {
        Probe { ok: false, reason: reason.into() }
    }
    fn baseline(name: Probe, ip: Probe) -> Baseline {
        Baseline { by_name: name, by_ip: ip }
    }
    fn probes(openai: Probe, anthropic: Probe) -> BTreeMap<String, Probe> {
        BTreeMap::from([("openai".to_string(), openai), ("anthropic".to_string(), anthropic)])
    }
    fn using(providers: &[&str]) -> Evidence {
        Evidence { in_use: providers.iter().map(|p| p.to_string()).collect(), ..Default::default() }
    }

    #[test]
    fn the_decision_table() {
        let up = baseline(ok(), ok());
        let none = Evidence::default();
        // The system says there is no network: offline at once, no probe needed.
        let d = decide(&sys(SystemNet::NoNetwork), None, &BTreeMap::new(), true, &none);
        assert_eq!((d.state, d.reason.as_str(), d.acts_offline), (Conn::Offline, "no network (system)", true));
        // Connected, but the baseline fails: offline, and the reason says which way.
        let d = decide(&sys(SystemNet::Connected), Some(&baseline(fail("dns"), ok())), &probes(fail("dns"), fail("dns")), true, &none);
        assert_eq!((d.state, d.reason.as_str()), (Conn::Offline, "no working connection (DNS is not answering)"));
        let d = decide(&sys(SystemNet::Connected), Some(&baseline(fail("tls"), fail("tls"))), &BTreeMap::new(), true, &none);
        assert_eq!(d.reason, "no working connection (captive portal)");
        let d = decide(&sys(SystemNet::Connected), Some(&baseline(fail("connect"), fail("timeout"))), &BTreeMap::new(), true, &none);
        assert_eq!(d.reason, "no working connection (no route to the internet)");
        // One provider fails: degraded, and it is named.
        let d = decide(&sys(SystemNet::Connected), Some(&up), &probes(fail("connect"), ok()), true, &none);
        assert_eq!((d.state, d.reason.as_str(), d.unreachable.clone(), d.acts_offline), (Conn::Degraded, "OpenAI unreachable", vec!["openai".to_string()], false));
        assert_eq!(d.providers["anthropic"].reachable, Some(true));
        // Every provider fails while the internet works: degraded, and the policy acts as offline.
        let d = decide(&sys(SystemNet::Connected), Some(&up), &probes(fail("connect"), fail("timeout")), true, &none);
        assert_eq!((d.state, d.reason.as_str(), d.acts_offline), (Conn::Degraded, "Claude and OpenAI unreachable", true));
        // Everything answers.
        let d = decide(&sys(SystemNet::Connected), Some(&up), &probes(ok(), ok()), true, &none);
        assert_eq!((d.state, d.reason.as_str()), (Conn::Online, "connected"));
        // An unknown system answer does not decide anything by itself.
        assert_eq!(decide(&sys(SystemNet::Unknown), Some(&up), &probes(ok(), ok()), true, &none).state, Conn::Online);
    }

    #[test]
    fn the_agents_are_evidence_too() {
        let up = baseline(ok(), ok());
        // The hosts answer, but the agents keep getting 529 from one provider: an outage.
        let mut e = using(&["anthropic"]);
        e.network.insert("anthropic".into(), 2);
        e.outage.insert("anthropic".into(), 2);
        let d = decide(&sys(SystemNet::Connected), Some(&up), &probes(ok(), ok()), true, &e);
        assert_eq!((d.state, d.reason.as_str()), (Conn::Degraded, "Claude unreachable"));
        assert_eq!(d.providers["anthropic"], Health { reachable: Some(false), reason: "outage".into(), source: "agents".into() });
        // One such error is not yet an outage.
        e.outage.insert("anthropic".into(), 1);
        assert_eq!(decide(&sys(SystemNet::Connected), Some(&up), &probes(ok(), ok()), true, &e).state, Conn::Online);
        // Probes off: one provider's agents failing is degraded.
        let mut e = using(&["openai", "anthropic"]);
        e.network.insert("openai".into(), 1);
        let d = decide(&sys(SystemNet::Connected), None, &BTreeMap::new(), false, &e);
        assert_eq!((d.state, d.reason.as_str()), (Conn::Degraded, "OpenAI unreachable"));
        assert_eq!(d.providers["anthropic"].reachable, None, "unknown stays unknown");
        // Probes off and every provider in use fails on connection errors: offline.
        e.network.insert("anthropic".into(), 3);
        let d = decide(&sys(SystemNet::Connected), None, &BTreeMap::new(), false, &e);
        assert_eq!((d.state, d.reason.as_str()), (Conn::Offline, "all agents lost their connection"));
        // The same with a provider that answers with failures of its own is an outage, not offline.
        e.outage.insert("anthropic".into(), 3);
        e.outage.insert("openai".into(), 1);
        assert_eq!(decide(&sys(SystemNet::Connected), None, &BTreeMap::new(), false, &e).state, Conn::Degraded);
        assert!(is_outage("API Error: 529 {\"type\":\"overloaded_error\"}"));
        assert!(is_outage("unexpected status 503 Service Unavailable"));
        assert!(!is_outage("getaddrinfo ENOTFOUND api.anthropic.com"));
    }

    #[test]
    fn settings_defaults_are_the_owners_decisions() {
        let s = Settings::default();
        assert!(s.enabled, "Continuity is on by default");
        assert_eq!(s.provider_order, ["openai", "anthropic"]);
        assert!(!s.allow_model_downloads && !s.allow_ollama_install && !s.prefetch, "downloads, the install and prefetch are off until asked");
        assert_eq!((s.ram_ceiling_percent, s.retry_for_hours, s.stall_seconds, s.context_target, s.context_floor), (40, 36, 90, 65536, 16384));
        assert_eq!((s.return_online.as_str(), s.local_harness.as_str()), ("offer", "opencode"));
        s.validate().unwrap();
        let keys: Vec<String> = serde_json::to_value(&s).unwrap().as_object().unwrap().keys().cloned().collect();
        assert_eq!(keys.len(), 19);
        assert!(keys.contains(&"ramCeilingPercent".to_string()) && keys.contains(&"allowModelDownloads".to_string()));
    }

    #[test]
    fn out_of_range_settings_are_refused_with_the_reason() {
        let s = Settings::default();
        let refused = |values: Value| s.merged(&values).unwrap_err().to_string();
        assert_eq!(refused(json!({"ramCeilingPercent": 60})), "ramCeilingPercent must be between 10 and 50 percent (one model never takes more than half of the memory); 60 was refused");
        assert!(refused(json!({"retryForHours": 48})).contains("the owner's limit is 36 hours"));
        assert!(refused(json!({"contextFloor": 32768, "contextTarget": 16384})).contains("it cannot be under the floor"));
        assert!(refused(json!({"returnOnline": "sometimes"})).contains("offer, auto or stay"));
        assert!(refused(json!({"localHarness": "claude"})).contains("opencode or codex"));
        assert!(refused(json!({"providerOrder": ["openai", "local"]})).contains("local is always last"));
        assert!(refused(json!({"providerOrder": ["openai", "openai"]})).contains("twice"));
        assert!(refused(json!({"registry": "http://mirror.example"})).contains("https"));
        assert!(refused(json!({"ramHeadroomGiB": 0.5})).contains("between 1 and 1024"));
        assert!(refused(json!({"stallSeconds": 5})).contains("between 30 and 3600"));
        assert_eq!(refused(json!({"turbo": true})), "turbo is not a Continuity setting");
        assert!(refused(json!({"enabled": "yes"})).contains("wrong type"));
        assert!(refused(json!([1, 2])).contains("must be an object"));
        // A refused change changes nothing; an accepted one changes only what was named.
        let next = s.merged(&json!({"ramCeilingPercent": 50, "enabled": false, "preferredModels": ["qwen3-coder:30b"], "ramHeadroomGiB": 8})).unwrap();
        assert_eq!((next.ram_ceiling_percent, next.enabled, next.ram_headroom_gib, next.retry_for_hours), (50, false, Some(8.0), 36));
        assert_eq!(next.pick_options(false).headroom, Some(8 * GIB));
        assert_eq!(s.merged(&json!({"ramHeadroomGiB": null})).unwrap().ram_headroom_gib, None);
    }

    #[test]
    fn providers_are_named_for_people() {
        assert_eq!(names(&["openai".into()]), "OpenAI");
        assert_eq!(names(&["openai".into(), "anthropic".into()]), "Claude and OpenAI");
    }
}
