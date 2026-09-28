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

/// An error with its own protocol code (and data), for refusals a client acts on.
/// Anything else is reported as `failed` with its message.
#[derive(Debug)]
pub struct ProtoError {
    pub code: &'static str,
    pub message: String,
    pub data: Value,
}

impl ProtoError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self { code, message: message.into(), data: Value::Null }
    }

    pub fn with_data(mut self, data: Value) -> Self {
        self.data = data;
        self
    }
}

impl std::fmt::Display for ProtoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ProtoError {}

/// The error object of a reply.
pub fn error_value(e: &anyhow::Error) -> Value {
    match e.downcast_ref::<ProtoError>() {
        Some(p) if p.data.is_null() => json!({"code": p.code, "message": p.message}),
        Some(p) => json!({"code": p.code, "message": p.message, "data": p.data}),
        None => json!({"code": "failed", "message": e.to_string()}),
    }
}

thread_local! {
    /// Who is acting on this thread while a request runs: `None` is the local user, otherwise a
    /// device (`phone:<name>`). Events a user causes carry it as their source.
    static ACTOR: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

/// Runs `f` as `actor`. Requests run to completion on one blocking thread, so a thread-local is enough.
pub fn with_actor<T>(actor: Option<String>, f: impl FnOnce() -> T) -> T {
    ACTOR.with(|a| *a.borrow_mut() = actor);
    let out = f();
    ACTOR.with(|a| *a.borrow_mut() = None);
    out
}

pub fn actor() -> Option<String> {
    ACTOR.with(|a| a.borrow().clone())
}

pub async fn serve(daemon: Arc<Daemon>) -> Result<()> {
    crate::audio::start(daemon.clone())?;
    crate::overseer::conflicts::start(daemon.clone());
    crate::overseer::session::start(daemon.clone());
    crate::voice::start(daemon.clone());
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
    crate::gateway::start(&daemon);
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
    static CONNECTIONS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let connection_id = CONNECTIONS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let result = connection_loop(&daemon, &mut reader, &mut buf, &tx, &mut ui, connection_id).await;
    daemon.gateway.focus.lock().unwrap().remove(&connection_id);
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
    connection_id: u64,
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
        if method == "ui.focus" {
            // Which agent this window is looking at, so a phone is not notified about it (AC-129).
            {
                let mut focus = daemon.gateway.focus.lock().unwrap();
                match params["run_id"].as_str().filter(|_| params["focused"].as_bool() != Some(false)) {
                    Some(run) => focus.insert(connection_id, run.to_string()),
                    None => focus.remove(&connection_id),
                };
            }
            let _ = tx.send(json!({"id": id, "result": {"ok": true}})).await;
            continue;
        }
        if method == "voice.subscribe" {
            // Voice Mode's live channel: state, levels, words in progress. Never stored.
            crate::voice::subscribe(id, tx.clone());
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
                Ok(Err(e)) => json!({"id": id, "error": error_value(&e)}),
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
pub(crate) fn subscribe(daemon: Arc<Daemon>, id: Value, params: Value, tx: mpsc::Sender<Value>) {
    let mut live = daemon.events.subscribe();
    tokio::spawn(async move {
        let mut cursor = params["after"].as_i64().unwrap_or(0);
        let run_filter = params["run_id"].as_str().map(str::to_string);
        // History is gone when the log no longer reaches back to the cursor, or when a run's older
        // events after the cursor were pruned (each pruning leaves a retention marker).
        let newest = daemon.store.lock().unwrap().max_seq().unwrap_or(0);
        let beyond = cursor > newest;
        let gap = beyond || cursor > 0 && {
            let store = daemon.store.lock().unwrap();
            let oldest = store.conn.query_row("SELECT MIN(seq) FROM events", [], |r| r.get::<_, Option<i64>>(0)).ok().flatten();
            matches!(oldest, Some(o) if o > cursor + 1) || store.pruned_after(cursor, run_filter.as_deref()).unwrap_or(false)
        };
        if beyond {
            // The client's cursor is past the end of the log: the log started again (another
            // data folder, a restored backup). Without this nothing would reach the client until
            // the new log grew past its old cursor. It reloads state; events go on from here.
            cursor = newest;
        }
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

pub fn dispatch(d: &Arc<Daemon>, method: &str, p: &Value) -> Result<Value> {
    Ok(match method {
        "hello" => json!({"protocol": PROTOCOL_VERSION, "version": env!("CARGO_PKG_VERSION"), "pid": std::process::id(), "data_dir": paths::data_dir(), "socket": paths::socket_path(), "instance": paths::instance()}),
        "state" => d.state_for(p["include_hidden"].as_bool().unwrap_or(false))?,
        "audio.get" => crate::audio::get(d)?,
        "audio.set" => crate::audio::set(d, p)?,
        "audio.preview" => crate::audio::preview(d, p)?,
        "audio.import_commander" => crate::audio::import_commander(d, p)?,
        "audio.voices" => crate::audio::voices()?,
        "voice.get" => crate::voice::get(d)?,
        "voice.set" => crate::voice::set(d, p)?,
        "voice.say" => crate::voice::say(d, p)?,
        "voice.simulate" => crate::voice::simulate(d, p)?,
        "voice.speak" => crate::voice::speak(d, p)?,
        "voice.focus" => crate::voice::focus(d, p)?,
        "voice.download" => crate::voice::download(d, p)?,
        "voice.requests" => crate::voice::request::list(d, p)?,
        "voice.cancel" => crate::voice::request::cancel(d, p)?,
        "voice.read_back" => crate::voice::request::read_back(d, p)?,
        "voice.answer" => crate::voice::request::answer(d, p)?,
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
        "workspace.file" => d.workspace_file(s(p, "workspace_id")?, s(p, "path")?, p["base"].as_str())?,
        "workspace.hunks" => d.workspace_hunks(s(p, "workspace_id")?, s(p, "path")?, s(p, "base")?, p["run_id"].as_str())?,
        "review.marks" => d.review_marks(s(p, "run_id")?)?,
        "review.accept" => d.review_accept(s(p, "run_id")?, p)?,
        "review.unaccept" => d.review_unaccept(s(p, "run_id")?, s(p, "key")?)?,
        "review.import" => d.review_import(s(p, "run_id")?, &p["marks"])?,
        "review.reject" => d.review_reject(s(p, "workspace_id")?, p)?,
        "workspace.pr_open" => d.pr_open(s(p, "workspace_id")?, p)?,
        "profile.device_login" => d.device_login(s(p, "id")?)?,
        "repo.known" => d.known_repos()?,
        "runs.stop_all" => d.stop_all_runs()?,
        m if m.starts_with("gateway.") => crate::gateway::local::dispatch(d, m, p)?,
        // Continuity (Gate L): connection state, settings, local inventory, pick and guard.
        m if crate::continuity::handles(m) => crate::continuity::dispatch(d, m, p)?,
        other => return Err(ProtoError::new("unknown_method", format!("unknown method {other}")).into()),
    })
}
