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
