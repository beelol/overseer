//! Agents that know about each other (AC-190) and context passed between them (AC-191): the
//! briefing an agent gets about the agents beside it, the channel back to Overseer (report, ask,
//! claim), rally, and shares. The daemon knows the sender of every channel message from the run's
//! token, stores it before it is acknowledged, and gives a repeated one one effect. Nothing here
//! needs a model turn except answering a question and reading a rally's map.

use crate::daemon::{Daemon, ACTIVE};
use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};
use sha2::Digest as _;
use std::collections::BTreeMap;
use std::sync::Arc;

/// A briefing is one short paragraph.
pub const BRIEFING_BYTES: usize = 1024;
/// A share travels inline up to this size; larger pieces go as a file in the receiving run's folder.
pub const SHARE_INLINE_BYTES: usize = 8 * 1024;
/// The tools an agent's channel is made of (with the roster every agent has).
pub const CHANNEL_TOOLS: &[&str] = &["roster", "report", "ask", "claim"];

fn short_sha(s: &str) -> String {
    format!("{:x}", sha2::Sha256::digest(s.as_bytes()))[..16].to_string()
}

fn strings(v: &Value) -> Vec<String> {
    match v {
        Value::Array(a) => a.iter().filter_map(|x| x.as_str()).map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect(),
        Value::String(s) => s.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect(),
        _ => Vec::new(),
    }
}

/// A relative path an agent may claim: no root, no `..`, no trailing slash.
fn clean_path(p: &str) -> Result<String> {
    let p = p.trim().trim_start_matches("./").trim_end_matches('/').to_string();
    if p.is_empty() || p.starts_with('/') || p.split('/').any(|c| c == "..") {
        bail!("{p:?} is not a path inside the repository");
    }
    Ok(p)
}

/// The Swarm jobs holding any of `paths` exclusively, each refusal recorded in the one claim
/// ledger (SWARM-44); empty when the agent may take them all.
pub(crate) fn ledger_refusals(conn: &rusqlite::Connection, run_id: &str, paths: &[String]) -> Result<Vec<crate::claims::Holder>> {
    let mut all = Vec::new();
    for p in paths {
        let me = crate::claims::Holder { kind: "agent", run: run_id.to_string(), job: None, resource: p.clone() };
        for holder in crate::claims::swarm_write_holders(conn, run_id, p)? {
            crate::claims::refuse(conn, &me, &holder)?;
            if !all.contains(&holder) {
                all.push(holder);
            }
        }
    }
    Ok(all)
}

/// The directory most of the paths share, for an agent that never claimed one.
fn suggest_area(paths: &[String]) -> Vec<String> {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for p in paths {
        if let Some((dir, _)) = p.rsplit_once('/') {
            let top = dir.split('/').next().unwrap_or(dir).to_string();
            *counts.entry(top).or_default() += 1;
        }
    }
    counts.into_iter().max_by_key(|(_, n)| *n).map(|(d, _)| vec![d]).unwrap_or_default()
}

fn cut(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

impl Daemon {
    // ------------------------------------------------------------------ who gets what

    /// Whether this agent gets a briefing and a channel: the owner's setting for it, else the
    /// default (`overseer.channel`: auto, on or off); auto means only when another agent works in
    /// its repository.
    pub fn channel_of(&self, run_id: &str) -> Result<(bool, bool)> {
        use rusqlite::OptionalExtension;
        if self.run_role(run_id) != "agent" {
            return Ok((false, false));
        }
        // A generic program is not a model: it can read no briefing and run no tool.
        if self.run(run_id)?.harness == "generic" {
            return Ok((false, false));
        }
        let (row, default) = {
            let store = self.store.lock().unwrap();
            let row: Option<(i64, i64)> = store.conn.query_row("SELECT briefing, channel FROM channels WHERE run_id=?1", [run_id], |r| Ok((r.get(0)?, r.get(1)?))).optional()?;
            let default: String = store.conn.query_row("SELECT value FROM meta WHERE key='overseer.channel'", [], |r| r.get(0)).optional()?.unwrap_or_else(|| "auto".into());
            (row, default)
        };
        if let Some((b, c)) = row {
            return Ok((b != 0, c != 0));
        }
        Ok(match default.as_str() {
            "on" => (true, true),
            "off" => (false, false),
            _ => {
                // Auto: once the owner has spoken to Overseer (its run exists), and only when
                // another agent works in the repository. Agents run without Overseer as before.
                let overseer_in_use: bool = {
                    use rusqlite::OptionalExtension;
                    self.store.lock().unwrap().conn.query_row("SELECT run_id FROM overseer_sessions WHERE archived_ms IS NULL AND run_id IS NOT NULL LIMIT 1", [], |r| r.get::<_, Option<String>>(0)).optional()?.flatten().is_some()
                };
                let many = overseer_in_use && !self.companions(run_id)?.is_empty();
                (many, many)
            }
        })
    }

    /// The owner sets it per agent, or the default for every agent without a setting.
    pub fn set_channel(&self, run_id: Option<&str>, briefing: Option<bool>, channel: Option<bool>, default: Option<&str>, by: &str) -> Result<Value> {
        let store = self.store.lock().unwrap();
        if let Some(d) = default {
            if !["auto", "on", "off"].contains(&d) {
                bail!("the default is auto, on or off");
            }
            store.conn.execute("INSERT OR REPLACE INTO meta(key, value) VALUES('overseer.channel', ?1)", [d])?;
        }
        if let Some(run) = run_id {
            let current: Option<(i64, i64)> = {
                use rusqlite::OptionalExtension;
                store.conn.query_row("SELECT briefing, channel FROM channels WHERE run_id=?1", [run], |r| Ok((r.get(0)?, r.get(1)?))).optional()?
            };
            let b = briefing.map(|b| b as i64).or(current.map(|c| c.0)).unwrap_or(1);
            let c = channel.map(|c| c as i64).or(current.map(|c| c.1)).unwrap_or(1);
            store.conn.execute("INSERT OR REPLACE INTO channels(run_id, briefing, channel, set_by, set_ms) VALUES(?1, ?2, ?3, ?4, ?5)", rusqlite::params![run, b, c, by, crate::daemon::now()])?;
        }
        drop(store);
        Ok(json!({"run_id": run_id, "default": default, "by": by}))
    }

    /// The other top-level agents working in the same repository.
    pub fn companions(&self, run_id: &str) -> Result<Vec<crate::store::Run>> {
        let run = self.run(run_id)?;
        let task = self.task(&run.task_id)?;
        let (runs, tasks) = {
            let store = self.store.lock().unwrap();
            (store.runs()?, store.tasks()?)
        };
        let mut out = Vec::new();
        for r in runs {
            if r.id == run.id || r.parent_run_id.is_some() || !ACTIVE.contains(&r.status.as_str()) {
                continue;
            }
            let same_repo = tasks.iter().any(|t| t.id == r.task_id && t.repo_root == task.repo_root);
            if same_repo && self.run_role(&r.id) == "agent" {
                out.push(r);
            }
        }
        out.sort_by_key(|r| r.created_ms);
        Ok(out)
    }

    // ------------------------------------------------------------------ the briefing

    /// The paragraph about the agents working beside this one, within the bound; empty when it
    /// works alone or the owner turned briefings off for it.
    pub fn briefing_text(&self, run_id: &str) -> Result<String> {
        let (briefing, channel) = self.channel_of(run_id)?;
        if !briefing {
            return Ok(String::new());
        }
        let others = self.companions(run_id)?;
        if others.is_empty() {
            return Ok(String::new());
        }
        let mut parts = Vec::new();
        for o in &others {
            let area = self.area_of(&o.id);
            let title = crate::redact::redact(&o.title);
            parts.push(if area.is_empty() { format!("“{title}” (no area claimed yet)") } else { format!("“{title}” in {}", area.join(", ")) });
        }
        let mut text = format!(
            "[Briefing from Overseer: {} other agent{} work{} in this repository: {}. Leave their areas to them.",
            others.len(),
            if others.len() == 1 { "" } else { "s" },
            if others.len() == 1 { "s" } else { "" },
            parts.join("; ")
        );
        if channel {
            text.push_str(" You have Overseer's tools: claim the paths you take, report what you are doing, what you changed and what you need, and ask Overseer what you cannot find out yourself; it answers, or asks the agent concerned.");
        } else {
            text.push_str(" If you need something from one of them, or find something it should know, say so in your reply and Overseer passes it on.");
        }
        text.push(']');
        Ok(super::bound(&text, BRIEFING_BYTES))
    }

    /// The briefing on an agent's first turn, added to its task and shown as an event.
    pub fn briefing_preface(&self, run_id: &str) -> String {
        let text = match self.briefing_text(run_id) {
            Ok(t) if !t.is_empty() => t,
            _ => return String::new(),
        };
        let _ = self.record_briefing(run_id, &text, "task");
        text
    }

    fn record_briefing(&self, run_id: &str, text: &str, how: &str) -> Result<()> {
        let run = self.run(run_id)?;
        self.store.lock().unwrap().conn.execute("INSERT INTO briefings(run_id, ts, text, how) VALUES(?1, ?2, ?3, ?4)", rusqlite::params![run_id, crate::daemon::now(), text, how])?;
        self.emit(Some(&run.task_id), Some(run_id), "briefing", "overseer", "exact", json!({"text": text, "how": how, "line": "Overseer added a briefing"}))?;
        Ok(())
    }

    /// An agent started in a repository: the ones already working there get their briefing (or
    /// a new one, when it changed) as a queued message.
    pub fn brief_companions(self: &Arc<Self>, new_run: &str) -> Result<()> {
        use rusqlite::OptionalExtension;
        if self.run_role(new_run) != "agent" {
            return Ok(());
        }
        for o in self.companions(new_run)? {
            let text = self.briefing_text(&o.id)?;
            if text.is_empty() {
                continue;
            }
            let last: Option<String> = self.store.lock().unwrap().conn.query_row("SELECT text FROM briefings WHERE run_id=?1 ORDER BY ts DESC LIMIT 1", [&o.id], |r| r.get(0)).optional()?;
            if last.as_deref() == Some(text.as_str()) {
                continue;
            }
            self.record_briefing(&o.id, &text, "queued")?;
            // A briefing still waiting is replaced by the newer one.
            self.store.lock().unwrap().conn.execute("DELETE FROM queued_messages WHERE run_id=?1 AND source='briefing' AND delivered_ms IS NULL", [&o.id])?;
            self.queue_message(&o.id, &text, "briefing", json!({"briefing": true}))?;
        }
        Ok(())
    }

    pub fn briefings_of(&self, run_id: &str) -> Result<Value> {
        let store = self.store.lock().unwrap();
        let mut stmt = store.conn.prepare("SELECT ts, text, how FROM briefings WHERE run_id=?1 ORDER BY ts")?;
        let rows: Vec<Value> = stmt.query_map([run_id], |r| Ok(json!({"ts": r.get::<_, i64>(0)?, "text": r.get::<_, String>(1)?, "how": r.get::<_, String>(2)?})))?.collect::<rusqlite::Result<_>>()?;
        Ok(json!({"briefings": rows}))
    }

    // ------------------------------------------------------------------ the channel's launch

    /// The harness arguments that give an agent's run its channel on a turn: the daemon's MCP
    /// server with the run's own token, kept in the run's folder. Nothing is written into the
    /// user's own configuration; OpenCode reads a project file, so it gets no channel yet.
    pub fn channel_launch_args(&self, run_id: &str, harness: &str) -> Result<Vec<String>> {
        let (_, channel) = self.channel_of(run_id)?;
        let watcher = self.run_role(run_id) == "agent" && self.is_watcher(run_id);
        if (!channel && !watcher) || !["claude", "codex"].contains(&harness) {
            return Ok(Vec::new());
        }
        let mut tools: Vec<&str> = if channel { CHANNEL_TOOLS.to_vec() } else { vec!["roster"] };
        if watcher {
            for t in super::watch::WATCHER_TOOLS {
                if !tools.contains(t) {
                    tools.push(t);
                }
            }
        }
        let dir = crate::paths::runs_dir().join(run_id);
        crate::paths::ensure_private_dir(&dir)?;
        let config = dir.join("overseer-mcp.json");
        let socket = crate::paths::socket_path().display().to_string();
        let exe = self.exe.display().to_string();
        let token = match std::fs::read(&config).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()) {
            Some(v) if v["mcpServers"]["overseer"]["env"]["OVERSEER_MCP_TOKEN"].is_string() => v["mcpServers"]["overseer"]["env"]["OVERSEER_MCP_TOKEN"].as_str().unwrap().to_string(),
            _ => {
                let token = self.overseer_token(run_id, "agent")?["token"].as_str().unwrap().to_string();
                std::fs::write(&config, serde_json::to_vec_pretty(&json!({"mcpServers": {"overseer": {"type": "stdio", "command": exe, "args": ["mcp", "--socket", socket], "env": {"OVERSEER_MCP_TOKEN": token}}}}))?)?;
                {
                    use std::os::unix::fs::PermissionsExt;
                    std::fs::set_permissions(&config, std::fs::Permissions::from_mode(0o600))?;
                }
                token
            }
        };
        Ok(match harness {
            "claude" => vec!["--mcp-config".into(), config.display().to_string(), "--allowedTools".into(), tools.iter().map(|t| format!("mcp__overseer__{t}")).collect::<Vec<_>>().join(",")],
            "codex" => {
                let mut args = vec!["-c".to_string(), format!("mcp_servers.overseer.command={}", json!(exe)), "-c".into(), format!("mcp_servers.overseer.args=[\"mcp\",\"--socket\",{}]", json!(socket)), "-c".into(), format!("mcp_servers.overseer.env={{ OVERSEER_MCP_TOKEN = {} }}", json!(token))];
                for t in &tools {
                    args.extend(["-c".into(), format!("mcp_servers.overseer.tools.{t}.approval_mode=\"approve\"")]);
                }
                args
            }
            _ => Vec::new(),
        })
    }

    // ------------------------------------------------------------------ report, ask, claim

    /// A channel message with a stable id from its sender, kind and content: stored once,
    /// however often it is repeated. Returns the id and whether it was new.
    fn channel_message(&self, run_id: &str, kind: &str, body: &Value) -> Result<(String, bool)> {
        let canonical = serde_json::to_string(body)?;
        // The one broker's rules (SWARM-60): a body is bounded like a Swarm envelope.
        if canonical.len() > crate::broker::MAX_BODY_BYTES {
            bail!("a {kind} is at most 32 KiB");
        }
        let id = format!("{kind}-{}", short_sha(&format!("{run_id}|{kind}|{canonical}")));
        let store = self.store.lock().unwrap();
        let n = store.conn.execute("INSERT OR IGNORE INTO agent_messages(id, run_id, kind, ts, body) VALUES(?1, ?2, ?3, ?4, ?5)", rusqlite::params![id, run_id, kind, crate::daemon::now(), canonical])?;
        Ok((id, n == 1))
    }

    /// Any word through the channel means the agent has taken in what it was sent.
    fn picked_up(&self, run_id: &str) -> Result<()> {
        self.store.lock().unwrap().conn.execute("UPDATE dispatches SET state='picked_up', picked_ms=?2 WHERE run_id=?1 AND state IN ('sent', 'delivered')", rusqlite::params![run_id, crate::daemon::now()])?;
        Ok(())
    }

    fn session_id(&self) -> Result<String> {
        Ok(self.overseer_session()?["id"].as_str().unwrap_or_default().to_string())
    }

    /// report: what the agent is doing, has changed, needs and is blocked by.
    pub fn channel_report(self: &Arc<Self>, run_id: &str, args: &Value) -> Result<String> {
        let run = self.run(run_id)?;
        // Redacted at the door: no credential enters a report (AC-200).
        let clean = |s: &str| crate::redact::redact(s.trim());
        let doing = clean(args["doing"].as_str().unwrap_or(""));
        if doing.is_empty() {
            bail!("a report says what you are doing (doing)");
        }
        let body = json!({"doing": doing, "changed": strings(&args["changed"]).iter().map(|s| clean(s)).collect::<Vec<_>>(), "needs": clean(args["needs"].as_str().unwrap_or("")), "blocked": clean(args["blocked"].as_str().unwrap_or(""))});
        let (id, new) = self.channel_message(run_id, "report", &body)?;
        self.picked_up(run_id)?;
        if new {
            let mut payload = body.clone();
            payload["id"] = json!(id);
            payload["title"] = json!(run.title);
            self.emit(Some(&run.task_id), Some(run_id), "report", "agent", "exact", payload)?;
            let mut card = body.clone();
            card["kind"] = json!("report");
            card["id"] = json!(id);
            card["agent"] = json!(run_id);
            card["title"] = json!(run.title);
            let mut text = format!("{} reports: {doing}", run.title);
            if let Some(n) = body["needs"].as_str().filter(|s| !s.is_empty()) {
                text.push_str(&format!(" Needs: {n}"));
            }
            if let Some(b) = body["blocked"].as_str().filter(|s| !s.is_empty()) {
                text.push_str(&format!(" Blocked by: {b}"));
            }
            self.append_session_message(&self.session_id()?, "agent", None, &text, Some(&card))?;
            crate::broker::mark(&self.store.lock().unwrap().conn, &crate::broker::agent_id(&id), "delivered")?;
            // A report Overseer asked for wakes it (one turn for those within the window).
            let asked: i64 = self.store.lock().unwrap().conn.query_row("SELECT COUNT(*) FROM dispatches WHERE run_id=?1 AND action='report' AND state NOT IN ('answered', 'failed', 'cancelled', 'not_sent')", [run_id], |r| r.get(0))?;
            if asked > 0 {
                self.store.lock().unwrap().conn.execute("UPDATE dispatches SET state='answered', answered_ms=?2 WHERE run_id=?1 AND action='report' AND state NOT IN ('answered', 'failed', 'cancelled', 'not_sent')", rusqlite::params![run_id, crate::daemon::now()])?;
                self.check_in_due_at(run_id, &format!("report:{id}"), 0)?;
            }
        }
        Ok(if new { "Recorded.".into() } else { "Recorded (Overseer already had this report).".into() })
    }

    /// ask: a question for Overseer; the answer comes back as a message from Overseer.
    pub fn channel_ask(self: &Arc<Self>, run_id: &str, args: &Value) -> Result<String> {
        let run = self.run(run_id)?;
        let question = crate::redact::redact(args["question"].as_str().unwrap_or("").trim());
        if question.is_empty() {
            bail!("ask needs a question");
        }
        let (id, new) = self.channel_message(run_id, "ask", &json!({"question": question}))?;
        self.picked_up(run_id)?;
        if new {
            self.emit(Some(&run.task_id), Some(run_id), "ask", "agent", "exact", json!({"id": id, "question": question, "title": run.title}))?;
            let card = json!({"kind": "ask", "id": id, "agent": run_id, "title": run.title, "question": question, "answer": Value::Null});
            self.append_session_message(&self.session_id()?, "agent", None, &format!("{} asks: {question}", run.title), Some(&card))?;
            crate::broker::mark(&self.store.lock().unwrap().conn, &crate::broker::agent_id(&id), "delivered")?;
            self.check_in_due_at(run_id, &format!("ask:{id}"), 0)?;
        } else {
            // Asked again and still unanswered: it is due again (a queued one is not doubled).
            let open: i64 = self.store.lock().unwrap().conn.query_row("SELECT COUNT(*) FROM agent_messages WHERE id=?1 AND answer IS NULL", [&id], |r| r.get(0))?;
            if open > 0 {
                self.check_in_due_at(run_id, &format!("ask:{id}"), 0)?;
            }
        }
        Ok("Asked Overseer; its answer arrives as a message from Overseer. Carry on meanwhile.".into())
    }

    /// claim: the paths the agent takes as its area.
    pub fn channel_claim(self: &Arc<Self>, run_id: &str, args: &Value) -> Result<String> {
        let run = self.run(run_id)?;
        let mut paths = Vec::new();
        for p in strings(&args["paths"]).into_iter().chain(args["path"].as_str().map(str::to_string)) {
            paths.push(clean_path(&p)?);
        }
        if paths.is_empty() {
            bail!("claim needs the paths you take");
        }
        paths.sort();
        paths.dedup();
        let (id, new) = self.channel_message(run_id, "claim", &json!({"paths": paths}))?;
        self.picked_up(run_id)?;
        {
            let store = self.store.lock().unwrap();
            // One ledger with Swarm's claims (SWARM-44): a path a Swarm job holds exclusively
            // is refused, whole claim or nothing, and both sides are told.
            let held = ledger_refusals(&store.conn, run_id, &paths)?;
            if !held.is_empty() {
                crate::broker::mark(&store.conn, &crate::broker::agent_id(&id), "refused")?;
                drop(store);
                self.notify_claim_refusals()?;
                bail!("{}", crate::claims::refusal_text(&held));
            }
            for p in &paths {
                store.conn.execute("INSERT OR IGNORE INTO areas(run_id, path, set_by, created_ms) VALUES(?1, ?2, 'agent', ?3)", rusqlite::params![run_id, p, crate::daemon::now()])?;
            }
            crate::broker::mark(&store.conn, &crate::broker::agent_id(&id), "applied")?;
        }
        if new {
            self.emit(Some(&run.task_id), Some(run_id), "claim", "agent", "exact", json!({"id": id, "paths": paths, "title": run.title}))?;
            let card = json!({"kind": "claim", "id": id, "agent": run_id, "title": run.title, "paths": paths});
            self.append_session_message(&self.session_id()?, "agent", None, &format!("{} claims {}", run.title, paths.join(", ")), Some(&card))?;
            self.conflicts_touch(run_id);
            // The agents beside it hear where it works.
            self.brief_companions(run_id)?;
        }
        Ok(format!("Claimed {}.", paths.join(", ")))
    }

    /// The channel's messages of one agent, or of every agent, oldest first.
    pub fn channel_messages(&self, run_id: Option<&str>, limit: i64) -> Result<Value> {
        let store = self.store.lock().unwrap();
        let mut stmt = store.conn.prepare("SELECT id, run_id, kind, ts, body, answer, answered_ms FROM agent_messages WHERE (?1 IS NULL OR run_id=?1) ORDER BY ts DESC LIMIT ?2")?;
        let mut rows: Vec<Value> = stmt
            .query_map(rusqlite::params![run_id, limit.clamp(1, 500)], |r| {
                Ok(json!({"id": r.get::<_, String>(0)?, "run_id": r.get::<_, String>(1)?, "kind": r.get::<_, String>(2)?, "ts": r.get::<_, i64>(3)?, "body": serde_json::from_str::<Value>(&r.get::<_, String>(4)?).unwrap_or(Value::Null), "answer": r.get::<_, Option<String>>(5)?, "answered_ms": r.get::<_, Option<i64>>(6)?}))
            })?
            .collect::<rusqlite::Result<_>>()?;
        rows.reverse();
        Ok(json!({"messages": rows}))
    }

    /// The latest report of a run, and its open questions, for the digest and the rally.
    pub fn channel_summary(&self, run_id: &str) -> (Option<Value>, Vec<Value>) {
        let store = self.store.lock().unwrap();
        let report: Option<Value> = store
            .conn
            .query_row("SELECT id, ts, body FROM agent_messages WHERE run_id=?1 AND kind='report' ORDER BY ts DESC LIMIT 1", [run_id], |r| {
                let mut body: Value = serde_json::from_str::<Value>(&r.get::<_, String>(2)?).unwrap_or(json!({}));
                body["id"] = json!(r.get::<_, String>(0)?);
                body["ts"] = json!(r.get::<_, i64>(1)?);
                Ok(body)
            })
            .ok();
        let asks: Vec<Value> = store
            .conn
            .prepare("SELECT id, body, answer FROM agent_messages WHERE run_id=?1 AND kind='ask' ORDER BY ts DESC LIMIT 3")
            .and_then(|mut s| s.query_map([run_id], |r| Ok(json!({"id": r.get::<_, String>(0)?, "question": serde_json::from_str::<Value>(&r.get::<_, String>(1)?).ok().and_then(|b| b["question"].as_str().map(str::to_string)), "answer": r.get::<_, Option<String>>(2)?}))).map(|rows| rows.flatten().collect()))
            .unwrap_or_default();
        (report, asks)
    }

    /// Overseer's answer to a question: recorded on it, sent to the agent as a message from
    /// Overseer, and shown in the conversation with the question.
    pub fn answer_ask(self: &Arc<Self>, ask_id: &str, text: &str, proposal: &str, by: &str) -> Result<String> {
        use rusqlite::OptionalExtension;
        let text = crate::redact::redact(text.trim());
        let text = text.as_str();
        if text.is_empty() {
            bail!("an answer needs text");
        }
        let row: Option<(String, String)> = self.store.lock().unwrap().conn.query_row("SELECT run_id, body FROM agent_messages WHERE id=?1 AND kind='ask'", [ask_id], |r| Ok((r.get(0)?, r.get(1)?))).optional()?;
        let (run_id, body) = row.ok_or_else(|| anyhow!("no question {ask_id}"))?;
        let question = serde_json::from_str::<Value>(&body).ok().and_then(|b| b["question"].as_str().map(str::to_string)).unwrap_or_default();
        let run = self.run(&run_id)?;
        self.store.lock().unwrap().conn.execute("UPDATE agent_messages SET answer=?2, answered_ms=?3 WHERE id=?1", rusqlite::params![ask_id, text, crate::daemon::now()])?;
        let msg = format!("Answer to your question “{question}”: {text}");
        let delivery = self.queue_message(&run_id, &msg, "overseer", json!({"proposal": proposal, "by": by, "ask": ask_id}))?;
        self.dispatch_record(proposal, &run_id, "answer", "add", &msg, "asked", if delivery == "queued" { "held" } else { "delivered" })?;
        self.emit(Some(&run.task_id), Some(&run_id), "answer", "overseer", "exact", json!({"ask": ask_id, "question": question, "answer": text}))?;
        let card = json!({"kind": "answer", "ask": ask_id, "agent": run_id, "title": run.title, "question": question, "answer": text});
        self.append_session_message(&self.session_id()?, "overseer", None, &format!("To {} (“{question}”): {text}", run.title), Some(&card))?;
        Ok(format!("answered {}{}", run.title, if delivery == "queued" { " (queued until its turn ends)" } else { "" }))
    }

    /// An area set from the conversation (rally's yes, or the owner): replaces what was there.
    pub fn set_area(self: &Arc<Self>, run_id: &str, paths: &[String], by: &str) -> Result<Value> {
        let run = self.run(run_id)?;
        let mut clean = Vec::new();
        for p in paths {
            clean.push(clean_path(p)?);
        }
        if clean.is_empty() {
            bail!("an area needs paths");
        }
        {
            let store = self.store.lock().unwrap();
            let held = ledger_refusals(&store.conn, run_id, &clean)?;
            if !held.is_empty() {
                drop(store);
                self.notify_claim_refusals()?;
                bail!("{}", crate::claims::refusal_text(&held));
            }
            store.conn.execute("DELETE FROM areas WHERE run_id=?1", [run_id])?;
            for p in &clean {
                store.conn.execute("INSERT OR IGNORE INTO areas(run_id, path, set_by, created_ms) VALUES(?1, ?2, ?3, ?4)", rusqlite::params![run_id, p, by, crate::daemon::now()])?;
            }
        }
        self.emit(Some(&run.task_id), Some(run_id), "area", by, "exact", json!({"paths": clean, "by": by}))?;
        self.conflicts_touch(run_id);
        self.brief_companions(run_id)?;
        Ok(json!({"run_id": run_id, "area": clean}))
    }

    // ------------------------------------------------------------------ rally

    /// Who owns what, where they overlap and what each needs, from the digests and the reports,
    /// with no model: the map Overseer reads, and the agents whose digest cannot answer (no area
    /// and no report), which a report would cost one agent turn each.
    pub fn rally(&self, repo: Option<&str>, agents: Option<Vec<String>>) -> Result<Value> {
        let (runs, tasks) = {
            let store = self.store.lock().unwrap();
            (store.runs()?, store.tasks()?)
        };
        let top: Vec<&crate::store::Run> = runs.iter().filter(|r| r.parent_run_id.is_none() && self.run_role(&r.id) == "agent" && tasks.iter().any(|t| t.id == r.task_id && t.archived_ms.is_none())).collect();
        let repo_of = |r: &crate::store::Run| tasks.iter().find(|t| t.id == r.task_id).map(|t| t.repo_root.clone()).unwrap_or_default();
        let chosen: Vec<&crate::store::Run> = match agents {
            Some(ids) if !ids.is_empty() => top.iter().copied().filter(|r| ids.contains(&r.id)).collect(),
            _ => {
                let repo = match repo.filter(|r| !r.is_empty()) {
                    Some(r) => crate::git::toplevel(std::path::Path::new(r)).map(|p| p.display().to_string()).unwrap_or_else(|_| r.to_string()),
                    None => {
                        // The repository with the most active agents.
                        let mut counts: BTreeMap<String, usize> = BTreeMap::new();
                        for r in &top {
                            *counts.entry(repo_of(r)).or_default() += if ACTIVE.contains(&r.status.as_str()) { 100 } else { 1 };
                        }
                        counts.into_iter().max_by_key(|(_, n)| *n).map(|(r, _)| r).unwrap_or_default()
                    }
                };
                top.iter().copied().filter(|r| repo_of(r) == repo).collect()
            }
        };
        let repository = chosen.first().map(|r| repo_of(r)).unwrap_or_else(|| repo.unwrap_or("").to_string());
        let mut map = Vec::new();
        let mut changed_by: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut ask = Vec::new();
        for r in &chosen {
            let d = self.digest(&r.id)?;
            let (report, asks) = self.channel_summary(&r.id);
            let mut paths: Vec<String> = d.changed.iter().map(|c| c.path.clone()).collect();
            if let Some(rep) = &report {
                for p in strings(&rep["changed"]) {
                    if !paths.contains(&p) {
                        paths.push(p);
                    }
                }
            }
            for p in &paths {
                changed_by.entry(p.clone()).or_default().push(r.id.clone());
            }
            let suggested = if d.area.is_empty() { suggest_area(&paths) } else { Vec::new() };
            if d.area.is_empty() && report.is_none() {
                ask.push(r.id.clone());
            }
            map.push(json!({
                "id": r.id, "title": d.title, "status": d.status, "area": d.area, "suggested_area": suggested, "changed": paths,
                "doing": report.as_ref().and_then(|x| x["doing"].as_str()).unwrap_or(""), "needs": report.as_ref().and_then(|x| x["needs"].as_str()).unwrap_or(""),
                "blocked": report.as_ref().and_then(|x| x["blocked"].as_str()).unwrap_or(""), "has_report": report.is_some(),
                "open_questions": asks.iter().filter(|a| a["answer"].is_null()).map(|a| a["question"].clone()).collect::<Vec<_>>(),
            }));
        }
        let overlaps: Vec<Value> = changed_by.iter().filter(|(_, ids)| ids.len() > 1).map(|(p, ids)| json!({"path": p, "agents": ids})).collect();
        let ids: Vec<&str> = chosen.iter().map(|r| r.id.as_str()).collect();
        let conflicts: Vec<Value> = self.conflicts_list(None, false)?["conflicts"].as_array().cloned().unwrap_or_default().into_iter().filter(|c| ids.contains(&c["run_a"].as_str().unwrap_or("")) || ids.contains(&c["run_b"].as_str().unwrap_or(""))).collect();
        Ok(json!({
            "repository": repository, "agents": map, "overlaps": overlaps, "conflicts": conflicts, "ask": ask,
            "cost": format!("{} agent turn{}", ask.len(), if ask.len() == 1 { "" } else { "s" }),
            "note": if ask.is_empty() { "Every agent's digest answers; propose the areas (area actions, one per agent from suggested_area or your reading) and any shares, in one proposal." } else { "The agents in `ask` have no area and no report; asking each for a report costs one agent turn (propose report actions for them, in one proposal, and say the cost). The map is complete once their reports are in." },
        }))
    }

    // ------------------------------------------------------------------ shares

    pub fn share_denied(&self, run_id: &str) -> bool {
        let store = self.store.lock().unwrap();
        store.conn.query_row("SELECT COUNT(*) FROM share_denials WHERE run_id=?1", [run_id], |r| r.get::<_, i64>(0)).map(|n| n > 0).unwrap_or(false)
    }

    pub fn share_deny(&self, run_id: &str, denied: bool, by: &str) -> Result<Value> {
        self.run(run_id)?;
        let store = self.store.lock().unwrap();
        if denied {
            store.conn.execute("INSERT OR REPLACE INTO share_denials(run_id, set_by, set_ms) VALUES(?1, ?2, ?3)", rusqlite::params![run_id, by, crate::daemon::now()])?;
        } else {
            store.conn.execute("DELETE FROM share_denials WHERE run_id=?1", [run_id])?;
        }
        Ok(json!({"run_id": run_id, "denied": denied}))
    }

    /// A run's worktree, its task's base and the tree of everything in its worktree now
    /// (uncommitted changes included), read without touching its index or checkout.
    fn base_and_tree(&self, run_id: &str) -> Result<(std::path::PathBuf, String, String)> {
        let run = self.run(run_id)?;
        let ws = self.workspace(&run.workspace_id)?;
        let root = std::fs::canonicalize(&ws.path).map_err(|_| anyhow!("the worktree of {} is gone", run.title))?;
        let task = self.task(&run.task_id)?;
        let base = {
            let store = self.store.lock().unwrap();
            task.start_snapshot.as_deref().and_then(|id| store.snapshot(id).ok().flatten()).map(|s| s.commit_sha)
        }
        .or_else(|| crate::git::head(&root))
        .ok_or_else(|| anyhow!("no base to diff against"))?;
        let trees = crate::git::capture_trees(&root, &crate::paths::data_dir().join("tmp"))?;
        Ok((root, base, trees.worktree_tree))
    }

    /// A large diff shared within one repository is also a commit on a branch of its own
    /// (`overseer/share/<id>`), made from the source's worktree as it is, on its task's base:
    /// the receiving agent reads it with git (AC-191). Nothing in either worktree changes.
    fn share_branch(&self, from: &str, to: &str, id: &str, source: &str) -> Result<Option<(String, String)>> {
        let (a, b) = (self.workspace(&self.run(from)?.workspace_id)?, self.workspace(&self.run(to)?.workspace_id)?);
        if a.common_dir != b.common_dir {
            return Ok(None);
        }
        let (root, base, tree) = self.base_and_tree(from)?;
        let message = format!("Shared by Overseer {source} (share {id})");
        let sha = crate::git::git(&root, &["-c", "user.name=Overseer", "-c", "user.email=overseer@localhost", "-c", "commit.gpgsign=false", "commit-tree", &tree, "-p", &base, "-m", &message])?.trim().to_string();
        let branch = format!("overseer/share/{id}");
        crate::git::git(&root, &["update-ref", &format!("refs/heads/{branch}"), &sha])?;
        Ok(Some((branch, sha)))
    }

    /// A run's diff against its task's base: one file, or everything.
    fn diff_of(&self, run_id: &str, path: Option<&str>) -> Result<String> {
        let (root, base, tree) = self.base_and_tree(run_id)?;
        let mut args = vec!["diff", "--no-color", &base, &tree];
        if let Some(p) = path {
            if p.is_empty() || p.starts_with('/') || p.split('/').any(|c| c == "..") {
                bail!("{p:?} is not a path inside the worktree");
            }
            args.extend(["--", p]);
        }
        let diff = crate::git::git(&root, &args)?;
        Ok(if diff.is_empty() { format!("{} is unchanged against the task's base.", path.unwrap_or("the worktree")) } else { diff })
    }

    /// Where a share goes and where it comes from decide its class: Steer within one repository,
    /// Confirm across. `None` when the source is Overseer's own note.
    pub fn share_across_repositories(&self, a: &Value) -> Result<bool> {
        let to = a["to"].as_str().or(a["agent"].as_str()).unwrap_or("");
        let Some(from) = a["from"].as_str().filter(|s| !s.is_empty()) else { return Ok(false) };
        let repo = |id: &str| -> Result<String> {
            let run = self.run(id).map_err(|_| anyhow!("no agent {id}"))?;
            Ok(self.task(&run.task_id)?.repo_root)
        };
        Ok(repo(from)? != repo(to)?)
    }

    /// Share: one agent's report, diff, messages, or a note or finding Overseer wrote, sent to
    /// another agent as a message from Overseer that names where it came from.
    pub fn share_perform(self: &Arc<Self>, a: &Value, proposal: &str, by: &str) -> Result<String> {
        let to = a["to"].as_str().or(a["agent"].as_str()).filter(|s| !s.is_empty()).ok_or_else(|| anyhow!("share needs the agent it goes to (to)"))?;
        let to_run = self.run(to).map_err(|_| anyhow!("no agent {to}"))?;
        if self.share_denied(to) {
            bail!("the owner denied shares to {}", to_run.title);
        }
        let from = a["from"].as_str().filter(|s| !s.is_empty());
        let what = a["what"].as_str().unwrap_or(if from.is_some() { "report" } else { "note" });
        let from_title = match from {
            Some(f) => Some(crate::redact::redact(&self.run(f).map_err(|_| anyhow!("no agent {f}"))?.title)),
            None => None,
        };
        let (label, content) = match what {
            "diff" => {
                let f = from.ok_or_else(|| anyhow!("a diff is shared from an agent (from)"))?;
                match a["path"].as_str().filter(|p| !p.is_empty()) {
                    Some(p) => (format!("diff of {p}"), self.diff_of(f, Some(p))?),
                    None => ("diff".to_string(), self.diff_of(f, None)?),
                }
            }
            "report" => {
                let f = from.ok_or_else(|| anyhow!("a report is shared from an agent (from)"))?;
                let (report, _) = self.channel_summary(f);
                let r = report.ok_or_else(|| anyhow!("{} has sent no report", from_title.clone().unwrap_or_default()))?;
                ("report".to_string(), format!("doing: {}\nchanged: {}\nneeds: {}\nblocked: {}", r["doing"].as_str().unwrap_or(""), strings(&r["changed"]).join(", "), r["needs"].as_str().unwrap_or(""), r["blocked"].as_str().unwrap_or("")))
            }
            "messages" => {
                let f = from.ok_or_else(|| anyhow!("messages are shared from an agent (from)"))?;
                ("messages".to_string(), self.conversation_text(f, a["after"].as_i64().unwrap_or(0), a["limit"].as_i64().unwrap_or(200))?)
            }
            "note" | "finding" => {
                let text = a["text"].as_str().unwrap_or("").trim();
                if text.is_empty() {
                    bail!("a {what} needs text");
                }
                (what.to_string(), text.to_string())
            }
            other => bail!("share carries a diff, a report, messages, a note or a finding, not {other:?}"),
        };
        let raw_len = content.len();
        let raw_id = short_sha(&content);
        let content = crate::redact::redact(&content);
        // Nothing was redacted: the piece may also travel as a commit (a commit carries the files as they are).
        let clean = content.len() == raw_len && short_sha(&content) == raw_id;
        let source = match &from_title {
            Some(t) => format!("from {t} ({label})"),
            None => format!("(Overseer's {label})"),
        };
        let id = format!("sh-{}", &uuid::Uuid::new_v4().simple().to_string()[..12]);
        let content_id = short_sha(&content);
        let bytes = content.len();
        let (inline, file) = if bytes <= SHARE_INLINE_BYTES {
            (content.clone(), None)
        } else {
            let dir = crate::paths::runs_dir().join(to).join("shares");
            crate::paths::ensure_private_dir(&dir)?;
            let path = dir.join(format!("{id}.patch"));
            std::fs::write(&path, &content)?;
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
            }
            (format!("{}\n[… the first part of {bytes} bytes; the whole piece is in {}]", cut(&content, SHARE_INLINE_BYTES - 256), path.display()), Some(path.display().to_string()))
        };
        // The whole of a large diff of everything is also a commit the receiver can read with git.
        let branch = match (what, from, &file, a["path"].as_str().filter(|p| !p.is_empty())) {
            // A branch that cannot be made (a ref in the way) leaves the patch file, which is enough.
            ("diff", Some(f), Some(_), None) if clean => self.share_branch(f, to, &id, &source).unwrap_or_else(|e| {
                crate::log(&format!("share {id}: no branch: {e:#}"));
                None
            }),
            _ => None,
        };
        let msg = format!(
            "Shared by Overseer {source}{}{}:\n{inline}",
            file.as_ref().map(|f| format!(", {bytes} bytes, the whole piece at {f}")).unwrap_or_default(),
            branch.as_ref().map(|(b, c)| format!(", and as commit {c} on branch {b} (git show {c})")).unwrap_or_default()
        );
        let delivery = self.queue_message(to, &msg, "overseer", json!({"proposal": proposal, "by": by, "share": id}))?;
        {
            let store = self.store.lock().unwrap();
            store.conn.execute(
                "INSERT INTO shares(id, ts, from_run, to_run, kind, source, bytes, inline_bytes, file, proposal, content_id, branch, commit_sha) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                rusqlite::params![id, crate::daemon::now(), from, to, what, source, bytes as i64, inline.len() as i64, file, proposal, content_id, branch.as_ref().map(|b| b.0.clone()), branch.as_ref().map(|b| b.1.clone())],
            )?;
        }
        self.emit(Some(&to_run.task_id), Some(to), "share", "overseer", "exact", json!({"id": id, "from": from, "what": what, "source": source, "bytes": bytes, "inline_bytes": inline.len(), "file": file, "branch": branch.as_ref().map(|b| b.0.clone()), "commit": branch.as_ref().map(|b| b.1.clone()), "proposal": proposal, "by": by}))?;
        self.dispatch_record(proposal, to, "share", "add", &format!("{source}: {bytes} bytes"), a["why"].as_str().unwrap_or("named"), if delivery == "queued" { "held" } else { "delivered" })?;
        Ok(format!("shared {source} with {}{}", to_run.title, if delivery == "queued" { " (queued until its turn ends)" } else { "" }))
    }

    /// A share that turned out wrong: withdrawn, and everyone who received the same piece is told.
    pub fn share_withdraw(self: &Arc<Self>, id: &str, by: &str) -> Result<Value> {
        use rusqlite::OptionalExtension;
        let content_id: Option<String> = self.store.lock().unwrap().conn.query_row("SELECT content_id FROM shares WHERE id=?1", [id], |r| r.get(0)).optional()?;
        let content_id = content_id.ok_or_else(|| anyhow!("no share {id}"))?;
        let rows: Vec<(String, String, String)> = {
            let store = self.store.lock().unwrap();
            let mut stmt = store.conn.prepare("SELECT id, to_run, source FROM shares WHERE content_id=?1 AND withdrawn_ms IS NULL ORDER BY ts")?;
            let rows = stmt.query_map([&content_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<rusqlite::Result<_>>()?;
            rows
        };
        if rows.is_empty() {
            bail!("share {id} was already withdrawn");
        }
        let mut told = Vec::new();
        for (sid, to, source) in &rows {
            self.store.lock().unwrap().conn.execute("UPDATE shares SET withdrawn_ms=?2 WHERE id=?1", rusqlite::params![sid, crate::daemon::now()])?;
            // A withdrawn share's branch goes too, so nobody builds on it later.
            let branch: Option<(String, String)> = self.store.lock().unwrap().conn.query_row("SELECT branch, from_run FROM shares WHERE id=?1 AND branch IS NOT NULL", [sid], |r| Ok((r.get(0)?, r.get(1)?))).optional()?;
            if let Some((b, from)) = branch {
                if let Ok(ws) = self.run(&from).and_then(|r| self.workspace(&r.workspace_id)) {
                    let _ = crate::git::git(std::path::Path::new(&ws.path), &["update-ref", "-d", &format!("refs/heads/{b}")]);
                }
            }
            let msg = format!("Withdrawn: what Overseer shared {source} (share {sid}) was wrong; do not rely on it.");
            self.queue_message(to, &msg, "overseer", json!({"withdraw": sid, "by": by}))?;
            if let Ok(run) = self.run(to) {
                self.emit(Some(&run.task_id), Some(to), "share_withdrawn", "overseer", "exact", json!({"id": sid, "source": source, "by": by}))?;
                told.push(json!({"share": sid, "agent": to, "title": run.title}));
            }
        }
        let card = json!({"kind": "withdrawn", "share": id, "source": rows[0].2, "told": told});
        self.append_session_message(&self.session_id()?, "overseer", None, &format!("Withdrew the share {} from {} agent{}", rows[0].2, told.len(), if told.len() == 1 { "" } else { "s" }), Some(&card))?;
        Ok(json!({"withdrawn": rows.iter().map(|r| r.0.clone()).collect::<Vec<_>>(), "told": told}))
    }

    pub fn shares_list(&self, run_id: Option<&str>) -> Result<Value> {
        let store = self.store.lock().unwrap();
        let mut stmt = store.conn.prepare("SELECT id, ts, from_run, to_run, kind, source, bytes, inline_bytes, file, proposal, withdrawn_ms, branch, commit_sha FROM shares WHERE (?1 IS NULL OR to_run=?1 OR from_run=?1) ORDER BY ts")?;
        let rows: Vec<Value> = stmt
            .query_map([run_id], |r| {
                Ok(json!({"id": r.get::<_, String>(0)?, "ts": r.get::<_, i64>(1)?, "from": r.get::<_, Option<String>>(2)?, "to": r.get::<_, String>(3)?, "kind": r.get::<_, String>(4)?, "source": r.get::<_, String>(5)?, "bytes": r.get::<_, i64>(6)?, "inline_bytes": r.get::<_, i64>(7)?, "file": r.get::<_, Option<String>>(8)?, "proposal": r.get::<_, Option<String>>(9)?, "withdrawn_ms": r.get::<_, Option<i64>>(10)?, "branch": r.get::<_, Option<String>>(11)?, "commit": r.get::<_, Option<String>>(12)?}))
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(json!({"shares": rows}))
    }
}

impl Daemon {
    /// For a run of the daemon's own (Overseer's run, a watcher): whether it runs read-only. A
    /// watcher with a copy to check in runs under its own permission mode; the others read only.
    pub fn own_read_only(&self, run_id: &str) -> Option<bool> {
        match self.run_role(run_id).as_str() {
            "overseer" => Some(true),
            "watcher" => Some(self.watch_of_watcher(run_id).map(|w| w.copy_workspace.is_none()).unwrap_or(true)),
            _ => None,
        }
    }

    /// Continuity is about to start `successor` in place of `predecessor`. A run of the daemon's
    /// own keeps its role (Overseer's run stays hidden and stays the conversation's; a watcher stays
    /// the watch's), gets a token of its own and the launch that gives its role's tools on the new
    /// harness, read-only as before (AC-197). Returns the permission mode that launch needs, or
    /// None for an agent, whose mode Continuity carries itself.
    pub fn carry_role(&self, predecessor: &str, successor: &crate::store::Run, harness: &str) -> Result<Option<Option<String>>> {
        let role = self.run_role(predecessor);
        let Some(read_only) = self.own_read_only(predecessor) else { return Ok(None) };
        let dir = std::path::PathBuf::from(self.workspace(&successor.workspace_id)?.path);
        let (extra_args, mut mode) = if role == "overseer" {
            // Its actual successor turn owns the native capability/config;
            // never persist a run-wide Overseer credential for later resumes.
            (Vec::new(), (harness == "codex").then_some("read-only"))
        } else {
            let token = self.overseer_token(&successor.id, &role)?["token"].as_str().unwrap_or_default().to_string();
            self.tools_launch(harness, &dir, &token, &role, read_only)?
        };
        // OpenCode's local server has no tools of the daemon's; its plan agent reads and never edits.
        if read_only && !["claude", "codex", "opencode"].contains(&harness) {
            mode = Some("plan");
        }
        let mode = mode.map(str::to_string);
        {
            let store = self.store.lock().unwrap();
            store.conn.execute("INSERT OR REPLACE INTO run_roles(run_id, role) VALUES(?1, ?2)", rusqlite::params![successor.id, role])?;
            store.conn.execute("UPDATE runs SET launch=?2 WHERE id=?1", rusqlite::params![successor.id, json!({"extra_args": extra_args, "opts": {"mode": mode}}).to_string()])?;
            // The predecessor's token speaks for nobody now.
            store.conn.execute("DELETE FROM overseer_tokens WHERE run_id=?1", [predecessor])?;
            if role == "overseer" {
                store.conn.execute("UPDATE overseer_sessions SET run_id=?2 WHERE run_id=?1", rusqlite::params![predecessor, successor.id])?;
            }
        }
        self.emit(Some(&successor.task_id), Some(&successor.id), "role_moved", "daemon", "exact", json!({"from": predecessor, "role": role, "read_only": read_only, "harness": harness, "mode": mode}))?;
        Ok(read_only.then_some(mode))
    }

    /// A handoff that could not start: the conversation goes back to the run that holds the work.
    pub fn uncarry_role(&self, successor: &str, predecessor: &str) -> Result<()> {
        if self.run_role(successor) == "overseer" {
            self.store.lock().unwrap().conn.execute("UPDATE overseer_sessions SET run_id=?2 WHERE run_id=?1", rusqlite::params![successor, predecessor])?;
        }
        Ok(())
    }

    /// Continuity handed an agent off: its holds, guardrails, area, watches, conflicts, cadence,
    /// channel setting and queued messages move to the successor, so it stays one agent to
    /// Overseer (AC-197).
    pub fn adopt_successor(self: &Arc<Self>, predecessor: &str, successor: &str) -> Result<()> {
        let moved: Vec<(&str, usize)> = {
            let store = self.store.lock().unwrap();
            let mut moved = Vec::new();
            let tx = store.conn.unchecked_transaction()?;
            // Old Stop controls still address the current owner, including the interval before
            // Continuity records its completed handoff. Rollback migration reverses these aliases.
            tx.execute("UPDATE queue_owners SET owner_id=?2 WHERE owner_id=?1", rusqlite::params![predecessor, successor])?;
            tx.execute("INSERT OR REPLACE INTO queue_owners(run_id, owner_id) VALUES(?1, ?2)", rusqlite::params![predecessor, successor])?;
            tx.execute("DELETE FROM queue_owners WHERE run_id=owner_id", [])?;
            store.conn.execute("INSERT INTO queue_states(run_id, paused, serial) SELECT ?2, paused, serial FROM queue_states WHERE run_id=?1 ON CONFLICT(run_id) DO UPDATE SET paused=MAX(queue_states.paused, excluded.paused), serial=MAX(queue_states.serial, excluded.serial)", rusqlite::params![predecessor, successor])?;
            store.conn.execute("DELETE FROM queue_states WHERE run_id=?1", [predecessor])?;
            for (table, column) in [("holds", "run_id"), ("guardrails", "run_id"), ("areas", "run_id"), ("cadences", "run_id"), ("channels", "run_id"), ("queued_messages", "run_id"), ("share_denials", "run_id"), ("watches", "subject"), ("watches", "watcher"), ("conflicts", "run_a"), ("conflicts", "run_b"), ("dispatches", "run_id")] {
                // Keys that would collide keep the successor's own row.
                let n = store.conn.execute(&format!("UPDATE OR IGNORE {table} SET {column}=?2 WHERE {column}=?1"), rusqlite::params![predecessor, successor])?;
                if n > 0 {
                    moved.push((table, n));
                }
            }
            tx.commit()?;
            moved
        };
        if moved.is_empty() {
            return Ok(());
        }
        let run = self.run(successor)?;
        // A guardrail reads enforced only where the new harness itself refuses the write (AC-187).
        self.store.lock().unwrap().conn.execute(
            "UPDATE guardrails SET enforcement=CASE WHEN ?2='claude' AND deny<>'[]' THEN 'enforced' ELSE 'watched' END WHERE run_id=?1 AND removed_ms IS NULL",
            rusqlite::params![successor, run.harness],
        )?;
        self.emit(Some(&run.task_id), Some(successor), "oversight_moved", "daemon", "exact", json!({"from": predecessor, "moved": moved.iter().map(|(t, n)| json!({"table": t, "rows": n})).collect::<Vec<_>>()}))?;
        self.conflicts_touch(successor);
        Ok(())
    }
}
