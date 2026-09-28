mod common;

use common::*;
use serde_json::json;

#[test]
fn stale_remote_stop_loses_to_newer_controls_and_fresh_stop_replays_once() {
    let mut d=Daemon::start(&[]);
    let run=d.call("swarm.create",json!({"category":"Remote Stop ordering",
        "objective":"Inspect backend","allowed_targets":["fixture-local"]}));
    let id=run["id"].as_str().unwrap();
    assert_eq!(run["control_revision"],0);
    d.call("swarm.pause",json!({"run_id":id,"generation":1,"revision":0}));
    d.call("swarm.resume",json!({"run_id":id,"generation":1,"revision":0}));
    let current=d.call("swarm.get",json!({"id":id}));
    assert_eq!(current["control_revision"],2);
    let old=json!({"run_id":id,"request_scope":"device-a","request_id":"queued-stop",
        "expected_revision":0,"expected_control_revision":0});
    let error=d.try_call("swarm.stop",old).unwrap_err();
    assert!(error.contains("stale stop control revision"),"{error}");
    assert_eq!(d.call("swarm.get",json!({"id":id}))["status"],"running");
    d.call("swarm.limit.set",json!({"run_id":id,"request_id":"newer-local-limit",
        "expected_limit_revision":0,"max_workers":7}));
    assert_eq!(d.call("swarm.get",json!({"id":id}))["control_revision"],3);
    let old_limit=d.try_call("swarm.stop",json!({"run_id":id,
        "request_scope":"device-a","request_id":"old-limit-stop",
        "expected_revision":0,"expected_control_revision":2})).unwrap_err();
    assert!(old_limit.contains("stale stop control revision"),"{old_limit}");
    d.call("swarm.plan",json!({"id":id,"generation":1,"revision":0,"jobs":[
        {"id":"inspect","title":"Inspect","acceptance":"evidence","deps":[]}
    ]}));
    let old_plan=d.try_call("swarm.stop",json!({"run_id":id,
        "request_scope":"device-a","request_id":"old-plan-stop",
        "expected_revision":0,"expected_control_revision":3})).unwrap_err();
    assert!(old_plan.contains("stale stop plan revision"),"{old_plan}");
    let fresh=json!({"run_id":id,"request_scope":"device-a","request_id":"fresh-stop",
        "expected_revision":1,"expected_control_revision":3});
    let first=d.call("swarm.stop",fresh.clone());
    assert_eq!(first["status"],"stopped","{first}");
    d.kill9();
    d.spawn();
    let replay=d.call("swarm.stop",fresh.clone());
    assert_eq!(replay["duplicate"],true,"{replay}");
    let changed=d.try_call("swarm.stop",json!({"run_id":id,"request_scope":"device-a",
        "request_id":"fresh-stop","expected_revision":1,"expected_control_revision":4})).unwrap_err();
    assert!(changed.contains("request id reused with different input"),"{changed}");
    let db=rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let stops:i64=db.query_row("SELECT COUNT(*) FROM swarm_operation_order
        WHERE run_id=?1 AND kind='stop'",[id],|r|r.get(0)).unwrap();
    assert_eq!(stops,1);
}
