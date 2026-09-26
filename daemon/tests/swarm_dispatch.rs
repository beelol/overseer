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
fn dispatch_recovers_admitted_but_unlaunched_worker_without_duplicate_attempt() {
    let mut d = Daemon::start(&[]);
    let temp = tmp();
    let checkout = repo(&temp.path().join("dispatch-source"));
    let run = d.call(
        "swarm.create",
        json!({"category":"Dispatch backend","objective":"Audit API",
        "allowed_targets":["fixture"]}),
    );
    let run_id = run["id"].as_str().unwrap();
    d.call(
        "swarm.plan",
        json!({"id":run_id,"generation":1,"revision":0,"jobs":[
            {"id":"routes","title":"Inspect routes","acceptance":"Route evidence","deps":[]},
            {"id":"db","title":"Inspect database","acceptance":"DB evidence","deps":[]},
            {"id":"verify","title":"Verify backend","acceptance":"Verification evidence","deps":[]}
        ]}),
    );
    let at = now();
    let base = json!({"request_id":"dispatch-one","target_id":"fixture","repo":checkout,
        "program":"/bin/sleep","args":["30"],"now_ms":at,
        "snapshot":{"version":1,"observed_ms":at-1000,"expires_ms":at+60000,
            "targets":[{"id":"fixture","account_id":"fixture","pool_ids":["pool"],
                "capabilities":["code"],"health":"up","auth":"ok"}],
            "pools":[{"id":"pool","windows":[{"id":"run","unit":"points",
                "remaining_milli":100000,"protected_milli":0,"reserved_milli":0,
                "confidence":"exact","expires_ms":at+60000}]}]},
        "required_capabilities":["code"],"estimate_milli":{"points":100},
        "purpose":"worker","inject_failure_after_admit_once":true});
    assert!(d
        .try_call("swarm.dispatch.next", base.clone())
        .unwrap_err()
        .contains("injected"));
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let attempts: i64 = db
        .query_row("SELECT COUNT(*) FROM swarm_attempts", [], |r| r.get(0))
        .unwrap();
    assert_eq!(attempts, 1);
    assert!(d.runs().is_empty());
    drop(db);
    d.kill9();
    d.spawn();
    // Startup reconciles only the already-admitted intent. No caller retry is needed.
    assert_eq!(d.runs().len(), 1);
    let first = d.call("swarm.dispatch.next", base.clone());
    assert_eq!(first["status"], "linked", "{first}");
    assert_eq!(first["run_id"], run_id);
    assert_eq!(first["job_id"], "db");
    let worker = first["overseer_run_id"].as_str().unwrap();
    d.kill9();
    d.spawn();
    let replay = d.call("swarm.dispatch.next", base.clone());
    assert_eq!(replay["overseer_run_id"], worker);
    assert_eq!(replay["duplicate"], true);
    assert!(d
        .try_call(
            "swarm.dispatch.next",
            json!({"request_id":"dispatch-one",
        "target_id":"other","repo":checkout,"program":"/bin/sleep","args":["30"],
        "now_ms":at,"snapshot":base["snapshot"],"required_capabilities":["code"],
        "estimate_milli":{"points":100},"purpose":"worker",
        "inject_failure_after_admit_once":true})
        )
        .is_err());
    let shared_snapshot = base["snapshot"].clone();
    let mut second = base;
    second["request_id"] = json!("dispatch-two");
    second["inject_failure_after_admit_once"] = json!(false);
    let next = d.call("swarm.dispatch.next", second);
    assert_eq!(next["status"], "launched", "{next}");
    assert_eq!(next["job_id"], "routes");
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let attempts: i64 = db
        .query_row("SELECT COUNT(*) FROM swarm_attempts", [], |r| r.get(0))
        .unwrap();
    assert_eq!(attempts, 2);
    let launches: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM swarm_worker_launches WHERE overseer_run_id IS NOT NULL",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(launches, 2);
    drop(db);
    let pending = json!({"request_id":"dispatch-three","target_id":"fixture","repo":checkout,
        "program":"/bin/sleep","args":["30"],"now_ms":at,
        "snapshot":shared_snapshot,"required_capabilities":["code"],
        "estimate_milli":{"points":100},"purpose":"worker",
        "inject_failure_after_admit_once":true});
    assert!(d
        .try_call("swarm.dispatch.next", pending.clone())
        .unwrap_err()
        .contains("injected"));
    d.call(
        "swarm.stop",
        json!({"run_id":run_id,"generation":1,"revision":1}),
    );
    assert!(d.try_call("swarm.dispatch.next", pending).is_err());
    assert_eq!(d.runs().len(), 2);
    d.wait_done(worker, 5);
}

#[test]
fn startup_does_not_launch_an_admitted_worker_from_an_expired_snapshot() {
    let mut d = Daemon::start(&[]);
    let temp = tmp();
    let checkout = repo(&temp.path().join("expired-dispatch-source"));
    let run = d.call(
        "swarm.create",
        json!({"category":"Expired dispatch","objective":"Inspect backend",
            "allowed_targets":["fixture"]}),
    );
    let run_id = run["id"].as_str().unwrap();
    d.call(
        "swarm.plan",
        json!({"id":run_id,"generation":1,"revision":0,"jobs":[
            {"id":"inspect","title":"Inspect backend","acceptance":"Evidence","deps":[]}
        ]}),
    );
    let at = now();
    let request = json!({"request_id":"expiring-dispatch","target_id":"fixture",
        "repo":checkout,"program":"/bin/sleep","args":["30"],"now_ms":at,
        "snapshot":{"version":1,"observed_ms":at-1000,"expires_ms":at+150,
            "targets":[{"id":"fixture","account_id":"fixture","pool_ids":["pool"],
                "capabilities":["code"],"health":"up","auth":"ok"}],
            "pools":[{"id":"pool","windows":[{"id":"run","unit":"points",
                "remaining_milli":100000,"protected_milli":0,"reserved_milli":0,
                "confidence":"exact","expires_ms":at+150}]}]},
        "required_capabilities":["code"],"estimate_milli":{"points":100},
        "purpose":"worker","inject_failure_after_admit_once":true});
    assert!(d.try_call("swarm.dispatch.next", request.clone()).is_err());
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    db.execute(
        "UPDATE swarm_runs SET allowed_targets='[]' WHERE id=?1",
        [run_id],
    )
    .unwrap();
    assert!(d
        .try_call("swarm.dispatch.next", request.clone())
        .unwrap_err()
        .contains("permission revoked"));
    db.execute(
        "UPDATE swarm_runs SET allowed_targets='[\"fixture\"]' WHERE id=?1",
        [run_id],
    )
    .unwrap();
    drop(db);
    std::thread::sleep(std::time::Duration::from_millis(200));
    d.kill9();
    d.spawn();
    assert!(d.runs().is_empty());
    assert!(d
        .try_call("swarm.dispatch.next", request)
        .unwrap_err()
        .contains("expired"));
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let attempts: i64 = db
        .query_row("SELECT COUNT(*) FROM swarm_attempts", [], |r| r.get(0))
        .unwrap();
    assert_eq!(attempts, 1);
}
