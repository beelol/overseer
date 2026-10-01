//! Merge back (AC-44). Never automatic: every step is a user action.
//!
//! 1. `merge_plan`: explains whether the run's branch can be merged and why not.
//! 2. `merge_prepare`: commits the worktree's uncommitted work to the run's branch, then merges
//!    the target branch *into the run's branch inside the worktree*. Conflicts stay there, where
//!    the agent's own session works; with `handoff` they are sent to the same run as a follow-up.
//! 3. `merge_resolved`: after the agent (or the user) resolved the files, stages them and
//!    completes that merge commit in the worktree (refuses while conflict markers remain).
//! 4. The user reviews what will land (merge-base comparison against the target).
//! 5. `merge_complete`: `git merge --no-ff` of the run's branch into the target branch in the
//!    source checkout, which is conflict-free by then. Refuses if that checkout has uncommitted
//!    changes or is not on the target branch, and never touches its dirty work.
//!
//! AC-243: the plan lists the files that land (and the untracked ones that step 2 commits), Git's
//! hooks run on every commit, `merge_abort` puts the worktree back as it was before step 2, and
//! the outcome is kept (`landings`) so every surface reads "Merged into main (commit)".

use crate::daemon::{Daemon, ACTIVE};
use crate::git;
use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};
use std::path::Path;
use std::sync::Arc;

const MARKERS: [&str; 3] = ["<<<<<<< ", "=======", ">>>>>>> "];

fn has_markers(text: &str) -> bool {
    let lines: Vec<&str> = text.lines().collect();
    lines.iter().any(|l| l.starts_with(MARKERS[0])) && lines.iter().any(|l| *l == MARKERS[1]) && lines.iter().any(|l| l.starts_with(MARKERS[2]))
}

pub(crate) fn merging(ws: &Path) -> bool {
    git::rev_parse(ws, "MERGE_HEAD").is_some()
}

/// Where a worktree's merge back remembers the HEAD it started from, so a cancel can put back the
/// work it committed (AC-243). Inside the worktree's own Git folder, never in the files.
fn pre_merge_file(ws: &Path) -> Option<std::path::PathBuf> {
    git::git(ws, &["rev-parse", "--absolute-git-dir"]).ok().map(|d| Path::new(&d).join("OVERSEER_PRE_MERGE"))
}

/// The remote a pull request would go to: its name, and whether it is on GitHub (AC-232).
pub(crate) fn remote_info(ws: &Path) -> Value {
    let remotes: Vec<String> = git::git(ws, &["remote"]).unwrap_or_default().lines().map(str::to_string).collect();
    let Some(name) = remotes.iter().find(|r| *r == "origin").or(remotes.first()).cloned() else { return Value::Null };
    let url = git::git(ws, &["config", "--get", &format!("remote.{name}.url")]).unwrap_or_default();
    let github = crate::pr::github_repo(&url);
    json!({"name": name, "github": github.is_some(), "owner": github.as_ref().map(|g| g.0.clone()), "repo": github.map(|g| g.1)})
}

/// What a workspace's work became (AC-243): `merged` (into `target`, at `commit`), `conflicts`
/// (a merge back stopped in the worktree) or `pr` (a pull request at `url`).
pub(crate) fn set_landing(conn: &rusqlite::Connection, ws: &str, state: &str, target: Option<&str>, branch: Option<&str>, commit: Option<&str>, url: Option<&str>, files: &[String]) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO landings(workspace_id,state,target,branch,commit_sha,url,files,ms) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
        rusqlite::params![ws, state, target, branch, commit, url, serde_json::to_string(files)?, crate::daemon::now()],
    )?;
    Ok(())
}

pub(crate) fn clear_landing(conn: &rusqlite::Connection, ws: &str) -> Result<()> {
    conn.execute("DELETE FROM landings WHERE workspace_id=?1", [ws])?;
    Ok(())
}

pub(crate) fn landing(conn: &rusqlite::Connection, ws: &str) -> Value {
    all_landings(conn).ok().and_then(|mut m| m.remove(ws)).unwrap_or(Value::Null)
}

pub(crate) fn all_landings(conn: &rusqlite::Connection) -> Result<serde_json::Map<String, Value>> {
    let mut stmt = conn.prepare("SELECT workspace_id,state,target,branch,commit_sha,url,files,ms FROM landings")?;
    let rows = stmt.query_map([], |r| {
        let files: Option<String> = r.get(6)?;
        Ok((r.get::<_, String>(0)?, json!({
            "state": r.get::<_, String>(1)?, "target": r.get::<_, Option<String>>(2)?, "branch": r.get::<_, Option<String>>(3)?,
            "commit": r.get::<_, Option<String>>(4)?, "url": r.get::<_, Option<String>>(5)?,
            "files": files.and_then(|f| serde_json::from_str::<Value>(&f).ok()).unwrap_or(json!([])), "ms": r.get::<_, i64>(7)?,
        })))
    })?;
    Ok(rows.collect::<rusqlite::Result<serde_json::Map<String, Value>>>()?)
}

/// The landings the state shows: a merge the agent has worked past (a turn started after it) no
/// longer reads "Merged".
pub(crate) fn landings_for_state(conn: &rusqlite::Connection, runs: &[crate::store::Run], turns: &serde_json::Map<String, Value>) -> Result<Value> {
    let mut out = all_landings(conn)?;
    out.retain(|ws, l| {
        if l["state"] != "merged" {
            return true;
        }
        let at = l["ms"].as_i64().unwrap_or(0);
        !runs.iter().filter(|r| &r.workspace_id == ws).any(|r| turns.get(&r.id).and_then(Value::as_array).is_some_and(|t| t.iter().any(|t| t["started_ms"].as_i64().unwrap_or(0) > at)))
    });
    Ok(Value::Object(out))
}

/// While a merge is in progress in the worktree: the files still conflicted, and the changed
/// files that still hold conflict markers (a file staged as resolved with its markers in it).
pub(crate) fn unresolved(ws: &Path) -> Result<Vec<String>> {
    if !merging(ws) {
        return Ok(Vec::new());
    }
    let st = git::status(ws)?;
    let mut out: Vec<String> = st.conflicted.clone();
    let changed = git::git(ws, &["diff", "--name-only", "HEAD"])?;
    for f in changed.lines().map(str::to_string).chain(st.untracked.iter().cloned()) {
        if !out.contains(&f) && std::fs::read_to_string(ws.join(&f)).map(|t| has_markers(&t)).unwrap_or(false) {
            out.push(f);
        }
    }
    Ok(out)
}

impl Daemon {
    /// The local branch a task merges back into: its target ref if that is a local branch, else
    /// the repository's default branch (local name), else the source checkout's current branch.
    pub(crate) fn merge_target(&self, repo: &Path, target_ref: Option<&str>) -> Option<String> {
        let local = |b: &str| git::git(repo, &["show-ref", "--verify", "--quiet", &format!("refs/heads/{b}")]).is_ok();
        if let Some(t) = target_ref.filter(|t| local(t)) {
            return Some(t.to_string());
        }
        if let Some(d) = git::default_branch(repo) {
            let name = d.strip_prefix("origin/").unwrap_or(&d).to_string();
            if local(&name) {
                return Some(name);
            }
        }
        git::head_branch(repo)
    }

    pub fn merge_plan(&self, workspace_id: &str) -> Result<Value> {
        let ws = self.workspace(workspace_id)?;
        let refuse = |reason: String| Ok(json!({"ok": false, "reason": reason, "workspace": ws}));
        if ws.kind != "worktree" {
            return refuse("This task works directly in the current checkout, so there is no separate branch to merge back.".into());
        }
        if ws.removed_ms.is_some() {
            return refuse("This task's worktree was removed.".into());
        }
        let (runs, task) = {
            let store = self.store.lock().unwrap();
            let runs: Vec<_> = store.runs()?.into_iter().filter(|r| r.workspace_id == ws.id).collect();
            let task = store.tasks()?.into_iter().find(|t| t.workspace_id == ws.id).ok_or_else(|| anyhow!("no task for this workspace"))?;
            (runs, task)
        };
        let root = runs.iter().find(|r| r.parent_run_id.is_none()).cloned();
        if let Some(active) = runs.iter().find(|r| ACTIVE.contains(&r.status.as_str())) {
            return refuse(format!("The run is still {} — wait for it to finish or interrupt it before merging back.", active.status.replace('_', " ")));
        }
        let path = Path::new(&ws.path);
        let repo = Path::new(&task.repo_root);
        let branch = ws.branch.clone().ok_or_else(|| anyhow!("worktree has no branch"))?;
        let Some(target) = self.merge_target(repo, task.target_ref.as_deref()) else {
            return refuse("No target branch: the task has no local start branch and the repository has no default branch.".into());
        };
        let source = git::status(repo)?;
        let wt = git::status(path)?;
        let state = if merging(path) {
            if wt.conflicted.is_empty() { "resolved" } else { "resolving" }
        } else {
            let ahead = git::git(path, &["rev-list", "--count", &format!("{target}..HEAD")]).ok().and_then(|n| n.parse::<u64>().ok()).unwrap_or(0);
            let behind = git::git(path, &["rev-list", "--count", &format!("HEAD..{target}")]).ok().and_then(|n| n.parse::<u64>().ok()).unwrap_or(0);
            let uncommitted = wt.staged.len() + wt.unstaged.len() + wt.untracked.len();
            if uncommitted == 0 && ahead == 0 {
                let landed = landing(&self.store.lock().unwrap().conn, &ws.id);
                return Ok(json!({"ok": false, "reason": format!("Nothing to merge: {branch} has no changes that are not already on {target}."), "workspace": ws, "landing": landed, "remote": remote_info(path)}));
            }
            if uncommitted == 0 && behind == 0 { "ready" } else { "idle" }
        };
        let source_dirty: Vec<String> = source.staged.iter().chain(source.unstaged.iter()).map(|c| c.path.clone()).chain(source.conflicted.iter().cloned()).collect();
        let mut blockers = Vec::new();
        if source.branch.as_deref() != Some(target.as_str()) {
            blockers.push(format!("The source checkout {} is on {} — switch it to {target} to merge back.", task.repo_root, source.branch.clone().unwrap_or_else(|| "a detached HEAD".into())));
        }
        if !source_dirty.is_empty() {
            blockers.push(format!("The source checkout {} has uncommitted changes ({}). Overseer never disturbs them; commit or stash them first.", task.repo_root, source_dirty.iter().take(5).cloned().collect::<Vec<_>>().join(", ")));
        }
        if merging(repo) {
            blockers.push(format!("A merge is already in progress in {}.", task.repo_root));
        }
        let uncommitted: Vec<String> = wt.staged.iter().chain(wt.unstaged.iter()).map(|c| c.path.clone()).chain(wt.untracked.iter().cloned()).collect();
        // What lands on the target: the branch's committed work and the worktree's uncommitted
        // files, against where the branch left the target (AC-243's confirmation lists them).
        let files: Vec<git::Change> = match git::merge_base(path, &target, "HEAD") {
            Some(base) => git::capture_trees(path, &crate::paths::data_dir().join("tmp")).and_then(|t| git::diff_trees(path, &base, &t.worktree_tree)).unwrap_or_default(),
            None => Vec::new(),
        };
        let landed = {
            let store = self.store.lock().unwrap();
            // A merge back stopped on conflicts that is no longer in progress (finished or cancelled in Git).
            if !merging(path) && landing(&store.conn, &ws.id)["state"] == "conflicts" {
                clear_landing(&store.conn, &ws.id)?;
            }
            landing(&store.conn, &ws.id)
        };
        Ok(json!({
            "ok": true, "state": state, "workspace": ws, "run_id": root.map(|r| r.id), "repo": task.repo_root, "branch": branch, "target": target,
            "worktree_uncommitted": uncommitted, "untracked": wt.untracked, "files": files, "conflicts": wt.conflicted, "source_branch": source.branch, "source_dirty": source_dirty,
            "blockers": blockers, "can_complete": state == "ready" && blockers_empty(&source_dirty, &source.branch, &target) && !merging(repo),
            "remote": remote_info(path), "landing": landed,
        }))
    }

    /// Commit the worktree's work and merge the target into the run's branch (in the worktree).
    pub fn merge_prepare(self: &Arc<Self>, workspace_id: &str, handoff: bool) -> Result<Value> {
        let plan = self.merge_plan(workspace_id)?;
        if plan["ok"] != true {
            bail!("{}", plan["reason"].as_str().unwrap_or("cannot merge back"));
        }
        let ws = self.workspace(workspace_id)?;
        let path = Path::new(&ws.path);
        let target = plan["target"].as_str().unwrap_or_default().to_string();
        let branch = plan["branch"].as_str().unwrap_or_default().to_string();
        let run_id = plan["run_id"].as_str().map(str::to_string);
        let task_id = self.store.lock().unwrap().tasks()?.into_iter().find(|t| t.workspace_id == ws.id).map(|t| t.id);
        if plan["state"] == "idle" {
            let dirty = plan["worktree_uncommitted"].as_array().map(|a| !a.is_empty()).unwrap_or(false);
            let before = git::head(path).unwrap_or_default();
            if dirty {
                let title = run_id.as_deref().and_then(|r| self.run(r).ok()).map(|r| r.title).unwrap_or_else(|| branch.clone());
                commit_worktree(path, &title)?;
            }
            // A cancel puts the worktree back here: the HEAD before, and whether Overseer committed on top of it.
            if let Some(f) = pre_merge_file(path) {
                let _ = std::fs::write(f, format!("{before} {}", dirty));
            }
            // Bring the target's newer commits into the run's branch where the agent works.
            let merged = git::git(path, &["merge", "--no-ff", "--no-edit", &target]);
            if merged.is_err() {
                let st = git::status(path)?;
                if st.conflicted.is_empty() {
                    let _ = git::git(path, &["merge", "--abort"]);
                    bail!("git merge of {target} into {branch} failed: {}", merged.unwrap_err());
                }
                set_landing(&self.store.lock().unwrap().conn, &ws.id, "conflicts", Some(&target), Some(&branch), None, None, &st.conflicted)?;
                self.emit(task_id.as_deref(), run_id.as_deref(), "merge_back", "daemon", "exact", json!({"state": "conflicts", "target": target, "branch": branch, "files": st.conflicted}))?;
                let mut out = json!({"state": "conflicts", "files": st.conflicted, "target": target, "branch": branch, "handoff": Value::Null});
                if handoff {
                    out["handoff"] = self.handoff_conflicts(run_id.as_deref(), &target, &branch, &st.conflicted);
                }
                return Ok(out);
            }
        } else if plan["state"] == "resolving" {
            bail!("Conflicts are still being resolved in the worktree: {}", plan["conflicts"]);
        } else if plan["state"] == "resolved" {
            return self.merge_resolved(workspace_id);
        }
        self.emit(task_id.as_deref(), run_id.as_deref(), "merge_back", "daemon", "exact", json!({"state": "ready", "target": target, "branch": branch}))?;
        Ok(json!({"state": "ready", "target": target, "branch": branch}))
    }

    fn handoff_conflicts(self: &Arc<Self>, run_id: Option<&str>, target: &str, branch: &str, files: &[String]) -> Value {
        let Some(run_id) = run_id else { return json!({"sent": false, "why": "no run"}) };
        let run = match self.run(run_id) { Ok(r) => r, Err(e) => return json!({"sent": false, "why": e.to_string()}) };
        if run.harness == "generic" {
            return json!({"sent": false, "why": "the generic harness cannot take follow-ups; resolve the files yourself"});
        }
        let prompt = format!(
            "Overseer is merging {target} into your branch {branch} so your work can be merged back. Git reported conflicts in: {}. \
             Resolve them in this working directory: edit each file to combine both sides correctly and remove every conflict marker \
             (<<<<<<<, =======, >>>>>>>). Only edit those files. Do not run git commands and do not commit; Overseer finishes the merge. \
             Then reply with one short line saying what you kept.",
            files.join(", ")
        );
        match self.start_turn(run_id, &prompt, true, &crate::daemon::TurnOpts::default()) {
            Ok(turn) => json!({"sent": true, "run_id": run_id, "turn": turn}),
            Err(e) => json!({"sent": false, "why": e.to_string()}),
        }
    }

    /// Stage the resolved files and finish the merge commit in the worktree.
    pub fn merge_resolved(&self, workspace_id: &str) -> Result<Value> {
        let ws = self.workspace(workspace_id)?;
        let path = Path::new(&ws.path);
        if !merging(path) {
            bail!("No merge is in progress in this worktree.");
        }
        let st = git::status(path)?;
        let files: Vec<String> = git::git(path, &["diff", "--name-only", "--diff-filter=U"])?.lines().map(str::to_string).collect();
        // Every file the resolution touched: the conflicted ones, and any changed since (a file
        // staged with its markers and fixed afterwards must be staged again, never committed as staged).
        let mut touched: Vec<String> = files.iter().chain(st.conflicted.iter()).cloned().collect();
        for f in git::git(path, &["diff", "--name-only", "HEAD"])?.lines().map(str::to_string).chain(st.untracked.iter().cloned()) {
            if !touched.contains(&f) {
                touched.push(f);
            }
        }
        let marked: Vec<String> = touched.iter().filter(|f| std::fs::read_to_string(path.join(f)).map(|t| has_markers(&t)).unwrap_or(false)).cloned().collect();
        if !marked.is_empty() {
            return Ok(json!({"state": "resolving", "remaining": marked}));
        }
        // The agent's own work was committed before the merge began, so what changed since is the resolution.
        git::git(path, &["add", "-A"])?;
        // Git's hooks run (AC-243): a refused commit leaves the merge in progress, with the hook's words.
        git::git(path, &["commit", "--no-edit", "-q"]).map_err(|e| anyhow!("The merge commit was refused (a Git hook may have stopped it): {e}"))?;
        let task_id = self.store.lock().unwrap().tasks()?.into_iter().find(|t| t.workspace_id == ws.id).map(|t| t.id);
        clear_landing(&self.store.lock().unwrap().conn, &ws.id)?;
        self.emit(task_id.as_deref(), ws.owner_run_id.as_deref(), "merge_back", "daemon", "exact", json!({"state": "ready", "resolved": files}))?;
        Ok(json!({"state": "ready", "resolved": files}))
    }

    /// Merge the run's branch into the target branch in the source checkout.
    pub fn merge_complete(&self, workspace_id: &str) -> Result<Value> {
        let plan = self.merge_plan(workspace_id)?;
        if plan["ok"] != true {
            bail!("{}", plan["reason"].as_str().unwrap_or("cannot merge back"));
        }
        if plan["state"] != "ready" {
            bail!("Prepare the merge first (state {}).", plan["state"]);
        }
        if let Some(b) = plan["blockers"].as_array().filter(|b| !b.is_empty()) {
            bail!("{}", b.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(" "));
        }
        let repo = plan["repo"].as_str().unwrap_or_default().to_string();
        let branch = plan["branch"].as_str().unwrap_or_default().to_string();
        let target = plan["target"].as_str().unwrap_or_default().to_string();
        let before = git::head(Path::new(&repo));
        let msg = format!("Merge {branch} into {target} (Overseer merge back)");
        if let Err(e) = git::git(Path::new(&repo), &["merge", "--no-ff", "-m", &msg, &branch]) {
            // Should not happen (the target was merged into the branch first); leave nothing half-done.
            let _ = git::git(Path::new(&repo), &["merge", "--abort"]);
            bail!("git merge into {target} failed and was aborted: {e}");
        }
        let after = git::head(Path::new(&repo));
        let ws = self.workspace(workspace_id)?;
        let task_id = self.store.lock().unwrap().tasks()?.into_iter().find(|t| t.workspace_id == ws.id).map(|t| t.id);
        let result = json!({"merged": true, "repo": repo, "target": target, "branch": branch, "before": before, "commit": after});
        let files: Vec<String> = plan["files"].as_array().map(|a| a.iter().filter_map(|f| f["path"].as_str().map(str::to_string)).collect()).unwrap_or_default();
        set_landing(&self.store.lock().unwrap().conn, &ws.id, "merged", Some(&target), Some(&branch), after.as_deref(), None, &files)?;
        if let Some(f) = pre_merge_file(Path::new(&ws.path)) {
            let _ = std::fs::remove_file(f);
        }
        self.emit(task_id.as_deref(), plan["run_id"].as_str(), "merge_back", "user", "exact", json!({"state": "merged", "target": target, "branch": branch, "commit": after}))?;
        // Merged work has been looked at: it leaves "to review" on every surface (T-26).
        let at = crate::daemon::now();
        let seen: serde_json::Map<String, Value> = self.store.lock().unwrap().runs()?.into_iter()
            .filter(|r| r.workspace_id == ws.id && r.parent_run_id.is_none()).map(|r| (r.id, json!(at))).collect();
        crate::menubar::mark_seen(self, &seen)?;
        Ok(result)
    }

    /// Cancels a merge back that stopped in the worktree (AC-243): aborts Git's merge and, when
    /// Overseer committed the worktree's work for it, un-commits that work again (a mixed reset to
    /// the HEAD it started from), so the worktree reads as it did before the merge.
    pub fn merge_abort(&self, workspace_id: &str) -> Result<Value> {
        let ws = self.workspace(workspace_id)?;
        let path = Path::new(&ws.path);
        let was_merging = merging(path);
        if was_merging {
            git::git(path, &["merge", "--abort"])?;
        }
        let mut restored = false;
        if let Some(f) = pre_merge_file(path) {
            let saved = std::fs::read_to_string(&f).unwrap_or_default();
            let mut parts = saved.split_whitespace();
            if let (Some(before), Some("true")) = (parts.next(), parts.next()) {
                // Only the commit Overseer made, directly on top of where it started.
                if was_merging && git::rev_parse(path, "HEAD~1").as_deref() == Some(before) {
                    git::git(path, &["reset", "-q", "--mixed", before])?;
                    restored = true;
                }
            }
            let _ = std::fs::remove_file(f);
        }
        clear_landing(&self.store.lock().unwrap().conn, &ws.id)?;
        if was_merging {
            let (task_id, run_id) = {
                let store = self.store.lock().unwrap();
                let task = store.tasks()?.into_iter().find(|t| t.workspace_id == ws.id).map(|t| t.id);
                let run = store.runs()?.into_iter().find(|r| r.workspace_id == ws.id && r.parent_run_id.is_none()).map(|r| r.id);
                (task, run)
            };
            self.emit(task_id.as_deref(), run_id.as_deref(), "merge_back", "user", "exact", json!({"state": "cancelled", "uncommitted": restored}))?;
        }
        Ok(json!({"aborted": true, "was_merging": was_merging, "uncommitted": restored}))
    }
}

/// Commits all of a worktree's uncommitted work to its branch (merge back and Open PR).
pub fn commit_worktree(path: &Path, title: &str) -> Result<bool> {
    // Never commit a merge that stopped on conflicts: `add -A` would stage the markers.
    let open = unresolved(path)?;
    if !open.is_empty() {
        bail!("A merge in this worktree is unfinished: {} still {} conflicts. Resolve them or cancel the merge first.", open.join(", "), if open.len() == 1 { "has" } else { "have" });
    }
    let st = git::status(path)?;
    if st.staged.is_empty() && st.unstaged.is_empty() && st.untracked.is_empty() {
        return Ok(false);
    }
    git::git(path, &["add", "-A"])?;
    // Git's hooks run (AC-243): a secret-scanning pre-commit hook can refuse the commit, and then
    // the files stay uncommitted (unstaged again) with the hook's words in the error.
    if let Err(e) = git::git(path, &["commit", "-q", "-m", &format!("Overseer: {title}")]) {
        let _ = git::git(path, &["reset", "-q"]);
        bail!("Committing the agent's work was refused (a Git hook may have stopped it): {e}");
    }
    Ok(true)
}

fn blockers_empty(source_dirty: &[String], source_branch: &Option<String>, target: &str) -> bool {
    source_dirty.is_empty() && source_branch.as_deref() == Some(target)
}
