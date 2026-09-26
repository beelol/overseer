mod artifacts;
mod admission;
mod broker;
mod control;
mod director;
mod plan;
mod policy;
mod revision;
mod settings;
pub mod schema;
pub use artifacts::{confirm_exit, decide, put};
pub use admission::admit;
pub use broker::{ack, direct, messages, register, report};
pub use control::{off, pause, resume};
pub use director::{claim_batch, complete_batch};
pub use policy::preview;
pub use settings::set_policy;
pub use revision::revise;

use crate::store::Store;
use anyhow::{anyhow, bail, Result};
use plan::JobSpec;
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};

fn required<'a>(p: &'a Value, key: &str) -> Result<&'a str> {
    p[key]
        .as_str()
        .ok_or_else(|| anyhow!("missing string parameter {key}"))
}

fn row_run(row: &rusqlite::Row<'_>) -> rusqlite::Result<Value> {
    let targets: String = row.get("allowed_targets")?;
    let policy: String = row.get("policy")?;
    Ok(json!({
        "id": row.get::<_, String>("id")?,
        "category": row.get::<_, String>("category")?,
        "objective": row.get::<_, String>("objective")?,
        "status": row.get::<_, String>("status")?,
        "generation": row.get::<_, i64>("generation")?,
        "revision": row.get::<_, i64>("revision")?,
        "allowed_targets": serde_json::from_str::<Value>(&targets).unwrap_or(Value::Null),
        "needs_account_selection": serde_json::from_str::<Value>(&targets).ok().and_then(|v|v.as_array().map(|a|a.is_empty())).unwrap_or(true),
        "policy": serde_json::from_str::<Value>(&policy).unwrap_or(Value::Null),
        "created_ms": row.get::<_, i64>("created_ms")?,
        "updated_ms": row.get::<_, i64>("updated_ms")?,
    }))
}

pub fn create(store: &mut Store, p: &Value) -> Result<Value> {
    let category = required(p, "category")?.trim();
    let objective = required(p, "objective")?.trim();
    if category.is_empty() || category.len() > 160 || objective.is_empty() || objective.len() > 8000
    {
        bail!("category or objective is empty or too long");
    }
    let key = category.to_lowercase();
    let occupied: bool = store.conn.query_row(
        "SELECT 1 FROM swarm_runs WHERE category_key=?1 AND status IN ('planning','running','paused','stalled','draining','stopping') LIMIT 1",
        params![key], |_| Ok(()),
    ).optional()?.is_some();
    if occupied {
        bail!("category already has an active swarm run");
    }
    let (policy,targets) = settings::resolve(store,category,p.get("policy"),p.get("allowed_targets"))?;
    let id = format!("sw-{}", &uuid::Uuid::new_v4().simple().to_string()[..12]);
    let now = crate::daemon::now();
    store.conn.execute(
        "INSERT INTO swarm_runs(id,category,category_key,objective,status,generation,revision,allowed_targets,policy,created_ms,updated_ms) VALUES(?1,?2,?3,?4,'planning',1,0,?5,?6,?7,?7)",
        params![id,category,key,objective,targets.to_string(),policy.to_string(),now],
    )?;
    get(store, &id)
}

pub fn get(store: &Store, id: &str) -> Result<Value> {
    store
        .conn
        .query_row("SELECT * FROM swarm_runs WHERE id=?1", params![id], row_run)
        .optional()?
        .ok_or_else(|| anyhow!("unknown swarm run {id}"))
}

pub fn plan(store: &mut Store, p: &Value) -> Result<Value> {
    let id = required(p, "id")?;
    let generation = p["generation"]
        .as_i64()
        .ok_or_else(|| anyhow!("missing generation"))?;
    let revision = p["revision"]
        .as_i64()
        .ok_or_else(|| anyhow!("missing revision"))?;
    let jobs: Vec<JobSpec> =
        serde_json::from_value(p["jobs"].clone()).map_err(|e| anyhow!("invalid jobs: {e}"))?;
    plan::validate(&jobs)?;
    let tx = store.conn.transaction()?;
    let current: (i64, i64, String) = tx
        .query_row(
            "SELECT generation,revision,status FROM swarm_runs WHERE id=?1",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?
        .ok_or_else(|| anyhow!("unknown swarm run {id}"))?;
    if generation != current.0 {
        bail!("stale director generation");
    }
    if revision != current.1 {
        bail!("stale plan revision");
    }
    if !["planning", "running", "paused", "stalled"].contains(&current.2.as_str()) {
        bail!("swarm run is not plannable");
    }
    let progressed: bool = tx.query_row(
        "SELECT 1 FROM swarm_jobs WHERE run_id=?1 AND status NOT IN ('planned','ready') LIMIT 1",
        params![id], |_| Ok(()),
    ).optional()?.is_some();
    if progressed {
        bail!("cannot replace a plan with active or completed jobs; use a revision transition");
    }
    tx.execute("DELETE FROM swarm_jobs WHERE run_id=?1", params![id])?;
    let now = crate::daemon::now();
    for job in &jobs {
        let status = if job.deps.is_empty() {
            "ready"
        } else {
            "planned"
        };
        tx.execute(
            "INSERT INTO swarm_jobs(run_id,id,plan_revision,title,acceptance,deps,status,created_ms,updated_ms) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?8)",
            params![id,job.id,revision+1,job.title,job.acceptance,serde_json::to_string(&job.deps)?,status,now],
        )?;
    }
    tx.execute(
        "UPDATE swarm_runs SET revision=?2,updated_ms=?3 WHERE id=?1",
        params![id, revision + 1, now],
    )?;
    tx.commit()?;
    Ok(json!({"id":id,"generation":generation,"revision":revision+1,"job_count":jobs.len()}))
}

pub fn jobs(store: &Store, p: &Value) -> Result<Value> {
    let id = required(p, "id")?;
    get(store, id)?;
    let cursor = p["cursor"].as_str().unwrap_or("");
    let limit = p["limit"].as_i64().unwrap_or(50).clamp(1, 100);
    let mut stmt = store
        .conn
        .prepare("SELECT * FROM swarm_jobs WHERE run_id=?1 AND id>?2 ORDER BY id LIMIT ?3")?;
    let rows = stmt.query_map(params![id,cursor,limit+1], |r| {
        let deps: String = r.get("deps")?;
        Ok(json!({
            "id":r.get::<_,String>("id")?,"run_id":r.get::<_,String>("run_id")?,
            "plan_revision":r.get::<_,i64>("plan_revision")?,
            "title":r.get::<_,String>("title")?,"acceptance":r.get::<_,String>("acceptance")?,
            "deps":serde_json::from_str::<Value>(&deps).unwrap_or(Value::Null),
            "status":r.get::<_,String>("status")?,"attempt_count":r.get::<_,i64>("attempt_count")?,
        }))
    })?.collect::<rusqlite::Result<Vec<_>>>()?;
    let has_more = rows.len() as i64 > limit;
    let page: Vec<Value> = rows.into_iter().take(limit as usize).collect();
    let next_cursor = if has_more {
        page.last().and_then(|j| j["id"].as_str())
    } else {
        None
    };
    Ok(json!({"jobs":page,"next_cursor":next_cursor}))
}

pub fn stop(store: &mut Store, p: &Value) -> Result<Value> {
    let id = required(p, "run_id")?;
    let generation = p["generation"]
        .as_i64()
        .ok_or_else(|| anyhow!("missing generation"))?;
    let revision = p["revision"]
        .as_i64()
        .ok_or_else(|| anyhow!("missing revision"))?;
    let current = get(store, id)?;
    if current["generation"] != generation {
        bail!("stale director generation");
    }
    if current["revision"] != revision {
        bail!("stale plan revision");
    }
    if current["status"] == "stopping" {
        return Ok(json!({"id":id,"status":"stopping","duplicate":true}));
    }
    if current["status"] == "stopped" || current["status"] == "completed" {
        bail!("swarm run is terminal");
    }
    let now = crate::daemon::now();
    let tx = store.conn.transaction()?;
    tx.execute(
        "UPDATE swarm_runs SET status='stopping',updated_ms=?2 WHERE id=?1",
        params![id, now],
    )?;
    tx.execute("UPDATE swarm_jobs SET status='cancelled',updated_ms=?2 WHERE run_id=?1 AND status IN ('planned','ready')", params![id,now])?;
    tx.execute("UPDATE swarm_jobs SET status='cancel_requested',updated_ms=?2 WHERE run_id=?1 AND status IN ('reserved','launching','running')", params![id,now])?;
    tx.execute(
        "INSERT OR IGNORE INTO swarm_messages(run_id,message_id,job_id,attempt_id,sender,recipient,kind,revision,payload,phase,created_ms,updated_ms) SELECT a.run_id,'stop-'||a.id,a.job_id,a.id,'control',a.id,'stop',a.revision,'{}','queued',?2,?2 FROM swarm_attempts a WHERE a.run_id=?1 AND a.status='registered'",
        params![id,now],
    )?;
    tx.commit()?;
    Ok(json!({"id":id,"status":"stopping","duplicate":false}))
}

pub fn claim(store: &mut Store, p: &Value) -> Result<Value> {
    let run = required(p, "run_id")?;
    let job = required(p, "job_id")?;
    let resource = required(p, "resource")?.trim();
    let mode = required(p, "mode")?;
    let generation = p["generation"]
        .as_i64()
        .ok_or_else(|| anyhow!("missing generation"))?;
    let revision = p["revision"]
        .as_i64()
        .ok_or_else(|| anyhow!("missing revision"))?;
    if resource.is_empty() || resource.len() > 512 || resource.chars().any(char::is_control) {
        bail!("invalid resource claim");
    }
    if mode != "read" && mode != "write" {
        bail!("claim mode must be read or write");
    }
    let current = get(store, run)?;
    if current["generation"] != generation {
        bail!("stale director generation");
    }
    if current["revision"] != revision {
        bail!("stale plan revision");
    }
    if current["status"] != "planning" && current["status"] != "running" {
        bail!("swarm run does not permit claims");
    }
    let status: String = store
        .conn
        .query_row(
            "SELECT status FROM swarm_jobs WHERE run_id=?1 AND id=?2",
            params![run, job],
            |r| r.get(0),
        )
        .optional()?
        .ok_or_else(|| anyhow!("unknown job"))?;
    if [
        "cancelled",
        "cancel_requested",
        "superseded",
        "failed",
        "accepted",
    ]
    .contains(&status.as_str())
    {
        bail!("job cannot claim a resource in this state");
    }
    let tx = store.conn.transaction()?;
    let mut stmt = tx.prepare(
        "SELECT run_id,job_id,mode FROM swarm_claims WHERE resource=?1 AND status='active'",
    )?;
    let existing = stmt
        .query_map(params![resource], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(stmt);
    for (owner_run, owner_job, owner_mode) in &existing {
        if owner_run == run && owner_job == job {
            if owner_mode == mode {
                return Ok(json!({"resource":resource,"mode":mode,"duplicate":true}));
            }
            bail!("claim conflict: mode change requires release and revalidation");
        }
        if mode == "write" || owner_mode == "write" {
            bail!("claim conflict on {resource}: owned by {owner_run}/{owner_job}");
        }
    }
    let now = crate::daemon::now();
    tx.execute("INSERT INTO swarm_claims(resource,run_id,job_id,mode,status,revision,created_ms,updated_ms) VALUES(?1,?2,?3,?4,'active',?5,?6,?6)",
        params![resource,run,job,mode,revision,now])?;
    tx.commit()?;
    Ok(json!({"resource":resource,"mode":mode,"duplicate":false}))
}
