//! T-02, T-04 and T-05: pages of nine, keyboard and mouse navigation, help, and typing to
//! agents. Real overseerd; generic fixture agents and the SYNTHETIC Claude fixture.
mod support;

use crossterm::event::{KeyCode, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use overseer_tui::app::{Filter, Mode};
use serde_json::json;
use std::path::Path;
use support::*;

fn fixture(name: &str) -> String {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().join("fixtures/fake-harness").join(name).display().to_string()
}

fn claude_daemon(mode: &str) -> Daemon {
    let claude = fixture("claude-fixture.js");
    Daemon::start(&[("OVERSEER_CLAUDE_PATH", &claude), ("CLAUDE_FIXTURE_MODE", mode), ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "CLAUDE_FIXTURE_MODE")])
}

fn titles_on_screen(s: &str, n: usize) -> Vec<usize> {
    (1..=n).filter(|i| s.contains(&format!("agent {i:02}"))).collect()
}

#[test]
fn t02_pages_of_nine_newest_first() {
    let t = tempfile::tempdir().unwrap();
    let d = Daemon::start(&[]);
    let repo = repo(&t.path().join("many"));
    let mut ids = Vec::new();
    for i in 1..=20 {
        ids.push(d.sh(&repo, &format!("agent {i:02}"), &format!("echo I am agent {i:02}")));
    }
    let mut tui = Tui::attach(&d, 200, 60);
    tui.until(10, |a| a.visible().len() == 20);
    let s = tui.screen();
    assert!(s.contains("page 1/3"), "{s}");
    assert_eq!(titles_on_screen(&s, 20), (12..=20).collect::<Vec<_>>(), "page 1 is the newest nine");
    tui.snapshot("t02-page-1");
    tui.key(KeyCode::Char(']'));
    let s = tui.screen();
    assert!(s.contains("page 2/3"), "{s}");
    assert_eq!(titles_on_screen(&s, 20), (3..=11).collect::<Vec<_>>());
    tui.snapshot("t02-page-2");
    tui.key(KeyCode::PageDown);
    let s = tui.screen();
    assert!(s.contains("page 3/3"));
    assert_eq!(titles_on_screen(&s, 20), vec![1, 2]);
    tui.snapshot("t02-page-3");
    tui.key(KeyCode::Char(']'));
    assert!(tui.screen().contains("page 3/3"), "stays on the last page");
    tui.key(KeyCode::PageUp);
    tui.key(KeyCode::Char('['));
    assert!(tui.screen().contains("page 1/3"));

    // Focus follows the agent: focus agent 15 (slot 6 on page 1), then a new agent starts.
    tui.key(KeyCode::Char('6'));
    assert_eq!(tui.app.focused().map(|r| r.title.as_str()), Some("agent 15"));
    let newest = d.sh(&repo, "agent 21", "echo I am agent 21; sleep 30");
    tui.until(10, |a| a.state.run(&newest).is_some());
    tui.pump(200);
    assert_eq!(tui.app.focused().map(|r| r.title.as_str()), Some("agent 15"), "focus stays on the same agent");
    let s = tui.screen();
    assert!(s.contains("1 ● agent 21"), "the new agent is tile 1 on page 1:\n{s}");
    assert!(s.contains("7 ✓ agent 15"), "agent 15 moved to slot 7:\n{s}");

    // Filter: all → active → needs you → all.
    tui.key(KeyCode::Char('f'));
    assert_eq!(tui.app.filter, Filter::Active);
    let s = tui.screen();
    assert!(s.contains("filter: active") && s.contains("agent 21") && !s.contains("agent 20"), "{s}");
    tui.key(KeyCode::Char('f'));
    assert!(tui.screen().contains("No agents match"));
    tui.key(KeyCode::Char('f'));
    assert_eq!(tui.app.filter, Filter::All);
    d.ctl("run.interrupt", json!({ "run_id": newest }));
}

#[test]
fn t04_keyboard_navigation_help_and_mouse() {
    let t = tempfile::tempdir().unwrap();
    let d = Daemon::start(&[]);
    let repo = repo(&t.path().join("nav"));
    let mut ids = Vec::new();
    for i in 1..=12 {
        ids.push(d.sh(&repo, &format!("agent {i:02}"), "echo hi"));
    }
    ids.reverse(); // newest first, as shown
    let mut tui = Tui::attach(&d, 180, 54);
    tui.until(10, |a| a.visible().len() == 12);
    let at = |tui: &Tui| tui.app.focus.as_deref().and_then(|f| ids.iter().position(|i| i == f));
    assert_eq!(at(&tui), Some(0), "the newest agent starts focused");
    tui.key(KeyCode::Right);
    assert_eq!(at(&tui), Some(1));
    tui.key(KeyCode::Char('l'));
    assert_eq!(at(&tui), Some(2));
    // Past the right edge: page 2 (three agents, one row).
    tui.key(KeyCode::Right);
    assert_eq!((at(&tui), tui.app.page), (Some(9), 1));
    tui.key(KeyCode::Left);
    assert_eq!((at(&tui), tui.app.page), (Some(2), 0), "back to page 1's rightmost tile");
    tui.key(KeyCode::Down);
    assert_eq!(at(&tui), Some(5));
    tui.key(KeyCode::Char('j'));
    assert_eq!(at(&tui), Some(8));
    tui.key(KeyCode::Down);
    assert_eq!(at(&tui), Some(8), "no row below");
    tui.key(KeyCode::Char('k'));
    tui.key(KeyCode::Char('h'));
    assert_eq!(at(&tui), Some(4));
    tui.key(KeyCode::Char('1'));
    assert_eq!(at(&tui), Some(0));
    tui.key(KeyCode::Char('9'));
    assert_eq!(at(&tui), Some(8));
    tui.key(KeyCode::Tab);
    assert_eq!((at(&tui), tui.app.page), (Some(9), 1), "tab walks across pages");
    tui.key(KeyCode::Char('1'));
    assert_eq!(at(&tui), Some(9), "1–9 are slots on the current page");
    tui.key(KeyCode::BackTab);
    assert_eq!(at(&tui), Some(8));
    tui.key(KeyCode::Char('1'));
    tui.key(KeyCode::BackTab);
    assert_eq!(at(&tui), Some(11), "shift+tab wraps to the last agent");

    // The focused tile is unmistakable: thick accent border and its title.
    tui.key(KeyCode::Char('['));
    tui.key(KeyCode::Char('1'));
    assert_eq!((at(&tui), tui.app.page), (Some(0), 0));
    let s = tui.screen();
    assert!(s.contains("┏ 1 "), "{s}");

    // Help lists every key; any key closes it.
    tui.key(KeyCode::Char('?'));
    let s = tui.screen();
    for k in ["move between agents", "next / previous page", "message the focused agent", "zoom", "allow / deny", "next agent waiting", "interrupt", "start a new agent", "filter", "quit"] {
        assert!(s.contains(k), "help lacks {k}:\n{s}");
    }
    tui.snapshot("t04-help");
    tui.key(KeyCode::Char('x'));
    assert_eq!(tui.app.mode, Mode::Grid);

    // A mouse click focuses the tile under it.
    tui.draw();
    let (id, x, y, w, h) = tui.app.hit.iter().find(|h| h.0 == ids[4]).cloned().unwrap();
    tui.app.handle_mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column: x + w / 2, row: y + h / 2, modifiers: KeyModifiers::NONE });
    assert_eq!(tui.app.focus.as_deref(), Some(id.as_str()));
}

#[test]
fn t05_messages_go_to_the_focused_agent_only_and_drafts_are_kept() {
    let t = tempfile::tempdir().unwrap();
    let d = claude_daemon("permission");
    let repo = repo(&t.path().join("talk"));
    let a = d.sh(&repo, "Agent A", "while read l; do echo \"A heard: $l\"; done");
    let b = d.sh(&repo, "Agent B", "while read l; do echo \"B heard: $l\"; done");
    let busy = d.ctl("task.create", json!({ "repo": repo, "harness": "claude", "prompt": "write perm.txt", "title": "Busy Claude" }))["run"]["id"].as_str().unwrap().to_string();
    d.wait_status(&busy, |s| s == "waiting_for_user", 20);
    let mut tui = Tui::attach(&d, 180, 50);
    tui.until(10, |app| app.visible().len() == 3);
    let focus = |tui: &mut Tui, id: &str| {
        let i = tui.app.visible().iter().position(|r| r.id == id).unwrap();
        tui.key(KeyCode::Char(char::from_digit(i as u32 + 1, 10).unwrap()));
    };

    focus(&mut tui, &a);
    tui.key(KeyCode::Char('i'));
    assert_eq!(tui.app.mode, Mode::Compose);
    tui.type_text("for A only");
    let s = tui.screen();
    assert!(s.contains("message → Agent A") && s.contains("for A only"), "{s}");
    tui.snapshot("t05-composer");
    tui.key(KeyCode::Enter);
    tui.until_screen(10, "A heard: for A only");
    let heard = |run: &str, text: &str| d.events(run).iter().any(|e| e["kind"] == "output" && e["payload"]["text"] == text);
    assert!(heard(&a, "A heard: for A only"));
    assert!(!d.events(&b).iter().any(|e| e["kind"] == "output"), "B got nothing");

    // Drafts are per agent and survive switching tiles.
    focus(&mut tui, &b);
    tui.key(KeyCode::Char('i'));
    tui.type_text("draft for B");
    tui.key_mod(KeyCode::Enter, KeyModifiers::ALT);
    tui.type_text("second line");
    tui.key(KeyCode::Esc);
    assert_eq!(tui.app.mode, Mode::Grid);
    assert!(tui.screen().contains("✎ draft"), "the tile shows it has a draft");
    focus(&mut tui, &a);
    focus(&mut tui, &b);
    assert_eq!(tui.app.drafts.get(&b).map(String::as_str), Some("draft for B\nsecond line"));
    tui.key(KeyCode::Char('i'));
    tui.key(KeyCode::Enter);
    tui.until_screen(10, "B heard: second line");
    assert!(heard(&b, "B heard: draft for B") && heard(&b, "B heard: second line"));
    assert!(!heard(&a, "A heard: draft for B"));

    // A running Claude turn cannot take a message: the composer says why instead of failing.
    focus(&mut tui, &busy);
    tui.key(KeyCode::Char('i'));
    tui.type_text("are you there?");
    let s = tui.screen();
    assert!(s.contains("can't send now: a turn is running"), "{s}");
    tui.key(KeyCode::Enter);
    assert!(tui.screen().contains("Not sent: a turn is running"));
    assert_eq!(tui.app.drafts.get(&busy).map(String::as_str), Some("are you there?"), "the draft is kept");
    tui.key(KeyCode::Esc);
    d.ctl("run.interrupt", json!({ "run_id": busy }));
}

#[test]
fn t15_search_filters_agents_as_you_type() {
    let t = tempfile::tempdir().unwrap();
    let d = Daemon::start(&[]);
    let web = repo(&t.path().join("web-app"));
    let api = repo(&t.path().join("payments-api"));
    for (r, title) in [(&web, "Fix login redirect"), (&api, "Retry refunds"), (&web, "Refresh sessions once"), (&api, "Add idempotency keys")] {
        d.sh(r, title, "echo ok");
    }
    let mut tui = Tui::attach(&d, 160, 44);
    tui.until(10, |a| a.visible().len() == 4);
    tui.key(KeyCode::Char('/'));
    assert_eq!(tui.app.mode, Mode::Search);
    tui.type_text("ref");
    let s = tui.screen();
    assert!(s.contains("/ref▌") && s.contains("Retry refunds") && s.contains("Refresh sessions once") && !s.contains("Fix login redirect"), "{s}");
    // Repository names match too.
    tui.key(KeyCode::Backspace);
    tui.key(KeyCode::Backspace);
    tui.key(KeyCode::Backspace);
    tui.type_text("payments");
    assert_eq!(tui.app.visible().len(), 2);
    tui.key(KeyCode::Enter);
    assert_eq!(tui.app.mode, Mode::Grid);
    let s = tui.screen();
    assert!(s.contains("/payments") && s.contains("2 agents"), "the search stays applied:\n{s}");
    tui.snapshot("t15-search");
    assert!(tui.app.visible().iter().any(|r| Some(r.id.as_str()) == tui.app.focus.as_deref()), "focus is on a match");
    tui.key(KeyCode::Esc);
    assert!(tui.app.search.is_empty() && tui.app.visible().len() == 4, "esc clears it");
}
