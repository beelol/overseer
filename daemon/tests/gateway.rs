//! Gate N: the phone gateway, tested with a real daemon and a reference phone.
//! Wire format: docs/rfcs/phone-remote-protocol.md. Criteria: AC-116 to AC-122, AC-125, AC-141.

mod common;

use common::phone::{self, closes_silently, contains, first_frame, frame, hello_payload, open, pair, parse_code, tap, Keys, Phone};
use common::*;
use futures_util::SinkExt;
use serde_json::{json, Value};
use std::time::{Duration, Instant};
use tokio_tungstenite::tungstenite::Message;

fn fixture(name: &str) -> String {
    repo_root().join("fixtures").join(name).display().to_string()
}

/// A daemon whose Claude harness is the synthetic fixture, with the network advertiser off.
fn daemon(mode: &str, extra: &[(&str, &str)]) -> Daemon {
    let claude = fixture("fake-harness/claude-fixture.js");
    let mut env: Vec<(&str, &str)> = vec![("OVERSEER_CLAUDE_PATH", &claude), ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE,FIXTURE_SLOW_MS"), ("FIXTURE_MODE", mode), ("OVERSEER_GATEWAY_MDNS", "off")];
    env.extend_from_slice(extra);
    Daemon::start(&env)
}

fn refused(port: u16) -> bool {
    std::net::TcpStream::connect_timeout(&format!("127.0.0.1:{port}").parse().unwrap(), Duration::from_millis(500)).is_err()
}

fn log(d: &Daemon) -> String {
    std::fs::read_to_string(d.home.path().join("overseerd.log")).unwrap_or_default()
}

fn events_of(d: &Daemon, kind: &str) -> Vec<Value> {
    d.call("events.list", json!({"limit": 5000}))["events"].as_array().unwrap().iter().filter(|e| e["kind"] == kind).cloned().collect()
}

// ---------------------------------------------------------------- AC-116

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ac116_phone_access_is_off_until_the_mac_turns_it_on() {
    let d = daemon("echo", &[]);
    let status = d.call("gateway.status", json!({}));
    assert_eq!(status["enabled"], false);
    assert!(status["port"].is_null() && status["addresses"].as_array().unwrap().is_empty());
    assert!(refused(phone::free_port()) && refused(47810) || status["enabled"] == false, "nothing listens by default");
    // Pairing cannot start while phone access is off.
    let err = d.try_call("gateway.pair_start", json!({})).unwrap_err();
    assert!(err.contains("turn phone access on"), "{err}");

    let port = phone::enable(&d);
    assert!(!refused(port), "listening after it is turned on");
    let (mut a, mut paired) = pair(&d, "Phone A").await;
    assert_eq!(a.call("hello", json!({"client": "phone"})).await["device"]["scope"], "full");
    let mut b = Phone::connect(&mut paired).await.unwrap();
    assert_eq!(d.call("gateway.status", json!({}))["sessions"], 2);

    // A phone cannot switch phone access, read its state, or start pairing.
    for method in ["gateway.disable", "gateway.enable", "gateway.status", "gateway.pair_start", "gateway.settings"] {
        let reply = b.act(method, json!({})).await;
        assert_eq!(Phone::code(&reply), "mac_only", "{method}: {reply}");
    }
    assert_eq!(d.call("gateway.status", json!({}))["enabled"], true);

    // Off: the phones are told, then every session and the listener close within a second.
    let started = Instant::now();
    let off = d.call("gateway.disable", json!({}));
    assert_eq!(off["enabled"], false);
    assert_eq!(off["closed"], 2);
    assert!(a.ends_within(Duration::from_secs(1)).await && b.ends_within(Duration::from_secs(1)).await);
    assert!(started.elapsed() < Duration::from_secs(1), "closed in {:?}", started.elapsed());
    assert_eq!(a.notices(), vec!["off"], "the notice arrives before the close");
    assert_eq!(b.notices(), vec!["off"]);
    assert!(refused(port), "the listener is closed");
    assert!(Phone::connect(&mut paired).await.is_err());

    // The setting is the daemon's: it holds across a restart with no UI connected.
    let mut d = d;
    d.kill9();
    d.spawn();
    assert_eq!(d.call("gateway.status", json!({}))["enabled"], false);
    assert!(refused(port));
    d.call("gateway.enable", json!({"port": port}));
    d.kill9();
    d.spawn();
    assert_eq!(d.call("gateway.status", json!({}))["enabled"], true, "on again after a restart");

    // On again: the paired phone reconnects by itself, with no pairing.
    let mut again = Phone::connect(&mut paired).await.expect("reconnects without pairing");
    assert_eq!(again.call("hello", json!({"client": "phone"})).await["device"]["id"], json!(paired.device));
    let kinds: Vec<String> = events_of(&d, "gateway_state").iter().map(|e| e["payload"]["state"].as_str().unwrap().to_string()).collect();
    assert_eq!(kinds, vec!["on", "off", "on", "on"], "every change is an event");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ac116_nothing_answers_before_a_session_is_authenticated() {
    let d = daemon("echo", &[]);
    let port = phone::enable(&d);
    let wait = Duration::from_secs(2);
    let junk: Vec<Vec<u8>> = vec![
        vec![],                                            // empty
        vec![0x01],                                        // truncated: version only
        vec![0x01, 0x01],                                  // truncated: no handshake
        vec![0x02, 0x01, 1, 2, 3],                         // wrong version
        vec![0x01, 0x09, 1, 2, 3],                         // unknown kind
        first_frame(0x01, &[0u8; 96]),                     // a handshake that cannot decrypt
        first_frame(0x02, &[7u8; 96]),                     // pairing while pairing is closed
        br#"{"id":1,"method":"hello","params":{}}"#.to_vec(), // the plain protocol
        vec![0x01; 5000],                                  // oversized handshake
    ];
    for bytes in &junk {
        let mut ws = open(port).await.unwrap();
        ws.send(Message::Binary(bytes.clone().into())).await.unwrap();
        assert!(closes_silently(&mut ws, wait).await, "closed without a reply: {:?}", &bytes[..bytes.len().min(8)]);
    }
    // A text frame is not part of the protocol.
    let mut ws = open(port).await.unwrap();
    ws.send(Message::Text(r#"{"id":1,"method":"state"}"#.into())).await.unwrap();
    assert!(closes_silently(&mut ws, wait).await);
    // The daemon is well, nothing was served, and every refusal is in the log.
    assert_eq!(d.call("hello", json!({}))["protocol"], 1);
    assert_eq!(d.call("gateway.status", json!({}))["sessions"], 0);
    let text = log(&d);
    for reason in ["the handshake did not decrypt", "malformed handshake", "pairing is not open", "no handshake"] {
        assert!(text.contains(reason), "{reason} is logged: {text}");
    }
    // That was ten failures from one address within a minute: it is not served for now.
    assert!(open(port).await.is_err(), "an address that keeps failing is not served for a minute");
    assert!(d.call("hello", json!({})).is_object(), "the local socket is not affected");

    // Another daemon: a frame larger than the transport allows, another path, and silence.
    let d = daemon("echo", &[("OVERSEER_TEST_GATEWAY_IDLE_MS", "400")]);
    let port = phone::enable(&d);
    let mut ws = open(port).await.unwrap();
    let _ = ws.send(Message::Binary(vec![1u8; 200_000].into())).await;
    assert!(closes_silently(&mut ws, wait).await);
    assert!(tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}/other")).await.is_err(), "only the gateway's path is a WebSocket");
    assert!(tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}/")).await.is_err());
    // A paired phone that goes silent is dropped (test-only: a shorter idle time).
    let (mut quiet, _) = pair(&d, "Quiet Phone").await;
    assert!(quiet.ends_within(Duration::from_secs(2)).await, "a silent session is ended");
    assert!(log(&d).contains("silent for a minute"));
    assert_eq!(d.call("gateway.status", json!({}))["sessions"], 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ac116_peers_outside_the_local_network_are_refused() {
    // Test-only: every peer is seen as a public address. It can only refuse more.
    let d = daemon("echo", &[("OVERSEER_TEST_GATEWAY_PEER", "8.8.8.8")]);
    let port = phone::enable(&d);
    assert!(open(port).await.is_err(), "no WebSocket for a public address");
    assert!(log(&d).contains("refused 8.8.8.8: outside the local network"));
    // A range the owner adds is served.
    let d = daemon("echo", &[("OVERSEER_TEST_GATEWAY_PEER", "100.64.0.7")]);
    let port = phone::enable(&d);
    assert!(open(port).await.is_err());
    let err = d.try_call("gateway.settings", json!({"allow": ["nonsense"]})).unwrap_err();
    assert!(err.contains("not a range"), "{err}");
    d.call("gateway.settings", json!({"allow": ["100.64.0.0/10"]}));
    assert!(open(port).await.is_ok(), "the added range is served");
}

// ---------------------------------------------------------------- AC-117

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ac117_pairing_needs_the_mac() {
    let d = daemon("echo", &[]);
    let port = phone::enable(&d);
    let started = d.call("gateway.pair_start", json!({}));
    let text = started["code"].as_str().unwrap();
    let code = parse_code(text);
    assert_eq!(code.port, port);
    assert_eq!(code.secret.len(), 16);
    assert!(code.addresses.contains(&"127.0.0.1".to_string()));
    assert_eq!(started["valid_ms"].as_u64().unwrap() / 1000, 119, "two minutes");
    let fingerprint = started["fingerprint"].as_str().unwrap().to_string();
    assert_eq!(noise::fingerprint(&code.gateway_public), fingerprint, "the code carries the gateway's key");

    // Declined on the Mac: nothing is paired and the phone hears nothing.
    let keys = Keys::new();
    let attempt = {
        let (keys, code) = (keys.clone(), parse_code(text));
        tokio::spawn(async move { Phone::pair_on(port, &code, &keys, "Declined Phone", Duration::from_secs(5)).await })
    };
    let waiting = phone::waiting_request(&d, Duration::from_secs(5)).await.unwrap();
    assert_eq!(waiting["name"], "Declined Phone");
    assert_eq!(waiting["platform"], "ios");
    assert_eq!(waiting["fingerprint"], json!(noise::fingerprint(&keys.public)));
    let answer = d.call("gateway.pair_confirm", json!({"request": waiting["request"], "accept": false}));
    assert_eq!(answer["accepted"], false);
    assert!(attempt.await.unwrap().is_err(), "a declined phone is not paired");
    assert!(d.call("gateway.devices", json!({}))["devices"].as_array().unwrap().is_empty());
    // The secret was used: the same code pairs nothing, even for another phone.
    assert!(Phone::pair_on(port, &parse_code(text), &Keys::new(), "Second", Duration::from_secs(2)).await.is_err(), "a used secret pairs nothing");
    assert!(d.call("gateway.status", json!({}))["pairing"].is_null(), "pairing closed after use");

    // Accepted: the device is stored with its own key, and its session starts at once.
    let (mut paired_phone, paired) = pair(&d, "Bilal's iPhone").await;
    let hello = paired_phone.call("hello", json!({"client": "phone"})).await;
    assert_eq!(hello["device"]["name"], "Bilal's iPhone");
    assert_eq!(hello["gateway"]["fingerprint"], json!(fingerprint));
    let devices = d.call("gateway.devices", json!({}))["devices"].as_array().unwrap().clone();
    assert_eq!(devices.len(), 1);
    assert_eq!(devices[0]["id"], json!(paired.device));
    assert_eq!(devices[0]["fingerprint"], json!(noise::fingerprint(&paired.keys.public)));
    assert_eq!(devices[0]["connected"], true);
    assert_eq!(devices[0]["scope"], "full");
    assert!(devices[0].get("public_key").is_none(), "keys are not listed");
    // A code that was used cannot be replayed to pair again.
    assert!(d.call("gateway.status", json!({}))["pairing"].is_null());

    // A wrong secret pairs nothing; five failures close pairing until it is started again.
    let started = d.call("gateway.pair_start", json!({}));
    let good = parse_code(started["code"].as_str().unwrap());
    for n in 1..=5 {
        let mut wrong = parse_code(started["code"].as_str().unwrap());
        wrong.secret = vec![n as u8; 16];
        assert!(Phone::pair_on(port, &wrong, &Keys::new(), "Guess", Duration::from_secs(2)).await.is_err());
    }
    assert!(d.call("gateway.status", json!({}))["pairing"].is_null(), "closed after five failures");
    assert!(Phone::pair_on(port, &good, &Keys::new(), "Too late", Duration::from_secs(2)).await.is_err(), "the right secret no longer works");
    assert_eq!(events_of(&d, "pairing_closed").last().unwrap()["payload"]["reason"], "too many failed attempts");
    assert_eq!(d.call("gateway.devices", json!({}))["devices"].as_array().unwrap().len(), 1);

    // Pairing starts on the Mac only; a phone cannot open it, confirm it or list devices.
    for method in ["gateway.pair_start", "gateway.pair_confirm", "gateway.pair_cancel", "gateway.devices", "gateway.device_revoke", "gateway.device_scope", "gateway.device_rename"] {
        let reply = paired_phone.act(method, json!({"id": paired.device, "scope": "full", "request": "x", "accept": true, "name": "n"})).await;
        assert_eq!(Phone::code(&reply), "mac_only", "{method}");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ac117_an_expired_code_and_an_unanswered_request_pair_nothing() {
    // Test-only: a shorter pairing window and a shorter wait for the owner.
    let d = daemon("echo", &[("OVERSEER_TEST_PAIRING_WINDOW_MS", "400"), ("OVERSEER_TEST_CONFIRM_WAIT_MS", "500")]);
    let port = phone::enable(&d);
    let started = d.call("gateway.pair_start", json!({}));
    let code = parse_code(started["code"].as_str().unwrap());
    tokio::time::sleep(Duration::from_millis(600)).await;
    assert!(Phone::pair_on(port, &code, &Keys::new(), "Late", Duration::from_secs(2)).await.is_err(), "an expired secret pairs nothing");
    // Nobody confirms: the phone is not paired and the code is spent.
    let started = d.call("gateway.pair_start", json!({}));
    let code = parse_code(started["code"].as_str().unwrap());
    assert!(Phone::pair_on(port, &code, &Keys::new(), "Unanswered", Duration::from_secs(3)).await.is_err());
    assert!(d.call("gateway.devices", json!({}))["devices"].as_array().unwrap().is_empty());
    assert_eq!(events_of(&d, "pairing_closed").last().unwrap()["payload"]["reason"], "declined or not confirmed");
    // Cancelled on the Mac.
    let started = d.call("gateway.pair_start", json!({}));
    d.call("gateway.pair_cancel", json!({}));
    assert!(Phone::pair_on(port, &parse_code(started["code"].as_str().unwrap()), &Keys::new(), "Cancelled", Duration::from_secs(2)).await.is_err());
}

// ---------------------------------------------------------------- AC-118

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ac118_sessions_are_encrypted_and_both_sides_are_authenticated() {
    let d = daemon("echo", &[]);
    let port = phone::enable(&d);
    let (first, mut paired) = pair(&d, "Capture Phone").await;
    drop(first);
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    // Known to Overseer, so a phone may start agents there.
    d.wait_done(&run_id(&d.call("task.create", json!({"repo": repo, "harness": "claude", "prompt": "seed", "title": "seed"}))), 15);

    // A packet capture of a live session: every byte both ways through a recording forwarder.
    let capture = tap(port).await;
    paired.port = capture.port;
    let mut p = Phone::connect(&mut paired).await.unwrap();
    p.call("hello", json!({"client": "phone"})).await;
    let secret_prompt = "PLAINTEXT-MARKER-prompt-4711";
    let created = p.act("task.create", json!({"repo": repo, "harness": "claude", "prompt": secret_prompt, "title": "captured"})).await;
    assert!(created.get("error").is_none(), "{created}");
    let run = created["result"]["run"]["id"].as_str().unwrap().to_string();
    p.send(&json!({"id": 900, "method": "events.subscribe", "params": {"after": 0}})).await.unwrap();
    d.wait_done(&run, 15);
    let state = p.call("state", json!({})).await;
    assert!(state.to_string().contains(secret_prompt), "the phone reads the prompt inside the session");
    let output: Vec<String> = d.events(&run).iter().filter_map(|e| e["payload"]["text"].as_str().map(str::to_string)).collect();
    assert!(output.iter().any(|t| t.contains("ECHO")), "the agent answered: {output:?}");
    tokio::time::sleep(Duration::from_millis(300)).await;
    let wire = capture.bytes.lock().unwrap().clone();
    assert!(wire.len() > 4000, "the capture holds the session ({} bytes)", wire.len());
    assert!(contains(&wire, "Upgrade"), "the capture is real: it holds the WebSocket upgrade");
    for readable in [secret_prompt, "task.create", "events.subscribe", "hello", "\"method\"", "\"params\"", "\"result\"", "ECHO", "request_id", "Capture Phone", &paired.device, "captured", "overseer"] {
        assert!(!contains(&wire, readable), "{readable:?} is readable on the wire");
    }
    paired.port = port;

    // An unknown device key is refused without a reply, and so is a known id with another key.
    let stranger = Keys::new();
    assert!(Phone::session_on(port, &paired.gateway_public, &stranger, &paired.device, phone::now_ms()).await.is_err());
    // A gateway with the wrong key: the phone's handshake is made for another key, so it fails.
    let other_gateway = Keys::new();
    assert!(Phone::session_on(port, &other_gateway.public, &paired.keys, &paired.device, phone::now_ms() + 10).await.is_err());

    // A recorded handshake replayed is refused: its counter is not higher than the last one.
    let counter = phone::now_ms() + 1000;
    let mut hs = noise::initiator(noise::Kind::Session, &paired.keys.private, &paired.gateway_public, None).unwrap();
    let mut m1 = vec![0u8; 1024];
    let n = hs.write_message(&hello_payload(&paired.device, "x", counter), &mut m1).unwrap();
    let recorded = first_frame(noise::KIND_SESSION, &m1[..n]);
    let mut ws = open(port).await.unwrap();
    ws.send(Message::Binary(recorded.clone().into())).await.unwrap();
    assert!(frame(&mut ws, Duration::from_secs(3)).await.is_some(), "the first use is answered");
    drop(ws);
    let mut ws = open(port).await.unwrap();
    ws.send(Message::Binary(recorded.into())).await.unwrap();
    assert!(closes_silently(&mut ws, Duration::from_secs(3)).await, "the replayed handshake is refused");
    assert!(log(&d).contains("a replayed handshake"));
    paired.counter = counter;

    // A tampered frame, a replayed frame and a frame out of order each end the session.
    for case in ["tampered", "replayed", "reordered"] {
        let mut p = Phone::connect(&mut paired).await.unwrap();
        p.call("hello", json!({"client": "phone"})).await;
        let one = noise::seal(&mut p.transport, br#"{"id":50,"method":"ping","params":{}}"#).unwrap().remove(0);
        let two = noise::seal(&mut p.transport, br#"{"id":51,"method":"ping","params":{}}"#).unwrap().remove(0);
        match case {
            "tampered" => {
                let mut bad = one.clone();
                let last = bad.len() - 3;
                bad[last] ^= 0x40;
                p.ws.send(Message::Binary(bad.into())).await.unwrap();
            }
            "replayed" => {
                p.ws.send(Message::Binary(one.clone().into())).await.unwrap();
                assert_eq!(p.reply_to(50, Duration::from_secs(3)).await.unwrap()["id"], 50);
                p.ws.send(Message::Binary(one.clone().into())).await.unwrap();
            }
            _ => p.ws.send(Message::Binary(two.clone().into())).await.unwrap(),
        }
        assert!(p.ends_within(Duration::from_secs(2)).await, "a {case} frame ends the session");
        assert!(p.inbox.iter().all(|m| m["id"] != 51 && (case == "replayed" || m["id"] != 50)), "nothing ran for the {case} frame: {:?}", p.inbox);
    }
    assert!(log(&d).contains("a frame that did not decrypt"));
    // The pairing still holds after all of that.
    assert!(Phone::connect(&mut paired).await.is_ok());
}

// ---------------------------------------------------------------- AC-119

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ac119_devices_scopes_and_revoking() {
    let d = daemon("slow", &[("FIXTURE_SLOW_MS", "30000")]);
    phone::enable(&d);
    let r = tmp();
    let repo = common::repo(&r.path().join("repo"));
    let created = d.call("task.create", json!({"repo": repo, "harness": "claude", "prompt": "long", "title": "long"}));
    let run = run_id(&created);
    d.wait_status(&run, |s| s == "running", 15);
    let (mut full, full_paired) = pair(&d, "Full Phone").await;
    let (mut watch, watch_paired) = pair(&d, "Watch Phone").await;
    d.call("gateway.device_scope", json!({"id": watch_paired.device, "scope": "watch"}));
    assert!(d.try_call("gateway.device_scope", json!({"id": watch_paired.device, "scope": "admin"})).unwrap_err().contains("full or watch"));

    // Every method of the daemon, from the protocol description: the table drives the test,
    // so a method added later is covered without touching this test.
    let description: Value = serde_json::from_str(&std::fs::read_to_string(repo_root().join("protocol/protocol.json")).unwrap()).unwrap();
    let methods: Vec<(String, String)> = description["methods"].as_object().unwrap().iter().filter(|(_, m)| m["planned"].as_bool() != Some(true)).map(|(k, m)| (k.clone(), m["class"].as_str().unwrap().to_string())).collect();
    assert!(methods.len() >= 55, "the whole method list: {}", methods.len());
    let turns_before = d.call("run.turns", json!({"run_id": run})).as_array().unwrap().len();
    let mut counted = std::collections::BTreeMap::new();
    for (method, class) in &methods {
        if method == "events.subscribe" {
            continue;
        }
        *counted.entry(class.clone()).or_insert(0) += 1;
        // Parameters that would really act if the request got through.
        let params = json!({"run_id": run, "prompt": "from the phone", "task_id": created["task"]["id"], "workspace_id": created["workspace"]["id"], "id": "system-claude", "name": "n", "harness": "claude", "provider": "anthropic", "repo": repo, "path": repo, "base": "HEAD", "request_id": "none", "allow": true, "client": "phone"});
        let from_watch = watch.act(method, params.clone()).await;
        match class.as_str() {
            "control" => assert_eq!(Phone::code(&from_watch), "watch_only", "{method} from a watch-only phone: {from_watch}"),
            "mac_only" => assert_eq!(Phone::code(&from_watch), "mac_only", "{method} from a watch-only phone: {from_watch}"),
            _ => assert!(!["watch_only", "mac_only", "unknown_method"].contains(&Phone::code(&from_watch).as_str()), "{method} is open to a watch-only phone: {from_watch}"),
        }
        if class == "mac_only" {
            let from_full = full.act(method, params).await;
            assert_eq!(Phone::code(&from_full), "mac_only", "{method} from a full-control phone: {from_full}");
        }
    }
    assert!(counted["control"] >= 15 && counted["mac_only"] >= 15 && counted["read"] >= 20, "{counted:?}");
    // Nothing the watch-only phone sent changed anything.
    assert_eq!(d.call("run.turns", json!({"run_id": run})).as_array().unwrap().len(), turns_before);
    assert_eq!(d.run(&run)["status"], "running");
    assert_eq!(d.call("gateway.status", json!({}))["enabled"], true);
    assert!(d.call("state", json!({}))["tasks"].as_array().unwrap().iter().all(|t| t["archived_ms"].is_null()));
    // Unknown and planned methods are refused.
    for method in ["no.such_method", "workspace.file", "review.reject"] {
        assert_eq!(Phone::code(&full.act(method, json!({})).await), "unknown_method", "{method}");
    }
    // A changing request without a request id is refused.
    let bare = full.ask("run.interrupt", json!({"run_id": run}), None).await.unwrap();
    assert_eq!(Phone::code(&bare), "request_id_required");
    assert_eq!(d.run(&run)["status"], "running");

    // From a phone, agents start only in repositories Overseer knows, and never as a program.
    let other = common::repo(&r.path().join("unknown"));
    assert_eq!(Phone::code(&full.act("task.create", json!({"repo": other, "harness": "claude", "prompt": "x"})).await), "mac_only");
    assert_eq!(Phone::code(&full.act("task.create", json!({"repo": repo, "harness": "generic", "program": "/bin/sh", "args": ["-c", "touch /tmp/ovs-should-not-exist"], "prompt": ""})).await), "mac_only");
    assert_eq!(Phone::code(&full.ask("repo.inspect", json!({"path": other}), None).await.unwrap()), "mac_only");
    assert_eq!(full.call("repo.known", json!({})).await["repos"][0]["root"], json!(repo));

    // What a phone does is an event with the phone as its source.
    let stopped = full.act("run.interrupt", json!({"run_id": run})).await;
    assert!(stopped.get("error").is_none(), "{stopped}");
    d.wait_done(&run, 20);
    let commands = events_of(&d, "remote_command");
    let last = commands.last().unwrap();
    assert_eq!(last["source"], "phone:Full Phone");
    assert_eq!(last["payload"]["method"], "run.interrupt");
    assert_eq!(last["payload"]["device"], json!(full_paired.device));
    assert_eq!(last["run_id"], json!(run));
    assert!(commands.iter().all(|e| e["source"] == "phone:Full Phone"), "refused requests are not commands");

    // The Mac lists devices with what it needs to tell them apart.
    let listed = d.call("gateway.devices", json!({}))["devices"].as_array().unwrap().clone();
    assert_eq!(listed.len(), 2);
    for dev in &listed {
        for key in ["id", "name", "platform", "scope", "paired_ms", "last_seen_ms", "address", "connected", "fingerprint"] {
            assert!(!dev[key].is_null(), "{key} is listed: {dev}");
        }
    }
    assert_eq!(listed.iter().find(|x| x["id"] == json!(watch_paired.device)).unwrap()["scope"], "watch");

    // Revoking during a live stream ends the session within a second, and the key never works again.
    watch.send(&json!({"id": 70, "method": "events.subscribe", "params": {"after": 0}})).await.unwrap();
    assert!(watch.reply_to(70, Duration::from_secs(5)).await.is_ok());
    let started = Instant::now();
    let revoked = d.call("gateway.device_revoke", json!({"id": watch_paired.device}));
    assert_eq!(revoked["closed"], 1);
    assert!(watch.ends_within(Duration::from_secs(1)).await, "ended in {:?}", started.elapsed());
    assert!(started.elapsed() < Duration::from_secs(1));
    assert_eq!(watch.notices(), vec!["revoked"]);
    let mut gone = watch_paired.clone();
    assert!(Phone::connect(&mut gone).await.is_err(), "a revoked key is refused");
    assert!(log(&d).contains("unknown or revoked device"));
    assert!(d.try_call("gateway.device_scope", json!({"id": watch_paired.device, "scope": "full"})).is_err(), "a revoked device cannot be given a scope");
    // The other phone is untouched.
    assert_eq!(full.call("ping", json!({})).await["now_ms"].as_i64().is_some(), true);
    assert_eq!(events_of(&d, "device_revoked").len(), 1);
}
