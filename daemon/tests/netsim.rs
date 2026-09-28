//! Continuity (AC-83) with the network SIMULATED and everything else real: the real daemon at its
//! real cadence (a check every 5 seconds, probes every 30 seconds while offline), and its real
//! probes (HTTP over TCP, with their real timeouts and failure classes) sent to a loopback network
//! the test cuts the way Wi-Fi going off cuts it (`common/netsim.rs`). Only the operating system's
//! own answer is replaced (a file holding `connected` or `none`), because producing that needs the
//! Wi-Fi itself; that one step is the owner's (`test/local/wifi-live.js`).

mod common;
#[path = "common/netsim.rs"]
mod netsim;
#[path = "common/world.rs"]
mod world;
#[path = "common/ollama.rs"]
mod ollama;

use common::*;
use netsim::{Mode, NetSim};
use serde_json::{json, Value};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use world::*;

fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as i64
}

/// Waits for a connection state, polling every 50 ms, up to `limit`.
fn until(d: &Daemon, what: &str, limit: Duration, pred: impl Fn(&Value) -> bool) -> Value {
    let started = Instant::now();
    loop {
        let s = conn(d);
        if pred(&s) {
            return s;
        }
        if started.elapsed() >= limit {
            for e in all_events(d, "connection") {
                let st = &e["payload"]["status"];
                eprintln!("  {} {} ({}) probed {} openai {} anthropic {}", e["ts"], st["state"], st["reason"], st["probed_ms"], st["providers"]["openai"]["reason"], st["providers"]["anthropic"]["reason"]);
            }
            panic!("the connection never became {what} within {limit:?}; it is {} ({})", s["state"], s["reason"]);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// The first connection event at or after `since` that says `state`.
fn event_after(d: &Daemon, since: i64, state: &str) -> Value {
    all_events(d, "connection").into_iter().find(|e| e["ts"].as_i64().unwrap() >= since && e["payload"]["status"]["state"] == state).unwrap_or_else(|| panic!("no {state} event after {since}"))
}

#[test]
fn ac83_a_simulated_wifi_toggle_is_seen_through_the_real_probes() {
    let dir = tmp();
    let system = dir.path().join("system");
    write_whole(&system, "connected");
    // The internet (the baseline by name and by IP) and each provider's hosts, cut separately.
    let (internet, openai, anthropic) = (NetSim::start(), NetSim::start(), NetSim::start());
    let urls = json!({"baseline:name": internet.url("/generate_204"), "baseline:ip": internet.url("/"), "openai": [openai.url("/")], "anthropic": [anthropic.url("/")]}).to_string();
    let ollama = no_ollama();
    let d = Daemon::start(&[
        ("OVERSEER_CONTINUITY_PROBES", "on"),
        ("OVERSEER_TEST_SYSTEM_NET", system.to_str().unwrap()),
        ("OVERSEER_TEST_PROBE_URLS", &urls),
        ("OVERSEER_OLLAMA_URL", &ollama),
        ("OVERSEER_OLLAMA_CANDIDATES", "/nonexistent/ollama"),
    ]);
    let everything = |m: Mode| [&internet, &openai, &anthropic].iter().for_each(|n| n.set(m));

    let s = until(&d, "online", Duration::from_secs(20), |s| s["state"] == "online" && s["baseline"]["by_name"]["ok"] == true);
    assert_eq!(s["system"]["detail"], "simulated: connected");
    assert_eq!(s["baseline"]["by_name"]["reason"], "answered 204", "the real probe reached the simulated internet");

    // Wi-Fi off: the interface goes down, so the system says so and every connection is refused.
    let off = now_ms();
    write_whole(&system, "none");
    everything(Mode::Refuse);
    until(&d, "offline", Duration::from_secs(15), |s| s["state"] == "offline");
    let e = event_after(&d, off, "offline");
    let gap = e["ts"].as_i64().unwrap() - off;
    assert_eq!(e["payload"]["status"]["reason"], "no network (system)");
    assert!(gap <= 10_000, "offline {gap} ms after the system's signal; the criterion allows 10 s");
    eprintln!("offline {gap} ms after the system said no network");

    // Wi-Fi on, but nothing comes back yet (associating, no address): the system says connected,
    // the probes time out, and Overseer stays offline; the system's word alone is not enough.
    let on = now_ms();
    write_whole(&system, "connected");
    everything(Mode::Blackhole);
    let s = until(&d, "offline for want of a route", Duration::from_secs(25), |s| s["reason"] == "no working connection (no route to the internet)");
    assert_eq!((s["state"].as_str(), s["baseline"]["by_name"]["reason"].as_str(), s["baseline"]["by_ip"]["reason"].as_str()), (Some("offline"), Some("timeout"), Some("timeout")));
    assert!(!all_events(&d, "connection").iter().any(|e| e["ts"].as_i64().unwrap() >= on && e["payload"]["status"]["state"] == "online"), "never online while the probes fail");

    // The link works again: online once the next probe round and the reading after it agree.
    let up = now_ms();
    everything(Mode::Online);
    let s = until(&d, "online again", Duration::from_secs(50), |s| s["state"] == "online");
    let e = event_after(&d, up, "online");
    assert_eq!(e["payload"]["status"]["reason"], "connected");
    assert!(s["status"].is_null() && s["baseline"]["by_name"]["ok"] == true && s["providers"]["openai"]["reachable"] == true && s["providers"]["anthropic"]["reachable"] == true, "the system, the baseline and the providers agree: {s}");
    assert!(e["payload"]["status"]["probed_ms"].as_i64().unwrap() >= up, "online only after a probe round that ran with the link up");
    eprintln!("online {} ms after the link came back", e["ts"].as_i64().unwrap() - up);

    // One provider's hosts refuse while the internet works: degraded, naming it, with the real
    // failure class; another that never answers times out. (Check now forces the probe round that
    // would otherwise come within 5 minutes.)
    openai.set(Mode::Refuse);
    d.call("connection.check", json!({}));
    let s = until(&d, "degraded", Duration::from_secs(20), |s| s["state"] == "degraded");
    assert_eq!((s["reason"].as_str(), s["providers"]["openai"]["reason"].as_str()), (Some("OpenAI unreachable"), Some("connect")));
    anthropic.set(Mode::Blackhole);
    d.call("connection.check", json!({}));
    let s = until(&d, "both providers down", Duration::from_secs(25), |s| s["reason"] == "Claude and OpenAI unreachable");
    assert_eq!((s["acts_offline"].as_bool(), s["providers"]["anthropic"]["reason"].as_str()), (Some(true), Some("timeout")));

    // Both come back while a slow round that began before is still waiting on its timeout: that
    // round's stale answer must not undo the fresh one (it did, until the rounds were ordered).
    std::thread::scope(|scope| {
        let slow = scope.spawn(|| d.call("connection.check", json!({})));
        std::thread::sleep(Duration::from_millis(1000));
        openai.set(Mode::Online);
        anthropic.set(Mode::Online);
        d.call("connection.check", json!({}));
        assert_eq!(conn(&d)["state"], "online", "the fresh round says online");
        slow.join().unwrap();
    });
    std::thread::sleep(Duration::from_secs(11)); // two more checks at the real cadence
    let s = conn(&d);
    assert_eq!((s["state"].as_str(), s["providers"]["anthropic"]["reachable"].as_bool()), (Some("online"), Some(true)), "the slow round's stale timeout was dropped: {s}");

    // The probes really went over the simulated link.
    assert!(internet.log().iter().any(|l| l.contains("HEAD /generate_204") && l.ends_with(": 204")), "{:?}", internet.log());
    assert!(internet.log().iter().any(|l| l.contains("accepted and never answered")), "{:?}", internet.log());
}
