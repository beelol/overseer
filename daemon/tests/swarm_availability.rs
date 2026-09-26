mod common;

use common::*;
use serde_json::{json, Value};
use std::time::{SystemTime, UNIX_EPOCH};

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

fn snapshot(at: i64, healthy: bool, remaining: i64) -> Value {
    json!({"version":1,"observed_ms":at-1000,"expires_ms":at+60000,
        "targets":if healthy { json!([{"id":"route-a","account_id":"account-a",
            "pool_ids":["pool-a"],"capabilities":["code"],"health":"up","auth":"ok"}]) }
            else { json!([]) },
        "pools":[{"id":"pool-a","windows":[{"id":"week","unit":"points",
            "remaining_milli":remaining,"protected_milli":0,"reserved_milli":0,
            "confidence":"exact","expires_ms":at+60000}]}]})
}

#[test]
fn missing_target_blocks_durably_and_only_eligibility_change_wakes() {
    let mut d = Daemon::start(&[]);
    let run = d.call(
        "swarm.create",
        json!({"category":"Unavailable backend route",
        "objective":"Audit backend","allowed_targets":["route-a"]}),
    );
    let id = run["id"].as_str().unwrap();
    let at = now();
    let observe = |d: &Daemon, when: i64, snap: Value| {
        d.call(
            "swarm.availability.observe",
            json!({
        "run_id":id,"snapshot":snap,"now_ms":when,
        "required_capabilities":["code"],"estimate_milli":{"points":100},
        "purpose":"worker"}),
        )
    };
    let blocked = observe(&d, at, snapshot(at, false, 100000));
    assert_eq!(blocked["state"], "blocked");
    assert_eq!(blocked["reason"], "allowed_target_missing");
    assert_eq!(blocked["woken"], false);
    assert_eq!(
        d.call("swarm.get", json!({"id":id}))["availability"]["state"],
        "blocked"
    );
    assert_eq!(
        d.call(
            "swarm.director.claim_batch",
            json!({"run_id":id,
        "generation":1,"revision":0,"now_ms":at})
        )["status"],
        "blocked"
    );
    d.kill9();
    d.spawn();
    assert_eq!(
        d.call("swarm.get", json!({"id":id}))["availability"]["reason"],
        "allowed_target_missing"
    );
    let unchanged = observe(&d, at, snapshot(at, false, 100000));
    assert_eq!(unchanged["changed"], false);
    assert_eq!(unchanged["woken"], false);
    assert_eq!(unchanged["wake_count"], 0);
    for (purpose, estimate) in [("finishing", 100), ("worker", 1)] {
        let err = d
            .try_call(
                "swarm.availability.observe",
                json!({"run_id":id,
                "snapshot":snapshot(at+2000,true,100000),"now_ms":at+2000,
                "required_capabilities":["code"],"estimate_milli":{"points":estimate},
                "purpose":purpose}),
            )
            .unwrap_err();
        assert!(err.contains("availability assessment changed"), "{err}");
    }
    assert_eq!(
        d.call("swarm.get", json!({"id":id}))["availability"]["state"],
        "blocked"
    );
    let available = observe(&d, at + 2000, snapshot(at + 2000, true, 100000));
    assert_eq!(available["state"], "eligible");
    assert_eq!(available["woken"], true);
    assert_eq!(available["wake_count"], 1);
    assert_eq!(
        d.call("swarm.get", json!({"id":id}))["availability"]["state"],
        "eligible"
    );
    let wake = d.call(
        "swarm.director.claim_batch",
        json!({"run_id":id,
        "generation":1,"revision":0,"now_ms":at+8000}),
    );
    assert_eq!(wake["status"], "claimed");
    assert_eq!(wake["messages"][0]["type"], "availability");
    assert!(d
        .try_call(
            "swarm.availability.observe",
            json!({"run_id":id,
        "snapshot":snapshot(at+1000,false,100000),"now_ms":at+1000,
        "required_capabilities":["code"],"estimate_milli":{"points":100},
        "purpose":"worker"})
        )
        .unwrap_err()
        .contains("out-of-order"));
}

#[test]
fn midrun_target_loss_preserves_worker_evidence_and_blocks_new_admission() {
    let d = Daemon::start(&[]);
    let run = d.call(
        "swarm.create",
        json!({"category":"Midrun route loss",
        "objective":"Audit backend","allowed_targets":["route-a"]}),
    );
    let id = run["id"].as_str().unwrap();
    d.call(
        "swarm.plan",
        json!({"id":id,"generation":1,"revision":0,"jobs":[
            {"id":"active","title":"Inspect active worker","acceptance":"evidence","deps":[]},
            {"id":"next","title":"Inspect next worker","acceptance":"evidence","deps":[]}
        ]}),
    );
    commit_beneficial_batch(&d, id, &["active".into(), "next".into()]);
    let at = now();
    let observe = |when: i64, healthy: bool| {
        d.call(
            "swarm.availability.observe",
            json!({
        "run_id":id,"snapshot":snapshot(when,healthy,100000),"now_ms":when,
        "required_capabilities":["code"],"estimate_milli":{"points":100},
        "purpose":"worker"}),
        )
    };
    observe(at, true);
    let admitted = d.call(
        "swarm.admit",
        json!({"run_id":id,"generation":1,
        "revision":1,"job_id":"active","target_id":"route-a","request_id":"first",
        "snapshot":snapshot(at,true,100000),"now_ms":at,
        "required_capabilities":["code"],"estimate_milli":{"points":100},
        "purpose":"worker"}),
    );
    assert_eq!(admitted["status"], "admitted");
    let blocked = observe(at + 2000, false);
    assert_eq!(blocked["state"], "blocked");
    assert_eq!(blocked["reason"], "allowed_target_missing");
    let held = d.call(
        "swarm.admit",
        json!({"run_id":id,"generation":1,
        "revision":1,"job_id":"next","target_id":"route-a","request_id":"second",
        "snapshot":snapshot(at+2000,true,100000),"now_ms":at+2000,
        "required_capabilities":["code"],"estimate_milli":{"points":100},
        "purpose":"worker"}),
    );
    assert_eq!(held["status"], "blocked");
    assert_eq!(held["reason"], "run_availability_blocked");
    let scheduler = d.call(
        "swarm.schedule.next",
        json!({"request_id":"held-schedule",
        "target_id":"route-a","snapshot":snapshot(at+2000,true,100000),
        "now_ms":at+2000,"required_capabilities":["code"],
        "estimate_milli":{"points":100},"purpose":"worker"}),
    );
    assert_eq!(scheduler["status"], "blocked");
    d.call(
        "swarm.artifact.put",
        json!({"run_id":id,"job_id":"active",
        "attempt_id":admitted["attempt_id"],"token":admitted["token"],
        "artifact_id":"before-outage","source_revision":1,"kind":"finding",
        "content":"confirmed endpoint response"}),
    );
    d.call(
        "swarm.report",
        json!({"run_id":id,"job_id":"active",
        "attempt_id":admitted["attempt_id"],"token":admitted["token"],
        "message_id":"discovery-before-outage","type":"discovery","revision":1,
        "payload":{"artifact_ids":["before-outage"]}}),
    );
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let artifact_count: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM swarm_artifacts
        WHERE run_id=?1 AND id='before-outage'",
            [id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(artifact_count, 1);
    let inbox = d.call(
        "swarm.director.claim_batch",
        json!({"run_id":id,
        "generation":1,"revision":1,"now_ms":at+8000}),
    );
    assert_eq!(inbox["status"], "claimed");
    assert_eq!(
        inbox["messages"][0]["message_id"],
        "discovery-before-outage"
    );
    let available = observe(at + 9000, true);
    assert_eq!(available["woken"], true);
    assert_eq!(available["wake_count"], 1);
    let resumed = d.call(
        "swarm.admit",
        json!({"run_id":id,"generation":1,
        "revision":1,"job_id":"next","target_id":"route-a","request_id":"second",
        "snapshot":snapshot(at+9000,true,100000),"now_ms":at+9000,
        "required_capabilities":["code"],"estimate_milli":{"points":100},
        "purpose":"worker"}),
    );
    assert_eq!(resumed["status"], "admitted");
}

#[test]
fn exhausted_finishing_capacity_blocks_then_wakes_with_more_headroom() {
    let d = Daemon::start(&[]);
    let run = d.call(
        "swarm.create",
        json!({"category":"Finishing capacity",
        "objective":"Review backend evidence","allowed_targets":["route-a"]}),
    );
    let id = run["id"].as_str().unwrap();
    let at = now();
    let observe = |when: i64, remaining: i64| {
        d.call(
            "swarm.availability.observe",
            json!({
        "run_id":id,"snapshot":snapshot(when,true,remaining),"now_ms":when,
        "required_capabilities":["code"],"estimate_milli":{"points":20000},
        "purpose":"finishing"}),
        )
    };
    let blocked = observe(at, 100000);
    assert_eq!(blocked["state"], "blocked");
    assert_eq!(blocked["reason"], "finishing_reserve");
    let available = observe(at + 2000, 1000000);
    assert_eq!(available["state"], "eligible");
    assert_eq!(available["woken"], true);
}

#[test]
fn eligibility_recovery_after_original_deadline_does_not_wake_run() {
    let d = Daemon::start(&[]);
    let run = d.call(
        "swarm.create",
        json!({"category":"Expired availability",
        "objective":"Audit backend","allowed_targets":["route-a"],
        "policy":{"deadline_ms":15000}}),
    );
    let id = run["id"].as_str().unwrap();
    let at = now();
    d.call(
        "swarm.availability.observe",
        json!({"run_id":id,
        "snapshot":snapshot(at,false,100000),"now_ms":at,
        "required_capabilities":["code"],"estimate_milli":{"points":100},
        "purpose":"worker"}),
    );
    let late = at + 20000;
    let recovery = d.call(
        "swarm.availability.observe",
        json!({"run_id":id,
        "snapshot":snapshot(late,true,100000),"now_ms":late,
        "required_capabilities":["code"],"estimate_milli":{"points":100},
        "purpose":"worker"}),
    );
    assert_eq!(recovery["state"], "blocked");
    assert_eq!(recovery["reason"], "run_deadline");
    assert_eq!(recovery["woken"], false);
    assert_eq!(d.call("swarm.get", json!({"id":id}))["status"], "stopping");
}
