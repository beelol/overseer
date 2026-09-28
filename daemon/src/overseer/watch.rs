//! Watches (AC-193, AC-194): one agent set to look closely at another. The watcher reads its
//! subject through the daemon's tools and files findings; it never messages or stops the
//! subject, Overseer acts on a finding at its level. The daemon wakes the watcher when the
//! subject's turn ends and once when it finishes, with what changed since the last wake, within
//! the limits: two watchers per subject, no watcher of a watcher, no circle, twelve wakes an
//! hour, a budget of wakes. A watch that checks gets a worktree of its own at the subject's
//! latest snapshot, refreshed at each wake and removed when the watch ends.

use crate::daemon::{Daemon, ACTIVE};
use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};
use sha2::Digest as _;
use std::path::Path;
use std::sync::Arc;

/// What a wake carries at most; the rest is on demand through the tools.
pub const WAKE_BYTES: usize = 32 * 1024;
pub const WAKES_PER_HOUR: i64 = 12;
pub const WATCHERS_PER_SUBJECT: i64 = 2;
/// Wakes a watch may use before it ends at its budget.
pub const DEFAULT_BUDGET: i64 = 60;
/// A watcher's tools: reads of its subject, and the finding.
pub const WATCHER_TOOLS: &[&str] = &["roster", "agent", "conversation", "changes", "diff", "file", "finding"];
/// The tools whose `id` a watcher may only point at its subject.
pub const SUBJECT_READS: &[&str] = &["agent", "conversation", "changes", "diff", "file"];

#[derive(Clone, Debug)]
pub struct Watch {
    pub id: String,
    pub subject: String,
    pub watcher: Option<String>,
    pub brief: String,
    pub mode: String,
    pub hold_on_stop: bool,
    pub harness: String,
    pub model: Option<String>,
    pub last_seq: i64,
    pub last_snapshot: Option<String>,
    pub wakes: i64,
    pub budget: i64,
    pub copy_workspace: Option<String>,
    pub copy_path: Option<String>,
    pub ended_ms: Option<i64>,
    pub end_reason: Option<String>,
}

impl Watch {
    fn json(&self, d: &Daemon) -> Value {
        let title = |id: &str| d.run(id).map(|r| crate::redact::redact(&r.title)).unwrap_or_else(|_| id.to_string());
        json!({
            "id": self.id, "subject": self.subject, "subject_title": title(&self.subject), "watcher": self.watcher, "watcher_title": self.watcher.as_deref().map(title),
            "brief": self.brief, "mode": self.mode, "hold_on_stop": self.hold_on_stop, "harness": self.harness, "wakes": self.wakes, "budget": self.budget,
            "last_snapshot": self.last_snapshot, "copy_workspace": self.copy_workspace, "copy_path": self.copy_path, "ended_ms": self.ended_ms, "end_reason": self.end_reason,
            "open": self.ended_ms.is_none(),
        })
    }
}

const COLUMNS: &str = "id, subject, watcher, brief, mode, hold_on_stop, harness, model, last_seq, last_snapshot, wakes, budget, copy_workspace, copy_path, ended_ms, end_reason";

fn row(r: &rusqlite::Row) -> rusqlite::Result<Watch> {
    Ok(Watch {
        id: r.get(0)?,
        subject: r.get(1)?,
        watcher: r.get::<_, Option<String>>(2)?.filter(|s| !s.is_empty()),
        brief: r.get(3)?,
        mode: r.get(4)?,
        hold_on_stop: r.get::<_, i64>(5)? != 0,
        harness: r.get(6)?,
        model: r.get(7)?,
        last_seq: r.get(8)?,
        last_snapshot: r.get(9)?,
        wakes: r.get(10)?,
        budget: r.get(11)?,
        copy_workspace: r.get(12)?,
        copy_path: r.get(13)?,
        ended_ms: r.get(14)?,
        end_reason: r.get(15)?,
    })
}

impl Daemon {
    // ------------------------------------------------------------------ the rows

    pub fn watch(&self, id: &str) -> Result<Watch> {
        let store = self.store.lock().unwrap();
        store.conn.query_row(&format!("SELECT {COLUMNS} FROM watches WHERE id=?1"), [id], row).map_err(|_| anyhow!("no watch {id}"))
    }

    fn watches_where(&self, clause: &str, params: &[&dyn rusqlite::ToSql]) -> Result<Vec<Watch>> {
        let store = self.store.lock().unwrap();
        let mut stmt = store.conn.prepare(&format!("SELECT {COLUMNS} FROM watches WHERE {clause} ORDER BY created_ms"))?;
        let rows = stmt.query_map(params, row)?.collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }

    fn open_watches_on(&self, subject: &str) -> Result<Vec<Watch>> {
        self.watches_where("subject=?1 AND ended_ms IS NULL", &[&subject])
    }

    /// The watch this run watches for: open first, else the one that ended last (a final finding
    /// may follow the final wake).
    pub fn watch_of_watcher(&self, run_id: &str) -> Option<Watch> {
        let open = self.watches_where("watcher=?1 AND ended_ms IS NULL", &[&run_id]).ok()?;
        if let Some(w) = open.into_iter().next() {
            return Some(w);
        }
        let ended = self.watches_where("watcher=?1 AND ended_ms > ?2", &[&run_id, &(crate::daemon::now() - 3_600_000)]).ok()?;
        ended.into_iter().last()
    }

    pub fn is_watcher(&self, run_id: &str) -> bool {
        self.watches_where("watcher=?1 AND ended_ms IS NULL", &[&run_id]).map(|w| !w.is_empty()).unwrap_or(false)
    }

    /// The watches a run is in, as subject or watcher, for its digest and the surfaces.
    pub fn watches_of(&self, run_id: &str) -> Vec<Value> {
        self.watches_where("(subject=?1 OR watcher=?1) AND ended_ms IS NULL", &[&run_id]).unwrap_or_default().iter().map(|w| w.json(self)).collect()
    }

    pub fn watches_list(&self, run_id: Option<&str>, open_only: bool) -> Result<Value> {
        let rows = match run_id {
            Some(r) => self.watches_where("(subject=?1 OR watcher=?1) AND (?2 = 0 OR ended_ms IS NULL)", &[&r, &(open_only as i64)])?,
            None => self.watches_where("(?1 = 0 OR ended_ms IS NULL)", &[&(open_only as i64)])?,
        };
        Ok(json!({"watches": rows.iter().map(|w| w.json(self)).collect::<Vec<_>>()}))
    }

    // ------------------------------------------------------------------ start and end

    /// Set a watcher on a subject with a brief. A new agent is created at the first wake (so an
    /// unchanged subject costs nothing); an idle agent the owner names is woken instead.
    pub fn watch_start(self: &Arc<Self>, p: &Value, by: &str) -> Result<Value> {
        let subject = p["subject"].as_str().or(p["agent"].as_str()).filter(|s| !s.is_empty()).ok_or_else(|| anyhow!("a watch needs a subject"))?.to_string();
        let brief = p["brief"].as_str().or(p["text"].as_str()).unwrap_or("").trim().to_string();
        if brief.is_empty() {
            bail!("a watch needs a brief: what to look for");
        }
        let mode = p["mode"].as_str().unwrap_or("watch").to_string();
        if !["watch", "check"].contains(&mode.as_str()) {
            bail!("a watch's mode is watch or check");
        }
        let subject_run = self.run(&subject).map_err(|_| anyhow!("no agent {subject}"))?;
        // A circle first: the named watcher is itself watched by the subject.
        if let Some(w) = p["watcher"].as_str().filter(|s| !s.is_empty()) {
            if self.open_watches_on(w)?.iter().any(|o| o.watcher.as_deref() == Some(subject.as_str())) {
                bail!("{} watches {}; no circle", subject_run.title, self.run(w).map(|r| r.title).unwrap_or(w.to_string()));
            }
        }
        match self.run_role(&subject).as_str() {
            "watcher" => bail!("{} is a watcher; no watcher of a watcher", subject_run.title),
            "overseer" => bail!("Overseer is not watched"),
            _ => {}
        }
        if self.is_watcher(&subject) {
            bail!("{} is watching another agent; no watcher of a watcher", subject_run.title);
        }
        let open = self.open_watches_on(&subject)?;
        if open.len() as i64 >= WATCHERS_PER_SUBJECT {
            bail!("{} already has {WATCHERS_PER_SUBJECT} watchers", subject_run.title);
        }
        let watcher = p["watcher"].as_str().filter(|s| !s.is_empty()).map(str::to_string);
        if let Some(w) = &watcher {
            let run = self.run(w).map_err(|_| anyhow!("no agent {w}"))?;
            if w == &subject {
                bail!("an agent cannot watch itself");
            }
            if run.parent_run_id.is_some() || self.run_role(w) != "agent" {
                bail!("{} cannot be a watcher (only a top-level agent can)", run.title);
            }
            if ACTIVE.contains(&run.status.as_str()) {
                bail!("{} is busy; name an idle agent, or leave the watcher out for a new one", run.title);
            }
            if !self.open_watches_on(w)?.is_empty() {
                bail!("{} is being watched; no circle", run.title);
            }
            if open.iter().any(|o| o.watcher.as_deref() == Some(w.as_str())) {
                bail!("{} already watches {}", run.title, subject_run.title);
            }
        }
        let session = self.overseer_session()?;
        let harness = p["harness"].as_str().map(str::to_string).or_else(|| session["harness"].as_str().map(str::to_string)).unwrap_or_else(|| "claude".into());
        if !["claude", "codex", "opencode"].contains(&harness.as_str()) && watcher.is_none() {
            bail!("a watcher runs on claude, codex or opencode, not {harness}");
        }
        let id = format!("wt-{}", &uuid::Uuid::new_v4().simple().to_string()[..12]);
        let budget = p["budget"].as_i64().filter(|b| *b > 0).unwrap_or(DEFAULT_BUDGET);
        let hold_on_stop = p["hold_on_stop"].as_bool().unwrap_or(false);
        let last_seq = self.store.lock().unwrap().max_seq()?;
        {
            let store = self.store.lock().unwrap();
            store.conn.execute(
                "INSERT INTO watches(id, subject, watcher, brief, mode, hold_on_stop, harness, model, set_by, created_ms, last_seq, wakes, budget) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 0, ?12)",
                rusqlite::params![id, subject, watcher.clone().unwrap_or_default(), brief, mode, hold_on_stop as i64, harness, p["model"].as_str(), by, crate::daemon::now(), last_seq, budget],
            )?;
        }
        // A watch that checks gets its copy now, so the first wake already has it.
        if mode == "check" {
            self.refresh_copy(&id)?;
        }
        let w = self.watch(&id)?;
        self.emit(Some(&subject_run.task_id), Some(&subject), "watch_started", by, "exact", w.json(self))?;
        if let Some(wr) = &watcher {
            if let Ok(run) = self.run(wr) {
                self.emit(Some(&run.task_id), Some(wr), "watch_started", by, "exact", w.json(self))?;
            }
        }
        let card = w.json(self);
        self.append_session_message(&session["id"].as_str().unwrap_or_default().to_string(), "card", None, &format!("Watching {}{}: {}", subject_run.title, watcher.as_ref().and_then(|w| self.run(w).ok()).map(|r| format!(" with {}", r.title)).unwrap_or_default(), brief), Some(&json!({"kind": "watch", "watch": card})))?;
        Ok(w.json(self))
    }

    /// The watch ends: the subject finished, it was ended, or its budget is spent; it says which.
    pub fn watch_end(self: &Arc<Self>, id: &str, reason: &str, by: &str) -> Result<Value> {
        let w = self.watch(id)?;
        if w.ended_ms.is_some() {
            return Ok(w.json(self));
        }
        self.store.lock().unwrap().conn.execute("UPDATE watches SET ended_ms=?2, end_reason=?3 WHERE id=?1", rusqlite::params![id, crate::daemon::now(), reason])?;
        let w = self.watch(id)?;
        for run in [Some(w.subject.clone()), w.watcher.clone()].into_iter().flatten() {
            if let Ok(r) = self.run(&run) {
                self.emit(Some(&r.task_id), Some(&run), "watch_ended", by, "exact", json!({"watch": id, "reason": reason}))?;
            }
        }
        let session = self.overseer_session()?;
        self.append_session_message(&session["id"].as_str().unwrap_or_default().to_string(), "card", None, &format!("The watch on {} ended: {reason}.", self.run(&w.subject).map(|r| r.title).unwrap_or(w.subject.clone())), Some(&json!({"kind": "watch_ended", "watch": w.json(self), "reason": reason})))?;
        // A new watcher's run ends with its watch once it is idle; its copy goes then.
        self.finish_ended_watches()?;
        Ok(w.json(self))
    }

    /// Ended watches whose copy is still there: removed once nothing runs in it (AC-24's rules,
    /// through the same cleanup as any worktree).
    pub fn finish_ended_watches(self: &Arc<Self>) -> Result<()> {
        for w in self.watches_where("ended_ms IS NOT NULL AND copy_workspace IS NOT NULL", &[])? {
            let Some(ws_id) = w.copy_workspace.clone() else { continue };
            let Ok(ws) = self.workspace(&ws_id) else { continue };
            if ws.removed_ms.is_some() {
                continue;
            }
            let plan = self.cleanup_plan(&ws_id)?;
            if plan["removable"] == true {
                self.cleanup(&ws_id, true)?;
                self.emit(None, Some(&w.subject), "watch_copy_removed", "daemon", "exact", json!({"watch": w.id, "path": ws.path}))?;
            } else if let Some(wr) = &w.watcher {
                // The watcher of a finished watch that is still working is asked to stop.
                if let Ok(run) = self.run(wr) {
                    if ACTIVE.contains(&run.status.as_str()) && self.run_role(wr) == "watcher" && w.ended_ms.map(|e| crate::daemon::now() - e > 60_000).unwrap_or(false) {
                        let _ = self.interrupt(wr);
                    }
                }
            }
        }
        Ok(())
    }

    // ------------------------------------------------------------------ the copy (AC-194)

    /// The watcher's own worktree at the subject's latest snapshot (uncommitted changes
    /// included): created once, then reset to each new snapshot. The subject's worktree is only
    /// read.
    fn refresh_copy(&self, watch_id: &str) -> Result<String> {
        let w = self.watch(watch_id)?;
        let subject = self.run(&w.subject)?;
        let ws = self.workspace(&subject.workspace_id)?;
        let snap = self.take_snapshot(&ws, "watch")?;
        let repo = Path::new(&ws.repo_root);
        let path = match &w.copy_path {
            Some(p) if Path::new(p).exists() => {
                crate::git::git(Path::new(p), &["reset", "-q", "--hard", &snap.commit_sha])?;
                crate::git::git(Path::new(p), &["clean", "-fdq"])?;
                p.clone()
            }
            _ => {
                let repo_name = repo.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "repo".into());
                let hash = format!("{:x}", sha2::Sha256::digest(ws.common_dir.as_bytes()))[..8].to_string();
                let parent = crate::paths::worktrees_dir().join(format!("{repo_name}-{hash}"));
                std::fs::create_dir_all(&parent)?;
                let path = parent.join(format!("watch-{}", &w.id[3..]));
                crate::git::git(repo, &["worktree", "add", "--detach", "-q", &path.display().to_string(), &snap.commit_sha])?;
                let path = std::fs::canonicalize(&path)?.display().to_string();
                self.store.lock().unwrap().conn.execute("UPDATE watches SET copy_path=?2 WHERE id=?1", rusqlite::params![watch_id, path])?;
                path
            }
        };
        self.store.lock().unwrap().conn.execute("UPDATE watches SET last_snapshot=?2 WHERE id=?1", rusqlite::params![watch_id, snap.commit_sha])?;
        Ok(path)
    }

    // ------------------------------------------------------------------ wakes

    /// The subject's turn ended, or it finished: each watcher is woken with what is new. Nothing
    /// new means no wake; a finished subject always gets its one last wake, then the watch ends.
    pub fn subject_changed(self: &Arc<Self>, subject: &str, reason: &str) -> Result<()> {
        // One wake decision at a time (the event loop and the ticker both get here), each on the
        // watch as it is now, so two wakes never take the same number or overlap.
        static WAKES: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _one = WAKES.lock().unwrap_or_else(|e| e.into_inner());
        let finished = reason == "finished";
        for w in self.open_watches_on(subject)? {
            let events = self.store.lock().unwrap().events_after(w.last_seq, Some(subject), 4000)?;
            let changed = events.iter().any(|e| ["output", "tool", "file_activity", "error", "turn_started"].contains(&e.kind.as_str()));
            if !changed && !finished {
                continue;
            }
            let now = crate::daemon::now();
            let hour: i64 = self.store.lock().unwrap().conn.query_row("SELECT COUNT(*) FROM watch_wakes WHERE watch_id=?1 AND ts > ?2", rusqlite::params![w.id, now - 3_600_000], |r| r.get(0))?;
            if hour >= WAKES_PER_HOUR && !finished {
                self.emit(None, Some(subject), "watch_capped", "daemon", "exact", json!({"watch": w.id, "wakes_this_hour": hour, "cap": WAKES_PER_HOUR}))?;
                continue;
            }
            if w.wakes >= w.budget {
                self.watch_end(&w.id, "budget spent", "daemon")?;
                continue;
            }
            self.wake(&w, reason, events.last().map(|e| e.seq).unwrap_or(w.last_seq))?;
            if finished {
                self.watch_end(&w.id, "the subject finished", "daemon")?;
            }
        }
        Ok(())
    }

    /// A watched subject completed a turn: it is finished when it stays idle for the grace
    /// period (the check-ins' own); a new turn inside it means the work goes on.
    pub fn subject_finishing(&self, subject: &str, status: &str) -> Result<()> {
        if !matches!(status, "completed" | "failed" | "interrupted") || self.open_watches_on(subject)?.is_empty() {
            return Ok(());
        }
        let not_before = crate::daemon::now() + self.grace_ms();
        self.store.lock().unwrap().conn.execute("INSERT OR REPLACE INTO watch_finish_queue(subject, not_before) VALUES(?1, ?2)", rusqlite::params![subject, not_before])?;
        Ok(())
    }

    pub fn subject_turn_started(&self, subject: &str) -> Result<()> {
        self.store.lock().unwrap().conn.execute("DELETE FROM watch_finish_queue WHERE subject=?1", [subject])?;
        Ok(())
    }

    /// Subjects idle past the grace period: their one last wake, then their watches end.
    pub fn finish_due_subjects(self: &Arc<Self>) -> Result<()> {
        let now = crate::daemon::now();
        let due: Vec<String> = {
            let store = self.store.lock().unwrap();
            let mut stmt = store.conn.prepare("SELECT subject FROM watch_finish_queue WHERE not_before <= ?1")?;
            let rows = stmt.query_map([now], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
            rows
        };
        for subject in due {
            self.store.lock().unwrap().conn.execute("DELETE FROM watch_finish_queue WHERE subject=?1", [&subject])?;
            let still_idle = self.run(&subject).map(|r| !ACTIVE.contains(&r.status.as_str())).unwrap_or(false);
            if still_idle {
                self.subject_changed(&subject, "finished")?;
            }
        }
        Ok(())
    }

    fn wake(self: &Arc<Self>, w: &Watch, reason: &str, seq: i64) -> Result<()> {
        let subject = self.run(&w.subject)?;
        let n = w.wakes + 1;
        let copy = if w.mode == "check" { Some(self.refresh_copy(&w.id)?) } else { None };
        let w = self.watch(&w.id)?;
        let mut delta = self.conversation_text(&w.subject, w.last_seq, 600)?;
        delta = super::bound(&crate::redact::redact(&delta), WAKE_BYTES - 2048);
        let title = crate::redact::redact(&subject.title);
        let mut text = format!("[Watch {} on “{title}”: wake {n}, {reason}]\nYour brief: {}\n", w.id, w.brief);
        if let Some(c) = &copy {
            text.push_str(&format!("Your copy of the subject's worktree, at its latest snapshot {}, is your working directory ({c}); run the project's tests or commands there. The subject's own worktree is only read.\n", w.last_snapshot.as_deref().unwrap_or("?")));
        }
        text.push_str(&format!("What changed since your last wake (the rest on demand through your tools, agent id {}):\n{delta}\n\nFile one finding with the finding tool: fine (nothing to report; it stays silent), concern (say what and where), or stop (the subject must be stopped; say why, with the evidence). You only read; Overseer acts on your finding.", w.subject));
        let text = super::bound(&text, WAKE_BYTES);
        let watcher = match &w.watcher {
            Some(run) => {
                self.queue_message(run, &text, "watch", json!({"watch": w.id, "wake": n}))?;
                run.clone()
            }
            None => self.create_watcher(&w, &text, copy.as_deref())?,
        };
        {
            let store = self.store.lock().unwrap();
            store.conn.execute("UPDATE watches SET last_seq=?2, wakes=?3 WHERE id=?1", rusqlite::params![w.id, seq, n])?;
            store.conn.execute("INSERT INTO watch_wakes(watch_id, ts, reason, seq) VALUES(?1, ?2, ?3, ?4)", rusqlite::params![w.id, crate::daemon::now(), reason, seq])?;
        }
        self.emit(Some(&subject.task_id), Some(&w.subject), "watch_wake", "daemon", "exact", json!({"watch": w.id, "wake": n, "reason": reason, "watcher": watcher, "bytes": text.len()}))?;
        if let Ok(r) = self.run(&watcher) {
            self.emit(Some(&r.task_id), Some(&watcher), "watch_wake", "daemon", "exact", json!({"watch": w.id, "wake": n, "reason": reason, "subject": w.subject, "bytes": text.len()}))?;
        }
        Ok(())
    }

    /// A new watcher: a read-only run of its own (in a folder of the daemon's, or in its copy
    /// when the watch checks), with the watcher's tools, on the first wake.
    fn create_watcher(self: &Arc<Self>, w: &Watch, first_prompt: &str, copy: Option<&str>) -> Result<String> {
        let subject = self.run(&w.subject)?;
        let token = self.overseer_token(&format!("pending-{}", w.id), "watcher")?["token"].as_str().unwrap().to_string();
        let (dir, read_only) = match copy {
            Some(c) => (std::path::PathBuf::from(c), false),
            None => {
                let dir = crate::paths::data_dir().join("overseer").join("watch").join(&w.id);
                if !dir.join(".git").exists() {
                    crate::paths::ensure_private_dir(&dir)?;
                    std::fs::write(dir.join("README.md"), format!("# Watch {}\n\nA watcher's own folder. Nothing here is part of your projects.\n", w.id))?;
                    for args in [vec!["init", "-q", "-b", "main"], vec!["add", "."], vec!["-c", "user.name=Overseer", "-c", "user.email=overseer@localhost", "-c", "commit.gpgsign=false", "commit", "-q", "-m", "watch"]] {
                        crate::git::git(&dir, &args)?;
                    }
                }
                (dir, true)
            }
        };
        let (extra_args, mode) = self.tools_launch(&w.harness, &dir, &token, "watcher", read_only)?;
        let mut params = json!({"repo": dir.display().to_string(), "harness": w.harness, "prompt": first_prompt, "title": format!("Watching {}", subject.title), "workspace_mode": "current", "extra_args": extra_args, "role": "watcher"});
        if let Some(m) = &w.model {
            params["model"] = json!(m);
        }
        if let Some(m) = mode {
            params["permission_mode"] = json!(m);
        }
        let created = self.create_task(&params)?;
        let run_id = created["run"]["id"].as_str().unwrap_or_default().to_string();
        let subject_repo = self.workspace(&subject.workspace_id)?.repo_root;
        {
            let store = self.store.lock().unwrap();
            store.conn.execute("INSERT OR REPLACE INTO run_roles(run_id, role) VALUES(?1, 'watcher')", [&run_id])?;
            store.conn.execute("UPDATE overseer_tokens SET run_id=?1 WHERE run_id=?2", rusqlite::params![run_id, format!("pending-{}", w.id)])?;
            store.conn.execute("UPDATE watches SET watcher=?2 WHERE id=?1", rusqlite::params![w.id, run_id])?;
            if let Some(c) = copy {
                // The copy is a worktree of the subject's repository, labelled, removable by cleanup.
                let ws = created["workspace"]["id"].as_str().unwrap_or_default();
                let label = json!({"clean": true, "watch": w.id, "label": format!("watch copy of {}", subject.title)});
                store.conn.execute("UPDATE workspaces SET kind='worktree', repo_root=?2, initial_dirty=?3 WHERE id=?1", rusqlite::params![ws, subject_repo, label.to_string()])?;
                store.conn.execute("UPDATE watches SET copy_workspace=?2, copy_path=?3 WHERE id=?1", rusqlite::params![w.id, ws, c])?;
            }
            if let Some(t) = store.turns(&run_id)?.first() {
                store.conn.execute("INSERT OR REPLACE INTO turn_sources(turn_id, source, detail) VALUES(?1, 'watch', ?2)", rusqlite::params![t.id, json!({"watch": w.id, "wake": 1}).to_string()])?;
            }
        }
        self.emit(Some(&subject.task_id), Some(&w.subject), "watcher_started", "daemon", "exact", json!({"watch": w.id, "watcher": run_id}))?;
        Ok(run_id)
    }

    // ------------------------------------------------------------------ findings

    /// The finding tool: fine is recorded and silent; concern and stop go to Overseer; stop with
    /// hold on stop holds the subject at once, with no model turn in between.
    pub fn watch_finding(self: &Arc<Self>, watcher: &str, args: &Value) -> Result<String> {
        let result = args["result"].as_str().unwrap_or("");
        if !["fine", "concern", "stop"].contains(&result) {
            bail!("a finding is fine, concern or stop");
        }
        // What a watcher writes is redacted before it is stored, shown or read by Overseer (AC-200).
        let text = crate::redact::redact(args["text"].as_str().unwrap_or("").trim());
        if text.is_empty() && result != "fine" {
            bail!("a {result} finding says what you saw");
        }
        let w = self.watch_of_watcher(watcher).ok_or_else(|| anyhow!("you are not watching anyone"))?;
        let subject = self.run(&w.subject)?;
        let watcher_run = self.run(watcher)?;
        let id = format!("f-{}", &uuid::Uuid::new_v4().simple().to_string()[..12]);
        let now = crate::daemon::now();
        self.store.lock().unwrap().conn.execute("INSERT INTO findings(id, watch_id, watcher, subject, ts, result, text, snapshot) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)", rusqlite::params![id, w.id, watcher, w.subject, now, result, text, w.last_snapshot])?;
        let payload = json!({"id": id, "watch": w.id, "result": result, "text": text, "snapshot": w.last_snapshot, "watcher": watcher, "watcher_title": watcher_run.title, "subject": w.subject, "subject_title": subject.title, "hold_on_stop": w.hold_on_stop});
        self.emit(Some(&subject.task_id), Some(&w.subject), "finding", "watcher", "exact", payload.clone())?;
        self.emit(Some(&watcher_run.task_id), Some(watcher), "finding", "watcher", "exact", payload.clone())?;
        let mut held = false;
        // Held whether it is mid-turn (stopped now) or idle (its next turn waits).
        if result == "stop" && w.hold_on_stop && self.hold_of(&w.subject).is_none() {
            self.agent_hold(&w.subject, &format!("stop finding from {}: {text}", watcher_run.title), "watch", true, Value::Null, None)?;
            held = true;
        }
        if result != "fine" {
            let session = self.overseer_session()?;
            let sid = session["id"].as_str().unwrap_or_default().to_string();
            let card = json!({"kind": "finding", "id": id, "watch": w.id, "result": result, "text": text, "snapshot": w.last_snapshot, "watcher": watcher, "watcher_title": watcher_run.title, "subject": w.subject, "subject_title": subject.title, "held": held});
            self.append_session_message(&sid, "watcher", None, &format!("{} on {}: {result}{} — {text}", watcher_run.title, subject.title, if held { " (held at once)" } else { "" }), Some(&card))?;
            self.check_in_due_at(&w.subject, &format!("finding:{id}"), 0)?;
        }
        Ok(format!("Recorded{}.", if held { "; the subject is held" } else { "" }))
    }

    pub fn findings_list(&self, watch: Option<&str>, run_id: Option<&str>) -> Result<Value> {
        let store = self.store.lock().unwrap();
        let mut stmt = store.conn.prepare("SELECT id, watch_id, watcher, subject, ts, result, text, snapshot FROM findings WHERE (?1 IS NULL OR watch_id=?1) AND (?2 IS NULL OR subject=?2 OR watcher=?2) ORDER BY ts")?;
        let rows: Vec<Value> = stmt
            .query_map(rusqlite::params![watch, run_id], |r| Ok(json!({"id": r.get::<_, String>(0)?, "watch": r.get::<_, String>(1)?, "watcher": r.get::<_, String>(2)?, "subject": r.get::<_, String>(3)?, "ts": r.get::<_, i64>(4)?, "result": r.get::<_, String>(5)?, "text": r.get::<_, String>(6)?, "snapshot": r.get::<_, Option<String>>(7)?})))?
            .collect::<rusqlite::Result<_>>()?;
        Ok(json!({"findings": rows}))
    }

    /// The finding for Overseer's turn.
    pub fn finding_json(&self, id: &str) -> Option<Value> {
        let store = self.store.lock().unwrap();
        store
            .conn
            .query_row("SELECT id, watch_id, watcher, subject, ts, result, text, snapshot FROM findings WHERE id=?1", [id], |r| Ok(json!({"id": r.get::<_, String>(0)?, "watch": r.get::<_, String>(1)?, "watcher": r.get::<_, String>(2)?, "subject": r.get::<_, String>(3)?, "ts": r.get::<_, i64>(4)?, "result": r.get::<_, String>(5)?, "text": r.get::<_, String>(6)?, "snapshot": r.get::<_, Option<String>>(7)?})))
            .ok()
    }
}
