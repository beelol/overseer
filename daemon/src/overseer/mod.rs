//! Overseer itself (Gate S): the orchestrator that knows every agent the daemon runs.
//! This module holds what the daemon coordinates with no model (digests, tokens, tools for the
//! agents) and the conversation with Overseer. The model never touches an agent, a worktree or a
//! shell directly: it reads through `overseer.tool` and asks the daemon to act.

pub mod channel;
pub mod checkin;
pub mod conflicts;
pub mod control;
pub mod digest;
pub mod mcp;
pub mod session;
pub mod watch;

use crate::daemon::Daemon;
use anyhow::{bail, Result};
use serde_json::{json, Value};
use sha2::Digest;

/// Tools a run may call, by its role. A token names the run and its role; the text of a call
/// never decides who is speaking.
pub(crate) fn tool_list(role: &str) -> Vec<Value> {
    let mut tools = vec![json!({
        "name": "roster",
        "description": "Every agent Overseer runs, one line each: id, title, status, harness, repository, worktree, files changed.",
        "inputSchema": {"type": "object", "properties": {}, "additionalProperties": false}
    })];
    if role == "agent" {
        // The channel (AC-190): report, ask and claim, attributed by the token.
        tools.push(json!({
            "name": "report",
            "description": "Tell Overseer what you are doing, what you have changed, what you need and what blocks you. The same report sent again has one effect.",
            "inputSchema": {"type": "object", "properties": {"doing": {"type": "string"}, "changed": {"type": "array", "items": {"type": "string"}}, "needs": {"type": "string"}, "blocked": {"type": "string"}}, "required": ["doing"], "additionalProperties": false}
        }));
        tools.push(json!({
            "name": "ask",
            "description": "Ask Overseer a question about the other agents or the work; it answers from what it knows or asks the agent concerned, and the answer arrives as a message from Overseer.",
            "inputSchema": {"type": "object", "properties": {"question": {"type": "string"}}, "required": ["question"], "additionalProperties": false}
        }));
        tools.push(json!({
            "name": "claim",
            "description": "Claim the paths (files or directories, relative to the repository) you are taking as your area; another agent writing there is a conflict.",
            "inputSchema": {"type": "object", "properties": {"paths": {"type": "array", "items": {"type": "string"}}}, "required": ["paths"], "additionalProperties": false}
        }));
    }
    if role == "watcher" {
        // A watcher reads its subject (the daemon holds every read to it) and files findings.
        tools.push(json!({
            "name": "agent",
            "description": "Your subject's digest: what was asked, status, changed files, last messages, what it waits for.",
            "inputSchema": {"type": "object", "properties": {"id": {"type": "string"}}, "required": ["id"], "additionalProperties": false}
        }));
        for (name, description, props, required) in [
            ("conversation", "A range of your subject's conversation: its messages and tool steps, oldest first, from an event sequence number (after) up to a limit.", json!({"id": {"type": "string"}, "after": {"type": "integer"}, "limit": {"type": "integer"}}), vec!["id"]),
            ("changes", "Your subject's changed files with added and removed line counts, against its task's base.", json!({"id": {"type": "string"}}), vec!["id"]),
            ("diff", "One file's diff in your subject's worktree against its task's base.", json!({"id": {"type": "string"}, "path": {"type": "string"}}), vec!["id", "path"]),
            ("file", "One file's contents in your subject's worktree.", json!({"id": {"type": "string"}, "path": {"type": "string"}}), vec!["id", "path"]),
        ] {
            tools.push(json!({"name": name, "description": description, "inputSchema": {"type": "object", "properties": props, "required": required, "additionalProperties": false}}));
        }
        tools.push(json!({
            "name": "finding",
            "description": "Your finding on the subject after a wake: fine (nothing to report; stays silent), concern (say what and where) or stop (the subject must be stopped; say why, with the evidence). You only read; Overseer acts on it.",
            "inputSchema": {"type": "object", "properties": {"result": {"type": "string", "enum": ["fine", "concern", "stop"]}, "text": {"type": "string"}}, "required": ["result"], "additionalProperties": false}
        }));
    }
    if role == "overseer" {
        tools.push(json!({
            "name": "agent",
            "description": "One agent's digest by id: what was asked, status, changed files, last messages, what it waits for.",
            "inputSchema": {"type": "object", "properties": {"id": {"type": "string"}}, "required": ["id"], "additionalProperties": false}
        }));
        tools.push(json!({
            "name": "conflicts",
            "description": "Open conflicts between agents in flight: same lines, same file, area crossed, target moved; each with the agents and the files.",
            "inputSchema": {"type": "object", "properties": {}, "additionalProperties": false}
        }));
        for (name, description, props, required) in [
            ("conversation", "A range of an agent's conversation by id: its messages and tool steps, oldest first, from an event sequence number (after) up to a limit; the answer names the last sequence so the next call can continue.", json!({"id": {"type": "string"}, "after": {"type": "integer"}, "limit": {"type": "integer"}}), vec!["id"]),
            ("changes", "An agent's changed files with added and removed line counts, against its task's base.", json!({"id": {"type": "string"}}), vec!["id"]),
            ("diff", "One file's diff in an agent's worktree against its task's base.", json!({"id": {"type": "string"}, "path": {"type": "string"}}), vec!["id", "path"]),
            ("file", "One file's contents in an agent's worktree (paths stay inside the worktree).", json!({"id": {"type": "string"}, "path": {"type": "string"}}), vec!["id", "path"]),
            ("search", "Agents whose title, prompt, messages, files, repository, account or status match a query.", json!({"query": {"type": "string"}}), vec!["query"]),
            ("usage", "What an agent's harness reported as usage, or 'not reported'.", json!({"id": {"type": "string"}}), vec!["id"]),
        ] {
            tools.push(json!({"name": name, "description": description, "inputSchema": {"type": "object", "properties": props, "required": required, "additionalProperties": false}}));
        }
        tools.push(json!({
            "name": "check_in",
            "description": "Your result of a check-in on one agent: on_task (nothing is sent to it), drifting (say why; then use propose for what to do), or done (what it did, and what it left out if anything).",
            "inputSchema": {"type": "object", "properties": {"agent": {"type": "string"}, "result": {"type": "string", "enum": ["on_task", "drifting", "done"]}, "reason": {"type": "string"}, "left_out": {"type": "string"}}, "required": ["agent", "result", "reason"], "additionalProperties": false}
        }));
        tools.push(json!({
            "name": "rally",
            "description": "The map of the agents in one repository (by default the one with the most active agents): who owns what (areas), where they overlap, what each needs, and which agents' digests cannot answer (no area, no report) and would cost one agent turn each to ask for a report. Built by the daemon with no model.",
            "inputSchema": {"type": "object", "properties": {"repo": {"type": "string"}, "agents": {"type": "array", "items": {"type": "string"}}}, "additionalProperties": false}
        }));
        tools.push(json!({
            "name": "answer",
            "description": "Answer an agent's question (its ask id): the answer goes to the agent as a message from Overseer, at the owner's level.",
            "inputSchema": {"type": "object", "properties": {"ask": {"type": "string"}, "text": {"type": "string"}}, "required": ["ask", "text"], "additionalProperties": false}
        }));
        tools.push(json!({
            "name": "propose",
            "description": "Ask the daemon for actions on agents: message (agent, text), stop (agent), pin (agent), start (repo, title, prompt), hold (agent, reason), release (agent), guardrail (agent, words, allow, deny), redirect (agent, text), cadence (agent, cadence), report (agent: ask it for a report), area (agent, paths), share (to, from, what: diff|report|messages|note|finding, path, text), withdraw (share), archive (agent). The daemon checks each one and, at the owner's level, either records a proposal that waits for the owner's yes or carries it out. Returns what happened.",
            "inputSchema": {"type": "object", "properties": {"actions": {"type": "array", "items": {"type": "object", "properties": {"action": {"type": "string", "enum": ["message", "stop", "pin", "start", "hold", "release", "guardrail", "redirect", "cadence", "report", "area", "share", "withdraw", "archive", "answer"]}, "agent": {"type": "string"}, "text": {"type": "string"}, "repo": {"type": "string"}, "title": {"type": "string"}, "prompt": {"type": "string"}, "reason": {"type": "string"}, "paths": {"type": "array", "items": {"type": "string"}}, "to": {"type": "string"}, "from": {"type": "string"}, "what": {"type": "string"}, "path": {"type": "string"}, "share": {"type": "string"}, "ask": {"type": "string"}, "cadence": {"type": "string"}, "words": {"type": "string"}, "allow": {"type": "array", "items": {"type": "string"}}, "deny": {"type": "array", "items": {"type": "string"}}, "confidence": {"type": "string", "enum": ["high", "medium", "low"]}, "why": {"type": "string"}}, "required": ["action"]}}}, "required": ["actions"], "additionalProperties": false}
        }));
    }
    tools
}

impl Daemon {
    /// Issue a token for a run (or for Overseer's own run) so its tool calls are attributed.
    pub fn overseer_token(&self, run_id: &str, role: &str) -> Result<Value> {
        if !["agent", "watcher", "overseer"].contains(&role) {
            bail!("unknown role {role}");
        }
        let token = uuid::Uuid::new_v4().simple().to_string();
        let sha = format!("{:x}", sha2::Sha256::digest(token.as_bytes()));
        let store = self.store.lock().unwrap();
        store.conn.execute("INSERT INTO overseer_tokens(sha, run_id, role, created_ms) VALUES(?1, ?2, ?3, ?4)", rusqlite::params![sha, run_id, role, crate::daemon::now()])?;
        Ok(json!({"token": token, "run_id": run_id, "role": role}))
    }

    fn token_holder(&self, token: &str) -> Result<(String, String)> {
        use rusqlite::OptionalExtension;
        let sha = format!("{:x}", sha2::Sha256::digest(token.as_bytes()));
        let store = self.store.lock().unwrap();
        store
            .conn
            .query_row("SELECT run_id, role FROM overseer_tokens WHERE sha=?1", [sha], |r| Ok((r.get(0)?, r.get(1)?)))
            .optional()?
            .ok_or_else(|| anyhow::anyhow!("unknown token"))
    }

    /// The tools a run has: its role's, and a watcher's while an agent the owner named watches.
    pub(crate) fn tools_of_run(&self, run_id: &str, role: &str) -> Vec<Value> {
        let mut tools = tool_list(role);
        if role == "agent" && self.is_watcher(run_id) {
            for t in tool_list("watcher") {
                if !tools.iter().any(|x| x["name"] == t["name"]) {
                    tools.push(t);
                }
            }
        }
        tools
    }

    pub fn overseer_tools(&self, token: &str) -> Result<Value> {
        let (run_id, role) = self.token_holder(token)?;
        Ok(json!({"tools": self.tools_of_run(&run_id, &role)}))
    }

    /// One tool call from a run. Every answer is bounded and redacted.
    pub fn overseer_tool(self: &std::sync::Arc<Self>, token: &str, name: &str, arguments: &Value) -> Result<Value> {
        let (run_id, role) = self.token_holder(token)?;
        if !self.tools_of_run(&run_id, &role).iter().any(|t| t["name"] == name) {
            bail!("{role} runs have no tool {name}");
        }
        // A watcher reads only its subject.
        if role != "overseer" && watch::SUBJECT_READS.contains(&name) {
            let subject = self.watch_of_watcher(&run_id).map(|w| w.subject);
            if subject.as_deref() != arguments["id"].as_str() {
                let why = format!("a watcher reads only its subject{}", subject.map(|s| format!(" ({s})")).unwrap_or_default());
                self.emit(None, Some(&run_id), "overseer_tool_call", "daemon", "exact", json!({"role": role, "name": name, "refused": why}))?;
                return Ok(json!({"text": format!("refused: {why}"), "is_error": true}));
            }
        }
        let text = match name {
            "roster" => self.roster_text()?,
            "agent" => {
                let id = arguments["id"].as_str().unwrap_or_default();
                self.digest_text(id)?
            }
            "check_in" => match self.record_check_in(arguments["agent"].as_str().unwrap_or(""), arguments["result"].as_str().unwrap_or(""), arguments["reason"].as_str().unwrap_or(""), arguments["left_out"].as_str().unwrap_or("")) {
                Ok(_) => "Recorded.".to_string(),
                Err(e) => return Ok(json!({"text": format!("refused: {e}"), "is_error": true})),
            },
            "report" => match self.channel_report(&run_id, arguments) {
                Ok(t) => t,
                Err(e) => return Ok(json!({"text": format!("refused: {e}"), "is_error": true})),
            },
            "ask" => match self.channel_ask(&run_id, arguments) {
                Ok(t) => t,
                Err(e) => return Ok(json!({"text": format!("refused: {e}"), "is_error": true})),
            },
            "claim" => match self.channel_claim(&run_id, arguments) {
                Ok(t) => t,
                Err(e) => return Ok(json!({"text": format!("refused: {e}"), "is_error": true})),
            },
            "finding" => match self.watch_finding(&run_id, arguments) {
                Ok(t) => t,
                Err(e) => return Ok(json!({"text": format!("refused: {e}"), "is_error": true})),
            },
            "rally" => {
                let agents = arguments["agents"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect::<Vec<_>>());
                serde_json::to_string_pretty(&self.rally(arguments["repo"].as_str(), agents)?)?
            }
            "answer" => match self.overseer_propose(&json!([{"action": "answer", "ask": arguments["ask"], "text": arguments["text"]}]), "tool") {
                Ok(r) => format!("{} (proposal {})", r["result"].as_str().unwrap_or(""), r["proposal"].as_str().unwrap_or("")),
                Err(e) => return Ok(json!({"text": format!("refused: {e}"), "is_error": true})),
            },
            "propose" => match self.overseer_propose(&arguments["actions"], "tool") {
                Ok(r) => format!("{} (proposal {})", r["result"].as_str().unwrap_or(""), r["proposal"].as_str().unwrap_or("")),
                Err(e) => {
                    self.emit(None, Some(&run_id), "overseer_tool_call", "daemon", "exact", json!({"role": role, "name": name, "refused": e.to_string()}))?;
                    return Ok(json!({"text": format!("refused: {e}"), "is_error": true}));
                }
            },
            "conversation" => self.conversation_text(arguments["id"].as_str().unwrap_or(""), arguments["after"].as_i64().unwrap_or(0), arguments["limit"].as_i64().unwrap_or(200))?,
            "changes" => {
                let run = self.run(arguments["id"].as_str().unwrap_or(""))?;
                let c = self.workspace_changes(&run.workspace_id)?;
                let names = c["names"].as_array().map(|a| a.iter().filter_map(|n| n.as_str()).collect::<Vec<_>>().join("\n")).unwrap_or_default();
                format!("{} files changed, +{} −{}\n{}{}", c["files"], c["added"], c["removed"], names, if c["files"].as_u64().unwrap_or(0) > 20 { "\n… (the first 20 names)" } else { "" })
            }
            "diff" => self.file_diff_text(arguments["id"].as_str().unwrap_or(""), arguments["path"].as_str().unwrap_or(""))?,
            "file" => self.file_text(arguments["id"].as_str().unwrap_or(""), arguments["path"].as_str().unwrap_or(""))?,
            "search" => {
                let query = arguments["query"].as_str().unwrap_or("");
                let found = self.search(query, 50)?;
                let ids = found["task_ids"].as_array().cloned().unwrap_or_default();
                let store = self.store.lock().unwrap();
                let runs = store.runs()?;
                let lines: Vec<String> = ids.iter().filter_map(|t| t.as_str()).filter_map(|t| runs.iter().find(|r| r.task_id == t && r.parent_run_id.is_none()).map(|r| format!("{} · {} · {}", r.id, crate::redact::redact(&r.title), r.status))).collect();
                if lines.is_empty() { "No agent matches.".to_string() } else { lines.join("\n") }
            }
            "usage" => {
                let d = self.digest(arguments["id"].as_str().unwrap_or(""))?;
                if d.usage.is_string() { d.usage.as_str().unwrap_or_default().to_string() } else { d.usage.to_string() }
            }
            "conflicts" => {
                let list = self.conflicts_list(None, false)?;
                let items = list["conflicts"].as_array().cloned().unwrap_or_default();
                if items.is_empty() {
                    "No open conflicts.".to_string()
                } else {
                    items.iter().map(|c| format!("{} · {} · {} with {} · {}", c["id"].as_str().unwrap_or(""), c["kind"].as_str().unwrap_or(""), c["title_a"].as_str().unwrap_or("?"), c["title_b"].as_str().or(c["target"].as_str()).unwrap_or("?"), c["paths"].as_array().map(|p| p.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(", ")).unwrap_or_default())).collect::<Vec<_>>().join("\n")
                }
            }
            _ => bail!("no tool {name}"),
        };
        self.emit(None, Some(&run_id), "overseer_tool_call", "daemon", "exact", json!({"role": role, "name": name, "bytes": text.len()}))?;
        Ok(json!({"text": bound(&crate::redact::redact(&text), 32 * 1024), "is_error": false}))
    }
}

pub(crate) fn bound(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n[cut at {max} bytes; ask for a smaller range]", &s[..end])
}

impl Daemon {
    /// A run's conversation as text: messages and tool steps, oldest first, from `after`.
    pub(crate) fn conversation_text(&self, run_id: &str, after: i64, limit: i64) -> Result<String> {
        let run = self.run(run_id)?;
        let events = self.store.lock().unwrap().events_after(after.max(0), Some(&run.id), limit.clamp(1, 1000))?;
        let mut lines = Vec::new();
        let mut last = after;
        for e in &events {
            last = e.seq;
            let p = &e.payload;
            let line = match e.kind.as_str() {
                "turn_started" => Some(format!("[{}] {}", if p["turn"]["prompt"].as_str().unwrap_or("").starts_with(session::FROM_OVERSEER) { "overseer" } else { "owner" }, p["turn"]["prompt"].as_str().unwrap_or("").chars().take(400).collect::<String>())),
                "output" => Some(format!("[{}] {}", p["role"].as_str().unwrap_or("agent"), p["text"].as_str().unwrap_or("").chars().take(600).collect::<String>())),
                "tool" => Some(format!("[tool] {} {}", p["name"].as_str().unwrap_or(""), p["summary"].as_str().unwrap_or("").chars().take(200).collect::<String>())),
                "file_activity" => Some(format!("[edit] {}", p["paths"].as_array().map(|a| a.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(", ")).unwrap_or_default())),
                "permission" => Some(format!("[waiting for permission] {}", p["tool"].as_str().unwrap_or(""))),
                "error" => Some(format!("[error {}] {}", p["class"].as_str().unwrap_or(""), p["message"].as_str().unwrap_or("").chars().take(300).collect::<String>())),
                "turn_done" => Some(format!("[turn {}]", if p["ok"] == true { "done" } else { "failed" })),
                _ => None,
            };
            if let Some(l) = line {
                lines.push(l.replace('\n', " "));
            }
        }
        if lines.is_empty() {
            return Ok(format!("Nothing after {after}."));
        }
        lines.push(format!("(last sequence {last}; call again with after={last} for more)"));
        Ok(lines.join("\n"))
    }

    /// A path inside a run's worktree: relative, no `..`, and not escaping through a symlink.
    fn inside_worktree(&self, run_id: &str, path: &str) -> Result<(std::path::PathBuf, std::path::PathBuf)> {
        let run = self.run(run_id)?;
        let ws = self.workspace(&run.workspace_id)?;
        let root = std::fs::canonicalize(&ws.path).map_err(|_| anyhow::anyhow!("the worktree is gone"))?;
        let rel = std::path::Path::new(path);
        if path.is_empty() || rel.is_absolute() || rel.components().any(|c| matches!(c, std::path::Component::ParentDir | std::path::Component::Prefix(_))) {
            bail!("{path:?} is not a path inside the worktree");
        }
        let full = root.join(rel);
        if full.exists() {
            let real = std::fs::canonicalize(&full)?;
            if !real.starts_with(&root) {
                bail!("{path:?} leaves the worktree");
            }
        }
        Ok((root, full))
    }

    fn file_text(&self, run_id: &str, path: &str) -> Result<String> {
        let (_, full) = self.inside_worktree(run_id, path)?;
        if !full.is_file() {
            bail!("{path:?} is not a file in the worktree");
        }
        let bytes = std::fs::read(&full)?;
        if bytes.len() > 4 * 1024 * 1024 {
            bail!("{path:?} is too large to read here ({} bytes)", bytes.len());
        }
        match String::from_utf8(bytes) {
            Ok(s) => Ok(s),
            Err(_) => bail!("{path:?} is not a text file"),
        }
    }

    fn file_diff_text(&self, run_id: &str, path: &str) -> Result<String> {
        let (root, _) = self.inside_worktree(run_id, path)?;
        let run = self.run(run_id)?;
        let task = self.task(&run.task_id)?;
        let base = {
            let store = self.store.lock().unwrap();
            task.start_snapshot.as_deref().and_then(|id| store.snapshot(id).ok().flatten()).map(|s| s.commit_sha)
        }
        .or_else(|| crate::git::head(&root))
        .ok_or_else(|| anyhow::anyhow!("no base to diff against"))?;
        let trees = crate::git::capture_trees(&root, &crate::paths::data_dir().join("tmp"))?;
        let diff = crate::git::git(&root, &["diff", "--no-color", &base, &trees.worktree_tree, "--", path])?;
        Ok(if diff.is_empty() { format!("{path} is unchanged against the task's base.") } else { diff })
    }
}
