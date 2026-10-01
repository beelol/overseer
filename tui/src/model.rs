//! The daemon's `state` as typed records (only the fields the TUI shows).

use serde::Deserialize;
use serde_json::Value;

// Continuity (Gate L): an agent that waits for its connection or for memory still holds its work.
pub const ACTIVE: [&str; 6] = ["queued", "starting", "running", "waiting_for_user", "waiting_for_connection", "waiting_for_memory"];
/// The rollup's states (extension/media/rollup.js): at work, done, failed.
const WORKING: [&str; 5] = ["queued", "starting", "running", "waiting_for_connection", "waiting_for_memory"];
pub const DONE: [&str; 2] = ["completed", "interrupted"];
pub const FAILED: [&str; 2] = ["failed", "disconnected"];
/// A finished agent stays "to review" for a week at most, as in VS Code and the menu bar.
pub const WEEK_MS: i64 = 7 * 86_400_000;

/// The rollup by state (T-26): the same five counts as VS Code's side bar and grid.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counts {
    pub working: usize,
    pub needs: usize,
    pub to_review: usize,
    pub reviewed: usize,
    pub failed: usize,
}

impl Counts {
    /// Only the states that have agents: "2 working · 1 needs you · 6 to review · 3 reviewed · 1 failed".
    pub fn parts(&self) -> Vec<(&'static str, usize, String)> {
        [("working", self.working, "working"), ("needs", self.needs, "needs you"), ("to_review", self.to_review, "to review"), ("reviewed", self.reviewed, "reviewed"), ("failed", self.failed, "failed")]
            .into_iter()
            .filter(|(_, n, _)| *n > 0)
            .map(|(k, n, w)| (k, n, format!("{n} {w}")))
            .collect()
    }
}

/// Milliseconds since the epoch, for the week the "to review" mark lasts.
pub fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

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
    /// A swarm worker or director: not one of the agents the rollup counts.
    #[serde(default)]
    pub swarm_membership: Option<Value>,
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
    #[serde(default)]
    pub archived_ms: Option<i64>,
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
    #[serde(default)]
    pub is_system: bool,
    /// How the daemon names the account (AC-235): provider and plan, the shortened email, whose login.
    #[serde(default)]
    pub account: Option<Account>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct Account {
    #[serde(default)]
    pub plan: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub short: String,
}

impl Profile {
    /// The account in full: "Claude Max · bil…@testbox.com · Mac's default login".
    pub fn label(&self) -> String {
        self.account.as_ref().map(|a| a.label.clone()).filter(|l| !l.is_empty()).unwrap_or_else(|| self.own_name())
    }
    /// Where room is tight: "Claude Max · bil…@testbox.com".
    pub fn short(&self) -> String {
        self.account.as_ref().map(|a| a.short.clone()).filter(|l| !l.is_empty()).unwrap_or_else(|| self.own_name())
    }
    fn own_name(&self) -> String {
        if self.is_system { crate::words::DEFAULT_LOGIN.to_string() } else { crate::words::account(&self.name) }
    }
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
    /// When each agent's review was last opened (or its work merged), kept by the daemon for
    /// every surface (`review.seen`, T-26).
    #[serde(default)]
    pub reviewed: std::collections::HashMap<String, i64>,
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

    /// The agents the rollup counts (extension/media/rollup.js `agents`): each non-archived task's
    /// newest top-level run, without Overseer's own run or swarm members.
    fn rollup_agents(&self) -> Vec<&Run> {
        let mut newest: std::collections::HashMap<&str, &Run> = std::collections::HashMap::new();
        for r in self.runs.iter().filter(|r| r.parent_run_id.is_none() && r.swarm_membership.is_none()) {
            if self.task(&r.task_id).is_some_and(|t| t.archived_ms.is_some()) || self.oversight[r.id.as_str()]["role"] == "overseer" {
                continue;
            }
            let keep = newest.get(r.task_id.as_str()).is_none_or(|cur| r.created_ms > cur.created_ms);
            if keep {
                newest.insert(r.task_id.as_str(), r);
            }
        }
        newest.into_values().collect()
    }

    /// Needs you, counted as the extension and the phone count it (AC-246, extension/media/rollup.js):
    /// each non-archived task's newest top-level run that waits on a permission or a question, and
    /// one more while Overseer has proposals or conflicts waiting for a decision.
    pub fn needs_you_count(&self) -> usize {
        let waiting = self.rollup_agents().iter().filter(|r| r.status == "waiting_for_user").count();
        let o = &self.overseer;
        let decide = !o["run_id"].is_null() && (o["open_proposals"].as_i64().unwrap_or(0) > 0 || o["conflicts_needing_decision"].as_i64().unwrap_or(0) > 0);
        waiting + usize::from(decide)
    }

    /// Whether an agent at its end still waits to be reviewed (T-26, rollup.js `unreviewed`): done,
    /// stopped or failed in the last week, with no review opened (nor merge) since it ended.
    pub fn unreviewed(&self, run: &Run, now: i64) -> bool {
        if run.parent_run_id.is_some() || !(DONE.contains(&run.status.as_str()) || FAILED.contains(&run.status.as_str())) {
            return false;
        }
        let ended = run.ended_ms.unwrap_or(run.created_ms);
        if now - ended > WEEK_MS {
            return false;
        }
        self.reviewed.get(&run.id).copied().unwrap_or(0) < ended
    }

    /// The rollup by state (T-26, rollup.js `counts`): a failed agent counts as failed until its
    /// review is opened, then as reviewed.
    pub fn counts(&self, now: i64) -> Counts {
        let mut c = Counts { needs: self.needs_you_count(), ..Default::default() };
        for r in self.rollup_agents() {
            if r.status == "waiting_for_user" {
                continue;
            }
            if WORKING.contains(&r.status.as_str()) {
                c.working += 1;
            } else if self.unreviewed(r, now) {
                if FAILED.contains(&r.status.as_str()) { c.failed += 1 } else { c.to_review += 1 }
            } else if DONE.contains(&r.status.as_str()) || FAILED.contains(&r.status.as_str()) {
                c.reviewed += 1;
            }
        }
        c
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

#[cfg(test)]
mod needs_you {
    use super::*;
    use serde_json::json;

    /// AC-246: the header's Needs-you count is the extension's (extension/media/rollup.js): waiting
    /// agents of tasks that are not archived, each task's newest run, plus Overseer's decisions.
    #[test]
    fn counts_as_the_extension_does() {
        let state: State = serde_json::from_value(json!({
            "tasks": [
                { "id": "t1", "repo_root": "/r", "workspace_id": "w", "title": "a" },
                { "id": "t2", "repo_root": "/r", "workspace_id": "w", "title": "b" },
                { "id": "t3", "repo_root": "/r", "workspace_id": "w", "title": "c", "archived_ms": 5 },
                { "id": "t4", "repo_root": "/r", "workspace_id": "w", "title": "d" }
            ],
            "runs": [
                { "id": "r1", "task_id": "t1", "harness": "claude", "workspace_id": "w", "status": "waiting_for_user", "created_ms": 1, "title": "a", "attention": { "kind": "permission" } },
                { "id": "r2", "task_id": "t2", "harness": "claude", "workspace_id": "w", "status": "failed", "created_ms": 1, "title": "b" },
                { "id": "r3", "task_id": "t3", "harness": "claude", "workspace_id": "w", "status": "waiting_for_user", "created_ms": 1, "title": "c" },
                { "id": "r4-old", "task_id": "t4", "harness": "claude", "workspace_id": "w", "status": "waiting_for_user", "created_ms": 1, "title": "d" },
                { "id": "r4", "task_id": "t4", "harness": "claude", "workspace_id": "w", "status": "completed", "created_ms": 2, "title": "d" }
            ],
            "overseer": { "run_id": "ov", "open_proposals": 1 }
        }))
        .unwrap();
        assert_eq!(state.needs_you_count(), 2, "the waiting agent and Overseer's proposal; not the failed, archived or superseded ones");
    }

    /// The same recorded state the extension's and the phone's tests read gives the same count.
    #[test]
    fn the_nine_agents_recording_counts_as_everywhere() {
        let recorded: serde_json::Value = serde_json::from_str(include_str!("../../phone/model/test/fixtures/nine-agents.json")).unwrap();
        let mut state: State = serde_json::from_value(recorded["final"].clone()).unwrap();
        assert_eq!(state.needs_you_count(), 1);
        state.overseer = json!({ "run_id": "ov", "open_proposals": 2 });
        assert_eq!(state.needs_you_count(), 2);
    }
}

#[cfg(test)]
mod rollup {
    use super::*;
    use serde_json::json;

    /// T-26: the five counts are the extension's (extension/media/rollup.js `counts`): working,
    /// needs you, to review, reviewed, failed; a reviewed mark older than the run's end does not count,
    /// a failed agent once reviewed is reviewed, and an agent older than a week is not to review.
    #[test]
    fn the_five_counts_are_the_extensions() {
        let now = 30 * 86_400_000;
        let run = |id: &str, task: &str, status: &str, ended: i64| json!({ "id": id, "task_id": task, "harness": "claude", "workspace_id": "w", "status": status, "created_ms": 1, "ended_ms": ended, "title": id });
        let state: State = serde_json::from_value(json!({
            "tasks": (1..=9).map(|n| json!({ "id": format!("t{n}"), "repo_root": "/r", "workspace_id": "w", "title": "x" })).collect::<Vec<_>>(),
            "runs": [
                run("work", "t1", "running", 0),
                run("wait", "t2", "waiting_for_user", 0),
                run("done", "t3", "completed", now - 1000),
                run("seen", "t4", "completed", now - 1000),
                run("stale-mark", "t5", "interrupted", now - 1000),
                run("broke", "t6", "failed", now - 1000),
                run("broke-seen", "t7", "disconnected", now - 1000),
                run("old", "t8", "completed", now - 8 * 86_400_000),
                { "id": "sub", "task_id": "t9", "parent_run_id": "done", "harness": "claude", "workspace_id": "w", "status": "completed", "created_ms": 1, "ended_ms": now - 5, "title": "sub" }
            ],
            "reviewed": { "seen": now - 500, "stale-mark": now - 5000, "broke-seen": now - 1 }
        }))
        .unwrap();
        let c = state.counts(now);
        assert_eq!(c, Counts { working: 1, needs: 1, to_review: 2, reviewed: 3, failed: 1 });
        let words: Vec<String> = c.parts().into_iter().map(|p| p.2).collect();
        assert_eq!(words.join(" · "), "1 working · 1 needs you · 2 to review · 3 reviewed · 1 failed");
        assert!(state.unreviewed(state.run("done").unwrap(), now) && !state.unreviewed(state.run("seen").unwrap(), now));
        assert_eq!(Counts { working: 2, ..Default::default() }.parts().len(), 1, "zero counts are left out");
    }
}
