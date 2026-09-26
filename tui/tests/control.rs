//! T-06, T-07 and T-08: answering permissions, interrupting, zoom with scrollback, and starting
//! agents from the New Agent form. Real overseerd; generic agents and the SYNTHETIC Claude
//! fixture (permission mode).
mod support;

use crossterm::event::KeyCode;
use overseer_tui::app::{Confirm, Mode};
use serde_json::json;
use std::path::Path;
use support::*;

fn claude_daemon(mode: &str) -> Daemon {
    let claude = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().join("fixtures/fake-harness/claude-fixture.js").display().to_string();
    Daemon::start(&[("OVERSEER_CLAUDE_PATH", &claude), ("CLAUDE_FIXTURE_MODE", mode), ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "CLAUDE_FIXTURE_MODE")])
}

fn claude_task(d: &Daemon, repo: &Path, title: &str) -> String {
    d.ctl("task.create", json!({ "repo": repo, "harness": "claude", "prompt": "write perm.txt", "title": title }))["run"]["id"].as_str().unwrap().to_string()
}

fn workspace_of(d: &Daemon, run: &str) -> std::path::PathBuf {
    let st = d.ctl("state", json!({}));
    let ws = d.run(run)["workspace_id"].clone();
    std::path::PathBuf::from(st["workspaces"].as_array().unwrap().iter().find(|w| w["id"] == ws).unwrap()["path"].as_str().unwrap())
}

#[test]
fn t06_answer_permissions_jump_to_waiting_agents_and_interrupt() {
    let t = tempfile::tempdir().unwrap();
    let d = claude_daemon("permission");
    let repo = repo(&t.path().join("perm"));
    let e1 = d.sh(&repo, "Echo one", "echo up; while read l; do echo $l; done");
    let p1 = claude_task(&d, &repo, "Claude one");
    let e2 = d.sh(&repo, "Echo two", "echo up; while read l; do echo $l; done");
    let p2 = claude_task(&d, &repo, "Claude two");
    for p in [&p1, &p2] {
        d.wait_status(p, |s| s == "waiting_for_user", 20);
    }
    let mut tui = Tui::attach(&d, 180, 50);
    tui.until(10, |a| a.visible().iter().filter(|r| r.needs_you()).count() == 2);
    let s = tui.screen();
    assert!(s.contains("◆ 2 need you"), "{s}");
    assert!(s.contains("wants Write perm.txt") && s.contains("a allow d deny"), "readable summary, not JSON:\n{s}");
    tui.snapshot("t06-waiting");

    // w: next agent waiting for you (newest first: Claude two, then Claude one).
    let focused = |tui: &Tui| tui.app.focus.clone().unwrap_or_default();
    tui.key(KeyCode::Char('4')); // Echo one, the oldest
    assert_eq!(focused(&tui), e1);
    tui.key(KeyCode::Char('w'));
    assert_eq!(focused(&tui), p2, "wraps to the first waiting agent");
    tui.key(KeyCode::Char('a'));
    tui.until(10, |a| a.state.run(&p2).is_some_and(|r| !r.needs_you()));
    tui.key(KeyCode::Char('w'));
    assert_eq!(focused(&tui), p1);
    tui.key(KeyCode::Char('d'));
    tui.until(10, |a| a.state.run(&p1).is_some_and(|r| !r.needs_you()));
    let answered = |run: &str| d.events(run).iter().find(|e| e["kind"] == "permission_answered").map(|e| e["payload"]["allow"].as_bool().unwrap());
    assert_eq!(answered(&p2), Some(true));
    assert_eq!(answered(&p1), Some(false));
    assert_eq!(std::fs::read_to_string(workspace_of(&d, &p2).join("perm.txt")).ok().as_deref(), Some("allowed\n"));
    assert!(!workspace_of(&d, &p1).join("perm.txt").exists(), "denied: nothing written");
    let s = tui.until_screen(10, "denied Write perm.txt");
    assert!(s.contains("allowed Write perm.txt"), "{s}");
    tui.key(KeyCode::Char('w'));
    assert!(tui.screen().contains("No agent is waiting for you"));

    // x interrupts only the focused agent, after y/n.
    tui.key(KeyCode::Char(char::from_digit(tui.app.visible().iter().position(|r| r.id == e2).unwrap() as u32 + 1, 10).unwrap()));
    tui.key(KeyCode::Char('x'));
    assert_eq!(tui.app.mode, Mode::Confirm(Confirm::Interrupt(e2.clone())));
    assert!(tui.screen().contains("Interrupt Echo two? y / n"));
    tui.key(KeyCode::Char('n'));
    assert_eq!(d.run(&e2)["status"], "running", "n cancels");
    tui.key(KeyCode::Char('x'));
    tui.key(KeyCode::Char('y'));
    assert_eq!(d.wait_status(&e2, |s| s == "interrupted", 10), "interrupted");
    assert_eq!(d.run(&e1)["status"], "running", "the other agent keeps running");
    d.ctl("run.interrupt", json!({ "run_id": e1 }));
}

#[test]
fn t07_zoom_shows_the_whole_conversation_with_scrollback() {
    let t = tempfile::tempdir().unwrap();
    let d = Daemon::start(&[]);
    let repo = repo(&t.path().join("zoom"));
    let long = d.sh(&repo, "Two thousand lines", "i=0; while [ $i -lt 2000 ]; do echo \"row $i\"; i=$((i+1)); done");
    d.wait_status(&long, |s| s == "completed", 30);
    let echo = d.sh(&repo, "Live echo", "while read l; do echo \"live: $l\"; done");
    let mut tui = Tui::attach(&d, 140, 40);
    tui.until(10, |a| a.visible().len() == 2);
    tui.key(KeyCode::Char('2'));
    tui.until(20, |a| a.feeds.get(&long).is_some_and(|f| f.items().any(|i| i.text == "row 1999")));
    tui.key(KeyCode::Char('z'));
    assert_eq!(tui.app.mode, Mode::Zoom { scroll: 0 });
    let s = tui.screen();
    assert!(s.contains("row 1999") && !s.contains("row 0\n") && !s.contains("row 1000 "), "{s}");
    tui.snapshot("t07-zoom-bottom");
    tui.key(KeyCode::Char('g'));
    let s = tui.screen();
    assert!(s.contains("│ row 0 ") || s.contains("┃ row 0 "), "top of history:\n{s}");
    assert!(!s.contains("row 1999"));
    tui.snapshot("t07-zoom-top");
    tui.key(KeyCode::Char('j'));
    tui.key(KeyCode::PageDown);
    assert!(!tui.screen().contains("row 0 "), "scrolled down");
    tui.key(KeyCode::Char('G'));
    assert!(tui.screen().contains("row 1999"));
    tui.key(KeyCode::Esc);
    assert_eq!(tui.app.mode, Mode::Grid);
    assert_eq!(tui.app.focus.as_deref(), Some(long.as_str()), "back on the same tile");

    // Zoomed on a live agent: new output keeps following at the bottom; messages work from zoom.
    tui.key(KeyCode::Char('1'));
    tui.key(KeyCode::Char('z'));
    tui.key(KeyCode::Char('i'));
    tui.type_text("from zoom");
    tui.key(KeyCode::Enter);
    assert!(matches!(tui.app.mode, Mode::Zoom { .. }), "sending returns to zoom");
    tui.until_screen(10, "live: from zoom");
    d.ctl("run.follow_up", json!({ "run_id": echo, "prompt": "again" }));
    tui.until_screen(10, "live: again");
    d.ctl("run.interrupt", json!({ "run_id": echo }));
}

#[test]
fn t08_start_agents_from_the_new_agent_form() {
    let t = tempfile::tempdir().unwrap();
    let d = claude_daemon("permission");
    let repo = repo(&t.path().join("form"));
    let old = d.sh(&repo, "Older agent", "echo older");
    let mut tui = Tui::attach(&d, 160, 48);
    tui.app.cwd_repo = Some(repo.display().to_string());
    tui.until(10, |a| a.visible().len() == 1);

    // A generic agent: program and arguments.
    tui.key(KeyCode::Char('n'));
    assert_eq!(tui.app.mode, Mode::NewAgent);
    tui.until(10, |a| a.form.harnesses.iter().any(|h| h.0 == "generic") && !a.form.accounts.is_empty());
    let s = tui.screen();
    assert!(s.contains("new agent") && s.contains("Repository") && s.contains("Harness") && s.contains("Prompt"), "{s}");
    tui.snapshot("t08-form");
    tui.key(KeyCode::Tab); // → Repository
    tui.key(KeyCode::Tab); // → Harness
    for _ in 0..8 {
        if tui.app.form.harness_id() == Some("generic") {
            break;
        }
        tui.key(KeyCode::Right);
    }
    assert_eq!(tui.app.form.harness_id(), Some("generic"));
    tui.key(KeyCode::Tab); // → Arguments
    tui.key(KeyCode::Backspace);
    tui.key(KeyCode::Backspace);
    tui.type_text(r#"["-c","echo started from the form; sleep 20"]"#);
    tui.key(KeyCode::Tab); // → Program
    tui.type_text("/bin/sh");
    tui.key(KeyCode::Tab); // → Prompt
    tui.type_text("Watch the form's agent");
    tui.key(KeyCode::Enter);
    tui.until(10, |a| a.mode == Mode::Grid && a.focus.as_deref().is_some_and(|f| a.state.run(f).is_some_and(|r| r.harness == "generic" && r.title == "Watch the form's agent")));
    let s = tui.until_screen(10, "started from the form");
    assert!(s.contains("┏ 1 ● Watch the form's agent"), "the new agent is tile 1 and focused:\n{s}");
    let new = tui.app.focus.clone().unwrap();
    let st = d.ctl("state", json!({}));
    let task = st["tasks"].as_array().unwrap().iter().find(|t| t["id"] == d.run(&new)["task_id"]).unwrap().clone();
    assert_eq!(task["repo_root"].as_str(), Some(repo.to_str().unwrap()));
    assert_eq!(task["prompt"], "Watch the form's agent");

    // A Claude agent: compatible account, prompt required.
    tui.key(KeyCode::Char('n'));
    tui.until(10, |a| !a.form.harnesses.is_empty());
    tui.key(KeyCode::Tab);
    tui.key(KeyCode::Tab);
    for _ in 0..8 {
        if tui.app.form.harness_id() == Some("claude") {
            break;
        }
        tui.key(KeyCode::Right);
    }
    assert_eq!(tui.app.form.harness_id(), Some("claude"));
    tui.key(KeyCode::Enter);
    assert_eq!(tui.app.form.error.as_deref(), Some("Type what the agent should do."));
    let s = tui.screen();
    assert!(s.contains("claude (existing login)"), "compatible account offered:\n{s}");
    tui.key(KeyCode::BackTab); // → Repository
    tui.key(KeyCode::BackTab); // → Prompt (wraps)
    assert_eq!(tui.app.form.field, 4);
    tui.type_text("write perm.txt from the form");
    tui.key(KeyCode::Enter);
    tui.until(10, |a| a.mode == Mode::Grid && a.focused().is_some_and(|r| r.harness == "claude"));
    let run = tui.app.focus.clone().unwrap();
    assert_eq!(d.run(&run)["profile_id"], "system-claude");
    tui.until_screen(15, "wants Write perm.txt");
    assert_ne!(tui.app.focus.as_deref(), Some(old.as_str()));
    d.ctl("run.interrupt", json!({ "run_id": run }));
    d.ctl("run.interrupt", json!({ "run_id": new }));
}

#[test]
fn t14_changes_view_lists_files_and_diffs_like_the_review() {
    let t = tempfile::tempdir().unwrap();
    let d = Daemon::start(&[]);
    let repo = repo(&t.path().join("changes"));
    let run = d.sh(&repo, "Edits two files", "printf 'more docs\\n' >> README.md; mkdir -p src/deep/nested; printf 'fn main() {}\\n' > src/deep/nested/new.rs; echo edited; sleep 30");
    let mut tui = Tui::attach(&d, 160, 44);
    tui.until_screen(15, "edited");
    tui.key(KeyCode::Char('v'));
    assert_eq!(tui.app.mode, Mode::Changes);
    tui.until(10, |a| a.changes.files.len() == 2 && !a.changes.loading);
    let s = tui.screen();
    assert!(s.contains("changes · Edits two files · Latest run · 2 files +2 −0"), "{s}");
    assert!(s.contains("M README.md +1 −0") && s.contains("A src/deep/nested/new.rs +1 −0"), "{s}");
    assert!(s.contains("+more docs"), "diff of the first file:\n{s}");
    tui.snapshot("t14-changes");
    tui.key(KeyCode::Char('j'));
    let s = tui.screen();
    assert!(s.contains("+fn main() {}") && !s.contains("+more docs"), "{s}");
    // Another comparison, like the review's comparison picker.
    tui.key(KeyCode::Char('c'));
    tui.until(10, |a| !a.changes.loading && a.changes.option == 1 && !a.changes.files.is_empty());
    assert!(tui.screen().contains("Since task start"), "{}", tui.screen());
    tui.key(KeyCode::Esc);
    assert_eq!(tui.app.mode, Mode::Grid);
    d.ctl("run.interrupt", json!({ "run_id": run }));
}

#[test]
fn t17_zoom_expands_tool_inputs_and_results() {
    let t = tempfile::tempdir().unwrap();
    let d = claude_daemon("permission");
    let repo = repo(&t.path().join("tools"));
    let run = claude_task(&d, &repo, "Writes a file");
    d.wait_status(&run, |s| s == "waiting_for_user", 20);
    let mut tui = Tui::attach(&d, 140, 40);
    tui.until(10, |a| a.state.run(&run).is_some_and(|r| r.needs_you()));
    tui.key(KeyCode::Char('a'));
    tui.until(15, |a| a.state.run(&run).is_some_and(|r| r.status == "completed"));
    tui.key(KeyCode::Char('z'));
    let folded = tui.screen();
    assert!(folded.contains("⚙ Write perm.txt ✓") && !folded.contains("│ perm.txt"), "{folded}");
    tui.key(KeyCode::Char('e'));
    let s = tui.screen();
    assert!(s.contains("│ perm.txt") && s.contains("│ 1 line") && s.contains("│ File created successfully at: ./perm.txt"), "input and result under the tool call:\n{s}");
    tui.snapshot("t17-expanded-tools");
    tui.key(KeyCode::Char('e'));
    assert!(!tui.screen().contains("│ perm.txt"), "e folds them again");
}
