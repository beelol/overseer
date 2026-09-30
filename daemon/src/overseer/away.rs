//! Overseer leads with what happened while you were away (AC-253). When the owner opens Talk to
//! Overseer (a surface says so with `overseer.visit`) after agents finished, failed or stopped
//! since the last visit, the conversation's newest message is one line grouped by repository
//! and outcome ("While you were away: 3 finished in site; 1 failed in notes."), before anything
//! else. Asking "what happened while I was away" gives the same line, from the same window,
//! with no model turn. Built by the daemon from its own records.

use crate::daemon::Daemon;
use anyhow::Result;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::Arc;

const LAST_VISIT: &str = "overseer.last_visit_ms";
const AWAY_FROM: &str = "overseer.away_from_ms";
/// The outcomes, in the order a line names them.
const OUTCOMES: [&str; 4] = ["finished", "need you", "failed", "stopped"];

/// "what happened", "what did I miss", "while I was away", "since I left": the owner asks for the
/// summary.
pub fn asks_what_happened(text: &str) -> bool {
    let t = text.trim().to_ascii_lowercase();
    let t = t.trim_end_matches(['?', '.', '!']);
    t.starts_with("what happened") || t.starts_with("what did i miss") || t.contains("while i was away") || t.contains("since i left") || t.contains("since i was last here")
}

impl Daemon {
    fn meta_ms(&self, key: &str) -> Option<i64> {
        use rusqlite::OptionalExtension;
        self.store.lock().unwrap().conn.query_row("SELECT value FROM meta WHERE key=?1", [key], |r| r.get::<_, String>(0)).optional().ok().flatten().and_then(|v| v.parse().ok())
    }

    fn set_meta_ms(&self, key: &str, value: i64) -> Result<()> {
        self.store.lock().unwrap().conn.execute("INSERT OR REPLACE INTO meta(key, value) VALUES(?1, ?2)", rusqlite::params![key, value.to_string()])?;
        Ok(())
    }

    /// What happened to the agents between two times: each agent whose run reached an end in
    /// that window (or started waiting for the owner), by repository and outcome.
    pub(crate) fn away_summary(&self, from_ms: i64, to_ms: i64) -> Result<Value> {
        let (runs, tasks) = {
            let store = self.store.lock().unwrap();
            (store.runs()?, store.tasks()?)
        };
        let mut groups: BTreeMap<String, BTreeMap<&'static str, Vec<String>>> = BTreeMap::new();
        for run in runs.iter().filter(|r| r.parent_run_id.is_none()) {
            if self.run_role(&run.id) != "agent" {
                continue;
            }
            let Some(task) = tasks.iter().find(|t| t.id == run.task_id) else { continue };
            if task.archived_ms.is_some() {
                continue;
            }
            let outcome = match run.status.as_str() {
                "completed" | "failed" | "interrupted" if run.ended_ms.is_some_and(|t| t > from_ms && t <= to_ms) => match run.status.as_str() {
                    // Finished, but its check-in found it not done: it needs the owner.
                    "completed" if self.last_verdict_not_done(&run.id) => "need you",
                    "completed" => "finished",
                    "failed" => "failed",
                    _ => "stopped",
                },
                "waiting_for_user" if run.attention.is_some() => "need you",
                _ => continue,
            };
            let repo = task.repo_root.rsplit('/').next().unwrap_or(&task.repo_root).to_string();
            groups.entry(repo).or_default().entry(outcome).or_default().push(run.title.clone());
        }
        // The repository with the most news leads.
        let mut groups: Vec<(String, BTreeMap<&'static str, Vec<String>>)> = groups.into_iter().collect();
        groups.sort_by_key(|(repo, by)| (std::cmp::Reverse(by.values().map(Vec::len).sum::<usize>()), repo.clone()));
        let total: usize = groups.iter().flat_map(|(_, g)| g.values()).map(Vec::len).sum();
        let parts: Vec<String> = groups
            .iter()
            .map(|(repo, by)| {
                let counts: Vec<String> = OUTCOMES.iter().filter_map(|o| by.get(o).map(|v| format!("{} {}", v.len(), if *o == "need you" && v.len() == 1 { "needs you" } else { o }))).collect();
                let counts = match counts.len() {
                    0 => String::new(),
                    1 => counts[0].clone(),
                    n => format!("{} and {}", counts[..n - 1].join(", "), counts[n - 1]),
                };
                format!("{counts} in {repo}")
            })
            .collect();
        let text = if total == 0 { "Nothing finished, failed or stopped while you were away.".to_string() } else { format!("While you were away: {}.", parts.join("; ")) };
        let detail: Vec<Value> = groups.iter().map(|(repo, by)| json!({"repository": repo, "outcomes": by.iter().map(|(o, titles)| json!({"outcome": o, "count": titles.len(), "agents": titles})).collect::<Vec<_>>()})).collect();
        Ok(json!({"text": text, "count": total, "groups": detail, "from_ms": from_ms, "to_ms": to_ms}))
    }

    fn last_verdict_not_done(&self, run_id: &str) -> bool {
        self.store.lock().unwrap().conn.query_row("SELECT result FROM check_ins WHERE run_id=?1 ORDER BY ts DESC LIMIT 1", [run_id], |r| r.get::<_, String>(0)).map(|r| r == "drifting").unwrap_or(false)
    }

    /// The owner opened Talk to Overseer: what happened since their last visit leads the
    /// conversation, once. The window is kept so asking "what happened" gives the same line.
    pub fn overseer_visit(self: &Arc<Self>, surface: &str) -> Result<Value> {
        let now = crate::daemon::now();
        let session = self.overseer_session()?;
        let sid = session["id"].as_str().unwrap_or_default().to_string();
        // The owner's own words count as being here too.
        let spoke: Option<i64> = self.store.lock().unwrap().conn.query_row("SELECT MAX(ts) FROM overseer_messages WHERE source='owner'", [], |r| r.get(0)).ok().flatten();
        let seen = self.meta_ms(LAST_VISIT).into_iter().chain(spoke).max();
        self.set_meta_ms(LAST_VISIT, now)?;
        // The first visit ever only marks the time: nobody was away yet.
        let Some(last) = seen else { return Ok(json!({"summary": null, "message": null, "first": true})) };
        let summary = self.away_summary(last, now)?;
        if summary["count"].as_u64().unwrap_or(0) == 0 {
            return Ok(json!({"summary": summary, "message": null}));
        }
        self.set_meta_ms(AWAY_FROM, last)?;
        let card = json!({"kind": "while_away", "groups": summary["groups"], "from_ms": last, "to_ms": now, "surface": surface});
        let msg = self.append_session_message(&sid, "overseer", Some(surface), summary["text"].as_str().unwrap_or(""), Some(&card))?;
        Ok(json!({"summary": summary, "message": msg}))
    }

    /// "What happened while I was away?": the same line as the last visit's, from the same window
    /// to now; said by the daemon, with no model turn.
    pub(crate) fn answer_what_happened(self: &Arc<Self>, text: &str, surface: &str) -> Result<Value> {
        let session = self.overseer_session()?;
        let sid = session["id"].as_str().unwrap_or_default().to_string();
        let from = match self.meta_ms(AWAY_FROM) {
            Some(t) => t,
            None => {
                let before: Option<i64> = self.store.lock().unwrap().conn.query_row("SELECT MAX(ts) FROM overseer_messages WHERE source='owner'", [], |r| r.get(0)).ok().flatten();
                before.or(self.meta_ms(LAST_VISIT)).unwrap_or(0)
            }
        };
        let summary = self.away_summary(from, crate::daemon::now())?;
        let asked = self.append_session_message(&sid, "owner", Some(surface), text, None)?;
        let card = json!({"kind": "while_away", "groups": summary["groups"], "from_ms": from, "to_ms": summary["to_ms"], "surface": surface});
        let reply = self.append_session_message(&sid, "overseer", None, summary["text"].as_str().unwrap_or(""), Some(&card))?;
        Ok(json!({"message": asked, "reply": reply, "queued": false, "handled": "what_happened"}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_owner_asks_what_happened() {
        for yes in ["What happened while I was away?", "what happened", "What did I miss?", "anything happen since I left?"] {
            assert!(asks_what_happened(yes), "{yes}");
        }
        for no in ["Tell Totals to add tests", "what is everyone doing?"] {
            assert!(!asks_what_happened(no), "{no}");
        }
    }
}
