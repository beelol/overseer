//! Versioned local protocol: newline-delimited JSON over an owner-only Unix socket.
//! Requests: {"id": n, "method": "...", "params": {...}}.
//! Responses: {"id": n, "result": ...} or {"id": n, "error": {"code", "message"}}.
//! Notifications: {"method": "event", "params": Event} and {"method": "resync", ...}.

use crate::daemon::{Daemon, ACTIVE};
use crate::store::Store;
use crate::paths;
use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};
use std::os::unix::io::AsRawFd;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock, TryLockError};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::mpsc;

pub const PROTOCOL_VERSION: i64 = 1;
pub const MAX_REQUEST_BYTES: u64 = 1024 * 1024;

pub async fn serve(daemon: Arc<Daemon>) -> Result<()> {
    crate::audio::start(daemon.clone())?;
    crate::overseer::conflicts::start(daemon.clone());
    crate::overseer::session::start(daemon.clone());
    let path = paths::socket_path();
    if let Some(dir) = path.parent() {
        paths::ensure_private_dir(dir)?;
    }
    if path.exists() {
        if UnixStream::connect(&path).await.is_ok() {
            return Err(anyhow!("another overseerd is already listening on {}", path.display()));
        }
        std::fs::remove_file(&path)?;
    }
    let listener = UnixListener::bind(&path)?;
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    }
    crate::log(&format!("listening on {}", path.display()));
    let deadline_daemon = daemon.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            let daemon = deadline_daemon.clone();
            match tokio::task::spawn_blocking(move || {
                crate::swarm::reconcile_control_verifications(&mut daemon.store.lock().unwrap())?;
                if let Ok(_serial)=daemon.swarm_integration_lock.try_lock() {
                    let mut store=crate::store::Store::connect_existing(&paths::db_path())?;
                    crate::swarm::reconcile_invalidated_integrations(&mut store)?;
                }
                crate::swarm::refresh_linked_director_owners(&daemon)?;
                crate::swarm::expire_director_owners(&mut daemon.store.lock().unwrap())?;
                let (expired, timed_out_workers, redirect_timeouts) = {
                    let _serial = daemon.swarm_launch_lock.lock().unwrap();
                    let mut store = daemon.store.lock().unwrap();
                    let now = crate::daemon::now();
                    (crate::swarm::expire_due(&mut store, now)?,
                     crate::swarm::expire_jobs_due(&mut store, now)?,
                     crate::swarm::expire_redirects_due(&mut store, now)?)
                };
                // State is committed before control sockets are contacted. A slow
                // or unreachable worker must not hold the launch lock while the
                // timer interrupts, reconciles, or samples processes.
                for run in expired {
                    crate::swarm::interrupt_workers(&daemon, &run)?;
                }
                for worker in timed_out_workers {
                    if let Err(error) = daemon.interrupt(&worker) {
                        crate::log(&format!("swarm job deadline interrupt {worker} failed: {error}"));
                    }
                }
                if let Some(workers) = redirect_timeouts["interrupt_pending"].as_array() {
                    for worker in workers {
                        if let Some(worker) = worker.as_str() {
                            if let Err(error) = daemon.interrupt(worker) {
                                crate::log(&format!("swarm redirect interrupt {worker} failed: {error}"));
                            }
                        }
                    }
                }
                crate::swarm::retry_revoked_interrupts(&daemon)?;
                crate::swarm::retry_stopping_interrupts(&daemon)?;
                crate::swarm::retry_targeted_interrupts(&daemon)?;
                crate::swarm::reconcile_terminal_workers(&daemon)?;
                crate::swarm::reconcile_stopping_runs(&daemon)?;
                crate::swarm::sample_due_workers(&daemon, crate::daemon::now())?;
                Ok::<(), anyhow::Error>(())
            })
            .await
            {
                Ok(Ok(_)) => {}
                Ok(Err(error)) => crate::log(&format!("swarm deadline check failed: {error}")),
                Err(error) => crate::log(&format!("swarm deadline task failed: {error}")),
            }
        }
    });
    let uid = unsafe { libc::getuid() };
    // Test-only: pretend the owner is another uid. It can only reject more peers (a peer must
    // still be this process's own uid), so it lets a test observe a "foreign" connection being
    // refused without a second macOS account; it can never admit a different user.
    let expected = std::env::var("OVERSEER_TEST_EXPECT_UID").ok().and_then(|v| v.parse::<u32>().ok());
    crate::auto_maintenance::start(daemon.clone());
    loop {
        let (stream, _) = listener.accept().await?;
        let peer = crate::shim::peer_uid_fd(stream.as_raw_fd());
        if peer != Some(uid) || expected.is_some_and(|e| peer != Some(e)) {
            crate::log(&format!("rejected connection from uid {peer:?}"));
            drop(stream);
            continue;
        }
        let daemon = daemon.clone();
        tokio::spawn(async move {
            if let Err(e) = connection(daemon, stream).await {
                crate::log(&format!("connection ended: {e}"));
            }
        });
    }
}

async fn connection(daemon: Arc<Daemon>, stream: UnixStream) -> Result<()> {
    let (read, mut write) = stream.into_split();
    let (tx, mut rx) = mpsc::channel::<Value>(1024);
    let writer = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            let mut text = msg.to_string();
            text.push('\n');
            if write.write_all(text.as_bytes()).await.is_err() {
                break;
            }
        }
    });
    let mut reader = BufReader::new(read);
    let mut buf = Vec::new();
    let mut ui = false;
    let result = connection_loop(&daemon, &mut reader, &mut buf, &tx, &mut ui).await;
    if ui {
        daemon.ui_disconnected();
    }
    drop(tx);
    let _ = writer.await;
    result
}

async fn connection_loop(
    daemon: &Arc<Daemon>,
    reader: &mut BufReader<tokio::net::unix::OwnedReadHalf>,
    buf: &mut Vec<u8>,
    tx: &mpsc::Sender<Value>,
    ui: &mut bool,
) -> Result<()> {
    loop {
        buf.clear();
        let n = (&mut *reader).take(MAX_REQUEST_BYTES + 1).read_until(b'\n', buf).await?;
        if n == 0 {
            break;
        }
        if buf.len() as u64 > MAX_REQUEST_BYTES {
            let _ = tx.send(json!({"id": null, "error": {"code": "request_too_large", "message": "request exceeds 1 MiB"}})).await;
            break;
        }
        let msg: Value = match serde_json::from_slice(buf) {
            Ok(v) => v,
            Err(e) => {
                let _ = tx.send(json!({"id": null, "error": {"code": "parse_error", "message": e.to_string()}})).await;
                continue;
            }
        };
        let id = msg.get("id").cloned().unwrap_or(Value::Null);
        let Some(method) = msg.get("method").and_then(|m| m.as_str()).map(str::to_string) else {
            let _ = tx.send(json!({"id": id, "error": {"code": "invalid_request", "message": "method must be a string"}})).await;
            continue;
        };
        let params = msg.get("params").cloned().unwrap_or(json!({}));
        if !params.is_object() {
            let _ = tx.send(json!({"id": id, "error": {"code": "invalid_params", "message": "params must be an object"}})).await;
            continue;
        }
        if method == "events.subscribe" {
            subscribe(daemon.clone(), id, params, tx.clone());
            continue;
        }
        if method == "hello" && (params["client"] == "vscode" || params["client"] == "tui") && !*ui {
            // A VS Code window or an overseer-tui: counted so closing the last one can surface
            // background agents.
            *ui = true;
            daemon.ui_connected();
        }
        let daemon = daemon.clone();
        let tx = tx.clone();
        tokio::spawn(async move {
            let shutdown = method == "daemon.shutdown" || method == "daemon.stop_all";
            let result = {
                let daemon = daemon.clone();
                let method = method.clone();
                tokio::task::spawn_blocking(move || dispatch(&daemon, &method, &params)).await
            };
            let reply = match result {
                Ok(Ok(v)) => json!({"id": id, "result": v}),
                Ok(Err(e)) => {
                    if let Some(limit) = e.downcast_ref::<crate::daemon::AgentLimitError>() {
                        json!({"id": id, "error": {"code": "agent_limit", "message": e.to_string(),
                            "active": limit.active, "limit": limit.limit,
                            "running_agents": limit.running_agents}})
                    } else {
                        json!({"id": id, "error": {"code": "failed", "message": e.to_string()}})
                    }
                },
                Err(e) => json!({"id": id, "error": {"code": "internal", "message": e.to_string()}}),
            };
            let _ = tx.send(reply).await;
            if shutdown {
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                crate::log("shutdown requested");
                std::process::exit(0);
            }
        });
    }
    Ok(())
}

/// Replay retained events after the client's cursor, then stream live events.
/// Subscribing to the broadcast before replaying avoids a gap; live events at or
/// below the replayed cursor are dropped so nothing is delivered twice.
fn subscribe(daemon: Arc<Daemon>, id: Value, params: Value, tx: mpsc::Sender<Value>) {
    let mut live = daemon.events.subscribe();
    tokio::spawn(async move {
        let mut cursor = params["after"].as_i64().unwrap_or(0);
        let run_filter = params["run_id"].as_str().map(str::to_string);
        let oldest = {
            let store = daemon.store.lock().unwrap();
            store.conn.query_row("SELECT MIN(seq) FROM events", [], |r| r.get::<_, Option<i64>>(0)).ok().flatten()
        };
        let gap = matches!(oldest, Some(o) if o > cursor + 1 && cursor > 0);
        if tx.send(json!({"id": id, "result": {"subscribed": true, "after": cursor, "history_truncated": gap}})).await.is_err() {
            return;
        }
        loop {
            let batch = {
                let store = daemon.store.lock().unwrap();
                store.events_after(cursor, run_filter.as_deref(), 1000).unwrap_or_default()
            };
            if batch.is_empty() {
                break;
            }
            for e in batch {
                cursor = e.seq;
                if tx.send(json!({"method": "event", "params": e})).await.is_err() {
                    return;
                }
            }
        }
        let _ = tx.send(json!({"method": "replayed", "params": {"cursor": cursor}})).await;
        loop {
            match live.recv().await {
                Ok(e) => {
                    if e.seq <= cursor {
                        continue;
                    }
                    if let Some(r) = &run_filter {
                        if e.run_id.as_deref() != Some(r) {
                            continue;
                        }
                    }
                    cursor = e.seq;
                    if tx.send(json!({"method": "event", "params": e})).await.is_err() {
                        return;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                    // Fall back to the durable log: tell the client to resume from its cursor.
                    let _ = tx.send(json!({"method": "resync", "params": {"cursor": cursor}})).await;
                    return;
                }
                Err(_) => return,
            }
        }
    });
}

fn maintain_visible_learning(d: &Daemon, store: &Store) -> bool {
    let ready = store.prune_auto_learning_history(crate::daemon::now()).is_ok();
    d.learning_maintenance_paused.store(!ready, std::sync::atomic::Ordering::Relaxed);
    ready
}

fn s<'a>(p: &'a Value, key: &str) -> Result<&'a str> {
    p[key].as_str().ok_or_else(|| anyhow!("missing string parameter {key}"))
}

fn auto_bridge_parent(d: &Arc<Daemon>, p: &Value) -> Result<(crate::store::Run, Value)> {
    use sha2::{Digest, Sha256};
    let run = d.run(s(p, "run_id")?)?;
    if run.parent_run_id.is_some() || run.process_generation < 1 {
        return Err(anyhow!("Auto bridge requires a launched top-level parent"));
    }
    let capability = s(p, "capability")?;
    if capability.len() != 64 || !capability.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(anyhow!("invalid Auto bridge capability"));
    }
    let launch: Option<String> = d.store.lock().unwrap().conn.query_row(
        "SELECT launch FROM runs WHERE id=?1", [&run.id], |row| row.get(0))?;
    let launch: Value = launch.as_deref().and_then(|value| serde_json::from_str(value).ok())
        .unwrap_or(Value::Null);
    let generic = launch.get("generic").unwrap_or(&launch);
    let actual_hash = format!("{:x}", Sha256::digest(capability.as_bytes()));
    if generic["auto_routing"] != true
        || generic["auto_bridge_generation"].as_i64() != Some(run.process_generation)
        || generic["auto_bridge_hash"].as_str() != Some(actual_hash.as_str()) {
        return Err(anyhow!("Auto bridge capability is not bound to this run"));
    }
    Ok((run, generic.clone()))
}

fn metadata_deadline(p: &Value) -> Result<Instant> {
    let ms = match p.get("timeout_ms") {
        None => 5000,
        Some(value) => value.as_u64().filter(|ms| (1..=5000).contains(ms))
            .ok_or_else(|| anyhow!("timeout_ms must be between 1 and 5000"))?,
    };
    Ok(Instant::now() + Duration::from_millis(ms))
}

fn remaining_metadata_ms(deadline: Instant) -> Result<u64> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    let ms = remaining.as_millis().min(5000) as u64;
    if ms < 20 { return Err(anyhow!("automatic metadata deadline elapsed")); }
    Ok(ms)
}

fn lock_gate_until<'a>(gate: &'a Mutex<()>, deadline: Instant) -> Result<MutexGuard<'a, ()>> {
    loop {
        match gate.try_lock() {
            Ok(guard) => return Ok(guard),
            Err(TryLockError::Poisoned(_)) => return Err(anyhow!("account profile gate is poisoned")),
            Err(TryLockError::WouldBlock) => {
                if Instant::now() >= deadline { return Err(anyhow!("account profile metadata deadline elapsed")); }
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }
}

static AUTO_COLLECTOR_SLOTS: OnceLock<(Mutex<usize>, Condvar)> = OnceLock::new();

struct AutoCollectorPermit;

fn admit_auto_collector(deadline: Instant) -> Result<AutoCollectorPermit> {
    let (lock, changed) = AUTO_COLLECTOR_SLOTS.get_or_init(|| (Mutex::new(0), Condvar::new()));
    let mut active = lock.lock().map_err(|_| anyhow!("automatic collector gate is poisoned"))?;
    while *active >= 4 {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining < Duration::from_millis(20) {
            return Err(anyhow!("automatic collector admission deadline elapsed"));
        }
        let (next, _) = changed.wait_timeout(active, remaining)
            .map_err(|_| anyhow!("automatic collector gate is poisoned"))?;
        active = next;
    }
    *active += 1;
    Ok(AutoCollectorPermit)
}

impl Drop for AutoCollectorPermit {
    fn drop(&mut self) {
        if let Some((lock, changed)) = AUTO_COLLECTOR_SLOTS.get() {
            let mut active = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            *active = active.saturating_sub(1);
            changed.notify_one();
        }
    }
}

/// Deduplicate collection keys and admit at most four collectors per wave.
/// Every real collector must honor the supplied budget so scoped joins settle.
fn collect_unique_bounded<T, F>(candidate_pools: &[String], budget: Duration, collect: F) -> Vec<(String, Result<T>)>
where T: Send, F: Fn(&str, Duration) -> Result<T> + Sync {
    let pools: Vec<String> = candidate_pools.iter().cloned().collect::<std::collections::BTreeSet<_>>()
        .into_iter().collect();
    let deadline = Instant::now() + budget;
    let mut results = Vec::new();
    for (wave, chunk) in pools.chunks(4).enumerate() {
        let waves_left = pools.len().div_ceil(4) - wave;
        let remaining = deadline.saturating_duration_since(Instant::now());
        let share = remaining / waves_left as u32;
        if share < Duration::from_millis(20) {
            results.extend(chunk.iter().cloned().map(|id| (id, Err(anyhow!("automatic collection deadline elapsed")))));
            continue;
        }
        let wave_results = std::thread::scope(|scope| {
            let handles = chunk.iter().map(|id| {
                let id = id.clone();
                let worker_id = id.clone();
                let collector = &collect;
                (id, scope.spawn(move || {
                    let deadline = Instant::now() + share;
                    let _permit = admit_auto_collector(deadline)?;
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    if remaining < Duration::from_millis(20) {
                        return Err(anyhow!("automatic collector deadline elapsed"));
                    }
                    collector(&worker_id, remaining)
                }))
            }).collect::<Vec<_>>();
            handles.into_iter().map(|(id, handle)| (id,
                handle.join().unwrap_or_else(|_| Err(anyhow!("automatic collector stopped unexpectedly")))))
                .collect::<Vec<_>>()
        });
        results.extend(wave_results);
    }
    results
}

#[derive(Clone)]
struct AutoProfileDiscovery {
    routes: Vec<crate::auto_select::Route>,
    evidence: Value,
    generation: Option<i64>,
}

struct AutoFlight<T> {
    result: Mutex<Option<std::result::Result<T, String>>>,
    ready: Condvar,
}

static AUTO_DISCOVERY_FLIGHTS: OnceLock<Mutex<std::collections::BTreeMap<
    (usize, String, String, String), Arc<AutoFlight<AutoProfileDiscovery>>>>> = OnceLock::new();

#[derive(Clone)]
struct CodexModelRead {
    models: Value,
    generation: Option<i64>,
}

static AUTO_CODEX_MODEL_FLIGHTS: OnceLock<Mutex<std::collections::BTreeMap<
    (usize, String), Arc<AutoFlight<CodexModelRead>>>>> = OnceLock::new();

#[derive(Clone)]
struct ClaudeAuthRead {
    auth: crate::auto_collect::ClaudeAuth,
    generation: i64,
}

static AUTO_CLAUDE_AUTH_FLIGHTS: OnceLock<Mutex<std::collections::BTreeMap<
    (usize, String), Arc<AutoFlight<ClaudeAuthRead>>>>> = OnceLock::new();

static AUTO_PUBLIC_STATUS_FLIGHTS: OnceLock<Mutex<std::collections::BTreeMap<
    (std::path::PathBuf, String), Arc<AutoFlight<crate::auto_health::Observation>>>>> = OnceLock::new();

fn collect_shared<K, T, F>(flights: &Mutex<std::collections::BTreeMap<K, Arc<AutoFlight<T>>>>,
    key: K, budget: Duration, collect: F) -> Result<T>
where K: Ord + Clone, T: Clone, F: FnOnce() -> Result<T> {
    let deadline = Instant::now() + budget;
    let (flight, owner) = {
        let mut active = flights.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        match active.get(&key) {
            Some(existing) => (existing.clone(), false),
            None => {
                let new = Arc::new(AutoFlight { result:Mutex::new(None), ready:Condvar::new() });
                active.insert(key.clone(), new.clone());
                (new, true)
            }
        }
    };
    if owner {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(collect))
            .unwrap_or_else(|_| Err(anyhow!("automatic collector stopped unexpectedly")))
            .map_err(|error| error.to_string());
        *flight.result.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(result.clone());
        flight.ready.notify_all();
        flights.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).remove(&key);
        return result.map_err(|message| anyhow!(message));
    }
    let mut shared = flight.result.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    loop {
        if let Some(result) = shared.as_ref() {
            return result.clone().map_err(|message| anyhow!(message));
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining < Duration::from_millis(20) {
            return Err(anyhow!("shared automatic collection deadline elapsed"));
        }
        let (next, _) = flight.ready.wait_timeout(shared, remaining)
            .map_err(|_| anyhow!("automatic collector gate is poisoned"))?;
        shared = next;
    }
}

/// Concurrent work units in one daemon/workspace share an in-flight profile
/// read, including its source timestamps and account generation. A completed
/// read is removed immediately; later decisions must collect fresh evidence.
fn discover_auto_profile_shared(d: &Arc<Daemon>, profile_id: &str, workspace_id: &str,
    parent_run_id: &str, repo: Option<&std::path::Path>,
    budget: Duration) -> Result<AutoProfileDiscovery> {
    let key = (Arc::as_ptr(d) as usize, profile_id.to_string(),
        repo.map(|path| format!("repo:{}", path.display()))
            .unwrap_or_else(|| format!("workspace:{workspace_id}")), parent_run_id.to_string());
    let flights = AUTO_DISCOVERY_FLIGHTS.get_or_init(|| Mutex::new(std::collections::BTreeMap::new()));
    collect_shared(flights, key, budget,
        || discover_auto_profile(d, profile_id, workspace_id, parent_run_id, repo, budget))
}

/// Public status has no account or workspace scope. Share only overlapping
/// reads of the same fixed feed, and preserve the source observation time.
fn collect_public_status_shared(program: &std::path::Path, provider: &str,
    budget: Duration) -> Result<crate::auto_health::Observation> {
    let flights = AUTO_PUBLIC_STATUS_FLIGHTS.get_or_init(||
        Mutex::new(std::collections::BTreeMap::new()));
    collect_shared(flights, (program.to_path_buf(), provider.to_string()), budget, || {
        let data = crate::auto_collect::public_status_json(program, provider, budget)?;
        crate::auto_health::parse_public_status(&data, provider, crate::daemon::now())
    })
}

fn auto_launch_resources(d: &Arc<Daemon>, work_unit_id: &str) -> Result<Value> {
    let resources = d.store.lock().unwrap().auto_launch_resources(work_unit_id)?;
    Ok(match resources {
        Some((branch, path, snapshot_id, snapshot_commit)) =>
            json!({"branch":branch,"path":path,"snapshot_id":snapshot_id,
                "snapshot_commit":snapshot_commit}),
        None => Value::Null,
    })
}

static AUTO_LAUNCHES: OnceLock<Mutex<std::collections::BTreeSet<String>>> = OnceLock::new();

fn auto_launches() -> &'static Mutex<std::collections::BTreeSet<String>> {
    AUTO_LAUNCHES.get_or_init(|| Mutex::new(std::collections::BTreeSet::new()))
}

fn auto_launch_active(work_unit_id: &str) -> bool {
    auto_launches().lock().unwrap().contains(work_unit_id)
}

struct ActiveAutoLaunch(String);

impl Drop for ActiveAutoLaunch {
    fn drop(&mut self) {
        auto_launches().lock().unwrap().remove(&self.0);
    }
}

fn auto_pending_response(work_unit_id: &str, route_id: &str, replayed: bool) -> Value {
    json!({"state":"launch_pending","work_unit_id":work_unit_id,"replayed":replayed,
        "decision":{"work_unit_id":work_unit_id,"selected":route_id,
            "exclusions":[],"reason":"selected_launch_in_progress"},
        "actions":["refresh"]})
}

fn apply_account_pool(routes: &mut [crate::auto_select::Route], pool_id: &str,
    observations: &[crate::auto_quota::StoredQuotaObservation], now_ms: i64) {
    for route in routes {
        route.pool_id = pool_id.to_string();
        for observation in observations {
            if observation.snapshot.state_for(&route.model, now_ms)
                == crate::auto_quota::QuotaState::Exhausted {
                route.quota = crate::auto_select::Allowance::Exhausted;
                route.quota_blocks.extend(observation.snapshot.blocking_scopes(&route.model, now_ms));
            }
        }
        route.quota_blocks.sort();
        route.quota_blocks.dedup();
    }
}

fn active_parent_metadata(socket: &std::path::Path, method: &str, params: Value,
    deadline: Instant) -> Result<Value> {
    let timeout_ms = remaining_metadata_ms(deadline)?.min(4000);
    let reply = crate::shim::control(socket, &json!({"op":"metadata_rpc",
        "method":method,"params":params,"timeout_ms":timeout_ms}))?;
    if reply["ok"] != true {
        return Err(anyhow!("active parent metadata unavailable"));
    }
    Ok(reply["result"].clone())
}

/// Initial Auto selection has no workspace yet. Inspect the caller's existing
/// repository in place, then recheck selected tools in the eventual workspace
/// before a model turn. Never create a Git resource just to read metadata.
fn auto_metadata_project(d: &Arc<Daemon>, p: &Value)
    -> Result<(std::path::PathBuf, Option<String>, Option<String>)> {
    match (p.get("workspace_id"), p.get("repo")) {
        (Some(Value::String(id)), None) => {
            let workspace = d.workspace(id)?;
            if workspace.removed_ms.is_some() { return Err(anyhow!("workspace was removed")); }
            Ok((std::path::PathBuf::from(workspace.path), Some(workspace.id), None))
        }
        (None, Some(Value::String(raw))) => {
            let path = std::path::Path::new(raw);
            if raw.is_empty() || !path.is_absolute() {
                return Err(anyhow!("repository preflight needs an absolute path"));
            }
            let canonical = std::fs::canonicalize(path)?;
            if !canonical.is_dir() || !canonical.join(".git").exists() {
                return Err(anyhow!("repository preflight needs a Git root"));
            }
            let label = canonical.to_str().ok_or_else(|| anyhow!("repository path is not UTF-8"))?;
            Ok((canonical.clone(), None, Some(label.to_string())))
        }
        _ => Err(anyhow!("metadata preflight requires exactly one workspace_id or repo")),
    }
}

fn active_parent_pages(socket: &std::path::Path, method: &str, tool_detail: bool,
    limit: usize, deadline: Instant) -> Result<Value> {
    let mut data = Vec::new();
    let mut cursor: Option<String> = None;
    let mut seen = std::collections::BTreeSet::new();
    for _ in 0..8 {
        let params = match (&cursor, tool_detail) {
            (None, false) => json!({}),
            (Some(cursor), false) => json!({"cursor":cursor}),
            (None, true) => json!({"detail":"toolsAndAuthOnly"}),
            (Some(cursor), true) => json!({"cursor":cursor,"detail":"toolsAndAuthOnly"}),
        };
        let page = active_parent_metadata(socket, method, params, deadline)?;
        let items = page["data"].as_array()
            .ok_or_else(|| anyhow!("active parent metadata page unavailable"))?;
        if items.len() > limit || data.len() + items.len() > limit {
            return Err(anyhow!("active parent metadata exceeded its bound"));
        }
        data.extend(items.iter().cloned());
        cursor = match page.get("nextCursor") {
            None | Some(Value::Null) => None,
            Some(Value::String(value)) if !value.is_empty() && value.len() <= 512
                && value.bytes().all(|byte| byte.is_ascii_graphic()) => Some(value.clone()),
            _ => return Err(anyhow!("active parent metadata cursor invalid")),
        };
        let Some(next) = &cursor else {
            return Ok(json!({"data":data,"nextCursor":null}));
        };
        if !seen.insert(next.clone()) {
            return Err(anyhow!("active parent metadata pagination repeated"));
        }
    }
    Err(anyhow!("active parent metadata pagination exceeded its bound"))
}

fn refresh_active_parent_discovery(d: &Arc<Daemon>, parent: &crate::store::Run,
    profile_id: &str, workspace_id: &str, expected_generation: i64,
    deadline: Instant) -> Result<AutoProfileDiscovery> {
    let socket = d.control_socket(parent)?;
    let account = active_parent_metadata(&socket, "account/read",
        json!({"refreshToken":false}), deadline)?;
    if account["requiresOpenaiAuth"] != true || account["account"]["type"] != "chatgpt" {
        return Err(anyhow!("active parent subscription authentication unavailable"));
    }
    let rate_limits = active_parent_metadata(&socket, "account/rateLimits/read", json!({}), deadline)?;
    let quota_observed_ms = crate::daemon::now();
    let fingerprint = crate::auto_quota::account_fingerprint(&rate_limits)?;
    let snapshot = crate::auto_quota::parse_codex_rate_limits(&rate_limits,
        profile_id, quota_observed_ms)?;
    let models = active_parent_pages(&socket, "model/list", false, 128, deadline)?;
    let model_observed_ms = crate::daemon::now();
    let tools = active_parent_pages(&socket, "mcpServerStatus/list", true, 64, deadline)?;
    let tool_observed_ms = crate::daemon::now();
    let catalog = crate::auto_route::parse_codex_catalog(&models, model_observed_ms)?;
    let tool_catalog = crate::auto_route::parse_codex_tools(&tools, tool_observed_ms)?;
    let store = d.store.lock().unwrap();
    let current = store.run(&parent.id)?
        .ok_or_else(|| anyhow!("active parent disappeared during metadata refresh"))?;
    if current.process_generation != parent.process_generation
        || current.profile_id.as_deref() != Some(profile_id)
        || current.workspace_id != workspace_id
        || !ACTIVE.contains(&current.status.as_str())
        || store.auto_account_generation(profile_id)? != Some(expected_generation) {
        return Err(anyhow!("active parent changed during metadata refresh"));
    }
    store.record_auto_account_identity(profile_id, &fingerprint)?;
    if store.auto_account_generation(profile_id)? != Some(expected_generation) {
        return Err(anyhow!("active parent account changed during metadata refresh"));
    }
    let saved: Option<String> = store.conn.query_row("SELECT launch FROM runs WHERE id=?1",
        [&parent.id], |row| row.get(0))?;
    let mut launch: Value = saved.as_deref().and_then(|text| serde_json::from_str(text).ok())
        .ok_or_else(|| anyhow!("active parent launch metadata unavailable"))?;
    let generic = if launch.get("generic").is_some() { &mut launch["generic"] } else { &mut launch };
    if generic["auto_routing"] != true
        || generic["auto_parent_discovery"]["account_generation"].as_i64() != Some(expected_generation)
        || generic["auto_parent_discovery"]["workspace_id"].as_str() != Some(workspace_id) {
        return Err(anyhow!("active parent preflight changed during metadata refresh"));
    }
    generic["auto_parent_discovery"] = json!({"workspace_id":workspace_id,
        "account_generation":expected_generation,"model_catalog":catalog,
        "tool_catalog":tool_catalog});
    let event = store.insert_event(quota_observed_ms, None, None, "quota",
        "codex-app/active-parent-metadata", "reported",
        &json!({"profile_id":profile_id,"snapshot":snapshot}))?;
    store.insert_auto_quota(event.seq, profile_id, "codex-app/active-parent-metadata", &snapshot)?;
    store.put_auto_model_catalog(profile_id, &catalog)?;
    store.conn.execute("UPDATE runs SET launch=?2 WHERE id=?1",
        rusqlite::params![parent.id, launch.to_string()])?;
    let observations = store.auto_account_quota_observations(profile_id)?;
    let pool_id = store.auto_account_pool_id(profile_id)?
        .ok_or_else(|| anyhow!("Codex account pool identity unavailable"))?;
    let now_ms = crate::daemon::now();
    let mut routes = crate::auto_route::codex_auto_routes(&catalog, &tool_catalog,
        Some(&snapshot), profile_id, now_ms);
    apply_account_pool(&mut routes, &pool_id, &observations, now_ms);
    Ok(AutoProfileDiscovery { routes, generation:Some(expected_generation),
        evidence:json!({"profile_id":profile_id,
            "source":"parent/codex-app/active-session-metadata",
            "model_observed_ms":model_observed_ms,"tool_observed_ms":tool_observed_ms,
            "quota_observed_ms":quota_observed_ms,
            "account_generation":expected_generation}) })
}

fn discover_auto_profile(d: &Arc<Daemon>, profile_id: &str, workspace_id: &str,
    parent_run_id: &str, repo: Option<&std::path::Path>,
    budget: Duration) -> Result<AutoProfileDiscovery> {
    let deadline = Instant::now() + budget;
    let profile = d.profile(profile_id)?;
    if profile.id == crate::opencode_bridge::LOCAL_PROFILE {
        return Err(anyhow!("Continuity's local model profile needs a verified Auto memory and capability adapter"));
    }
    if profile.harness == "codex" {
        let store = d.store.lock().unwrap();
        let active = store.runs()?.into_iter().filter(|run|
            run.profile_id.as_deref() == Some(profile_id) && ACTIVE.contains(&run.status.as_str()))
            .collect::<Vec<_>>();
        if !active.is_empty() {
            if active.len() != 1 || active[0].id != parent_run_id {
                return Err(anyhow!("account profile has another active run"));
            }
            let saved: Option<String> = store.conn.query_row(
                "SELECT launch FROM runs WHERE id=?1", [parent_run_id], |row| row.get(0))?;
            let launch: Value = saved.as_deref().and_then(|text| serde_json::from_str(text).ok())
                .ok_or_else(|| anyhow!("parent Auto route preflight is unavailable"))?;
            let generic = launch.get("generic").unwrap_or(&launch);
            if generic["auto_routing"] != true {
                return Err(anyhow!("parent Auto routing is disabled"));
            }
            let saved = &generic["auto_parent_discovery"];
            let generation = saved["account_generation"].as_i64()
                .ok_or_else(|| anyhow!("parent Auto account preflight is unavailable"))?;
            if saved["workspace_id"].as_str() != Some(workspace_id)
                || store.auto_account_generation(profile_id)? != Some(generation) {
                return Err(anyhow!("parent Auto route preflight changed account or workspace"));
            }
            let catalog: crate::auto_route::ModelCatalog =
                serde_json::from_value(saved["model_catalog"].clone())?;
            let tools: crate::auto_route::ToolCatalog =
                serde_json::from_value(saved["tool_catalog"].clone())?;
            let now_ms = crate::daemon::now();
            let observations = store.auto_account_quota_observations(profile_id)?;
            let observation = observations.iter().find(|value| value.pool_id == profile_id).cloned();
            if now_ms < catalog.observed_ms || now_ms >= catalog.expires_ms
                || now_ms < tools.observed_ms || now_ms >= tools.expires_ms
                || observation.as_ref().is_some_and(|value| value.snapshot.needs_refresh(now_ms)) {
                let parent = active[0].clone();
                drop(store);
                return refresh_active_parent_discovery(d, &parent, profile_id,
                    workspace_id, generation, deadline);
            }
            let pool_id = store.auto_account_pool_id(profile_id)?
                .ok_or_else(|| anyhow!("Codex account pool identity unavailable"))?;
            let mut routes = crate::auto_route::codex_auto_routes(&catalog, &tools,
                observation.as_ref().map(|value| &value.snapshot), profile_id, now_ms);
            apply_account_pool(&mut routes, &pool_id, &observations, now_ms);
            return Ok(AutoProfileDiscovery { routes, generation: Some(generation),
                evidence:json!({"profile_id":profile_id,
                    "source":"parent/codex-app/pre-turn-model-and-tool-metadata",
                    "model_observed_ms":catalog.observed_ms,"tool_observed_ms":tools.observed_ms,
                    "quota_observed_ms":observation.as_ref().map(|value| value.snapshot.observed_ms),
                    "account_generation":generation}) });
        }
    }
    match profile.harness.as_str() {
        "codex" => {
            let flights = AUTO_CODEX_MODEL_FLIGHTS.get_or_init(||
                Mutex::new(std::collections::BTreeMap::new()));
            let read = collect_shared(flights, (Arc::as_ptr(d) as usize, profile_id.to_string()),
                deadline.saturating_duration_since(Instant::now()), || {
                    let models = dispatch(d, "auto.models.refresh", &json!({"profile_id":profile_id,
                        "timeout_ms":remaining_metadata_ms(deadline)?}))?;
                    let generation = d.store.lock().unwrap().auto_account_generation(profile_id)?;
                    Ok(CodexModelRead { models, generation })
                })?;
            let models = read.models;
            let first_generation = read.generation;
            let mut tool_request = json!({"profile_id":profile_id,
                "timeout_ms":remaining_metadata_ms(deadline)?});
            match repo {
                Some(root) => tool_request["repo"] = json!(root),
                None => tool_request["workspace_id"] = json!(workspace_id),
            }
            let tools = dispatch(d, "auto.tools.inspect", &tool_request)?;
            let store = d.store.lock().unwrap();
            let generation = store.auto_account_generation(profile_id)?;
            if generation.is_none() || generation != first_generation {
                return Err(anyhow!("account changed during automatic route discovery"));
            }
            if store.runs()?.iter().any(|run| run.profile_id.as_deref() == Some(profile_id)
                && ACTIVE.contains(&run.status.as_str())) {
                return Err(anyhow!("account profile became active during automatic route discovery"));
            }
            let catalog: crate::auto_route::ModelCatalog = serde_json::from_value(models["catalog"].clone())?;
            let tool_catalog: crate::auto_route::ToolCatalog = serde_json::from_value(tools["catalog"].clone())?;
            let observations = store.auto_account_quota_observations(profile_id)?;
            let observation = observations.iter().find(|value| value.pool_id == profile_id).cloned();
            let pool_id = store.auto_account_pool_id(profile_id)?
                .ok_or_else(|| anyhow!("Codex account pool identity unavailable"))?;
            drop(store);
            let now_ms = crate::daemon::now();
            let mut routes = crate::auto_route::codex_auto_routes(&catalog, &tool_catalog,
                observation.as_ref().map(|value| &value.snapshot), profile_id, now_ms);
            apply_account_pool(&mut routes, &pool_id, &observations, now_ms);
            Ok(AutoProfileDiscovery {
                routes,
                evidence:json!({"profile_id":profile_id,"source":"codex-app/model-and-tool-metadata",
                    "model_observed_ms":catalog.observed_ms,"tool_observed_ms":tool_catalog.observed_ms,
                    "quota_observed_ms":observation.as_ref().map(|value| value.snapshot.observed_ms),
                    "shared_account_quota_observed_ms":observations.iter().map(|value| value.snapshot.observed_ms).collect::<Vec<_>>(),
                    "account_generation":generation}),
                generation,
            })
        }
        "claude" => {
            let flights = AUTO_CLAUDE_AUTH_FLIGHTS.get_or_init(||
                Mutex::new(std::collections::BTreeMap::new()));
            let read = collect_shared(flights, (Arc::as_ptr(d) as usize, profile_id.to_string()),
                deadline.saturating_duration_since(Instant::now()), || {
                    let gate = d.profile_gate(profile_id);
                    let _guard = lock_gate_until(&gate, deadline)?;
                    let program = crate::adapters::resolve_program("claude")
                        .ok_or_else(|| anyhow!("Claude executable unavailable"))?;
                    let auth = crate::auto_collect::claude_auth_status(&program,
                        &crate::daemon::Daemon::profile_env(&profile),
                        Duration::from_millis(remaining_metadata_ms(deadline)?), crate::daemon::now())?;
                    let store = d.store.lock().unwrap();
                    store.record_auto_account_identity(profile_id, &auth.fingerprint)?;
                    let generation = store.auto_account_generation(profile_id)?
                        .ok_or_else(|| anyhow!("Claude account generation unavailable"))?;
                    Ok(ClaudeAuthRead { auth, generation })
                })?;
            let store = d.store.lock().unwrap();
            if store.auto_account_generation(profile_id)? != Some(read.generation) {
                return Err(anyhow!("Claude account changed during shared discovery"));
            }
            let observations = store.auto_account_quota_observations(profile_id)?;
            let observation = observations.iter().find(|value| value.pool_id == profile_id).cloned();
            let pool_id = store.auto_account_pool_id(profile_id)?
                .ok_or_else(|| anyhow!("Claude account pool identity unavailable"))?;
            let now_ms = crate::daemon::now();
            let mut routes = crate::auto_route::claude_auto_routes(&read.auth,
                observation.as_ref().map(|value| &value.snapshot), profile_id, now_ms);
            apply_account_pool(&mut routes, &pool_id, &observations, now_ms);
            Ok(AutoProfileDiscovery {
                routes,
                evidence:json!({"profile_id":profile_id,"source":"claude/auth-status-and-native-quota",
                    "auth_observed_ms":read.auth.observed_ms,
                    "quota_observed_ms":observation.as_ref().map(|value| value.snapshot.observed_ms),
                    "shared_account_quota_observed_ms":observations.iter().map(|value| value.snapshot.observed_ms).collect::<Vec<_>>(),
                    "account_generation":read.generation}),
                generation:Some(read.generation),
            })
        }
        "opencode" => {
            let gate = d.profile_gate(profile_id);
            let _guard = lock_gate_until(&gate, deadline)?;
            if d.store.lock().unwrap().runs()?.iter().any(|run|
                run.profile_id.as_deref() == Some(profile_id)
                    && ACTIVE.contains(&run.status.as_str())) {
                return Err(anyhow!("OpenCode profile has an active run"));
            }
            let project_path = match repo {
                Some(root) => root.to_path_buf(),
                None => {
                    let workspace = d.workspace(workspace_id)?;
                    if workspace.removed_ms.is_some() { return Err(anyhow!("workspace was removed")); }
                    std::path::PathBuf::from(workspace.path)
                }
            };
            let project = project_path.as_path();
            let program = crate::adapters::resolve_program("opencode")
                .ok_or_else(|| anyhow!("OpenCode executable unavailable"))?;
            let mut env = crate::adapters::base_env(&program.display().to_string());
            env.extend(Daemon::profile_env(&profile));
            let catalog = crate::auto_collect::opencode_local_catalog(&program, &env,
                project, Duration::from_millis(remaining_metadata_ms(deadline)?),
                crate::daemon::now())?;
            let mut routes = crate::auto_route::opencode_local_routes(&catalog, profile_id,
                crate::daemon::now());
            routes.retain(|route| crate::auto_opencode::auto_local_inline_config(&profile,
                project, &route.model, &route.endpoint).is_ok());
            for route in &mut routes {
                if crate::auto_opencode::probe_local_endpoint(&route.endpoint)
                    != crate::auto_opencode::EndpointProbe::Reachable {
                    route.health = crate::auto_select::Health::Unavailable;
                }
            }
            Ok(AutoProfileDiscovery { evidence:json!({"profile_id":profile_id,
                "source":"opencode/local-config-providers","observed_ms":catalog.observed_ms,
                "capability_prior":"opencode-gpt-oss-120b-v1",
                "allowance":"unknown","route_count":routes.len()}), routes,
                generation:None })
        }
        _ => Err(anyhow!("unsupported automatic harness")),
    }
}

/// A metadata-only rejection here is known to precede child creation. A
/// rejection after `delegate_run` has made a run is not retryable by this path.
fn recheck_claude_account_before_child(d: &Arc<Daemon>, profile_id: &str,
    expected_generation: i64, decision_deadline: Instant) -> Result<()> {
    let deadline = decision_deadline.min(Instant::now() + Duration::from_secs(2));
    let gate = d.profile_gate(profile_id);
    let _guard = lock_gate_until(&gate, deadline)?;
    let profile = d.profile(profile_id)?;
    if profile.harness != "claude" { return Err(anyhow!("account profile changed harness")); }
    let program = crate::adapters::resolve_program("claude")
        .ok_or_else(|| anyhow!("Claude executable unavailable"))?;
    let auth = crate::auto_collect::claude_auth_status(&program,
        &crate::daemon::Daemon::profile_env(&profile),
        Duration::from_millis(remaining_metadata_ms(deadline)?), crate::daemon::now())?;
    let store = d.store.lock().unwrap();
    store.record_auto_account_identity(profile_id, &auth.fingerprint)?;
    if store.auto_account_generation(profile_id)? != Some(expected_generation) {
        return Err(anyhow!("Claude account changed before child creation"));
    }
    Ok(())
}

fn fixture_only() -> Result<()> {
    if std::env::var("OVERSEER_SWARM_FIXTURE_API").as_deref() == Ok("1") {
        Ok(())
    } else {
        Err(anyhow!("fixture-only swarm transition; live runtime authority is not implemented"))
    }
}

fn storage_fault(error: &anyhow::Error) -> bool {
    error.chain().filter_map(|cause| cause.downcast_ref::<rusqlite::Error>())
        .any(|error| matches!(error, rusqlite::Error::SqliteFailure(code, _)
            if matches!(code.code, rusqlite::ErrorCode::DiskFull |
                rusqlite::ErrorCode::SystemIoFailure | rusqlite::ErrorCode::ReadOnly)))
}

fn require_swarm_storage(d: &Daemon) -> Result<()> {
    if d.swarm_storage_blocked.load(Ordering::SeqCst) {
        bail!("swarm storage is blocked; recover write capacity before launching new work");
    }
    Ok(())
}

/// Select an initial route from repository-scoped evidence without creating a
/// workspace. The eventual auto.start admission must revalidate this result;
/// preview by itself never owns a process, Git resource, or allowance claim.
/// Past a failure's cooldown, the first unit on an endpoint is its one
/// shared recovery check; other routes on that endpoint wait for it.
fn mark_endpoint_recovery(d: &Arc<Daemon>, routes: &mut [crate::auto_select::Route],
    requesting_parent: Option<&str>) -> Result<Vec<crate::auto_health::RecoveringEndpoint>> {
    let store = d.store.lock().unwrap();
    let recovering = crate::auto_health::recovering_endpoints(&store, crate::daemon::now())?;
    for route in routes.iter_mut() {
        if let Some(check) = recovering.iter().find(|r| r.provider == route.provider && r.endpoint == route.endpoint) {
            route.endpoint_recovery_in_flight = crate::auto_health::endpoint_recovery_in_flight(
                &store.conn, check, requesting_parent)?;
        }
    }
    Ok(recovering)
}

fn recovery_check_for(recovering: &[crate::auto_health::RecoveringEndpoint],
    route: &crate::auto_select::Route) -> Option<Value> {
    recovering.iter().find(|r| r.provider == route.provider && r.endpoint == route.endpoint)
        .map(|r| json!({"provider":r.provider,"endpoint":r.endpoint,"failed_ms":r.failed_ms}))
}

fn auto_root_preview(d: &Arc<Daemon>, p: &Value) -> Result<Value> {
    use crate::auto_select::{CapabilityTier, Sandbox, WorkUnit};
    use std::collections::{BTreeMap, BTreeSet};
    let fields = p.as_object().ok_or_else(|| anyhow!("automatic root preview must be an object"))?;
    for field in fields.keys() {
        if !matches!(field.as_str(), "repo" | "work_unit_id" | "allowed_profiles" | "min_tier"
            | "required_tools" | "context_needed" | "requires_approvals" | "sandbox"
            | "pinned_route" | "preferred_harness" | "task_class" | "execution_budget_ms") {
            return Err(anyhow!("unsupported automatic root preview field: {field}"));
        }
    }
    if !d.store.lock().unwrap().auto_mode_enabled()? {
        return Err(anyhow!("Auto Mode is disabled"));
    }
    let work_unit_id = s(p, "work_unit_id")?;
    if work_unit_id.is_empty() || work_unit_id.len() > 120 || !work_unit_id.bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')) {
        return Err(anyhow!("invalid automatic root work-unit identity"));
    }
    let repo = crate::git::toplevel(std::path::Path::new(s(p, "repo")?))?;
    let min_tier = match p.get("min_tier") {
        None => CapabilityTier::General,
        Some(value) => serde_json::from_value(value.clone())?,
    };
    let tool_values = match p.get("required_tools") {
        None => Vec::new(),
        Some(Value::Array(values)) if values.len() <= 16 => values.clone(),
        _ => return Err(anyhow!("required_tools must be a bounded list")),
    };
    let mut required_tools = BTreeSet::new();
    for value in tool_values {
        let tool = value.as_str().filter(|name| crate::daemon::valid_required_tool(name))
            .ok_or_else(|| anyhow!("invalid required tool"))?;
        if !required_tools.insert(tool.to_string()) { return Err(anyhow!("duplicate required tool")); }
    }
    let context_needed = match p.get("context_needed") {
        None => 0,
        Some(value) => value.as_u64().ok_or_else(|| anyhow!("context_needed must be nonnegative"))?,
    };
    let requires_approvals = match p.get("requires_approvals") {
        None => false,
        Some(value) => value.as_bool().ok_or_else(|| anyhow!("requires_approvals must be a boolean"))?,
    };
    let sandbox = match p.get("sandbox") {
        None => Sandbox::WorkspaceWrite,
        Some(value) => {
            let requested: Sandbox = serde_json::from_value(value.clone())?;
            if !matches!(requested, Sandbox::ReadOnly | Sandbox::WorkspaceWrite) {
                return Err(anyhow!("automatic root sandbox is unsupported"));
            }
            requested
        }
    };
    let optional_name = |name: &str, max: usize| -> Result<Option<String>> {
        match p.get(name) {
            None => Ok(None),
            Some(Value::String(value)) if !value.is_empty() && value.len() <= max => Ok(Some(value.clone())),
            _ => Err(anyhow!("invalid {name}")),
        }
    };
    let pinned_route = optional_name("pinned_route", 300)?;
    let preferred_harness = optional_name("preferred_harness", 40)?;
    let task_class = match p.get("task_class") {
        None => None,
        Some(Value::String(value)) if matches!(value.as_str(),
            "browser_check" | "routine_edit" | "difficult_diagnosis" | "general") => Some(value.clone()),
        _ => return Err(anyhow!("task_class must be a supported broad work category")),
    };
    let execution_budget_ms = match p.get("execution_budget_ms") {
        None => crate::daemon::DEFAULT_AUTO_EXECUTION_BUDGET_MS,
        Some(value) => value.as_u64().filter(|ms| (1_000..=1_800_000).contains(ms))
            .ok_or_else(|| anyhow!("execution_budget_ms must be 1000-1800000"))?,
    };
    let allowed_profiles = match p.get("allowed_profiles") {
        None => d.store.lock().unwrap().profiles()?.into_iter()
            .filter(|profile| profile.is_system && matches!(profile.harness.as_str(),
                "codex" | "claude" | "opencode"))
            .map(|profile| profile.id).collect::<BTreeSet<_>>(),
        Some(Value::Array(values)) if !values.is_empty() && values.len() <= 8 => {
            let mut allowed = BTreeSet::new();
            for value in values {
                let id = value.as_str().filter(|id| !id.is_empty() && id.len() <= 120)
                    .ok_or_else(|| anyhow!("invalid allowed account profile"))?;
                let profile = d.profile(id)?;
                if !matches!(profile.harness.as_str(), "codex" | "claude" | "opencode")
                    || !allowed.insert(id.to_string()) {
                    return Err(anyhow!("invalid or duplicate allowed account profile"));
                }
            }
            allowed
        }
        _ => return Err(anyhow!("allowed_profiles must be a bounded nonempty account list")),
    };
    if allowed_profiles.is_empty() || allowed_profiles.len() > 8 {
        return Err(anyhow!("no bounded authorized account profiles for automatic root"));
    }
    let work = WorkUnit { id:work_unit_id.into(), min_tier, required_tools,
        context_needed, requires_approvals, min_sandbox:sandbox, max_sandbox:sandbox,
        allowed_profiles:allowed_profiles.clone(), pinned_route, preferred_harness,
        task_class, execution_budget_ms:Some(execution_budget_ms) };
    let deadline = Instant::now() + Duration::from_secs(10);
    let candidate_ids = allowed_profiles.into_iter().collect::<Vec<_>>();
    let mut routes = Vec::new();
    let mut evidence = Vec::new();
    let mut discovery_failures = Vec::new();
    let mut generations = BTreeMap::new();
    for (id, discovered) in collect_unique_bounded(&candidate_ids,
        Duration::from_secs(8).min(deadline.saturating_duration_since(Instant::now())),
        |id, budget| discover_auto_profile_shared(d, id, "", "", Some(&repo), budget)) {
        match discovered {
            Ok(found) => {
                routes.extend(found.routes);
                if let Some(generation) = found.generation { generations.insert(id, generation); }
                evidence.push(found.evidence);
            }
            Err(_) => discovery_failures.push(json!({"profile_id":id,
                "reason":"metadata_or_auth_unavailable"})),
        }
    }
    if routes.len() > 128 { return Err(anyhow!("automatic root candidate catalog exceeded its bound")); }
    let now_ms = crate::daemon::now();
    let mut health = match crate::auto_health::recent_local_observations(
        &d.store.lock().unwrap(), now_ms) {
        Ok(observations) => observations,
        Err(_) => { discovery_failures.push(json!({"reason":"local_health_evidence_unavailable"})); Vec::new() }
    };
    let public = routes.iter().map(|route| route.provider.clone())
        .filter(|provider| matches!(provider.as_str(), "openai" | "anthropic"))
        .collect::<Vec<_>>();
    let public_budget = Duration::from_millis(750)
        .min(deadline.saturating_duration_since(Instant::now()));
    if !public.is_empty() && public_budget >= Duration::from_millis(20) {
        for (provider, reading) in collect_unique_bounded(&public, public_budget, |id, timeout|
            collect_public_status_shared(std::path::Path::new("/usr/bin/curl"), id, timeout)) {
            match reading {
                Ok(observation) => { evidence.push(json!({"provider":provider,
                    "source":"official-status-summary","observed_ms":observation.observed_ms,
                    "advisory":true})); health.push(observation); }
                Err(_) => discovery_failures.push(json!({"provider":provider,
                    "reason":"public_status_unavailable"})),
            }
        }
    }
    let now_ms = crate::daemon::now();
    for route in &mut routes {
        if route.health != crate::auto_select::Health::Unavailable {
            route.health = crate::auto_health::evaluate(route, &health, now_ms);
        }
    }
    if routes.iter().any(|route| route.harness != "opencode"
        && route.quota == crate::auto_select::Allowance::Exhausted) {
        for route in routes.iter_mut().filter(|route| route.harness == "opencode") {
            route.unresolved_quota_pool_identity = true;
        }
    }
    {
        let store = d.store.lock().unwrap();
        let now_ms = crate::daemon::now();
        for route in &mut routes {
            route.in_flight_pool_claim = store.auto_pool_claimed(&route.pool_id)?
                && !store.auto_pool_open_to_known_windows(route, work.task_class.as_deref(), now_ms)?;
        }
    }
    let recovering = mark_endpoint_recovery(d, &mut routes, None)?;
    let fit_now_ms = crate::daemon::now();
    let (fit_inputs, fit_evidence) = if d.learning_is_paused() {
        (routes.iter().map(|_| crate::auto_fit::FitEvidenceInput::Unavailable {
            reason:"learning_paused".into() }).collect::<Vec<_>>(),
         routes.iter().map(|route| json!({"route_id":route.id,"fit":"unknown",
            "reason":"learning_paused","source":null,"observed_ms":null,
            "expected_windows":[]})).collect::<Vec<_>>())
    } else {
        crate::auto_fit::apply_scoped_fit_with_inputs(&d.store.lock().unwrap(), &work,
            &mut routes, &generations, fit_now_ms)
    };
    let decision = crate::auto_fit::select_with_estimates(&work, &routes, &fit_inputs, fit_now_ms);
    let selected_route = decision.selected.as_deref().and_then(|id|
        routes.iter().find(|route| route.id == id));
    let mut trace = json!({"selector_version":"multi-harness-preflight-v8","decision":decision,
        "selected_route":selected_route.map(|route| json!({
            "harness":route.harness,"provider":route.provider,"profile_id":route.profile_id,
            "model":route.model,"effort":route.effort,"quota":route.quota,
            "fit":route.fit,"health":route.health})),
        "estimator":{"state":"scoped_fit","version":"v3","now_ms":fit_now_ms,
            "inputs":fit_inputs,"routes":fit_evidence},
        "ranking":{"version":"v1","complete_costs":crate::auto_fit::complete_costs(
            &work, &routes, &fit_inputs, fit_now_ms)},
        "inference":{"state":"not_used","output":null},
        "selection_input":{"work":work,"routes":routes,
            "attempt_limit_reached":false,"deadline_exhausted":false},
        "evidence":evidence,"discovery_failures":discovery_failures});
    if let Some(check) = selected_route.and_then(|route| recovery_check_for(&recovering, route)) {
        trace["recovery_check"] = check;
    }
    Ok(json!({"repo":repo,"decision":decision,"selected_route":selected_route,
        "evidence":evidence,"discovery_failures":discovery_failures,"trace":trace}))
}

fn auto_root_response(d: &Arc<Daemon>, work_unit_id: &str, request_hash: &str,
    replayed: bool) -> Result<Value> {
    let (intent, decision) = {
        let store = d.store.lock().unwrap();
        let intent = store.auto_root_intent(work_unit_id)?
            .ok_or_else(|| anyhow!("automatic root intent is unavailable"))?;
        if intent.requirements_hash != request_hash {
            return Err(anyhow!("work unit was already used for different automatic work"));
        }
        let decision = intent.decision_event_seq.and_then(|seq| store.conn.query_row(
            "SELECT payload FROM events WHERE seq=?1 AND kind='auto_decision'",
            [seq], |row| row.get::<_, String>(0)).ok())
            .and_then(|text| serde_json::from_str::<Value>(&text).ok())
            .map(|trace| trace["decision"].clone()).unwrap_or(Value::Null);
        (intent, decision)
    };
    let run = d.run(&intent.run_id)?;
    let task = d.task(&intent.task_id)?;
    let workspace = d.workspace(&intent.workspace_id)?;
    let state = if run.status == "completed" { "completed" }
        else if intent.phase == "paused" || matches!(run.status.as_str(), "failed" | "interrupted" | "unknown") {
            "paused"
        } else if run.process_generation > 0 { "running" }
        else if auto_launch_active(work_unit_id) { "launch_pending" }
        else { "paused" };
    let launch_resources = json!({"branch":workspace.branch,"path":workspace.path,
        "snapshot_id":task.start_snapshot,"start_commit":task.fork_commit,
        "effect_state":if state == "paused" { "unknown" } else { "planned_or_active" }});
    Ok(json!({"state":state,"work_unit_id":work_unit_id,"replayed":replayed,
        "task":task,"run":run,"workspace":workspace,"decision":decision,
        "launch_phase":intent.phase,"launch_resources":launch_resources,
        "pause_reason":if state == "paused" { Some("launch_effects_uncertain") } else { None },
        "actions":if state == "paused" { vec!["inspect_launch","choose_manual_route"] }
            else { vec!["refresh"] }}))
}

fn auto_root_launch_worker(d: &Arc<Daemon>, work_unit_id: &str,
    route: &crate::auto_select::Route, required_tools: &[String],
    expected_generation: Option<i64>, request_hash: &str) -> Result<Value> {
    d.store.lock().unwrap().set_auto_root_phase(work_unit_id, "preparing")?;
    if !d.store.lock().unwrap().auto_mode_enabled()? {
        return Err(anyhow!("Auto Mode was disabled before root launch"));
    }
    let intent = d.store.lock().unwrap().auto_root_intent(work_unit_id)?
        .ok_or_else(|| anyhow!("automatic root intent disappeared"))?;
    let workspace = d.workspace(&intent.workspace_id)?;
    let task = d.task(&intent.task_id)?;
    let run = d.run(&intent.run_id)?;
    if run.profile_id.as_deref() != Some(route.profile_id.as_str())
        || run.model.as_deref() != Some(route.model.as_str())
        || run.effort.as_deref() != Some(route.effort.as_str()) {
        return Err(anyhow!("selected automatic root route changed before launch"));
    }
    if let Some(generation) = expected_generation {
        if d.store.lock().unwrap().auto_account_generation(&route.profile_id)? != Some(generation) {
            return Err(anyhow!("automatic root account changed before Git preparation"));
        }
    }
    if workspace.kind == "worktree" {
        let branch = workspace.branch.as_deref()
            .ok_or_else(|| anyhow!("automatic root planned branch is unavailable"))?;
        let start = task.fork_commit.as_deref()
            .ok_or_else(|| anyhow!("automatic root start commit is unavailable"))?;
        let (created, _) = crate::git::worktree_add_planned_auto(
            std::path::Path::new(&task.repo_root), std::path::Path::new(&workspace.path),
            branch, start)?;
        if created != std::fs::canonicalize(&workspace.path)? {
            return Err(anyhow!("automatic root worktree path changed during Git preparation"));
        }
    }
    d.store.lock().unwrap().set_auto_root_phase(work_unit_id, "prepared")?;
    let snapshot = d.take_snapshot(&workspace, "task-start")?;
    d.store.lock().unwrap().set_task_start_snapshot(&task.id, &snapshot.id)?;
    if !d.store.lock().unwrap().auto_mode_enabled()? {
        return Err(anyhow!("Auto Mode was disabled before the root model turn"));
    }
    if d.store.lock().unwrap().runs()?.iter().any(|other|
        other.id != run.id && other.profile_id.as_deref() == Some(route.profile_id.as_str())
            && ACTIVE.contains(&other.status.as_str())) {
        return Err(anyhow!("automatic root profile has another active run"));
    }
    if route.harness == "codex-app" {
        // The model catalog was collected before the queued root occupied
        // this profile. Recheck effective project tools in the actual worktree.
        let catalog = d.store.lock().unwrap().auto_model_catalog(&route.profile_id)?
            .ok_or_else(|| anyhow!("automatic root model catalog is unavailable"))?;
        let now_ms = crate::daemon::now();
        if now_ms < catalog.observed_ms || now_ms >= catalog.expires_ms
            || !catalog.models.iter().any(|model| model.model == route.model
                && model.efforts.contains(&route.effort)) {
            return Err(anyhow!("automatic root model catalog changed before launch"));
        }
        let tools = dispatch(d, "auto.tools.inspect", &json!({"profile_id":route.profile_id,
            "workspace_id":workspace.id,"timeout_ms":4000}))?;
        let tool_catalog: crate::auto_route::ToolCatalog =
            serde_json::from_value(tools["catalog"].clone())?;
        if required_tools.iter().any(|tool| !tool_catalog.tools.contains(tool)) {
            return Err(anyhow!("automatic root required tool is unavailable in the selected workspace"));
        }
        let store = d.store.lock().unwrap();
        if store.auto_account_generation(&route.profile_id)? != expected_generation {
            return Err(anyhow!("automatic root account changed during tool preflight"));
        }
        let launch: Option<String> = store.conn.query_row("SELECT launch FROM runs WHERE id=?1",
            [&run.id], |row| row.get(0))?;
        let mut launch: Value = launch.as_deref().and_then(|text| serde_json::from_str(text).ok())
            .ok_or_else(|| anyhow!("automatic root launch metadata is unavailable"))?;
        launch["generic"]["auto_parent_discovery"] = json!({"workspace_id":workspace.id,
            "account_generation":expected_generation,"model_catalog":catalog,
            "tool_catalog":tool_catalog});
        store.conn.execute("UPDATE runs SET launch=?2 WHERE id=?1",
            rusqlite::params![run.id, launch.to_string()])?;
    } else if route.harness == "claude" {
        recheck_claude_account_before_child(d, &route.profile_id,
            expected_generation.ok_or_else(|| anyhow!("Claude account generation unavailable"))?,
            Instant::now() + Duration::from_secs(2))?;
    } else if route.harness == "opencode" {
        let profile = d.profile(&route.profile_id)?;
        crate::auto_opencode::auto_local_inline_config(&profile,
            std::path::Path::new(&workspace.path), &route.model, &route.endpoint)?;
        if crate::auto_opencode::probe_local_endpoint(&route.endpoint)
            != crate::auto_opencode::EndpointProbe::Reachable {
            return Err(anyhow!("selected local OpenCode endpoint is unavailable before root turn"));
        }
    }
    if let Some(observation) = d.store.lock().unwrap().latest_auto_quota(&route.profile_id)? {
        if observation.snapshot.state_for(&route.model, crate::daemon::now())
            == crate::auto_quota::QuotaState::Exhausted {
            return Err(anyhow!("automatic root account allowance is exhausted"));
        }
    }
    d.start_turn(&run.id, &task.prompt, false, &crate::daemon::TurnOpts::default())?;
    d.store.lock().unwrap().set_auto_root_phase(work_unit_id, "running")?;
    auto_root_response(d, work_unit_id, request_hash, false)
}

pub fn dispatch(d: &Arc<Daemon>, method: &str, p: &Value) -> Result<Value> {
    let result = dispatch_inner(d, method, p);
    if method.starts_with("swarm.") && result.as_ref().is_err_and(storage_fault) {
        d.swarm_storage_blocked.store(true, Ordering::SeqCst);
    }
    result
}

fn dispatch_inner(d: &Arc<Daemon>, method: &str, p: &Value) -> Result<Value> {
    Ok(match method {
        "hello" => json!({"protocol": PROTOCOL_VERSION, "version": env!("CARGO_PKG_VERSION"), "pid": std::process::id(), "data_dir": paths::data_dir(), "socket": paths::socket_path()}),
        "state" => d.state_for(p["include_hidden"].as_bool().unwrap_or(false))?,
        "audio.get" => crate::audio::get(d)?,
        "audio.set" => crate::audio::set(d, p)?,
        "audio.preview" => crate::audio::preview(d, p)?,
        "audio.import_commander" => crate::audio::import_commander(d, p)?,
        "audio.voices" => crate::audio::voices()?,
        "harness.list" => {
            let list: Vec<Value> = ["codex", "codex-app", "claude", "opencode", "opencode-serve", "generic"]
                .iter()
                .map(|h| {
                    let program = crate::adapters::resolve_program(h);
                    let version = program.as_deref().and_then(crate::adapters::version_of);
                    json!({"harness": h, "program": program, "version": version, "installed": program.is_some() || *h == "generic", "capabilities": crate::adapters::capabilities(h)})
                })
                .collect();
            json!(list)
        }
        "profile.list" => json!(d.store.lock().unwrap().profiles()?),
        "swarm.create" => crate::swarm::create(&mut d.store.lock().unwrap(), p)?,
        "swarm.start" => {
            require_swarm_storage(d)?;
            crate::swarm::start(d, p)?
        }
        "swarm.native_director.get" => crate::swarm::native::setting(&d.store.lock().unwrap())?,
        "swarm.native_director.set" => crate::swarm::native::set_setting(&d.store.lock().unwrap(), p)?,
        "swarm.director.owner.begin" => {
            fixture_only()?;
            crate::swarm::begin_director_owner(&mut d.store.lock().unwrap(), p)?
        }
        "swarm.director.owner.renew" => {
            fixture_only()?;
            crate::swarm::renew_director_owner(&mut d.store.lock().unwrap(), p)?
        }
        "swarm.director.owner.refresh_linked" => {
            fixture_only()?;
            json!({"renewed":crate::swarm::refresh_linked_director_owners(d)?})
        }
        "swarm.director.owner.expire_due" => {
            fixture_only()?;
            json!({"stalled":crate::swarm::expire_director_owners(&mut d.store.lock().unwrap())?})
        }
        "swarm.director.launch" => {
            fixture_only()?;
            require_swarm_storage(d)?;
            crate::swarm::launch_director(d,p)?
        }
        "swarm.storage.status" => json!({"state":if d.swarm_storage_blocked.load(Ordering::SeqCst) { "blocked" } else { "ready" }}),
        "swarm.storage.recover" => {
            let _serial = d.swarm_launch_lock.lock().unwrap();
            let mut store = d.store.lock().unwrap();
            store.probe_swarm_write_capacity()?;
            d.swarm_storage_blocked.store(false, Ordering::SeqCst);
            json!({"state":"ready"})
        }
        "swarm.storage.limit_pages" => {
            fixture_only()?;
            let store = d.store.lock().unwrap();
            let pages: i64 = match p["mode"].as_str() {
                Some("current") => store.conn.pragma_query_value(None, "page_count", |r| r.get(0))?,
                Some("unlimited") => 4_294_967_294,
                _ => bail!("invalid storage limit mode"),
            };
            store.conn.pragma_update(None, "max_page_count", pages)?;
            json!({"max_page_count":pages})
        }
        "swarm.get" => crate::swarm::get(&d.store.lock().unwrap(), s(p, "id")?)?,
        "swarm.list" => crate::swarm::list(&d.store.lock().unwrap(), p)?,
        "swarm.plan" => {
            fixture_only()?;
            crate::swarm::plan(&mut d.store.lock().unwrap(), p)?
        }
        "swarm.jobs" => crate::swarm::jobs(&d.store.lock().unwrap(), p)?,
        "swarm.coverage" => crate::swarm::coverage_report(&d.store.lock().unwrap(), p)?,
        "swarm.attempt.register" => {
            fixture_only()?;
            require_swarm_storage(d)?;
            crate::swarm::register(&mut d.store.lock().unwrap(), p)?
        }
        "swarm.report" => crate::swarm::report(&mut d.store.lock().unwrap(), p)?,
        "swarm.direct" => {
            fixture_only()?;
            crate::swarm::direct(&mut d.store.lock().unwrap(), p)?
        }
        "swarm.messages" => {
            fixture_only()?;
            crate::swarm::messages(&d.store.lock().unwrap(), p)?
        }
        "swarm.ack" => {
            if p["recipient"] == "director" {
                fixture_only()?;
            }
            crate::swarm::ack(&mut d.store.lock().unwrap(), p)?
        }
        "swarm.stop" => {
            let fault_interrupt_once = p["fault_interrupt_once"] == true;
            if fault_interrupt_once { fixture_only()?; }
            let mut stopped = {
                let _serial = d.swarm_launch_lock.lock().unwrap();
                crate::swarm::stop(&mut d.store.lock().unwrap(), p)?
            };
            // The stop transition is committed before socket control. Keeping the
            // launch lock during an unresponsive worker's I/O would hold unrelated
            // admission and safety controls behind that worker.
            stopped["workers"] = crate::swarm::interrupt_workers_with_fault(
                d, s(p,"run_id")?, fault_interrupt_once)?;
            stopped
        }
        "swarm.partial" => {
            fixture_only()?;
            require_swarm_storage(d)?;
            let mut closed = {
                let _serial = d.swarm_launch_lock.lock().unwrap();
                crate::swarm::partial(&mut d.store.lock().unwrap(), p)?
            };
            closed["workers"] = crate::swarm::interrupt_workers(d, s(p,"run_id")?)?;
            closed
        }
        "swarm.pause" => crate::swarm::pause(&mut d.store.lock().unwrap(), p)?,
        "swarm.resume" => crate::swarm::resume(&mut d.store.lock().unwrap(), p)?,
        "swarm.off" => crate::swarm::off(&mut d.store.lock().unwrap(), p)?,
        "swarm.claim" => {
            fixture_only()?;
            let claim = crate::swarm::claim(&mut d.store.lock().unwrap(), p)?;
            if claim["status"] == "contaminated" {
                if let Err(error) = crate::swarm::retry_targeted_interrupts(d) {
                    crate::log(&format!("swarm contamination interrupt failed: {error}"));
                }
            }
            claim
        }
        "swarm.artifact.put" => crate::swarm::put(&mut d.store.lock().unwrap(), p)?,
        "swarm.integrate" => {
            fixture_only()?;
            let _serial = d.swarm_integration_lock.lock().unwrap();
            let mut store = crate::store::Store::connect_existing(&paths::db_path())?;
            crate::swarm::integrate(&mut store, p)?
        }
        "swarm.verify" => {
            fixture_only()?;
            let prepared = {
                let mut store = d.store.lock().unwrap();
                crate::swarm::prepare_verification(&mut store, p)?
            };
            match prepared {
                crate::swarm::PreparedVerification::Existing(result) => result,
                crate::swarm::PreparedVerification::Ready(plan) => {
                    let outcome = crate::swarm::run_verification(&plan);
                    crate::swarm::record_verification(&mut d.store.lock().unwrap(), &plan, outcome)?
                }
            }
        }
        "swarm.decide" => {
            fixture_only()?;
            crate::swarm::decide(&mut d.store.lock().unwrap(), p)?
        }
        "swarm.conflict.open" => {
            fixture_only()?;
            let opened = crate::swarm::open_conflict(&mut d.store.lock().unwrap(), p)?;
            if let Err(error) = crate::swarm::retry_targeted_interrupts(d) {
                crate::log(&format!("swarm evidence conflict interrupt failed: {error}"));
            }
            opened
        }
        "swarm.conflict.resolve" => {
            fixture_only()?;
            crate::swarm::resolve_conflict(&mut d.store.lock().unwrap(), p)?
        }
        "swarm.conflicts" => crate::swarm::list_conflicts(&d.store.lock().unwrap(), p)?,
        "swarm.complete" => {
            fixture_only()?;
            crate::swarm::complete(&mut d.store.lock().unwrap(), p)?
        }
        "swarm.attempt.confirm_exit" => {
            fixture_only()?;
            crate::swarm::confirm_exit(&mut d.store.lock().unwrap(), p)?
        }
        "swarm.revise" => {
            fixture_only()?;
            crate::swarm::revise(&mut d.store.lock().unwrap(), p)?
        }
        "swarm.policy.preview" => crate::swarm::preview(p)?,
        "swarm.benefit.preview" => {
            fixture_only()?;
            crate::swarm::preview_benefit(p)?
        }
        "swarm.benefit.commit" => {
            fixture_only()?;
            crate::swarm::commit_benefit(&mut d.store.lock().unwrap(), p)?
        }
        "swarm.availability.observe" => {
            fixture_only()?;
            let mut observation=crate::swarm::observe_availability(&mut d.store.lock().unwrap(), p)?;
            if observation["revoked_jobs"].as_array().is_some_and(|jobs| !jobs.is_empty()) {
                observation["workers"]=crate::swarm::retry_targeted_interrupts(d)?;
            }
            observation
        }
        "swarm.policy.set" => crate::swarm::set_policy(&mut d.store.lock().unwrap(), p)?,
        "swarm.estimate.revoke" => {
            let _serial = d.swarm_launch_lock.lock().unwrap();
            crate::swarm::revoke_estimated_quota(&mut d.store.lock().unwrap(), p)?
        }
        "swarm.targets.set" => {
            fixture_only()?;
            let _serial = d.swarm_launch_lock.lock().unwrap();
            crate::swarm::set_run_targets(&mut d.store.lock().unwrap(), p)?
        }
        "swarm.limit.set" => {
            let _serial = d.swarm_launch_lock.lock().unwrap();
            crate::swarm::set_run_limit(&mut d.store.lock().unwrap(), p)?
        }
        "swarm.deadline.extend" => {
            let _serial = d.swarm_launch_lock.lock().unwrap();
            require_swarm_storage(d)?;
            crate::swarm::extend_deadline(&mut d.store.lock().unwrap(), p)?
        }
        "swarm.admit" => {
            fixture_only()?;
            let _serial = d.swarm_launch_lock.lock().unwrap();
            require_swarm_storage(d)?;
            crate::swarm::admit(&mut d.store.lock().unwrap(), p)?
        }
        "swarm.schedule.next" => {
            fixture_only()?;
            let _serial = d.swarm_launch_lock.lock().unwrap();
            require_swarm_storage(d)?;
            crate::swarm::schedule_next(&mut d.store.lock().unwrap(), p)?
        }
        "swarm.dispatch.next" => {
            fixture_only()?;
            require_swarm_storage(d)?;
            crate::swarm::dispatch_next(d, p)?
        }
        "swarm.worker.launch" => {
            fixture_only()?;
            require_swarm_storage(d)?;
            crate::swarm::launch_worker(d, p)?
        }
        "swarm.effect.begin" => {
            fixture_only()?;
            let begun = crate::swarm::begin_effect(&mut d.store.lock().unwrap(), p)?;
            if p["fixture_drop_ack_after_commit"] == true {
                return Err(anyhow!("injected effect acknowledgement loss after commit"));
            }
            begun
        }
        "swarm.effect.reconcile" => {
            fixture_only()?;
            crate::swarm::reconcile_effect(&mut d.store.lock().unwrap(), p)?
        }
        "swarm.worker.brief" => {
            fixture_only()?;
            crate::swarm::worker_brief(&d.store.lock().unwrap(), p)?
        }
        "swarm.context.get" => {
            fixture_only()?;
            crate::swarm::artifact_chunk(&d.store.lock().unwrap(), p)?
        }
        "swarm.context.revoke" => {
            fixture_only()?;
            crate::swarm::revoke_artifact(d, p)?
        }
        "swarm.context.grant" => {
            fixture_only()?;
            crate::swarm::grant_artifact(d, p)?
        }
        "swarm.director.summary" => {
            fixture_only()?;
            crate::swarm::director_summary(&d.store.lock().unwrap(), p)?
        }
        "swarm.worker.reconcile" => {
            fixture_only()?;
            crate::swarm::reconcile_worker(d, p)?
        }
        "swarm.worker.liveness" => crate::swarm::liveness(&d.store.lock().unwrap(), p)?,
        "swarm.worker.liveness.sample" => {
            fixture_only()?;
            crate::swarm::sample_liveness(&mut d.store.lock().unwrap(), p)?
        }
        "swarm.worker.liveness.poll" => {
            fixture_only()?;
            let now_ms = p["now_ms"]
                .as_i64()
                .ok_or_else(|| anyhow!("missing poll time"))?;
            if now_ms < 0 {
                return Err(anyhow!("invalid poll time"));
            }
            json!({"sampled": crate::swarm::sample_due_workers(d, now_ms)?})
        }
        "swarm.job.deadline.persist_due" => {
            fixture_only()?;
            let now_ms = p["now_ms"]
                .as_i64()
                .ok_or_else(|| anyhow!("missing deadline time"))?;
            if now_ms < 0 {
                return Err(anyhow!("invalid deadline time"));
            }
            // Fault seam: commit the timeout and stop message but leave the external
            // interrupt unsent, as if the daemon died between these two steps.
            let pending = crate::swarm::expire_jobs_due(&mut d.store.lock().unwrap(), now_ms)?;
            json!({"interrupt_pending": pending})
        }
        "swarm.redirect.persist_due" => {
            fixture_only()?;
            let now_ms = p["now_ms"].as_i64()
                .ok_or_else(|| anyhow!("missing deadline time"))?;
            if now_ms < 0 { return Err(anyhow!("invalid deadline time")); }
            // Fault seam: persist timeout/checkpoint but leave the external
            // interrupt unsent, as if the daemon died before signaling it.
            crate::swarm::expire_redirects_due(&mut d.store.lock().unwrap(), now_ms)?
        }
        "swarm.director.claim_batch" => {
            fixture_only()?;
            crate::swarm::claim_batch(&mut d.store.lock().unwrap(), p)?
        }
        "swarm.director.complete_batch" => {
            fixture_only()?;
            crate::swarm::complete_batch(&mut d.store.lock().unwrap(), p)?
        }
        "swarm.director.recover" => {
            fixture_only()?;
            crate::swarm::recover(&mut d.store.lock().unwrap(), p)?
        }
        "profile.create" => json!(d.create_profile(s(p, "name")?, s(p, "harness")?)?),
        "agents.limit.get" => {
            let store = d.store.lock().unwrap();
            json!({"max_active": store.agent_limit()?, "active": store.active_agent_count()?})
        }
        "agents.limit.set" => {
            let limit = p["max_active"].as_i64().ok_or_else(|| anyhow!("max_active must be an integer"))?;
            let store = d.store.lock().unwrap();
            store.set_agent_limit(limit)?;
            json!({"max_active": store.agent_limit()?, "active": store.active_agent_count()?})
        }
        "profile.rename" => {
            let name = s(p, "name")?.trim();
            if name.is_empty() || name.len() > 80 {
                return Err(anyhow!("profile name must be 1-80 characters"));
            }
            d.store.lock().unwrap().rename_profile(s(p, "id")?, name)?;
            json!({"ok": true})
        }
        "profile.status" => d.profile_status(s(p, "id")?)?,
        "profile.login_command" => d.login_command(s(p, "id")?, p["device"].as_bool().unwrap_or(false))?,
        "profile.logout" => d.logout(s(p, "id")?)?,
        "repo.inspect" => {
            let root = crate::git::toplevel(std::path::Path::new(s(p, "path")?))?;
            json!({"root": root, "head": crate::git::head(&root), "branch": crate::git::head_branch(&root), "default_branch": crate::git::default_branch(&root),
                "branches": crate::git::branches(&root), "status": crate::git::status(&root)?})
        }
        "task.create" => d.create_task(p)?,
        "auto.root.preview" => auto_root_preview(d, p)?,
        "auto.start" => {
            use sha2::{Digest, Sha256};
            use crate::store::{Run, Task, Workspace};
            let fields = p.as_object().ok_or_else(|| anyhow!("automatic root request must be an object"))?;
            for field in fields.keys() {
                if !matches!(field.as_str(), "repo" | "work_unit_id" | "allowed_profiles" | "min_tier"
                    | "required_tools" | "context_needed" | "requires_approvals" | "sandbox"
                    | "pinned_route" | "preferred_harness" | "task_class" | "execution_budget_ms"
                    | "prompt" | "title" | "workspace_mode" | "target_ref" | "approval_policy") {
                    return Err(anyhow!("unsupported automatic root launch field: {field}"));
                }
            }
            let work_unit_id = s(p, "work_unit_id")?;
            if work_unit_id.is_empty() || work_unit_id.len() > 120 || !work_unit_id.bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')) {
                return Err(anyhow!("invalid automatic root work-unit identity"));
            }
            let prompt = s(p, "prompt")?;
            if prompt.is_empty() || prompt.len() > 32_768 {
                return Err(anyhow!("automatic root prompt must be 1-32768 bytes"));
            }
            let title = p["title"].as_str().unwrap_or_default();
            let title = if title.is_empty() { prompt.chars().take(60).collect::<String>() }
                else { title.to_string() };
            if title.trim().is_empty() || title.len() > 80 {
                return Err(anyhow!("automatic root title must be 1-80 bytes"));
            }
            let mode = p["workspace_mode"].as_str().unwrap_or("worktree");
            if !matches!(mode, "worktree" | "current") {
                return Err(anyhow!("automatic root workspace mode is unsupported"));
            }
            let approval = p["approval_policy"].as_str().unwrap_or("on-request");
            if !matches!(approval, "on-request" | "never") {
                return Err(anyhow!("automatic root approval policy is unsupported"));
            }
            if approval == "never" && p["requires_approvals"] == true {
                return Err(anyhow!("automatic root requires approvals disabled by its launch policy"));
            }
            let target_ref = match p.get("target_ref") {
                None => None,
                Some(Value::String(value)) if !value.is_empty() && value.len() <= 200
                    && !value.starts_with('-') => Some(value.clone()),
                _ => return Err(anyhow!("automatic root target_ref is invalid")),
            };
            let repo = crate::git::toplevel(std::path::Path::new(s(p, "repo")?))?;
            let mut preview_request = p.clone();
            for field in ["prompt", "title", "workspace_mode", "target_ref", "approval_policy"] {
                preview_request.as_object_mut().unwrap().remove(field);
            }
            preview_request["repo"] = json!(repo);
            let request_hash = format!("{:x}", Sha256::digest(serde_json::to_vec(&json!({
                "repo":repo,"preview":preview_request,"prompt":prompt,"title":title,
                "workspace_mode":mode,"target_ref":target_ref,"approval_policy":approval
            }))?));
            let gate = d.work_unit_gate(work_unit_id);
            let _guard = gate.lock().unwrap();
            if d.store.lock().unwrap().auto_root_intent(work_unit_id)?.is_some() {
                return auto_root_response(d, work_unit_id, &request_hash, true);
            }
            if d.store.lock().unwrap().auto_launch_intent(work_unit_id)?.is_some() {
                return Err(anyhow!("automatic work-unit identity was already used by a child"));
            }
            let preview = auto_root_preview(d, &preview_request)?;
            let decision = &preview["decision"];
            if decision["selected"].is_null() {
                return Ok(json!({"state":"paused","work_unit_id":work_unit_id,
                    "decision":decision,"discovery_failures":preview["discovery_failures"],
                    "actions":["refresh","choose_manual_route"]}));
            }
            let route: crate::auto_select::Route =
                serde_json::from_value(preview["selected_route"].clone())?;
            let work: crate::auto_select::WorkUnit = serde_json::from_value(
                preview["trace"]["selection_input"]["work"].clone())?;
            let generation = preview["evidence"].as_array().and_then(|items| items.iter()
                .find(|item| item["profile_id"] == route.profile_id))
                .and_then(|item| item["account_generation"].as_i64());
            if route.harness != "opencode" && generation.is_none() {
                return Err(anyhow!("selected automatic root account generation is unavailable"));
            }
            let common = crate::git::common_dir(&repo)?;
            let start_name = target_ref.as_deref().unwrap_or("HEAD");
            let start_sha = crate::git::rev_parse(&repo, start_name)
                .ok_or_else(|| anyhow!("automatic root starting ref is unavailable"))?;
            let (path, branch, initial_dirty) = if mode == "worktree" {
                let repo_name = repo.file_name().map(|name| name.to_string_lossy().to_string())
                    .unwrap_or_else(|| "repo".into());
                let common_hash = format!("{:x}", Sha256::digest(common.display().to_string().as_bytes()));
                let parent = crate::paths::worktrees_dir().join(format!("{repo_name}-{}", &common_hash[..8]));
                let (path, branch) = crate::git::plan_worktree_add(&repo, &parent, &title)?;
                (path, Some(branch), json!({"clean":true}))
            } else {
                let status = crate::git::status(&repo)?;
                (repo.clone(), status.branch.clone(), serde_json::to_value(status)?)
            };
            let stamp = crate::daemon::now();
            let workspace = Workspace { id:format!("w-{}", uuid::Uuid::new_v4().simple()),
                path:path.display().to_string(), repo_root:repo.display().to_string(),
                common_dir:common.display().to_string(), kind:mode.into(), branch,
                owner_run_id:None, initial_dirty, created_ms:stamp, removed_ms:None };
            let task = Task { id:format!("t-{}", uuid::Uuid::new_v4().simple()),
                title:title.clone(), prompt:prompt.into(), repo_root:workspace.repo_root.clone(),
                target_ref, workspace_id:workspace.id.clone(), start_snapshot:None,
                fork_commit:Some(start_sha), fork_provenance:Some("recorded: Auto root start ref".into()),
                created_ms:stamp, archived_ms:None };
            let profile = d.profile(&route.profile_id)?;
            let expected_harness = if route.harness == "codex-app" { "codex" }
                else { route.harness.as_str() };
            if profile.harness != expected_harness {
                return Err(anyhow!("selected automatic root profile changed harness"));
            }
            let run = Run { id:format!("r-{}", uuid::Uuid::new_v4().simple()),
                task_id:task.id.clone(), parent_run_id:None, harness:route.harness.clone(),
                harness_version:crate::adapters::resolve_program(&route.harness)
                    .and_then(|program| crate::adapters::version_of(&program)),
                profile_id:Some(route.profile_id.clone()), model:Some(route.model.clone()),
                effort:Some(route.effort.clone()), workspace_id:workspace.id.clone(),
                native_id:None, status:"queued".into(), exit_reason:None, created_ms:stamp,
                ended_ms:None, title:title.clone(), relation_source:None,
                relation_confidence:None,
                capabilities:crate::adapters::capabilities(&route.harness),
                process_generation:0, attention:None };
            let auto_routing = route.harness == "codex-app";
            let sandbox = if work.min_sandbox == crate::auto_select::Sandbox::ReadOnly {
                "read-only" } else { "workspace-write" };
            let launch = json!({"generic":{"approval":approval,"sandbox":sandbox,
                "auto_selected":true,"expected_account_generation":generation,
                "required_tools":work.required_tools,"execution_budget_ms":work.execution_budget_ms,
                "auto_local_endpoint":if route.harness == "opencode" {
                    Some(route.endpoint.as_str()) } else { None },
                "auto_routing":auto_routing,
                "auto_allowed_profiles":if auto_routing { Some(work.allowed_profiles.iter()
                    .cloned().collect::<Vec<_>>()) } else { None },
                "auto_parent_budget_ms":crate::daemon::DEFAULT_AUTO_PARENT_BUDGET_MS}});
            let admitted = match d.store.lock().unwrap().insert_auto_root_selected(work_unit_id,
                &request_hash, &route.id, &route.pool_id, generation,
                &workspace, &task, &run, &launch, &preview["trace"]) {
                Err(error) if error.downcast_ref::<crate::daemon::AutoDrawExceedsAllowance>().is_some()
                    || error.downcast_ref::<crate::daemon::AutoEndpointRecoveryInFlight>().is_some() => {
                    let reason = if error.downcast_ref::<crate::daemon::AutoDrawExceedsAllowance>().is_some() {
                        "estimated_draw_exceeds_allowance" } else { "endpoint_recovery_in_progress" };
                    let mut paused: crate::auto_select::Decision = serde_json::from_value(decision.clone())?;
                    paused.selected = None;
                    paused.reason = reason.into();
                    paused.exclusions.push(crate::auto_select::Exclusion {
                        route_id: route.id.clone(), reason: reason.into(),
                    });
                    return Ok(json!({"state":"paused","work_unit_id":work_unit_id,
                        "decision":paused,"pause_reason":reason,
                        "actions":["refresh","choose_manual_route"]}));
                }
                other => other?,
            };
            if admitted.is_none() {
                return Ok(json!({"state":"paused","work_unit_id":work_unit_id,
                    "decision":decision,"pause_reason":"admission_conflict",
                    "actions":["refresh","choose_manual_route"]}));
            }
            let id = work_unit_id.to_string();
            let worker_daemon = d.clone();
            let worker_hash = request_hash.clone();
            let worker_route = route.clone();
            let required = work.required_tools.into_iter().collect::<Vec<_>>();
            let runtime = tokio::runtime::Handle::current();
            auto_launches().lock().unwrap().insert(id.clone());
            let spawn = std::thread::Builder::new().name("auto-root-launch".into()).spawn(move || {
                let _active = ActiveAutoLaunch(id.clone());
                let _runtime = runtime.enter();
                if let Err(error) = auto_root_launch_worker(&worker_daemon, &id, &worker_route,
                    &required, generation, &worker_hash) {
                    let store = worker_daemon.store.lock().unwrap();
                    let _ = store.set_auto_root_phase(&id, "paused");
                    if let Ok(Some(intent)) = store.auto_root_intent(&id) {
                        let _ = store.conn.execute("UPDATE runs SET status='unknown',
                            exit_reason=?2 WHERE id=?1 AND run_dir IS NULL
                            AND status IN ('queued','starting')",
                            rusqlite::params![intent.run_id, format!("automatic root launch paused: {error}")]);
                    }
                    let _ = store.release_unstarted_auto_root_pool_claim(&id);
                }
            });
            if spawn.is_err() {
                auto_launches().lock().unwrap().remove(work_unit_id);
                d.store.lock().unwrap().set_auto_root_phase(work_unit_id, "paused")?;
            }
            auto_root_response(d, work_unit_id, &request_hash, false)?
        }
        "run.delegate" => d.delegate_run(p, false)?,
        // The two pre-merge handoff contracts share a method name. A completed
        // Auto checkpoint names its source; Continuity names the waiting run.
        // Keep both request shapes until they have a single admission owner.
        "run.handoff" if p.get("source_run_id").is_some() => d.handoff_run(p)?,
        "auto.mode.get" => json!({"enabled":d.store.lock().unwrap().auto_mode_enabled()?}),
        "auto.mode.set" => {
            if p.as_object().is_none_or(|fields| fields.len() != 1)
                || !p["enabled"].is_boolean() {
                return Err(anyhow!("enabled must be a boolean"));
            }
            let enabled = p["enabled"].as_bool().unwrap();
            d.store.lock().unwrap().set_auto_mode_enabled(enabled)?;
            json!({"enabled":enabled})
        }
        "auto.bridge.submit" => {
            let (parent, generic) = auto_bridge_parent(d, p)?;
            if parent.status != "running" || !d.store.lock().unwrap().auto_mode_enabled()? {
                return Err(anyhow!("Auto parent is not running or Auto Mode is disabled"));
            }
            if p.get("parent_run_id").is_some() || p.get("allowed_profiles").is_some()
                || p.get("pinned_route").is_some() || p.get("preferred_harness").is_some() {
                return Err(anyhow!("Auto bridge cannot change its parent or account authority"));
            }
            if generic["sandbox"] == "read-only" && p["sandbox"] != "read_only" {
                return Err(anyhow!("Auto child cannot widen the parent's read-only sandbox"));
            }
            if generic["approval"] == "never" && p["requires_approvals"] == true {
                return Err(anyhow!("Auto child cannot require approvals disabled by its parent"));
            }
            let mut request = p.clone();
            let fields = request.as_object_mut().ok_or_else(|| anyhow!("Auto bridge request must be an object"))?;
            fields.remove("run_id");
            fields.remove("capability");
            fields.insert("parent_run_id".into(), json!(parent.id));
            fields.insert("allowed_profiles".into(), generic["auto_allowed_profiles"].clone());
            let budget = match request.get("execution_budget_ms") {
                None => crate::daemon::DEFAULT_AUTO_EXECUTION_BUDGET_MS,
                Some(value) => value.as_u64().ok_or_else(|| anyhow!("invalid child execution budget"))?,
            };
            if !(1_000..=crate::daemon::DEFAULT_AUTO_EXECUTION_BUDGET_MS).contains(&budget) {
                return Err(anyhow!("child execution budget exceeds the Auto parent limit"));
            }
            request["execution_budget_ms"] = json!(budget);
            dispatch(d, "auto.dispatch", &request)?
        }
        "auto.bridge.result" => {
            let (parent, _) = auto_bridge_parent(d, p)?;
            if p.as_object().is_none_or(|fields| fields.keys().any(|field|
                !matches!(field.as_str(), "run_id" | "capability" | "child_run_id"))) {
                return Err(anyhow!("unsupported Auto bridge result field"));
            }
            let child = d.run(s(p, "child_run_id")?)?;
            if child.parent_run_id.as_deref() != Some(parent.id.as_str())
                || child.relation_source.as_deref() != Some("managed-delegation") {
                return Err(anyhow!("child result is outside this Auto parent"));
            }
            d.delegated_result(&child.id)?
        }
        "auto.dispatch" => {
            use crate::auto_select::{CapabilityTier, Sandbox, WorkUnit};
            use sha2::{Digest, Sha256};
            use std::collections::{BTreeMap, BTreeSet};

            for field in p.as_object().ok_or_else(|| anyhow!("automatic work request must be an object"))?.keys() {
                if !matches!(field.as_str(), "work_unit_id" | "parent_run_id" | "prompt" | "title"
                    | "min_tier" | "required_tools" | "context_needed" | "requires_approvals"
                    | "pinned_route" | "preferred_harness" | "allowed_profiles" | "sandbox"
                    | "execution_budget_ms" | "task_class") {
                    return Err(anyhow!("unsupported automatic work constraint: {field}"));
                }
            }
            let work_unit_id = s(p, "work_unit_id")?;
            if work_unit_id.is_empty() || work_unit_id.len() > 120
                || !work_unit_id.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')) {
                return Err(anyhow!("invalid automatic work-unit identity"));
            }
            let work_unit_gate = d.work_unit_gate(work_unit_id);
            let _work_unit_guard = work_unit_gate.lock().unwrap();
            let parent = d.run(s(p, "parent_run_id")?)?;
            if parent.parent_run_id.is_some() && parent.relation_source.as_deref() != Some("managed-continuation") {
                return Err(anyhow!("automatic delegation requires a top-level parent"));
            }
            let profile_id = parent.profile_id.as_deref().ok_or_else(|| anyhow!("parent has no account profile"))?;
            let profile = d.profile(profile_id)?;
            if !matches!(profile.harness.as_str(), "codex" | "claude" | "opencode") {
                return Err(anyhow!("automatic route discovery is not yet supported for this harness"));
            }
            let prompt = s(p, "prompt")?;
            if prompt.is_empty() || prompt.len() > 32_768 { return Err(anyhow!("automatic work prompt is invalid")); }
            let title = p["title"].as_str().unwrap_or("delegated work").trim();
            if title.is_empty() || title.len() > 80 { return Err(anyhow!("automatic work title is invalid")); }
            let min_tier: CapabilityTier = serde_json::from_value(p.get("min_tier").cloned()
                .ok_or_else(|| anyhow!("min_tier is required"))?)?;
            let tool_values = p.get("required_tools").and_then(Value::as_array)
                .ok_or_else(|| anyhow!("required_tools must be a list"))?;
            if tool_values.len() > 16 { return Err(anyhow!("too many required tools")); }
            let mut required_tools = BTreeSet::new();
            for value in tool_values {
                let tool = value.as_str().filter(|name| crate::daemon::valid_required_tool(name))
                    .ok_or_else(|| anyhow!("invalid required tool"))?;
                if !required_tools.insert(tool.to_string()) { return Err(anyhow!("duplicate required tool")); }
            }
            let context_needed = match p.get("context_needed") {
                None => 0,
                Some(value) => value.as_u64().ok_or_else(|| anyhow!("context_needed must be a nonnegative integer"))?,
            };
            let requires_approvals = match p.get("requires_approvals") {
                None => false,
                Some(value) => value.as_bool().ok_or_else(|| anyhow!("requires_approvals must be a boolean"))?,
            };
            let execution_budget_ms = match p.get("execution_budget_ms") {
                None => crate::daemon::DEFAULT_AUTO_EXECUTION_BUDGET_MS,
                Some(value) => value.as_u64().filter(|ms| (1_000..=1_800_000).contains(ms))
                    .ok_or_else(|| anyhow!("execution_budget_ms must be 1000-1800000"))?,
            };
            // A broad product category can scope learning, but free-text
            // labels must not copy task content into the usage store.
            let task_class = match p.get("task_class") {
                None => None,
                Some(Value::String(value)) if matches!(value.as_str(),
                    "browser_check" | "routine_edit" | "difficult_diagnosis" | "general") =>
                    Some(value.clone()),
                _ => return Err(anyhow!("task_class must be a supported broad work category")),
            };
            let sandbox = match p.get("sandbox") {
                None => Sandbox::WorkspaceWrite,
                Some(value) => {
                    let requested: Sandbox = serde_json::from_value(value.clone())?;
                    if !matches!(requested, Sandbox::ReadOnly | Sandbox::WorkspaceWrite) {
                        return Err(anyhow!("automatic sandbox is unsupported"));
                    }
                    requested
                }
            };
            let pinned_route = match p.get("pinned_route") {
                None => None,
                Some(value) => Some(value.as_str().filter(|s| !s.is_empty() && s.len() <= 300)
                    .ok_or_else(|| anyhow!("pinned_route must be a route ID"))?.to_string()),
            };
            let preferred_harness = match p.get("preferred_harness") {
                None => None,
                Some(value) => Some(value.as_str().filter(|s| !s.is_empty() && s.len() <= 40)
                    .ok_or_else(|| anyhow!("preferred_harness must be a harness name"))?.to_string()),
            };
            let explicit_allowed_profiles = match p.get("allowed_profiles") {
                None => None,
                Some(Value::Array(values)) if !values.is_empty() && values.len() <= 8 => {
                    let mut allowed = BTreeSet::new();
                    for value in values {
                        let id = value.as_str().filter(|id| !id.is_empty() && id.len() <= 120)
                            .ok_or_else(|| anyhow!("invalid allowed account profile"))?;
                        let candidate = d.profile(id)?;
                        if !matches!(candidate.harness.as_str(), "codex" | "claude" | "opencode") {
                            return Err(anyhow!("automatic route discovery is not yet supported for this account profile"));
                        }
                        if !allowed.insert(id.to_string()) { return Err(anyhow!("duplicate allowed account profile")); }
                    }
                    Some(allowed)
                }
                _ => return Err(anyhow!("allowed_profiles must be a bounded nonempty account list")),
            };
            let allowed_profiles = explicit_allowed_profiles.clone()
                .unwrap_or_else(|| BTreeSet::from([profile_id.to_string()]));
            let requirements = json!({"work_unit_id":work_unit_id,"parent_run_id":parent.id,
                "profile_id":profile_id,"prompt":prompt,"title":title,"min_tier":min_tier,
                "required_tools":required_tools,"context_needed":context_needed,
                "requires_approvals":requires_approvals,"pinned_route":pinned_route,
                "preferred_harness":preferred_harness});
            let mut requirements = requirements;
            if let Some(explicit) = &explicit_allowed_profiles { requirements["allowed_profiles"] = json!(explicit); }
            if p.get("sandbox").is_some() { requirements["sandbox"] = json!(sandbox); }
            if p.get("execution_budget_ms").is_some() {
                requirements["execution_budget_ms"] = json!(execution_budget_ms);
            }
            if let Some(class) = &task_class { requirements["task_class"] = json!(class); }
            let requirements_hash = Sha256::digest(serde_json::to_vec(&requirements)?)
                .iter().map(|byte| format!("{byte:02x}")).collect::<String>();
            let saved_work_unit = {
                let store = d.store.lock().unwrap();
                store.managed_work_unit(work_unit_id)?
            };
            let saved_intent = {
                let store = d.store.lock().unwrap();
                store.auto_launch_intent(work_unit_id)?
            };
            if let Some((saved_parent, child_id, _)) = saved_work_unit {
                if saved_parent != parent.id { return Err(anyhow!("work unit belongs to another parent")); }
                let child = d.run(&child_id)?;
                let launch: Option<String> = d.store.lock().unwrap().conn.query_row(
                    "SELECT launch FROM runs WHERE id=?1", [&child_id], |row| row.get(0))?;
                let launch: Value = launch.as_deref().and_then(|value| serde_json::from_str(value).ok()).unwrap_or(Value::Null);
                if launch["generic"]["requirements_hash"] != requirements_hash
                    && launch["requirements_hash"] != requirements_hash {
                    return Err(anyhow!("work unit was already used for different automatic work"));
                }
                let route_id = format!("{}/{}/{}", child.profile_id.as_deref().unwrap_or("unknown"),
                    child.model.as_deref().unwrap_or("unknown"), child.effort.as_deref().unwrap_or("unknown"));
                // The child row is committed before the launch notice and
                // supervisor are started. A queued row alone is not proof that
                // any harness process exists, including after a lost response.
                if child.process_generation == 0 && auto_launch_active(work_unit_id) {
                    return Ok(auto_pending_response(work_unit_id, &route_id, true));
                }
                let unstarted = child.process_generation == 0
                    && (ACTIVE.contains(&child.status.as_str())
                        || matches!(child.exit_reason.as_deref(),
                            Some("daemon stopped before the run was launched" |
                                "delegated launch stopped before model turn")));
                let replay_state = if (!ACTIVE.contains(&child.status.as_str())
                    && child.status != "completed") || unstarted {
                    "paused"
                } else { "dispatched" };
                let mut replay = json!({"state":replay_state,"work_unit_id":work_unit_id,"run":child,
                    "workspace":d.workspace(&child.workspace_id)?,"replayed":true,
                    "decision":{"work_unit_id":work_unit_id,"selected":route_id,"exclusions":[],"reason":"replayed_existing_work_unit"}});
                if replay_state == "paused" {
                    if unstarted {
                        replay["pause_reason"] = json!("launch_effects_uncertain");
                        replay["actions"] = json!(["inspect_launch", "choose_manual_route"]);
                    } else {
                        replay["actions"] = json!(["refresh", "choose_manual_route"]);
                    }
                }
                replay
            } else if let Some((intent_parent, intent_hash, route_id, _, phase)) = saved_intent {
                if intent_parent != parent.id || intent_hash != requirements_hash {
                    return Err(anyhow!("work unit was already used for different automatic work"));
                }
                if phase == "preparing" && auto_launch_active(work_unit_id) {
                    return Ok(auto_pending_response(work_unit_id, &route_id, true));
                }
                // Preparation may already have changed Git even when no child row
                // survived. Replaying the same request must not repeat that effect.
                json!({"state":"paused","work_unit_id":work_unit_id,"replayed":true,
                    "pause_reason":"launch_effects_uncertain","launch_phase":phase,
                    "launch_resources":auto_launch_resources(d, work_unit_id)?,
                    "decision":{"work_unit_id":work_unit_id,"selected":route_id,
                        "exclusions":[],"reason":"replayed_unsettled_launch"},
                    "actions":["inspect_launch","choose_manual_route"]})
            } else {
                if !d.store.lock().unwrap().auto_mode_enabled()? {
                    return Err(anyhow!("Auto Mode is disabled; enable it before dispatch"));
                }
                if !matches!(parent.status.as_str(), "running" | "completed") {
                    return Err(anyhow!("automatic delegation requires a running or completed top-level parent"));
                }
                let mut routes = Vec::new();
                let mut evidence = Vec::new();
                let mut discovery_failures = Vec::new();
                let mut account_generations = BTreeMap::new();
                let decision_deadline = Instant::now() + Duration::from_secs(10);
                let candidate_ids = allowed_profiles.iter().cloned().collect::<Vec<_>>();
                for (candidate_id, discovered) in collect_unique_bounded(&candidate_ids,
                    Duration::from_secs(8).min(decision_deadline.saturating_duration_since(Instant::now())), |id, budget|
                        discover_auto_profile_shared(d, id, &parent.workspace_id, &parent.id, None, budget)) {
                    match discovered {
                        Ok(discovered) => {
                            routes.extend(discovered.routes);
                            if let Some(generation) = discovered.generation {
                                account_generations.insert(candidate_id, generation);
                            }
                            evidence.push(discovered.evidence);
                        }
                        Err(_) => discovery_failures.push(json!({"profile_id":candidate_id,
                            "reason":"metadata_or_auth_unavailable"})),
                    }
                }
                if routes.len() > 128 { return Err(anyhow!("automatic candidate catalog exceeded its bound")); }
                let health_now = crate::daemon::now();
                let mut health_observations = match crate::auto_health::recent_local_observations(
                    &d.store.lock().unwrap(), health_now) {
                    Ok(observations) => observations,
                    Err(_) => {
                        discovery_failures.push(json!({"reason":"local_health_evidence_unavailable"}));
                        Vec::new()
                    }
                };
                let public_providers = routes.iter().map(|route| route.provider.clone())
                    .filter(|provider| matches!(provider.as_str(), "openai" | "anthropic"))
                    .collect::<Vec<_>>();
                let public_budget = Duration::from_millis(750)
                    .min(decision_deadline.saturating_duration_since(Instant::now()));
                if !public_providers.is_empty() && public_budget >= Duration::from_millis(20) {
                    for (provider, reading) in collect_unique_bounded(&public_providers,
                        public_budget, |id, timeout| {
                            collect_public_status_shared(std::path::Path::new("/usr/bin/curl"),
                                id, timeout)
                        }) {
                        match reading {
                            Ok(observation) => {
                                evidence.push(json!({"provider":provider,"source":"official-status-summary",
                                    "observed_ms":observation.observed_ms,"advisory":true}));
                                health_observations.push(observation);
                            }
                            Err(_) => discovery_failures.push(json!({"provider":provider,
                                "reason":"public_status_unavailable"})),
                        }
                    }
                }
                let health_now = crate::daemon::now();
                for route in &mut routes {
                    if route.health != crate::auto_select::Health::Unavailable {
                        route.health = crate::auto_health::evaluate(route, &health_observations, health_now);
                    }
                }
                // A loopback OpenCode provider can forward to a paid account.
                // Until its upstream is attested, an allowed account's proven
                // exhaustion must not become apparent fresh capacity by
                // changing harness. Keep the local route's allowance unknown.
                if routes.iter().any(|route| route.harness != "opencode"
                    && route.quota == crate::auto_select::Allowance::Exhausted) {
                    for route in routes.iter_mut().filter(|route| route.harness == "opencode") {
                        route.unresolved_quota_pool_identity = true;
                    }
                }
                // Selection sees active/uncertain unknown-draw claims. The
                // decision insert checks again in one SQLite transaction so
                // two clients collecting concurrently cannot both admit.
                {
                    let store = d.store.lock().unwrap();
                    let now_ms = crate::daemon::now();
                    for route in &mut routes {
                        route.in_flight_pool_claim = store.auto_pool_claimed_for_child(
                            &route.pool_id, &parent.id, account_generations.get(&route.profile_id).copied())?
                            && !store.auto_pool_open_to_known_windows(route, task_class.as_deref(), now_ms)?;
                    }
                }
                let recovering = mark_endpoint_recovery(d, &mut routes, Some(&parent.id))?;
                let work = WorkUnit { id:work_unit_id.into(), min_tier, required_tools:required_tools.clone(),
                    context_needed, requires_approvals, min_sandbox:sandbox,
                    max_sandbox:sandbox,
                    allowed_profiles:allowed_profiles.clone(),
                    pinned_route, preferred_harness, task_class:task_class.clone(),
                    execution_budget_ms:Some(execution_budget_ms) };
                let fit_now_ms = crate::daemon::now();
                let (fit_inputs, fit_evidence) = if d.learning_is_paused() {
                    let inputs = routes.iter().map(|_| crate::auto_fit::FitEvidenceInput::Unavailable {
                        reason:"learning_paused".into(),
                    }).collect::<Vec<_>>();
                    let evidence = routes.iter().map(|route| json!({"route_id":route.id,"fit":"unknown",
                        "reason":"learning_paused","source":null,"observed_ms":null,
                        "expected_windows":[]})).collect::<Vec<_>>();
                    (inputs, evidence)
                } else {
                    let store = d.store.lock().unwrap();
                    crate::auto_fit::apply_scoped_fit_with_inputs(&store, &work, &mut routes,
                        &account_generations, fit_now_ms)
                };
                let mut decision = crate::auto_fit::select_with_estimates(
                    &work, &routes, &fit_inputs, fit_now_ms);
                let mut pre_effect_failures = Vec::new();
                let mut attempt_limit_reached = false;
                let mut deadline_exhausted = false;
                for attempt in 0..3 {
                    let Some(selected) = decision.selected.as_deref() else { break; };
                    if remaining_metadata_ms(decision_deadline).is_err() {
                        deadline_exhausted = true;
                        decision.selected = None;
                        decision.reason = "collection_deadline_elapsed".into();
                        break;
                    }
                    let route = routes.iter().find(|route| route.id == selected)
                        .ok_or_else(|| anyhow!("selected route disappeared"))?;
                    if route.harness == "opencode" {
                        let local_profile = d.profile(&route.profile_id)?;
                        let parent_workspace = d.workspace(&parent.workspace_id)?;
                        let valid = crate::auto_opencode::auto_local_inline_config(&local_profile,
                            std::path::Path::new(&parent_workspace.path), &route.model,
                            &route.endpoint).is_ok()
                            && crate::auto_opencode::probe_local_endpoint(&route.endpoint)
                                == crate::auto_opencode::EndpointProbe::Reachable;
                        if valid { break; }
                        pre_effect_failures.push(json!({"profile_id":route.profile_id,
                            "route_id":route.id,"reason":"local_endpoint_or_config_rejected_before_child"}));
                        let endpoint = route.endpoint.clone();
                        for alternate in routes.iter_mut().filter(|candidate|
                            candidate.harness == "opencode" && candidate.endpoint == endpoint) {
                            alternate.health = crate::auto_select::Health::Unavailable;
                        }
                        decision = crate::auto_fit::select_with_estimates(
                            &work, &routes, &fit_inputs, fit_now_ms);
                        if attempt == 2 && decision.selected.is_some() {
                            attempt_limit_reached = true;
                            decision.selected = None;
                            decision.reason = "pre_effect_attempt_limit".into();
                        }
                        continue;
                    }
                    if route.harness != "claude" { break; }
                    let rejected_profile = route.profile_id.clone();
                    let rejected_route = route.id.clone();
                    let generation = account_generations.get(&rejected_profile)
                        .ok_or_else(|| anyhow!("selected account generation unavailable"))?;
                    if recheck_claude_account_before_child(d, &rejected_profile, *generation,
                        decision_deadline).is_ok() {
                        break;
                    }
                    pre_effect_failures.push(json!({"profile_id":rejected_profile,
                        "route_id":rejected_route,"reason":"account_or_metadata_rejected_before_child"}));
                    for route in routes.iter_mut().filter(|route| route.profile_id == rejected_profile) {
                        route.health = crate::auto_select::Health::Unavailable;
                    }
                    decision = crate::auto_fit::select_with_estimates(
                        &work, &routes, &fit_inputs, fit_now_ms);
                    if attempt == 2 && decision.selected.is_some() {
                        attempt_limit_reached = true;
                        decision.selected = None;
                        decision.reason = "pre_effect_attempt_limit".into();
                    }
                }
                if decision.selected.is_some() && remaining_metadata_ms(decision_deadline).is_err() {
                    deadline_exhausted = true;
                    decision.selected = None;
                    decision.reason = "collection_deadline_elapsed".into();
                }
                let selected_route = decision.selected.as_deref().and_then(|id|
                    routes.iter().find(|route| route.id == id)).map(|route| json!({
                        "harness":route.harness,"provider":route.provider,
                        "profile_id":route.profile_id,"model":route.model,"effort":route.effort,
                        "quota":route.quota,"fit":route.fit,"health":route.health,
                    }));
                let mut trace = json!({"selector_version":"multi-harness-preflight-v8","decision":decision,
                    "selected_route":selected_route,
                    "estimator":{"state":"scoped_fit","version":"v3","now_ms":fit_now_ms,
                        "inputs":fit_inputs,"routes":fit_evidence},
                    "ranking":{"version":"v1","complete_costs":crate::auto_fit::complete_costs(
                        &work, &routes, &fit_inputs, fit_now_ms)},
                    "inference":{"state":"not_used","output":null},
                    "selection_input":{"work":&work,"routes":&routes,
                        "attempt_limit_reached":attempt_limit_reached,
                        "deadline_exhausted":deadline_exhausted},
                    "requirements":{"min_tier":min_tier,"required_tools":required_tools,
                        "context_needed":context_needed,"requires_approvals":requires_approvals,
                        "task_class":task_class,
                        "execution_budget_ms":execution_budget_ms},
                    "evidence":evidence,"discovery_failures":discovery_failures,
                    "pre_effect_failures":pre_effect_failures,
                    "candidates":routes.iter().map(|route| json!({"id":route.id,"quota":route.quota,
                        "fit":route.fit,"health":route.health})).collect::<Vec<_>>()});
                if let Some(check) = decision.selected.as_deref().and_then(|id| routes.iter().find(|r| r.id == id))
                    .and_then(|route| recovery_check_for(&recovering, route)) {
                    trace["recovery_check"] = check;
                }
                if let Some(selected) = decision.selected.as_deref() {
                    let route = routes.iter().find(|route| route.id == selected)
                        .ok_or_else(|| anyhow!("selected route disappeared"))?;
                    let generation = if route.harness == "opencode" { None } else {
                        Some(account_generations.get(&route.profile_id)
                            .ok_or_else(|| anyhow!("selected account generation unavailable"))?)
                    };
                    let required = required_tools.iter().cloned().collect::<Vec<_>>();
                    let admitted = match d.record_auto_selected_decision(work_unit_id, &parent,
                        &requirements_hash, selected, &route.pool_id, generation.copied(),
                        execution_budget_ms, trace.clone()) {
                        Err(error) if error.downcast_ref::<crate::daemon::AutoDrawExceedsAllowance>().is_some()
                            || error.downcast_ref::<crate::daemon::AutoEndpointRecoveryInFlight>().is_some() => {
                            // Selection's preview fitted, but the booking
                            // no longer does (another unit booked the room
                            // first), or another unit became the endpoint's
                            // one recovery check. Either way, pause.
                            let (reason, field) = if error.downcast_ref::<crate::daemon::AutoDrawExceedsAllowance>().is_some() {
                                ("estimated_draw_exceeds_allowance", "admission_draw_exceeds_allowance")
                            } else { ("endpoint_recovery_in_progress", "admission_endpoint_recovery_in_flight") };
                            let mut paused = decision.clone();
                            paused.selected = None;
                            paused.reason = reason.into();
                            paused.exclusions.push(crate::auto_select::Exclusion {
                                route_id: selected.into(), reason: reason.into(),
                            });
                            let mut pause_trace = trace;
                            pause_trace["decision"] = json!(paused);
                            pause_trace["selected_route"] = Value::Null;
                            pause_trace["selection_input"][field] = json!(selected);
                            d.emit(Some(&parent.task_id), Some(&parent.id), "auto_decision",
                                "daemon", "exact", pause_trace)?;
                            return Ok(json!({"state":"paused","work_unit_id":work_unit_id,
                                "decision":paused,"actions":["refresh","choose_manual_route"]}));
                        }
                        other => other?,
                    };
                    if admitted.is_none() {
                        // The pool was claimed after selection. This is an
                        // admission race, not evidence of provider failure or
                        // exhausted quota. No child or launch intent exists.
                        let mut paused = decision.clone();
                        paused.selected = None;
                        paused.reason = "pool_in_flight_unknown_draw".into();
                        paused.exclusions.push(crate::auto_select::Exclusion {
                            route_id: selected.into(), reason: "pool_in_flight_unknown_draw".into(),
                        });
                        let mut pause_trace = trace;
                        pause_trace["decision"] = json!(paused);
                        pause_trace["selected_route"] = Value::Null;
                        pause_trace["selection_input"]["admission_pool_conflict"] = json!(selected);
                        d.emit(Some(&parent.task_id), Some(&parent.id), "auto_decision",
                            "daemon", "exact", pause_trace)?;
                        return Ok(json!({"state":"paused","work_unit_id":work_unit_id,
                            "decision":paused,"actions":["refresh","choose_manual_route"]}));
                    }
                    let launch_request = json!({"work_unit_id":work_unit_id,
                        "parent_run_id":parent.id,"harness":route.harness,"profile_id":route.profile_id,
                        "model":route.model,"effort":route.effort,"prompt":prompt,"title":title,
                        "required_tools":required,"auto_selected":true,
                        "sandbox":sandbox,
                        "execution_budget_ms":execution_budget_ms,
                        "requirements_hash":requirements_hash,"expected_account_generation":generation,
                        "auto_local_endpoint":if route.harness == "opencode" {
                            Some(route.endpoint.as_str()) } else { None }});
                    let launch_id = work_unit_id.to_string();
                    let selected_route = selected.to_string();
                    let worker_daemon = d.clone();
                    let runtime = tokio::runtime::Handle::current();
                    let (result_tx, result_rx) = std::sync::mpsc::channel();
                    auto_launches().lock().unwrap().insert(launch_id.clone());
                    let spawn = std::thread::Builder::new().name("auto-launch".into()).spawn(move || {
                        let _active = ActiveAutoLaunch(launch_id.clone());
                        let _runtime = runtime.enter();
                        let outcome = (|| -> Result<Value> {
                            match worker_daemon.delegate_run(&launch_request, true) {
                                Ok(mut delegated) => {
                                    if delegated.get("launch_error").is_some() {
                                        delegated["state"] = json!("paused");
                                        delegated["actions"] = json!(["refresh", "choose_manual_route"]);
                                    } else {
                                        delegated["state"] = json!("dispatched");
                                    }
                                    delegated["decision"] = json!(decision);
                                    delegated["discovery_failures"] = json!(discovery_failures);
                                    delegated["pre_effect_failures"] = json!(pre_effect_failures);
                                    Ok(delegated)
                                }
                                Err(_) => {
                                    let store = worker_daemon.store.lock().unwrap();
                                    store.set_auto_launch_intent_phase(&launch_id, "paused")?;
                                    store.release_unstarted_auto_pool_claim(&launch_id)?;
                                    drop(store);
                                    Ok(json!({"state":"paused","work_unit_id":launch_id,
                                        "pause_reason":"launch_effects_uncertain",
                                        "launch_resources":auto_launch_resources(&worker_daemon, &launch_id)?,
                                        "decision":decision,"discovery_failures":discovery_failures,
                                        "pre_effect_failures":pre_effect_failures,
                                        "actions":["inspect_launch","choose_manual_route"]}))
                                }
                            }
                        })();
                        let _ = result_tx.send(outcome);
                    });
                    if let Err(error) = spawn {
                        auto_launches().lock().unwrap().remove(work_unit_id);
                        let store = d.store.lock().unwrap();
                        store.set_auto_launch_intent_phase(work_unit_id, "paused")?;
                        if !store.release_unstarted_auto_pool_claim(work_unit_id)? {
                            return Err(anyhow!("unstarted launch retained an uncertain pool claim: {error}"));
                        }
                        return Err(error.into());
                    }
                    // The persisted intent and active-worker set now identify
                    // this launch. Let another client replay it while the
                    // first caller waits for a fast completion.
                    drop(_work_unit_guard);
                    let wait = decision_deadline.saturating_duration_since(Instant::now());
                    match result_rx.recv_timeout(wait) {
                        Ok(result) => result?,
                        Err(std::sync::mpsc::RecvTimeoutError::Timeout) =>
                            auto_pending_response(work_unit_id, &selected_route, false),
                        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) =>
                            return Err(anyhow!("automatic launch worker stopped unexpectedly")),
                    }
                } else {
                    d.emit(Some(&parent.task_id), Some(&parent.id), "auto_decision", "daemon", "exact", trace)?;
                    json!({"state":"paused","work_unit_id":work_unit_id,"decision":decision,
                        "discovery_failures":discovery_failures,"pre_effect_failures":pre_effect_failures,
                        "actions":["refresh","choose_manual_route"]})
                }
            }
        }
        "auto.decision.replay" => {
            let event_seq = p["event_seq"].as_i64().filter(|seq| *seq > 0)
                .ok_or_else(|| anyhow!("event_seq must be a positive event identity"))?;
            let payload: String = d.store.lock().unwrap().conn.query_row(
                "SELECT payload FROM events WHERE seq=?1 AND kind='auto_decision'",
                [event_seq], |row| row.get(0))?;
            let payload: Value = serde_json::from_str(&payload)?;
            let selector_version = payload["selector_version"].as_str()
                .ok_or_else(|| anyhow!("automatic selector version is missing"))?;
            let input = &payload["selection_input"];
            let work: crate::auto_select::WorkUnit = serde_json::from_value(input["work"].clone())?;
            let mut routes: Vec<crate::auto_select::Route> = serde_json::from_value(input["routes"].clone())?;
            if routes.len() > 128 { return Err(anyhow!("automatic decision replay exceeded its candidate bound")); }
            let mut replay_fit = None;
            let estimator_matches_recorded = if matches!(selector_version,
                "multi-harness-preflight-v7" | "multi-harness-preflight-v8") {
                let estimator = &payload["estimator"];
                if estimator["state"] != "scoped_fit"
                    || !matches!(estimator["version"].as_str(), Some("v2" | "v3"))
                    || payload["inference"]["state"] != "not_used"
                    || !payload["inference"]["output"].is_null() {
                    return Err(anyhow!("unsupported automatic estimator or inference replay"));
                }
                let now_ms = estimator["now_ms"].as_i64()
                    .ok_or_else(|| anyhow!("automatic estimator observation time is missing"))?;
                let raw_inputs = estimator["inputs"].as_array()
                    .ok_or_else(|| anyhow!("automatic estimator inputs are missing"))?;
                if raw_inputs.len() != routes.len() || raw_inputs.len() > 128
                    || serde_json::to_vec(raw_inputs)?.len() > 128 * 1024 {
                    return Err(anyhow!("automatic estimator replay inputs are incomplete or oversized"));
                }
                let inputs: Vec<crate::auto_fit::FitEvidenceInput> =
                    serde_json::from_value(estimator["inputs"].clone())?;
                let saved_results = estimator["routes"].as_array()
                    .ok_or_else(|| anyhow!("automatic estimator results are missing"))?;
                if inputs.len() != routes.len() || saved_results.len() != routes.len()
                    || !crate::auto_fit::fit_inputs_within_bounds(&inputs) {
                    return Err(anyhow!("automatic estimator replay inputs are incomplete or oversized"));
                }
                let mut matches = true;
                for (index, route) in routes.iter_mut().enumerate() {
                    let evaluated = crate::auto_fit::evaluate_fit(&work, route, &inputs[index], now_ms);
                    let recomputed = json!({"route_id":route.id,"fit":evaluated.fit,
                        "reason":evaluated.reason,"source":evaluated.source,
                        "observed_ms":evaluated.observed_ms,
                        "expected_windows":evaluated.expected_windows});
                    matches &= route.fit == evaluated.fit && saved_results[index] == recomputed;
                    route.fit = evaluated.fit;
                }
                replay_fit = Some((inputs, now_ms));
                Some(matches)
            } else { None };
            let ranking_matches_recorded = if selector_version == "multi-harness-preflight-v8" {
                if payload["ranking"]["version"] != "v1" {
                    return Err(anyhow!("unsupported automatic ranking replay"));
                }
                let (inputs, now_ms) = replay_fit.as_ref()
                    .ok_or_else(|| anyhow!("automatic ranking inputs are missing"))?;
                let costs = crate::auto_fit::complete_costs(&work, &routes, inputs, *now_ms);
                Some(json!(costs) == payload["ranking"]["complete_costs"])
            } else { None };
            let decision = match selector_version {
                "codex-cold-start-v1" => crate::auto_select::select_legacy_v1(&work, &routes),
                "codex-cold-start-v2" | "multi-harness-cold-start-v1" =>
                    crate::auto_select::select_pre_status_v1(&work, &routes),
                "multi-harness-preflight-v1" | "multi-harness-preflight-v2" | "multi-harness-preflight-v3" | "multi-harness-preflight-v4" | "multi-harness-preflight-v5" | "multi-harness-preflight-v6" | "multi-harness-preflight-v7" | "multi-harness-preflight-v8" => {
                    let mut decision = if selector_version == "multi-harness-preflight-v1" {
                        crate::auto_select::select_pre_status_v1(&work, &routes)
                    } else if selector_version == "multi-harness-preflight-v8" {
                        let (inputs, now_ms) = replay_fit.as_ref()
                            .ok_or_else(|| anyhow!("automatic ranking inputs are missing"))?;
                        crate::auto_fit::select_with_estimates(&work, &routes, inputs, *now_ms)
                    } else if matches!(selector_version, "multi-harness-preflight-v4" | "multi-harness-preflight-v5" | "multi-harness-preflight-v6" | "multi-harness-preflight-v7") {
                        crate::auto_select::select(&work, &routes)
                    } else { crate::auto_select::select_pre_scoped_pool_v1(&work, &routes) };
                    if input["attempt_limit_reached"] == true {
                        decision.selected = None;
                        decision.reason = "pre_effect_attempt_limit".into();
                    }
                    if input["deadline_exhausted"] == true {
                        decision.selected = None;
                        decision.reason = "collection_deadline_elapsed".into();
                    }
                    if matches!(selector_version, "multi-harness-preflight-v5" | "multi-harness-preflight-v6" | "multi-harness-preflight-v7" | "multi-harness-preflight-v8") {
                        if let Some(route_id) = input["admission_pool_conflict"].as_str() {
                            decision.selected = None;
                            decision.reason = "pool_in_flight_unknown_draw".into();
                            decision.exclusions.push(crate::auto_select::Exclusion {
                                route_id: route_id.into(), reason: "pool_in_flight_unknown_draw".into(),
                            });
                        }
                        if let Some(route_id) = input["admission_endpoint_recovery_in_flight"].as_str() {
                            decision.selected = None;
                            decision.reason = "endpoint_recovery_in_progress".into();
                            decision.exclusions.push(crate::auto_select::Exclusion {
                                route_id: route_id.into(), reason: "endpoint_recovery_in_progress".into(),
                            });
                        }
                        if let Some(route_id) = input["admission_draw_exceeds_allowance"].as_str() {
                            decision.selected = None;
                            decision.reason = "estimated_draw_exceeds_allowance".into();
                            decision.exclusions.push(crate::auto_select::Exclusion {
                                route_id: route_id.into(), reason: "estimated_draw_exceeds_allowance".into(),
                            });
                        }
                    }
                    decision
                }
                _ => return Err(anyhow!("unsupported automatic selector version")),
            };
            let selected_route_matches = if matches!(selector_version,
                "multi-harness-preflight-v7" | "multi-harness-preflight-v8") {
                let selected = decision.selected.as_deref().and_then(|id|
                    routes.iter().find(|route| route.id == id)).map(|route| json!({
                        "harness":route.harness,"provider":route.provider,
                        "profile_id":route.profile_id,"model":route.model,"effort":route.effort,
                        "quota":route.quota,"fit":route.fit,"health":route.health,
                    }));
                json!(selected) == payload["selected_route"]
            } else { true };
            let matches_recorded = serde_json::to_value(&decision)? == payload["decision"]
                && estimator_matches_recorded.unwrap_or(true)
                && ranking_matches_recorded.unwrap_or(true) && selected_route_matches;
            json!({"event_seq":event_seq,"matches_recorded":matches_recorded,
                "replay_scope":if ranking_matches_recorded.is_some() {
                    "selector_estimator_and_ranking"
                } else if estimator_matches_recorded.is_some() {
                    "selector_and_estimator" } else { "selector_only" },
                "estimator_recomputed":estimator_matches_recorded.is_some(),
                "estimator_matches_recorded":estimator_matches_recorded,
                "ranking_matches_recorded":ranking_matches_recorded,
                "decision":decision})
        }
        "run.result" => d.delegated_result(s(p, "run_id")?)?,
        "run.follow_up" => {
            // A held agent takes no new turn: the owner's own message offers Release and send.
            let run_id = s(p, "run_id")?;
            if let Some(hold) = d.hold_of(run_id) {
                if p["release"].as_bool().unwrap_or(false) {
                    d.agent_release(run_id, "owner", "released to send a message")?;
                } else {
                    return Err(anyhow!("held: {} (by {}). Release and send?", hold["reason"].as_str().unwrap_or(""), hold["by"].as_str().unwrap_or("")));
                }
            }
            json!(d.start_turn(run_id, s(p, "prompt")?, true, &crate::daemon::TurnOpts::from_params(p)?)?)
        }
        "run.interrupt" => d.interrupt(s(p, "run_id")?)?,
        "run.permission" => d.answer_permission(s(p, "run_id")?, s(p, "request_id")?, p["allow"].as_bool().unwrap_or(false), p["message"].as_str().unwrap_or(""))?,
        "run.raw_output" => d.raw_output(s(p, "run_id")?, p["max_bytes"].as_u64().unwrap_or(256 * 1024).min(4 * 1024 * 1024) as usize)?,
        "run.turns" => json!(d.store.lock().unwrap().turns(s(p, "run_id")?)?),
        "run.active" => {
            let runs = d.store.lock().unwrap().runs()?;
            json!(runs.into_iter().filter(|r| ACTIVE.contains(&r.status.as_str()) && d.run_role(&r.id) != "overseer").collect::<Vec<_>>())
        }
        "events.list" => {
            let store = d.store.lock().unwrap();
            let run = p["run_id"].as_str();
            let after = p["after"].as_i64().unwrap_or(0);
            let events = store.events_after(after, run, p["limit"].as_i64().unwrap_or(1000).clamp(1, 5000))?;
            let oldest = match run {
                Some(r) => store.oldest_retained(r)?,
                None => None,
            };
            json!({"events": events, "oldest_retained": oldest})
        }
        "auto.models.refresh" => {
            let deadline = metadata_deadline(p)?;
            let profile = d.profile(s(p, "profile_id")?)?;
            if profile.harness != "codex" {
                return Err(anyhow!("structured model discovery is not supported for this profile"));
            }
            let gate = d.profile_gate(&profile.id);
            let _profile_guard = lock_gate_until(&gate, deadline)?;
            let active = d.store.lock().unwrap().runs()?.iter().any(|run| run.profile_id.as_deref() == Some(profile.id.as_str()) && crate::daemon::ACTIVE.contains(&run.status.as_str()));
            if active {
                return Err(anyhow!("account profile has an active run; use its native updates"));
            }
            let read = (|| -> anyhow::Result<_> {
                let program = crate::adapters::resolve_program("codex-app").ok_or_else(|| anyhow!("Codex is not installed"))?;
                let mut env = crate::adapters::base_env(&program.display().to_string());
                env.extend(Daemon::profile_env(&profile));
                let raw = crate::auto_collect::codex_model_list(&program, &env, &crate::adapters::neutral_dir(),
                    Duration::from_millis(remaining_metadata_ms(deadline)?))?;
                let observed_ms = crate::daemon::now();
                let catalog = crate::auto_route::parse_codex_catalog(&raw.models, observed_ms)?;
                let snapshot = crate::auto_quota::parse_codex_rate_limits(&raw.rate_limits, &profile.id, raw.rate_limits_observed_ms)?;
                let fingerprint = crate::auto_quota::account_fingerprint(&raw.rate_limits)?;
                Ok((observed_ms, catalog, snapshot, fingerprint))
            })();
            let (observed_ms, catalog, snapshot, fingerprint) = match read {
                Ok(value) => value,
                Err(error) => {
                    d.store.lock().unwrap().invalidate_auto_profile_evidence(&profile.id)?;
                    return Err(error);
                }
            };
            let store = d.store.lock().unwrap();
            store.record_auto_account_identity(&profile.id, &fingerprint)?;
            let event = store.insert_event(observed_ms, None, None, "quota", "codex-app/metadata-read", "reported", &json!({"profile_id":profile.id,"snapshot":snapshot}))?;
            store.insert_auto_quota(event.seq, &profile.id, "codex-app/metadata-read", &snapshot)?;
            store.put_auto_model_catalog(&profile.id, &catalog)?;
            json!({"profile_id":profile.id,"catalog":catalog})
        }
        "auto.models.list" => {
            let profile = d.profile(s(p, "profile_id")?)?;
            let catalog = d.store.lock().unwrap().auto_model_catalog(&profile.id)?;
            let fresh = catalog.as_ref().is_some_and(|c| crate::daemon::now() >= c.observed_ms && crate::daemon::now() < c.expires_ms);
            json!({"profile_id":profile.id,"catalog":catalog,"fresh":fresh})
        }
        "auto.opencode.local.inspect" => {
            use std::collections::BTreeMap;
            let timeout_ms = match p.get("timeout_ms") {
                None => 8000,
                Some(value) => value.as_u64().filter(|ms| (1..=8000).contains(ms))
                    .ok_or_else(|| anyhow!("timeout_ms must be between 1 and 8000"))?,
            };
            let deadline = Instant::now() + Duration::from_millis(timeout_ms);
            let profile = d.profile(s(p, "profile_id")?)?;
            if profile.harness != "opencode" {
                return Err(anyhow!("local OpenCode discovery requires an OpenCode profile"));
            }
            let (project, workspace_id, repo) = auto_metadata_project(d, p)?;
            let gate = d.profile_gate(&profile.id);
            let _profile_guard = lock_gate_until(&gate, deadline)?;
            if d.store.lock().unwrap().runs()?.iter().any(|run| run.profile_id.as_deref() == Some(profile.id.as_str())
                && crate::daemon::ACTIVE.contains(&run.status.as_str())) {
                return Err(anyhow!("OpenCode profile has an active run"));
            }
            let program = crate::adapters::resolve_program("opencode")
                .ok_or_else(|| anyhow!("OpenCode is not installed"))?;
            let mut env = crate::adapters::base_env(&program.display().to_string());
            env.extend(Daemon::profile_env(&profile));
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining < Duration::from_millis(20) {
                return Err(anyhow!("OpenCode local metadata deadline elapsed"));
            }
            let catalog = crate::auto_collect::opencode_local_catalog(&program, &env,
                &project, remaining, crate::daemon::now())?;
            let mut endpoint_health = BTreeMap::new();
            let mut local_execution_config_verified = BTreeMap::new();
            let mut local_execution_config_reason = BTreeMap::new();
            for model in &catalog.models {
                let eligibility = crate::auto_opencode::auto_local_inline_config(&profile,
                    &project, &model.model, &model.endpoint);
                local_execution_config_reason.insert(model.model.clone(), eligibility.as_ref().err()
                    .map(ToString::to_string));
                local_execution_config_verified.insert(model.model.clone(), eligibility.is_ok());
                if endpoint_health.contains_key(&model.provider_id) { continue; }
                let state = if endpoint_health.len() >= 4 { "unprobed" } else {
                    match crate::auto_opencode::probe_local_endpoint(&model.endpoint) {
                        crate::auto_opencode::EndpointProbe::Reachable => "reachable",
                        crate::auto_opencode::EndpointProbe::Unavailable => "unavailable",
                        crate::auto_opencode::EndpointProbe::Invalid => "invalid",
                    }
                };
                endpoint_health.insert(model.provider_id.clone(), state);
            }
            json!({"profile_id":profile.id,"workspace_id":workspace_id,"repo":repo,
                "source":"opencode/config-providers","catalog":catalog,
                "endpoint_health":endpoint_health,"local_execution_config_verified":local_execution_config_verified,
                "local_execution_config_reason":local_execution_config_reason,
                "allowance":"unknown",
                "limitation":"loopback reachability and model metadata do not prove task capability or tool permissions"})
        }
        "auto.tools.inspect" => {
            let deadline = metadata_deadline(p)?;
            let profile = d.profile(s(p, "profile_id")?)?;
            if profile.harness != "codex" {
                return Err(anyhow!("structured tool discovery is not supported for this profile"));
            }
            let (project, workspace_id, repo) = auto_metadata_project(d, p)?;
            let gate = d.profile_gate(&profile.id);
            let _profile_guard = lock_gate_until(&gate, deadline)?;
            let read = (|| -> anyhow::Result<_> {
                let program = crate::adapters::resolve_program("codex-app").ok_or_else(|| anyhow!("Codex is not installed"))?;
                let mut env = crate::adapters::base_env(&program.display().to_string());
                env.extend(Daemon::profile_env(&profile));
                let raw = crate::auto_collect::codex_tool_inventory(&program, &env,
                    &project, Duration::from_millis(remaining_metadata_ms(deadline)?))?;
                let observed_ms = crate::daemon::now();
                let catalog = crate::auto_route::parse_codex_tools(&raw.tools, observed_ms)?;
                let fingerprint = crate::auto_quota::account_fingerprint(&raw.rate_limits)?;
                Ok((catalog, fingerprint))
            })();
            let (catalog, fingerprint) = match read {
                Ok(value) => value,
                Err(error) => {
                    d.store.lock().unwrap().invalidate_auto_profile_evidence(&profile.id)?;
                    return Err(error);
                }
            };
            d.store.lock().unwrap().record_auto_account_identity(&profile.id, &fingerprint)?;
            json!({"profile_id":profile.id,"workspace_id":workspace_id,"repo":repo,"source":"codex-app/mcpServerStatus-list","catalog":catalog,
                "limitation":"discovery only; effective child tool permission requires launch-time verification"})
        }
        "auto.quota.refresh" => {
            let deadline = metadata_deadline(p)?;
            let profile = d.profile(s(p, "profile_id")?)?;
            if profile.harness != "codex" {
                return Err(anyhow!("structured quota refresh is not supported for this profile"));
            }
            let gate = d.profile_gate(&profile.id);
            let _profile_guard = lock_gate_until(&gate, deadline)?;
            let active = d.store.lock().unwrap().runs()?.iter().any(|run| run.profile_id.as_deref() == Some(profile.id.as_str()) && crate::daemon::ACTIVE.contains(&run.status.as_str()));
            if active {
                return Err(anyhow!("account profile has an active run; use its native updates"));
            }
            let read = (|| -> anyhow::Result<_> {
                let program = crate::adapters::resolve_program("codex-app").ok_or_else(|| anyhow!("Codex is not installed"))?;
                let mut env = crate::adapters::base_env(&program.display().to_string());
                env.extend(Daemon::profile_env(&profile));
                let raw = crate::auto_collect::codex_rate_limits(&program, &env, &crate::adapters::neutral_dir(),
                    Duration::from_millis(remaining_metadata_ms(deadline)?))?;
                let observed_ms = crate::daemon::now();
                let snapshot = crate::auto_quota::parse_codex_rate_limits(&raw, &profile.id, observed_ms)?;
                let fingerprint = crate::auto_quota::account_fingerprint(&raw)?;
                Ok((observed_ms, snapshot, fingerprint))
            })();
            let (observed_ms, snapshot, fingerprint) = match read {
                Ok(value) => value,
                Err(error) => {
                    d.store.lock().unwrap().invalidate_auto_profile_evidence(&profile.id)?;
                    return Err(error);
                }
            };
            let store = d.store.lock().unwrap();
            store.record_auto_account_identity(&profile.id, &fingerprint)?;
            let event = store.insert_event(observed_ms, None, None, "quota", "codex-app/metadata-read", "reported", &json!({"profile_id":profile.id,"snapshot":snapshot}))?;
            store.insert_auto_quota(event.seq, &profile.id, "codex-app/metadata-read", &snapshot)?;
            let state = crate::auto_select::observed_allowance(Some(&snapshot), p["model"].as_str().unwrap_or(""), observed_ms);
            json!({"pool_id":profile.id,"state":state,"snapshot":snapshot})
        }
        "auto.quota.state" => {
            let profile_id = s(p, "profile_id")?;
            let harness = s(p, "harness")?;
            let model = s(p, "model")?;
            let profile = d.profile(profile_id)?;
            let family = if harness == "codex-app" { "codex" } else { harness };
            if profile.harness != family {
                return Err(anyhow!("harness does not use the selected account profile"));
            }
            let observation = d.store.lock().unwrap().latest_auto_quota(profile_id)?;
            let state = crate::auto_select::observed_allowance(observation.as_ref().map(|o| &o.snapshot), model, crate::daemon::now());
            json!({"pool_id": profile_id, "state": state, "observation": observation})
        }
        "auto.quota.list" => {
            let rows = d.store.lock().unwrap().auto_quotas(p["limit"].as_i64().unwrap_or(100).clamp(1, 5000))?;
            json!({"observations": rows})
        }
        "auto.usage.list" => {
            let store = d.store.lock().unwrap();
            if !maintain_visible_learning(d, &store) {
                json!({"measurements": [], "learning_paused": true})
            } else {
                let rows = store.auto_measurements(p["limit"].as_i64().unwrap_or(100).clamp(1, 5000))?;
                let paused = d.learning_is_paused() || store.auto_learning_is_paused()?;
                json!({"measurements": rows, "learning_paused": paused})
            }
        }
        "auto.usage.work.list" => {
            let store = d.store.lock().unwrap();
            if !maintain_visible_learning(d, &store) {
                json!({"work_units": [], "learning_paused": true})
            } else {
                let rows = store.auto_work_observations(p["limit"].as_i64().unwrap_or(100).clamp(1, 5000))?;
                let paused = d.learning_is_paused() || store.auto_learning_is_paused()?;
                json!({"work_units":rows,"learning_paused":paused})
            }
        }
        "auto.usage.thread.refresh" => {
            let deadline = metadata_deadline(p)?;
            let run = d.run(s(p, "run_id")?)?;
            if run.harness != "codex-app" || run.status != "completed" {
                return Err(anyhow!("thread usage is available only after a completed Codex app-server run"));
            }
            let profile_id = run.profile_id.as_deref().ok_or_else(|| anyhow!("run account profile is unavailable"))?;
            let thread_id = run.native_id.as_deref().ok_or_else(|| anyhow!("run thread identity is unavailable"))?;
            let gate = d.profile_gate(profile_id);
            let _profile_guard = lock_gate_until(&gate, deadline)?;
            if d.run(&run.id)?.status != "completed" {
                return Err(anyhow!("run changed while reading usage"));
            }
            let active = d.store.lock().unwrap().runs()?.iter().any(|other| other.profile_id.as_deref() == Some(profile_id) && crate::daemon::ACTIVE.contains(&other.status.as_str()));
            if active {
                return Err(anyhow!("account profile has an active run; defer usage metadata read"));
            }
            let profile = d.profile(profile_id)?;
            let read = (|| -> anyhow::Result<_> {
                let program = crate::adapters::resolve_program("codex-app").ok_or_else(|| anyhow!("Codex is not installed"))?;
                let mut env = crate::adapters::base_env(&program.display().to_string());
                env.extend(Daemon::profile_env(&profile));
                let raw = crate::auto_collect::codex_thread_usage(&program, &env, &crate::adapters::neutral_dir(), thread_id,
                    Duration::from_millis(remaining_metadata_ms(deadline)?))?;
                let observed_ms = crate::daemon::now();
                let fingerprint = crate::auto_quota::account_fingerprint(&raw.rate_limits)?;
                let snapshot = crate::auto_quota::parse_codex_rate_limits(&raw.rate_limits, profile_id, raw.rate_limits_observed_ms)?;
                let estimate = crate::auto_consumption::parse_codex_thread_usage(&raw.usage, thread_id, observed_ms)?;
                Ok((observed_ms, fingerprint, snapshot, estimate))
            })();
            let (observed_ms, fingerprint, snapshot, estimate) = match read {
                Ok(value) => value,
                Err(error) => {
                    d.store.lock().unwrap().invalidate_auto_profile_evidence(profile_id)?;
                    return Err(error);
                }
            };
            let store = d.store.lock().unwrap();
            let account_recorded = store.record_auto_account_identity(profile_id, &fingerprint);
            d.learning_account_paused.store(account_recorded.is_err(), std::sync::atomic::Ordering::Relaxed);
            account_recorded?;
            let generation = store.auto_account_generation(profile_id)?.ok_or_else(|| anyhow!("account generation is unavailable"))?;
            let prior = store.auto_run_pre_turn_quota(&run.id)?;
            // External usage, source precision, and reporting settlement are not
            // established by a completed local thread. Keep observed changes
            // visible as unverified, never as a numeric subscription charge.
            let allowance_delta = crate::auto_consumption::assess_window_delta(prior.as_ref(),
                &snapshot, &crate::auto_consumption::DeltaContext {
                    model:run.model.as_deref(), effort:run.effort.as_deref(),
                    resolved_model_version:None, task_signature:None,
                    same_account_generation:store.auto_run_account_matches(&run.id, profile_id, generation)?,
                    model_version_stable:false, local_overlap_excluded:false,
                    external_usage_excluded:false, reporting_settled:false,
                    meter_error_percent:None,
                });
            let event = store.insert_event(observed_ms, Some(&run.task_id), Some(&run.id), "quota",
                "codex-app/metadata-read", "reported", &json!({"profile_id":profile_id,"snapshot":snapshot}))?;
            store.insert_auto_quota(event.seq, profile_id, "codex-app/metadata-read", &snapshot)?;
            let observation = if let Some(mut estimate) = estimate {
                estimate.plan_type = snapshot.reported_plan_type().map(str::to_string);
                let attribution = store.auto_thread_usage_attribution(&run.id, profile_id,
                    generation, estimate.plan_type.as_deref())?;
                let thread_recorded = store.insert_auto_thread_usage(&run.id, profile_id, generation,
                    "codex-app/account-usage-read", &estimate);
                d.learning_thread_paused.store(thread_recorded.is_err(), std::sync::atomic::Ordering::Relaxed);
                let id = thread_recorded?;
                Some(json!({"id":id,"run_id":run.id,"profile_id":profile_id,"read_account_generation":generation,"attribution":attribution,"subscription_window_relation":"unverified","source":"codex-app/account-usage-read","estimate":estimate}))
            } else { None };
            match store.refresh_auto_work_observation(&run.id, &allowance_delta) {
                Ok(true) => d.learning_work_paused.store(false, std::sync::atomic::Ordering::Relaxed),
                Err(_) => d.learning_work_paused.store(true, std::sync::atomic::Ordering::Relaxed),
                Ok(false) => {}
            }
            json!({"state":if observation.is_some() {"estimated"} else {"unavailable"},
                "observation":observation,"allowance_delta":allowance_delta})
        }
        "auto.usage.thread.list" => {
            let store = d.store.lock().unwrap();
            if !maintain_visible_learning(d, &store) {
                json!({"observations": [], "learning_paused": true})
            } else {
                let rows = store.auto_thread_usage_observations(p["limit"].as_i64().unwrap_or(100).clamp(1, 5000))?;
                json!({"observations":rows,"learning_paused":d.learning_is_paused() || store.auto_learning_is_paused()?})
            }
        }
        "auto.usage.summary" => {
            let store = d.store.lock().unwrap();
            if !maintain_visible_learning(d, &store) {
                json!({"aggregates": [], "learning_paused": true})
            } else {
                let rows = store.auto_daily_aggregates(p["limit"].as_i64().unwrap_or(100).clamp(1, 10_000))?;
                json!({"aggregates": rows,"learning_paused":d.learning_is_paused() || store.auto_learning_is_paused()?})
            }
        }
        "auto.usage.export" => {
            use std::io::Write;
            use std::os::unix::fs::OpenOptionsExt;
            let path = std::path::Path::new(s(p, "path")?);
            if !path.is_absolute() {
                return Err(anyhow!("export path must be absolute"));
            }
            let store = d.store.lock().unwrap();
            if !maintain_visible_learning(d, &store) {
                return Err(anyhow!("Auto learning cleanup failed; export is unavailable until storage recovers"));
            }
            let rows = store.auto_measurements(50_000)?;
            let aggregates = store.auto_daily_aggregates(10_000)?;
            let thread_usage_observations = store.auto_thread_usage_observations(5000)?;
            let work_units = store.auto_work_observations(5000)?;
            drop(store);
            let mut file = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(path)?;
            let result = serde_json::to_writer_pretty(&mut file, &json!({
                "schema_version": 3, "generated_ms": crate::daemon::now(),
                "measurements": rows, "aggregates": aggregates,
                "thread_usage_observations": thread_usage_observations,
                "work_units": work_units,
                "note": "Local usage observations and daily summaries; missing fields are unknown, not zero."
            })).and_then(|_| file.write_all(b"\n").map_err(serde_json::Error::io));
            if let Err(error) = result {
                drop(file);
                let _ = std::fs::remove_file(path);
                return Err(error.into());
            }
            file.sync_all()?;
            json!({"path": path, "count": rows.len()})
        }
        "auto.usage.clear" => {
            let deleted = d.store.lock().unwrap().clear_auto_learning()?;
            json!({"deleted": deleted})
        }
        "comparison.options" => d.comparisons(s(p, "run_id")?, p["branch"].as_str())?,
        "workspace.diff" => d.workspace_diff_opts(s(p, "workspace_id")?, s(p, "base")?, p["status"].as_bool().unwrap_or(true))?,
        "workspace.status" => {
            let ws = d.workspace(s(p, "workspace_id")?)?;
            json!(crate::git::status(std::path::Path::new(&ws.path))?)
        }
        "workspace.cleanup_plan" => d.cleanup_plan(s(p, "workspace_id")?)?,
        "workspace.cleanup" => d.cleanup(s(p, "workspace_id")?, p["discard_dirty"].as_bool().unwrap_or(false))?,
        "account.usage" => d.account_usage(s(p, "id")?)?,
        "task.archive" => d.task_archive(s(p, "task_id")?, p["archived"].as_bool().unwrap_or(true))?,
        "search" => d.search(p["query"].as_str().unwrap_or(""), p["limit"].as_i64().unwrap_or(200))?,
        "repo.files" => d.repo_files(p["workspace_id"].as_str(), p["repo"].as_str(), p["query"].as_str().unwrap_or(""), p["limit"].as_u64().unwrap_or(30) as usize)?,
        "workspace.changes" => d.workspace_changes(s(p, "workspace_id")?)?,
        "workspace.tree" => d.workspace_tree(s(p, "workspace_id")?, p["dir"].as_str().unwrap_or(""))?,
        "account.list" => d.account_list()?,
        "account.create" => d.account_create(s(p, "provider")?, s(p, "name")?)?,
        "account.remove" => d.account_remove(s(p, "id")?)?,
        "workspace.pr_plan" => d.pr_plan(s(p, "workspace_id")?)?,
        "workspace.pr_prepare" => d.pr_prepare(s(p, "workspace_id")?)?,
        "workspace.pr_opened" => d.pr_opened(s(p, "workspace_id")?, s(p, "url")?, p["number"].as_i64().unwrap_or(0))?,
        "workspace.merge_plan" => d.merge_plan(s(p, "workspace_id")?)?,
        "workspace.merge_prepare" => d.merge_prepare(s(p, "workspace_id")?, p["handoff"].as_bool().unwrap_or(false))?,
        "workspace.merge_resolved" => d.merge_resolved(s(p, "workspace_id")?)?,
        "workspace.merge_complete" => d.merge_complete(s(p, "workspace_id")?)?,
        "workspace.merge_abort" => d.merge_abort(s(p, "workspace_id")?)?,
        "overseer.token" => d.overseer_token(s(p, "run_id")?, p["role"].as_str().unwrap_or("agent"))?,
        "agent.digest" => {
            let id = s(p, "run_id")?;
            json!({"digest": d.digest(id)?, "text": d.digest_text(id)?})
        }
        "agents.roster" => json!({"roster": d.roster()?, "text": d.roster_text()?}),
        "conflicts.list" => d.conflicts_list(p["run_id"].as_str(), p["include_closed"].as_bool().unwrap_or(false))?,
        "conflict.dismiss" => d.conflict_dismiss(s(p, "id")?, p["by"].as_str().unwrap_or("user"))?,
        "overseer.scan" => d.scan_conflicts(s(p, "run_id")?)?,
        "overseer.session" => d.overseer_session()?,
        "overseer.messages" => d.overseer_messages(p["after"].as_i64().unwrap_or(0), p["limit"].as_i64().unwrap_or(100))?,
        "overseer.send" => d.overseer_send(s(p, "text")?, p["surface"].as_str().unwrap_or("vscode"), p["harness"].as_str(), p["model"].as_str())?,
        "overseer.propose" => d.overseer_propose(&p["actions"], p["source"].as_str().unwrap_or("api"))?,
        "overseer.answer" => d.overseer_answer(s(p, "id")?, p["yes"].as_bool().unwrap_or(false), p["surface"].as_str().unwrap_or("vscode"), p["by"].as_str().unwrap_or("owner"))?,
        "overseer.level" => d.overseer_level(p["level"].as_str())?,
        "overseer.cancel" => d.overseer_cancel(s(p, "id")?, p["by"].as_str().unwrap_or("owner"))?,
        "overseer.fresh" => d.overseer_fresh()?,
        "run.queue" => json!({"delivery": d.queue_message(s(p, "run_id")?, s(p, "text")?, p["source"].as_str().unwrap_or("owner"), json!({}))?}),
        "run.redirect" => d.agent_redirect(s(p, "run_id")?, s(p, "text")?, p["source"].as_str().unwrap_or("owner"), json!({}))?,
        "agent.hold" => d.agent_hold(s(p, "run_id")?, p["reason"].as_str().unwrap_or("held by the owner"), p["by"].as_str().unwrap_or("owner"), p["now"].as_bool().unwrap_or(false), p["release_on"].clone(), p["card"].as_str())?,
        "agent.release" => d.agent_release(s(p, "run_id")?, p["by"].as_str().unwrap_or("owner"), p["why"].as_str().unwrap_or("released"))?,
        "agent.holds" => d.holds_list()?,
        "agent.guardrail" => {
            let list = |k: &str| p[k].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect::<Vec<_>>()).unwrap_or_default();
            d.agent_guardrail(s(p, "run_id")?, p["words"].as_str().unwrap_or(""), &list("allow"), &list("deny"), p["hold_on_cross"].as_bool().unwrap_or(false), p["by"].as_str().unwrap_or("owner"))?
        }
        "agent.guardrail_remove" => d.agent_guardrail_remove(s(p, "id")?, p["by"].as_str().unwrap_or("owner"))?,
        "agent.guardrails" => json!({"guardrails": d.guardrails_of(s(p, "run_id")?)?}),
        "agent.redirect" => d.agent_redirect(s(p, "run_id")?, s(p, "text")?, p["source"].as_str().unwrap_or("owner"), json!({}))?,
        "conflict.resolve" => d.conflict_resolve(s(p, "id")?, s(p, "how")?, p["keeper"].as_str(), p["by"].as_str().unwrap_or("owner"))?,
        "overseer.card" => d.card(s(p, "id")?)?,
        "agent.cadence" => match p["cadence"].as_str() {
            Some(c) => d.set_cadence(p["run_id"].as_str(), c, p["by"].as_str().unwrap_or("owner"))?,
            None => json!({"run_id": p["run_id"], "cadence": d.cadence_of(p["run_id"].as_str().unwrap_or("")).text()}),
        },
        "agent.check_ins" => d.check_ins_of(s(p, "run_id")?)?,
        "agent.channel" => match (p["run_id"].as_str(), p["briefing"].as_bool(), p["channel"].as_bool(), p["default"].as_str()) {
            (Some(run), None, None, None) => {
                let (b, c) = d.channel_of(run)?;
                json!({"run_id": run, "briefing": b, "channel": c})
            }
            (run, b, c, default) => d.set_channel(run, b, c, default, p["by"].as_str().unwrap_or("owner"))?,
        },
        "agent.briefings" => d.briefings_of(s(p, "run_id")?)?,
        "agent.area" => {
            let paths: Vec<String> = p["paths"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default();
            d.set_area(s(p, "run_id")?, &paths, p["by"].as_str().unwrap_or("owner"))?
        }
        "channel.messages" => d.channel_messages(p["run_id"].as_str(), p["limit"].as_i64().unwrap_or(200))?,
        "overseer.rally" => {
            let agents = p["agents"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect::<Vec<_>>());
            d.rally(p["repo"].as_str(), agents)?
        }
        "share.list" => d.shares_list(p["run_id"].as_str())?,
        "watch.start" => d.watch_start(p, p["by"].as_str().unwrap_or("owner"))?,
        "watch.end" => d.watch_end(s(p, "id")?, p["reason"].as_str().unwrap_or("ended by the owner"), p["by"].as_str().unwrap_or("owner"))?,
        "watch.list" => d.watches_list(p["run_id"].as_str(), p["open_only"].as_bool().unwrap_or(false))?,
        "watch.findings" => d.findings_list(p["watch"].as_str(), p["run_id"].as_str())?,
        "share.withdraw" => d.share_withdraw(s(p, "id")?, p["by"].as_str().unwrap_or("owner"))?,
        "agent.share_deny" => d.share_deny(s(p, "run_id")?, p["denied"].as_bool().unwrap_or(true), p["by"].as_str().unwrap_or("owner"))?,
        "overseer.cap" => match p["cap"].as_i64() {
            Some(c) => d.set_cap(c)?,
            None => json!({"cap": d.cap_of(), "self_started_today": d.self_started_today()}),
        },
        "run.queued" => d.queued_messages(s(p, "run_id")?)?,
        "run.unqueue" => d.unqueue_message(s(p, "run_id")?, p["id"].as_i64().unwrap_or(0))?,
        "overseer.tools" => d.overseer_tools(s(p, "token")?)?,
        "overseer.tool" => d.overseer_tool(s(p, "token")?, s(p, "name")?, &p["arguments"])?,
        "daemon.shutdown" => json!({"ok": true}),
        "daemon.stop_all" => d.stop_all()?,
        "daemon.background_notice" => json!({"notice": d.background_notice()?}),
        "daemon.test_notice" => {
            let via = crate::background::notify("Overseer notifications are on", "This is how Overseer tells you agents are still running after VS Code closes.");
            crate::log(&format!("test notice ({via})"));
            json!({"delivered_via": via})
        }
        "daemon.last_notice" => {
            use rusqlite::OptionalExtension;
            let store = d.store.lock().unwrap();
            let notice = store
                .conn
                .query_row("SELECT seq, ts, payload FROM events WHERE kind='background_notice' ORDER BY seq DESC LIMIT 1", [], |r| {
                    Ok(json!({"seq": r.get::<_, i64>(0)?, "ts": r.get::<_, i64>(1)?, "payload": serde_json::from_str::<Value>(&r.get::<_, String>(2)?).unwrap_or(Value::Null)}))
                })
                .optional()?;
            json!({"notice": notice})
        }
        // "vscode" is kept for older callers; it counts every watching UI (VS Code windows and TUIs).
        "daemon.clients" => {
            let n = d.ui_clients.load(std::sync::atomic::Ordering::SeqCst);
            json!({"vscode": n, "ui": n})
        }
        // Continuity (Gate L): connection state, settings, local inventory, pick and guard.
        m if crate::continuity::handles(m) => crate::continuity::dispatch(d, m, p)?,
        other => return Err(anyhow!("unknown method {other}")),
    })
}

#[cfg(test)]
mod auto_collector_tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Barrier;

    #[test]
    fn concurrent_public_status_reads_share_one_fetch_then_refresh() {
        let dir = tempfile::tempdir().unwrap();
        let program = dir.path().join("fixture-curl");
        let counter = dir.path().join("status-count");
        std::fs::write(&program, format!("#!/bin/sh\nsleep 0.25\nprintf 'x\\n' >> '{}'\nprintf '%s' '{{\"components\":[{{\"name\":\"Claude Code\",\"status\":\"operational\"}}]}}'\n", counter.display())).unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700)).unwrap();
        let barrier = Barrier::new(3);
        let readings = std::thread::scope(|scope| {
            let first = scope.spawn(|| {
                barrier.wait();
                collect_public_status_shared(&program, "anthropic", Duration::from_millis(750)).unwrap()
            });
            let second = scope.spawn(|| {
                barrier.wait();
                collect_public_status_shared(&program, "anthropic", Duration::from_millis(750)).unwrap()
            });
            barrier.wait();
            [first.join().unwrap(), second.join().unwrap()]
        });
        assert_eq!(readings[0].observed_ms, readings[1].observed_ms);
        assert_eq!(std::fs::read_to_string(&counter).unwrap().lines().count(), 1);
        collect_public_status_shared(&program, "anthropic", Duration::from_millis(750)).unwrap();
        assert_eq!(std::fs::read_to_string(&counter).unwrap().lines().count(), 2,
            "a later decision must fetch fresh public status");
    }

    #[test]
    fn hundred_candidates_coalesce_duplicate_keys_with_four_collectors_and_a_deadline() {
        let candidates = (0..100).map(|n| format!("pool-{}", n % 25)).collect::<Vec<_>>();
        let calls = AtomicUsize::new(0);
        let active = AtomicUsize::new(0);
        let peak = AtomicUsize::new(0);
        let start = Instant::now();
        let results: Vec<(String, Result<()>)> = collect_unique_bounded(&candidates,
            Duration::from_secs(8), |_, budget| {
                calls.fetch_add(1, Ordering::SeqCst);
                let current = active.fetch_add(1, Ordering::SeqCst) + 1;
                peak.fetch_max(current, Ordering::SeqCst);
                std::thread::sleep(budget);
                active.fetch_sub(1, Ordering::SeqCst);
                Err(anyhow!("stalled fixture collector"))
            });
        assert_eq!(results.len(), 25);
        assert_eq!(calls.load(Ordering::SeqCst), 25, "duplicate pool reads must coalesce");
        assert!(peak.load(Ordering::SeqCst) <= 4);
        assert!(start.elapsed() <= Duration::from_secs(11));
    }

    #[test]
    fn simultaneous_auto_decisions_never_run_more_than_four_collectors_total() {
        let barrier = Barrier::new(2);
        let active = AtomicUsize::new(0);
        let peak = AtomicUsize::new(0);
        std::thread::scope(|scope| {
            let launch = || {
                barrier.wait();
                let ids = (0..4).map(|n| format!("pool-{n}")).collect::<Vec<_>>();
                let _: Vec<(String, Result<()>)> = collect_unique_bounded(&ids,
                    Duration::from_secs(2), |_, _| {
                        let current = active.fetch_add(1, Ordering::SeqCst) + 1;
                        peak.fetch_max(current, Ordering::SeqCst);
                        std::thread::sleep(Duration::from_millis(250));
                        active.fetch_sub(1, Ordering::SeqCst);
                        Ok(())
                    });
            };
            let first = scope.spawn(launch);
            let second = scope.spawn(launch);
            first.join().unwrap();
            second.join().unwrap();
        });
        assert!(peak.load(Ordering::SeqCst) <= 4,
            "collector cap applies across concurrent decisions, not only each work unit");
    }
}
