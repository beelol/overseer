//! Requests (AC-165 to AC-171): what happens to the owner's words.
//!
//! An utterance becomes a request only when it reads as meant for the one being spoken to (AC-164):
//! words that are not stay in a rolling context in memory for ten minutes and are never stored
//! (AC-173). A request goes to Overseer's session (Gate S) as the owner's message from the voice
//! surface; Overseer's plan comes back as a proposal, which for a spoken request settles for the
//! settle window at every level (the owner's decision) and is cancelled or corrected by voice
//! inside it. The daemon, not the model, says what was sent: the quick "On it." when the request is
//! taken, the plan line when the proposal is made, and "Sent." when the daemon has sent it.
//!
//! Permission requests are answered by voice after a read-back, with a cue and a toast, and can be
//! cancelled inside the window (the owner's decision); that is done here, not by the model.

use super::{floor, settings, Voice};
use crate::daemon::Daemon;
use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

/// What Overseer is told about a spoken request, the first time and with each one.
const VOICE_NOTE: &str = "(Spoken aloud to you in Voice Mode. Answer in one or two short sentences: they are read out. If this was not meant for you, reply exactly NOT_FOR_OVERSEER and propose nothing. When you message agents, write only what each should do; the daemon adds the owner's words. Give each action a confidence of high, medium or low; when low, ask one short question instead of proposing.)";

/// The rolling context: words heard that were not a request, kept in memory for ten minutes.
struct Heard {
    at: Instant,
    text: String,
}

#[derive(Clone, Debug)]
pub struct Open {
    pub id: String,
    pub words: String,
    pub at: Instant,
    pub proposal: Option<String>,
    pub settle_until: Option<Instant>,
}

/// A permission read back and waiting for the owner's yes or no.
#[derive(Clone, Debug)]
struct ReadBack {
    run: String,
    request: String,
    title: String,
    at: Instant,
}

/// A permission answered by voice, inside its window.
#[derive(Clone, Debug)]
struct Answering {
    id: String,
    run: String,
    request: String,
    allow: bool,
    title: String,
    until: Instant,
}

#[derive(Default)]
struct Requests {
    context: VecDeque<Heard>,
    open: Vec<Open>,
    read_back: Option<ReadBack>,
    answering: Option<Answering>,
    /// When "that's the hour's limit" and "Overseer isn't answering" were last said: once each.
    limit_said: Option<Instant>,
    unreachable_said: Option<Instant>,
}

static REQ: OnceLock<Mutex<Requests>> = OnceLock::new();

fn req() -> &'static Mutex<Requests> {
    REQ.get_or_init(Default::default)
}

const CANCEL: &[&str] = &[
    "cancel",
    "cancel that",
    "no",
    "no wait",
    "wait",
    "never mind",
    "nevermind",
    "stop that",
    "scratch that",
    "don't",
    "dont",
    "do not",
    "no no",
    "no stop",
];
const YES: &[&str] = &[
    "yes",
    "yes allow",
    "yes allow it",
    "allow",
    "allow it",
    "go ahead",
    "yeah allow it",
    "yes please",
    "do it",
    "approve",
    "approve it",
    "yes approve",
];
const NO: &[&str] = &[
    "no",
    "deny",
    "deny it",
    "no deny it",
    "no deny",
    "don't allow it",
    "dont allow it",
    "do not allow it",
    "reject",
    "reject it",
];

fn plain(text: &str) -> String {
    text.to_lowercase()
        .replace(['-', '’', '\''], " ")
        .split_whitespace()
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()).to_string())
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn is_backchannel(text: &str) -> bool {
    const B: &[&str] = &[
        "mm", "mmm", "mhm", "hmm", "hm", "uh", "huh", "um", "ah", "oh", "yeah", "yep", "okay",
        "ok", "right", "sure", "alright", "uhhuh",
    ];
    let p = plain(text);
    !p.is_empty() && (p.split(' ').all(|w| B.contains(&w)) || p == "got it")
}

pub fn start(v: &Arc<Voice>) {
    let d = v.d.clone();
    {
        let store = d.store.lock().unwrap();
        let _ = store.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS voice_requests(
                id TEXT PRIMARY KEY, ts INTEGER NOT NULL, words TEXT NOT NULL, via TEXT NOT NULL, target TEXT NOT NULL,
                state TEXT NOT NULL, proposal TEXT, answer TEXT, done TEXT, superseded_by TEXT, kind TEXT NOT NULL DEFAULT 'request');
             CREATE INDEX IF NOT EXISTS voice_requests_ts ON voice_requests(ts);",
        );
    }
    // Follow Overseer's session: proposals for spoken requests, their outcome, Overseer's replies.
    let v = v.clone();
    let mut events = d.events.subscribe();
    tokio::spawn(async move {
        loop {
            let e = match events.recv().await {
                Ok(e) => e,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => break,
            };
            let v = v.clone();
            match e.kind.as_str() {
                "status" | "task_archived" => {
                    let v2 = v.clone();
                    let run = if e.kind == "task_archived" {
                        e.task_id.clone()
                    } else {
                        e.run_id.clone()
                    };
                    let payload = e.payload.clone();
                    let kind = e.kind.clone();
                    let _ = tokio::task::spawn_blocking(move || {
                        target_gone(&v2, &kind, run.as_deref(), &payload)
                    })
                    .await;
                    if e.kind == "task_archived" {
                        continue;
                    }
                    let _ = tokio::task::spawn_blocking(move || {
                        on_session_event(&v, &e.kind, &e.payload, e.run_id.as_deref())
                    })
                    .await;
                }
                "proposal" | "proposal_answered" | "overseer_message" | "turn_completed"
                | "turn_failed" => {
                    let _ = tokio::task::spawn_blocking(move || {
                        on_session_event(&v, &e.kind, &e.payload, e.run_id.as_deref())
                    })
                    .await;
                }
                _ => {}
            }
        }
    });
    // Windows that close: permission answers inside their window, read-backs that waited 20 s.
    let v2 = voice_or_none();
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(100));
        if let Some(v) = v2.or_else(voice_or_none) {
            tick(v);
        }
    });
}

fn voice_or_none() -> Option<&'static Arc<Voice>> {
    super::voice()
}

fn next_id(d: &Daemon) -> String {
    let n: i64 = d
        .store
        .lock()
        .unwrap()
        .conn
        .query_row("SELECT COUNT(*) FROM voice_requests", [], |r| r.get(0))
        .unwrap_or(0);
    format!("V-{:04}", n + 1)
}

fn record(
    d: &Daemon,
    id: &str,
    words: &str,
    via: &str,
    target: &str,
    state: &str,
    kind: &str,
) -> Result<()> {
    let s = settings(d)?;
    let store = d.store.lock().unwrap();
    store.conn.execute(
        "INSERT INTO voice_requests(id, ts, words, via, target, state, kind) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        rusqlite::params![id, crate::daemon::now(), crate::redact::redact(words), via, target, state, kind],
    )?;
    // Kept for `keep_days`, 5,000 at most (AC-173).
    store.conn.execute(
        "DELETE FROM voice_requests WHERE ts < ?1",
        [crate::daemon::now() - s.keep_days as i64 * 86_400_000],
    )?;
    store.conn.execute("DELETE FROM voice_requests WHERE id NOT IN (SELECT id FROM voice_requests ORDER BY ts DESC LIMIT 5000)", [])?;
    Ok(())
}

fn update(d: &Daemon, id: &str, field: &str, value: &str) {
    let sql = format!("UPDATE voice_requests SET {field}=?2 WHERE id=?1");
    let _ = d
        .store
        .lock()
        .unwrap()
        .conn
        .execute(&sql, rusqlite::params![id, value]);
}

fn row(d: &Daemon, id: &str) -> Option<Value> {
    let store = d.store.lock().unwrap();
    store
        .conn
        .query_row("SELECT id, ts, words, via, target, state, proposal, answer, done, superseded_by, kind FROM voice_requests WHERE id=?1", [id], |r| {
            Ok(json!({"id": r.get::<_, String>(0)?, "ts": r.get::<_, i64>(1)?, "words": r.get::<_, String>(2)?, "via": r.get::<_, String>(3)?, "target": r.get::<_, String>(4)?,
                "state": r.get::<_, String>(5)?, "proposal": r.get::<_, Option<String>>(6)?, "answer": r.get::<_, Option<String>>(7)?, "done": r.get::<_, Option<String>>(8)?,
                "superseded_by": r.get::<_, Option<String>>(9)?, "kind": r.get::<_, String>(10)?}))
        })
        .ok()
}

fn announce(v: &Voice, id: &str) {
    if let Some(r) = row(&v.d, id) {
        v.emit(json!({"kind": "request", "request": r}));
    }
}

/// Speaks a line when the floor is free: never while the owner speaks; after 20 s it goes to the
/// card alone (AC-164).
fn speak_when_free(v: &Arc<Voice>, text: &str) {
    let v = v.clone();
    let text = text.to_string();
    std::thread::spawn(move || {
        let start = Instant::now();
        while v.st.lock().unwrap().gate {
            if start.elapsed() > Duration::from_secs(20) {
                v.emit(json!({"kind": "spoke", "event": "card_only", "text": text}));
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        v.say_line(&text);
    });
}

/// The heard signal: a soft Reactor cue under Audio Mode's rules (AC-165, AC-172).
fn heard_signal(v: &Voice) {
    crate::audio::cue(&v.d, "agent_queued");
    v.emit(json!({"kind": "heard_signal"}));
}

/// `target` for `voice.set`: "overseer" or an active top-level agent.
pub fn check_target(d: &Arc<Daemon>, t: &str) -> Result<String> {
    if t == "overseer" {
        return Ok(t.into());
    }
    let run = d.run(t).map_err(|_| anyhow!("no agent {t}"))?;
    if run.parent_run_id.is_some() {
        bail!("{} is a native child; talk to its parent", run.title);
    }
    Ok(run.id)
}

impl Voice {
    /// One utterance: the words of a thought, from the listener or `voice.say`.
    pub fn on_utterance(self: &Arc<Self>, text: &str, _complete: bool, via: &str) -> Value {
        let text = text.trim().to_string();
        if text.is_empty() || is_backchannel(&text) {
            return json!({"taken": false, "why": "backchannel"});
        }
        let p = plain(&text);
        // 1. A permission answer inside its window: cancel it.
        if req().lock().unwrap().answering.is_some()
            && CANCEL
                .iter()
                .any(|c| p == *c || p.starts_with(&format!("{c} ")))
        {
            return self.cancel_answer("voice");
        }
        // 2. A read-back waiting for yes or no.
        let read_back = req().lock().unwrap().read_back.clone();
        if let Some(rb) = read_back {
            if YES.contains(&p.as_str()) || NO.contains(&p.as_str()) {
                return self.answer_permission(&rb, YES.contains(&p.as_str()));
            }
        }
        // 3. A spoken request inside its settle window: cancel, correct or add.
        let settling = req()
            .lock()
            .unwrap()
            .open
            .iter()
            .rev()
            .find(|o| o.proposal.is_some() && o.settle_until.is_some_and(|u| Instant::now() < u))
            .cloned();
        if let Some(o) = settling {
            if CANCEL.iter().any(|c| p == *c) {
                return self.cancel_request(&o, "cancelled");
            }
            let correction = ["i meant", "i mean", "not ", "no ", "actually", "instead"]
                .iter()
                .any(|c| p.starts_with(c));
            if correction || floor::addressed(&text, &self.agent_names(), false) {
                self.cancel_request(&o, if correction { "corrected" } else { "joined" });
                let words = if correction {
                    format!("{} (correction: {text})", o.words)
                } else {
                    format!("{} {text}", o.words)
                };
                return self.request(&words, via, Some(&o.id));
            }
        }
        // 4. Asking about a permission request reads it back (then a yes or no answers it).
        let asks_permission = p.contains("permission")
            || (p.starts_with("what does ") && p.ends_with(" want"))
            || matches!(
                p.as_str(),
                "any requests"
                    | "what does it want"
                    | "what s waiting"
                    | "whats waiting"
                    | "what is waiting"
            );
        if asks_permission {
            heard_signal(self);
            return self.read_back();
        }
        // 5. Built-in phrases that need no model (AC-175).
        if let Some(r) = self.built_in(&p, &text, via) {
            return r;
        }
        // 6. Meant for the one being spoken to? Otherwise it stays in memory only.
        let s = match settings(&self.d) {
            Ok(s) => s,
            Err(e) => return json!({"taken": false, "why": e.to_string()}),
        };
        let awaiting = self.st.lock().unwrap().awaiting_answer;
        let to_agent = s.target != "overseer"
            && (p.split(' ').any(|w| w == "you" || w == "your")
                || (p.split(' ').count() >= 3 && p.split(' ').any(|w| floor::is_command_verb(w))));
        let meant = floor::addressed(&text, &self.agent_names(), awaiting) || to_agent;
        if !meant {
            let mut r = req().lock().unwrap();
            r.context.push_back(Heard {
                at: Instant::now(),
                text: text.clone(),
            });
            while r.context.len() > 30
                || r.context
                    .front()
                    .is_some_and(|h| h.at.elapsed() > Duration::from_secs(600))
            {
                r.context.pop_front();
            }
            self.emit(json!({"kind": "not_meant", "text": text}));
            return json!({"taken": false, "why": "not meant for Overseer"});
        }
        self.request(&text, via, None)
    }

    /// A request: recorded, taken with a quick answer, and sent to Overseer (or to the agent).
    fn request(self: &Arc<Self>, words: &str, via: &str, replaces: Option<&str>) -> Value {
        let d = self.d.clone();
        let s = match settings(&d) {
            Ok(s) => s,
            Err(e) => return json!({"taken": false, "why": e.to_string()}),
        };
        // At most `requests_per_hour` requests; then built-in phrases only (AC-173).
        let recent: i64 = d
            .store
            .lock()
            .unwrap()
            .conn
            .query_row(
                "SELECT COUNT(*) FROM voice_requests WHERE ts > ?1 AND kind='request'",
                [crate::daemon::now() - 3_600_000],
                |r| r.get(0),
            )
            .unwrap_or(0);
        if recent as u64 >= s.requests_per_hour {
            let say = {
                let mut r = req().lock().unwrap();
                let say = r
                    .limit_said
                    .is_none_or(|t| t.elapsed() > Duration::from_secs(600));
                if say {
                    r.limit_said = Some(Instant::now());
                }
                say
            };
            if say {
                speak_when_free(self, "That's this hour's limit for spoken requests. Stop, mute and what's running still work.");
            }
            return json!({"taken": false, "why": "the hour's limit"});
        }
        let id = next_id(&d);
        let target = s.target.clone();
        if let Err(e) = record(&d, &id, words, via, &target, "taken", "request") {
            return json!({"taken": false, "why": e.to_string()});
        }
        if let Some(old) = replaces {
            update(&d, old, "superseded_by", &id);
            announce(self, old);
        }
        heard_signal(self);
        // The quick answer: at once, with no model (AC-165).
        self.say_line("On it.");
        req().lock().unwrap().open.push(Open {
            id: id.clone(),
            words: words.to_string(),
            at: Instant::now(),
            proposal: None,
            settle_until: None,
        });
        self.st.lock().unwrap().thinking += 1;
        self.refresh();
        // Words heard before that were not requests go along as context, once.
        let context: Vec<String> = req()
            .lock()
            .unwrap()
            .context
            .drain(..)
            .map(|h| h.text)
            .collect();
        // Talking to one agent: the words go to it as they are, with no model turn (AC-166).
        if target != "overseer" && !plain(words).split(' ').any(|w| w == "overseer") {
            let action = json!([{"action": "message", "agent": target, "text": words, "why": "the owner is talking to it", "confidence": "high"}]);
            let _ = d.overseer_set_cause("voice");
            return match d.overseer_propose(&action, "voice") {
                Ok(r) => {
                    json!({"taken": true, "request": id, "direct": true, "proposal": r["proposal"]})
                }
                Err(e) => {
                    self.close(&id, "not_sent", Some(&format!("Not sent: {e}")));
                    json!({"taken": true, "request": id, "state": "not_sent", "why": e.to_string()})
                }
            };
        }
        let focus = self.st.lock().unwrap().focus.clone();
        let mut message = String::new();
        if target != "overseer" {
            let title = d
                .run(&target)
                .map(|r| r.title)
                .unwrap_or_else(|_| target.clone());
            message.push_str(&format!("(The owner is talking to {title} ({target}) directly: unless they name you, message only that agent, with their words as they are.)\n"));
        }
        if let Some(f) = focus {
            message.push_str(&format!(
                "(The owner has {} selected.)\n",
                d.run(&f).map(|r| format!("{} ({f})", r.title)).unwrap_or(f)
            ));
        }
        if !context.is_empty() {
            message.push_str(&format!(
                "(Earlier, not addressed to you: “{}”.)\n",
                context.join("” “")
            ));
        }
        message.push_str(&format!("{VOICE_NOTE}\nRequest {id}: {words}"));
        update(&d, &id, "state", "thinking");
        announce(self, &id);
        match d.overseer_send(&message, "voice", None, None) {
            Ok(r) => json!({"taken": true, "request": id, "queued": r["queued"]}),
            Err(e) => {
                // Overseer cannot be reached: said once, kept as not sent, never sent later (AC-175).
                self.close(&id, "not_sent", Some(&format!("Not sent: {e}")));
                speak_when_free(
                    self,
                    "I can't reach Overseer right now, so nothing was sent.",
                );
                json!({"taken": true, "request": id, "state": "not_sent", "why": e.to_string()})
            }
        }
    }

    fn close(self: &Arc<Self>, id: &str, state: &str, done: Option<&str>) {
        update(&self.d, id, "state", state);
        if let Some(t) = done {
            update(&self.d, id, "done", t);
        }
        let was_open = {
            let mut r = req().lock().unwrap();
            let before = r.open.len();
            r.open.retain(|o| o.id != id);
            before != r.open.len()
        };
        if was_open {
            let mut st = self.st.lock().unwrap();
            st.thinking = st.thinking.saturating_sub(1);
        }
        announce(self, id);
        self.refresh();
    }

    fn cancel_request(self: &Arc<Self>, o: &Open, how: &str) -> Value {
        if let Some(p) = &o.proposal {
            if let Err(e) = self.d.overseer_cancel(p, "owner (voice)") {
                return json!({"cancelled": false, "why": e.to_string()});
            }
        }
        self.close(&o.id, how, Some("Cancelled: nothing was sent."));
        if how == "cancelled" {
            self.say_line("Cancelled.");
        }
        json!({"cancelled": true, "request": o.id, "how": how})
    }

    /// Phrases that work with no model at all (AC-175): stop an agent by name, stop everyone,
    /// mute, what's running.
    fn built_in(self: &Arc<Self>, p: &str, text: &str, via: &str) -> Option<Value> {
        let d = self.d.clone();
        if matches!(p, "mute" | "stop listening" | "mute yourself") {
            let _ = super::set(&d, &json!({"muted": true}));
            return Some(json!({"taken": true, "built_in": "mute"}));
        }
        if matches!(
            p,
            "what s running"
                | "whats running"
                | "what is running"
                | "who s running"
                | "whos running"
                | "who is running"
        ) {
            let active: Vec<String> = d
                .roster()
                .unwrap_or_default()
                .into_iter()
                .filter(|l| crate::daemon::ACTIVE.contains(&l.status.as_str()))
                .map(|l| l.title)
                .collect();
            let line = match active.len() {
                0 => "Nothing is running.".to_string(),
                1 => format!("One agent is running: {}.", active[0]),
                n => format!(
                    "{n} agents are running: {} and {}.",
                    active[..n - 1].join(", "),
                    active[n - 1]
                ),
            };
            heard_signal(self);
            speak_when_free(self, &line);
            return Some(json!({"taken": true, "built_in": "running", "said": line}));
        }
        let everyone = matches!(
            p,
            "stop everyone"
                | "stop everything"
                | "stop all"
                | "stop all agents"
                | "everyone stop"
                | "everybody stop"
                | "stop everybody"
        );
        let one = p.strip_prefix("stop ").map(|rest| {
            rest.trim_start_matches("the ")
                .trim_end_matches(" agent")
                .to_string()
        });
        let roster = d.roster().unwrap_or_default();
        let targets: Vec<(String, String)> = if everyone {
            roster
                .iter()
                .filter(|l| crate::daemon::ACTIVE.contains(&l.status.as_str()))
                .map(|l| (l.id.clone(), l.title.clone()))
                .collect()
        } else if let Some(name) = one.filter(|n| !n.is_empty()) {
            roster
                .iter()
                .filter(|l| {
                    crate::daemon::ACTIVE.contains(&l.status.as_str())
                        && plain(&l.title).contains(&name)
                })
                .map(|l| (l.id.clone(), l.title.clone()))
                .collect()
        } else {
            return None;
        };
        if targets.is_empty() {
            if everyone {
                speak_when_free(self, "Nothing is running.");
                return Some(json!({"taken": true, "built_in": "stop", "stopped": []}));
            }
            return None; // not an agent's name: let Overseer read it
        }
        let id = next_id(&d);
        let _ = record(&d, &id, text, via, "overseer", "taken", "request");
        heard_signal(self);
        let actions: Vec<Value> = targets.iter().map(|(run, _)| json!({"action": "stop", "agent": run, "why": if everyone { "everyone" } else { "named" }, "confidence": "high"})).collect();
        let _ = d.overseer_set_cause("voice");
        let result = d.overseer_propose(&json!(actions), "voice");
        let names: Vec<String> = targets.iter().map(|(_, t)| t.clone()).collect();
        match result {
            Ok(r) => {
                update(&d, &id, "proposal", r["proposal"].as_str().unwrap_or(""));
                self.close(
                    &id,
                    "done",
                    Some(r["result"].as_str().unwrap_or("Stopped.")),
                );
                speak_when_free(self, &format!("Stopped {}.", join_names(&names)));
                Some(json!({"taken": true, "built_in": "stop", "request": id, "stopped": names}))
            }
            Err(e) => {
                self.close(&id, "not_sent", Some(&format!("Not done: {e}")));
                Some(
                    json!({"taken": true, "built_in": "stop", "request": id, "error": e.to_string()}),
                )
            }
        }
    }

    // ------------------------------------------------------------------ permissions by voice

    /// Reads the oldest waiting permission request back and waits for a yes or no.
    pub fn read_back(self: &Arc<Self>) -> Value {
        let d = self.d.clone();
        let s = settings(&d).unwrap_or_default();
        let waiting: Option<(String, String, String, String)> = d.roster().ok().and_then(|r| {
            r.into_iter()
                .filter(|l| l.status == "waiting_for_user")
                .find_map(|l| {
                    let run = d.run(&l.id).ok()?;
                    let att = run.attention.clone()?;
                    (att["kind"] == "permission").then(|| {
                        (
                            l.id.clone(),
                            att["request_id"].as_str().unwrap_or("").to_string(),
                            l.title.clone(),
                            summarize(&att),
                        )
                    })
                })
        });
        let Some((run, request, title, what)) = waiting else {
            speak_when_free(self, "Nothing is waiting for your permission.");
            return json!({"read_back": null});
        };
        if !s.permission_answers {
            speak_when_free(
                self,
                &format!(
                    "{title} wants to {what}. Answer it in VS Code: answering by voice is off."
                ),
            );
            return json!({"read_back": null, "why": "answering by voice is off"});
        }
        req().lock().unwrap().read_back = Some(ReadBack {
            run: run.clone(),
            request: request.clone(),
            title: title.clone(),
            at: Instant::now(),
        });
        self.st.lock().unwrap().awaiting_answer = true;
        let line = format!("{title} wants to {what}. Allow?");
        speak_when_free(self, &line);
        self.emit(json!({"kind": "read_back", "agent": run, "title": title, "what": what}));
        json!({"read_back": {"agent": run, "request": request, "said": line}})
    }

    fn answer_permission(self: &Arc<Self>, rb: &ReadBack, allow: bool) -> Value {
        let d = self.d.clone();
        let s = settings(&d).unwrap_or_default();
        let id = next_id(&d);
        let words = if allow {
            "yes, allow it"
        } else {
            "no, deny it"
        };
        let _ = record(&d, &id, words, "voice", &rb.run, "settling", "permission");
        {
            let mut r = req().lock().unwrap();
            r.read_back = None;
            r.answering = Some(Answering {
                id: id.clone(),
                run: rb.run.clone(),
                request: rb.request.clone(),
                allow,
                title: rb.title.clone(),
                until: Instant::now() + Duration::from_secs(s.settle_seconds),
            });
        }
        self.st.lock().unwrap().awaiting_answer = false;
        // No doubt it was taken: a cue under Audio Mode's rules, and a toast with Cancel.
        crate::audio::cue(
            &d,
            if allow {
                "agent_unblocked"
            } else {
                "agent_stopped"
            },
        );
        self.emit(json!({"kind": "toast", "request": id, "text": format!("{}: {} for {}", if allow { "Allowed" } else { "Denied" }, "the request", rb.title), "cancel": true, "seconds": s.settle_seconds}));
        announce(self, &id);
        json!({"taken": true, "request": id, "allow": allow, "window_s": s.settle_seconds})
    }

    fn cancel_answer(self: &Arc<Self>, by: &str) -> Value {
        let a = req().lock().unwrap().answering.take();
        let Some(a) = a else {
            return json!({"cancelled": false, "why": "nothing to cancel"});
        };
        update(&self.d, &a.id, "state", "cancelled");
        update(
            &self.d,
            &a.id,
            "done",
            "Cancelled: the request still waits for your answer.",
        );
        self.emit(json!({"kind": "toast", "request": a.id, "text": format!("Cancelled: {} still waits for your answer", a.title), "cancel": false, "by": by}));
        announce(self, &a.id);
        self.say_line("Cancelled.");
        json!({"cancelled": true, "request": a.id})
    }
}

fn summarize(att: &Value) -> String {
    let tool = att["tool"].as_str().unwrap_or("do something");
    let input = &att["input"];
    let detail = input["command"]
        .as_str()
        .or(input["file_path"].as_str())
        .or(input["path"].as_str())
        .map(|s| s.chars().take(80).collect::<String>());
    match (tool, detail) {
        ("Bash" | "bash" | "shell" | "exec_command" | "command", Some(c)) => format!("run {c}"),
        ("Edit" | "Write" | "edit" | "write" | "apply_patch", Some(p)) => {
            format!("change {}", p.rsplit('/').next().unwrap_or(&p))
        }
        (t, Some(x)) => format!("use {t} on {x}"),
        (t, None) => format!("use {t}"),
    }
}

fn join_names(names: &[String]) -> String {
    match names.len() {
        0 => String::new(),
        1 => names[0].clone(),
        n => format!("{} and {}", names[..n - 1].join(", "), names[n - 1]),
    }
}

/// How long Overseer has to answer a spoken request (a plan or a reply) before it counts as not
/// reachable: 45 s (`OVERSEER_VOICE_ANSWER_S` in tests).
fn answer_limit() -> Duration {
    Duration::from_secs(
        std::env::var("OVERSEER_VOICE_ANSWER_S")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(45),
    )
}

/// Every 100 ms: permission answers whose window has passed go to the agent; read-backs that
/// waited 20 s lapse; a request Overseer has not answered in time is not sent (AC-175).
fn tick(v: &Arc<Voice>) {
    let late: Vec<Open> = req()
        .lock()
        .unwrap()
        .open
        .iter()
        .filter(|o| o.proposal.is_none() && o.at.elapsed() > answer_limit())
        .cloned()
        .collect();
    for o in late {
        let answered = row(&v.d, &o.id).is_some_and(|r| r["answer"].is_string());
        if answered {
            v.close(&o.id, "answered", None);
            continue;
        }
        v.close(
            &o.id,
            "not_sent",
            Some("Not sent: Overseer did not answer. Nothing will be sent later."),
        );
        let say = {
            let mut r = req().lock().unwrap();
            let say = r
                .unreachable_said
                .is_none_or(|t| t.elapsed() > Duration::from_secs(600));
            if say {
                r.unreachable_said = Some(Instant::now());
            }
            say
        };
        if say {
            speak_when_free(v, "Overseer isn't answering, so nothing was sent. Stop, mute and what's running still work.");
        }
    }
    let due = {
        let mut r = req().lock().unwrap();
        if r.read_back
            .as_ref()
            .is_some_and(|rb| rb.at.elapsed() > Duration::from_secs(20))
        {
            r.read_back = None;
            drop(r);
            v.st.lock().unwrap().awaiting_answer = false;
            v.emit(json!({"kind": "read_back", "lapsed": true}));
            return;
        }
        match &r.answering {
            Some(a) if Instant::now() >= a.until => r.answering.take(),
            _ => None,
        }
    };
    if let Some(a) = due {
        let result = v.d.answer_permission(
            &a.run,
            &a.request,
            a.allow,
            if a.allow {
                "Allowed by voice"
            } else {
                "Denied by voice"
            },
        );
        let (state, text) = match result {
            Ok(_) => (
                "sent",
                format!(
                    "Sent: {} {}",
                    if a.allow { "allowed" } else { "denied" },
                    a.title
                ),
            ),
            Err(e) => ("not_sent", format!("Not sent: {e}")),
        };
        update(&v.d, &a.id, "state", state);
        update(&v.d, &a.id, "done", &text);
        v.emit(json!({"kind": "toast", "request": a.id, "text": text, "cancel": false}));
        announce(v, &a.id);
    }
}

/// Overseer's session moved: link proposals to spoken requests, speak the plan and the outcome,
/// and read Overseer's replies out.
fn on_session_event(v: &Arc<Voice>, kind: &str, p: &Value, run_id: Option<&str>) {
    match kind {
        "proposal" if p["cause"] == "voice" || p["via"] == "voice" => {
            let id = p["id"].as_str().unwrap_or("").to_string();
            let settle_until = p["settle_until"].as_i64();
            let open = {
                let mut r = req().lock().unwrap();
                let o = r.open.iter_mut().find(|o| o.proposal.is_none());
                o.map(|o| {
                    o.proposal = Some(id.clone());
                    o.settle_until = settle_until.map(|t| {
                        Instant::now()
                            + Duration::from_millis((t - crate::daemon::now()).max(0) as u64)
                    });
                    o.clone()
                })
            };
            if let Some(o) = open {
                update(&v.d, &o.id, "proposal", &id);
                update(
                    &v.d,
                    &o.id,
                    "state",
                    if p["state"] == "settling" {
                        "settling"
                    } else {
                        "waiting"
                    },
                );
                announce(v, &o.id);
                let line = plan_line(&v.d, p);
                if p["state"] == "open" && p["confirm"] == true {
                    v.st.lock().unwrap().awaiting_answer = true;
                    speak_when_free(v, &format!("{line} Say yes to go ahead."));
                } else if !line.is_empty() {
                    speak_when_free(v, &line);
                }
            }
        }
        "proposal_answered" => {
            let id = p["id"].as_str().unwrap_or("");
            let o = req()
                .lock()
                .unwrap()
                .open
                .iter()
                .find(|o| o.proposal.as_deref() == Some(id))
                .cloned();
            if let Some(o) = o {
                let state = p["state"].as_str().unwrap_or("");
                let result = p["result"].as_str().unwrap_or("");
                let (req_state, line) = match state {
                    "yes" if result.contains("failed") => (
                        "partly_sent",
                        "Some of it could not be sent; the card says what.",
                    ),
                    "yes" => ("sent", "Sent."),
                    "cancelled" => ("cancelled", ""),
                    "stale" => ("not_sent", "Not sent: an agent changed in the meantime."),
                    "no" => ("not_sent", "Not sent."),
                    _ => ("", ""),
                };
                if !req_state.is_empty() {
                    v.close(&o.id, req_state, Some(result));
                    if !line.is_empty() {
                        speak_when_free(v, line);
                    }
                }
            }
        }
        "overseer_message" => {
            let m = &p["message"];
            if m["source"] != "overseer" {
                return;
            }
            let text = m["text"].as_str().unwrap_or("").trim().to_string();
            let o = req().lock().unwrap().open.first().cloned();
            let Some(o) = o else { return };
            update(&v.d, &o.id, "answer", &text);
            if text.contains("NOT_FOR_OVERSEER") {
                v.close(&o.id, "not_for_overseer", None);
                return;
            }
            if o.proposal.is_none() {
                // An answer with no plan (a question about the agents): read out, briefly.
                let short = first_sentences(&text, 2);
                if short.ends_with('?') {
                    v.st.lock().unwrap().awaiting_answer = true;
                }
                if !short.is_empty() {
                    speak_when_free(v, &short);
                }
            }
            announce(v, &o.id);
        }
        "turn_completed" | "turn_failed" | "status" => {
            // When Overseer's turn ends, a request with no plan is answered.
            let Some(run) = run_id else { return };
            if v.d.run_role(run) != "overseer" {
                return;
            }
            if kind == "status"
                && !matches!(
                    p["status"].as_str(),
                    Some("completed" | "failed" | "interrupted")
                )
            {
                return;
            }
            let done: Vec<Open> = req()
                .lock()
                .unwrap()
                .open
                .iter()
                .filter(|o| o.proposal.is_none() && o.at.elapsed() > Duration::from_millis(200))
                .cloned()
                .collect();
            for o in done {
                let answered =
                    row(&v.d, &o.id).and_then(|r| r["answer"].as_str().map(String::from));
                let failed = kind == "turn_failed" || (kind == "status" && p["status"] == "failed");
                if failed && answered.is_none() {
                    v.close(
                        &o.id,
                        "not_sent",
                        Some("Overseer's turn failed; nothing was sent."),
                    );
                    speak_when_free(v, "Overseer could not work on that; nothing was sent.");
                } else {
                    v.close(&o.id, "answered", None);
                }
            }
        }
        _ => {}
    }
}

/// The agent the owner was talking to finished or was archived: back to Overseer, said once.
fn target_gone(v: &Arc<Voice>, kind: &str, run: Option<&str>, p: &Value) {
    let Ok(s) = settings(&v.d) else { return };
    if s.target == "overseer" {
        return;
    }
    let gone = match kind {
        "status" => {
            run == Some(s.target.as_str())
                && matches!(
                    p["status"].as_str(),
                    Some("completed" | "failed" | "interrupted")
                )
        }
        // "task_archived" carries the task: the chosen agent's task was archived.
        _ => {
            p["archived"] == true
                && v.d
                    .run(&s.target)
                    .map(|r| Some(r.task_id.as_str()) == run)
                    .unwrap_or(false)
        }
    };
    if !gone {
        return;
    }
    let title = v.d.run(&s.target).map(|r| r.title).unwrap_or_default();
    let _ = super::set_meta(&v.d, "voice.target", "overseer");
    v.emit(json!({"kind": "target", "target": "overseer", "why": format!("{title} finished")}));
    speak_when_free(v, &format!("Back to Overseer: {title} finished."));
}

/// What the plan does, said in one line (the daemon's words, from the checked actions).
fn plan_line(d: &Daemon, p: &Value) -> String {
    let mut by_verb: Vec<(&str, Vec<String>)> = Vec::new();
    let mut new_agents = 0;
    for a in p["actions"].as_array().cloned().unwrap_or_default() {
        let verb = match a["action"].as_str().unwrap_or("") {
            "message" | "share" => "Telling",
            "redirect" => "Redirecting",
            "stop" => "Stopping",
            "hold" => "Holding",
            "release" => "Releasing",
            "report" => "Asking for a report from",
            "answer" => "Answering",
            "pin" => "Pinning",
            "watch" => "Setting a watch on",
            "archive" => "Archiving",
            "start" => {
                new_agents += 1;
                continue;
            }
            _ => "Acting on",
        };
        let who = a["title"]
            .as_str()
            .map(String::from)
            .or_else(|| {
                a["agent"]
                    .as_str()
                    .and_then(|id| d.run(id).ok().map(|r| r.title))
            })
            .unwrap_or_default();
        match by_verb.iter_mut().find(|(v, _)| *v == verb) {
            Some((_, names)) => names.push(who),
            None => by_verb.push((verb, vec![who])),
        }
    }
    let mut parts: Vec<String> = by_verb
        .into_iter()
        .map(|(verb, names)| format!("{verb} {}", join_names(&names)))
        .collect();
    match new_agents {
        0 => {}
        1 => parts.push("starting one agent".into()),
        n => parts.push(format!("starting {n} agents")),
    }
    if parts.is_empty() {
        return String::new();
    }
    let mut line = parts.join(", and ");
    line.push('.');
    let mut chars = line.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
        None => line,
    }
}

fn first_sentences(text: &str, n: usize) -> String {
    let mut out = String::new();
    let mut count = 0;
    for (i, c) in text.char_indices() {
        if matches!(c, '.' | '?' | '!') && text[i + 1..].starts_with(|n: char| n.is_whitespace())
            || i + c.len_utf8() == text.len()
        {
            count += 1;
            if count == n || i + c.len_utf8() == text.len() {
                out = text[..i + c.len_utf8()].to_string();
                break;
            }
        }
    }
    let out = if out.is_empty() {
        text.to_string()
    } else {
        out
    };
    // No paths, identifiers, code or markdown read aloud.
    let out = out
        .split("```")
        .next()
        .unwrap_or("")
        .replace("**", "")
        .replace("__", "");
    out.split_whitespace()
        .filter(|w| {
            !w.contains('/') && !w.contains("::") && !w.starts_with('`') && *w != "-" && *w != "*"
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The owner's words quoted in each message a spoken request sends, with the other agents told
/// (AC-169); more new agents than the owner's limit need a yes; the owner's delivery setting wins
/// over the model's choice of add or redirect (AC-167, AC-168).
pub fn decorate(d: &Daemon, actions: &mut [Value]) -> Result<bool> {
    let s = settings(d)?;
    let open = req()
        .lock()
        .unwrap()
        .open
        .iter()
        .find(|o| o.proposal.is_none())
        .cloned();
    let (id, words) = match &open {
        Some(o) => (o.id.clone(), o.words.clone()),
        None => (String::new(), String::new()),
    };
    let starts = actions.iter().filter(|a| a["action"] == "start").count() as u64;
    if starts > 8 {
        bail!("at most eight new agents from one request");
    }
    let needs_yes = starts > s.new_agents_per_request;
    let names: Vec<(String, String)> = actions
        .iter()
        .filter_map(|a| {
            let who = a["title"].as_str().map(String::from).or_else(|| {
                a["repo"]
                    .as_str()
                    .map(|r| format!("a new agent in {}", r.rsplit('/').next().unwrap_or(r)))
            })?;
            Some((a["agent"].as_str().unwrap_or("").to_string(), who))
        })
        .collect();
    for a in actions.iter_mut() {
        let kind = a["action"].as_str().unwrap_or("").to_string();
        // The owner's delivery setting.
        if kind == "redirect" && s.delivery == "add" {
            a["action"] = json!("message");
        } else if kind == "message" && s.delivery == "redirect" {
            let working = a["agent"]
                .as_str()
                .and_then(|id| d.run(id).ok())
                .is_some_and(|r| r.status == "running");
            if working {
                a["action"] = json!("redirect");
            }
        }
        if a["confidence"] == "low" {
            bail!("a low-confidence action is not sent: ask the owner one short question instead");
        }
        let text_key = if a["action"] == "start" {
            "prompt"
        } else {
            "text"
        };
        if matches!(a["action"].as_str(), Some("message" | "redirect" | "start"))
            && !words.is_empty()
        {
            let me = a["agent"].as_str().unwrap_or("").to_string();
            let others: Vec<String> = names
                .iter()
                .filter(|(id, _)| id != &me || me.is_empty())
                .map(|(_, n)| n.clone())
                .filter(|n| a["title"].as_str() != Some(n.as_str()))
                .collect();
            let own = a[text_key].as_str().unwrap_or("").to_string();
            let mut full = if own.trim() == words.trim() {
                format!("(voice, request {id}) The owner said: “{words}”")
            } else {
                format!("(voice, request {id}) The owner said: “{words}”\nFor you: {own}")
            };
            if !others.is_empty() {
                full.push_str(&format!("\nAlso told: {}.", others.join(", ")));
            }
            a[text_key] = json!(full);
        }
    }
    Ok(needs_yes)
}

/// Medium confidence waits longer, with every target named (AC-166).
pub fn settle_ms(d: &Daemon, actions: &[Value]) -> i64 {
    let base = settings(d).map(|s| s.settle_seconds).unwrap_or(2) as i64 * 1000;
    if actions.iter().any(|a| a["confidence"] == "medium") {
        base.max(4000)
    } else {
        base
    }
}

/// Overseer's voice ended a line: nothing to do yet (kept for the done line's order).
pub fn spoke_done(_v: &Arc<Voice>, _line: u64, _event: &str) {}

/// `voice.requests`: the latest requests, or those whose words contain `query`.
pub fn list(d: &Arc<Daemon>, p: &Value) -> Result<Value> {
    let limit = p["limit"].as_u64().unwrap_or(50).min(500) as i64;
    let query = p["query"]
        .as_str()
        .map(|q| format!("%{}%", q.to_lowercase()));
    let store = d.store.lock().unwrap();
    let mut out = Vec::new();
    let sql = "SELECT id FROM voice_requests WHERE (?1 IS NULL OR lower(words) LIKE ?1) ORDER BY ts DESC LIMIT ?2";
    let mut stmt = store.conn.prepare(sql)?;
    let ids: Vec<String> = stmt
        .query_map(rusqlite::params![query, limit], |r| r.get(0))?
        .flatten()
        .collect();
    drop(stmt);
    drop(store);
    for id in ids {
        if let Some(r) = row(d, &id) {
            out.push(r);
        }
    }
    Ok(json!({"requests": out}))
}

/// `voice.cancel`: cancels a request inside its window, or a permission answer inside its window.
pub fn cancel(_d: &Arc<Daemon>, p: &Value) -> Result<Value> {
    let v = super::voice().ok_or_else(|| anyhow!("the voice session is not running"))?;
    let id = p["id"].as_str().unwrap_or("");
    if req()
        .lock()
        .unwrap()
        .answering
        .as_ref()
        .is_some_and(|a| a.id == id || id.is_empty())
    {
        return Ok(v.cancel_answer("toast"));
    }
    let o = req()
        .lock()
        .unwrap()
        .open
        .iter()
        .find(|o| o.id == id)
        .cloned();
    match o {
        Some(o) => Ok(v.cancel_request(&o, "cancelled")),
        None => bail!("{id} is not waiting: it has been sent or closed"),
    }
}

/// `voice.read_back`: reads the oldest waiting permission request back (also asked by voice).
pub fn read_back(_d: &Arc<Daemon>, _p: &Value) -> Result<Value> {
    let v = super::voice().ok_or_else(|| anyhow!("the voice session is not running"))?;
    Ok(v.read_back())
}
