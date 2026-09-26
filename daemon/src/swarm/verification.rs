//! A fixture-only combined-check runner. The executable is selected by the test
//! environment, not by a worker or director message. Live verifier authority and
//! sandboxing remain prerequisites before this can be enabled outside fixtures.

use super::{get, required};
use crate::{git, store::Store};
use anyhow::{anyhow, bail, Result};
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::io::{Read, Seek, SeekFrom};
use std::os::unix::{fs::PermissionsExt, process::CommandExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const MAX_VERIFIER_BYTES: u64 = 1024 * 1024;
const MAX_OUTPUT_BYTES: usize = 8192;

pub struct VerificationPlan {
    run: String,
    request_id: String,
    generation: i64,
    revision: i64,
    commit: String,
    workspace: PathBuf,
    verifier: PathBuf,
    verifier_sha256: String,
}

pub enum PreparedVerification {
    Ready(VerificationPlan),
    Existing(Value),
}

pub struct VerificationOutcome {
    status: &'static str,
    exit_code: Option<i32>,
    stdout: String,
    stderr: String,
}

fn configured_verifier() -> Result<(PathBuf, String)> {
    let raw = std::env::var("OVERSEER_SWARM_VERIFIER_PATH")
        .map_err(|_| anyhow!("combined verifier is not configured"))?;
    let path = std::fs::canonicalize(raw)?;
    let metadata = std::fs::metadata(&path)?;
    if !metadata.is_file()
        || metadata.len() > MAX_VERIFIER_BYTES
        || metadata.permissions().mode() & 0o111 == 0
    {
        bail!("combined verifier must be a bounded executable file");
    }
    let digest = format!("{:x}", Sha256::digest(std::fs::read(&path)?));
    Ok((path, digest))
}

fn clean_at(workspace: &Path, commit: &str) -> Result<bool> {
    Ok(git::head(workspace).as_deref() == Some(commit)
        && git::git(workspace, &["status", "--porcelain=v1"])?.is_empty())
}

pub fn prepare(store: &mut Store, p: &Value) -> Result<PreparedVerification> {
    let run = required(p, "run_id")?;
    let request_id = required(p, "request_id")?;
    if request_id.is_empty() || request_id.len() > 128 {
        bail!("invalid verification request id");
    }
    let generation = p["generation"]
        .as_i64()
        .ok_or_else(|| anyhow!("missing generation"))?;
    let revision = p["revision"]
        .as_i64()
        .ok_or_else(|| anyhow!("missing revision"))?;
    let current = get(store, run)?;
    if current["generation"] != generation || current["revision"] != revision {
        bail!("stale verification generation or revision");
    }
    if current["status"] != "planning" && current["status"] != "running" {
        bail!("run cannot verify in this state");
    }
    let (workspace, commit): (String, String) = store
        .conn
        .query_row(
            "SELECT workspace_path,current_commit FROM swarm_integrations WHERE run_id=?1",
            [run],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?
        .ok_or_else(|| anyhow!("no integration workspace to verify"))?;
    let workspace = PathBuf::from(workspace);
    if !clean_at(&workspace, &commit)? {
        bail!("integration workspace requires reconciliation");
    }
    let (verifier, verifier_sha256) = configured_verifier()?;
    let existing: Option<(i64, i64, String, String, String, Option<i32>)> = store
        .conn
        .query_row(
            "SELECT generation,revision,commit_sha,verifier_sha256,status,exit_code
         FROM swarm_verifications WHERE run_id=?1 AND request_id=?2",
            params![run, request_id],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                ))
            },
        )
        .optional()?;
    if let Some((old_generation, old_revision, old_commit, old_verifier, status, exit_code)) =
        existing
    {
        if (
            old_generation,
            old_revision,
            old_commit.as_str(),
            old_verifier.as_str(),
        ) != (
            generation,
            revision,
            commit.as_str(),
            verifier_sha256.as_str(),
        ) {
            bail!("verification request id reused with different input");
        }
        return Ok(PreparedVerification::Existing(
            json!({"status":status,"commit":commit,
            "exit_code":exit_code,"duplicate":true}),
        ));
    }
    let running: bool = store
        .conn
        .prepare("SELECT 1 FROM swarm_verifications WHERE run_id=?1 AND status='running'")?
        .exists([run])?;
    if running {
        bail!("combined verification is already running or requires reconciliation");
    }
    store.conn.execute(
        "INSERT INTO swarm_verifications(run_id,request_id,generation,revision,commit_sha,verifier_sha256,status,created_ms)
         VALUES(?1,?2,?3,?4,?5,?6,'running',?7)",
        params![run,request_id,generation,revision,commit,verifier_sha256,crate::daemon::now()],
    )?;
    Ok(PreparedVerification::Ready(VerificationPlan {
        run: run.to_string(),
        request_id: request_id.to_string(),
        generation,
        revision,
        commit,
        workspace,
        verifier,
        verifier_sha256,
    }))
}

fn bounded_output(file: &mut std::fs::File) -> Result<String> {
    file.seek(SeekFrom::Start(0))?;
    let mut bytes = Vec::new();
    file.take((MAX_OUTPUT_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    let truncated = bytes.len() > MAX_OUTPUT_BYTES;
    bytes.truncate(MAX_OUTPUT_BYTES);
    let mut text = String::from_utf8_lossy(&bytes).to_string();
    if truncated {
        text.push_str("\n[output truncated]");
    }
    Ok(text)
}

pub fn run(plan: &VerificationPlan) -> VerificationOutcome {
    run_inner(plan).unwrap_or_else(|error| VerificationOutcome {
        status: "interrupted",
        exit_code: None,
        stdout: String::new(),
        stderr: format!("verifier error: {error}"),
    })
}

fn run_inner(plan: &VerificationPlan) -> Result<VerificationOutcome> {
    let mut stdout = tempfile::tempfile()?;
    let mut stderr = tempfile::tempfile()?;
    let spawned = Command::new(&plan.verifier)
        .current_dir(&plan.workspace)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(stdout.try_clone()?)
        .stderr(stderr.try_clone()?)
        .spawn();
    let mut child = match spawned {
        Ok(child) => child,
        Err(error) => {
            return Ok(VerificationOutcome {
                status: "interrupted",
                exit_code: None,
                stdout: String::new(),
                stderr: format!("verifier could not start: {error}"),
            })
        }
    };
    let deadline = Instant::now() + Duration::from_secs(10);
    let (status, exit_code) = loop {
        match child.try_wait() {
            Ok(Some(exit)) => {
                break (
                    if exit.success() { "passed" } else { "failed" },
                    exit.code(),
                )
            }
            Ok(None) => {}
            Err(error) => {
                unsafe {
                    libc::kill(-(child.id() as i32), libc::SIGKILL);
                }
                let _ = child.wait();
                return Err(error.into());
            }
        }
        if Instant::now() >= deadline {
            unsafe {
                libc::kill(-(child.id() as i32), libc::SIGKILL);
            }
            let _ = child.wait();
            break ("interrupted", None);
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    Ok(VerificationOutcome {
        status,
        exit_code,
        stdout: bounded_output(&mut stdout)?,
        stderr: bounded_output(&mut stderr)?,
    })
}

pub fn record(
    store: &mut Store,
    plan: &VerificationPlan,
    outcome: VerificationOutcome,
) -> Result<Value> {
    let current = get(store, &plan.run)?;
    let saved: Option<(String, String)> = store
        .conn
        .query_row(
            "SELECT workspace_path,current_commit FROM swarm_integrations WHERE run_id=?1",
            [&plan.run],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let valid = current["generation"] == plan.generation
        && current["revision"] == plan.revision
        && (current["status"] == "planning" || current["status"] == "running")
        && saved.as_ref().is_some_and(|(path, commit)| {
            path == &plan.workspace.to_string_lossy() && commit == &plan.commit
        })
        && clean_at(&plan.workspace, &plan.commit).unwrap_or(false)
        && configured_verifier()
            .ok()
            .is_some_and(|(_, digest)| digest == plan.verifier_sha256);
    let status = if valid { outcome.status } else { "interrupted" };
    let changed = store.conn.execute(
        "UPDATE swarm_verifications SET status=?4,exit_code=?5,stdout=?6,stderr=?7,finished_ms=?8
         WHERE run_id=?1 AND request_id=?2 AND status='running' AND commit_sha=?3",
        params![
            plan.run,
            plan.request_id,
            plan.commit,
            status,
            outcome.exit_code,
            outcome.stdout,
            outcome.stderr,
            crate::daemon::now()
        ],
    )?;
    if changed != 1 {
        bail!("verification attempt changed before acknowledgement");
    }
    Ok(
        json!({"status":status,"commit":plan.commit,"exit_code":outcome.exit_code,
        "stdout":outcome.stdout,"stderr":outcome.stderr,"duplicate":false}),
    )
}

pub fn completion_passed(conn: &rusqlite::Connection, run: &str, revision: i64) -> Result<bool> {
    let saved: Option<(String, String)> = conn
        .query_row(
            "SELECT workspace_path,current_commit FROM swarm_integrations WHERE run_id=?1",
            [run],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let Some((workspace, commit)) = saved else {
        return Ok(false);
    };
    if !clean_at(Path::new(&workspace), &commit)? {
        return Ok(false);
    }
    let (_, digest) = configured_verifier()?;
    let latest: Option<(String, String, String, i64)> = conn
        .query_row(
            "SELECT status,commit_sha,verifier_sha256,revision FROM swarm_verifications
         WHERE run_id=?1 ORDER BY id DESC LIMIT 1",
            [run],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()?;
    Ok(latest.is_some_and(
        |(status, checked_commit, checked_verifier, checked_revision)| {
            status == "passed"
                && checked_commit == commit
                && checked_verifier == digest
                && checked_revision == revision
        },
    ))
}
