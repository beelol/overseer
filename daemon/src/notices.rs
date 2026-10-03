//! You hear about it outside VS Code (AC-240). When no VS Code window has the OS focus (or VS
//! Code is closed), an agent that needs the owner, finishes or fails posts one Mac notification:
//! titled with the agent, saying what it needs, grouped per agent (the notification's thread), and
//! a click opens that agent in VS Code (in the TUI when VS Code is closed). Which kinds is the owner's setting (`notices.set`, from
//! VS Code's `overseer.notifications.*`). Overseer's own runs (Overseer, its watchers) never notify.

use crate::daemon::Daemon;
use crate::store::{Event, Run};
use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Every kind, in the order the settings show them.
pub const KINDS: &[&str] = &["permission", "question", "failure", "finished"];
const SETTING: &str = "notices.kinds";

/// The moment an event is, as a notification: `permission`, `question`, `failure` or `finished`.
pub fn kind_of(event: &Event) -> Option<&'static str> {
    match event.kind.as_str() {
        // The daemon allowed its own tool (Overseer's reads): nothing waits for the owner.
        "permission" if event.payload.get("auto_allowed").is_some() => None,
        "permission" => Some(if event.payload["kind"] == "question" { "question" } else { "permission" }),
        "status" => match event.payload["status"].as_str() {
            Some("failed") => Some("failure"),
            Some("completed") => Some("finished"),
            _ => None,
        },
        _ => None,
    }
}

/// The kinds the owner wants (all of them until they choose).
pub fn kinds(d: &Daemon) -> Vec<String> {
    match crate::continuity::meta_get(d, SETTING) {
        Some(text) => serde_json::from_str::<Vec<String>>(&text).unwrap_or_else(|_| KINDS.iter().map(|k| k.to_string()).collect()),
        None => KINDS.iter().map(|k| k.to_string()).collect(),
    }
}

pub fn get(d: &Daemon) -> Result<Value> {
    Ok(json!({"kinds": kinds(d), "all": KINDS, "vscode_focused": vscode_focused(d)}))
}

/// `{"kinds": [...]}`: which moments notify. Unknown kinds are refused.
pub fn set(d: &Daemon, p: &Value) -> Result<Value> {
    let list = p["kinds"].as_array().ok_or_else(|| anyhow!("kinds must be a list"))?;
    let mut chosen = Vec::new();
    for k in list {
        let k = k.as_str().ok_or_else(|| anyhow!("each kind is a string"))?;
        if !KINDS.contains(&k) {
            return Err(anyhow!("unknown kind {k:?}: one of {}", KINDS.join(", ")));
        }
        if !chosen.iter().any(|c: &String| c == k) {
            chosen.push(k.to_string());
        }
    }
    crate::continuity::meta_set(d, SETTING, &serde_json::to_string(&chosen)?)?;
    get(d)
}

/// True while some VS Code window has the OS focus: the owner is in VS Code and sees it there.
pub fn vscode_focused(d: &Daemon) -> bool {
    d.windows.lock().unwrap().values().any(|focused| *focused)
}

/// Where a click on an agent's notification goes: that agent in VS Code (the extension's URI handler).
pub fn open_url(run_id: &str) -> String {
    format!("vscode://beelol.overseer/open-agent?run={run_id}")
}

/// Where the click goes when VS Code is closed: the TUI on that agent, as a shell command the
/// notifier runs in Terminal (`overseer-tui --focus RUN`, with `--home` for a daemon outside the
/// standard data folder). `None` when no `overseer-tui` is found: the click opens VS Code.
pub fn tui_command(run_id: &str) -> Option<String> {
    let bin = tui_binary()?;
    let q = |s: &str| format!("'{}'", s.replace('\'', "'\\''"));
    let mut cmd = format!("{} --focus {}", q(&bin.display().to_string()), q(run_id));
    let home = crate::paths::data_dir();
    if home != crate::paths::standard_data_dir() {
        cmd.push_str(&format!(" --home {}", q(&home.display().to_string())));
    }
    Some(cmd)
}

/// The TUI: `OVERSEER_TUI`, else beside this daemon (a workspace build, a dev instance's bin), else
/// on `PATH`, else `~/.cargo/bin` (`cargo install`).
fn tui_binary() -> Option<std::path::PathBuf> {
    use std::path::PathBuf;
    if let Some(p) = std::env::var_os("OVERSEER_TUI").filter(|p| !p.is_empty()) {
        return Some(PathBuf::from(p)).filter(|p| p.is_file());
    }
    let beside = std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join("overseer-tui")));
    let on_path = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).map(|d| d.join("overseer-tui")).collect::<Vec<_>>()).unwrap_or_default();
    let cargo = std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cargo/bin/overseer-tui"));
    beside.into_iter().chain(on_path).chain(cargo).find(|p| p.is_file())
}

fn sentence(kind: &str, tool: Option<&str>) -> String {
    match (kind, tool) {
        ("permission", Some(tool)) if !tool.is_empty() => format!("Needs your permission to use {tool}"),
        ("permission", _) => "Needs your permission".into(),
        ("question", _) => "Has a question for you".into(),
        ("failure", _) => "Stopped with an error".into(),
        _ => "Finished".into(),
    }
}

/// The notification's title (the agent) and body (what it needs, and where).
pub fn message(kind: &str, run: &Run, repo: &str, tool: Option<&str>) -> (String, String) {
    let repo_name = std::path::Path::new(repo).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let title = crate::redact::redact(run.title.trim()).chars().take(80).collect::<String>();
    let title = if title.is_empty() { format!("{} agent", run.harness) } else { title };
    let what = sentence(kind, tool.map(|t| t.chars().take(40).collect::<String>()).as_deref());
    let body = if repo_name.is_empty() { what } else { format!("{what} · {repo_name}") };
    (title, body)
}

/// Delivered only where a notification is meant to be seen: the owner's own (standard) daemon, or
/// a daemon given a notify command (a dev daemon's notifications.log, tests). A test daemon with its
/// own data folder never posts a real banner.
fn deliverable() -> bool {
    crate::background::notify_command().is_some() || crate::paths::data_dir() == crate::paths::standard_data_dir()
}

/// The same moment reported twice (a status repeated by two sources) notifies once.
fn fresh(seen: &Mutex<HashMap<String, Instant>>, key: String) -> bool {
    let mut seen = seen.lock().unwrap();
    seen.retain(|_, at| at.elapsed() < Duration::from_secs(5));
    seen.insert(key, Instant::now()).is_none()
}

fn is_overseers_own(d: &Daemon, run_id: &str) -> bool {
    use rusqlite::OptionalExtension;
    d.store.lock().unwrap().conn.query_row("SELECT role FROM run_roles WHERE run_id=?1", [run_id], |r| r.get::<_, String>(0)).optional().ok().flatten().is_some()
}

/// Why this moment is not a Mac notification, or `None` when it is one.
pub fn why_not(d: &Daemon, run: &Run, kind: &str) -> Option<&'static str> {
    if run.parent_run_id.is_some() {
        Some("a child's moments belong to its parent")
    } else if is_overseers_own(d, &run.id) {
        Some("Overseer's own run")
    } else if !kinds(d).iter().any(|k| k == kind) {
        Some("this kind is off in the settings")
    } else if vscode_focused(d) {
        Some("VS Code is focused")
    } else if !deliverable() {
        Some("a test data folder without a notify command")
    } else {
        None
    }
}

fn notice(d: &Daemon, event: &Event, kind: &str, seen: &Mutex<HashMap<String, Instant>>) {
    let Some(run_id) = event.run_id.as_deref() else { return };
    let Ok(run) = d.run(run_id) else { return };
    if let Some(why) = why_not(d, &run, kind) {
        if run.parent_run_id.is_none() && !is_overseers_own(d, &run.id) {
            crate::log(&format!("notice for {} ({kind}) not posted: {why}", run.id));
        }
        return;
    }
    let request = event.payload["request_id"].as_str().map(str::to_string).or_else(|| event.payload["request_id"].as_i64().map(|n| n.to_string())).unwrap_or_default();
    if !fresh(seen, format!("{}:{kind}:{request}", run.id)) {
        return;
    }
    let repo = d.store.lock().unwrap().task(&run.task_id).ok().flatten().map(|t| t.repo_root).unwrap_or_default();
    let (title, body) = message(kind, &run, &repo, event.payload["tool"].as_str());
    let via = crate::background::notify_agent(&title, &body, &open_url(&run.id), Some(&run.id), tui_command(&run.id).as_deref());
    crate::log(&format!("notice for {} ({kind}, {via}): {title} — {body}", run.id));
}

/// Follows the daemon's events and posts the notifications.
pub fn watch(d: Arc<Daemon>) {
    let mut events = d.events.subscribe();
    let seen = Arc::new(Mutex::new(HashMap::new()));
    tokio::spawn(async move {
        loop {
            match events.recv().await {
                Ok(event) => {
                    if let Some(kind) = kind_of(&event) {
                        let (d, seen) = (d.clone(), seen.clone());
                        let _ = tokio::task::spawn_blocking(move || notice(&d, &event, kind, &seen)).await;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => return,
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(title: &str) -> Run {
        Run {
            id: "r-1".into(), task_id: "t-1".into(), parent_run_id: None, harness: "claude".into(), harness_version: None, profile_id: None, model: None, effort: None,
            workspace_id: "w".into(), native_id: None, status: "waiting_for_user".into(), exit_reason: None, created_ms: 0, ended_ms: None, title: title.into(),
            relation_source: None, relation_confidence: None, capabilities: json!({}), process_generation: 0, attention: None,
        }
    }

    fn event(kind: &str, payload: Value) -> Event {
        Event { seq: 1, ts: 0, task_id: None, run_id: Some("r-1".into()), kind: kind.into(), source: "harness".into(), confidence: "exact".into(), payload }
    }

    #[test]
    fn the_moments_that_notify() {
        assert_eq!(kind_of(&event("permission", json!({"kind": "permission", "tool": "Write"}))), Some("permission"));
        assert_eq!(kind_of(&event("permission", json!({"kind": "question"}))), Some("question"));
        assert_eq!(kind_of(&event("permission", json!({"kind": "permission", "auto_allowed": "Overseer's own tool"}))), None);
        assert_eq!(kind_of(&event("status", json!({"status": "completed"}))), Some("finished"));
        assert_eq!(kind_of(&event("status", json!({"status": "failed"}))), Some("failure"));
        assert_eq!(kind_of(&event("status", json!({"status": "running"}))), None);
        assert_eq!(kind_of(&event("status", json!({"status": "interrupted"}))), None);
        assert_eq!(kind_of(&event("output", json!({"text": "hi"}))), None);
    }

    #[test]
    fn a_notification_names_the_agent_and_what_it_needs() {
        assert_eq!(message("permission", &run("Add the changelog"), "/src/site", Some("Write")), ("Add the changelog".to_string(), "Needs your permission to use Write · site".to_string()));
        assert_eq!(message("finished", &run("Add the changelog"), "/src/site", None).1, "Finished · site");
        assert_eq!(message("failure", &run("x"), "", None).1, "Stopped with an error");
        assert_eq!(message("question", &run(""), "/r", None).0, "claude agent");
    }

    #[test]
    fn a_click_opens_that_agent() {
        assert_eq!(open_url("r-abc"), "vscode://beelol.overseer/open-agent?run=r-abc");
    }

    #[test]
    fn with_vscode_closed_a_click_opens_the_tui_on_that_agent() {
        let dir = tempfile::tempdir().unwrap();
        let tui = dir.path().join("overseer tui");
        std::fs::write(&tui, "").unwrap();
        std::env::set_var("OVERSEER_TUI", &tui);
        let cmd = tui_command("r-1").unwrap();
        assert!(cmd.starts_with(&format!("'{}' --focus 'r-1'", tui.display())), "{cmd}");
        std::env::set_var("OVERSEER_TUI", dir.path().join("missing"));
        assert_eq!(tui_command("r-1"), None, "no TUI: the click opens VS Code");
        std::env::remove_var("OVERSEER_TUI");
    }

    #[test]
    fn the_same_moment_notifies_once() {
        let seen = Mutex::new(HashMap::new());
        assert!(fresh(&seen, "r:finished:".into()));
        assert!(!fresh(&seen, "r:finished:".into()));
        assert!(fresh(&seen, "r:permission:1".into()));
        assert!(fresh(&seen, "r:permission:2".into()));
    }
}
