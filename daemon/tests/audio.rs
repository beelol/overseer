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
fn child_completion_stays_silent_until_root_finishes() {
    let root = tmp();
    let repo = repo(&root.path().join("repo"));
    let audio_log = root.path().join("audio.log");
    let audio_log_str = audio_log.to_str().unwrap();
    let fixture = repo_root()
        .join("fixtures/fake-harness/codex-app-fixture.js")
        .display()
        .to_string();
    let d = Daemon::start(&[
        ("OVERSEER_TEST_AUDIO_LOG", audio_log_str),
        ("OVERSEER_CODEX_PATH", &fixture),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE"),
        ("FIXTURE_MODE", "tree"),
    ]);
    d.call("audio.set", json!({"enabled": true}));
    let created = d.call(
        "task.create",
        json!({
            "repo": repo,
            "harness": "codex-app",
            "prompt": "delegate",
            "title": "child cue",
            "approval_policy": "untrusted",
            "extra_args": ["-c", "agents.max_depth=2"]
        }),
    );
    let run = run_id(&created);
    let waiting = d.wait_status(&run, |s| s == "waiting_for_user", 15);
    let runs = d.runs();
    let child = runs
        .iter()
        .find(|r| r["native_id"] == "thr-child")
        .expect("child run");
    assert_eq!(child["parent_run_id"], run);
    assert!(d
        .events(child["id"].as_str().unwrap())
        .iter()
        .any(|e| e["kind"] == "status" && e["payload"]["status"] == "completed"));
    wait_audio_lines(&audio_log, 2);
    std::thread::sleep(std::time::Duration::from_millis(200));
    let before = std::fs::read_to_string(&audio_log).unwrap();
    assert!(
        !before.lines().any(|line| line == "reactor:agent_complete"),
        "a child finishing must not sound like the root finished: {before}"
    );

    d.call(
        "run.permission",
        json!({
            "run_id": run,
            "request_id": waiting["attention"]["request_id"],
            "allow": true
        }),
    );
    assert_eq!(d.wait_done(&run, 15)["status"], "completed");
    wait_audio_lines(&audio_log, 3);
    let lines = std::fs::read_to_string(&audio_log).unwrap();
    for key in ["agent_started", "agent_needs_attention", "agent_complete"] {
        assert_eq!(
            lines
                .lines()
                .filter(|line| *line == format!("reactor:{key}"))
                .count(),
            1,
            "{key} must play once for the root: {lines}"
        );
    }
    assert_eq!(
        lines.lines().count(),
        3,
        "children and tools must stay silent: {lines}"
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
fn a_folder_that_is_not_a_commander_pack_is_refused_with_the_reason() {
    let root = tmp();
    let d = Daemon::start(&[]);
    let import = |path: &std::path::Path| {
        d.try_call("audio.import_commander", json!({"path": path}))
            .unwrap_err()
    };
    let gone = import(&root.path().join("nowhere"));
    assert!(gone.contains("that folder does not exist"), "{gone}");

    let empty = root.path().join("empty");
    std::fs::create_dir_all(&empty).unwrap();
    let missing = import(&empty);
    assert!(
        missing.contains("not a Commander pack: agent_started/transmission/commander.wav is missing"),
        "{missing}"
    );

    let text = root.path().join("text");
    for key in ["agent_started", "agent_complete", "agent_needs_attention"] {
        let file = text.join(key).join("transmission/commander.wav");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, b"this is not audio, only text").unwrap();
    }
    let other = import(&text);
    assert!(other.contains("non-WAV"), "{other}");
    assert_eq!(d.call("audio.get", json!({}))["commander_imported"], false);
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
fn a_platform_without_players_reports_unavailable_and_stays_silent() {
    let root = tmp();
    let repo = repo(&root.path().join("repo"));
    let mut d = Daemon::start(&[]);
    if d.call("audio.get", json!({}))["available"] == true {
        // The setting is already on when the players go away.
        d.call("audio.set", json!({"enabled": true}));
    }
    d.kill9();
    d.env
        .push(("OVERSEER_TEST_AUDIO_UNAVAILABLE".into(), "1".into()));
    d.spawn();

    let audio = d.call("audio.get", json!({}));
    assert_eq!(audio["available"], false);
    assert_eq!(audio["keys"].as_array().unwrap().len(), 12);
    for (method, params) in [
        ("audio.set", json!({"enabled": true})),
        ("audio.preview", json!({"key": "agent_started"})),
        ("audio.voices", json!({})),
    ] {
        let error = d.try_call(method, params).unwrap_err();
        assert!(error.contains("unavailable"), "{method}: {error}");
    }
    let run = run_id(&d.generic(&repo, "worktree", "/bin/sh", &["-c", "exit 0"]));
    assert_eq!(d.wait_done(&run, 10)["status"], "completed");
    std::thread::sleep(std::time::Duration::from_millis(300));
    assert!(!d.home.path().join("audio").exists());
    assert_eq!(
        d.call("audio.set", json!({"enabled": false}))["enabled"],
        false
    );
}

fn cues(path: &std::path::Path) -> Vec<String> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect()
}

#[test]
fn a_lost_session_plays_one_attention_cue() {
    let root = tmp();
    let repo = repo(&root.path().join("repo"));
    let audio_log = root.path().join("audio.log");
    let d = Daemon::start(&[("OVERSEER_TEST_AUDIO_LOG", audio_log.to_str().unwrap())]);
    d.call("audio.set", json!({"enabled": true}));
    let run = run_id(&d.generic(&repo, "worktree", "/bin/sh", &["-c", "sleep 30"]));
    d.wait_status(&run, |s| s == "running", 10);
    wait_audio_lines(&audio_log, 1);
    let (shim, _) = launch_info(&d, &run);
    signal(shim["shim_pid"].as_i64().unwrap(), 9); // the supervisor is lost
    assert_eq!(d.wait_done(&run, 15)["status"], "disconnected");
    wait_audio_lines(&audio_log, 2);
    signal(shim["child_pid"].as_i64().unwrap(), 9);
    std::thread::sleep(std::time::Duration::from_millis(500));
    assert_eq!(d.run(&run)["status"], "disconnected");
    assert_eq!(
        cues(&audio_log),
        ["reactor:agent_started", "reactor:agent_needs_attention"]
    );
}

#[test]
fn an_agent_stopped_on_request_stays_silent() {
    let root = tmp();
    let repo = repo(&root.path().join("repo"));
    let audio_log = root.path().join("audio.log");
    let d = Daemon::start(&[("OVERSEER_TEST_AUDIO_LOG", audio_log.to_str().unwrap())]);
    d.call("audio.set", json!({"enabled": true}));
    let run = run_id(&d.generic(&repo, "worktree", "/bin/sh", &["-c", "sleep 30"]));
    d.wait_status(&run, |s| s == "running", 10);
    wait_audio_lines(&audio_log, 1);
    d.call("run.interrupt", json!({"run_id": run}));
    assert_eq!(d.wait_done(&run, 15)["status"], "interrupted");
    std::thread::sleep(std::time::Duration::from_millis(500));
    assert_eq!(cues(&audio_log), ["reactor:agent_started"]);
}

#[test]
fn a_session_lost_while_the_daemon_was_down_makes_no_sound() {
    let root = tmp();
    let repo = repo(&root.path().join("repo"));
    let audio_log = root.path().join("audio.log");
    let mut d = Daemon::start(&[("OVERSEER_TEST_AUDIO_LOG", audio_log.to_str().unwrap())]);
    d.call("audio.set", json!({"enabled": true}));
    let run = run_id(&d.generic(&repo, "worktree", "/bin/sh", &["-c", "sleep 30"]));
    d.wait_status(&run, |s| s == "running", 10);
    wait_audio_lines(&audio_log, 1);
    let (shim, _) = launch_info(&d, &run);
    d.kill9();
    signal(shim["child_pid"].as_i64().unwrap(), 9);
    signal(shim["shim_pid"].as_i64().unwrap(), 9);
    std::thread::sleep(std::time::Duration::from_millis(300));
    d.spawn();
    let lost = d.run(&run);
    assert_eq!(lost["status"], "disconnected", "{lost}");
    assert!(lost["exit_reason"].as_str().unwrap().contains("lost"), "{lost}");
    assert_eq!(d.call("audio.get", json!({}))["enabled"], true);
    std::thread::sleep(std::time::Duration::from_millis(800));
    assert_eq!(
        cues(&audio_log),
        ["reactor:agent_started"],
        "nothing is played for what happened while the daemon was down"
    );
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

#[test]
fn simultaneous_permissions_make_one_cue_and_two_visible_needs() {
    let root = tmp();
    let repo = repo(&root.path().join("repo"));
    let audio_log = root.path().join("audio.log");
    let audio_log_str = audio_log.to_str().unwrap();
    let barrier = root.path().join("release-permissions");
    let barrier_str = barrier.to_str().unwrap();
    let fixture = repo_root()
        .join("fixtures/fake-harness/claude-fixture.js")
        .display()
        .to_string();
    let d = Daemon::start(&[
        ("OVERSEER_TEST_AUDIO_LOG", audio_log_str),
        ("OVERSEER_CLAUDE_PATH", &fixture),
        (
            "OVERSEER_HARNESS_ENV_PASSTHROUGH",
            "FIXTURE_MODE,FIXTURE_PERMISSION_BARRIER",
        ),
        ("FIXTURE_MODE", "permission"),
        ("FIXTURE_PERMISSION_BARRIER", barrier_str),
    ]);
    d.call("audio.set", json!({"enabled": true}));
    let one = run_id(&d.call(
        "task.create",
        json!({"repo": repo, "harness": "claude", "prompt": "one", "title": "permission one"}),
    ));
    let two = run_id(&d.call(
        "task.create",
        json!({"repo": repo, "harness": "claude", "prompt": "two", "title": "permission two"}),
    ));
    d.wait_status(&one, |s| s == "running", 5);
    d.wait_status(&two, |s| s == "running", 5);
    assert_eq!(d.run(&one)["attention"], serde_json::Value::Null);
    assert_eq!(d.run(&two)["attention"], serde_json::Value::Null);
    std::fs::write(barrier, b"go").unwrap();
    d.wait_status(&one, |s| s == "waiting_for_user", 15);
    d.wait_status(&two, |s| s == "waiting_for_user", 15);
    std::thread::sleep(std::time::Duration::from_millis(300));
    let needs = d
        .runs()
        .iter()
        .filter(|r| r["parent_run_id"].is_null() && r["status"] == "waiting_for_user")
        .count();
    assert_eq!(needs, 2, "both needs remain visible to the UI");
    let lines = std::fs::read_to_string(&audio_log).unwrap_or_default();
    assert_eq!(
        lines
            .lines()
            .filter(|line| *line == "reactor:agent_needs_attention")
            .count(),
        1,
        "{lines}"
    );
    for run in [&one, &two] {
        d.call("run.interrupt", json!({"run_id": run}));
    }
}
