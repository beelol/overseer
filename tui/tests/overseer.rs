//! Gate S (AC-199) from the terminal: the tiles show held, watched and in conflict; `o` opens the
//! conversation with Overseer; a proposal is answered with ctrl+y and the agent gets its turn from
//! Overseer. Real overseerd; the SYNTHETIC Claude fixture (Overseer mode); no paid tokens.
mod support;

use crossterm::event::{KeyCode, KeyModifiers};
use overseer_tui::app::Mode;
use serde_json::json;
use std::path::Path;
use support::*;

#[test]
fn t25_talk_to_overseer_from_the_terminal_and_see_who_is_held() {
    let t = tempfile::tempdir().unwrap();
    let claude = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().join("fixtures/fake-harness/claude-fixture.js").display().to_string();
    let mode_file = t.path().join("mode");
    std::fs::write(&mode_file, "echo").unwrap();
    let d = Daemon::start(&[("OVERSEER_CLAUDE_PATH", &claude), ("CLAUDE_FIXTURE_MODE_FILE", mode_file.to_str().unwrap()), ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "CLAUDE_FIXTURE_MODE_FILE")]);
    let repo = repo(&t.path().join("repo"));
    let api = d.ctl("task.create", json!({"repo": repo, "harness": "claude", "prompt": "write the API", "title": "API tests"}))["run"]["id"].as_str().unwrap().to_string();
    d.wait_status(&api, |s| s == "completed", 30);
    let sleeper = d.sh(&repo, "Sleeper", "sleep 120");
    d.wait_status(&sleeper, |s| s == "running", 20);
    d.ctl("agent.hold", json!({"run_id": sleeper, "reason": "wait for the API", "by": "owner"}));
    let mut tui = Tui::attach(&d, 180, 50);
    tui.until(10, |a| a.visible().len() == 2);
    // The held agent's tile says so (from the daemon's state, like every other surface).
    let s = tui.until_screen(10, "⏸ held");
    assert!(s.contains("Sleeper"), "{s}");
    tui.snapshot("t25-held");

    // o opens the conversation; typing and Enter sends; Overseer (the fixture) proposes.
    std::fs::write(&mode_file, "overseer").unwrap();
    tui.key(KeyCode::Char('o'));
    tui.until(5, |a| matches!(a.mode, Mode::Overseer));
    tui.type_text("Tell API tests to add tests");
    tui.key(KeyCode::Enter);
    let s = tui.until_screen(60, "Overseer will:");
    assert!(s.contains("Send API tests") && s.contains("Please add tests"), "{s}");
    assert!(s.contains("you › Tell API tests to add tests"), "the owner's words are in the conversation:\n{s}");
    tui.snapshot("t25-proposal");

    // ctrl+y answers the proposal: the agent gets its turn from Overseer.
    let before = d.ctl("run.turns", json!({"run_id": api})).as_array().unwrap().len();
    tui.key_mod(KeyCode::Char('y'), KeyModifiers::CONTROL);
    tui.until(20, |a| a.overseer["proposals"].as_array().map(|p| p.iter().all(|x| x["state"] != "open")).unwrap_or(false));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    let turns = loop {
        let turns = d.ctl("run.turns", json!({"run_id": api})).as_array().unwrap().clone();
        if turns.len() > before {
            break turns;
        }
        assert!(std::time::Instant::now() < deadline, "the agent got no turn");
        tui.pump(200);
    };
    assert!(turns.last().unwrap()["prompt"].as_str().unwrap().starts_with("From Overseer: Please add tests."), "{:?}", turns.last());
    let s = tui.until_screen(20, "Done");
    tui.snapshot("t25-answered");
    assert!(!s.contains("ctrl+y yes"), "no proposal waits any more:\n{s}");
    tui.key(KeyCode::Esc);
    assert!(matches!(tui.app.mode, Mode::Grid));
    d.ctl("agent.release", json!({"run_id": sleeper, "by": "owner"}));
    d.ctl("run.interrupt", json!({"run_id": sleeper}));
}
