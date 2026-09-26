mod common;
use common::*;
use serde_json::json;

#[test]
fn audio_mode_is_off_until_enabled_and_survives_restart() {
    let mut d = Daemon::start(&[]);
    let initial = d.call("audio.get", json!({}));
    assert_eq!(initial["enabled"], false);
    assert_eq!(initial["pack"], "reactor");
    assert_eq!(
        initial["default_keys"],
        json!(["agent_started", "agent_complete", "agent_needs_attention"])
    );
    assert_eq!(initial["keys"].as_array().unwrap().len(), 12);
    assert!(d.try_call("audio.set", json!({"enabled": "yes"})).is_err());
    assert!(d
        .try_call("audio.preview", json!({"key": "made_up"}))
        .is_err());
    if initial["available"] == true {
        assert_eq!(
            d.call("audio.set", json!({"enabled": true}))["enabled"],
            true
        );
        d.kill9();
        d.spawn();
        assert_eq!(d.call("audio.get", json!({}))["enabled"], true);
        assert_eq!(
            d.call("audio.set", json!({"enabled": false}))["enabled"],
            false
        );
    } else {
        assert!(d.try_call("audio.set", json!({"enabled": true})).is_err());
    }
}

#[test]
fn disabled_audio_never_materializes_a_player_asset() {
    let root = tmp();
    let repo = repo(&root.path().join("repo"));
    let d = Daemon::start(&[]);
    let created = d.generic(&repo, "worktree", "/bin/sh", &["-c", "exit 0"]);
    let run = run_id(&created);
    assert_eq!(d.wait_done(&run, 10)["status"], "completed");
    assert!(!d.home.path().join("audio").exists());
}

#[test]
fn live_root_events_play_once_without_or_with_multiple_ui_clients() {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixStream;
    use std::time::{Duration, Instant};

    let root = tmp();
    let repo = repo(&root.path().join("repo"));
    let audio_log = root.path().join("audio.log");
    let audio_log_str = audio_log.to_str().unwrap();
    let d = Daemon::start(&[("OVERSEER_TEST_AUDIO_LOG", audio_log_str)]);
    assert_eq!(
        d.call("audio.set", json!({"enabled": true}))["enabled"],
        true
    );

    // No UI is connected for this first run.
    let finished = run_id(&d.generic(&repo, "worktree", "/bin/sh", &["-c", "exit 0"]));
    assert_eq!(d.wait_done(&finished, 10)["status"], "completed");
    wait_audio_lines(&audio_log, 2);
    let lines = std::fs::read_to_string(&audio_log).unwrap();
    assert_eq!(
        lines
            .lines()
            .filter(|line| *line == "reactor:agent_started")
            .count(),
        1
    );
    assert_eq!(
        lines
            .lines()
            .filter(|line| *line == "reactor:agent_complete")
            .count(),
        1
    );

    // Two UI clients observe the same daemon; neither owns playback.
    let mut clients = Vec::new();
    for _ in 0..2 {
        let mut stream = UnixStream::connect(d.socket()).unwrap();
        stream
            .write_all(b"{\"id\":1,\"method\":\"hello\",\"params\":{\"client\":\"vscode\"}}\n")
            .unwrap();
        let mut reply = String::new();
        BufReader::new(stream.try_clone().unwrap())
            .read_line(&mut reply)
            .unwrap();
        assert!(reply.contains("result"));
        clients.push(stream);
    }
    let failed = run_id(&d.generic(&repo, "worktree", "/bin/sh", &["-c", "exit 3"]));
    assert_eq!(d.wait_done(&failed, 10)["status"], "failed");
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        let lines = std::fs::read_to_string(&audio_log).unwrap_or_default();
        if lines
            .lines()
            .any(|line| line == "reactor:agent_needs_attention")
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let lines = std::fs::read_to_string(&audio_log).unwrap();
    assert_eq!(
        lines
            .lines()
            .filter(|line| *line == "reactor:agent_needs_attention")
            .count(),
        1
    );
    drop(clients);
}

fn wait_audio_lines(path: &std::path::Path, n: usize) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while std::time::Instant::now() < deadline {
        if std::fs::read_to_string(path)
            .unwrap_or_default()
            .lines()
            .count()
            >= n
        {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    panic!("timed out waiting for {n} audio cues");
}

#[test]
fn live_permission_and_waiting_status_share_one_attention_cue() {
    let root = tmp();
    let repo = repo(&root.path().join("repo"));
    let audio_log = root.path().join("audio.log");
    let audio_log_str = audio_log.to_str().unwrap();
    let fixture = repo_root()
        .join("fixtures/fake-harness/claude-fixture.js")
        .display()
        .to_string();
    let d = Daemon::start(&[
        ("OVERSEER_TEST_AUDIO_LOG", audio_log_str),
        ("OVERSEER_CLAUDE_PATH", &fixture),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE"),
        ("FIXTURE_MODE", "permission"),
    ]);
    d.call("audio.set", json!({"enabled": true}));
    let created = d.call("task.create", json!({"repo": repo, "harness": "claude", "prompt": "write perm.txt", "title": "permission cue"}));
    let run = run_id(&created);
    let waiting = d.wait_status(&run, |s| s == "waiting_for_user", 15);
    wait_audio_lines(&audio_log, 2);
    std::thread::sleep(std::time::Duration::from_millis(200));
    let lines = std::fs::read_to_string(&audio_log).unwrap();
    assert_eq!(
        lines
            .lines()
            .filter(|line| *line == "reactor:agent_needs_attention")
            .count(),
        1
    );
    let request_id = waiting["attention"]["request_id"].as_str().unwrap();
    d.call(
        "run.permission",
        json!({"run_id": run, "request_id": request_id, "allow": true}),
    );
    assert_eq!(d.wait_done(&run, 15)["status"], "completed");
    wait_audio_lines(&audio_log, 3);
    let lines = std::fs::read_to_string(&audio_log).unwrap();
    assert_eq!(
        lines
            .lines()
            .filter(|line| *line == "reactor:agent_needs_attention")
            .count(),
        1
    );
    assert_eq!(
        lines
            .lines()
            .filter(|line| *line == "reactor:agent_complete")
            .count(),
        1
    );
}

#[test]
fn system_and_private_commander_tracks_are_selectable_without_bundling_voice_files() {
    let root = tmp();
    let audio_log = root.path().join("audio.log");
    let audio_log_str = audio_log.to_str().unwrap();
    let mut d = Daemon::start(&[("OVERSEER_TEST_AUDIO_LOG", audio_log_str)]);
    assert_eq!(d.call("audio.get", json!({}))["track"], "reactor");
    assert!(d
        .try_call("audio.set", json!({"track": "commander"}))
        .is_err());

    let private_pack = root.path().join("private-commander");
    for key in ["agent_started", "agent_complete", "agent_needs_attention"] {
        let file = private_pack.join(key).join("transmission/commander.wav");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, b"RIFF\x00\x00\x00\x00WAVEfmt ").unwrap();
    }
    assert_eq!(
        d.call("audio.import_commander", json!({"path": private_pack}))["imported"],
        true
    );
    assert_eq!(
        d.call("audio.set", json!({"track": "commander"}))["track"],
        "commander"
    );
    assert_eq!(
        d.call("audio.preview", json!({"key": "agent_started"}))["queued"],
        true
    );
    wait_audio_lines(&audio_log, 1);
    assert!(std::fs::read_to_string(&audio_log)
        .unwrap()
        .contains("commander:agent_started"));

    assert_eq!(
        d.call("audio.set", json!({"track": "system"}))["track"],
        "system"
    );
    assert_eq!(
        d.call("audio.preview", json!({"key": "agent_needs_attention"}))["queued"],
        true
    );
    wait_audio_lines(&audio_log, 2);
    assert!(std::fs::read_to_string(&audio_log)
        .unwrap()
        .contains("system:agent_needs_attention"));

    d.kill9();
    d.spawn();
    assert_eq!(d.call("audio.get", json!({}))["track"], "system");
    assert_eq!(d.call("audio.get", json!({}))["commander_imported"], true);
    assert!(
        !d.home.path().join("audio").exists(),
        "private voice files are played in place"
    );
}

#[test]
fn missing_local_cache_does_not_interrupt_agents() {
    if !cfg!(target_os = "macos") {
        return;
    }
    let root = tmp();
    let repo = repo(&root.path().join("repo"));
    let d = Daemon::start(&[]);
    std::fs::write(d.home.path().join("audio"), b"cache directory blocked").unwrap();
    d.call("audio.set", json!({"enabled": true}));
    let run = run_id(&d.generic(&repo, "worktree", "/bin/sh", &["-c", "exit 0"]));
    assert_eq!(d.wait_done(&run, 10)["status"], "completed");
    let log = d.home.path().join("overseerd.log");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while std::time::Instant::now() < deadline {
        if std::fs::read_to_string(&log)
            .unwrap_or_default()
            .contains("audio playback failed")
        {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    panic!("audio failure was not logged");
}

#[test]
fn authentication_failure_makes_one_attention_cue() {
    let root = tmp();
    let repo = repo(&root.path().join("repo"));
    let audio_log = root.path().join("audio.log");
    let audio_log_str = audio_log.to_str().unwrap();
    let fixture = repo_root()
        .join("fixtures/fake-harness/claude-fixture.js")
        .display()
        .to_string();
    let d = Daemon::start(&[
        ("OVERSEER_TEST_AUDIO_LOG", audio_log_str),
        ("OVERSEER_CLAUDE_PATH", &fixture),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE"),
        ("FIXTURE_MODE", "auth"),
    ]);
    d.call("audio.set", json!({"enabled": true}));
    let created = d.call(
        "task.create",
        json!({"repo": repo, "harness": "claude", "prompt": "try", "title": "auth cue"}),
    );
    let run = run_id(&created);
    assert_eq!(d.wait_done(&run, 15)["status"], "failed");
    wait_audio_lines(&audio_log, 2);
    let lines = std::fs::read_to_string(&audio_log).unwrap();
    assert_eq!(
        lines
            .lines()
            .filter(|line| *line == "reactor:agent_needs_attention")
            .count(),
        1
    );
    assert_eq!(
        lines
            .lines()
            .filter(|line| *line == "reactor:agent_complete")
            .count(),
        0
    );
}

#[test]
fn installed_system_voice_can_be_selected() {
    if !cfg!(target_os = "macos") {
        return;
    }
    let d = Daemon::start(&[]);
    let voices = d.call("audio.voices", json!({}));
    let first = voices
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["name"] == "Daniel")
        .unwrap();
    assert_eq!(first["locale"], "en_GB");
    assert!(d
        .try_call(
            "audio.set",
            json!({"track": "system", "voice": "no-such-voice-12345"})
        )
        .is_err());
    assert_eq!(
        d.call("audio.set", json!({"track": "system", "voice": "Daniel"}))["voice"],
        "Daniel"
    );
}
