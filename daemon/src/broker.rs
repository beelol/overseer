//! One durable broker for every envelope between an agent and its coordinator (SWARM-60;
//! Gate S's "Messages from agents: one broker. Reports, asks and claims use the rules Swarm's
//! broker has").
//!
//! Swarm's broker (`swarm_messages`) carries peer evidence, questions, directives and
//! advisories inside a swarm; Gate S's channel (`agent_messages`) carries an ordinary agent's
//! reports, asks and claims to Overseer. Each keeps its content where it was, and every
//! envelope of either also lives in `broker_envelopes`, the one ledger: one stable id per
//! envelope (`swarm/<run>/<message>` or `agent/<message>`), received once however often it is
//! repeated, and separate delivery and application states. Swarm envelopes enter and change
//! phase through triggers on its own table, in the same transaction as the change; Gate S's
//! enter by trigger and are marked delivered (shown in Overseer's conversation), applied (an
//! ask answered, a claim written, a report taken into Overseer's next turn) or refused (a claim
//! the claim ledger refused) where that happens.
//!
//! The same rules as Swarm's broker: the sender comes from the caller's token, never the text;
//! a body is at most [`MAX_BODY_BYTES`] and redacted before it is stored; a repeated envelope
//! has one effect. Envelope text is data: it cannot reassign a job, admit work or change a
//! claim.

use anyhow::Result;
use rusqlite::{params, Connection};
use serde_json::{json, Value};

/// The largest envelope body, as in Swarm's broker.
pub const MAX_BODY_BYTES: usize = 32 * 1024;

pub fn migrate(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS broker_envelopes(
          id TEXT PRIMARY KEY,
          origin TEXT NOT NULL CHECK(origin IN ('swarm','agent')),
          scope TEXT NOT NULL,
          message_id TEXT NOT NULL,
          sender TEXT NOT NULL,
          recipient TEXT NOT NULL,
          kind TEXT NOT NULL,
          phase TEXT NOT NULL CHECK(phase IN ('queued','delivered','applied','refused')),
          received_ms INTEGER NOT NULL,
          delivered_ms INTEGER,
          applied_ms INTEGER
        );
        CREATE INDEX IF NOT EXISTS broker_envelopes_scope ON broker_envelopes(origin, scope, received_ms);
        CREATE TRIGGER IF NOT EXISTS broker_swarm_received AFTER INSERT ON swarm_messages BEGIN
          INSERT OR IGNORE INTO broker_envelopes(id,origin,scope,message_id,sender,recipient,kind,phase,
            received_ms,delivered_ms,applied_ms)
          VALUES('swarm/'||NEW.run_id||'/'||NEW.message_id,'swarm',NEW.run_id,NEW.message_id,NEW.sender,
            NEW.recipient,NEW.kind,
            CASE WHEN NEW.phase IN ('delivered','applied') THEN NEW.phase ELSE 'queued' END,
            NEW.created_ms,
            CASE WHEN NEW.phase IN ('delivered','applied') THEN NEW.created_ms END,
            CASE WHEN NEW.phase='applied' THEN NEW.created_ms END);
        END;
        CREATE TRIGGER IF NOT EXISTS broker_swarm_phase AFTER UPDATE OF phase ON swarm_messages
          WHEN NEW.phase IS NOT OLD.phase AND NEW.phase IN ('queued','delivered','applied') BEGIN
          UPDATE broker_envelopes SET phase=NEW.phase,
            delivered_ms=COALESCE(delivered_ms, CASE WHEN NEW.phase IN ('delivered','applied') THEN NEW.updated_ms END),
            applied_ms=COALESCE(applied_ms, CASE WHEN NEW.phase='applied' THEN NEW.updated_ms END)
          WHERE id='swarm/'||NEW.run_id||'/'||NEW.message_id;
        END;
        CREATE TRIGGER IF NOT EXISTS broker_agent_received AFTER INSERT ON agent_messages BEGIN
          INSERT OR IGNORE INTO broker_envelopes(id,origin,scope,message_id,sender,recipient,kind,phase,received_ms)
          VALUES('agent/'||NEW.id,'agent',NEW.run_id,NEW.id,NEW.run_id,'overseer',NEW.kind,'queued',NEW.ts);
        END;
        CREATE TRIGGER IF NOT EXISTS broker_agent_answered AFTER UPDATE OF answer ON agent_messages
          WHEN NEW.answer IS NOT NULL AND OLD.answer IS NULL BEGIN
          UPDATE broker_envelopes SET phase='applied',
            delivered_ms=COALESCE(delivered_ms, NEW.answered_ms), applied_ms=NEW.answered_ms
          WHERE id='agent/'||NEW.id AND phase IN ('queued','delivered');
        END;
        CREATE TRIGGER IF NOT EXISTS broker_reports_taken AFTER INSERT ON overseer_turns BEGIN
          UPDATE broker_envelopes SET phase='applied', applied_ms=NEW.ts
          WHERE origin='agent' AND kind='report' AND phase='delivered' AND delivered_ms<=NEW.ts;
        END;
        -- Envelopes stored before the one ledger existed enter it once, with what their own
        -- tables knew.
        INSERT OR IGNORE INTO broker_envelopes(id,origin,scope,message_id,sender,recipient,kind,phase,
            received_ms,delivered_ms,applied_ms)
          SELECT 'swarm/'||run_id||'/'||message_id,'swarm',run_id,message_id,sender,recipient,kind,
            CASE WHEN phase IN ('delivered','applied') THEN phase ELSE 'queued' END,created_ms,
            CASE WHEN phase IN ('delivered','applied') THEN updated_ms END,
            CASE WHEN phase='applied' THEN updated_ms END
          FROM swarm_messages;
        INSERT OR IGNORE INTO broker_envelopes(id,origin,scope,message_id,sender,recipient,kind,phase,
            received_ms,delivered_ms,applied_ms)
          SELECT 'agent/'||id,'agent',run_id,id,run_id,'overseer',kind,
            CASE WHEN answered_ms IS NOT NULL THEN 'applied' ELSE 'delivered' END,ts,ts,answered_ms
          FROM agent_messages;
        "#,
    )?;
    Ok(())
}

/// The stable id of an ordinary agent's envelope.
pub fn agent_id(message_id: &str) -> String {
    format!("agent/{message_id}")
}

/// Move an envelope forward: `delivered` (its recipient has it), `applied` (it took effect) or
/// `refused` (it was received and will never take effect). A phase never moves back.
pub fn mark(conn: &Connection, id: &str, phase: &str) -> Result<()> {
    let now = crate::daemon::now();
    match phase {
        "delivered" => conn.execute(
            "UPDATE broker_envelopes SET phase='delivered', delivered_ms=COALESCE(delivered_ms,?2)
             WHERE id=?1 AND phase='queued'", params![id, now])?,
        "applied" => conn.execute(
            "UPDATE broker_envelopes SET phase='applied', delivered_ms=COALESCE(delivered_ms,?2),
             applied_ms=COALESCE(applied_ms,?2) WHERE id=?1 AND phase IN ('queued','delivered')", params![id, now])?,
        "refused" => conn.execute(
            "UPDATE broker_envelopes SET phase='refused', delivered_ms=COALESCE(delivered_ms,?2)
             WHERE id=?1 AND phase IN ('queued','delivered')", params![id, now])?,
        other => anyhow::bail!("no broker phase {other}"),
    };
    Ok(())
}

/// The one ledger's envelopes, oldest first, optionally of one origin or scope.
pub fn envelopes(conn: &Connection, p: &Value) -> Result<Value> {
    let origin = p["origin"].as_str();
    let scope = p["scope"].as_str();
    let limit = p["limit"].as_i64().unwrap_or(200).clamp(1, 1000);
    let mut stmt = conn.prepare(
        "SELECT id,origin,scope,message_id,sender,recipient,kind,phase,received_ms,delivered_ms,applied_ms
         FROM broker_envelopes WHERE (?1 IS NULL OR origin=?1) AND (?2 IS NULL OR scope=?2)
         ORDER BY received_ms, id LIMIT ?3")?;
    let rows = stmt.query_map(params![origin, scope, limit], |r| Ok(json!({
        "id": r.get::<_, String>(0)?, "origin": r.get::<_, String>(1)?, "scope": r.get::<_, String>(2)?,
        "message_id": r.get::<_, String>(3)?, "sender": r.get::<_, String>(4)?, "recipient": r.get::<_, String>(5)?,
        "kind": r.get::<_, String>(6)?, "phase": r.get::<_, String>(7)?, "received_ms": r.get::<_, i64>(8)?,
        "delivered_ms": r.get::<_, Option<i64>>(9)?, "applied_ms": r.get::<_, Option<i64>>(10)?})))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(json!({"envelopes": rows}))
}
