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
    // The header: VS Code's five counts (T-26), zero counts left out.
    let s = tui.screen();
    assert!(s.contains("● 2 working · ◆ 2 need you · ✦ 14 to review · 1 reviewed · ✗ 1 failed"), "{s}");

    // Grouped by repository, the most recently active first, each heading with its counts.
    let groups: Vec<String> = tui.app.groups().iter().map(|g| g.0.rsplit('/').next().unwrap().to_string()).collect();
    assert_eq!(groups, ["web-app", "api-server", "docs-site"], "web-app is working now; api-server finished after docs-site");
    for (w, h, name) in [(200u16, 60u16, "parity-t25-list-200x60"), (140, 40, "parity-t25-list-140x40"), (100, 30, "parity-t25-list-100x30")] {
        tui.resize(w, h);
        let s = tui.screen();
        assert!(s.contains(" agents "), "the list at {w}×{h}:\n{s}");
        assert!(s.contains("web-app  2 working · 2 needs you") || (s.contains(" web-app ") && s.contains("   2 working · 2 needs you")), "web-app's counts at {w}×{h}:\n{s}");
        assert!(s.contains("api-server  6 to review · 1 failed") || s.contains("   6 to review · 1 failed"), "api-server's counts at {w}×{h}:\n{s}");
        assert!(s.contains("docs-site  4 to review · 1 merged") || s.contains("   4 to review · 1 merged"), "docs-site's counts at {w}×{h}:\n{s}");
        // Each row: status, title, account, mark.
        assert!(s.lines().any(|l| l.contains("◆ Tidy the") && l.contains("Claude Max ◆")), "the Mac's login and the needs-you mark at {w}×{h}:\n{s}");
        assert!(s.lines().any(|l| l.contains("◆ Write the") && l.contains("Personal ◆")), "the named account at {w}×{h}:\n{s}");
        // (At 100×30 the list is longer than the screen and ends before docs-site's last row.)
        assert!(w == 100 || s.lines().any(|l| l.contains("✓ Add the") && l.contains(" ✓ │")), "the merged mark (merging marks it reviewed) at {w}×{h}:\n{s}");
        assert!(s.lines().any(|l| l.contains("✓ Endpoint") && l.contains(" ✦ │")), "the to-review mark at {w}×{h}:\n{s}");
        assert!(s.lines().any(|l| l.contains("✗ Migration") && l.contains(" ✦ │")), "a failed agent is to review too at {w}×{h}:\n{s}");
        assert!(s.contains("◆ needs you ✦ to review ✓ merged") || s.contains("◆ you ✦ review ✓ merged"), "the legend at {w}×{h}:\n{s}");
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
    assert!(s.contains("docs-site  4 to review · 1 merged") && !s.contains("web-app") && !s.contains("api-server"), "{s}");
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

/// VS Code's rollup (extension/media/rollup.js, the side bar's and the grid's counts) over the
/// daemon's state and the daemon's reviewed marks, as "working needs to-review reviewed failed".
fn vscode_counts(d: &Daemon) -> String {
    let state = d.ctl("state", json!({}));
    let rollup = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().join("extension/media/rollup.js");
    let script = format!(
        "const R = require({:?}); const s = JSON.parse(require('fs').readFileSync(0, 'utf8')); const c = R.counts(s, s.reviewed || {{}}); process.stdout.write([c.working, c.needs, c.unreviewed, c.reviewed, c.failed].join(' '));",
        rollup.display().to_string()
    );
    let mut child = std::process::Command::new("node").args(["-e", &script]).stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).spawn().expect("node");
    use std::io::Write;
    child.stdin.take().unwrap().write_all(state.to_string().as_bytes()).unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    String::from_utf8(out.stdout).unwrap()
}

fn tui_counts(tui: &Tui) -> String {
    let c = tui.app.state.counts(overseer_tui::model::now_ms());
    format!("{} {} {} {} {}", c.working, c.needs, c.to_review, c.reviewed, c.failed)
}

/// T-26: six finished agents in two repositories. The TUI's header gives VS Code's five counts
/// (rollup.js over the same state and the daemon's reviewed marks); opening one agent's review in
/// the TUI clears only that mark, in the daemon (where VS Code and the menu bar read it); a mark
/// sent the way VS Code sends it (`review.seen`) clears in the TUI within 2 s; merging clears one.
#[test]
fn t26_the_same_counts_and_the_same_unreviewed_marks() {
    let t = tempfile::tempdir().unwrap();
    let d = Daemon::start(&[]);
    let app = repo(&t.path().join("app"));
    let site = repo(&t.path().join("site"));
    let mut done = Vec::new();
    for i in 1..=3 {
        done.push(d.sh(&app, &format!("App task {i}"), &format!("echo output of app task {i}")));
    }
    let failed = d.sh(&app, "App check", "echo 'error: check failed' 1>&2; exit 1");
    let edits = d.sh(&site, "Site page", "printf 'page\\n' > page.txt; echo wrote the page");
    let other = d.sh(&site, "Site notes", "echo output of site notes");
    for r in done.iter().chain([&edits, &other]) {
        d.wait_status(r, |s| s == "completed", 20);
    }
    d.wait_status(&failed, |s| s == "failed", 20);

    let mut tui = Tui::attach(&d, 200, 50);
    tui.until(10, |a| a.visible().len() == 6 && a.state.counts(overseer_tui::model::now_ms()).to_review == 5);
    assert_eq!(tui_counts(&tui), "0 0 5 0 1");
    assert_eq!(vscode_counts(&d), tui_counts(&tui), "the TUI counts as VS Code does");
    let s = tui.screen();
    assert!(s.lines().next().unwrap().contains("✦ 5 to review · ✗ 1 failed"), "{s}");
    assert!(s.contains("app  4 to review · 1 failed") || s.contains("app  3 to review · 1 failed"), "{s}");
    assert!(s.contains("site  2 to review"), "{s}");
    assert_eq!(d.ctl("menubar.snapshot", json!({}))["counts"]["review"], 6, "the menu bar's to review (failed included)");
    tui.snapshot("parity-t26-counts");

    // Opening one agent's review in the TUI clears only its mark, in the daemon too.
    let first = done[0].clone();
    for _ in 0..20 {
        if tui.app.focus.as_deref() == Some(first.as_str()) {
            break;
        }
        tui.key(KeyCode::Tab);
    }
    assert_eq!(tui.app.focus.as_deref(), Some(first.as_str()));
    tui.key(KeyCode::Char('v'));
    assert_eq!(tui.app.mode, Mode::Changes);
    tui.pump(300);
    tui.key(KeyCode::Esc);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while d.ctl("state", json!({}))["reviewed"].get(&first).is_none() {
        assert!(std::time::Instant::now() < deadline, "the daemon has the TUI's mark");
        tui.pump(50);
    }
    let marks = d.ctl("state", json!({}))["reviewed"].clone();
    assert_eq!(marks.as_object().unwrap().len(), 1, "only that mark: {marks}");
    tui.until(5, |a| a.state.counts(overseer_tui::model::now_ms()).to_review == 4);
    assert_eq!(tui_counts(&tui), "0 0 4 1 1");
    assert_eq!(vscode_counts(&d), tui_counts(&tui), "VS Code's rollup over the daemon's marks agrees");
    assert_eq!(d.ctl("menubar.snapshot", json!({}))["counts"]["review"], 5);
    let s = tui.screen();
    assert!(s.lines().next().unwrap().contains("✦ 4 to review · 1 reviewed · ✗ 1 failed"), "{s}");
    assert!(s.lines().any(|l| l.contains("✓ App task 1") && !l.contains("✦")), "its row lost the mark:\n{s}");
    assert!(s.lines().any(|l| l.contains("✓ App task 2") && l.contains(" ✦ │")), "the others keep it:\n{s}");
    tui.snapshot("parity-t26-one-reviewed");

    // A mark sent as VS Code sends it (extension.js markReviewed) clears in the TUI within 2 s.
    let at = d.run(&failed)["ended_ms"].as_i64().unwrap() + 1;
    let sent = std::time::Instant::now();
    d.ctl("review.seen", json!({ "marks": { failed.clone(): at } }));
    tui.until(2, |a| a.state.counts(overseer_tui::model::now_ms()).failed == 0);
    assert!(sent.elapsed() < std::time::Duration::from_secs(2), "cleared in {:?}", sent.elapsed());
    assert_eq!(tui_counts(&tui), "0 0 4 2 0");
    assert_eq!(vscode_counts(&d), tui_counts(&tui));

    // Merging an agent's work back clears its mark, whoever merges it.
    let ws = d.run(&edits)["workspace_id"].as_str().unwrap().to_string();
    assert_eq!(d.ctl("workspace.merge_prepare", json!({ "workspace_id": ws }))["state"], "ready");
    d.ctl("workspace.merge_complete", json!({ "workspace_id": ws }));
    tui.until(2, |a| a.state.counts(overseer_tui::model::now_ms()).to_review == 3);
    assert_eq!(tui_counts(&tui), "0 0 3 3 0");
    assert_eq!(vscode_counts(&d), tui_counts(&tui));
    assert_eq!(d.ctl("menubar.snapshot", json!({}))["counts"]["review"], 3);
    let s = tui.screen();
    assert!(s.lines().next().unwrap().contains("✦ 3 to review · 3 reviewed") && !s.lines().next().unwrap().contains("failed"), "zero counts left out:\n{s}");
    tui.snapshot("parity-t26-merged");
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
/// Latest run and Entire worktree, and the header names what is shown with its counts, both for an
/// agent in its own worktree and for one in the owner's checkout.
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
    // Entire worktree is offered and shows the worktree's changes.
    assert!(tui.app.changes.options.iter().any(|c| c.mode == "entire_worktree" && c.available), "Entire worktree offered: {:?}", tui.app.changes.options.iter().map(|c| (&c.mode, c.available)).collect::<Vec<_>>());
    tui.key(KeyCode::Char('3'));
    tui.until(10, |a| !a.changes.loading && a.changes.comparison().is_some_and(|c| c.mode == "entire_worktree") && !a.changes.files.is_empty());
    let s = tui.screen();
    assert!(s.contains("review · Edits in its worktree · Entire worktree · 2 files +3 −2") && s.contains("3 [Entire worktree]"), "{s}");
    tui.snapshot("parity-t27-entire-worktree");
    // c still cycles through every available comparison.
    let before = tui.app.changes.option;
    tui.key(KeyCode::Char('c'));
    tui.until(10, |a| !a.changes.loading);
    assert_ne!(tui.app.changes.option, before);
    tui.key(KeyCode::Esc);

    // In the owner's checkout: also Since task start, with Entire worktree offered.
    open_review(&mut tui, &here);
    let opts = d.ctl("comparison.options", json!({ "run_id": here }))["options"].as_array().unwrap().clone();
    assert_eq!(opts.iter().find(|o| o["default"] == true).map(|o| o["mode"].as_str().unwrap()), Some("task_start"), "the daemon's default in the checkout: {opts:?}");
    let s = tui.screen();
    assert!(s.contains("review · Edits in the checkout · Since task start · 2 files +3 −2") && s.contains("1 [Since task start]"), "{s}");
    tui.snapshot("parity-t27-checkout");
    assert!(tui.app.changes.options.iter().any(|c| c.mode == "entire_worktree" && c.available), "Entire worktree offered in the checkout");
    tui.key(KeyCode::Char('3'));
    tui.until(10, |a| !a.changes.loading && a.changes.comparison().is_some_and(|c| c.mode == "entire_worktree") && !a.changes.files.is_empty());
    let s = tui.screen();
    assert!(s.contains("review · Edits in the checkout · Entire worktree · 2 files +3 −2"), "{s}");
    tui.snapshot("parity-t27-checkout-entire-worktree");
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

/// The display column where `needle` first starts on any line of the screen.
fn column_of(s: &str, needle: &str) -> Option<usize> {
    s.lines().find_map(|l| l.find(needle).map(|i| l[..i].width()))
}

/// Dashboard mode's three columns: the list from the left edge, the review in the middle and the
/// conversation on the right, at the widths `ui::dashboard_widths` gives.
fn assert_three_columns(s: &str, w: u16, title: &str) {
    let (list_w, conv_w) = overseer_tui::ui::dashboard_widths(w, true);
    let conv_x = (w - conv_w) as usize;
    assert!(s.lines().nth(1).is_some_and(|l| l.starts_with("┏ agents ") || l.starts_with("╭ agents ")), "the list on the left:\n{s}");
    let review = column_of(s, &format!("review · {title} · Since task start")).unwrap_or_else(|| panic!("the review of {title}:\n{s}"));
    assert!(review > list_w as usize && review < conv_x, "the review in the middle ({review}, between {list_w} and {conv_x}):\n{s}");
    let said = column_of(s, &format!("output of {}", title.to_lowercase())).unwrap_or_else(|| panic!("the conversation of {title}:\n{s}"));
    assert!(said > conv_x, "the conversation on the right ({said}, from {conv_x}):\n{s}");
}

/// T-40: with 9 agents, `D` shows the list, the picked agent's review and its conversation side by
/// side (at 240×70 and 200×60); `J` changes the review and the conversation together; Tab moves
/// the keys between the columns (the review's own keys work in its column); `D` returns to the
/// grid on that agent; below 160 columns it says it needs a wider terminal and stays on the grid;
/// `--dashboard` (the app's `dashboard` flag) starts in it once the terminal is wide enough.
#[test]
fn t40_dashboard_mode_for_big_screens() {
    let t = tempfile::tempdir().unwrap();
    let d = Daemon::start(&[]);
    let r = five_files(&t.path().join("dash"));
    let runs: Vec<String> = (1..=9)
        .map(|i| d.sh(&r, &format!("Dashboard agent {i}"), &format!("printf 'agent {i} was here\\n' >> a.txt; printf 'note {i}\\n' > note{i}.txt; echo output of dashboard agent {i}")))
        .collect();
    for id in &runs {
        d.wait_status(id, |s| s == "completed", 20);
    }
    let mut tui = Tui::attach(&d, 240, 70);
    tui.until(10, |a| a.visible().len() == 9);
    tui.pump(300);
    tui.screen();

    // D: the first agent in the list is picked; its review and its conversation show.
    tui.key(KeyCode::Char('D'));
    assert!(tui.app.dashboard_shown());
    let order = tui.app.list_ids();
    let first = order[0].clone();
    assert_eq!(tui.app.focus.as_deref(), Some(first.as_str()));
    tui.until(10, |a| a.changes.run == first && !a.changes.loading && !a.changes.diff.is_empty());
    let title = tui.app.state.run(&first).unwrap().title.clone();
    let s = tui.until_screen(10, &format!("output of {}", title.to_lowercase()));
    assert!(s.lines().next().unwrap().contains(" dashboard "), "the header names the mode:\n{s}");
    assert!(s.contains(&format!("review · {title} · Since task start · 2 files +2 −0")), "{s}");
    assert_three_columns(&s, 240, &title);
    no_line_wider(&s, 240);
    tui.snapshot("parity-t40-dashboard-240x70");
    tui.resize(200, 60);
    let s = tui.screen();
    assert_three_columns(&s, 200, &title);
    no_line_wider(&s, 200);
    tui.snapshot("parity-t40-dashboard-200x60");

    // J: the next agent in the list; the review and the conversation both change to it.
    tui.key(KeyCode::Char('J'));
    let second = order[1].clone();
    assert_eq!(tui.app.focus.as_deref(), Some(second.as_str()));
    tui.until(10, |a| a.changes.run == second && !a.changes.loading && !a.changes.diff.is_empty());
    let title2 = tui.app.state.run(&second).unwrap().title.clone();
    let s = tui.until_screen(10, &format!("output of {}", title2.to_lowercase()));
    assert_three_columns(&s, 200, &title2);
    assert!(!s.contains(&format!("output of {}", title.to_lowercase())) && !s.contains(&format!("review · {title} ·")), "nothing of the first agent is left in the middle or on the right:\n{s}");
    tui.snapshot("parity-t40-next-agent");
    // K back and J again: the list's order both ways.
    tui.key(KeyCode::Char('K'));
    assert_eq!(tui.app.focus.as_deref(), Some(first.as_str()));
    tui.key(KeyCode::Char('J'));
    tui.until(10, |a| a.changes.run == second && !a.changes.loading && !a.changes.hunks.is_empty());

    // Tab: list → review → conversation → list (Shift-Tab back); the review's keys in its column.
    assert_eq!(tui.app.dash_col, 0);
    tui.key(KeyCode::Tab);
    assert_eq!(tui.app.dash_col, 1);
    let s = tui.screen();
    assert!(s.contains("┏ review · "), "the review column is drawn as focused:\n{s}");
    let key = tui.app.changes.hunks[tui.app.changes.change].key.clone();
    tui.key(KeyCode::Char('a'));
    tui.until(10, |a| a.changes.hunks.iter().any(|h| h.key == key && h.reviewed));
    let marks = d.ctl("review.marks", json!({ "run_id": second }));
    assert!(marks["keys"].as_array().unwrap().iter().any(|k| k == key.as_str()), "accepted through the daemon: {marks}");
    tui.until_screen(5, "✓ Accepted");
    tui.app.notice = None;
    let s = tui.screen();
    assert!(s.contains("Accept change/file") && s.contains("tab conversation"), "the review's keys in the footer:\n{s}");
    tui.snapshot("parity-t40-review-column");
    tui.key(KeyCode::Tab);
    assert_eq!(tui.app.dash_col, 2);
    assert!(tui.screen().contains("tab list"));
    tui.key(KeyCode::Tab);
    assert_eq!(tui.app.dash_col, 0);
    tui.key(KeyCode::BackTab);
    assert_eq!(tui.app.dash_col, 2);
    tui.key(KeyCode::Esc);
    assert_eq!(tui.app.dash_col, 0, "esc gives the keys back to the list");

    // `?` names D and Tab.
    tui.key(KeyCode::Char('?'));
    let s = tui.screen();
    assert!(s.contains("dashboard mode ⇄ grid, same agent") && s.contains("list → review → conversation"), "{s}");
    tui.key(KeyCode::Esc);

    // D: back to the grid on that agent.
    tui.key(KeyCode::Char('D'));
    assert!(!tui.app.dashboard && !tui.app.dashboard_shown());
    assert_eq!(tui.app.mode, Mode::Grid);
    assert_eq!(tui.app.focus.as_deref(), Some(second.as_str()), "the grid on the agent picked in dashboard mode");
    let s = tui.screen();
    assert!(s.lines().next().unwrap().contains("page 1/1") && !s.contains("review · Dashboard"), "{s}");
    assert!(s.lines().any(|l| l.contains("┏") && l.contains(&title2)), "its tile is the focused one:\n{s}");
    tui.snapshot("parity-t40-back-to-grid");

    // Below 160 columns: it says so and stays on the grid.
    tui.resize(159, 50);
    tui.screen();
    tui.key(KeyCode::Char('D'));
    assert!(!tui.app.dashboard && tui.app.mode == Mode::Grid);
    let s = tui.screen();
    assert!(s.contains("Dashboard mode needs a terminal at least 160 columns wide (this one is 159); staying on the grid"), "{s}");
    assert!(!s.contains("review · Dashboard"));
    no_line_wider(&s, 159);
    tui.snapshot("parity-t40-too-narrow");

    // --dashboard in a narrow terminal: the grid with a standing line; widened, dashboard mode.
    let mut start = Tui::attach(&d, 150, 44);
    start.app.dashboard = true;
    start.until(10, |a| a.visible().len() == 9);
    let s = start.screen();
    assert!(s.contains("Dashboard mode needs a terminal at least 160 columns wide (this one is 150); showing the grid"), "{s}");
    assert!(!start.app.dashboard_shown());
    start.resize(200, 60);
    start.screen();
    start.until(10, |a| a.dashboard_shown() && a.focus.is_some() && a.changes.run == *a.focus.as_ref().unwrap() && !a.changes.loading && !a.changes.diff.is_empty());
    let picked = start.app.focus.clone().unwrap();
    let t1 = start.app.state.run(&picked).unwrap().title.clone();
    let s = start.until_screen(10, &format!("output of {}", t1.to_lowercase()));
    assert_three_columns(&s, 200, &t1);
}

/// T-40 and T-41 in the real binary: `--help` names both options and the keys; `--dashboard` and
/// `--grid` together are refused.
#[test]
fn t40_t41_options_in_help() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_overseer-tui")).arg("--help").output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    for s in ["--dashboard", "--grid", "Show only the grid of agents", "dashboard mode <-> the grid", "ctrl+o devices"] {
        assert!(text.contains(s), "--help lacks {s}:\n{text}");
    }
    let both = std::process::Command::new(env!("CARGO_BIN_EXE_overseer-tui")).args(["--dashboard", "--grid"]).output().unwrap();
    assert!(!both.status.success() && String::from_utf8_lossy(&both.stderr).contains("pick one per terminal"));
}

fn claude_daemon(mode: &str) -> Daemon {
    let claude = fixture("claude-fixture.js");
    Daemon::start(&[("OVERSEER_CLAUDE_PATH", &claude), ("CLAUDE_FIXTURE_MODE", mode), ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "CLAUDE_FIXTURE_MODE")])
}

/// T-41: two TUIs on one daemon, one `--grid` (only the grid) and one `--dashboard`. Picking in
/// one does not move the other; a new fixture agent appears in both; its permission answered in
/// the grid-only terminal clears in the dashboard.
#[test]
fn t41_a_grid_only_terminal_beside_it() {
    let t = tempfile::tempdir().unwrap();
    let d = claude_daemon("permission");
    let r = five_files(&t.path().join("two"));
    for i in 1..=3 {
        let id = d.sh(&r, &format!("Shared agent {i}"), &format!("printf 'agent {i}\\n' >> a.txt; echo output of shared agent {i}"));
        d.wait_status(&id, |s| s == "completed", 20);
    }
    let mut grid = Tui::attach(&d, 160, 45);
    grid.app.grid_only = true;
    let mut dash = Tui::attach(&d, 240, 70);
    dash.app.dashboard = true;
    grid.until(10, |a| a.visible().len() == 3);
    dash.until(10, |a| a.visible().len() == 3);
    dash.screen();
    dash.until(10, |a| a.dashboard_shown() && a.focus.is_some() && a.changes.run == *a.focus.as_ref().unwrap() && !a.changes.loading);

    // The grid-only terminal: the grid and nothing else.
    let s = grid.screen();
    assert!(s.lines().next().unwrap().contains("grid only · page 1/1"), "{s}");
    assert!(!s.contains("╭ agents ") && !s.contains("┏ agents ") && !s.contains("review · Shared"), "no list, no review:\n{s}");
    grid.key(KeyCode::Char('J'));
    assert!(!grid.app.picked, "no conversation column opens");
    for k in ['L', 'D'] {
        grid.key(KeyCode::Char(k));
        assert!(grid.screen().contains("This terminal shows the grid only (--grid)"), "{k} in the grid-only terminal");
    }
    assert!(!grid.app.dashboard && !grid.app.list_shown());

    // Picking in one does not move the other.
    let dash_focus = dash.app.focus.clone();
    let before = grid.app.focus.clone();
    grid.key(KeyCode::Tab);
    grid.key(KeyCode::Tab);
    assert_ne!(grid.app.focus, before);
    dash.pump(200);
    assert_eq!(dash.app.focus, dash_focus, "the dashboard keeps its agent");
    let grid_focus = grid.app.focus.clone();
    dash.key(KeyCode::Char('J'));
    assert_ne!(dash.app.focus, dash_focus);
    grid.pump(200);
    assert_eq!(grid.app.focus, grid_focus, "the grid keeps its agent");

    // A new fixture agent appears in both; it waits for a permission.
    let asks = d.ctl("task.create", json!({ "repo": r, "harness": "claude", "prompt": "Write the changelog", "title": "Write the changelog" }))["run"]["id"].as_str().unwrap().to_string();
    d.wait_status(&asks, |s| s == "waiting_for_user", 20);
    grid.until(10, |a| a.visible().len() == 4 && a.state.run(&asks).is_some_and(|r| r.needs_you()));
    dash.until(10, |a| a.visible().len() == 4 && a.state.run(&asks).is_some_and(|r| r.needs_you()));
    dash.until(10, |a| !a.changes.loading && !a.changes.files.is_empty());
    let g = grid.until_screen(10, "Write the changelog");
    let s = dash.until_screen(10, "Write the changelog");
    assert!(g.lines().next().unwrap().contains("1 needs you") && s.lines().next().unwrap().contains("1 needs you"), "both count it:\n{g}\n{s}");
    assert!(s.lines().any(|l| l.contains("◆ Write the") && (l.contains(" ◆ │") || l.contains(" ◆ ┃"))), "the dashboard's list marks it:\n{s}");
    assert_eq!(grid.app.focus, grid_focus, "a new agent does not take the grid's focus");
    no_line_wider(&g, 160);
    no_line_wider(&s, 240);
    grid.snapshot("parity-t41-grid-only-waiting");
    dash.snapshot("parity-t41-dashboard-waiting");

    // Answered in the grid-only terminal: it clears in the dashboard.
    for _ in 0..8 {
        if grid.app.focus.as_deref() == Some(asks.as_str()) {
            break;
        }
        grid.key(KeyCode::Tab);
    }
    assert_eq!(grid.app.focus.as_deref(), Some(asks.as_str()));
    grid.key(KeyCode::Char('a'));
    d.wait_status(&asks, |s| s != "waiting_for_user", 20);
    dash.until(10, |a| a.state.run(&asks).is_some_and(|r| !r.needs_you()));
    grid.until(10, |a| a.state.run(&asks).is_some_and(|r| !r.needs_you()));
    dash.pump(300);
    grid.pump(300);
    let s = dash.screen();
    assert!(!s.lines().next().unwrap().contains("need"), "the dashboard's header has no one waiting:\n{s}");
    assert!(!s.lines().any(|l| l.contains("Write the") && (l.contains(" ◆ │") || l.contains(" ◆ ┃"))), "its mark is gone:\n{s}");
    assert!(!grid.screen().lines().next().unwrap().contains("need"));
    grid.snapshot("parity-t41-grid-only");
    dash.snapshot("parity-t41-dashboard");
}

/// Strips terminal escape sequences from a pseudo-terminal's output.
fn plain(out: &str) -> String {
    let mut s = String::new();
    let mut chars = out.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for x in chars.by_ref() {
                    if ('@'..='~').contains(&x) {
                        break;
                    }
                }
            } else {
                chars.next();
            }
        } else {
            s.push(c);
        }
    }
    s
}

/// T-41 with the real binary: `overseer-tui --grid` and `overseer-tui --dashboard` in two
/// pseudo-terminals on one daemon at the same time; an agent started while both run shows in both.
#[test]
fn t41_two_real_terminals_on_one_daemon() {
    let t = tempfile::tempdir().unwrap();
    let d = Daemon::start(&[]);
    let r = five_files(&t.path().join("pty"));
    let first = d.sh(&r, "Already here", "printf 'x\\n' >> a.txt; echo output of already here");
    d.wait_status(&first, |s| s == "completed", 20);
    let bin = env!("CARGO_BIN_EXE_overseer-tui");
    let helper = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/pty_run.py");
    let start = |flag: &str| {
        std::process::Command::new("python3")
            .arg(&helper)
            .args(["50", "200", "5", "q", "--", bin, flag, "--daemon"])
            .arg(&d.bin)
            .arg("--home")
            .arg(d.home.path())
            .env("TERM", "xterm-256color")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap()
    };
    let grid = start("--grid");
    let dash = start("--dashboard");
    std::thread::sleep(std::time::Duration::from_millis(2000));
    let newcomer = d.sh(&r, "Pty newcomer", "echo output of pty newcomer; sleep 30");
    let (g, s) = (grid.wait_with_output().unwrap(), dash.wait_with_output().unwrap());
    assert!(g.status.success() && s.status.success(), "both quit cleanly");
    let (g, s) = (plain(&String::from_utf8_lossy(&g.stdout)), plain(&String::from_utf8_lossy(&s.stdout)));
    assert!(g.contains("grid only · page 1/1") && !g.contains("╭ agents ") && !g.contains("┏ agents ") && !g.contains("review · Already"), "--grid shows only the grid:\n{g}");
    assert!(s.contains(" dashboard ") && (s.contains("┏ agents ") || s.contains("╭ agents ")) && s.contains("review · Already here"), "--dashboard starts in dashboard mode:\n{s}");
    assert!(g.contains("Pty newcomer") && s.contains("Pty newcomer"), "the new agent in both");
    d.ctl("run.interrupt", json!({ "run_id": newcomer }));
}

/// T-30: a fixture agent edits three files in turn (the Claude fixture's editor mode, one edit per
/// barrier file); with Follow on, the review moves to each file and to the change being made
/// within 250 ms of the edit reaching the disk; `j` pauses it ("Paused"); `F` resumes it.
#[test]
fn t30_follow_in_the_review() {
    let t = tempfile::tempdir().unwrap();
    let barrier = t.path().join("barrier");
    std::fs::create_dir_all(&barrier).unwrap();
    let barrier_s = barrier.display().to_string();
    let d = Daemon::start(&[("OVERSEER_CLAUDE_PATH", &fixture("claude-fixture.js")), ("CLAUDE_FIXTURE_MODE", "editor"), ("FIXTURE_EDIT_BARRIER", &barrier_s), ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "CLAUDE_FIXTURE_MODE,FIXTURE_EDIT_BARRIER")]);
    let r = five_files(&t.path().join("follow"));
    let run = d.ctl("task.create", json!({ "repo": r, "harness": "claude", "prompt": "edit a.txt:8 edit b.txt:1 edit c.txt:1 edit d.txt:1 edit a.txt:2", "title": "Edits three files in turn" }))["run"]["id"].as_str().unwrap().to_string();
    d.wait_status(&run, |s| s == "running", 20);
    let wt = worktree(&d, &run);
    let mut tui = Tui::attach(&d, 160, 44);
    tui.until(10, |a| a.visible().len() == 1);
    tui.key(KeyCode::Char('v'));
    assert_eq!(tui.app.mode, Mode::Changes);
    tui.until(10, |a| !a.changes.loading && !a.changes.options.is_empty());
    tui.key(KeyCode::Char('F'));
    assert_eq!(tui.app.changes.follow, overseer_tui::app::Follow::On);
    assert!(tui.screen().contains("◉ Following the agent"));

    // Each edit: the review shows that file, at the change the agent made (the line it edited).
    let edit = |tui: &mut Tui, step: usize, file: &str, line: &str| -> std::time::Duration {
        let before = std::fs::read_to_string(wt.join(file)).unwrap();
        std::fs::write(barrier.join(format!("go-{step}")), "").unwrap();
        let end = std::time::Instant::now() + std::time::Duration::from_secs(20);
        let mut written = None;
        loop {
            assert!(std::time::Instant::now() < end, "step {step}: the review did not move to {file}:\n{}", tui.screen());
            if written.is_none() && std::fs::read_to_string(wt.join(file)).unwrap() != before {
                written = Some(std::time::Instant::now());
            }
            let c = &tui.app.changes;
            let there = c.path == file && !c.loading && c.hunks.get(c.change).is_some_and(|h| h.modified_lines.iter().any(|l| l == line)) && c.follow_to.is_none();
            if there {
                if let Some(at) = written {
                    return at.elapsed();
                }
            }
            tui.pump(5);
        }
    };
    let mut took = Vec::new();
    for (step, file, line) in [(1, "a.txt", "line 8 (edited by the agent)"), (2, "b.txt", "b one (edited by the agent)"), (3, "c.txt", "c one (edited by the agent)")] {
        let dt = edit(&mut tui, step, file, line);
        took.push((file, dt));
        assert!(dt < std::time::Duration::from_millis(250), "{file}: the review moved {dt:?} after the edit");
        tui.until(5, |a| !a.changes.loading && a.changes.files.iter().any(|f| f.1 == file && f.2 > 0));
        let s = tui.screen();
        assert!(s.contains(&format!("{file}  change")) && s.contains(&format!("+{line}")), "{s}");
        tui.snapshot(&format!("parity-t30-follow-{step}"));
    }
    eprintln!("T-30 Follow latencies: {took:?}");

    // j (a move by hand) pauses Follow: the next edit does not move the review.
    tui.key(KeyCode::Char('j'));
    assert_eq!(tui.app.changes.follow, overseer_tui::app::Follow::Paused);
    let at = tui.app.changes.path.clone();
    std::fs::write(barrier.join("go-4"), "").unwrap();
    let end = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while !std::fs::read_to_string(wt.join("d.txt")).unwrap().contains("edited by the agent") {
        assert!(std::time::Instant::now() < end);
        tui.pump(20);
    }
    tui.until(10, |a| a.last_edit.values().any(|p| p == "d.txt"));
    tui.pump(500);
    assert_eq!(tui.app.changes.path, at, "paused: the review stays where it was moved by hand");
    let s = tui.screen();
    assert!(s.contains("Follow Paused (F resumes)"), "{s}");
    tui.snapshot("parity-t30-paused");
    // F resumes it: straight to the file the agent edited last, then on to the next edit.
    tui.key(KeyCode::Char('F'));
    assert_eq!(tui.app.changes.follow, overseer_tui::app::Follow::On);
    tui.until(5, |a| a.changes.path == "d.txt" && !a.changes.loading && a.changes.follow_to.is_none() && !a.changes.hunks.is_empty());
    let dt = edit(&mut tui, 5, "a.txt", "line 2 (edited by the agent)");
    assert!(dt < std::time::Duration::from_millis(250), "after resuming, {dt:?}");
    assert!(tui.screen().contains("◉ Following the agent"));
    tui.snapshot("parity-t30-resumed");
    // `?` names the key.
    tui.key(KeyCode::Esc);
    tui.key(KeyCode::Char('?'));
    assert!(tui.screen().contains("Follow the agent's edits"));
}

/// T-31: on the Claude fixture, every waiting agent can be answered from the terminal: `a` Allow
/// once (asked again for the same tool), `s` Allow for this session (Claude Code's own session
/// rule: not asked again), `d` Deny with a note (the note reaches the fixture as the reason).
#[test]
fn t31_a_waiting_agent_can_always_be_answered() {
    let t = tempfile::tempdir().unwrap();
    let log = t.path().join("stdin");
    std::fs::create_dir_all(&log).unwrap();
    let log_s = log.display().to_string();
    let d = Daemon::start(&[("OVERSEER_CLAUDE_PATH", &fixture("claude-fixture.js")), ("CLAUDE_FIXTURE_MODE", "session-rule"), ("FIXTURE_STDIN_LOG_DIR", &log_s), ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "CLAUDE_FIXTURE_MODE,FIXTURE_STDIN_LOG_DIR")]);
    let r = five_files(&t.path().join("answers"));
    let start = |title: &str| d.ctl("task.create", json!({ "repo": r, "harness": "claude", "prompt": "twice: npm test", "title": title }))["run"]["id"].as_str().unwrap().to_string();
    let session = start("Allowed for the session");
    let once = start("Allowed once");
    let denied = start("Denied with a note");
    for id in [&session, &once, &denied] {
        d.wait_status(id, |s| s == "waiting_for_user", 20);
    }
    let mut tui = Tui::attach(&d, 240, 44);
    tui.until(10, |a| a.visible().iter().filter(|r| r.needs_you()).count() == 3);
    let focus = |tui: &mut Tui, id: &str| {
        for _ in 0..10 {
            if tui.app.focus.as_deref() == Some(id) {
                return;
            }
            tui.key(KeyCode::Tab);
        }
        panic!("could not focus {id}");
    };
    let asked = |id: &str| d.events(id).iter().filter(|e| e["kind"] == "permission").count();

    // The prompt: the three answers, with Allow for this session because Claude Code offers its rule.
    focus(&mut tui, &session);
    let s = tui.until_screen(10, "s this session");
    assert!(s.lines().any(|l| l.contains("◆ Bash npm test") && l.contains("a allow once s this session d deny…")), "{s}");
    tui.snapshot("parity-t31-prompt");

    // s: allowed for this session; the second npm test is not asked again.
    tui.key(KeyCode::Char('s'));
    d.wait_status(&session, |s| s == "completed", 20);
    assert_eq!(asked(&session), 1, "Allow for this session is not asked again for the same tool");
    let said: Vec<String> = d.events(&session).iter().filter_map(|e| e["payload"]["text"].as_str().map(str::to_string)).collect();
    assert!(said.iter().any(|t| t.contains("npm test --again (allowed for this session, not asked again)")), "{said:?}");
    let answered = d.events(&session).into_iter().find(|e| e["kind"] == "permission_answered").unwrap();
    assert_eq!(answered["payload"]["always"], "Bash(npm test:*) · this session", "{answered}");

    // a: allowed once; the same tool is asked again, and a allows it again.
    focus(&mut tui, &once);
    tui.key(KeyCode::Char('a'));
    tui.until(10, |a| a.state.run(&once).is_some_and(|r| r.permission_request().is_some_and(|q| q == "req-bash-2")));
    assert_eq!(asked(&once), 2, "Allow once asks again");
    tui.key(KeyCode::Char('a'));
    d.wait_status(&once, |s| s == "completed", 20);

    // d: a one-line note; Enter sends it as the reason.
    focus(&mut tui, &denied);
    tui.key(KeyCode::Char('d'));
    assert!(matches!(tui.app.mode, Mode::DenyNote { .. }));
    tui.type_text("use the staging database instead");
    let s = tui.screen();
    assert!(s.contains("deny → Denied with a note") && s.contains("use the staging database instead") && s.contains("enter denies"), "{s}");
    tui.snapshot("parity-t31-deny-note");
    tui.key(KeyCode::Enter);
    assert_eq!(tui.app.mode, Mode::Grid);
    d.wait_status(&denied, |s| s == "completed", 20);
    let wt = worktree(&d, &denied);
    let input = std::fs::read_to_string(log.join(format!("{}.log", wt.file_name().unwrap().to_string_lossy()))).unwrap();
    let reply = input.lines().find(|l| l.contains("control_response")).expect("the denial reached the fixture");
    assert!(reply.contains("\"behavior\":\"deny\"") && reply.contains("\"message\":\"use the staging database instead\""), "{reply}");
    tui.until(10, |a| a.state.run(&denied).is_some_and(|r| !r.needs_you() && !r.active()));
    let s = tui.until_screen(10, "permission denied: use the staging database instead");
    assert!(!s.lines().next().unwrap().contains("need"), "no one waits:\n{s}");
    tui.snapshot("parity-t31-answered");
    // `?` names all three.
    tui.key(KeyCode::Char('?'));
    assert!(tui.screen().contains("allow / deny: once, this session, with a note"));
}
