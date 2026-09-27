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

/// At daemon startup, an unlinked supervised owner still in `reserved` has
/// never reached the task/run link or supervisor request. Only this exact
/// state proves no process could have started; all later phases require the
/// ordinary uncertain-spawn or confirmed-exit reconciliation path.
pub fn recover_reserved_no_spawn(store: &mut Store) -> Result<usize> {
    let mut stmt = store.conn.prepare(
        "SELECT o.run_id,o.generation,s.revision FROM swarm_director_owners o
         JOIN swarm_runs s ON s.id=o.run_id AND s.generation=o.generation
         WHERE o.status='active' AND o.supervised_launch=1
           AND o.launch_phase='reserved' AND o.overseer_run_id IS NULL
           AND (s.status IN ('planning','running','paused','draining')
                OR (s.status='stalled' AND s.stall_reason='director_termination_unknown'))
         ORDER BY o.run_id",
    )?;
    let pending = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?, row.get::<_, i64>(2)?))
    })?.collect::<rusqlite::Result<Vec<_>>>()?;
    drop(stmt);
    let mut recovered = 0;
    for (run, generation, revision) in pending {
        recover(store, &json!({"run_id":run,"generation":generation,
            "revision":revision,"termination":"confirmed_no_spawn"}))?;
        recovered += 1;
    }
    Ok(recovered)
}

pub fn claim_batch(store: &mut Store, p: &Value) -> Result<Value> {
    let run = required(p, "run_id")?;
    super::owner::require(store,run,p)?;
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
    if current["status"] == "stopping" || current["status"] == "stopped" || current["status"] == "invalidated" {
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
                r.get::<_, Option<String>>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, i64>(5)?,
                r.get::<_, String>(6)?,
                r.get::<_, i64>(7)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(stmt);
    if rows.is_empty() {
        let blocked = tx.prepare("SELECT 1 FROM swarm_availability WHERE run_id=?1 AND state='blocked'")?
            .exists(params![run])?;
        if blocked {
            return Ok(json!({"status":"blocked","messages":[],
                "reason":current["availability"]["reason"]}));
        }
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
    let accepted_at_claim: i64 = tx.query_row(
        "SELECT COALESCE(MAX(id),0) FROM swarm_decisions WHERE run_id=?1 AND decision='accept'",
        params![run], |r| r.get(0))?;
    let conflicts_at_claim: i64 = tx.query_row(
        "SELECT COUNT(*) FROM swarm_conflicts WHERE run_id=?1 AND status='resolved'",
        params![run], |r| r.get(0))?;
    tx.execute("INSERT INTO swarm_director_turns(id,run_id,generation,revision,token_sha256,status,accepted_decision_id_at_claim,resolved_conflict_count_at_claim,created_ms) VALUES(?1,?2,?3,?4,?5,'active',?6,?7,?8)",
        params![id,run,generation,revision,hash(&token),accepted_at_claim,conflicts_at_claim,now])?;
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

/// Fixture recovery transition. A supervised director requires a linked process and persisted
/// exit before `confirmed_dead`; legacy unlinked fixtures supply their own termination evidence.
/// An unreachable process remains `unknown` with its capacity reserved.
pub fn recover(store: &mut Store, p: &Value) -> Result<Value> {
    let run = required(p, "run_id")?;
    let generation = p["generation"]
        .as_i64()
        .ok_or_else(|| anyhow!("missing generation"))?;
    let revision = p["revision"]
        .as_i64()
        .ok_or_else(|| anyhow!("missing revision"))?;
    let termination = required(p, "termination")?;
    if termination != "unknown" && termination != "confirmed_dead"
        && termination != "confirmed_no_spawn" {
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
    if current["status"] == "stalled" && current["stall_reason"] != "director_termination_unknown" {
        bail!("director is stalled by planning or no-progress policy; process recovery cannot clear it");
    }
    let mut confirmed_process = None;
    if termination != "unknown" {
        let linked: Option<(Option<String>,i64,Option<String>)> = store.conn.query_row(
            "SELECT overseer_run_id,supervised_launch,launch_phase FROM swarm_director_owners
             WHERE run_id=?1 AND generation=?2 AND status='active'",
            params![run,generation], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?)),
        ).optional()?;
        if termination == "confirmed_no_spawn" {
            match linked.as_ref() {
                Some((None,1,Some(phase))) if phase == "reserved" => {}
                Some((Some(process),1,Some(phase))) if phase == "linked" => {
                    let dir: Option<String> = store.conn.query_row(
                        "SELECT run_dir FROM runs WHERE id=?1",
                        [process], |r| r.get(0))?;
                    if dir.is_some() {
                        bail!("director launch has a recorded process directory");
                    }
                }
                _ => bail!("director launch may have spawned; termination remains uncertain"),
            }
        } else {
            if matches!(linked.as_ref(),Some((None,1,_))) {
                bail!("supervised director launch has no linked process; use no-spawn reconciliation");
            }
            if let Some((Some(process),_,_)) = linked {
                let (status, ended, dir): (String,Option<i64>,Option<String>) = store.conn.query_row(
                    "SELECT status,ended_ms,run_dir FROM runs WHERE id=?1",
                    [&process], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?)),
                )?;
                let exit_recorded = dir.as_deref().is_some_and(|path|
                    std::path::Path::new(path).join("exit.json").exists());
                if ended.is_none() || !["completed","failed","interrupted"].contains(&status.as_str()) || !exit_recorded {
                    bail!("linked director process has no confirmed exit");
                }
                if super::runtime::descendant_receipts(&store.conn,&process)?.0 > 0 {
                    bail!("linked director has unconfirmed native descendants");
                }
                confirmed_process = Some(process);
            }
        }
    }
    let now = crate::daemon::now();
    let tx = store.conn.transaction()?;
    if termination == "unknown" {
        tx.execute(
            "UPDATE swarm_runs SET stalled_from=CASE WHEN status='stalled' THEN stalled_from ELSE status END,
             status='stalled',stall_reason='director_termination_unknown',updated_ms=?2 WHERE id=?1",
            params![run, now],
        )?;
        tx.commit()?;
        return Ok(
            json!({"status":"stalled","generation":generation,"reason":"director_termination_unknown"}),
        );
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
    if let Some(process) = confirmed_process {
        let mut stmt = tx.prepare(
            "SELECT a.id,a.job_id,a.revision,EXISTS(
                 SELECT 1 FROM swarm_messages m WHERE m.run_id=a.run_id
                   AND m.attempt_id=a.id AND m.kind='result') FROM swarm_attempts a
             WHERE a.run_id=?1 AND a.executor='director' AND a.executor_run_id=?2
               AND a.status='registered'",
        )?;
        let unfinished = stmt.query_map(params![run,process], |r|
            Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,
                r.get::<_,i64>(2)?,r.get::<_,i64>(3)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        drop(stmt);
        for (attempt,job,attempt_revision,has_result) in unfinished {
            let message_id = format!("terminal-self-{attempt}");
            let payload = json!({"overseer_run_id":process,"reason":"director_process_exited",
                "result_submitted":has_result != 0,"usage":"uncertain"});
            tx.execute("INSERT OR IGNORE INTO swarm_messages(run_id,message_id,job_id,attempt_id,
                sender,recipient,kind,revision,payload,phase,created_ms,updated_ms)
                VALUES(?1,?2,?3,?4,'runtime','director','terminal',?5,?6,'queued',?7,?7)",
                params![run,message_id,job,attempt,attempt_revision,payload.to_string(),now])?;
            if has_result != 0 { continue; }
            tx.execute("UPDATE swarm_attempts SET status='finished' WHERE id=?1 AND status='registered'",
                [&attempt])?;
            tx.execute("UPDATE swarm_reservations SET status='uncertain'
                WHERE attempt_id=?1 AND run_id=?2 AND status='active'",
                params![attempt,run])?;
            let unsafe_effects: i64 = tx.query_row(
                "SELECT COUNT(*) FROM swarm_effects WHERE run_id=?1 AND job_id=?2
                 AND outcome IN ('unknown','applied')",params![run,job], |r|r.get(0))?;
            let (job_status,stop_reason,count,deadline,deps_raw): (String,Option<String>,i64,Option<i64>,String) = tx.query_row(
                "SELECT status,stop_reason,attempt_count,deadline_at_ms,deps FROM swarm_jobs
                 WHERE run_id=?1 AND id=?2",params![run,job],
                |r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?)))?;
            if ["reserved","launching","running","cancel_requested"].contains(&job_status.as_str()) {
                let deps: Vec<String> = serde_json::from_str(&deps_raw)?;
                let deps_ready = deps.iter().try_fold(true, |ready, dep| {
                    Ok::<bool,anyhow::Error>(ready && super::artifacts::dep_satisfied(&tx,run,dep)?)
                })?;
                let next = if unsafe_effects > 0 { "blocked" }
                    else if job_status == "cancel_requested" {
                        match stop_reason.as_deref() {
                            Some("scope_narrowed") => "superseded",
                            Some("job_deadline") => "failed",
                            Some("resource_contamination" | "account_identity_revoked") => {
                                if count >= 2 || deadline.is_some_and(|at| now >= at) { "failed" }
                                else if deps_ready { "ready" } else { "planned" }
                            }
                            _ => "blocked",
                        }
                    }
                    else if current["status"] == "draining" || current["stalled_from"] == "draining" { "cancelled" }
                    else if count >= 2 || deadline.is_some_and(|at| now >= at) { "failed" }
                    else if deps_ready { "ready" } else { "planned" };
                tx.execute("UPDATE swarm_jobs SET status=?3,updated_ms=?4
                    WHERE run_id=?1 AND id=?2",params![run,job,next,now])?;
                if unsafe_effects == 0 {
                    tx.execute("UPDATE swarm_claims SET status='released',updated_ms=?3
                        WHERE run_id=?1 AND job_id=?2 AND status='active'",
                        params![run,job,now])?;
                }
            }
        }
    }
    let workers: i64 = tx.query_row(
        "SELECT COUNT(*) FROM swarm_attempts WHERE run_id=?1 AND status='registered'",
        params![run],
        |r| r.get(0),
    )?;
    let prior_status = if current["status"] == "stalled" {
        current["stalled_from"].as_str().unwrap_or("")
    } else {
        current["status"].as_str().unwrap_or("")
    };
    let next_status = if prior_status == "draining" {
        "draining"
    } else if prior_status == "paused" {
        "paused"
    } else if workers > 0 {
        "running"
    } else {
        "planning"
    };
    let owner_bound: bool = tx.prepare(
        "SELECT 1 FROM swarm_director_owners WHERE run_id=?1 AND generation=?2 AND status='active'"
    )?.exists(params![run,generation])?;
    if owner_bound {
        tx.execute("UPDATE swarm_runs SET generation=generation+1,status='stalled',
            stalled_from=?2,stall_reason='director_replacement_pending',updated_ms=?3 WHERE id=?1",
            params![run,next_status,now])?;
        tx.execute("UPDATE swarm_director_owners SET status='released' WHERE run_id=?1 AND generation=?2",
            params![run,generation])?;
    } else {
        tx.execute(
            "UPDATE swarm_runs SET generation=generation+1,status=?2,stalled_from=NULL,stall_reason=NULL,updated_ms=?3 WHERE id=?1",
            params![run, next_status, now],
        )?;
    }
    tx.commit()?;
    Ok(json!({"status":if owner_bound { "stalled" } else { next_status },
        "generation":generation+1,"replacement_pending":owner_bound,
        "replayed_turn":turn,"workers_preserved":workers}))
}

pub fn complete_batch(store: &mut Store, p: &Value) -> Result<Value> {
    let run = required(p, "run_id")?;
    super::owner::require(store,run,p)?;
    let id = required(p, "turn_id")?;
    let token = required(p, "token")?;
    let generation = p["generation"]
        .as_i64()
        .ok_or_else(|| anyhow!("missing generation"))?;
    let outcome = p["outcome"].as_str().unwrap_or("no_progress");
    if outcome != "progress" && outcome != "no_progress" {
        bail!("invalid director turn outcome");
    }
    let current = get(store, run)?;
    if current["generation"] != generation {
        bail!("stale director generation");
    }
    let (stored_hash, status, turn_revision, accepted_at_claim, conflicts_at_claim, stored_applied, stored_pending): (String, String, i64, i64, i64, i64, i64) = store
        .conn
        .query_row(
            "SELECT token_sha256,status,revision,accepted_decision_id_at_claim,resolved_conflict_count_at_claim,applied_count,pending_review_count FROM swarm_director_turns WHERE id=?1 AND run_id=?2 AND generation=?3",
            params![id,run,generation],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?)),
        )
        .optional()?
        .ok_or_else(|| anyhow!("unknown director turn"))?;
    if stored_hash != hash(token) {
        bail!("invalid director turn identity");
    }
    if status == "complete" {
        return Ok(json!({"turn_id":id,"applied":stored_applied,
            "pending_review":stored_pending,"duplicate":true,
            "status":current["status"],"no_progress_turns":current["no_progress_turns"]}));
    }
    if current["status"] == "stalled" {
        bail!("director is stalled pending recovery");
    }
    let now = crate::daemon::now();
    let tx = store.conn.transaction()?;
    // A claimed batch is only delivery. A current terminal report remains reviewable
    // until a durable decision covers that report sequence. A superseded report
    // cannot receive a decision for the revised job, but the director still sees
    // its original revision once before this acknowledgement. Contaminated attempts
    // are similarly quarantined. Otherwise a completed turn could silently consume
    // the only notification for a result that can still be reviewed.
    let pending_review = if current["status"] == "stopping" || current["status"] == "stopped" || current["status"] == "invalidated" {
        0
    } else {
        tx.execute(
            "UPDATE swarm_messages AS m SET phase='queued',updated_ms=?2
             WHERE m.seq IN (SELECT seq FROM swarm_director_turn_messages WHERE turn_id=?1)
             AND m.phase='delivered' AND m.kind IN ('result','submit')
             AND EXISTS (SELECT 1 FROM swarm_jobs j WHERE j.run_id=m.run_id
                 AND j.id=m.job_id AND j.plan_revision=m.revision)
             AND NOT EXISTS (SELECT 1 FROM swarm_resource_contamination c
                 WHERE c.run_id=m.run_id AND c.job_id=m.job_id AND c.attempt_id=m.attempt_id)
             AND NOT EXISTS (SELECT 1 FROM swarm_decisions d WHERE d.run_id=m.run_id
                 AND d.job_id=m.job_id AND d.attempt_id=m.attempt_id
                 AND d.reviewed_message_seq>=m.seq)",
            params![id,now],
        )?
    };
    let applied = tx.execute("UPDATE swarm_messages SET phase='applied',updated_ms=?2 WHERE seq IN (SELECT seq FROM swarm_director_turn_messages WHERE turn_id=?1) AND phase='delivered'",params![id,now])?;
    tx.execute("UPDATE swarm_director_turns SET status='complete',completed_ms=?2,
        applied_count=?3,pending_review_count=?4 WHERE id=?1 AND status='active'",
        params![id,now,applied as i64,pending_review as i64])?;
    if current["status"] == "stopping" || current["status"] == "stopped" || current["status"] == "invalidated" {
        tx.commit()?;
        return Ok(json!({"turn_id":id,"applied":applied,"pending_review":0,"duplicate":false,
            "status":current["status"],"no_progress_turns":current["no_progress_turns"]}));
    }
    let accepted_now: i64 = tx.query_row(
        "SELECT COALESCE(MAX(id),0) FROM swarm_decisions WHERE run_id=?1 AND decision='accept'",
        params![run], |r| r.get(0))?;
    let conflicts_now: i64 = tx.query_row(
        "SELECT COUNT(*) FROM swarm_conflicts WHERE run_id=?1 AND status='resolved'",
        params![run], |r| r.get(0))?;
    let material_progress = current["revision"].as_i64().unwrap_or(0) > turn_revision
        || accepted_now > accepted_at_claim
        || conflicts_now > conflicts_at_claim;
    let turns = if !material_progress {
        current["no_progress_turns"]
            .as_i64()
            .unwrap_or(0)
            .saturating_add(1)
    } else {
        0
    };
    let next_status = if turns >= 2 {
        "stalled"
    } else {
        current["status"].as_str().unwrap_or("running")
    };
    if turns >= 2 {
        tx.execute(
            "UPDATE swarm_runs SET status='stalled',stalled_from=status,
            stall_reason='director_no_progress',no_progress_turns=?2,updated_ms=?3 WHERE id=?1",
            params![run, turns, now],
        )?;
    } else {
        tx.execute(
            "UPDATE swarm_runs SET no_progress_turns=?2,updated_ms=?3 WHERE id=?1",
            params![run, turns, now],
        )?;
    }
    tx.commit()?;
    Ok(json!({"turn_id":id,"applied":applied,"pending_review":pending_review,"duplicate":false,
        "status":next_status,"no_progress_turns":turns,
        "material_progress":material_progress,"declared_outcome":outcome}))
}
