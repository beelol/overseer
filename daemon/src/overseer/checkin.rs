//! Keeping agents on task (AC-189). The daemon's free checks need no model: a write outside an
//! agent's area or across a guardrail, a conflict, the same command failing three times in a
//! row. A check-in is Overseer's model looking at what changed since it last looked and saying
//! on task, drifting or done; it is due every third turn (the owner directs otherwise), when the
//! agent finishes, and when a free check trips. Due check-ins for several agents become one turn.

use crate::daemon::{Daemon, ACTIVE};
use anyhow::{bail, Result};
use serde_json::{json, Value};
use std::sync::Arc;

pub const DEFAULT_EVERY: i64 = 3;
pub const MAX_EVERY: i64 = 20;
/// Due check-ins within this window are one turn.
pub const BATCH_MS: i64 = 5000;
pub const BATCH_MAX: usize = 20;
/// What one turn Overseer starts by itself reads, at most.
pub const TURN_BYTES: usize = 32 * 1024;
/// Turns Overseer starts by itself in a day, check-ins included (AC-198).
pub const DEFAULT_CAP: i64 = 100;
/// An agent is finished when it has stayed idle this long after completing: a follow-up inside
/// it means the work goes on.
pub const DEFAULT_GRACE_MS: i64 = 30_000;
const CIRCLES: i64 = 3;

/// How closely an agent is followed: every N turns, only when done, or off.
#[derive(Clone, Debug, PartialEq)]
pub enum Cadence {
    Every(i64),
    DoneOnly,
    Off,
}

impl Cadence {
    pub fn parse(s: &str) -> Result<Self> {
        let s = s.trim().to_ascii_lowercase();
        Ok(match s.as_str() {
            "off" | "never" => Cadence::Off,
            "done" | "done_only" | "when_done" => Cadence::DoneOnly,
            "every_turn" | "every turn" | "1" => Cadence::Every(1),
            other => {
                let n = other.strip_prefix("every:").or_else(|| other.strip_prefix("every ")).unwrap_or(other).trim().parse::<i64>().map_err(|_| anyhow::anyhow!("no cadence {other:?}: choose off, done, every_turn or every:N (1 to {MAX_EVERY})"))?;
                if !(1..=MAX_EVERY).contains(&n) {
                    bail!("a cadence is every 1 to {MAX_EVERY} turns");
                }
                Cadence::Every(n)
            }
        })
    }
    pub fn text(&self) -> String {
        match self {
            Cadence::Every(1) => "every turn".into(),
            Cadence::Every(n) => format!("every:{n}"),
            Cadence::DoneOnly => "done".into(),
            Cadence::Off => "off".into(),
        }
    }
}

impl Daemon {
    // ------------------------------------------------------------------ cadence

    pub fn cadence_of(&self, run_id: &str) -> Cadence {
        use rusqlite::OptionalExtension;
        let store = self.store.lock().unwrap();
        let own: Option<String> = store.conn.query_row("SELECT cadence FROM cadences WHERE run_id=?1", [run_id], |r| r.get(0)).optional().ok().flatten();
        let global: Option<String> = store.conn.query_row("SELECT value FROM meta WHERE key='overseer.check_ins'", [], |r| r.get(0)).optional().ok().flatten();
        own.or(global).and_then(|s| Cadence::parse(&s).ok()).unwrap_or(Cadence::Every(DEFAULT_EVERY))
    }

    /// Set how closely one agent (or, with no run, every agent) is followed.
    pub fn set_cadence(&self, run_id: Option<&str>, cadence: &str, by: &str) -> Result<Value> {
        let c = Cadence::parse(cadence)?;
        let store = self.store.lock().unwrap();
        match run_id {
            Some(r) => {
                store.conn.execute("INSERT OR REPLACE INTO cadences(run_id, cadence, set_by, set_ms) VALUES(?1, ?2, ?3, ?4)", rusqlite::params![r, c.text(), by, crate::daemon::now()])?;
            }
            None => {
                store.conn.execute("INSERT OR REPLACE INTO meta(key, value) VALUES('overseer.check_ins', ?1)", [c.text()])?;
            }
        }
        drop(store);
        let task = run_id.and_then(|r| self.run(r).ok()).map(|r| r.task_id);
        self.emit(task.as_deref(), run_id, "cadence", by, "exact", json!({"cadence": c.text(), "by": by}))?;
        Ok(json!({"run_id": run_id, "cadence": c.text()}))
    }

    pub fn cap_of(&self) -> i64 {
        use rusqlite::OptionalExtension;
        let store = self.store.lock().unwrap();
        store.conn.query_row("SELECT value FROM meta WHERE key='overseer.cap'", [], |r| r.get::<_, String>(0)).optional().ok().flatten().and_then(|v| v.parse().ok()).unwrap_or(DEFAULT_CAP)
    }

    pub fn set_cap(&self, cap: i64) -> Result<Value> {
        if !(1..=10_000).contains(&cap) {
            bail!("the cap is 1 to 10,000 turns a day");
        }
        self.store.lock().unwrap().conn.execute("INSERT OR REPLACE INTO meta(key, value) VALUES('overseer.cap', ?1)", [cap.to_string()])?;
        Ok(json!({"cap": cap}))
    }

    /// Turns Overseer started by itself in the last day.
    pub fn self_started_today(&self) -> i64 {
        let since = crate::daemon::now() - 24 * 3600 * 1000;
        self.store.lock().unwrap().conn.query_row("SELECT COUNT(*) FROM overseer_turns WHERE cause != 'owner' AND ts > ?1", [since], |r| r.get(0)).unwrap_or(0)
    }

    // ------------------------------------------------------------------ what is due

    /// A check-in is due for this agent; several within the batch window become one turn. The
    /// same reason is not queued twice.
    pub fn check_in_due(&self, run_id: &str, reason: &str) -> Result<()> {
        self.check_in_due_at(run_id, reason, 0)
    }

    pub(crate) fn check_in_due_at(&self, run_id: &str, reason: &str, not_before: i64) -> Result<()> {
        let run = self.run(run_id)?;
        if run.parent_run_id.is_some() || self.run_role(run_id) != "agent" {
            return Ok(());
        }
        let store = self.store.lock().unwrap();
        let pending: i64 = store.conn.query_row("SELECT COUNT(*) FROM check_in_queue WHERE run_id=?1 AND reason=?2", rusqlite::params![run_id, reason], |r| r.get(0))?;
        if pending > 0 {
            return Ok(());
        }
        store.conn.execute("INSERT INTO check_in_queue(run_id, reason, ts, not_before) VALUES(?1, ?2, ?3, ?4)", rusqlite::params![run_id, reason, crate::daemon::now(), not_before])?;
        Ok(())
    }

    pub fn grace_ms(&self) -> i64 {
        use rusqlite::OptionalExtension;
        let store = self.store.lock().unwrap();
        store.conn.query_row("SELECT value FROM meta WHERE key='overseer.grace_ms'", [], |r| r.get::<_, String>(0)).optional().ok().flatten().and_then(|v| v.parse().ok()).unwrap_or(DEFAULT_GRACE_MS)
    }

    /// A new turn: the agent is not finished after all.
    pub fn turn_started_for_check_in(&self, run_id: &str) -> Result<()> {
        self.store.lock().unwrap().conn.execute("DELETE FROM check_in_queue WHERE run_id=?1 AND reason='finished'", [run_id])?;
        Ok(())
    }

    /// An agent's turn ended: the cadence says whether a check-in is due.
    pub fn turn_ended_for_check_in(&self, run_id: &str) -> Result<()> {
        let turns = self.store.lock().unwrap().turns(run_id)?.len() as i64;
        match self.cadence_of(run_id) {
            Cadence::Every(n) if turns > 0 && turns % n == 0 => self.check_in_due(run_id, &format!("turn {turns}")),
            _ => Ok(()),
        }
    }

    /// An agent completed a turn: if it stays idle for the grace period, it is finished and a
    /// check-in says done, or drifting if it stopped short.
    pub fn finished_for_check_in(&self, run_id: &str, status: &str) -> Result<()> {
        if !matches!(status, "completed" | "failed") {
            return Ok(());
        }
        if self.cadence_of(run_id) == Cadence::Off {
            return Ok(());
        }
        let grace = self.grace_ms();
        self.check_in_due_at(run_id, "finished", crate::daemon::now() + grace)
    }

    /// A free check tripped (outside its area, across a guardrail, a conflict that needs a
    /// decision, going in circles).
    pub fn free_check_tripped(&self, run_id: &str, what: &str) -> Result<()> {
        if self.cadence_of(run_id) == Cadence::Off {
            return Ok(());
        }
        self.check_in_due(run_id, what)
    }

    /// A write outside the agent's area, when it has one.
    pub fn check_area(&self, run_id: &str, paths: &[String]) -> Result<()> {
        let area = self.area_of(run_id);
        if area.is_empty() || paths.is_empty() {
            return Ok(());
        }
        let outside: Vec<&String> = paths.iter().filter(|p| !area.iter().any(|a| p.as_str() == a.trim_end_matches('/') || p.starts_with(&format!("{}/", a.trim_end_matches('/'))))).collect();
        if outside.is_empty() {
            return Ok(());
        }
        let key = serde_json::to_string(&outside)?;
        let already: i64 = self.store.lock().unwrap().conn.query_row("SELECT COUNT(*) FROM free_checks WHERE run_id=?1 AND kind='outside_area' AND detail=?2", rusqlite::params![run_id, key], |r| r.get(0))?;
        if already > 0 {
            return Ok(());
        }
        self.store.lock().unwrap().conn.execute("INSERT INTO free_checks(run_id, kind, detail, ts) VALUES(?1, 'outside_area', ?2, ?3)", rusqlite::params![run_id, key, crate::daemon::now()])?;
        let run = self.run(run_id)?;
        self.emit(Some(&run.task_id), Some(run_id), "outside_area", "daemon", "exact", json!({"paths": outside, "area": area}))?;
        self.free_check_tripped(run_id, "wrote outside its area")
    }

    /// The same command failing three times in a row. A tool's call carries its input and its
    /// result carries the outcome, joined by the tool id.
    pub fn check_circles(&self, run_id: &str, tool_id: &str, input: &Value, is_error: bool, is_result: bool) -> Result<()> {
        use rusqlite::OptionalExtension;
        let store = self.store.lock().unwrap();
        if !input.is_null() {
            store.conn.execute("INSERT OR REPLACE INTO tool_inputs(run_id, tool_id, input) VALUES(?1, ?2, ?3)", rusqlite::params![run_id, tool_id, input.to_string()])?;
            store.conn.execute("DELETE FROM tool_inputs WHERE run_id=?1 AND rowid NOT IN (SELECT rowid FROM tool_inputs WHERE run_id=?1 ORDER BY rowid DESC LIMIT 100)", [run_id])?;
            if !is_result {
                return Ok(());
            }
        }
        let key: Option<String> = if input.is_null() { store.conn.query_row("SELECT input FROM tool_inputs WHERE run_id=?1 AND tool_id=?2", rusqlite::params![run_id, tool_id], |r| r.get(0)).optional()? } else { Some(input.to_string()) };
        let Some(key) = key else { return Ok(()) };
        let last: Option<(String, i64)> = store.conn.query_row("SELECT detail, count FROM circles WHERE run_id=?1", [run_id], |r| Ok((r.get(0)?, r.get(1)?))).optional()?;
        if !is_error {
            store.conn.execute("DELETE FROM circles WHERE run_id=?1", [run_id])?;
            return Ok(());
        }
        let count = match last {
            Some((d, c)) if d == key => c + 1,
            _ => 1,
        };
        store.conn.execute("INSERT OR REPLACE INTO circles(run_id, detail, count) VALUES(?1, ?2, ?3)", rusqlite::params![run_id, key, count])?;
        drop(store);
        if count == CIRCLES {
            let run = self.run(run_id)?;
            self.emit(Some(&run.task_id), Some(run_id), "going_in_circles", "daemon", "exact", json!({"input": serde_json::from_str::<Value>(&key).unwrap_or(Value::Null), "times": count}))?;
            self.free_check_tripped(run_id, "the same command failed three times in a row")?;
        }
        Ok(())
    }

    // ------------------------------------------------------------------ the turn

    /// Due check-ins whose window has passed (or enough of them): one Overseer turn, unless
    /// Overseer is busy or at its cap.
    pub fn run_due_check_ins(self: &Arc<Self>) -> Result<()> {
        let now = crate::daemon::now();
        let due: Vec<(i64, String, String, i64)> = {
            let store = self.store.lock().unwrap();
            let mut stmt = store.conn.prepare("SELECT rowid, run_id, reason, ts FROM check_in_queue WHERE not_before <= ?1 ORDER BY rowid")?;
            let rows = stmt.query_map([now], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?.collect::<rusqlite::Result<_>>()?;
            rows
        };
        if due.is_empty() {
            return Ok(());
        }
        // One turn takes at most twenty items, oldest first; the rest are the next turn's.
        let due: Vec<(i64, String, String, i64)> = due.into_iter().take(BATCH_MAX).collect();
        // The window runs from when the oldest entry became due.
        let oldest = due.iter().map(|d| d.3).min().unwrap_or(now);
        let became_due: i64 = self.store.lock().unwrap().conn.query_row("SELECT MIN(MAX(ts, not_before)) FROM check_in_queue WHERE not_before <= ?1", [now], |r| r.get::<_, Option<i64>>(0)).ok().flatten().unwrap_or(oldest);
        if now - became_due < BATCH_MS && due.len() < BATCH_MAX {
            return Ok(());
        }
        let session = self.overseer_session()?;
        // A turn Overseer starts by itself never creates its run: until the owner has spoken to
        // Overseer there is no model to spend, and the free checks keep recording on their own.
        if session["run_id"].is_null() {
            self.store.lock().unwrap().conn.execute("DELETE FROM check_in_queue", [])?;
            return Ok(());
        }
        let _one_at_a_time = super::session::TURN_START.lock().unwrap_or_else(|e| e.into_inner());
        let busy = session["run_id"].as_str().and_then(|r| self.run(r).ok()).map(|r| ACTIVE.contains(&r.status.as_str())).unwrap_or(false);
        if busy {
            return Ok(());
        }
        // The cap: say so once, then wait for the owner.
        let cap = self.cap_of();
        if self.self_started_today() >= cap {
            let sid = session["id"].as_str().unwrap().to_string();
            let said: i64 = self.store.lock().unwrap().conn.query_row("SELECT COUNT(*) FROM overseer_messages WHERE session_id=?1 AND source='overseer' AND text LIKE 'At the cap%'", [&sid], |r| r.get(0))?;
            if said == 0 {
                self.append_session_message(&sid, "overseer", None, &format!("At the cap: I started {cap} turns by myself today. I will check in again when you ask, or tomorrow; the free checks keep running."), None)?;
            }
            self.store.lock().unwrap().conn.execute("DELETE FROM check_in_queue", [])?;
            return Ok(());
        }
        // One entry per agent, reasons joined; agents that are gone are dropped.
        let mut per: Vec<(String, Vec<String>)> = Vec::new();
        for (_, run, reason, _) in &due {
            if self.run(run).is_err() {
                continue;
            }
            match per.iter_mut().find(|(r, _)| r == run) {
                Some((_, reasons)) => {
                    if !reasons.contains(reason) {
                        reasons.push(reason.clone());
                    }
                }
                None => per.push((run.clone(), vec![reason.clone()])),
            }
        }
        {
            let store = self.store.lock().unwrap();
            for (rowid, _, _, _) in &due {
                store.conn.execute("DELETE FROM check_in_queue WHERE rowid=?1", [rowid])?;
            }
        }
        if per.is_empty() {
            return Ok(());
        }
        let prompt = self.compose_check_in(&per)?;
        let harness = session["harness"].as_str().unwrap_or("claude").to_string();
        // Questions and reports from the agents' channel ride the same turn; a turn with nothing
        // else is theirs.
        let is_channel = |r: &String| r.starts_with("ask:") || r.starts_with("report:") || r.starts_with("finding:");
        let cause = if per.iter().any(|(_, reasons)| reasons.iter().any(|r| !is_channel(r))) { "check_in" } else if per.iter().any(|(_, reasons)| reasons.iter().any(|r| r.starts_with("finding:"))) { "finding" } else if per.iter().any(|(_, reasons)| reasons.iter().any(|r| r.starts_with("ask:"))) { "ask" } else { "report" };
        self.overseer_turn_with_cause(&session, &prompt, &harness, session["model"].as_str(), cause)?;
        for (run, reasons) in &per {
            let reasons: Vec<&String> = reasons.iter().filter(|r| !is_channel(r)).collect();
            if reasons.is_empty() {
                continue;
            }
            let task = self.run(run).ok().map(|r| r.task_id);
            self.emit(task.as_deref(), Some(run), "check_in_started", "overseer", "exact", json!({"reasons": reasons}))?;
        }
        Ok(())
    }

    /// What Overseer reads for a check-in: each agent's digest and why it is looked at.
    fn compose_check_in(&self, per: &[(String, Vec<String>)]) -> Result<String> {
        use rusqlite::OptionalExtension;
        let level = self.overseer_session()?["level"].as_str().unwrap_or("ask_first").to_string();
        let mut items = Vec::new();
        let mut questions = Vec::new();
        let mut reports = Vec::new();
        let mut findings = Vec::new();
        let mut texts = Vec::new();
        for (run, reasons) in per {
            let d = self.digest(run)?;
            let last_check = d.last_check_in.clone();
            let plain: Vec<&String> = reasons.iter().filter(|r| !r.starts_with("ask:") && !r.starts_with("report:") && !r.starts_with("finding:")).collect();
            for r in reasons {
                if let Some(id) = r.strip_prefix("finding:") {
                    if let Some(mut f) = self.finding_json(id) {
                        f["subject_title"] = json!(d.title);
                        f["watcher_title"] = json!(self.run(f["watcher"].as_str().unwrap_or("")).map(|r| r.title).unwrap_or_default());
                        f["held"] = json!(self.hold_of(run).is_some());
                        f["hold_on_stop"] = json!(self.watch(f["watch"].as_str().unwrap_or("")).map(|w| w.hold_on_stop).unwrap_or(false));
                        findings.push(f);
                    }
                } else if let Some(id) = r.strip_prefix("ask:") {
                    let q: Option<String> = self.store.lock().unwrap().conn.query_row("SELECT body FROM agent_messages WHERE id=?1 AND answer IS NULL", [id], |r| r.get(0)).optional()?;
                    if let Some(body) = q.and_then(|b| serde_json::from_str::<Value>(&b).ok()) {
                        questions.push(json!({"id": id, "agent": d.id, "title": d.title, "question": body["question"], "area": d.area, "repository": d.repository}));
                    }
                } else if let Some(id) = r.strip_prefix("report:") {
                    let b: Option<String> = self.store.lock().unwrap().conn.query_row("SELECT body FROM agent_messages WHERE id=?1", [id], |r| r.get(0)).optional()?;
                    if let Some(mut body) = b.and_then(|b| serde_json::from_str::<Value>(&b).ok()) {
                        body["agent"] = json!(d.id);
                        body["title"] = json!(d.title);
                        body["repository"] = json!(d.repository);
                        reports.push(body);
                    }
                }
            }
            if !plain.is_empty() {
                items.push(json!({"id": d.id, "title": d.title, "status": d.status, "reasons": plain, "area": d.area, "changed": d.changed.iter().map(|c| c.path.clone()).collect::<Vec<_>>(), "asked": d.asked.iter().map(|a| a.text.clone()).collect::<Vec<_>>(), "last_check_in": last_check}));
            }
            texts.push(self.digest_text(run)?);
        }
        let mut out = format!("{}\n", super::session::OPEN);
        if !items.is_empty() {
            out.push_str(&format!("Check-in. For each agent below, decide whether it is doing what was asked (its task, the owner's later messages, your directions, its guardrails and its area) and answer with the check_in tool once per agent: result on_task, drifting or done, with a reason; for done, also what was left out, if anything. An agent that is on task hears nothing from you. For one that is drifting, use propose: at the Steer level a message or a hold, at the Auto level a redirect; at Ask first the owner decides. Judge what the agent is doing, not how. The level is {level}.\n\nCheck-in (JSON):\n{}\n\n", serde_json::to_string_pretty(&items)?));
        }
        if !questions.is_empty() {
            out.push_str(&format!("Questions from agents. Answer each with the answer tool (its id and your text) from what you know: the roster, the digests, the other agents' reports and files; if only another agent can answer, ask it with propose (a message) and answer once it replies. The level is {level}.\n\nQuestions (JSON):\n{}\n\n", serde_json::to_string_pretty(&questions)?));
        }
        if !reports.is_empty() {
            out.push_str(&format!("Reports that came back from the agents you asked. Call rally for the repository's map, then propose in one proposal the areas (area actions) and shares it needs.\n\nReports (JSON):\n{}\n\n", serde_json::to_string_pretty(&reports)?));
        }
        if !findings.is_empty() {
            out.push_str(&format!("Findings from watchers. The watcher only reads; you act on its subject at your level with propose: for stop, at Ask first propose a hold and say why, at Steer hold now (a redirect is a proposal), at Auto hold and redirect; for concern, a message to the subject or nothing, as you judge; when `held` is true the daemon already holds the subject (hold on stop) and what follows is still yours. Tell the owner what you did. The level is {level}.\n\nFindings (JSON):\n{}\n\n", serde_json::to_string_pretty(&findings)?));
        }
        out.push_str(&format!("Digests:\n{}\n{}\n", texts.join("\n\n"), super::session::CLOSE));
        out.push_str(if items.is_empty() { "\nAnswer the agents." } else { "\nCheck in on these agents." });
        Ok(super::bound(&out, TURN_BYTES))
    }

    /// The check_in tool: Overseer's result for one agent, recorded on that agent.
    pub fn record_check_in(self: &Arc<Self>, agent: &str, result: &str, reason: &str, left_out: &str) -> Result<Value> {
        if !["on_task", "drifting", "done"].contains(&result) {
            bail!("a check-in result is on_task, drifting or done");
        }
        let run = self.run(agent).map_err(|_| anyhow::anyhow!("no agent {agent}"))?;
        let now = crate::daemon::now();
        self.store.lock().unwrap().conn.execute("INSERT INTO check_ins(run_id, ts, result, reason, left_out) VALUES(?1, ?2, ?3, ?4, ?5)", rusqlite::params![agent, now, result, reason, left_out])?;
        self.emit(Some(&run.task_id), Some(agent), "check_in", "overseer", "exact", json!({"result": result, "reason": reason, "left_out": left_out, "title": run.title}))?;
        if result == "done" {
            let session = self.overseer_session()?;
            let sid = session["id"].as_str().unwrap().to_string();
            let asked: Vec<String> = self.digest(agent)?.asked.iter().map(|a| a.text.clone()).collect();
            let card = json!({"kind": "done", "agent": agent, "title": run.title, "asked": asked, "done": reason, "left_out": left_out});
            self.append_session_message(&sid, "card", None, &format!("{} is done: {}{}", run.title, reason, if left_out.is_empty() { String::new() } else { format!(" Left out: {left_out}.") }), Some(&card))?;
        }
        Ok(json!({"agent": agent, "result": result, "recorded": true}))
    }

    pub fn check_ins_of(&self, run_id: &str) -> Result<Value> {
        // The cadence takes the store lock itself, so it is read before the lock below.
        let cadence = self.cadence_of(run_id).text();
        let store = self.store.lock().unwrap();
        let mut stmt = store.conn.prepare("SELECT ts, result, reason, left_out FROM check_ins WHERE run_id=?1 ORDER BY ts")?;
        let rows: Vec<Value> = stmt.query_map([run_id], |r| Ok(json!({"ts": r.get::<_, i64>(0)?, "result": r.get::<_, String>(1)?, "reason": r.get::<_, String>(2)?, "left_out": r.get::<_, String>(3)?})))?.collect::<rusqlite::Result<_>>()?;
        Ok(json!({"check_ins": rows, "cadence": cadence}))
    }
}
