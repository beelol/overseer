//! Open a pull request from a run (AC-50). The daemon plans and commits; the VS Code extension
//! pushes and creates the PR with VS Code's own GitHub sign-in, so no token ever reaches the
//! daemon. Never automatic, and nothing is merged.

use crate::daemon::{Daemon, ACTIVE};
use crate::git;
use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};
use std::path::Path;

/// `owner/repo` from a GitHub remote URL (https, ssh or scp-like), or None for other hosts.
pub fn github_repo(url: &str) -> Option<(String, String)> {
    let rest = url
        .strip_prefix("https://github.com/")
        .or_else(|| url.strip_prefix("http://github.com/"))
        .or_else(|| url.strip_prefix("git@github.com:"))
        .or_else(|| url.strip_prefix("ssh://git@github.com/"))?;
    let rest = rest.trim_end_matches('/').trim_end_matches(".git");
    let mut parts = rest.splitn(2, '/');
    let owner = parts.next()?.to_string();
    let repo = parts.next()?.to_string();
    (!owner.is_empty() && !repo.is_empty() && !repo.contains('/')).then_some((owner, repo))
}

impl Daemon {
    pub fn pr_plan(&self, workspace_id: &str) -> Result<Value> {
        let ws = self.workspace(workspace_id)?;
        let refuse = |reason: String| Ok(json!({"ok": false, "reason": reason}));
        if ws.kind != "worktree" {
            return refuse("This task works directly in the current checkout, so there is no separate branch to open a pull request from.".into());
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
        if let Some(active) = runs.iter().find(|r| ACTIVE.contains(&r.status.as_str())) {
            return refuse(format!("The run is still {} — wait for it to finish or interrupt it before opening a pull request.", active.status.replace('_', " ")));
        }
        let path = Path::new(&ws.path);
        let branch = ws.branch.clone().ok_or_else(|| anyhow!("worktree has no branch"))?;
        let remotes: Vec<String> = git::git(path, &["remote"])?.lines().map(str::to_string).collect();
        let Some(remote) = remotes.iter().find(|r| *r == "origin").or(remotes.first()).cloned() else {
            return refuse(format!("The repository {} has no Git remote. Add a GitHub remote (git remote add origin https://github.com/OWNER/REPO.git) to open pull requests.", task.repo_root));
        };
        // The configured URL (not `get-url`, which applies insteadOf rewrites).
        let url = git::git(path, &["config", "--get", &format!("remote.{remote}.url")])?;
        let Some((owner, repo)) = github_repo(&url) else {
            return refuse(format!("The remote {remote} ({url}) is not on GitHub; Open PR only supports github.com remotes."));
        };
        let target = task.target_ref.clone().filter(|t| !t.is_empty() && !t.contains("..")).and_then(|t| {
            let local = git::git(Path::new(&task.repo_root), &["show-ref", "--verify", "--quiet", &format!("refs/heads/{t}")]).is_ok();
            local.then_some(t)
        }).or_else(|| git::default_branch(Path::new(&task.repo_root)).map(|d| d.strip_prefix(&format!("{remote}/")).unwrap_or(&d).strip_prefix("origin/").unwrap_or(&d).to_string()))
          .or_else(|| git::head_branch(Path::new(&task.repo_root)))
          .ok_or_else(|| anyhow!("no target branch"))?;
        let st = git::status(path)?;
        let uncommitted: Vec<String> = st.staged.iter().chain(st.unstaged.iter()).map(|c| c.path.clone()).chain(st.untracked.iter().cloned()).collect();
        let base = git::merge_base(path, "HEAD", &target);
        let commits: Vec<String> = base.as_ref().map(|b| git::git(path, &["log", "--format=%s", &format!("{b}..HEAD")]).unwrap_or_default().lines().map(str::to_string).collect()).unwrap_or_default();
        if uncommitted.is_empty() && commits.is_empty() {
            return refuse(format!("Nothing to propose: {branch} has no changes that are not already on {target}."));
        }
        let root = runs.iter().find(|r| r.parent_run_id.is_none());
        Ok(json!({
            "ok": true, "workspace": ws, "run_id": root.map(|r| r.id.clone()), "title": root.map(|r| r.title.clone()).unwrap_or_else(|| task.title.clone()),
            "prompt": task.prompt, "harness": root.map(|r| r.harness.clone()), "model": root.and_then(|r| r.model.clone()),
            "remote": remote, "remote_url": url, "owner": owner, "repo": repo, "branch": branch, "target": target,
            "uncommitted": uncommitted, "commits": commits,
        }))
    }

    /// Commits the worktree's uncommitted work to the run's branch; returns the head to push.
    pub fn pr_prepare(&self, workspace_id: &str) -> Result<Value> {
        let plan = self.pr_plan(workspace_id)?;
        if plan["ok"] != true {
            bail!("{}", plan["reason"].as_str().unwrap_or("cannot open a pull request"));
        }
        let ws = self.workspace(workspace_id)?;
        let path = Path::new(&ws.path);
        let committed = crate::merge::commit_worktree(path, plan["title"].as_str().unwrap_or("Overseer run"))?;
        let head = git::head(path).ok_or_else(|| anyhow!("no HEAD"))?;
        let base = git::merge_base(path, "HEAD", plan["target"].as_str().unwrap_or("HEAD"));
        let files: Vec<Value> = match &base { Some(b) => serde_json::to_value(git::diff_trees(path, b, &head)?)?.as_array().cloned().unwrap_or_default(), None => vec![] };
        let commits: Vec<String> = base.as_ref().map(|b| git::git(path, &["log", "--format=%s", &format!("{b}..HEAD")]).unwrap_or_default().lines().map(str::to_string).collect()).unwrap_or_default();
        Ok(json!({"plan": plan, "committed": committed, "head": head, "files": files, "commits": commits}))
    }

    /// Records the PR the extension created (URL and number only).
    pub fn pr_opened(&self, workspace_id: &str, url: &str, number: i64) -> Result<Value> {
        let ws = self.workspace(workspace_id)?;
        let (task_id, run_id) = {
            let store = self.store.lock().unwrap();
            let task = store.tasks()?.into_iter().find(|t| t.workspace_id == ws.id).map(|t| t.id);
            let run = store.runs()?.into_iter().find(|r| r.workspace_id == ws.id && r.parent_run_id.is_none()).map(|r| r.id);
            (task, run)
        };
        if !url.starts_with("https://") && !url.starts_with("http://127.0.0.1") && !url.starts_with("http://localhost") {
            bail!("not a pull request URL");
        }
        self.emit(task_id.as_deref(), run_id.as_deref(), "pull_request", "user", "exact", json!({"url": url, "number": number, "branch": ws.branch}))?;
        Ok(json!({"recorded": true}))
    }
}

#[cfg(test)]
mod tests {
    use super::github_repo;
    #[test]
    fn parses_github_remotes_only() {
        let ok = |u: &str| github_repo(u).map(|(o, r)| format!("{o}/{r}"));
        assert_eq!(ok("https://github.com/beelol/overseer.git").as_deref(), Some("beelol/overseer"));
        assert_eq!(ok("https://github.com/beelol/overseer").as_deref(), Some("beelol/overseer"));
        assert_eq!(ok("git@github.com:beelol/overseer.git").as_deref(), Some("beelol/overseer"));
        assert_eq!(ok("ssh://git@github.com/beelol/overseer.git").as_deref(), Some("beelol/overseer"));
        assert_eq!(ok("https://gitlab.com/a/b.git"), None);
        assert_eq!(ok("/tmp/bare.git"), None);
        assert_eq!(ok("https://github.com/only-owner"), None);
    }
}
