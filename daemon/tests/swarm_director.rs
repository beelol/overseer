mod common;

use common::*;
use serde_json::json;
use std::time::{SystemTime, UNIX_EPOCH};

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

#[test]
fn director_batches_twenty_events_and_never_claims_two_active_turns() {
    let mut d = Daemon::start(&[]);
    let run = d.call(
        "swarm.create",
        json!({"category":"Batch","objective":"Audit","allowed_targets":["system-codex"]}),
    );
    let id = run["id"].as_str().unwrap();
    d.call("swarm.plan",json!({"id":id,"generation":1,"revision":0,"jobs":[{"id":"j","title":"J","acceptance":"evidence","deps":[]}]}));
    let attempt = d.call(
        "swarm.attempt.register",
        json!({"run_id":id,"generation":1,"revision":1,"job_id":"j"}),
    );
    let aid = attempt["id"].as_str().unwrap();
    let token = attempt["token"].as_str().unwrap();
    for n in 0..21 {
        d.call(
            "swarm.report",
            json!({"run_id":id,"job_id":"j","attempt_id":aid,"token":token,
            "message_id":format!("event-{n}"),"type":"progress","revision":1,"payload":{"step":n}}),
        );
    }
    let first = d.call(
        "swarm.director.claim_batch",
        json!({"run_id":id,"generation":1,"revision":1,"now_ms":now()}),
    );
    assert_eq!(first["status"], "claimed");
    assert_eq!(first["messages"].as_array().unwrap().len(), 20);
    assert_eq!(
        d.call(
            "swarm.director.claim_batch",
            json!({"run_id":id,"generation":1,"revision":1,"now_ms":now()+6000})
        )["status"],
        "busy"
    );
    d.kill9();
    d.spawn();
    assert_eq!(
        d.call(
            "swarm.director.claim_batch",
            json!({"run_id":id,"generation":1,"revision":1,"now_ms":now()+6000})
        )["status"],
        "busy"
    );
    let done = d.call(
        "swarm.director.complete_batch",
        json!({"run_id":id,"generation":1,"turn_id":first["turn_id"],"token":first["token"]}),
    );
    assert_eq!(done["applied"], 20);
    assert_eq!(
        d.call(
            "swarm.director.complete_batch",
            json!({"run_id":id,"generation":1,"turn_id":first["turn_id"],"token":first["token"]})
        )["duplicate"],
        true
    );
    let next = d.call(
        "swarm.director.claim_batch",
        json!({"run_id":id,"generation":1,"revision":1,"now_ms":now()+6000}),
    );
    assert_eq!(next["messages"].as_array().unwrap().len(), 1);
}

#[test]
fn director_waits_for_age_or_byte_limit_without_background_inference() {
    let d = Daemon::start(&[]);
    let run = d.call(
        "swarm.create",
        json!({"category":"Byte batch","objective":"Audit","allowed_targets":["system-codex"]}),
    );
    let id = run["id"].as_str().unwrap();
    d.call("swarm.plan",json!({"id":id,"generation":1,"revision":0,"jobs":[{"id":"j","title":"J","acceptance":"evidence","deps":[]}]}));
    let attempt = d.call(
        "swarm.attempt.register",
        json!({"run_id":id,"generation":1,"revision":1,"job_id":"j"}),
    );
    let aid = attempt["id"].as_str().unwrap();
    let token = attempt["token"].as_str().unwrap();
    assert_eq!(
        d.call(
            "swarm.director.claim_batch",
            json!({"run_id":id,"generation":1,"revision":1,"now_ms":now()})
        )["status"],
        "idle"
    );
    d.call(
        "swarm.report",
        json!({"run_id":id,"job_id":"j","attempt_id":aid,"token":token,
        "message_id":"one","type":"discovery","revision":1,"payload":{"finding":"x"}}),
    );
    assert_eq!(
        d.call(
            "swarm.director.claim_batch",
            json!({"run_id":id,"generation":1,"revision":1,"now_ms":now()})
        )["status"],
        "waiting"
    );
    let aged = d.call(
        "swarm.director.claim_batch",
        json!({"run_id":id,"generation":1,"revision":1,"now_ms":now()+6000}),
    );
    assert_eq!(aged["status"], "claimed");
    assert_eq!(aged["messages"].as_array().unwrap().len(), 1);
}

#[test]
fn inline_batch_limit_defers_excess_and_stop_halts_future_turns() {
    let d = Daemon::start(&[]);
    let run = d.call(
        "swarm.create",
        json!({"category":"Large updates","objective":"Audit","allowed_targets":["system-codex"]}),
    );
    let id = run["id"].as_str().unwrap();
    d.call("swarm.plan",json!({"id":id,"generation":1,"revision":0,"jobs":[{"id":"j","title":"J","acceptance":"evidence","deps":[]}]}));
    let attempt = d.call(
        "swarm.attempt.register",
        json!({"run_id":id,"generation":1,"revision":1,"job_id":"j"}),
    );
    let aid = attempt["id"].as_str().unwrap();
    let token = attempt["token"].as_str().unwrap();
    for n in 0..2 {
        d.call("swarm.report",json!({"run_id":id,"job_id":"j","attempt_id":aid,"token":token,
            "message_id":format!("large-{n}"),"type":"progress","revision":1,"payload":{"text":"x".repeat(20_000)}}));
    }
    let first = d.call(
        "swarm.director.claim_batch",
        json!({"run_id":id,"generation":1,"revision":1,"now_ms":now()}),
    );
    assert_eq!(first["messages"].as_array().unwrap().len(), 1);
    assert_eq!(first["more_pending"], true);
    assert!(first["inline_bytes"].as_i64().unwrap() <= 32 * 1024);
    d.call(
        "swarm.director.complete_batch",
        json!({"run_id":id,"generation":1,"turn_id":first["turn_id"],"token":first["token"]}),
    );
    d.call(
        "swarm.stop",
        json!({"run_id":id,"generation":1,"revision":1}),
    );
    let halted = d.call(
        "swarm.director.claim_batch",
        json!({"run_id":id,"generation":1,"revision":1,"now_ms":now()+6000}),
    );
    assert_eq!(halted["status"], "halted");
}
