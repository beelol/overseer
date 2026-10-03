//! What needs the owner, handled by conversation (AC-227): "handle what needs me" answers the
//! waiting item or asks the one question it needs, and "tell it yes" answers it, typed or spoken
//! the same way. The daemon does this with no model turn: it knows what waits (an agent's
//! permission request, a proposal of Overseer's), and the owner's words are the answer.

use crate::daemon::Daemon;
use anyhow::Result;
use serde_json::{json, Value};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ask {
    /// "Handle what needs me": answer it, or ask the one question it needs.
    Handle,
    /// "Tell it yes", "allow it".
    Yes,
    /// "Tell it no", "deny it".
    No,
}

/// The words, lowercased, with only letters, digits and single spaces.
pub fn plain(words: &str) -> String {
    let lower = words.to_lowercase().replace(['’', '\''], " ");
    let mut out = String::new();
    for w in lower.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()) {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(w);
    }
    out
}

const HANDLE: &[&str] = &[
    "handle what needs me", "handle whatever needs me", "handle what needs you", "handle what needs my attention",
    "handle what s waiting", "handle whats waiting", "handle what is waiting", "handle what s waiting for me",
    "take care of what needs me", "deal with what needs me", "handle the needs you", "handle needs you",
    "handle it", "handle that", "what needs me", "what needs me now",
];
const YES: &[&str] = &[
    "tell it yes", "tell it to go ahead", "tell it go ahead", "tell it it can", "tell them yes", "allow it", "approve it",
    "let it", "let it do it", "let it do that", "yes allow it", "yes tell it yes", "say yes", "say yes to it",
];
const NO: &[&str] = &["tell it no", "tell them no", "deny it", "don t allow it", "dont allow it", "do not allow it", "say no", "no deny it", "reject it"];
/// A bare answer, which counts only right after the question Overseer asked about what needs you.
const BARE_YES: &[&str] = &["yes", "yes please", "allow", "go ahead", "do it", "approve", "sure", "ok", "okay"];
const BARE_NO: &[&str] = &["no", "deny", "reject", "no thanks"];

/// Whether the words ask Overseer to handle what needs the owner, or to answer it.
pub fn ask(words: &str) -> Option<Ask> {
    let mut p = plain(words);
    for lead in ["overseer ", "hey overseer ", "please ", "ok ", "okay ", "and "] {
        if let Some(rest) = p.strip_prefix(lead) {
            p = rest.to_string();
        }
    }
    for tail in [" please", " overseer", " for me", " now"] {
        if let Some(rest) = p.strip_suffix(tail) {
            p = rest.to_string();
        }
    }
    if HANDLE.contains(&p.as_str()) {
        return Some(Ask::Handle);
    }
    if YES.contains(&p.as_str()) {
        return Some(Ask::Yes);
    }
    if NO.contains(&p.as_str()) {
        return Some(Ask::No);
    }
    None
}

/// An agent's permission request that waits for the owner.
#[derive(Clone, Debug)]
pub struct Waiting {
    pub run: String,
    pub request: String,
    pub title: String,
    /// What it wants, in a few words ("change page.md", "run npm test").
    pub what: String,
}

/// What it wants, said in a few words.
pub fn summarize(att: &Value) -> String {
    let tool = att["tool"].as_str().unwrap_or("do something");
    let input = &att["input"];
    let detail = input["command"].as_str().or(input["file_path"].as_str()).or(input["path"].as_str()).map(|s| s.to_string());
    match (tool, detail) {
        ("Bash" | "bash" | "shell" | "exec_command" | "command", Some(c)) => format!("run {}", c.chars().take(80).collect::<String>()),
        ("Edit" | "Write" | "edit" | "write" | "apply_patch", Some(p)) => format!("change {}", p.rsplit('/').next().unwrap_or(&p)),
        (t, Some(x)) => format!("use {t} on {}", x.rsplit('/').next().unwrap_or(&x).chars().take(80).collect::<String>()),
        (t, None) => format!("use {t}"),
    }
}

impl Daemon {
    /// The permission requests waiting for the owner, oldest agent first.
    pub fn needs_waiting(&self) -> Vec<Waiting> {
        let runs = self.store.lock().unwrap().runs().unwrap_or_default();
        let mut out: Vec<(i64, Waiting)> = runs
            .into_iter()
            .filter(|r| r.parent_run_id.is_none() && r.status == "waiting_for_user" && self.run_role(&r.id) != "overseer")
            .filter_map(|r| {
                let att = r.attention.clone().filter(|a| a["kind"] == "permission")?;
                Some((r.created_ms, Waiting { run: r.id.clone(), request: att["request_id"].as_str().unwrap_or("").to_string(), title: r.title.clone(), what: summarize(&att) }))
            })
            .collect();
        out.sort_by_key(|(t, _)| *t);
        out.into_iter().map(|(_, w)| w).collect()
    }

    /// The owner's words about what needs them, typed or spoken: handled here with no model turn.
    /// `None` when the words are not about that (they go to Overseer as usual). Voice Mode answers
    /// a permission itself (with its toast and window, AC-171) and passes `answer: false`.
    pub fn needs_handle(self: &Arc<Self>, words: &str, surface: &str, owner_text: Option<&str>, answer: bool) -> Result<Option<Value>> {
        let session = self.overseer_session()?;
        let sid = session["id"].as_str().unwrap_or_default().to_string();
        let p = plain(words);
        // A bare yes or no right after the question about what needs you answers that question.
        let pending_question: Option<String> = session["messages"].as_array().and_then(|m| m.last()).filter(|m| m["card"]["kind"] == "needs" && m["card"]["state"] == "asked").and_then(|m| m["card"]["proposal"].as_str().map(str::to_string));
        let bare = if BARE_YES.contains(&p.as_str()) { Some(true) } else if BARE_NO.contains(&p.as_str()) { Some(false) } else { None };
        if let (Some(yes), Some(proposal)) = (bare, pending_question.clone()) {
            let open = session["proposals"].as_array().is_some_and(|ps| ps.iter().any(|x| x["id"] == proposal.as_str()));
            if open {
                let owner = self.append_session_message(&sid, "owner", Some(surface), owner_text.unwrap_or(words), None)?;
                let r = self.overseer_answer(&proposal, yes, surface, "owner");
                let text = match &r {
                    Ok(r) if yes => format!("{}", r["result"].as_str().unwrap_or("Done.")),
                    Ok(_) => "Declined: nothing was done.".to_string(),
                    Err(e) => format!("That could not be done: {e}"),
                };
                let reply = self.append_session_message(&sid, "overseer", None, &text, Some(&json!({"kind": "needs", "state": if yes { "allowed" } else { "denied" }, "proposal": proposal})))?;
                return Ok(Some(json!({"message": owner, "handled": true, "reply": reply["text"], "queued": false})));
            }
        }
        let Some(ask) = ask(words) else { return Ok(None) };
        let owner = self.append_session_message(&sid, "owner", Some(surface), owner_text.unwrap_or(words), None)?;
        let waiting = self.needs_waiting();
        let open: Vec<Value> = session["proposals"].as_array().cloned().unwrap_or_default().into_iter().filter(|x| x["state"] == "open").collect();
        let (text, card) = match (ask, waiting.as_slice()) {
            (Ask::Handle, [w]) => {
                // The one question it needs: allow it? A proposal waits for the owner's yes.
                let _ = self.overseer_set_cause(if surface == "voice" { "voice" } else { "owner" });
                // The question that came up by itself (AC-230) is the same one: asked again, not
                // proposed twice.
                let already = open.iter().find(|x| x["actions"].as_array().is_some_and(|a| a.iter().any(|y| y["action"] == "permission" && y["agent"] == w.run.as_str() && y["request"] == w.request.as_str()))).and_then(|x| x["id"].as_str().map(str::to_string));
                let proposal = if already.is_some() {
                    already
                } else if answer {
                    self.overseer_propose(&json!([{"action": "permission", "agent": w.run, "request": w.request, "allow_request": true, "why": "the owner asked to handle what needs them"}]), "needs").ok().and_then(|r| r["proposal"].as_str().map(str::to_string))
                } else {
                    None
                };
                (format!("{} wants to {}. Allow it?", w.title, w.what), json!({"kind": "needs", "state": "asked", "agent": w.run, "proposal": proposal}))
            }
            (Ask::Yes | Ask::No, [w]) => {
                let allow = ask == Ask::Yes;
                if answer {
                    match self.answer_permission(&w.run, &w.request, allow, "Denied by the owner through Overseer") {
                        Ok(_) => (format!("{} {} to {}.", if allow { "Allowed" } else { "Denied" }, w.title, w.what), json!({"kind": "needs", "state": if allow { "allowed" } else { "denied" }, "agent": w.run})),
                        Err(e) => (format!("That could not be answered: {e}"), json!({"kind": "needs", "state": "failed", "agent": w.run})),
                    }
                } else {
                    (format!("{} {} to {}.", if allow { "Allowing" } else { "Denying" }, w.title, w.what), json!({"kind": "needs", "state": if allow { "allowed" } else { "denied" }, "agent": w.run}))
                }
            }
            (_, []) if open.len() == 1 => {
                let lines = open[0]["lines"].as_array().map(|l| l.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join("; ")).unwrap_or_default();
                let id = open[0]["id"].as_str().unwrap_or("").to_string();
                match ask {
                    Ask::Handle => (format!("Overseer's proposal waits for your yes: {lines}. Say yes to go ahead."), json!({"kind": "needs", "state": "asked", "proposal": id})),
                    _ => {
                        let yes = ask == Ask::Yes;
                        let r = self.overseer_answer(&id, yes, surface, "owner");
                        let text = match &r { Ok(r) if yes => r["result"].as_str().unwrap_or("Done.").to_string(), Ok(_) => "Declined: nothing was done.".into(), Err(e) => format!("That could not be done: {e}") };
                        (text, json!({"kind": "needs", "state": if yes { "allowed" } else { "denied" }, "proposal": id}))
                    }
                }
            }
            (_, []) => ("Nothing needs you right now.".to_string(), json!({"kind": "needs", "state": "nothing"})),
            (_, many) => {
                let names: Vec<String> = many.iter().map(|w| w.title.clone()).collect();
                let list = match names.len() { 2 => format!("{} or {}", names[0], names[1]), _ => format!("{}, or {}", names[..names.len() - 1].join(", "), names[names.len() - 1]) };
                (format!("{} agents are waiting for your permission. Which one: {list}?", many.len()), json!({"kind": "needs", "state": "which", "agents": many.iter().map(|w| w.run.clone()).collect::<Vec<_>>()}))
            }
        };
        let reply = self.append_session_message(&sid, "overseer", None, &text, Some(&card))?;
        Ok(Some(json!({"message": owner, "handled": true, "reply": reply["text"], "card": card, "queued": false})))
    }
}

impl Daemon {
    /// An agent's permission request comes up by itself (AC-230): the moment it waits, the
    /// conversation asks the owner the one question it needs ("Sessions wants to change perm.txt.
    /// Allow it?") with a yes/no proposal, without the owner having to ask; Voice Mode reads it out.
    /// Once per request; answered anywhere, the proposal closes as not needed (AC-228).
    pub fn needs_prompt(self: &Arc<Self>, run_id: &str) -> Result<()> {
        let Ok(run) = self.run(run_id) else { return Ok(()) };
        if run.parent_run_id.is_some() || self.run_role(run_id) == "overseer" || run.status != "waiting_for_user" {
            return Ok(());
        }
        let Some(att) = run.attention.clone().filter(|a| a["kind"] == "permission") else { return Ok(()) };
        let request = att["request_id"].as_str().unwrap_or("").to_string();
        let session = self.overseer_session()?;
        let sid = session["id"].as_str().unwrap_or_default().to_string();
        // Once per request: the owner may already have been asked (by "handle what needs me").
        let asked = session["messages"].as_array().is_some_and(|m| m.iter().any(|x| x["card"]["kind"] == "needs" && x["card"]["agent"] == run_id && x["card"]["request"] == request.as_str()))
            || session["proposals"].as_array().is_some_and(|ps| ps.iter().any(|x| x["state"] == "open" && x["actions"].as_array().is_some_and(|a| a.iter().any(|y| y["action"] == "permission" && y["agent"] == run_id && y["request"] == request.as_str()))));
        if asked {
            return Ok(());
        }
        let action = json!([{"action": "permission", "agent": run_id, "request": request, "allow_request": true, "why": "it is waiting for your permission"}]);
        let proposal = self.overseer_propose_as(&action, "needs", Some("needs"))?["proposal"].as_str().map(str::to_string);
        let text = format!("{} wants to {}. Allow it?", run.title, summarize(&att));
        self.append_session_message(&sid, "overseer", None, &text, Some(&json!({"kind": "needs", "state": "asked", "agent": run_id, "request": request, "proposal": proposal, "by_itself": true})))?;
        crate::voice::request::permission_waiting(self, run_id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_words_that_handle_what_needs_the_owner() {
        assert_eq!(ask("Handle what needs me."), Some(Ask::Handle));
        assert_eq!(ask("Overseer, handle what needs me please"), Some(Ask::Handle));
        assert_eq!(ask("Tell it yes."), Some(Ask::Yes));
        assert_eq!(ask("allow it"), Some(Ask::Yes));
        assert_eq!(ask("Tell it no"), Some(Ask::No));
        assert_eq!(ask("Tell Phone to use the new wire format."), None);
        assert_eq!(ask("yes"), None, "a bare yes answers only the question just asked");
        assert_eq!(ask("what is everyone doing?"), None);
    }
}
