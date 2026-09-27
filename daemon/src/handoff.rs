//! Keeping the work going (Continuity, AC-84, AC-91 to AC-93, AC-96 and the valve of AC-140).
//!
//! A turn that fails on the connection does not end its run. The run is **parked**
//! (`waiting_for_connection`) with its message kept, and a scheduler decides, every time the run
//! is due, what happens next:
//!
//! - the provider answers again: the same turn is **retried** through the harness's own resume;
//! - Continuity is on and another provider works: the work is **handed off** to it;
//! - Continuity is on and nothing online works: it is handed off to a **local** model that fits;
//! - otherwise it waits, with a backoff, for at most the owner's 36 hours.
//!
//! A handoff is a successor run in the same task and worktree, started with a bounded prompt
//! built from the daemon's own records. The predecessor ends as `handed_off`, never as failed.
//! Permission modes are carried over and never loosened; where the target cannot honour a mode,
//! nothing moves on its own and the move is offered instead.

use crate::accounts::provider_of;
use crate::continuity::{self, provider_name, Conn, Status, KNOWN_PROVIDERS};
use crate::daemon::{now, Daemon, TurnOpts, ACTIVE};
use crate::opencode_bridge;
use crate::store::{Run, Turn};
use crate::sys::{self, Pressure};
use anyhow::{anyhow, bail, Result};
use serde::Serialize;
use serde_json::{json, Value};
use std::path::Path;
use std::sync::Arc;

pub const WAITING_FOR_CONNECTION: &str = "waiting_for_connection";
pub const WAITING_FOR_MEMORY: &str = "waiting_for_memory";
pub const HANDED_OFF: &str = "handed_off";

pub fn ensure_tables(conn: &rusqlite::Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS continuity_waits(run_id TEXT PRIMARY KEY, turn_id TEXT NOT NULL, kind TEXT NOT NULL, provider TEXT NOT NULL, reason TEXT NOT NULL,
           attempts INTEGER NOT NULL DEFAULT 0, started_ms INTEGER NOT NULL, next_ms INTEGER NOT NULL, note TEXT, scheduled_ms INTEGER NOT NULL DEFAULT 0);
         CREATE TABLE IF NOT EXISTS continuity_handoffs(id INTEGER PRIMARY KEY AUTOINCREMENT, predecessor TEXT NOT NULL, successor TEXT NOT NULL, reason TEXT NOT NULL, at_ms INTEGER NOT NULL,
           stay INTEGER NOT NULL DEFAULT 0, offered_ms INTEGER);",
    )?;
    Ok(())
}

#[derive(Serialize, Clone, Debug)]
pub struct Wait {
    pub run_id: String,
    pub turn_id: String,
    /// `connection` or `memory`.
    pub kind: String,
    pub provider: String,
    pub reason: String,
    /// Checks made so far that found nothing to do but wait.
    pub attempts: i64,
    pub started_ms: i64,
    pub next_ms: i64,
    pub note: Option<String>,
    /// When the next look was set.
    pub scheduled_ms: i64,
}

fn waits(d: &Daemon) -> Vec<Wait> {
    let store = d.store.lock().unwrap();
    let Ok(mut stmt) = store.conn.prepare("SELECT run_id, turn_id, kind, provider, reason, attempts, started_ms, next_ms, note, scheduled_ms FROM continuity_waits ORDER BY started_ms") else { return Vec::new() };
    let rows = stmt.query_map([], |r| Ok(Wait { run_id: r.get(0)?, turn_id: r.get(1)?, kind: r.get(2)?, provider: r.get(3)?, reason: r.get(4)?, attempts: r.get(5)?, started_ms: r.get(6)?, next_ms: r.get(7)?, note: r.get(8)?, scheduled_ms: r.get(9)? }));
    rows.map(|r| r.flatten().collect()).unwrap_or_default()
}

fn wait_of(d: &Daemon, run: &str) -> Option<Wait> {
    waits(d).into_iter().find(|w| w.run_id == run)
}

fn forget(d: &Daemon, run: &str) {
    let _ = d.store.lock().unwrap().conn.execute("DELETE FROM continuity_waits WHERE run_id=?1", [run]);
}

fn ms(var: &str, default: i64) -> i64 {
    std::env::var(var).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

/// 5 s, doubling to the cap, with a fifth of jitter either way.
pub fn backoff(attempts: i64, cap_seconds: u64, jitter: f64) -> i64 {
    let base = ms("OVERSEER_TEST_RETRY_BASE_MS", 5000);
    let cap = ms("OVERSEER_TEST_RETRY_CAP_MS", cap_seconds as i64 * 1000);
    let plain = base.saturating_mul(1i64 << attempts.clamp(0, 20)).min(cap);
    (plain as f64 * (1.0 + 0.2 * jitter.clamp(-1.0, 1.0))) as i64
}

fn jitter(seed: &str, attempts: i64) -> f64 {
    use sha2::Digest;
    let h = sha2::Sha256::digest(format!("{seed}:{attempts}").as_bytes());
    (h[0] as f64 / 255.0) * 2.0 - 1.0
}

fn clock(ms: i64) -> String {
    let secs = (ms / 1000) as libc::time_t;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe { libc::localtime_r(&secs, &mut tm) };
    format!("{:02}:{:02}", tm.tm_hour, tm.tm_min)
}

pub fn harness_name(harness: &str) -> &'static str {
    match harness {
        "claude" => "Claude Code",
        "codex" | "codex-app" => "Codex",
        "opencode" | "opencode-serve" => "OpenCode",
        _ => "the agent",
    }
}

fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    format!("{}…", s.chars().take(max).collect::<String>())
}

// ------------------------------------------------------------------ parking a run

/// Called when a turn's process has ended, before the run is marked. Returns true when the run
/// was parked, in which case it is not marked as ended.
pub fn park(d: &Daemon, run: &Run, status: &str, last_error: Option<&(String, String)>, dir: &Path) -> Result<bool> {
    if run.parent_run_id.is_some() {
        return Ok(false);
    }
    if run.status == WAITING_FOR_CONNECTION || run.status == WAITING_FOR_MEMORY {
        // Already parked (the daemon restarted and read the ended process again): it stays parked.
        if wait_of(d, &run.id).is_some() {
            return Ok(true);
        }
    }
    let stalled = dir.join("stall.requested").exists();
    let squeezed = dir.join("memory.requested").exists();
    if status == "interrupted" || (!stalled && !squeezed && status != "failed") {
        forget(d, &run.id);
        return Ok(false);
    }
    let provider = provider_of(&run.harness);
    let class = last_error.map(|(c, _)| c.as_str()).unwrap_or_default();
    let offline = continuity::status().is_some_and(|s| s.state == Conn::Offline);
    let (kind, reason) = if squeezed && provider == "local" {
        ("memory", "the system reported critical memory pressure; the local model was unloaded".to_string())
    } else if !KNOWN_PROVIDERS.contains(&provider) {
        return Ok(false);
    } else if stalled {
        ("connection", "no answer while offline; the turn was interrupted by Overseer".to_string())
    } else if class == "network" {
        ("connection", format!("the connection to {} failed: {}", provider_name(provider), clip(last_error.map(|(_, m)| m.as_str()).unwrap_or_default(), 200)))
    } else if class == "auth" && offline {
        // A sign-in cannot be checked without a connection: while offline this is the connection's failure.
        ("connection", format!("{} could not check the sign-in while offline", provider_name(provider)))
    } else {
        forget(d, &run.id);
        return Ok(false);
    };
    let Some(turn) = d.store.lock().unwrap().turns(&run.id)?.pop() else { return Ok(false) };
    let settings = continuity::settings();
    let (attempts, started) = match wait_of(d, &run.id) {
        Some(w) if w.turn_id == turn.id => (w.attempts + 1, w.started_ms),
        _ => (0, now()),
    };
    // The first look is five seconds away; after a retry that failed again the backoff goes on.
    let next = now() + if kind == "memory" { ms("OVERSEER_TEST_RETRY_BASE_MS", 5000) } else { backoff(attempts, settings.retry_cap_seconds, jitter(&run.id, attempts)) };
    let waiting = if kind == "memory" { WAITING_FOR_MEMORY } else { WAITING_FOR_CONNECTION };
    {
        let store = d.store.lock().unwrap();
        store.conn.execute(
            "INSERT INTO continuity_waits(run_id, turn_id, kind, provider, reason, attempts, started_ms, next_ms, scheduled_ms) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)
             ON CONFLICT(run_id) DO UPDATE SET turn_id=excluded.turn_id, kind=excluded.kind, provider=excluded.provider, reason=excluded.reason, attempts=excluded.attempts, started_ms=excluded.started_ms, next_ms=excluded.next_ms, scheduled_ms=excluded.scheduled_ms, note=NULL",
            rusqlite::params![run.id, turn.id, kind, provider, reason, attempts, started, next, now()],
        )?;
        store.update_run_status(&run.id, waiting, Some(&reason), None)?;
        store.set_run_attention(&run.id, None)?;
        store.conn.execute("UPDATE turns SET status='waiting' WHERE id=?1", [&turn.id])?;
    }
    d.emit(Some(&run.task_id), Some(&run.id), "status", "daemon", "exact", json!({"status": waiting, "reason": reason, "turn": turn.id, "message_kept": true}))?;
    Ok(true)
}

// ------------------------------------------------------------------ where the work can go

#[derive(Serialize, Clone, Debug)]
pub struct Target {
    /// `local` or a provider id.
    pub to: String,
    pub harness: String,
    pub profile_id: Option<String>,
    pub account: Option<String>,
    pub model: Option<String>,
    /// The mode the successor runs in.
    pub mode: Option<String>,
    /// A session of the target harness to continue (switching back).
    pub resume: Option<String>,
    /// Why the move cannot be made without asking, if it cannot.
    pub difference: Option<String>,
    pub label: String,
}

/// The mode of `from` on `to`, and what would be looser if it cannot be kept exactly.
pub fn carry_mode(from: &str, mode: Option<&str>, to: &str) -> (Option<String>, Option<String>) {
    let family = |h: &str| match h {
        "codex" | "codex-app" => "codex",
        "opencode" | "opencode-serve" => "opencode",
        other => if other == "claude" { "claude" } else { "other" },
    };
    match (family(from), family(to)) {
        (a, b) if a == b => (mode.map(str::to_string), None),
        (_, "opencode") => (Some(opencode_bridge::carried_mode(from, mode).to_string()), None),
        ("codex", "claude") => (Some(opencode_bridge::carried_mode(from, mode).to_string()), None),
        ("opencode", "claude") => (mode.map(str::to_string), None),
        (_, "codex") => match mode {
            Some("plan") | Some("read-only") => (Some("read-only".into()), None),
            Some("auto") | Some("workspace-write") => (Some("workspace-write".into()), None),
            Some("acceptEdits") => (Some("workspace-write".into()), Some("Codex runs commands in its sandbox without asking first".into())),
            _ => (Some("workspace-write".into()), Some("Codex edits files and runs commands in its sandbox without asking first".into())),
        },
        _ => (None, Some(format!("{} has no permission modes", harness_name(to)))),
    }
}

fn mode_of(d: &Daemon, run: &Run) -> Option<String> {
    let launch: Option<String> = d.store.lock().unwrap().conn.query_row("SELECT launch FROM runs WHERE id=?1", [&run.id], |r| r.get(0)).ok().flatten();
    let v: Value = launch.and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Value::Null);
    v["generic"]["opts"]["mode"].as_str().or(v["opts"]["mode"].as_str()).map(str::to_string)
}

/// The best other online provider that works now: reachable, its harness installed, an account
/// signed in. Among several accounts the one with the most quota left, then the most recently used.
pub fn failover_target(d: &Daemon, run: &Run, status: &Status) -> Option<Target> {
    failover_targets(d, run, status).into_iter().next()
}

/// The owner's order of online providers, every known provider after it, without the failing one.
pub fn provider_order(configured: &[String], failing: &str) -> Vec<String> {
    let mut order: Vec<String> = configured.to_vec();
    for p in KNOWN_PROVIDERS {
        if !order.iter().any(|o| o == p) {
            order.push(p.to_string());
        }
    }
    order.retain(|p| p != failing);
    order
}

/// Every other online provider that works now, best first.
pub fn failover_targets(d: &Daemon, run: &Run, status: &Status) -> Vec<Target> {
    let failing = provider_of(&run.harness);
    let settings = continuity::settings();
    let mut found = Vec::new();
    let runs = d.store.lock().unwrap().runs().unwrap_or_default();
    for provider in provider_order(&settings.provider_order, failing) {
        if status.providers.get(&provider).is_none_or(|h| h.reachable != Some(true)) {
            continue;
        }
        let harness = match provider.as_str() {
            "openai" => "codex",
            "anthropic" => "claude",
            _ => continue,
        };
        if crate::adapters::resolve_program(harness).is_none() {
            continue;
        }
        let profiles: Vec<_> = d.store.lock().unwrap().profiles().unwrap_or_default().into_iter().filter(|p| p.harness == harness).collect();
        let mut accounts: Vec<(bool, i64, i64, crate::store::Profile)> = Vec::new();
        for p in profiles {
            if d.profile_status(&p.id).map(|s| s["logged_in"] == true).unwrap_or(false) {
                let usage = d.account_usage(&p.id).unwrap_or(Value::Null);
                let used = usage["windows"].as_array().map(|w| w.iter().filter_map(|x| x["used"].as_f64()).fold(0.0, f64::max));
                let left = ((1.0 - used.unwrap_or(0.5)) * 1000.0) as i64;
                let last = runs.iter().filter(|r| r.profile_id.as_deref() == Some(&p.id)).map(|r| r.created_ms).max().unwrap_or(0);
                accounts.push((usage["limited"] == true, -left, -last, p));
            }
        }
        accounts.sort_by(|a, b| (a.0, a.1, a.2).cmp(&(b.0, b.1, b.2)));
        let Some((limited, _, _, account)) = accounts.into_iter().next() else { continue };
        if limited {
            continue;
        }
        // The model last used on that harness, otherwise the harness's own default.
        let model = runs.iter().rev().find(|r| r.harness == harness && r.model.is_some()).and_then(|r| r.model.clone());
        let (mode, difference) = carry_mode(&run.harness, mode_of(d, run).as_deref(), harness);
        found.push(Target { to: provider.clone(), harness: harness.into(), profile_id: Some(account.id.clone()), account: Some(account.name.clone()), model, mode, resume: None, difference, label: harness_name(harness).into() });
    }
    found
}

/// The local model that would take the work, or why none can.
pub fn local_target(d: &Daemon, run: &Run) -> Result<Target> {
    let Some(opencode) = crate::adapters::resolve_program(opencode_bridge::HARNESS) else {
        bail!("OpenCode is not installed, so no local model can take over");
    };
    let served = opencode_bridge::server_available(&opencode, &Daemon::profile_env(&opencode_bridge::local_profile(d)?));
    let picked = continuity::pick_value(d)?;
    if picked["ollama"]["running"] != true {
        bail!("{}", picked["ollama"]["detail"].as_str().unwrap_or("Ollama is not running"));
    }
    let chosen = &picked["pick"]["chosen"];
    if chosen.is_null() || chosen["installed"] != true {
        let why = picked["pick"]["rejected"].as_array().and_then(|r| r.first()).map(|r| format!("{}: {}", r["tag"].as_str().unwrap_or_default(), r["reason"].as_str().unwrap_or_default()));
        bail!("no local model is installed that is verified and fits the memory budget of {} GiB{}; downloads need a connection", sys::gib(picked["pick"]["budget"]["budget"].as_u64().unwrap_or(0)), why.map(|w| format!(" ({w})")).unwrap_or_default());
    }
    let tag = chosen["tag"].as_str().unwrap_or_default().to_string();
    if !served {
        // Without the server the local agent runs through `opencode run`, which cannot ask. That
        // is looser than any mode but Auto, so the move is offered with the difference stated.
        let asked = opencode_bridge::carried_mode(&run.harness, mode_of(d, run).as_deref());
        let difference = (asked != "auto").then(|| "this OpenCode has no server, so the local agent cannot ask before it edits or runs a command".to_string());
        return Ok(Target { to: "local".into(), harness: "opencode".into(), profile_id: Some(opencode_bridge::local_profile(d)?.id), account: None, model: None, mode: Some("auto".into()), resume: None, difference, label: tag });
    }
    let (mode, difference) = carry_mode(&run.harness, mode_of(d, run).as_deref(), opencode_bridge::HARNESS);
    // The model itself is chosen when the run is launched, on fresh numbers and through the guard.
    Ok(Target { to: "local".into(), harness: opencode_bridge::HARNESS.into(), profile_id: Some(opencode_bridge::local_profile(d)?.id), account: None, model: None, mode, resume: None, difference, label: tag })
}

// ------------------------------------------------------------------ the handoff

/// The prompt a successor starts with, built from the daemon's own records and bounded so that
/// it fits the smallest context Overseer runs.
pub fn handoff_prompt(task_prompt: &str, repo: &str, worktree: &str, branch: Option<&str>, messages: &[String], files: &Value, pending: Option<&str>, why: &str) -> String {
    let mut out = format!("You are continuing a task another agent started; {why}.\nTask: {}\n", clip(task_prompt.trim(), 4000));
    out.push_str(&format!("Repository {repo}, working tree {worktree}{}. Do not change branches.\n", branch.map(|b| format!(", branch {b}")).unwrap_or_default()));
    if messages.is_empty() {
        out.push_str("The previous agent had not reported anything yet.\n");
    } else {
        out.push_str("Done so far (the previous agent's last messages, newest last):\n");
        for m in messages.iter().rev().take(8).collect::<Vec<_>>().into_iter().rev() {
            out.push_str(&format!("- {}\n", clip(&m.replace('\n', " "), 600)));
        }
    }
    let names: Vec<&str> = files["names"].as_array().map(|a| a.iter().filter_map(|n| n.as_str()).collect()).unwrap_or_default();
    if names.is_empty() {
        out.push_str("No file has been changed since the task started.\n");
    } else {
        let more = files["files"].as_u64().unwrap_or(0).saturating_sub(names.len() as u64);
        out.push_str(&format!("Files changed since the task started (+{} −{}): {}{}\n", files["added"], files["removed"], names.join(", "), if more > 0 { format!(" and {more} more") } else { String::new() }));
    }
    if let Some(p) = pending.map(str::trim).filter(|p| !p.is_empty() && *p != task_prompt.trim()) {
        out.push_str(&format!("The user's last message, not yet answered: {}\n", clip(p, 4000)));
    }
    out.push_str("Continue from here. Check the files before editing; do not redo finished work.");
    out
}

fn announce(d: &Daemon, run: &Run, text: &str) {
    let _ = d.emit(Some(&run.task_id), Some(&run.id), "output", "daemon", "exact", json!({"role": "system", "text": text, "continuity": true}));
}

/// Moves the work of `predecessor` to a successor run on `target`. The predecessor must have no
/// process running. On success the predecessor is `handed_off`; on failure it stays as it was.
pub fn handoff(d: &Arc<Daemon>, predecessor: &Run, target: &Target, reason: &str) -> Result<Run> {
    if ["starting", "running", "waiting_for_user"].contains(&predecessor.status.as_str()) {
        bail!("the agent is still working; stop it before moving its work");
    }
    let task = d.task(&predecessor.task_id)?;
    let ws = d.workspace(&predecessor.workspace_id)?;
    let turns = d.store.lock().unwrap().turns(&predecessor.id)?;
    let pending = turns.iter().rev().find(|t| t.status == "waiting" || t.status == "failed").map(|t| t.prompt.clone());
    // The previous agent's messages, through every predecessor of its own.
    let mut chain = vec![predecessor.id.clone()];
    while let Some(p) = predecessor_of(d, chain.last().unwrap()) {
        chain.push(p);
    }
    let mut messages: Vec<(i64, String)> = Vec::new();
    for id in &chain {
        for e in d.store.lock().unwrap().events_after(0, Some(id), 5000)? {
            if e.kind == "output" && e.payload["role"] == "assistant" {
                messages.push((e.seq, e.payload["text"].as_str().unwrap_or_default().to_string()));
            }
        }
    }
    messages.sort_by_key(|(seq, _)| *seq);
    let messages: Vec<String> = messages.into_iter().map(|(_, m)| m).collect();
    let files = d.workspace_changes(&ws.id).unwrap_or(json!({"files": 0, "names": []}));
    let why = match reason {
        "back_online" => "the connection is back and the work returns to its first agent".to_string(),
        "user" => "the user moved the work here".to_string(),
        _ => "its model became unreachable".to_string(),
    };
    let repo = Path::new(&task.repo_root).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let prompt = handoff_prompt(&task.prompt, &repo, &ws.path, ws.branch.as_deref(), &messages, &files, pending.as_deref(), &why);
    let successor = Run {
        id: format!("r-{}", &uuid::Uuid::new_v4().simple().to_string()[..12]),
        task_id: predecessor.task_id.clone(),
        parent_run_id: None,
        harness: target.harness.clone(),
        harness_version: crate::adapters::resolve_program(&target.harness).and_then(|p| crate::adapters::version_of(&p)),
        profile_id: target.profile_id.clone(),
        model: target.model.clone(),
        workspace_id: predecessor.workspace_id.clone(),
        native_id: target.resume.clone(),
        status: "queued".into(),
        exit_reason: None,
        created_ms: now(),
        ended_ms: None,
        title: predecessor.title.clone(),
        relation_source: Some(format!("handoff from {} ({reason})", predecessor.id)),
        relation_confidence: Some("exact (made by Overseer)".into()),
        capabilities: crate::adapters::capabilities(&target.harness),
        process_generation: 0,
        attention: None,
    };
    d.store.lock().unwrap().insert_run(&successor)?;
    // The one-shot OpenCode transport takes no mode of its own.
    let opts = TurnOpts { mode: target.mode.clone().filter(|_| target.harness != "opencode"), handoff: true, ..Default::default() };
    if let Err(e) = d.start_turn(&successor.id, &prompt, target.resume.is_some(), &opts) {
        let current = d.run(&successor.id)?;
        if ACTIVE.contains(&current.status.as_str()) {
            d.mark_ended(&current, "failed", &format!("not launched: {e}"))?;
        }
        // The worktree goes back to the run that holds the work.
        d.store.lock().unwrap().set_workspace_owner(&ws.id, Some(&predecessor.id))?;
        return Err(e);
    }
    let successor = d.run(&successor.id)?;
    let at = now();
    let lost = wait_of(d, &predecessor.id).map(|w| w.started_ms).unwrap_or(at);
    {
        let store = d.store.lock().unwrap();
        store.update_run_status(&predecessor.id, HANDED_OFF, Some(&format!("handed off to {} ({reason})", successor.id)), Some(at))?;
        store.set_run_attention(&predecessor.id, None)?;
        store.finish_open_turns(&predecessor.id, HANDED_OFF, at)?;
        store.conn.execute("UPDATE turns SET status=?2, ended_ms=COALESCE(ended_ms, ?3) WHERE run_id=?1 AND status='waiting'", rusqlite::params![predecessor.id, HANDED_OFF, at])?;
        store.conn.execute("DELETE FROM continuity_waits WHERE run_id=?1", [&predecessor.id])?;
        store.conn.execute("INSERT INTO continuity_handoffs(predecessor, successor, reason, at_ms) VALUES(?1,?2,?3,?4)", rusqlite::params![predecessor.id, successor.id, reason, at])?;
    }
    let status = continuity::status();
    let said = match (target.to.as_str(), reason) {
        (_, "back_online") => format!("Back online. Continuing with **{}**.", target.label),
        ("local", "user") => format!("Moving to **{}** (local, Ollama) as you asked. Work continues in the same worktree.", target.label),
        ("local", _) if status.as_ref().is_some_and(|s| s.state == Conn::Offline) => format!("Transitioning to **{}** (local, Ollama) because you've disconnected. Work continues in the same worktree.", target.label),
        ("local", _) => format!("Transitioning to **{}** (local, Ollama) because no provider can be reached. Work continues in the same worktree.", target.label),
        _ => format!("{} is unreachable; continuing with **{}**{} because it is the best working option.", provider_name(provider_of(&predecessor.harness)), target.label, target.account.as_ref().map(|a| format!(" (account \"{a}\")")).unwrap_or_default()),
    };
    let opened = match reason {
        "back_online" => format!("Continued from \"{}\" now that the connection is back.", clip(&predecessor.title, 60)),
        "user" => format!("Continued from \"{}\".", clip(&predecessor.title, 60)),
        _ => format!("Continued from \"{}\" after the connection was lost at {}.", clip(&predecessor.title, 60), clock(lost)),
    };
    let detail = json!({"predecessor": predecessor.id, "successor": successor.id, "reason": reason, "target": target, "at_ms": at, "pending_message": pending.is_some(), "connection": status.as_ref().map(|s| json!({"state": s.state, "reason": s.reason}))});
    d.emit(Some(&predecessor.task_id), Some(&predecessor.id), "handoff", "daemon", "exact", detail.clone())?;
    d.emit(Some(&predecessor.task_id), Some(&predecessor.id), "status", "daemon", "exact", json!({"status": HANDED_OFF, "reason": format!("handed off to {} ({reason})", successor.id), "successor": successor.id}))?;
    announce(d, predecessor, &said);
    d.emit(Some(&successor.task_id), Some(&successor.id), "handoff", "daemon", "exact", detail)?;
    announce(d, &successor, &opened);
    Ok(successor)
}

pub fn predecessor_of(d: &Daemon, run: &str) -> Option<String> {
    d.store.lock().unwrap().conn.query_row("SELECT predecessor FROM continuity_handoffs WHERE successor=?1 ORDER BY id DESC LIMIT 1", [run], |r| r.get(0)).ok()
}

pub fn successor_of(d: &Daemon, run: &str) -> Option<String> {
    d.store.lock().unwrap().conn.query_row("SELECT successor FROM continuity_handoffs WHERE predecessor=?1 ORDER BY id DESC LIMIT 1", [run], |r| r.get(0)).ok()
}

pub fn handoffs(d: &Daemon) -> Value {
    let store = d.store.lock().unwrap();
    let Ok(mut stmt) = store.conn.prepare("SELECT predecessor, successor, reason, at_ms, stay, offered_ms FROM continuity_handoffs ORDER BY id") else { return json!([]) };
    let rows = stmt.query_map([], |r| Ok(json!({"predecessor": r.get::<_, String>(0)?, "successor": r.get::<_, String>(1)?, "reason": r.get::<_, String>(2)?, "at_ms": r.get::<_, i64>(3)?, "stay": r.get::<_, i64>(4)? != 0, "offered_ms": r.get::<_, Option<i64>>(5)?})));
    json!(rows.map(|r| r.flatten().collect::<Vec<_>>()).unwrap_or_default())
}

// ------------------------------------------------------------------ the scheduler

fn reachable(status: &Status, provider: &str) -> bool {
    status.state != Conn::Offline && status.system.state != crate::net::SystemNet::NoNetwork && status.providers.get(provider).is_none_or(|h| h.reachable != Some(false))
}

fn retry(d: &Arc<Daemon>, run: &Run, wait: &Wait) -> Result<()> {
    let turn: Turn = d.store.lock().unwrap().turns(&run.id)?.into_iter().find(|t| t.id == wait.turn_id).ok_or_else(|| anyhow!("the waiting turn is gone"))?;
    d.emit(Some(&run.task_id), Some(&run.id), "retry", "daemon", "exact", json!({"attempt": wait.attempts + 1, "sending": true, "after_ms": now() - wait.started_ms, "turn": turn.id}))?;
    let opts = TurnOpts { retry_of: Some(turn.id.clone()), ..Default::default() };
    // The harness's own resume continues the session when one was reported; otherwise the turn starts it.
    d.start_turn(&run.id, &turn.prompt, run.native_id.is_some(), &opts)?;
    Ok(())
}

fn wait_more(d: &Daemon, run: &Run, wait: &Wait, note: Option<String>, offers: Vec<Value>) -> Result<()> {
    let settings = continuity::settings();
    let attempts = wait.attempts + 1;
    let delay = backoff(attempts, settings.retry_cap_seconds, jitter(&run.id, attempts));
    d.store.lock().unwrap().conn.execute("UPDATE continuity_waits SET attempts=?2, next_ms=?3, note=?4, scheduled_ms=?5 WHERE run_id=?1", rusqlite::params![run.id, attempts, now() + delay, note, now()])?;
    d.emit(Some(&run.task_id), Some(&run.id), "retry", "daemon", "exact", json!({"attempt": attempts, "sending": false, "next_in_ms": delay, "waiting_ms": now() - wait.started_ms, "gives_up_after_hours": settings.retry_for_hours, "reason": wait.reason, "note": note, "offers": offers, "continuity": settings.enabled}))?;
    Ok(())
}

fn give_up(d: &Daemon, run: &Run, wait: &Wait) -> Result<()> {
    let hours = continuity::settings().retry_for_hours;
    let reason = if wait.kind == "memory" { format!("memory pressure stayed critical for {hours} hours") } else { format!("no connection for {hours} hours") };
    d.store.lock().unwrap().conn.execute("UPDATE continuity_waits SET note='expired', next_ms=?2 WHERE run_id=?1", rusqlite::params![run.id, i64::MAX])?;
    d.mark_ended(run, "failed", &reason)?;
    let attention = json!({"kind": wait.kind, "reason": reason, "message_kept": true, "turn": wait.turn_id, "actions": ["retry_now", "use_local"]});
    d.store.lock().unwrap().set_run_attention(&run.id, Some(&attention))?;
    d.emit(Some(&run.task_id), Some(&run.id), "attention", "daemon", "exact", attention)?;
    Ok(())
}

/// One pass at a time: the scheduler and the user's own requests never act on the same run together.
static PASS: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// One pass over everything that waits. Called after every check of the connection.
pub fn tick(d: &Arc<Daemon>) -> Result<()> {
    let _one = PASS.lock().unwrap_or_else(|e| e.into_inner());
    pass(d)
}

fn pass(d: &Arc<Daemon>) -> Result<()> {
    let Some(status) = continuity::status() else { return Ok(()) };
    let settings = continuity::settings();
    valve(d)?;
    stalls(d, &status, &settings)?;
    for wait in waits(d) {
        let Ok(run) = d.run(&wait.run_id) else { continue };
        if run.status != WAITING_FOR_CONNECTION && run.status != WAITING_FOR_MEMORY {
            if wait.note.as_deref() != Some("expired") {
                forget(d, &run.id);
            }
            continue;
        }
        if now() - wait.started_ms > settings.retry_for_hours as i64 * 3_600_000 {
            give_up(d, &run, &wait)?;
            continue;
        }
        // A run is looked at when it is due, and at once when the connection has changed since
        // its next look was set. A provider that answers the probe while the agent still fails
        // is tried again with the backoff, not at every pass.
        let online_again = wait.kind == "connection" && reachable(&status, &wait.provider);
        let changed = wait.kind == "connection" && status.changed_ms > wait.scheduled_ms;
        if now() < wait.next_ms && !changed {
            continue;
        }
        if wait.kind == "memory" {
            if sys::memory().is_ok_and(|m| m.pressure != Pressure::Critical) {
                if let Err(e) = retry(d, &run, &wait) {
                    wait_more(d, &run, &wait, Some(format!("not started again: {e}")), Vec::new())?;
                }
            } else {
                wait_more(d, &run, &wait, Some("memory pressure is still critical".into()), Vec::new())?;
            }
            continue;
        }
        if online_again {
            if let Err(e) = retry(d, &run, &wait) {
                wait_more(d, &run, &wait, Some(format!("not sent: {e}")), Vec::new())?;
            }
            continue;
        }
        // The provider cannot be reached. Where else can the work go?
        let degraded = status.state == Conn::Degraded && !status.acts_offline;
        let elsewhere = if degraded { failover_target(d, &run, &status) } else { None };
        let local = if elsewhere.is_none() && (status.state == Conn::Offline || status.acts_offline) { Some(local_target(d, &run)) } else { None };
        let mut offers = Vec::new();
        let mut note = None;
        let target = match (&elsewhere, &local) {
            (Some(t), _) => Some(t.clone()),
            (None, Some(Ok(t))) => Some(t.clone()),
            (None, Some(Err(e))) => {
                note = Some(e.to_string());
                None
            }
            _ => None,
        };
        if let Some(t) = target {
            let reason = if t.to == "local" { "offline".to_string() } else { format!("provider_unreachable:{}", wait.provider) };
            if settings.enabled && t.difference.is_none() {
                match handoff(d, &run, &t, &reason) {
                    Ok(_) => continue,
                    Err(e) => note = Some(format!("the work could not move to {}: {e}", t.label)),
                }
            } else {
                // Continuity is off, or the target cannot keep the run's permission mode: offered, not done.
                offers.push(json!({"to": t.to, "label": t.label, "account": t.account, "mode": t.mode, "difference": t.difference}));
            }
        }
        wait_more(d, &run, &wait, note, offers)?;
    }
    back_online(d, &status, &settings)?;
    Ok(())
}

/// A turn that produces nothing while the connection is gone is interrupted by Overseer, and
/// then treated as a turn that failed on the connection.
fn stalls(d: &Arc<Daemon>, status: &Status, settings: &continuity::Settings) -> Result<()> {
    if status.state != Conn::Offline {
        return Ok(());
    }
    let limit = ms("OVERSEER_TEST_STALL_MS", settings.stall_seconds as i64 * 1000);
    let runs = d.store.lock().unwrap().runs()?;
    for run in runs.iter().filter(|r| r.parent_run_id.is_none() && ["starting", "running"].contains(&r.status.as_str()) && KNOWN_PROVIDERS.contains(&provider_of(&r.harness))) {
        let last: i64 = d.store.lock().unwrap().conn.query_row("SELECT COALESCE(MAX(ts), 0) FROM events WHERE run_id=?1", [&run.id], |r| r.get(0)).unwrap_or(0);
        if now() - last.max(run.created_ms) < limit || now() - status.since_ms < limit {
            continue;
        }
        let Some((dir, _, _)) = d.store.lock().unwrap().run_process(&run.id)? else { continue };
        let marker = Path::new(&dir).join("stall.requested");
        if marker.exists() {
            continue;
        }
        std::fs::write(&marker, now().to_string())?;
        d.emit(Some(&run.task_id), Some(&run.id), "stall", "daemon", "exact", json!({"silent_ms": now() - last, "limit_ms": limit, "action": "interrupted by Overseer", "connection": status.reason}))?;
        if let Ok(sock) = d.control_socket(run) {
            let _ = crate::shim::control(&sock, &json!({"op": "close_stdin"}));
            let _ = crate::shim::control(&sock, &json!({"op": "signal", "sig": libc::SIGINT}));
        }
    }
    Ok(())
}

/// At critical memory pressure local runs are paused with their message kept, and the model is
/// unloaded. It is the only case in which Overseer stops a working local turn.
fn valve(d: &Arc<Daemon>) -> Result<()> {
    if !sys::memory().is_ok_and(|m| m.pressure == Pressure::Critical) {
        return Ok(());
    }
    let runs = d.store.lock().unwrap().runs()?;
    for run in runs.iter().filter(|r| r.parent_run_id.is_none() && continuity::is_local(r) && ["starting", "running", "waiting_for_user"].contains(&r.status.as_str())) {
        let Some((dir, _, _)) = d.store.lock().unwrap().run_process(&run.id)? else { continue };
        let marker = Path::new(&dir).join("memory.requested");
        if marker.exists() {
            continue;
        }
        std::fs::write(&marker, now().to_string())?;
        d.emit(Some(&run.task_id), Some(&run.id), "memory_valve", "daemon", "exact", json!({"pressure": "critical", "action": "paused by Overseer; the message is kept", "model": run.model}))?;
        if let Ok(sock) = d.control_socket(run) {
            let _ = crate::shim::control(&sock, &json!({"op": "stdin", "data": opencode_bridge::abort_line()}));
            let _ = crate::shim::control(&sock, &json!({"op": "signal", "sig": libc::SIGINT}));
        }
        if let Some(tag) = run.model.as_deref().map(opencode_bridge::tag_of) {
            let unloaded = crate::local::unload(tag);
            crate::local::note_unloaded(&d.store.lock().unwrap().conn, tag);
            d.emit(Some(&run.task_id), Some(&run.id), "local_load", "daemon", "exact", json!({"tag": tag, "unloaded": unloaded.is_ok(), "why": "critical memory pressure", "error": unloaded.err().map(|e| e.to_string())}))?;
        }
    }
    Ok(())
}

/// The connection is back: waiting runs are due at once, and a run that went local or moved to
/// another provider is offered the way back (or takes it at its next turn, or stays).
fn back_online(d: &Arc<Daemon>, status: &Status, settings: &continuity::Settings) -> Result<()> {
    if status.state != Conn::Online {
        return Ok(());
    }
    let rows: Vec<(i64, String, String, String)> = {
        let store = d.store.lock().unwrap();
        let mut stmt = store.conn.prepare("SELECT id, predecessor, successor, reason FROM continuity_handoffs WHERE stay=0 AND offered_ms IS NULL AND reason <> 'back_online' AND reason <> 'user'")?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?.flatten().collect();
        rows
    };
    for (id, predecessor, successor, _) in rows {
        if successor_of(d, &successor).is_some() {
            continue; // the work has already moved on from there
        }
        let (Ok(run), Ok(first)) = (d.run(&successor), d.run(&predecessor)) else { continue };
        if !reachable(status, provider_of(&first.harness)) {
            continue;
        }
        d.store.lock().unwrap().conn.execute("UPDATE continuity_handoffs SET offered_ms=?2 WHERE id=?1", rusqlite::params![id, now()])?;
        let local = provider_of(&run.harness) == "local";
        let text = if local { "Back online. This agent is still on a local model.".to_string() } else { format!("Back online. This agent is still on {}.", harness_name(&run.harness)) };
        d.emit(Some(&run.task_id), Some(&run.id), "back_online", "daemon", "exact", json!({"return_online": settings.return_online, "back_to": {"harness": first.harness, "label": harness_name(&first.harness), "run": first.id}, "offer": settings.return_online == "offer", "at_next_turn": settings.return_online == "auto"}))?;
        if settings.return_online != "stay" {
            announce(d, &run, &text);
        }
    }
    Ok(())
}

/// What a new agent can be started on now, and what the composer offers first: the online
/// harness and account the user last chose while they can be reached, a local model otherwise.
pub fn new_agents(d: &Daemon, status: &Status) -> Value {
    let runs = d.store.lock().unwrap().runs().unwrap_or_default();
    let mut harnesses = Vec::new();
    let mut usable: Vec<&str> = Vec::new();
    for harness in ["codex", "codex-app", "claude"] {
        let provider = provider_of(harness);
        let installed = crate::adapters::resolve_program(harness).is_some();
        let why = if !installed {
            Some(format!("{} is not installed", harness_name(harness)))
        } else if status.state == Conn::Offline {
            Some(format!("offline: {}", status.reason))
        } else if !reachable(status, provider) {
            Some(format!("{} cannot be reached", provider_name(provider)))
        } else {
            None
        };
        if why.is_none() {
            usable.push(harness);
        }
        harnesses.push(json!({"harness": harness, "provider": provider, "usable": why.is_none(), "why": why}));
    }
    let local_installed = crate::adapters::resolve_program(opencode_bridge::HARNESS).is_some();
    harnesses.push(json!({"harness": opencode_bridge::HARNESS, "provider": "local", "usable": local_installed, "why": if local_installed { Value::Null } else { json!("OpenCode is not installed") }}));
    // The user's own last choice: a run they started, not one Overseer moved the work to.
    let own = |r: &&Run| r.parent_run_id.is_none() && !r.relation_source.as_deref().is_some_and(|s| s.starts_with("handoff from "));
    let last = runs.iter().rev().filter(own).find(|r| usable.contains(&r.harness.as_str()));
    let order = provider_order(&continuity::settings().provider_order, "");
    let first = order.iter().filter_map(|p| match p.as_str() {
        "openai" => Some("codex"),
        "anthropic" => Some("claude"),
        _ => None,
    }).find(|h| usable.contains(h));
    let default = match (last, first) {
        (Some(r), _) => json!({"harness": r.harness, "profile_id": r.profile_id, "local": false, "why": "the harness and account you last chose"}),
        (None, Some(h)) => json!({"harness": h, "profile_id": null, "local": false, "why": "the first provider in your order that can be reached"}),
        _ => json!({"harness": opencode_bridge::HARNESS, "profile_id": opencode_bridge::LOCAL_PROFILE, "local": true, "why": if status.state == Conn::Offline { format!("offline: {}", status.reason) } else { "no online provider can be reached".to_string() }}),
    };
    json!({"default": default, "harnesses": harnesses, "local_only": usable.is_empty()})
}

/// The target that takes the work back to where it came from.
fn back_target(d: &Daemon, run: &Run) -> Result<Target> {
    let first_id = predecessor_of(d, &run.id).ok_or_else(|| anyhow!("this agent did not come from another one"))?;
    let first = d.run(&first_id)?;
    // The first agent goes on in the mode the user gave it, unless the mode was changed since:
    // then the mode of now is carried, and what would be looser is said.
    let own = mode_of(d, &first);
    let given = carry_mode(&first.harness, own.as_deref(), &run.harness).0;
    let (mode, difference) = if mode_of(d, run) == given { (own, None) } else { carry_mode(&run.harness, mode_of(d, run).as_deref(), &first.harness) };
    Ok(Target { to: provider_of(&first.harness).into(), harness: first.harness.clone(), profile_id: first.profile_id.clone(), account: None, model: first.model.clone(), mode, resume: first.native_id.clone(), difference, label: harness_name(&first.harness).into() })
}

/// With `returnOnline: auto`, a message sent to a run that went local goes back to the first
/// agent once the connection has returned. Returns the turn it started there.
pub fn before_follow_up(d: &Arc<Daemon>, run: &Run, prompt: &str) -> Result<Option<Turn>> {
    if continuity::settings().return_online != "auto" || ACTIVE.contains(&run.status.as_str()) {
        return Ok(None);
    }
    let due: Option<i64> = d.store.lock().unwrap().conn.query_row("SELECT id FROM continuity_handoffs WHERE successor=?1 AND stay=0 AND offered_ms IS NOT NULL AND reason <> 'back_online' AND reason <> 'user'", [&run.id], |r| r.get(0)).ok();
    if due.is_none() || successor_of(d, &run.id).is_some() || !continuity::status().is_some_and(|s| s.state == Conn::Online) {
        return Ok(None);
    }
    let _one = PASS.lock().unwrap_or_else(|e| e.into_inner());
    let target = back_target(d, run)?;
    // The message travels as the pending one of the run that is left.
    let n = d.store.lock().unwrap().turns(&run.id)?.len() as i64 + 1;
    let kept = Turn { id: format!("u-{}", &uuid::Uuid::new_v4().simple().to_string()[..12]), run_id: run.id.clone(), n, prompt: prompt.into(), snapshot_id: None, started_ms: now(), ended_ms: None, status: "waiting".into() };
    d.store.lock().unwrap().insert_turn(&kept)?;
    let successor = handoff(d, run, &target, "back_online")?;
    Ok(d.store.lock().unwrap().turns(&successor.id)?.pop())
}

// ------------------------------------------------------------------ what the user asks for

fn waiting(d: &Daemon, run_id: &str) -> Result<(Run, Option<Wait>)> {
    let run = d.run(run_id)?;
    Ok((run, wait_of(d, run_id)))
}

/// Stop on a run that waits: the wait ends, and nothing is relaunched.
pub fn stop_waiting(d: &Daemon, run: &Run) -> Result<Value> {
    forget(d, &run.id);
    d.emit(Some(&run.task_id), Some(&run.id), "interrupt_requested", "user", "exact", json!({"while": run.status}))?;
    d.mark_ended(run, "interrupted", "stopped by the user while waiting")?;
    Ok(json!({"ok": true}))
}

pub fn handles(method: &str) -> bool {
    matches!(method, "run.handoff" | "run.targets" | "run.retry_now" | "run.stay" | "continuity.handoffs" | "continuity.waits" | "continuity.test_age")
}

pub fn dispatch(d: &Arc<Daemon>, method: &str, p: &Value) -> Result<Value> {
    let id = || p["run_id"].as_str().ok_or_else(|| anyhow!("missing string parameter run_id"));
    let _one = PASS.lock().unwrap_or_else(|e| e.into_inner());
    Ok(match method {
        "continuity.handoffs" => json!({"handoffs": handoffs(d)}),
        "continuity.waits" => json!({"waits": waits(d)}),
        "run.retry_now" => {
            let (run, wait) = waiting(d, id()?)?;
            let wait = wait.ok_or_else(|| anyhow!("this agent is not waiting for a connection"))?;
            if run.status == "failed" {
                // Given up after the limit: the wait starts again, and the message is still there.
                d.store.lock().unwrap().conn.execute("UPDATE continuity_waits SET note=NULL, attempts=0, started_ms=?2, next_ms=?2 WHERE run_id=?1", rusqlite::params![run.id, now()])?;
                let store = d.store.lock().unwrap();
                store.update_run_status(&run.id, WAITING_FOR_CONNECTION, Some(&wait.reason), None)?;
                store.conn.execute("UPDATE runs SET ended_ms=NULL, attention=NULL WHERE id=?1", [&run.id])?;
                store.conn.execute("UPDATE turns SET status='waiting', ended_ms=NULL WHERE id=?1", [&wait.turn_id])?;
            } else {
                d.store.lock().unwrap().conn.execute("UPDATE continuity_waits SET next_ms=?2 WHERE run_id=?1", rusqlite::params![run.id, now()])?;
            }
            d.emit(Some(&run.task_id), Some(&run.id), "retry", "user", "exact", json!({"retry_now": true}))?;
            pass(d)?;
            json!({"run": d.run(&run.id)?})
        }
        // Where this agent's work could go now, best first; nothing is moved.
        "run.targets" => {
            let run = d.run(id()?)?;
            let status = continuity::status().ok_or_else(|| anyhow!("Continuity has not started"))?;
            let local = match local_target(d, &run) {
                Ok(t) => json!({"target": t}),
                Err(e) => json!({"target": null, "why": e.to_string()}),
            };
            let back = back_target(d, &run).ok().filter(|_| status.state == Conn::Online);
            json!({"online": failover_targets(d, &run, &status), "local": local, "back": back, "connection": {"state": status.state, "reason": status.reason}})
        }
        "run.handoff" => {
            let (run, wait) = waiting(d, id()?)?;
            let to = p["to"].as_str().ok_or_else(|| anyhow!("missing string parameter to (local, back or a provider)"))?;
            let status = continuity::status().ok_or_else(|| anyhow!("Continuity has not started"))?;
            let mut target = match to {
                "local" => local_target(d, &run)?,
                "back" => back_target(d, &run)?,
                _ => failover_targets(d, &run, &status).into_iter().find(|t| t.to == to).ok_or_else(|| anyhow!("{} cannot take the work now", provider_name(to)))?,
            };
            if let Some(diff) = &target.difference {
                // The user is told the difference and accepts it by naming the mode.
                if p["accept_mode"].as_str() != target.mode.as_deref() {
                    bail!("{diff}; to continue, accept the mode {}", target.mode.as_deref().unwrap_or("of that agent"));
                }
                target.difference = None;
            }
            if run.status == "failed" && wait.as_ref().is_some_and(|w| w.note.as_deref() == Some("expired")) {
                d.store.lock().unwrap().conn.execute("UPDATE turns SET status='waiting' WHERE id=?1", [&wait.as_ref().unwrap().turn_id])?;
            }
            let successor = handoff(d, &run, &target, if to == "back" { "back_online" } else { "user" })?;
            json!({"successor": successor, "predecessor": d.run(&run.id)?})
        }
        "run.stay" => {
            let run = d.run(id()?)?;
            d.store.lock().unwrap().conn.execute("UPDATE continuity_handoffs SET stay=1 WHERE successor=?1", [&run.id])?;
            d.emit(Some(&run.task_id), Some(&run.id), "back_online", "user", "exact", json!({"stay": true}))?;
            json!({"ok": true})
        }
        // Tests only (a fixture network must be in use): ages a wait, as if time had passed.
        "continuity.test_age" => {
            if std::env::var_os("OVERSEER_TEST_NET").is_none() {
                bail!("only with a fixture network");
            }
            let by = p["by_ms"].as_i64().unwrap_or(0);
            d.store.lock().unwrap().conn.execute("UPDATE continuity_waits SET started_ms=started_ms-?2, next_ms=?3 WHERE run_id=?1", rusqlite::params![id()?, by, now()])?;
            pass(d)?;
            json!({"run": d.run(id()?)?})
        }
        other => bail!("unknown method {other}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_backoff_doubles_to_the_cap() {
        let plain: Vec<i64> = (0..8).map(|a| backoff(a, 120, 0.0)).collect();
        assert_eq!(plain, [5000, 10_000, 20_000, 40_000, 80_000, 120_000, 120_000, 120_000]);
        assert_eq!(backoff(3, 120, 1.0), 48_000, "a fifth more");
        assert_eq!(backoff(3, 120, -1.0), 32_000, "a fifth less");
        assert_eq!(backoff(60, 30, 0.0), 30_000, "many attempts do not overflow");
        let j: Vec<f64> = (0..50).map(|a| jitter("r-1", a)).collect();
        assert!(j.iter().all(|x| (-1.0..=1.0).contains(x)) && j.iter().any(|x| *x > 0.3) && j.iter().any(|x| *x < -0.3));
    }

    #[test]
    fn modes_are_carried_and_never_loosened() {
        let c = |from: &str, mode: Option<&str>, to: &str| carry_mode(from, mode, to);
        // To a local model every mode is kept.
        assert_eq!(c("claude", Some("manual"), "opencode-serve"), (Some("manual".into()), None));
        assert_eq!(c("claude", Some("plan"), "opencode-serve"), (Some("plan".into()), None));
        assert_eq!(c("claude", None, "opencode-serve"), (Some("manual".into()), None), "Claude Code asks unless told otherwise");
        assert_eq!(c("codex", Some("read-only"), "opencode-serve"), (Some("plan".into()), None));
        assert_eq!(c("codex", None, "opencode-serve"), (Some("acceptEdits".into()), None));
        // From Codex to Claude Code the mode gets stricter, which is allowed.
        assert_eq!(c("codex", Some("workspace-write"), "claude"), (Some("acceptEdits".into()), None));
        assert_eq!(c("codex-app", Some("read-only"), "claude"), (Some("plan".into()), None));
        // From Claude Code to Codex only Plan only and Auto can be kept; the others are offered.
        assert_eq!(c("claude", Some("plan"), "codex"), (Some("read-only".into()), None));
        assert_eq!(c("claude", Some("auto"), "codex"), (Some("workspace-write".into()), None));
        let (mode, diff) = c("claude", Some("manual"), "codex");
        assert_eq!(mode.as_deref(), Some("workspace-write"));
        assert!(diff.unwrap().contains("without asking first"));
        assert!(c("claude", Some("acceptEdits"), "codex").1.unwrap().contains("runs commands in its sandbox without asking"));
        assert!(c("claude", None, "codex").1.is_some());
        // The same harness keeps its mode as it is.
        assert_eq!(c("claude", Some("manual"), "claude"), (Some("manual".into()), None));
        assert_eq!(c("opencode-serve", Some("acceptEdits"), "claude"), (Some("acceptEdits".into()), None));
    }

    #[test]
    fn the_handoff_prompt_is_built_from_the_record_and_bounded() {
        let files = json!({"files": 3, "added": 12, "removed": 4, "names": ["src/a.rs", "README.md"]});
        let messages: Vec<String> = (1..=12).map(|i| format!("message {i}\nwith a second line")).collect();
        let p = handoff_prompt("Fix the login bug", "demo", "/work/demo", Some("overseer/fix-login"), &messages, &files, Some("Also add a test"), "its model became unreachable");
        assert!(p.starts_with("You are continuing a task another agent started; its model became unreachable.\nTask: Fix the login bug\n"));
        assert!(p.contains("Repository demo, working tree /work/demo, branch overseer/fix-login. Do not change branches."));
        assert!(!p.contains("message 4 ") && p.contains("- message 5 with a second line\n") && p.contains("- message 12 with a second line\n"), "the last eight, newest last");
        assert!(p.contains("Files changed since the task started (+12 −4): src/a.rs, README.md and 1 more\n"));
        assert!(p.contains("The user's last message, not yet answered: Also add a test\n"));
        assert!(p.ends_with("Continue from here. Check the files before editing; do not redo finished work."));
        // The first turn's message is the task itself: it is not repeated.
        let first = handoff_prompt("Fix the login bug", "demo", "/w", None, &[], &json!({"files": 0, "names": []}), Some("Fix the login bug"), "x");
        assert!(!first.contains("not yet answered") && first.contains("had not reported anything yet") && first.contains("No file has been changed"));
        // Long records are cut, so the prompt fits a 16k context with room for the work.
        let long = handoff_prompt(&"t".repeat(50_000), "demo", "/w", None, &vec!["m".repeat(5000); 20], &files, Some(&"p".repeat(50_000)), "x");
        assert!(long.chars().count() < 14_000, "{}", long.chars().count());
    }

    #[test]
    fn the_owners_order_of_providers_is_kept() {
        let o = |configured: &[&str], failing: &str| provider_order(&configured.iter().map(|s| s.to_string()).collect::<Vec<_>>(), failing);
        assert_eq!(o(&["openai", "anthropic"], "local"), ["openai", "anthropic"], "both alternatives, OpenAI first");
        assert_eq!(o(&["anthropic", "openai"], "local"), ["anthropic", "openai"], "the owner's order");
        assert_eq!(o(&["anthropic"], "none"), ["anthropic", "openai"], "a provider left out comes after the listed ones");
        assert_eq!(o(&["openai", "anthropic"], "openai"), ["anthropic"], "never the failing one");
        assert!(o(&["openai"], "openai").contains(&"anthropic".to_string()));
    }

    #[test]
    fn harnesses_are_named_for_people() {
        assert_eq!((harness_name("claude"), harness_name("codex-app"), harness_name("opencode-serve")), ("Claude Code", "Codex", "OpenCode"));
        assert_eq!(clock(0).len(), 5);
    }
}
