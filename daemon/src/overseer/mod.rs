//! Overseer itself (Gate S): the orchestrator that knows every agent the daemon runs.
//! This module holds what the daemon coordinates with no model (digests, tokens, tools for the
//! agents) and the conversation with Overseer. The model never touches an agent, a worktree or a
//! shell directly: it reads through `overseer.tool` and asks the daemon to act.

pub mod mcp;

use crate::daemon::Daemon;
use anyhow::{bail, Result};
use serde_json::{json, Value};
use sha2::Digest;

/// Tools a run may call, by its role. A token names the run and its role; the text of a call
/// never decides who is speaking.
fn tool_list(role: &str) -> Vec<Value> {
    let mut tools = vec![json!({
        "name": "roster",
        "description": "Every agent Overseer runs, one line each: id, title, status, harness, repository, worktree, files changed.",
        "inputSchema": {"type": "object", "properties": {}, "additionalProperties": false}
    })];
    if role == "overseer" {
        tools.push(json!({
            "name": "agent",
            "description": "One agent's digest by id: what was asked, status, changed files, last messages, what it waits for.",
            "inputSchema": {"type": "object", "properties": {"id": {"type": "string"}}, "required": ["id"], "additionalProperties": false}
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

    pub fn overseer_tools(&self, token: &str) -> Result<Value> {
        let (_, role) = self.token_holder(token)?;
        Ok(json!({"tools": tool_list(&role)}))
    }

    /// One tool call from a run. Every answer is bounded and redacted.
    pub fn overseer_tool(&self, token: &str, name: &str, arguments: &Value) -> Result<Value> {
        let (run_id, role) = self.token_holder(token)?;
        if !tool_list(&role).iter().any(|t| t["name"] == name) {
            bail!("{role} runs have no tool {name}");
        }
        let text = match name {
            "roster" => self.roster_text()?,
            "agent" => {
                let id = arguments["id"].as_str().unwrap_or_default();
                self.digest_text(id)?
            }
            _ => bail!("no tool {name}"),
        };
        self.emit(None, Some(&run_id), "overseer_tool_call", "daemon", "exact", json!({"role": role, "name": name, "bytes": text.len()}))?;
        Ok(json!({"text": bound(&crate::redact::redact(&text), 32 * 1024), "is_error": false}))
    }

    fn roster_text(&self) -> Result<String> {
        let state = self.state()?;
        let tasks = state["tasks"].as_array().cloned().unwrap_or_default();
        let workspaces = state["workspaces"].as_array().cloned().unwrap_or_default();
        let mut lines = Vec::new();
        for r in state["runs"].as_array().cloned().unwrap_or_default().iter().filter(|r| r["parent_run_id"].is_null()) {
            let task = tasks.iter().find(|t| t["id"] == r["task_id"]);
            let ws = workspaces.iter().find(|w| w["id"] == r["workspace_id"]);
            let repo = task.and_then(|t| t["repo_root"].as_str()).map(|p| p.rsplit('/').next().unwrap_or(p).to_string()).unwrap_or_default();
            let changed = ws.and_then(|w| w["id"].as_str()).and_then(|id| self.workspace_changes(id).ok()).and_then(|c| c["changes"].as_array().map(Vec::len)).unwrap_or(0);
            lines.push(format!("{} · {} · {} · {} · {} · {} · {} files changed", r["id"].as_str().unwrap_or_default(), r["title"].as_str().unwrap_or_default(), r["status"].as_str().unwrap_or_default(), r["harness"].as_str().unwrap_or_default(), repo, ws.and_then(|w| w["path"].as_str()).unwrap_or_default(), changed));
        }
        if lines.is_empty() {
            return Ok("No agents.".into());
        }
        Ok(lines.join("\n"))
    }

    fn digest_text(&self, id: &str) -> Result<String> {
        let run = self.run(id)?;
        let task = self.task(&run.task_id)?;
        let turns = self.store.lock().unwrap().turns(id)?;
        let asked: Vec<String> = turns.iter().map(|t| t.prompt.chars().take(400).collect()).collect();
        let changes = self.workspace_changes(&run.workspace_id).ok().and_then(|c| c["changes"].as_array().cloned()).unwrap_or_default();
        let files: Vec<String> = changes.iter().take(50).map(|c| format!("{} {}", c["status"].as_str().unwrap_or("?"), c["path"].as_str().unwrap_or(""))).collect();
        Ok(format!(
            "id: {}\ntitle: {}\nstatus: {}\nharness: {}\nrepository: {}\nasked:\n{}\nchanged files ({}):\n{}",
            run.id, run.title, run.status, run.harness, task.repo_root, asked.iter().map(|a| format!("- {a}")).collect::<Vec<_>>().join("\n"), changes.len(), files.join("\n")
        ))
    }
}

fn bound(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n[cut at {max} bytes; ask for a smaller range]", &s[..end])
}
