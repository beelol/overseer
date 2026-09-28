//! Phone access from the terminal (Gate N: AC-116, AC-117, AC-119, and the Mac's side of AC-129):
//! the switch and the status line, the Devices panel with revoke and scope, and pairing with the
//! code as text and as a QR code and the owner's y/n. Real overseerd in its own home, with the
//! network advertiser off and its own port; the phone is the reference phone of the UI
//! scenarios (test/ui/ref-phone.js), which does the real pairing handshake.
mod support;

use crossterm::event::KeyCode;
use overseer_tui::app::{code_groups, fingerprint, Confirm, Mode, PairingState};
use overseer_tui::ui::QR_PAPER;
use ratatui::buffer::Buffer;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{channel, Receiver};
use std::time::{Duration, Instant};
use support::*;

fn free_port() -> u16 {
    std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap().local_addr().unwrap().port()
}

fn refused(port: u16) -> bool {
    std::net::TcpStream::connect_timeout(&format!("127.0.0.1:{port}").parse().unwrap(), Duration::from_millis(500)).is_err()
}

/// A daemon that advertises nothing on the network and uses a port of its own.
fn phone_daemon(extra: &[(&str, &str)]) -> (Daemon, u16) {
    let port = free_port();
    let port_text = port.to_string();
    let mut env = vec![("OVERSEER_GATEWAY_MDNS", "off"), ("OVERSEER_GATEWAY_PORT", port_text.as_str())];
    env.extend_from_slice(extra);
    (Daemon::start(&env), port)
}

/// The reference phone as a program: it presents the code, waits for the owner, then holds its session.
struct RefPhone {
    child: Child,
    lines: Receiver<Value>,
    seen: Vec<Value>,
}

impl RefPhone {
    fn pair(code: &str, name: &str, platform: &str, hold_secs: u32) -> RefPhone {
        let script = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().join("test/ui/ref-phone.js");
        let mut child = Command::new("node").arg(script).args(["pair", code, "--name", name, "--platform", platform, "--hold", &hold_secs.to_string()]).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().expect("node runs the reference phone");
        let out = child.stdout.take().unwrap();
        let (tx, lines) = channel();
        std::thread::spawn(move || {
            for line in BufReader::new(out).lines().map_while(Result::ok) {
                if let Ok(v) = serde_json::from_str::<Value>(&line) {
                    if tx.send(v).is_err() {
                        break;
                    }
                }
            }
        });
        RefPhone { child, lines, seen: Vec::new() }
    }

    /// The first `event` the phone reports within `secs`.
    fn wait(&mut self, event: &str, secs: u64) -> Option<Value> {
        let end = Instant::now() + Duration::from_secs(secs);
        loop {
            if let Some(v) = self.seen.iter().find(|v| v["event"] == event) {
                return Some(v.clone());
            }
            match self.lines.recv_timeout(end.saturating_duration_since(Instant::now())) {
                Ok(v) => self.seen.push(v),
                Err(_) => return None,
            }
        }
    }
}

impl Drop for RefPhone {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn waiting_request(d: &Daemon, secs: u64) -> Value {
    let end = Instant::now() + Duration::from_secs(secs);
    loop {
        let status = d.ctl("gateway.status", json!({}));
        if let Some(first) = status["pairing"]["waiting"].as_array().and_then(|w| w.first()) {
            return first.clone();
        }
        assert!(Instant::now() < end, "no phone asked to pair: {status}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Pairs a phone the way VS Code would: a code from the daemon, the owner's yes through `ctl`.
fn pair_through_ctl(d: &Daemon, name: &str, platform: &str) -> RefPhone {
    let code = d.ctl("gateway.pair_start", json!({}))["code"].as_str().unwrap().to_string();
    let mut phone = RefPhone::pair(&code, name, platform, 60);
    let request = waiting_request(d, 10);
    assert_eq!(request["name"], json!(name));
    d.ctl("gateway.pair_confirm", json!({ "request": request["request"], "accept": true }));
    assert!(phone.wait("paired", 10).is_some(), "{name} paired");
    phone
}

fn header(screen: &str) -> String {
    screen.lines().next().unwrap_or_default().to_string()
}

/// The QR code on the screen: the cells on the code's white paper, as drawn lines.
fn qr_on_screen(buf: &Buffer) -> Option<Vec<String>> {
    let (mut x0, mut y0, mut x1, mut y1) = (u16::MAX, u16::MAX, 0u16, 0u16);
    for y in 0..buf.area.height {
        for x in 0..buf.area.width {
            if buf[(x, y)].bg == QR_PAPER {
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x);
                y1 = y1.max(y);
            }
        }
    }
    if x0 == u16::MAX {
        return None;
    }
    Some((y0..=y1).map(|y| (x0..=x1).map(|x| { assert_eq!(buf[(x, y)].bg, QR_PAPER, "the code is one rectangle"); buf[(x, y)].symbol().to_string() }).collect()).collect())
}

fn read_qr(buf: &Buffer) -> Option<(String, usize)> {
    let lines = qr_on_screen(buf)?;
    for quiet in [4, 2] {
        if let Ok(rows) = qr_read::from_half_blocks(&lines, quiet) {
            if let Ok(read) = qr_read::decode(&rows) {
                return Some((read.text, quiet));
            }
        }
    }
    panic!("the drawn code does not read back:\n{}", lines.join("\n"));
}

/// The code as typed text on the screen: the groups after "Code to type", joined.
fn typed_code(screen: &str) -> String {
    let lines: Vec<&str> = screen.lines().collect();
    let (row, col) = lines.iter().enumerate().find_map(|(i, l)| l.find("OVSR1-").map(|c| (i, l[..c].chars().count()))).expect("the code is on the screen");
    let mut out = String::new();
    for line in &lines[row..] {
        let part: String = line.chars().skip(col).collect();
        let part = part.split('│').next().unwrap_or_default().trim().to_string();
        let groups: Vec<&str> = part.split(' ').filter(|g| !g.is_empty()).collect();
        if groups.is_empty() || !groups.iter().all(|g| *g == "OVSR1-" || g.chars().all(|c| c.is_ascii_uppercase() || ('2'..='7').contains(&c))) {
            break;
        }
        out.push_str(&groups.concat());
    }
    out
}

#[test]
fn n01_phone_access_is_switched_with_a_key_and_shown_in_the_status_line() {
    let (d, port) = phone_daemon(&[]);
    let mut tui = Tui::attach(&d, 140, 40);
    tui.until(10, |a| a.phone.available == Some(true));
    let s = tui.screen();
    assert!(header(&s).contains("phone access off") && header(&s).contains("● connected"), "{s}");
    assert!(!s.to_lowercase().contains("gateway"), "{s}");
    assert!(refused(port), "nothing listens while phone access is off");
    tui.snapshot("phone-access/01-switch-off");

    tui.key(KeyCode::Char('O'));
    tui.until(10, |a| a.phone.enabled);
    let s = tui.screen();
    assert!(header(&s).contains("phone access on") && !header(&s).contains("phone access off"), "{s}");
    assert!(s.contains("Phone access is on"), "{s}");
    let status = d.ctl("gateway.status", json!({}));
    assert_eq!(status["enabled"], true);
    assert_eq!(status["port"], json!(port));
    assert!(!refused(port), "listening after O");
    tui.snapshot("phone-access/02-switch-on");

    // The keys are in the help.
    tui.key(KeyCode::Char('?'));
    let s = tui.screen();
    assert!(s.contains("phone access on / off") && s.contains("devices: pair a phone, revoke, scope"), "{s}");
    tui.snapshot("phone-access/03-help");
    tui.key(KeyCode::Esc);

    // With no phone connected, O turns it off at once.
    tui.key(KeyCode::Char('O'));
    tui.until(10, |a| !a.phone.enabled);
    assert!(header(&tui.screen()).contains("phone access off"));
    assert_eq!(d.ctl("gateway.status", json!({}))["enabled"], false);
    assert!(refused(port));

    // A change made elsewhere (VS Code, ctl) shows here without a key.
    d.ctl("gateway.enable", json!({}));
    tui.until(10, |a| a.phone.enabled);

    // A phone pairs (the owner answers elsewhere). In the grid the terminal only points to D:
    // a key typed for an agent must never answer a phone.
    let code = d.ctl("gateway.pair_start", json!({}))["code"].as_str().unwrap().to_string();
    let mut phone = RefPhone::pair(&code, "Test iPhone", "ios", 60);
    let request = waiting_request(&d, 10);
    let s = tui.until_screen(10, "asks to pair");
    assert!(s.contains("\"Test iPhone\" asks to pair — press D to answer"), "{s}");
    assert_eq!(tui.app.mode, Mode::Grid);
    tui.key(KeyCode::Char('y'));
    assert!(d.ctl("gateway.devices", json!({}))["devices"].as_array().unwrap().is_empty(), "y in the grid pairs nothing");
    d.ctl("gateway.pair_confirm", json!({ "request": request["request"], "accept": true }));
    assert!(phone.wait("paired", 10).is_some());
    tui.until(10, |a| a.phone.connected() == 1);
    let s = tui.screen();
    assert!(header(&s).contains("phone access on · 1 phone"), "{s}");
    tui.snapshot("phone-access/04-one-phone");

    // With a phone connected, O asks first and names it.
    tui.key(KeyCode::Char('O'));
    assert!(matches!(tui.app.mode, Mode::Confirm(Confirm::PhoneOff { .. })), "{:?}", tui.app.mode);
    let s = tui.screen();
    assert!(s.contains("Turn off phone access? 1 phone will be disconnected: Test iPhone.") && s.contains("y / n"), "{s}");
    tui.snapshot("phone-access/05-switch-off-asks");
    tui.key(KeyCode::Char('n'));
    assert_eq!(tui.app.mode, Mode::Grid);
    tui.pump(300);
    assert_eq!(d.ctl("gateway.status", json!({}))["enabled"], true, "n keeps phone access on");
    tui.key(KeyCode::Char('O'));
    tui.key(KeyCode::Char('y'));
    tui.until(10, |a| !a.phone.enabled);
    assert_eq!(phone.wait("notice", 5).map(|v| v["state"].clone()), Some(json!("off")), "the phone is told before it is disconnected");
    assert_eq!(phone.wait("closed", 5).map(|v| v["by"].clone()), Some(json!("mac")));
    assert!(refused(port));
    tui.until(10, |a| a.phone.connected() == 0);
    assert!(header(&tui.screen()).contains("phone access off"));

    // A narrow terminal still says it, and nothing is wider than the terminal.
    tui.resize(80, 24);
    let s = tui.screen();
    assert!(header(&s).contains("phone access off") || header(&s).contains("phones off"), "{s}");
    assert!(s.lines().all(|l| unicode_width::UnicodeWidthStr::width(l) <= 80));
}

#[test]
fn n02_the_devices_panel_lists_phones_changes_scope_and_revokes() {
    let (d, port) = phone_daemon(&[]);
    d.ctl("gateway.enable", json!({}));
    let mut iphone = pair_through_ctl(&d, "Bilal's iPhone", "ios");
    let mut pixel = pair_through_ctl(&d, "Pixel 9", "android");
    let mut tui = Tui::attach(&d, 140, 40);
    tui.until(10, |a| a.phone.devices.len() == 2 && a.phone.connected() == 2);
    assert!(header(&tui.screen()).contains("phone access on · 2 phones"));

    tui.key(KeyCode::Char('D'));
    assert_eq!(tui.app.mode, Mode::Devices);
    tui.pump(300);
    let s = tui.screen();
    assert!(s.contains(" devices ") && s.contains(&format!("phone access on · port {port} · notifications on")), "{s}");
    for text in ["Bilal's iPhone", "iPhone", "Pixel 9", "Android", "● connected", "Full control", "127.0.0.1"] {
        assert!(s.contains(text), "missing {text}:\n{s}");
    }
    assert!(!s.to_lowercase().contains("gateway"), "{s}");
    tui.snapshot("phone-access/06-devices");

    // s changes the scope of the selected phone, and back.
    let at = tui.app.phone.devices.iter().position(|x| x.name == "Pixel 9").unwrap();
    while tui.app.phone.sel != at {
        tui.key(KeyCode::Char('j'));
    }
    let id = tui.app.phone.devices[at].id.clone();
    let scope_of = |d: &Daemon, id: &str| d.ctl("gateway.devices", json!({}))["devices"].as_array().unwrap().iter().find(|x| x["id"] == id).map(|x| x["scope"].as_str().unwrap().to_string()).unwrap();
    tui.key(KeyCode::Char('s'));
    tui.until(10, |a| a.phone.devices.iter().any(|x| x.id == id && x.scope == "watch"));
    assert_eq!(scope_of(&d, &id), "watch");
    let s = tui.screen();
    assert!(s.contains("Watch only") && s.contains("\"Pixel 9\" can watch only"), "{s}");
    tui.snapshot("phone-access/07-watch-only");
    tui.key(KeyCode::Char('s'));
    tui.until(10, |a| a.phone.devices.iter().any(|x| x.id == id && x.scope == "full"));
    assert_eq!(scope_of(&d, &id), "full");
    tui.key(KeyCode::Char('s'));
    tui.until(10, |a| a.phone.devices.iter().any(|x| x.id == id && x.scope == "watch"));

    // N is the Mac's switch for notifications to every phone.
    tui.key(KeyCode::Char('N'));
    tui.until(10, |a| !a.phone.notifications);
    assert_eq!(d.ctl("gateway.status", json!({}))["settings"]["notifications"], false);
    assert!(tui.screen().contains("notifications off"));
    tui.key(KeyCode::Char('N'));
    tui.until(10, |a| a.phone.notifications);
    assert_eq!(d.ctl("gateway.status", json!({}))["settings"]["notifications"], true);

    // x revokes after y/n, naming the phone; n keeps it.
    let at = tui.app.phone.devices.iter().position(|x| x.name == "Pixel 9").unwrap();
    while tui.app.phone.sel != at {
        tui.key(KeyCode::Char('j'));
    }
    tui.key(KeyCode::Char('x'));
    assert!(matches!(tui.app.mode, Mode::Confirm(Confirm::Revoke { .. })));
    let s = tui.screen();
    assert!(s.contains("Revoke \"Pixel 9\"? It is disconnected now. Its key never works again") && s.contains("y / n"), "{s}");
    assert!(s.contains(" devices ") && s.contains("Bilal's iPhone"), "the list stays in view:\n{s}");
    tui.snapshot("phone-access/08-revoke-asks");
    tui.key(KeyCode::Char('n'));
    assert_eq!(tui.app.mode, Mode::Devices);
    tui.pump(300);
    assert_eq!(d.ctl("gateway.status", json!({}))["paired"], 2, "n revokes nothing");
    tui.key(KeyCode::Char('x'));
    tui.key(KeyCode::Char('y'));
    tui.until(10, |a| a.phone.devices.len() == 1);
    assert_eq!(pixel.wait("notice", 5).map(|v| v["state"].clone()), Some(json!("revoked")));
    assert_eq!(pixel.wait("closed", 5).map(|v| v["by"].clone()), Some(json!("mac")), "the revoked phone's session ends");
    let revoked = d.ctl("gateway.devices", json!({}))["devices"].as_array().unwrap().iter().find(|x| x["id"] == id.as_str()).cloned().unwrap();
    assert!(revoked["revoked_ms"].is_i64() && revoked["connected"] == false, "{revoked}");
    let s = tui.screen();
    assert!(!s.contains("Pixel 9 ") && s.contains("Bilal's iPhone") && s.contains("Revoked \"Pixel 9\""), "a revoked phone leaves the list:\n{s}");
    assert_eq!(tui.app.mode, Mode::Devices);
    tui.snapshot("phone-access/09-after-revoke");

    // A phone that goes away shows when it was last seen.
    drop(iphone.child.kill());
    let _ = iphone.child.wait();
    tui.until(10, |a| a.phone.connected() == 0);
    let s = tui.screen();
    assert!(s.contains("○ last seen just now"), "{s}");
    tui.key(KeyCode::Esc);
    assert_eq!(tui.app.mode, Mode::Grid);
    let s = tui.screen();
    assert!(header(&s).contains("phone access on") && !header(&s).contains("1 phone"), "{s}");

    // A phone revoked elsewhere (VS Code) leaves the list here too.
    let last = tui.app.phone.devices[0].id.clone();
    d.ctl("gateway.device_revoke", json!({ "id": last }));
    tui.until(10, |a| a.phone.devices.is_empty());
    tui.key(KeyCode::Char('D'));
    assert!(tui.screen().contains("No phone is paired. Press p to pair one."));
    tui.snapshot("phone-access/10-no-phone");
}

#[test]
fn n03_pairing_shows_the_code_and_the_owner_answers() {
    let (d, port) = phone_daemon(&[]);
    let mut tui = Tui::attach(&d, 140, 46);
    tui.until(10, |a| a.phone.available == Some(true));
    tui.key(KeyCode::Char('D'));
    // Pairing with phone access off offers to turn it on first.
    tui.key(KeyCode::Char('p'));
    assert_eq!(tui.app.mode, Mode::Confirm(Confirm::PhoneOnAndPair));
    assert!(tui.screen().contains("Phone access is off. Turn it on and pair a phone? y / n"));
    tui.key(KeyCode::Char('n'));
    tui.pump(300);
    assert_eq!(d.ctl("gateway.status", json!({}))["enabled"], false, "n changes nothing");
    tui.key(KeyCode::Char('p'));
    tui.key(KeyCode::Char('y'));
    tui.until(10, |a| a.mode == Mode::Pairing && a.pairing.is_some());
    let status = d.ctl("gateway.status", json!({}));
    assert!(status["enabled"] == true && status["pairing"]["open"] == true, "{status}");
    assert!(!refused(port));

    // The code as text, and as a QR code that reads back as exactly that text.
    let code = tui.app.pairing.as_ref().unwrap().code.clone();
    assert!(code.starts_with("OVSR1-") && code.len() > 80, "{code}");
    let buf = tui.draw();
    let s = buffer_text(&buf);
    assert_eq!(typed_code(&s), code, "{s}");
    let (read, quiet) = read_qr(&buf).expect("a QR code is drawn");
    assert_eq!(read, code, "the drawn QR code is the pairing code");
    assert_eq!(quiet, 4, "with room, the quiet zone is the standard's four modules");
    for text in [" pair a phone ", "With ", "1 Open Overseer on your phone.", "2 Scan this code, or type it.", "3 Confirm the phone here, on this Mac.", "Code to type", "esc cancels pairing"] {
        assert!(s.contains(text), "missing {text}:\n{s}");
    }
    // The time left counts down.
    let left = |s: &str| s.lines().find_map(|l| l.split("Works once, for ").nth(1).map(|r| r.split(' ').next().unwrap().to_string())).expect("the time left");
    let first = left(&s);
    assert!(first == "2:00" || first == "1:59", "{first}");
    tui.snapshot("phone-access/11-pairing");
    tui.pump(2100);
    let later = left(&tui.screen());
    assert!(later < first && later.starts_with("1:5"), "{first} then {later}");

    // A phone presents the code: the question names it, its address and its key.
    let mut phone = RefPhone::pair(&code, "Bilal's iPhone", "ios", 60);
    let key = phone.wait("asked", 10).expect("the phone asked")["key"].as_str().unwrap().to_string();
    tui.until(10, |a| matches!(a.mode, Mode::Confirm(Confirm::Pair { .. })));
    let s = tui.screen();
    assert!(s.contains(&format!("Pair \"Bilal's iPhone\"? iPhone · 127.0.0.1 · key {}.", fingerprint(&key))) && s.contains("y / n"), "{s}");
    assert!(s.contains("\"Bilal's iPhone\" is asking to pair."), "the panel stays in view:\n{s}");
    assert!(qr_on_screen(&tui.draw()).is_none(), "a code that was used is no longer shown");
    tui.snapshot("phone-access/12-pair-question");
    assert!(d.ctl("gateway.devices", json!({}))["devices"].as_array().unwrap().is_empty(), "nothing is paired before the owner answers");
    // Only y and n answer.
    tui.key(KeyCode::Char('j'));
    tui.key(KeyCode::Enter);
    assert!(matches!(tui.app.mode, Mode::Confirm(Confirm::Pair { .. })));
    tui.key(KeyCode::Char('y'));
    let paired = phone.wait("paired", 10).expect("the phone's handshake completes");
    assert_eq!(paired["scope"], "full");
    tui.until(10, |a| a.phone.devices.len() == 1 && a.phone.connected() == 1);
    assert_eq!(tui.app.mode, Mode::Pairing);
    let s = tui.screen();
    assert!(s.contains("Paired with \"Bilal's iPhone\".") && s.contains("p pairs another phone"), "{s}");
    tui.snapshot("phone-access/13-paired");
    let devices = d.ctl("gateway.devices", json!({}))["devices"].clone();
    assert_eq!(devices.as_array().unwrap().len(), 1);
    assert_eq!(devices[0]["id"], paired["device"]);
    assert_eq!(devices[0]["fingerprint"], json!(key));

    // Another phone, and the owner says no: nothing is added.
    tui.key(KeyCode::Char('p'));
    tui.until(10, |a| a.pairing.as_ref().is_some_and(|p| p.state == PairingState::Open && p.code != code));
    let second = tui.app.pairing.as_ref().unwrap().code.clone();
    assert_eq!(read_qr(&tui.draw()).unwrap().0, second);
    let mut stranger = RefPhone::pair(&second, "Unknown Pixel", "android", 10);
    tui.until(10, |a| matches!(a.mode, Mode::Confirm(Confirm::Pair { .. })));
    assert!(tui.screen().contains("Pair \"Unknown Pixel\"? Android · 127.0.0.1"));
    tui.key(KeyCode::Char('n'));
    assert!(stranger.wait("refused", 10).is_some(), "the declined phone gets no answer");
    tui.until(10, |a| a.pairing.as_ref().is_some_and(|p| matches!(p.state, PairingState::Over(_))));
    let s = tui.screen();
    assert!(s.contains("\"Unknown Pixel\" was not paired"), "{s}");
    assert_eq!(d.ctl("gateway.status", json!({}))["paired"], 1);
    tui.snapshot("phone-access/14-not-paired");

    // A third phone is answered in VS Code while the question is open here: it closes by itself.
    tui.key(KeyCode::Char('p'));
    tui.until(10, |a| a.pairing.as_ref().is_some_and(|p| p.state == PairingState::Open && p.code != second));
    let third = tui.app.pairing.as_ref().unwrap().code.clone();
    let mut tablet = RefPhone::pair(&third, "Spare Phone", "android", 60);
    tui.until(10, |a| matches!(a.mode, Mode::Confirm(Confirm::Pair { .. })));
    let request = waiting_request(&d, 10);
    d.ctl("gateway.pair_confirm", json!({ "request": request["request"], "accept": true }));
    assert!(tablet.wait("paired", 10).is_some());
    tui.until(10, |a| a.mode == Mode::Pairing && a.phone.devices.len() == 2);
    assert!(tui.screen().contains("Paired with \"Spare Phone\"."));

    // Esc leaves pairing; Devices lists the phones; the status line counts them.
    tui.key(KeyCode::Esc);
    assert_eq!(tui.app.mode, Mode::Devices);
    tui.pump(300);
    let s = tui.screen();
    assert!(s.contains("Bilal's iPhone") && s.contains("Spare Phone") && !s.contains("Unknown Pixel"), "{s}");
    tui.key(KeyCode::Esc);
    assert!(header(&tui.screen()).contains("phone access on · 2 phones"));
}

#[test]
fn n04_a_code_expires_or_is_taken_back_and_a_small_terminal_shows_the_text() {
    let (d, _port) = phone_daemon(&[("OVERSEER_TEST_PAIRING_WINDOW_MS", "4000")]);
    d.ctl("gateway.enable", json!({}));
    let mut tui = Tui::attach(&d, 80, 24);
    tui.until(10, |a| a.phone.enabled);
    tui.key(KeyCode::Char('D'));
    tui.key(KeyCode::Char('p'));
    tui.until(10, |a| a.mode == Mode::Pairing && a.pairing.is_some());
    let code = tui.app.pairing.as_ref().unwrap().code.clone();
    // 80×24 has no room for the QR code: the text is the way in, and the panel says so.
    let buf = tui.draw();
    let s = buffer_text(&buf);
    assert!(qr_on_screen(&buf).is_none(), "{s}");
    assert_eq!(typed_code(&s), code, "{s}");
    assert!(s.contains("Make this window larger to see the QR code."), "{s}");
    assert!(s.lines().count() == 24 && s.lines().all(|l| unicode_width::UnicodeWidthStr::width(l) <= 80), "{s}");
    assert_eq!(code_groups(&code).concat(), code);
    tui.snapshot("phone-access/15-pairing-80x24");
    // A taller window shows the QR code; a narrow one puts it above the words.
    tui.resize(80, 50);
    let buf = tui.draw();
    assert_eq!(read_qr(&buf).expect("a QR code is drawn").0, code, "{}", buffer_text(&buf));
    assert!(buffer_text(&buf).lines().all(|l| unicode_width::UnicodeWidthStr::width(l) <= 80));
    tui.snapshot("phone-access/16-pairing-80x50");

    // After its time the code is over, here and in the daemon.
    tui.until(10, |a| a.pairing.as_ref().is_some_and(|p| matches!(p.state, PairingState::Over(_))));
    let s = tui.screen();
    assert!(s.contains("This code has expired.") && s.contains("p makes a new code"), "{s}");
    assert!(qr_on_screen(&tui.draw()).is_none());
    tui.snapshot("phone-access/17-expired");
    let mut late = RefPhone::pair(&code, "Late Phone", "ios", 5);
    assert!(late.wait("refused", 10).is_some(), "an expired code pairs nothing");
    assert_eq!(d.ctl("gateway.status", json!({}))["paired"], 0);

    // p makes a new code; esc takes it back.
    tui.key(KeyCode::Char('p'));
    tui.until(10, |a| a.pairing.as_ref().is_some_and(|p| p.state == PairingState::Open && p.code != code));
    let second = tui.app.pairing.as_ref().unwrap().code.clone();
    assert_eq!(d.ctl("gateway.status", json!({}))["pairing"]["open"], true);
    tui.key(KeyCode::Esc);
    assert_eq!(tui.app.mode, Mode::Devices);
    tui.pump(400);
    assert!(d.ctl("gateway.status", json!({}))["pairing"].is_null(), "esc takes the code back");
    let closed: Vec<Value> = d.ctl("events.list", json!({ "limit": 1000 }))["events"].as_array().unwrap().iter().filter(|e| e["kind"] == "pairing_closed").cloned().collect();
    assert!(closed.iter().any(|e| e["payload"]["reason"] == "cancelled on the Mac"), "{closed:?}");
    let mut other = RefPhone::pair(&second, "Other Phone", "ios", 5);
    assert!(other.wait("refused", 10).is_some(), "a code taken back pairs nothing");
    assert_eq!(d.ctl("gateway.status", json!({}))["paired"], 0);

    // Quitting with a code on screen takes it back too.
    tui.key(KeyCode::Char('p'));
    tui.until(10, |a| a.mode == Mode::Pairing && a.pairing.as_ref().is_some_and(|p| p.state == PairingState::Open));
    assert_eq!(d.ctl("gateway.status", json!({}))["pairing"]["open"], true);
    tui.key_mod(KeyCode::Char('c'), crossterm::event::KeyModifiers::CONTROL);
    assert!(tui.app.quit);
    tui.pump(400);
    assert!(d.ctl("gateway.status", json!({}))["pairing"].is_null(), "quitting takes the code back");
}

#[test]
fn n05_the_real_binary_switches_phone_access_in_a_terminal() {
    let (d, port) = phone_daemon(&[]);
    let bin = env!("CARGO_BIN_EXE_overseer-tui");
    let helper = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/pty_run.py");
    // O turns it on, D opens Devices, p shows a code, esc twice and q leave.
    let out = Command::new("python3")
        .arg(helper)
        .args(["46", "140", "2.5", "O", "1.5", "D", "1.0", "p", "2.0", "\u{1b}", "1.0", "\u{1b}", "0.8", "q", "--", bin, "--daemon"])
        .arg(&d.bin)
        .arg("--home")
        .arg(d.home.path())
        .env("TERM", "xterm-256color")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    assert!(out.status.success(), "{text}");
    for shown in ["phone access off", "phone access on", "devices", "pair a phone", "OVSR1-", "Works once, for"] {
        assert!(text.contains(shown), "the terminal showed {shown:?}");
    }
    // The code is drawn black on white (fixed colours), whatever the terminal's own colours are.
    assert!(text.contains("\u{1b}[38;5;16") && text.contains("48;5;231"), "the QR code's colours");
    assert!(text.contains('█') && text.contains('▀') && text.contains('▄'));
    let status = d.ctl("gateway.status", json!({}));
    assert_eq!(status["enabled"], true, "O in the real terminal turned phone access on");
    assert_eq!(status["port"], json!(port));
    assert!(status["pairing"].is_null(), "esc took the code back");
    let help = Command::new(bin).arg("--help").output().unwrap();
    let help = String::from_utf8_lossy(&help.stdout).to_string();
    assert!(help.contains("phone access on / off") && help.contains("devices: pair a phone, revoke, scope"), "{help}");
}
