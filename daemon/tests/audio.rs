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
