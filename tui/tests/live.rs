//! T-01 and T-03: the TUI on a real overseerd — one source of truth, live tiles, and resuming
//! the event stream after a dropped connection. Generic fixture agents only (no paid tokens).
mod support;

use crossterm::event::KeyCode;
use overseer_tui::client::{Client, Msg, Requests};
use serde_json::json;
use std::sync::mpsc::channel;
use std::time::{Duration, Instant};
use support::*;

#[test]
fn t01_the_tui_and_another_client_see_the_same_daemon() {
    let t = tempfile::tempdir().unwrap();
    let d = Daemon::start(&[]);
    let repo = repo(&t.path().join("web"));
    let echo = d.sh(&repo, "Echo agent", "echo ready; while read l; do echo \"echo: $l\"; done");
    let mut tui = Tui::attach(&d, 160, 48);
    let s = tui.until_screen(15, "ready");
    assert!(s.contains("Echo agent"), "{s}");

    // A second client subscribed to the same daemon, as a VS Code window is.
    let (tx, rx) = channel();
    let vscode = Client::start(None, d.socket.clone(), tx);
    let end = Instant::now() + Duration::from_secs(10);
    while !vscode.connected() && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(20));
    }
    vscode.set_cursor_if_unset(d.ctl("state", json!({}))["cursor"].as_i64().unwrap());
    vscode.subscribe();
    std::thread::sleep(Duration::from_millis(200));

    // Typed in the TUI: reaches the daemon (turn record) and the other client (event).
    tui.key(KeyCode::Char('i'));
    tui.type_text("hello from the tui");
    tui.key(KeyCode::Enter);
    tui.until_screen(10, "echo: hello from the tui");
    let turns = d.ctl("run.turns", json!({ "run_id": echo }));
    assert!(turns.as_array().unwrap().iter().any(|t| t["prompt"] == "hello from the tui"), "{turns}");
    let end = Instant::now() + Duration::from_secs(10);
    let mut seen = false;
    while Instant::now() < end && !seen {
        if let Ok(Msg::Event(e)) = rx.recv_timeout(Duration::from_millis(100)) {
            seen = e["kind"] == "output" && e["payload"]["text"] == "echo: hello from the tui";
        }
    }
    assert!(seen, "the other client saw the TUI's message as an event");

    // Done elsewhere (a script or VS Code): shows up in the TUI.
    let other = d.sh(&repo, "Started elsewhere", "echo made outside the tui; sleep 30");
    let s = tui.until_screen(10, "made outside the tui");
    assert!(s.contains("Started elsewhere"), "{s}");
    d.ctl("run.follow_up", json!({ "run_id": echo, "prompt": "from vscode" }));
    tui.until_screen(10, "echo: from vscode");
    // The TUI keeps no state of its own: nothing written outside the daemon's home.
    assert_eq!(tui.app.state.run(&other).map(|r| r.title.as_str()), Some("Started elsewhere"));
    tui.snapshot("t01-same-daemon");
}

#[test]
fn t03_tiles_update_live_and_resume_after_a_dropped_connection() {
    let t = tempfile::tempdir().unwrap();
    let d = Daemon::start(&[]);
    let repo = repo(&t.path().join("api"));
    let echo = d.sh(&repo, "Latency probe", "while read l; do echo \"got $l\"; done");
    let mut tui = Tui::attach(&d, 160, 48);
    tui.until_screen(10, "Latency probe");
    // Latency: from the daemon's event timestamp to the TUI handling it.
    for i in 0..10 {
        d.ctl("run.follow_up", json!({ "run_id": echo, "prompt": format!("p{i}") }));
        tui.until_screen(10, &format!("got p{i}"));
    }
    let mut lag = tui.app.stats.lag_ms.clone();
    lag.sort();
    let p95 = lag[(lag.len() * 95 / 100).min(lag.len() - 1)];
    eprintln!("event → tile lag: n={} p50={}ms p95={}ms max={}ms", lag.len(), lag[lag.len() / 2], p95, lag[lag.len() - 1]);
    assert!(p95 < 250, "p95 lag {p95} ms");

    // A chatty agent; the connection drops mid-stream; every line arrives exactly once.
    let ticker = d.sh(&repo, "Ticker", "i=0; while [ $i -lt 60 ]; do echo \"tick $i\"; i=$((i+1)); sleep 0.03; done");
    tui.until_screen(10, "tick 5");
    tui.client.drop_connection();
    tui.until(10, |a| !a.connected);
    tui.until(15, |a| a.connected);
    d.wait_status(&ticker, |s| s == "completed", 20);
    tui.until(10, |a| a.feeds.get(&ticker).is_some_and(|f| f.items().any(|i| i.text == "tick 59")));
    tui.pump(500);
    let feed = &tui.app.feeds[&ticker];
    for i in 0..60 {
        let n = feed.items().filter(|it| it.text == format!("tick {i}")).count();
        assert_eq!(n, 1, "tick {i} appears {n} times");
    }
    tui.snapshot("t03-live-tiles");
}
