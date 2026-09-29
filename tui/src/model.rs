//! The daemon's `state` as typed records (only the fields the TUI shows).

use serde::Deserialize;
use serde_json::Value;

// Continuity (Gate L): an agent that waits for its connection or for memory still holds its work.
pub const ACTIVE: [&str; 6] = ["queued", "starting", "running", "waiting_for_user", "waiting_for_connection", "waiting_for_memory"];

#[derive(Debug, Clone, Deserialize, Default)]
pub struct Run {
    pub id: String,
    pub task_id: String,
    pub parent_run_id: Option<String>,
    pub harness: String,
    pub profile_id: Option<String>,
    pub model: Option<String>,
    pub workspace_id: String,
    pub status: String,
    pub exit_reason: Option<String>,
    pub created_ms: i64,
    pub ended_ms: Option<i64>,
    pub title: String,
    #[serde(default)]
    pub capabilities: Value,
    #[serde(default)]
    pub attention: Option<Value>,
}

impl Run {
    pub fn active(&self) -> bool {
        ACTIVE.contains(&self.status.as_str())
    }

    /// Waiting on the user (a permission request or another question).
    pub fn needs_you(&self) -> bool {
        self.status == "waiting_for_user" || self.attention.as_ref().is_some_and(|a| a["kind"] == "permission")
    }

    /// The pending permission request id reported by the daemon, if any.
    pub fn permission_request(&self) -> Option<String> {
        let a = self.attention.as_ref()?;
        if a["kind"] != "permission" {
            return None;
        }
        a["request_id"].as_str().map(str::to_string).or_else(|| (!a["request_id"].is_null()).then(|| a["request_id"].to_string()))
    }

    pub fn capability(&self, key: &str) -> &str {
        self.capabilities[key].as_str().unwrap_or("")
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct Task {
    pub id: String,
    pub repo_root: String,
    pub workspace_id: String,
    pub title: String,
    #[serde(default)]
    pub prompt: String,
    #[serde(default)]
    pub created_ms: i64,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct Workspace {
    pub id: String,
    pub path: String,
    pub kind: String,
    pub branch: Option<String>,
    #[serde(default)]
    pub removed_ms: Option<i64>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub harness: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct State {
    #[serde(default)]
    pub cursor: i64,
    #[serde(default)]
    pub tasks: Vec<Task>,
    #[serde(default)]
    pub runs: Vec<Run>,
    #[serde(default)]
    pub workspaces: Vec<Workspace>,
    #[serde(default)]
    pub profiles: Vec<Profile>,
    /// Held, watched, watching, in conflict, per top-level run (Gate S, AC-199).
    #[serde(default)]
    pub oversight: Value,
    /// Overseer's own summary: the level, what waits for the owner.
    #[serde(default)]
    pub overseer: Value,
    /// What each workspace's work became (AC-243): merged, stopped on conflicts, or a pull request.
    #[serde(default)]
    pub landings: Value,
}

impl State {
    /// The oversight marks of a run, as short words: held, watched, watching, N conflicts.
    pub fn marks(&self, run_id: &str) -> Vec<String> {
        let mut out = Vec::new();
        // Shown once the owner has spoken to Overseer (its run exists), like the other surfaces.
        if self.overseer["run_id"].is_null() {
            return out;
        }
        let o = &self.oversight[run_id];
        if o["held"] == true {
            out.push("⏸ held".to_string());
        }
        if o["watched"] == true {
            out.push("◉ watched".to_string());
        }
        if o["watching"].as_array().map(|w| !w.is_empty()).unwrap_or(false) {
            out.push("◉ watching".to_string());
        }
        if let Some(n) = o["conflicts"].as_i64().filter(|n| *n > 0) {
            out.push(format!("⚠ {n} conflict{}", if n == 1 { "" } else { "s" }));
        }
        out
    }
}

impl State {
    /// What an agent's work became, in the words every surface uses (extension/media/landing-text.js):
    /// "Merged into main (1a2b3c4)", "Merge stopped: conflicts in a.txt", "Pull request open".
    pub fn landing_text(&self, workspace_id: &str) -> Option<String> {
        let l = &self.landings[workspace_id];
        match l["state"].as_str()? {
            "merged" => {
                let commit: String = l["commit"].as_str().unwrap_or_default().chars().take(7).collect();
                Some(format!("Merged into {}{}", l["target"].as_str().unwrap_or("main"), if commit.is_empty() { String::new() } else { format!(" ({commit})") }))
            }
            "conflicts" => {
                let files: Vec<&str> = l["files"].as_array().map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
                Some(if files.is_empty() { "Merge stopped: conflicts".to_string() } else { format!("Merge stopped: conflicts in {}", files.join(", ")) })
            }
            "pr" => Some(match l["url"].as_str().and_then(|u| u.rsplit('/').next()).filter(|n| n.chars().all(|c| c.is_ascii_digit()) && !n.is_empty()) {
                Some(n) => format!("Pull request #{n} open"),
                None => "Pull request open".to_string(),
            }),
            _ => None,
        }
    }

    pub fn run(&self, id: &str) -> Option<&Run> {
        self.runs.iter().find(|r| r.id == id)
    }
    pub fn task(&self, id: &str) -> Option<&Task> {
        self.tasks.iter().find(|t| t.id == id)
    }
    pub fn workspace(&self, id: &str) -> Option<&Workspace> {
        self.workspaces.iter().find(|w| w.id == id)
    }
    pub fn profile(&self, id: &str) -> Option<&Profile> {
        self.profiles.iter().find(|p| p.id == id)
    }

    /// The top-level run a run belongs to.
    pub fn root_of(&self, id: &str) -> String {
        let mut cur = id.to_string();
        for _ in 0..64 {
            match self.run(&cur).and_then(|r| r.parent_run_id.clone()) {
                Some(p) => cur = p,
                None => break,
            }
        }
        cur
    }

    pub fn descendants(&self, id: &str) -> Vec<&Run> {
        let mut out = Vec::new();
        let mut queue = vec![id.to_string()];
        while let Some(cur) = queue.pop() {
            for r in self.runs.iter().filter(|r| r.parent_run_id.as_deref() == Some(cur.as_str())) {
                if out.iter().any(|o: &&Run| o.id == r.id) {
                    continue;
                }
                queue.push(r.id.clone());
                out.push(r);
            }
        }
        out
    }

    /// Top-level agents, newest first (a stable order: tiles do not jump when statuses change).
    pub fn agents(&self) -> Vec<&Run> {
        let mut roots: Vec<&Run> = self.runs.iter().filter(|r| r.parent_run_id.is_none()).collect();
        roots.sort_by(|a, b| b.created_ms.cmp(&a.created_ms).then_with(|| b.id.cmp(&a.id)));
        roots
    }
}

#[cfg(test)]
mod landing {
    use super::*;
    use serde_json::json;

    /// AC-243: the words for what an agent's work became are the extension's.
    #[test]
    fn merged_conflicts_and_pull_request_read_as_everywhere() {
        let state: State = serde_json::from_value(json!({ "landings": {
            "w1": { "state": "merged", "target": "main", "commit": "1a2b3c4d5e6f" },
            "w2": { "state": "conflicts", "files": ["a.txt", "b.txt"] },
            "w3": { "state": "pr", "url": "https://github.com/o/r/pull/7" }
        } })).unwrap();
        assert_eq!(state.landing_text("w1").as_deref(), Some("Merged into main (1a2b3c4)"));
        assert_eq!(state.landing_text("w2").as_deref(), Some("Merge stopped: conflicts in a.txt, b.txt"));
        assert_eq!(state.landing_text("w3").as_deref(), Some("Pull request #7 open"));
        assert_eq!(state.landing_text("w4"), None);
    }
}
