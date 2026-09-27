//! Model downloads and prefetch (Continuity, AC-89) against the real daemon. Ollama and its
//! registry are SYNTHETIC (a loopback server that pretends to download, in steps); nothing is
//! fetched from the internet.

mod common;
#[path = "common/ollama.rs"]
mod ollama;
#[path = "common/world.rs"]
mod world;

use common::*;
use ollama::{Ollama, GIB};
use serde_json::{json, Value};
use std::time::{Duration, Instant};
use world::*;

fn qwen25_coder_7b() -> (Value, Value) {
    ollama::model("qwen2.5-coder:7b", "", 4_683_087_332, "qwen2", 28, json!(4), 128, 7_615_616_512, 32_768, None, &["completion", "tools", "insert"])
}

fn world(o: &Ollama) -> (World, Daemon) {
    {
        let mut s = o.state.lock().unwrap();
        s.pull_steps = 20;
        s.pull_step_ms = 40;
    }
    let w = World::new();
    let d = w.start(&o.url(), &[("OVERSEER_TEST_DOWNLOAD_EVENT_MS", "50"), ("OVERSEER_TEST_DISK_FREE", &(500 * GIB).to_string())]);
    (w, d)
}

fn download(d: &Daemon, tag: &str) -> Value {
    d.call("local.downloads", json!({}))["downloads"].as_array().unwrap().iter().find(|x| x["tag"] == tag).cloned().unwrap_or(Value::Null)
}

fn wait_download(d: &Daemon, tag: &str, status: &str) -> Value {
    let until = Instant::now() + Duration::from_secs(15);
    loop {
        let x = download(d, tag);
        if x["status"] == status {
            return x;
        }
        assert!(Instant::now() < until, "the download of {tag} never became {status}: {x}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn installed(d: &Daemon) -> Vec<String> {
    d.call("local.inventory", json!({}))["models"].as_array().unwrap().iter().map(|m| m["tag"].as_str().unwrap().to_string()).collect()
}

#[test]
fn ac89_models_are_downloaded_only_when_allowed() {
    let o = Ollama::start();
    o.install(ollama::qwen3_coder_30b()).offer(qwen25_coder_7b()).offer(ollama::qwen25_coder_14b());
    let (w, d) = world(&o);

    // Downloads are off by default: a model that is needed is reported as not installed, and
    // nothing is asked of the registry.
    let e = d.try_call("local.pull", json!({"tag": "qwen2.5-coder:7b", "confirm": true})).unwrap_err();
    assert_eq!(e, "qwen2.5-coder:7b is not installed, and model downloads are off (overseer.continuity.allowModelDownloads)");
    let p = d.call("local.pick", json!({}));
    assert_eq!(p["may_download"], false);
    assert!(p["pick"]["rejected"].as_array().unwrap().iter().any(|r| r["tag"] == "qwen2.5-coder:32b" && r["reason"].as_str().unwrap().contains("not verified") || r["reason"].as_str().unwrap().contains("not installed")));
    assert!(o.asked("/api/pull").is_empty());

    // Allowed: the first pull ever is confirmed once, with its size.
    d.call("settings.set", json!({"values": {"allowModelDownloads": true}}));
    let first = d.call("local.pull", json!({"tag": "qwen2.5-coder:7b"}));
    assert_eq!((first["needs_confirmation"].clone(), first["bytes"].as_u64()), (json!(true), Some(4_683_087_332)));
    assert!(o.asked("/api/pull").is_empty(), "nothing is pulled before the confirmation");
    let started = d.call("local.pull", json!({"tag": "qwen2.5-coder:7b", "confirm": true}));
    assert_eq!((started["download"]["status"].as_str(), started["download"]["by"].as_str()), (Some("starting"), Some("user")));
    // A second model has to wait its turn.
    assert_eq!(d.try_call("local.pull", json!({"tag": "qwen2.5-coder:14b"})).unwrap_err(), "qwen2.5-coder:7b is being downloaded; one model at a time");
    let done = wait_download(&d, "qwen2.5-coder:7b", "done");
    assert_eq!((done["percent"].as_u64(), done["completed"].as_u64()), (Some(100), Some(4_683_087_332)));
    assert!(installed(&d).contains(&"qwen2.5-coder:7b".to_string()));
    assert_eq!(o.asked("/api/pull"), vec![json!({"model": "qwen2.5-coder:7b", "stream": true})]);
    // Progress was streamed as events.
    let events = all_events(&d, "local_download");
    let seen: Vec<(String, u64)> = events.iter().map(|e| (e["payload"]["download"]["status"].as_str().unwrap().to_string(), e["payload"]["download"]["percent"].as_u64().unwrap_or(0))).collect();
    assert_eq!(seen.first().unwrap().0, "starting");
    assert_eq!(seen.last().unwrap(), &("done".to_string(), 100));
    let progress: Vec<u64> = seen.iter().filter(|(s, _)| s == "downloading").map(|(_, p)| *p).collect();
    assert!(progress.len() >= 3 && progress.windows(2).all(|w| w[0] < w[1]), "progress rises: {progress:?}");
    assert_eq!(d.try_call("local.pull", json!({"tag": "qwen2.5-coder:7b"})).unwrap_err(), "qwen2.5-coder:7b is already installed");

    // Later pulls rely on the setting. One is cancelled midway and continued.
    o.state.lock().unwrap().pull_step_ms = 80;
    let again = d.call("local.pull", json!({"tag": "qwen2.5-coder:14b"}));
    assert_eq!(again["needs_confirmation"], Value::Null, "confirmed once, not again: {again}");
    let until = Instant::now() + Duration::from_secs(10);
    while download(&d, "qwen2.5-coder:14b")["percent"].as_u64().unwrap_or(0) < 20 {
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(20));
    }
    d.call("local.pull_cancel", json!({"tag": "qwen2.5-coder:14b"}));
    let cancelled = wait_download(&d, "qwen2.5-coder:14b", "cancelled");
    assert!(cancelled["reason"].as_str().unwrap().contains("what was downloaded is kept"));
    assert!(!installed(&d).contains(&"qwen2.5-coder:14b".to_string()));
    std::thread::sleep(Duration::from_millis(200));
    let reached = o.state.lock().unwrap().pulled["qwen2.5-coder:14b"];
    assert!(reached > 0 && reached < 8_988_124_069, "the pull stopped partway: {reached}");
    assert_eq!(d.try_call("local.pull_cancel", json!({"tag": "qwen2.5-coder:14b"})).unwrap_err(), "qwen2.5-coder:14b is not being downloaded");
    let before = all_events(&d, "local_download").len();
    d.call("local.pull", json!({"tag": "qwen2.5-coder:14b"}));
    wait_download(&d, "qwen2.5-coder:14b", "done");
    let resumed: Vec<u64> = all_events(&d, "local_download")[before..].iter().filter(|e| e["payload"]["download"]["status"] == "downloading").map(|e| e["payload"]["download"]["completed"].as_u64().unwrap()).collect();
    // The fixture continues from the step it had reached, so at most a step or two (a tenth) is repeated.
    assert!(resumed[0] + 8_988_124_069 / 10 >= reached && resumed[0] > 0, "the second pull continues from about {reached}, not from nothing: {resumed:?}");
    assert!(installed(&d).contains(&"qwen2.5-coder:14b".to_string()));

    // A model the registry does not have fails with Ollama's own words.
    d.call("local.pull", json!({"tag": "no-such-model:1b"}));
    let failed = wait_download(&d, "no-such-model:1b", "failed");
    assert_eq!(failed["reason"], "Ollama could not download no-such-model:1b: pull model manifest: file does not exist");

    // Never offline.
    w.net(json!({"system": "none"}));
    wait_conn(&d, "offline", |s| s["state"] == "offline");
    assert_eq!(d.try_call("local.pull", json!({"tag": "qwen2.5-coder:3b"})).unwrap_err(), "downloads need a connection; Overseer is offline (no network (system))");
}

#[test]
fn ac89_a_full_disk_refuses_the_download() {
    let o = Ollama::start();
    o.offer(qwen25_coder_7b());
    let w = World::new();
    let d = w.start(&o.url(), &[("OVERSEER_TEST_DISK_FREE", &(3 * GIB).to_string())]);
    d.call("settings.set", json!({"values": {"allowModelDownloads": true}}));
    let e = d.try_call("local.pull", json!({"tag": "qwen2.5-coder:7b", "confirm": true})).unwrap_err();
    assert_eq!(e, "qwen2.5-coder:7b needs 4.8 GiB with room to spare and the disk has 3 GiB free: 1.8 GiB are missing");
    assert!(o.asked("/api/pull").is_empty());
    assert_eq!(d.call("local.downloads", json!({}))["first_pull_confirmed"], false, "a refused pull confirms nothing");
}

#[test]
fn ac89_prefetch_fetches_the_pick_and_nothing_else() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let o = Ollama::start();
    // Nothing is installed; the registry offers three coders. On 128 GiB the pick is the 30B.
    o.offer(ollama::qwen3_coder_30b()).offer(qwen25_coder_7b()).offer(ollama::qwen25_coder_14b());
    let (w, d) = world(&o);
    let pick = |d: &Daemon| d.call("local.pick", json!({}))["pick"]["chosen"].clone();

    // Prefetch is off until asked: allowing downloads alone fetches nothing ahead of need.
    d.call("settings.set", json!({"values": {"allowModelDownloads": true}}));
    assert_eq!((pick(&d)["tag"].as_str(), pick(&d)["installed"].clone(), pick(&d)["download_bytes"].as_u64()), (Some("qwen3-coder:30b"), json!(false), Some(18_556_700_761)));
    std::thread::sleep(Duration::from_millis(500));
    assert!(o.asked("/api/pull").is_empty(), "prefetch is off");

    // Turned on, but the first pull was never confirmed: still nothing.
    d.call("settings.set", json!({"values": {"prefetch": true}}));
    std::thread::sleep(Duration::from_millis(500));
    assert!(o.asked("/api/pull").is_empty(), "the first pull has not been confirmed");

    // The offer is accepted (the first pull is confirmed with the model and its size): while a
    // paid turn runs nothing is fetched; when it ends the pick is, and only the pick.
    w.replay("codex-rate-limit.jsonl");
    std::fs::write(w.file("replay.jsonl"), "{\"type\":\"thread.started\",\"thread_id\":\"t\"}\n{\"type\":\"turn.started\"}\n{\"type\":\"item.completed\",\"item\":{\"id\":\"i\",\"type\":\"agent_message\",\"text\":\"working\"}}\n{\"type\":\"turn.completed\",\"usage\":{}}\n").unwrap();
    let slow = w.start(&o.url(), &[("REPLAY_DELAY_MS", "700"), ("OVERSEER_TEST_DOWNLOAD_EVENT_MS", "50"), ("OVERSEER_TEST_DISK_FREE", &(500 * GIB).to_string())]);
    slow.call("settings.set", json!({"values": {"allowModelDownloads": true, "prefetch": true}}));
    let paid = slow.call("task.create", json!({"repo": repo, "harness": "codex", "prompt": "x", "title": "paid"}));
    slow.wait_status(&run_id(&paid), |s| s == "running", 10);
    // Confirming the offer is a pull asked for by the user; it is refused nothing, but here the
    // offer is accepted through the confirmation alone.
    let confirm = slow.call("local.pull", json!({"tag": "qwen2.5-coder:7b", "confirm": true}));
    assert_eq!(confirm["download"]["by"], "user");
    wait_download(&slow, "qwen2.5-coder:7b", "done");
    let during: Vec<Value> = o.asked("/api/pull");
    assert_eq!(during, vec![json!({"model": "qwen2.5-coder:7b", "stream": true})], "while the paid turn runs, prefetch fetches nothing");
    slow.wait_done(&run_id(&paid), 20);
    let done = wait_download(&slow, "qwen3-coder:30b", "done");
    assert_eq!(done["by"], "prefetch");
    assert_eq!(o.asked("/api/pull"), vec![json!({"model": "qwen2.5-coder:7b", "stream": true}), json!({"model": "qwen3-coder:30b", "stream": true})], "exactly the model the pick names");
    assert_eq!((pick(&slow)["tag"].as_str(), pick(&slow)["installed"].clone()), (Some("qwen3-coder:30b"), json!(true)));
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(o.asked("/api/pull").len(), 2, "and nothing else afterwards");

    // With downloads switched off again prefetch stops, whatever its own setting says.
    drop(slow);
    let o2 = Ollama::start();
    o2.offer(ollama::qwen3_coder_30b());
    let (_w2, d2) = world(&o2);
    d2.call("settings.set", json!({"values": {"allowModelDownloads": true, "prefetch": true}}));
    d2.call("settings.set", json!({"values": {"allowModelDownloads": false}}));
    std::thread::sleep(Duration::from_millis(500));
    assert!(o2.asked("/api/pull").is_empty());
    let _ = d;
}
