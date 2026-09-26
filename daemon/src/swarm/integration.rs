//! Accepted patches are applied in an Overseer-owned worktree, never in the user's checkout.
//! This is a fixture-qualified integration seam; crash reconciliation and combined checks
//! remain explicit prerequisites before any job can be called fully integrated.

use super::{get, required};
use crate::{git, paths, store::Store};
use anyhow::{anyhow, bail, Result};
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

const IDENTITY: [(&str, &str); 4] = [
    ("GIT_AUTHOR_NAME", "Overseer Integration"),
    ("GIT_AUTHOR_EMAIL", "overseer@localhost"),
    ("GIT_COMMITTER_NAME", "Overseer Integration"),
    ("GIT_COMMITTER_EMAIL", "overseer@localhost"),
];

fn ensure_no_active_commit_hooks(repo: &Path) -> Result<()> {
    if git::git(repo, &["config", "--get", "core.hooksPath"]).is_ok() {
        bail!("configured commit hook path blocks unattended integration");
    }
    let hooks = git::git(
        repo,
        &["rev-parse", "--path-format=absolute", "--git-path", "hooks"],
    )?;
    let path = Path::new(&hooks);
    if !path.exists() {
        return Ok(());
    }
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        if entry.file_name().to_string_lossy().ends_with(".sample") {
            continue;
        }
        let metadata = entry.metadata()?;
        if metadata.is_file() && metadata.permissions().mode() & 0o111 != 0 {
            bail!("active commit hook blocks unattended integration");
        }
    }
    Ok(())
}

pub fn integrate(store: &mut Store, p: &Value) -> Result<Value> {
    let run = required(p, "run_id")?;
    let job = required(p, "job_id")?;
    let artifact = required(p, "artifact_id")?;
    let repo = Path::new(required(p, "repo")?);
    let requested_base = required(p, "base_revision")?;
    let generation = p["generation"]
        .as_i64()
        .ok_or_else(|| anyhow!("missing generation"))?;
    let revision = p["revision"]
        .as_i64()
        .ok_or_else(|| anyhow!("missing revision"))?;
    let current = get(store, run)?;
    if current["generation"] != generation {
        bail!("stale director generation");
    }
    if current["revision"] != revision {
        bail!("stale plan revision");
    }
    if ["stopping", "stopped", "stalled"].contains(&current["status"].as_str().unwrap_or("")) {
        bail!("run cannot integrate in this state");
    }
    let root = git::toplevel(repo)?;
    if root != std::fs::canonicalize(repo)? {
        bail!("integration repo must be its root");
    }
    ensure_no_active_commit_hooks(&root)?;
    let base =
        git::rev_parse(&root, requested_base).ok_or_else(|| anyhow!("invalid base revision"))?;
    if base != requested_base {
        bail!("base revision must be a pinned commit SHA");
    }
    let (job_status, job_revision): (String, i64) = store
        .conn
        .query_row(
            "SELECT status,plan_revision FROM swarm_jobs WHERE run_id=?1 AND id=?2",
            params![run, job],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?
        .ok_or_else(|| anyhow!("unknown job"))?;
    if job_status != "accepted" || job_revision != revision {
        bail!("job is not accepted at this revision");
    }
    let (attempt, source_revision, kind, content, digest): (String, i64, String, String, String) =
        store
            .conn
            .query_row(
                "SELECT attempt_id,source_revision,kind,content,sha256 FROM swarm_artifacts
         WHERE run_id=?1 AND job_id=?2 AND id=?3",
                params![run, job, artifact],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .optional()?
            .ok_or_else(|| anyhow!("missing patch artifact"))?;
    if source_revision != revision || kind != "patch" {
        bail!("stale or non-patch artifact");
    }
    if format!("{:x}", Sha256::digest(content.as_bytes())) != digest {
        bail!("artifact integrity check failed");
    }
    let finished: bool = store.conn.prepare(
        "SELECT 1 FROM swarm_attempts WHERE id=?1 AND run_id=?2 AND job_id=?3 AND status='finished'"
    )?.exists(params![attempt,run,job])?;
    if !finished {
        bail!("worker exit is not confirmed");
    }
    let accepted = store.conn.prepare(
        "SELECT evidence FROM swarm_decisions WHERE run_id=?1 AND job_id=?2 AND attempt_id=?3 AND decision='accept'"
    )?.query_map(params![run,job,attempt], |r| r.get::<_,String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?.into_iter().any(|raw| {
            serde_json::from_str::<Vec<String>>(&raw).ok()
                .is_some_and(|ids| ids.iter().any(|id| id == artifact))
        });
    if !accepted {
        bail!("patch artifact was not accepted as evidence");
    }
    let uncertain: i64 = store.conn.query_row(
        "SELECT COUNT(*) FROM swarm_effects WHERE run_id=?1 AND job_id=?2 AND outcome='unknown'",
        params![run, job],
        |r| r.get(0),
    )?;
    if uncertain > 0 {
        bail!("unreconciled side effect blocks integration");
    }
    let duplicate: Option<String> = store
        .conn
        .query_row(
            "SELECT commit_sha FROM swarm_integrated_artifacts WHERE run_id=?1 AND artifact_id=?2",
            params![run, artifact],
            |r| r.get(0),
        )
        .optional()?;
    if duplicate.is_none() && git::head(&root).as_deref() != Some(base.as_str()) {
        bail!("source commit changed since the patch base was pinned");
    }
    let existing: Option<(String,String,String,String,String)> = store.conn.query_row(
        "SELECT repo_root,base_commit,workspace_path,branch,current_commit FROM swarm_integrations WHERE run_id=?1",
        params![run], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?)),
    ).optional()?;
    let root_str = root
        .to_str()
        .ok_or_else(|| anyhow!("non-UTF-8 repository path"))?;
    let (workspace, branch, prior_commit) = if let Some((
        saved_root,
        saved_base,
        path,
        branch,
        commit,
    )) = existing
    {
        if saved_root != root_str || saved_base != base {
            bail!("integration source or base changed");
        }
        (std::path::PathBuf::from(path), branch, commit)
    } else {
        let parent = paths::data_dir().join("swarm-integrations");
        paths::ensure_private_dir(&parent)?;
        let (path, branch) =
            git::worktree_add(&root, &parent, &format!("integration-{run}"), &base)?;
        let now = crate::daemon::now();
        store.conn.execute(
            "INSERT INTO swarm_integrations(run_id,repo_root,base_commit,workspace_path,branch,current_commit,created_ms,updated_ms)
             VALUES(?1,?2,?3,?4,?5,?3,?6,?6)",
            params![run,root_str,base,path.to_string_lossy(),branch,now],
        )?;
        (path, branch, base.clone())
    };
    let actual_head =
        git::head(&workspace).ok_or_else(|| anyhow!("integration workspace is missing"))?;
    if actual_head != prior_commit
        || !git::git(&workspace, &["status", "--porcelain=v1"])?.is_empty()
    {
        bail!("integration workspace requires reconciliation");
    }
    if let Some(commit) = duplicate {
        return Ok(
            json!({"status":"integrated","duplicate":true,"commit":commit,
            "workspace_path":workspace,"branch":branch}),
        );
    }
    if content.trim().is_empty() {
        bail!("empty patch artifact");
    }
    git::git_stdin(
        &workspace,
        &["apply", "--index", "--check", "-"],
        content.as_bytes(),
    )?;
    git::git_stdin(&workspace, &["apply", "--index", "-"], content.as_bytes())?;
    ensure_no_active_commit_hooks(&root)?;
    let message = format!("Integrate Swarm {run} {job} {artifact}");
    git::git_env(&workspace, &["commit", "-m", &message], &IDENTITY)?;
    let commit = git::head(&workspace).ok_or_else(|| anyhow!("integration commit missing"))?;
    let now = crate::daemon::now();
    let tx = store.conn.transaction()?;
    tx.execute(
        "UPDATE swarm_integrations SET current_commit=?2,updated_ms=?3 WHERE run_id=?1 AND current_commit=?4",
        params![run,commit,now,prior_commit],
    )?;
    tx.execute(
        "INSERT INTO swarm_integrated_artifacts(run_id,artifact_id,job_id,commit_sha,created_ms)
         VALUES(?1,?2,?3,?4,?5)",
        params![run, artifact, job, commit, now],
    )?;
    super::artifacts::release_and_unlock(&tx, run, job, now)?;
    tx.commit()?;
    Ok(
        json!({"status":"integrated","duplicate":false,"commit":commit,
        "workspace_path":workspace,"branch":branch}),
    )
}
