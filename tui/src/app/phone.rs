//! Phone access from the terminal (Gate N): the switch (`O`), the Devices panel (Ctrl-O) with
//! revoke and scope, and pairing with the code as text and as a QR code. The daemon holds the
//! state: one `gateway.status` per connection and one after each change it reports. Nothing polls.

use super::{short, App, Confirm, Mode, Pending};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde_json::{json, Value};
use std::time::{Duration, Instant};

/// Daemon events that change what the terminal shows about phones.
const EVENTS: [&str; 9] = ["gateway_state", "gateway_sessions", "pairing_opened", "pairing_request", "pairing_closed", "device_paired", "device_revoked", "device_scope", "power"];

pub fn is_phone_event(kind: &str) -> bool {
    EVENTS.contains(&kind)
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Device {
    pub id: String,
    pub name: String,
    pub platform: String,
    pub scope: String,
    pub fingerprint: String,
    pub paired_ms: i64,
    pub last_seen_ms: Option<i64>,
    pub address: Option<String>,
    pub connected: bool,
}

impl Device {
    pub fn platform_name(&self) -> &'static str {
        platform_name(&self.platform)
    }
    pub fn scope_name(&self) -> &'static str {
        if self.scope == "watch" {
            "Watch only"
        } else {
            "Full control"
        }
    }
    /// "connected", "last seen 5m ago" or "not seen yet".
    pub fn presence(&self, now_ms: i64) -> String {
        if self.connected {
            return "connected".into();
        }
        match self.last_seen_ms {
            Some(ms) => format!("last seen {}", ago(ms, now_ms)),
            None => "not seen yet".into(),
        }
    }
}

pub fn platform_name(platform: &str) -> &'static str {
    match platform {
        "ios" => "iPhone",
        "android" => "Android",
        _ => "Phone",
    }
}

/// "just now", "5m ago", "2h ago", "3d ago".
pub fn ago(ms: i64, now_ms: i64) -> String {
    let s = ((now_ms - ms) / 1000).max(0);
    match s {
        0..=59 => "just now".into(),
        60..=3599 => format!("{}m ago", s / 60),
        3600..=86_399 => format!("{}h ago", s / 3600),
        _ => format!("{}d ago", s / 86_400),
    }
}

/// A key fingerprint in groups of four.
pub fn fingerprint(f: &str) -> String {
    f.chars().collect::<Vec<_>>().chunks(4).map(|c| c.iter().collect::<String>()).collect::<Vec<_>>().join(" ")
}

/// The code in groups of four for the eye: `OVSR1-` then the rest. A phone reads it with or
/// without the spaces.
pub fn code_groups(code: &str) -> Vec<String> {
    let (head, rest) = match code.strip_prefix("OVSR1-") {
        Some(rest) => (vec!["OVSR1-".to_string()], rest),
        None => (Vec::new(), code),
    };
    head.into_iter().chain(rest.chars().collect::<Vec<_>>().chunks(4).map(|c| c.iter().collect())).collect()
}

/// A phone that asked to pair and waits for the owner.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PairRequest {
    pub request: String,
    pub name: String,
    pub platform: String,
    pub address: String,
    pub fingerprint: String,
}

impl PairRequest {
    fn from(v: &Value) -> PairRequest {
        let s = |k: &str| v[k].as_str().unwrap_or_default().to_string();
        PairRequest { request: s("request"), name: s("name"), platform: s("platform"), address: s("address"), fingerprint: s("fingerprint") }
    }
    /// The question, on one line.
    pub fn question(&self) -> String {
        format!("Pair \"{}\"? {} · {} · key {}.", short(&self.name, 40), platform_name(&self.platform), self.address, fingerprint(&self.fingerprint))
    }
}

/// What the daemon says about phone access.
#[derive(Debug, Clone, Default)]
pub struct Phone {
    /// `None` until the daemon answered; `Some(false)` when this daemon has no phone access.
    pub available: Option<bool>,
    pub enabled: bool,
    pub port: Option<u16>,
    pub mac: String,
    /// Paired devices; a revoked one is gone from the list.
    pub devices: Vec<Device>,
    pub waiting: Vec<PairRequest>,
    /// The Mac's switch for notifications to every phone.
    pub notifications: bool,
    pub awake: bool,
    /// The selected row of the Devices panel.
    pub sel: usize,
}

impl Phone {
    /// Phones connected now (one phone with two sessions is one phone).
    pub fn connected(&self) -> usize {
        self.devices.iter().filter(|d| d.connected).count()
    }

    /// The status line: the long and the short form, and whether phone access is on.
    pub fn line(&self) -> Option<(String, String, bool)> {
        if self.available != Some(true) {
            return None;
        }
        if !self.enabled {
            return Some(("phone access off".into(), "phones off".into(), false));
        }
        Some(match self.connected() {
            0 => ("phone access on".into(), "phones on".into(), true),
            n => (format!("phone access on · {n} phone{}", if n == 1 { "" } else { "s" }), format!("{n} phone{}", if n == 1 { "" } else { "s" }), true),
        })
    }

    fn load(&mut self, v: &Value) {
        self.available = Some(true);
        self.enabled = v["enabled"].as_bool().unwrap_or(false);
        self.port = v["port"].as_u64().map(|p| p as u16);
        self.mac = v["name"].as_str().unwrap_or("this Mac").to_string();
        self.notifications = v["settings"]["notifications"].as_bool().unwrap_or(true);
        self.awake = v["awake"].as_bool().unwrap_or(false);
        let keep = self.devices.get(self.sel).map(|d| d.id.clone());
        self.devices = v["devices"].as_array().into_iter().flatten().filter(|d| d["revoked_ms"].is_null()).map(|d| {
            let s = |k: &str| d[k].as_str().unwrap_or_default().to_string();
            Device { id: s("id"), name: s("name"), platform: s("platform"), scope: s("scope"), fingerprint: s("fingerprint"), paired_ms: d["paired_ms"].as_i64().unwrap_or(0),
                last_seen_ms: d["last_seen_ms"].as_i64(), address: d["address"].as_str().map(str::to_string), connected: d["connected"].as_bool().unwrap_or(false) }
        }).collect();
        // Connected first, then the most recently seen.
        self.devices.sort_by(|a, b| b.connected.cmp(&a.connected).then(b.last_seen_ms.unwrap_or(b.paired_ms).cmp(&a.last_seen_ms.unwrap_or(a.paired_ms))).then(a.name.cmp(&b.name)));
        self.sel = keep.and_then(|k| self.devices.iter().position(|d| d.id == k)).unwrap_or(0).min(self.devices.len().saturating_sub(1));
        self.waiting = v["pairing"]["waiting"].as_array().into_iter().flatten().map(PairRequest::from).collect();
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum PairingState {
    /// The code is shown and works.
    Open,
    /// A phone presented the code; the owner decides.
    Waiting(String),
    Paired(String),
    /// The code stopped working: why.
    Over(String),
}

/// What the pairing panel shows (only in the terminal that started pairing).
#[derive(Debug, Clone)]
pub struct Pairing {
    pub code: String,
    pub until: Instant,
    pub mac: String,
    pub state: PairingState,
}

impl Pairing {
    pub fn seconds_left(&self, now: Instant) -> u64 {
        let left = self.until.saturating_duration_since(now);
        left.as_secs() + u64::from(left.subsec_nanos() > 0)
    }
    /// "1:59"
    pub fn left(&self, now: Instant) -> String {
        let s = self.seconds_left(now);
        format!("{}:{:02}", s / 60, s % 60)
    }
    pub fn live(&self) -> bool {
        matches!(self.state, PairingState::Open | PairingState::Waiting(_))
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

/// Why pairing ended, in the owner's words.
fn ended(reason: &str, name: Option<&str>) -> String {
    if reason.contains("declined") || reason.contains("not confirmed") {
        return match name {
            Some(n) => format!("\"{n}\" was not paired."),
            None => "The phone was not paired.".into(),
        };
    }
    if reason.contains("too many") {
        return "Pairing stopped after too many wrong codes.".into();
    }
    if reason.contains("cancelled") {
        return "Pairing was cancelled.".into();
    }
    "This code no longer works.".into()
}

impl App {
    /// One `gateway.status` (on connect, and after the daemon reports a change).
    pub(super) fn phone_request(&mut self) {
        self.phone_due = None;
        if self.connected {
            self.request("gateway.status", json!({}), Pending::PhoneStatus);
        }
    }

    fn phone_soon(&mut self) {
        if self.phone_due.is_none() {
            self.phone_due = Some(Instant::now() + Duration::from_millis(60));
        }
    }

    /// Timers: the debounced status, and the pairing clock. True when the screen changed.
    pub(super) fn phone_tick(&mut self, now: Instant) -> bool {
        if self.phone_due.is_some_and(|due| now >= due) {
            self.phone_request();
        }
        let mut changed = false;
        if let Some(p) = &mut self.pairing {
            if p.state == PairingState::Open {
                if now >= p.until {
                    p.state = PairingState::Over("This code has expired.".into());
                    changed = true;
                } else if self.mode == Mode::Pairing {
                    let s = p.seconds_left(now);
                    if s != self.pairing_shown {
                        self.pairing_shown = s;
                        changed = true;
                    }
                }
            }
        }
        changed
    }

    pub(super) fn on_phone_event(&mut self, kind: &str, ev: &Value) {
        let p = &ev["payload"];
        self.phone_soon();
        match kind {
            "pairing_request" => {
                // A request the phone has stopped waiting for is history, not a question.
                let stale = match (ev["ts"].as_i64(), p["wait_ms"].as_i64()) {
                    (Some(ts), Some(wait)) => now_ms() - ts > wait,
                    _ => false,
                };
                if !stale {
                    self.ask_to_pair(PairRequest::from(p));
                }
            }
            "pairing_closed" => {
                let request = p["request"].as_str().unwrap_or_default();
                let name = p["name"].as_str();
                if let Mode::Confirm(Confirm::Pair { request: open, .. }) = &self.mode {
                    if open == request {
                        self.mode = self.confirm_back.take().unwrap_or(Mode::Grid);
                        self.say(format!("\"{}\" stopped waiting to pair", name.unwrap_or("The phone")), false);
                    }
                }
                let reason = p["reason"].as_str().unwrap_or_default();
                if let Some(pairing) = self.pairing.as_mut().filter(|x| x.live()) {
                    pairing.state = PairingState::Over(ended(reason, name));
                }
            }
            "device_paired" => {
                let name = p["name"].as_str().unwrap_or("the phone").to_string();
                if matches!(self.mode, Mode::Confirm(Confirm::Pair { .. })) {
                    // Answered in VS Code or in another terminal.
                    self.mode = self.confirm_back.take().unwrap_or(Mode::Grid);
                }
                if let Some(pairing) = self.pairing.as_mut().filter(|x| x.live()) {
                    pairing.state = PairingState::Paired(name.clone());
                }
                self.say(format!("Paired with \"{}\"", short(&name, 40)), false);
            }
            "pairing_opened" => {
                // A code made somewhere else replaces the one shown here.
                if self.pair_starting > 0 {
                    self.pair_starting -= 1;
                } else if let Some(pairing) = self.pairing.as_mut().filter(|x| x.live()) {
                    pairing.state = PairingState::Over("A new code was made somewhere else, so this one no longer works.".into());
                }
            }
            "gateway_state" if p["state"] == "off" => {
                if let Some(pairing) = self.pairing.as_mut().filter(|x| x.live()) {
                    pairing.state = PairingState::Over("Phone access was turned off.".into());
                }
            }
            _ => {}
        }
    }

    /// A phone asks to pair. The question opens where the owner is looking at phones; elsewhere
    /// a key typed for something else must never answer it, so a notice points to Ctrl-O.
    fn ask_to_pair(&mut self, request: PairRequest) {
        if request.request.is_empty() || self.pair_asked.contains(&request.request) {
            return;
        }
        if let Some(pairing) = self.pairing.as_mut().filter(|x| x.live()) {
            pairing.state = PairingState::Waiting(request.name.clone());
        }
        if matches!(self.mode, Mode::Devices | Mode::Pairing) {
            self.pair_asked.insert(request.request.clone());
            self.confirm_back = Some(self.mode.clone());
            self.mode = Mode::Confirm(Confirm::Pair { text: request.question(), request: request.request });
        } else if !matches!(self.mode, Mode::Confirm(Confirm::Pair { .. })) {
            self.bell = true;
            self.say(format!("◆ \"{}\" asks to pair — press Ctrl-O to answer", short(&request.name, 40)), false);
        }
    }

    pub(super) fn on_phone_reply(&mut self, why: Pending, result: Result<Value, String>) {
        match (why, result) {
            (Pending::PhoneStatus, Ok(v)) => {
                self.phone.load(&v);
                // The daemon no longer holds the pairing shown here (it restarted, or pairing ended while away).
                // (A status asked for before the pairing began says nothing about it.)
                let settled = self.pairing_since.is_some_and(|since| since.elapsed() > Duration::from_millis(400));
                if v["pairing"].is_null() && self.pair_starting == 0 && settled {
                    let enabled = self.phone.enabled;
                    if let Some(pairing) = self.pairing.as_mut().filter(|x| x.live() && x.until > Instant::now()) {
                        pairing.state = PairingState::Over(if enabled { "This code no longer works.".into() } else { "Phone access was turned off.".into() });
                    }
                }
                if matches!(self.mode, Mode::Devices | Mode::Pairing) {
                    if let Some(next) = self.phone.waiting.iter().find(|w| !self.pair_asked.contains(&w.request)).cloned() {
                        self.ask_to_pair(next);
                    }
                }
            }
            (Pending::PhoneStatus, Err(e)) => {
                // A daemon from before phone access: say nothing rather than something false.
                if e.contains("unknown method") || e.contains("unknown_method") {
                    self.phone.available = Some(false);
                } else {
                    self.say(format!("phone access: {e}"), true);
                }
            }
            (Pending::PhoneSwitch { on, then_pair }, Ok(v)) => {
                self.phone.load(&v);
                self.say(if on { "Phone access is on" } else { "Phone access is off" }, false);
                if on && then_pair {
                    self.pair_start();
                }
            }
            (Pending::PairStart, Ok(v)) => {
                let valid = v["valid_ms"].as_u64().unwrap_or(120_000);
                self.pairing = Some(Pairing { code: v["code"].as_str().unwrap_or_default().to_string(), until: Instant::now() + Duration::from_millis(valid), mac: v["name"].as_str().unwrap_or("this Mac").to_string(), state: PairingState::Open });
                self.pairing_since = Some(Instant::now());
                self.pairing_shown = 0;
                self.mode = Mode::Pairing;
            }
            (Pending::PairStart, Err(e)) => {
                self.pair_starting = self.pair_starting.saturating_sub(1);
                self.say(format!("Pairing did not start: {e}"), true);
            }
            (Pending::PairCancel, _) => {}
            (Pending::PairConfirm { accept, name }, Ok(_)) => {
                if !accept {
                    self.say(format!("\"{}\" was not paired", short(&name, 40)), false);
                }
                self.phone_soon();
            }
            (Pending::PairConfirm { name, .. }, Err(e)) => {
                // One answer wins: VS Code or another terminal answered first, or the phone gave up.
                if e.contains("no phone is waiting") || e.contains("stopped waiting") {
                    self.say(format!("\"{}\" is no longer waiting to pair", short(&name, 40)), false);
                } else {
                    self.say(e, true);
                }
                self.phone_soon();
            }
            (Pending::DeviceRevoke(name), Ok(_)) => {
                self.say(format!("Revoked \"{}\"", short(&name, 40)), false);
                self.phone_request();
            }
            (Pending::DeviceScope { name, scope }, Ok(_)) => {
                self.say(if scope == "watch" { format!("\"{}\" can watch only", short(&name, 40)) } else { format!("\"{}\" has full control", short(&name, 40)) }, false);
                self.phone_request();
            }
            (Pending::PhoneNotifications(on), Ok(v)) => {
                self.phone.notifications = v["notifications"].as_bool().unwrap_or(on);
                self.say(if self.phone.notifications { "Notifications to phones are on" } else { "Notifications to phones are off; nothing is sent" }, false);
            }
            (_, Err(e)) => self.say(e, true),
            _ => {}
        }
    }

    fn phone_missing(&mut self) -> bool {
        match self.phone.available {
            Some(true) => false,
            Some(false) => {
                self.say("This Overseer daemon has no phone access. Restart the daemon to update it.", true);
                true
            }
            None => {
                self.say(if self.connected { "Phone access is loading…" } else { "Not connected to overseerd" }, false);
                true
            }
        }
    }

    /// `O`: turn phone access on, or off (asking first when phones are connected).
    pub(super) fn toggle_phone_access(&mut self) {
        if self.phone_missing() {
            return;
        }
        if !self.phone.enabled {
            self.request("gateway.enable", json!({}), Pending::PhoneSwitch { on: true, then_pair: false });
            return;
        }
        let here: Vec<String> = self.phone.devices.iter().filter(|d| d.connected).map(|d| short(&d.name, 30)).collect();
        if here.is_empty() {
            self.request("gateway.disable", json!({}), Pending::PhoneSwitch { on: false, then_pair: false });
            return;
        }
        let text = format!("Turn off phone access? {} phone{} will be disconnected: {}. Paired phones connect again when it is back on.", here.len(), if here.len() == 1 { "" } else { "s" }, here.join(", "));
        self.confirm_back = Some(if self.mode == Mode::Devices { Mode::Devices } else { Mode::Grid });
        self.mode = Mode::Confirm(Confirm::PhoneOff { text });
    }

    /// Ctrl-O: the Devices panel (it was `D` until dashboard mode took `D`, T-40).
    pub(super) fn open_devices(&mut self) {
        if self.phone_missing() {
            return;
        }
        self.mode = Mode::Devices;
        self.phone_request();
    }

    fn pair_start(&mut self) {
        self.pair_starting += 1;
        self.request("gateway.pair_start", json!({}), Pending::PairStart);
    }

    /// `p`: a pairing code. With phone access off, asks to turn it on first.
    fn pair(&mut self) {
        if self.phone_missing() {
            return;
        }
        if self.phone.enabled {
            self.pair_start();
            return;
        }
        self.confirm_back = Some(self.mode.clone());
        self.mode = Mode::Confirm(Confirm::PhoneOnAndPair);
    }

    /// A code this terminal shows and that still works is taken back (closing its panel, quitting).
    pub(super) fn take_back_pairing(&mut self) {
        if self.pairing.as_ref().is_some_and(|p| p.live()) {
            self.request("gateway.pair_cancel", json!({}), Pending::PairCancel);
        }
        self.pairing = None;
    }

    /// Leaves the pairing panel.
    fn close_pairing(&mut self) {
        self.take_back_pairing();
        self.mode = Mode::Devices;
        self.phone_request();
    }

    pub(super) fn devices_key(&mut self, k: KeyEvent) {
        let n = self.phone.devices.len();
        match k.code {
            KeyCode::Esc | KeyCode::Char('q') => self.mode = Mode::Grid,
            KeyCode::Char('o') if k.modifiers.contains(KeyModifiers::CONTROL) => self.mode = Mode::Grid,
            KeyCode::Down | KeyCode::Char('j') if n > 0 => self.phone.sel = (self.phone.sel + 1) % n,
            KeyCode::Up | KeyCode::Char('k') if n > 0 => self.phone.sel = (self.phone.sel + n - 1) % n,
            KeyCode::Char('r') => self.phone_request(),
            KeyCode::Char('O') => self.toggle_phone_access(),
            KeyCode::Char('p') => self.pair(),
            KeyCode::Char('N') => {
                let on = !self.phone.notifications;
                self.request("gateway.settings", json!({ "notifications": on }), Pending::PhoneNotifications(on));
            }
            KeyCode::Char('s') => {
                if let Some(d) = self.phone.devices.get(self.phone.sel).cloned() {
                    let scope = if d.scope == "watch" { "full" } else { "watch" };
                    self.request("gateway.device_scope", json!({ "id": d.id, "scope": scope }), Pending::DeviceScope { name: d.name, scope: scope.into() });
                }
            }
            KeyCode::Char('x') => {
                if let Some(d) = self.phone.devices.get(self.phone.sel).cloned() {
                    let text = format!("Revoke \"{}\"? {}Its key never works again; to use this phone again, pair it again.", short(&d.name, 40), if d.connected { "It is disconnected now. " } else { "" });
                    self.confirm_back = Some(Mode::Devices);
                    self.mode = Mode::Confirm(Confirm::Revoke { id: d.id, name: d.name, text });
                }
            }
            _ => {}
        }
    }

    pub(super) fn pairing_key(&mut self, k: KeyEvent) {
        match k.code {
            KeyCode::Esc | KeyCode::Char('q') => self.close_pairing(),
            // A new code, once this one is used or over.
            KeyCode::Char('p') if !self.pairing.as_ref().is_some_and(|p| p.live()) => self.pair(),
            _ => {}
        }
    }

    /// The owner's answer to one of the phone questions. Returns false when the key is not an answer.
    pub(super) fn phone_confirm(&mut self, c: &Confirm, k: KeyEvent) -> bool {
        let yes = matches!(k.code, KeyCode::Char('y') | KeyCode::Char('Y'));
        let no = matches!(k.code, KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc);
        if !yes && !no {
            // Pairing a phone is answered on purpose: any other key is not an answer.
            return matches!(c, Confirm::Pair { .. });
        }
        let back = self.confirm_back.take().unwrap_or(Mode::Grid);
        match c {
            Confirm::PhoneOff { .. } => {
                self.mode = back;
                if yes {
                    self.request("gateway.disable", json!({}), Pending::PhoneSwitch { on: false, then_pair: false });
                }
            }
            Confirm::PhoneOnAndPair => {
                self.mode = back;
                if yes {
                    self.request("gateway.enable", json!({}), Pending::PhoneSwitch { on: true, then_pair: true });
                }
            }
            Confirm::Revoke { id, name, .. } => {
                self.mode = back;
                if yes {
                    self.request("gateway.device_revoke", json!({ "id": id }), Pending::DeviceRevoke(name.clone()));
                }
            }
            Confirm::Pair { request, .. } => {
                self.mode = back;
                let name = self.phone.waiting.iter().find(|w| &w.request == request).map(|w| w.name.clone()).or_else(|| self.pairing.as_ref().and_then(|p| if let PairingState::Waiting(n) = &p.state { Some(n.clone()) } else { None })).unwrap_or_else(|| "The phone".into());
                self.request("gateway.pair_confirm", json!({ "request": request, "accept": yes }), Pending::PairConfirm { accept: yes, name });
            }
            _ => return false,
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(enabled: bool, devices: Value) -> Value {
        json!({ "enabled": enabled, "port": 47810, "name": "Test Mac", "devices": devices, "pairing": null, "settings": { "notifications": true }, "awake": false })
    }

    #[test]
    fn the_status_line_says_off_on_and_how_many_phones() {
        let mut p = Phone::default();
        assert_eq!(p.line(), None, "nothing until the daemon answered");
        p.load(&status(false, json!([])));
        assert_eq!(p.line().unwrap().0, "phone access off");
        p.load(&status(true, json!([])));
        assert_eq!(p.line().unwrap(), ("phone access on".to_string(), "phones on".to_string(), true));
        p.load(&status(true, json!([{ "id": "a", "name": "A", "connected": true, "revoked_ms": null }, { "id": "b", "name": "B", "connected": false, "revoked_ms": null }, { "id": "c", "name": "C", "connected": true, "revoked_ms": 5 }])));
        assert_eq!(p.line().unwrap().0, "phone access on · 1 phone");
        assert_eq!(p.devices.len(), 2, "a revoked device is gone from the list");
        assert_eq!(p.devices[0].name, "A", "connected first");
        p.load(&status(true, json!([{ "id": "a", "name": "A", "connected": true, "revoked_ms": null }, { "id": "b", "name": "B", "connected": true, "revoked_ms": null }])));
        assert_eq!(p.line().unwrap().0, "phone access on · 2 phones");
        assert_eq!(p.line().unwrap().1, "2 phones");
        p.available = Some(false);
        assert_eq!(p.line(), None, "a daemon without phone access shows nothing");
    }

    #[test]
    fn relative_times_and_groups() {
        assert_eq!(ago(1_000, 50_000), "just now");
        assert_eq!(ago(0, 60_000), "1m ago");
        assert_eq!(ago(0, 3 * 3_600_000), "3h ago");
        assert_eq!(ago(0, 49 * 3_600_000), "2d ago");
        assert_eq!(ago(70_000, 60_000), "just now", "a clock a little ahead is not the future");
        assert_eq!(fingerprint("0123456789abcdef"), "0123 4567 89ab cdef");
        let groups = code_groups("OVSR1-ABCDEFGHIJ");
        assert_eq!(groups, vec!["OVSR1-", "ABCD", "EFGH", "IJ"]);
        assert_eq!(groups.concat(), "OVSR1-ABCDEFGHIJ");
        let d = Device { connected: false, last_seen_ms: Some(0), scope: "watch".into(), platform: "android".into(), ..Default::default() };
        assert_eq!(d.presence(5 * 60_000), "last seen 5m ago");
        assert_eq!((d.scope_name(), d.platform_name()), ("Watch only", "Android"));
        assert_eq!(Device::default().presence(0), "not seen yet");
    }

    #[test]
    fn the_question_names_the_phone_its_address_and_its_key() {
        let r = PairRequest { request: "r".into(), name: "Bilal's iPhone".into(), platform: "ios".into(), address: "192.168.1.23".into(), fingerprint: "0123456789abcdef".into() };
        assert_eq!(r.question(), "Pair \"Bilal's iPhone\"? iPhone · 192.168.1.23 · key 0123 4567 89ab cdef.");
    }
}
