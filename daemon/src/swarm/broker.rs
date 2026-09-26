use crate::store::Store;
use anyhow::{anyhow, bail, Result};
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use super::{get, required};

fn token_hash(token: &str) -> String {
    format!("{:x}", Sha256::digest(token.as_bytes()))
}

pub(super) fn check_attempt(
    store: &Store,
    run: &str,
    job: &str,
    attempt: &str,
    token: &str,
) -> Result<i64> {
    let data: Option<(i64,String)> = store.conn.query_row(
        "SELECT revision,token_sha256 FROM swarm_attempts WHERE id=?1 AND run_id=?2 AND job_id=?3 AND status IN ('registered','finished')",
        params![attempt,run,job], |r| Ok((r.get(0)?,r.get(1)?)),
    ).optional()?;
    let (revision, hash) = data.ok_or_else(|| anyhow!("unknown or inactive attempt identity"))?;
    if token_hash(token) != hash {
        bail!("invalid attempt identity");
    }
    Ok(revision)
}

pub fn register(store: &mut Store, p: &Value) -> Result<Value> {
    let run = required(p, "run_id")?;
    let job = required(p, "job_id")?;
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
    if current["status"] != "planning" && current["status"] != "running" {
        bail!("swarm run does not permit new attempts");
    }
    let (status, job_revision): (String, i64) = store
        .conn
        .query_row(
            "SELECT status,plan_revision FROM swarm_jobs WHERE run_id=?1 AND id=?2",
            params![run, job],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?
        .ok_or_else(|| anyhow!("unknown job"))?;
    if status != "ready" {
        bail!("job is not ready");
    }
    let existing: i64 = store.conn.query_row(
        "SELECT COUNT(*) FROM swarm_attempts WHERE run_id=?1 AND job_id=?2",
        params![run, job],
        |r| r.get(0),
    )?;
    if existing >= 2 {
        bail!("job attempt limit reached");
    }
    let id = format!("att-{}", &uuid::Uuid::new_v4().simple().to_string()[..16]);
    let token = uuid::Uuid::new_v4().simple().to_string();
    let now = crate::daemon::now();
    let tx = store.conn.transaction()?;
    tx.execute("INSERT INTO swarm_attempts(id,run_id,job_id,revision,token_sha256,status,created_ms) VALUES(?1,?2,?3,?4,?5,'registered',?6)",
        params![id,run,job,job_revision,token_hash(&token),now])?;
    tx.execute("UPDATE swarm_jobs SET attempt_count=attempt_count+1,status='reserved',updated_ms=?3 WHERE run_id=?1 AND id=?2",params![run,job,now])?;
    tx.commit()?;
    Ok(
        json!({"id":id,"token":token,"run_id":run,"job_id":job,"revision":job_revision,"status":"registered"}),
    )
}

fn validate_envelope(p: &Value) -> Result<(&str, &str, i64, String)> {
    let id = required(p, "message_id")?;
    if id.is_empty() || id.len() > 128 {
        bail!("invalid message id");
    }
    let kind = required(p, "type")?;
    if ![
        "progress",
        "discovery",
        "result",
        "question",
        "blocker",
        "claim",
        "submit",
        "redirect",
        "checkpoint",
        "stop",
        "retract",
    ]
    .contains(&kind)
    {
        bail!("invalid message type");
    }
    let revision = p["revision"]
        .as_i64()
        .ok_or_else(|| anyhow!("missing revision"))?;
    let payload = p.get("payload").cloned().unwrap_or(json!({}));
    if !payload.is_object() {
        bail!("message payload must be an object");
    }
    let payload_text = payload.to_string();
    if payload_text.len() > 32 * 1024 {
        bail!("message payload exceeds 32 KiB");
    }
    let safe = crate::redact::redact(&payload_text);
    Ok((id, kind, revision, safe))
}

fn insert_message(
    store: &mut Store,
    p: &Value,
    sender: &str,
    recipient: &str,
    job: &str,
    attempt: &str,
) -> Result<Value> {
    let run = required(p, "run_id")?;
    let (id, kind, revision, payload) = validate_envelope(p)?;
    let current = get(store, run)?;
    if current["status"] == "stopped" || current["status"] == "completed" {
        bail!("swarm run is terminal");
    }
    let existing: Option<(String,String,String,String,i64,String,String,i64,String)> = store.conn.query_row(
        "SELECT sender,recipient,job_id,attempt_id,revision,kind,payload,seq,phase FROM swarm_messages WHERE run_id=?1 AND message_id=?2",
        params![run,id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?,r.get(8)?)),
    ).optional()?;
    if let Some((
        old_sender,
        old_recipient,
        old_job,
        old_attempt,
        old_rev,
        old_kind,
        old_payload,
        seq,
        phase,
    )) = existing
    {
        if (
            old_sender.as_str(),
            old_recipient.as_str(),
            old_job.as_str(),
            old_attempt.as_str(),
            old_rev,
            old_kind.as_str(),
            old_payload.as_str(),
        ) != (
            sender,
            recipient,
            job,
            attempt,
            revision,
            kind,
            payload.as_str(),
        ) {
            bail!("message id reused with different content");
        }
        return Ok(json!({"message_id":id,"seq":seq,"phase":phase,"duplicate":true}));
    }
    if recipient == "director" {
        let pending: i64=store.conn.query_row("SELECT COUNT(*) FROM swarm_messages WHERE run_id=?1 AND recipient='director' AND phase='queued'",params![run],|r|r.get(0))?;
        if pending >= 1000 && kind != "result" && kind != "submit" && kind != "blocker" {
            bail!("director inbox is full");
        }
    }
    let now = crate::daemon::now();
    store.conn.execute("INSERT INTO swarm_messages(run_id,message_id,job_id,attempt_id,sender,recipient,kind,revision,payload,phase,created_ms,updated_ms) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,'queued',?10,?10)",
        params![run,id,job,attempt,sender,recipient,kind,revision,payload,now])?;
    let seq = store.conn.last_insert_rowid();
    Ok(json!({"message_id":id,"seq":seq,"phase":"queued","duplicate":false}))
}

pub fn report(store: &mut Store, p: &Value) -> Result<Value> {
    let run = required(p, "run_id")?;
    let job = required(p, "job_id")?;
    let attempt = required(p, "attempt_id")?;
    let token = required(p, "token")?;
    let attempt_revision = check_attempt(store, run, job, attempt, token)?;
    let revision = p["revision"]
        .as_i64()
        .ok_or_else(|| anyhow!("missing revision"))?;
    if revision != attempt_revision {
        bail!("stale or incorrect attempt revision");
    }
    if ![
        "progress",
        "discovery",
        "result",
        "question",
        "blocker",
        "claim",
        "submit",
    ]
    .contains(&required(p, "type")?)
    {
        bail!("worker cannot send this message type");
    }
    insert_message(store, p, attempt, "director", job, attempt)
}

pub fn direct(store: &mut Store, p: &Value) -> Result<Value> {
    let run = required(p, "run_id")?;
    let job = required(p, "job_id")?;
    let attempt = required(p, "attempt_id")?;
    let current = get(store, run)?;
    if p["generation"] != current["generation"] {
        bail!("stale director generation");
    }
    if p["revision"] != current["revision"] {
        bail!("stale plan revision");
    }
    if current["status"] == "stopping" && p["type"] != "stop" && p["type"] != "checkpoint" {
        bail!("swarm run is stopping");
    }
    if !["redirect", "checkpoint", "stop", "retract"].contains(&required(p, "type")?) {
        bail!("director cannot send this message type");
    }
    let exists: bool = store
        .conn
        .query_row(
            "SELECT 1 FROM swarm_attempts WHERE id=?1 AND run_id=?2 AND job_id=?3",
            params![attempt, run, job],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    if !exists {
        bail!("unknown attempt identity");
    }
    insert_message(store, p, "director", attempt, job, attempt)
}

pub fn messages(store: &Store, p: &Value) -> Result<Value> {
    let run = required(p, "run_id")?;
    get(store, run)?;
    let recipient = required(p, "recipient")?;
    if recipient != "director" {
        let exists: bool = store
            .conn
            .query_row(
                "SELECT 1 FROM swarm_attempts WHERE id=?1 AND run_id=?2",
                params![recipient, run],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if !exists {
            bail!("unknown recipient");
        }
    }
    let cursor = p["cursor"].as_i64().unwrap_or(0).max(0);
    let limit = p["limit"].as_i64().unwrap_or(20).clamp(1, 100);
    let mut stmt=store.conn.prepare("SELECT seq,message_id,job_id,attempt_id,sender,recipient,kind,revision,payload,phase FROM swarm_messages WHERE run_id=?1 AND recipient=?2 AND seq>?3 ORDER BY seq LIMIT ?4")?;
    let rows=stmt.query_map(params![run,recipient,cursor,limit+1],|r|{
        let payload:String=r.get(8)?;
        Ok(json!({"seq":r.get::<_,i64>(0)?,"message_id":r.get::<_,String>(1)?,"job_id":r.get::<_,String>(2)?,"attempt_id":r.get::<_,String>(3)?,
            "sender":r.get::<_,String>(4)?,"recipient":r.get::<_,String>(5)?,"type":r.get::<_,String>(6)?,"revision":r.get::<_,i64>(7)?,
            "payload":serde_json::from_str::<Value>(&payload).unwrap_or(Value::Null),"phase":r.get::<_,String>(9)?}))
    })?.collect::<rusqlite::Result<Vec<_>>>()?;
    let more = rows.len() as i64 > limit;
    let page: Vec<Value> = rows.into_iter().take(limit as usize).collect();
    let next = if more {
        page.last().and_then(|m| m["seq"].as_i64())
    } else {
        None
    };
    Ok(json!({"messages":page,"next_cursor":next}))
}

pub fn ack(store: &mut Store, p: &Value) -> Result<Value> {
    let run = required(p, "run_id")?;
    let id = required(p, "message_id")?;
    let recipient = required(p, "recipient")?;
    let phase = required(p, "phase")?;
    let revision = p["revision"]
        .as_i64()
        .ok_or_else(|| anyhow!("missing revision"))?;
    let (owner, old_phase, msg_rev): (String, String, i64) = store
        .conn
        .query_row(
            "SELECT recipient,phase,revision FROM swarm_messages WHERE run_id=?1 AND message_id=?2",
            params![run, id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?
        .ok_or_else(|| anyhow!("unknown message"))?;
    if owner != recipient {
        bail!("wrong message recipient");
    }
    if revision != msg_rev {
        bail!("stale message revision");
    }
    if recipient != "director" {
        let token = required(p, "token")?;
        let (job,): (String,) = store.conn.query_row(
            "SELECT job_id FROM swarm_attempts WHERE id=?1 AND run_id=?2",
            params![recipient, run],
            |r| Ok((r.get(0)?,)),
        )?;
        check_attempt(store, run, &job, recipient, token)?;
    } else {
        let current = get(store, run)?;
        if p["generation"] != current["generation"] {
            bail!("stale director generation");
        }
    }
    if !["delivered", "applied"].contains(&phase) {
        bail!("invalid acknowledgement phase");
    }
    if old_phase == "applied" && phase == "delivered" {
        bail!("acknowledgement phase cannot move backward");
    }
    let target = if old_phase == "applied" {
        "applied"
    } else {
        phase
    };
    store.conn.execute(
        "UPDATE swarm_messages SET phase=?3,updated_ms=?4 WHERE run_id=?1 AND message_id=?2",
        params![run, id, target, crate::daemon::now()],
    )?;
    Ok(json!({"message_id":id,"phase":target}))
}
