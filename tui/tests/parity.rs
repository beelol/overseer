//! T-25 onward (docs/rfcs/tui.md, "Parity with VS Code and the review in the terminal"): the agent
//! list beside the grid, the grid that fits the count, one key to a single agent, and the review
//! in the terminal. Real overseerd; generic fixture agents and the SYNTHETIC Claude fixture.
mod support;

use crossterm::event::KeyCode;
use overseer_tui::app::Mode;
use serde_json::json;
use std::path::Path;
use support::*;
use unicode_width::UnicodeWidthStr;

fn fixture(name: &str) -> String {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().join("fixtures/fake-harness").join(name).display().to_string()
}

fn no_line_wider(s: &str, w: usize) {
    for l in s.lines() {
        assert!(l.width() <= w, "a line wider than {w}: {l:?}");
    }
}

/// T-25: 20 agents in 3 repositories, grouped in the list with their counts, accounts and marks;
/// `J` picks agents in the list's order and shows the picked one's conversation beside the grid;
/// Esc returns to the grid on that agent; a search for one repository leaves only its group.
#[test]
fn t25_an_agent_list_beside_the_grid() {
    let t = tempfile::tempdir().unwrap();
    let sys = t.path().join("desktop-home");
    std::fs::create_dir_all(sys.join(".claude")).unwrap();
    // The fixture's accounts (synthetic): the Mac's default login and a named one.
    std::fs::write(sys.join(".claude/fixture-account.json"), r#"{"email":"bilal@testbox.com","plan":"max"}"#).unwrap();
    let claude = fixture("claude-fixture.js");
    let sys_s = sys.display().to_string();
    let d = Daemon::start(&[("OVERSEER_CLAUDE_PATH", &claude), ("OVERSEER_TEST_SYSTEM_HOME", &sys_s), ("CLAUDE_FIXTURE_MODE", "permission"), ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "CLAUDE_FIXTURE_MODE")]);
    let personal = d.ctl("account.create", json!({ "provider": "anthropic", "name": "Personal" }))["account"].clone();
    std::fs::write(Path::new(personal["home"].as_str().unwrap()).join("claude/fixture-account.json"), r#"{"email":"ana.silva@personal.example","plan":"pro"}"#).unwrap();
    let web = repo(&t.path().join("web-app"));
    let api = repo(&t.path().join("api-server"));
    let docs = repo(&t.path().join("docs-site"));

    // docs-site: one agent whose work was merged, then four finished ones.
    let merged = d.sh(&docs, "Add the features list", "printf 'export const features = [];\\n' > features.js; echo output of the features list");
    d.wait_status(&merged, |s| s == "completed", 20);
    let ws = d.run(&merged)["workspace_id"].as_str().unwrap().to_string();
    assert_eq!(d.ctl("workspace.merge_prepare", json!({ "workspace_id": ws }))["state"], "ready");
    d.ctl("workspace.merge_complete", json!({ "workspace_id": ws }));
    for i in 1..=4 {
        d.sh(&docs, &format!("Docs page {i}"), &format!("echo output of docs page {i}"));
    }
    // api-server: six finished, one failed.
    for i in 1..=6 {
        d.sh(&api, &format!("Endpoint {i}"), &format!("echo output of endpoint {i}"));
    }
    d.sh(&api, "Migration dry-run", "echo 'error: relation users_v2 missing' 1>&2; exit 1");
    // web-app: four finished, two working, two waiting for you on two accounts.
    for i in 1..=4 {
        d.sh(&web, &format!("Web task {i}"), &format!("echo output of web task {i}"));
    }
    let busy: Vec<String> = (1..=2).map(|i| d.sh(&web, &format!("Watch build {i}"), &format!("echo output of watch build {i}; sleep 60"))).collect();
    let mac = d.ctl("task.create", json!({ "repo": web, "harness": "claude", "prompt": "Tidy the login page", "title": "Tidy the login page" }))["run"]["id"].as_str().unwrap().to_string();
    let own = d.ctl("task.create", json!({ "repo": web, "harness": "claude", "prompt": "Write the release notes", "title": "Write the release notes", "profile_id": personal["id"] }))["run"]["id"].as_str().unwrap().to_string();
    d.wait_status(&mac, |s| s == "waiting_for_user", 20);
    d.wait_status(&own, |s| s == "waiting_for_user", 20);

    let mut tui = Tui::attach(&d, 200, 60);
    tui.until(20, |a| a.visible().len() == 20 && a.visible().iter().filter(|r| r.active()).count() == 4);
    tui.until(15, |a| a.state.profiles.iter().filter(|p| p.account.as_ref().is_some_and(|x| x.email.is_some())).count() >= 2);
    tui.until(10, |a| a.state.landing_text(&ws).is_some());
    tui.pump(300);

    // Grouped by repository, the most recently active first, each heading with its counts.
    let groups: Vec<String> = tui.app.groups().iter().map(|g| g.0.rsplit('/').next().unwrap().to_string()).collect();
    assert_eq!(groups, ["web-app", "api-server", "docs-site"], "web-app is working now; api-server finished after docs-site");
    for (w, h, name) in [(200u16, 60u16, "parity-t25-list-200x60"), (140, 40, "parity-t25-list-140x40"), (100, 30, "parity-t25-list-100x30")] {
        tui.resize(w, h);
        let s = tui.screen();
        assert!(s.contains(" agents "), "the list at {w}×{h}:\n{s}");
        assert!(s.contains("web-app  2 working · 2 needs you") || (s.contains(" web-app ") && s.contains("   2 working · 2 needs you")), "web-app's counts at {w}×{h}:\n{s}");
        assert!(s.contains("api-server  1 failed"), "api-server's counts at {w}×{h}:\n{s}");
        assert!(s.contains("docs-site  1 merged"), "docs-site's counts at {w}×{h}:\n{s}");
        // Each row: status, title, account, mark.
        assert!(s.lines().any(|l| l.contains("◆ Tidy the") && l.contains("Claude Max ◆")), "the Mac's login and the needs-you mark at {w}×{h}:\n{s}");
        assert!(s.lines().any(|l| l.contains("◆ Write the") && l.contains("Personal ◆")), "the named account at {w}×{h}:\n{s}");
        assert!(s.lines().any(|l| l.contains("✓ Add the") && l.contains(" ✓ │")), "the merged mark at {w}×{h}:\n{s}");
        no_line_wider(&s, w as usize);
        tui.snapshot(name);
    }

    // J three times: the third agent in the list, its conversation beside the grid.
    tui.resize(200, 60);
    let order = tui.app.list_ids();
    for _ in 0..3 {
        tui.key(KeyCode::Char('J'));
    }
    let third = order[2].clone();
    assert_eq!(tui.app.focus.as_deref(), Some(third.as_str()));
    assert!(tui.app.picked);
    let title = tui.app.state.run(&third).unwrap().title.clone();
    let words = format!("output of {}", title.to_lowercase());
    let s = tui.until_screen(10, &words);
    // The conversation is its own column, right of the grid: its first line is past the list and the tiles.
    let (_, conv_w) = overseer_tui::app::side_widths(200, true, true);
    let conv_x = 200 - conv_w as usize;
    assert!(s.lines().any(|l| l.rfind(&words).is_some_and(|i| l[..i].width() > conv_x)), "the conversation is in the right-hand column (from {conv_x}):\n{s}");
    no_line_wider(&s, 200);
    tui.snapshot("parity-t25-picked");
    tui.key(KeyCode::Esc);
    assert_eq!(tui.app.mode, Mode::Grid);
    assert!(!tui.app.picked);
    assert_eq!(tui.app.focus.as_deref(), Some(third.as_str()), "back on the grid with that agent focused");

    // A search for one repository leaves only its group, in the list and the grid.
    tui.key(KeyCode::Char('/'));
    tui.type_text("docs-site");
    tui.key(KeyCode::Enter);
    assert_eq!(tui.app.groups().len(), 1);
    let s = tui.screen();
    assert!(s.contains("docs-site  1 merged") && !s.contains("web-app") && !s.contains("api-server"), "{s}");
    assert_eq!(tui.app.visible().len(), 5);
    tui.snapshot("parity-t25-search");
    tui.key(KeyCode::Esc);

    // L hides the list and shows it again; below 100 columns the compact layout stays.
    tui.key(KeyCode::Char('L'));
    assert!(!tui.screen().contains("╭ agents "));
    tui.key(KeyCode::Char('L'));
    assert!(tui.screen().contains("╭ agents "));
    tui.resize(99, 30);
    let s = tui.screen();
    assert!(s.contains(" agents ") && !s.contains("web-app  2 working"), "the compact list, not the grouped one:\n{s}");
    for id in busy.iter().chain([&mac, &own]) {
        d.ctl("run.interrupt", json!({ "run_id": id }));
    }
}

/// The grid's rows and columns as drawn: the tiles left of the conversation column, by row.
fn drawn_shape(tui: &mut Tui) -> (usize, usize) {
    tui.draw();
    let (w, _) = tui.app.size;
    let (_, conv_w) = overseer_tui::app::side_widths(w, tui.app.list_shown(), tui.app.picked);
    let tiles: Vec<(u16, u16)> = tui.app.hit.iter().filter(|h| h.1 + h.3 <= w - conv_w).map(|h| (h.1, h.2)).collect();
    let mut ys: Vec<u16> = tiles.iter().map(|t| t.1).collect();
    ys.sort();
    ys.dedup();
    let first = tiles.iter().filter(|t| t.1 == ys[0]).count();
    (ys.len(), first)
}

/// T-37: top-level agents only, the grid fitting the count up to 16 (1, 2×2, 3×3, 3×4, 4×4),
/// the 17th on page 2, with and without the picked agent's conversation beside the grid.
#[test]
fn t37_the_grid_fits_the_count() {
    let t = tempfile::tempdir().unwrap();
    // The Claude fixture's default run starts native sub-agents ("child task", "grandchild task").
    let d = Daemon::start(&[("OVERSEER_CLAUDE_PATH", &fixture("claude-fixture.js"))]);
    let repo = repo(&t.path().join("fits"));
    let parent = d.ctl("task.create", json!({ "repo": repo, "harness": "claude", "prompt": "Delegate to a sub-agent", "title": "Delegates to sub-agents" }))["run"]["id"].as_str().unwrap().to_string();
    d.wait_status(&parent, |s| s == "completed", 20);
    let mut tui = Tui::attach(&d, 200, 60);
    tui.until(15, |a| a.state.runs.iter().filter(|r| r.parent_run_id.is_some()).count() >= 1 && a.visible().len() == 1);
    let mut made = 1;
    for (n, want) in [(1usize, (1usize, 1usize)), (4, (2, 2)), (7, (3, 3)), (12, (3, 4)), (16, (4, 4)), (17, (4, 4))] {
        while made < n {
            made += 1;
            d.sh(&repo, &format!("agent {made:02}"), &format!("echo I am agent {made:02}"));
        }
        tui.until(15, |a| a.visible().len() == n);
        tui.pump(200);
        assert!(tui.app.page_agents().iter().all(|r| r.parent_run_id.is_none()), "sub-agents never get a tile");
        assert!(tui.app.state.runs.len() > n, "the sub-agents are in the daemon's state");
        tui.app.picked = false;
        assert_eq!(drawn_shape(&mut tui), want, "{n} agents");
        let s = tui.screen();
        // A tile's title follows its slot number ("2 ✓ child task"); the parent's conversation may name it.
        let tile_titled = |l: &str| l.match_indices("child task").any(|(at, _)| l[..at].chars().rev().nth(3).is_some_and(|c| c.is_ascii_digit()));
        assert!(!s.lines().any(tile_titled), "no tile for a sub-agent:\n{s}");
        assert!(s.contains(&format!("page 1/{}", if n > 16 { 2 } else { 1 })), "{s}");
        no_line_wider(&s, 200);
        tui.snapshot(&format!("parity-t37-{n:02}"));
        // With the picked agent's conversation beside the grid: the same shape, narrower.
        tui.key(KeyCode::Char('J'));
        assert!(tui.app.picked);
        assert_eq!(drawn_shape(&mut tui), want, "{n} agents beside a conversation");
        no_line_wider(&tui.screen(), 200);
        tui.snapshot(&format!("parity-t37-{n:02}-conversation"));
        tui.key(KeyCode::Esc);
    }
    // The 17th agent (the oldest) is alone on page 2.
    tui.key(KeyCode::Char(']'));
    let s = tui.screen();
    assert!(s.contains("page 2/2"), "{s}");
    assert_eq!(tui.app.page_agents().len(), 1);
    assert_eq!(tui.app.page_agents()[0].id, parent, "the oldest agent is on page 2");
    assert_eq!(drawn_shape(&mut tui), (1, 1));
    tui.snapshot("parity-t37-17-page-2");
    // Arrow keys follow the shape: on page 1's 4×4, down moves four agents on.
    tui.key(KeyCode::Char('['));
    tui.key(KeyCode::Char('1'));
    let first = tui.app.focus.clone();
    tui.key(KeyCode::Down);
    let ids: Vec<String> = tui.app.visible().iter().map(|r| r.id.clone()).collect();
    assert_eq!(tui.app.focus.as_deref(), Some(ids[4].as_str()), "down from tile 1 is tile 5 (was {first:?})");
    tui.key(KeyCode::Right);
    assert_eq!(tui.app.focus.as_deref(), Some(ids[5].as_str()));
}
