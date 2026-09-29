//! AC-245 in the terminal: the rendered screens show no internal words (ids, snake_case states,
//! mcp__ tool names, lowercase harness ids, confidence tags, raw HTTP or OS errors), a usage
//! limit is said in plain words, and Overseer's Markdown is drawn as a list, not as its marks.
//! AC-246: the header's Needs-you count is the one the extension and the phone show.
//! Real overseerd; the SYNTHETIC Claude fixture; no paid tokens.
mod support;

use crossterm::event::KeyCode;
use overseer_tui::app::Mode;
use serde_json::json;
use std::path::Path;
use support::*;

/// Whatever a screen shows that the owner must never read (the same list as test/ui/plain-words.js).
fn leaks(screen: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in screen.lines() {
        for w in line.split(|c: char| c.is_whitespace() || "·›│┃║|()[],:;\"'".contains(c)) {
            let w = w.trim_matches(|c: char| c == '.' || c == '…');
            let snake = w.contains('_') && w.split('_').all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_lowercase()));
            let id = w.split_once('-').is_some_and(|(p, r)| matches!(p, "r" | "p" | "sh" | "w") && r.len() >= 8 && r.chars().all(|c| c.is_ascii_hexdigit()));
            if snake || id || w.starts_with("mcp__") || matches!(w, "claude" | "codex" | "opencode" | "tool-input" | "rate_limit") {
                out.push(format!("{w:?} in {line:?}"));
            }
        }
        for raw in ["os error", "Connection refused", "API Error", "turn reported failure", "(429)", "HTTP 4", "HTTP 5", "owner · vscode"] {
            if line.contains(raw) {
                out.push(format!("{raw:?} in {line:?}"));
            }
        }
    }
    out
}

#[test]
fn t30_no_internal_words_and_overseer_markdown_as_a_list() {
    let t = tempfile::tempdir().unwrap();
    let claude = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().join("fixtures/fake-harness/claude-fixture.js").display().to_string();
    let mode_file = t.path().join("mode");
    let d = Daemon::start(&[("OVERSEER_CLAUDE_PATH", &claude), ("CLAUDE_FIXTURE_MODE_FILE", mode_file.to_str().unwrap()), ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "CLAUDE_FIXTURE_MODE_FILE")]);
    let web = repo(&t.path().join("web-app"));
    // A usage limit (raw text from the harness), a permission that waits, a program that fails, and one done. What a
    // program itself prints is its own output, like a terminal; what Overseer says about it is checked.
    std::fs::write(&mode_file, "ratelimit").unwrap();
    let limited = d.ctl("task.create", json!({ "repo": web, "harness": "claude", "prompt": "Write the release notes", "title": "Release notes" }))["run"]["id"].as_str().unwrap().to_string();
    d.wait_status(&limited, |s| s == "failed" || s == "completed", 30);
    std::fs::write(&mode_file, "permission").unwrap();
    let waiting = d.ctl("task.create", json!({ "repo": web, "harness": "claude", "prompt": "Add a changelog entry", "title": "Changelog entry" }))["run"]["id"].as_str().unwrap().to_string();
    d.wait_status(&waiting, |s| s == "waiting_for_user", 30);
    let broken = d.sh(&web, "Migration dry-run", "echo 'relation users_v2 missing' 1>&2; exit 1");
    d.wait_status(&broken, |s| s == "failed", 20);
    let done = d.sh(&web, "Tidy lint", "echo tidy");
    d.wait_status(&done, |s| s == "completed", 20);

    let mut tui = Tui::attach(&d, 200, 60);
    tui.until(15, |a| a.visible().len() == 4 && a.state.run(&waiting).is_some_and(|r| r.needs_you()));
    tui.pump(800);
    let grid = tui.screen();
    tui.snapshot("t30-agents");
    assert!(leaks(&grid).is_empty(), "internal words on the agents' screen: {:?}\n{grid}", leaks(&grid));
    assert!(grid.contains("Claude Code"), "the harness by name:\n{grid}");
    assert!(grid.contains("usage limit"), "the usage limit in plain words:\n{grid}");
    // AC-246: one agent waits for an answer; the failures are not "needs you".
    assert!(grid.contains("1 needs you"), "the header counts what waits for an answer:\n{grid}");
    assert_eq!(tui.app.state.needs_you_count(), 1);

    // Overseer's reply is a Markdown list: drawn as bullets, without its ** marks.
    std::fs::write(&mode_file, "overseer").unwrap();
    tui.key(KeyCode::Char('o'));
    tui.until(5, |a| matches!(a.mode, Mode::Overseer));
    tui.type_text("What is everyone doing?");
    tui.key(KeyCode::Enter);
    let s = tui.until_screen(60, "Here is what everyone is doing");
    let s = if s.contains("• ") { s } else { tui.until_screen(10, "• ") };
    tui.snapshot("t30-overseer-markdown");
    assert!(s.contains("• Changelog entry"), "the list is drawn with bullets:\n{s}");
    assert!(!s.contains("**"), "no Markdown marks:\n{s}");
    assert!(s.contains("Changelog entry: waiting for you") && leaks(&s).is_empty(), "an agent's state in words, nothing internal: {:?}\n{s}", leaks(&s));
}
