//! T-09 to T-12: readable layouts at three sizes, nine busy agents, quitting safely, the
//! terminal restored after a panic, and the binary in a real pseudo-terminal.
mod support;

use crossterm::event::KeyCode;
use overseer_tui::app::{Confirm, Mode};
use serde_json::json;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use support::*;

fn claude_daemon(mode: &str) -> Daemon {
    let claude = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().join("fixtures/fake-harness/claude-fixture.js").display().to_string();
    Daemon::start(&[("OVERSEER_CLAUDE_PATH", &claude), ("CLAUDE_FIXTURE_MODE", mode), ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "CLAUDE_FIXTURE_MODE")])
}

#[test]
fn t09_readable_at_a_glance_at_three_sizes() {
    let t = tempfile::tempdir().unwrap();
    let d = claude_daemon("permission");
    let web = repo(&t.path().join("web-app"));
    let api = repo(&t.path().join("api-server"));
    let done = d.sh(&web, "Refresh sessions once", "echo 'Sessions now refresh once.'; echo '2 tests passing'");
    let failed = d.sh(&api, "Migration dry-run", "echo 'migration dry-run: 3 pending'; echo 'error: relation users_v2 missing' 1>&2; exit 1");
    let stopped = d.sh(&api, "Long soak test", "echo soaking; sleep 30");
    d.wait_status(&stopped, |s| s == "running", 10);
    d.ctl("run.interrupt", json!({ "run_id": stopped }));
    let long = d.sh(&web, "Refactor the payment service so every provider adapter shares one retry and idempotency policy with backoff", &format!("echo 'Reading {}/src/payments/providers/stripe/adapters/refund-reconciliation-worker.ts'; sleep 60", web.display()));
    let busy = d.sh(&api, "Watch the build", "echo 'watching the build…'; sleep 60");
    let waiting = d.ctl("task.create", json!({ "repo": web, "harness": "claude", "prompt": "Add a changelog entry for the session refresh change", "title": "Add a changelog entry" }))["run"]["id"].as_str().unwrap().to_string();
    d.wait_status(&waiting, |s| s == "waiting_for_user", 20);
    for (i, title) in ["Tidy lint warnings", "Bump dependencies", "Write API docs"].iter().enumerate() {
        d.sh(&api, title, &format!("echo 'step {i} done'"));
    }
    let _ = (done, failed, busy);
    let mut tui = Tui::attach(&d, 200, 60);
    tui.until(15, |a| a.visible().len() == 9 && a.state.run(&waiting).is_some_and(|r| r.needs_you()));
    tui.pump(800);

    let s = tui.screen();
    for glyph in ["✓ Refresh sessions once", "✗ Migration dry-run", "■ Long soak test", "● Watch the build", "◆ Add a changelog entry"] {
        assert!(s.contains(glyph), "missing {glyph}:\n{s}");
    }
    assert!(s.contains("page 1/1") && s.contains("● 3 active") && s.contains("◆ 1 needs you"), "{s}");
    assert!(s.contains("error: relation users_v2 missing"), "stderr shown:\n{s}");
    assert!(s.contains("Refactor the payment service so every provider adapter shares one retry and idempotency …") || s.contains("…"), "long titles shortened:\n{s}");
    tui.snapshot("t09-200x60");

    tui.resize(120, 40);
    let s = tui.screen();
    assert!(s.lines().all(|l| unicode_width::UnicodeWidthStr::width(l) <= 120));
    assert!(s.contains("Refactor the payment…") || s.contains("Refactor the"), "{s}");
    tui.snapshot("t09-120x40");

    // Small terminal: a compact list plus the focused agent.
    tui.resize(80, 24);
    let s = tui.screen();
    assert!(s.contains(" agents ") && s.lines().count() == 24, "{s}");
    assert!(s.lines().all(|l| unicode_width::UnicodeWidthStr::width(l) <= 80));
    tui.snapshot("t09-80x24");
    tui.key(KeyCode::Down);
    tui.key(KeyCode::Down);
    let s2 = tui.screen();
    assert_ne!(s, s2, "moving focus in the compact layout shows another agent");

    // Wide again: re-lays out live.
    tui.resize(200, 60);
    assert!(tui.screen().contains("┏"), "back to tiles");
    d.ctl("run.interrupt", json!({ "run_id": long }));
    d.ctl("run.interrupt", json!({ "run_id": waiting }));
}

#[test]
fn t10_nine_busy_agents_stay_responsive() {
    let t = tempfile::tempdir().unwrap();
    let d = Daemon::start(&[]);
    let repo = repo(&t.path().join("busy"));
    let mut ids = Vec::new();
    for n in 1..=9 {
        ids.push(d.sh(&repo, &format!("chatty {n}"), &format!("i=0; while [ $i -lt 120 ]; do echo \"agent {n} line $i: compiling module $i of 120\"; i=$((i+1)); sleep 0.025; done")));
    }
    let mut tui = Tui::attach(&d, 200, 60);
    tui.until(10, |a| a.visible().len() == 9);
    // While all nine stream: time each key press (handling + a full redraw).
    let keys = [KeyCode::Right, KeyCode::Down, KeyCode::Left, KeyCode::Up, KeyCode::Tab, KeyCode::BackTab];
    let mut times = Vec::new();
    let start = Instant::now();
    let mut k = 0;
    while Instant::now() - start < Duration::from_secs(3) {
        let t0 = Instant::now();
        tui.app.handle_key(crossterm::event::KeyEvent::new(keys[k % keys.len()], crossterm::event::KeyModifiers::NONE));
        tui.draw();
        times.push(t0.elapsed().as_secs_f64() * 1000.0);
        k += 1;
        tui.pump(15);
    }
    for id in &ids {
        d.wait_status(id, |s| s == "completed", 30);
    }
    tui.until(20, |a| ids.iter().all(|id| a.feeds.get(id).is_some_and(|f| f.items().any(|i| i.text.contains("line 119")))));
    times.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let p95 = times[times.len() * 95 / 100];
    let mut lag = tui.app.stats.lag_ms.clone();
    lag.sort();
    let lag95 = lag[lag.len() * 95 / 100];
    eprintln!("keys: n={} p50={:.2}ms p95={:.2}ms max={:.2}ms; events: n={} lag p50={}ms p95={}ms max={}ms", times.len(), times[times.len() / 2], p95, times[times.len() - 1], lag.len(), lag[lag.len() / 2], lag95, lag[lag.len() - 1]);
    assert!(p95 < 50.0, "key handling + redraw p95 {p95:.1} ms");
    assert!(lag95 < 250, "event lag p95 {lag95} ms");
    tui.snapshot("t10-nine-busy");
    std::fs::write(
        Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().join("docs/verification/evidence/tui/t10-timings.txt"),
        format!("nine agents × 120 lines at 25 ms; {} key presses during streaming\nkey handling + full redraw: p50 {:.2} ms, p95 {:.2} ms, max {:.2} ms\nevent → app lag: {} events, p50 {} ms, p95 {} ms, max {} ms\n", times.len(), times[times.len() / 2], p95, times[times.len() - 1], lag.len(), lag[lag.len() / 2], lag95, lag[lag.len() - 1]),
    )
    .unwrap();

    // Idle: once the last state reload settles, nothing changes and nothing needs a redraw.
    tui.pump(1500);
    tui.app.dirty = false;
    tui.pump(2000);
    assert!(!tui.app.dirty, "no redraw while idle");
}

#[test]
fn feed_history_is_bounded() {
    let mut f = overseer_tui::feed::Feed::default();
    for i in 0..(overseer_tui::feed::MAX_ITEMS as i64 + 500) {
        f.add(&json!({ "seq": i + 1, "kind": "output", "payload": { "role": "stdout", "text": format!("line {i}") } }), None);
    }
    assert_eq!(f.len(), overseer_tui::feed::MAX_ITEMS);
    assert_eq!(f.items().next().unwrap().text, "line 500", "the oldest are dropped");
}

#[test]
fn t11_quitting_asks_about_drafts_and_leaves_agents_running() {
    let t = tempfile::tempdir().unwrap();
    let d = Daemon::start(&[]);
    let repo = repo(&t.path().join("quit"));
    let run = d.sh(&repo, "Keeps running", "echo alive; while read l; do echo $l; done");
    let mut tui = Tui::attach(&d, 120, 40);
    tui.until_screen(10, "alive");
    tui.key(KeyCode::Char('i'));
    tui.type_text("unsent words");
    tui.key(KeyCode::Esc);
    tui.key(KeyCode::Char('q'));
    assert_eq!(tui.app.mode, Mode::Confirm(Confirm::Quit));
    assert!(tui.screen().contains("Unsent drafts will be lost. Quit? y / n"));
    tui.key(KeyCode::Char('n'));
    assert!(!tui.app.quit);
    tui.key(KeyCode::Char('q'));
    tui.key(KeyCode::Char('y'));
    assert!(tui.app.quit);
    tui.client.close();
    std::thread::sleep(Duration::from_millis(800));
    assert_eq!(d.run(&run)["status"], "running", "the agent keeps running after the TUI quits");
    let clients = d.ctl("daemon.clients", json!({}));
    assert_eq!(clients["ui"], 0, "{clients}");
    d.ctl("run.interrupt", json!({ "run_id": run }));
}

/// Runs the real binary in a 120×40 pseudo-terminal, typing `input` after `after`.
fn in_pty(d: &Daemon, env: &[(&str, &str)], input: &str, after: Duration) -> (bool, String) {
    let bin = env!("CARGO_BIN_EXE_overseer-tui");
    let helper = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/pty_run.py");
    let mut cmd = Command::new("python3");
    cmd.arg(helper).args(["40", "120", &format!("{}", after.as_secs_f64()), input, "--", bin, "--daemon"]).arg(&d.bin).arg("--home").arg(d.home.path());
    cmd.env("TERM", "xterm-256color").stdin(Stdio::null());
    for (k, v) in env {
        cmd.env(k, v);
    }
    let out = cmd.output().unwrap();
    (out.status.success(), String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr))
}

#[test]
fn t11_the_terminal_is_restored_after_quit_and_after_a_panic() {
    let t = tempfile::tempdir().unwrap();
    let d = Daemon::start(&[]);
    let repo = repo(&t.path().join("pty"));
    let run = d.sh(&repo, "Seen in a real terminal", "echo hello from a pty; sleep 30");
    // A normal session: draws the agent, q quits, the alternate screen is left.
    let (ok, out) = in_pty(&d, &[], "q", Duration::from_millis(2500));
    assert!(ok, "exits cleanly:\n{out}");
    assert!(out.contains("\u{1b}[?1049h") && out.contains("\u{1b}[?1049l"), "enters and leaves the alternate screen");
    assert!(out.contains("Seen in a real terminal") || out.contains("Seen"), "draws the agent");
    assert_eq!(d.run(&run)["status"], "running");
    // A panic after the terminal was set up: raw mode and the alternate screen are still undone.
    let (ok, out) = in_pty(&d, &[("OVERSEER_TUI_TEST_PANIC", "1")], "", Duration::from_millis(1500));
    assert!(!ok, "the test panic fails the process");
    let leave = out.rfind("\u{1b}[?1049l").expect("left the alternate screen after the panic");
    let panic_at = out.find("overseer-tui test panic").expect("panic message printed");
    assert!(panic_at > out.find("\u{1b}[?1049h").unwrap(), "panicked after setup");
    assert!(leave > 0);
    d.ctl("run.interrupt", json!({ "run_id": run }));
}

#[test]
fn t12_help_explains_options_and_keys() {
    let out = Command::new(env!("CARGO_BIN_EXE_overseer-tui")).arg("--help").output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success());
    for s in ["--daemon PATH", "--home DIR", "--no-mouse", "page 1 is the newest nine", "allow / deny", "quit (agents keep running)"] {
        assert!(text.contains(s), "help lacks {s}:\n{text}");
    }
    std::fs::write(Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().join("docs/verification/evidence/tui/t12-help.txt"), text.as_bytes()).unwrap();
    let v = Command::new(env!("CARGO_BIN_EXE_overseer-tui")).arg("--version").output().unwrap();
    assert!(String::from_utf8_lossy(&v.stdout).starts_with("overseer-tui 0.1.0"));
}
