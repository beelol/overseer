//! Opt-in, daemon-owned audio cues. No playback occurs while disabled.
use crate::daemon::Daemon;
use crate::paths;
use crate::store::Event;
use anyhow::{anyhow, Result};
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

const KEYS: &[&str] = &[
    "agent_queued",
    "agent_started",
    "agent_resumed",
    "agent_progress",
    "agent_complete",
    "agent_stopped",
    "agent_needs_attention",
    "agent_failed",
    "agent_unblocked",
    "review_ready",
    "delivery_ready",
    "verification_passed",
];
const DEFAULT_KEYS: &[&str] = &["agent_started", "agent_complete", "agent_needs_attention"];
const MANIFEST: &str = include_str!("../assets/reactor/manifest.json");

#[derive(Clone, Copy, PartialEq, Eq)]
enum Track {
    Reactor,
    System,
    Commander,
}

impl Track {
    fn name(self) -> &'static str {
        match self {
            Self::Reactor => "reactor",
            Self::System => "system",
            Self::Commander => "commander",
        }
    }

    fn parse(name: &str) -> Result<Self> {
        match name {
            "reactor" => Ok(Self::Reactor),
            "system" => Ok(Self::System),
            "commander" => Ok(Self::Commander),
            _ => Err(anyhow!("unknown audio track")),
        }
    }
}

#[derive(Clone)]
struct Selection {
    track: Track,
    voice: String,
    commander_dir: Option<PathBuf>,
}

struct Cue {
    key: &'static str,
    preview: bool,
    selection: Selection,
}

struct Runtime {
    enabled: AtomicBool,
    routine: mpsc::Sender<Cue>,
    urgent: mpsc::Sender<Cue>,
}

impl Runtime {
    fn enqueue(&self, key: &'static str, preview: bool, selection: Selection) -> bool {
        let lane = if key == "agent_needs_attention" {
            &self.urgent
        } else {
            &self.routine
        };
        lane.try_send(Cue {
            key,
            preview,
            selection,
        })
        .is_ok()
    }
}

static RUNTIME: OnceLock<Runtime> = OnceLock::new();

fn test_sink() -> bool {
    std::env::var_os("OVERSEER_TEST_AUDIO_LOG").is_some()
}

fn player(path: &str) -> bool {
    // A test can take the players away to stand in for a platform without them.
    std::env::var_os("OVERSEER_TEST_AUDIO_UNAVAILABLE").is_none()
        && cfg!(target_os = "macos")
        && Path::new(path).exists()
}

fn reactor_available() -> bool {
    test_sink() || player("/usr/bin/afplay")
}

fn system_available() -> bool {
    test_sink() || player("/usr/bin/say")
}

fn meta(conn: &rusqlite::Connection, key: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row("SELECT value FROM meta WHERE key=?1", [key], |r| r.get(0))
        .optional()?)
}

fn stored_enabled(d: &Arc<Daemon>) -> Result<bool> {
    let store = d.store.lock().unwrap();
    Ok(meta(&store.conn, "audio.reactor.enabled")?.as_deref() == Some("1"))
}

fn selection(d: &Arc<Daemon>) -> Result<Selection> {
    let store = d.store.lock().unwrap();
    let track =
        Track::parse(&meta(&store.conn, "audio.track")?.unwrap_or_else(|| "reactor".into()))?;
    let voice = meta(&store.conn, "audio.system_voice")?.unwrap_or_default();
    let commander_dir = meta(&store.conn, "audio.commander_dir")?.map(PathBuf::from);
    Ok(Selection {
        track,
        voice,
        commander_dir,
    })
}

fn commander_file(dir: &Path, key: &str) -> PathBuf {
    dir.join(key).join("transmission/commander.wav")
}

fn valid_commander_pack(dir: &Path) -> Result<()> {
    use std::io::Read;
    for key in DEFAULT_KEYS {
        let path = commander_file(dir, key);
        let meta = std::fs::metadata(&path).map_err(|_| {
            anyhow!("not a Commander pack: {key}/transmission/commander.wav is missing")
        })?;
        if !meta.is_file() || !(16..=10_000_000).contains(&meta.len()) {
            return Err(anyhow!(
                "private Commander pack needs three valid WAV files"
            ));
        }
        let mut header = [0u8; 12];
        std::fs::File::open(path)?.read_exact(&mut header)?;
        if &header[..4] != b"RIFF" || &header[8..] != b"WAVE" {
            return Err(anyhow!("private Commander pack contains a non-WAV file"));
        }
    }
    Ok(())
}

fn playback_available(selected: &Selection) -> bool {
    match selected.track {
        Track::Reactor => reactor_available(),
        Track::System => system_available(),
        Track::Commander => {
            reactor_available()
                && selected
                    .commander_dir
                    .as_deref()
                    .is_some_and(|dir| valid_commander_pack(dir).is_ok())
        }
    }
}

pub fn get(d: &Arc<Daemon>) -> Result<Value> {
    let selected = selection(d)?;
    Ok(json!({
        "enabled": stored_enabled(d)?,
        "available": playback_available(&selected),
        "track": selected.track.name(),
        "voice": selected.voice,
        "commander_imported": selected.commander_dir.as_deref().is_some_and(|dir| valid_commander_pack(dir).is_ok()),
        "pack": "reactor",
        "keys": KEYS,
        "default_keys": DEFAULT_KEYS,
        "manifest": serde_json::from_str::<Value>(MANIFEST)?,
    }))
}

pub fn set(d: &Arc<Daemon>, p: &Value) -> Result<Value> {
    if p.get("enabled").is_none() && p.get("track").is_none() && p.get("voice").is_none() {
        return Err(anyhow!("provide enabled, track, or voice"));
    }
    let enabled = match p.get("enabled") {
        Some(value) => value
            .as_bool()
            .ok_or_else(|| anyhow!("enabled must be a boolean"))?,
        None => stored_enabled(d)?,
    };
    let mut selected = selection(d)?;
    if let Some(value) = p.get("track") {
        selected.track = Track::parse(
            value
                .as_str()
                .ok_or_else(|| anyhow!("track must be a string"))?,
        )?;
    }
    if let Some(value) = p.get("voice") {
        selected.voice = value
            .as_str()
            .ok_or_else(|| anyhow!("voice must be a string"))?
            .to_string();
        if !selected.voice.is_empty()
            && !test_sink()
            && !installed_voices()?
                .iter()
                .any(|(name, _)| name == &selected.voice)
        {
            return Err(anyhow!("system voice is not installed"));
        }
    }
    if selected.track == Track::Commander && !playback_available(&selected) {
        return Err(anyhow!("import the private Commander pack first"));
    }
    if enabled && !playback_available(&selected) {
        return Err(anyhow!("selected audio track is unavailable on this Mac"));
    }
    {
        let store = d.store.lock().unwrap();
        let transaction = store.conn.unchecked_transaction()?;
        for (key, value) in [
            ("audio.reactor.enabled", if enabled { "1" } else { "0" }),
            ("audio.track", selected.track.name()),
            ("audio.system_voice", selected.voice.as_str()),
        ] {
            transaction.execute("INSERT INTO meta(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value", params![key, value])?;
        }
        transaction.commit()?;
    }
    if let Some(runtime) = RUNTIME.get() {
        runtime.enabled.store(enabled, Ordering::Relaxed);
    }
    get(d)
}

pub fn import_commander(d: &Arc<Daemon>, p: &Value) -> Result<Value> {
    let path = p["path"]
        .as_str()
        .ok_or_else(|| anyhow!("path must be a string"))?;
    let directory =
        std::fs::canonicalize(path).map_err(|_| anyhow!("that folder does not exist"))?;
    valid_commander_pack(&directory)?;
    d.store.lock().unwrap().conn.execute(
        "INSERT INTO meta(key,value) VALUES('audio.commander_dir',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        [directory.to_string_lossy().as_ref()],
    )?;
    Ok(json!({"imported": true}))
}

fn installed_voices() -> Result<Vec<(String, String)>> {
    if !player("/usr/bin/say") {
        return Err(anyhow!("macOS system speech is unavailable"));
    }
    let output = std::process::Command::new("/usr/bin/say")
        .args(["-v", "?"])
        .output()?;
    if !output.status.success() {
        return Err(anyhow!("could not list system voices"));
    }
    let mut voices = Vec::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let fields: Vec<&str> = line
            .split('#')
            .next()
            .unwrap_or("")
            .split_whitespace()
            .collect();
        if fields.len() < 2 {
            continue;
        }
        voices.push((
            fields[..fields.len() - 1].join(" "),
            fields[fields.len() - 1].to_string(),
        ));
    }
    Ok(voices)
}

pub fn voices() -> Result<Value> {
    Ok(json!(installed_voices()?
        .into_iter()
        .map(|(name, locale)| json!({"name":name,"locale":locale}))
        .collect::<Vec<_>>()))
}

pub fn preview(d: &Arc<Daemon>, p: &Value) -> Result<Value> {
    let key = p["key"]
        .as_str()
        .ok_or_else(|| anyhow!("key must be a string"))?;
    let selected = KEYS
        .iter()
        .copied()
        .find(|candidate| *candidate == key)
        .ok_or_else(|| anyhow!("unknown audio cue"))?;
    let config = selection(d)?;
    if config.track != Track::Reactor && !DEFAULT_KEYS.contains(&selected) {
        return Err(anyhow!("this track has only the three core cues"));
    }
    if !playback_available(&config) {
        return Err(anyhow!("selected audio track is unavailable"));
    }
    if !enqueue(selected, true, config) {
        return Err(anyhow!("audio is busy; try the preview again"));
    }
    Ok(json!({"queued": true, "key": selected}))
}

fn enqueue(key: &'static str, preview: bool, selection: Selection) -> bool {
    // Both lanes are bounded; attention cannot be displaced by routine bursts.
    RUNTIME
        .get()
        .is_some_and(|runtime| runtime.enqueue(key, preview, selection))
}

#[derive(Default)]
struct BurstGate {
    last_by_key: HashMap<&'static str, Instant>,
}

impl BurstGate {
    fn allow(&mut self, key: &'static str, now: Instant) -> bool {
        if self
            .last_by_key
            .get(key)
            .is_some_and(|last| now.duration_since(*last) < Duration::from_millis(800))
        {
            return false;
        }
        self.last_by_key.insert(key, now);
        true
    }
}

/// Start once after reconciliation. Subscribe only to future live events: a daemon
/// restart must not replay old cues.
pub fn start(d: Arc<Daemon>) -> Result<()> {
    let enabled = stored_enabled(&d)?;
    let mut events = d.events.subscribe();
    let (routine_tx, mut routine_rx) = mpsc::channel::<Cue>(4);
    let (urgent_tx, mut urgent_rx) = mpsc::channel::<Cue>(2);
    let _ = RUNTIME.set(Runtime {
        enabled: AtomicBool::new(enabled),
        routine: routine_tx,
        urgent: urgent_tx,
    });
    tokio::spawn(async move {
        loop {
            let cue = tokio::select! {
                biased;
                Some(cue) = urgent_rx.recv() => cue,
                Some(cue) = routine_rx.recv() => cue,
                else => break,
            };
            if !cue.preview
                && !RUNTIME
                    .get()
                    .is_some_and(|runtime| runtime.enabled.load(Ordering::Relaxed))
            {
                continue;
            }
            match tokio::task::spawn_blocking(move || play(cue.key, &cue.selection)).await {
                Ok(Ok(())) => {}
                Ok(Err(e)) => crate::log(&format!("audio playback failed: {e}")),
                Err(e) => crate::log(&format!("audio worker ended: {e}")),
            }
        }
    });
    tokio::spawn(async move {
        let mut attention = HashSet::<String>::new();
        let mut burst = BurstGate::default();
        loop {
            let event = match events.recv().await {
                Ok(event) => event,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => break,
            };
            let Some(runtime) = RUNTIME.get() else { break };
            if !runtime.enabled.load(Ordering::Relaxed) {
                continue;
            }
            let Some(run_id) = event.run_id.as_deref() else {
                continue;
            };
            let is_root = d
                .store
                .lock()
                .unwrap()
                .run(run_id)
                .ok()
                .flatten()
                .is_some_and(|run| run.parent_run_id.is_none());
            if !is_root {
                continue;
            }
            let Some(key) = classify(&event, &mut attention) else {
                continue;
            };
            // Keep simultaneous attention quiet even when start or completion
            // events from other agents arrive between blocker events.
            if !burst.allow(key, Instant::now()) {
                continue;
            }
            match selection(&d) {
                Ok(config) => {
                    let _ = enqueue(key, false, config);
                }
                Err(e) => crate::log(&format!("audio selection failed: {e}")),
            }
        }
    });
    Ok(())
}

const MAX_ATTENTION_HISTORY: usize = 1024;

fn remember_attention(attention: &mut HashSet<String>, run_id: &str) -> bool {
    if attention.contains(run_id) {
        return false;
    }
    if attention.len() >= MAX_ATTENTION_HISTORY {
        // Terminal failures can remain in the log indefinitely; keep only a fixed
        // number of recent dedupe identities in this long-lived process.
        if let Some(evicted) = attention.iter().next().cloned() {
            attention.remove(&evicted);
        }
    }
    attention.insert(run_id.to_owned())
}

fn classify(event: &Event, attention: &mut HashSet<String>) -> Option<&'static str> {
    let run_id = event.run_id.as_ref()?;
    if event.kind == "turn_started" && event.payload["turn"]["n"] == 1 {
        return Some("agent_started");
    }
    if event.kind != "status" {
        return None;
    }
    match event.payload["status"].as_str()? {
        "waiting_for_user" | "failed" | "disconnected" if remember_attention(attention, run_id) => {
            Some("agent_needs_attention")
        }
        "completed" => {
            attention.remove(run_id);
            Some("agent_complete")
        }
        "running" | "starting" | "queued" | "interrupted" => {
            attention.remove(run_id);
            None
        }
        _ => None,
    }
}

fn bytes(key: &str) -> &'static [u8] {
    match key {
        "agent_queued" => include_bytes!("../assets/reactor/agent_queued.mp3"),
        "agent_started" => include_bytes!("../assets/reactor/agent_started.mp3"),
        "agent_resumed" => include_bytes!("../assets/reactor/agent_resumed.mp3"),
        "agent_progress" => include_bytes!("../assets/reactor/agent_progress.mp3"),
        "agent_complete" => include_bytes!("../assets/reactor/agent_complete.mp3"),
        "agent_stopped" => include_bytes!("../assets/reactor/agent_stopped.mp3"),
        "agent_needs_attention" => include_bytes!("../assets/reactor/agent_needs_attention.mp3"),
        "agent_failed" => include_bytes!("../assets/reactor/agent_failed.mp3"),
        "agent_unblocked" => include_bytes!("../assets/reactor/agent_unblocked.mp3"),
        "review_ready" => include_bytes!("../assets/reactor/review_ready.mp3"),
        "delivery_ready" => include_bytes!("../assets/reactor/delivery_ready.mp3"),
        "verification_passed" => include_bytes!("../assets/reactor/verification_passed.mp3"),
        _ => unreachable!("validated cue key"),
    }
}

fn play(key: &str, selected: &Selection) -> Result<()> {
    if let Some(path) = std::env::var_os("OVERSEER_TEST_AUDIO_LOG") {
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        writeln!(file, "{}:{key}", selected.track.name())?;
        return Ok(());
    }
    match selected.track {
        Track::Reactor => play_reactor(key),
        Track::System => play_system(key, &selected.voice),
        Track::Commander => {
            let dir = selected
                .commander_dir
                .as_deref()
                .ok_or_else(|| anyhow!("private Commander pack is not imported"))?;
            play_file(&commander_file(dir, key))
        }
    }
}

fn play_file(path: &Path) -> Result<()> {
    let status = std::process::Command::new("/usr/bin/afplay")
        .arg(path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()?;
    if !status.success() {
        return Err(anyhow!("afplay exited with {status}"));
    }
    Ok(())
}

fn play_system(key: &str, voice: &str) -> Result<()> {
    let phrase = match key {
        "agent_started" => "Agent started.",
        "agent_complete" => "Agent complete.",
        "agent_needs_attention" => "An agent needs your attention.",
        _ => return Err(anyhow!("system speech has no phrase for {key}")),
    };
    let mut command = std::process::Command::new("/usr/bin/say");
    if !voice.is_empty() {
        command.arg("-v").arg(voice);
    }
    let status = command
        .arg(phrase)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()?;
    if !status.success() {
        return Err(anyhow!("system speech exited with {status}"));
    }
    Ok(())
}

fn play_reactor(key: &str) -> Result<()> {
    if !reactor_available() {
        return Ok(());
    }
    let dir = paths::data_dir().join("audio/reactor-v1");
    paths::ensure_private_dir(&dir)?;
    play_file(&cached_cue(&dir, key)?)
}

fn cached_cue(dir: &Path, key: &str) -> Result<PathBuf> {
    let path = dir.join(format!("{key}.mp3"));
    // Compare the content, not the length: a cue from an earlier pack can have
    // the same size as the bundled one.
    if std::fs::read(&path).ok().as_deref() != Some(bytes(key)) {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let tmp = dir.join(format!("{key}.tmp"));
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp)?;
        file.write_all(bytes(key))?;
        std::fs::rename(tmp, &path)?;
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn event(kind: &str, status: Value) -> Event {
        Event {
            seq: 1,
            ts: 0,
            task_id: None,
            run_id: Some("root".into()),
            kind: kind.into(),
            source: "test".into(),
            confidence: "exact".into(),
            payload: status,
        }
    }
    #[test]
    fn core_transitions_are_broad_and_attention_is_deduped() {
        let mut attention = HashSet::new();
        assert_eq!(
            classify(
                &event("turn_started", json!({"turn":{"n":1}})),
                &mut attention
            ),
            Some("agent_started")
        );
        assert_eq!(
            classify(
                &event("turn_started", json!({"turn":{"n":2}})),
                &mut attention
            ),
            None
        );
        assert_eq!(
            classify(
                &event("permission", json!({"kind":"permission"})),
                &mut attention
            ),
            None
        );
        assert_eq!(
            classify(
                &event("status", json!({"status":"waiting_for_user"})),
                &mut attention
            ),
            Some("agent_needs_attention")
        );
        assert_eq!(
            classify(
                &event("status", json!({"status":"waiting_for_user"})),
                &mut attention
            ),
            None
        );
        assert_eq!(
            classify(
                &event("status", json!({"status":"running"})),
                &mut attention
            ),
            None
        );
        assert_eq!(
            classify(
                &event("status", json!({"status":"waiting_for_user"})),
                &mut attention
            ),
            Some("agent_needs_attention")
        );
        assert_eq!(
            classify(
                &event("status", json!({"status":"completed"})),
                &mut attention
            ),
            Some("agent_complete")
        );
        assert_eq!(
            classify(&event("status", json!({"status":"failed"})), &mut attention),
            Some("agent_needs_attention")
        );
        assert_eq!(
            classify(&event("status", json!({"status":"failed"})), &mut attention),
            None
        );
        assert_eq!(
            classify(
                &event("status", json!({"status":"disconnected"})),
                &mut attention
            ),
            None
        );
    }
    #[test]
    fn attention_can_queue_when_routine_cues_fill_their_lane() {
        let (routine, _routine_rx) = mpsc::channel(4);
        let (urgent, _urgent_rx) = mpsc::channel(2);
        let runtime = Runtime {
            enabled: AtomicBool::new(true),
            routine,
            urgent,
        };
        let selection = Selection {
            track: Track::Reactor,
            voice: String::new(),
            commander_dir: None,
        };
        for _ in 0..4 {
            assert!(runtime.enqueue("agent_started", false, selection.clone()));
        }
        assert!(runtime.enqueue("agent_needs_attention", false, selection));
    }
    #[test]
    fn simultaneous_attention_is_coalesced_even_when_starts_interleave() {
        let start = Instant::now();
        let mut gate = BurstGate::default();
        assert!(gate.allow("agent_needs_attention", start));
        assert!(gate.allow("agent_started", start + Duration::from_millis(100)));
        assert!(!gate.allow("agent_needs_attention", start + Duration::from_millis(200)));
        assert!(gate.allow("agent_needs_attention", start + Duration::from_millis(900)));
    }
    #[test]
    fn attention_history_stays_bounded_during_long_daemon_uptime() {
        let mut attention = HashSet::new();
        for i in 0..1100 {
            let mut item = event("status", json!({"status":"failed"}));
            item.run_id = Some(format!("failed-{i}"));
            assert_eq!(
                classify(&item, &mut attention),
                Some("agent_needs_attention")
            );
        }
        assert!(attention.len() <= 1024);
    }
    #[test]
    fn pack_has_twelve_short_original_cues() {
        let manifest: Value = serde_json::from_str(MANIFEST).unwrap();
        assert_eq!(manifest.as_array().unwrap().len(), KEYS.len());
        for cue in manifest.as_array().unwrap() {
            let key = cue["key"].as_str().unwrap();
            assert!(KEYS.contains(&key));
            assert!(cue["duration"].as_f64().unwrap() < 0.5);
            assert!(bytes(key).len() < 4_000);
            assert!(cue["provenance"].as_str().unwrap().contains("original"));
        }
    }
    #[test]
    fn a_cached_cue_from_another_pack_is_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let bundled = bytes("agent_started");
        let stale = dir.path().join("agent_started.mp3");
        std::fs::write(&stale, vec![0u8; bundled.len()]).unwrap();
        let path = cached_cue(dir.path(), "agent_started").unwrap();
        assert_eq!(path, stale);
        assert_eq!(std::fs::read(&path).unwrap(), bundled);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }
    #[test]
    fn manifest_describes_the_bundled_bytes() {
        use sha2::{Digest, Sha256};
        let manifest: Value = serde_json::from_str(MANIFEST).unwrap();
        let cues = manifest.as_array().unwrap();
        let listed: Vec<&str> = cues.iter().map(|cue| cue["key"].as_str().unwrap()).collect();
        assert_eq!(listed, KEYS, "one entry per key, in the daemon's order");
        let mut total = 0;
        for cue in cues {
            let key = cue["key"].as_str().unwrap();
            let bundled = bytes(key);
            total += bundled.len();
            assert_eq!(cue["mp3"], format!("{key}.mp3"));
            assert_eq!(cue["mp3_bytes"], bundled.len(), "{key}");
            assert_eq!(
                cue["sha256"].as_str().unwrap(),
                format!("{:x}", Sha256::digest(bundled)),
                "{key}"
            );
            assert_eq!(
                cue["default_auto"],
                DEFAULT_KEYS.contains(&key),
                "{key}: only the three core cues play by themselves"
            );
            assert!(cue.get("wav").is_none(), "{key}: no WAV is part of the pack");
        }
        assert_eq!(total, 31_488, "the owner-approved pack");
    }
}
