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
                crate::swarm::reconcile_control_verifications(&mut daemon.store.lock().unwrap())?;
                let _serial = daemon.swarm_launch_lock.lock().unwrap();
                let expired = crate::swarm::expire_due(
                    &mut daemon.store.lock().unwrap(),
                    crate::daemon::now(),
                )?;
                for run in expired {
                    crate::swarm::interrupt_workers(&daemon, &run)?;
                }
                let timed_out_workers = crate::swarm::expire_jobs_due(
                    &mut daemon.store.lock().unwrap(),
                    crate::daemon::now(),
                )?;
                for worker in timed_out_workers {
                    if let Err(error) = daemon.interrupt(&worker) {
                        crate::log(&format!("swarm job deadline interrupt {worker} failed: {error}"));
                    }
                }
                crate::swarm::retry_revoked_interrupts(&daemon)?;
                crate::swarm::retry_stopping_interrupts(&daemon)?;
                crate::swarm::reconcile_terminal_workers(&daemon)?;
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
        "swarm.coverage" => crate::swarm::coverage_report(&d.store.lock().unwrap(), p)?,
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
            let fault_interrupt_once = p["fault_interrupt_once"] == true;
            if fault_interrupt_once { fixture_only()?; }
            let _serial = d.swarm_launch_lock.lock().unwrap();
            let mut stopped = crate::swarm::stop(&mut d.store.lock().unwrap(), p)?;
            stopped["workers"] = crate::swarm::interrupt_workers_with_fault(
                d, s(p,"run_id")?, fault_interrupt_once)?;
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
            crate::swarm::observe_availability(&mut d.store.lock().unwrap(), p)?
        }
        "swarm.policy.set" => crate::swarm::set_policy(&mut d.store.lock().unwrap(), p)?,
        "swarm.admit" => {
            fixture_only()?;
            let _serial = d.swarm_launch_lock.lock().unwrap();
            let pending = d.pending_agent_slots.lock().unwrap();
            crate::swarm::admit(&mut d.store.lock().unwrap(), p, *pending)?
        }
        "swarm.schedule.next" => {
            fixture_only()?;
            let _serial = d.swarm_launch_lock.lock().unwrap();
            let pending = d.pending_agent_slots.lock().unwrap();
            crate::swarm::schedule_next(&mut d.store.lock().unwrap(), p, *pending)?
        }
        "swarm.dispatch.next" => {
            fixture_only()?;
            crate::swarm::dispatch_next(d, p)?
        }
        "swarm.worker.launch" => {
            fixture_only()?;
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
            let pending = d.pending_agent_slots.lock().unwrap();
            let store = d.store.lock().unwrap();
            json!({"max_active": store.agent_limit()?, "active": store.active_agent_count()? + *pending})
        }
        "agents.limit.set" => {
            let limit = p["max_active"].as_i64().ok_or_else(|| anyhow!("max_active must be an integer"))?;
            let pending = d.pending_agent_slots.lock().unwrap();
            let store = d.store.lock().unwrap();
            store.set_agent_limit(limit)?;
            json!({"max_active": store.agent_limit()?, "active": store.active_agent_count()? + *pending})
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
        "run.follow_up" => json!(d.start_turn(s(p, "run_id")?, s(p, "prompt")?, true, &crate::daemon::TurnOpts::from_params(p)?)?),
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
