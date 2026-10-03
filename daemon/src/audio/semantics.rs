//! Private, bounded current semantic state. Existing normalized events provide
//! causal identity; this is not another event/recovery/account ledger.
use super::lines::Line;
use crate::store::{Event, Run, Store};
use anyhow::Result;
use rusqlite::{params, OptionalExtension};
use serde_json::json;

const MAX_ROWS: i64 = 4096;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Ticket {
    pub subject: String,
    pub slot: String,
    pub seq: i64,
}

/// A settlement caller must identify its actual decision. Unknown/pending
/// recovery callers use None and cannot acquire authority from status text.
#[derive(Clone, Copy)]
pub(crate) enum Settlement {
    Success,
    FailedWithoutAutomaticRecovery,
    AuthenticationWithoutPermittedFallback,
    LostGenericExecutionWithoutRestore,
    ExhaustedConnectionRecovery,
    ExhaustedMemoryRecovery,
}

pub(crate) fn migrate(conn: &rusqlite::Connection) -> Result<()> {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS audio_semantic_state(
        subject TEXT NOT NULL, slot TEXT NOT NULL, run_id TEXT NOT NULL,
        generation INTEGER NOT NULL, line TEXT NOT NULL, identity TEXT NOT NULL,
        event_seq INTEGER NOT NULL, live INTEGER NOT NULL DEFAULT 1,
        played_seq INTEGER NOT NULL DEFAULT 0, updated_ms INTEGER NOT NULL,
        PRIMARY KEY(subject,slot));")?;
    Ok(())
}

fn subject(store: &Store, run: &Run, routine: bool) -> Result<Option<(String, bool)>> {
    if run.parent_run_id.is_some() { return Ok(None); }
    let director: Option<String> = store.conn.query_row(
        "SELECT o.run_id FROM swarm_director_owners o JOIN swarm_runs s ON s.id=o.run_id
         WHERE o.overseer_run_id=?1 AND o.status='active' AND o.generation=s.generation",
        [&run.id], |r| r.get(0)).optional()?;
    if let Some(id) = director { return Ok(Some((format!("swarm:{id}"), true))); }
    if store.is_swarm_linked_run(&run.id)? { return Ok(None); }
    if routine && store.conn.prepare("SELECT 1 FROM run_roles WHERE run_id=?1
        AND role IN ('overseer','watcher')")?.exists([&run.id])? { return Ok(None); }
    Ok(Some((format!("task:{}", run.task_id), false)))
}

// Caller holds Store and broadcasts the returned event only after its original
// transaction commits. Failure to retain audio metadata never fails agent work.
fn record(store: &Store, run: &Run, subject: &str, slot: &str, line: Line,
    identity: &str) -> Option<Event> {
    if store.conn.execute_batch("SAVEPOINT audio_semantic_record").is_err() {
        crate::log("audio semantic state unavailable"); return None;
    }
    let attempted = (|| -> Result<Option<Event>> {
        let existing: Option<(String, bool)> = store.conn.query_row(
            "SELECT identity,live FROM audio_semantic_state WHERE subject=?1 AND slot=?2",
            params![subject,slot], |r| Ok((r.get(0)?,r.get(1)?))).optional()?;
        if existing.as_ref().is_some_and(|(old,live)| old == identity && *live) { return Ok(None); }
        if existing.is_none() {
            // Remove only already delivered archived/orphan state.
            // Never evict unresolved identities to make room. At the hard cap
            // refuse another audio candidate with a bounded diagnostic.
            store.conn.execute("DELETE FROM audio_semantic_state WHERE rowid IN (
                SELECT a.rowid FROM audio_semantic_state a LEFT JOIN runs r ON r.id=a.run_id
                LEFT JOIN tasks t ON t.id=r.task_id WHERE a.played_seq=a.event_seq
                AND (r.id IS NULL OR t.archived_ms IS NOT NULL) LIMIT 64)", [])?;
            let count: i64 = store.conn.query_row("SELECT COUNT(*) FROM audio_semantic_state", [], |r|r.get(0))?;
            if count >= MAX_ROWS { crate::log("audio semantic state capacity reached"); return Ok(None); }
        }
        let event = store.insert_event(crate::daemon::now(), Some(&run.task_id), Some(&run.id),
            "audio_transition", "daemon", "exact", &json!({"subject":subject,"slot":slot,"line":line.key()}))?;
        store.conn.execute("INSERT INTO audio_semantic_state(subject,slot,run_id,generation,line,
            identity,event_seq,live,played_seq,updated_ms) VALUES(?1,?2,?3,?4,?5,?6,?7,1,0,?8)
            ON CONFLICT(subject,slot) DO UPDATE SET run_id=excluded.run_id,generation=excluded.generation,
            line=excluded.line,identity=excluded.identity,event_seq=excluded.event_seq,live=1,
            played_seq=0,updated_ms=excluded.updated_ms",
            params![subject,slot,run.id,run.process_generation,line.key(),identity,event.seq,event.ts])?;
        Ok(Some(event))
    })();
    match attempted {
        Ok(event) => {
            if store.conn.execute_batch("RELEASE audio_semantic_record").is_ok() {event}
            else { let _=store.conn.execute_batch("ROLLBACK TO audio_semantic_record; RELEASE audio_semantic_record");
                crate::log("audio semantic state unavailable"); None }
        },
        Err(_) => { let _=store.conn.execute_batch("ROLLBACK TO audio_semantic_record; RELEASE audio_semantic_record");
            crate::log("audio semantic state unavailable"); None }
    }
}

pub(crate) fn started(store: &Store, observed: &Run) -> Option<Event> {
    let attempt = (|| -> Result<Option<Event>> {
        let Some(run) = store.run(&observed.id)? else { return Ok(None) };
        if run.process_generation != observed.process_generation || run.status != "running" { return Ok(None); }
        let Some((subject,swarm)) = subject(store,&run,true)? else { return Ok(None) };
        // Even after bounded cleanup or a software upgrade, a later actual
        // owner turn/successor must not manufacture another first-task start.
        let prior: i64 = store.conn.query_row("SELECT COUNT(*) FROM turns t JOIN runs r ON r.id=t.run_id
            WHERE r.task_id=?1", [&run.task_id], |r|r.get(0))?;
        if prior != 1 || run.process_generation != 1 { return Ok(None); }
        if swarm {
            let generation: i64 = store.conn.query_row("SELECT generation FROM swarm_runs WHERE id=?1",
                [subject.strip_prefix("swarm:").unwrap()], |r|r.get(0))?;
            if generation != 1 { return Ok(None); }
        }
        Ok(record(store,&run,&subject,"start",if swarm {Line::SwarmInitiated} else {Line::AgentStarted},"first_work"))
    })();
    attempt.unwrap_or_else(|_| { crate::log("audio start state unavailable"); None })
}

pub(crate) fn permission(store: &Store, observed: &Run, request: &str) -> Option<Event> {
    let attempt = (|| -> Result<Option<Event>> {
        let Some(run) = store.run(&observed.id)? else { return Ok(None) };
        if run.process_generation != observed.process_generation || run.status != "waiting_for_user"
            || !run.attention.as_ref().is_some_and(|a| a["kind"]=="permission" && a["request_id"]==request) { return Ok(None); }
        let Some((subject,swarm)) = subject(store,&run,false)? else {return Ok(None)};
        let identity=format!("{}:{}:{request}",run.id,run.process_generation);
        Ok(record(store,&run,&subject,"need",if swarm {Line::SwarmNeedsAttention}
            else {Line::AgentPermissionRequired},&identity))
    })();
    attempt.unwrap_or_else(|_| { crate::log("audio permission state unavailable"); None })
}

pub(crate) fn attention_changed(store: &Store, run: &str) -> Result<()> {
    store.conn.execute("UPDATE audio_semantic_state SET live=0,updated_ms=?2
        WHERE run_id=?1 AND slot='need'", params![run,crate::daemon::now()])?;
    Ok(())
}

pub(crate) fn settled(store: &Store, observed: &Run, settlement: Settlement) -> Option<Event> {
    let attempt=(|| -> Result<Option<Event>> {
        let Some(run)=store.run(&observed.id)? else {return Ok(None)};
        if run.process_generation!=observed.process_generation {return Ok(None)};
        let owner:Option<String>=store.conn.query_row("SELECT owner_id FROM queue_owners WHERE run_id=?1",
            [&run.id],|r|r.get(0)).optional()?;
        if owner.is_some_and(|id|id!=run.id) {return Ok(None)};
        let Some((subject,swarm))=subject(store,&run,true)? else {return Ok(None)};
        // Director/worker process failure or success is never the objective's
        // recovery or completion authority. Only whole-Swarm producers qualify.
        if swarm {return Ok(None)};
        let line=match settlement {
            Settlement::Success=>Line::AgentComplete,
            Settlement::FailedWithoutAutomaticRecovery=>Line::AgentFailed,
            Settlement::AuthenticationWithoutPermittedFallback=>Line::AgentSignInRequired,
            Settlement::LostGenericExecutionWithoutRestore=>Line::AgentStoppedUnexpectedly,
            Settlement::ExhaustedConnectionRecovery|Settlement::ExhaustedMemoryRecovery=>Line::AgentCannotContinue,
        };
        let turn=store.turns(&run.id)?.last().map(|t|t.id.clone()).unwrap_or_default();
        Ok(record(store,&run,&subject,"end",line,&format!("{}:{}:{turn}:{}",run.id,run.process_generation,line.key())))
    })();
    attempt.unwrap_or_else(|_| {crate::log("audio settlement state unavailable");None})
}

pub(crate) fn swarm_completed(store: &Store, id: &str) -> Option<Event> {
    let attempt=(|| -> Result<Option<Event>> {
        let process: Option<String>=store.conn.query_row("SELECT o.overseer_run_id FROM swarm_runs s
            JOIN swarm_director_owners o ON o.run_id=s.id JOIN swarm_completions c ON c.run_id=s.id
            WHERE s.id=?1 AND s.status='completed' AND c.generation=s.generation
            AND NOT EXISTS(SELECT 1 FROM swarm_completion_invalidations i WHERE i.run_id=s.id)",
            [id],|r|r.get(0)).optional()?.flatten();
        let Some(run)=process.and_then(|p|store.run(&p).ok().flatten()) else {return Ok(None)};
        let generation:i64=store.conn.query_row("SELECT generation FROM swarm_runs WHERE id=?1",[id],|r|r.get(0))?;
        Ok(record(store,&run,&format!("swarm:{id}"),"end",Line::SwarmComplete,&format!("verified:{generation}")))
    })();
    attempt.unwrap_or_else(|_| {crate::log("audio Swarm completion state unavailable");None})
}

pub(super) fn ticket(event: &Event) -> Option<Ticket> {
    (event.kind=="audio_transition").then(||Some(Ticket{subject:event.payload["subject"].as_str()?.into(),
        slot:event.payload["slot"].as_str()?.into(),seq:event.seq}))?
}

fn current(store: &Store, ticket: &Ticket) -> Result<Option<Line>> {
    let saved:Option<(String,i64,String)>=store.conn.query_row("SELECT run_id,generation,line
        FROM audio_semantic_state WHERE subject=?1 AND slot=?2 AND event_seq=?3
        AND live=1 AND played_seq<>event_seq",params![ticket.subject,ticket.slot,ticket.seq],
        |r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
    let Some((owner,generation,key))=saved else{return Ok(None)};
    let Some(run)=store.run(&owner)? else{return Ok(None)};
    let Some(line)=Line::parse(&key) else{return Ok(None)};
    if run.process_generation!=generation {return Ok(None)};
    if let Some(id)=ticket.subject.strip_prefix("swarm:") {
        let state:String=store.conn.query_row("SELECT status FROM swarm_runs WHERE id=?1",[id],|r|r.get(0))?;
        if line==Line::SwarmComplete {
            let invalid=store.conn.prepare("SELECT 1 FROM swarm_completion_invalidations WHERE run_id=?1")?.exists([id])?;
            return Ok((state=="completed" && !invalid).then_some(line));
        }
        if !store.conn.prepare("SELECT 1 FROM swarm_director_owners WHERE run_id=?1
            AND overseer_run_id=?2 AND status='active' AND generation=(SELECT generation FROM swarm_runs WHERE id=?1)")?
            .exists(params![id,owner])? {return Ok(None)};
        if line==Line::SwarmInitiated {return Ok((run.status=="running" && ["planning","running"].contains(&state.as_str())).then_some(line));}
        return Ok((run.status=="waiting_for_user" && run.attention.is_some()).then_some(line));
    }
    if store.task(&run.task_id)?.is_none_or(|task| task.archived_ms.is_some()) {return Ok(None)};
    let owner:Option<String>=store.conn.query_row("SELECT owner_id FROM queue_owners WHERE run_id=?1",
        [&run.id],|r|r.get(0)).optional()?;
    if owner.is_some_and(|id|id!=run.id) {return Ok(None)};
    let other_live=store.conn.prepare("SELECT 1 FROM runs WHERE task_id=?1 AND id<>?2
        AND status IN ('queued','starting','running','waiting_for_user','waiting_for_connection','waiting_for_memory')")?
        .exists(params![run.task_id,run.id])?;
    if other_live {return Ok(None)};
    if ticket.slot=="start" {return Ok((crate::daemon::ACTIVE.contains(&run.status.as_str()) && run.attention.is_none()).then_some(line));}
    if ticket.slot=="need" {return Ok((run.status=="waiting_for_user" && run.attention.is_some()).then_some(line));}
    // Known continuations/pending validations/owner decisions prevent success.
    let queued=store.conn.prepare("SELECT 1 FROM queued_messages q JOIN runs r ON r.id=q.run_id
        WHERE r.task_id=?1 AND q.delivered_ms IS NULL")?.exists([&run.task_id])?;
    if queued {return Ok(None)};
    let live=match line {
        Line::AgentComplete=>run.status=="completed" && run.attention.is_none(),
        Line::AgentStoppedUnexpectedly=>["disconnected","failed"].contains(&run.status.as_str()),
        Line::AgentFailed|Line::AgentSignInRequired=>run.status=="failed",
        Line::AgentCannotContinue=>run.status=="failed" && run.attention.is_some(),
        _=>false,
    };
    Ok(live.then_some(line))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Selection {
    pub line: Line,
    pub tickets: Vec<Ticket>,
}

pub(super) fn selection(store: &Store, tickets: &[Ticket]) -> Result<Option<Selection>> {
    let mut live=Vec::<(Ticket,Line)>::new();
    for ticket in tickets {
        if let Some(line)=current(store,ticket)? {
            if !live.iter().any(|(saved,_)|saved.subject==ticket.subject) {live.push((ticket.clone(),line));}
        }
    }
    let line=match live.as_slice() { []=>return Ok(None),[(_,line)]=>*line,_=>Line::AgentsNeedAttention };
    Ok(Some(Selection{line,tickets:live.into_iter().map(|(ticket,_)|ticket).collect()}))
}

pub(super) fn fresh(store: &Store, tickets: &[Ticket]) -> Result<Option<Line>> {
    Ok(selection(store,tickets)?.map(|selection|selection.line))
}

pub(super) fn played(store: &Store, tickets: &[Ticket]) -> Result<()> {
    for ticket in tickets {
        store.conn.execute("UPDATE audio_semantic_state SET played_seq=event_seq WHERE subject=?1
            AND slot=?2 AND event_seq=?3",params![ticket.subject,ticket.slot,ticket.seq])?;
    }
    Ok(())
}
