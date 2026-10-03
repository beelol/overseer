//! AC-287/288 baseline: synthetic complete folder packs through the real daemon API.
//! Authored before implementation; unrun until the coordinator grants a slot.
//! The short PCM fixtures prove mapping/validation, never canonical spoken content.
mod common;
use common::*;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const LINES: &[(&str, &str)] = &[
    ("agent_started", "Agent started."),
    ("agent_complete", "Agent complete."),
    ("agent_permission_required", "Agent needs permission."),
    ("agent_reply_required", "Agent needs a reply."),
    ("agent_sign_in_required", "Agent needs you to sign in."),
    ("agent_cannot_continue", "Agent cannot continue."),
    ("agent_failed", "Agent failed."),
    ("agent_stopped_unexpectedly", "Agent stopped unexpectedly."),
    ("agents_need_attention", "Several agents need attention."),
    ("swarm_initiated", "Swarm initiated."),
    ("swarm_complete", "Swarm complete."),
    ("swarm_needs_attention", "Swarm needs attention."),
];

fn wav(sample: i16) -> Vec<u8> {
    let frames = 160u32;
    let mut b = Vec::new();
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + frames * 2).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&8000u32.to_le_bytes());
    b.extend_from_slice(&16000u32.to_le_bytes());
    b.extend_from_slice(&2u16.to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&(frames * 2).to_le_bytes());
    for _ in 0..frames {
        b.extend_from_slice(&sample.to_le_bytes());
    }
    b
}
fn pack(dir: &Path, id: &str) -> Value {
    std::fs::create_dir_all(dir.join("audio")).unwrap();
    let mut lines = serde_json::Map::new();
    for (n, (key, _)) in LINES.iter().enumerate() {
        let relative = format!("audio/{key}.wav");
        std::fs::write(dir.join(&relative), wav(n as i16 + 1)).unwrap();
        lines.insert((*key).into(), json!(relative));
    }
    let manifest = json!({"schema":1,"id":id,"label":"Synthetic fixture pack","lines":lines});
    save_manifest(dir, &manifest);
    manifest
}
fn save_manifest(dir: &Path, manifest: &Value) {
    std::fs::write(
        dir.join("audio-pack.json"),
        serde_json::to_vec(manifest).unwrap(),
    )
    .unwrap();
}
fn revision(d: &Daemon) -> i64 {
    d.call("audio.get", json!({}))["revision"]
        .as_i64()
        .unwrap_or(0)
}
fn select(d: &Daemon, dir: &Path) -> Value {
    d.try_call(
        "audio.source.set",
        json!({"source":"folder","path":dir,"expected_revision":revision(d)}),
    )
    .expect("complete twelve-key folder source must be selectable")
}
fn audio_files(root: &Path) -> Vec<PathBuf> {
    let mut todo = vec![root.to_path_buf()];
    let mut out = vec![];
    while let Some(dir) = todo.pop() {
        for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let p = e.path();
            let kind = e.file_type().unwrap();
            if kind.is_dir() {
                todo.push(p);
            } else if matches!(p.extension().and_then(|x| x.to_str()), Some("wav" | "mp3")) {
                out.push(p);
            }
        }
    }
    out
}
fn wait_log(path: &Path, expected: usize) -> String {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let value = std::fs::read_to_string(path).unwrap_or_default();
        if value.lines().count() >= expected {
            return value;
        }
        assert!(
            Instant::now() < deadline,
            "expected {expected} bounded synthetic playback receipts, got {value:?}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn public_inventory_is_the_exact_twelve_phrases_and_old_keys_are_not_previews() {
    let d = Daemon::start(&[]);
    let current = d.call("audio.get", json!({}));
    assert_eq!(current["enabled"], false);
    assert_eq!(
        current["lines"],
        json!(LINES
            .iter()
            .map(|(key, phrase)| json!({"key":key,"phrase":phrase}))
            .collect::<Vec<_>>())
    );
    for key in [
        "agent_queued",
        "agent_unblocked",
        "agent_stopped",
        "agent_needs_attention",
        "review_ready",
    ] {
        assert!(
            d.try_call("audio.preview", json!({"key":key})).is_err(),
            "legacy key {key} is not an exposed Audio Mode line"
        );
    }
    assert!(
        !d.home.path().join("audio").exists(),
        "inspection while off does not materialize assets"
    );
}

#[test]
fn complete_folder_selection_stays_off_private_in_place_and_survives_restart() {
    let root = tmp();
    let folder = root.path().join("synthetic-local-pack");
    pack(&folder, "fixture-a");
    let bytes_before = std::fs::read(folder.join("audio/agent_started.wav")).unwrap();
    let mut d = Daemon::start(&[]);
    let selected = select(&d, &folder);
    assert_eq!(selected["source"]["kind"], "folder");
    assert_eq!(selected["source"]["label"], "Synthetic fixture pack");
    assert_eq!(selected["enabled"], false);
    assert!(
        !selected.to_string().contains(folder.to_str().unwrap()),
        "public projection hides private path"
    );
    assert!(
        audio_files(d.home.path()).is_empty(),
        "folder selection never copies audio"
    );
    d.kill9();
    d.spawn();
    let after = d.call("audio.get", json!({}));
    assert_eq!(after["source"], selected["source"]);
    assert_eq!(after["revision"], selected["revision"]);
    assert_eq!(after["enabled"], false);
    assert_eq!(
        std::fs::read(folder.join("audio/agent_started.wav")).unwrap(),
        bytes_before
    );
    assert!(audio_files(d.home.path()).is_empty());
}

#[test]
fn incomplete_unknown_and_wrong_schema_manifests_leave_the_prior_selection_unchanged() {
    let root = tmp();
    let good = root.path().join("good");
    let bad = root.path().join("bad");
    pack(&good, "fixture-good");
    let original = pack(&bad, "fixture-bad");
    let d = Daemon::start(&[]);
    let before = select(&d, &good);
    let mut missing = original.clone();
    missing["lines"]
        .as_object_mut()
        .unwrap()
        .remove("swarm_needs_attention");
    let mut extra = original.clone();
    extra["lines"]["agent_unblocked"] = json!("audio/agent_started.wav");
    let mut schema = original.clone();
    schema["schema"] = json!(2);
    let mut wrong_type = original.clone();
    wrong_type["lines"]["agent_started"] = json!(42);
    for invalid in [missing, extra, schema, wrong_type] {
        save_manifest(&bad, &invalid);
        assert!(d
            .try_call(
                "audio.source.set",
                json!({"source":"folder","path":bad,"expected_revision":before["revision"]})
            )
            .is_err());
        assert_eq!(
            d.call("audio.get", json!({})),
            before,
            "failed validation commits no partial settings"
        );
    }
}

#[test]
fn unsafe_manifest_paths_and_non_audio_files_are_refused_without_private_path_errors() {
    let root = tmp();
    let good = root.path().join("good");
    let bad = root.path().join("opaque-private-folder");
    pack(&good, "fixture-good");
    let original = pack(&bad, "fixture-bad");
    let d = Daemon::start(&[]);
    let before = select(&d, &good);
    let outside = root.path().join("outside.wav");
    std::fs::write(&outside, wav(8)).unwrap();
    for unsafe_path in [
        outside.to_string_lossy().to_string(),
        "../outside.wav".into(),
        "audio/not-audio.wav".into(),
        "audio/link.wav".into(),
    ] {
        std::fs::write(bad.join("audio/not-audio.wav"), b"not a decoded audio file").unwrap();
        #[cfg(unix)]
        {
            let link = bad.join("audio/link.wav");
            let _ = std::fs::remove_file(&link);
            std::os::unix::fs::symlink(&outside, &link).unwrap();
        }
        let mut invalid = original.clone();
        invalid["lines"]["agent_started"] = json!(unsafe_path);
        save_manifest(&bad, &invalid);
        let error = d
            .try_call(
                "audio.source.set",
                json!({"source":"folder","path":bad,"expected_revision":before["revision"]}),
            )
            .unwrap_err();
        assert!(
            !error.contains(bad.to_str().unwrap()) && !error.contains(outside.to_str().unwrap()),
            "safe diagnostic must not echo private paths: {error}"
        );
        assert_eq!(d.call("audio.get", json!({})), before);
    }
}

#[test]
fn a_stale_source_revision_cannot_replace_another_clients_selected_folder() {
    let root = tmp();
    let a = root.path().join("a");
    let b = root.path().join("b");
    pack(&a, "fixture-a");
    pack(&b, "fixture-b");
    let d = Daemon::start(&[]);
    let old = revision(&d);
    let current = select(&d, &a);
    assert!(current["revision"].as_i64().unwrap() > old);
    let error = d
        .try_call(
            "audio.source.set",
            json!({"source":"folder","path":b,"expected_revision":old}),
        )
        .unwrap_err();
    assert!(
        error.contains("revision"),
        "conflict is actionable: {error}"
    );
    assert_eq!(d.call("audio.get", json!({})), current);
    assert_eq!(current["enabled"], false);
}

#[test]
fn removing_the_selected_folder_shows_unavailable_without_switching_to_builtin() {
    let root = tmp();
    let folder = root.path().join("selected");
    pack(&folder, "fixture-missing");
    let log = root.path().join("audio.log");
    let d = Daemon::start(&[("OVERSEER_TEST_AUDIO_LOG", log.to_str().unwrap())]);
    let selected = select(&d, &folder);
    std::fs::remove_dir_all(&folder).unwrap();
    let missing = d.call("audio.get", json!({}));
    assert_eq!(missing["source"]["kind"], "folder");
    assert_eq!(missing["available"], false);
    assert_eq!(missing["source"]["available"], false);
    assert_eq!(missing["revision"], selected["revision"]);
    assert!(!missing.to_string().contains(folder.to_str().unwrap()));
    assert!(d
        .try_call("audio.preview", json!({"key":"agent_started"}))
        .is_err());
    assert!(!log.exists(), "no fallback player receipt");
    assert!(audio_files(d.home.path()).is_empty());
}

#[test]
fn all_twelve_explicit_previews_use_the_folder_while_audio_stays_off() {
    let root = tmp();
    let folder = root.path().join("selected");
    pack(&folder, "fixture-previews");
    let log = root.path().join("audio.log");
    let d = Daemon::start(&[("OVERSEER_TEST_AUDIO_LOG", log.to_str().unwrap())]);
    select(&d, &folder);
    let mut expected = vec![];
    for (key, _) in LINES {
        assert_eq!(d.call("audio.preview", json!({"key":key}))["queued"], true);
        expected.push(format!("fixture-previews:{key}"));
        let receipt = wait_log(&log, expected.len());
        assert_eq!(
            receipt.lines().collect::<Vec<_>>(),
            expected.iter().map(String::as_str).collect::<Vec<_>>()
        );
    }
    assert_eq!(d.call("audio.get", json!({}))["enabled"], false);
    assert!(
        audio_files(d.home.path()).is_empty(),
        "preview does not cache private audio"
    );
}

#[test]
fn legacy_private_selection_migrates_unavailable_without_enabling_or_falling_back() {
    let root = tmp();
    let private = root.path().join("legacy-private-path");
    let mut d = Daemon::start(&[]);
    d.kill9();
    // Real old meta settings, not fabricated task/run state. Migration must retain
    // selection privacy even when that old three-file folder is not available.
    let db = rusqlite::Connection::open_with_flags(
        d.home.path().join("overseer.sqlite"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE,
    )
    .unwrap();
    for (key, value) in [
        ("audio.track", "commander"),
        ("audio.commander_dir", private.to_str().unwrap()),
        ("audio.reactor.enabled", "0"),
    ] {
        db.execute("INSERT INTO meta(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",[key,value]).unwrap();
    }
    drop(db);
    d.spawn();
    let migrated = d.call("audio.get", json!({}));
    assert_eq!(migrated["source"]["kind"], "folder");
    assert_eq!(migrated["source"]["available"], false);
    assert_eq!(migrated["enabled"], false);
    assert!(!migrated.to_string().contains(private.to_str().unwrap()));
    assert!(d
        .try_call(
            "audio.set",
            json!({"enabled":true,"expected_revision":migrated["revision"]})
        )
        .is_err());
    assert_eq!(d.call("audio.get", json!({}))["enabled"], false);
    assert!(audio_files(d.home.path()).is_empty());
}

// Additional boundary fixtures, authored for the next allocated RED/control run.
// Their later assertions were NOT reached by the original missing-method baseline.
#[test]
fn manifest_media_and_duration_limits_refuse_without_committing_selection() {
    let root = tmp();
    let folder = root.path().join("limits");
    let original = pack(&folder, "fixture-limits");
    let d = Daemon::start(&[]);
    let before = d.call("audio.get", json!({}));
    let attempt = || {
        d.try_call(
            "audio.source.set",
            json!({"source":"folder","path":folder,"expected_revision":before["revision"]}),
        )
    };
    let mut manifest = serde_json::to_vec(&original).unwrap();
    manifest.resize(64 * 1024 + 1, b' ');
    std::fs::write(folder.join("audio-pack.json"), manifest).unwrap();
    assert!(attempt().is_err(), "oversized manifest must refuse");
    assert_eq!(d.call("audio.get", json!({})), before);
    save_manifest(&folder, &original);
    let audio = std::fs::OpenOptions::new()
        .write(true)
        .open(folder.join("audio/agent_started.wav"))
        .unwrap();
    audio.set_len(8 * 1024 * 1024 + 1).unwrap();
    drop(audio);
    assert!(
        attempt().is_err(),
        "sparse oversized media must refuse without loading it"
    );
    assert_eq!(d.call("audio.get", json!({})), before);
    // Genuine PCM just over fifteen seconds, not only a forged duration header.
    let frames = 15 * 8000u32 + 1;
    let mut long = wav(1);
    long.resize(44 + frames as usize * 2, 0);
    long[4..8].copy_from_slice(&(36 + frames * 2).to_le_bytes());
    long[40..44].copy_from_slice(&(frames * 2).to_le_bytes());
    std::fs::write(folder.join("audio/agent_started.wav"), long).unwrap();
    assert!(
        attempt().is_err(),
        "actual PCM exceeding fifteen seconds must refuse"
    );
    assert_eq!(d.call("audio.get", json!({})), before);
}

#[test]
fn truncated_wav_and_non_decodable_mp3_refuse_without_committing_selection() {
    let root = tmp();
    let folder = root.path().join("malformed");
    let mut manifest = pack(&folder, "fixture-malformed");
    let d = Daemon::start(&[]);
    let before = d.call("audio.get", json!({}));
    let mut truncated = wav(1);
    truncated.truncate(44);
    std::fs::write(folder.join("audio/agent_started.wav"), truncated).unwrap();
    assert!(
        d.try_call(
            "audio.source.set",
            json!({"source":"folder","path":folder,"expected_revision":before["revision"]})
        )
        .is_err(),
        "RIFF header alone is not playable audio"
    );
    assert_eq!(d.call("audio.get", json!({})), before);
    manifest["lines"]["agent_started"] = json!("audio/broken.mp3");
    save_manifest(&folder, &manifest);
    std::fs::write(
        folder.join("audio/broken.mp3"),
        b"ID3\0\0\0not a complete MP3 stream",
    )
    .unwrap();
    assert!(d
        .try_call(
            "audio.source.set",
            json!({"source":"folder","path":folder,"expected_revision":before["revision"]})
        )
        .is_err());
    assert_eq!(d.call("audio.get", json!({})), before);
}

#[test]
fn parent_symlink_and_fifo_refuse_without_committing_selection() {
    let root = tmp();
    let folder = root.path().join("special");
    let outside = root.path().join("outside");
    let manifest = pack(&folder, "fixture-special");
    pack(&outside, "fixture-outside");
    let d = Daemon::start(&[]);
    let before = d.call("audio.get", json!({}));
    std::fs::remove_dir_all(folder.join("audio")).unwrap();
    std::os::unix::fs::symlink(outside.join("audio"), folder.join("audio")).unwrap();
    assert!(
        d.try_call(
            "audio.source.set",
            json!({"source":"folder","path":folder,"expected_revision":before["revision"]})
        )
        .is_err(),
        "a parent symlink may not escape the selected pack"
    );
    assert_eq!(d.call("audio.get", json!({})), before);
    std::fs::remove_file(folder.join("audio")).unwrap();
    pack(&folder, "fixture-special");
    save_manifest(&folder, &manifest);
    let leaf = folder.join("audio/agent_started.wav");
    std::fs::remove_file(&leaf).unwrap();
    use std::os::unix::ffi::OsStrExt;
    let name = std::ffi::CString::new(leaf.as_os_str().as_bytes()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
    let start = Instant::now();
    assert!(
        d.try_call(
            "audio.source.set",
            json!({"source":"folder","path":folder,"expected_revision":before["revision"]})
        )
        .is_err(),
        "a FIFO must be rejected, not read"
    );
    assert!(
        start.elapsed() < Duration::from_secs(2),
        "special-file validation must not wait for a writer"
    );
    assert_eq!(d.call("audio.get", json!({})), before);
}

#[test]
fn source_fields_and_enablement_revision_are_strict_and_never_model_authority() {
    let root = tmp();
    let folder = root.path().join("strict");
    pack(&folder, "fixture-strict");
    let log = root.path().join("strict.log");
    let d = Daemon::start(&[("OVERSEER_TEST_AUDIO_LOG", log.to_str().unwrap())]);
    let before = d.call("audio.get", json!({}));
    for fields in [
        json!({"source":"builtin","path":folder,"expected_revision":before["revision"]}),
        json!({"source":"folder","path":folder,"expected_revision":before["revision"],"actor":"owner"}),
        json!({"source":"folder","path":folder,"expected_revision":before["revision"],"approved":true}),
    ] {
        assert!(d.try_call("audio.source.set", fields).is_err());
        assert_eq!(d.call("audio.get", json!({})), before);
    }
    let selected = select(&d, &folder);
    assert!(d
        .try_call(
            "audio.set",
            json!({"enabled":true,"expected_revision":before["revision"]})
        )
        .is_err());
    assert_eq!(d.call("audio.get", json!({})), selected);
    assert!(d
        .try_call(
            "audio.set",
            json!({"track":"system","expected_revision":selected["revision"]})
        )
        .is_err());
    assert_eq!(d.call("audio.get", json!({})), selected);
}

struct AudioGate(PathBuf);
impl Drop for AudioGate {
    fn drop(&mut self) {
        let _ = std::fs::write(self.0.join("release"), b"release");
    }
}
fn gate_ready(gate: &Path) -> Value {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if let Ok(bytes) = std::fs::read(gate.join("ready.json")) {
            if let Ok(value) = serde_json::from_slice(&bytes) {
                return value;
            }
        }
        assert!(
            Instant::now() < deadline,
            "synthetic audio admission gate never reached"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}
fn audio_rpc(socket: &Path, method: &str, params: Value) -> Result<Value, String> {
    use std::io::{BufRead, BufReader, Write};
    let mut stream = std::os::unix::net::UnixStream::connect(socket).map_err(|e| e.to_string())?;
    stream
        .set_read_timeout(Some(Duration::from_secs(15)))
        .unwrap();
    writeln!(
        stream,
        "{}",
        json!({"id":1,"method":method,"params":params})
    )
    .map_err(|e| e.to_string())?;
    let mut line = String::new();
    BufReader::new(stream)
        .read_line(&mut line)
        .map_err(|e| e.to_string())?;
    let result: Value = serde_json::from_str(&line).map_err(|e| e.to_string())?;
    if result.get("error").is_some() {
        Err(result["error"]["message"].to_string())
    } else {
        Ok(result["result"].clone())
    }
}

#[test]
fn an_older_enable_cannot_publish_runtime_after_a_later_disable() {
    let root = tmp();
    let folder = root.path().join("ordering");
    pack(&folder, "fixture-ordering");
    let gate = root.path().join("transition-gate");
    std::fs::create_dir(&gate).unwrap();
    let release = AudioGate(gate.clone());
    let log = root.path().join("audio.log");
    let trace = root.path().join("transitions.log");
    let d = Daemon::start(&[
        ("OVERSEER_TEST_AUDIO_LOG", log.to_str().unwrap()),
        (
            "OVERSEER_TEST_AUDIO_TRANSITION_HOLD",
            gate.to_str().unwrap(),
        ),
        (
            "OVERSEER_TEST_AUDIO_TRANSITION_LOG",
            trace.to_str().unwrap(),
        ),
    ]);
    let initial = select(&d, &folder);
    let socket = d.socket();
    let enable_socket = socket.clone();
    let initial_revision = initial["revision"].clone();
    let enable = std::thread::spawn(move || {
        audio_rpc(
            &enable_socket,
            "audio.set",
            json!({"enabled":true,"expected_revision":initial_revision}),
        )
    });
    let committed = gate_ready(&gate);
    assert_eq!(committed["enabled"], true);
    assert_eq!(
        d.call("audio.get", json!({}))["revision"],
        committed["revision"],
        "older enable really committed before the hold"
    );
    let (tx, rx) = std::sync::mpsc::channel();
    let disable_socket = socket.clone();
    let expected = committed["revision"].clone();
    let disable = std::thread::spawn(move || {
        let r = audio_rpc(
            &disable_socket,
            "audio.set",
            json!({"enabled":false,"expected_revision":expected}),
        );
        let _ = tx.send(r.clone());
        r
    });
    // On the old source the disable completes during this hold, then the released
    // older enable wrongly writes the runtime true. On the guarded source it waits
    // only for the short commit/runtime boundary. Completion itself is not the assertion.
    let _ = rx.recv_timeout(Duration::from_secs(2));
    drop(release);
    enable.join().unwrap().unwrap();
    disable.join().unwrap().unwrap();
    assert_eq!(d.call("audio.get", json!({}))["enabled"], false);
    let values: Vec<Value> = std::fs::read_to_string(&trace)
        .unwrap()
        .lines()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect();
    assert_eq!(
        values.len(),
        2,
        "both actual runtime updates must be observed"
    );
    assert_eq!(
        values.last().unwrap()["enabled"],
        false,
        "actual runtime must agree with the last committed disable"
    );
    assert_eq!(
        values.last().unwrap()["revision"],
        d.call("audio.get", json!({}))["revision"]
    );
}

#[test]
fn source_change_after_decode_preserves_live_preview_in_new_pack_only() {
    let root = tmp();
    let a = root.path().join("pack-a");
    let b = root.path().join("pack-b");
    pack(&a, "fixture-a");
    pack(&b, "fixture-b");
    let gate = root.path().join("decoded-gate");
    std::fs::create_dir(&gate).unwrap();
    let release = AudioGate(gate.clone());
    let log = root.path().join("audio.log");
    let d = Daemon::start(&[
        ("OVERSEER_TEST_AUDIO_LOG", log.to_str().unwrap()),
        ("OVERSEER_TEST_AUDIO_DECODED_HOLD", gate.to_str().unwrap()),
    ]);
    let selected = select(&d, &a);
    assert_eq!(
        d.call("audio.preview", json!({"key":"agent_started"}))["queued"],
        true
    );
    let decoded = gate_ready(&gate);
    assert_eq!(decoded["revision"], selected["revision"]);
    assert!(!log.exists(), "A decoded but has not been admitted");
    let switched = select(&d, &b);
    assert!(switched["revision"].as_i64() > decoded["revision"].as_i64());
    drop(release);
    assert_eq!(
        wait_log(&log, 1),
        "fixture-b:agent_started\n",
        "the original live cue resolves B; A is never admitted"
    );
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(
        std::fs::read_to_string(&log).unwrap(),
        "fixture-b:agent_started\n",
        "there is exactly one receipt, without a second preview request"
    );
    assert_eq!(d.call("audio.get", json!({}))["enabled"], false);
}

#[test]
fn disable_after_source_switch_cancels_unstarted_preview_and_next_explicit_preview_works() {
    let root = tmp();
    let a = root.path().join("cancel-a");
    let b = root.path().join("cancel-b");
    pack(&a, "fixture-cancel-a");
    pack(&b, "fixture-cancel-b");
    let log = root.path().join("audio.log");
    let gate = root.path().join("cancel-gate");
    std::fs::create_dir(&gate).unwrap();
    let release = AudioGate(gate.clone());
    let d = Daemon::start(&[
        ("OVERSEER_TEST_AUDIO_LOG", log.to_str().unwrap()),
        ("OVERSEER_TEST_AUDIO_DECODED_HOLD", gate.to_str().unwrap()),
    ]);
    let selected = select(&d, &a);
    let on = d.call(
        "audio.set",
        json!({"enabled":true,"expected_revision":selected["revision"]}),
    );
    assert_eq!(on["enabled"], true);
    d.call("audio.preview", json!({"key":"agent_started"}));
    gate_ready(&gate);
    assert!(!log.exists());
    let switched = select(&d, &b);
    let off = d.call(
        "audio.set",
        json!({"enabled":false,"expected_revision":switched["revision"]}),
    );
    assert_eq!(off["enabled"], false);
    drop(release);
    // A second explicit preview is an ordered positive progress marker behind
    // the cancelled one; no elapsed-time-only absence assertion is used.
    d.call("audio.preview", json!({"key":"agent_complete"}));
    assert_eq!(
        wait_log(&log, 1),
        "fixture-cancel-b:agent_complete\n",
        "cancelled A must not be re-resolved/admitted in B"
    );
    assert_eq!(d.call("audio.get", json!({}))["enabled"], false);
}

#[test]
fn resolved_permission_after_source_switch_never_reappears_in_new_pack() {
    let root = tmp();
    let a = root.path().join("need-a");
    let b = root.path().join("need-b");
    pack(&a, "fixture-need-a");
    pack(&b, "fixture-need-b");
    let log = root.path().join("audio.log");
    let checkout = repo(&root.path().join("repo"));
    let fixture = repo_root().join("fixtures/fake-harness/claude-fixture.js");
    let gate = root.path().join("need-gate");
    std::fs::create_dir(&gate).unwrap();
    let release = AudioGate(gate.clone());
    let d = Daemon::start(&[
        ("OVERSEER_TEST_AUDIO_LOG", log.to_str().unwrap()),
        ("OVERSEER_TEST_AUDIO_DECODED_HOLD", gate.to_str().unwrap()),
        (
            "OVERSEER_TEST_AUDIO_DECODED_KEY",
            "agent_permission_required",
        ),
        ("OVERSEER_TEST_AUTO_DISABLED", "1"),
        ("OVERSEER_CLAUDE_PATH", fixture.to_str().unwrap()),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE"),
        ("FIXTURE_MODE", "permission"),
    ]);
    let selected = select(&d, &a);
    d.call(
        "audio.set",
        json!({"enabled":true,"expected_revision":selected["revision"]}),
    );
    let run=run_id(&d.call("task.create",json!({"repo":checkout,"harness":"claude","prompt":"write perm.txt","title":"synthetic current need"})));
    assert_eq!(
        d.wait_status(&run, |s| s == "waiting_for_user", 15)["attention"]["request_id"],
        "req-1"
    );
    let decoded = gate_ready(&gate);
    assert_eq!(decoded["key"], "agent_permission_required");
    assert!(!std::fs::read_to_string(&log)
        .unwrap_or_default()
        .contains("agent_permission_required"));
    select(&d, &b);
    d.call(
        "run.permission",
        json!({"run_id":run,"request_id":"req-1","allow":true}),
    );
    assert_eq!(d.wait_done(&run, 15)["status"], "completed");
    drop(release);
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let captures = std::fs::read_to_string(&log).unwrap_or_default();
        assert!(
            !captures.contains("agent_permission_required"),
            "resolved need may not be admitted from either pack: {captures}"
        );
        if captures
            .lines()
            .any(|s| s == "fixture-need-b:agent_complete")
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "current completion must progress after stale need drops: {captures}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

struct HeldDecoder(i64);
impl Drop for HeldDecoder {
    fn drop(&mut self) {
        // Only the exact child PID published by this fixture's worker gate; no
        // global process matching. The barrier is not released before this guard.
        if pid_alive(self.0) {
            signal(self.0, 9);
        }
    }
}

#[cfg(target_os = "macos")]
#[test]
fn successful_decoder_exit_between_wait_and_memory_check_keeps_valid_selection() {
    let root = tmp();
    let folder = root.path().join("valid-exit-pack");
    pack(&folder, "fixture-valid-exit");
    let gate = root.path().join("exit-gate");
    std::fs::create_dir(&gate).unwrap();
    std::fs::write(gate.join("armed"), b"armed").unwrap();
    let release = AudioGate(gate.clone());
    let log = root.path().join("audio.log");
    let d = Daemon::start(&[
        ("OVERSEER_TEST_AUDIO_WORKER_HOLD", gate.to_str().unwrap()),
        ("OVERSEER_TEST_AUDIO_EXIT_BEFORE_MEMORY", gate.to_str().unwrap()),
        ("OVERSEER_TEST_AUDIO_LOG", log.to_str().unwrap()),
    ]);
    let before = d.call("audio.get", json!({}));
    let result = d.try_call("audio.source.set", json!({"source":"folder","path":folder,
        "expected_revision":before["revision"]}));
    let observed: Value = serde_json::from_slice(&std::fs::read(gate.join("exit-observed.json"))
        .expect("SETUP: actual owned decoder must exit at the injected boundary")).unwrap();
    let worker: Value = serde_json::from_slice(&std::fs::read(gate.join("ready.json"))
        .expect("SETUP: actual worker must reach its synthetic hold")).unwrap();
    assert_eq!(observed["pid"], worker["pid"], "SETUP: only the owned worker may be observed");
    assert_eq!(observed["wait_code"], libc::CLD_EXITED, "SETUP: normal worker exit, not timeout/kill");
    assert_eq!(observed["exit_status"], 0, "SETUP: valid media worker must exit successfully");
    assert_eq!(observed["observed_without_reap"], true);
    assert_eq!(observed["kill_zero"], 0, "SETUP: actual zombie must still answer kill-zero");
    drop(release);
    let selected = result.expect("valid decoder output/exit must survive the exit-before-RSS race");
    assert_eq!(selected["source"]["kind"], "folder");
    assert_eq!(selected["source"]["available"], true);
    assert_eq!(selected["enabled"], false);
    assert_eq!(selected["revision"].as_i64(), Some(before["revision"].as_i64().unwrap() + 1));
}

#[cfg(target_os = "macos")]
fn decoder_exit_refusal_control(malformed_stdout: bool) {
    let root = tmp();
    let folder = root.path().join("refusal-pack");
    pack(&folder, "fixture-exit-refusal");
    if !malformed_stdout {
        // Actual native decoder failure, not a synthetic exit status override.
        for (key, _) in LINES {
            std::fs::write(folder.join(format!("audio/{key}.wav")), b"not-WAV").unwrap();
        }
    }
    let gate = root.path().join("exit-gate");
    std::fs::create_dir(&gate).unwrap();
    std::fs::write(gate.join("armed"), b"armed").unwrap();
    let release = AudioGate(gate.clone());
    let log = root.path().join("audio.log");
    let mut env = vec![
        ("OVERSEER_TEST_AUDIO_WORKER_HOLD", gate.to_str().unwrap()),
        ("OVERSEER_TEST_AUDIO_EXIT_BEFORE_MEMORY", gate.to_str().unwrap()),
        ("OVERSEER_TEST_AUDIO_LOG", log.to_str().unwrap()),
    ];
    if malformed_stdout { env.push(("OVERSEER_TEST_AUDIO_WORKER_STDOUT", "malformed")); }
    let d = Daemon::start(&env);
    let before = d.call("audio.get", json!({}));
    let result = d.try_call("audio.source.set", json!({"source":"folder","path":folder,
        "expected_revision":before["revision"]}));
    let observed: Value = serde_json::from_slice(&std::fs::read(gate.join("exit-observed.json"))
        .expect("SETUP: exit boundary must be reached before refusal")).unwrap();
    let worker: Value = serde_json::from_slice(&std::fs::read(gate.join("ready.json")).unwrap()).unwrap();
    assert_eq!(observed["pid"], worker["pid"]);
    assert_eq!(observed["wait_code"], libc::CLD_EXITED);
    assert_eq!(observed["exit_status"], if malformed_stdout { 0 } else { 1 });
    assert_eq!(observed["observed_without_reap"], true);
    assert_eq!(observed["kill_zero"], 0);
    drop(release);
    let error = result.expect_err("failed worker/malformed stdout may never commit a source");
    // Before the race correction this may fail on the earlier memory error;
    // retain that distinction rather than claiming the later guard was reached.
    assert!(error.contains("malformed"), "must reach exit/output refusal, not setup/earlier memory refusal: {error}");
    assert_eq!(d.call("audio.get", json!({})), before);
    assert!(!log.exists(), "source validation produces no playback receipt");
}

#[cfg(target_os = "macos")]
#[test]
fn exited_decoder_with_malformed_stdout_still_refuses_unchanged_source() {
    decoder_exit_refusal_control(true);
}

#[cfg(target_os = "macos")]
#[test]
fn genuinely_failed_decoder_exit_still_refuses_unchanged_source() {
    decoder_exit_refusal_control(false);
}

#[cfg(target_os = "macos")]
#[test]
fn live_held_decoder_with_injected_unreadable_memory_still_refuses() {
    let root = tmp();
    let folder = root.path().join("live-memory-pack");
    pack(&folder, "fixture-live-memory");
    let gate = root.path().join("worker-gate");
    std::fs::create_dir(&gate).unwrap();
    std::fs::write(gate.join("armed"), b"armed").unwrap();
    let release = AudioGate(gate.clone());
    let log = root.path().join("audio.log");
    let d = Daemon::start(&[
        ("OVERSEER_TEST_AUDIO_WORKER_HOLD", gate.to_str().unwrap()),
        ("OVERSEER_TEST_AUDIO_MEMORY_UNAVAILABLE", gate.to_str().unwrap()),
        ("OVERSEER_TEST_AUDIO_LOG", log.to_str().unwrap()),
    ]);
    let before = d.call("audio.get", json!({}));
    let result = d.try_call("audio.source.set", json!({"source":"folder","path":folder,
        "expected_revision":before["revision"]}));
    let observed: Value = serde_json::from_slice(&std::fs::read(gate.join("memory-fault-observed.json"))
        .expect("SETUP: held-live memory fault must be injected at the real boundary")).unwrap();
    let worker: Value = serde_json::from_slice(&std::fs::read(gate.join("ready.json")).unwrap()).unwrap();
    assert_eq!(observed["pid"], worker["pid"]);
    assert_eq!(observed["held_live"], true);
    assert_eq!(observed["injected"], true);
    assert_eq!(observed["kill_zero"], 0);
    assert!(!gate.join("release").exists(), "worker was never released before memory refusal");
    assert!(!pid_alive(worker["pid"].as_i64().unwrap()), "refused owned worker must be reaped by the completed RPC");
    drop(release);
    let error = result.expect_err("live unreadable worker must remain refused");
    assert!(error.contains("Cannot supervise audio validation memory"), "wrong refusal boundary: {error}");
    assert_eq!(d.call("audio.get", json!({})), before);
    assert!(!log.exists());
}

#[test]
fn daemon_shutdown_reaps_a_decoder_held_during_source_validation_before_exit() {
    let root = tmp();
    let folder = root.path().join("shutdown-validation");
    pack(&folder, "fixture-shutdown-validation");
    let gate = root.path().join("worker-gate");
    std::fs::create_dir(&gate).unwrap();
    std::fs::write(gate.join("armed"), b"armed").unwrap();
    let release = AudioGate(gate.clone());
    let mut d = Daemon::start(&[("OVERSEER_TEST_AUDIO_WORKER_HOLD", gate.to_str().unwrap())]);
    let before = d.call("audio.get", json!({}));
    let socket = d.socket();
    let expected = before["revision"].clone();
    let selection = std::thread::spawn(move || {
        audio_rpc(
            &socket,
            "audio.source.set",
            json!({"source":"folder","path":folder,"expected_revision":expected}),
        )
    });
    let held = gate_ready(&gate);
    let pid = held["pid"].as_i64().unwrap();
    let worker = HeldDecoder(pid);
    assert!(
        pid_alive(pid),
        "actual owned worker must still be held before shutdown"
    );
    let start = Instant::now();
    d.shutdown();
    assert!(
        start.elapsed() < Duration::from_secs(2),
        "shutdown must not merely wait for the validation timeout"
    );
    let selection_result = selection.join().unwrap();
    assert!(
        selection_result.is_err(),
        "interrupted validation must not commit a folder"
    );
    let alive = pid_alive(pid);
    drop(worker);
    drop(release);
    assert!(
        !alive,
        "decoder must be cancelled/reaped before daemon exit, not orphaned until its timeout"
    );
}

#[test]
fn a_closing_audio_runtime_refuses_new_source_admission_before_daemon_exit() {
    let root = tmp();
    let folder = root.path().join("closing-source");
    pack(&folder, "fixture-closing-source");
    let gate = root.path().join("shutdown-gate");
    std::fs::create_dir(&gate).unwrap();
    let release = AudioGate(gate.clone());
    let mut d = Daemon::start(&[("OVERSEER_TEST_AUDIO_SHUTDOWN_HOLD", gate.to_str().unwrap())]);
    // Prove the native validator setup first; a codec error must not count as
    // closing refusal. Both post-close calls require the explicit closing reason.
    let before = select(&d, &folder);
    d.call("daemon.shutdown", json!({}));
    let held = gate_ready(&gate);
    assert_eq!(
        held["pid"].as_u64(),
        Some(d.child.as_ref().unwrap().id() as u64)
    );
    let result = d.try_call(
        "audio.source.set",
        json!({"source":"folder","path":folder,"expected_revision":before["revision"]}),
    );
    let builtin_result = d.try_call(
        "audio.source.set",
        json!({"source":"builtin","expected_revision":revision(&d)}),
    );
    let during = d.call("audio.get", json!({}));
    drop(release);
    if let Some(mut child) = d.child.take() {
        child.wait().unwrap();
    }
    for result in [result, builtin_result] {
        let error =
            result.expect_err("closing must refuse both decoder and decoder-free source admission");
        assert!(error.to_ascii_lowercase().contains("shutting down") || error.to_ascii_lowercase().contains("closing"),
            "only a closing refusal proves this boundary, not codec/revision/setup failure: {error}");
    }
    assert_eq!(
        during["revision"], before["revision"],
        "closing request cannot commit new selection"
    );
    assert_eq!(during["source"]["kind"], before["source"]["kind"]);
}

#[test]
fn disabling_audio_cancels_and_reaps_source_validation_without_changing_selected_folder() {
    let root = tmp();
    let a = root.path().join("validation-a");
    let b = root.path().join("validation-b");
    let mut ma = pack(&a, "fixture-validation-a");
    ma["label"] = json!("Validation A");
    save_manifest(&a, &ma);
    let mut mb = pack(&b, "fixture-validation-b");
    mb["label"] = json!("Validation B");
    save_manifest(&b, &mb);
    let gate = root.path().join("worker-gate");
    std::fs::create_dir(&gate).unwrap();
    let release = AudioGate(gate.clone());
    let log = root.path().join("audio.log");
    let d = Daemon::start(&[
        ("OVERSEER_TEST_AUDIO_LOG", log.to_str().unwrap()),
        ("OVERSEER_TEST_AUDIO_WORKER_HOLD", gate.to_str().unwrap()),
    ]);
    let selected = select(&d, &a);
    let on = d.call(
        "audio.set",
        json!({"enabled":true,"expected_revision":selected["revision"]}),
    );
    std::fs::write(gate.join("armed"), b"armed").unwrap();
    let socket = d.socket();
    let expected = on["revision"].clone();
    let (tx, rx) = std::sync::mpsc::channel();
    let selection = std::thread::spawn(move || {
        let r = audio_rpc(
            &socket,
            "audio.source.set",
            json!({"source":"folder","path":b,"expected_revision":expected}),
        );
        let _ = tx.send(r.clone());
        r
    });
    let held = gate_ready(&gate);
    let pid = held["pid"].as_i64().unwrap();
    let worker = HeldDecoder(pid);
    assert!(pid_alive(pid));
    let start = Instant::now();
    let off = d.call(
        "audio.set",
        json!({"enabled":false,"expected_revision":on["revision"]}),
    );
    assert_eq!(off["enabled"], false);
    let cancelled = rx.recv_timeout(Duration::from_millis(500));
    let alive = pid_alive(pid);
    drop(worker);
    drop(release);
    let final_result = selection.join().unwrap();
    assert!(
        start.elapsed() < Duration::from_secs(2),
        "cancellation proof must precede original validation timeout"
    );
    assert!(
        matches!(cancelled, Ok(Err(_))),
        "source validation must finish cancelled promptly: {cancelled:?}"
    );
    assert!(final_result.is_err());
    assert!(!alive, "cancelled worker must be reaped, not left active");
    let after = d.call("audio.get", json!({}));
    assert_eq!(after["source"]["label"], "Validation A");
    assert_eq!(after["revision"], off["revision"]);
    assert_eq!(after["enabled"], false);
}
