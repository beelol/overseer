//! T-13: real harnesses in the TUI. Opt-in (spends a few tokens): OVERSEER_TUI_LIVE=1.
//! Isolated OVERSEER_HOME; the desktop logins are used as they are and never signed out.
//! Tiny prompts: Claude Code (haiku) and Codex (gpt-5.6-luna), plus one follow-up typed in the TUI.
mod support;

use crossterm::event::KeyCode;
use serde_json::json;
use std::path::Path;
use support::*;

#[test]
fn t13_live_claude_and_codex_stream_in_the_tui() {
    if std::env::var_os("OVERSEER_TUI_LIVE").is_none() {
        eprintln!("skipped: set OVERSEER_TUI_LIVE=1 to run the live harness check");
        return;
    }
    let t = tempfile::tempdir().unwrap();
    let d = Daemon::start(&[]);
    let repo = repo(&t.path().join("live-demo"));
    let claude = d.ctl("task.create", json!({ "repo": repo, "harness": "claude", "profile_id": "system-claude", "model": "haiku", "title": "Live Claude hello", "prompt": "Reply with exactly: hi from claude" }))["run"]["id"].as_str().unwrap().to_string();
    let codex = d.ctl("task.create", json!({ "repo": repo, "harness": "codex", "profile_id": "system-codex", "model": "gpt-5.6-luna", "title": "Live Codex hello", "prompt": "Reply with exactly: hi from codex" }))["run"]["id"].as_str().unwrap().to_string();
    let mut tui = Tui::attach(&d, 160, 44);
    tui.until(10, |a| a.visible().len() == 2);
    let s = tui.until_screen(180, "hi from claude");
    eprintln!("{s}");
    tui.until_screen(180, "hi from codex");
    d.wait_status(&claude, |s| s != "running" && s != "starting", 120);
    tui.until(30, |a| a.state.run(&claude).is_some_and(|r| !r.active()));
    tui.snapshot("t13-live-first-turns");

    // A follow-up typed in the TUI reaches the live Claude agent.
    let i = tui.app.visible().iter().position(|r| r.id == claude).unwrap();
    tui.key(KeyCode::Char(char::from_digit(i as u32 + 1, 10).unwrap()));
    tui.key(KeyCode::Char('i'));
    tui.type_text("Reply with exactly: follow-up received");
    tui.key(KeyCode::Enter);
    tui.until_screen(180, "follow-up received");
    d.wait_status(&claude, |s| s != "running" && s != "starting", 120);
    tui.pump(1500);
    tui.snapshot("t13-live-follow-up");
    let turns = d.ctl("run.turns", json!({ "run_id": claude }));
    let record = json!({
        "claude": { "run": claude, "status": d.run(&claude)["status"], "model": d.run(&claude)["model"], "turns": turns },
        "codex": { "run": codex, "status": d.run(&codex)["status"], "model": d.run(&codex)["model"] },
    });
    std::fs::write(Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().join("docs/verification/evidence/tui/t13-live-records.json"), serde_json::to_string_pretty(&record).unwrap()).unwrap();
    assert!(turns.as_array().unwrap().iter().any(|t| t["prompt"] == "Reply with exactly: follow-up received"), "{turns}");
}
