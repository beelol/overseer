//! Conflicts between agents in flight (AC-192), found by the daemon with no model.
//!
//! For two agents in the same repository, their working trees are captured through a private
//! index (`git::capture_trees`: nothing in a worktree, index or branch changes) and merged in
//! memory with `git merge-tree --write-tree` from the tree of their merge base. Files that fail
//! to merge are *same lines*; files both changed that merge cleanly are *same file*. A write
//! inside another agent's area is *area crossed*, and an agent whose work no longer merges into
//! its target branch has *target moved*. Scans run when an agent's events say it changed
//! something, after a short settle, and on a slow sweep for harnesses that report nothing.

use crate::daemon::{Daemon, ACTIVE};
use crate::git;
use crate::store::{Run, Task, Workspace};
use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Events that mean an agent may have changed files or finished.
const TRIGGERS: &[&str] = &["file_activity", "turn_done", "status", "child"];
const SETTLE: Duration = Duration::from_secs(2);
const SWEEP: Duration = Duration::from_secs(8);
/// A run is not rescanned within this time unless its events ask again.
const MIN_GAP: Duration = Duration::from_millis(500);

/// What the daemon keeps between scans.
pub struct Coordination {
    pub dirty: Mutex<HashSet<String>>,
    pub notify: tokio::sync::Notify,
    scanned: Mutex<HashMap<String, (Instant, String)>>,
    /// Changed files per workspace from the last scan: path → status letter.
    changes: Mutex<HashMap<String, Vec<(String, String)>>>,
    scanning: Mutex<HashSet<String>>,
}

impl Default for Coordination {
    fn default() -> Self {
        Self {
            dirty: Mutex::new(HashSet::new()),
            notify: tokio::sync::Notify::new(),
            scanned: Mutex::new(HashMap::new()),
            changes: Mutex::new(HashMap::new()),
            scanning: Mutex::new(HashSet::new()),
        }
    }
}

/// The kinds that need a decision; the others are advisory badges.
pub fn needs_decision(kind: &str) -> bool {
    matches!(kind, "same_lines" | "area_crossed")
}

fn git_out(cwd: &Path, args: &[&str]) -> Result<(i32, String)> {
    let mut cmd = std::process::Command::new("git");
    cmd.current_dir(cwd).args(args);
    for var in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_INDEX_FILE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_COMMON_DIR",
    ] {
        cmd.env_remove(var);
    }
    cmd.env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0");
    let out = cmd.output()?;
    Ok((
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).to_string(),
    ))
}

/// `git merge-tree` of two trees from a base tree: the conflicted paths, or an empty list when
/// the merge is clean.
fn trial_merge(cwd: &Path, base_tree: &str, a: &str, b: &str) -> Result<Vec<String>> {
    let (code, out) = git_out(
        cwd,
        &[
            "merge-tree",
            "--write-tree",
            "--name-only",
            &format!("--merge-base={base_tree}"),
            a,
            b,
        ],
    )?;
    match code {
        0 => Ok(Vec::new()),
        1 => Ok(out
            .lines()
            .skip(1)
            .take_while(|l| !l.is_empty())
            .map(str::to_string)
            .collect()),
        _ => Err(anyhow!("merge-tree failed: {}", out.trim())),
    }
}

fn changed_between(cwd: &Path, from: &str, to: &str) -> Result<Vec<(String, String)>> {
    let (code, out) = git_out(
        cwd,
        &["diff-tree", "-r", "--name-status", "--no-renames", from, to],
    )?;
    if code != 0 {
        return Err(anyhow!("diff-tree failed"));
    }
    Ok(out
        .lines()
        .filter_map(|l| {
            let mut parts = l.splitn(2, '\t');
            let status = parts.next()?.chars().next()?.to_string();
            let path = parts.next()?.to_string();
            Some((path, status))
        })
        .collect())
}

fn tree_of(cwd: &Path, commit: &str) -> Option<String> {
    git::git(
        cwd,
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("{commit}^{{tree}}"),
        ],
    )
    .ok()
    .filter(|s| !s.is_empty())
}

fn inside(path: &str, area: &str) -> bool {
    let area = area.trim_end_matches('/');
    path == area || path.starts_with(&format!("{area}/"))
}

struct Party {
    run: Run,
    task: Task,
    ws: Workspace,
    head: String,
    tree: String,
}

impl Daemon {
    pub fn changes_cache(&self, workspace_id: &str) -> Option<Vec<(String, String)>> {
        self.coord
            .changes
            .lock()
            .unwrap()
            .get(workspace_id)
            .cloned()
    }

    /// The top-level run a run belongs to.
    fn root_of(&self, run_id: &str) -> Result<Run> {
        let mut run = self.run(run_id)?;
        while let Some(parent) = run.parent_run_id.clone() {
            run = self.run(&parent)?;
        }
        Ok(run)
    }

    /// The agents this run can collide with: top-level runs in the same repository, active or
    /// finished with their worktree still there, in another workspace.
    fn parties(&self, root: &Run) -> Result<(Option<Party>, Vec<Party>)> {
        let (runs, tasks, workspaces) = {
            let store = self.store.lock().unwrap();
            (store.runs()?, store.tasks()?, store.workspaces()?)
        };
        let me_task = tasks
            .iter()
            .find(|t| t.id == root.task_id)
            .cloned()
            .ok_or_else(|| anyhow!("no task"))?;
        let tmp = crate::paths::data_dir().join("tmp");
        let mut load = |r: &Run| -> Option<Party> {
            let task = tasks.iter().find(|t| t.id == r.task_id)?.clone();
            let ws = workspaces.iter().find(|w| w.id == r.workspace_id)?.clone();
            if ws.removed_ms.is_some() || !Path::new(&ws.path).exists() {
                return None;
            }
            let trees = git::capture_trees(Path::new(&ws.path), &tmp).ok()?;
            Some(Party {
                run: r.clone(),
                task,
                ws,
                head: trees.head?,
                tree: trees.worktree_tree,
            })
        };
        let me = load(root);
        let mut others = Vec::new();
        for r in runs
            .iter()
            .filter(|r| r.parent_run_id.is_none() && r.id != root.id)
        {
            let Some(task) = tasks.iter().find(|t| t.id == r.task_id) else {
                continue;
            };
            if task.repo_root != me_task.repo_root
                || r.workspace_id == root.workspace_id
                || task.archived_ms.is_some()
            {
                continue;
            }
            let finished_but_here = !ACTIVE.contains(&r.status.as_str())
                && workspaces
                    .iter()
                    .any(|w| w.id == r.workspace_id && w.removed_ms.is_none());
            if ACTIVE.contains(&r.status.as_str()) || finished_but_here {
                if let Some(p) = load(r) {
                    others.push(p);
                }
            }
        }
        Ok((me, others))
    }

    /// Scan one agent against every agent it can collide with, and record what is found.
    pub fn scan_conflicts(self: &Arc<Self>, run_id: &str) -> Result<Value> {
        let root = self.root_of(run_id)?;
        // A scan of the same agent already under way (the sweep's) is waited out, so a caller
        // always gets a scan of what is there now, never a silent skip.
        let started = Instant::now();
        loop {
            let mut scanning = self.coord.scanning.lock().unwrap();
            if scanning.insert(root.id.clone()) {
                break;
            }
            drop(scanning);
            if started.elapsed() > Duration::from_secs(20) {
                return Ok(json!({"run_id": root.id, "skipped": "already scanning"}));
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let result = self.scan_conflicts_inner(&root);
        self.coord.scanning.lock().unwrap().remove(&root.id);
        result
    }

    fn scan_conflicts_inner(self: &Arc<Self>, root: &Run) -> Result<Value> {
        let started = Instant::now();
        let (me, others) = self.parties(root)?;
        let Some(me) = me else {
            return Ok(json!({"run_id": root.id, "skipped": "no worktree"}));
        };
        let cwd = Path::new(&me.ws.path);
        self.coord
            .scanned
            .lock()
            .unwrap()
            .insert(root.id.clone(), (Instant::now(), me.tree.clone()));
        // This agent's own changes, cached for the digest and the roster.
        if let Some(base) = self.task_base(&me.task, cwd) {
            if let Ok(changed) = changed_between(cwd, &base, &me.tree) {
                let paths: Vec<String> = changed.iter().map(|(p, _)| p.clone()).collect();
                self.coord
                    .changes
                    .lock()
                    .unwrap()
                    .insert(me.ws.id.clone(), changed);
                self.check_guardrails(&root.id, &paths)?;
            }
        }
        let mut found: BTreeMap<
            String,
            (String, String, Option<String>, Option<String>, Vec<String>),
        > = BTreeMap::new();
        let mut note = |key: String,
                        kind: &str,
                        a: &str,
                        b: Option<&str>,
                        target: Option<&str>,
                        paths: Vec<String>| {
            found.insert(
                key,
                (
                    kind.into(),
                    a.into(),
                    b.map(str::to_string),
                    target.map(str::to_string),
                    paths,
                ),
            );
        };
        let my_area = self.area_of(&me.run.id);
        for other in &others {
            let Some(base) = git::merge_base(cwd, &me.head, &other.head) else {
                continue;
            };
            let Some(base_tree) = tree_of(cwd, &base) else {
                continue;
            };
            let conflicted = trial_merge(cwd, &base_tree, &me.tree, &other.tree)?;
            let mine: BTreeSet<String> = changed_between(cwd, &base_tree, &me.tree)?
                .into_iter()
                .map(|(p, _)| p)
                .collect();
            let theirs: BTreeSet<String> = changed_between(cwd, &base_tree, &other.tree)?
                .into_iter()
                .map(|(p, _)| p)
                .collect();
            let same_file: Vec<String> = mine
                .intersection(&theirs)
                .filter(|p| !conflicted.contains(p))
                .cloned()
                .collect();
            let (a, b) = if me.run.id < other.run.id {
                (&me.run.id, &other.run.id)
            } else {
                (&other.run.id, &me.run.id)
            };
            if !conflicted.is_empty() {
                note(
                    format!("same_lines:{a}:{b}"),
                    "same_lines",
                    a,
                    Some(b),
                    None,
                    conflicted,
                );
            }
            if !same_file.is_empty() {
                note(
                    format!("same_file:{a}:{b}"),
                    "same_file",
                    a,
                    Some(b),
                    None,
                    same_file,
                );
            }
            // Writes inside another agent's area, in both directions.
            let their_area = self.area_of(&other.run.id);
            let crossed_by_me: Vec<String> = mine
                .iter()
                .filter(|p| their_area.iter().any(|ar| inside(p, ar)))
                .cloned()
                .collect();
            if !crossed_by_me.is_empty() {
                note(
                    format!("area_crossed:{}:{}", me.run.id, other.run.id),
                    "area_crossed",
                    &me.run.id,
                    Some(&other.run.id),
                    None,
                    crossed_by_me,
                );
            }
            let crossed_by_them: Vec<String> = theirs
                .iter()
                .filter(|p| my_area.iter().any(|ar| inside(p, ar)))
                .cloned()
                .collect();
            if !crossed_by_them.is_empty() {
                note(
                    format!("area_crossed:{}:{}", other.run.id, me.run.id),
                    "area_crossed",
                    &other.run.id,
                    Some(&me.run.id),
                    None,
                    crossed_by_them,
                );
            }
        }
        // The target branch moved under this agent.
        let repo = Path::new(&me.task.repo_root);
        if let Some(target) = self.merge_target(repo, me.task.target_ref.as_deref()) {
            if let Some(tip) = git::rev_parse(repo, &target) {
                let ancestor = git::is_ancestor(cwd, &tip, &me.head);
                if !ancestor && tip != me.head {
                    if let Some(base) = git::merge_base(cwd, &me.head, &tip) {
                        if let (Some(base_tree), Some(tip_tree)) =
                            (tree_of(cwd, &base), tree_of(cwd, &tip))
                        {
                            let conflicted = trial_merge(cwd, &base_tree, &me.tree, &tip_tree)?;
                            if !conflicted.is_empty() {
                                note(
                                    format!("target_moved:{}:{target}", me.run.id),
                                    "target_moved",
                                    &me.run.id,
                                    None,
                                    Some(&target),
                                    conflicted,
                                );
                            }
                        }
                    }
                }
            }
        }
        let report = self.record_conflicts(&me, &others, found)?;
        Ok(
            json!({"run_id": root.id, "compared_with": others.len(), "ms": started.elapsed().as_millis() as u64, "open": report["open"], "opened": report["opened"], "closed": report["closed"]}),
        )
    }

    /// The base a run's changes are counted from: the task's start snapshot, else its head.
    fn task_base(&self, task: &Task, cwd: &Path) -> Option<String> {
        let snap = {
            let store = self.store.lock().unwrap();
            task.start_snapshot
                .as_deref()
                .and_then(|id| store.snapshot(id).ok().flatten())
                .map(|s| s.commit_sha)
        };
        snap.or_else(|| git::head(cwd))
    }

    fn record_conflicts(
        &self,
        me: &Party,
        others: &[Party],
        found: BTreeMap<String, (String, String, Option<String>, Option<String>, Vec<String>)>,
    ) -> Result<Value> {
        use rusqlite::OptionalExtension;
        let now = crate::daemon::now();
        let title = |id: &str| -> String {
            if id == me.run.id {
                me.run.title.clone()
            } else {
                others
                    .iter()
                    .find(|p| p.run.id == id)
                    .map(|p| p.run.title.clone())
                    .unwrap_or_else(|| id.to_string())
            }
        };
        let mut opened = Vec::new();
        let mut closed = Vec::new();
        let mut events = Vec::new();
        {
            let store = self.store.lock().unwrap();
            // Open conflicts this scan could have found again: those naming this run against a
            // party that was compared, or its target.
            let compared: HashSet<&str> = others.iter().map(|p| p.run.id.as_str()).collect();
            let mut stmt = store.conn.prepare("SELECT id, key, kind, run_a, run_b, paths FROM conflicts WHERE state='open' AND (run_a=?1 OR run_b=?1)")?;
            let open: Vec<(String, String, String, String, Option<String>, String)> = stmt
                .query_map([&me.run.id], |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                    ))
                })?
                .collect::<rusqlite::Result<_>>()?;
            drop(stmt);
            for (id, key, kind, a, b, _) in &open {
                let other = if a == &me.run.id {
                    b.as_deref()
                } else {
                    Some(a.as_str())
                };
                let comparable =
                    kind == "target_moved" || other.map(|o| compared.contains(o)).unwrap_or(false);
                if comparable && !found.contains_key(key) {
                    store.conn.execute(
                        "UPDATE conflicts SET state='gone', closed_ms=?2, last_ms=?2 WHERE id=?1",
                        rusqlite::params![id, now],
                    )?;
                    closed.push(id.clone());
                    for run in [Some(a.as_str()), b.as_deref()].into_iter().flatten() {
                        events.push((run.to_string(), "conflict_closed", json!({"id": id, "kind": kind, "state": "gone", "why": "the overlap is gone"})));
                    }
                }
            }
            for (key, (kind, a, b, target, paths)) in &found {
                let existing: Option<(String, String)> = store
                    .conn
                    .query_row(
                        "SELECT id, paths FROM conflicts WHERE key=?1 AND state='open'",
                        [key],
                        |r| Ok((r.get(0)?, r.get(1)?)),
                    )
                    .optional()?;
                let paths_json = serde_json::to_string(paths)?;
                match existing {
                    Some((id, old_paths)) => {
                        store.conn.execute(
                            "UPDATE conflicts SET paths=?2, last_ms=?3 WHERE id=?1",
                            rusqlite::params![id, paths_json, now],
                        )?;
                        if old_paths != paths_json {
                            for run in [Some(a.as_str()), b.as_deref()].into_iter().flatten() {
                                events.push((run.to_string(), "conflict", json!({"id": id, "kind": kind, "state": "open", "changed": true, "paths": paths, "run_a": a, "run_b": b, "target": target, "title_a": title(a), "title_b": b.as_deref().map(title)})));
                            }
                        }
                    }
                    None => {
                        let id = format!("c-{}", &uuid::Uuid::new_v4().simple().to_string()[..12]);
                        store.conn.execute(
                            "INSERT INTO conflicts(id, key, kind, repo, run_a, run_b, target, paths, first_ms, last_ms, state) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9, 'open')",
                            rusqlite::params![id, key, kind, me.task.repo_root, a, b, target, paths_json, now],
                        )?;
                        opened.push(id.clone());
                        for run in [Some(a.as_str()), b.as_deref()].into_iter().flatten() {
                            events.push((run.to_string(), "conflict", json!({"id": id, "kind": kind, "state": "open", "paths": paths, "run_a": a, "run_b": b, "target": target, "title_a": title(a), "title_b": b.as_deref().map(title), "needs_decision": needs_decision(kind)})));
                        }
                    }
                }
            }
        }
        for (run, kind, payload) in events {
            let task = self.run(&run).ok().map(|r| r.task_id);
            self.emit(
                task.as_deref(),
                Some(&run),
                kind,
                "daemon",
                "exact",
                payload,
            )?;
        }
        let open = self.open_conflicts_of(&me.run.id)?.len();
        Ok(json!({"open": open, "opened": opened, "closed": closed}))
    }

    /// Open conflicts naming a run, as the digest and the roster show them.
    pub fn open_conflicts_of(&self, run_id: &str) -> Result<Vec<Value>> {
        let store = self.store.lock().unwrap();
        let mut stmt = store.conn.prepare("SELECT id, kind, run_a, run_b, target, paths, first_ms, last_ms FROM conflicts WHERE state='open' AND (run_a=?1 OR run_b=?1) ORDER BY first_ms")?;
        let rows = stmt.query_map([run_id], |r| {
            let a: String = r.get(2)?;
            let b: Option<String> = r.get(3)?;
            let other = if a == run_id { b.clone() } else { Some(a.clone()) };
            Ok(json!({"id": r.get::<_, String>(0)?, "kind": r.get::<_, String>(1)?, "run_a": a, "run_b": b, "other": other, "target": r.get::<_, Option<String>>(4)?, "paths": serde_json::from_str::<Value>(&r.get::<_, String>(5)?).unwrap_or(json!([])), "first_ms": r.get::<_, i64>(6)?, "last_ms": r.get::<_, i64>(7)?}))
        })?;
        let mut out: Vec<Value> = rows.collect::<rusqlite::Result<_>>()?;
        drop(stmt);
        for c in &mut out {
            let other = c["other"].as_str().map(str::to_string);
            let title = other
                .as_deref()
                .and_then(|o| store.run(o).ok().flatten())
                .map(|r| r.title)
                .or_else(|| c["target"].as_str().map(|t| format!("branch {t}")));
            c["other_title"] = json!(title);
            c["needs_decision"] = json!(needs_decision(c["kind"].as_str().unwrap_or("")));
        }
        Ok(out)
    }

    /// Every conflict, open first; closed ones from the last hour follow when asked.
    pub fn conflicts_list(&self, run_id: Option<&str>, include_closed: bool) -> Result<Value> {
        let store = self.store.lock().unwrap();
        let since = crate::daemon::now() - 3600 * 1000;
        let mut stmt = store.conn.prepare("SELECT id, kind, repo, run_a, run_b, target, paths, first_ms, last_ms, state, resolution, closed_ms FROM conflicts WHERE (state='open' OR (?2 AND last_ms > ?3)) AND (?1 IS NULL OR run_a=?1 OR run_b=?1) ORDER BY state='open' DESC, first_ms")?;
        let rows = stmt.query_map(rusqlite::params![run_id, include_closed, since], |r| {
            Ok(json!({"id": r.get::<_, String>(0)?, "kind": r.get::<_, String>(1)?, "repo": r.get::<_, String>(2)?, "run_a": r.get::<_, String>(3)?, "run_b": r.get::<_, Option<String>>(4)?, "target": r.get::<_, Option<String>>(5)?,
                "paths": serde_json::from_str::<Value>(&r.get::<_, String>(6)?).unwrap_or(json!([])), "first_ms": r.get::<_, i64>(7)?, "last_ms": r.get::<_, i64>(8)?, "state": r.get::<_, String>(9)?,
                "resolution": r.get::<_, Option<String>>(10)?.and_then(|s| serde_json::from_str::<Value>(&s).ok()), "closed_ms": r.get::<_, Option<i64>>(11)?}))
        })?;
        let mut out: Vec<Value> = rows.collect::<rusqlite::Result<_>>()?;
        drop(stmt);
        for c in &mut out {
            for side in ["run_a", "run_b"] {
                if let Some(id) = c[side].as_str() {
                    c[format!("title_{}", &side[4..])] =
                        json!(store.run(id).ok().flatten().map(|r| r.title));
                }
            }
            c["needs_decision"] = json!(needs_decision(c["kind"].as_str().unwrap_or("")));
        }
        Ok(json!({"conflicts": out}))
    }

    /// The owner (or Overseer) says this one is fine.
    pub fn conflict_dismiss(&self, id: &str, by: &str) -> Result<Value> {
        let now = crate::daemon::now();
        let (kind, a, b) = {
            let store = self.store.lock().unwrap();
            let row: (String, String, Option<String>, String) = store
                .conn
                .query_row(
                    "SELECT kind, run_a, run_b, state FROM conflicts WHERE id=?1",
                    [id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                )
                .map_err(|_| anyhow!("unknown conflict {id}"))?;
            if row.3 != "open" {
                return Err(anyhow!("conflict {id} is already {}", row.3));
            }
            store.conn.execute("UPDATE conflicts SET state='dismissed', resolution=?2, closed_ms=?3, last_ms=?3 WHERE id=?1", rusqlite::params![id, json!({"action": "dismiss", "by": by}).to_string(), now])?;
            (row.0, row.1, row.2)
        };
        for run in [Some(a.as_str()), b.as_deref()].into_iter().flatten() {
            let task = self.run(run).ok().map(|r| r.task_id);
            self.emit(
                task.as_deref(),
                Some(run),
                "conflict_closed",
                by,
                "exact",
                json!({"id": id, "kind": kind, "state": "dismissed"}),
            )?;
        }
        Ok(json!({"id": id, "state": "dismissed"}))
    }

    /// Mark a run for the next scan (the events loop and the tests call this).
    pub fn conflicts_touch(&self, run_id: &str) {
        self.coord.dirty.lock().unwrap().insert(run_id.to_string());
        self.coord.notify.notify_one();
    }

    fn needs_sweep(&self, run: &Run) -> bool {
        let Ok(ws) = self.workspace(&run.workspace_id) else {
            return false;
        };
        if ws.removed_ms.is_some() || !Path::new(&ws.path).exists() {
            return false;
        }
        let Ok(trees) =
            git::capture_trees(Path::new(&ws.path), &crate::paths::data_dir().join("tmp"))
        else {
            return false;
        };
        let scanned = self.coord.scanned.lock().unwrap();
        scanned
            .get(&run.id)
            .map(|(_, t)| *t != trees.worktree_tree)
            .unwrap_or(true)
    }
}

/// The scan loop: agents whose events said they changed something are scanned after a short
/// settle; every active agent is checked on a slow sweep in case its harness reports nothing.
pub fn start(daemon: Arc<Daemon>) {
    let d = daemon.clone();
    tokio::spawn(async move {
        let mut live = d.events.subscribe();
        loop {
            match live.recv().await {
                Ok(e) => {
                    if let Some(run) = e.run_id.as_deref() {
                        if TRIGGERS.contains(&e.kind.as_str()) {
                            d.conflicts_touch(run);
                        }
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                    let runs = d.store.lock().unwrap().runs().unwrap_or_default();
                    for r in runs.iter().filter(|r| {
                        r.parent_run_id.is_none() && ACTIVE.contains(&r.status.as_str())
                    }) {
                        d.conflicts_touch(&r.id);
                    }
                }
                Err(_) => return,
            }
        }
    });
    tokio::spawn(async move {
        loop {
            let woke = tokio::time::timeout(SWEEP, daemon.coord.notify.notified())
                .await
                .is_ok();
            if woke {
                tokio::time::sleep(SETTLE).await;
            }
            let mut batch: Vec<String> = daemon.coord.dirty.lock().unwrap().drain().collect();
            if !woke {
                let runs = daemon.store.lock().unwrap().runs().unwrap_or_default();
                for r in runs
                    .iter()
                    .filter(|r| r.parent_run_id.is_none() && ACTIVE.contains(&r.status.as_str()))
                {
                    if !batch.contains(&r.id) && daemon.needs_sweep(r) {
                        batch.push(r.id.clone());
                    }
                }
            }
            // Roots only, once each, and not again within the minimum gap.
            let mut roots: Vec<String> = Vec::new();
            for id in batch {
                if let Ok(root) = daemon.root_of(&id) {
                    let recent = daemon
                        .coord
                        .scanned
                        .lock()
                        .unwrap()
                        .get(&root.id)
                        .map(|(at, _)| at.elapsed() < MIN_GAP)
                        .unwrap_or(false);
                    if !recent && !roots.contains(&root.id) {
                        roots.push(root.id);
                    }
                }
            }
            for root in roots {
                let d = daemon.clone();
                let result = tokio::task::spawn_blocking(move || d.scan_conflicts(&root)).await;
                if let Ok(Err(e)) = result {
                    crate::log(&format!("conflict scan failed: {e:#}"));
                }
            }
        }
    });
}
