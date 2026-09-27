//! The digest of an agent (AC-183): what the daemon knows about a run from its own records and
//! events, with no model call and no git command in the path. It is read on demand, so it is as
//! current as the last event. The roster is one line per agent.

use crate::daemon::{Daemon, ACTIVE};
use crate::store::Run;
use anyhow::Result;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub const DIGEST_BYTES: usize = 4 * 1024;
pub const ROSTER_BYTES: usize = 16 * 1024;
const LAST_MESSAGES: usize = 3;
const MESSAGE_CHARS: usize = 400;
const FILES_LISTED: usize = 50;

#[derive(Serialize, Clone, Debug)]
pub struct Digest {
    pub id: String,
    pub title: String,
    pub role: String,
    /// The task, then the owner's later messages and Overseer's directions, each with its source.
    pub asked: Vec<Asked>,
    pub status: String,
    pub since_ms: i64,
    pub waiting: Option<Value>,
    pub harness: String,
    pub account: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub permission_mode: Option<String>,
    pub repository: String,
    pub branch: Option<String>,
    pub worktree: String,
    pub base: Option<String>,
    pub changed: Vec<ChangedFile>,
    pub changed_total: usize,
    pub last_messages: Vec<String>,
    pub children: Vec<ChildLine>,
    /// The harness's own numbers, or "not reported".
    pub usage: Value,
    pub last_report: Option<Value>,
    /// The agent's questions to Overseer, latest first, each with its answer when it has one.
    pub asks: Vec<Value>,
    pub last_check_in: Option<Value>,
    pub area: Vec<String>,
    pub holds: Vec<Value>,
    pub guardrails: Vec<Value>,
    pub watches: Vec<Value>,
    pub conflicts: Vec<Value>,
    pub updated_ms: i64,
}

#[derive(Serialize, Clone, Debug)]
pub struct Asked {
    pub source: String,
    pub text: String,
}

#[derive(Serialize, Clone, Debug)]
pub struct ChangedFile {
    pub path: String,
    pub kind: String,
}

#[derive(Serialize, Clone, Debug)]
pub struct ChildLine {
    pub id: String,
    pub title: String,
    pub status: String,
}

#[derive(Serialize, Clone, Debug)]
pub struct RosterLine {
    pub id: String,
    pub title: String,
    pub role: String,
    pub status: String,
    pub harness: String,
    pub repository: String,
    pub branch: Option<String>,
    pub changed_total: usize,
    pub waiting: Option<String>,
    pub children: usize,
    pub open_conflicts: usize,
}

fn head(s: &str, n: usize) -> String {
    let mut out: String = s.chars().take(n).collect();
    if s.chars().count() > n {
        out.push('…');
    }
    out
}

fn short_repo(root: &str) -> String {
    root.rsplit('/').next().unwrap_or(root).to_string()
}

impl Daemon {
    /// Every run in the tree under `root` (the run itself excluded), depth first.
    fn descendants(&self, root: &str) -> Result<Vec<Run>> {
        let store = self.store.lock().unwrap();
        let mut out = Vec::new();
        let mut queue = vec![root.to_string()];
        while let Some(id) = queue.pop() {
            for child in store.children(&id)? {
                queue.push(child.id.clone());
                out.push(child);
            }
        }
        Ok(out)
    }

    pub fn digest(&self, run_id: &str) -> Result<Digest> {
        let run = self.run(run_id)?;
        let task = self.task(&run.task_id)?;
        let ws = self.workspace(&run.workspace_id)?;
        let (turns, events, launch, account) = {
            let store = self.store.lock().unwrap();
            let turns = store.turns(run_id)?;
            let events = store.events_after(0, Some(run_id), crate::store::EVENTS_PER_RUN)?;
            let launch: Value = store.conn.query_row("SELECT launch FROM runs WHERE id=?1", [run_id], |r| r.get::<_, Option<String>>(0))?.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(Value::Null);
            let account = run.profile_id.as_deref().and_then(|p| store.profile(p).ok().flatten()).map(|p| p.name);
            (turns, events, launch, account)
        };
        let opts = launch.get("generic").map(|g| g["opts"].clone()).unwrap_or_else(|| launch["opts"].clone());
        // What was asked: the task and every later turn, each with the source that sent it.
        let mut asked: Vec<Asked> = Vec::new();
        if turns.is_empty() || run.parent_run_id.is_some() {
            asked.push(Asked { source: "task".into(), text: head(&task.prompt, MESSAGE_CHARS) });
        }
        for (i, t) in turns.iter().enumerate() {
            let source = self.turn_source(&t.id).unwrap_or_else(|| if i == 0 { "task".into() } else { "owner".into() });
            asked.push(Asked { source, text: head(&t.prompt, MESSAGE_CHARS) });
        }
        // From the events: the agent's last messages, the files it touched, what it used.
        let mut messages: Vec<String> = Vec::new();
        let mut changed: BTreeMap<String, String> = BTreeMap::new();
        let mut usage = Value::Null;
        let mut last_check_in = None;
        let mut updated_ms = run.created_ms;
        for e in &events {
            updated_ms = updated_ms.max(e.ts);
            match e.kind.as_str() {
                "output" if e.payload["role"] == "assistant" => {
                    let text = e.payload["text"].as_str().unwrap_or_default().trim();
                    if !text.is_empty() {
                        messages.push(head(text, MESSAGE_CHARS));
                        if messages.len() > LAST_MESSAGES {
                            messages.remove(0);
                        }
                    }
                }
                "file_activity" => {
                    let kind = e.payload["kind"].as_str().unwrap_or("edit").to_string();
                    for p in e.payload["paths"].as_array().cloned().unwrap_or_default() {
                        if let Some(p) = p.as_str() {
                            changed.insert(p.to_string(), kind.clone());
                        }
                    }
                }
                // The latest usage the harness reported (rate-limit windows alone are not usage).
                "usage" if e.payload.get("rate_limits").is_none() || e.payload.as_object().map(|o| o.len() > 1).unwrap_or(false) => usage = e.payload.clone(),
                "check_in" => last_check_in = Some(e.payload.clone()),
                _ => {}
            }
        }
        if let Some(cached) = self.changes_cache(&ws.id) {
            for (path, kind) in cached {
                changed.entry(path).or_insert(kind);
            }
        }
        let changed_total = changed.len();
        let changed: Vec<ChangedFile> = changed.into_iter().take(FILES_LISTED).map(|(path, kind)| ChangedFile { path, kind }).collect();
        let children: Vec<ChildLine> = self.descendants(run_id)?.into_iter().map(|c| ChildLine { id: c.id, title: c.title, status: c.status }).collect();
        let waiting = if run.status == "waiting_for_user" { run.attention.clone().or_else(|| Some(json!({"kind": "question"}))) } else { None };
        let since_ms = events.iter().rev().find(|e| e.kind == "status").map(|e| e.ts).unwrap_or(run.created_ms);
        let role = self.run_role(run_id);
        let (last_report, asks) = self.channel_summary(run_id);
        let redact = |s: &str| crate::redact::redact(s);
        let asked: Vec<Asked> = asked.into_iter().map(|a| Asked { source: a.source, text: redact(&a.text) }).collect();
        let messages: Vec<String> = messages.iter().map(|m| redact(m)).collect();
        Ok(Digest {
            id: run.id.clone(),
            title: redact(&run.title),
            role,
            asked,
            status: run.status.clone(),
            since_ms,
            waiting,
            harness: run.harness.clone(),
            account,
            model: run.model.clone(),
            effort: opts["effort"].as_str().map(str::to_string),
            permission_mode: opts["mode"].as_str().map(str::to_string),
            repository: task.repo_root.clone(),
            branch: ws.branch.clone(),
            worktree: ws.path.clone(),
            base: task.target_ref.clone(),
            changed,
            changed_total,
            last_messages: messages,
            children,
            usage: if usage.is_null() { json!("not reported") } else { usage },
            last_report,
            asks,
            last_check_in,
            area: self.area_of(run_id),
            holds: Vec::new(),
            guardrails: Vec::new(),
            watches: self.watches_of(run_id),
            conflicts: self.open_conflicts_of(run_id).unwrap_or_default(),
            updated_ms,
        })
    }

    /// The digest as the model reads it, redacted and within the bound.
    pub fn digest_text(&self, run_id: &str) -> Result<String> {
        let d = self.digest(run_id)?;
        let mut lines = vec![
            format!("id: {}", d.id),
            format!("title: {}", d.title),
            format!("role: {}", d.role),
            format!("status: {} (since {})", d.status, ago(d.since_ms)),
            format!("harness: {}{}{}", d.harness, d.account.as_ref().map(|a| format!(" · account {a}")).unwrap_or_default(), d.model.as_ref().map(|m| format!(" · model {m}")).unwrap_or_default()),
            format!("repository: {} · branch {} · worktree {}", short_repo(&d.repository), d.branch.as_deref().unwrap_or("?"), d.worktree),
        ];
        if let Some(w) = &d.waiting {
            lines.push(format!("waiting for: {}", w["kind"].as_str().unwrap_or("the owner")));
        }
        lines.push("asked:".into());
        for a in &d.asked {
            lines.push(format!("- [{}] {}", a.source, a.text.replace('\n', " ")));
        }
        lines.push(format!("changed files ({}):", d.changed_total));
        for c in &d.changed {
            lines.push(format!("- {} {}", c.kind, c.path));
        }
        if d.changed_total > d.changed.len() {
            lines.push(format!("- … and {} more", d.changed_total - d.changed.len()));
        }
        lines.push("last messages:".into());
        for m in &d.last_messages {
            lines.push(format!("- {}", m.replace('\n', " ")));
        }
        if !d.children.is_empty() {
            lines.push(format!("children: {}", d.children.iter().map(|c| format!("{} ({})", c.title, c.status)).collect::<Vec<_>>().join(", ")));
        }
        lines.push(format!("usage: {}", if d.usage.is_string() { d.usage.as_str().unwrap_or_default().to_string() } else { d.usage.to_string() }));
        if !d.area.is_empty() {
            lines.push(format!("area: {}", d.area.join(", ")));
        }
        if let Some(r) = &d.last_report {
            lines.push(format!("report: {}{}{}", r["doing"].as_str().unwrap_or(""), r["needs"].as_str().filter(|s| !s.is_empty()).map(|n| format!(" · needs {n}")).unwrap_or_default(), r["blocked"].as_str().filter(|s| !s.is_empty()).map(|b| format!(" · blocked by {b}")).unwrap_or_default()));
        }
        for a in &d.asks {
            lines.push(format!("asked Overseer: {} → {}", a["question"].as_str().unwrap_or(""), a["answer"].as_str().unwrap_or("(no answer yet)")));
        }
        for w in &d.watches {
            lines.push(if w["subject"] == d.id { format!("watched by {} ({}): {}", w["watcher_title"].as_str().unwrap_or("a watcher to come"), w["mode"].as_str().unwrap_or("watch"), w["brief"].as_str().unwrap_or("")) } else { format!("watching {}: {}", w["subject_title"].as_str().unwrap_or("?"), w["brief"].as_str().unwrap_or("")) });
        }
        if !d.conflicts.is_empty() {
            lines.push(format!("open conflicts: {}", d.conflicts.iter().map(|c| format!("{} with {} on {}", c["kind"].as_str().unwrap_or("?"), c["other_title"].as_str().unwrap_or("?"), c["paths"].as_array().map(|p| p.len()).unwrap_or(0))).collect::<Vec<_>>().join("; ")));
        }
        Ok(super::bound(&crate::redact::redact(&lines.join("\n")), DIGEST_BYTES))
    }

    /// One line per agent (top-level runs), newest first.
    pub fn roster(&self) -> Result<Vec<RosterLine>> {
        let (runs, tasks, workspaces) = {
            let store = self.store.lock().unwrap();
            (store.runs()?, store.tasks()?, store.workspaces()?)
        };
        let mut out = Vec::new();
        for r in runs.iter().filter(|r| r.parent_run_id.is_none() && self.run_role(&r.id) != "overseer") {
            let task = tasks.iter().find(|t| t.id == r.task_id);
            let ws = workspaces.iter().find(|w| w.id == r.workspace_id);
            let changed_total = self.changed_total(&r.id);
            out.push(RosterLine {
                id: r.id.clone(),
                title: crate::redact::redact(&r.title),
                role: self.run_role(&r.id),
                status: r.status.clone(),
                harness: r.harness.clone(),
                repository: task.map(|t| short_repo(&t.repo_root)).unwrap_or_default(),
                branch: ws.and_then(|w| w.branch.clone()),
                changed_total,
                waiting: if r.status == "waiting_for_user" { Some(r.attention.as_ref().and_then(|a| a["kind"].as_str()).unwrap_or("the owner").to_string()) } else { None },
                children: self.descendants(&r.id).map(|c| c.len()).unwrap_or(0),
                open_conflicts: self.open_conflicts_of(&r.id).map(|c| c.len()).unwrap_or(0),
            });
        }
        out.sort_by_key(|l| std::cmp::Reverse(runs.iter().find(|r| r.id == l.id).map(|r| r.created_ms).unwrap_or(0)));
        Ok(out)
    }

    /// The roster as the model reads it: within the bound, the oldest finished agents folded
    /// into a count when there are too many.
    pub fn roster_text(&self) -> Result<String> {
        let lines = self.roster()?;
        if lines.is_empty() {
            return Ok("No agents.".into());
        }
        let render = |l: &RosterLine| {
            format!(
                "{} · {} · {}{} · {} · {}{} · {} files changed{}{}",
                l.id,
                l.title,
                l.status,
                l.waiting.as_ref().map(|w| format!(" ({w})")).unwrap_or_default(),
                l.harness,
                l.repository,
                l.branch.as_ref().map(|b| format!(" @{b}")).unwrap_or_default(),
                l.changed_total,
                if l.children > 0 { format!(" · {} children", l.children) } else { String::new() },
                if l.open_conflicts > 0 { format!(" · {} open conflicts", l.open_conflicts) } else { String::new() }
            )
        };
        let mut shown: Vec<String> = Vec::new();
        let mut folded = 0usize;
        let mut size = 0usize;
        // Active agents first so the ones that matter are never the ones folded away.
        let mut ordered: Vec<&RosterLine> = lines.iter().filter(|l| ACTIVE.contains(&l.status.as_str())).collect();
        ordered.extend(lines.iter().filter(|l| !ACTIVE.contains(&l.status.as_str())));
        for l in ordered {
            let line = render(l);
            if size + line.len() + 1 > ROSTER_BYTES - 64 {
                folded += 1;
                continue;
            }
            size += line.len() + 1;
            shown.push(line);
        }
        if folded > 0 {
            shown.push(format!("… and {folded} more finished agents (ask for one by id)"));
        }
        Ok(crate::redact::redact(&shown.join("\n")))
    }

    fn changed_total(&self, run_id: &str) -> usize {
        let events = self.store.lock().unwrap().events_after(0, Some(run_id), crate::store::EVENTS_PER_RUN).unwrap_or_default();
        let mut paths = std::collections::BTreeSet::new();
        for e in events.iter().filter(|e| e.kind == "file_activity") {
            for p in e.payload["paths"].as_array().cloned().unwrap_or_default() {
                if let Some(p) = p.as_str() {
                    paths.insert(p.to_string());
                }
            }
        }
        if let Ok(run) = self.run(run_id) {
            if let Some(cached) = self.changes_cache(&run.workspace_id) {
                for (p, _) in cached {
                    paths.insert(p);
                }
            }
        }
        paths.len()
    }

    /// Who sent a turn: the owner, Overseer, or the task itself. Stored by later steps; a turn
    /// without a record is the owner's.
    fn turn_source(&self, turn_id: &str) -> Option<String> {
        use rusqlite::OptionalExtension;
        let store = self.store.lock().unwrap();
        store.conn.query_row("SELECT source FROM turn_sources WHERE turn_id=?1", [turn_id], |r| r.get::<_, String>(0)).optional().ok().flatten()
    }

    pub fn run_role(&self, run_id: &str) -> String {
        use rusqlite::OptionalExtension;
        let store = self.store.lock().unwrap();
        let role: Option<String> = store.conn.query_row("SELECT role FROM run_roles WHERE run_id=?1", [run_id], |r| r.get(0)).optional().ok().flatten();
        role.unwrap_or_else(|| {
            let parent: Option<String> = store.conn.query_row("SELECT parent_run_id FROM runs WHERE id=?1", [run_id], |r| r.get(0)).ok().flatten();
            if parent.is_some() { "child".into() } else { "agent".into() }
        })
    }

    pub fn area_of(&self, run_id: &str) -> Vec<String> {
        let store = self.store.lock().unwrap();
        let mut stmt = match store.conn.prepare("SELECT path FROM areas WHERE run_id=?1 ORDER BY path") {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        stmt.query_map([run_id], |r| r.get::<_, String>(0)).map(|rows| rows.flatten().collect()).unwrap_or_default()
    }
}

fn ago(ms: i64) -> String {
    let secs = (crate::daemon::now() - ms).max(0) / 1000;
    if secs < 60 {
        format!("{secs} s")
    } else if secs < 3600 {
        format!("{} min", secs / 60)
    } else {
        format!("{} h", secs / 3600)
    }
}
