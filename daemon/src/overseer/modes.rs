//! Permission modes by conversation (AC-230): Overseer sets an agent's permission mode (Ask first,
//! Accept edits, Auto), typed or spoken, and starts agents in a stated mode. The owner decided
//! (2026-10-02) that Overseer may suggest Auto only in the repositories the owner allows
//! (`overseer.auto_repos`), but always waits for their explicit yes and records its reason.
//!
//! A mode is the run's: it is stored with the run's turn options, so every later turn starts in
//! it, and a Claude turn that is running gets it at once (its `set_permission_mode` control
//! request); other harnesses take it from their next turn.

use crate::daemon::Daemon;
use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};
use std::sync::Arc;

/// The modes Overseer sets, as the harnesses name them, with the owner's words for them.
pub const MODES: &[(&str, &str)] = &[("manual", "Ask first"), ("acceptEdits", "Accept edits"), ("auto", "Auto")];

/// The meta key holding the repositories where Overseer may suggest Auto.
const AUTO_REPOS: &str = "overseer.auto_repos";

/// A mode from the owner's or the model's words: "Ask first", "accept edits", "acceptEdits",
/// "auto mode", "manual". None for anything else (plan and bypass are not set by conversation).
pub fn normalize(words: &str) -> Option<&'static str> {
    let w: String = words.to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect();
    let w = w.strip_suffix("mode").unwrap_or(&w);
    match w {
        "askfirst" | "manual" | "ask" | "default" => Some("manual"),
        "acceptedits" | "accept" | "edits" => Some("acceptEdits"),
        "auto" => Some("auto"),
        _ => None,
    }
}

/// The owner's words for a mode.
pub fn label(mode: &str) -> &str {
    MODES.iter().find(|(m, _)| *m == mode).map(|(_, l)| *l).unwrap_or(mode)
}

fn repo_root(path: &str) -> String {
    crate::git::toplevel(std::path::Path::new(path)).map(|p| p.to_string_lossy().to_string()).unwrap_or_else(|_| path.trim_end_matches('/').to_string())
}

impl Daemon {
    /// `overseer.auto_repos`: the repositories where Overseer may suggest Auto (the owner's,
    /// from the Mac only). With `repos`, sets the list.
    pub fn overseer_auto_repos(&self, repos: Option<&Value>) -> Result<Value> {
        if let Some(list) = repos {
            let list = list.as_array().ok_or_else(|| anyhow!("repos must be a list of repository paths"))?;
            let mut roots: Vec<String> = Vec::new();
            for r in list {
                let p = r.as_str().ok_or_else(|| anyhow!("repos must be a list of repository paths"))?;
                let root = repo_root(p);
                if !roots.contains(&root) {
                    roots.push(root);
                }
            }
            crate::continuity::meta_set(self, AUTO_REPOS, &serde_json::to_string(&roots)?)?;
            return Ok(json!({"repos": roots}));
        }
        Ok(json!({"repos": self.auto_repos()}))
    }

    fn auto_repos(&self) -> Vec<String> {
        crate::continuity::meta_get(self, AUTO_REPOS).and_then(|j| serde_json::from_str(&j).ok()).unwrap_or_default()
    }

    /// Checks a `mode` action before it is proposed (Overseer's actions are checked by the daemon,
    /// whatever the model claims): a mode the agent's harness takes; Auto set by Overseer itself
    /// only suggested in a repository the owner allows, with a reason and explicit confirmation.
    pub fn check_mode_action(&self, a: &mut Value, owner_asked: bool, cause: &str) -> Result<()> {
        let agent = a["agent"].as_str().unwrap_or("").to_string();
        let run = self.run(&agent)?;
        let words = a["mode"].as_str().or(a["permission_mode"].as_str()).unwrap_or("");
        let mode = normalize(words).ok_or_else(|| anyhow!("{words:?} is not a mode Overseer sets; say Ask first, Accept edits or Auto"))?;
        crate::adapters::check_turn_options(&run.harness, None, Some(mode), 0).map_err(|_| anyhow!("{} runs on {}, which does not take {}", run.title, crate::handoff::harness_name(&run.harness), label(mode)))?;
        a["mode"] = json!(mode);
        let repo = self.task(&run.task_id).map(|t| t.repo_root).unwrap_or_default();
        self.check_auto_by_itself(mode, &repo, owner_asked, cause, a)?;
        Ok(())
    }

    /// A start in a stated mode: the mode in the owner's words becomes the harness's, and Auto
    /// chosen by Overseer itself follows the same rule as setting it.
    pub fn check_start_mode(&self, a: &mut Value, owner_asked: bool, cause: &str) -> Result<()> {
        let Some(words) = a["permission_mode"].as_str().filter(|s| !s.is_empty()).map(str::to_string) else { return Ok(()) };
        // A harness's own mode ("plan", "read-only") is passed on as it is; the harness checks it.
        let Some(mode) = normalize(&words) else { return Ok(()) };
        a["permission_mode"] = json!(mode);
        let repo = repo_root(a["repo"].as_str().unwrap_or(""));
        self.check_auto_by_itself(mode, &repo, owner_asked, cause, a)
    }

    fn check_auto_by_itself(&self, mode: &str, repo: &str, owner_asked: bool, cause: &str, a: &mut Value) -> Result<()> {
        if mode != "auto" {
            return Ok(());
        }
        // The turn's cause is not proof that the owner requested a permission change.
        // Every Auto transition waits for an explicit yes, including owner/voice turns.
        a["class"] = json!(super::control::CONFIRM);
        if owner_asked {
            return Ok(());
        }
        let allowed = self.auto_repos();
        let name = repo.rsplit('/').next().unwrap_or(repo);
        if !allowed.iter().any(|r| r == repo) {
            bail!("Overseer sets Auto by itself only in the repositories the owner allows, and {name} is not one; this turn was started by {cause}");
        }
        if a["why"].as_str().map(str::trim).unwrap_or("").is_empty() {
            bail!("say why: Overseer setting Auto by itself is recorded with its reason");
        }
        Ok(())
    }

    /// Sets a run's permission mode: stored with its turn options (every later turn starts in it)
    /// and, for a Claude turn that is running, sent to it at once. Recorded on the run with who
    /// set it, what led to it and why.
    pub fn set_agent_mode(self: &Arc<Self>, run_id: &str, mode: &str, detail: Value) -> Result<Value> {
        let run = self.run(run_id)?;
        crate::adapters::check_turn_options(&run.harness, None, Some(mode), 0)?;
        {
            let store = self.store.lock().unwrap();
            let launch: Value = store.conn.query_row("SELECT launch FROM runs WHERE id=?1", [run_id], |r| r.get::<_, Option<String>>(0))?.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(Value::Null);
            let mut launch = if launch.is_object() { launch } else { json!({"generic": {}}) };
            // The turn options live in the generic part when there is one (see `start_turn`).
            let generic = if launch.get("generic").is_some() { &mut launch["generic"] } else { &mut launch };
            if !generic["opts"].is_object() {
                generic["opts"] = json!({});
            }
            generic["opts"]["mode"] = json!(mode);
            store.conn.execute("UPDATE runs SET launch=?2 WHERE id=?1", rusqlite::params![run_id, launch.to_string()])?;
        }
        // A Claude turn that is running takes it now; everything else from its next turn.
        let live = run.harness == "claude" && matches!(run.status.as_str(), "running" | "waiting_for_user") && {
            let request = json!({"type": "control_request", "request_id": format!("mode-{}", &uuid::Uuid::new_v4().simple().to_string()[..8]), "request": {"subtype": "set_permission_mode", "mode": mode}});
            self.send_stdin(&run, &format!("{request}\n")).is_ok()
        };
        let mut payload = json!({"action": "mode", "mode": mode, "label": label(mode), "live": live});
        if let Some(obj) = detail.as_object() {
            for (k, v) in obj {
                payload[k] = v.clone();
            }
        }
        self.emit(Some(&run.task_id), Some(run_id), "overseer_action", "overseer", "exact", payload)?;
        Ok(json!({"run_id": run_id, "mode": mode, "label": label(mode), "live": live}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_owner_s_words_for_each_mode() {
        assert_eq!(normalize("Ask first"), Some("manual"));
        assert_eq!(normalize("ask-first mode"), Some("manual"));
        assert_eq!(normalize("Accept edits"), Some("acceptEdits"));
        assert_eq!(normalize("acceptEdits"), Some("acceptEdits"));
        assert_eq!(normalize("Auto"), Some("auto"));
        assert_eq!(normalize("auto mode"), Some("auto"));
        assert_eq!(normalize("bypassPermissions"), None);
        assert_eq!(normalize("plan"), None);
        assert_eq!(label("acceptEdits"), "Accept edits");
    }
}
