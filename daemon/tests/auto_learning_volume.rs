//! AUTO-AC-39: the local learning store never grows a volume that could not
//! hold it. Below the documented floor (the learning file's page cap plus a
//! rollback journal of at most the same size), a run's usage is recorded as
//! an execution event but no learning sample is written and learning reports
//! paused; with room again, the next run's sample is written and the skipped
//! one is not reconstructed.

mod common;
use common::*;
use serde_json::{json, Value};

fn fixture(name: &str) -> String {
    repo_root().join("fixtures").join(name).display().to_string()
}

#[test]
fn learning_pauses_below_the_volume_floor_and_resumes_with_room() {
    let r = tmp();
    let checkout = repo(&r.path().join("repo"));
    let mut d = Daemon::start(&[("OVERSEER_CLAUDE_PATH", fixture("fake-harness/claude-fixture.js").as_str()),
        ("OVERSEER_TEST_DISK_FREE", "1000000")]);
    let usage = |d: &Daemon| d.call("auto.usage.list", json!({"limit":5000}));
    let samples = |d: &Daemon, run: &str| usage(d)["measurements"].as_array().unwrap().iter()
        .filter(|m| m["run_id"] == run).count();
    let task = |d: &Daemon, title: &str| -> String {
        let run = run_id(&d.call("task.create", json!({"repo":checkout,"harness":"claude","prompt":"hello","title":title})));
        assert_eq!(d.wait_done(&run, 20)["status"], "completed");
        run
    };

    let tight = task(&d, "tight");
    assert!(d.events(&tight).iter().any(|e| e["kind"] == "usage"), "the execution usage event is intact");
    assert_eq!(samples(&d, &tight), 0, "no learning sample below the floor");
    assert_eq!(usage(&d)["learning_paused"], true);

    d.kill9();
    d.env.retain(|(key, _)| key != "OVERSEER_TEST_DISK_FREE");
    d.env.push(("OVERSEER_TEST_DISK_FREE".into(), "100000000000".into()));
    d.spawn();
    let roomy = task(&d, "roomy");
    assert!(samples(&d, &roomy) >= 1, "with room, the next run is sampled: {}", usage(&d));
    assert_eq!(usage(&d)["learning_paused"], Value::Bool(false));
    assert_eq!(samples(&d, &tight), 0, "the skipped sample is not reconstructed");
}
