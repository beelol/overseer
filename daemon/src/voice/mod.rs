//! Voice Mode in the daemon (Gate R, docs/rfcs/voice-mode.md).
//!
//! The daemon owns the voice session (AC-163): its settings, the listener process that collects
//! audio on the Mac (a Rust process of its own, `overseer-listener`), the state the mark shows, the
//! floor, and the requests. The listener sends words and one loudness level, never the recording;
//! levels and words-in-progress go to the windows on a live channel (`voice.subscribe`) and are
//! never stored (AC-173). Voice Mode is off until the owner turns it on, and muted means the
//! listener has stopped, so the microphone is closed.

mod candidates;
mod floor;
mod model;
mod proc;
pub mod request;

use crate::daemon::Daemon;
use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use tokio::sync::broadcast;

static VOICE: OnceLock<Arc<Voice>> = OnceLock::new();

pub fn voice() -> Option<&'static Arc<Voice>> {
    VOICE.get()
}

/// Whether the simulated voice is on: the listener hears a simulated room instead of the
/// microphone (`OVERSEER_VOICE_SIMULATE=1`, for development and tests; never from a phone).
pub fn simulated() -> bool {
    std::env::var("OVERSEER_VOICE_SIMULATE").is_ok_and(|v| v == "1")
}

/// The settings (docs/rfcs/voice-mode.md#settings), stored under `voice.<name>` in `meta`.
#[derive(Clone, Debug)]
pub struct Settings {
    pub enabled: bool,
    pub muted: bool,
    pub target: String,
    pub floor: String,
    pub delivery: String,
    pub settle_seconds: u64,
    pub speak: String,
    pub voice: String,
    pub rate: u64,
    pub permission_answers: bool,
    pub new_agents_per_request: u64,
    pub requests_per_hour: u64,
    pub keep_days: u64,
    pub model: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: false,
            muted: false,
            target: "overseer".into(),
            floor: "open".into(),
            delivery: "auto".into(),
            settle_seconds: 2,
            speak: "all".into(),
            voice: String::new(),
            rate: 0,
            permission_answers: true,
            new_agents_per_request: 3,
            requests_per_hour: 120,
            keep_days: 30,
            model: "small.en".into(),
        }
    }
}

fn meta(d: &Daemon, key: &str) -> Result<Option<String>> {
    use rusqlite::OptionalExtension;
    let store = d.store.lock().unwrap();
    Ok(store
        .conn
        .query_row("SELECT value FROM meta WHERE key=?1", [key], |r| r.get(0))
        .optional()?)
}

fn set_meta(d: &Daemon, key: &str, value: &str) -> Result<()> {
    let store = d.store.lock().unwrap();
    store.conn.execute("INSERT INTO meta(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value", rusqlite::params![key, value])?;
    Ok(())
}

pub fn settings(d: &Daemon) -> Result<Settings> {
    let mut s = Settings::default();
    let b = |v: Option<String>, default: bool| v.map(|v| v == "1").unwrap_or(default);
    let n = |v: Option<String>, default: u64| v.and_then(|v| v.parse().ok()).unwrap_or(default);
    s.enabled = b(meta(d, "voice.enabled")?, s.enabled);
    s.muted = b(meta(d, "voice.muted")?, s.muted);
    s.target = meta(d, "voice.target")?.unwrap_or(s.target);
    s.floor = meta(d, "voice.floor")?.unwrap_or(s.floor);
    s.delivery = meta(d, "voice.delivery")?.unwrap_or(s.delivery);
    s.settle_seconds = n(meta(d, "voice.settle_seconds")?, s.settle_seconds);
    s.speak = meta(d, "voice.speak")?.unwrap_or(s.speak);
    s.voice = meta(d, "voice.voice")?.unwrap_or(s.voice);
    s.rate = n(meta(d, "voice.rate")?, s.rate);
    s.permission_answers = b(meta(d, "voice.permission_answers")?, s.permission_answers);
    s.new_agents_per_request = n(
        meta(d, "voice.new_agents_per_request")?,
        s.new_agents_per_request,
    );
    s.requests_per_hour = n(meta(d, "voice.requests_per_hour")?, s.requests_per_hour);
    s.keep_days = n(meta(d, "voice.keep_days")?, s.keep_days);
    s.model = meta(d, "voice.model")?.unwrap_or(s.model);
    Ok(s)
}

/// What the voice session knows right now. Nothing here is audio.
#[derive(Default)]
pub struct Live {
    pub ready: bool,
    pub gate: bool,
    pub speaking: bool,
    /// Requests being worked on (Overseer is thinking).
    pub thinking: usize,
    pub failed: Option<String>,
    pub last_state: String,
    pub restarts: Vec<Instant>,
    /// Words heard so far in the utterance going on.
    pub heard: String,
    pub barge: Option<floor::Barge>,
    pub next_line: u64,
    pub focus: Option<String>,
    /// Counts phrase ends and line ends, for the arbiter.
    pub spoke_seq: u64,
    /// Overseer asked a question and waits for the owner's answer.
    pub awaiting_answer: bool,
    pub pid: Option<i32>,
    pub started_at: Option<Instant>,
    pub download: Option<Value>,
    /// Another app is recording (a call): Voice Mode is paused until it is done (AC-173).
    pub paused_for: Option<Vec<String>>,
    /// The listener's or the recognizer's last error, shown in the strip (AC-175).
    pub last_error: Option<(String, i64)>,
}

pub struct Voice {
    pub d: Arc<Daemon>,
    pub live: broadcast::Sender<Value>,
    pub st: Mutex<Live>,
    listener: Mutex<Option<Arc<proc::Listener>>>,
    /// The daemon's async runtime: work started from the listener's threads runs inside it, as
    /// it would from a request (starting Overseer's harness needs it).
    pub rt: Option<tokio::runtime::Handle>,
}

impl Voice {
    /// Runs `f` on the runtime's blocking pool (or a thread, with no runtime).
    pub fn run_blocking(&self, f: impl FnOnce() + Send + 'static) {
        match &self.rt {
            Some(h) => {
                h.spawn_blocking(f);
            }
            None => {
                std::thread::spawn(f);
            }
        }
    }

    /// Sends one message on the live channel (never stored).
    pub fn emit(&self, v: Value) {
        let _ = self.live.send(v);
    }

    pub fn send(&self, cmd: Value) {
        if let Some(l) = self.listener.lock().unwrap().as_ref() {
            if let Err(e) = l.send(&cmd) {
                crate::log(&format!("voice: could not reach the listener: {e}"));
            }
        }
    }

    pub fn running(&self) -> bool {
        self.listener.lock().unwrap().is_some()
    }

    /// Agent names, for the recognizer's hint and the floor's judgement.
    pub fn agent_names(&self) -> Vec<String> {
        self.d
            .roster()
            .map(|r| {
                r.into_iter()
                    .filter(|l| {
                        crate::daemon::ACTIVE.contains(&l.status.as_str())
                            || l.status == "completed"
                    })
                    .map(|l| l.title)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The state the mark shows (AC-177).
    pub fn state(&self, s: &Settings) -> (&'static str, Option<String>) {
        let st = self.st.lock().unwrap();
        if !s.enabled {
            return ("off", st.failed.clone());
        }
        if s.muted {
            return ("muted", None);
        }
        if let Some(f) = &st.failed {
            return ("failed", Some(f.clone()));
        }
        if !st.ready {
            return ("starting", None);
        }
        if let Some(apps) = &st.paused_for {
            return (
                "paused",
                Some(format!(
                    "Paused while {} uses the microphone",
                    apps.join(" and ")
                )),
            );
        }
        if st.gate {
            ("hearing", None)
        } else if st.speaking {
            ("speaking", None)
        } else if st.thinking > 0 {
            ("thinking", None)
        } else {
            ("listening", None)
        }
    }

    /// Tells the windows when the state changes.
    pub fn refresh(&self) {
        let Ok(s) = settings(&self.d) else { return };
        let (state, reason) = self.state(&s);
        let changed = {
            let mut st = self.st.lock().unwrap();
            let changed = st.last_state != state;
            st.last_state = state.to_string();
            changed
        };
        if changed {
            self.emit(
                json!({"kind": "state", "state": state, "reason": reason, "target": s.target}),
            );
        }
    }

    /// Starts or stops the listener to match the settings.
    fn apply(self: &Arc<Self>) -> Result<()> {
        let s = settings(&self.d)?;
        let want = s.enabled && !s.muted;
        let running = self.running();
        if want && !running {
            self.st.lock().unwrap().failed = None;
            self.launch(&s)?;
        } else if !want && running {
            self.stop_listener();
        }
        self.refresh();
        Ok(())
    }

    fn stop_listener(&self) {
        let l = self.listener.lock().unwrap().take();
        if let Some(l) = l {
            l.stop();
        }
        let mut st = self.st.lock().unwrap();
        st.ready = false;
        st.gate = false;
        st.speaking = false;
        st.pid = None;
        st.heard.clear();
    }

    fn launch(self: &Arc<Self>, s: &Settings) -> Result<()> {
        let path = listener_path().ok_or_else(|| {
            anyhow!("the listener (overseer-listener) is not installed next to the daemon")
        })?;
        // The speech model loads only inside Gate L's memory budget (AC-173); the simulated voice
        // loads none, unless a test asks for the check.
        if !simulated() || std::env::var("OVERSEER_VOICE_TEST_BUDGET").is_ok_and(|v| v == "1") {
            model::check_budget(&s.model)?;
        }
        let mut args: Vec<String> = Vec::new();
        if simulated() {
            args.extend(["--input", "sim", "--script-live"].map(String::from));
        } else {
            let model = model::path(&s.model);
            if !model.exists() {
                bail!("the speech model ({}) is not downloaded", s.model);
            }
            args.extend([
                "--input".into(),
                "mic".into(),
                "--model".into(),
                model.display().to_string(),
            ]);
        }
        let names = self.agent_names().join(", ");
        if !names.is_empty() {
            args.extend(["--hint".into(), names]);
        }
        if !s.voice.is_empty() {
            args.extend(["--voice".into(), s.voice.clone()]);
        }
        if s.rate > 0 {
            args.extend(["--rate".into(), s.rate.to_string()]);
        }
        args.extend([
            "--lock".into(),
            crate::paths::data_dir()
                .join("voice-listener.lock")
                .display()
                .to_string(),
        ]);
        let (listener, stdout) = proc::Listener::spawn(&path, &args, &[])?;
        let listener = Arc::new(listener);
        let pid = listener.pid;
        *self.listener.lock().unwrap() = Some(listener.clone());
        {
            let mut st = self.st.lock().unwrap();
            st.pid = Some(pid);
            st.ready = false;
            st.started_at = Some(Instant::now());
        }
        let v = self.clone();
        std::thread::spawn(move || {
            use std::io::BufRead;
            let _in_runtime = v.rt.as_ref().map(|h| h.enter());
            for line in std::io::BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if let Ok(event) = serde_json::from_str::<Value>(&line) {
                    v.on_event(event);
                }
            }
            let how = proc::reap(pid);
            v.on_exit(&listener, how);
        });
        Ok(())
    }

    /// The listener ended. Unless it was asked to, it is restarted, at most three times in ten
    /// minutes; a fourth end leaves Voice Mode off with the reason (AC-175).
    fn on_exit(self: &Arc<Self>, which: &Arc<proc::Listener>, how: String) {
        let ours = self
            .listener
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|l| Arc::ptr_eq(l, which));
        if !ours {
            return; // stopped on purpose
        }
        *self.listener.lock().unwrap() = None;
        crate::log(&format!("voice: the listener {how}"));
        let give_up = {
            let mut st = self.st.lock().unwrap();
            st.ready = false;
            st.gate = false;
            st.speaking = false;
            st.pid = None;
            let now = Instant::now();
            st.restarts
                .retain(|t| now.duration_since(*t) < Duration::from_secs(600));
            st.restarts.push(now);
            st.restarts.len() > 3
        };
        if give_up {
            let reason = format!("the listener stopped four times in ten minutes (last: {how})");
            let _ = set_meta(&self.d, "voice.enabled", "0");
            self.st.lock().unwrap().failed = Some(reason.clone());
            self.emit(json!({"kind": "state", "state": "off", "reason": reason}));
            let _ = self.d.emit(
                None,
                None,
                "voice_settings",
                "voice",
                "exact",
                json!({"enabled": false, "reason": reason}),
            );
            self.st.lock().unwrap().last_state = "off".into();
            return;
        }
        self.emit(json!({"kind": "listener", "event": "restarting", "why": how}));
        if let Err(e) = self.apply() {
            self.st.lock().unwrap().failed = Some(e.to_string());
            self.refresh();
        }
    }

    /// One event from the listener.
    fn on_event(self: &Arc<Self>, e: Value) {
        match e["type"].as_str().unwrap_or("") {
            "paused" => {
                // Said in the strip only: the owner is on a call.
                let apps: Vec<String> = e["apps"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|x| x.as_str().map(|n| app_name(n)))
                            .collect()
                    })
                    .unwrap_or_default();
                {
                    let mut st = self.st.lock().unwrap();
                    st.paused_for = Some(apps.clone());
                    st.gate = false;
                }
                self.emit(json!({"kind": "listener", "event": "paused", "apps": apps}));
            }
            "resumed" => {
                self.st.lock().unwrap().paused_for = None;
                self.emit(json!({"kind": "listener", "event": "resumed"}));
            }
            "ready" => {
                self.st.lock().unwrap().ready = true;
                self.emit(json!({"kind": "listener", "event": "ready", "input": e["input"], "model": e["model"]}));
            }
            "gate" => {
                let open = e["open"].as_bool().unwrap_or(false);
                let mut st = self.st.lock().unwrap();
                st.gate = open;
                if open {
                    st.heard.clear();
                }
            }
            "level" => {
                self.emit(json!({"kind": "level", "source": e["source"], "value": e["value"]}));
            }
            "words" => {
                let text = e["text"].as_str().unwrap_or("").to_string();
                self.st.lock().unwrap().heard = text.clone();
                self.emit(json!({"kind": "heard", "id": e["id"], "text": text, "final": false}));
            }
            "utterance" => {
                let text = e["text"].as_str().unwrap_or("").to_string();
                {
                    let mut st = self.st.lock().unwrap();
                    st.heard.clear();
                    if st
                        .barge
                        .as_ref()
                        .is_some_and(|b| Some(b.utterance) == e["id"].as_u64())
                    {
                        st.barge = None;
                    }
                }
                self.emit(json!({"kind": "heard", "id": e["id"], "text": text, "final": true}));
                let v = self.clone();
                let complete = e["complete"].as_bool().unwrap_or(true);
                self.run_blocking(move || {
                    v.on_utterance(&text, complete, "voice");
                });
            }
            "dropped" => {
                self.emit(json!({"kind": "dropped", "id": e["id"], "reason": e["reason"]}));
            }
            "barge" => {
                let text = e["text"].as_str().unwrap_or("");
                self.emit(json!({"kind": "floor", "event": if e["stop_word"] == true { "stopped" } else { "lowered" }, "words": text}));
                self.on_barge(e["id"].as_u64().unwrap_or(0), text, e["stop_word"] == true);
            }
            "spoke" => {
                let event = e["event"].as_str().unwrap_or("");
                {
                    let mut st = self.st.lock().unwrap();
                    match event {
                        "start" => st.speaking = true,
                        "stopped" | "done" => {
                            st.speaking = false;
                            st.spoke_seq += 1;
                        }
                        "phrase" => st.spoke_seq += 1,
                        _ => {}
                    }
                }
                self.emit(json!({"kind": "spoke", "line": e["line"], "event": event}));
                if matches!(event, "stopped" | "done") {
                    request::spoke_done(self, e["line"].as_u64().unwrap_or(0), event);
                }
            }
            "error" => {
                let message = e["message"].as_str().unwrap_or("").to_string();
                crate::log(&format!("voice: listener: {message}"));
                self.st.lock().unwrap().last_error = Some((message.clone(), crate::daemon::now()));
                self.emit(json!({"kind": "listener", "event": "error", "message": message}));
            }
            _ => {}
        }
        self.refresh();
    }

    /// Says a line (Overseer's voice), unless the owner turned speech off. Returns its number.
    pub fn say_line(&self, text: &str) -> Option<u64> {
        let s = settings(&self.d).ok()?;
        if s.speak == "none" || !self.running() {
            return None;
        }
        let line = {
            let mut st = self.st.lock().unwrap();
            st.next_line += 1;
            st.next_line
        };
        self.send(json!({"cmd": "speak", "line": line, "text": text}));
        // What Overseer says is also shown (a caption in the view, and the tests' record).
        self.emit(json!({"kind": "say", "line": line, "text": text}));
        Some(line)
    }
}

/// Where the listener is: `OVERSEER_LISTENER`, else inside `Overseer Listener.app` next to the
/// daemon (the packaged extension), else `overseer-listener` next to it (a development build).
pub fn listener_path() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("OVERSEER_LISTENER") {
        return Some(PathBuf::from(p));
    }
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    [
        dir.join("Overseer Listener.app/Contents/MacOS/overseer-listener"),
        dir.join("overseer-listener"),
    ]
    .into_iter()
    .find(|p| p.exists())
}

/// Starts the voice session with the daemon.
pub fn start(d: Arc<Daemon>) {
    let (live, _) = broadcast::channel(1024);
    let v = Arc::new(Voice {
        d,
        live,
        st: Mutex::new(Live::default()),
        listener: Mutex::new(None),
        rt: tokio::runtime::Handle::try_current().ok(),
    });
    if VOICE.set(v.clone()).is_err() {
        return;
    }
    if let Err(e) = v.apply() {
        crate::log(&format!("voice: not started: {e}"));
        v.st.lock().unwrap().failed = Some(e.to_string());
    }
    // Keep the recognizer's hint to the agents that run.
    let h = v.clone();
    std::thread::spawn(move || {
        let mut last = String::new();
        loop {
            std::thread::sleep(Duration::from_secs(20));
            if !h.running() {
                continue;
            }
            let names = h.agent_names().join(", ");
            if names != last {
                h.send(json!({"cmd": "hint", "text": names}));
                last = names;
            }
        }
    });
    request::start(&v);
}

fn get_voice() -> Result<&'static Arc<Voice>> {
    voice().ok_or_else(|| anyhow!("the voice session is not running"))
}

/// `voice.get`: the settings, the state, and what Voice Mode needs.
pub fn get(d: &Arc<Daemon>) -> Result<Value> {
    let s = settings(d)?;
    let v = get_voice()?;
    let (state, reason) = v.state(&s);
    // Before the state's lock: the targets read the request list and the cards.
    let targeted = request::targeted(d);
    let st = v.st.lock().unwrap();
    let model_path = model::path(&s.model);
    Ok(json!({
        "enabled": s.enabled,
        "muted": s.muted,
        "state": state,
        "reason": reason,
        "available": cfg!(target_os = "macos") && listener_path().is_some(),
        "simulated": simulated(),
        "target": s.target,
        "focus": st.focus,
        "settings": {
            "floor": s.floor, "delivery": s.delivery, "settle_seconds": s.settle_seconds, "speak": s.speak, "voice": s.voice, "rate": s.rate,
            "permission_answers": s.permission_answers, "new_agents_per_request": s.new_agents_per_request, "requests_per_hour": s.requests_per_hour,
            "keep_days": s.keep_days, "model": s.model,
            "start_defaults": meta(d, "voice.start_defaults").ok().flatten().and_then(|j| serde_json::from_str::<Value>(&j).ok()),
        },
        "model": {"name": s.model, "downloaded": model_path.exists() || simulated(), "bytes": model::size(&s.model), "download": st.download},
        "targeted": targeted,
        "listener": {"running": st.pid.is_some(), "pid": st.pid, "restarts": st.restarts.len(),
            "last_error": st.last_error.as_ref().map(|(m, at)| json!({"message": m, "at": at}))},
    }))
}

/// `voice.set`: changes settings, each checked; turning Voice Mode on starts the listener.
pub fn set(d: &Arc<Daemon>, p: &Value) -> Result<Value> {
    let obj = p
        .as_object()
        .ok_or_else(|| anyhow!("params must be an object"))?;
    if obj.is_empty() {
        bail!("say what to change");
    }
    let mut changes: Vec<(&str, String)> = Vec::new();
    let flag = |k: &str| -> Result<Option<String>> {
        match p.get(k) {
            None => Ok(None),
            Some(v) => Ok(Some(
                if v.as_bool()
                    .ok_or_else(|| anyhow!("{k} must be true or false"))?
                {
                    "1".into()
                } else {
                    "0".into()
                },
            )),
        }
    };
    let choice = |k: &str, allowed: &[&str]| -> Result<Option<String>> {
        match p.get(k) {
            None => Ok(None),
            Some(v) => {
                let s = v.as_str().ok_or_else(|| anyhow!("{k} must be text"))?;
                if !allowed.contains(&s) {
                    bail!("{k} must be one of {}", allowed.join(", "));
                }
                Ok(Some(s.to_string()))
            }
        }
    };
    let number = |k: &str, lo: u64, hi: u64| -> Result<Option<String>> {
        match p.get(k) {
            None => Ok(None),
            Some(v) => {
                let n = v
                    .as_u64()
                    .ok_or_else(|| anyhow!("{k} must be a whole number"))?;
                if n < lo || n > hi {
                    bail!("{k} must be from {lo} to {hi}");
                }
                Ok(Some(n.to_string()))
            }
        }
    };
    for (k, key) in [
        ("enabled", "voice.enabled"),
        ("muted", "voice.muted"),
        ("permission_answers", "voice.permission_answers"),
    ] {
        if let Some(v) = flag(k)? {
            changes.push((key, v));
        }
    }
    if let Some(v) = choice("floor", &["open", "name_first", "push_to_talk"])? {
        changes.push(("voice.floor", v));
    }
    if let Some(v) = choice("delivery", &["auto", "add", "redirect"])? {
        changes.push(("voice.delivery", v));
    }
    if let Some(v) = choice("speak", &["all", "first", "none"])? {
        changes.push(("voice.speak", v));
    }
    if let Some(v) = choice("model", model::NAMES)? {
        changes.push(("voice.model", v));
    }
    for (k, key, lo, hi) in [
        ("settle_seconds", "voice.settle_seconds", 0, 10),
        (
            "new_agents_per_request",
            "voice.new_agents_per_request",
            0,
            8,
        ),
        ("requests_per_hour", "voice.requests_per_hour", 10, 600),
        ("keep_days", "voice.keep_days", 1, 365),
    ] {
        if let Some(v) = number(k, lo, hi)? {
            changes.push((key, v));
        }
    }
    if let Some(v) = p.get("rate") {
        let n = v
            .as_u64()
            .ok_or_else(|| anyhow!("rate must be a whole number"))?;
        if n != 0 && !(90..=360).contains(&n) {
            bail!("rate must be 0 (the voice's own) or from 90 to 360 words a minute");
        }
        changes.push(("voice.rate", n.to_string()));
    }
    if let Some(v) = p.get("voice") {
        let name = v.as_str().ok_or_else(|| anyhow!("voice must be text"))?;
        if !name.is_empty() && !crate::audio::voice_installed(name)? {
            bail!("the voice {name} is not installed on this Mac");
        }
        changes.push(("voice.voice", name.to_string()));
    }
    if let Some(v) = p.get("target") {
        let t = v.as_str().ok_or_else(|| anyhow!("target must be text"))?;
        changes.push(("voice.target", request::check_target(d, t)?));
    }
    // The composer's remembered choices (AC-59), which agents started by voice take (AC-168).
    if let Some(v) = p.get("start_defaults") {
        let o = v
            .as_object()
            .ok_or_else(|| anyhow!("start_defaults must be an object"))?;
        let mut kept = serde_json::Map::new();
        for k in ["harness", "profile_id", "model", "workspace_mode"] {
            if let Some(x) = o.get(k).and_then(|x| x.as_str()).filter(|x| !x.is_empty()) {
                kept.insert(k.into(), json!(x));
            }
        }
        if let Some(t) = o.get("trusted").and_then(|t| t.as_bool()) {
            kept.insert("trusted".into(), json!(t));
        }
        if let Some(m) = kept.get("workspace_mode").and_then(|m| m.as_str()) {
            if !["worktree", "current"].contains(&m) {
                bail!("workspace_mode must be worktree or current");
            }
        }
        changes.push(("voice.start_defaults", Value::Object(kept).to_string()));
    }
    if changes.is_empty() {
        bail!(
            "nothing to change: unknown settings {:?}",
            obj.keys().collect::<Vec<_>>()
        );
    }
    let turning_on = changes
        .iter()
        .any(|(k, v)| *k == "voice.enabled" && v == "1");
    if turning_on {
        if !cfg!(target_os = "macos") {
            bail!("Voice Mode is available on macOS only");
        }
        if listener_path().is_none() {
            bail!("the listener (overseer-listener) is not installed next to the daemon");
        }
        let model = changes
            .iter()
            .find(|(k, _)| *k == "voice.model")
            .map(|(_, v)| v.clone())
            .unwrap_or(settings(d)?.model);
        if !simulated() && !model::path(&model).exists() {
            bail!(
                "the speech model ({model}, {} MiB) is not downloaded: voice.download first",
                model::size(&model) / (1024 * 1024)
            );
        }
    }
    let target_changed = changes
        .iter()
        .find(|(k, _)| *k == "voice.target")
        .map(|(_, v)| v.clone());
    for (k, v) in &changes {
        set_meta(d, k, v)?;
    }
    let v = get_voice()?;
    if turning_on {
        // The owner turned it on again: the crash count starts over (AC-175).
        v.st.lock().unwrap().restarts.clear();
    }
    let settings_changed: Vec<&str> = changes
        .iter()
        .map(|(k, _)| k.trim_start_matches("voice."))
        .collect();
    // Settings changes are ordinary events (no audio, no words).
    let _ = d.emit(
        None,
        None,
        "voice_settings",
        "voice",
        "exact",
        json!({"changed": settings_changed}),
    );
    let restart = changes
        .iter()
        .any(|(k, _)| matches!(*k, "voice.voice" | "voice.rate" | "voice.model"));
    if restart && v.running() {
        v.stop_listener();
    }
    v.apply().inspect_err(|e| {
        v.st.lock().unwrap().failed = Some(e.to_string());
        v.refresh();
    })?;
    if let Some(t) = target_changed {
        v.emit(json!({"kind": "target", "target": t}));
    }
    get(d)
}

/// `voice.say`: words as if heard (the words layer). The listener's utterances take the same path.
pub fn say(d: &Arc<Daemon>, p: &Value) -> Result<Value> {
    let text = p["text"]
        .as_str()
        .ok_or_else(|| anyhow!("text is required"))?
        .trim()
        .to_string();
    let s = settings(d)?;
    if !s.enabled || s.muted {
        bail!("Voice Mode is not listening");
    }
    let v = get_voice()?;
    let complete = p["complete"].as_bool().unwrap_or(true);
    v.emit(json!({"kind": "heard", "text": text, "final": true, "via": "say"}));
    Ok(v.on_utterance(&text, complete, "say"))
}

/// `voice.simulate` (simulated voice only): mixes speech, a made-up voice or a noise into the
/// listener's simulated room.
pub fn simulate(_d: &Arc<Daemon>, p: &Value) -> Result<Value> {
    if !simulated() {
        bail!("the simulated voice is off (start the daemon with OVERSEER_VOICE_SIMULATE=1)");
    }
    let v = get_voice()?;
    if !v.running() {
        bail!("Voice Mode is not listening");
    }
    let mut cmd = json!({"cmd": "simulate"});
    for k in ["speech", "voice", "speechlike", "noise", "words", "gain"] {
        if let Some(x) = p.get(k) {
            cmd[k] = x.clone();
        }
    }
    v.send(cmd);
    Ok(json!({"sent": true}))
}

/// `voice.speak` (simulated voice only): makes Overseer say a line.
pub fn speak(_d: &Arc<Daemon>, p: &Value) -> Result<Value> {
    if !simulated() {
        bail!("the simulated voice is off (start the daemon with OVERSEER_VOICE_SIMULATE=1)");
    }
    let text = p["text"]
        .as_str()
        .ok_or_else(|| anyhow!("text is required"))?;
    let v = get_voice()?;
    Ok(json!({"line": v.say_line(text)}))
}

/// `voice.focus`: the agent the owner has selected or is tracking, for context.
pub fn focus(_d: &Arc<Daemon>, p: &Value) -> Result<Value> {
    let v = get_voice()?;
    v.st.lock().unwrap().focus = p["run_id"].as_str().map(String::from);
    Ok(json!({"focus": p["run_id"]}))
}

/// `voice.download`: fetches the speech model, checked against its SHA-256.
pub fn download(d: &Arc<Daemon>, _p: &Value) -> Result<Value> {
    let s = settings(d)?;
    let v = get_voice()?;
    model::download(v, &s.model)
}

/// A connection that asked for `voice.subscribe`: the live channel, never stored.
pub fn subscribe(id: Value, tx: tokio::sync::mpsc::Sender<Value>) {
    let Some(v) = voice() else {
        let _ = tx.try_send(json!({"id": id, "error": {"code": "failed", "message": "the voice session is not running"}}));
        return;
    };
    let mut rx = v.live.subscribe();
    let first = get(&v.d).unwrap_or(json!({}));
    tokio::spawn(async move {
        if tx
            .send(json!({"id": id, "result": {"subscribed": true, "voice": first}}))
            .await
            .is_err()
        {
            return;
        }
        loop {
            match rx.recv().await {
                Ok(m) => {
                    if tx
                        .send(json!({"method": "voice", "params": m}))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => break,
            }
        }
    });
}

/// Before an Audio Mode cue plays (AC-172).
pub fn before_cue(key: &str, cue_ms: u64) -> bool {
    match voice() {
        Some(v) => v.before_cue(key, cue_ms),
        None => true,
    }
}

/// A readable name for an app that is recording: "us.zoom.xos" reads as "zoom".
fn app_name(id: &str) -> String {
    const KNOWN: &[(&str, &str)] = &[
        ("us.zoom.xos", "Zoom"),
        ("com.apple.FaceTime", "FaceTime"),
        ("com.microsoft.teams", "Teams"),
        ("com.microsoft.teams2", "Teams"),
        ("com.tinyspeck.slackmacgap", "Slack"),
        ("com.google.Chrome", "Chrome"),
        ("com.apple.Safari", "Safari"),
        ("com.hnc.Discord", "Discord"),
        ("com.apple.QuickTimePlayerX", "QuickTime"),
        ("com.apple.VoiceMemos", "Voice Memos"),
    ];
    KNOWN
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(id))
        .map(|(_, n)| n.to_string())
        .unwrap_or_else(|| {
            if id.contains('.') && !id.contains(' ') {
                id.rsplit('.').next().unwrap_or(id).to_string()
            } else {
                id.to_string()
            }
        })
}
