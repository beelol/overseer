use super::{get, required};
use crate::store::Store;
use anyhow::Result;
use rusqlite::params;
use serde_json::{json, Value};

pub fn report(store: &Store, p: &Value) -> Result<Value> {
    let run = required(p, "run_id")?;
    get(store, run)?;
    let mut stmt = store
        .conn
        .prepare("SELECT id,plan_revision,status FROM swarm_jobs WHERE run_id=?1 ORDER BY id")?;
    let jobs = stmt
        .query_map(params![run], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(stmt);
    let mut rows = Vec::with_capacity(jobs.len());
    for (job, revision, status) in jobs {
        let mut results_stmt = store.conn.prepare(
            "SELECT attempt_id,message_id,payload FROM swarm_messages WHERE run_id=?1 AND job_id=?2 AND revision=?3 AND sender=attempt_id AND kind='result' ORDER BY seq DESC",
        )?;
        let results = results_stmt
            .query_map(params![run, job, revision], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        drop(results_stmt);
        let current_attempt = results.first().map(|item| item.0.as_str());
        let mut selected = results.first();
        for item in &results {
            if Some(item.0.as_str()) != current_attempt {
                continue;
            }
            let candidate: Value = serde_json::from_str(&item.2)?;
            if candidate["audit_outcome"] == "environment_failure" {
                selected = Some(item);
                break;
            }
        }
        let (attempt, message, payload) = match selected {
            Some((attempt, message, raw)) => (
                Some(attempt.clone()),
                Some(message.clone()),
                serde_json::from_str::<Value>(raw)?,
            ),
            None => (None, None, Value::Null),
        };
        let outcome = payload["audit_outcome"].as_str();
        let state = match outcome {
            Some("environment_failure") => "environment_blocked",
            Some("negative") if status == "accepted" => "checked_negative",
            Some("negative") => "negative_awaiting_review",
            Some("confirmed_defect") if status == "accepted" => "confirmed_application_defect",
            Some("confirmed_defect") => "defect_awaiting_review",
            _ if payload.is_null() => "unreported",
            _ => "unclassified",
        };
        rows.push(json!({
            "job_id":job,"plan_revision":revision,"job_status":status,
            "attempt_id":attempt,"message_id":message,
            "audit_outcome":outcome,"coverage_state":state,
            "unavailable_resource":payload["unavailable_resource"],
            "artifact_ids":payload["artifact_ids"]
        }));
    }
    Ok(json!({"run_id":run,"rows":rows}))
}
