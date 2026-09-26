use super::{get, required};
use crate::store::Store;
use anyhow::{anyhow, bail, Result};
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

const MAX_BATCH: usize = 20;
const MAX_INLINE_BYTES: usize = 32 * 1024;
const MAX_DELAY_MS: i64 = 5_000;

fn hash(token: &str) -> String {
    format!("{:x}", Sha256::digest(token.as_bytes()))
}

pub fn claim_batch(store: &mut Store, p: &Value) -> Result<Value> {
    let run = required(p, "run_id")?;
    let generation = p["generation"]
        .as_i64()
        .ok_or_else(|| anyhow!("missing generation"))?;
    let revision = p["revision"]
        .as_i64()
        .ok_or_else(|| anyhow!("missing revision"))?;
    let now = p["now_ms"]
        .as_i64()
        .ok_or_else(|| anyhow!("missing batch time"))?;
    let current = get(store, run)?;
    if current["generation"] != generation {
        bail!("stale director generation");
    }
    if current["revision"] != revision {
        bail!("stale plan revision");
    }
    if current["status"] == "stalled" {
        return Ok(json!({"status":"stalled","messages":[]}));
    }
    if current["status"] == "stopping" || current["status"] == "stopped" {
        return Ok(json!({"status":"halted","messages":[]}));
    }
    let tx = store.conn.transaction()?;
    let active: Option<String> = tx
        .query_row(
            "SELECT id FROM swarm_director_turns WHERE run_id=?1 AND status='active'",
            params![run],
            |r| r.get(0),
        )
        .optional()?;
    if active.is_some() {
        return Ok(json!({"status":"busy","messages":[]}));
    }
    let mut stmt = tx.prepare(
        "SELECT seq,message_id,job_id,attempt_id,kind,revision,payload,created_ms FROM swarm_messages WHERE run_id=?1 AND recipient='director' AND phase='queued' ORDER BY seq LIMIT 21",
    )?;
    let rows = stmt
        .query_map(params![run], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, i64>(5)?,
                r.get::<_, String>(6)?,
                r.get::<_, i64>(7)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(stmt);
    if rows.is_empty() {
        return Ok(json!({"status":"idle","messages":[]}));
    }
    let oldest = rows[0].7;
    let mut batch = Vec::new();
    let mut seqs = Vec::new();
    let mut bytes = 0;
    for (seq, message_id, job_id, attempt_id, kind, msg_rev, payload, _) in &rows {
        if batch.len() == MAX_BATCH {
            break;
        }
        if !batch.is_empty() && bytes + payload.len() > MAX_INLINE_BYTES {
            break;
        }
        bytes += payload.len();
        seqs.push(*seq);
        batch.push(json!({"seq":seq,"message_id":message_id,"job_id":job_id,
            "attempt_id":attempt_id,"type":kind,"revision":msg_rev,
            "payload":serde_json::from_str::<Value>(payload).unwrap_or(Value::Null)}));
    }
    let filled = rows.len() >= MAX_BATCH || bytes >= MAX_INLINE_BYTES || batch.len() < rows.len();
    if !filled && now.saturating_sub(oldest) < MAX_DELAY_MS {
        return Ok(
            json!({"status":"waiting","messages":[],"retry_after_ms":MAX_DELAY_MS-now.saturating_sub(oldest)}),
        );
    }
    let id = format!("dt-{}", &uuid::Uuid::new_v4().simple().to_string()[..16]);
    let token = uuid::Uuid::new_v4().simple().to_string();
    tx.execute("INSERT INTO swarm_director_turns(id,run_id,generation,revision,token_sha256,status,created_ms) VALUES(?1,?2,?3,?4,?5,'active',?6)",
        params![id,run,generation,revision,hash(&token),now])?;
    for seq in seqs {
        tx.execute(
            "INSERT INTO swarm_director_turn_messages(turn_id,seq) VALUES(?1,?2)",
            params![id, seq],
        )?;
        tx.execute("UPDATE swarm_messages SET phase='delivered',updated_ms=?2 WHERE seq=?1 AND phase='queued'",params![seq,now])?;
    }
    tx.commit()?;
    Ok(
        json!({"status":"claimed","turn_id":id,"token":token,"messages":batch,
        "more_pending":rows.len()>batch.len(),"inline_bytes":bytes}),
    )
}

/// Fixture recovery transition. A live caller must prove process termination before choosing
/// `confirmed_dead`; an unreachable process remains `unknown` with its capacity reserved.
pub fn recover(store: &mut Store, p: &Value) -> Result<Value> {
    let run = required(p, "run_id")?;
    let generation = p["generation"]
        .as_i64()
        .ok_or_else(|| anyhow!("missing generation"))?;
    let revision = p["revision"]
        .as_i64()
        .ok_or_else(|| anyhow!("missing revision"))?;
    let termination = required(p, "termination")?;
    if termination != "unknown" && termination != "confirmed_dead" {
        bail!("invalid director termination evidence");
    }
    let current = get(store, run)?;
    if current["generation"] != generation {
        bail!("stale director generation");
    }
    if current["revision"] != revision {
        bail!("stale plan revision");
    }
    if !["planning", "running", "paused", "stalled", "draining"]
        .contains(&current["status"].as_str().unwrap_or(""))
    {
        bail!("run cannot recover director in this state");
    }
    let now = crate::daemon::now();
    let tx = store.conn.transaction()?;
    if termination == "unknown" {
        tx.execute(
            "UPDATE swarm_runs SET status='stalled',updated_ms=?2 WHERE id=?1",
            params![run, now],
        )?;
        tx.commit()?;
        return Ok(json!({"status":"stalled","generation":generation,"reason":"director_termination_unknown"}));
    }
    let turn: Option<String> = tx
        .query_row(
            "SELECT id FROM swarm_director_turns WHERE run_id=?1 AND status='active'",
            params![run],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(turn_id) = &turn {
        tx.execute(
            "UPDATE swarm_messages SET phase='queued',updated_ms=?2 WHERE seq IN
             (SELECT seq FROM swarm_director_turn_messages WHERE turn_id=?1) AND phase='delivered'",
            params![turn_id, now],
        )?;
        tx.execute(
            "UPDATE swarm_director_turns SET status='complete',completed_ms=?2 WHERE id=?1",
            params![turn_id, now],
        )?;
    }
    let workers: i64 = tx.query_row(
        "SELECT COUNT(*) FROM swarm_attempts WHERE run_id=?1 AND status='registered'",
        params![run],
        |r| r.get(0),
    )?;
    let next_status = if current["status"] == "draining" {
        "draining"
    } else if current["status"] == "paused" {
        "paused"
    } else if workers > 0 {
        "running"
    } else {
        "planning"
    };
    tx.execute(
        "UPDATE swarm_runs SET generation=generation+1,status=?2,updated_ms=?3 WHERE id=?1",
        params![run, next_status, now],
    )?;
    tx.commit()?;
    Ok(json!({"status":next_status,"generation":generation+1,
        "replayed_turn":turn,"workers_preserved":workers}))
}

pub fn complete_batch(store: &mut Store, p: &Value) -> Result<Value> {
    let run = required(p, "run_id")?;
    let id = required(p, "turn_id")?;
    let token = required(p, "token")?;
    let generation = p["generation"]
        .as_i64()
        .ok_or_else(|| anyhow!("missing generation"))?;
    let current = get(store, run)?;
    if current["generation"] != generation {
        bail!("stale director generation");
    }
    if current["status"] == "stalled" {
        bail!("director is stalled pending recovery");
    }
    let (stored_hash, status): (String, String) = store
        .conn
        .query_row(
            "SELECT token_sha256,status FROM swarm_director_turns WHERE id=?1 AND run_id=?2 AND generation=?3",
            params![id,run,generation],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?
        .ok_or_else(|| anyhow!("unknown director turn"))?;
    if stored_hash != hash(token) {
        bail!("invalid director turn identity");
    }
    let count: i64 = store.conn.query_row(
        "SELECT COUNT(*) FROM swarm_director_turn_messages WHERE turn_id=?1",
        params![id],
        |r| r.get(0),
    )?;
    if status == "complete" {
        return Ok(json!({"turn_id":id,"applied":count,"duplicate":true}));
    }
    let now = crate::daemon::now();
    let tx = store.conn.transaction()?;
    tx.execute("UPDATE swarm_messages SET phase='applied',updated_ms=?2 WHERE seq IN (SELECT seq FROM swarm_director_turn_messages WHERE turn_id=?1) AND phase='delivered'",params![id,now])?;
    tx.execute("UPDATE swarm_director_turns SET status='complete',completed_ms=?2 WHERE id=?1 AND status='active'",params![id,now])?;
    tx.commit()?;
    Ok(json!({"turn_id":id,"applied":count,"duplicate":false}))
}
