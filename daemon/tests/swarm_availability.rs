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
fn unchanged_waiting_route_does_not_create_director_turns_until_recovery() {
    let mut d=Daemon::start(&[]);
    let created=d.call("swarm.create",json!({"category":"Waiting route without planning churn",
        "objective":"Audit backend routes","allowed_targets":["route-a"]}));
    let run=created["id"].as_str().unwrap();
    d.call("swarm.plan",json!({"id":run,"generation":1,"revision":0,"jobs":[
        {"id":"j","title":"Inspect route","acceptance":"evidence","deps":[]}]}));
    let at=now();
    for n in 0..3 {
        let when=at+n*1000;
        let observed=d.call("swarm.availability.observe",json!({"run_id":run,
            "snapshot":snapshot(when,false,100000),"now_ms":when,
            "required_capabilities":["code"],"estimate_milli":{"points":100},
            "purpose":"worker"}));
        assert_eq!(observed["state"],"blocked","{observed}");
        assert_eq!(observed["woken"],false);
        let claim=d.call("swarm.director.claim_batch",json!({"run_id":run,
            "generation":1,"revision":1,"now_ms":when+6000}));
        assert_eq!(claim["status"],"blocked","{claim}");
    }
    d.kill9();d.spawn();
    let after_restart=d.call("swarm.director.claim_batch",json!({"run_id":run,
        "generation":1,"revision":1,"now_ms":at+9000}));
    assert_eq!(after_restart["status"],"blocked","{after_restart}");
    let db=rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let turns:i64=db.query_row("SELECT COUNT(*) FROM swarm_director_turns WHERE run_id=?1",
        [run],|row|row.get(0)).unwrap();
    let wakes:i64=db.query_row("SELECT COUNT(*) FROM swarm_messages WHERE run_id=?1
        AND kind='availability'",[run],|row|row.get(0)).unwrap();
    assert_eq!((turns,wakes),(0,0),"polling a blocked route must not spend model turns");
    drop(db);
    let recovered=d.call("swarm.availability.observe",json!({"run_id":run,
        "snapshot":snapshot(at+4000,true,100000),"now_ms":at+4000,
        "required_capabilities":["code"],"estimate_milli":{"points":100},
        "purpose":"worker"}));
    assert_eq!(recovered["woken"],true,"{recovered}");
    let claimed=d.call("swarm.director.claim_batch",json!({"run_id":run,
        "generation":1,"revision":1,"now_ms":at+10000}));
    assert_eq!(claimed["status"],"claimed","{claimed}");
    assert_eq!(claimed["messages"].as_array().unwrap().len(),1);
    assert_eq!(claimed["messages"][0]["type"],"availability");
}

#[test]
fn confirmed_target_selection_requires_fresh_observation_and_wakes_once() {
    let mut d=Daemon::start(&[]);
    let made=d.call("swarm.create",json!({"category":"Target selection recovery",
        "objective":"Audit backend routes","allowed_targets":[]}));
    let run=made["id"].as_str().unwrap();
    let at=now();
    let observe=|d:&Daemon,when:i64|d.call("swarm.availability.observe",json!({
        "run_id":run,"snapshot":snapshot(when,true,100000),"now_ms":when,
        "required_capabilities":["code"],"estimate_milli":{"points":100},
        "purpose":"worker"}));
    assert_eq!(observe(&d,at)["reason"],"no_allowed_target");
    let change=json!({"run_id":run,"request_id":"choose-route-a",
        "expected_control_revision":0,"owner_confirmed":true,
        "allowed_targets":["route-a"]});
    let mut unconfirmed=change.clone();
    unconfirmed["owner_confirmed"]=json!(false);
    assert!(d.try_call("swarm.targets.set",unconfirmed).is_err());
    let changed=d.call("swarm.targets.set",change.clone());
    assert_eq!(changed["control_revision"],1);
    assert_eq!(changed["allowed_targets"],json!(["route-a"]));
    assert_eq!(changed["duplicate"],false);
    let current=d.call("swarm.get",json!({"id":run}));
    assert_eq!(current["availability"]["reason"],"target_selection_changed");
    assert_eq!(current["policy"]["effective"]["deadline_ms"],3600000);
    let held=d.call("swarm.admit",json!({"run_id":run,"generation":1,"revision":0,
        "job_id":"not-planned","target_id":"route-a","request_id":"before-refresh",
        "snapshot":snapshot(at,true,100000),"now_ms":at,
        "required_capabilities":["code"],"estimate_milli":{"points":100},
        "purpose":"worker"}));
    assert_eq!(held["reason"],"run_availability_blocked");
    let fresh=observe(&d,now()+2000);
    assert_eq!(fresh["state"],"eligible");
    assert_eq!(fresh["woken"],true);
    assert_eq!(fresh["wake_count"],1);
    let refreshed=observe(&d,now()+4000);
    assert_eq!(refreshed["state"],"eligible");
    assert_eq!(refreshed["woken"],false);
    assert_eq!(refreshed["wake_count"],1);
    let unchanged=d.call("swarm.targets.set",json!({"run_id":run,
        "request_id":"keep-route-a","expected_control_revision":1,
        "owner_confirmed":true,"allowed_targets":["route-a"]}));
    assert_eq!(unchanged["changed"],false);
    assert_eq!(unchanged["control_revision"],1);
    assert_eq!(d.call("swarm.get",json!({"id":run}))["availability"]["state"],"eligible");
    d.kill9(); d.spawn();
    assert_eq!(d.call("swarm.targets.set",change.clone())["duplicate"],true);
    assert_eq!(d.call("swarm.get",json!({"id":run}))["availability"]["wake_count"],1);
    let mut reused=change.clone(); reused["allowed_targets"]=json!([]);
    assert!(d.try_call("swarm.targets.set",reused).unwrap_err().contains("reused"));
    let mut stale=change; stale["request_id"]=json!("stale-selection");
    assert!(d.try_call("swarm.targets.set",stale).unwrap_err().contains("stale"));
}

#[test]
fn changing_selected_targets_preserves_admitted_evidence_and_restricts_future_work() {
    let d=Daemon::start(&[]);
    let made=d.call("swarm.create",json!({"category":"Target change with active work",
        "objective":"Audit two backend routes","allowed_targets":["route-a","route-b"]}));
    let run=made["id"].as_str().unwrap();
    d.call("swarm.plan",json!({"id":run,"generation":1,"revision":0,"jobs":[
        {"id":"first","title":"Inspect first route","acceptance":"evidence","deps":[]},
        {"id":"second","title":"Inspect second route","acceptance":"evidence","deps":[]}
    ]}));
    commit_beneficial_batch(&d,run,&["first".into(),"second".into()]);
    let at=now();
    let snap=|when:i64,b_remaining:i64|json!({"version":1,"observed_ms":when-1000,
        "expires_ms":when+60000,"targets":[
            {"id":"route-a","account_id":"account-a","pool_ids":["pool-a"],
                "capabilities":["code"],"health":"up","auth":"ok"},
            {"id":"route-b","account_id":"account-b","pool_ids":["pool-b"],
                "capabilities":["code"],"health":"up","auth":"ok"}],
        "pools":[
            {"id":"pool-a","windows":[{"id":"week","unit":"points",
                "remaining_milli":100000,"protected_milli":0,"reserved_milli":0,
                "confidence":"exact","expires_ms":when+60000}]},
            {"id":"pool-b","windows":[{"id":"week","unit":"points",
                "remaining_milli":b_remaining,"protected_milli":0,"reserved_milli":0,
                "confidence":"exact","expires_ms":when+60000}]}]});
    let observe=|when:i64,b_remaining:i64|d.call("swarm.availability.observe",json!({"run_id":run,
        "snapshot":snap(when,b_remaining),"now_ms":when,"required_capabilities":["code"],
        "estimate_milli":{"points":100},"purpose":"worker"}));
    assert_eq!(observe(at,100000)["state"],"eligible");
    let admit=|job:&str,target:&str,request:&str,when:i64,b_remaining:i64,estimate:i64|d.call("swarm.admit",json!({
        "run_id":run,"generation":1,"revision":1,"job_id":job,"target_id":target,
        "request_id":request,"snapshot":snap(when,b_remaining),"now_ms":when,
        "required_capabilities":["code"],"estimate_milli":{"points":estimate},
        "purpose":"worker"}));
    let first=admit("first","route-a","before-selection",at,100000,100);
    assert_eq!(first["status"],"admitted","{first}");
    d.call("swarm.artifact.put",json!({"run_id":run,"job_id":"first",
        "attempt_id":first["attempt_id"],"token":first["token"],
        "artifact_id":"first-evidence","source_revision":1,"kind":"finding",
        "content":"Observed route behavior"}));
    let action=d.call("swarm.targets.set",json!({"run_id":run,
        "request_id":"switch-to-b","expected_control_revision":0,
        "owner_confirmed":true,"allowed_targets":["route-b"]}));
    assert_eq!(action["control_revision"],1);
    let pending=admit("second","route-b","before-reobserve",at+1000,100000,100);
    assert_eq!(pending["reason"],"run_availability_blocked");
    let fresh_at=now()+6000;
    let available=observe(fresh_at,1000000);
    assert_eq!(available["eligible_targets"],json!(["route-b"]));
    assert_eq!(available["woken"],true);
    let old=admit("second","route-a","old-target",fresh_at,1000000,100);
    assert_eq!(old["reason"],"not_allowed","{old}");
    let over_frozen=admit("second","route-b","over-frozen-budget",fresh_at,1000000,9000);
    assert_eq!(over_frozen["reason"],"finishing_reserve","{over_frozen}");
    let second=admit("second","route-b","new-target",fresh_at,1000000,100);
    assert_eq!(second["status"],"admitted","{second}");
    let current=d.call("swarm.get",json!({"id":run}));
    assert_eq!(current["allowed_targets"],json!(["route-b"]));
    assert_eq!(current["registered_attempts"],2);
    assert_eq!(current["capacity"]["windows"].as_array().unwrap().len(),2);
    let db=rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let retained:i64=db.query_row("SELECT COUNT(*) FROM swarm_artifacts WHERE run_id=?1
        AND id='first-evidence'",[run],|row|row.get(0)).unwrap();
    assert_eq!(retained,1);
}

#[test]
fn director_can_close_a_fresh_target_block_without_claiming_success() {
    let mut d=Daemon::start(&[]);
    let made=d.call("swarm.create",json!({"category":"Unavailable route closeout",
        "objective":"Audit backend routes","allowed_targets":["route-a"]}));
    let run=made["id"].as_str().unwrap();
    d.call("swarm.plan",json!({"id":run,"generation":1,"revision":0,"jobs":[
        {"id":"audit","title":"Inspect route","acceptance":"route evidence","deps":[]}
    ]}));
    let request=json!({"run_id":run,"generation":1,"revision":1,
        "expected_revision":1,"expected_control_revision":0,
        "request_id":"close-missing-route","incomplete_reason":"allowed_target_missing",
        "summary":"The approved route is unavailable; the audit is incomplete",
        "limitations":"The route could not be probed"});
    assert!(d.try_call("swarm.partial",request.clone()).is_err());
    let at=now();
    let observed=d.call("swarm.availability.observe",json!({"run_id":run,
        "snapshot":snapshot(at,false,100000),"now_ms":at,
        "required_capabilities":["code"],"estimate_milli":{"points":100},
        "purpose":"worker"}));
    assert_eq!(observed["reason"],"allowed_target_missing");
    let mut false_reason=request.clone();
    false_reason["request_id"]=json!("close-wrong-reason");
    false_reason["incomplete_reason"]=json!("finishing_reserve");
    assert!(d.try_call("swarm.partial",false_reason).is_err());
    let db=rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    db.execute("UPDATE swarm_availability SET expires_ms=1 WHERE run_id=?1",[run]).unwrap();
    assert!(d.try_call("swarm.partial",request.clone()).unwrap_err()
        .contains("recorded evidence"));
    db.execute("UPDATE swarm_availability SET expires_ms=?2 WHERE run_id=?1",
        rusqlite::params![run,at+60000]).unwrap();
    let fingerprint:String=db.query_row(
        "SELECT snapshot_sha256 FROM swarm_availability WHERE run_id=?1",
        [run],|r|r.get(0)).unwrap();
    db.execute("UPDATE swarm_availability SET snapshot_sha256='' WHERE run_id=?1",
        [run]).unwrap();
    assert!(d.try_call("swarm.partial",request.clone()).unwrap_err()
        .contains("recorded evidence"));
    db.execute("UPDATE swarm_availability SET snapshot_sha256=?2 WHERE run_id=?1",
        rusqlite::params![run,fingerprint]).unwrap();
    let closed=d.call("swarm.partial",request.clone());
    assert_eq!(closed["status"],"stopped");
    assert_eq!(closed["stop_reason"],"allowed_target_missing");
    let coverage=d.call("swarm.coverage",json!({"run_id":run}));
    assert_eq!(coverage["outcome"],"incomplete");
    assert_eq!(coverage["partial_report"]["reason"],"allowed_target_missing");
    assert_eq!(coverage["partial_report"]["finalized"],true);
    assert!(d.try_call("swarm.complete",json!({"run_id":run,"generation":1,
        "revision":1,"request_id":"false-completion","summary":"done",
        "verification":"none","checks":[]})).is_err());
    d.kill9();
    d.spawn();
    assert_eq!(d.call("swarm.partial",request)["duplicate"],true);
    assert_eq!(d.call("swarm.coverage",json!({"run_id":run}))["outcome"],"incomplete");
}

#[test]
fn availability_readout_preserves_the_observed_allowance_drop() {
    let mut d=Daemon::start(&[]);
    let made=d.call("swarm.create",json!({"category":"Allowance change readout",
        "objective":"Audit backend","allowed_targets":["route-a"]}));
    let run=made["id"].as_str().unwrap();
    let at=now();
    let observe=|d:&Daemon,when:i64,remaining:i64| {
        let mut snap=snapshot(when,true,remaining);
        snap["pools"].as_array_mut().unwrap().push(json!({"id":"unrelated-pool",
            "windows":[{"id":"week","unit":"points","remaining_milli":999999,
                "protected_milli":0,"reserved_milli":0,"confidence":"exact",
                "expires_ms":when+60000}]}));
        d.call("swarm.availability.observe",json!({"run_id":run,
            "snapshot":snap,"now_ms":when,"required_capabilities":["code"],
            "estimate_milli":{"points":100},"purpose":"worker"}))
    };
    assert_eq!(observe(&d,at,100000)["state"],"eligible");
    let dropped=observe(&d,at+1000,500);
    assert_eq!(dropped["reason"],"finishing_reserve");
    let read=d.call("swarm.get",json!({"id":run}));
    let windows=read["availability"]["allowance_windows"].as_array().unwrap();
    assert_eq!(windows.len(),1);
    let week=&windows[0];
    assert_eq!(week["pool_id"],"pool-a");
    assert_eq!(week["unit"],"points");
    assert_eq!(week["remaining_milli"],500);
    assert_eq!(week["previous_remaining_milli"],100000);
    assert_eq!(week["change_milli"],-99500);
    assert_eq!(week["confidence"],"exact");
    d.kill9();
    d.spawn();
    assert_eq!(d.call("swarm.get",json!({"id":run}))["availability"]["allowance_windows"],
        read["availability"]["allowance_windows"]);
    assert_eq!(observe(&d,at+1000,500)["changed"],false);
    assert_eq!(d.call("swarm.get",json!({"id":run}))["availability"]["allowance_windows"],
        read["availability"]["allowance_windows"]);
    let mut unknown=snapshot(at+2000,true,0);
    unknown["pools"][0]["windows"][0]["remaining_milli"]=Value::Null;
    unknown["pools"][0]["windows"][0]["confidence"]=json!("unknown");
    let no_balance=d.call("swarm.availability.observe",json!({"run_id":run,
        "snapshot":unknown,"now_ms":at+2000,"required_capabilities":["code"],
        "estimate_milli":{"points":100},"purpose":"worker"}));
    assert_eq!(no_balance["reason"],"unknown_quota");
    let unknown_read=d.call("swarm.get",json!({"id":run}));
    let current=&unknown_read["availability"]["allowance_windows"][0];
    assert_eq!(current["remaining_milli"],Value::Null);
    assert_eq!(current["previous_remaining_milli"],500);
    assert_eq!(current["change_milli"],Value::Null);
    assert_eq!(current["confidence"],"unknown");
    let mut many=snapshot(at+3000,true,100000);
    many["pools"][0]["windows"]=json!((0..101).map(|i|json!({
        "id":format!("window-{i:03}"),"unit":"points",
        "remaining_milli":100000,"protected_milli":0,"reserved_milli":0,
        "confidence":"exact","expires_ms":at+63000
    })).collect::<Vec<_>>());
    assert_eq!(d.call("swarm.availability.observe",json!({"run_id":run,
        "snapshot":many,"now_ms":at+3000,"required_capabilities":["code"],
        "estimate_milli":{"points":100},"purpose":"worker"}))["state"],"eligible");
    let bounded=d.call("swarm.get",json!({"id":run}));
    assert_eq!(bounded["availability"]["allowance_window_count"],101);
    assert_eq!(bounded["availability"]["allowance_windows_truncated"],true);
    assert_eq!(bounded["availability"]["allowance_windows"].as_array().unwrap().len(),100);
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
        "objective":"Audit separate routes","allowed_targets":["opencode-a","opencode-a-alt","opencode-b"]}));
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
                "endpoint_id":"endpoint-a","health_scope":"endpoint",
                "capabilities":["code"],"health":if limited {"rate_limited"} else {"up"},"auth":"ok"},
            {"id":"opencode-a-alt","account_id":"account-a","pool_ids":["pool-a"],
                "endpoint_id":"endpoint-a",
                "capabilities":["code"],"health":"up","auth":"ok"},
            {"id":"opencode-b","account_id":"account-b","pool_ids":["pool-b"],
                "endpoint_id":"endpoint-b",
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
    let denied_alias=admit("c","opencode-a-alt","limited-a-alias",limited.clone(),at+1500);
    assert_eq!(denied_alias["reason"],"rate_limited");
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
