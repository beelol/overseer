//! T-02, T-04 and T-05: pages of agents (sixteen per page since T-37), keyboard and mouse navigation, help, and typing to
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

/// Agents with a tile on screen: a title after its slot number ("7 ✓ agent 15"); the agent list's
/// rows (T-25) have no number.
fn titles_on_screen(s: &str, n: usize) -> Vec<usize> {
    (1..=n)
        .filter(|i| {
            let t = format!("agent {i:02}");
            s.lines().any(|l| l.match_indices(&t).any(|(at, _)| l[..at].chars().rev().nth(3).is_some_and(|c| c.is_ascii_digit())))
        })
        .collect()
}

#[test]
fn t02_pages_newest_first() {
    let t = tempfile::tempdir().unwrap();
    let d = Daemon::start(&[]);
    let repo = repo(&t.path().join("many"));
    let mut ids = Vec::new();
    for i in 1..=20 {
        ids.push(d.sh(&repo, &format!("agent {i:02}"), &format!("echo I am agent {i:02}")));
    }
    let mut tui = Tui::attach(&d, 200, 60);
    tui.until(10, |a| a.visible().len() == 20);
    // T-37: sixteen per page (a 4×4 grid), the rest on page 2.
    let s = tui.screen();
    assert!(s.contains("page 1/2"), "{s}");
    assert_eq!(titles_on_screen(&s, 20), (5..=20).collect::<Vec<_>>(), "page 1 is the newest sixteen");
    tui.snapshot("t02-page-1");
    tui.key(KeyCode::Char(']'));
    let s = tui.screen();
    assert!(s.contains("page 2/2"), "{s}");
    assert_eq!(titles_on_screen(&s, 20), (1..=4).collect::<Vec<_>>());
    tui.snapshot("t02-page-2");
    tui.key(KeyCode::PageDown);
    assert!(tui.screen().contains("page 2/2"), "stays on the last page");
    tui.key(KeyCode::PageUp);
    tui.key(KeyCode::Char('['));
    assert!(tui.screen().contains("page 1/2"));

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

    // Filter: all → active → needs you → archived (T-34) → all.
    tui.key(KeyCode::Char('f'));
    assert_eq!(tui.app.filter, Filter::Active);
    let s = tui.screen();
    assert!(s.contains("filter: active") && s.contains("agent 21") && !s.contains("agent 20"), "{s}");
    tui.key(KeyCode::Char('f'));
    assert!(tui.screen().contains("No agents match"));
    tui.key(KeyCode::Char('f'));
    assert_eq!(tui.app.filter, Filter::Archived);
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
    for i in 1..=20 {
        ids.push(d.sh(&repo, &format!("agent {i:02}"), "echo hi"));
    }
    ids.reverse(); // newest first, as shown
    let mut tui = Tui::attach(&d, 180, 54);
    tui.until(10, |a| a.visible().len() == 20);
    // T-37: page 1 is a 4×4 grid of the newest sixteen, page 2 a 2×2 of the other four.
    let at = |tui: &Tui| tui.app.focus.as_deref().and_then(|f| ids.iter().position(|i| i == f));
    assert_eq!(at(&tui), Some(0), "the newest agent starts focused");
    tui.key(KeyCode::Right);
    assert_eq!(at(&tui), Some(1));
    tui.key(KeyCode::Char('l'));
    assert_eq!(at(&tui), Some(2));
    tui.key(KeyCode::Right);
    assert_eq!(at(&tui), Some(3));
    // Past the right edge: page 2's same row.
    tui.key(KeyCode::Right);
    assert_eq!((at(&tui), tui.app.page), (Some(16), 1));
    tui.key(KeyCode::Left);
    assert_eq!((at(&tui), tui.app.page), (Some(3), 0), "back to page 1's rightmost tile");
    tui.key(KeyCode::Down);
    assert_eq!(at(&tui), Some(7));
    tui.key(KeyCode::Char('j'));
    assert_eq!(at(&tui), Some(11));
    tui.key(KeyCode::Down);
    assert_eq!(at(&tui), Some(15));
    tui.key(KeyCode::Down);
    assert_eq!(at(&tui), Some(15), "no row below");
    tui.key(KeyCode::Char('k'));
    tui.key(KeyCode::Char('h'));
    assert_eq!(at(&tui), Some(10));
    tui.key(KeyCode::Char('1'));
    assert_eq!(at(&tui), Some(0));
    tui.key(KeyCode::Char('9'));
    assert_eq!(at(&tui), Some(8));
    for _ in 0..7 {
        tui.key(KeyCode::Tab);
    }
    assert_eq!((at(&tui), tui.app.page), (Some(15), 0));
    tui.key(KeyCode::Tab);
    assert_eq!((at(&tui), tui.app.page), (Some(16), 1), "tab walks across pages");
    tui.key(KeyCode::Char('1'));
    assert_eq!(at(&tui), Some(16), "1–9 are slots on the current page");
    tui.key(KeyCode::BackTab);
    assert_eq!(at(&tui), Some(15));
    tui.key(KeyCode::Char('1'));
    tui.key(KeyCode::BackTab);
    assert_eq!(at(&tui), Some(19), "shift+tab wraps to the last agent");

    // The focused tile is unmistakable: thick accent border and its title.
    tui.key(KeyCode::Char('['));
    tui.key(KeyCode::Char('1'));
    assert_eq!((at(&tui), tui.app.page), (Some(0), 0));
    let s = tui.screen();
    assert!(s.contains("┏ 1 "), "{s}");

    // Help lists every key; any key closes it.
    tui.key(KeyCode::Char('?'));
    let s = tui.screen();
    for k in ["move between agents", "next / previous page", "message the focused agent", "zoom", "allow / deny", "next agent waiting", "stop the focused agent and pause its queue", "start a new agent", "filter", "quit"] {
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

    // AC-241: a Claude agent waiting on a permission takes a reply: it denies the request, and the
    // agent reads the reply as the reason.
    focus(&mut tui, &busy);
    let s = tui.screen();
    assert!(s.contains("a allow") && s.contains("d deny"), "the tile offers the answers: {s}");
    tui.key(KeyCode::Char('i'));
    tui.type_text("not perm.txt; write notes.md instead");
    let s = tui.screen();
    assert!(s.contains("deny with a note → Busy Claude") && !s.contains("can't send now"), "{s}");
    tui.snapshot("t05-deny-with-a-note");
    tui.key(KeyCode::Enter);
    d.wait_status(&busy, |s| !matches!(s, "waiting_for_user" | "running" | "queued" | "starting"), 20);
    let answered = d.events(&busy).into_iter().find(|e| e["kind"] == "permission_answered").expect("answered");
    assert_eq!(answered["payload"]["allow"], false);
    assert!(d.events(&busy).iter().any(|e| e["kind"] == "tool_result" && e["payload"].to_string().contains("write notes.md instead")), "the note reached the agent as the reason");
    assert!(tui.app.drafts.get(&busy).is_none(), "the note was sent");
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
