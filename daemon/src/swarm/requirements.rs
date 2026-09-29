//! The owner's changed requirements (SWARM-21): user intake that becomes a plan
//! revision. The owner's words enter the director's durable inbox as a
//! `requirement` from `owner`; until the director revises the plan against them
//! (naming the request), no new work is admitted under the old requirements. The
//! revision records which request it applied, so the plan keeps its provenance.
//! The director generation does not change.

use super::{get, required};
use crate::store::Store;
use anyhow::{anyhow, bail, Result};
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};

pub fn change(store: &mut Store, p: &Value) -> Result<Value> {
    let run = required(p, "run_id")?;
    let request_id = required(p, "request_id")?;
    if request_id.is_empty() || request_id.len() > 128 || request_id.chars().any(char::is_control)
        || crate::redact::redact(request_id) != request_id {
        bail!("invalid requirement request id");
    }
    let text = required(p, "text")?.trim();
    if text.is_empty() || text.len() > 8000 {
        bail!("a requirement change needs text of at most 8,000 bytes");
    }
    let text = crate::redact::redact(text);
    let current = get(store, run)?;
    let prior: Option<(String, Option<i64>)> = store.conn.query_row(
        "SELECT text,applied_revision FROM swarm_requirement_changes WHERE run_id=?1 AND request_id=?2",
        params![run, request_id], |r| Ok((r.get(0)?, r.get(1)?))).optional()?;
    if let Some((old, applied)) = prior {
        if old != text {
            bail!("requirement request id reused with different text");
        }
        return Ok(json!({"run_id":run,"request_id":request_id,"applied_revision":applied,"duplicate":true}));
    }
    if !matches!(current["status"].as_str(), Some("planning" | "running" | "paused" | "stalled")) {
        bail!("swarm run cannot take a requirement change in this state");
    }
    let now = crate::daemon::now();
    let revision = current["revision"].as_i64().unwrap_or(0);
    let tx = store.conn.transaction()?;
    tx.execute("INSERT INTO swarm_requirement_changes(run_id,request_id,text,created_ms) VALUES(?1,?2,?3,?4)",
        params![run, request_id, text, now])?;
    tx.execute("INSERT INTO swarm_messages(run_id,message_id,job_id,attempt_id,sender,recipient,kind,revision,payload,phase,created_ms,updated_ms)
        VALUES(?1,?2,NULL,NULL,'owner','director','requirement',?3,?4,'queued',?5,?5)",
        params![run, format!("requirement-{request_id}"), revision,
            json!({"request_id":request_id,"text":text}).to_string(), now])?;
    tx.commit()?;
    Ok(json!({"run_id":run,"request_id":request_id,"applied_revision":Value::Null,
        "admission":"held_until_revised","duplicate":false}))
}

/// Requirement changes still waiting for the director's revision.
pub(super) fn pending(conn: &rusqlite::Connection, run: &str) -> Result<bool> {
    Ok(conn.prepare("SELECT 1 FROM swarm_requirement_changes WHERE run_id=?1 AND applied_revision IS NULL")?
        .exists(params![run])?)
}

/// The director's revision names the requests it applies; each must be pending.
pub(super) fn requested(p: &Value) -> Result<Vec<String>> {
    let Some(value) = p.get("requirements") else { return Ok(Vec::new()) };
    let list = value.as_array().filter(|l| !l.is_empty() && l.len() <= 32)
        .ok_or_else(|| anyhow!("requirements must be a bounded list of request ids"))?;
    list.iter().map(|v| v.as_str().map(str::to_string)
        .ok_or_else(|| anyhow!("requirements must be request ids"))).collect()
}

pub(super) fn check_pending(conn: &rusqlite::Connection, run: &str, ids: &[String]) -> Result<()> {
    for id in ids {
        let applied: Option<Option<i64>> = conn.query_row(
            "SELECT applied_revision FROM swarm_requirement_changes WHERE run_id=?1 AND request_id=?2",
            params![run, id], |r| r.get(0)).optional()?;
        match applied {
            None => bail!("unknown requirement change {id}"),
            Some(Some(_)) => bail!("requirement change {id} was already applied"),
            Some(None) => {}
        }
    }
    Ok(())
}

pub(super) fn mark_applied(conn: &rusqlite::Connection, run: &str, ids: &[String], revision: i64, now: i64) -> Result<()> {
    for id in ids {
        conn.execute("UPDATE swarm_requirement_changes SET applied_revision=?3,applied_ms=?4
            WHERE run_id=?1 AND request_id=?2 AND applied_revision IS NULL", params![run, id, revision, now])?;
    }
    Ok(())
}

pub(super) fn list(conn: &rusqlite::Connection, run: &str) -> Result<Value> {
    let mut stmt = conn.prepare("SELECT request_id,text,created_ms,applied_revision,applied_ms
        FROM swarm_requirement_changes WHERE run_id=?1 ORDER BY created_ms,request_id LIMIT 100")?;
    let rows = stmt.query_map(params![run], |r| Ok(json!({"request_id":r.get::<_,String>(0)?,
        "text":r.get::<_,String>(1)?,"created_ms":r.get::<_,i64>(2)?,
        "applied_revision":r.get::<_,Option<i64>>(3)?,"applied_ms":r.get::<_,Option<i64>>(4)?})))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(json!(rows))
}
