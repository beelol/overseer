//! AC-265: TUI rendering and owner keys against a real isolated daemon, synthetic harness only.
mod support;
use support::*;
use crossterm::event::KeyCode;
use serde_json::json;
use std::path::Path;

#[test]
fn ac265_tui_shows_ordered_paused_queue_and_owner_keys_remove_clear_resume() {
    let t = tempfile::tempdir().unwrap(); let repo = repo(&t.path().join("repo"));
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().join("fixtures/fake-harness/claude-fixture.js");
    let mode = t.path().join("mode"); std::fs::write(&mode, "slow").unwrap();
    let d = Daemon::start(&[("OVERSEER_CLAUDE_PATH", fixture.to_str().unwrap()), ("OVERSEER_CODEX_PATH", "/nonexistent/codex"), ("OVERSEER_OPENCODE_PATH", "/nonexistent/opencode"),
        ("CLAUDE_FIXTURE_MODE_FILE", mode.to_str().unwrap()), ("FIXTURE_SLOW_MS", "18000"), ("FIXTURE_INTERRUPT_DELAY_MS", "1500"), ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "CLAUDE_FIXTURE_MODE_FILE,FIXTURE_SLOW_MS,FIXTURE_INTERRUPT_DELAY_MS"), ("OVERSEER_CONTINUITY_PROBES", "off"), ("OVERSEER_PUBLIC_STATUS", "off")]);
    let run = d.ctl("task.create", json!({"repo":repo,"harness":"claude","title":"Queue demo","prompt":"original work"}))["run"]["id"].as_str().unwrap().to_string();
    assert_eq!(d.wait_status(&run, |s| s == "running", 20), "running");
    d.ctl("run.redirect", json!({"run_id":run,"text":"typed direction"}));
    d.ctl("run.queue", json!({"run_id":run,"text":"spoken direction","source":"overseer"}));
    assert_eq!(d.ctl("run.queued", json!({"run_id":run}))["queued"][0]["redirect"], true);
    assert_eq!(d.ctl("state", json!({}))["runs"].as_array().unwrap().iter().find(|r| r["id"] == run).unwrap()["status"], "running");
    let mut ui = Tui::attach(&d, 140, 40); ui.app.focus = Some(run.clone());
    ui.key(KeyCode::Char('x')); ui.key(KeyCode::Char('y'));
    let s = ui.until_screen(20, "Queue paused");
    assert!(s.contains("1. Paused typed direction") && s.contains("2. Paused spoken direction"), "both messages on the grid tile: {s}");
    assert!(s.find("typed direction").unwrap() < s.find("spoken direction").unwrap());
    ui.snapshot("queue-pause/grid-paused");
    ui.key(KeyCode::Char('Q'));
    let s = ui.until_screen(10, "s Send queued"); assert!(s.contains("c Clear") && s.contains("d Remove"), "{s}");
    ui.snapshot("queue-pause/owner-controls");
    ui.key(KeyCode::Char('j')); ui.key(KeyCode::Char('d'));
    ui.until(10, |_| d.ctl("run.queued", json!({"run_id":run}))["queued"].as_array().unwrap().len() == 1);
    assert_eq!(d.ctl("run.queued", json!({"run_id":run}))["queued"][0]["text"], "typed direction");
    ui.key(KeyCode::Char('c'));
    ui.until_screen(10, "No queued messages");
    assert_eq!(d.ctl("run.queued", json!({"run_id":run}))["paused"], true);
    assert_eq!(d.ctl("run.turns", json!({"run_id":run})).as_array().unwrap().len(), 1);
    // Another message joins the empty paused queue; no key or reconnect resumes it by itself.
    d.ctl("run.queue", json!({"run_id":run,"text":"send only on s"}));
    ui.until_screen(10, "send only on s");
    std::fs::write(&mode, "echo").unwrap();
    ui.key(KeyCode::Char('s'));
    ui.until(20, |_| d.ctl("run.turns", json!({"run_id":run})).as_array().unwrap().len() == 2);
    assert_eq!(d.ctl("run.turns", json!({"run_id":run}))[1]["prompt"], "send only on s");
    assert_eq!(d.wait_status(&run, |s| s == "completed", 20), "completed");
}
