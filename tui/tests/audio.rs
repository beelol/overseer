//! Audio Mode controls use the daemon RPC; the terminal bell yields to daemon audio.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use overseer_tui::app::{App, Mode};
use overseer_tui::client::{Msg, Requests};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct Fake {
    next: AtomicU64,
    calls: Mutex<Vec<(u64, String, Value)>>,
}

impl Fake {
    fn take(&self, method: &str) -> (u64, Value) {
        let mut calls = self.calls.lock().unwrap();
        let index = calls.iter().position(|(_, name, _)| name == method).unwrap_or_else(|| panic!("missing {method} request: {calls:?}"));
        let (id, _, params) = calls.remove(index);
        (id, params)
    }
}

impl Requests for Fake {
    fn request(&self, method: &str, params: Value) -> u64 {
        let id = self.next.fetch_add(1, Ordering::SeqCst) + 1;
        self.calls.lock().unwrap().push((id, method.to_string(), params));
        id
    }
    fn connected(&self) -> bool { true }
    fn set_cursor_if_unset(&self, _: i64) {}
    fn subscribe(&self) {}
}

fn key(app: &mut App, code: KeyCode) {
    app.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
}

fn reply(app: &mut App, id: u64, result: Value) {
    app.handle_msg(Msg::Reply { id, result: Ok(result) });
}

fn audio(enabled: bool, track: &str) -> Value {
    json!({"enabled": enabled, "available": true, "track": track,
        "voice": "Daniel", "commander_imported": true})
}

fn state(status: &str) -> Value {
    json!({"cursor": 1, "tasks": [{"id": "task", "repo_root": "/tmp/repo", "workspace_id": "ws", "title": "Agent"}],
        "runs": [{"id": "run", "task_id": "task", "parent_run_id": null, "harness": "generic",
            "profile_id": null, "model": null, "workspace_id": "ws", "status": status,
            "exit_reason": null, "created_ms": 1, "ended_ms": null, "title": "Agent"}],
        "workspaces": [], "profiles": []})
}

#[test]
fn audio_panel_controls_daemon_settings_and_preview() {
    let fake = Arc::new(Fake::default());
    let mut app = App::new(fake.clone());
    key(&mut app, KeyCode::Char('S'));
    assert_eq!(app.mode, Mode::Audio);
    let (get, _) = fake.take("audio.get");
    let (voices, _) = fake.take("audio.voices");
    reply(&mut app, get, audio(false, "reactor"));
    reply(&mut app, voices, json!([{"name":"Daniel","locale":"en_US"},{"name":"Samantha","locale":"en_US"}]));

    key(&mut app, KeyCode::Char(' '));
    let (set, params) = fake.take("audio.set");
    assert_eq!(params, json!({"enabled":true}));
    reply(&mut app, set, audio(true, "reactor"));

    key(&mut app, KeyCode::Char('p'));
    let (preview, params) = fake.take("audio.preview");
    assert_eq!(params, json!({"key":"agent_started"}));
    reply(&mut app, preview, json!({"queued":true}));
    key(&mut app, KeyCode::Tab);
    key(&mut app, KeyCode::Char('p'));
    let (_, params) = fake.take("audio.preview");
    assert_eq!(params, json!({"key":"agent_complete"}));

    key(&mut app, KeyCode::Char('2'));
    let (set, params) = fake.take("audio.set");
    assert_eq!(params, json!({"track":"system"}));
    reply(&mut app, set, audio(true, "system"));
    key(&mut app, KeyCode::Char('v'));
    let (_, params) = fake.take("audio.set");
    assert_eq!(params, json!({"track":"system", "voice":"Samantha"}));

    key(&mut app, KeyCode::Char('i'));
    assert_eq!(app.mode, Mode::AudioImport);
    for c in "/tmp/private-pack".chars() { key(&mut app, KeyCode::Char(c)); }
    key(&mut app, KeyCode::Enter);
    let (import, params) = fake.take("audio.import_commander");
    assert_eq!(params, json!({"path":"/tmp/private-pack"}));
    reply(&mut app, import, json!({"imported":true}));
    let (_, params) = fake.take("audio.set");
    assert_eq!(params, json!({"track":"commander"}));

    app.mode = Mode::AudioImport;
    app.audio.import_path.clear();
    app.paste(&"x".repeat(10_000));
    assert_eq!(app.audio.import_path.len(), 4096, "private path input stays bounded");
}

#[test]
fn new_waiting_agent_queries_audio_before_ringing_terminal_bell() {
    let fake = Arc::new(Fake::default());
    let mut app = App::new(fake.clone());
    app.handle_msg(Msg::Connected);
    let (id, _) = fake.take("state");
    reply(&mut app, id, state("running"));
    key(&mut app, KeyCode::Char('r'));
    let (id, _) = fake.take("state");
    reply(&mut app, id, state("waiting_for_user"));
    assert!(!app.bell, "bell waits for the current audio setting");
    let (id, _) = fake.take("audio.get");
    reply(&mut app, id, audio(true, "reactor"));
    assert!(!app.bell, "daemon audio replaces the terminal bell");

    key(&mut app, KeyCode::Char('r'));
    let (id, _) = fake.take("state");
    reply(&mut app, id, state("running"));
    key(&mut app, KeyCode::Char('r'));
    let (id, _) = fake.take("state");
    reply(&mut app, id, state("waiting_for_user"));
    let (id, _) = fake.take("audio.get");
    reply(&mut app, id, audio(false, "reactor"));
    assert!(app.bell, "the existing terminal bell remains when Audio Mode is off");

    app.bell = false;
    key(&mut app, KeyCode::Char('r'));
    let (id, _) = fake.take("state");
    reply(&mut app, id, state("running"));
    key(&mut app, KeyCode::Char('r'));
    let (id, _) = fake.take("state");
    reply(&mut app, id, state("waiting_for_user"));
    let (id, _) = fake.take("audio.get");
    app.handle_msg(Msg::Reply { id, result: Err("unknown method".into()) });
    assert!(app.bell, "an older daemon keeps the TUI's existing terminal bell");
}

#[test]
fn audio_panel_renders_at_compact_terminal_size() {
    use overseer_tui::ui;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    let fake = Arc::new(Fake::default());
    let mut app = App::new(fake.clone());
    key(&mut app, KeyCode::Char('S'));
    let (id, _) = fake.take("audio.get");
    reply(&mut app, id, audio(false, "reactor"));
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|frame| ui::draw(frame, &mut app)).unwrap();
    let screen: String = terminal.backend().buffer().content().iter().map(|cell| cell.symbol()).collect();
    for text in ["Audio Mode", "OFF", "Reactor", "Preview", "Commander"] {
        assert!(screen.contains(text), "missing {text} in compact Audio Mode panel");
    }
}
