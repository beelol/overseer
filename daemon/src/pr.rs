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

/// A remote's address without a user name or token written into it.
pub fn without_userinfo(url: &str) -> String {
    match url.split_once("://") {
        Some((scheme, rest)) => {
            let (authority, path) = rest.split_once('/').map(|(a, p)| (a, format!("/{p}"))).unwrap_or((rest, String::new()));
            let host = authority.rsplit_once('@').map(|(_, h)| h).unwrap_or(authority);
            format!("{scheme}://{host}{path}")
        }
        None => url.to_string(),
    }
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
            // AC-232: the surfaces offer the local merge (or Publish to GitHub) instead, by `why`.
            return Ok(json!({"ok": false, "why": "no_remote", "reason": format!("The repository {} has no Git remote, so there is no pull request to open. Merge the work into its branch here instead, or publish the repository to GitHub first.", task.repo_root)}));
        };
        // The configured URL (not `get-url`, which applies insteadOf rewrites).
        let url = git::git(path, &["config", "--get", &format!("remote.{remote}.url")])?;
        let Some((owner, repo)) = github_repo(&url) else {
            return Ok(json!({"ok": false, "why": "not_github", "reason": format!("The remote {remote} ({}) is not on GitHub; Open PR only supports github.com remotes.", without_userinfo(&url))}));
        };
        let target = task.target_ref.clone().filter(|t| !t.is_empty() && !t.contains("..")).and_then(|t| {
            let local = git::git(Path::new(&task.repo_root), &["show-ref", "--verify", "--quiet", &format!("refs/heads/{t}")]).is_ok();
            local.then_some(t)
        }).or_else(|| git::default_branch(Path::new(&task.repo_root)).map(|d| {
            // A clone's default is the remote-tracking `origin/master`; GitHub wants the branch name.
            d.strip_prefix(&format!("{remote}/")).or_else(|| d.strip_prefix("origin/")).unwrap_or(&d).to_string()
        }))
          .or_else(|| git::head_branch(Path::new(&task.repo_root)))
          .ok_or_else(|| anyhow!("no target branch"))?;
        // What to compare against locally: the branch if it exists here, else its remote-tracking ref.
        let base_ref = if git::git(Path::new(&task.repo_root), &["show-ref", "--verify", "--quiet", &format!("refs/heads/{target}")]).is_ok() { target.clone() } else { format!("{remote}/{target}") };
        // A merge back that stopped on conflicts leaves the worktree mid-merge; committing that
        // would publish the conflict markers.
        let open = crate::merge::unresolved(path)?;
        if !open.is_empty() {
            return refuse(format!("Merging {target} into {branch} is unfinished: {} still {} conflicts. Resolve them, or cancel the merge back, before opening a pull request.", open.join(", "), if open.len() == 1 { "has" } else { "have" }));
        }
        // Resolved but not finished is still the middle of a merge (AC-243): finish or cancel it first.
        if crate::merge::merging(path) {
            return refuse(format!("Merging {target} into {branch} is still in progress in the worktree. Finish the merge, or cancel it, before opening a pull request."));
        }
        let st = git::status(path)?;
        let uncommitted: Vec<String> = st.staged.iter().chain(st.unstaged.iter()).map(|c| c.path.clone()).chain(st.untracked.iter().cloned()).collect();
        let base = git::merge_base(path, "HEAD", &base_ref);
        let commits: Vec<String> = base.as_ref().map(|b| git::git(path, &["log", "--format=%s", &format!("{b}..HEAD")]).unwrap_or_default().lines().map(str::to_string).collect()).unwrap_or_default();
        if uncommitted.is_empty() && commits.is_empty() {
            return refuse(format!("Nothing to propose: {branch} has no changes that are not already on {target}."));
        }
        let root = runs.iter().find(|r| r.parent_run_id.is_none());
        Ok(json!({
            "ok": true, "workspace": ws, "run_id": root.map(|r| r.id.clone()), "title": root.map(|r| r.title.clone()).unwrap_or_else(|| task.title.clone()),
            "prompt": task.prompt, "harness": root.map(|r| r.harness.clone()), "model": root.and_then(|r| r.model.clone()),
            "remote": remote, "remote_url": without_userinfo(&url), "owner": owner, "repo": repo, "branch": branch, "target": target, "base_ref": base_ref,
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
        let base = git::merge_base(path, "HEAD", plan["base_ref"].as_str().unwrap_or("HEAD"));
        let files: Vec<Value> = match &base { Some(b) => serde_json::to_value(git::diff_trees(path, b, &head)?)?.as_array().cloned().unwrap_or_default(), None => vec![] };
        let commits: Vec<String> = base.as_ref().map(|b| git::git(path, &["log", "--format=%s", &format!("{b}..HEAD")]).unwrap_or_default().lines().map(str::to_string).collect()).unwrap_or_default();
        Ok(json!({"plan": plan, "committed": committed, "head": head, "files": files, "commits": commits}))
    }

    /// Opens the pull request from the daemon (Gate N): commits, pushes the branch with the
    /// owner's own Git credentials and creates it with their GitHub CLI. The daemon starts `git`
    /// and `gh`; it reads no token, and none is sent to the phone that asked. Reuses an open
    /// pull request for the branch. Nothing is merged.
    pub fn pr_open(&self, workspace_id: &str, p: &Value) -> Result<Value> {
        let prepared = self.pr_prepare(workspace_id)?;
        let plan = &prepared["plan"];
        let text = |k: &str| plan[k].as_str().unwrap_or_default().to_string();
        let ws = self.workspace(workspace_id)?;
        let (remote, branch, target, repo) = (text("remote"), text("branch"), text("target"), format!("{}/{}", text("owner"), text("repo")));
        let title: String = p["title"].as_str().map(str::trim).filter(|t| !t.is_empty()).map(str::to_string).unwrap_or_else(|| text("title")).chars().take(200).collect();
        let body = match p["body"].as_str().filter(|b| !b.trim().is_empty()) {
            Some(b) => b.chars().take(20_000).collect::<String>(),
            None => pr_body(&prepared),
        };
        let push = std::process::Command::new("git").args(["-C", &ws.path, "push", &remote, &format!("HEAD:refs/heads/{branch}")]).env("GIT_TERMINAL_PROMPT", "0").output()?;
        if !push.status.success() {
            bail!("git push failed: {}", crate::redact::redact(String::from_utf8_lossy(&push.stderr).trim()));
        }
        let gh = std::env::var("OVERSEER_GH").unwrap_or_else(|_| "gh".into());
        let mut args = vec!["pr", "create", "--repo", &repo, "--head", &branch, "--base", &target, "--title", &title, "--body", &body];
        if p["draft"].as_bool() == Some(true) {
            args.push("--draft");
        }
        let created = std::process::Command::new(&gh).args(&args).current_dir(&ws.path).output().map_err(|e| {
            crate::server::ProtoError::new("mac_setup", format!("The GitHub CLI (gh) was not found on the Mac ({e}). Install it and run gh auth login there, or open the pull request in VS Code."))
        })?;
        let out = String::from_utf8_lossy(&created.stdout).to_string();
        let err = String::from_utf8_lossy(&created.stderr).to_string();
        let (url, reused) = if created.status.success() {
            (out.lines().rev().find(|l| l.starts_with("https://")).map(str::to_string), false)
        } else if err.contains("already exists") {
            (err.lines().chain(out.lines()).find_map(|l| l.split_whitespace().find(|w| w.starts_with("https://")).map(str::to_string)), true)
        } else if err.contains("gh auth login") || err.contains("not logged") {
            return Err(crate::server::ProtoError::new("mac_setup", "The GitHub CLI on the Mac is not signed in. Run gh auth login there.").into());
        } else {
            bail!("gh pr create failed: {}", crate::redact::redact(err.trim()));
        };
        let url = url.ok_or_else(|| anyhow!("gh did not report the pull request's address"))?;
        let number = url.rsplit('/').next().and_then(|n| n.parse::<i64>().ok()).unwrap_or(0);
        self.pr_opened(workspace_id, &url, number)?;
        Ok(json!({"url": url, "number": number, "reused": reused, "branch": branch, "target": target, "repo": repo, "committed": prepared["committed"], "head": prepared["head"]}))
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
        crate::merge::set_landing(&self.store.lock().unwrap().conn, &ws.id, "pr", None, ws.branch.as_deref(), None, Some(url), &[])?;
        self.emit(task_id.as_deref(), run_id.as_deref(), "pull_request", "user", "exact", json!({"url": url, "number": number, "branch": ws.branch}))?;
        Ok(json!({"recorded": true}))
    }
}

/// What a pull request says when the owner wrote nothing: the task, the commits and the files.
fn pr_body(prepared: &Value) -> String {
    let plan = &prepared["plan"];
    let mut out = String::new();
    let prompt = plan["prompt"].as_str().unwrap_or_default().trim();
    if !prompt.is_empty() {
        out.push_str("**Task**\n\n");
        for line in prompt.lines().take(40) {
            out.push_str(&format!("> {line}\n"));
        }
        out.push('\n');
    }
    if let Some(h) = plan["harness"].as_str() {
        out.push_str(&format!("Agent: {h}{}\n\n", plan["model"].as_str().map(|m| format!(" ({m})")).unwrap_or_default()));
    }
    let commits = prepared["commits"].as_array().cloned().unwrap_or_default();
    if !commits.is_empty() {
        out.push_str(&format!("**Commits** ({})\n\n", commits.len()));
        for c in commits.iter().take(50) {
            out.push_str(&format!("- {}\n", c.as_str().unwrap_or_default()));
        }
        out.push('\n');
    }
    let files = prepared["files"].as_array().cloned().unwrap_or_default();
    if !files.is_empty() {
        out.push_str(&format!("**Files changed** ({})\n\n", files.len()));
        for f in files.iter().take(50) {
            out.push_str(&format!("- `{}` {}\n", f["status"].as_str().unwrap_or("M"), f["path"].as_str().unwrap_or_default()));
        }
        out.push('\n');
    }
    out.push_str("_Review before merging. Overseer never merges automatically._");
    out
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
        use super::without_userinfo;
        assert_eq!(without_userinfo("https://user:ghp_secret@github.com/o/r.git"), "https://github.com/o/r.git");
        assert_eq!(without_userinfo("https://github.com/o/r.git"), "https://github.com/o/r.git");
        assert_eq!(without_userinfo("ssh://git@github.com/o/r.git"), "ssh://github.com/o/r.git");
        assert_eq!(without_userinfo("git@github.com:o/r.git"), "git@github.com:o/r.git");
        assert_eq!(without_userinfo("https://tok@example.com"), "https://example.com");
    }
}
