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

fn merging(ws: &Path) -> bool {
    git::rev_parse(ws, "MERGE_HEAD").is_some()
}

impl Daemon {
    /// The local branch a task merges back into: its target ref if that is a local branch, else
    /// the repository's default branch (local name), else the source checkout's current branch.
    fn merge_target(&self, repo: &Path, target_ref: Option<&str>) -> Option<String> {
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
            if uncommitted == 0 && ahead == 0 { return refuse(format!("Nothing to merge: {branch} has no changes that are not already on {target}.")); }
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
        Ok(json!({
            "ok": true, "state": state, "workspace": ws, "run_id": root.map(|r| r.id), "repo": task.repo_root, "branch": branch, "target": target,
            "worktree_uncommitted": uncommitted, "conflicts": wt.conflicted, "source_branch": source.branch, "source_dirty": source_dirty,
            "blockers": blockers, "can_complete": state == "ready" && blockers_empty(&source_dirty, &source.branch, &target) && !merging(repo),
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
            if dirty {
                let title = run_id.as_deref().and_then(|r| self.run(r).ok()).map(|r| r.title).unwrap_or_else(|| branch.clone());
                commit_worktree(path, &title)?;
            }
            // Bring the target's newer commits into the run's branch where the agent works.
            let merged = git::git(path, &["merge", "--no-ff", "--no-edit", &target]);
            if merged.is_err() {
                let st = git::status(path)?;
                if st.conflicted.is_empty() {
                    let _ = git::git(path, &["merge", "--abort"]);
                    bail!("git merge of {target} into {branch} failed: {}", merged.unwrap_err());
                }
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
        let marked: Vec<String> = files.iter().chain(st.conflicted.iter()).filter(|f| std::fs::read_to_string(path.join(f)).map(|t| has_markers(&t)).unwrap_or(false)).cloned().collect();
        if !marked.is_empty() {
            return Ok(json!({"state": "resolving", "remaining": marked}));
        }
        for f in files.iter().chain(st.conflicted.iter()) {
            git::git(path, &["add", "--", f])?;
        }
        git::git(path, &["commit", "--no-verify", "--no-edit", "-q"])?;
        let task_id = self.store.lock().unwrap().tasks()?.into_iter().find(|t| t.workspace_id == ws.id).map(|t| t.id);
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
        self.emit(task_id.as_deref(), plan["run_id"].as_str(), "merge_back", "user", "exact", json!({"state": "merged", "target": target, "branch": branch, "commit": after}))?;
        Ok(result)
    }

    pub fn merge_abort(&self, workspace_id: &str) -> Result<Value> {
        let ws = self.workspace(workspace_id)?;
        let path = Path::new(&ws.path);
        if merging(path) {
            git::git(path, &["merge", "--abort"])?;
        }
        Ok(json!({"aborted": true}))
    }
}

/// Commits all of a worktree's uncommitted work to its branch (merge back and Open PR).
pub fn commit_worktree(path: &Path, title: &str) -> Result<bool> {
    let st = git::status(path)?;
    if st.staged.is_empty() && st.unstaged.is_empty() && st.untracked.is_empty() {
        return Ok(false);
    }
    git::git(path, &["add", "-A"])?;
    git::git(path, &["commit", "--no-verify", "-q", "-m", &format!("Overseer: {title}")])?;
    Ok(true)
}

fn blockers_empty(source_dirty: &[String], source_branch: &Option<String>, target: &str) -> bool {
    source_dirty.is_empty() && source_branch.as_deref() == Some(target)
}
