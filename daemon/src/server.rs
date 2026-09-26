//! Versioned local protocol: newline-delimited JSON over an owner-only Unix socket.
//! Requests: {"id": n, "method": "...", "params": {...}}.
//! Responses: {"id": n, "result": ...} or {"id": n, "error": {"code", "message"}}.
//! Notifications: {"method": "event", "params": Event} and {"method": "resync", ...}.

use crate::daemon::{Daemon, ACTIVE};
use crate::paths;
use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::os::unix::io::AsRawFd;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock, TryLockError};
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::mpsc;

pub const PROTOCOL_VERSION: i64 = 1;
pub const MAX_REQUEST_BYTES: u64 = 1024 * 1024;

pub async fn serve(daemon: Arc<Daemon>) -> Result<()> {
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
    let uid = unsafe { libc::getuid() };
    // Test-only: pretend the owner is another uid. It can only reject more peers (a peer must
    // still be this process's own uid), so it lets a test observe a "foreign" connection being
    // refused without a second macOS account; it can never admit a different user.
    let expected = std::env::var("OVERSEER_TEST_EXPECT_UID").ok().and_then(|v| v.parse::<u32>().ok());
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
        if method == "hello" && params["client"] == "vscode" && !*ui {
            // A VS Code window: counted so closing the last one can surface background agents.
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
                Ok(Err(e)) => json!({"id": id, "error": {"code": "failed", "message": e.to_string()}}),
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

fn s<'a>(p: &'a Value, key: &str) -> Result<&'a str> {
    p[key].as_str().ok_or_else(|| anyhow!("missing string parameter {key}"))
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

struct AutoProfileDiscovery {
    routes: Vec<crate::auto_select::Route>,
    evidence: Value,
    generation: i64,
}

fn discover_auto_profile(d: &Arc<Daemon>, profile_id: &str, workspace_id: &str,
    budget: Duration) -> Result<AutoProfileDiscovery> {
    let deadline = Instant::now() + budget;
    let profile = d.profile(profile_id)?;
    match profile.harness.as_str() {
        "codex" => {
            let models = dispatch(d, "auto.models.refresh", &json!({"profile_id":profile_id,
                "timeout_ms":remaining_metadata_ms(deadline)?}))?;
            let first_generation = d.store.lock().unwrap().auto_account_generation(profile_id)?;
            let tools = dispatch(d, "auto.tools.inspect", &json!({"profile_id":profile_id,
                "workspace_id":workspace_id,"timeout_ms":remaining_metadata_ms(deadline)?}))?;
            let generation = d.store.lock().unwrap().auto_account_generation(profile_id)?;
            if generation.is_none() || generation != first_generation {
                return Err(anyhow!("account changed during automatic route discovery"));
            }
            let catalog: crate::auto_route::ModelCatalog = serde_json::from_value(models["catalog"].clone())?;
            let tool_catalog: crate::auto_route::ToolCatalog = serde_json::from_value(tools["catalog"].clone())?;
            let observation = d.store.lock().unwrap().latest_auto_quota(profile_id)?;
            let now_ms = crate::daemon::now();
            Ok(AutoProfileDiscovery {
                routes:crate::auto_route::codex_auto_routes(&catalog, &tool_catalog,
                    observation.as_ref().map(|value| &value.snapshot), profile_id, now_ms),
                evidence:json!({"profile_id":profile_id,"source":"codex-app/model-and-tool-metadata",
                    "model_observed_ms":catalog.observed_ms,"tool_observed_ms":tool_catalog.observed_ms,
                    "quota_observed_ms":observation.as_ref().map(|value| value.snapshot.observed_ms),
                    "account_generation":generation}),
                generation:generation.unwrap(),
            })
        }
        "claude" => {
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
            let observation = store.latest_auto_quota(profile_id)?;
            let now_ms = crate::daemon::now();
            Ok(AutoProfileDiscovery {
                routes:crate::auto_route::claude_auto_routes(&auth,
                    observation.as_ref().map(|value| &value.snapshot), profile_id, now_ms),
                evidence:json!({"profile_id":profile_id,"source":"claude/auth-status-and-native-quota",
                    "auth_observed_ms":auth.observed_ms,
                    "quota_observed_ms":observation.as_ref().map(|value| value.snapshot.observed_ms),
                    "account_generation":generation}),
                generation,
            })
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

pub fn dispatch(d: &Arc<Daemon>, method: &str, p: &Value) -> Result<Value> {
    Ok(match method {
        "hello" => json!({"protocol": PROTOCOL_VERSION, "version": env!("CARGO_PKG_VERSION"), "pid": std::process::id(), "data_dir": paths::data_dir(), "socket": paths::socket_path()}),
        "state" => d.state()?,
        "harness.list" => {
            let list: Vec<Value> = ["codex", "codex-app", "claude", "opencode", "generic"]
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
        "profile.create" => json!(d.create_profile(s(p, "name")?, s(p, "harness")?)?),
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
        "run.delegate" => d.delegate_run(p)?,
        "auto.dispatch" => {
            use crate::auto_select::{CapabilityTier, Sandbox, WorkUnit};
            use sha2::{Digest, Sha256};
            use std::collections::{BTreeMap, BTreeSet};

            for field in p.as_object().ok_or_else(|| anyhow!("automatic work request must be an object"))?.keys() {
                if !matches!(field.as_str(), "work_unit_id" | "parent_run_id" | "prompt" | "title"
                    | "min_tier" | "required_tools" | "context_needed" | "requires_approvals"
                    | "pinned_route" | "preferred_harness" | "allowed_profiles") {
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
            if parent.parent_run_id.is_some() || parent.status != "completed" {
                return Err(anyhow!("automatic delegation requires a completed top-level parent"));
            }
            let profile_id = parent.profile_id.as_deref().ok_or_else(|| anyhow!("parent has no account profile"))?;
            let profile = d.profile(profile_id)?;
            if !matches!(profile.harness.as_str(), "codex" | "claude") {
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
                        if !matches!(candidate.harness.as_str(), "codex" | "claude") {
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
            let requirements_hash = Sha256::digest(serde_json::to_vec(&requirements)?)
                .iter().map(|byte| format!("{byte:02x}")).collect::<String>();
            let saved_work_unit = {
                let store = d.store.lock().unwrap();
                store.managed_work_unit(work_unit_id)?
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
                let replay_state = if child.status == "failed" { "paused" } else { "dispatched" };
                let mut replay = json!({"state":replay_state,"work_unit_id":work_unit_id,"run":child,
                    "workspace":d.workspace(&child.workspace_id)?,"replayed":true,
                    "decision":{"work_unit_id":work_unit_id,"selected":route_id,"exclusions":[],"reason":"replayed_existing_work_unit"}});
                if replay_state == "paused" {
                    replay["actions"] = json!(["refresh", "choose_manual_route"]);
                }
                replay
            } else {
                let mut routes = Vec::new();
                let mut evidence = Vec::new();
                let mut discovery_failures = Vec::new();
                let mut account_generations = BTreeMap::new();
                let decision_deadline = Instant::now() + Duration::from_secs(10);
                let candidate_ids = allowed_profiles.iter().cloned().collect::<Vec<_>>();
                for (candidate_id, discovered) in collect_unique_bounded(&candidate_ids,
                    Duration::from_secs(8).min(decision_deadline.saturating_duration_since(Instant::now())), |id, budget|
                        discover_auto_profile(d, id, &parent.workspace_id, budget)) {
                    match discovered {
                        Ok(discovered) => {
                            routes.extend(discovered.routes);
                            account_generations.insert(candidate_id, discovered.generation);
                            evidence.push(discovered.evidence);
                        }
                        Err(_) => discovery_failures.push(json!({"profile_id":candidate_id,
                            "reason":"metadata_or_auth_unavailable"})),
                    }
                }
                if routes.len() > 128 { return Err(anyhow!("automatic candidate catalog exceeded its bound")); }
                let health_now = crate::daemon::now();
                match crate::auto_health::recent_local_observations(&d.store.lock().unwrap(), health_now) {
                    Ok(observations) => {
                        for route in &mut routes {
                            route.health = crate::auto_health::evaluate(route, &observations, health_now);
                        }
                    }
                    Err(_) => discovery_failures.push(json!({"reason":"local_health_evidence_unavailable"})),
                }
                let work = WorkUnit { id:work_unit_id.into(), min_tier, required_tools:required_tools.clone(),
                    context_needed, requires_approvals, min_sandbox:Sandbox::WorkspaceWrite,
                    max_sandbox:Sandbox::WorkspaceWrite,
                    allowed_profiles:allowed_profiles.clone(),
                    pinned_route, preferred_harness };
                let mut decision = crate::auto_select::select(&work, &routes);
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
                    decision = crate::auto_select::select(&work, &routes);
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
                let trace = json!({"selector_version":"multi-harness-preflight-v1","decision":decision,
                    "selection_input":{"work":&work,"routes":&routes,
                        "attempt_limit_reached":attempt_limit_reached,
                        "deadline_exhausted":deadline_exhausted},
                    "requirements":{"min_tier":min_tier,"required_tools":required_tools,
                        "context_needed":context_needed,"requires_approvals":requires_approvals},
                    "evidence":evidence,"discovery_failures":discovery_failures,
                    "pre_effect_failures":pre_effect_failures,
                    "candidates":routes.iter().map(|route| json!({"id":route.id,"quota":route.quota,
                        "fit":route.fit,"health":route.health})).collect::<Vec<_>>()});
                d.emit(Some(&parent.task_id), Some(&parent.id), "auto_decision", "daemon", "exact", trace)?;
                if let Some(selected) = decision.selected.as_deref() {
                    let route = routes.iter().find(|route| route.id == selected)
                        .ok_or_else(|| anyhow!("selected route disappeared"))?;
                    let generation = account_generations.get(&route.profile_id)
                        .ok_or_else(|| anyhow!("selected account generation unavailable"))?;
                    let required = required_tools.iter().cloned().collect::<Vec<_>>();
                    let mut delegated = d.delegate_run(&json!({"work_unit_id":work_unit_id,
                        "parent_run_id":parent.id,"harness":route.harness,"profile_id":route.profile_id,
                        "model":route.model,"effort":route.effort,"prompt":prompt,"title":title,
                        "required_tools":required,"auto_selected":true,
                        "requirements_hash":requirements_hash,"expected_account_generation":generation}))?;
                    if delegated.get("launch_error").is_some() {
                        delegated["state"] = json!("paused");
                        delegated["actions"] = json!(["refresh", "choose_manual_route"]);
                    } else {
                        delegated["state"] = json!("dispatched");
                    }
                    delegated["decision"] = json!(decision);
                    delegated["discovery_failures"] = json!(discovery_failures);
                    delegated["pre_effect_failures"] = json!(pre_effect_failures);
                    delegated
                } else {
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
            let routes: Vec<crate::auto_select::Route> = serde_json::from_value(input["routes"].clone())?;
            if routes.len() > 128 { return Err(anyhow!("automatic decision replay exceeded its candidate bound")); }
            let decision = match selector_version {
                "codex-cold-start-v1" => crate::auto_select::select_legacy_v1(&work, &routes),
                "codex-cold-start-v2" | "multi-harness-cold-start-v1" => crate::auto_select::select(&work, &routes),
                "multi-harness-preflight-v1" => {
                    let mut decision = crate::auto_select::select(&work, &routes);
                    if input["attempt_limit_reached"] == true {
                        decision.selected = None;
                        decision.reason = "pre_effect_attempt_limit".into();
                    }
                    if input["deadline_exhausted"] == true {
                        decision.selected = None;
                        decision.reason = "collection_deadline_elapsed".into();
                    }
                    decision
                }
                _ => return Err(anyhow!("unsupported automatic selector version")),
            };
            let matches_recorded = serde_json::to_value(&decision)? == payload["decision"];
            json!({"event_seq":event_seq,"matches_recorded":matches_recorded,"decision":decision})
        }
        "run.result" => d.delegated_result(s(p, "run_id")?)?,
        "run.follow_up" => json!(d.start_turn(s(p, "run_id")?, s(p, "prompt")?, true)?),
        "run.interrupt" => d.interrupt(s(p, "run_id")?)?,
        "run.permission" => d.answer_permission(s(p, "run_id")?, s(p, "request_id")?, p["allow"].as_bool().unwrap_or(false), p["message"].as_str().unwrap_or(""))?,
        "run.raw_output" => d.raw_output(s(p, "run_id")?, p["max_bytes"].as_u64().unwrap_or(256 * 1024).min(4 * 1024 * 1024) as usize)?,
        "run.turns" => json!(d.store.lock().unwrap().turns(s(p, "run_id")?)?),
        "run.active" => {
            let runs = d.store.lock().unwrap().runs()?;
            json!(runs.into_iter().filter(|r| ACTIVE.contains(&r.status.as_str())).collect::<Vec<_>>())
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
                let snapshot = crate::auto_quota::parse_codex_rate_limits(&raw.rate_limits, &profile.id, observed_ms)?;
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
            let workspace = d.workspace(s(p, "workspace_id")?)?;
            if workspace.removed_ms.is_some() { return Err(anyhow!("workspace was removed")); }
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
                std::path::Path::new(&workspace.path), remaining, crate::daemon::now())?;
            let mut endpoint_health = BTreeMap::new();
            let mut local_execution_config_verified = BTreeMap::new();
            let mut local_execution_config_reason = BTreeMap::new();
            for model in &catalog.models {
                let eligibility = crate::auto_opencode::auto_local_execution_guard(&profile,
                    std::path::Path::new(&workspace.path), &model.model, &model.endpoint);
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
            json!({"profile_id":profile.id,"workspace_id":workspace.id,
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
            let workspace = d.workspace(s(p, "workspace_id")?)?;
            if workspace.removed_ms.is_some() {
                return Err(anyhow!("workspace was removed"));
            }
            let gate = d.profile_gate(&profile.id);
            let _profile_guard = lock_gate_until(&gate, deadline)?;
            let read = (|| -> anyhow::Result<_> {
                let program = crate::adapters::resolve_program("codex-app").ok_or_else(|| anyhow!("Codex is not installed"))?;
                let mut env = crate::adapters::base_env(&program.display().to_string());
                env.extend(Daemon::profile_env(&profile));
                let raw = crate::auto_collect::codex_tool_inventory(&program, &env,
                    std::path::Path::new(&workspace.path), Duration::from_millis(remaining_metadata_ms(deadline)?))?;
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
            json!({"profile_id":profile.id,"workspace_id":workspace.id,"source":"codex-app/mcpServerStatus-list","catalog":catalog,
                "limitation":"discovery only; effective child tool permission requires launch-time verification"})
        }
        "auto.quota.refresh" => {
            let profile = d.profile(s(p, "profile_id")?)?;
            if profile.harness != "codex" {
                return Err(anyhow!("structured quota refresh is not supported for this profile"));
            }
            let gate = d.profile_gate(&profile.id);
            let _profile_guard = gate.lock().unwrap();
            let active = d.store.lock().unwrap().runs()?.iter().any(|run| run.profile_id.as_deref() == Some(profile.id.as_str()) && crate::daemon::ACTIVE.contains(&run.status.as_str()));
            if active {
                return Err(anyhow!("account profile has an active run; use its native updates"));
            }
            let read = (|| -> anyhow::Result<_> {
                let program = crate::adapters::resolve_program("codex-app").ok_or_else(|| anyhow!("Codex is not installed"))?;
                let mut env = crate::adapters::base_env(&program.display().to_string());
                env.extend(Daemon::profile_env(&profile));
                let raw = crate::auto_collect::codex_rate_limits(&program, &env, &crate::adapters::neutral_dir(), std::time::Duration::from_secs(5))?;
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
            let rows = d.store.lock().unwrap().auto_measurements(p["limit"].as_i64().unwrap_or(100).clamp(1, 5000))?;
            json!({"measurements": rows, "learning_paused": d.learning_paused.load(std::sync::atomic::Ordering::Relaxed)})
        }
        "auto.usage.thread.refresh" => {
            let run = d.run(s(p, "run_id")?)?;
            if run.harness != "codex-app" || run.status != "completed" {
                return Err(anyhow!("thread usage is available only after a completed Codex app-server run"));
            }
            let profile_id = run.profile_id.as_deref().ok_or_else(|| anyhow!("run account profile is unavailable"))?;
            let thread_id = run.native_id.as_deref().ok_or_else(|| anyhow!("run thread identity is unavailable"))?;
            let gate = d.profile_gate(profile_id);
            let _profile_guard = gate.lock().unwrap();
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
                let raw = crate::auto_collect::codex_thread_usage(&program, &env, &crate::adapters::neutral_dir(), thread_id, std::time::Duration::from_secs(5))?;
                let observed_ms = crate::daemon::now();
                let fingerprint = crate::auto_quota::account_fingerprint(&raw.rate_limits)?;
                let snapshot = crate::auto_quota::parse_codex_rate_limits(&raw.rate_limits, profile_id, observed_ms)?;
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
            store.record_auto_account_identity(profile_id, &fingerprint)?;
            let event = store.insert_event(observed_ms, None, None, "quota", "codex-app/metadata-read", "reported", &json!({"profile_id":profile_id,"snapshot":snapshot}))?;
            store.insert_auto_quota(event.seq, profile_id, "codex-app/metadata-read", &snapshot)?;
            let observation = if let Some(estimate) = estimate {
                let generation = store.auto_account_generation(profile_id)?.ok_or_else(|| anyhow!("account generation is unavailable"))?;
                let attribution = if store.auto_run_account_matches(&run.id, profile_id, generation)? {
                    "same_account_generation"
                } else {
                    "unverified_run_account"
                };
                let id = store.insert_auto_thread_usage(&run.id, profile_id, generation, "codex-app/account-usage-read", &estimate)?;
                Some(json!({"id":id,"run_id":run.id,"profile_id":profile_id,"read_account_generation":generation,"attribution":attribution,"subscription_window_relation":"unverified","source":"codex-app/account-usage-read","estimate":estimate}))
            } else { None };
            json!({"state":if observation.is_some() {"estimated"} else {"unavailable"},"observation":observation})
        }
        "auto.usage.thread.list" => {
            let rows = d.store.lock().unwrap().auto_thread_usage_observations(p["limit"].as_i64().unwrap_or(100).clamp(1, 5000))?;
            json!({"observations":rows})
        }
        "auto.usage.summary" => {
            let rows = d.store.lock().unwrap().auto_daily_aggregates(p["limit"].as_i64().unwrap_or(100).clamp(1, 10_000))?;
            json!({"aggregates": rows})
        }
        "auto.usage.export" => {
            use std::io::Write;
            use std::os::unix::fs::OpenOptionsExt;
            let path = std::path::Path::new(s(p, "path")?);
            if !path.is_absolute() {
                return Err(anyhow!("export path must be absolute"));
            }
            let store = d.store.lock().unwrap();
            let rows = store.auto_measurements(50_000)?;
            let aggregates = store.auto_daily_aggregates(10_000)?;
            let thread_usage_observations = store.auto_thread_usage_observations(5000)?;
            drop(store);
            let mut file = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(path)?;
            let result = serde_json::to_writer_pretty(&mut file, &json!({
                "schema_version": 2, "generated_ms": crate::daemon::now(),
                "measurements": rows, "aggregates": aggregates,
                "thread_usage_observations": thread_usage_observations,
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
        "daemon.clients" => json!({"vscode": d.ui_clients.load(std::sync::atomic::Ordering::SeqCst)}),
        other => return Err(anyhow!("unknown method {other}")),
    })
}

#[cfg(test)]
mod auto_collector_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Barrier;

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
