//! One agent's conversation as compact display items, built from daemon events (the same
//! normalized events the VS Code conversation view reads). Items are keyed by event sequence
//! number, so history loaded later and live events merge without duplicates.

use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};

/// Items kept per agent (older ones are dropped; the daemon keeps the full history).
pub const MAX_ITEMS: usize = 4000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    /// The user's prompt for a turn (or a follow-up).
    User,
    /// Agent text (Markdown source).
    Agent,
    /// Reasoning or a plan: one dim line.
    Thinking,
    /// A tool call: `name target`, with its outcome once known.
    Tool { name: String, status: ToolStatus },
    /// Files the agent changed.
    Edit,
    /// Program output (generic harness stdout).
    Out,
    /// Program error output.
    ErrOut,
    /// A permission request (pending until answered).
    Permission { request_id: String, answer: Option<bool> },
    Error,
    /// A native child agent started (by title).
    Child,
    /// End of a turn (`ok`), with usage when reported.
    TurnDone { ok: bool },
    /// Status notes (interrupted, failed, reattached…).
    Note,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolStatus {
    Running,
    Done,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub seq: i64,
    pub kind: Kind,
    pub text: String,
    /// Set when the event came from a native child (its title), shown indented.
    pub child: Option<String>,
}

#[derive(Debug, Default, Clone)]
pub struct Feed {
    items: BTreeMap<i64, Item>,
    seen: HashSet<i64>,
    /// Tool id → sequence number of its item (results update it in place).
    tools: HashMap<String, i64>,
    /// Latest usage text for the current turn, attached to the next turn-done line.
    usage: Option<String>,
    /// Total tokens and cost reported across turns.
    pub tokens_in: u64,
    pub tokens_out: u64,
    pub cost_usd: f64,
    /// Whether the full history was loaded from the daemon.
    pub history_loaded: bool,
    /// Version counter: bumps on every change (drives redraws and caches).
    pub version: u64,
    /// Workspace path, to show paths relative to it.
    pub root: Option<String>,
    /// The task's repository (path, name): its paths in output show as `name/…`.
    pub repo: Option<(String, String)>,
}

impl Feed {
    pub fn items(&self) -> impl DoubleEndedIterator<Item = &Item> {
        self.items.values()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Adds one event. `child` is the native child's title when the event belongs to a child run.
    pub fn add(&mut self, ev: &Value, child: Option<&str>) -> bool {
        let seq = ev["seq"].as_i64().unwrap_or(0);
        if !self.seen.insert(seq) {
            return false;
        }
        let p = &ev["payload"];
        let child = child.map(str::to_string);
        let push = |this: &mut Feed, kind: Kind, text: String| {
            this.items.insert(seq, Item { seq, kind, text, child: child.clone() });
        };
        // Harness housekeeping lines that say nothing about the work.
        if ev["kind"] == "output" && matches!(p["text"].as_str().map(str::trim), Some("Reading additional input from stdin...")) {
            return self.bump();
        }
        match ev["kind"].as_str().unwrap_or_default() {
            "turn_started" => {
                if child.is_none() {
                    let prompt = p["turn"]["prompt"].as_str().unwrap_or_default().trim().to_string();
                    if !prompt.is_empty() {
                        push(self, Kind::User, prompt);
                    }
                }
            }
            "output" => {
                let text = p["text"].as_str().unwrap_or_default().trim_end().to_string();
                if text.trim().is_empty() {
                    return self.bump();
                }
                match p["role"].as_str().unwrap_or("assistant") {
                    "assistant" => push(self, Kind::Agent, text),
                    "reasoning" => push(self, Kind::Thinking, first_line(&text)),
                    "plan" => push(self, Kind::Thinking, format!("plan: {}", first_line(&text))),
                    "stdout" => push(self, Kind::Out, self.shorten(&text)),
                    "stderr" => push(self, Kind::ErrOut, self.shorten(&text)),
                    "user" => push(self, Kind::User, text),
                    // Harness notices ("session started (model …)") are noise in a tile.
                    "system" => return self.bump(),
                    other => push(self, Kind::Note, format!("{other}: {}", first_line(&text))),
                }
            }
            "tool" => {
                let name = p["name"].as_str().unwrap_or("tool").to_string();
                let summary = p["summary"].as_str().unwrap_or_default();
                let (target, status) = tool_target(&name, summary, self.root.as_deref());
                let key = p["id"].as_str().map(str::to_string);
                if let Some(existing) = key.as_ref().and_then(|k| self.tools.get(k)).copied() {
                    if let Some(item) = self.items.get_mut(&existing) {
                        item.text = target;
                        if let (Kind::Tool { status: s, .. }, Some(st)) = (&mut item.kind, status) {
                            *s = st;
                        }
                    }
                } else {
                    push(self, Kind::Tool { name: display_tool(&name), status: status.unwrap_or(ToolStatus::Running) }, target);
                    if let Some(k) = key {
                        self.tools.insert(k, seq);
                    }
                }
            }
            "tool_result" => {
                let id = p["id"].as_str().unwrap_or_default();
                let failed = p["is_error"].as_bool().unwrap_or(false) || matches!(p["status"].as_str(), Some("failed" | "error" | "declined"));
                let done = failed || matches!(p["status"].as_str(), Some("completed" | "success"));
                if let Some(item) = self.tools.get(id).and_then(|s| self.items.get_mut(s)) {
                    if let Kind::Tool { status, .. } = &mut item.kind {
                        if failed {
                            *status = ToolStatus::Failed;
                        } else if done {
                            *status = ToolStatus::Done;
                        }
                    }
                }
            }
            "file_activity" => {
                let paths: Vec<String> = p["paths"].as_array().map(|a| a.iter().filter_map(|x| x.as_str()).map(|x| short_path(x, self.root.as_deref())).collect()).unwrap_or_default();
                if !paths.is_empty() {
                    push(self, Kind::Edit, paths.join(", "));
                }
            }
            "permission" => {
                let tool = p["tool"].as_str().unwrap_or("a tool");
                let request_id = p["request_id"].as_str().map(str::to_string).unwrap_or_else(|| p["request_id"].to_string());
                push(self, Kind::Permission { request_id, answer: None }, permission_summary(tool, &p["input"], self.root.as_deref()));
            }
            "permission_answered" => {
                let id = p["request_id"].as_str().map(str::to_string).unwrap_or_else(|| p["request_id"].to_string());
                let allow = p["allow"].as_bool().unwrap_or(false);
                for item in self.items.values_mut() {
                    if let Kind::Permission { request_id, answer } = &mut item.kind {
                        if *request_id == id {
                            *answer = Some(allow);
                        }
                    }
                }
            }
            "error" => {
                let class = p["class"].as_str().unwrap_or("error");
                let msg = p["message"].as_str().unwrap_or_default();
                push(self, Kind::Error, format!("{}: {}", class.replace('_', " "), first_line(msg)));
            }
            "status" => {
                let st = p["status"].as_str().unwrap_or_default();
                if matches!(st, "interrupted" | "failed" | "disconnected") && child.is_none() {
                    let reason = p["reason"].as_str().map(|r| format!(": {}", first_line(r))).unwrap_or_default();
                    push(self, Kind::Note, format!("{}{reason}", st.replace('_', " ")));
                }
            }
            "child" => {
                let title = p["child"]["title"].as_str().unwrap_or("sub-agent").to_string();
                push(self, Kind::Child, title);
            }
            "turn_done" => {
                if child.is_none() {
                    let ok = p["ok"].as_bool().unwrap_or(false);
                    let mut text = if ok { "done".to_string() } else { format!("failed{}", p["summary"].as_str().map(|s| format!(": {}", first_line(s))).unwrap_or_default()) };
                    if let Some(u) = self.usage.take() {
                        text.push_str(&format!(" · {u}"));
                    }
                    push(self, Kind::TurnDone { ok }, text);
                }
            }
            "usage" => {
                if child.is_none() {
                    if let Some(u) = self.read_usage(p) {
                        self.usage = Some(u);
                    }
                }
                return self.bump();
            }
            "reattached" => push(self, Kind::Note, "daemon restarted; reattached".into()),
            "retention" => push(self, Kind::Note, "older history truncated".into()),
            _ => return self.bump(),
        }
        while self.items.len() > MAX_ITEMS {
            let first = *self.items.keys().next().unwrap();
            self.items.remove(&first);
        }
        self.bump()
    }

    /// Display-only: the workspace path becomes `.`, the repository `name`, home `~`.
    pub fn shorten(&self, text: &str) -> String {
        let mut out = text.to_string();
        if let Some(r) = self.root.as_deref().filter(|r| r.len() > 1) {
            out = out.replace(&format!("{r}/"), "./").replace(r, ".");
        }
        if let Some((path, name)) = &self.repo {
            if path.len() > 1 {
                out = out.replace(path.as_str(), name);
            }
        }
        if let Some(home) = std::env::var_os("HOME").map(|h| h.to_string_lossy().to_string()).filter(|h| h.len() > 1) {
            out = out.replace(&home, "~");
        }
        out
    }

    fn bump(&mut self) -> bool {
        self.version += 1;
        true
    }

    /// Pending permission (request id, summary) if one is shown and unanswered.
    pub fn pending_permission(&self) -> Option<(&str, &str)> {
        self.items.values().rev().find_map(|i| match &i.kind {
            Kind::Permission { request_id, answer: None } => Some((request_id.as_str(), i.text.as_str())),
            _ => None,
        })
    }

    fn read_usage(&mut self, p: &Value) -> Option<String> {
        let u = if p["usage"].is_object() { &p["usage"] } else if p["total"].is_object() { &p["total"] } else if p["tokens"].is_object() { &p["tokens"] } else { p };
        let pick = |keys: &[&str]| keys.iter().find_map(|k| u[*k].as_u64());
        let input = pick(&["input_tokens", "inputTokens", "input"]);
        let output = pick(&["output_tokens", "outputTokens", "output"]);
        let cost = p["total_cost_usd"].as_f64().or(p["cost"].as_f64());
        if input.is_none() && output.is_none() && cost.is_none() {
            return None;
        }
        self.tokens_in += input.unwrap_or(0);
        self.tokens_out += output.unwrap_or(0);
        self.cost_usd += cost.unwrap_or(0.0);
        let mut parts = Vec::new();
        if let Some(i) = input {
            parts.push(format!("{} in", compact(i)));
        }
        if let Some(o) = output {
            parts.push(format!("{} out", compact(o)));
        }
        if let Some(c) = cost {
            parts.push(format!("${c:.2}"));
        }
        Some(parts.join(" / "))
    }
}

/// 18423 → "18.4k".
pub fn compact(n: u64) -> String {
    match n {
        0..=999 => n.to_string(),
        1_000..=999_999 => format!("{:.1}k", n as f64 / 1000.0).replace(".0k", "k"),
        _ => format!("{:.1}M", n as f64 / 1_000_000.0).replace(".0M", "M"),
    }
}

fn first_line(s: &str) -> String {
    s.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or_default().to_string()
}

fn display_tool(name: &str) -> String {
    match name {
        "shell" | "command_execution" | "commandExecution" => "Run".into(),
        "apply_patch" => "Patch".into(),
        n if n.starts_with("collab:") => "Agent".into(),
        n => n.to_string(),
    }
}

/// A tool call's readable target and, when the summary reports it, its outcome.
/// Claude summaries are the tool input as JSON; Codex ones are `command [status, exit N]`.
pub fn tool_target(name: &str, summary: &str, root: Option<&str>) -> (String, Option<ToolStatus>) {
    let mut status = None;
    let mut body = summary.trim().to_string();
    if let Some(open) = body.rfind(" [") {
        if body.ends_with(']') {
            let tag = body[open + 2..body.len() - 1].to_lowercase();
            status = if tag.starts_with("completed") && !tag.contains("exit 1") && !tag.contains("exit 2") {
                Some(ToolStatus::Done)
            } else if tag.starts_with("failed") || tag.starts_with("declined") || tag.contains(", exit") && !tag.contains("exit 0") {
                Some(ToolStatus::Failed)
            } else if tag.starts_with("inprogress") || tag.starts_with("in_progress") || tag.starts_with("running") {
                Some(ToolStatus::Running)
            } else {
                None
            };
            body.truncate(open);
        }
    }
    if body.starts_with('{') {
        if let Ok(v) = serde_json::from_str::<Value>(&body) {
            for key in ["file_path", "notebook_path", "command", "pattern", "path", "url", "query", "description", "prompt"] {
                if let Some(s) = v[key].as_str() {
                    let s = if key.ends_with("path") { short_path(s, root) } else { first_line(s) };
                    let extra = if key == "pattern" { v["path"].as_str().map(|p| format!(" in {}", short_path(p, root))).unwrap_or_default() } else { String::new() };
                    return (format!("{s}{extra}"), status);
                }
            }
            return (String::new(), status);
        }
    }
    let _ = name;
    (first_line(&body), status)
}

fn permission_summary(tool: &str, input: &Value, root: Option<&str>) -> String {
    let (target, _) = tool_target(tool, &input.to_string(), root);
    let target = if target.is_empty() {
        input["command"].as_array().map(|a| a.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(" ")).or_else(|| input["command"].as_str().map(str::to_string)).unwrap_or_default()
    } else {
        target
    };
    if target.is_empty() {
        tool.to_string()
    } else {
        format!("{tool} {target}")
    }
}

/// Paths relative to the agent's workspace, else with the home directory as `~`.
pub fn short_path(path: &str, root: Option<&str>) -> String {
    if let Some(r) = root {
        if let Some(rest) = path.strip_prefix(r) {
            let rest = rest.trim_start_matches('/');
            if !rest.is_empty() {
                return rest.to_string();
            }
        }
    }
    if let Some(home) = std::env::var_os("HOME") {
        let home = home.to_string_lossy();
        if let Some(rest) = path.strip_prefix(home.as_ref()) {
            return format!("~{rest}");
        }
    }
    path.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ev(seq: i64, kind: &str, payload: Value) -> Value {
        json!({ "seq": seq, "kind": kind, "payload": payload, "run_id": "r1" })
    }

    #[test]
    fn claude_tool_input_becomes_a_short_target() {
        let (t, s) = tool_target("Edit", r#"{"file_path":"/w/src/a.ts","old_string":"x"}"#, Some("/w"));
        assert_eq!((t.as_str(), s), ("src/a.ts", None));
        let (t, _) = tool_target("Grep", r#"{"pattern":"refresh","path":"/w/src"}"#, Some("/w"));
        assert_eq!(t, "refresh in src");
        let (t, _) = tool_target("Bash", r#"{"command":"npm test\n","description":"x"}"#, None);
        assert_eq!(t, "npm test");
    }

    #[test]
    fn codex_command_summary_carries_its_status() {
        assert_eq!(tool_target("shell", "ls -la [completed, exit 0]", None), ("ls -la".into(), Some(ToolStatus::Done)));
        assert_eq!(tool_target("shell", "false [completed, exit 1]", None).1, Some(ToolStatus::Failed));
        assert_eq!(tool_target("shell", "sleep 5 [inProgress]", None).1, Some(ToolStatus::Running));
    }

    #[test]
    fn events_merge_without_duplicates_and_tools_update_in_place() {
        let mut f = Feed::default();
        f.add(&ev(1, "turn_started", json!({"turn": {"n": 1, "prompt": "fix it"}})), None);
        f.add(&ev(2, "tool", json!({"id": "t1", "name": "Read", "summary": "{\"file_path\":\"/x/a.rs\"}"})), None);
        f.add(&ev(3, "tool_result", json!({"id": "t1", "status": "completed"})), None);
        f.add(&ev(2, "tool", json!({"id": "t1", "name": "Read", "summary": "dup"})), None);
        f.add(&ev(4, "usage", json!({"usage": {"input_tokens": 18423, "output_tokens": 1204}, "total_cost_usd": 0.0412})), None);
        f.add(&ev(5, "turn_done", json!({"ok": true})), None);
        let items: Vec<_> = f.items().cloned().collect();
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].kind, Kind::User);
        assert_eq!(items[1].kind, Kind::Tool { name: "Read".into(), status: ToolStatus::Done });
        assert_eq!(items[2].text, "done · 18.4k in / 1.2k out / $0.04");
    }

    #[test]
    fn permissions_are_pending_until_answered() {
        let mut f = Feed::default();
        f.add(&ev(1, "permission", json!({"request_id": "req-1", "tool": "Write", "input": {"file_path": "/w/CHANGELOG.md"}})), None);
        assert_eq!(f.pending_permission(), Some(("req-1", "Write /w/CHANGELOG.md")));
        f.add(&ev(2, "permission_answered", json!({"request_id": "req-1", "allow": true})), None);
        assert_eq!(f.pending_permission(), None);
    }
}
