//! Stuck, failed and limited agents come back to Overseer (AC-239). An agent that fails, hits a
//! usage limit, finds its account signed out, or says nothing for a long time while it runs is
//! noticed by the daemon with no model: the conversation gets a card that says what happened in
//! plain words (never an error class or an HTTP code) and what can be done, and Overseer gets a
//! turn with the reason, which explains it and offers the fix (at the Auto level, does it):
//! continue on another account or model, retry, or stop. What needs Overseer's turn waits for its
//! first turn like the agents' questions do (AC-248).

use crate::daemon::Daemon;
use crate::store::Run;
use anyhow::Result;
use serde_json::{json, Value};
use std::sync::Arc;

/// How long a running agent may go with no event before it counts as stuck (10 minutes; the
/// owner, or a test, sets `overseer.silence_ms`).
pub const DEFAULT_SILENCE_MS: i64 = 10 * 60 * 1000;
/// The queue reason's prefix: kept until a turn of Overseer's can take it.
pub const REASON: &str = "trouble:";

fn ensure(conn: &rusqlite::Connection) -> Result<()> {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS overseer_trouble(run_id TEXT NOT NULL, kind TEXT NOT NULL, key TEXT NOT NULL, ts INTEGER NOT NULL, reason TEXT NOT NULL, PRIMARY KEY(run_id, kind, key));")?;
    Ok(())
}

/// What went wrong, in the owner's words: the kind (limit, signed_out, failed, silent) and one
/// clause that follows the agent's name.
pub fn plain(class: Option<&str>, message: &str, exit_reason: &str) -> (&'static str, String) {
    let text = if message.trim().is_empty() { exit_reason } else { message };
    let lower = text.to_ascii_lowercase();
    let class = class.unwrap_or("");
    if matches!(class, "rate_limit" | "quota") || lower.contains("usage limit") || lower.contains("rate limit") || lower.contains("429") {
        let resets = regex_lite_reset(text).map(|r| format!("; it resets {r}")).unwrap_or_default();
        return ("limit", format!("reached its account's usage limit{resets}"));
    }
    if class == "auth" || lower.contains("authenticate") || lower.contains("not logged in") || lower.contains("oauth") {
        return ("signed_out", "could not sign in: its account needs signing in again".to_string());
    }
    let cleaned = clean(text);
    ("failed", if cleaned.is_empty() { "failed".to_string() } else { format!("failed: {cleaned}") })
}

/// "Your limit will reset at 5pm." → "at 5pm".
fn regex_lite_reset(text: &str) -> Option<String> {
    let lower = text.to_ascii_lowercase();
    let at = lower.find("reset")?;
    let rest = &text[at..];
    let from = rest.find(" at ").or_else(|| rest.find(" in "))?;
    let clause: String = rest[from + 1..].chars().take_while(|c| *c != '.' && *c != '\n' && *c != ';').collect();
    let clause = clause.trim().to_string();
    (!clause.is_empty() && clause.len() <= 40).then_some(clause)
}

/// A harness's failure text without the plumbing around it: the daemon's own prefix, the error
/// class in brackets, "API Error", HTTP status codes and anything JSON-shaped.
pub fn clean(text: &str) -> String {
    let mut t = text.trim().to_string();
    if let Some(at) = t.find("last error [") {
        if let Some(end) = t[at..].find("]: ") {
            t = t[at + end + 3..].to_string();
        }
    }
    for prefix in ["turn reported failure", "API Error:", "Error:", "error:"] {
        if let Some(rest) = t.strip_prefix(prefix) {
            t = rest.trim_start_matches([';', ':', ' ']).to_string();
        }
    }
    // Bracketed classes and HTTP codes: "[rate_limit]", "(429)", "HTTP 503", "status 500".
    let mut out = String::new();
    let mut skip_to: Option<char> = None;
    for c in t.chars() {
        if let Some(end) = skip_to {
            if c == end {
                skip_to = None;
            }
            continue;
        }
        if c == '[' || c == '{' {
            skip_to = Some(if c == '[' { ']' } else { '}' });
            continue;
        }
        out.push(c);
    }
    let words: Vec<&str> = out.split_whitespace().collect();
    let mut kept: Vec<String> = Vec::new();
    for (i, w) in words.iter().enumerate() {
        let bare = w.trim_matches(|c: char| !c.is_ascii_alphanumeric());
        let code = bare.len() == 3 && bare.chars().all(|c| c.is_ascii_digit()) && (bare.starts_with('4') || bare.starts_with('5'));
        let http = bare.eq_ignore_ascii_case("http") && words.get(i + 1).is_some_and(|n| n.trim_matches(|c: char| !c.is_ascii_alphanumeric()).chars().all(|c| c.is_ascii_digit()));
        if code || http || *w == "·" || w.is_empty() {
            continue;
        }
        kept.push(w.to_string());
    }
    let joined = kept.join(" ").replace("()", "").replace(" ,", ",").trim().trim_end_matches([',', ';', ':', '-']).trim().to_string();
    joined.chars().take(200).collect()
}

impl Daemon {
    /// A top-level agent's run (not Overseer's, not a watcher, not a native child).
    fn is_plain_agent(&self, run: &Run) -> bool {
        run.parent_run_id.is_none() && self.run_role(&run.id) == "agent"
    }

    /// An agent's run ended failed: why, in plain words, and back to Overseer.
    pub(crate) fn trouble_on_status(self: &Arc<Self>, run_id: &str, status: &str) -> Result<()> {
        if status != "failed" {
            return Ok(());
        }
        let run = self.run(run_id)?;
        if !self.is_plain_agent(&run) || self.store.lock().unwrap().is_swarm_linked_run(&run.id)? {
            return Ok(());
        }
        let last_error: Option<(String, String)> = {
            let store = self.store.lock().unwrap();
            store.conn.query_row("SELECT json_extract(payload, '$.class'), json_extract(payload, '$.message') FROM events WHERE run_id=?1 AND kind='error' ORDER BY seq DESC LIMIT 1", [run_id], |r| Ok((r.get::<_, Option<String>>(0)?.unwrap_or_default(), r.get::<_, Option<String>>(1)?.unwrap_or_default()))).ok()
        };
        let (class, message) = last_error.map(|(c, m)| (Some(c), m)).unwrap_or((None, String::new()));
        let (kind, reason) = plain(class.as_deref(), &message, run.exit_reason.as_deref().unwrap_or(""));
        // One per ended run: its end time is the key.
        self.raise_trouble(&run, kind, &reason, &run.ended_ms.unwrap_or(0).to_string())
    }

    /// Running agents that have said nothing for the silence limit: stuck, back to Overseer, once
    /// per silence (a later event starts a new one).
    pub(crate) fn find_silent_agents(self: &Arc<Self>) -> Result<()> {
        use rusqlite::OptionalExtension;
        let limit = {
            let store = self.store.lock().unwrap();
            store.conn.query_row("SELECT value FROM meta WHERE key='overseer.silence_ms'", [], |r| r.get::<_, String>(0)).optional()?.and_then(|v| v.parse::<i64>().ok()).unwrap_or(DEFAULT_SILENCE_MS)
        };
        let now = crate::daemon::now();
        let quiet: Vec<(String, i64, i64)> = {
            let store = self.store.lock().unwrap();
            let mut stmt = store.conn.prepare("SELECT r.id, COALESCE((SELECT MAX(ts) FROM events e WHERE e.run_id=r.id), r.created_ms), COALESCE((SELECT MAX(seq) FROM events e WHERE e.run_id=r.id), 0) FROM runs r WHERE r.status='running' AND r.parent_run_id IS NULL")?;
            let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
            rows
        };
        for (id, last_ms, last_seq) in quiet {
            if now - last_ms < limit {
                continue;
            }
            let Ok(run) = self.run(&id) else { continue };
            if !self.is_plain_agent(&run) || self.hold_of(&id).is_some() || self.store.lock().unwrap().is_swarm_linked_run(&id)? {
                continue;
            }
            let minutes = ((now - last_ms) / 60_000).max(1);
            let reason = if limit >= 60_000 { format!("has said nothing for {minutes} minute{} while working", if minutes == 1 { "" } else { "s" }) } else { "has said nothing for a while as it works".to_string() };
            self.raise_trouble(&run, "silent", &reason, &last_seq.to_string())?;
        }
        Ok(())
    }

    /// The card (no model) and Overseer's turn (queued, kept until a turn can take it).
    fn raise_trouble(self: &Arc<Self>, run: &Run, kind: &str, reason: &str, key: &str) -> Result<()> {
        let fresh = {
            let store = self.store.lock().unwrap();
            ensure(&store.conn)?;
            store.conn.execute("INSERT OR IGNORE INTO overseer_trouble(run_id, kind, key, ts, reason) VALUES(?1, ?2, ?3, ?4, ?5)", rusqlite::params![run.id, kind, key, crate::daemon::now(), reason])? == 1
        };
        if !fresh {
            return Ok(());
        }
        let offers = self.trouble_offers(run, kind);
        let session = self.overseer_session()?;
        let sid = session["id"].as_str().unwrap_or_default().to_string();
        let text = format!("{} {reason}.", run.title);
        let card = json!({"kind": "trouble", "agent": run.id, "title": run.title, "trouble": kind, "reason": reason, "offers": offers});
        self.append_session_message(&sid, "card", None, &text, Some(&card))?;
        self.emit(Some(&run.task_id), Some(&run.id), "trouble", "daemon", "exact", json!({"kind": kind, "reason": reason}))?;
        // One queue entry per agent and kind; its details are read when the turn is composed.
        self.check_in_due(&run.id, &format!("{REASON}{kind}"))
    }

    /// What can be done, best first: another account (signed in) or the same account again for a
    /// limit or a sign-in; a retry for a failure; a stop or a question for one that is stuck.
    fn trouble_offers(&self, run: &Run, kind: &str) -> Vec<Value> {
        let mut offers = Vec::new();
        match kind {
            "limit" | "signed_out" => {
                for (id, name, harness) in self.other_accounts(run) {
                    offers.push(json!({"action": "continue", "profile": id, "harness": harness, "label": format!("Continue on {name}")}));
                }
                if kind == "limit" {
                    offers.push(json!({"action": "retry", "label": "Retry later on the same account"}));
                }
            }
            "failed" => offers.push(json!({"action": "retry", "label": "Retry"})),
            _ => {
                offers.push(json!({"action": "message", "label": "Ask what it is doing"}));
                offers.push(json!({"action": "stop", "label": "Stop it"}));
            }
        }
        offers
    }

    /// Signed-in accounts other than the run's, on its harness first.
    pub(crate) fn other_accounts(&self, run: &Run) -> Vec<(String, String, String)> {
        let family = crate::daemon::profile_harness(&run.harness).to_string();
        let mine = run.profile_id.clone().unwrap_or_else(|| format!("system-{family}"));
        let mut profiles: Vec<crate::store::Profile> = self.store.lock().unwrap().profiles().unwrap_or_default().into_iter().filter(|p| p.id != mine && ["claude", "codex", "opencode"].contains(&p.harness.as_str())).collect();
        profiles.sort_by_key(|p| (p.harness != family, !p.is_system, p.created_ms));
        profiles
            .into_iter()
            .filter(|p| crate::adapters::resolve_program(&p.harness).is_some() && self.profile_status(&p.id).map(|s| s["logged_in"] == true).unwrap_or(false))
            .take(3)
            .map(|p| (p.id, p.name, p.harness))
            .collect()
    }

    /// What Overseer reads about an agent in trouble, for its turn.
    pub(crate) fn trouble_item(&self, run_id: &str, kinds: &[&str]) -> Result<Option<Value>> {
        let run = self.run(run_id)?;
        let rows: Vec<(String, String)> = {
            let store = self.store.lock().unwrap();
            ensure(&store.conn)?;
            let mut stmt = store.conn.prepare("SELECT kind, reason FROM overseer_trouble WHERE run_id=?1 ORDER BY ts DESC")?;
            let rows = stmt.query_map([run_id], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
            rows
        };
        let Some((kind, reason)) = rows.into_iter().find(|(k, _)| kinds.contains(&k.as_str())) else { return Ok(None) };
        let account = run.profile_id.as_deref().and_then(|p| self.profile(p).ok()).map(|p| p.name);
        let others: Vec<Value> = self.other_accounts(&run).into_iter().map(|(id, name, harness)| json!({"id": id, "name": name, "harness": harness})).collect();
        Ok(Some(json!({"id": run.id, "title": run.title, "status": run.status, "kind": kind, "reason": reason, "harness": run.harness, "model": run.model, "account": account, "other_accounts": others, "offers": self.trouble_offers(&run, &kind)})))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reasons_are_plain() {
        let (k, r) = plain(Some("rate_limit"), "API Error: Request rejected (429) · rate limited", "turn reported failure; last error [rate_limit]: API Error: Request rejected (429) · rate limited");
        assert_eq!((k, r.as_str()), ("limit", "reached its account's usage limit"));
        let (k, r) = plain(Some("quota"), "Claude usage limit reached. Your limit will reset at 5pm.", "");
        assert_eq!((k, r.as_str()), ("limit", "reached its account's usage limit; it resets at 5pm"));
        let (k, r) = plain(Some("auth"), "Failed to authenticate: OAuth session expired", "");
        assert_eq!(k, "signed_out");
        assert!(!r.contains("OAuth"), "{r}");
        let (k, r) = plain(None, "", "turn reported failure; last error [turn_failed]: Migration failed: relation users_v2 does not exist");
        assert_eq!((k, r.as_str()), ("failed", "failed: Migration failed: relation users_v2 does not exist"));
        assert_eq!(clean("HTTP 503 Service Unavailable {\"type\":\"error\"}"), "Service Unavailable");
        assert_eq!(clean("stream error: 500 Internal Server Error"), "stream error: Internal Server Error");
    }
}
