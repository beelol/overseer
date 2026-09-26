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

fn admit(d: &Daemon, run: &str, job: &str, target: &str) -> Value {
    let at = now();
    let result = d.call("swarm.admit", json!({"run_id":run,"generation":1,"revision":1,
        "job_id":job,"target_id":target,"request_id":format!("admit-{job}"),"now_ms":at,
        "snapshot":{"version":1,"observed_ms":at-1000,"expires_ms":at+60000,
            "targets":[
                {"id":"account-a","account_id":"a","pool_ids":["pool-a"],"capabilities":["code"],"health":"up","auth":"ok"},
                {"id":"account-b","account_id":"b","pool_ids":["pool-b"],"capabilities":["code"],"health":"up","auth":"ok"}],
            "pools":[
                {"id":"pool-a","windows":[{"id":"run","unit":"points","remaining_milli":100000,"protected_milli":0,"reserved_milli":0,"confidence":"exact","expires_ms":at+60000}]},
                {"id":"pool-b","windows":[{"id":"run","unit":"points","remaining_milli":100000,"protected_milli":0,"reserved_milli":0,"confidence":"exact","expires_ms":at+60000}]}]},
        "required_capabilities":["code"],"estimate_milli":{"points":100},"purpose":"worker"}));
    assert_eq!(result["status"], "admitted", "{result}");
    result
}

#[test]
fn hundred_job_summary_and_scoped_large_artifact_context() {
    let d = Daemon::start(&[]);
    let run = d.call(
        "swarm.create",
        json!({"category":"Context fixture", "objective":"Audit backend",
        "allowed_targets":["account-a","account-b"],"policy":{"max_executing":4}}),
    );
    let id = run["id"].as_str().unwrap();
    let mut jobs = vec![
        json!({"id":"parent","title":"Inspect API contract","acceptance":"Save checked evidence","deps":[]}),
        json!({"id":"child-a","title":"Check client A","acceptance":"Verify parent contract","deps":["parent"]}),
        json!({"id":"child-b","title":"Check client B","acceptance":"Verify parent contract","deps":["parent"]}),
    ];
    for n in 0..97 {
        jobs.push(json!({"id":format!("j{n:02}"),"title":format!("Independent {n}"),"acceptance":"Evidence","deps":[]}));
    }
    d.call(
        "swarm.plan",
        json!({"id":id,"generation":1,"revision":0,"jobs":jobs}),
    );
    let summary = d.call(
        "swarm.director.summary",
        json!({"run_id":id,"generation":1,"revision":1,"max_inline_bytes":4096}),
    );
    assert!(summary.to_string().len() <= 4096, "{summary}");
    assert_eq!(summary["counts"]["total"], 100);
    assert!(summary["next_cursor"].is_string());

    let parent = admit(&d, id, "parent", "account-a");
    let aid = parent["attempt_id"].as_str().unwrap();
    let token = parent["token"].as_str().unwrap();
    let evidence = "é".repeat(40_000);
    d.call("swarm.artifact.put",json!({"run_id":id,"job_id":"parent","attempt_id":aid,
        "token":token,"artifact_id":"contract-v1","source_revision":1,"kind":"contract","content":evidence}));
    d.call("swarm.report",json!({"run_id":id,"job_id":"parent","attempt_id":aid,"token":token,
        "message_id":"parent-result","type":"result","revision":1,"payload":{"artifact_ids":["contract-v1"]}}));
    d.call(
        "swarm.decide",
        json!({"run_id":id,"generation":1,"revision":1,"job_id":"parent",
        "decision":"accept","evidence":["contract-v1"]}),
    );
    d.call(
        "swarm.attempt.confirm_exit",
        json!({"run_id":id,"generation":1,"revision":1,
        "job_id":"parent","attempt_id":aid}),
    );
    let same = admit(&d, id, "child-a", "account-a");
    let other = admit(&d, id, "child-b", "account-b");
    let brief = d.call(
        "swarm.worker.brief",
        json!({"run_id":id,"job_id":"child-a",
        "attempt_id":same["attempt_id"],"token":same["token"],"max_inline_bytes":4096}),
    );
    assert!(brief.to_string().len() <= 4096, "{brief}");
    assert_eq!(brief["job"]["acceptance"], "Verify parent contract");
    assert_eq!(brief["artifacts"][0]["id"], "contract-v1");
    assert!(!brief.to_string().contains("Independent 42"));
    let denied = d.call(
        "swarm.worker.brief",
        json!({"run_id":id,"job_id":"child-b",
        "attempt_id":other["attempt_id"],"token":other["token"]}),
    );
    assert!(denied["artifacts"].as_array().unwrap().is_empty());
    let chunk = d.call(
        "swarm.context.get",
        json!({"run_id":id,"job_id":"child-a",
        "attempt_id":same["attempt_id"],"token":same["token"],
        "artifact_id":"contract-v1","offset_bytes":0,"max_bytes":4096}),
    );
    assert!(chunk["content"].as_str().unwrap().len() <= 4096);
    assert_eq!(chunk["next_offset_bytes"], 4096);
    let end = d.call(
        "swarm.context.get",
        json!({"run_id":id,"job_id":"child-a",
        "attempt_id":same["attempt_id"],"token":same["token"],
        "artifact_id":"contract-v1","offset_bytes":4096,"max_bytes":16384}),
    );
    assert_eq!(end["next_offset_bytes"], 20480);
    assert!(d
        .try_call(
            "swarm.context.get",
            json!({"run_id":id,"job_id":"child-b",
        "attempt_id":other["attempt_id"],"token":other["token"],"artifact_id":"contract-v1"})
        )
        .is_err());
    assert!(d
        .try_call(
            "swarm.context.get",
            json!({"run_id":id,"job_id":"child-a",
        "attempt_id":same["attempt_id"],"token":other["token"],"artifact_id":"contract-v1"})
        )
        .is_err());
    assert!(d.try_call("swarm.context.get",json!({"run_id":id,"job_id":"child-a",
        "attempt_id":same["attempt_id"],"token":same["token"],"artifact_id":"contract-v1","offset_bytes":1})).is_err());
    let temp = tmp();
    let checkout = repo(&temp.path().join("context-worker"));
    let launched = d.call(
        "swarm.worker.launch",
        json!({"run_id":id,"job_id":"child-a",
        "attempt_id":same["attempt_id"],"token":same["token"],"repo":checkout,
        "program":"/bin/true","args":[],"prompt":"Check the client","title":"Context worker"}),
    );
    assert_eq!(launched["status"], "launched");
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let saved_prompt: String = db
        .query_row(
            "SELECT t.prompt FROM tasks t JOIN runs r ON r.task_id=t.id WHERE r.id=?1",
            [launched["overseer_run_id"].as_str().unwrap()],
            |r| r.get(0),
        )
        .unwrap();
    assert!(saved_prompt.contains("contract-v1"));
    assert!(saved_prompt.contains("Verify parent contract"));
    assert!(!saved_prompt.contains(&evidence));
    assert!(saved_prompt.len() <= 32 * 1024);
}
