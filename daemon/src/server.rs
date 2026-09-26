//! Versioned local protocol: newline-delimited JSON over an owner-only Unix socket.
//! Requests: {"id": n, "method": "...", "params": {...}}.
//! Responses: {"id": n, "result": ...} or {"id": n, "error": {"code", "message"}}.
//! Notifications: {"method": "event", "params": Event} and {"method": "resync", ...}.

use crate::daemon::{Daemon, ACTIVE};
use crate::paths;
use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::os::unix::io::AsRawFd;
use std::sync::Arc;
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
    let deadline_daemon = daemon.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            let daemon = deadline_daemon.clone();
            match tokio::task::spawn_blocking(move || {
                let _serial = daemon.swarm_launch_lock.lock().unwrap();
                let expired = crate::swarm::expire_due(
                    &mut daemon.store.lock().unwrap(),
                    crate::daemon::now(),
                )?;
                for run in expired {
                    crate::swarm::interrupt_workers(&daemon, &run)?;
                }
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
    loop {
        let (stream, _) = listener.accept().await?;
        let peer = crate::shim::peer_uid_fd(stream.as_raw_fd());
        if peer != Some(uid) {
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
    loop {
        buf.clear();
        let n = (&mut reader).take(MAX_REQUEST_BYTES + 1).read_until(b'\n', &mut buf).await?;
        if n == 0 {
            break;
        }
        if buf.len() as u64 > MAX_REQUEST_BYTES {
            let _ = tx.send(json!({"id": null, "error": {"code": "request_too_large", "message": "request exceeds 1 MiB"}})).await;
            break;
        }
        let msg: Value = match serde_json::from_slice(&buf) {
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
        let daemon = daemon.clone();
        let tx = tx.clone();
        tokio::spawn(async move {
            let shutdown = method == "daemon.shutdown";
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
    drop(tx);
    let _ = writer.await;
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

fn fixture_only() -> Result<()> {
    if std::env::var("OVERSEER_SWARM_FIXTURE_API").as_deref() == Ok("1") {
        Ok(())
    } else {
        Err(anyhow!("fixture-only swarm transition; live runtime authority is not implemented"))
    }
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
        "swarm.create" => crate::swarm::create(&mut d.store.lock().unwrap(), p)?,
        "swarm.get" => crate::swarm::get(&d.store.lock().unwrap(), s(p, "id")?)?,
        "swarm.plan" => {
            fixture_only()?;
            crate::swarm::plan(&mut d.store.lock().unwrap(), p)?
        }
        "swarm.jobs" => crate::swarm::jobs(&d.store.lock().unwrap(), p)?,
        "swarm.attempt.register" => {
            fixture_only()?;
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
            let _serial = d.swarm_launch_lock.lock().unwrap();
            let mut stopped = crate::swarm::stop(&mut d.store.lock().unwrap(), p)?;
            stopped["workers"] = crate::swarm::interrupt_workers(d, s(p,"run_id")?)?;
            stopped
        }
        "swarm.pause" => crate::swarm::pause(&mut d.store.lock().unwrap(), p)?,
        "swarm.resume" => crate::swarm::resume(&mut d.store.lock().unwrap(), p)?,
        "swarm.off" => crate::swarm::off(&mut d.store.lock().unwrap(), p)?,
        "swarm.claim" => {
            fixture_only()?;
            crate::swarm::claim(&mut d.store.lock().unwrap(), p)?
        }
        "swarm.artifact.put" => crate::swarm::put(&mut d.store.lock().unwrap(), p)?,
        "swarm.decide" => {
            fixture_only()?;
            crate::swarm::decide(&mut d.store.lock().unwrap(), p)?
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
        "swarm.policy.set" => crate::swarm::set_policy(&mut d.store.lock().unwrap(), p)?,
        "swarm.admit" => {
            fixture_only()?;
            crate::swarm::admit(&mut d.store.lock().unwrap(), p)?
        }
        "swarm.worker.launch" => {
            fixture_only()?;
            crate::swarm::launch_worker(d, p)?
        }
        "swarm.worker.reconcile" => {
            fixture_only()?;
            crate::swarm::reconcile_worker(d, p)?
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
        "comparison.options" => d.comparisons(s(p, "run_id")?, p["branch"].as_str())?,
        "workspace.diff" => d.workspace_diff_opts(s(p, "workspace_id")?, s(p, "base")?, p["status"].as_bool().unwrap_or(true))?,
        "workspace.status" => {
            let ws = d.workspace(s(p, "workspace_id")?)?;
            json!(crate::git::status(std::path::Path::new(&ws.path))?)
        }
        "workspace.cleanup_plan" => d.cleanup_plan(s(p, "workspace_id")?)?,
        "workspace.cleanup" => d.cleanup(s(p, "workspace_id")?, p["discard_dirty"].as_bool().unwrap_or(false))?,
        "daemon.shutdown" => json!({"ok": true}),
        other => return Err(anyhow!("unknown method {other}")),
    })
}
