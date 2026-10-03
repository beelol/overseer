//! Control (AC-185 to AC-188): the classes every action and every daemon method fall into, holds,
//! guardrails, redirects, and the dispatch states a card shows. The daemon, not the model,
//! decides what an action is and what it may do at the owner's level.

use crate::daemon::{Daemon, TurnOpts, ACTIVE};
use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};
use std::sync::Arc;

/// The four classes Voice Mode uses (AC-171), shared here.
pub const LOOK: &str = "look";
pub const STEER: &str = "steer";
pub const CONFIRM: &str = "confirm";
pub const NEVER: &str = "never";

/// Overseer's actions and their class. `share` across repositories and more than three new
/// agents are Confirm at the call site.
pub const ACTION_CLASSES: &[(&str, &str)] = &[
    ("pin", LOOK),
    // Overseer moves the owner around VS Code (AC-226): shown in the owner's window, never a yes.
    ("focus", LOOK),
    ("open_review", LOOK),
    ("open_file", LOOK),
    ("open_worktree", LOOK),
    ("show_work", LOOK),
    ("message", STEER),
    ("share", STEER),
    ("report", STEER),
    ("area", STEER),
    ("hold", STEER),
    ("release", STEER),
    ("guardrail", STEER),
    ("cadence", STEER),
    ("redirect", STEER),
    ("stop", STEER),
    ("watch", STEER),
    ("start", STEER),
    ("answer", STEER),
    ("withdraw", STEER),
    // AC-239: an agent that stopped goes on elsewhere, or tries again.
    ("continue", STEER),
    ("retry", STEER),
    // AC-230: an agent's permission mode, by conversation; Auto set by Overseer itself only in
    // the repositories the owner allows, with its reason (checked in the session).
    ("mode", STEER),
    ("archive", CONFIRM),
    ("permission", CONFIRM),
    ("merge_back", CONFIRM),
    ("pull_request", CONFIRM),
    ("swarm", CONFIRM),
];

/// Quiet Steer actions: at the Steer level Overseer takes them by itself.
pub const QUIET: &[&str] = &["message", "share", "report", "area", "hold", "release", "guardrail", "pin", "cadence", "answer", "withdraw"];

/// Every daemon method and the class of what it does, so a table-driven test can check that no
/// method is unclassified and that Overseer's actions reach only what their class allows.
/// `read` is a read; `look`, `steer` and `confirm` are reachable through the actions of that
/// class; `never` is not from the conversation.
pub const METHOD_CLASSES: &[(&str, &str)] = &[
    ("mods.list", "read"),
    ("mods.why", "read"),
    ("mods.bind", NEVER), ("mods.unbind", NEVER),
    ("mods.preview", NEVER), ("mods.install", NEVER), ("mods.remove", NEVER),
    ("hello", "read"), ("state", "read"), ("harness.list", "read"), ("profile.list", "read"), ("profile.status", "read"),
    ("repo.inspect", "read"), ("run.turns", "read"), ("run.active", "read"), ("run.raw_output", "read"), ("events.list", "read"),
    ("events.subscribe", "read"), ("comparison.options", "read"), ("workspace.diff", "read"), ("workspace.status", "read"),
    ("workspace.cleanup_plan", "read"), ("account.usage", "read"), ("search", "read"), ("repo.files", "read"), ("workspace.changes", "read"),
    ("workspace.tree", "read"), ("account.list", "read"), ("workspace.pr_plan", "read"), ("workspace.merge_plan", "read"),
    ("daemon.background_notice", "read"), ("daemon.last_notice", "read"), ("daemon.clients", "read"), ("audio.get", "read"), ("audio.voices", "read"), ("notices.get", "read"),
    ("agent.digest", "read"), ("agents.roster", "read"), ("conflicts.list", "read"), ("overseer.session", "read"), ("overseer.messages", "read"), ("agent.check_ins", "read"),
    ("overseer.tools", "read"), ("overseer.tool", "read"), ("run.queued", "read"), ("overseer.card", "read"), ("agent.holds", "read"), ("agent.guardrails", "read"),
    ("channel.messages", "read"), ("agent.briefings", "read"), ("overseer.rally", "read"), ("share.list", "read"), ("watch.list", "read"), ("watch.findings", "read"),
    ("workspace.file", "read"), ("workspace.hunks", "read"), ("review.marks", "read"), ("repo.known", "read"), ("menubar.snapshot", "read"),
    // What Overseer's Steer actions reach.
    ("task.create", STEER), ("run.follow_up", STEER), ("run.queue", STEER), ("run.unqueue", STEER), ("run.redirect", STEER), ("run.interrupt", STEER),
    ("agent.hold", STEER), ("agent.release", STEER), ("agent.guardrail", STEER), ("agent.guardrail_remove", STEER), ("agent.redirect", STEER),
    ("conflict.dismiss", STEER), ("conflict.resolve", STEER), ("overseer.scan", STEER), ("overseer.propose", STEER), ("agent.cadence", STEER),
    ("agent.channel", STEER), ("agent.area", STEER), ("share.withdraw", STEER), ("watch.start", STEER), ("watch.end", STEER),
    // Confirm: only when the owner asked, read back, then a yes.
    ("run.permission", CONFIRM), ("task.archive", CONFIRM), ("workspace.merge_prepare", CONFIRM), ("workspace.merge_resolved", CONFIRM),
    ("workspace.merge_complete", CONFIRM), ("workspace.merge_abort", CONFIRM), ("workspace.pr_prepare", CONFIRM), ("workspace.pr_opened", CONFIRM),
    // Not from the conversation.
    ("profile.create", NEVER), ("profile.rename", NEVER), ("profile.login_command", NEVER), ("profile.logout", NEVER),
    ("account.create", NEVER), ("account.remove", NEVER), ("workspace.cleanup", NEVER), ("audio.set", NEVER), ("audio.preview", NEVER),
    ("audio.import_commander", NEVER), ("run.resume_queue", NEVER), ("run.clear_queue", NEVER), ("daemon.shutdown", NEVER), ("daemon.stop_all", NEVER), ("daemon.test_notice", NEVER), ("notices.set", NEVER),
    // Voice Mode (Gate R): the owner's own, never from the conversation.
    ("voice.get", "read"), ("voice.requests", "read"), ("voice.subscribe", "read"), ("voice.set", NEVER), ("voice.say", NEVER), ("voice.simulate", NEVER),
    ("voice.speak", NEVER), ("voice.focus", NEVER), ("voice.download", NEVER), ("voice.cancel", NEVER), ("voice.read_back", NEVER), ("voice.answer", NEVER),
    ("overseer.token", NEVER), ("overseer.level", NEVER), ("overseer.auto_repos", NEVER), ("agent.share_deny", NEVER), ("overseer.cap", NEVER), ("overseer.fresh", NEVER), ("overseer.send", NEVER), ("overseer.visit", NEVER), ("overseer.answer", NEVER), ("overseer.cancel", NEVER),
    // Auto Mode and Swarm (claude/auto-swarm). Reads are reads. Starting a swarm, raising its
    // limits or deadline, changing its targets, resuming it, or starting an Auto root need the
    // owner's confirmation (the Swarm/Auto contract: Overseer's level grants no route, allocation
    // or worker). Stopping or pausing only reduces work. The director and worker protocol, the
    // fixture launch bridges, the account booking inputs and every setting are not from the
    // conversation.
    ("swarm.get", "read"), ("swarm.list", "read"), ("swarm.jobs", "read"), ("swarm.coverage", "read"),
    ("swarm.messages", "read"), ("swarm.conflicts", "read"), ("swarm.policy.preview", "read"), ("claims.ledger", "read"), ("swarm.findings", "read"), ("swarm.report.final", "read"), ("broker.envelopes", "read"),
    ("swarm.benefit.preview", "read"), ("swarm.storage.status", "read"), ("swarm.director.summary", "read"),
    ("swarm.worker.liveness", "read"), ("swarm.route.replay", "read"), ("swarm.native_director.get", "read"), ("agents.limit.get", "read"), ("auto.root.preview", "read"),
    ("auto.mode.get", "read"), ("auto.models.list", "read"), ("auto.quota.state", "read"), ("auto.quota.list", "read"),
    ("auto.usage.list", "read"), ("auto.usage.work.list", "read"), ("auto.usage.thread.list", "read"),
    ("auto.usage.summary", "read"), ("auto.decision.replay", "read"), ("run.result", "read"),
    ("swarm.stop", STEER), ("swarm.pause", STEER), ("swarm.off", STEER),
    ("swarm.create", CONFIRM), ("swarm.start", CONFIRM), ("swarm.resume", CONFIRM), ("swarm.limit.set", CONFIRM), ("swarm.deadline.extend", CONFIRM),
    ("swarm.targets.set", CONFIRM), ("swarm.requirements.change", CONFIRM), ("auto.start", CONFIRM),
    ("swarm.director.owner.begin", NEVER), ("swarm.director.owner.renew", NEVER), ("swarm.director.owner.refresh_linked", NEVER),
    ("swarm.director.owner.expire_due", NEVER), ("swarm.director.launch", NEVER), ("swarm.storage.recover", NEVER),
    ("swarm.storage.limit_pages", NEVER), ("swarm.plan", NEVER), ("swarm.attempt.register", NEVER), ("swarm.report", NEVER),
    ("swarm.direct", NEVER), ("swarm.ack", NEVER), ("swarm.partial", NEVER), ("swarm.claim", NEVER), ("swarm.artifact.put", NEVER),
    ("swarm.integrate", NEVER), ("swarm.verify", NEVER), ("swarm.decide", NEVER), ("swarm.conflict.open", NEVER),
    ("swarm.conflict.resolve", NEVER), ("swarm.finding.record", NEVER), ("swarm.finding.merge", NEVER), ("swarm.reproduce", NEVER), ("swarm.complete", NEVER), ("swarm.attempt.confirm_exit", NEVER), ("swarm.revise", NEVER),
    ("swarm.benefit.commit", NEVER), ("swarm.availability.observe", NEVER), ("swarm.policy.set", NEVER),
    ("swarm.estimate.revoke", NEVER), ("swarm.admit", NEVER), ("swarm.schedule.next", NEVER), ("swarm.dispatch.next", NEVER),
    ("swarm.worker.launch", NEVER), ("swarm.effect.begin", NEVER), ("swarm.effect.reconcile", NEVER), ("swarm.worker.brief", NEVER),
    ("swarm.context.get", NEVER), ("swarm.context.revoke", NEVER), ("swarm.context.grant", NEVER), ("swarm.worker.reconcile", NEVER),
    ("swarm.worker.liveness.sample", NEVER), ("swarm.worker.liveness.poll", NEVER), ("swarm.job.deadline.persist_due", NEVER),
    ("swarm.redirect.persist_due", NEVER), ("swarm.director.claim_batch", NEVER), ("swarm.director.complete_batch", NEVER),
    ("swarm.director.recover", NEVER), ("swarm.native_director.set", NEVER), ("agents.limit.set", NEVER), ("run.delegate", NEVER), ("auto.mode.set", NEVER),
    ("auto.bridge.submit", NEVER), ("auto.bridge.result", NEVER), ("auto.dispatch", NEVER), ("auto.models.refresh", NEVER),
    ("auto.opencode.local.inspect", NEVER), ("auto.tools.inspect", NEVER), ("auto.quota.refresh", NEVER),
    ("auto.usage.thread.refresh", NEVER), ("auto.usage.export", NEVER), ("auto.usage.clear", NEVER),
    // What the owner does from VS Code or a phone (Gate N): their review marks, putting lines back,
    // a pull request, a sign-in and stopping everyone at once are not Overseer's to do.
    ("review.accept", NEVER), ("review.unaccept", NEVER), ("review.import", NEVER), ("review.reject", NEVER), ("review.seen", NEVER),
    ("workspace.pr_open", NEVER), ("profile.device_login", NEVER), ("runs.stop_all", NEVER),
];

/// The Look actions that only move the owner around VS Code (AC-226): they change nothing, so they
/// happen at once at every level, typed or spoken, and never wait for a yes.
pub const NAVIGATE: &[&str] = &["focus", "open_review", "open_file", "open_worktree", "show_work"];

pub fn action_class(action: &str) -> Option<&'static str> {
    ACTION_CLASSES.iter().find(|(a, _)| *a == action).map(|(_, c)| *c)
}

pub fn method_class(method: &str) -> Option<&'static str> {
    METHOD_CLASSES.iter().find(|(m, _)| *m == method).map(|(_, c)| *c)
}

const GUARDRAIL_WORDS: usize = 1024;

fn inside(path: &str, area: &str) -> bool {
    let area = area.trim_end_matches('/');
    area == "." || path == area || path.starts_with(&format!("{area}/"))
}

impl Daemon {
    // ------------------------------------------------------------------ holds

    /// A hold: no new turn until released. `now` stops the current turn too.
    pub fn agent_hold(self: &Arc<Self>, run_id: &str, reason: &str, by: &str, now: bool, release_on: Value, card: Option<&str>) -> Result<Value> {
        let run = self.run(run_id)?;
        if run.parent_run_id.is_some() {
            bail!("{} is a native child; it is steered through its parent", run.title);
        }
        if self.run_role(run_id) == "overseer" {
            bail!("Overseer does not hold itself");
        }
        let release = if release_on.is_null() { json!({"kind": "release"}) } else { release_on };
        let ts = crate::daemon::now();
        {
            let store = self.store.lock().unwrap();
            store.conn.execute("INSERT OR REPLACE INTO holds(run_id, set_by, reason, set_ms, release_on, card_id) VALUES(?1, ?2, ?3, ?4, ?5, ?6)", rusqlite::params![run_id, by, reason, ts, release.to_string(), card])?;
        }
        let mut stopped = false;
        if now && ACTIVE.contains(&run.status.as_str()) {
            self.interrupt_turn(run_id)?;
            stopped = true;
        }
        self.emit(Some(&run.task_id), Some(run_id), "hold", by, "exact", json!({"reason": reason, "by": by, "now": now, "stopped": stopped, "release_on": release, "card": card}))?;
        Ok(json!({"run_id": run_id, "held": true, "stopped": stopped, "release_on": release}))
    }

    pub fn agent_release(self: &Arc<Self>, run_id: &str, by: &str, why: &str) -> Result<Value> {
        let run = self.run(run_id)?;
        let removed = self.store.lock().unwrap().conn.execute("DELETE FROM holds WHERE run_id=?1", [run_id])?;
        if removed == 0 {
            return Ok(json!({"run_id": run_id, "held": false, "released": false}));
        }
        self.emit(Some(&run.task_id), Some(run_id), "release", by, "exact", json!({"by": by, "why": why}))?;
        // What waited behind the hold goes now.
        self.deliver_queued(run_id)?;
        Ok(json!({"run_id": run_id, "held": false, "released": true}))
    }

    pub fn hold_of(&self, run_id: &str) -> Option<Value> {
        let store = self.store.lock().unwrap();
        store
            .conn
            .query_row("SELECT set_by, reason, set_ms, release_on, card_id FROM holds WHERE run_id=?1", [run_id], |r| {
                Ok(json!({"run_id": run_id, "by": r.get::<_, String>(0)?, "reason": r.get::<_, String>(1)?, "set_ms": r.get::<_, i64>(2)?, "release_on": serde_json::from_str::<Value>(&r.get::<_, String>(3)?).unwrap_or(json!({"kind": "release"})), "card": r.get::<_, Option<String>>(4)?}))
            })
            .ok()
    }

    pub fn holds_list(&self) -> Result<Value> {
        let store = self.store.lock().unwrap();
        let mut stmt = store.conn.prepare("SELECT run_id, set_by, reason, set_ms, release_on, card_id FROM holds ORDER BY set_ms")?;
        let rows: Vec<Value> = stmt
            .query_map([], |r| {
                let run: String = r.get(0)?;
                let title = store.run(&run).ok().flatten().map(|x| x.title).unwrap_or_default();
                Ok(json!({"run_id": run, "title": title, "by": r.get::<_, String>(1)?, "reason": r.get::<_, String>(2)?, "set_ms": r.get::<_, i64>(3)?, "release_on": serde_json::from_str::<Value>(&r.get::<_, String>(4)?).unwrap_or(json!({"kind": "release"})), "card": r.get::<_, Option<String>>(5)?}))
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(json!({"holds": rows}))
    }

    /// Holds whose release condition has come: a conflict closed, an agent finished, a time passed.
    pub fn release_due_holds(self: &Arc<Self>, event_kind: &str, event_run: Option<&str>, payload: &Value) -> Result<()> {
        let holds = self.holds_list()?;
        let now = crate::daemon::now();
        for h in holds["holds"].as_array().cloned().unwrap_or_default() {
            let on = &h["release_on"];
            let run = h["run_id"].as_str().unwrap_or("");
            let due = match on["kind"].as_str().unwrap_or("release") {
                "conflict" => event_kind == "conflict_closed" && payload["id"] == on["id"],
                "agent_done" => event_kind == "status" && event_run == on["id"].as_str() && matches!(payload["status"].as_str(), Some("completed") | Some("failed") | Some("interrupted")),
                "time" => event_kind == "tick" && on["at_ms"].as_i64().map(|at| now >= at).unwrap_or(false),
                _ => false,
            };
            if due {
                let why = match on["kind"].as_str().unwrap_or("") {
                    "conflict" => "the conflict it waited for is closed".to_string(),
                    "agent_done" => format!("{} finished", self.run(on["id"].as_str().unwrap_or("")).map(|r| r.title).unwrap_or_default()),
                    _ => "the time it waited for has come".to_string(),
                };
                self.agent_release(run, "overseer", &why)?;
            }
        }
        Ok(())
    }

    // ------------------------------------------------------------------ guardrails

    pub fn agent_guardrail(self: &Arc<Self>, run_id: &str, words: &str, allow: &[String], deny: &[String], hold_on_cross: bool, by: &str) -> Result<Value> {
        let run = self.run(run_id)?;
        if run.parent_run_id.is_some() {
            bail!("{} is a native child; it is steered through its parent", run.title);
        }
        if words.trim().is_empty() && allow.is_empty() && deny.is_empty() {
            bail!("a guardrail needs words or paths");
        }
        for p in allow.iter().chain(deny.iter()) {
            if p.starts_with('/') || p.split('/').any(|c| c == "..") {
                bail!("{p:?} is not a path inside the worktree");
            }
        }
        // Words already on this agent count toward the bound.
        let existing: i64 = self.store.lock().unwrap().conn.query_row("SELECT COALESCE(SUM(LENGTH(words)), 0) FROM guardrails WHERE run_id=?1 AND removed_ms IS NULL", [run_id], |r| r.get(0))?;
        if existing as usize + words.len() > GUARDRAIL_WORDS {
            bail!("guardrail words are bounded to {GUARDRAIL_WORDS} bytes per agent");
        }
        // Enforced only where the harness itself refuses the write: Claude Code takes deny rules
        // for its edit tools on later turns; every other harness is watched by the daemon.
        let enforcement = if run.harness == "claude" && !deny.is_empty() { "enforced" } else { "watched" };
        let id = format!("g-{}", &uuid::Uuid::new_v4().simple().to_string()[..12]);
        let ts = crate::daemon::now();
        self.store.lock().unwrap().conn.execute(
            "INSERT INTO guardrails(id, run_id, set_by, words, allow, deny, hold_on_cross, enforcement, created_ms) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            rusqlite::params![id, run_id, by, words, serde_json::to_string(allow)?, serde_json::to_string(deny)?, hold_on_cross as i64, enforcement, ts],
        )?;
        self.emit(Some(&run.task_id), Some(run_id), "guardrail", by, "exact", json!({"id": id, "words": words, "allow": allow, "deny": deny, "hold_on_cross": hold_on_cross, "enforcement": enforcement}))?;
        Ok(json!({"id": id, "run_id": run_id, "enforcement": enforcement}))
    }

    pub fn agent_guardrail_remove(&self, id: &str, by: &str) -> Result<Value> {
        let run: String = self.store.lock().unwrap().conn.query_row("SELECT run_id FROM guardrails WHERE id=?1 AND removed_ms IS NULL", [id], |r| r.get(0)).map_err(|_| anyhow!("unknown guardrail {id}"))?;
        self.store.lock().unwrap().conn.execute("UPDATE guardrails SET removed_ms=?2 WHERE id=?1", rusqlite::params![id, crate::daemon::now()])?;
        let task = self.run(&run).ok().map(|r| r.task_id);
        self.emit(task.as_deref(), Some(&run), "guardrail_removed", by, "exact", json!({"id": id}))?;
        Ok(json!({"id": id, "removed": true}))
    }

    pub fn guardrails_of(&self, run_id: &str) -> Result<Vec<Value>> {
        let store = self.store.lock().unwrap();
        let mut stmt = store.conn.prepare("SELECT id, set_by, words, allow, deny, hold_on_cross, enforcement, created_ms FROM guardrails WHERE run_id=?1 AND removed_ms IS NULL ORDER BY created_ms")?;
        let rows = stmt.query_map([run_id], |r| {
            Ok(json!({"id": r.get::<_, String>(0)?, "by": r.get::<_, String>(1)?, "words": r.get::<_, String>(2)?, "allow": serde_json::from_str::<Value>(&r.get::<_, String>(3)?).unwrap_or(json!([])), "deny": serde_json::from_str::<Value>(&r.get::<_, String>(4)?).unwrap_or(json!([])), "hold_on_cross": r.get::<_, i64>(5)? != 0, "enforcement": r.get::<_, String>(6)?, "created_ms": r.get::<_, i64>(7)?}))
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// The words repeated to an agent at the start of each later turn, within the bound.
    pub fn guardrail_preface(&self, run_id: &str) -> String {
        let rails = self.guardrails_of(run_id).unwrap_or_default();
        let mut lines = Vec::new();
        for g in &rails {
            let words = g["words"].as_str().unwrap_or("").trim();
            if !words.is_empty() {
                lines.push(words.to_string());
            }
            let allow: Vec<&str> = g["allow"].as_array().map(|a| a.iter().filter_map(|x| x.as_str()).collect()).unwrap_or_default();
            let deny: Vec<&str> = g["deny"].as_array().map(|a| a.iter().filter_map(|x| x.as_str()).collect()).unwrap_or_default();
            if !allow.is_empty() {
                lines.push(format!("Stay inside: {}.", allow.join(", ")));
            }
            if !deny.is_empty() {
                lines.push(format!("Do not change: {}.", deny.join(", ")));
            }
        }
        if lines.is_empty() {
            return String::new();
        }
        let text = format!("[Guardrails from Overseer: {}]", lines.join(" "));
        super::bound(&text, GUARDRAIL_WORDS + 64)
    }

    /// Claude Code's deny rules for the paths a guardrail forbids, added to that run's later turns.
    pub fn guardrail_launch_args(&self, run_id: &str, harness: &str) -> Vec<String> {
        if harness != "claude" {
            return Vec::new();
        }
        let rails = self.guardrails_of(run_id).unwrap_or_default();
        let mut rules = Vec::new();
        for g in &rails {
            for p in g["deny"].as_array().map(|a| a.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>()).unwrap_or_default() {
                let pattern = format!("{}/**", p.trim_end_matches('/'));
                rules.push(format!("Edit({pattern})"));
                rules.push(format!("Write({pattern})"));
                rules.push(format!("MultiEdit({pattern})"));
            }
        }
        if rules.is_empty() {
            return Vec::new();
        }
        vec!["--disallowedTools".into(), rules.join(",")]
    }

    /// Paths an agent wrote, checked against its guardrails: a write across one is reported and,
    /// when the guardrail says so, holds the agent at once.
    pub fn check_guardrails(self: &Arc<Self>, run_id: &str, paths: &[String]) -> Result<()> {
        let rails = self.guardrails_of(run_id)?;
        if rails.is_empty() || paths.is_empty() {
            return Ok(());
        }
        let run = self.run(run_id)?;
        for g in &rails {
            let allow: Vec<&str> = g["allow"].as_array().map(|a| a.iter().filter_map(|x| x.as_str()).collect()).unwrap_or_default();
            let deny: Vec<&str> = g["deny"].as_array().map(|a| a.iter().filter_map(|x| x.as_str()).collect()).unwrap_or_default();
            let crossed: Vec<&String> = paths
                .iter()
                .filter(|p| (!allow.is_empty() && !allow.iter().any(|a| inside(p, a))) || deny.iter().any(|d| inside(p, d)))
                .collect();
            if crossed.is_empty() {
                continue;
            }
            let id = g["id"].as_str().unwrap_or("");
            // Once per path per guardrail: a second event for the same write says nothing new.
            let already: i64 = self.store.lock().unwrap().conn.query_row("SELECT COUNT(*) FROM guardrail_crossings WHERE guardrail_id=?1 AND paths=?2", rusqlite::params![id, serde_json::to_string(&crossed)?], |r| r.get(0))?;
            if already > 0 {
                continue;
            }
            self.store.lock().unwrap().conn.execute("INSERT INTO guardrail_crossings(guardrail_id, run_id, paths, ts) VALUES(?1, ?2, ?3, ?4)", rusqlite::params![id, run_id, serde_json::to_string(&crossed)?, crate::daemon::now()])?;
            let hold = g["hold_on_cross"].as_bool().unwrap_or(false);
            self.emit(Some(&run.task_id), Some(run_id), "guardrail_crossed", "daemon", "exact", json!({"guardrail": id, "paths": crossed, "held": hold, "enforcement": g["enforcement"]}))?;
            if hold {
                self.agent_hold(run_id, &format!("wrote across a guardrail: {}", crossed.iter().map(|p| p.as_str()).collect::<Vec<_>>().join(", ")), "overseer", true, json!({"kind": "release"}), None)?;
            }
        }
        Ok(())
    }

    // ------------------------------------------------------------------ redirect

    /// Stop the turn (where the harness can be stopped), keep a snapshot, and give the agent a
    /// new direction as its next turn.
    pub fn agent_redirect(self: &Arc<Self>, run_id: &str, text: &str, source: &str, detail: Value) -> Result<Value> {
        let owner = self.queue_owner(run_id);
        let run_id = owner.as_str();
        let run = self.run(run_id)?;
        self.validate_follow_up_target(&run)?;
        if run.parent_run_id.is_some() {
            bail!("{} is a native child; it is steered through its parent", run.title);
        }
        let text = text.trim();
        if text.is_empty() {
            bail!("a redirect needs a direction");
        }
        let ws = self.workspace(&run.workspace_id)?;
        let snap = self.take_snapshot(&ws, "redirect")?;
        let gate = self.work_unit_gate(&format!("queue:{run_id}"));
        let _guard = gate.lock().unwrap();
        if self.queue_owner(run_id) != owner { drop(_guard); return self.agent_redirect(run_id, text, source, detail); }
        let run = self.run(run_id)?;
        let active = ACTIVE.contains(&run.status.as_str());
        let mut detail = detail;
        detail["redirect"] = json!(true);
        detail["snapshot"] = json!(snap.id);
        let prompt = if source == "overseer" { format!("{}{text}", super::session::FROM_OVERSEER) } else { text.to_string() };
        if self.queued_messages(run_id)?["paused"] == true {
            let store = self.store.lock().unwrap();
            let current: String = store.conn.query_row("SELECT owner_id FROM queue_owners WHERE run_id=?1", [run_id], |r| r.get(0)).unwrap_or_else(|_| run_id.to_string());
            if current != owner { drop(store); drop(_guard); return self.agent_redirect(&current, text, source, detail); }
            store.conn.execute("INSERT INTO queued_messages(run_id, ts, source, text, detail) VALUES(?1, ?2, ?3, ?4, ?5)", rusqlite::params![run_id, crate::daemon::now(), source, prompt, detail.to_string()])?;
            drop(store);
            self.emit(Some(&run.task_id), Some(run_id), "queued", source, "exact", json!({"text":text,"detail":detail,"paused":true}))?;
            return Ok(json!({"run_id":run_id,"snapshot":snap.id,"delivery":"paused"}));
        }
        let waits = run.status == crate::handoff::WAITING_FOR_CONNECTION || run.status == crate::handoff::WAITING_FOR_MEMORY;
        self.emit(Some(&run.task_id), Some(run_id), "redirect", source, "exact", json!({"text": text, "snapshot": snap.id, "stopped": active && !waits, "waiting": waits, "detail": detail}))?;
        if waits {
            // Nothing runs to be stopped: the direction takes the place of the message the run
            // kept and goes once, when the wait ends or to the agent Continuity moves the work to.
            let preface = self.guardrail_preface(run_id);
            let prompt = if preface.is_empty() { prompt } else { format!("{preface}\n\n{prompt}") };
            let turn = crate::handoff::replace_waiting_turn(self, &run, &prompt)?;
            self.store.lock().unwrap().conn.execute("INSERT OR REPLACE INTO turn_sources(turn_id, source, detail) VALUES(?1, ?2, ?3)", rusqlite::params![turn.id, source, detail.to_string()])?;
            return Ok(json!({"run_id": run_id, "snapshot": snap.id, "delivery": "when the agent can run again", "turn": turn.id}));
        }
        if active {
            // Queued first, so the turn that starts when the stop lands carries the direction.
            let store = self.store.lock().unwrap();
            let current: String = store.conn.query_row("SELECT owner_id FROM queue_owners WHERE run_id=?1", [run_id], |r| r.get(0)).unwrap_or_else(|_| run_id.to_string());
            if current != owner { drop(store); drop(_guard); return self.agent_redirect(&current, text, source, detail); }
            store.conn.execute("INSERT INTO queued_messages(run_id, ts, source, text, detail) VALUES(?1, ?2, ?3, ?4, ?5)", rusqlite::params![run_id, crate::daemon::now(), source, prompt, detail.to_string()])?;
            drop(store);
            let waits = matches!(crate::adapters::interrupt_plan(&run.harness), crate::adapters::InterruptPlan::Signal) && run.harness == "generic";
            self.interrupt_turn(run_id)?;
            return Ok(json!({"run_id": run_id, "snapshot": snap.id, "delivery": if waits { "queued until the turn ends" } else { "stopping, then the direction" }}));
        }
        let turn = self.start_turn(run_id, &prompt, true, &TurnOpts { model: None, effort: None, mode: None, images: Vec::new(), ..Default::default() })?;
        self.store.lock().unwrap().conn.execute("INSERT OR REPLACE INTO turn_sources(turn_id, source, detail) VALUES(?1, ?2, ?3)", rusqlite::params![turn.id, source, detail.to_string()])?;
        Ok(json!({"run_id": run_id, "snapshot": snap.id, "delivery": "sent", "turn": turn.id}))
    }

    /// The latest redirect snapshot of a workspace, for the review's "since the change of direction".
    pub fn redirect_snapshot(&self, workspace_id: &str) -> Option<crate::store::Snapshot> {
        let store = self.store.lock().unwrap();
        let id: Option<String> = store.conn.query_row("SELECT id FROM snapshots WHERE workspace_id=?1 AND kind='redirect' ORDER BY created_ms DESC LIMIT 1", [workspace_id], |r| r.get(0)).ok();
        id.and_then(|i| store.snapshot(&i).ok().flatten())
    }

    // ------------------------------------------------------------------ dispatch states

    /// One row of a card: what was sent to whom, and how far it got.
    pub fn dispatch_record(&self, card: &str, run_id: &str, action: &str, delivery: &str, message: &str, why: &str, state: &str) -> Result<String> {
        let id = format!("d-{}", &uuid::Uuid::new_v4().simple().to_string()[..12]);
        let now = crate::daemon::now();
        let store = self.store.lock().unwrap();
        let sent = if state == "held" { None } else { Some(now) };
        let delivered = if state == "delivered" { Some(now) } else { None };
        store.conn.execute(
            "INSERT INTO dispatches(id, card_id, run_id, action, delivery, message, why, state, held_ms, sent_ms, delivered_ms) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            rusqlite::params![id, card, run_id, action, delivery, message, why, state, now, sent, delivered],
        )?;
        Ok(id)
    }

    pub fn dispatch_advance(&self, run_id: &str, state: &str, turn_id: Option<&str>) -> Result<Vec<Value>> {
        let now = crate::daemon::now();
        let column = match state {
            "sent" => "sent_ms",
            "delivered" => "delivered_ms",
            "picked_up" => "picked_ms",
            "answered" => "answered_ms",
            _ => bail!("no dispatch state {state}"),
        };
        let order: Vec<&str> = vec!["held", "sent", "delivered", "picked_up", "answered"];
        let rank = order.iter().position(|s| *s == state).unwrap_or(0);
        let store = self.store.lock().unwrap();
        let mut stmt = store.conn.prepare("SELECT id, card_id, state FROM dispatches WHERE run_id=?1 AND state NOT IN ('answered', 'failed', 'cancelled', 'not_sent')")?;
        let rows: Vec<(String, String, String)> = stmt.query_map([run_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<rusqlite::Result<_>>()?;
        drop(stmt);
        let mut advanced = Vec::new();
        for (id, card, current) in rows {
            let cur = order.iter().position(|s| *s == current).unwrap_or(0);
            if cur >= rank {
                continue;
            }
            // A state jumped over gets the same time, so a card never shows a later step without the earlier ones.
            store.conn.execute(&format!("UPDATE dispatches SET state=?2, {column}=?3, sent_ms=COALESCE(sent_ms, ?3), delivered_ms=CASE WHEN ?5 >= 2 THEN COALESCE(delivered_ms, ?3) ELSE delivered_ms END, turn_id=COALESCE(?4, turn_id) WHERE id=?1"), rusqlite::params![id, state, now, turn_id, rank as i64])?;
            advanced.push(json!({"id": id, "card": card, "state": state}));
        }
        Ok(advanced)
    }

    pub fn card(&self, id: &str) -> Result<Value> {
        let store = self.store.lock().unwrap();
        let proposal = store
            .conn
            .query_row("SELECT id, session_id, ts, actions, state, answered_by, answered_ms, result, source, surface FROM overseer_proposals WHERE id=?1", [id], |r| {
                Ok(json!({"id": r.get::<_, String>(0)?, "session": r.get::<_, String>(1)?, "ts": r.get::<_, i64>(2)?, "actions": serde_json::from_str::<Value>(&r.get::<_, String>(3)?).unwrap_or(json!([])), "state": r.get::<_, String>(4)?, "answered_by": r.get::<_, Option<String>>(5)?, "answered_ms": r.get::<_, Option<i64>>(6)?, "result": r.get::<_, Option<String>>(7)?, "via": r.get::<_, Option<String>>(8)?, "surface": r.get::<_, Option<String>>(9)?}))
            })
            .map_err(|_| anyhow!("unknown card {id}"))?;
        let mut stmt = store.conn.prepare("SELECT id, run_id, action, delivery, message, why, state, held_ms, sent_ms, delivered_ms, picked_ms, answered_ms, turn_id FROM dispatches WHERE card_id=?1 ORDER BY held_ms")?;
        let rows: Vec<Value> = stmt
            .query_map([id], |r| {
                let run: String = r.get(1)?;
                Ok(json!({"id": r.get::<_, String>(0)?, "run_id": run.clone(), "title": store.run(&run).ok().flatten().map(|x| x.title), "action": r.get::<_, String>(2)?, "delivery": r.get::<_, String>(3)?, "message": r.get::<_, String>(4)?, "why": r.get::<_, String>(5)?, "state": r.get::<_, String>(6)?,
                    "held_ms": r.get::<_, i64>(7)?, "sent_ms": r.get::<_, Option<i64>>(8)?, "delivered_ms": r.get::<_, Option<i64>>(9)?, "picked_ms": r.get::<_, Option<i64>>(10)?, "answered_ms": r.get::<_, Option<i64>>(11)?, "turn_id": r.get::<_, Option<String>>(12)?}))
            })?
            .collect::<rusqlite::Result<_>>()?;
        let mut card = proposal;
        card["rows"] = json!(rows);
        // Titles come from agents (a generic run's title is its command line): redacted (AC-200).
        Ok(crate::daemon::redact_value(card))
    }

    /// A conflict's card actions (AC-192): assign, sequence, dismiss; share follows with AC-191.
    pub fn conflict_resolve(self: &Arc<Self>, id: &str, how: &str, keeper: Option<&str>, by: &str) -> Result<Value> {
        let (kind, a, b, paths): (String, String, Option<String>, Vec<String>) = {
            let store = self.store.lock().unwrap();
            let row: (String, String, Option<String>, String, String) = store
                .conn
                .query_row("SELECT kind, run_a, run_b, paths, state FROM conflicts WHERE id=?1", [id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))
                .map_err(|_| anyhow!("unknown conflict {id}"))?;
            if row.4 != "open" {
                bail!("conflict {id} is already {}", row.4);
            }
            (row.0, row.1, row.2, serde_json::from_str(&row.3).unwrap_or_default())
        };
        let Some(b) = b else { bail!("{how} needs two agents; this conflict is with a branch") };
        let (keep, other) = match keeper {
            Some(k) if k == a => (a.clone(), b.clone()),
            Some(k) if k == b => (b.clone(), a.clone()),
            Some(k) => bail!("{k} is not one of this conflict's agents"),
            None => (a.clone(), b.clone()),
        };
        let keep_title = self.run(&keep).map(|r| r.title).unwrap_or_default();
        let other_title = self.run(&other).map(|r| r.title).unwrap_or_default();
        let resolution = match how {
            "assign" => {
                // The keeper owns the paths; the other agent gets a guardrail and a word.
                let dirs: Vec<String> = paths.clone();
                // The keeper's area is in the one claim ledger (SWARM-44): a Swarm job holding
                // one of the paths refuses the whole assignment before anything changes.
                let held = {
                    let store = self.store.lock().unwrap();
                    super::channel::ledger_refusals(&store.conn, &keep, &dirs)?
                };
                if !held.is_empty() {
                    self.notify_claim_refusals()?;
                    bail!("{}", crate::claims::refusal_text(&held));
                }
                self.agent_guardrail(&other, &format!("{keep_title} owns {}; leave those files to it.", dirs.join(", ")), &[], &dirs, false, by)?;
                {
                    let store = self.store.lock().unwrap();
                    for p in &dirs {
                        store.conn.execute("INSERT OR REPLACE INTO areas(run_id, path, set_by, created_ms) VALUES(?1, ?2, ?3, ?4)", rusqlite::params![keep, p, by, crate::daemon::now()])?;
                    }
                }
                self.queue_message(&other, &format!("{keep_title} owns {} now; leave those files to it and bring in its branch when you need them.", dirs.join(", ")), "overseer", json!({"conflict": id}))?;
                self.queue_message(&keep, &format!("You own {}; {other_title} will leave those files to you.", dirs.join(", ")), "overseer", json!({"conflict": id}))?;
                json!({"action": "assign", "keeper": keep, "by": by})
            }
            "sequence" => {
                // The other agent waits until the keeper finishes, then is told to bring in its branch.
                self.agent_hold(&other, &format!("waits for {keep_title} to finish before touching {}", paths.join(", ")), by, false, json!({"kind": "agent_done", "id": keep}), None)?;
                self.queue_message(&other, &format!("{keep_title} is finishing its changes to {}. When it is done, bring in its branch before you continue with those files.", paths.join(", ")), "overseer", json!({"conflict": id}))?;
                json!({"action": "sequence", "first": keep, "then": other, "by": by})
            }
            "dismiss" => return self.conflict_dismiss(id, by),
            other => bail!("no resolution {other}"),
        };
        let now = crate::daemon::now();
        self.store.lock().unwrap().conn.execute("UPDATE conflicts SET state='resolved', resolution=?2, closed_ms=?3, last_ms=?3 WHERE id=?1", rusqlite::params![id, resolution.to_string(), now])?;
        for run in [&a, &b] {
            let task = self.run(run).ok().map(|r| r.task_id);
            self.emit(task.as_deref(), Some(run), "conflict_closed", by, "exact", json!({"id": id, "kind": kind, "state": "resolved", "resolution": resolution}))?;
        }
        Ok(json!({"id": id, "state": "resolved", "resolution": resolution}))
    }
}
