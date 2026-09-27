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
fn revoked_selected_identity_cancels_only_its_active_attempt_and_keeps_usage_uncertain() {
    let mut d=Daemon::start(&[]);
    let created=d.call("swarm.create",json!({"category":"Revoked identity",
        "objective":"Audit selected accounts","allowed_targets":["route-a","route-a-alt","route-b"]}));
    let run=created["id"].as_str().unwrap();
    d.call("swarm.plan",json!({"id":run,"generation":1,"revision":0,"jobs":[
        {"id":"a","title":"Inspect A","acceptance":"A evidence","deps":[]},
        {"id":"b","title":"Inspect B","acceptance":"B evidence","deps":[]},
        {"id":"c","title":"Inspect A alias","acceptance":"A alias evidence","deps":[]}
    ]}));
    commit_beneficial_batch(&d,run,&["a".into(),"b".into(),"c".into()]);
    let at=now();
    let snap=|time:i64,revoked:bool|json!({"version":1,"observed_ms":time,
        "expires_ms":time+60000,"targets":[
            {"id":"route-a","account_id":"account-a","pool_ids":["pool-a"],
                "capabilities":["code"],"health":"up",
                "auth":if revoked {"revoked"} else {"ok"}},
            {"id":"route-a-alt","account_id":"account-a","pool_ids":["pool-a"],
                "capabilities":["code"],"health":"up","auth":"ok"},
            {"id":"route-b","account_id":"account-b","pool_ids":["pool-b"],
                "capabilities":["code"],"health":"up","auth":"ok"}],
        "pools":[{"id":"pool-a","windows":[{"id":"week","unit":"points",
            "remaining_milli":100000,"protected_milli":0,"reserved_milli":0,
            "confidence":"exact","expires_ms":time+60000}]},
            {"id":"pool-b","windows":[{"id":"week","unit":"points",
            "remaining_milli":100000,"protected_milli":0,"reserved_milli":0,
            "confidence":"exact","expires_ms":time+60000}]}]});
    let observe=|time:i64,revoked:bool|d.call("swarm.availability.observe",json!({
        "run_id":run,"snapshot":snap(time,revoked),"now_ms":time,
        "required_capabilities":["code"],"estimate_milli":{"points":100},
        "purpose":"worker"}));
    assert_eq!(observe(at,false)["state"],"eligible");
    let admit=|job:&str,target:&str,time:i64,revoked:bool|d.call("swarm.admit",json!({
        "run_id":run,"generation":1,"revision":1,"job_id":job,"target_id":target,
        "request_id":format!("admit-{job}"),"snapshot":snap(time,revoked),
        "now_ms":time,"required_capabilities":["code"],
        "estimate_milli":{"points":100},"purpose":"worker"}));
    let a=admit("a","route-a",at,false);
    assert_eq!(a["status"],"admitted","{a}");
    let c=admit("c","route-a-alt",at,false);
    assert_eq!(c["status"],"admitted","{c}");
    let changed=observe(at+1000,true);
    assert_eq!(changed["state"],"eligible","{changed}");
    assert_eq!(changed["revoked_jobs"],json!(["a","c"]));
    let replay=observe(at+1000,true);
    assert_eq!(replay["changed"],false);
    assert_eq!(replay["revoked_jobs"],json!([]));
    let jobs=d.call("swarm.jobs",json!({"id":run}));
    let a_job=jobs["jobs"].as_array().unwrap().iter().find(|j|j["id"]=="a").unwrap();
    assert_eq!(a_job["status"],"cancel_requested");
    assert_eq!(a_job["stop_reason"],"account_identity_revoked");
    let c_job=jobs["jobs"].as_array().unwrap().iter().find(|j|j["id"]=="c").unwrap();
    assert_eq!(c_job["status"],"cancel_requested");
    assert_eq!(c_job["stop_reason"],"account_identity_revoked");
    assert_eq!(admit("b","route-a",at+1000,true)["reason"],"auth_unavailable");
    assert_eq!(admit("b","route-a-alt",at+1000,true)["reason"],"auth_unavailable");
    let b=admit("b","route-b",at+1000,true);
    assert_eq!(b["status"],"admitted","{b}");
    d.kill9();
    d.spawn();
    let after_restart=d.call("swarm.jobs",json!({"id":run}));
    let a_after=after_restart["jobs"].as_array().unwrap().iter()
        .find(|job|job["id"]=="a").unwrap();
    assert_eq!(a_after["stop_reason"],"account_identity_revoked");
    d.call("swarm.attempt.confirm_exit",json!({"run_id":run,"generation":1,
        "revision":1,"job_id":"a","attempt_id":a["attempt_id"]}));
    let db=rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let reservation:String=db.query_row("SELECT status FROM swarm_reservations
        WHERE attempt_id=?1 LIMIT 1",[a["attempt_id"].as_str().unwrap()],|r|r.get(0)).unwrap();
    assert_eq!(reservation,"uncertain");
    let revoke_count:i64=db.query_row("SELECT COUNT(*) FROM swarm_operation_order
        WHERE run_id=?1 AND kind='revoke'",[run],|r|r.get(0)).unwrap();
    assert_eq!(revoke_count,1);
}

#[test]
fn rate_limited_provider_blocks_new_launches_without_stopping_an_independent_peer() {
    let d=Daemon::start(&[]);
    let created=d.call("swarm.create",json!({"category":"Scoped provider rate limit",
        "objective":"Audit separate routes","allowed_targets":["opencode-a","opencode-b"]}));
    let run=created["id"].as_str().unwrap();
    d.call("swarm.plan",json!({"id":run,"generation":1,"revision":0,"jobs":[
        {"id":"a","title":"Inspect A","acceptance":"A evidence","deps":[]},
        {"id":"b","title":"Inspect B","acceptance":"B evidence","deps":[]},
        {"id":"c","title":"Continue B","acceptance":"C evidence","deps":[]}
    ]}));
    commit_beneficial_batch(&d,run,&["a".into(),"b".into(),"c".into()]);
    let at=now();
    let snap=|time:i64,limited:bool|json!({"version":1,"observed_ms":time,
        "expires_ms":time+60000,"targets":[
            {"id":"opencode-a","account_id":"account-a","pool_ids":["pool-a"],
                "capabilities":["code"],"health":if limited {"rate_limited"} else {"up"},"auth":"ok"},
            {"id":"opencode-b","account_id":"account-b","pool_ids":["pool-b"],
                "capabilities":["code"],"health":"up","auth":"ok"}],
        "pools":[{"id":"pool-a","windows":[{"id":"week","unit":"points",
                "remaining_milli":100000,"protected_milli":0,"reserved_milli":0,
                "confidence":"exact","expires_ms":time+60000}]},
            {"id":"pool-b","windows":[{"id":"week","unit":"points",
                "remaining_milli":100000,"protected_milli":0,"reserved_milli":0,
                "confidence":"exact","expires_ms":time+60000}]}]});
    let healthy=snap(at-1000,false);
    d.call("swarm.availability.observe",json!({"run_id":run,"snapshot":healthy,
        "now_ms":at,"required_capabilities":["code"],
        "estimate_milli":{"points":100},"purpose":"worker"}));
    let admit=|job:&str,target:&str,id:&str,snapshot:Value,time:i64|d.call("swarm.admit",json!({
        "run_id":run,"generation":1,"revision":1,"job_id":job,"target_id":target,
        "request_id":id,"snapshot":snapshot,"now_ms":time,
        "required_capabilities":["code"],"estimate_milli":{"points":100},
        "purpose":"worker"}));
    let a=admit("a","opencode-a","first-a",healthy.clone(),at);
    let b=admit("b","opencode-b","first-b",healthy,at);
    assert_eq!(a["status"],"admitted");
    assert_eq!(b["status"],"admitted");
    let limited=snap(at+1000,true);
    let observed=d.call("swarm.availability.observe",json!({"run_id":run,
        "snapshot":limited,"now_ms":at+1500,"required_capabilities":["code"],
        "estimate_milli":{"points":100},"purpose":"worker"}));
    assert_eq!(observed["state"],"eligible");
    assert_eq!(observed["eligible_targets"],json!(["opencode-b"]));
    assert_eq!(observed["revoked_jobs"],json!([]));
    let denied=admit("c","opencode-a","limited-a",limited.clone(),at+1500);
    assert_eq!(denied["reason"],"rate_limited");
    let replacement=admit("c","opencode-b","healthy-b",limited,at+1500);
    assert_eq!(replacement["status"],"admitted","{replacement}");
    let jobs=d.call("swarm.jobs",json!({"id":run}));
    for job in ["a","b"] {
        assert_eq!(jobs["jobs"].as_array().unwrap().iter()
            .find(|record|record["id"]==job).unwrap()["status"],"reserved");
    }
    let reported=d.call("swarm.report",json!({"run_id":run,"job_id":"b",
        "attempt_id":b["attempt_id"],"token":b["token"],
        "message_id":"peer-still-working","type":"discovery","revision":1,
        "payload":{"note":"independent route still has evidence"}}));
    assert_eq!(reported["duplicate"],false);
}

#[test]
fn identity_revocation_stop_result_and_review_have_one_durable_order() {
    for steps in [
        ["result","accept","revoke","stop"],
        ["result","revoke","accept","stop"],
        ["revoke","stop","result","accept"],
    ] {
        let d=Daemon::start(&[]);
        let created=d.call("swarm.create",json!({"category":format!("Identity race {steps:?}"),
            "objective":"Audit account-scoped route","allowed_targets":["route-a"]}));
        let run=created["id"].as_str().unwrap();
        d.call("swarm.plan",json!({"id":run,"generation":1,"revision":0,"jobs":[
            {"id":"audit","title":"Audit route","acceptance":"status and row evidence","deps":[]},
            {"id":"sibling","title":"Independent route","acceptance":"separate evidence","deps":[]}
        ]}));
        commit_beneficial_batch(&d,run,&["audit".into(),"sibling".into()]);
        let at=now();
        let snap=|time:i64,revoked:bool|json!({"version":1,"observed_ms":time,
            "expires_ms":time+60000,"targets":[{"id":"route-a",
                "account_id":"account-a","pool_ids":["pool-a"],
                "capabilities":["code"],"health":"up",
                "auth":if revoked {"revoked"} else {"ok"}}],
            "pools":[{"id":"pool-a","windows":[{"id":"week","unit":"points",
                "remaining_milli":100000,"protected_milli":0,"reserved_milli":0,
                "confidence":"exact","expires_ms":time+60000}]}]});
        let healthy=snap(at-1000,false);
        d.call("swarm.availability.observe",json!({"run_id":run,"snapshot":healthy,
            "now_ms":at,"required_capabilities":["code"],
            "estimate_milli":{"points":100},"purpose":"worker"}));
        let admitted=d.call("swarm.admit",json!({"run_id":run,"generation":1,
            "revision":1,"job_id":"audit","target_id":"route-a",
            "request_id":"audit-first","snapshot":healthy,"now_ms":at,
            "required_capabilities":["code"],"estimate_milli":{"points":100},
            "purpose":"worker"}));
        assert_eq!(admitted["status"],"admitted");
        d.call("swarm.artifact.put",json!({"run_id":run,"job_id":"audit",
            "attempt_id":admitted["attempt_id"],"token":admitted["token"],
            "artifact_id":"route-proof","source_revision":1,"kind":"reproduction",
            "content":"fixture response and database row"}));
        let report=json!({"run_id":run,"job_id":"audit",
            "attempt_id":admitted["attempt_id"],"token":admitted["token"],
            "message_id":"route-result","type":"result","revision":1,
            "payload":{"audit_outcome":"confirmed_defect","artifact_ids":["route-proof"]}});
        let decide=json!({"run_id":run,"generation":1,"revision":1,
            "job_id":"audit","decision":"accept","evidence":["route-proof"]});
        let revoked=snap(at+1000,true);
        let mut accepted=false;
        for step in steps {
            match step {
                "result"=>{d.call("swarm.report",report.clone());},
                "accept"=>{accepted=d.try_call("swarm.decide",decide.clone()).is_ok();},
                "revoke"=>{let response=d.call("swarm.availability.observe",json!({
                    "run_id":run,"snapshot":revoked,"now_ms":at+1500,
                    "required_capabilities":["code"],"estimate_milli":{"points":100},
                    "purpose":"worker"}));
                    assert_eq!(response["revoked_jobs"],json!(["audit"]));},
                "stop"=>{d.call("swarm.stop",json!({"run_id":run}));},
                _=>unreachable!(),
            }
        }
        assert_eq!(accepted,steps==["result","accept","revoke","stop"]);
        let db=rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
        let mut stmt=db.prepare("SELECT kind FROM swarm_operation_order
            WHERE run_id=?1 ORDER BY seq").unwrap();
        let order:Vec<String>=stmt.query_map([run],|r|r.get(0)).unwrap()
            .collect::<rusqlite::Result<_>>().unwrap();
        let expected:Vec<String>=steps.iter().filter(|step| **step!="accept" || accepted)
            .map(|step|step.to_string()).collect();
        assert_eq!(order,expected,"{steps:?}");
        assert_eq!(d.call("swarm.report",report.clone())["duplicate"],true);
        assert_eq!(d.call("swarm.stop",json!({"run_id":run}))["duplicate"],true);
        let inbox=d.call("swarm.messages",json!({"run_id":run,"recipient":"director"}));
        assert!(inbox["messages"].as_array().unwrap().iter()
            .any(|message|message["message_id"]=="route-result"));
        assert!(inbox["messages"].as_array().unwrap().iter()
            .any(|message|message["type"]=="availability"
                && message["job_id"].is_null() && message["attempt_id"].is_null()));
        assert!(d.try_call("swarm.decide",decide).is_err() || accepted);
        assert!(d.try_call("swarm.complete",json!({"run_id":run,
            "generation":1,"revision":1,"summary":"done",
            "verification":"fixture","checks":[]})).is_err());
        let temp=tmp();
        let checkout=repo(&temp.path().join("identity-race"));
        assert!(d.try_call("swarm.worker.launch",json!({"run_id":run,
            "job_id":"audit","attempt_id":admitted["attempt_id"],
            "token":admitted["token"],"repo":checkout,
            "program":"/bin/true","args":[],"prompt":"Inspect route",
            "title":"Late worker"})).is_err());
        let replay_order:i64=db.query_row("SELECT COUNT(*) FROM swarm_operation_order
            WHERE run_id=?1",[run],|r|r.get(0)).unwrap();
        assert_eq!(replay_order,expected.len() as i64);
    }
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
    assert_eq!(d.call("swarm.get", json!({"id":id}))["status"], "stopped");
}
