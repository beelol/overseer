mod common;

use common::*;
use serde_json::{json, Value};
use std::time::{SystemTime, UNIX_EPOCH};

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as i64
}

fn snapshot(at: i64) -> Value {
    json!({"version":1,"observed_ms":at-1000,"expires_ms":at+60000,
        "targets":[
            {"id":"route-a","account_id":"account-a","pool_ids":["pool-a"],
                "capabilities":["code"],"health":"up","auth":"ok"},
            {"id":"route-b","account_id":"account-b","pool_ids":["pool-b"],
                "capabilities":["code"],"health":"up","auth":"ok"}],
        "pools":[
            {"id":"pool-a","windows":[{"id":"run","unit":"points",
                "remaining_milli":100000,"protected_milli":0,"reserved_milli":0,
                "confidence":"exact","expires_ms":at+60000}]},
            {"id":"pool-b","windows":[{"id":"run","unit":"points",
                "remaining_milli":100000,"protected_milli":0,"reserved_milli":0,
                "confidence":"exact","expires_ms":at+60000}]}]})
}

#[test]
fn two_failed_launch_routes_exhaust_one_logical_jobs_attempts() {
    let d = Daemon::start(&[]);
    let temp = tmp();
    let checkout = repo(&temp.path().join("routing-source"));
    let run = d.call("swarm.create", json!({"category":"Routing failure",
        "objective":"Audit a backend","allowed_targets":["route-a","route-b"]}));
    let id = run["id"].as_str().unwrap();
    d.call("swarm.plan", json!({"id":id,"generation":1,"revision":0,"jobs":[
        {"id":"inspect","title":"Inspect backend","acceptance":"evidence","deps":[]}
    ]}));
    let at = now();
    for (n,target) in ["route-a","route-b"].iter().enumerate() {
        let admitted = d.call("swarm.admit", json!({"run_id":id,"generation":1,
            "revision":1,"job_id":"inspect","target_id":target,
            "request_id":format!("routing-{n}"),"snapshot":snapshot(at),"now_ms":at,
            "required_capabilities":["code"],"estimate_milli":{"points":100},
            "purpose":"worker"}));
        assert_eq!(admitted["status"], "admitted", "{admitted}");
        let launched = d.call("swarm.worker.launch", json!({"run_id":id,
            "job_id":"inspect","attempt_id":admitted["attempt_id"],
            "token":admitted["token"],"repo":checkout,
            "program":"/definitely/missing/swarm-worker","args":[],
            "prompt":"Inspect the backend","title":format!("Route {n}")}));
        assert_eq!(launched["status"], "launched", "{launched}");
        let worker = launched["overseer_run_id"].as_str().unwrap();
        assert_eq!(d.wait_done(worker, 5)["status"], "failed");
        let terminal = d.call("swarm.worker.reconcile", json!({"run_id":id,
            "job_id":"inspect","attempt_id":admitted["attempt_id"],
            "generation":1,"revision":1}));
        assert_eq!(terminal["status"], "terminal", "{terminal}");
        let job = &d.call("swarm.jobs", json!({"id":id}))["jobs"][0];
        assert_eq!(job["attempt_count"], n + 1);
        assert_eq!(job["status"], if n == 0 { "ready" } else { "failed" });
        let replay = d.call("swarm.worker.reconcile", json!({"run_id":id,
            "job_id":"inspect","attempt_id":admitted["attempt_id"],
            "generation":1,"revision":1}));
        assert_eq!(replay["status"], "terminal");
        assert_eq!(replay["duplicate"], true);
        assert_eq!(d.call("swarm.jobs", json!({"id":id}))["jobs"][0]["attempt_count"], n + 1);
    }
    d.call("swarm.revise", json!({"id":id,"generation":1,"expected_revision":1,
        "reason":"Retry route again","jobs":[
            {"id":"inspect","title":"Inspect backend again","acceptance":"evidence","deps":[]}
        ]}));
    assert_eq!(d.call("swarm.jobs", json!({"id":id}))["jobs"][0]["status"], "failed");
    assert!(d.try_call("swarm.attempt.register", json!({"run_id":id,
        "generation":1,"revision":2,"job_id":"inspect"})).is_err());
}

#[test]
fn failed_worker_with_unknown_effect_cannot_route_to_replacement() {
    let d = Daemon::start(&[]);
    let temp = tmp();
    let checkout = repo(&temp.path().join("effect-route-source"));
    let run = d.call("swarm.create", json!({"category":"Effect route failure",
        "objective":"Audit a billing event","allowed_targets":["route-a","route-b"]}));
    let id = run["id"].as_str().unwrap();
    d.call("swarm.plan", json!({"id":id,"generation":1,"revision":0,"jobs":[
        {"id":"inspect","title":"Inspect billing event","acceptance":"evidence","deps":[]}
    ]}));
    let at = now();
    let admitted = d.call("swarm.admit", json!({"run_id":id,"generation":1,
        "revision":1,"job_id":"inspect","target_id":"route-a",
        "request_id":"effect-routing-0","snapshot":snapshot(at),"now_ms":at,
        "required_capabilities":["code"],"estimate_milli":{"points":100},
        "purpose":"worker","resource_claims":[{"resource":"billing:evt-45","mode":"write"}]}));
    d.call("swarm.effect.begin", json!({"run_id":id,"job_id":"inspect",
        "attempt_id":admitted["attempt_id"],"token":admitted["token"],
        "effect_id":"evt-45-grant","operation_id":"fixture:billing:evt-45:grant",
        "revision":1}));
    let launched = d.call("swarm.worker.launch", json!({"run_id":id,
        "job_id":"inspect","attempt_id":admitted["attempt_id"],
        "token":admitted["token"],"repo":checkout,
        "program":"/definitely/missing/swarm-worker","args":[],
        "prompt":"Inspect billing event","title":"Effect route"}));
    let worker = launched["overseer_run_id"].as_str().unwrap();
    assert_eq!(d.wait_done(worker, 5)["status"], "failed");
    d.call("swarm.worker.reconcile", json!({"run_id":id,
        "job_id":"inspect","attempt_id":admitted["attempt_id"],
        "generation":1,"revision":1}));
    assert_eq!(d.call("swarm.jobs", json!({"id":id}))["jobs"][0]["status"], "blocked");
    let replacement = d.call("swarm.admit", json!({"run_id":id,"generation":1,
        "revision":1,"job_id":"inspect","target_id":"route-b",
        "request_id":"effect-routing-1","snapshot":snapshot(at),"now_ms":at,
        "required_capabilities":["code"],"estimate_milli":{"points":100},
        "purpose":"worker"}));
    assert_eq!(replacement["status"], "blocked");
}
