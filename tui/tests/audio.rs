//! T-23 and T-24: Audio Mode from the terminal, and one signal when an agent needs you (the
//! daemon's cue or the terminal bell). A real overseerd that writes each cue it would play to a
//! log instead of the speakers, the SYNTHETIC Claude fixture (permission mode), and a fake
//! client for the daemons that cannot be started: one without audio methods and one that does
//! not answer.
mod support;

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
use overseer_tui::app::{App, Mode};
use overseer_tui::client::{Msg, Requests};
use overseer_tui::ui;
use ratatui::backend::TestBackend;
use ratatui::Terminal;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use support::*;

// ---------------------------------------------------------------- a real daemon

fn audio_daemon(log: &Path) -> Daemon {
    let claude = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().join("fixtures/fake-harness/claude-fixture.js").display().to_string();
    Daemon::start(&[("OVERSEER_TEST_AUDIO_LOG", log.to_str().unwrap()), ("OVERSEER_CLAUDE_PATH", &claude), ("CLAUDE_FIXTURE_MODE", "permission"), ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "CLAUDE_FIXTURE_MODE")])
}

/// A Claude fixture agent that asks for permission and waits.
fn asks(d: &Daemon, repo: &Path, title: &str) -> String {
    d.ctl("task.create", json!({ "repo": repo, "harness": "claude", "prompt": "write perm.txt", "title": title }))["run"]["id"].as_str().unwrap().to_string()
}

fn cues(log: &Path) -> Vec<String> {
    std::fs::read_to_string(log).unwrap_or_default().lines().map(str::to_owned).collect()
}

fn attention(log: &Path) -> usize {
    cues(log).iter().filter(|c| c.ends_with(":agent_needs_attention")).count()
}

/// Pumps the TUI until the daemon's cue log satisfies `cond`.
fn until_cues(tui: &mut Tui, log: &Path, secs: u64, cond: impl Fn(&[String]) -> bool) -> Vec<String> {
    let end = Instant::now() + Duration::from_secs(secs);
    loop {
        let now = cues(log);
        if cond(&now) {
            return now;
        }
        assert!(Instant::now() < end, "timed out; cues: {now:?}");
        tui.pump(50);
    }
}

/// Three files that pass for recordings: the daemon checks the header, the test plays nothing.
fn synthetic_pack(dir: &Path) -> PathBuf {
    for key in ["agent_started", "agent_complete", "agent_needs_attention"] {
        let file = dir.join(key).join("transmission/commander.wav");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, b"RIFF\x00\x00\x00\x00WAVEfmt ").unwrap();
    }
    dir.canonicalize().unwrap()
}

fn files_under(dir: &Path, suffix: &str) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut todo = vec![dir.to_path_buf()];
    while let Some(d) = todo.pop() {
        for entry in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                todo.push(path);
            } else if path.to_string_lossy().to_lowercase().ends_with(suffix) {
                found.push(path);
            }
        }
    }
    found
}

fn press(tui: &mut Tui, code: KeyCode, times: usize) {
    for _ in 0..times {
        tui.app.handle_key(KeyEvent { code, modifiers: KeyModifiers::NONE, kind: KeyEventKind::Press, state: KeyEventState::NONE });
    }
    tui.pump(30);
}

#[test]
fn t23_audio_mode_from_the_terminal() {
    let t = tempfile::tempdir().unwrap();
    let log = t.path().join("cues.log");
    let d = audio_daemon(&log);
    let mut tui = Tui::attach(&d, 140, 40);
    tui.until(5, |a| a.audio.known);

    // `?` and --help list S.
    tui.key(KeyCode::Char('?'));
    assert!(tui.screen().contains("Audio Mode: settings and preview"));
    tui.key(KeyCode::Esc);
    let help = std::process::Command::new(env!("CARGO_BIN_EXE_overseer-tui")).arg("--help").output().unwrap();
    assert!(String::from_utf8_lossy(&help.stdout).contains("S               Audio Mode"));

    // The panel shows what the daemon reports.
    tui.key(KeyCode::Char('S'));
    assert_eq!(tui.app.mode, Mode::Audio);
    let s = tui.until_screen(5, "OFF · ready");
    assert!(s.contains("Track: reactor") && s.contains("Voice: system default") && s.contains("Commander: no private folder"), "{s}");
    tui.snapshot("t23-audio-off");

    // On from the terminal: the daemon has it.
    tui.key(KeyCode::Char(' '));
    tui.until(5, |a| a.audio.enabled);
    assert_eq!(d.ctl("audio.get", json!({}))["enabled"], true);
    assert!(tui.until_screen(5, "ON · ready").contains("Audio Mode on · reactor"));

    // A preview is played by the daemon: exactly that cue.
    tui.key(KeyCode::Char('p'));
    assert_eq!(until_cues(&mut tui, &log, 5, |c| !c.is_empty()), ["reactor:agent_started"]);
    tui.key(KeyCode::Tab);
    tui.key(KeyCode::Char('p'));
    assert_eq!(until_cues(&mut tui, &log, 5, |c| c.len() > 1), ["reactor:agent_started", "reactor:agent_complete"]);
    tui.pump(500);
    assert_eq!(cues(&log).len(), 2, "the TUI plays nothing itself and asks once");

    // Tracks and the voice reach the daemon.
    tui.key(KeyCode::Char('2'));
    tui.until(5, |a| a.audio.track == "system");
    assert_eq!(d.ctl("audio.get", json!({}))["track"], "system");
    if cfg!(target_os = "macos") {
        tui.until(5, |a| !a.audio.voices.is_empty());
        tui.key(KeyCode::Char('v'));
        tui.until(5, |a| !a.audio.voice.is_empty());
        let voice = tui.app.audio.voice.clone();
        assert!(tui.app.audio.voices.contains(&voice));
        assert_eq!(d.ctl("audio.get", json!({}))["voice"], voice);
        assert!(tui.until_screen(5, &format!("Voice: {voice}")).contains("Track: system"));
    }
    tui.key(KeyCode::Char('1'));
    tui.until(5, |a| a.audio.track == "reactor");
    assert_eq!(d.ctl("audio.get", json!({}))["track"], "reactor");

    // Commander needs a folder first; a folder that is not a pack is refused with the reason.
    tui.key(KeyCode::Char('3'));
    tui.until_screen(5, "import the private Commander pack first");
    assert_eq!(d.ctl("audio.get", json!({}))["track"], "reactor");
    let empty = t.path().join("not-a-pack");
    std::fs::create_dir_all(&empty).unwrap();
    tui.key(KeyCode::Char('i'));
    assert_eq!(tui.app.mode, Mode::AudioImport);
    tui.type_text(empty.to_str().unwrap());
    tui.key(KeyCode::Enter);
    tui.until_screen(5, "not a Commander pack: agent_started/transmission/commander.wav is missing");
    assert_eq!(tui.app.mode, Mode::AudioImport, "refused: still asking for the folder");
    assert_eq!(d.ctl("audio.get", json!({}))["commander_imported"], false);

    // A pack is accepted and stays where it is.
    let pack = synthetic_pack(&t.path().join("private-commander"));
    press(&mut tui, KeyCode::Backspace, empty.to_str().unwrap().chars().count());
    tui.type_text(pack.to_str().unwrap());
    tui.key(KeyCode::Enter);
    tui.until(5, |a| a.audio.commander_imported && a.audio.track == "commander");
    assert_eq!(tui.app.mode, Mode::Audio);
    let now = d.ctl("audio.get", json!({}));
    assert_eq!((now["commander_imported"].clone(), now["track"].clone()), (json!(true), json!("commander")));
    assert!(tui.until_screen(5, "Commander: private folder ready").contains("Track: commander"));
    assert!(files_under(d.home.path(), ".wav").is_empty(), "nothing is copied under the daemon's folder");
    assert!(!d.home.path().join("audio").exists());
    assert_eq!(files_under(&pack, ".wav").len(), 3, "the recordings stay in their folder");

    // A change made in another client shows in the open panel within 2 s.
    d.ctl("audio.set", json!({ "enabled": false, "track": "reactor" }));
    let changed = Instant::now();
    let s = tui.until_screen(2, "OFF · ready");
    assert!(s.contains("Track: reactor"), "{s}");
    assert!(changed.elapsed() < Duration::from_secs(2));

    d.ctl("audio.set", json!({ "enabled": true }));
    tui.until_screen(2, "ON · ready");
    tui.until(8, |a| a.notice.is_none());
    tui.snapshot("t23-audio-140x40");
    tui.resize(80, 24);
    let s = tui.screen();
    for text in ["Audio Mode", "ON · ready", "Track: reactor", "Preview: agent complete", "Commander: private folder ready"] {
        assert!(s.contains(text), "missing {text:?} at 80×24:\n{s}");
    }
    tui.snapshot("t23-audio-80x24");
    tui.key(KeyCode::Esc);
    assert_eq!(tui.app.mode, Mode::Grid);
}

#[test]
fn t24_one_signal_when_an_agent_needs_you() {
    let t = tempfile::tempdir().unwrap();
    let log = t.path().join("cues.log");
    let d = audio_daemon(&log);
    let repo = repo(&t.path().join("attention"));
    let quiet = d.sh(&repo, "Quiet agent", "echo quiet; while read l; do echo $l; done");
    let mut tui = Tui::attach(&d, 140, 40);
    tui.until_screen(10, "quiet");
    tui.until(5, |a| a.audio.known);
    assert!(!tui.app.bell && !tui.app.audio.plays());

    // Off: the bell, in the same pass as the state, and no cue.
    let first = asks(&d, &repo, "Asks first");
    tui.until(20, |a| a.state.run(&first).is_some_and(|r| r.needs_you()));
    assert!(tui.app.bell, "off: the terminal bell");
    assert_eq!(tui.app.window_title(), "Overseer · 1 needs you · 2 active");
    assert!(tui.screen().contains("◆ Asks first needs you — press w"));
    tui.app.bell = false; // the event loop rings and clears it
    tui.pump(1200);
    assert!(cues(&log).is_empty(), "off: the daemon plays nothing: {:?}", cues(&log));
    assert!(!tui.app.bell, "one bell");

    // Turned on in another client while the TUI runs: a need 2 s later gives the cue, no bell.
    d.ctl("audio.set", json!({ "enabled": true }));
    tui.pump(2000);
    let second = asks(&d, &repo, "Asks second");
    tui.until(20, |a| a.state.run(&second).is_some_and(|r| r.needs_you()));
    assert!(!tui.app.bell, "on: no bell");
    assert_eq!(tui.app.window_title(), "Overseer · 2 need you · 3 active", "the title does not wait for audio");
    assert!(tui.screen().contains("◆ Asks second needs you — press w"), "nor does the notice");
    until_cues(&mut tui, &log, 5, |c| c.iter().any(|c| c == "reactor:agent_needs_attention"));
    tui.pump(1200);
    assert_eq!(attention(&log), 1, "on: one cue: {:?}", cues(&log));
    assert!(!tui.app.bell, "on: still no bell");

    // On, but the Commander folder is gone: nothing can play, so the bell.
    let pack = synthetic_pack(&t.path().join("private-commander"));
    d.ctl("audio.import_commander", json!({ "path": pack }));
    d.ctl("audio.set", json!({ "track": "commander" }));
    tui.until(5, |a| a.audio.track == "commander" && a.audio.plays());
    std::fs::remove_dir_all(&pack).unwrap();
    tui.pump(2000);
    assert!(tui.app.audio.enabled && !tui.app.audio.available && !tui.app.audio.plays());
    let third = asks(&d, &repo, "Asks third");
    tui.until(20, |a| a.state.run(&third).is_some_and(|r| r.needs_you()));
    assert!(tui.app.bell, "on but unavailable: the terminal bell");
    assert_eq!(tui.app.window_title(), "Overseer · 3 need you · 4 active");
    for r in [quiet, first, second, third] {
        d.ctl("run.interrupt", json!({ "run_id": r }));
    }
}

#[test]
fn t24_the_real_binary_rings_only_when_the_daemon_does_not_play() {
    let t = tempfile::tempdir().unwrap();
    let helper = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/pty_run.py");
    // The binary runs for six seconds in a pty; an agent starts waiting while it is attached.
    let session = |on: bool| -> (usize, usize, Vec<String>) {
        let log = t.path().join(format!("cues-{on}.log"));
        let d = audio_daemon(&log);
        let repo = repo(&t.path().join(format!("attention-{on}")));
        let quiet = d.sh(&repo, "Quiet agent", "echo quiet; while read l; do echo $l; done");
        d.ctl("audio.set", json!({ "enabled": on }));
        let run = std::thread::spawn({
            let (helper, bin_dir, home) = (helper.clone(), d.bin.clone(), d.home.path().to_path_buf());
            move || std::process::Command::new("python3").arg(helper).args(["30", "120", "6.0", "q", "--", env!("CARGO_BIN_EXE_overseer-tui"), "--daemon"]).arg(bin_dir).arg("--home").arg(home).env("TERM", "xterm-256color").output().unwrap()
        });
        std::thread::sleep(Duration::from_millis(2500));
        let waiting = asks(&d, &repo, "Asks permission");
        d.wait_status(&waiting, |s| s == "waiting_for_user", 20);
        let out = run.join().unwrap();
        let text = String::from_utf8_lossy(&out.stdout).to_string();
        assert!(text.contains("\u{1b}]0;Overseer · 1 needs you · 2 active\u{7}"), "window title set");
        let shown = cues(&log);
        for r in [quiet, waiting] {
            d.ctl("run.interrupt", json!({ "run_id": r }));
        }
        (text.matches('\u{7}').count(), text.matches("\u{1b}]0;").count(), shown)
    };
    let (bells, titles, played) = session(false);
    assert_eq!(bells, titles + 1, "off: one bell beyond the title's terminators");
    assert!(played.is_empty(), "off: no cue: {played:?}");
    let (bells, titles, played) = session(true);
    assert_eq!(bells, titles, "on: no bell beyond the title's terminators");
    assert_eq!(played.iter().filter(|c| *c == "reactor:agent_needs_attention").count(), 1, "on: one cue: {played:?}");
}

// ---------------------------------------------------------------- a fake client

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

    fn asked(&self, method: &str) -> usize {
        self.calls.lock().unwrap().iter().filter(|(_, name, _)| name == method).count()
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

fn refuse(app: &mut App, id: u64, error: &str) {
    app.handle_msg(Msg::Reply { id, result: Err(error.into()) });
}

fn audio(enabled: bool, available: bool) -> Value {
    json!({"enabled": enabled, "available": available, "track": "reactor", "voice": "", "commander_imported": false})
}

fn state(status: &str) -> Value {
    json!({"cursor": 1, "tasks": [{"id": "task", "repo_root": "/tmp/repo", "workspace_id": "ws", "title": "Agent"}],
        "runs": [{"id": "run", "task_id": "task", "parent_run_id": null, "harness": "generic",
            "profile_id": null, "model": null, "workspace_id": "ws", "status": status,
            "exit_reason": null, "created_ms": 1, "ended_ms": null, "title": "Agent"}],
        "workspaces": [], "profiles": []})
}

fn screen(app: &mut App, w: u16, h: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    terminal.draw(|frame| ui::draw(frame, app)).unwrap();
    buffer_text(terminal.backend().buffer())
}

/// A connected app that has loaded a running agent; the first `audio.get` is still unanswered.
fn attached() -> (Arc<Fake>, App, u64) {
    let fake = Arc::new(Fake::default());
    let mut app = App::new(fake.clone());
    app.handle_msg(Msg::Connected);
    let (id, _) = fake.take("state");
    reply(&mut app, id, state("running"));
    let (audio, _) = fake.take("audio.get");
    (fake, app, audio)
}

/// The agent starts waiting: the state is reloaded and answered. True when the bell rang in
/// that same pass.
fn a_need_arrives(fake: &Fake, app: &mut App) -> bool {
    app.bell = false;
    key(app, KeyCode::Char('r'));
    let (id, _) = fake.take("state");
    reply(app, id, state("running"));
    key(app, KeyCode::Char('r'));
    let (id, _) = fake.take("state");
    reply(app, id, state("waiting_for_user"));
    assert_eq!(app.window_title(), "Overseer · 1 needs you · 1 active", "the title never waits for audio");
    app.bell
}

#[test]
fn t24_the_bell_is_withheld_only_while_the_daemon_says_it_plays() {
    // No answer yet, and a daemon that never answers: the bell, in the same pass.
    let (fake, mut app, _unanswered) = attached();
    assert!(a_need_arrives(&fake, &mut app), "no answer yet");
    assert!(a_need_arrives(&fake, &mut app), "still no answer");

    // An answer with an error (a daemon without audio.get): the bell.
    let (fake, mut app, asked) = attached();
    refuse(&mut app, asked, "unknown method: audio.get");
    assert!(!app.audio.known);
    assert!(a_need_arrives(&fake, &mut app), "a daemon without audio.get");

    // Off: the bell. On and available: the daemon's cue, no bell. On but unavailable: the bell.
    let (fake, mut app, asked) = attached();
    reply(&mut app, asked, audio(false, true));
    assert!(a_need_arrives(&fake, &mut app), "off");
    app.tick(Instant::now() + Duration::from_millis(1100));
    let (asked, _) = fake.take("audio.get");
    reply(&mut app, asked, audio(true, true));
    assert!(!a_need_arrives(&fake, &mut app), "on and available: the daemon plays the cue");
    app.tick(Instant::now() + Duration::from_millis(1100));
    let (asked, _) = fake.take("audio.get");
    reply(&mut app, asked, audio(true, false));
    assert!(a_need_arrives(&fake, &mut app), "on but unavailable");

    // What an old connection said does not count on a new one.
    app.tick(Instant::now() + Duration::from_millis(1100));
    let (asked, _) = fake.take("audio.get");
    reply(&mut app, asked, audio(true, true));
    assert!(app.audio.plays());
    app.handle_msg(Msg::Disconnected("daemon restarted".into()));
    app.handle_msg(Msg::Connected);
    let (id, _) = fake.take("state");
    reply(&mut app, id, state("running"));
    assert!(!app.audio.plays());
    assert!(a_need_arrives(&fake, &mut app), "reconnected, no answer yet");
}

#[test]
fn t23_the_daemon_is_asked_again_every_second_and_rarely_after_an_error() {
    let (fake, mut app, asked) = attached();
    let start = Instant::now();
    assert!(!app.tick(start + Duration::from_secs(5)));
    assert_eq!(fake.asked("audio.get"), 0, "one question at a time");
    reply(&mut app, asked, audio(false, true));
    app.dirty = false;
    app.tick(start + Duration::from_millis(500));
    assert_eq!(fake.asked("audio.get"), 0);
    app.tick(start + Duration::from_millis(1100));
    let (asked, _) = fake.take("audio.get");
    reply(&mut app, asked, audio(false, true));
    assert!(!app.dirty, "an unchanged answer redraws nothing");
    app.tick(Instant::now() + Duration::from_millis(1100));
    let (asked, _) = fake.take("audio.get");
    reply(&mut app, asked, audio(true, true));
    assert!(app.dirty, "a changed answer redraws");

    app.tick(Instant::now() + Duration::from_millis(1100));
    let (asked, _) = fake.take("audio.get");
    refuse(&mut app, asked, "unknown method: audio.get");
    app.tick(Instant::now() + Duration::from_secs(5));
    assert_eq!(fake.asked("audio.get"), 0, "after an error the question waits");
    app.tick(Instant::now() + Duration::from_secs(31));
    assert_eq!(fake.asked("audio.get"), 1);
}

#[test]
fn t23_a_daemon_without_audio_methods_says_unavailable_and_changes_nothing() {
    let (fake, mut app, asked) = attached();
    refuse(&mut app, asked, "unknown method: audio.get");
    key(&mut app, KeyCode::Char('S'));
    assert_eq!(app.mode, Mode::Grid, "nothing opens before the daemon answers");
    let (asked, _) = fake.take("audio.get");
    refuse(&mut app, asked, "unknown method: audio.get");
    assert_eq!(app.mode, Mode::Grid);
    assert!(screen(&mut app, 120, 30).contains("Audio Mode is unavailable: unknown method: audio.get"));
    for method in ["audio.set", "audio.preview", "audio.voices", "audio.import_commander"] {
        assert_eq!(fake.asked(method), 0, "{method}");
    }

    // The daemon stops answering while the panel is open: the panel closes and says so.
    let (fake, mut app, asked) = attached();
    reply(&mut app, asked, audio(true, true));
    key(&mut app, KeyCode::Char('S'));
    assert_eq!(app.mode, Mode::Audio);
    let (asked, _) = fake.take("audio.get");
    refuse(&mut app, asked, "unknown method: audio.get");
    assert_eq!(app.mode, Mode::Grid);
    assert!(screen(&mut app, 120, 30).contains("Audio Mode is unavailable"));
}

#[test]
fn t23_the_panel_asks_the_daemon_for_every_change() {
    let (fake, mut app, asked) = attached();
    reply(&mut app, asked, audio(false, true));
    key(&mut app, KeyCode::Char('S'));
    assert_eq!(app.mode, Mode::Audio);
    let (voices, _) = fake.take("audio.voices");
    reply(&mut app, voices, json!([{"name":"Daniel","locale":"en_GB"},{"name":"Samantha","locale":"en_US"}]));

    key(&mut app, KeyCode::Char(' '));
    let (set, params) = fake.take("audio.set");
    assert_eq!(params, json!({"enabled":true}));
    assert!(!app.audio.enabled, "nothing changes before the daemon says so");
    reply(&mut app, set, audio(true, true));
    assert!(app.audio.enabled);

    key(&mut app, KeyCode::Char('p'));
    assert_eq!(fake.take("audio.preview").1, json!({"key":"agent_started"}));
    key(&mut app, KeyCode::Tab);
    key(&mut app, KeyCode::Char('p'));
    assert_eq!(fake.take("audio.preview").1, json!({"key":"agent_complete"}));
    key(&mut app, KeyCode::Tab);
    key(&mut app, KeyCode::Char('p'));
    assert_eq!(fake.take("audio.preview").1, json!({"key":"agent_needs_attention"}));

    key(&mut app, KeyCode::Char('2'));
    assert_eq!(fake.take("audio.set").1, json!({"track":"system"}));
    key(&mut app, KeyCode::Char('v'));
    assert_eq!(fake.take("audio.set").1, json!({"track":"system", "voice":"Daniel"}));
    key(&mut app, KeyCode::Char('1'));
    assert_eq!(fake.take("audio.set").1, json!({"track":"reactor"}));
    key(&mut app, KeyCode::Char('3'));
    assert_eq!(fake.take("audio.set").1, json!({"track":"commander"}));

    key(&mut app, KeyCode::Char('i'));
    assert_eq!(app.mode, Mode::AudioImport);
    for c in "/private/pack".chars() {
        key(&mut app, KeyCode::Char(c));
    }
    key(&mut app, KeyCode::Enter);
    let (import, params) = fake.take("audio.import_commander");
    assert_eq!(params, json!({"path":"/private/pack"}));
    reply(&mut app, import, json!({"imported":true}));
    assert_eq!(app.mode, Mode::Audio);
    assert_eq!(fake.take("audio.set").1, json!({"track":"commander"}));

    app.mode = Mode::AudioImport;
    app.audio.import_path.clear();
    app.paste(&"x".repeat(10_000));
    assert_eq!(app.audio.import_path.len(), 4096, "the path stays bounded");
}
