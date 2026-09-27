mod common;

use common::*;
use serde_json::{json, Value};

fn jobs(title: &str) -> Value {
    json!([{"id":"route","title":title,"acceptance":"checked evidence","deps":[]}])
}

#[test]
fn committed_revision_replays_after_lost_reply_and_restart() {
    let mut d = Daemon::start(&[]);
    let run = d.call("swarm.create", json!({"category":"Revision replay",
        "objective":"Audit route","allowed_targets":["fixture"]}))["id"]
        .as_str().unwrap().to_string();
    d.call("swarm.plan",json!({"id":run,"generation":1,"revision":0,
        "jobs":jobs("Inspect route")}));
    let request = json!({"id":run,"generation":1,"expected_revision":1,
        "request_id":"repair-1","reason":"New evidence changes the check",
        "jobs":jobs("Inspect route with new evidence")});
    let first = d.call("swarm.revise",request.clone());
    assert_eq!(first["revision"],2);
    assert_eq!(first["duplicate"],false);
    d.kill9();
    d.spawn();
    let replay = d.call("swarm.revise",request.clone());
    assert_eq!(replay["revision"],2);
    assert_eq!(replay["duplicate"],true);
    assert_eq!(d.call("swarm.get",json!({"id":run}))["revision"],2);
    assert_eq!(d.call("swarm.jobs",json!({"id":run}))["jobs"][0]["title"],
        "Inspect route with new evidence");

    let mut changed = request.clone();
    changed["jobs"] = jobs("Different repair under reused ID");
    let error = d.try_call("swarm.revise",changed).unwrap_err();
    assert!(error.contains("reused with different input"),"{error}");
    let mut stale = request;
    stale["request_id"] = json!("repair-2");
    let error = d.try_call("swarm.revise",stale).unwrap_err();
    assert!(error.contains("stale plan revision"),"{error}");
}

#[test]
fn unchanged_revision_request_also_replays_without_new_plan_effect() {
    let mut d = Daemon::start(&[]);
    let run = d.call("swarm.create",json!({"category":"No-op revision",
        "objective":"Audit route","allowed_targets":["fixture"]}))["id"]
        .as_str().unwrap().to_string();
    d.call("swarm.plan",json!({"id":run,"generation":1,"revision":0,
        "jobs":jobs("Inspect route")}));
    let request=json!({"id":run,"generation":1,"expected_revision":1,
        "request_id":"same-1","reason":"Recheck scope","jobs":jobs("Inspect route")});
    let first=d.call("swarm.revise",request.clone());
    assert_eq!(first["unchanged"],true);
    assert_eq!(first["duplicate"],false);
    d.kill9();
    d.spawn();
    let replay=d.call("swarm.revise",request);
    assert_eq!(replay["unchanged"],true);
    assert_eq!(replay["duplicate"],true);
    assert_eq!(d.call("swarm.get",json!({"id":run}))["revision"],1);
}

#[test]
fn replay_does_not_bypass_director_owner_identity() {
    let d=Daemon::start(&[]);
    let run=d.call("swarm.create",json!({"category":"Owned revision",
        "objective":"Audit route","allowed_targets":["fixture"]}))["id"]
        .as_str().unwrap().to_string();
    let token=d.call("swarm.director.owner.begin",json!({"run_id":run,
        "generation":1}))["owner_token"].as_str().unwrap().to_string();
    d.call("swarm.plan",json!({"id":run,"generation":1,"revision":0,
        "owner_token":token,"jobs":jobs("Inspect route")}));
    let request=json!({"id":run,"generation":1,"expected_revision":1,
        "owner_token":token,"request_id":"owned-1","reason":"Revise check",
        "jobs":jobs("Inspect route again")});
    assert_eq!(d.call("swarm.revise",request.clone())["revision"],2);
    let mut forged=request.clone();
    forged["owner_token"]=json!("wrong-token");
    let error=d.try_call("swarm.revise",forged).unwrap_err();
    assert!(error.contains("invalid director owner identity"),"{error}");
    assert_eq!(d.call("swarm.revise",request)["duplicate"],true);
}

#[test]
fn rejected_repair_request_replays_without_consuming_a_second_turn() {
    let mut d = Daemon::start(&[]);
    let run = d.call("swarm.create", json!({"category":"Rejected repair replay",
        "objective":"Audit","allowed_targets":["fixture"]}))["id"]
        .as_str().unwrap().to_string();
    d.call("swarm.plan", json!({"id":run,"generation":1,"revision":0,"jobs":[
        {"id":"keep","title":"Keep","acceptance":"proof"},
        {"id":"retire","title":"Retire","acceptance":"proof"}
    ]}));
    d.call("swarm.revise", json!({"id":run,"generation":1,"expected_revision":1,
        "reason":"Narrow scope","jobs":[
            {"id":"keep","title":"Keep","acceptance":"proof"}
        ]}));
    let request = json!({"id":run,"generation":1,"expected_revision":2,
        "request_id":"failed-repair-1","reason":"Restore retired ID","jobs":[
            {"id":"keep","title":"Keep","acceptance":"proof"},
            {"id":"retire","title":"Retire again","acceptance":"proof"}
        ]});
    assert!(d.try_call("swarm.revise", request.clone()).unwrap_err()
        .contains("superseded job id"));
    assert_eq!(d.call("swarm.get", json!({"id":run}))["failed_planning_turns"], 1);
    d.kill9();
    d.spawn();
    assert!(d.try_call("swarm.revise", request.clone()).unwrap_err()
        .contains("superseded job id"));
    assert_eq!(d.call("swarm.get", json!({"id":run}))["failed_planning_turns"], 1);
    let mut changed = request.clone();
    changed["reason"] = json!("Different repair under reused ID");
    assert!(d.try_call("swarm.revise", changed).unwrap_err()
        .contains("reused with different input"));
    assert_eq!(d.call("swarm.get", json!({"id":run}))["failed_planning_turns"], 1);
    let mut second_turn = request;
    second_turn["request_id"] = json!("failed-repair-2");
    assert!(d.try_call("swarm.revise", second_turn).unwrap_err()
        .contains("superseded job id"));
    let state = d.call("swarm.get", json!({"id":run}));
    assert_eq!(state["failed_planning_turns"], 2);
    assert_eq!(state["status"], "stalled");
}

#[test]
fn invalid_dependency_repair_request_replays_without_consuming_a_second_turn() {
    let mut d = Daemon::start(&[]);
    let run = d.call("swarm.create", json!({"category":"Invalid dependency replay",
        "objective":"Audit","allowed_targets":["fixture"]}))["id"]
        .as_str().unwrap().to_string();
    d.call("swarm.plan", json!({"id":run,"generation":1,"revision":0,
        "jobs":jobs("Inspect route")}));
    let request = json!({"id":run,"generation":1,"expected_revision":1,
        "request_id":"invalid-dependency-1","reason":"Repair dependencies",
        "jobs":[{"id":"route","title":"Inspect route","acceptance":"proof",
            "deps":["missing"]}]});
    assert!(d.try_call("swarm.revise", request.clone()).unwrap_err()
        .contains("unknown dependency"));
    assert_eq!(d.call("swarm.get", json!({"id":run}))["failed_planning_turns"], 1);
    d.kill9();
    d.spawn();
    assert!(d.try_call("swarm.revise", request.clone()).unwrap_err()
        .contains("unknown dependency"));
    assert_eq!(d.call("swarm.get", json!({"id":run}))["failed_planning_turns"], 1);
    let mut second = request;
    second["request_id"] = json!("invalid-dependency-2");
    assert!(d.try_call("swarm.revise", second).unwrap_err()
        .contains("unknown dependency"));
    assert_eq!(d.call("swarm.get", json!({"id":run}))["status"], "stalled");
}
