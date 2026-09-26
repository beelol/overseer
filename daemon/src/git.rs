//! Git plumbing. Every invocation passes arguments as a vector (never a shell string).
//! Snapshots use a private temporary index so the user's index, working tree, stash
//! and branches are never modified; snapshot commits are kept alive by hidden refs
//! under `refs/overseer/`.

use anyhow::{anyhow, bail, Context, Result};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::process::Stdio;
use std::io::Write;

const SNAPSHOT_IDENTITY: [(&str, &str); 4] = [
    ("GIT_AUTHOR_NAME", "Overseer Snapshot"),
    ("GIT_AUTHOR_EMAIL", "overseer@localhost"),
    ("GIT_COMMITTER_NAME", "Overseer Snapshot"),
    ("GIT_COMMITTER_EMAIL", "overseer@localhost"),
];

pub fn git_env(cwd: &Path, args: &[&str], env: &[(&str, &str)]) -> Result<Vec<u8>> {
    let mut cmd = Command::new("git");
    cmd.current_dir(cwd).args(args);
    // Never let a caller's GIT_DIR/GIT_INDEX_FILE leak into our plumbing.
    for var in ["GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE", "GIT_OBJECT_DIRECTORY", "GIT_COMMON_DIR"] {
        cmd.env_remove(var);
    }
    cmd.env("GIT_OPTIONAL_LOCKS", "0").env("GIT_TERMINAL_PROMPT", "0");
    for (k, v) in env {
        cmd.env(k, v);
    }
    let out = cmd.output().with_context(|| format!("running git {}", args.join(" ")))?;
    if !out.status.success() {
        bail!("git {} failed: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(out.stdout)
}

pub fn git(cwd: &Path, args: &[&str]) -> Result<String> {
    Ok(String::from_utf8_lossy(&git_env(cwd, args, &[])?).trim_end_matches('\n').to_string())
}

/// Run Git with bytes on stdin, used for an already-reviewed patch without a disk copy.
pub fn git_stdin(cwd: &Path, args: &[&str], input: &[u8]) -> Result<String> {
    git_stdin_env(cwd, args, input, &[])
}

pub fn git_stdin_env(cwd: &Path, args: &[&str], input: &[u8], env: &[(&str, &str)]) -> Result<String> {
    let mut cmd = Command::new("git");
    cmd.current_dir(cwd).args(args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    for var in ["GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE", "GIT_OBJECT_DIRECTORY", "GIT_COMMON_DIR"] {
        cmd.env_remove(var);
    }
    cmd.env("GIT_OPTIONAL_LOCKS", "0").env("GIT_TERMINAL_PROMPT", "0");
    for (k, v) in env {
        cmd.env(k, v);
    }
    let mut child = cmd.spawn().with_context(|| format!("running git {}", args.join(" ")))?;
    child.stdin.take().ok_or_else(|| anyhow!("git stdin unavailable"))?.write_all(input)?;
    let out = child.wait_with_output()?;
    if !out.status.success() {
        bail!("git {} failed: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim_end_matches('\n').to_string())
}

pub fn toplevel(path: &Path) -> Result<PathBuf> {
    let top = git(path, &["rev-parse", "--show-toplevel"])?;
    Ok(std::fs::canonicalize(top)?)
}

pub fn common_dir(path: &Path) -> Result<PathBuf> {
    let dir = git(path, &["rev-parse", "--path-format=absolute", "--git-common-dir"])?;
    Ok(std::fs::canonicalize(dir)?)
}

pub fn rev_parse(path: &Path, rev: &str) -> Option<String> {
    if rev.starts_with('-') {
        return None;
    }
    git(path, &["rev-parse", "--verify", "--quiet", &format!("{rev}^{{commit}}")]).ok().filter(|s| !s.is_empty())
}

pub fn head(path: &Path) -> Option<String> {
    rev_parse(path, "HEAD")
}

pub fn head_branch(path: &Path) -> Option<String> {
    git(path, &["symbolic-ref", "--quiet", "--short", "HEAD"]).ok().filter(|s| !s.is_empty())
}

pub fn merge_base(path: &Path, a: &str, b: &str) -> Option<String> {
    git(path, &["merge-base", a, b]).ok().filter(|s| !s.is_empty())
}

pub fn branches(path: &Path) -> Vec<String> {
    git(path, &["for-each-ref", "--format=%(refname:short)", "refs/heads", "refs/remotes"])
        .map(|s| s.lines().filter(|l| !l.ends_with("/HEAD")).map(str::to_string).collect())
        .unwrap_or_default()
}

/// Configured integration branch (`overseer.integrationBranch`), else origin/HEAD, else main/master.
pub fn default_branch(path: &Path) -> Option<String> {
    if let Ok(configured) = git(path, &["config", "--get", "overseer.integrationBranch"]) {
        if !configured.is_empty() && rev_parse(path, &configured).is_some() {
            return Some(configured);
        }
    }
    if let Ok(origin) = git(path, &["symbolic-ref", "--quiet", "--short", "refs/remotes/origin/HEAD"]) {
        if !origin.is_empty() {
            return Some(origin);
        }
    }
    ["main", "master", "origin/main", "origin/master"].into_iter().find(|b| rev_parse(path, b).is_some()).map(str::to_string)
}

fn valid_branch_name(path: &Path, name: &str) -> bool {
    !name.starts_with('-') && git(path, &["check-ref-format", "--branch", name]).is_ok()
}

/// Create a new worktree on a new branch. Existing branches and paths are never reused
/// or deleted: a numeric suffix is appended until both are free.
pub fn worktree_add(repo: &Path, parent_dir: &Path, name: &str, start: &str) -> Result<(PathBuf, String)> {
    let slug: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' { c.to_ascii_lowercase() } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .chars()
        .take(40)
        .collect();
    let slug = if slug.is_empty() { "task".to_string() } else { slug };
    std::fs::create_dir_all(parent_dir)?;
    for n in 0..100 {
        let suffix = if n == 0 { String::new() } else { format!("-{}", n + 1) };
        let branch = format!("overseer/{slug}{suffix}");
        let path = parent_dir.join(format!("{slug}{suffix}"));
        if path.exists() || rev_parse(repo, &format!("refs/heads/{branch}")).is_some() || !valid_branch_name(repo, &branch) {
            continue;
        }
        let path_str = path.to_str().ok_or_else(|| anyhow!("non-UTF-8 path"))?;
        git(repo, &["worktree", "add", "-b", &branch, path_str, start])?;
        return Ok((std::fs::canonicalize(&path)?, branch));
    }
    bail!("could not find a free branch/path name for {slug}")
}

#[derive(Debug, Clone, Serialize)]
pub struct SnapshotTrees {
    pub head: Option<String>,
    pub index_tree: String,
    pub worktree_tree: String,
}

fn index_path(workspace: &Path) -> Result<PathBuf> {
    let p = git(workspace, &["rev-parse", "--path-format=absolute", "--git-path", "index"])?;
    Ok(PathBuf::from(p))
}

/// Trees for the current index and the full nonignored working tree, computed through
/// a private copy of the index. Nothing in the repository's index or checkout changes.
pub fn capture_trees(workspace: &Path, tmp_dir: &Path) -> Result<SnapshotTrees> {
    std::fs::create_dir_all(tmp_dir)?;
    let tmp = tmp_dir.join(format!("index-{}", uuid::Uuid::new_v4().simple()));
    let real = index_path(workspace)?;
    if real.exists() {
        std::fs::copy(&real, &tmp)?;
    }
    let tmp_str = tmp.to_str().ok_or_else(|| anyhow!("non-UTF-8 temp path"))?.to_string();
    let result = (|| {
        let env = [("GIT_INDEX_FILE", tmp_str.as_str())];
        // Unmerged (conflicted) entries cannot be written as a tree. Resolve them to their
        // working-tree content inside the private index only; the real index keeps its stages.
        let unmerged = String::from_utf8_lossy(&git_env(workspace, &["diff", "--name-only", "--diff-filter=U", "-z"], &env)?).to_string();
        let unmerged: Vec<&str> = unmerged.split('\0').filter(|p| !p.is_empty()).collect();
        if !unmerged.is_empty() {
            let mut args = vec!["add", "-A", "--"];
            args.extend(unmerged.iter().copied());
            git_env(workspace, &args, &env)?;
        }
        let index_tree = String::from_utf8_lossy(&git_env(workspace, &["write-tree"], &env)?).trim().to_string();
        git_env(workspace, &["add", "-A", "--", "."], &env)?;
        let worktree_tree = String::from_utf8_lossy(&git_env(workspace, &["write-tree"], &env)?).trim().to_string();
        Ok(SnapshotTrees { head: head(workspace), index_tree, worktree_tree })
    })();
    let _ = std::fs::remove_file(&tmp);
    let _ = std::fs::remove_file(tmp.with_extension("lock"));
    result
}

/// Record a tree as a commit object pinned by a hidden ref. Returns the commit SHA.
pub fn pin_tree(workspace: &Path, tree: &str, parent: Option<&str>, message: &str, refname: &str) -> Result<String> {
    let mut args = vec!["commit-tree", tree, "-m", message];
    if let Some(p) = parent {
        args.push("-p");
        args.push(p);
    }
    let commit = String::from_utf8_lossy(&git_env(workspace, &args, &SNAPSHOT_IDENTITY)?).trim().to_string();
    git(workspace, &["update-ref", refname, &commit])?;
    Ok(commit)
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Change {
    /// A, M, D, R, C, T, U (unmerged), ? (untracked)
    pub status: String,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old_path: Option<String>,
}

pub fn diff_trees(workspace: &Path, a: &str, b: &str) -> Result<Vec<Change>> {
    let raw = git_env(workspace, &["diff-tree", "-r", "-z", "--no-commit-id", "--name-status", "-M", a, b], &[])?;
    let mut parts = raw.split(|c| *c == 0).map(|s| String::from_utf8_lossy(s).to_string());
    let mut out = Vec::new();
    while let Some(status) = parts.next() {
        if status.is_empty() {
            continue;
        }
        let code = status[..1].to_string();
        if code == "R" || code == "C" {
            let old = parts.next().unwrap_or_default();
            let new = parts.next().unwrap_or_default();
            out.push(Change { status: code, path: new, old_path: Some(old) });
        } else {
            let path = parts.next().unwrap_or_default();
            out.push(Change { status: code, path, old_path: None });
        }
    }
    Ok(out)
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct Status {
    pub branch: Option<String>,
    pub head: Option<String>,
    pub staged: Vec<Change>,
    pub unstaged: Vec<Change>,
    pub untracked: Vec<String>,
    pub conflicted: Vec<String>,
}

impl Status {
    pub fn is_clean(&self) -> bool {
        self.staged.is_empty() && self.unstaged.is_empty() && self.untracked.is_empty() && self.conflicted.is_empty()
    }
}

pub fn status(workspace: &Path) -> Result<Status> {
    let raw = git_env(workspace, &["status", "--porcelain=v2", "-z", "--untracked-files=all", "--no-renames"], &[])?;
    let mut st = Status { branch: head_branch(workspace), head: head(workspace), ..Default::default() };
    let mut records = raw.split(|c| *c == 0).map(|s| String::from_utf8_lossy(s).to_string());
    while let Some(rec) = records.next() {
        if rec.is_empty() {
            continue;
        }
        match &rec[..1] {
            "1" => {
                let fields: Vec<&str> = rec.splitn(9, ' ').collect();
                if fields.len() < 9 {
                    continue;
                }
                let (x, y) = (&fields[1][..1], &fields[1][1..2]);
                let path = fields[8].to_string();
                if x != "." {
                    st.staged.push(Change { status: x.to_string(), path: path.clone(), old_path: None });
                }
                if y != "." {
                    st.unstaged.push(Change { status: y.to_string(), path, old_path: None });
                }
            }
            "2" => {
                let fields: Vec<&str> = rec.splitn(10, ' ').collect();
                let orig = records.next().unwrap_or_default();
                if fields.len() < 10 {
                    continue;
                }
                let (x, y) = (&fields[1][..1], &fields[1][1..2]);
                let path = fields[9].to_string();
                if x != "." {
                    st.staged.push(Change { status: x.to_string(), path: path.clone(), old_path: Some(orig.clone()) });
                }
                if y != "." {
                    st.unstaged.push(Change { status: y.to_string(), path, old_path: Some(orig) });
                }
            }
            "u" => {
                let fields: Vec<&str> = rec.splitn(11, ' ').collect();
                if let Some(p) = fields.get(10) {
                    st.conflicted.push(p.to_string());
                }
            }
            "?" => st.untracked.push(rec[2..].to_string()),
            _ => {}
        }
    }
    Ok(st)
}

/// Does `path` exist in `tree_ish`?
pub fn tree_has(workspace: &Path, tree_ish: &str, path: &str) -> bool {
    git(workspace, &["cat-file", "-e", &format!("{tree_ish}:{path}")]).is_ok()
}

pub fn show_blob(workspace: &Path, tree_ish: &str, path: &str) -> Result<Vec<u8>> {
    git_env(workspace, &["cat-file", "blob", &format!("{tree_ish}:{path}")], &[])
}

pub fn is_ancestor(workspace: &Path, a: &str, b: &str) -> bool {
    Command::new("git").current_dir(workspace).args(["merge-base", "--is-ancestor", a, b]).status().map(|s| s.success()).unwrap_or(false)
}

pub fn worktree_list(repo: &Path) -> Vec<PathBuf> {
    git(repo, &["worktree", "list", "--porcelain"])
        .map(|s| s.lines().filter_map(|l| l.strip_prefix("worktree ")).map(PathBuf::from).collect())
        .unwrap_or_default()
}

pub fn worktree_remove(repo: &Path, path: &Path) -> Result<()> {
    let p = path.to_str().ok_or_else(|| anyhow!("non-UTF-8 path"))?;
    git(repo, &["worktree", "remove", p])?;
    Ok(())
}
