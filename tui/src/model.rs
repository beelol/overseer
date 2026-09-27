//! The daemon's `state` as typed records (only the fields the TUI shows).

use serde::Deserialize;
use serde_json::Value;

pub const ACTIVE: [&str; 4] = ["queued", "starting", "running", "waiting_for_user"];

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
}

impl State {
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
