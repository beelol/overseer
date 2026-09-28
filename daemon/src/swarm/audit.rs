//! Native workers in an audit (the owner's decision 3 of 2026-09-28, Swarm
//! RFC "S0 normal start"). In an audit (`source_change_permission: none`) a
//! native Claude worker is admitted only in Claude's read-only permission
//! mode (`--permission-mode plan`) with every write tool denied. After the
//! attempt the daemon checks that no source file in its workspace changed
//! against the pinned revision: HEAD, `git status` (untracked files
//! included) and every tracked file's content hash against the pinned tree.
//! Any change fails the attempt (the director cannot accept it and the job
//! is blocked `audit_source_changed`), is reported to the director (its
//! terminal message) and to Overseer (an event on the worker's run, shown in
//! its digest), and is kept as evidence: the workspace is left in place and
//! its whole tree is pinned under `refs/overseer/audit/<attempt>`. Every
//! other harness keeps being refused in an audit
//! (`audit_source_boundary_unqualified`).

use anyhow::{anyhow, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
use std::path::Path;

/// The only native harness with a qualified audit boundary.
pub const AUDIT_HARNESS: &str = "claude";
/// Claude's read-only permission mode.
pub const READ_ONLY_MODE: &str = "plan";
/// Tools denied to a read-only audit worker besides native delegation.
pub const WRITE_TOOLS: [&str; 5] = ["Edit", "Write", "MultiEdit", "NotebookEdit", "Bash"];
/// Changed paths listed in a report; the evidence ref holds all of them.
const LISTED: usize = 50;

/// Whether a native worker of `harness` may take part in an audit.
pub fn qualified(harness: &str) -> bool {
    harness == AUDIT_HARNESS
}

/// What changed in `workspace` against `pinned`; empty when nothing did.
pub fn source_changes(workspace: &Path, pinned: &str) -> Result<Vec<Value>> {
    let mut changes = Vec::new();
    let pinned = crate::git::rev_parse(workspace, &format!("{pinned}^{{commit}}"))
        .ok_or_else(|| anyhow!("pinned revision {pinned} is not in the workspace"))?;
    let head = crate::git::head(workspace);
    if head.as_deref() != Some(pinned.as_str()) {
        changes.push(json!({"kind":"head_moved","from":pinned,"to":head}));
    }
    // Every path Git sees as changed, staged or untracked (ignored files are
    // not source).
    let status = crate::git::git_env(workspace,
        &["status", "--porcelain=v1", "-z", "--untracked-files=all"], &[])?;
    for entry in status.split(|b| *b == 0).filter(|e| e.len() > 3) {
        let text = String::from_utf8_lossy(entry);
        changes.push(json!({"kind":"status","code":text[..2].to_string(),"path":text[3..].to_string()}));
    }
    // Every tracked file's content against the pinned tree, so an index
    // trick (assume-unchanged, skip-worktree) cannot hide a change.
    let tree = crate::git::git_env(workspace, &["ls-tree", "-r", "-z", &pinned], &[])?;
    let mut files = Vec::new();
    for entry in tree.split(|b| *b == 0).filter(|e| !e.is_empty()) {
        let text = String::from_utf8_lossy(entry);
        let Some((meta, path)) = text.split_once('\t') else { continue };
        let fields: Vec<&str> = meta.split_whitespace().collect();
        if fields.len() == 3 && fields[1] == "blob" {
            files.push((fields[0].to_string(), fields[2].to_string(), path.to_string()));
        }
    }
    let mut regular = Vec::new();
    for (mode, oid, path) in &files {
        let full = workspace.join(path);
        let meta = std::fs::symlink_metadata(&full);
        if meta.is_err() {
            changes.push(json!({"kind":"missing","path":path}));
            continue;
        }
        if mode == "120000" {
            let target = std::fs::read_link(&full).map(|t| t.to_string_lossy().to_string()).unwrap_or_default();
            let hashed = crate::git::git_stdin(workspace, &["hash-object", "--stdin"], target.as_bytes())?;
            if hashed.trim() != oid {
                changes.push(json!({"kind":"content","path":path}));
            }
        } else {
            regular.push((oid.clone(), path.clone()));
        }
    }
    if !regular.is_empty() {
        let input: String = regular.iter().map(|(_, p)| format!("{p}\n")).collect();
        let hashes = crate::git::git_stdin(workspace, &["hash-object", "--stdin-paths"], input.as_bytes())?;
        for ((oid, path), hashed) in regular.iter().zip(hashes.lines()) {
            if hashed.trim() != oid {
                changes.push(json!({"kind":"content","path":path}));
            }
        }
    }
    changes.dedup();
    Ok(changes)
}

/// The recorded check of one attempt: `clean` or `changed`, if made.
pub fn outcome(conn: &Connection, attempt: &str) -> Result<Option<String>> {
    Ok(conn.query_row("SELECT outcome FROM swarm_audit_checks WHERE attempt_id=?1", [attempt],
        |row| row.get(0)).optional()?)
}

/// Whether this attempt is a native worker in an audit, which needs the check.
pub fn needs_check(conn: &Connection, run: &str, attempt: &str) -> Result<bool> {
    Ok(conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM swarm_admissions a JOIN swarm_runs s ON s.id=a.run_id
         WHERE a.attempt_id=?1 AND a.run_id=?2 AND s.source_change_permission='none'
           AND COALESCE(a.target_harness,'generic')<>'generic')",
        params![attempt, run], |row| row.get(0))?)
}

/// Check an exited native audit worker once and record the result. Returns
/// the report for the director's terminal message.
pub fn check_attempt(conn: &Connection, run: &str, job: &str, attempt: &str, worker_run: &str,
    now_ms: i64) -> Result<Value> {
    if let Some(recorded) = conn.query_row(
        "SELECT outcome,changes,evidence_ref FROM swarm_audit_checks WHERE attempt_id=?1", [attempt],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, Option<String>>(2)?)))
        .optional()? {
        return Ok(report(&recorded.0, &serde_json::from_str(&recorded.1)?, recorded.2.as_deref()));
    }
    let (path, pinned): (String, Option<String>) = conn.query_row(
        "SELECT w.path,t.fork_commit FROM runs r JOIN tasks t ON t.id=r.task_id
         JOIN workspaces w ON w.id=r.workspace_id WHERE r.id=?1",
        [worker_run], |row| Ok((row.get(0)?, row.get(1)?)))?;
    let pinned = pinned.ok_or_else(|| anyhow!("the worker's pinned revision is not recorded"))?;
    let workspace = Path::new(&path);
    // A check that cannot be made fails closed: the attempt is not accepted.
    let (changes, failed) = match source_changes(workspace, &pinned) {
        Ok(changes) => (changes, false),
        Err(error) => (vec![json!({"kind":"check_failed","detail":crate::redact::redact(&error.to_string())})], true),
    };
    let (outcome, evidence) = if changes.is_empty() {
        ("clean", None)
    } else if failed {
        ("unavailable", None)
    } else {
        // Keep the whole tree the worker left, untracked files included.
        let trees = crate::git::capture_trees(workspace, &crate::paths::data_dir().join("tmp"))?;
        let refname = format!("refs/overseer/audit/{attempt}");
        crate::git::pin_tree(workspace, &trees.worktree_tree, trees.head.as_deref(),
            &format!("overseer audit boundary evidence for {attempt}"), &refname)?;
        ("changed", Some(refname))
    };
    conn.execute(
        "INSERT INTO swarm_audit_checks(attempt_id,run_id,job_id,worker_run_id,pinned,outcome,changes,
         evidence_ref,checked_ms) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
        params![attempt, run, job, worker_run, pinned, outcome, serde_json::to_string(&changes)?,
            evidence, now_ms])?;
    Ok(report(outcome, &json!(changes), evidence.as_deref()))
}

fn report(outcome: &str, changes: &Value, evidence: Option<&str>) -> Value {
    let changes = changes.as_array().cloned().unwrap_or_default();
    json!({"outcome":outcome,"changed_count":changes.len(),
        "changed":changes.into_iter().take(LISTED).collect::<Vec<_>>(),"evidence_ref":evidence})
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo() -> (tempfile::TempDir, std::path::PathBuf, String) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("repo");
        std::fs::create_dir_all(path.join("src")).unwrap();
        let git = |args: &[&str]| { crate::git::git_env(&path, args, &[("GIT_AUTHOR_NAME","t"),("GIT_AUTHOR_EMAIL","t@t"),
            ("GIT_COMMITTER_NAME","t"),("GIT_COMMITTER_EMAIL","t@t")]).unwrap(); };
        git(&["init", "-q"]);
        std::fs::write(path.join("src/a.txt"), "a\n").unwrap();
        std::fs::write(path.join(".gitignore"), "build/\n").unwrap();
        git(&["add", "."]);
        git(&["commit", "-q", "-m", "pinned"]);
        let pinned = crate::git::head(&path).unwrap();
        (dir, path, pinned)
    }

    /// The daemon's no-source-change check: clean when nothing changed or
    /// only ignored build output appeared; a changed, added, deleted or
    /// committed file is found, and so is a change hidden from `git status`
    /// by an index flag (the tracked-file hash comparison).
    #[test]
    fn the_source_check_finds_every_kind_of_change_against_the_pinned_revision() {
        let paths = |changes: &[Value]| changes.iter().filter_map(|c| c["path"].as_str().map(str::to_string)).collect::<Vec<_>>();
        let (_d, path, pinned) = repo();
        assert!(source_changes(&path, &pinned).unwrap().is_empty());
        std::fs::create_dir_all(path.join("build")).unwrap();
        std::fs::write(path.join("build/out.bin"), "x").unwrap();
        assert!(source_changes(&path, &pinned).unwrap().is_empty(), "ignored output is not source");

        std::fs::write(path.join("src/a.txt"), "changed\n").unwrap();
        std::fs::write(path.join("src/new.txt"), "new\n").unwrap();
        let found = source_changes(&path, &pinned).unwrap();
        assert!(paths(&found).contains(&"src/a.txt".to_string()) && paths(&found).contains(&"src/new.txt".to_string()), "{found:?}");

        let (_d, path, pinned) = repo();
        std::fs::remove_file(path.join("src/a.txt")).unwrap();
        assert!(source_changes(&path, &pinned).unwrap().iter().any(|c| c["kind"] == "missing"));

        let (_d, path, pinned) = repo();
        crate::git::git(&path, &["update-index", "--skip-worktree", "src/a.txt"]).unwrap();
        std::fs::write(path.join("src/a.txt"), "hidden\n").unwrap();
        let found = source_changes(&path, &pinned).unwrap();
        assert!(found.iter().any(|c| c["kind"] == "content" && c["path"] == "src/a.txt"), "{found:?}");

        let (_d, path, pinned) = repo();
        std::fs::write(path.join("src/a.txt"), "committed\n").unwrap();
        crate::git::git_env(&path, &["commit", "-qam", "slipped"], &[("GIT_AUTHOR_NAME","t"),("GIT_AUTHOR_EMAIL","t@t"),
            ("GIT_COMMITTER_NAME","t"),("GIT_COMMITTER_EMAIL","t@t")]).unwrap();
        let found = source_changes(&path, &pinned).unwrap();
        assert!(found.iter().any(|c| c["kind"] == "head_moved"), "{found:?}");
        assert!(found.iter().any(|c| c["kind"] == "content"), "{found:?}");
    }

    #[test]
    fn only_claude_has_a_qualified_audit_boundary() {
        assert!(qualified("claude"));
        for harness in ["codex", "codex-app", "opencode", "generic"] {
            assert!(!qualified(harness), "{harness}");
        }
    }
}
