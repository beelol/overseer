//! Continuity (Gate L) against the real daemon binary. The network, the machine's memory and
//! Ollama are FIXTURES here (a JSON file each for the first two, a loopback server for the third),
//! so every state can be produced on any machine; harness transcripts are synthetic. Test names
//! carry the acceptance criterion they support. No model is loaded and nothing is downloaded.

mod common;
#[path = "common/ollama.rs"]
mod ollama;
#[path = "common/world.rs"]
mod world;

use common::*;
use ollama::Ollama;
use serde_json::{json, Value};
use std::time::Duration;
use world::*;

fn connection_events(d: &Daemon) -> Vec<Value> {
    all_events(d, "connection")
}

fn codex_turn(d: &Daemon, w: &World, repo: &std::path::Path, transcript: &str) -> Value {
    w.replay(transcript);
    let created = d.call("task.create", json!({"repo": repo, "harness": "codex", "prompt": "x", "title": transcript}));
    let run = run_id(&created);
    d.wait_done(&run, 15);
    d.events(&run).into_iter().filter(|e| e["kind"] == "error").next_back().unwrap_or(Value::Null)
}

// ---------------------------------------------------------------- AC-83

#[test]
fn ac83_offline_is_told_from_an_outage() {
    let w = World::new();
    let d = w.start(&no_ollama(), &[]);
    // The daemon answers at once from the system's own answer; the probes follow.
    let (s, _) = wait_conn(&d, "probed", |s| s["providers"]["openai"]["reachable"] == true);
    assert_eq!((s["state"].as_str(), s["reason"].as_str()), (Some("online"), Some("connected")));
    assert_eq!(s["system"]["state"], "connected");
    assert_eq!(s["providers"]["openai"], json!({"reachable": true, "reason": "answered (fixture)", "source": "probe"}));

    // One provider's hosts stop answering while the internet works: degraded, and it is named.
    w.net(json!({"system": "connected", "baseline": {"by_name": true, "by_ip": true}, "providers": {"openai": "connect", "anthropic": true}}));
    let (s, _) = wait_conn(&d, "degraded", is("degraded", "OpenAI unreachable"));
    assert_eq!(s["unreachable"], json!(["openai"]));
    assert_eq!(s["acts_offline"], false);
    assert_eq!(s["providers"]["anthropic"]["reachable"], true, "Claude still works, so this is not offline");

    // Both providers: still degraded (the internet works), but there is nothing left to fail over to.
    w.net(json!({"system": "connected", "baseline": {"by_name": true, "by_ip": true}, "providers": {"openai": "connect", "anthropic": "timeout"}}));
    let (s, _) = wait_conn(&d, "degraded without a provider", is("degraded", "Claude and OpenAI unreachable"));
    assert_eq!(s["acts_offline"], true);

    // The system says connected, but names do not resolve: offline, and the reason says so.
    w.net(json!({"system": "connected", "baseline": {"by_name": "dns", "by_ip": true}, "providers": {"openai": "dns", "anthropic": "dns"}}));
    wait_conn(&d, "offline (DNS)", is("offline", "no working connection (DNS is not answering)"));
    // A captive portal answers with its own certificate.
    w.net(json!({"system": "connected", "baseline": {"by_name": "tls", "by_ip": "tls"}, "providers": {"openai": "tls", "anthropic": "tls"}}));
    wait_conn(&d, "offline (portal)", is("offline", "no working connection (captive portal)"));

    // Back online needs the system, the baseline and the providers to agree.
    w.net(json!({"system": "connected", "baseline": {"by_name": true, "by_ip": true}, "providers": {"openai": true, "anthropic": true}}));
    wait_conn(&d, "online", is("online", "connected"));

    // The system itself says there is no network: offline at once, without a probe.
    w.net(json!({"system": "none"}));
    let (s, took) = wait_conn(&d, "offline (system)", is("offline", "no network (system)"));
    assert!(took < Duration::from_millis(1000), "the system's answer is trusted at once ({took:?})");
    assert_eq!(s["baseline"], Value::Null, "no probe was needed");
    assert_eq!(s["system"]["state"], "no_network");
    w.net(json!({"system": "connected", "baseline": {"by_name": true, "by_ip": true}, "providers": {"openai": true, "anthropic": true}}));
    wait_conn(&d, "online again", is("online", "connected"));

    // Every change is an event with the reason, the system's answer and the providers.
    let events = connection_events(&d);
    let seen: Vec<String> = events.iter().map(|e| format!("{}: {}", e["payload"]["status"]["state"].as_str().unwrap(), e["payload"]["status"]["reason"].as_str().unwrap())).collect();
    assert_eq!(
        seen,
        [
            "online: connected",
            "degraded: OpenAI unreachable",
            "degraded: Claude and OpenAI unreachable",
            "offline: no working connection (DNS is not answering)",
            "offline: no working connection (captive portal)",
            "online: connected",
            "offline: no network (system)",
            "online: connected",
        ]
    );
    for e in &events {
        let s = &e["payload"]["status"];
        assert!(s["system"]["detail"].is_string() && s["providers"]["openai"].is_object() && s["since_ms"].is_i64(), "{e}");
    }
    assert_eq!(events[1]["payload"]["previous"]["state"], "online");
}

#[test]
fn ac83_rate_limits_and_usage_limits_are_never_offline() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let w = World::new();
    // Probes only run when something asks for them, so a probe round can be attributed.
    let d = w.start(&no_ollama(), &[("OVERSEER_TEST_PROBE_MS", "600000"), ("OVERSEER_TEST_PROBE_IDLE_MS", "600000")]);
    let e = codex_turn(&d, &w, &repo, "codex-rate-limit.jsonl");
    assert_eq!(e["payload"]["class"], "rate_limit");
    let e = codex_turn(&d, &w, &repo, "codex-usage-limit.jsonl");
    assert_eq!(e["payload"]["class"], "quota");
    std::thread::sleep(Duration::from_millis(400));
    assert_eq!(conn(&d)["state"], "online", "a 429 and a usage limit are account states");
    assert_eq!(connection_events(&d).len(), 1, "nothing but the first reading");

    // A connection error from an agent is its own class and starts a probe round at once. The
    // hosts answer, so one such error does not change the state.
    let probed = wait_conn(&d, "probed once", |s| s["probed_ms"].is_i64()).0["probed_ms"].as_i64().unwrap();
    let e = codex_turn(&d, &w, &repo, "codex-network-error.jsonl");
    assert_eq!(e["payload"]["class"], "network");
    let (s, _) = wait_conn(&d, "probed again", |s| s["probed_ms"].as_i64().unwrap() > probed);
    assert_eq!(s["state"], "online");

    // The provider answers, but with failures of its own, twice within two minutes: an outage of
    // that provider, which is degraded and not offline.
    codex_turn(&d, &w, &repo, "codex-outage.jsonl");
    codex_turn(&d, &w, &repo, "codex-outage.jsonl");
    let (s, _) = wait_conn(&d, "degraded by an outage", is("degraded", "OpenAI unreachable"));
    assert_eq!(s["providers"]["openai"], json!({"reachable": false, "reason": "outage", "source": "agents"}));
    assert_eq!(s["providers"]["anthropic"]["reachable"], true);
}

#[test]
fn ac83_with_probes_off_the_system_and_the_agents_decide() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let w = World::new();
    w.net(json!({"system": "connected", "baseline": {"by_name": "connect", "by_ip": "connect"}, "providers": {"openai": "connect", "anthropic": "connect"}}));
    let d = w.start(&no_ollama(), &[]);
    wait_conn(&d, "offline", |s| s["state"] == "offline");
    d.call("settings.set", json!({"values": {"probes": false}}));
    // With probes off the failing probes are not asked, so nothing says offline any more.
    let (s, _) = wait_conn(&d, "online without probes", is("online", "connected"));
    assert_eq!((s["probes"].clone(), s["baseline"].clone()), (json!(false), Value::Null));
    assert_eq!(s["providers"]["openai"]["reachable"], Value::Null, "unknown stays unknown");
    // Every provider in use fails on connection errors: the connection is gone.
    let e = codex_turn(&d, &w, &repo, "codex-network-error.jsonl");
    assert_eq!(e["payload"]["class"], "network");
    wait_conn(&d, "offline by the agents", is("offline", "all agents lost their connection"));
    // The system's own answer still works without probes.
    w.net(json!({"system": "none"}));
    wait_conn(&d, "offline by the system", is("offline", "no network (system)"));
}

// ---------------------------------------------------------------- AC-85

#[test]
fn ac85_inventory_reports_the_machine_and_ollama_without_guessing() {
    let o = Ollama::start();
    o.install(ollama::qwen3_coder_30b()).install(ollama::qwen3_coder_30b_64k()).install(ollama::qwen25_coder_14b()).install(ollama::qwen35_122b());
    o.set_loaded("qwen3-coder:30b-64k", 25_411_736_042, 65536);
    let w = World::new();
    w.memory(32.0, 20.5, "warn");
    let d = w.start(&o.url(), &[("OVERSEER_OLLAMA_CANDIDATES", "/bin/sh")]);
    let inv = d.call("local.inventory", json!({}));
    assert_eq!((gib(&inv["memory"]["total"]), gib(&inv["memory"]["available"]), inv["memory"]["pressure"].as_str()), (32.0, 20.5, Some("warn")));
    assert_eq!(inv["ollama"], json!({"installed": "/bin/sh", "version": "0.34.2", "running": true, "url": o.url(), "detail": "Ollama is running"}));
    let models = inv["models"].as_array().unwrap();
    assert_eq!(models.len(), 4);
    let m = models.iter().find(|m| m["tag"] == "qwen3-coder:30b-64k").unwrap();
    assert_eq!(m["base"], "qwen3-coder:30b");
    assert_eq!((m["size"].as_u64(), m["configured_context"].as_u64(), m["max_context"].as_u64(), m["parameters"].as_u64()), (Some(18_556_700_444), Some(65536), Some(262_144), Some(30_532_122_624)));
    assert_eq!((m["family"].as_str(), m["quantization"].as_str(), m["parameter_size"].as_str()), (Some("qwen3moe"), Some("Q4_K_M"), Some("30.5B")));
    assert_eq!(m["capabilities"], json!(["completion", "tools"]));
    assert_eq!(m["geometry"]["kv_heads_per_layer"].as_array().unwrap().len(), 48);
    let hybrid = models.iter().find(|m| m["tag"] == "qwen3.5:122b").unwrap();
    assert_eq!(hybrid["geometry"]["kv_heads_per_layer"].as_array().unwrap().iter().filter(|h| **h == json!(2)).count(), 12, "per-layer heads are kept");
    assert_eq!(inv["loaded"], json!([{"tag": "qwen3-coder:30b-64k", "size": 25_411_736_042u64, "size_vram": 25_411_736_042u64, "context": 65536, "expires_at": "2026-09-27T00:00:00Z"}]));
    assert!(inv["disk_free"].as_u64().is_some_and(|b| b > 0));
    // On this 32 GiB machine the loaded model is over the share (30% under a pressure warning), so
    // the guard refuses it even though something else loaded it.
    let e = d.try_call("local.approve", json!({"tag": "qwen3-coder:30b-64k", "context": 65536})).unwrap_err();
    assert!(e.contains("23.7 GiB at a 64k context is over the budget of 9.6 GiB (30% of 32 GiB is 9.6 GiB"), "{e}");
    // What Ollama measured replaces the estimate from now on, and the copy that is already
    // loaded is not counted twice.
    w.memory(128.0, 40.0, "normal");
    let pick = d.call("local.approve", json!({"tag": "qwen3-coder:30b-64k", "context": 65536}));
    assert_eq!((pick["measured"].clone(), pick["bytes"].as_u64(), pick["already_loaded"].clone()), (json!(true), Some(25_411_736_042), json!(true)));
    // Reading the inventory asks Ollama for facts and changes nothing.
    assert!(o.asked("/api/generate").is_empty() && o.asked("/api/create").is_empty());

    // Installed but stopped, and not installed at all: said so, with nothing invented.
    let stopped = World::new().start(&no_ollama(), &[("OVERSEER_OLLAMA_CANDIDATES", "/bin/sh")]);
    let inv = stopped.call("local.inventory", json!({}));
    assert_eq!((inv["ollama"]["installed"].as_str(), inv["ollama"]["running"].clone(), inv["ollama"]["version"].clone()), (Some("/bin/sh"), json!(false), Value::Null));
    assert!(inv["ollama"]["detail"].as_str().unwrap().starts_with("Ollama is installed but not running"));
    assert_eq!((inv["models"].clone(), inv["loaded"].clone()), (json!([]), json!([])));
    let absent = World::new().start(&no_ollama(), &[]);
    let inv = absent.call("local.inventory", json!({}));
    assert_eq!(inv["ollama"]["installed"], Value::Null);
    assert_eq!(inv["ollama"]["detail"], "Ollama is not installed");
    assert!(absent.try_call("local.approve", json!({"tag": "qwen3-coder:30b", "context": 16384})).unwrap_err().contains("Ollama is not installed; nothing can be loaded"));
    // A machine whose memory cannot be read gets no pick at all.
    let blind = World::new();
    std::fs::write(blind.file("memory.json"), "{}").unwrap();
    let d = blind.start(&o.url(), &[]);
    let inv = d.call("local.inventory", json!({}));
    assert_eq!(inv["memory"], Value::Null);
    assert!(inv["memory_error"].as_str().unwrap().contains("needs total"));
    assert!(d.try_call("local.pick", json!({})).unwrap_err().contains("memory cannot be read"));
    // And Ollama is never addressed anywhere but on this machine.
    let far = World::new().start("http://192.168.1.20:11434", &[]);
    assert!(far.call("local.inventory", json!({}))["ollama"]["url"].as_str().unwrap().contains("loopback"));
}

// ---------------------------------------------------------------- AC-86

#[test]
fn ac86_the_pick_follows_the_machines_memory() {
    let o = Ollama::start();
    o.install(ollama::qwen3_coder_30b()).install(ollama::qwen3_coder_30b_64k()).install(ollama::qwen25_coder_14b()).install(ollama::qwen35_122b());
    let w = World::new();
    let d = w.start(&o.url(), &[]);
    // Checks recorded on this machine are laid over the shipped catalogue.
    std::fs::write(d.home.path().join("local_catalogue.json"), json!({"models": [{"tag": "qwen2.5-coder:14b", "tier": 2, "disk_bytes": 8_988_124_069u64, "verified": {"opencode": {"status": "passed", "note": "test"}}}]}).to_string()).unwrap();

    // 128 GiB with nothing else running: the share decides.
    let p = d.call("local.pick", json!({}));
    let c = &p["pick"]["chosen"];
    assert_eq!((c["tag"].as_str(), c["context"].as_u64(), c["run_tag"].as_str(), c["run_tag_exists"].clone()), (Some("qwen3-coder:30b"), Some(65536), Some("qwen3-coder:30b-64k"), json!(true)));
    assert_eq!((gib(&c["bytes"]), c["measured"].clone(), gib(&p["pick"]["budget"]["budget"])), (24.3, json!(false), 51.2));
    assert_eq!(p["pick"]["alternatives"][0]["tag"], "qwen2.5-coder:14b");
    assert!(p["pick"]["rejected"].as_array().unwrap().iter().any(|r| r["tag"] == "qwen2.5-coder:7b" && r["reason"].as_str().unwrap().starts_with("not verified")));

    // 100 GiB in use by other things: what is free now decides, and the pick drops.
    w.memory(128.0, 28.0, "normal");
    let p = d.call("local.pick", json!({}));
    let c = &p["pick"]["chosen"];
    assert_eq!((c["tag"].as_str(), c["context"].as_u64(), gib(&c["bytes"]), gib(&c["budget"]["budget"])), (Some("qwen2.5-coder:14b"), Some(16384), 12.4, 15.2));
    assert_eq!(c["run_tag"], "overseer/qwen2.5-coder-14b-16k");
    let big = p["pick"]["rejected"].as_array().unwrap().iter().find(|r| r["tag"] == "qwen3-coder:30b").unwrap();
    assert_eq!(big["reason"], "too big: 19.8 GiB at a 16k context is over the budget of 15.2 GiB");

    // The same machine described as 32 GiB and 64 GiB.
    w.memory(32.0, 28.8, "normal");
    assert_eq!(d.call("local.pick", json!({}))["pick"]["chosen"]["tag"], "qwen2.5-coder:14b");
    w.memory(64.0, 57.6, "normal");
    let c = d.call("local.pick", json!({}))["pick"]["chosen"].clone();
    assert_eq!((c["tag"].as_str(), c["context"].as_u64(), gib(&c["budget"]["budget"])), (Some("qwen3-coder:30b"), Some(65536), 25.6));
    w.memory(16.0, 14.4, "normal");
    let p = d.call("local.pick", json!({}));
    assert_eq!(p["pick"]["chosen"], Value::Null, "nothing installed fits 6.4 GiB, and nothing is picked");
    // Memory pressure: a warning lowers the ceiling, a critical level picks nothing.
    w.memory(128.0, 115.2, "warn");
    assert_eq!(gib(&d.call("local.pick", json!({}))["pick"]["budget"]["ceiling_share"]), 38.4);
    w.memory(128.0, 115.2, "critical");
    let p = d.call("local.pick", json!({}));
    assert_eq!((p["pick"]["chosen"].clone(), p["pick"]["budget"]["budget"].as_u64()), (Value::Null, Some(0)));

    // The ceiling is the owner's to change up to 50%, and no further.
    w.memory(128.0, 115.2, "normal");
    assert!(d.try_call("settings.set", json!({"values": {"ramCeilingPercent": 60}})).unwrap_err().contains("60 was refused"));
    assert_eq!(gib(&d.call("local.pick", json!({}))["pick"]["budget"]["ceiling_share"]), 51.2, "a refused change changes nothing");
    d.call("settings.set", json!({"values": {"ramCeilingPercent": 50, "contextTarget": 131072}}));
    let c = d.call("local.pick", json!({}))["pick"]["chosen"].clone();
    assert_eq!((c["context"].as_u64(), gib(&c["bytes"]), gib(&c["budget"]["ceiling_share"]), c["run_tag"].as_str()), (Some(131072), 30.3, 64.0, Some("overseer/qwen3-coder-30b-128k")));
    // Even at the widest setting, with unverified models allowed, the 122B model is only ever rejected.
    d.call("settings.set", json!({"values": {"allowUnverifiedModels": true}}));
    let p = d.call("local.pick", json!({}));
    let huge = p["pick"]["rejected"].as_array().unwrap().iter().find(|r| r["tag"] == "qwen3.5:122b").unwrap();
    assert!(huge["reason"].as_str().unwrap().starts_with("too big: 77.2 GiB at a 16k context is over the budget of 64 GiB"), "{huge}");
    for c in p["pick"]["alternatives"].as_array().unwrap().iter().chain([&p["pick"]["chosen"]]) {
        assert!(c["bytes"].as_u64().unwrap() <= c["budget"]["budget"].as_u64().unwrap(), "{c}");
    }

    // A measured size replaces the estimate as soon as Ollama reports one.
    d.call("settings.set", json!({"values": {"contextTarget": 65536, "ramCeilingPercent": 40, "allowUnverifiedModels": false}}));
    o.set_loaded("qwen3-coder:30b-64k", 25_411_736_042, 65536);
    let c = d.call("local.pick", json!({}))["pick"]["chosen"].clone();
    assert_eq!((c["measured"].clone(), c["bytes"].as_u64(), gib(&c["bytes"])), (json!(true), Some(25_411_736_042), 23.7));
    let estimate = 26_072_893_212f64;
    assert!(((estimate - 25_411_736_042f64) / 25_411_736_042f64).abs() < 0.15, "the estimate was within 15% of the measurement");
}

// ---------------------------------------------------------------- AC-88

#[test]
fn ac88_settings_are_kept_and_enforced_by_the_daemon() {
    let w = World::new();
    let mut d = w.start(&no_ollama(), &[]);
    let first = d.call("settings.get", json!({}));
    assert_eq!(first["settings"], first["defaults"]);
    assert_eq!(first["settings"]["enabled"], true);
    assert_eq!(first["settings"].as_object().unwrap().len(), 19);
    // Every setting is changed, read back, and still there after the daemon restarts.
    let values = json!({
        "enabled": false, "providerOrder": ["anthropic", "openai"], "allowModelDownloads": true, "allowOllamaInstall": true, "prefetch": true,
        "ramCeilingPercent": 35, "ramHeadroomGiB": 6, "contextTarget": 32768, "contextFloor": 8192, "preferredModels": ["qwen3-coder:30b"],
        "allowUnverifiedModels": true, "localHarness": "codex", "returnOnline": "auto", "retryCapSeconds": 60, "retryForHours": 12, "stallSeconds": 45,
        "probes": false, "ollamaIdleMinutes": 5, "registry": "https://mirror.example.invalid"
    });
    assert_eq!(values.as_object().unwrap().len(), 19, "every setting");
    for (k, v) in values.as_object().unwrap() {
        let mut one = serde_json::Map::new();
        one.insert(k.clone(), v.clone());
        let got = d.call("settings.set", json!({"values": one}));
        assert_eq!(got["settings"][k].as_f64().map(|f| json!(f)).unwrap_or(got["settings"][k].clone()), v.as_f64().map(|f| json!(f)).unwrap_or(v.clone()), "{k}");
    }
    let before = d.call("settings.get", json!({}))["settings"].clone();
    d.shutdown();
    d.spawn();
    assert_eq!(d.call("settings.get", json!({}))["settings"], before, "kept across a restart");
    assert_eq!(conn(&d)["probes"], false, "and enforced: no probes run");
    // Out of range, of the wrong type, or unknown: refused with the reason, and nothing changes.
    for (values, why) in [
        (json!({"ramCeilingPercent": 51}), "between 10 and 50"),
        (json!({"retryForHours": 37}), "the owner's limit is 36 hours"),
        (json!({"contextTarget": 4096}), "it cannot be under the floor"),
        (json!({"returnOnline": "never"}), "offer, auto or stay"),
        (json!({"probes": "no"}), "wrong type"),
        (json!({"fasterPlease": true}), "is not a Continuity setting"),
        (json!({"ramCeilingPercent": 45, "stallSeconds": 1}), "between 30 and 3600"),
    ] {
        let e = d.try_call("settings.set", json!({"values": values})).unwrap_err();
        assert!(e.contains(why), "{e}");
    }
    assert_eq!(d.call("settings.get", json!({}))["settings"], before);
    // `overseerd ctl continuity.status` prints the state, the budget and the pick.
    let out = std::process::Command::new(BIN).args(["ctl", "continuity.status"]).env("OVERSEER_HOME", d.home.path()).output().unwrap();
    let printed: Value = serde_json::from_slice(&out.stdout).unwrap();
    let r = &printed["result"];
    assert_eq!((r["connection"]["state"].as_str(), r["settings"]["retryForHours"].as_u64(), gib(&r["budget"]["budget"]), gib(&r["budget"]["headroom"])), (Some("online"), Some(12), 44.8, 6.0));
    // With downloads and the Ollama install both allowed, the pick names what it would fetch.
    assert_eq!((r["pick"]["tag"].as_str(), r["pick"]["installed"].clone(), r["pick"]["download_bytes"].as_u64(), r["ollama"]["detail"].as_str()), (Some("qwen3-coder:30b"), json!(false), Some(18_556_700_761), Some("Ollama is not installed")));
    d.call("settings.set", json!({"values": {"allowOllamaInstall": false}}));
    let p = d.call("local.pick", json!({}));
    assert_eq!((p["pick"]["chosen"].clone(), p["may_download"].clone()), (Value::Null, json!(false)), "without Ollama and without leave to install it, nothing is picked");
    // Each accepted change is an event.
    let changes = d.call("events.list", json!({"limit": 5000}))["events"].as_array().unwrap().iter().filter(|e| e["kind"] == "continuity_settings").count();
    assert_eq!(changes, 20, "nineteen settings, and the install switched off again");
}

// ---------------------------------------------------------------- AC-98

#[test]
fn ac98_the_notice_is_shown_once_per_machine() {
    let w = World::new();
    let mut d = w.start(&no_ollama(), &[]);
    let n = d.call("continuity.notice", json!({}));
    assert_eq!(n, json!({"show": true, "shown_ms": null, "enabled": true, "allow_model_downloads": false, "allow_ollama_install": false}));
    assert_eq!(d.call("continuity.notice", json!({}))["show"], true, "asking does not dismiss it");
    // Allow downloads from the notice, then dismiss it.
    d.call("settings.set", json!({"values": {"allowModelDownloads": true}}));
    let n = d.call("continuity.notice", json!({"dismiss": true}));
    assert_eq!((n["show"].clone(), n["allow_model_downloads"].clone()), (json!(false), json!(true)));
    assert!(n["shown_ms"].as_i64().unwrap() > 0);
    assert_eq!(d.call("continuity.notice", json!({}))["show"], false, "another window asks: not shown again");
    d.shutdown();
    d.spawn();
    assert_eq!(d.call("continuity.notice", json!({}))["show"], false, "nor after a restart");
    // With Continuity off there is nothing to explain.
    let off = World::new().start(&no_ollama(), &[]);
    off.call("settings.set", json!({"values": {"enabled": false}}));
    assert_eq!(off.call("continuity.notice", json!({}))["show"], false);
    off.call("settings.set", json!({"values": {"enabled": true}}));
    assert_eq!(off.call("continuity.notice", json!({}))["show"], true);
}

// ---------------------------------------------------------------- AC-140

#[test]
fn ac140_no_model_over_the_budget_is_loaded_by_any_path() {
    let o = Ollama::start();
    o.install(ollama::qwen3_coder_30b()).install(ollama::qwen3_coder_30b_64k()).install(ollama::qwen25_coder_14b()).install(ollama::qwen35_122b());
    o.state.lock().unwrap().loaded_size.insert("qwen3-coder:30b-64k".into(), 25_411_736_042);
    let w = World::new();
    let d = w.start(&o.url(), &[]);

    // The 122B model: refused by the guard and by a load, at the smallest context, at the widest
    // ceiling, with unverified models allowed. Nothing reaches Ollama.
    d.call("settings.set", json!({"values": {"ramCeilingPercent": 50, "allowUnverifiedModels": true}}));
    for method in ["local.approve", "local.load"] {
        let e = d.try_call(method, json!({"tag": "qwen3.5:122b", "context": 16384})).unwrap_err();
        assert!(e.starts_with("qwen3.5:122b is too big to load: 77.2 GiB at a 16k context is over the budget of 64 GiB (50% of 128 GiB is 64 GiB; 115.2 GiB available minus 12.8 GiB headroom is 102.4 GiB)"), "{e}");
    }
    assert!(d.try_call("local.load", json!({"tag": "mystery:latest", "context": 16384})).unwrap_err().contains("its size is unknown"));
    assert!(o.asked("/api/generate").is_empty() && o.asked("/api/create").is_empty(), "nothing was loaded or created");
    d.call("settings.set", json!({"values": {"ramCeilingPercent": 40, "allowUnverifiedModels": false}}));

    // A model inside the budget loads, watched, and what Ollama measures is recorded.
    o.state.lock().unwrap().load_ms = 200;
    let l = d.call("local.load", json!({"tag": "qwen3-coder:30b", "context": 65536}));
    assert_eq!(l["detail"]["run_tag"], "qwen3-coder:30b-64k", "the installed tag that already sets the context is used");
    assert!(l["loaded"]["samples"].as_u64().unwrap() >= 2, "memory was sampled while it loaded: {}", l["loaded"]);
    assert_eq!(l["detail"]["measured"]["size"].as_u64(), Some(25_411_736_042));
    assert_eq!(o.asked("/api/generate"), vec![json!({"model": "qwen3-coder:30b-64k", "prompt": "", "keep_alive": "30m", "options": {"num_ctx": 65536}})]);
    assert_eq!(d.call("local.approve", json!({"tag": "qwen3-coder:30b", "context": 65536}))["measured"], true);
    d.call("local.unload", json!({"tag": "qwen3-coder:30b-64k"}));
    assert_eq!(d.call("local.inventory", json!({}))["loaded"], json!([]));

    // The budget is decided on fresh numbers: the same model is refused when memory is short now.
    w.memory(128.0, 30.0, "normal");
    let e = d.try_call("local.load", json!({"tag": "qwen3-coder:30b", "context": 65536})).unwrap_err();
    assert!(e.contains("23.7 GiB at a 64k context is over the budget of 17.2 GiB"), "{e}");
    w.memory(128.0, 115.2, "critical");
    assert!(d.try_call("local.load", json!({"tag": "qwen2.5-coder:14b", "context": 16384})).unwrap_err().contains("critical memory pressure; nothing may be loaded"));
    assert_eq!(o.asked("/api/generate").len(), 2, "one load and one unload; the refused loads asked for nothing");

    // Memory runs short while a model loads: the load is cancelled and the model unloaded.
    w.memory(128.0, 115.2, "normal");
    o.state.lock().unwrap().load_ms = 1500;
    std::thread::scope(|s| {
        let loading = s.spawn(|| d.try_call("local.load", json!({"tag": "qwen3-coder:30b", "context": 65536})));
        std::thread::sleep(Duration::from_millis(400));
        w.memory(128.0, 5.0, "normal");
        let e = loading.join().unwrap().unwrap_err();
        assert_eq!(e, "the load of qwen3-coder:30b-64k was cancelled and the model unloaded: available memory fell to 5 GiB, under half the headroom of 12.8 GiB");
    });
    assert_eq!(o.asked("/api/generate").last().unwrap(), &json!({"model": "qwen3-coder:30b-64k", "keep_alive": 0}));
    // The system's own critical signal stops a load the same way.
    w.memory(128.0, 115.2, "normal");
    std::thread::scope(|s| {
        let loading = s.spawn(|| d.try_call("local.load", json!({"tag": "qwen2.5-coder:14b", "context": 16384})));
        std::thread::sleep(Duration::from_millis(400));
        w.memory(128.0, 115.2, "critical");
        assert!(loading.join().unwrap().unwrap_err().contains("cancelled and the model unloaded: the system reports critical memory pressure"));
    });
    std::thread::sleep(Duration::from_millis(1700)); // the fixture finishes its pretend loads
    // Every load, allowed or stopped, is in the event log with memory before and after.
    let loads: Vec<Value> = d.call("events.list", json!({"limit": 5000}))["events"].as_array().unwrap().iter().filter(|e| e["kind"] == "local_load").cloned().collect();
    assert_eq!(loads.len(), 4, "one load, one unload, two stopped loads");
    assert!(loads[0]["payload"]["memory_before"]["available"].is_u64() && loads[0]["payload"]["memory_after"]["available"].is_u64());
    assert!(loads[2]["payload"]["error"].as_str().unwrap().contains("cancelled"));
}
