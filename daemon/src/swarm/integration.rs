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

fn expected_tree(workspace: &Path, patch: &str) -> Result<String> {
    let index = git::git(
        workspace,
        &["rev-parse", "--path-format=absolute", "--git-path", "index"],
    )?;
    let scratch = tempfile::NamedTempFile::new()?;
    std::fs::copy(index, scratch.path())?;
    let scratch_path = scratch
        .path()
        .to_str()
        .ok_or_else(|| anyhow!("non-UTF-8 scratch index"))?;
    let env = [("GIT_INDEX_FILE", scratch_path)];
    git::git_stdin_env(
        workspace,
        &["apply", "--cached", "--check", "-"],
        patch.as_bytes(),
        &env,
    )?;
    git::git_stdin_env(
        workspace,
        &["apply", "--cached", "-"],
        patch.as_bytes(),
        &env,
    )?;
    git::git_env(workspace, &["write-tree"], &env)
        .map(|bytes| String::from_utf8_lossy(&bytes).trim().to_string())
}

fn acknowledge(
    store: &mut Store,
    run: &str,
    job: &str,
    artifact: &str,
    prior: &str,
    commit: &str,
    generation: i64,
    revision: i64,
) -> Result<()> {
    let now = crate::daemon::now();
    let tx = store.conn.transaction()?;
    let current: (i64, i64, String) = tx.query_row(
        "SELECT generation,revision,status FROM swarm_runs WHERE id=?1",
        [run], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    if current.0 != generation || current.1 != revision
        || !["planning", "running"].contains(&current.2.as_str()) {
        bail!("run cannot integrate after its control state changed");
    }
    let changed = tx.execute(
        "UPDATE swarm_integrations SET current_commit=?2,updated_ms=?3 WHERE run_id=?1 AND current_commit=?4",
        params![run,commit,now,prior],
    )?;
    if changed != 1 {
        bail!("integration revision changed during acknowledgement");
    }
    tx.execute(
        "INSERT INTO swarm_integrated_artifacts(run_id,artifact_id,job_id,commit_sha,created_ms)
         VALUES(?1,?2,?3,?4,?5)",
        params![run, artifact, job, commit, now],
    )?;
    tx.execute(
        "DELETE FROM swarm_integration_intents WHERE run_id=?1 AND artifact_id=?2",
        params![run, artifact],
    )?;
    super::artifacts::release_and_unlock(&tx, run, job, now)?;
    tx.commit().map_err(Into::into)
}

fn ensure_current(store: &Store, run: &str, generation: i64, revision: i64) -> Result<()> {
    let current: (i64, i64, String) = store.conn.query_row(
        "SELECT generation,revision,status FROM swarm_runs WHERE id=?1",
        [run], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    if current.0 != generation || current.1 != revision
        || !["planning", "running"].contains(&current.2.as_str()) {
        bail!("run cannot integrate after its control state changed");
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
    if current["source_change_permission"] != "isolated" {
        bail!("source changes are not permitted for this run");
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
    // An unrelated plan edit advances the run revision but leaves an accepted
    // job's plan revision intact. Its evidence remains valid only at that job
    // revision; the caller must still hold the current run revision above.
    if job_status != "accepted" || job_revision > revision {
        bail!("job is not accepted at its retained revision");
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
    if source_revision != job_revision || kind != "patch" {
        bail!("stale or non-patch artifact");
    }
    if format!("{:x}", Sha256::digest(content.as_bytes())) != digest {
        bail!("artifact integrity check failed");
    }
    let finished: bool = store.conn.prepare(
        "SELECT 1 FROM swarm_attempts WHERE id=?1 AND run_id=?2 AND job_id=?3 AND revision=?4 AND status='finished'"
    )?.exists(params![attempt,run,job,job_revision])?;
    if !finished {
        bail!("worker exit is not confirmed");
    }
    let accepted = store.conn.prepare(
        "SELECT evidence FROM swarm_decisions WHERE run_id=?1 AND job_id=?2 AND attempt_id=?3 AND revision=?4 AND decision='accept'"
    )?.query_map(params![run,job,attempt,job_revision], |r| r.get::<_,String>(0))?
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
    let pending_intent: bool = store
        .conn
        .prepare("SELECT 1 FROM swarm_integration_intents WHERE run_id=?1")?
        .exists([run])?;
    if duplicate.is_none() && !pending_intent && git::head(&root).as_deref() != Some(base.as_str())
    {
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
    let intent: Option<(String,String,String,String,String)> = store.conn.query_row(
        "SELECT artifact_id,job_id,prior_commit,expected_tree,artifact_sha256 FROM swarm_integration_intents WHERE run_id=?1",
        [run], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?)),
    ).optional()?;
    if intent.as_ref().is_some_and(
        |(saved_artifact, saved_job, saved_prior, _, saved_digest)| {
            saved_artifact != artifact
                || saved_job != job
                || saved_prior != &prior_commit
                || saved_digest != &digest
        },
    ) {
        bail!("another integration intent requires reconciliation");
    }
    let clean = git::git(&workspace, &["status", "--porcelain=v1"])?.is_empty();
    if intent.is_none() && (actual_head != prior_commit || !clean) {
        bail!("integration workspace requires reconciliation");
    }
    if let Some(commit) = duplicate {
        if intent.is_some() {
            bail!("integration intent requires reconciliation");
        }
        return Ok(
            json!({"status":"integrated","duplicate":true,"commit":commit,
            "workspace_path":workspace,"branch":branch}),
        );
    }
    if content.trim().is_empty() {
        bail!("empty patch artifact");
    }
    let message = format!("Integrate Swarm {run} {job} {artifact}");
    if let Some((_, _, _, tree, _)) = &intent {
        if actual_head != prior_commit {
            let parents = git::git(&workspace, &["rev-list", "--parents", "-n", "1", "HEAD"])?;
            let committed_tree = git::git(&workspace, &["rev-parse", "HEAD^{tree}"])?;
            let committed_message = git::git(&workspace, &["log", "-1", "--format=%s"])?;
            if !clean
                || parents != format!("{actual_head} {prior_commit}")
                || &committed_tree != tree
                || committed_message != message
            {
                bail!("integration workspace requires reconciliation");
            }
            acknowledge(store, run, job, artifact, &prior_commit, &actual_head, generation, revision)?;
            return Ok(
                json!({"status":"integrated","duplicate":false,"recovered":true,
                "commit":actual_head,"workspace_path":workspace,"branch":branch}),
            );
        }
        if !clean {
            let staged_tree = git::git(&workspace, &["write-tree"])?;
            let unstaged = git::git(&workspace, &["diff", "--name-only"])?;
            let untracked = git::git(&workspace, &["ls-files", "--others", "--exclude-standard"])?;
            if staged_tree != *tree || !unstaged.is_empty() || !untracked.is_empty() {
                bail!("integration workspace requires reconciliation");
            }
        }
    } else {
        let tree = expected_tree(&workspace, &content)?;
        store.conn.execute(
            "INSERT INTO swarm_integration_intents(run_id,artifact_id,job_id,prior_commit,expected_tree,artifact_sha256,created_ms)
             VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![run,artifact,job,prior_commit,tree,digest,crate::daemon::now()],
        )?;
    }
    if clean {
        git::git_stdin(
            &workspace,
            &["apply", "--index", "--check", "-"],
            content.as_bytes(),
        )?;
        git::git_stdin(&workspace, &["apply", "--index", "-"], content.as_bytes())?;
    }
    let expected: String = store.conn.query_row(
        "SELECT expected_tree FROM swarm_integration_intents WHERE run_id=?1",
        [run],
        |r| r.get(0),
    )?;
    if git::git(&workspace, &["write-tree"])? != expected
        || !git::git(&workspace, &["diff", "--name-only"])?.is_empty()
        || !git::git(&workspace, &["ls-files", "--others", "--exclude-standard"])?.is_empty()
    {
        bail!("integration workspace changed before commit");
    }
    if p["fixture_fault"] == "after_apply" {
        bail!("fixture interruption after patch apply");
    }
    if let Some(delay) = p.get("fixture_delay_before_commit_ms") {
        let millis = delay.as_u64().ok_or_else(|| anyhow!("invalid fixture integration delay"))?;
        if millis > 5000 { bail!("fixture integration delay exceeds limit"); }
        std::thread::sleep(std::time::Duration::from_millis(millis));
    }
    ensure_current(store, run, generation, revision)?;
    ensure_no_active_commit_hooks(&root)?;
    git::git_env(&workspace, &["commit", "-m", &message], &IDENTITY)?;
    let commit = git::head(&workspace).ok_or_else(|| anyhow!("integration commit missing"))?;
    if git::git(&workspace, &["rev-parse", "HEAD^{tree}"])? != expected
        || git::git(&workspace, &["rev-list", "--parents", "-n", "1", "HEAD"])?
            != format!("{commit} {prior_commit}")
        || !git::git(&workspace, &["status", "--porcelain=v1"])?.is_empty()
    {
        bail!("integration commit differs from accepted patch");
    }
    if p["fixture_fault"] == "after_commit" {
        bail!("fixture interruption after Git commit");
    }
    acknowledge(store, run, job, artifact, &prior_commit, &commit, generation, revision)?;
    Ok(
        json!({"status":"integrated","duplicate":false,"commit":commit,
        "workspace_path":workspace,"branch":branch}),
    )
}
