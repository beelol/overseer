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

/// T-38: `g` switches between the grid and the focused agent's full view and back, on the same
/// agent, from a tile and from the list.
#[test]
fn t38_one_key_between_the_grid_and_one_agent() {
    let t = tempfile::tempdir().unwrap();
    let d = Daemon::start(&[]);
    let repo = repo(&t.path().join("one"));
    for i in 1..=9 {
        // Forty lines each: a tile shows its tail, the full view the whole conversation.
        d.sh(&repo, &format!("agent {i:02}"), &format!("for n in $(seq 1 40); do echo agent {i:02} line $n; done"));
    }
    let mut tui = Tui::attach(&d, 160, 60);
    tui.until(10, |a| a.visible().len() == 9);
    tui.until_screen(10, "agent 01 line 40");
    // From a tile: the fourth.
    tui.key(KeyCode::Char('4'));
    let fourth = tui.app.focus.clone().unwrap();
    assert_eq!(tui.app.state.run(&fourth).unwrap().title, "agent 06");
    let grid = tui.screen();
    assert!(!grid.contains("agent 06 line 1\n") && !grid.contains("agent 06 line 1 "), "the tile shows only the tail");
    tui.key(KeyCode::Char('g'));
    assert!(matches!(tui.app.mode, Mode::Zoom { .. }));
    tui.key(KeyCode::Home);
    let s = tui.screen();
    assert!(s.contains("agent 06 line 1 ") && s.contains("agent 06 line 30"), "its whole conversation from the top:\n{s}");
    assert!(!s.contains("agent 05 line"), "only that agent:\n{s}");
    tui.snapshot("parity-t38-full-view");
    tui.key(KeyCode::Char('g'));
    assert_eq!(tui.app.mode, Mode::Grid);
    assert_eq!(tui.app.focus.as_deref(), Some(fourth.as_str()), "back on the grid with the fourth focused");
    let s = tui.screen();
    assert!(s.contains("┏ 4 ✓ agent 06"), "the fourth tile is the focused one:\n{s}");
    // From the list: J picks the first agent in the list; g shows it, g returns.
    tui.key(KeyCode::Char('J'));
    let picked = tui.app.focus.clone().unwrap();
    assert_eq!(picked, tui.app.list_ids()[0]);
    tui.key(KeyCode::Char('g'));
    assert!(matches!(tui.app.mode, Mode::Zoom { .. }));
    let title = tui.app.state.run(&picked).unwrap().title.clone();
    assert!(tui.screen().contains(&format!("{title} line 40")));
    tui.key(KeyCode::Char('g'));
    assert_eq!((tui.app.mode.clone(), tui.app.focus.clone()), (Mode::Grid, Some(picked)));
    // `?` names the key.
    tui.key(KeyCode::Char('?'));
    let s = tui.screen();
    assert!(s.contains("g  z") && s.contains("one agent's full view"), "{s}");
    tui.key(KeyCode::Esc);
    assert_eq!(tui.app.mode, Mode::Grid);
}

/// A repository with five files: a.txt has ten numbered lines, b.txt to e.txt one line each.
fn five_files(dir: &Path) -> std::path::PathBuf {
    let r = repo(dir);
    let ten: String = (1..=10).map(|n| format!("line {n}\n")).collect();
    std::fs::write(r.join("a.txt"), ten).unwrap();
    for f in ["b", "c", "d"] {
        std::fs::write(r.join(format!("{f}.txt")), format!("{f} one\n")).unwrap();
    }
    let git = |args: &[&str]| assert!(std::process::Command::new("git").args(args).current_dir(&r).status().unwrap().success());
    git(&["add", "."]);
    git(&["commit", "-q", "-m", "five files"]);
    r
}

/// The agent's edits: line 2 and line 8 of a.txt (two changes), one line added to b.txt.
const EDITS: &str = "printf 'line 1\\nline TWO\\nline 3\\nline 4\\nline 5\\nline 6\\nline 7\\nline EIGHT\\nline 9\\nline 10\\n' > a.txt; printf 'b one\\nb two\\n' > b.txt; echo edited";

fn worktree(d: &Daemon, run: &str) -> std::path::PathBuf {
    let ws = d.run(run)["workspace_id"].as_str().unwrap().to_string();
    let st = d.ctl("state", json!({}));
    let w = st["workspaces"].as_array().unwrap().iter().find(|w| w["id"] == ws.as_str()).unwrap().clone();
    std::path::PathBuf::from(w["path"].as_str().unwrap())
}

fn open_review(tui: &mut Tui, run: &str) {
    let i = tui.app.visible().iter().position(|r| r.id == run).unwrap();
    for _ in 0..20 {
        if tui.app.focus.as_deref() == Some(run) {
            break;
        }
        tui.key(KeyCode::Tab);
    }
    assert_eq!(tui.app.focus.as_deref(), Some(run), "focused agent {i}");
    tui.key(KeyCode::Char('v'));
    assert_eq!(tui.app.mode, Mode::Changes);
    tui.until(10, |a| !a.changes.loading && !a.changes.files.is_empty() && !a.changes.diff.is_empty());
}

/// T-27: the review opens on the daemon's default comparison; 1, 2, 3 switch to Since task start,
/// Latest run and Entire worktree, and the header names what is shown with its counts.
#[test]
fn t27_the_review_opens_on_since_task_start() {
    let t = tempfile::tempdir().unwrap();
    let d = Daemon::start(&[]);
    let r = five_files(&t.path().join("compare"));
    let wt = d.sh(&r, "Edits in its worktree", EDITS);
    d.wait_status(&wt, |s| s == "completed", 20);
    let here = d.ctl("task.create", json!({ "repo": r, "harness": "generic", "program": "/bin/sh", "args": ["-c", EDITS], "prompt": "", "title": "Edits in the checkout", "workspace_mode": "current" }))["run"]["id"].as_str().unwrap().to_string();
    d.wait_status(&here, |s| s == "completed", 20);
    let mut tui = Tui::attach(&d, 160, 44);
    tui.until(10, |a| a.visible().len() == 2);

    // In its own worktree, finished: Since task start.
    open_review(&mut tui, &wt);
    let s = tui.screen();
    assert!(s.contains("review · Edits in its worktree · Since task start · 2 files +3 −2"), "{s}");
    assert!(s.contains("1 [Since task start]"), "{s}");
    tui.snapshot("parity-t27-since-task-start");
    tui.key(KeyCode::Char('2'));
    tui.until(10, |a| !a.changes.loading && a.changes.comparison().is_some_and(|c| c.mode == "latest_run") && !a.changes.files.is_empty());
    let s = tui.screen();
    assert!(s.contains("review · Edits in its worktree · Latest run · 2 files +3 −2") && s.contains("2 [Latest run]"), "{s}");
    tui.snapshot("parity-t27-latest-run");
    tui.key(KeyCode::Char('1'));
    tui.until(10, |a| !a.changes.loading && a.changes.comparison().is_some_and(|c| c.mode == "task_start") && !a.changes.files.is_empty());
    assert!(tui.screen().contains("· Since task start · 2 files +3 −2"));
    // Entire worktree: shown, or it says why it is not (a daemon before AC-263 does not offer it).
    tui.key(KeyCode::Char('3'));
    tui.until(10, |a| !a.changes.loading);
    let s = tui.screen();
    let offered = tui.app.changes.options.iter().any(|c| c.mode == "entire_worktree" && c.available);
    if offered {
        tui.until(10, |a| !a.changes.loading && a.changes.comparison().is_some_and(|c| c.mode == "entire_worktree") && !a.changes.files.is_empty());
        assert!(tui.screen().contains("· Entire worktree · 2 files"), "{}", tui.screen());
    } else {
        assert!(s.contains("Entire worktree is not available") && s.contains("· Since task start ·"), "says why and keeps what was shown:\n{s}");
    }
    tui.snapshot("parity-t27-entire-worktree");
    // c still cycles through every available comparison.
    let before = tui.app.changes.option;
    tui.key(KeyCode::Char('c'));
    tui.until(10, |a| !a.changes.loading);
    assert_ne!(tui.app.changes.option, before);
    tui.key(KeyCode::Esc);

    // In the owner's checkout: the daemon's default (Since task start once AC-263 is on main).
    open_review(&mut tui, &here);
    let default = d.ctl("comparison.options", json!({ "run_id": here }))["options"].as_array().unwrap().iter().find(|o| o["default"] == true).map(|o| o["label"].as_str().unwrap().to_string()).unwrap();
    let s = tui.screen();
    assert!(s.contains(&format!("review · Edits in the checkout · {default} · 2 files")), "opens on the daemon's default {default}:\n{s}");
    tui.snapshot("parity-t27-checkout");
}

/// T-28: `t` switches between the changed files and every file of the worktree; changed files
/// keep their counts; an unchanged file's contents show read-only.
#[test]
fn t28_changed_or_all_files() {
    let t = tempfile::tempdir().unwrap();
    let d = Daemon::start(&[]);
    let r = five_files(&t.path().join("all"));
    let run = d.sh(&r, "Edits two of five", EDITS);
    d.wait_status(&run, |s| s == "completed", 20);
    let mut tui = Tui::attach(&d, 160, 44);
    tui.until(10, |a| a.visible().len() == 1);
    open_review(&mut tui, &run);
    assert_eq!(tui.app.changes.shown().len(), 2);
    assert!(tui.screen().contains("[Changed] | All files"));
    tui.key(KeyCode::Char('t'));
    tui.until(10, |a| a.changes.all_files && a.changes.all_loading == 0 && !a.changes.loading);
    let names: Vec<String> = tui.app.changes.shown().iter().map(|f| f.1.clone()).collect();
    assert_eq!(names, ["README.md", "a.txt", "b.txt", "c.txt", "d.txt"], "the whole worktree");
    let s = tui.screen();
    assert!(s.contains("Changed | [All files]"), "{s}");
    assert!(s.contains("M a.txt +2 −2") && s.contains("M b.txt +1 −0"), "changed files keep their counts:\n{s}");
    // The first file, README.md, is unchanged: its contents, read-only.
    assert!(s.contains("README.md  unchanged · read-only") && s.contains("# fixture"), "{s}");
    tui.snapshot("parity-t28-all-files");
    tui.key(KeyCode::Char('j'));
    tui.key(KeyCode::Char('j'));
    tui.key(KeyCode::Char('j'));
    let s = tui.until_screen(10, "c.txt  unchanged · read-only");
    assert!(s.contains("c one"), "{s}");
    tui.key(KeyCode::Char('t'));
    tui.until(10, |a| !a.changes.all_files && !a.changes.loading);
    assert_eq!(tui.app.changes.shown().len(), 2);
    assert!(tui.screen().contains("[Changed] | All files"));
    tui.snapshot("parity-t28-changed");
}

/// T-29: Accept and Reject per change and per file through the daemon, with its conflict check;
/// the marks are the daemon's (what VS Code's review reads), both ways; no Keep, Undo or Save.
#[test]
fn t29_accept_and_reject_in_the_terminal() {
    let t = tempfile::tempdir().unwrap();
    let d = Daemon::start(&[]);
    let r = five_files(&t.path().join("review"));
    let go = t.path().join("go");
    // The agent edits, then (on a signal) edits line 2 again.
    let script = format!("{EDITS}; while [ ! -f '{}' ]; do sleep 0.1; done; printf 'line 1\\nline 2 AGAIN\\nline 3\\nline 4\\nline 5\\nline 6\\nline 7\\nline EIGHT\\nline 9\\nline 10\\n' > a.txt; echo again; sleep 30", go.display());
    let run = d.sh(&r, "Reviewed in the terminal", &script);
    let wt_path = {
        let end = std::time::Instant::now() + std::time::Duration::from_secs(20);
        loop {
            let p = worktree(&d, &run);
            if std::fs::read_to_string(p.join("b.txt")).unwrap_or_default().contains("b two") || std::time::Instant::now() > end {
                break p;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    };
    let mut tui = Tui::attach(&d, 160, 44);
    tui.until(10, |a| a.visible().len() == 1);
    open_review(&mut tui, &run);
    assert_eq!(tui.app.changes.selected().unwrap().1, "a.txt");
    assert_eq!(tui.app.changes.hunks.len(), 2, "two changes in a.txt");
    let s = tui.screen();
    assert!(s.contains("a.txt  change 1 of 2 · 0 accepted") && s.contains("-line 2") && s.contains("+line TWO"), "{s}");

    // n/p move between changes; a accepts the one under the cursor.
    tui.key(KeyCode::Char('n'));
    assert_eq!(tui.app.changes.change, 1);
    let second = tui.app.changes.hunks[1].key.clone();
    tui.key(KeyCode::Char('a'));
    tui.until(10, |a| a.changes.hunks.iter().any(|h| h.key == second && h.reviewed));
    let marks = d.ctl("review.marks", json!({ "run_id": run }));
    assert!(marks["keys"].as_array().unwrap().iter().any(|k| k == second.as_str()), "accepted in the daemon's marks, which VS Code's review reads: {marks}");
    let s = tui.until_screen(5, "1 accepted");
    assert!(s.contains("✓ Accepted"), "{s}");
    tui.snapshot("parity-t29-accepted");
    tui.key(KeyCode::Char('p'));
    assert_eq!(tui.app.changes.change, 0);

    // The fixture agent edits line 2 again before Reject: refused with the daemon's reason.
    std::fs::write(&go, "").unwrap();
    let end = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !std::fs::read_to_string(wt_path.join("a.txt")).unwrap().contains("AGAIN") {
        assert!(std::time::Instant::now() < end, "the agent edits again");
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    tui.key(KeyCode::Char('r'));
    let s = tui.screen();
    assert!(s.contains("Reject this change in a.txt? 1 line goes back to what was there before; 1 line the agent wrote is removed. y / n"), "{s}");
    tui.snapshot("parity-t29-reject-asks");
    tui.key(KeyCode::Char('y'));
    let s = tui.until_screen(10, "changed while you were rejecting");
    assert!(s.contains("conflict"), "{s}");
    assert!(std::fs::read_to_string(wt_path.join("a.txt")).unwrap().contains("line 2 AGAIN"), "nothing was written");
    tui.snapshot("parity-t29-conflict");

    // Reloaded, Reject on the current text puts back the comparison's line 2.
    tui.until(10, |a| !a.changes.loading && a.changes.hunks.iter().any(|h| h.modified_lines == ["line 2 AGAIN"]));
    tui.key(KeyCode::Char('1'));
    tui.until(10, |a| !a.changes.loading && !a.changes.hunks.is_empty());
    let at = tui.app.changes.hunks.iter().position(|h| h.modified_lines == ["line 2 AGAIN"]).unwrap();
    while tui.app.changes.change != at {
        tui.key(KeyCode::Char('n'));
    }
    tui.key(KeyCode::Char('r'));
    tui.key(KeyCode::Char('n'));
    assert_eq!(tui.app.mode, Mode::Changes, "n keeps everything");
    assert!(std::fs::read_to_string(wt_path.join("a.txt")).unwrap().contains("line 2 AGAIN"));
    tui.key(KeyCode::Char('r'));
    tui.key(KeyCode::Char('y'));
    let end = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !std::fs::read_to_string(wt_path.join("a.txt")).unwrap().contains("line 2\n") {
        assert!(std::time::Instant::now() < end, "the base line is back:\n{}", tui.screen());
        tui.pump(50);
    }
    tui.until(10, |a| !a.changes.loading && a.changes.hunks.len() == 1);

    // A mark made elsewhere (as VS Code does) shows here: the whole-file accept of b.txt through ctl.
    tui.key(KeyCode::Char('j'));
    tui.until(10, |a| !a.changes.loading && a.changes.selected().is_some_and(|f| f.1 == "b.txt") && !a.changes.hunks.is_empty());
    let h = tui.app.changes.hunks[0].clone();
    d.ctl("review.accept", json!({ "run_id": run, "path": "b.txt", "key": h.key, "modified_start": h.modified_start, "modified_lines": h.modified_lines, "base_lines": h.base_lines }));
    tui.until(10, |a| a.changes.hunks.first().is_some_and(|h| h.reviewed));
    tui.pump(100);
    assert!(tui.screen().contains("b.txt  change 1 of 1 · 1 accepted"));

    // A whole file: R rejects every change of b.txt, then the file is as before.
    tui.key(KeyCode::Char('R'));
    assert!(tui.screen().contains("Reject all 1 change in b.txt?"), "{}", tui.screen());
    tui.key(KeyCode::Char('y'));
    let end = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while std::fs::read_to_string(wt_path.join("b.txt")).unwrap() != "b one\n" {
        assert!(std::time::Instant::now() < end, "b.txt is back");
        tui.pump(50);
    }
    tui.until(10, |a| !a.changes.loading && a.changes.files.len() == 1);
    // And A accepts every change of a file: a.txt's remaining one.
    tui.until(10, |a| a.changes.selected().is_some_and(|f| f.1 == "a.txt") && !a.changes.hunks.is_empty());
    tui.key(KeyCode::Char('A'));
    tui.until(10, |a| !a.changes.hunks.is_empty() && a.changes.hunks.iter().all(|h| h.reviewed));
    let marks = d.ctl("review.marks", json!({ "run_id": run }));
    assert!(tui.app.changes.hunks.iter().all(|h| marks["keys"].as_array().unwrap().iter().any(|k| k == h.key.as_str())), "{marks}");
    let s = tui.screen();
    tui.snapshot("parity-t29-file-accepted");
    // The words are Accept and Reject, never Keep, Undo or Save.
    tui.key(KeyCode::Char('?'));
    let help = tui.screen();
    for screen in [&s, &help] {
        for w in ["Keep", "Undo", "Save"] {
            assert!(!screen.contains(w), "{w} on the review:\n{screen}");
        }
    }
    let src = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/ui.rs")).unwrap();
    let review = &src[src.find("fn changes(").unwrap()..src.find("const REVIEW_MODES").unwrap()];
    for w in ["Keep", "Undo", "Save"] {
        assert!(!review.contains(w), "{w} in the review's code");
    }
    d.ctl("run.interrupt", json!({ "run_id": run }));
}

/// An editor for the tests: says whether the terminal was handed over in its normal (cooked)
/// mode, then appends the owner's line to the file it was given (`+LINE PATH`).
fn fake_editor(dir: &Path) -> String {
    let p = dir.join("editor.sh");
    std::fs::write(&p, "#!/bin/sh\necho \"EDITOR-RAN $(stty -a < /dev/tty 2>/dev/null | grep -o -- '-*icanon' | head -1) $1\"\necho 'written by the owner' >> \"$2\"\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    p.display().to_string()
}

/// T-39: `e` in the review opens the file at the change in `$EDITOR`; back in the review the
/// owner's line shows as theirs; the real binary hands the terminal over and takes it back.
#[test]
fn t39_edit_in_your_own_editor() {
    let t = tempfile::tempdir().unwrap();
    let d = Daemon::start(&[]);
    let r = five_files(&t.path().join("edit"));
    let run = d.sh(&r, "Edited by the owner", EDITS);
    d.wait_status(&run, |s| s == "completed", 20);
    let editor = fake_editor(t.path());
    std::env::set_var("EDITOR", &editor);
    let mut tui = Tui::attach(&d, 160, 44);
    tui.until(10, |a| a.visible().len() == 1);
    open_review(&mut tui, &run);
    tui.key(KeyCode::Char('n'));
    tui.key(KeyCode::Char('e'));
    let exec = tui.app.exec.take().expect("e asks the event loop to run the editor");
    let file = worktree(&d, &run).join("a.txt");
    assert_eq!((exec.program.as_str(), exec.args.clone()), (editor.as_str(), vec!["+8".to_string(), file.display().to_string()]), "at the change (line 8)");
    // What the event loop does: run it with the terminal, then come back.
    let status = std::process::Command::new(&exec.program).args(&exec.args).stdout(std::process::Stdio::null()).status().unwrap();
    tui.app.after_exec(Ok(status.code().unwrap_or(-1)));
    let s = tui.until_screen(10, "written by the owner");
    assert!(s.lines().any(|l| l.contains("+written by the owner") && l.contains("✎ your edit")), "the owner's line as theirs:\n{s}");
    assert!(!s.lines().any(|l| l.contains("+line EIGHT") && l.contains("your edit")), "the agent's lines stay the agent's:\n{s}");
    tui.snapshot("parity-t39-your-edit");
    drop(tui);

    // The real binary in a terminal: v, e, the editor runs in a cooked terminal, the TUI comes back.
    let bin = env!("CARGO_BIN_EXE_overseer-tui");
    let helper = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/pty_run.py");
    let out = std::process::Command::new("python3").arg(helper).args(["40", "140", "2.0", "v", "1.5", "e", "2.5", "q", "0.8", "q", "--", bin, "--daemon"]).arg(&d.bin).arg("--home").arg(d.home.path())
        .env("TERM", "xterm-256color").env("EDITOR", &editor).stdin(std::process::Stdio::null()).output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    assert!(out.status.success(), "exits cleanly:\n{text}");
    let ran = text.find("EDITOR-RAN").expect("the editor ran");
    assert!(text[ran..].starts_with("EDITOR-RAN icanon +1") || text[ran..].starts_with("EDITOR-RAN icanon +"), "the terminal was handed over cooked, not raw: {:?}", &text[ran..ran + 30]);
    let before = &text[..ran];
    assert!(before.rfind("\u{1b}[?1049l") > before.rfind("\u{1b}[?1049h"), "the alternate screen was left before the editor");
    let after = &text[ran..];
    assert!(after.contains("\u{1b}[?1049h"), "the TUI came back after the editor");
    assert!(after.contains("your edit"), "the review shows the owner's edit again");
    assert!(after.rfind("\u{1b}[?1049l") > after.rfind("\u{1b}[?1049h"), "and leaves the terminal as it found it on quit");
    assert_eq!(std::fs::read_to_string(&file).unwrap().matches("written by the owner").count(), 2);
}
