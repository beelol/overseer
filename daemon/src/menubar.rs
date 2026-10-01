//! The Mac's menu-bar item (AC-262): `menubar.snapshot` is everything its menu shows, in one
//! light read, and `review.seen` is how VS Code tells the daemon which finished agents the owner
//! has looked at, so "to review" means the same thing in the menu as in the side bar.
//!
//! The item itself (`extension/menubar`) only displays this and sends the owner's answers back
//! (`run.permission`, `voice.set`); it never keeps state of its own.

use crate::daemon::{now, Daemon, ACTIVE};
use crate::store::{Profile, Run, Task};
use anyhow::Result;
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::sync::Arc;

/// Agents listed in one repository's submenu; the rest are behind "Show all in Overseer…".
pub const AGENTS_PER_REPO: usize = 8;
/// A finished agent stays "to review" for a week at most, as in the side bar (rollup.js WEEK).
const WEEK_MS: i64 = 7 * 24 * 3600 * 1000;
const REVIEWED_KEY: &str = "menubar.reviewed";
const DONE: &[&str] = &["completed", "interrupted"];
const FAILED: &[&str] = &["failed", "disconnected"];

/// One top-level agent as the snapshot reads it, before it is grouped.
pub struct Agent<'a> {
    pub run: &'a Run,
    pub task: Option<&'a Task>,
    /// When its latest turn started (the run's start when it has none).
    pub turn_started_ms: i64,
    /// When its pending permission was asked, for the Needs-you order.
    pub asked_ms: Option<i64>,
}

/// "4m", "1h", "2d": how long ago, as the menu shows it.
pub fn ago(ms: i64) -> String {
    let s = (ms.max(0) / 1000) as u64;
    match s {
        0..=44 => "now".into(),
        45..=3599 => format!("{}m", ((s + 30) / 60).max(1)),
        3600..=86_399 => format!("{}h", s / 3600),
        _ => format!("{}d", s / 86_400),
    }
}

/// A permission request as a short question: "Run npm test?", "Edit ci.yml?", "Fetch docs.rs?".
pub fn question(tool: &str, input: &Value) -> String {
    let clip = |t: &str, n: usize| -> String {
        let t = t.split_whitespace().collect::<Vec<_>>().join(" ");
        if t.chars().count() > n { format!("{}…", t.chars().take(n - 1).collect::<String>()) } else { t }
    };
    let file = |p: &str| p.rsplit('/').next().unwrap_or(p).to_string();
    let command = input["command"].as_str().map(str::to_string).or_else(|| input["command"].as_array().map(|a| a.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(" ")));
    let path = input["file_path"].as_str().or(input["path"].as_str()).or(input["notebook_path"].as_str());
    match tool {
        "Bash" => format!("Run {}?", clip(command.as_deref().unwrap_or("a command"), 40)),
        "Write" | "Edit" | "MultiEdit" | "NotebookEdit" => format!("Edit {}?", clip(&file(path.unwrap_or("a file")), 40)),
        "WebFetch" => {
            let url = input["url"].as_str().unwrap_or("");
            let host = url.split("://").nth(1).unwrap_or(url).split('/').next().unwrap_or("");
            format!("Fetch {}?", clip(if host.is_empty() { "a web page" } else { host }, 40))
        }
        "WebSearch" => format!("Search the web for {}?", clip(input["query"].as_str().unwrap_or("something"), 30)),
        "file change" => "Change files?".into(),
        t if t.starts_with("command: ") => format!("Run {}?", clip(&t["command: ".len()..], 40)),
        t if t.starts_with("edit: ") => format!("Edit {}?", clip(&t["edit: ".len()..], 40)),
        t => format!("Use {}?", clip(t, 40)),
    }
}

fn account_of(run: &Run, profiles: &[Profile]) -> String {
    let profile = run.profile_id.as_deref().and_then(|id| profiles.iter().find(|p| p.id == id))
        .or_else(|| profiles.iter().find(|p| p.is_system && crate::accounts::provider_of(&p.harness) == crate::accounts::provider_of(&run.harness)));
    profile.and_then(|p| p.account.as_ref()).and_then(|a| a["short"].as_str()).map(str::to_string)
        .unwrap_or_else(|| crate::accounts::provider_words(&run.harness).to_string())
}

fn repo_name(root: &str) -> String {
    root.trim_end_matches('/').rsplit('/').next().unwrap_or(root).to_string()
}

/// The menu's content from the agents (pure, so it is tested without a daemon).
pub fn build(agents: &[Agent], profiles: &[Profile], reviewed: &HashMap<String, i64>, now_ms: i64) -> Value {
    struct Row { kind: &'static str, status: String, last: i64, value: Value, repo: String, root: String }
    let mut rows = Vec::new();
    let mut waiting = Vec::new();
    for a in agents {
        let r = a.run;
        let ended = r.ended_ms.unwrap_or(r.created_ms);
        let (kind, status) = if r.status == "waiting_for_user" {
            ("needs", "Needs you".to_string())
        } else if ACTIVE.contains(&r.status.as_str()) {
            ("working", format!("Working · {}", ago(now_ms - a.turn_started_ms)))
        } else if (DONE.contains(&r.status.as_str()) || FAILED.contains(&r.status.as_str()))
            && now_ms - ended <= WEEK_MS && reviewed.get(&r.id).copied().unwrap_or(0) < ended {
            ("review", if FAILED.contains(&r.status.as_str()) { "Failed · to review".to_string() } else { "To review".to_string() })
        } else {
            ("idle", format!("Idle · {}", ago(now_ms - ended)))
        };
        let title = a.task.map(|t| t.title.clone()).filter(|t| !t.trim().is_empty()).unwrap_or_else(|| r.title.clone());
        let title = crate::redact::redact(&title);
        let root = a.task.map(|t| t.repo_root.clone()).unwrap_or_default();
        let repo = repo_name(&root);
        let account = account_of(r, profiles);
        let last = r.created_ms.max(a.turn_started_ms).max(r.ended_ms.unwrap_or(0)).max(a.asked_ms.unwrap_or(0));
        if kind == "needs" {
            let att = r.attention.clone().unwrap_or(Value::Null);
            let permission = att["kind"] == "permission";
            let asked = a.asked_ms.unwrap_or(last);
            waiting.push((asked, json!({
                "run_id": r.id, "request_id": att["request_id"].as_str().map(str::to_string).or_else(|| att["request_id"].as_i64().map(|n| n.to_string())),
                "answerable": permission,
                "question": if permission { question(att["tool"].as_str().unwrap_or(""), &att["input"]) } else { format!("{} is waiting for you", title) },
                "always": att["always"]["label"].clone(),
                "harness": r.harness, "repo": repo, "title": title, "account": account, "ago": ago(now_ms - asked),
            })));
        }
        rows.push(Row { kind, last, value: json!({"run_id": r.id, "title": title, "kind": kind, "status": status, "account": account}), status, repo, root });
    }
    let count = |k: &str| rows.iter().filter(|r| r.kind == k || (k == "working" && r.kind == "needs")).count();
    let (working, review, idle) = (count("working"), count("review"), count("idle"));
    let summary = [(working, "working"), (review, "to review"), (idle, "idle")].iter().filter(|(n, _)| *n > 0)
        .map(|(n, w)| format!("{n} {w}")).collect::<Vec<_>>().join(" · ");
    // Repositories and their agents, most recent first.
    rows.sort_by(|a, b| b.last.cmp(&a.last));
    let mut order: Vec<String> = Vec::new();
    let mut by_repo: HashMap<String, Vec<&Row>> = HashMap::new();
    for r in &rows {
        if !by_repo.contains_key(&r.root) { order.push(r.root.clone()); }
        by_repo.entry(r.root.clone()).or_default().push(r);
    }
    let repos: Vec<Value> = order.iter().map(|root| {
        let list = &by_repo[root];
        json!({"name": list[0].repo, "root": root, "count": list.len(), "waiting": list.iter().any(|r| r.kind == "needs"),
               "agents": list.iter().take(AGENTS_PER_REPO).map(|r| { let mut v = r.value.clone(); v["status"] = json!(r.status); v }).collect::<Vec<_>>()})
    }).collect();
    waiting.sort_by(|a, b| b.0.cmp(&a.0));
    json!({
        "summary": if summary.is_empty() { "No agents".to_string() } else { summary },
        "counts": {"working": working, "review": review, "idle": idle, "needs": waiting.len(), "total": rows.len()},
        "waiting": waiting.into_iter().map(|(_, v)| v).collect::<Vec<_>>(),
        "repos": repos,
    })
}

/// The reviewed marks (run id -> when its review was last opened, or it was merged), as `state`
/// gives them to every client (the TUI counts "to review" from them, T-26).
pub fn reviewed_marks(conn: &rusqlite::Connection) -> Result<HashMap<String, i64>> {
    use rusqlite::OptionalExtension;
    let raw: Option<String> = conn.query_row("SELECT value FROM meta WHERE key=?1", [REVIEWED_KEY], |r| r.get(0)).optional()?;
    Ok(raw.and_then(|j| serde_json::from_str::<HashMap<String, i64>>(&j).ok()).unwrap_or_default())
}

fn reviewed(d: &Daemon) -> Result<HashMap<String, i64>> {
    let store = d.store.lock().unwrap();
    reviewed_marks(&store.conn)
}

/// `menubar.snapshot`: what the item and its menu show now.
pub fn snapshot(d: &Arc<Daemon>) -> Result<Value> {
    let marks = reviewed(d)?;
    let (runs, tasks, profiles, cursor) = {
        let store = d.store.lock().unwrap();
        (store.runs()?, store.tasks()?, store.profiles()?, store.max_seq()?)
    };
    let tasks_by: HashMap<&str, &Task> = tasks.iter().map(|t| (t.id.as_str(), t)).collect();
    let mut agents = Vec::new();
    for r in runs.iter().filter(|r| r.parent_run_id.is_none()) {
        let task = tasks_by.get(r.task_id.as_str()).copied();
        if task.is_some_and(|t| t.archived_ms.is_some()) || d.run_role(&r.id) == "overseer" { continue; }
        // A run that handed its work off is the same agent as its successor (as in the roster).
        if r.status == crate::handoff::HANDED_OFF && crate::handoff::successor_of(d, &r.id).is_some() { continue; }
        let store = d.store.lock().unwrap();
        let turn_started_ms: i64 = store.conn.query_row("SELECT MAX(started_ms) FROM turns WHERE run_id=?1", [&r.id], |x| x.get::<_, Option<i64>>(0)).ok().flatten().unwrap_or(r.created_ms);
        let asked_ms = if r.status == "waiting_for_user" {
            store.conn.query_row("SELECT MAX(ts) FROM events WHERE run_id=?1 AND kind='permission'", [&r.id], |x| x.get::<_, Option<i64>>(0)).ok().flatten()
        } else { None };
        drop(store);
        agents.push(Agent { run: r, task, turn_started_ms, asked_ms });
    }
    let mut out = build(&agents, &profiles, &marks, now());
    let voice = crate::voice::get(d).unwrap_or(Value::Null);
    let instance = crate::paths::instance();
    out["voice"] = json!({"enabled": voice["enabled"].as_bool().unwrap_or(false), "muted": voice["muted"].as_bool().unwrap_or(false), "available": voice["available"].as_bool().unwrap_or(false)});
    out["dev"] = json!(instance.as_deref().is_some_and(|i| i.starts_with("dev-")));
    out["instance"] = json!(instance);
    out["cursor"] = json!(cursor);
    Ok(out)
}

/// `review.seen {marks: {run_id: ms}}`: the finished agents VS Code or the TUI has shown the
/// owner, merged with what the daemon knew (the later time wins). Marks that moved are announced
/// as a `review_seen` event, so every open surface clears them at once (T-26).
pub fn review_seen(d: &Daemon, p: &Value) -> Result<Value> {
    let incoming: &Map<String, Value> = p["marks"].as_object().ok_or_else(|| anyhow::anyhow!("marks must be an object of run id to time"))?;
    Ok(json!({"ok": true, "kept": mark_seen(d, incoming)?}))
}

/// Merges reviewed marks into the daemon's, announces the ones that moved, and returns how many
/// are kept. Merging an agent's work back marks it too, whoever merged it.
pub fn mark_seen(d: &Daemon, incoming: &Map<String, Value>) -> Result<usize> {
    // Keep a month: older agents are idle whatever their mark says.
    let horizon = now() - 4 * WEEK_MS;
    let (kept, changed) = {
        let store = d.store.lock().unwrap();
        let mut marks = reviewed_marks(&store.conn)?;
        let mut changed = Map::new();
        for (run, ms) in incoming {
            let Some(ms) = ms.as_i64() else { continue };
            let e = marks.entry(run.clone()).or_insert(0);
            if ms > *e {
                *e = ms;
                if ms >= horizon { changed.insert(run.clone(), json!(ms)); }
            }
        }
        marks.retain(|_, ms| *ms >= horizon);
        store.conn.execute("INSERT INTO meta(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value", rusqlite::params![REVIEWED_KEY, serde_json::to_string(&marks)?])?;
        (marks.len(), changed)
    };
    if !changed.is_empty() {
        d.emit(None, None, "review_seen", "user", "exact", json!({"marks": changed}))?;
    }
    Ok(kept)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(id: &str, task: &str, status: &str, created: i64, ended: Option<i64>, profile: Option<&str>) -> Run {
        Run {
            id: id.into(), task_id: task.into(), parent_run_id: None, harness: "claude".into(), harness_version: None, profile_id: profile.map(str::to_string),
            model: None, effort: None, workspace_id: "w".into(), native_id: None, status: status.into(), exit_reason: None,
            created_ms: created, ended_ms: ended, title: id.into(), relation_source: None, relation_confidence: None,
            capabilities: json!({}), process_generation: 0, attention: None,
        }
    }
    fn task(id: &str, title: &str, root: &str) -> Task {
        Task { id: id.into(), title: title.into(), prompt: String::new(), repo_root: root.into(), target_ref: None, workspace_id: "w".into(),
            start_snapshot: None, fork_commit: None, fork_provenance: None, created_ms: 0, archived_ms: None }
    }

    #[test]
    fn ago_reads_like_the_mockup() {
        assert_eq!(ago(10_000), "now");
        assert_eq!(ago(4 * 60_000), "4m");
        assert_eq!(ago(3 * 3_600_000 + 5), "3h");
        assert_eq!(ago(2 * 86_400_000), "2d");
    }

    #[test]
    fn questions_are_short() {
        assert_eq!(question("Bash", &json!({"command": "npm test"})), "Run npm test?");
        assert_eq!(question("Write", &json!({"file_path": "/w/.github/workflows/ci.yml"})), "Edit ci.yml?");
        assert_eq!(question("WebFetch", &json!({"url": "https://docs.rs/tokio/latest"})), "Fetch docs.rs?");
        assert_eq!(question("command: cargo test -p overseerd", &json!({})), "Run cargo test -p overseerd?");
        assert!(question("Bash", &json!({"command": "x".repeat(90)})).chars().count() <= 46);
    }

    #[test]
    fn thirty_agents_fold_into_three_repositories_of_at_most_eight() {
        let now_ms = 100 * 86_400_000;
        let mut runs = Vec::new();
        let mut tasks = Vec::new();
        for (repo, n) in [("overseer", 18), ("site", 7), ("notes", 5)] {
            for k in 0..n {
                let id = format!("{repo}-{k}");
                // Newest first by creation: k = 0 is the most recent.
                let created = now_ms - (k as i64 + 1) * 60_000 - if repo == "overseer" { 0 } else if repo == "site" { 1_000 } else { 2_000 };
                let (status, ended) = match k % 3 { 0 => ("running", None), 1 => ("completed", Some(created + 1_000)), _ => ("completed", Some(created + 1_000)) };
                runs.push(run(&id, &id, status, created, ended, None));
                tasks.push(task(&id, &format!("{repo} task {k}"), &format!("/code/{repo}")));
            }
        }
        runs[0].status = "waiting_for_user".into();
        runs[0].attention = Some(json!({"kind": "permission", "request_id": "req-1", "tool": "Bash", "input": {"command": "npm test"}, "always": {"label": "Bash(npm test:*) · this session"}}));
        // Two of the finished ones were looked at in VS Code.
        let reviewed: HashMap<String, i64> = [("overseer-1".to_string(), now_ms), ("site-1".to_string(), now_ms)].into();
        let agents: Vec<Agent> = runs.iter().map(|r| Agent { run: r, task: tasks.iter().find(|t| t.id == r.task_id), turn_started_ms: r.created_ms, asked_ms: None }).collect();
        let v = build(&agents, &[], &reviewed, now_ms);
        let repos = v["repos"].as_array().unwrap();
        assert_eq!(repos.iter().map(|r| (r["name"].as_str().unwrap(), r["count"].as_u64().unwrap())).collect::<Vec<_>>(), [("overseer", 18), ("site", 7), ("notes", 5)]);
        assert!(repos.iter().all(|r| r["agents"].as_array().unwrap().len() <= AGENTS_PER_REPO));
        assert_eq!(repos[0]["agents"][0]["title"], "overseer task 0");
        assert_eq!(repos[0]["agents"][0]["status"], "Needs you");
        assert_eq!(repos[0]["waiting"], true);
        assert_eq!(repos[1]["waiting"], false);
        assert_eq!(v["counts"]["total"], 30);
        assert_eq!(v["counts"]["working"].as_u64().unwrap() + v["counts"]["review"].as_u64().unwrap() + v["counts"]["idle"].as_u64().unwrap(), 30);
        assert_eq!(v["counts"]["idle"], 2, "the two reviewed ones");
        assert_eq!(v["waiting"][0]["question"], "Run npm test?");
        assert_eq!(v["waiting"][0]["always"], "Bash(npm test:*) · this session");
        assert_eq!(v["waiting"][0]["answerable"], true);
        assert_eq!(v["waiting"][0]["account"], "Claude", "no profile: the provider's name");
    }

    #[test]
    fn summary_leaves_out_zero_counts() {
        let runs = [run("a", "a", "running", 0, None, None), run("b", "b", "running", 0, None, None), run("c", "c", "starting", 0, None, None)];
        let tasks = [task("a", "A", "/r/x"), task("b", "B", "/r/x"), task("c", "C", "/r/y")];
        let agents: Vec<Agent> = runs.iter().map(|r| Agent { run: r, task: tasks.iter().find(|t| t.id == r.task_id), turn_started_ms: 0, asked_ms: None }).collect();
        let v = build(&agents, &[], &HashMap::new(), 4 * 60_000);
        assert_eq!(v["summary"], "3 working");
        assert_eq!(v["repos"][0]["agents"][0]["status"], "Working · 4m");
        assert!(v["waiting"].as_array().unwrap().is_empty());
        assert_eq!(build(&[], &[], &HashMap::new(), 0)["summary"], "No agents");
    }

    #[test]
    fn needs_you_is_newest_first() {
        let mut runs = [run("old", "old", "waiting_for_user", 0, None, None), run("new", "new", "waiting_for_user", 0, None, None)];
        for r in runs.iter_mut() { r.attention = Some(json!({"kind": "permission", "request_id": "q", "tool": "Bash", "input": {"command": r.id.clone()}})); }
        let tasks = [task("old", "Old", "/r/x"), task("new", "New", "/r/x")];
        let agents = vec![
            Agent { run: &runs[0], task: Some(&tasks[0]), turn_started_ms: 0, asked_ms: Some(1_000) },
            Agent { run: &runs[1], task: Some(&tasks[1]), turn_started_ms: 0, asked_ms: Some(9_000) },
        ];
        let v = build(&agents, &[], &HashMap::new(), 10_000);
        assert_eq!(v["waiting"][0]["title"], "New");
        assert_eq!(v["waiting"][0]["always"], Value::Null, "no offer, no Always allow");
        assert_eq!(v["summary"], "2 working");
    }
}
