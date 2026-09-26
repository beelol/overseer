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

fn snapshot(at: i64) -> Value {
    let pools: Vec<Value> = ["pa", "pb", "pc"]
        .iter()
        .map(|id| {
            json!({"id":id,"windows":[
        {"id":"week","unit":"points","remaining_milli":1000000,"protected_milli":0,
        "reserved_milli":0,"confidence":"exact","expires_ms":at+60000}]})
        })
        .collect();
    json!({"version":1,"observed_ms":at-1000,"expires_ms":at+60000,
        "targets":[
            {"id":"a","account_id":"a","pool_ids":["pa"],"capabilities":["audit"],"health":"up","auth":"ok"},
            {"id":"b","account_id":"b","pool_ids":["pb"],"capabilities":["audit"],"health":"up","auth":"ok"},
            {"id":"c","account_id":"c","pool_ids":["pc"],"capabilities":["audit"],"health":"up","auth":"ok"}],
        "pools":pools})
}

fn admit(d: &Daemon, run: &str, target: &str, at: i64, request: &str) -> Value {
    d.call(
        "swarm.admit",
        json!({"run_id":run,"generation":1,"revision":1,
        "job_id":"sql","target_id":target,"request_id":request,
        "snapshot":snapshot(at),"now_ms":at,"required_capabilities":["audit"],
        "estimate_milli":{"points":100},"purpose":"worker"}),
    )
}

#[test]
fn failed_worker_checkpoint_requires_destination_grant_before_replacement() {
    let mut d = Daemon::start(&[]);
    let tmp = tmp();
    let checkout = repo(&tmp.path().join("shipment"));
    let run = d.call(
        "swarm.create",
        json!({"category":"Shipment checkpoint",
        "objective":"Inspect sanitized incident bundle","allowed_targets":["a","b"]}),
    );
    let id = run["id"].as_str().unwrap();
    d.call(
        "swarm.plan",
        json!({"id":id,"generation":1,"revision":0,"jobs":[
            {"id":"sql","title":"Inspect SQL retries","acceptance":"evidence","deps":[]}
        ]}),
    );
    let at = now();
    let first = admit(&d, id, "a", at, "checkpoint-a");
    assert_eq!(first["status"], "admitted");
    let content = json!({"trace_ids":["ship-trace-021"],
        "source":"shipment.go:consumeWithRetry",
        "question":"Does retry hold the pool connection?"})
    .to_string();
    d.call(
        "swarm.artifact.put",
        json!({"run_id":id,"job_id":"sql",
        "attempt_id":first["attempt_id"],"token":first["token"],
        "artifact_id":"sql-checkpoint","source_revision":1,
        "kind":"checkpoint","content":content}),
    );
    let launched = d.call(
        "swarm.worker.launch",
        json!({"run_id":id,
        "job_id":"sql","attempt_id":first["attempt_id"],"token":first["token"],
        "repo":checkout,"program":"/definitely/missing/checkpoint-worker","args":[],
        "prompt":"Inspect SQL retry","title":"SQL first attempt"}),
    );
    let worker = launched["overseer_run_id"].as_str().unwrap();
    assert_eq!(d.wait_done(worker, 5)["status"], "failed");
    d.call(
        "swarm.worker.reconcile",
        json!({"run_id":id,"job_id":"sql",
        "attempt_id":first["attempt_id"],"generation":1,"revision":1}),
    );
    assert_eq!(
        d.call("swarm.jobs", json!({"id":id}))["jobs"][0]["status"],
        "ready"
    );
    d.kill9();
    d.spawn();
    assert_eq!(
        admit(&d, id, "b", at + 1000, "checkpoint-b-before")["reason"],
        "checkpoint_permission_required"
    );
    assert_eq!(
        admit(&d, id, "c", at + 1000, "checkpoint-c")["status"],
        "blocked"
    );
    let grant = json!({"run_id":id,"generation":1,"revision":1,
        "artifact_id":"sql-checkpoint","target_id":"b"});
    assert!(d
        .try_call(
            "swarm.context.grant",
            json!({"run_id":id,
        "generation":0,"revision":1,"artifact_id":"sql-checkpoint","target_id":"b"})
        )
        .is_err());
    assert!(d
        .try_call(
            "swarm.context.grant",
            json!({"run_id":id,
        "generation":1,"revision":1,"artifact_id":"sql-checkpoint","target_id":"c"})
        )
        .is_err());
    assert_eq!(
        d.call("swarm.context.grant", grant.clone())["status"],
        "granted"
    );
    assert_eq!(
        d.call("swarm.context.grant", grant.clone())["duplicate"],
        true
    );
    let second = admit(&d, id, "b", at + 1000, "checkpoint-b-after");
    assert_eq!(second["status"], "admitted", "{second}");
    assert_eq!(d.call("swarm.context.grant", grant)["duplicate"], true);
    let context = json!({"run_id":id,"job_id":"sql",
        "attempt_id":second["attempt_id"],"token":second["token"],
        "artifact_id":"sql-checkpoint"});
    let brief = d.call(
        "swarm.worker.brief",
        json!({"run_id":id,"job_id":"sql",
        "attempt_id":second["attempt_id"],"token":second["token"]}),
    );
    assert_eq!(brief["artifacts"][0]["id"], "sql-checkpoint");
    assert_eq!(
        d.call("swarm.context.get", context.clone())["content"],
        content
    );
    let replacement = d.call(
        "swarm.worker.launch",
        json!({"run_id":id,
        "job_id":"sql","attempt_id":second["attempt_id"],"token":second["token"],
        "repo":checkout,"program":"/bin/sleep","args":["30"],
        "prompt":"Continue from checkpoint","title":"SQL replacement"}),
    );
    let replacement_worker = replacement["overseer_run_id"].as_str().unwrap();
    d.wait_status(replacement_worker, |status| status == "running", 10);
    let revoked = d.call(
        "swarm.context.revoke",
        json!({"run_id":id,
        "generation":1,"revision":1,"artifact_id":"sql-checkpoint","target_id":"b"}),
    );
    assert_eq!(revoked["status"], "revoked");
    assert!(revoked["interrupt_requested"]
        .as_array()
        .unwrap()
        .iter()
        .any(|id| id == replacement_worker));
    assert_ne!(d.wait_done(replacement_worker, 10)["status"], "completed");
    assert!(d.try_call("swarm.context.get", context).is_err());
    assert_eq!(
        d.call("swarm.jobs", json!({"id":id}))["jobs"][0]["status"],
        "blocked"
    );
}
