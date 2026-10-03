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

/// What Overseer is told about a spoken request that may not have been meant for it: it judges,
/// and nothing has been said yet (AC-228).
const VOICE_NOTE: &str = "(Spoken aloud to you in Voice Mode. Answer in one or two short sentences: they are read out. If this was not meant for you, reply exactly NOT_FOR_OVERSEER and propose nothing. When you message agents, write only what each should do; the daemon adds the owner's words. Give each action a confidence of high, medium or low; when low, ask one short question instead of proposing. Never name a request's id (V-…) to the owner: say \"your request\".)";
/// What Overseer is told about a spoken request that was surely for it (it names Overseer or gives
/// a command): the daemon has already said "On it.", so Overseer answers or acts, and asks one
/// short question when it cannot place what is meant (AC-228, AC-229).
const VOICE_NOTE_TAKEN: &str = "(Spoken aloud to you in Voice Mode, and meant for you: answer it or act on it. Answer in one or two short sentences: they are read out. When you message agents, write only what each should do; the daemon adds the owner's words. Give each action a confidence of high, medium or low; when low, ask one short question instead of proposing. Never name a request's id (V-…) to the owner: say \"your request\".)";

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
    /// The daemon's candidates for it, with their reasons (AC-166).
    pub candidates: Vec<(String, &'static str)>,
    /// A Confirm-tier plan read back and waiting for a yes by voice since then (AC-171).
    pub confirm_at: Option<Instant>,
    /// "Still working on it." was said (once at most, AC-165).
    pub holding_said: bool,
    /// When Overseer's turn about it began (it may wait behind a turn Overseer started itself).
    pub turn_seen: Option<Instant>,
    /// Sent straight to the agent the owner is talking to, with no Overseer turn (AC-166).
    pub direct: bool,
    /// The sent request this one corrects (a correction after the send, AC-170).
    pub replaces_sent: Option<String>,
    /// Only probably meant for Overseer: nothing was said, and Overseer judges (AC-228).
    pub weak: bool,
    /// It names Overseer and gives an instruction: Overseer's "not for me" never drops it; it is
    /// asked again, once, as surely its (AC-229).
    pub named: bool,
    pub asked_again: bool,
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
    /// Its spoken answer window ended; retain identity until it is answered elsewhere.
    lapsed_read_back: Option<ReadBack>,
    answering: Option<Answering>,
    /// When "that's the hour's limit" and "Overseer isn't answering" were last said: once each.
    limit_said: Option<Instant>,
    unreachable_said: Option<Instant>,
    /// The agents the last spoken request reached ("tell them also").
    last_targets: Vec<String>,
    /// The last request that went out, for a correction after the send (AC-170): id, words, when.
    last_sent: Option<(String, String, Instant)>,
    /// The targets last announced, so the mark changes are sent once.
    targets_shown: Vec<String>,
    /// Requests taken while four were open: they begin as others close (AC-173).
    waiting: VecDeque<(String, String, String)>,
    /// A follow-up's id and the sent request it replaces, until it begins.
    replaces_pending: std::collections::HashMap<String, String>,
    /// Requests taken with no "On it." (only probably meant for Overseer), until they begin.
    weak_pending: std::collections::HashSet<String>,
}

/// At most four open requests at once (AC-173).
const MAX_OPEN: usize = 4;

/// One message to an agent, in characters (AC-173).
const MAX_MESSAGE: usize = 4000;

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
    // A permission read out by itself (AC-230) is answered as naturally as any question.
    "yes do that",
    "yes go ahead",
    "yes please do that",
    "ok do that",
    "okay do that",
    "sure go ahead",
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
    "tell it yes",
    "tell it to go ahead",
    "say yes",
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
    "tell it no",
    "say no",
];

/// A plan read back and waiting for a yes more recently than `than` (the answer is for it).
fn newer_plan(than: Instant) -> bool {
    req()
        .lock()
        .unwrap()
        .open
        .iter()
        .any(|o| o.proposal.is_some() && o.confirm_at.is_some_and(|t| t > than))
}

/// Answering every permission at once is refused: one at a time (AC-171).
const ALL_AT_ONCE: &[&str] = &[
    "allow everything",
    "allow all",
    "allow them all",
    "allow all of them",
    "approve everything",
    "approve all",
    "approve them all",
    "yes to everything",
    "yes to all",
    "accept all",
    "accept everything",
    "allow everything from now on",
];

/// How long a read-back or a Confirm plan waits for a yes by voice: 20 s
/// (`OVERSEER_VOICE_CONFIRM_S` in tests).
fn confirm_limit() -> Duration {
    Duration::from_secs(
        std::env::var("OVERSEER_VOICE_CONFIRM_S")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(20),
    )
}

/// When "Still working on it." is said if Overseer has not answered: 8 s after the request
/// (`OVERSEER_VOICE_HOLDING_MS` in tests). "On it." is said at once by the daemon, so the holding
/// line comes later than the side RFC's 2.5 s (the spike's revision, AC-162).
fn holding_after() -> Duration {
    Duration::from_millis(
        std::env::var("OVERSEER_VOICE_HOLDING_MS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(8000),
    )
}

/// Things that are not done by voice (AC-171): the place in the UI that does them.
fn not_by_voice(p: &str) -> Option<(&'static str, &'static str)> {
    let has = |w: &str| p.split(' ').any(|x| x == w);
    let phrase = |w: &str| format!(" {p} ").contains(&format!(" {w} "));
    if phrase("sign in")
        || phrase("sign me in")
        || phrase("sign out")
        || phrase("log in")
        || phrase("log out")
        || phrase("log me in")
        || (has("account") || has("accounts"))
            && (has("add") || has("remove") || has("switch") || has("change") || has("new"))
    {
        return Some(("accounts", "Accounts"));
    }
    if has("phone") && (has("pair") || has("unpair") || phrase("phone access") || has("pairing")) {
        return Some(("phone", "Phone access"));
    }
    if has("continuity")
        && (has("download") || has("install") || has("model") || has("models"))
        && !has("tell")
    {
        return Some(("continuity", "Continuity's settings"));
    }
    if (phrase("clean up") || has("cleanup"))
        && (has("workspace") || has("workspaces") || has("worktrees") || has("worktree"))
    {
        return Some(("cleanup", "Workspace cleanup"));
    }
    if (has("stop") || has("quit") || has("restart") || has("kill"))
        && (has("daemon") || phrase("overseer itself"))
    {
        return Some(("daemon", "the daemon's controls"));
    }
    if (has("change") || has("turn") || has("edit") || has("disable") || has("loosen"))
        && (has("rules") || has("tiers") || phrase("voice settings") || phrase("what voice may do"))
    {
        return Some(("rules", "Voice Mode's settings"));
    }
    None
}

/// The rolling context holds 10 minutes or 30 exchanges, whichever is less, in memory only (AC-173).
fn trim_context(ctx: &mut VecDeque<Heard>) {
    while ctx.len() > 30
        || ctx
            .front()
            .is_some_and(|h| h.at.elapsed() > Duration::from_secs(600))
    {
        ctx.pop_front();
    }
}

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
                "proposal" | "proposal_answered" | "overseer_message" | "overseer_turn_processed" => {
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
            let _in_runtime = v.rt.as_ref().map(|h| h.enter());
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
    // The agents targeted by open requests carry a voice mark in the side bar and the grid.
    let now = targeted(&v.d);
    let changed = {
        let mut r = req().lock().unwrap();
        let changed = r.targets_shown != now;
        r.targets_shown = now.clone();
        changed
    };
    if changed {
        v.emit(json!({"kind": "targets", "runs": now}));
    }
}

/// The agents that open spoken requests are for (their plans settle or wait for a yes, AC-169).
pub fn targeted(d: &Daemon) -> Vec<String> {
    let proposals: Vec<String> = req()
        .lock()
        .unwrap()
        .open
        .iter()
        .filter_map(|o| o.proposal.clone())
        .collect();
    let mut out: Vec<String> = Vec::new();
    for p in proposals {
        if let Ok(card) = d.card(&p) {
            for a in card["actions"].as_array().into_iter().flatten() {
                if let Some(id) = a["agent"].as_str() {
                    if !out.iter().any(|x| x == id) {
                        out.push(id.to_string());
                    }
                }
            }
        }
    }
    out
}

/// Lines waiting for a free floor (the speech queue, AC-173).
static WAITING_LINES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// How long a line waits for a free floor before it goes to the card alone: 20 s
/// (`OVERSEER_VOICE_HOLD_S` in tests).
fn hold_limit() -> Duration {
    Duration::from_secs(
        std::env::var("OVERSEER_VOICE_HOLD_S")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(20),
    )
}

/// Speaks a line when the floor is free: never while the owner speaks; after 20 s it goes to the
/// card alone (AC-164); with three lines already waiting, a fourth goes to the card at once.
fn speak_when_free(v: &Arc<Voice>, text: &str) {
    use std::sync::atomic::Ordering;
    if WAITING_LINES.fetch_add(1, Ordering::SeqCst) >= 3 {
        WAITING_LINES.fetch_sub(1, Ordering::SeqCst);
        v.emit(json!({"kind": "spoke", "event": "card_only", "text": text, "why": "three lines are waiting"}));
        return;
    }
    let v = v.clone();
    let text = text.to_string();
    std::thread::spawn(move || {
        let start = Instant::now();
        while v.st.lock().unwrap().gate {
            if start.elapsed() > hold_limit() {
                WAITING_LINES.fetch_sub(1, Ordering::SeqCst);
                v.emit(json!({"kind": "spoke", "event": "card_only", "text": text, "why": "the owner kept talking"}));
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        WAITING_LINES.fetch_sub(1, Ordering::SeqCst);
        v.say_line(&text);
    });
}

/// Dedicated nonspoken feedback, separate from the twelve Audio Mode lines.
fn heard_signal(v: &Voice) {
    crate::audio::heard(&v.d);
    v.emit(json!({"kind": "heard_signal"}));
}

/// The agents as the candidates see them: active top-level agents, what they changed, and whether
/// they are waiting on the owner.
fn agents_for(d: &Arc<Daemon>) -> Vec<super::candidates::Agent> {
    d.roster()
        .unwrap_or_default()
        .into_iter()
        .filter(|l| l.role != "overseer" && l.role != "watcher")
        .map(|l| {
            let files = d
                .digest(&l.id)
                .map(|g| g.changed.into_iter().map(|c| c.path).collect())
                .unwrap_or_default();
            let task: String = d
                .store
                .lock()
                .unwrap()
                .turns(&l.id)
                .ok()
                .and_then(|t| t.first().map(|t| t.prompt.chars().take(400).collect()))
                .unwrap_or_default();
            super::candidates::Agent {
                active: crate::daemon::ACTIVE.contains(&l.status.as_str()),
                just_asked: l.status == "waiting_for_user" || l.waiting.is_some(),
                id: l.id,
                title: l.title,
                repository: l.repository,
                branch: l.branch,
                files,
                task,
            }
        })
        .collect()
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
        // Every permission at once is refused: one at a time, each read back (AC-171).
        if ALL_AT_ONCE
            .iter()
            .any(|a| p == *a || p.starts_with(&format!("{a} ")) || p.ends_with(&format!(" {a}")))
        {
            heard_signal(self);
            speak_when_free(
                self,
                "Not all at once. Ask me what's waiting, and I'll read each one back.",
            );
            return json!({"taken": false, "why": "all at once is refused"});
        }
        // What needs the owner ("handle what needs me", "tell it yes"): the same as typed, with no
        // model turn (AC-227); a permission is answered with the toast and its window (AC-171).
        // With a read-back waiting, a yes or a no is its answer (2, below).
        if let Some(ask) = crate::overseer::needs::ask(&text) {
            // The lock is let go before newer_plan takes it again.
            let pending = req().lock().unwrap().read_back.clone();
            let read_back_waits = pending.is_some_and(|rb| !newer_plan(rb.at));
            if ask == crate::overseer::needs::Ask::Handle || !read_back_waits {
                return self.needs(ask, &text, via);
            }
        }
        // A short reply that is neither a yes, a no nor a command ("maybe") is no answer; a command
        // said meanwhile ("stop Phone", "talk to Continuity") is still taken.
        let short_unclear = p.split(' ').count() <= 4
            && !CANCEL.contains(&p.as_str())
            && !floor::addressed(&text, &self.agent_names(), false);
        // 2. A read-back waiting for yes or no; anything unclear is no answer. When a plan was read
        // back after it, the answer is for the plan (the most recent question, 2b).
        let read_back = req().lock().unwrap().read_back.clone();
        let read_back = read_back.filter(|rb| !newer_plan(rb.at));
        if let Some(rb) = read_back {
            if YES.contains(&p.as_str()) || NO.contains(&p.as_str()) {
                return self.answer_permission(&rb, YES.contains(&p.as_str()));
            }
            if short_unclear {
                speak_when_free(self, "I need a clear yes or no.");
                return json!({"taken": false, "why": "unclear: no answer"});
            }
        }
        // 2b. A Confirm-tier plan read back and waiting for a yes by voice.
        let confirming = req()
            .lock()
            .unwrap()
            .open
            .iter()
            .filter(|o| o.proposal.is_some() && o.confirm_at.is_some_and(|t| t.elapsed() < confirm_limit()))
            .max_by_key(|o| o.confirm_at)
            .cloned();
        if let (Some(o), Some(p_id)) = (
            confirming.as_ref(),
            confirming.as_ref().and_then(|o| o.proposal.clone()),
        ) {
            let yes = YES.contains(&p.as_str());
            if yes || NO.contains(&p.as_str()) || CANCEL.contains(&p.as_str()) {
                if let Some(x) = req().lock().unwrap().open.iter_mut().find(|x| x.id == o.id) {
                    x.confirm_at = None;
                }
                self.st.lock().unwrap().awaiting_answer = false;
                heard_signal(self);
                return match self.d.overseer_answer(&p_id, yes, "voice", "owner (voice)") {
                    Ok(r) => {
                        json!({"taken": true, "request": o.id, "answered": yes, "result": r["result"]})
                    }
                    Err(e) => json!({"taken": false, "why": e.to_string()}),
                };
            }
            if short_unclear {
                speak_when_free(self, "I need a clear yes or no.");
                return json!({"taken": false, "why": "unclear: no answer"});
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
            if correction || !is_backchannel(&text) {
                self.cancel_request(&o, if correction { "corrected" } else { "joined" });
                let words = if correction {
                    format!("{} (correction: {text})", o.words)
                } else {
                    format!("{} {text}", o.words)
                };
                return self.request(&words, via, Some(&o.id));
            }
        }
        // 3b. A correction after the send: a follow-up to the same agents, naming the request it
        // replaces, which then reads superseded (AC-170).
        let is_correction = ["i meant", "i mean", "actually", "instead", "not "]
            .iter()
            .any(|c| p.starts_with(c));
        let last = req().lock().unwrap().last_sent.clone();
        if let Some((old, words, at)) =
            last.filter(|(_, _, at)| is_correction && at.elapsed() < Duration::from_secs(120))
        {
            let _ = at;
            // The follow-up (the next id) names the request it replaces.
            let next = next_id(&self.d);
            req()
                .lock()
                .unwrap()
                .replaces_pending
                .insert(next, old.clone());
            let r = self.request(
                &format!("{words} (correction after it was sent: {text})"),
                via,
                Some(&old),
            );
            update(&self.d, &old, "state", "superseded");
            announce(self, &old);
            return r;
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
        // 4b. Not by voice: the place in the UI is opened, and Overseer says so (AC-171).
        if let Some((place, name)) = not_by_voice(&p) {
            let id = next_id(&self.d);
            let _ = record(
                &self.d,
                &id,
                &text,
                via,
                "overseer",
                "not_sent",
                "not_by_voice",
            );
            update(
                &self.d,
                &id,
                "done",
                &format!("Not done by voice: {name} opened."),
            );
            heard_signal(self);
            self.emit(json!({"kind": "open", "place": place, "request": id}));
            speak_when_free(self, &format!("That isn't done by voice. I opened {name}."));
            announce(self, &id);
            return json!({"taken": true, "request": id, "not_by_voice": place});
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
        // "Yes, do that" / "go ahead" while an agent waits on the owner answers that agent (AC-166).
        let answers_an_agent = [
            "yes do that",
            "yes go ahead",
            "go ahead",
            "do that",
            "yes please do that",
            "ok do that",
            "okay do that",
            "sure go ahead",
        ]
        .iter()
        .any(|a| p == *a || p.starts_with(&format!("{a} ")))
            && agents_for(&self.d).iter().any(|a| a.just_asked);
        let meant =
            floor::addressed(&text, &self.agent_names(), awaiting) || to_agent || answers_an_agent;
        // Surely meant for the one spoken to: "On it." at once. Only probably: Overseer decides
        // first, and says nothing until it has (AC-228).
        let strong = floor::addressed_strongly(&text, awaiting) || to_agent || answers_an_agent || s.target != "overseer";
        if !meant {
            let mut r = req().lock().unwrap();
            r.context.push_back(Heard {
                at: Instant::now(),
                text: text.clone(),
            });
            trim_context(&mut r.context);
            self.emit(json!({"kind": "not_meant", "text": text}));
            return json!({"taken": false, "why": "not meant for Overseer"});
        }
        self.request_as(&text, via, None, strong)
    }

    /// A request: recorded, taken with a quick answer, and sent to Overseer (or to the agent).
    fn request(self: &Arc<Self>, words: &str, via: &str, replaces: Option<&str>) -> Value {
        self.request_as(words, via, replaces, true)
    }

    /// A request, said "On it." to at once only when it was surely meant for Overseer (`strong`).
    fn request_as(self: &Arc<Self>, words: &str, via: &str, replaces: Option<&str>, strong: bool) -> Value {
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
                speak_when_free(self, "That's this hour's limit for spoken requests.");
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
        // The quick answer: at once, with no model (AC-165), once it is decided the request is
        // Overseer's (AC-228). Otherwise Overseer's own answer or plan is the first thing said.
        if strong {
            self.say_line("On it.");
        } else {
            req().lock().unwrap().weak_pending.insert(id.clone());
        }
        // At most four open requests; a fifth waits for one to close (AC-173).
        let waiting_behind = {
            let mut r = req().lock().unwrap();
            if r.open.len() >= MAX_OPEN {
                r.waiting
                    .push_back((id.clone(), words.to_string(), via.to_string()));
                Some(r.open.len())
            } else {
                None
            }
        };
        if let Some(n) = waiting_behind {
            update(&d, &id, "state", "waiting_turn");
            update(&d, &id, "done", &format!("Waiting: {n} requests are open."));
            announce(self, &id);
            return json!({"taken": true, "request": id, "waits_behind": n});
        }
        self.begin(&id, words, via)
    }

    /// A taken request goes to Overseer (or to the agent spoken to).
    fn begin(self: &Arc<Self>, id: &str, words: &str, _via: &str) -> Value {
        let d = self.d.clone();
        let id = id.to_string();
        let s = match settings(&d) {
            Ok(s) => s,
            Err(e) => return json!({"taken": false, "why": e.to_string()}),
        };
        let target = s.target.clone();
        // Who it may be for, with the reasons, from the daemon's own records (AC-166).
        let focus_now = self.st.lock().unwrap().focus.clone();
        let previous = req().lock().unwrap().last_targets.clone();
        let agents = agents_for(&d);
        let ctx = super::candidates::Context {
            agents: &agents,
            focus: focus_now.as_deref(),
            previous: &previous,
        };
        let found = candidates_for(words, &ctx);
        let replaces_sent = req().lock().unwrap().replaces_pending.remove(&id);
        let weak = req().lock().unwrap().weak_pending.remove(&id);
        let direct = target != "overseer" && !plain(words).split(' ').any(|w| w == "overseer");
        req().lock().unwrap().open.push(Open {
            id: id.clone(),
            words: words.to_string(),
            at: Instant::now(),
            proposal: None,
            settle_until: None,
            candidates: found.clone(),
            confirm_at: None,
            holding_said: false,
            turn_seen: None,
            direct,
            replaces_sent,
            weak,
            named: floor::names_overseer_with_instruction(words),
            asked_again: false,
        });
        self.st.lock().unwrap().thinking += 1;
        self.refresh();
        // Words heard before that were not requests go along as context, once.
        let context: Vec<String> = {
            let mut r = req().lock().unwrap();
            trim_context(&mut r.context);
            r.context.drain(..).map(|h| h.text).collect()
        };
        // Talking to one agent: the words go to it as they are, with no model turn (AC-166).
        if direct {
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
        if found.is_empty() {
            message.push_str("(Candidates from the daemon: none named. If the owner asks for new work (\"someone should…\"), start a new agent for it in the repository the context gives; if it is about everyone or answers itself, act on it; otherwise ask the owner one short question: who?)\n");
        } else {
            let list: Vec<String> = found
                .iter()
                .map(|(run, why)| {
                    format!(
                        "{} ({run}): {why}",
                        agents
                            .iter()
                            .find(|a| &a.id == run)
                            .map(|a| a.title.as_str())
                            .unwrap_or(run)
                    )
                })
                .collect();
            message.push_str(&format!("(Candidates from the daemon: {}. Choose among them; anyone else waits longer and is named aloud.)\n", list.join("; ")));
        }
        message.push_str(&format!("{}\nRequest {id}: {words}", if weak { VOICE_NOTE } else { VOICE_NOTE_TAKEN }));
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
        self.next_waiting();
    }

    /// A request that waited for a free place begins.
    fn next_waiting(self: &Arc<Self>) {
        let next = {
            let mut r = req().lock().unwrap();
            if r.open.len() < MAX_OPEN {
                r.waiting.pop_front()
            } else {
                None
            }
        };
        if let Some((id, words, via)) = next {
            let v = self.clone();
            self.run_blocking(move || {
                v.begin(&id, &words, &via);
            });
        }
    }

    fn cancel_request(self: &Arc<Self>, o: &Open, how: &str) -> Value {
        // Out of the open list first, so the proposal's own "cancelled" event, which may arrive
        // on another thread, cannot close a correction or a join under the wrong name.
        let taken = {
            let mut r = req().lock().unwrap();
            let before = r.open.len();
            r.open.retain(|x| x.id != o.id);
            before != r.open.len()
        };
        if let Some(p) = &o.proposal {
            if let Err(e) = self.d.overseer_cancel(p, "owner (voice)") {
                if taken {
                    req().lock().unwrap().open.push(o.clone());
                }
                return json!({"cancelled": false, "why": e.to_string()});
            }
        }
        if taken {
            let mut st = self.st.lock().unwrap();
            st.thinking = st.thinking.saturating_sub(1);
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
        // AC-265: these exact owner phrases bypass the model. No proposal/check-in may resume.
        let queue_command = [("send queued", "resume_queue"), ("resume queue", "resume_queue"), ("clear queued", "clear_queue"), ("clear queue", "clear_queue")]
            .into_iter().find_map(|(phrase, action)| {
                if p == phrase { Some((action, "")) }
                else { p.strip_prefix(&format!("{phrase} ")).map(|name| (action, name.trim_start_matches("for ").trim_start_matches("the ").trim_end_matches(" agent"))) }
            });
        if let Some((action, name)) = queue_command {
            let selected = super::settings(&d).map(|s| s.target).unwrap_or_default();
            let targets: Vec<_> = d.roster().unwrap_or_default().into_iter()
                .filter(|l| l.role != "overseer" && if name.is_empty() { l.id == selected } else { plain(&l.title).contains(name) }).collect();
            heard_signal(self);
            let [target] = targets.as_slice() else {
                let why = if targets.is_empty() { "Name the agent whose queue you want." } else { "More than one agent matches; say its full name." };
                speak_when_free(self, why);
                return Some(json!({"taken":false,"built_in":action,"why":why}));
            };
            let id = next_id(&d);
            let _ = record(&d, &id, text, via, &target.id, "taken", "request");
            let result = if action == "resume_queue" { d.resume_queue(&target.id) } else { d.clear_queue(&target.id) };
            return Some(match result {
                Ok(result) => {
                    let said = if action == "resume_queue" { format!("Sending queued messages to {}.", target.title) } else { format!("Cleared {}'s queue.", target.title) };
                    self.close(&id, "done", Some(&said)); speak_when_free(self, &said);
                    json!({"taken":true,"built_in":action,"request":id,"run":target.id,"result":result})
                }
                Err(e) => {
                    self.close(&id, "not_sent", Some(&e.to_string()));
                    json!({"taken":false,"built_in":action,"error":e.to_string()})
                }
            });
        }
        // The zero-friction loop by voice (AC-252): open Overseer, follow an agent, and switch it
        // between Follow and Manual edit, each one sentence with no yes. The owner's VS Code window
        // does it (the live channel's "open"). Before "switch to …", which picks who is spoken to.
        if let Some((place, run)) = self.loop_place(p) {
            heard_signal(self);
            return Some(match run {
                Ok(run) => {
                    let mut open = json!({"kind": "open", "place": place});
                    if let Some(run) = &run {
                        open["run"] = json!(run);
                    }
                    self.emit(open);
                    json!({"taken": true, "built_in": "open", "place": place, "run": run})
                }
                Err(why) => {
                    speak_when_free(self, &why);
                    json!({"taken": false, "why": why})
                }
            });
        }
        // Who is spoken to, by voice (AC-166): "talk to Continuity", "back to Overseer".
        if matches!(
            p,
            "back to overseer" | "talk to overseer" | "talk to overseer again" | "overseer again"
        ) {
            let _ = super::set(&d, &json!({"target": "overseer"}));
            heard_signal(self);
            speak_when_free(self, "Back to Overseer.");
            return Some(json!({"taken": true, "built_in": "target", "target": "overseer"}));
        }
        if let Some(name) = p
            .strip_prefix("talk to ")
            .or_else(|| p.strip_prefix("switch to "))
            .or_else(|| p.strip_prefix("let me talk to "))
            .map(|n| {
                n.trim_start_matches("the ")
                    .trim_end_matches(" agent")
                    .to_string()
            })
            .filter(|n| !n.is_empty())
        {
            let found: Vec<(String, String)> = d
                .roster()
                .unwrap_or_default()
                .into_iter()
                .filter(|l| {
                    crate::daemon::ACTIVE.contains(&l.status.as_str())
                        && plain(&l.title).contains(&name)
                })
                .map(|l| (l.id, l.title))
                .collect();
            heard_signal(self);
            return Some(match found.as_slice() {
                [(id, title)] => match super::set(&d, &json!({"target": id})) {
                    Ok(_) => {
                        speak_when_free(self, &format!("Talking to {title}."));
                        json!({"taken": true, "built_in": "target", "target": id})
                    }
                    Err(e) => json!({"taken": false, "why": e.to_string()}),
                },
                [] => {
                    speak_when_free(self, &format!("No agent called {name} is running."));
                    json!({"taken": false, "why": "no such agent"})
                }
                many => {
                    let names: Vec<String> = many.iter().map(|(_, t)| t.clone()).collect();
                    speak_when_free(
                        self,
                        &format!(
                            "Which one: {}?",
                            join_names(&names).replace(" and ", " or ")
                        ),
                    );
                    self.st.lock().unwrap().awaiting_answer = true;
                    json!({"taken": false, "why": "more than one agent has that name"})
                }
            });
        }
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

    /// The loop's spoken forms (AC-252): `(place, Ok(agent))` to show, or `Err(what to say)` when
    /// the agent named is not clear; `None` when the words are not one of them.
    #[allow(clippy::type_complexity)]
    fn loop_place(&self, p: &str) -> Option<(&'static str, Result<Option<String>, String>)> {
        if matches!(
            p,
            "open overseer" | "show overseer" | "show me overseer" | "overseer open" | "open the overseer layout" | "open the overseer workspace"
        ) {
            return Some(("overseer", Ok(None)));
        }
        if matches!(
            p,
            "manual edit" | "manual edit mode" | "switch to manual edit" | "let me edit" | "let me edit it" | "edit it myself" | "i ll edit it"
        ) {
            return Some(("manual_edit", Ok(None)));
        }
        if matches!(p, "follow" | "follow it" | "follow again" | "follow mode" | "back to follow" | "switch to follow" | "follow the agent") {
            return Some(("follow", Ok(None)));
        }
        let name = p.strip_prefix("follow ")?.trim_start_matches("the ").trim_end_matches(" agent").trim();
        if name.is_empty() {
            return None;
        }
        let roster = self.d.roster().unwrap_or_default();
        let named: Vec<_> = roster.iter().filter(|l| plain(&l.title).contains(name)).collect();
        // A running agent goes before a finished one of the same name.
        let active: Vec<_> = named.iter().filter(|l| crate::daemon::ACTIVE.contains(&l.status.as_str())).collect();
        let pick = match (active.as_slice(), named.as_slice()) {
            ([one], _) => Ok(Some(one.id.clone())),
            ([], [one]) => Ok(Some(one.id.clone())),
            // Not an agent's name ("follow up with …"): Overseer reads it.
            ([], []) => return None,
            (many, _) if !many.is_empty() => Err(format!("Which one: {}?", join_names(&many.iter().map(|l| l.title.clone()).collect::<Vec<_>>()).replace(" and ", " or "))),
            (_, many) => Err(format!("Which one: {}?", join_names(&many.iter().map(|l| l.title.clone()).collect::<Vec<_>>()).replace(" and ", " or "))),
        };
        Some(("follow", pick))
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
        let mut r = req().lock().unwrap();
        r.lapsed_read_back = None;
        r.read_back = Some(ReadBack {
            run: run.clone(),
            request: request.clone(),
            title: title.clone(),
            at: Instant::now(),
        });
        drop(r);
        self.st.lock().unwrap().awaiting_answer = true;
        let line = format!("{title} wants to {what}. Allow?");
        speak_when_free(self, &line);
        self.emit(json!({"kind": "read_back", "agent": run, "title": title, "what": what}));
        json!({"read_back": {"agent": run, "request": request, "said": line}})
    }

    /// "Handle what needs me", "tell it yes" by voice (AC-227): the conversation gets the same
    /// cards as typed; the one permission waiting is read back (handle) or answered (yes, no).
    fn needs(self: &Arc<Self>, ask: crate::overseer::needs::Ask, text: &str, via: &str) -> Value {
        use crate::overseer::needs::Ask;
        heard_signal(self);
        let d = self.d.clone();
        let id = next_id(&d);
        let _ = record(&d, &id, text, via, "overseer", "answered", "needs");
        let waiting = d.needs_waiting();
        let pending = req().lock().unwrap().read_back.clone();
        let reply = d.needs_handle(text, "voice", Some(&format!("Request {id}: {text}")), false).ok().flatten();
        let line = reply.as_ref().and_then(|r| r["reply"].as_str().map(str::to_string)).unwrap_or_default();
        let which = ask == Ask::Handle && reply.as_ref().is_some_and(|r| r["card"]["state"] == "which");
        if which {
            // "Which one?" leaves no yes/no target. Keep a queue marker so the earlier
            // permission is not automatically read again and made answerable behind that question.
            let _queue = PERMISSION_READ_BACK.lock().unwrap();
            let mut r = req().lock().unwrap();
            if let Some(rb) = r.read_back.take() {
                r.lapsed_read_back = Some(rb);
            }
            drop(r);
            self.st.lock().unwrap().awaiting_answer = false;
            self.emit(json!({"kind": "read_back", "lapsed": true}));
        }
        let one = if which { None } else { pending.clone().or_else(|| match waiting.as_slice() {
            [w] => Some(ReadBack { run: w.run.clone(), request: w.request.clone(), title: w.title.clone(), at: Instant::now() }),
            _ => None,
        }) };
        let answered = match (ask, one) {
            (Ask::Handle, Some(rb)) => {
                // Read back: the next yes or no answers it.
                let s = settings(&d).unwrap_or_default();
                if s.permission_answers {
                    let mut r = req().lock().unwrap();
                    r.lapsed_read_back = None;
                    r.read_back = Some(ReadBack { at: Instant::now(), ..rb.clone() });
                    drop(r);
                    self.st.lock().unwrap().awaiting_answer = true;
                    self.emit(json!({"kind": "read_back", "agent": rb.run, "title": rb.title, "what": waiting.iter().find(|w| w.run == rb.run).map(|w| w.what.clone()).unwrap_or_default()}));
                }
                speak_when_free(self, &line);
                Value::Null
            }
            (Ask::Yes | Ask::No, Some(rb)) => self.answer_permission(&rb, ask == Ask::Yes),
            _ => {
                if !line.is_empty() {
                    speak_when_free(self, &line);
                }
                Value::Null
            }
        };
        update(&d, &id, "done", &line);
        announce(self, &id);
        json!({"taken": true, "request": id, "needs": true, "said": line, "answer": answered})
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
            r.lapsed_read_back = None;
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
        // The owner already took this action. Keep confirmation and Cancel,
        // without a redundant automatic Audio Mode notification.
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
        .map(|s| s.to_string());
    match (tool, detail) {
        ("Bash" | "bash" | "shell" | "exec_command" | "command", Some(c)) => format!("run {}", c.chars().take(80).collect::<String>()),
        ("Edit" | "Write" | "edit" | "write" | "apply_patch", Some(p)) => {
            format!("change {}", p.rsplit('/').next().unwrap_or(&p))
        }
        (t, Some(x)) => format!("use {t} on {}", x.rsplit('/').next().unwrap_or(&x).chars().take(80).collect::<String>()),
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
    // Which requests Overseer's current turn is about: their answer clock starts now.
    let unseen = req()
        .lock()
        .unwrap()
        .open
        .iter()
        .any(|o| o.proposal.is_none() && o.turn_seen.is_none());
    if unseen {
        let now_about = turn_requests(&v.d, false);
        let mut r = req().lock().unwrap();
        for o in r.open.iter_mut() {
            if o.turn_seen.is_none() && now_about.contains(&o.id) {
                o.turn_seen = Some(Instant::now());
            }
        }
    }
    // The holding line: once per request, when Overseer has neither planned nor answered.
    let holding: Vec<String> = {
        let mut r = req().lock().unwrap();
        r.open
            .iter_mut()
            .filter(|o| {
                o.proposal.is_none() && !o.weak && !o.holding_said && o.at.elapsed() >= holding_after()
            })
            .map(|o| {
                o.holding_said = true;
                o.id.clone()
            })
            .collect()
    };
    for id in holding {
        if row(&v.d, &id).is_some_and(|r| r["answer"].is_null()) {
            speak_when_free(v, "Still working on it.");
            v.emit(json!({"kind": "holding", "request": id}));
        }
    }
    // A Confirm plan left without a yes by voice: no answer; it stays in the card for a click.
    let lapsed = {
        let mut r = req().lock().unwrap();
        let mut any = false;
        for o in r.open.iter_mut() {
            if o.confirm_at.is_some_and(|t| t.elapsed() >= confirm_limit()) {
                o.confirm_at = None;
                any = true;
            }
        }
        any
    };
    if lapsed {
        v.st.lock().unwrap().awaiting_answer = false;
        v.emit(json!({"kind": "confirm", "lapsed": true}));
    }
    let late: Vec<Open> = req()
        .lock()
        .unwrap()
        .open
        .iter()
        .filter(|o| {
            o.proposal.is_none()
                && match o.turn_seen {
                    Some(t) => t.elapsed() > answer_limit(),
                    // Waiting behind Overseer's own turns: four times as long at most.
                    None => o.at.elapsed() > answer_limit() * 4,
                }
        })
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
            speak_when_free(v, "Overseer isn't answering, so nothing was sent.");
        }
    }
    // An answer from VS Code, the phone or the native harness also frees the spoken queue.
    // Reconcile before expiring the read-back, so a resolved request does not strand the next.
    let listening = v.running() && settings(&v.d).map(|s| s.enabled && !s.muted).unwrap_or(false);
    let read_back = {
        let mut r = req().lock().unwrap();
        if !listening {
            r.lapsed_read_back = None;
            None
        } else {
            r.read_back.clone().or_else(|| r.lapsed_read_back.clone())
        }
    };
    if read_back.as_ref().is_some_and(|rb| !permission_still_waiting(&v.d, rb)) {
        permission_waiting(&v.d, "");
    }
    let due = {
        let mut r = req().lock().unwrap();
        if r.read_back
            .as_ref()
            .is_some_and(|rb| rb.at.elapsed() > confirm_limit())
        {
            r.lapsed_read_back = r.read_back.take();
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
        // One at a time: when another permission is waiting, it is read back next.
        let v2 = v.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(500));
            permission_waiting(&v2.d, &a.run);
        });
    }
}

static PERMISSION_READ_BACK: Mutex<()> = Mutex::new(());

fn permission_still_waiting(d: &Arc<Daemon>, rb: &ReadBack) -> bool {
    d.run(&rb.run).ok().is_some_and(|run| {
        run.status == "waiting_for_user" && run.attention.as_ref().is_some_and(|a| {
            a["kind"] == "permission" && a["request_id"].as_str() == Some(rb.request.as_str())
        })
    })
}

/// An agent's permission request began to wait (AC-230): with Voice Mode listening it is read out
/// by itself, without the owner asking, and "allow it" answers it. One at a time: while another is
/// read back or answered, it waits, and is read after that one (as in `tick`).
pub fn permission_waiting(d: &Arc<Daemon>, _run: &str) {
    // A new waiting event can race the periodic reconciliation or a settled voice answer.
    // Serialize automatic read-backs so the next request is spoken only once.
    let _queue = PERMISSION_READ_BACK.lock().unwrap();
    let Some(v) = voice_or_none() else { return };
    if !v.running() || settings(d).map(|s| !s.enabled || s.muted).unwrap_or(true) {
        return;
    }
    let previous = {
        let r = req().lock().unwrap();
        r.read_back.clone().or_else(|| r.lapsed_read_back.clone())
    };
    if let Some(rb) = previous.filter(|rb| !permission_still_waiting(d, rb)) {
        let cleared = {
            let mut r = req().lock().unwrap();
            if r.lapsed_read_back.as_ref().is_some_and(|current| current.run == rb.run && current.request == rb.request) {
                r.lapsed_read_back = None;
            }
            if r.read_back.as_ref().is_some_and(|current| current.run == rb.run && current.request == rb.request) {
                r.read_back = None;
                true
            } else {
                false
            }
        };
        if cleared {
            v.st.lock().unwrap().awaiting_answer = false;
            v.emit(json!({"kind": "read_back", "resolved": true, "lapsed": true}));
        }
    }
    let busy = {
        let r = req().lock().unwrap();
        r.read_back.is_some() || r.lapsed_read_back.is_some() || r.answering.is_some()
    };
    if !busy && !d.needs_waiting().is_empty() {
        let _in_runtime = v.rt.as_ref().map(|h| h.enter());
        v.read_back();
    }
}

/// Overseer's session moved: link proposals to spoken requests, speak the plan and the outcome,
/// and read Overseer's replies out.
fn on_session_event(v: &Arc<Voice>, kind: &str, p: &Value, run_id: Option<&str>) {
    match kind {
        "proposal" if p["cause"] == "voice" || p["via"] == "voice" => {
            let id = p["id"].as_str().unwrap_or("").to_string();
            let settle_until = p["settle_until"].as_i64();
            let captured = p["turn"].is_object();
            let about = if captured { Some(captured_request_ids(&p["turn"])) } else { turn_requests_known(&v.d, false) };
            let open = {
                let mut r = req().lock().unwrap();
                // The request Overseer's turn is about; with several in flight, not just the first.
                // A turn about none of them (a check-in Overseer started) links to none.
                let at = r
                    .open
                    .iter()
                    .position(|o| o.proposal.is_none() && o.direct && (!captured || about.as_ref().is_some_and(|ids| ids.contains(&o.id))))
                    .or_else(|| match &about {
                        Some(ids) => r
                            .open
                            .iter()
                            .position(|o| o.proposal.is_none() && ids.contains(&o.id)),
                        None => r.open.iter().position(|o| o.proposal.is_none()),
                    });
                if at.is_some() && p["state"] == "open" && p["confirm"] == true {
                    // Only the question just published can take a bare yes. Older proposals
                    // remain open on their cards, but must never become eligible again.
                    for o in &mut r.open {
                        o.confirm_at = None;
                    }
                }
                let o = at.map(|i| &mut r.open[i]);
                o.map(|o| {
                    o.proposal = Some(id.clone());
                    o.settle_until = settle_until.map(|t| {
                        Instant::now()
                            + Duration::from_millis((t - crate::daemon::now()).max(0) as u64)
                    });
                    if p["state"] == "open" && p["confirm"] == true {
                        o.confirm_at = Some(Instant::now());
                    }
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
                if state == "yes" {
                    req().lock().unwrap().last_sent =
                        Some((o.id.clone(), o.words.clone(), Instant::now()));
                    if let Ok(card) = v.d.card(id) {
                        let targets: Vec<String> = card["rows"]
                            .as_array()
                            .map(|r| {
                                r.iter()
                                    .filter_map(|x| x["run_id"].as_str().map(String::from))
                                    .collect()
                            })
                            .unwrap_or_default();
                        if !targets.is_empty() {
                            req().lock().unwrap().last_targets = targets;
                        }
                    }
                }
                // Showing something (AC-226) was said as the plan: no "Sent." after it.
                let nav_only = v.d.card(id).ok().and_then(|c| c["actions"].as_array().cloned()).is_some_and(|a| !a.is_empty() && a.iter().all(|x| crate::overseer::control::NAVIGATE.contains(&x["action"].as_str().unwrap_or(""))));
                let (req_state, line) = match state {
                    "yes" if nav_only && !result.contains("failed") => ("sent", ""),
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
            // A daemon-generated permission notice is independent of the model's
            // reply to an open spoken request. permission_waiting handles its read-back.
            if m["source"] != "overseer" || (m["card"]["kind"] == "needs" && m["card"]["by_itself"] == true) {
                return;
            }
            let text = m["text"].as_str().unwrap_or("").trim().to_string();
            let about = if p["turn"].is_object() {
                Some(p["turn"]["requests"].as_array().map(|ids| ids.iter().filter_map(|id| id.as_str().map(str::to_string)).collect()).unwrap_or_default())
            } else { turn_requests_known(&v.d, false) };
            let o = {
                let r = req().lock().unwrap();
                match &about {
                    Some(ids) => r.open.iter().find(|o| ids.contains(&o.id)).cloned(),
                    None => r.open.first().cloned(),
                }
            };
            let Some(o) = o else { return };
            update(&v.d, &o.id, "answer", &text);
            // Only the exact reply counts: a reply that quotes the instruction is still an answer.
            // The daemon stores it in plain words, as an aside card (AC-228).
            if m["card"]["kind"] == "aside" || text.trim_matches(|c: char| !c.is_alphanumeric() && c != '_') == crate::overseer::session::NOT_FOR_OVERSEER {
                // It names Overseer and instructs it: never dropped. Asked again, once, as surely
                // Overseer's (AC-229).
                if o.named && !o.asked_again && o.proposal.is_none() {
                    ask_again(v, &o);
                    return;
                }
                if o.weak {
                    v.close(&o.id, "not_for_overseer", None);
                } else {
                    // "On it." was said: it stays Overseer's, answered with nothing to do.
                    v.close(&o.id, "answered", Some("Overseer found nothing to do for this."));
                }
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
        "overseer_turn_processed" => {
            // When Overseer's turn ends, a request with no plan is answered.
            let Some(run) = run_id else { return };
            if v.d.run_role(run) != "overseer" {
                return;
            }
            let about: Vec<String> = p["turn"]["requests"].as_array().map(|ids| ids.iter().filter_map(|id| id.as_str().map(str::to_string)).collect()).unwrap_or_default();
            let again = p["turn"]["asked_again"] == true;
            let done: Vec<Open> = req()
                .lock()
                .unwrap()
                .open
                .iter()
                .filter(|o| o.proposal.is_none() && about.contains(&o.id) && (!o.asked_again || again))
                .cloned()
                .collect();
            for o in done {
                let answered =
                    row(&v.d, &o.id).and_then(|r| r["answer"].as_str().map(String::from));
                let failed = p["status"] == "failed";
                if failed {
                    // Rate-limited, signed out, offline: not sent, never sent later by itself, and
                    // said once (AC-175).
                    let why = answered
                        .map(|a| first_sentences(&a, 1))
                        .filter(|a| !a.is_empty())
                        .unwrap_or_else(|| "Overseer's turn failed".into());
                    v.close(
                        &o.id,
                        "not_sent",
                        Some(&format!("Not sent: {why}. Nothing will be sent later.")),
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
                        speak_when_free(v, "Overseer can't work on requests right now, so nothing was sent.");
                    }
                } else {
                    v.close(&o.id, "answered", None);
                }
            }
        }
        _ => {}
    }
}

/// A request that names Overseer and gives it an instruction came back "not for me": it goes to
/// Overseer again, once, saying it is surely Overseer's (AC-229). If that fails, it is not sent and
/// said so, as any request Overseer cannot take (AC-175).
/// The words that mark the turn asking a request again.
const ASKED_AGAIN: &str = "(This spoken request names you and gives you an instruction";


fn ask_again(v: &Arc<Voice>, o: &Open) {
    if let Some(x) = req().lock().unwrap().open.iter_mut().find(|x| x.id == o.id) {
        x.asked_again = true;
        x.turn_seen = None;
        x.at = Instant::now();
        x.holding_said = true;
    }
    update(&v.d, &o.id, "state", "thinking");
    announce(v, &o.id);
    let message = format!(
        "{ASKED_AGAIN}, so it is yours: act on it; do not answer NOT_FOR_OVERSEER.)\n{VOICE_NOTE_TAKEN}\nRequest {}: {}",
        o.id, o.words
    );
    if let Err(e) = v.d.overseer_send(&message, "voice", None, None) {
        v.close(&o.id, "not_sent", Some(&format!("Not sent: {e}")));
        speak_when_free(v, "I can't reach Overseer right now, so nothing was sent.");
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

/// The part of a decorated message Overseer wrote for the agent (after "For you:"), or the
/// owner's words when they went as they are (AC-169's message shape).
fn own_part(text: &str) -> String {
    let body = match text.split_once("\nFor you: ") {
        Some((_, rest)) => rest.to_string(),
        None => match (text.find('“'), text.rfind('”')) {
            (Some(a), Some(b)) if b > a => text[a + '“'.len_utf8()..b].to_string(),
            _ => text.to_string(),
        },
    };
    body.split("\nAlso told:").next().unwrap_or("").trim().to_string()
}

/// A task said back as the end of a question: "Please draft the page's sections." becomes "draft
/// the page's sections".
fn task_words(text: &str) -> String {
    let t = own_part(text);
    let t = t.trim().trim_end_matches(['.', '!', '?']).trim();
    let t = t.strip_prefix("Please ").or_else(|| t.strip_prefix("please ")).unwrap_or(t);
    let mut c = t.chars();
    match c.next() {
        Some(f) if !t.starts_with("I ") => f.to_lowercase().collect::<String>() + c.as_str(),
        Some(_) => t.to_string(),
        None => String::new(),
    }
}

/// What a plan that starts or redirects an agent will send, read back as a question before it
/// goes (AC-229): "Start an agent in the site repo to draft the page's sections?". Permission
/// changes name the mode too, so the owner knows what their yes approves (AC-230).
pub fn read_back_line(d: &Daemon, actions: &[Value]) -> Option<String> {
    let starts: Vec<&Value> = actions.iter().filter(|a| a["action"] == "start").collect();
    let redirects: Vec<&Value> = actions.iter().filter(|a| a["action"] == "redirect").collect();
    let modes: Vec<&Value> = actions.iter().filter(|a| a["action"] == "mode").collect();
    if starts.is_empty() && redirects.is_empty() && modes.is_empty() {
        return None;
    }
    let repo_name = |a: &Value| {
        let r = a["repo"].as_str().unwrap_or("").trim_end_matches('/');
        r.rsplit('/').next().unwrap_or(r).to_string()
    };
    let mut parts: Vec<String> = Vec::new();
    for a in starts {
        let mode = a["permission_mode"].as_str().filter(|m| !m.is_empty())
            .map(|m| format!(" in {}", crate::overseer::modes::label(m))).unwrap_or_default();
        parts.push(format!("start an agent in the {} repo{mode} to {}", repo_name(a), task_words(a["prompt"].as_str().unwrap_or(""))));
    }
    for a in redirects {
        let who = a["title"].as_str().map(String::from).or_else(|| a["agent"].as_str().and_then(|id| d.run(id).ok().map(|r| r.title))).unwrap_or_else(|| "the agent".into());
        parts.push(format!("redirect {who} to {}", task_words(a["text"].as_str().unwrap_or(""))));
    }
    for a in modes {
        let who = a["title"].as_str().map(String::from).or_else(|| a["agent"].as_str().and_then(|id| d.run(id).ok().map(|r| r.title))).unwrap_or_else(|| "the agent".into());
        parts.push(format!("set {who} to {}", crate::overseer::modes::label(a["mode"].as_str().unwrap_or(""))));
    }
    // A yes approves the whole bundle: never hide a permission, merge or archive
    // behind a count. Reuse the daemon's checked-action descriptions from its cards.
    for a in actions.iter().filter(|a| !matches!(a["action"].as_str(), Some("start" | "redirect" | "mode"))) {
        parts.push(d.describe(a));
    }
    let line = parts.join(", and ");
    let mut c = line.chars();
    let line = match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => line,
    };
    Some(format!("{line}?"))
}

/// What the plan does, said in one line (the daemon's words, from the checked actions). A plan
/// that starts or redirects an agent is read back as a question (AC-229).
fn plan_line(d: &Daemon, p: &Value) -> String {
    if let Some(line) = read_back_line(d, p["actions"].as_array().map(Vec::as_slice).unwrap_or(&[])) {
        return line;
    }
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
            "focus" => "Showing",
            "show_work" => "Showing the work of",
            "open_review" => "Opening the review of",
            "open_file" => "Opening the file from",
            "open_worktree" => "Opening the worktree of",
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
    decorate_for_turn(d, actions, None)
}

pub(crate) fn decorate_for_turn(d: &Daemon, actions: &mut [Value], turn: Option<&Value>) -> Result<bool> {
    let s = settings(d)?;
    let about = turn.map(captured_request_ids).or_else(|| turn_requests_known(d, false));
    let open = {
        let r = req().lock().unwrap();
        r.open
            .iter()
            .find(|o| o.proposal.is_none() && o.direct && (turn.is_none() || about.as_ref().is_some_and(|ids| ids.contains(&o.id))))
            .or_else(|| match &about {
                Some(ids) => r
                    .open
                    .iter()
                    .find(|o| o.proposal.is_none() && ids.contains(&o.id)),
                None => r.open.iter().find(|o| o.proposal.is_none()),
            })
            .cloned()
    };
    let words = open.as_ref().map(|o| o.words.clone()).unwrap_or_default();
    // The request's id stays in the daemon's records and logs, never in what the owner reads in
    // the agent's chat (AC-219).
    let head = match open.as_ref().and_then(|o| o.replaces_sent.as_ref()) {
        Some(_) => "(by voice; it replaces an earlier spoken request that was already sent)".to_string(),
        None => "(by voice)".to_string(),
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
    let found = open
        .as_ref()
        .map(|o| o.candidates.clone())
        .unwrap_or_default();
    for a in actions.iter_mut() {
        let kind = a["action"].as_str().unwrap_or("").to_string();
        // Why this agent: the daemon's reason when it was a candidate; otherwise it is Overseer's
        // own choice, which waits the longer window and is named aloud (AC-166).
        if let Some(agent) = a["agent"].as_str().map(String::from) {
            match found.iter().find(|(id, _)| *id == agent) {
                Some((_, why)) => {
                    if a["why"].as_str().unwrap_or("").is_empty() {
                        a["why"] = json!(why);
                    }
                }
                None if !found.is_empty() || a["confidence"] != "high" => {
                    a["why"] = json!("chosen by Overseer");
                    if a["confidence"] != "low" {
                        a["confidence"] = json!("medium");
                    }
                }
                None => {}
            }
        }
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
        // A new agent takes the composer's remembered harness, account, model and workspace mode
        // (AC-168), unless Overseer chose one.
        if a["action"] == "start" {
            let defaults: Value = super::meta(d, "voice.start_defaults")
                .ok()
                .flatten()
                .and_then(|j| serde_json::from_str(&j).ok())
                .unwrap_or(json!({}));
            for k in ["harness", "profile_id", "model", "workspace_mode"] {
                if a[k].as_str().unwrap_or("").is_empty() {
                    if let Some(v) = defaults[k].as_str() {
                        a[k] = json!(v);
                    }
                }
            }
            if defaults["trusted"] == false {
                a["untrusted"] = json!(true);
            }
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
                format!("{head} The owner said: “{words}”")
            } else {
                format!("{head} The owner said: “{words}”\nFor you: {own}")
            };
            if !others.is_empty() {
                full.push_str(&format!("\nAlso told: {}.", others.join(", ")));
            }
            // One message is 4,000 characters at most (AC-173): the owner's quote is cut first
            // (the card keeps the whole request); a part Overseer wrote that is too long alone is
            // not sent.
            let len = full.chars().count();
            if len > MAX_MESSAGE {
                let quote = words.chars().count();
                let room = quote.saturating_sub(len - MAX_MESSAGE + 90);
                if room < 200 {
                    bail!("a message to an agent is 4,000 characters at most; this one is {len}");
                }
                let cut: String = words.chars().take(room).collect();
                full = full.replacen(
                    &format!("“{words}”"),
                    &format!(
                        "“{cut}…” (cut here: the whole request is in Overseer's conversation)"
                    ),
                    1,
                );
            }
            a[text_key] = json!(full);
        }
    }
    Ok(needs_yes)
}

/// What to do about a new agent that could not start (AC-168): one line, shown in its row.
pub fn start_fix(error: &str) -> &'static str {
    let e = error.to_lowercase();
    if e.contains("not a git")
        || e.contains("no such file")
        || e.contains("does not exist")
        || e.contains("repository")
    {
        "check the repository path, or say which repository"
    } else if e.contains("sign")
        || e.contains("logged out")
        || e.contains("login")
        || e.contains("auth")
    {
        "sign in to the account in Accounts, or choose another in the composer"
    } else if e.contains("not installed")
        || e.contains("not found")
        || e.contains("no such harness")
        || e.contains("cannot run")
    {
        "install the harness, or choose another in the composer"
    } else if e.contains("trust") {
        "trust the workspace in VS Code"
    } else {
        "start it from the composer to see what it needs"
    }
}

/// Medium confidence waits longer, with every target named (AC-166). A start or a redirect is
/// read back first (AC-229): the window is that long besides, so the owner hears all of it and
/// still has the whole window to correct it (about 150 words a minute, 8 s at most).
pub fn settle_ms(d: &Daemon, actions: &[Value]) -> i64 {
    let base = settings(d).map(|s| s.settle_seconds).unwrap_or(2) as i64 * 1000;
    let base = if actions.iter().any(|a| a["confidence"] == "medium") {
        base.max(4000)
    } else {
        base
    };
    base + read_back_ms(read_back_line(d, actions).as_deref())
}

/// How long a read-back takes to say.
fn read_back_ms(line: Option<&str>) -> i64 {
    line.map(|l| (l.split_whitespace().count() as i64 * 400).min(8000)).unwrap_or(0)
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

/// `voice.answer`: yes or no from the keyboard to what was read back — a permission request or a
/// Confirm plan — the same as saying it (AC-171, AC-174).
pub fn answer(_d: &Arc<Daemon>, p: &Value) -> Result<Value> {
    let v = super::voice().ok_or_else(|| anyhow!("the voice session is not running"))?;
    let yes = p["yes"].as_bool().unwrap_or(false);
    let rb = req().lock().unwrap().read_back.clone();
    let rb = rb.filter(|rb| !newer_plan(rb.at));
    if let Some(rb) = rb {
        return Ok(v.answer_permission(&rb, yes));
    }
    let o = req()
        .lock()
        .unwrap()
        .open
        .iter()
        .filter(|o| o.proposal.is_some() && o.confirm_at.is_some_and(|t| t.elapsed() < confirm_limit()))
        .max_by_key(|o| o.confirm_at)
        .cloned();
    let Some(o) = o else {
        bail!("nothing is waiting for a yes");
    };
    if let Some(x) = req().lock().unwrap().open.iter_mut().find(|x| x.id == o.id) {
        x.confirm_at = None;
    }
    v.st.lock().unwrap().awaiting_answer = false;
    let r = v.d.overseer_answer(
        o.proposal.as_deref().unwrap_or(""),
        yes,
        "vscode",
        "owner (keyboard)",
    )?;
    Ok(json!({"request": o.id, "answered": yes, "result": r["result"]}))
}

/// `voice.read_back`: reads the oldest waiting permission request back (also asked by voice).
pub fn read_back(_d: &Arc<Daemon>, _p: &Value) -> Result<Value> {
    let v = super::voice().ok_or_else(|| anyhow!("the voice session is not running"))?;
    Ok(v.read_back())
}

/// The candidates for a request's words. A correction names the new targets ("I meant the phone
/// agent") or leaves one out ("not Continuity"); with neither, the first words' candidates hold.
fn candidates_for(words: &str, ctx: &super::candidates::Context) -> Vec<(String, &'static str)> {
    let of = |t: &str| -> Vec<(String, &'static str)> {
        super::candidates::candidates(t, ctx)
            .into_iter()
            .map(|c| (c.id, c.reason))
            .collect()
    };
    let Some(at) = words.rfind(" (correction") else {
        return of(words);
    };
    let first = &words[..at];
    let fix = words[at..]
        .split_once(": ")
        .map(|(_, f)| f.trim_end_matches(')'))
        .unwrap_or("");
    let before = candidates_for(first, ctx);
    if plain(fix).starts_with("not ") {
        let gone: Vec<String> = of(fix).into_iter().map(|(id, _)| id).collect();
        return before
            .into_iter()
            .filter(|(id, _)| !gone.contains(id))
            .collect();
    }
    let named = of(fix);
    if named.iter().any(|(_, why)| *why != "selected") {
        return named;
    }
    before
}

/// The spoken requests an Overseer turn is about, from its prompt ("Request V-0042: …"): the turn
/// now running, or with `ended` the last one that ended.
fn turn_requests(d: &Daemon, ended: bool) -> Vec<String> {
    turn_requests_known(d, ended).unwrap_or_default()
}

/// The spoken requests Overseer's current turn is about, when every one of them was closed as not
/// sent ("Nothing will be sent later"): what that turn proposes afterwards is withdrawn (AC-248).
pub fn turn_requests_not_sent(d: &Daemon) -> Option<Vec<String>> {
    let ids = turn_requests_known(d, false)?;
    let closed = !ids.is_empty() && ids.iter().all(|id| row(d, id).is_some_and(|r| r["state"] == "not_sent"));
    closed.then_some(ids)
}

fn captured_request_ids(turn: &Value) -> Vec<String> {
    turn["requests"].as_array().map(|ids| ids.iter().filter_map(|id| id.as_str().map(str::to_string)).collect()).unwrap_or_default()
}

pub(crate) fn captured_requests_not_sent(d: &Daemon, turn: &Value) -> Option<Vec<String>> {
    let ids = captured_request_ids(turn);
    let closed = !ids.is_empty() && ids.iter().all(|id| row(d,id).is_some_and(|r| matches!(r["state"].as_str(), Some("not_sent" | "cancelled" | "superseded"))));
    closed.then_some(ids)
}

/// The same, or `None` when there is no Overseer turn to read (then the caller may fall back).
fn turn_requests_known(d: &Daemon, ended: bool) -> Option<Vec<String>> {
    let session = d.overseer_session().ok()?;
    let run = session["run_id"].as_str()?.to_string();
    let turns = d.store.lock().unwrap().turns(&run).unwrap_or_default();
    let turn = if ended {
        turns.iter().rev().find(|t| t.ended_ms.is_some())
    } else {
        turns.last()
    }?;
    Some(request_ids(&turn.prompt))
}

pub(crate) fn captured_turn(id: &str, prompt: &str) -> Value {
    json!({"id":id,"requests":request_ids(prompt),"asked_again":prompt.contains(ASKED_AGAIN)})
}

fn request_ids(prompt: &str) -> Vec<String> {
        prompt
            .match_indices("Request V-")
            .map(|(i, _)| {
                prompt[i + "Request ".len()..]
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '-')
                    .collect()
            })
            .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// AC-162: the words recorded from the chosen recognizer (small.en with the hint, three system
    /// voices) replayed through the local rules. Every command reads as meant for Overseer; an agent
    /// named and heard is a candidate, and one misheard is not ("phonation" names no one); a yes or
    /// a cancel counts only when heard right, so a mishearing is no answer.
    #[test]
    fn the_recorded_words_replay_through_the_local_rules() {
        use super::super::candidates::{candidates, Agent, Context};
        let doc: Value = serde_json::from_str(include_str!(
            "../../../voice/tests/fixtures/words-small-en.json"
        ))
        .unwrap();
        let agent = |id: &str, title: &str, task: &str| Agent {
            id: id.into(),
            title: title.into(),
            repository: "/w/overseer".into(),
            branch: None,
            active: true,
            just_asked: false,
            files: vec![],
            task: task.into(),
        };
        let agents = vec![
            agent("r-phone", "Phone app", ""),
            agent("r-cont", "Continuity", ""),
            agent("r-codex", "Codex", ""),
            agent("r-swarm", "Swarm", ""),
            agent(
                "r-gw",
                "Gateway tests",
                "Work on AC-116: the gateway's tests.",
            ),
        ];
        let names: Vec<String> = agents.iter().map(|a| a.title.clone()).collect();
        let ctx = Context {
            agents: &agents,
            focus: None,
            previous: &[],
        };
        let named = [
            ("phone", "r-phone"),
            ("continuity", "r-cont"),
            ("codex", "r-codex"),
            ("swarm", "r-swarm"),
            ("ac-116", "r-gw"),
        ];
        let mut misheard = Vec::new();
        let list = doc["utterances"].as_array().unwrap();
        assert_eq!(list.len(), 36);
        for u in list {
            let said = u["said"].as_str().unwrap();
            let heard = u["heard"].as_str().unwrap();
            let p = plain(heard);
            if p != plain(said) {
                misheard.push(heard.to_string());
            }
            match said {
                "Yes, allow it." => {
                    assert_eq!(YES.contains(&p.as_str()), plain(said) == p, "{heard}");
                    assert!(!NO.contains(&p.as_str()), "{heard}");
                }
                "Cancel that." => {
                    assert_eq!(CANCEL.iter().any(|c| p == *c), plain(said) == p, "{heard}")
                }
                "Talk to Continuity." => assert_eq!(p, "talk to continuity"),
                "Back to Overseer." => assert_eq!(p, "back to overseer"),
                _ => {
                    assert!(
                        floor::addressed(heard, &names, false),
                        "meant for Overseer: {heard}"
                    );
                    let got: Vec<String> =
                        candidates(heard, &ctx).into_iter().map(|c| c.id).collect();
                    let said_l = said.to_lowercase().replace("ac 116", "ac-116");
                    let heard_l = heard.to_lowercase();
                    for (word, id) in named {
                        if said_l.contains(word) {
                            assert_eq!(
                                got.iter().any(|g| g == id),
                                heard_l.contains(word),
                                "{id} for {heard:?}: {got:?}"
                            );
                        }
                    }
                }
            }
        }
        // The recognizer's own mistakes in the recording: five utterances (four of Flo's, one of Eddy's).
        assert_eq!(misheard.len(), 5, "{misheard:?}");
    }

    /// AC-173: the rolling context keeps 10 minutes or 30 exchanges.
    #[test]
    fn the_rolling_context_keeps_ten_minutes_or_thirty() {
        let mut ctx: VecDeque<Heard> = VecDeque::new();
        let now = Instant::now();
        let ago = |s: u64| now.checked_sub(Duration::from_secs(s)).unwrap_or(now);
        ctx.push_back(Heard {
            at: ago(660),
            text: "eleven minutes ago".into(),
        });
        ctx.push_back(Heard {
            at: ago(540),
            text: "nine minutes ago".into(),
        });
        trim_context(&mut ctx);
        assert_eq!(
            ctx.iter().map(|h| h.text.as_str()).collect::<Vec<_>>(),
            vec!["nine minutes ago"]
        );
        for i in 0..40 {
            ctx.push_back(Heard {
                at: now,
                text: format!("line {i}"),
            });
        }
        trim_context(&mut ctx);
        assert_eq!(ctx.len(), 30);
        assert_eq!(ctx.front().unwrap().text, "line 10");
    }
}
