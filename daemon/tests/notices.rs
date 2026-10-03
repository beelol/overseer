//! AC-240: you hear about it outside VS Code. With no focused VS Code window, an agent that needs
//! the owner, finishes or fails posts one notification naming it, with a click URL for that agent;
//! a focused window posts none; a setting chooses which kinds. Fixture harnesses only; the
//! notification goes to a logging command (OVERSEER_NOTIFY_COMMAND), never a real banner.

mod common;
use common::*;
use serde_json::json;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

fn fixture(name: &str) -> String {
    repo_root().join("fixtures").join(name).display().to_string()
}

/// A notify command that logs title|body|url, one line per notification.
fn logger(dir: &Path) -> (String, PathBuf) {
    let log = dir.join("notifications.log");
    let script = dir.join("notify.sh");
    std::fs::write(&script, format!("#!/bin/sh\nprintf '%s|%s|%s\\n' \"$1\" \"$2\" \"$3\" >> '{}'\n", log.display())).unwrap();
    std::process::Command::new("chmod").arg("+x").arg(&script).status().unwrap();
    (script.display().to_string(), log)
}

fn lines(log: &Path) -> Vec<String> {
    std::fs::read_to_string(log).unwrap_or_default().lines().map(str::to_string).collect()
}

fn request(conn: &mut std::os::unix::net::UnixStream, id: u64, method: &str, params: serde_json::Value) -> String {
    conn.write_all(format!("{}\n", json!({"id": id, "method": method, "params": params})).as_bytes()).unwrap();
    let mut line = String::new();
    BufReader::new(conn.try_clone().unwrap()).read_line(&mut line).unwrap();
    line
}

/// A VS Code window that says whether it has the OS focus (what the extension sends).
fn window(d: &Daemon, focused: bool) -> std::os::unix::net::UnixStream {
    let mut conn = std::os::unix::net::UnixStream::connect(d.socket()).unwrap();
    conn.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    assert!(request(&mut conn, 1, "hello", json!({"client": "vscode"})).contains("\"protocol\""));
    assert!(request(&mut conn, 2, "ui.window", json!({"focused": focused})).contains("\"ok\":true"));
    conn
}

fn start(t: &Path) -> (Daemon, PathBuf, PathBuf, PathBuf) {
    let (cmd, log) = logger(t);
    let mode = t.join("claude-mode");
    std::fs::write(&mode, "echo").unwrap();
    let d = Daemon::start(&[("OVERSEER_NOTIFY_COMMAND", &cmd), ("OVERSEER_CLAUDE_PATH", &fixture("fake-harness/claude-fixture.js")),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "CLAUDE_FIXTURE_MODE_FILE"), ("CLAUDE_FIXTURE_MODE_FILE", mode.to_str().unwrap())]);
    let repo = repo(&t.join("site"));
    (d, log, mode, repo)
}

fn claude(d: &Daemon, repo: &Path, title: &str) -> String {
    run_id(&d.call("task.create", json!({"repo": repo, "harness": "claude", "prompt": title, "title": title})))
}

fn failing(d: &Daemon, repo: &Path, title: &str) -> String {
    run_id(&d.call("task.create", json!({"repo": repo, "harness": "generic", "program": "/bin/sh", "args": ["-c", "echo boom; exit 2"], "prompt": "", "title": title})))
}

fn wait_lines(log: &Path, n: usize) -> Vec<String> {
    for _ in 0..100 {
        if lines(log).len() >= n {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    std::thread::sleep(Duration::from_millis(500)); // and nothing more arrives
    lines(log)
}

#[test]
fn ac240_an_unfocused_window_hears_about_a_permission_a_finish_and_a_failure_once_each() {
    let t = tmp();
    let (d, log, mode, repo) = start(t.path());
    let _w = window(&d, false);
    std::fs::write(&mode, "permission").unwrap();
    let asks = claude(&d, &repo, "Write the changelog");
    d.wait_status(&asks, |s| s == "waiting_for_user", 20);
    std::fs::write(&mode, "echo").unwrap();
    let done = claude(&d, &repo, "Summarise the notes");
    d.wait_done(&done, 20);
    let broken = failing(&d, &repo, "Broken build");
    assert_eq!(d.wait_done(&broken, 20)["status"], "failed");
    let got = wait_lines(&log, 3);
    let of = |title: &str| got.iter().filter(|l| l.starts_with(&format!("{title}|"))).cloned().collect::<Vec<_>>();
    assert_eq!(of("Write the changelog"), vec![format!("Write the changelog|Needs your permission to use Write · site|vscode://beelol.overseer/open-agent?run={asks}")], "{got:?}");
    assert_eq!(of("Summarise the notes"), vec![format!("Summarise the notes|Finished · site|vscode://beelol.overseer/open-agent?run={done}")], "{got:?}");
    assert_eq!(of("Broken build"), vec![format!("Broken build|Stopped with an error · site|vscode://beelol.overseer/open-agent?run={broken}")], "{got:?}");
    assert_eq!(got.len(), 3, "one notification each: {got:?}");
    d.call("run.interrupt", json!({"run_id": asks}));
    d.wait_done(&asks, 20);
}

#[test]
fn ac240_a_focused_window_writes_none_and_losing_focus_brings_them_back() {
    let t = tmp();
    let (d, log, _mode, repo) = start(t.path());
    let mut w = window(&d, true);
    let unfocused = window(&d, false); // a second window in the background: one focused is enough
    let done = claude(&d, &repo, "Summarise the notes");
    d.wait_done(&done, 20);
    let broken = failing(&d, &repo, "Broken build");
    d.wait_done(&broken, 20);
    std::thread::sleep(Duration::from_millis(800));
    assert!(lines(&log).is_empty(), "a focused window hears about it in VS Code: {:?}", lines(&log));
    assert_eq!(d.call("notices.get", json!({}))["vscode_focused"], true);
    // The owner switches to another app.
    assert!(request(&mut w, 3, "ui.window", json!({"focused": false})).contains("\"ok\":true"));
    let again = failing(&d, &repo, "Second failure");
    d.wait_done(&again, 20);
    assert_eq!(wait_lines(&log, 1).len(), 1, "{:?}", lines(&log));
    // A focused window closes without saying it lost focus: its focus goes with it.
    let focused = window(&d, true);
    assert_eq!(d.call("notices.get", json!({}))["vscode_focused"], true);
    drop(focused);
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(d.call("notices.get", json!({}))["vscode_focused"], false);
    drop(unfocused);
}

#[test]
fn ac240_with_vscode_closed_the_moments_still_notify() {
    let t = tmp();
    let (d, log, _mode, repo) = start(t.path());
    let done = claude(&d, &repo, "Summarise the notes");
    d.wait_done(&done, 20);
    let got = wait_lines(&log, 1);
    assert_eq!(got, vec![format!("Summarise the notes|Finished · site|vscode://beelol.overseer/open-agent?run={done}")]);
}

#[test]
fn ac240_the_setting_chooses_which_kinds() {
    let t = tmp();
    let (d, log, mode, repo) = start(t.path());
    assert_eq!(d.call("notices.get", json!({}))["kinds"], json!(["permission", "question", "failure", "finished"]));
    assert!(d.try_call("notices.set", json!({"kinds": ["finished", "sometimes"]})).is_err(), "unknown kinds are refused");
    assert_eq!(d.call("notices.set", json!({"kinds": ["permission"]}))["kinds"], json!(["permission"]));
    let _w = window(&d, false);
    let done = claude(&d, &repo, "Summarise the notes");
    d.wait_done(&done, 20);
    let broken = failing(&d, &repo, "Broken build");
    d.wait_done(&broken, 20);
    std::fs::write(&mode, "permission").unwrap();
    let asks = claude(&d, &repo, "Write the changelog");
    d.wait_status(&asks, |s| s == "waiting_for_user", 20);
    let got = wait_lines(&log, 1);
    assert_eq!(got.len(), 1, "{got:?}");
    assert!(got[0].starts_with("Write the changelog|Needs your permission"), "{got:?}");
    d.call("run.interrupt", json!({"run_id": asks}));
    d.wait_done(&asks, 20);
}

/// AC-240: with VS Code closed, the notification carries the TUI on that agent (the notify
/// command's fourth argument; the notifier runs it in Terminal on a click while VS Code is not
/// running). Run as the notifier would, it starts `overseer-tui --focus <run> --home <this daemon's
/// data folder>`; here a stand-in TUI records its arguments, so no terminal opens.
#[test]
fn ac240_with_vscode_closed_a_click_opens_the_tui_on_that_agent() {
    let t = tmp();
    let log = t.path().join("notifications.log");
    let notify = t.path().join("notify.sh");
    std::fs::write(&notify, format!("#!/bin/sh\nprintf '%s|%s|%s|%s\\n' \"$1\" \"$2\" \"$3\" \"$4\" >> '{}'\n", log.display())).unwrap();
    let tui_args = t.path().join("tui-args");
    let tui = t.path().join("bin dir/overseer-tui");
    std::fs::create_dir_all(tui.parent().unwrap()).unwrap();
    std::fs::write(&tui, format!("#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\n", tui_args.display())).unwrap();
    for f in [&notify, &tui] {
        std::process::Command::new("chmod").arg("+x").arg(f).status().unwrap();
    }
    let mode = t.path().join("claude-mode");
    std::fs::write(&mode, "echo").unwrap();
    let d = Daemon::start(&[("OVERSEER_NOTIFY_COMMAND", notify.to_str().unwrap()), ("OVERSEER_TUI", tui.to_str().unwrap()),
        ("OVERSEER_CLAUDE_PATH", &fixture("fake-harness/claude-fixture.js")), ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "CLAUDE_FIXTURE_MODE_FILE"), ("CLAUDE_FIXTURE_MODE_FILE", mode.to_str().unwrap())]);
    let repo = repo(&t.path().join("site"));
    let done = claude(&d, &repo, "Summarise the notes");
    d.wait_done(&done, 20);
    let got = wait_lines(&log, 1);
    assert_eq!(got.len(), 1, "{got:?}");
    let parts: Vec<&str> = got[0].splitn(4, '|').collect();
    assert_eq!(parts[2], format!("vscode://beelol.overseer/open-agent?run={done}"));
    let command = parts[3];
    assert!(command.starts_with(&format!("'{}' --focus '{done}'", tui.display())), "{command}");
    // What Terminal runs (the notifier's .command file ends with `exec <command>`).
    assert!(std::process::Command::new("/bin/sh").arg("-c").arg(format!("exec {command}")).status().unwrap().success());
    let args: Vec<String> = std::fs::read_to_string(&tui_args).unwrap().lines().map(str::to_string).collect();
    assert_eq!(args, vec!["--focus".to_string(), done.clone(), "--home".to_string(), d.home.path().display().to_string()]);
}
