mod common;

use common::*;
use serde_json::json;

#[test]
fn pause_resume_and_off_keep_active_evidence_but_stop_new_delegation() {
    let d = Daemon::start(&[]);
    let run = d.call(
        "swarm.create",
        json!({"category":"Controls","objective":"Audit","allowed_targets":["system-codex"]}),
    );
    let id = run["id"].as_str().unwrap();
    d.call(
        "swarm.plan",
        json!({"id":id,"generation":1,"revision":0,"jobs":[
            {"id":"active","title":"Active","acceptance":"evidence","deps":[]},
            {"id":"queued","title":"Queued","acceptance":"evidence","deps":[]}
        ]}),
    );
    let attempt = d.call(
        "swarm.attempt.register",
        json!({"run_id":id,"generation":1,"revision":1,"job_id":"active"}),
    );
    let aid = attempt["id"].as_str().unwrap();
    let token = attempt["token"].as_str().unwrap();
    assert_eq!(
        d.call(
            "swarm.pause",
            json!({"run_id":id,"generation":1,"revision":1})
        )["status"],
        "paused"
    );
    assert!(d
        .try_call(
            "swarm.attempt.register",
            json!({"run_id":id,"generation":1,"revision":1,"job_id":"queued"})
        )
        .is_err());
    let pending = d.call("swarm.messages", json!({"run_id":id,"recipient":aid}));
    assert!(pending["messages"]
        .as_array()
        .unwrap()
        .iter()
        .any(|m| m["type"] == "checkpoint"));
    assert_eq!(
        d.call(
            "swarm.resume",
            json!({"run_id":id,"generation":1,"revision":1})
        )["status"],
        "running"
    );
    assert_eq!(
        d.call(
            "swarm.off",
            json!({"run_id":id,"generation":1,"revision":1})
        )["status"],
        "draining"
    );
    let jobs = d.call("swarm.jobs", json!({"id":id}));
    assert_eq!(
        jobs["jobs"]
            .as_array()
            .unwrap()
            .iter()
            .find(|j| j["id"] == "queued")
            .unwrap()["status"],
        "cancelled"
    );
    assert_eq!(
        jobs["jobs"]
            .as_array()
            .unwrap()
            .iter()
            .find(|j| j["id"] == "active")
            .unwrap()["status"],
        "reserved"
    );
    d.call(
        "swarm.report",
        json!({"run_id":id,"job_id":"active","attempt_id":aid,"token":token,
        "message_id":"late-after-off","type":"result","revision":1,"payload":{"artifact_ids":[]}}),
    );
    assert_eq!(
        d.call("swarm.jobs", json!({"id":id}))["jobs"]
            .as_array()
            .unwrap()
            .iter()
            .find(|j| j["id"] == "active")
            .unwrap()["status"],
        "submitted"
    );
    assert!(d
        .try_call(
            "swarm.attempt.register",
            json!({"run_id":id,"generation":1,"revision":1,"job_id":"queued"})
        )
        .is_err());
}

#[test]
fn deadline_on_admission_stops_queued_work_without_claiming_completion() {
    let d = Daemon::start(&[]);
    let run=d.call("swarm.create",json!({"category":"Deadline","objective":"Audit","allowed_targets":["target"],"policy":{"deadline_ms":60000}}));
    let id = run["id"].as_str().unwrap();
    d.call("swarm.plan",json!({"id":id,"generation":1,"revision":0,"jobs":[{"id":"j","title":"J","acceptance":"evidence","deps":[]}]}));
    let at = run["created_ms"].as_i64().unwrap() + 60000;
    let snapshot = json!({"version":1,"observed_ms":at-1000,"expires_ms":at+60000,
        "targets":[{"id":"target","account_id":"account","pool_ids":["pool"],"capabilities":["code"],"health":"up","auth":"ok"}],
        "pools":[{"id":"pool","windows":[{"id":"week","unit":"points","remaining_milli":1000000,"protected_milli":0,"reserved_milli":0,"confidence":"exact","expires_ms":at+60000}]}]});
    let result=d.call("swarm.admit",json!({"run_id":id,"generation":1,"revision":1,"job_id":"j","target_id":"target","request_id":"expired","snapshot":snapshot,"now_ms":at,"required_capabilities":["code"],"estimate_milli":{"points":100},"purpose":"worker"}));
    assert_eq!(result["reason"], "run_deadline");
    assert_eq!(d.call("swarm.get", json!({"id":id}))["status"], "stopping");
    assert_eq!(
        d.call("swarm.jobs", json!({"id":id}))["jobs"][0]["status"],
        "cancelled"
    );
}
