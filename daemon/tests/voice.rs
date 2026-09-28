//! Voice Mode in the daemon (Gate R): a real daemon with the simulated voice
//! (`OVERSEER_VOICE_SIMULATE=1`): the real listener in its simulated room, a made-up voice for
//! Overseer, and the Claude fixture as Overseer's model. No microphone, no paid turn.

mod common;

use common::*;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

/// The listener binary, built once for these tests.
fn listener_bin() -> PathBuf {
    static BIN: OnceLock<PathBuf> = OnceLock::new();
    BIN.get_or_init(|| {
        let root = repo_root();
        let target = std::env::var_os("CARGO_TARGET_DIR").map(PathBuf::from).unwrap_or_else(|| root.join("target"));
        let bin = target.join("debug/overseer-listener");
        let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
        let ok = std::process::Command::new(cargo).args(["build", "-q", "-p", "overseer-listener"]).current_dir(&root).status().map(|s| s.success()).unwrap_or(false);
        assert!(ok || bin.exists(), "could not build overseer-listener");
        bin
    })
    .clone()
}

fn claude_fixture() -> String {
    repo_root().join("fixtures/fake-harness/claude-fixture.js").display().to_string()
}

struct Env {
    d: Daemon,
    dir: tempfile::TempDir,
}

impl Env {
    fn cue_log(&self) -> Vec<String> {
        std::fs::read_to_string(self.dir.path().join("cues.log")).unwrap_or_default().lines().map(String::from).collect()
    }
    fn mode_file(&self) -> PathBuf {
        self.dir.path().join("mode")
    }
}

/// A daemon with the simulated voice, its cues written to a log, and the Claude fixture.
fn voice_daemon(extra: &[(&str, &str)]) -> Env {
    let dir = tempfile::tempdir().unwrap();
    let mode = dir.path().join("mode");
    std::fs::write(&mode, "overseer").unwrap();
    let log = dir.path().join("cues.log").display().to_string();
    let bin = listener_bin().display().to_string();
    let fixture = claude_fixture();
    let mode_s = mode.display().to_string();
    let mut env: Vec<(&str, &str)> = vec![
        ("OVERSEER_VOICE_SIMULATE", "1"),
        ("OVERSEER_LISTENER", &bin),
        ("OVERSEER_LISTENER_TEST_VOICE", "1"),
        ("OVERSEER_TEST_AUDIO_LOG", &log),
        ("OVERSEER_CLAUDE_PATH", &fixture),
        ("CLAUDE_FIXTURE_MODE_FILE", &mode_s),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "CLAUDE_FIXTURE_MODE_FILE"),
    ];
    env.extend_from_slice(extra);
    Env { d: Daemon::start(&env), dir }
}

/// The live channel, collected with the time each message arrived.
struct Live {
    msgs: Arc<Mutex<Vec<(Instant, Value)>>>,
}

impl Live {
    fn open(d: &Daemon) -> Live {
        let mut conn = UnixStream::connect(d.socket()).unwrap();
        conn.write_all(format!("{}\n", json!({"id": 1, "method": "voice.subscribe", "params": {}})).as_bytes()).unwrap();
        let msgs: Arc<Mutex<Vec<(Instant, Value)>>> = Default::default();
        let sink = msgs.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(conn).lines() {
                let Ok(line) = line else { break };
                if let Ok(v) = serde_json::from_str::<Value>(&line) {
                    if v["method"] == "voice" {
                        sink.lock().unwrap().push((Instant::now(), v["params"].clone()));
                    }
                }
            }
        });
        std::thread::sleep(Duration::from_millis(100));
        Live { msgs }
    }
    fn all(&self) -> Vec<Value> {
        self.msgs.lock().unwrap().iter().map(|(_, v)| v.clone()).collect()
    }
    fn kind(&self, kind: &str) -> Vec<Value> {
        self.all().into_iter().filter(|v| v["kind"] == kind).collect()
    }
    fn states(&self) -> Vec<String> {
        self.kind("state").iter().map(|v| v["state"].as_str().unwrap_or("").to_string()).collect()
    }
    fn wait(&self, what: &str, secs: u64, f: impl Fn(&Value) -> bool) -> Value {
        let deadline = Instant::now() + Duration::from_secs(secs);
        loop {
            if let Some(v) = self.all().into_iter().find(|v| f(v)) {
                return v;
            }
            assert!(Instant::now() < deadline, "no {what} within {secs} s; the live channel had: {:#?}", self.all().iter().filter(|v| v["kind"] != "level").collect::<Vec<_>>());
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    fn clear(&self) {
        self.msgs.lock().unwrap().clear();
    }
}

fn pid_alive(pid: i64) -> bool {
    unsafe { libc_kill(pid as i32, 0) == 0 }
}

extern "C" {
    #[link_name = "kill"]
    fn libc_kill(pid: i32, sig: i32) -> i32;
}

fn listening(env: &Env) -> Live {
    let live = Live::open(&env.d);
    env.d.call("voice.set", json!({"enabled": true}));
    live.wait("listening", 20, |v| v["kind"] == "state" && v["state"] == "listening");
    live
}

// ---------------------------------------------------------------------- AC-163

/// AC-163: off by default and kept across a daemon kill; while off or muted no listener runs;
/// a second daemon client sees the same state; turning it on starts one listener.
#[test]
fn ac163_off_by_default_kept_across_a_kill_and_muted_means_no_listener() {
    let mut env = voice_daemon(&[]);
    let g = env.d.call("voice.get", json!({}));
    assert_eq!(g["enabled"], false);
    assert_eq!(g["state"], "off");
    assert_eq!(g["listener"]["running"], false, "no listener while off");
    let live = listening(&env);
    let pid = env.d.call("voice.get", json!({}))["listener"]["pid"].as_i64().unwrap();
    assert!(pid_alive(pid));
    // Kept across a kill of the daemon.
    env.d.kill9();
    std::thread::sleep(Duration::from_millis(300));
    env.d.spawn();
    let live2 = Live::open(&env.d);
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let g = env.d.call("voice.get", json!({}));
        if g["state"] == "listening" {
            assert_eq!(g["enabled"], true);
            break;
        }
        assert!(Instant::now() < deadline, "not listening again after the restart: {g}");
        std::thread::sleep(Duration::from_millis(100));
    }
    let pid2 = env.d.call("voice.get", json!({}))["listener"]["pid"].as_i64().unwrap();
    // Muted: the listener is stopped, so the microphone is closed.
    env.d.call("voice.set", json!({"muted": true}));
    live2.wait("muted", 5, |v| v["kind"] == "state" && v["state"] == "muted");
    std::thread::sleep(Duration::from_millis(700));
    assert!(!pid_alive(pid2), "the listener stopped when muted");
    assert_eq!(env.d.call("voice.get", json!({}))["listener"]["running"], false);
    env.d.call("voice.set", json!({"muted": false}));
    live2.wait("listening again", 20, |v| v["kind"] == "state" && v["state"] == "listening");
    env.d.call("voice.set", json!({"enabled": false}));
    let _ = live;
    let g = env.d.call("voice.get", json!({}));
    assert_eq!(g["state"], "off");
    std::thread::sleep(Duration::from_millis(700));
    assert_eq!(g["listener"]["running"], false);
}

/// AC-163 and AC-175: a listener that dies leaves the daemon and a running agent untouched and
/// is started again; a fourth death in ten minutes leaves Voice Mode off with the reason.
#[test]
fn ac175_a_dying_listener_never_touches_an_agent_and_four_deaths_turn_voice_off() {
    let env = voice_daemon(&[]);
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let created = env.d.generic(&repo, "worktree", "/bin/sleep", &["20"]);
    let run = created["run"]["id"].as_str().unwrap().to_string();
    let live = listening(&env);
    for death in 1..=4 {
        let pid = env.d.call("voice.get", json!({}))["listener"]["pid"].as_i64().unwrap();
        live.clear();
        unsafe {
            libc_kill(pid as i32, 9);
        }
        if death < 4 {
            live.wait("a restart", 20, |v| v["kind"] == "state" && v["state"] == "listening");
            let again = env.d.call("voice.get", json!({}))["listener"]["pid"].as_i64().unwrap();
            assert_ne!(again, pid, "a new listener after death {death}");
        } else {
            let off = live.wait("Voice Mode off", 10, |v| v["kind"] == "state" && v["state"] == "off");
            assert!(off["reason"].as_str().unwrap_or("").contains("four times"), "{off}");
        }
        assert_eq!(env.d.run(&run)["status"], "running", "the agent kept running after death {death}");
    }
    let g = env.d.call("voice.get", json!({}));
    assert_eq!(g["enabled"], false);
    assert!(g["reason"].as_str().unwrap_or("").contains("four times"), "{g}");
}

// ---------------------------------------------------------------------- AC-164, AC-177

/// AC-164 and AC-177: noise in the room never opens the gate, never moves the mark and never
/// makes a request; speech does, and its levels arrive on the live channel.
#[test]
fn ac164_noise_never_moves_the_mark_and_speech_does() {
    let env = voice_daemon(&[]);
    let live = listening(&env);
    for noise in ["typing", "taps", "cough", "door", "cup", "music", "fan"] {
        env.d.call("voice.simulate", json!({"noise": noise}));
    }
    std::thread::sleep(Duration::from_secs(16));
    assert!(live.kind("level").is_empty(), "noise moved the mark: {:?}", live.kind("level").first());
    assert!(!live.states().contains(&"hearing".to_string()), "noise was heard as the owner");
    assert!(live.kind("heard").is_empty());
    assert_eq!(env.d.call("voice.requests", json!({}))["requests"].as_array().unwrap().len(), 0);
    // A voice: hearing, levels that move, then listening again.
    live.clear();
    env.d.call("voice.simulate", json!({"speechlike": 1.5, "words": "just thinking out loud here"}));
    live.wait("hearing", 5, |v| v["kind"] == "state" && v["state"] == "hearing");
    live.wait("listening", 8, |v| v["kind"] == "state" && v["state"] == "listening");
    let levels: Vec<f64> = live.kind("level").iter().filter(|v| v["source"] == "owner").filter_map(|v| v["value"].as_f64()).collect();
    assert!(levels.len() >= 10, "levels while hearing: {}", levels.len());
    assert!(levels.iter().any(|l| *l > 0.5) && levels.iter().any(|l| *l < 0.3), "the level moves with syllables: {levels:?}");
}

/// AC-173 and AC-177: levels and words in progress travel on the live channel only; nothing of
/// them is written to the database, and words not meant for Overseer are not stored at all.
#[test]
fn ac173_levels_and_side_talk_are_never_stored() {
    let env = voice_daemon(&[]);
    let live = listening(&env);
    env.d.call("voice.simulate", json!({"speechlike": 1.5, "words": "I'll grab lunch at noon with Priya"}));
    live.wait("the words", 8, |v| v["kind"] == "heard" && v["final"] == true);
    live.wait("not meant", 5, |v| v["kind"] == "not_meant");
    assert!(!live.kind("level").is_empty());
    std::thread::sleep(Duration::from_millis(300));
    let db = std::fs::read(env.d.home.path().join("overseer.sqlite")).unwrap_or_default();
    let wal = std::fs::read(env.d.home.path().join("overseer.sqlite-wal")).unwrap_or_default();
    for bytes in [&db, &wal] {
        let text = String::from_utf8_lossy(bytes);
        assert!(!text.contains("Priya") && !text.contains("lunch at noon"), "side talk reached the database");
    }
    let events = env.d.call("events.list", json!({"after": 0, "limit": 1000}));
    let s = events.to_string();
    assert!(!s.contains("\"level\"") || !s.contains("\"source\":\"owner\""), "levels were stored as events");
    assert_eq!(env.d.call("voice.requests", json!({}))["requests"].as_array().unwrap().len(), 0);
}

// ---------------------------------------------------------------------- AC-172

/// AC-172: a cue due while Overseer speaks waits for the end of its phrase; a routine cue due
/// while the owner speaks is dropped.
#[test]
fn ac172_cues_wait_for_overseer_s_phrase_and_give_way_to_the_owner() {
    let env = voice_daemon(&[]);
    env.d.call("audio.set", json!({"enabled": true}));
    let live = listening(&env);
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    // Overseer speaks a three-phrase line (one second a phrase); an agent starts meanwhile.
    env.d.call("voice.speak", json!({"text": "First phrase here, second phrase here, and the third phrase here."}));
    live.wait("speaking", 5, |v| v["kind"] == "spoke" && v["event"] == "start");
    let started = Instant::now();
    env.d.generic(&repo, "worktree", "/bin/echo", &["hi"]);
    let phrase = live.wait("the end of a phrase", 5, |v| v["kind"] == "spoke" && v["event"] == "phrase");
    let played = live.wait("the cue", 10, |v| v["kind"] == "cue" && v["result"] == "played");
    let (t_phrase, t_cue) = {
        let m = live.msgs.lock().unwrap();
        (m.iter().find(|(_, v)| *v == phrase).unwrap().0, m.iter().find(|(_, v)| *v == played).unwrap().0)
    };
    assert!(t_cue >= t_phrase, "the cue waited for the end of the phrase");
    assert!(t_cue.duration_since(started) < Duration::from_secs(3));
    assert!(env.cue_log().iter().any(|l| l.ends_with("agent_started")), "{:?}", env.cue_log());
    // While the owner speaks, a routine cue is dropped.
    live.clear();
    env.d.call("voice.simulate", json!({"speechlike": 3.0, "words": "just talking for a while now"}));
    live.wait("hearing", 5, |v| v["kind"] == "state" && v["state"] == "hearing");
    let before = env.cue_log().len();
    env.d.generic(&repo, "worktree", "/bin/echo", &["again"]);
    live.wait("a dropped cue", 5, |v| v["kind"] == "cue" && v["result"] == "dropped");
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(env.cue_log().len(), before, "nothing played while the owner spoke");
}

fn _unused(_: &Path) {}
