//! Overseer itself (Gate S): the orchestrator that knows every agent the daemon runs.
//! This module holds what the daemon coordinates with no model (digests, tokens, tools for the
//! agents) and the conversation with Overseer. The model never touches an agent, a worktree or a
//! shell directly: it reads through `overseer.tool` and asks the daemon to act.

pub mod conflicts;
pub mod digest;
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
        tools.push(json!({
            "name": "conflicts",
            "description": "Open conflicts between agents in flight: same lines, same file, area crossed, target moved; each with the agents and the files.",
            "inputSchema": {"type": "object", "properties": {}, "additionalProperties": false}
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
