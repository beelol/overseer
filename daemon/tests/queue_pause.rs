//! AC-265: real daemon, SYNTHETIC Claude fixture, simulated spoken requests, no paid turns.
mod common;
use common::*;
use serde_json::{json, Value};
use std::time::{Duration, Instant};

fn listener() -> String { repo_root().join("fixtures/fake-harness/queue-listener.js").display().to_string() }
fn fixture() -> String { repo_root().join("fixtures/fake-harness/claude-fixture.js").display().to_string() }
fn until(what: &str, mut check: impl FnMut() -> bool) {
    let end = Instant::now() + Duration::from_secs(30);
    while !check() {
        assert!(Instant::now() < end, "never {what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}
fn queued(d: &Daemon, id: &str) -> Value { d.call("run.queued", json!({"run_id":id})) }
fn turns(d: &Daemon, id: &str) -> Vec<Value> { d.call("run.turns", json!({"run_id":id})).as_array().unwrap().clone() }
fn start(d: &Daemon, repo: &std::path::Path, title: &str) -> String {
    let run = run_id(&d.call("task.create", json!({"repo":repo,"harness":"claude","title":title,"prompt":"original work"})));
    d.wait_status(&run, |s| s == "running", 20);
    run
}

#[test]
fn ac265_stop_keeps_typed_and_spoken_messages_paused_then_resumes_in_order() {
    let r = tmp(); let repo = repo(&r.path().join("repo")); let mode = r.path().join("mode");
    std::fs::write(&mode, "slow").unwrap();
    let mut d = Daemon::start(&[("OVERSEER_VOICE_SIMULATE", "1"), ("OVERSEER_LISTENER", &listener()), ("OVERSEER_CLAUDE_PATH", &fixture()), ("CLAUDE_FIXTURE_MODE_FILE", mode.to_str().unwrap()),
        ("FIXTURE_SLOW_MS", "18000"), ("FIXTURE_INTERRUPT_DELAY_MS", "1500"), ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "CLAUDE_FIXTURE_MODE_FILE,FIXTURE_SLOW_MS,FIXTURE_INTERRUPT_DELAY_MS")]);
    let run = start(&d, &repo, "Queue demo");
    d.call("overseer.level", json!({"level":"auto"}));
    d.call("voice.set", json!({"target":run,"enabled":true,"delivery":"add","settle_seconds":0}));
    assert_eq!(d.call("run.redirect", json!({"run_id":run,"text":"typed direction"}))["delivery"], "stopping, then the direction");
    assert_eq!(d.call("voice.say", json!({"text":"also add the spoken direction"}))["direct"], true);
    until("both queued", || queued(&d, &run)["queued"].as_array().unwrap().len() == 2);
    let before = queued(&d, &run)["queued"].clone();
    assert_eq!(before[0]["redirect"], true, "a real typed redirect is still queued");
    assert_eq!(d.run(&run)["status"], "running", "both messages queued mid-turn before Stop");
    d.call("run.interrupt", json!({"run_id":run}));
    assert_eq!(d.wait_done(&run, 20)["status"], "interrupted");
    std::thread::sleep(Duration::from_secs(10));
    assert_eq!(turns(&d, &run).len(), 1, "Stop cannot start the queue after the turn ends");
    assert_eq!(queued(&d, &run)["paused"], true);
    assert_eq!(queued(&d, &run)["queued"], before, "same messages in the same order");
    assert_eq!(d.run(&run)["queue"]["paused"], true, "one state for chat/grid/TUI/phone");
    d.kill9(); d.spawn();
    assert_eq!(queued(&d, &run)["paused"], true, "restart never resumes");
    assert_eq!(turns(&d, &run).len(), 1);
    // Direct redirects, messages from Overseer, and releasing a hold cannot unpause the queue.
    assert_eq!(d.call("run.redirect", json!({"run_id":run,"text":"later redirect"}))["delivery"], "paused");
    d.call("agent.hold", json!({"run_id":run,"reason":"review","by":"owner"}));
    d.call("agent.release", json!({"run_id":run,"by":"overseer"}));
    assert_eq!(turns(&d, &run).len(), 1);
    std::fs::write(&mode, "echo").unwrap();
    let resumed = d.call("voice.say", json!({"text":"send queued for Queue demo"}));
    assert_eq!(resumed["built_in"], "resume_queue", "{resumed}");
    until("three queued turns delivered", || turns(&d, &run).len() == 4 && d.run(&run)["status"] == "completed");
    let delivered = turns(&d, &run);
    assert_eq!(delivered[1]["prompt"], "typed direction");
    assert!(delivered[2]["prompt"].as_str().unwrap().contains("also add the spoken direction"));
    assert_eq!(delivered[3]["prompt"], "later redirect");
    for i in 2..4 { assert!(delivered[i]["started_ms"].as_i64().unwrap() >= delivered[i-1]["ended_ms"].as_i64().unwrap(), "one FIFO message per turn: {delivered:?}"); }
    assert!(queued(&d, &run)["queued"].as_array().unwrap().is_empty());
}

#[test]
fn ac265_clear_and_remove_do_not_send_messages() {
    let r = tmp(); let repo = repo(&r.path().join("repo"));
    let d = Daemon::start(&[("OVERSEER_VOICE_SIMULATE", "1"), ("OVERSEER_LISTENER", &listener()), ("OVERSEER_CLAUDE_PATH", &fixture()), ("CLAUDE_FIXTURE_MODE", "slow"), ("FIXTURE_SLOW_MS", "18000"),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "CLAUDE_FIXTURE_MODE,FIXTURE_SLOW_MS")]);
    d.call("voice.set", json!({"enabled":true,"settle_seconds":0}));
    let run = start(&d, &repo, "Clear demo");
    d.call("run.queue", json!({"run_id":run,"text":"remove me"}));
    d.call("run.queue", json!({"run_id":run,"text":"clear me"}));
    d.call("run.interrupt", json!({"run_id":run})); d.wait_done(&run, 20);
    let id = queued(&d, &run)["queued"][0]["id"].as_i64().unwrap();
    assert_eq!(d.call("run.unqueue", json!({"run_id":run,"id":id}))["removed"], 1);
    assert_eq!(queued(&d, &run)["queued"][0]["text"], "clear me");
    assert_eq!(d.call("voice.say", json!({"text":"clear queued for Clear demo"}))["built_in"], "clear_queue");
    assert_eq!(queued(&d, &run)["paused"], true, "Clear is not Resume");
    d.call("run.resume_queue", json!({"run_id":run}));
    std::thread::sleep(Duration::from_secs(10));
    assert_eq!(turns(&d, &run).len(), 1, "a cleared queue sends nothing");
}

#[test]
fn ac265_voice_stop_and_stop_everyone_pause_the_same_queue() {
    let r = tmp(); let repo = repo(&r.path().join("repo"));
    let d = Daemon::start(&[("OVERSEER_VOICE_SIMULATE", "1"), ("OVERSEER_LISTENER", &listener()), ("OVERSEER_CLAUDE_PATH", &fixture()), ("CLAUDE_FIXTURE_MODE", "slow"), ("FIXTURE_SLOW_MS", "18000"),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "CLAUDE_FIXTURE_MODE,FIXTURE_SLOW_MS")]);
    d.call("overseer.level", json!({"level":"auto"}));
    d.call("voice.set", json!({"enabled":true,"settle_seconds":0}));
    for title in ["Voice demo", "Everyone demo"] {
        let run = start(&d, &repo, title);
        d.call("run.queue", json!({"run_id":run,"text":"wait for owner"}));
        let phrase = if title == "Voice demo" { "stop Voice demo" } else { "stop everyone" };
        assert_eq!(d.call("voice.say", json!({"text":phrase}))["built_in"], "stop");
        d.wait_done(&run, 20);
        assert_eq!(queued(&d, &run)["paused"], true);
        assert_eq!(turns(&d, &run).len(), 1);
    }
}

#[test]
fn ac265_overseer_has_no_resume_action() {
    let d = Daemon::start(&[]);
    assert!(d.try_call("overseer.propose", json!({"actions":[{"action":"resume_queue","agent":"anything"}]})).unwrap_err().contains("not an action"));
}

#[test]
fn ac265_stop_pauses_before_turn_end_can_send_a_queued_message() {
    let r = tmp(); let repo = repo(&r.path().join("repo"));
    let d = Daemon::start(&[("OVERSEER_CLAUDE_PATH", &fixture()), ("CLAUDE_FIXTURE_MODE", "slow"), ("FIXTURE_SLOW_MS", "18000"),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "CLAUDE_FIXTURE_MODE,FIXTURE_SLOW_MS")]);
    let run = start(&d, &repo, "Stop demo");
    d.call("run.queue", json!({"run_id":run,"text":"must wait"}));
    d.call("run.interrupt", json!({"run_id":run}));
    assert_eq!(queued(&d, &run)["paused"], true);
}

#[test]
fn ac265_normal_additions_still_batch_and_concurrent_resume_is_once() {
    let r = tmp(); let repo = repo(&r.path().join("repo")); let mode = r.path().join("mode");
    std::fs::write(&mode, "slow").unwrap();
    let d = Daemon::start(&[("OVERSEER_CLAUDE_PATH", &fixture()), ("CLAUDE_FIXTURE_MODE_FILE", mode.to_str().unwrap()), ("FIXTURE_SLOW_MS", "1500"),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "CLAUDE_FIXTURE_MODE_FILE,FIXTURE_SLOW_MS")]);
    let run = start(&d, &repo, "Normal batch");
    d.call("run.queue", json!({"run_id":run,"text":"first addition"}));
    d.call("run.queue", json!({"run_id":run,"text":"second addition"}));
    until("normal batch", || turns(&d, &run).len() == 2);
    assert_eq!(turns(&d, &run)[1]["prompt"], "first addition\n\nsecond addition");
    d.wait_done(&run, 20);
    d.call("run.follow_up", json!({"run_id":run,"prompt":"stop this turn"}));
    d.call("run.queue", json!({"run_id":run,"text":"send once"}));
    d.call("run.interrupt", json!({"run_id":run}));
    d.wait_done(&run, 20);
    std::fs::write(&mode, "echo").unwrap();
    std::thread::scope(|scope| {
        for _ in 0..2 { scope.spawn(|| { d.call("run.resume_queue", json!({"run_id":run})); }); }
    });
    until("resumed once", || d.run(&run)["status"] == "completed");
    assert_eq!(turns(&d, &run).len(), 4);
    assert_eq!(turns(&d, &run)[3]["prompt"], "send once");
}

#[test]
fn ac265_empty_resume_restores_normal_addition_batching() {
    let r = tmp(); let repo = repo(&r.path().join("repo"));
    let d = Daemon::start(&[("OVERSEER_CLAUDE_PATH", &fixture()), ("CLAUDE_FIXTURE_MODE", "slow"), ("FIXTURE_SLOW_MS", "1500"),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "CLAUDE_FIXTURE_MODE,FIXTURE_SLOW_MS")]);
    let run = start(&d, &repo, "Empty resume");
    for clear in [false, true] {
        if clear { d.call("run.queue", json!({"run_id":run,"text":"discard"})); }
        d.call("run.interrupt", json!({"run_id":run})); d.wait_done(&run, 20);
        if clear { d.call("run.clear_queue", json!({"run_id":run})); }
        d.call("run.resume_queue", json!({"run_id":run}));
        let before = turns(&d, &run).len();
        d.call("run.follow_up", json!({"run_id":run,"prompt":"normal work"}));
        d.call("run.queue", json!({"run_id":run,"text":"first normal addition"}));
        d.call("run.queue", json!({"run_id":run,"text":"second normal addition"}));
        until("normal additions batched after empty resume", || turns(&d, &run).len() > before + 1);
        assert_eq!(turns(&d, &run)[before + 1]["prompt"], "first normal addition\n\nsecond normal addition");
        d.wait_done(&run, 20);
        if !clear { d.call("run.follow_up", json!({"run_id":run,"prompt":"next stop"})); }
    }
}
