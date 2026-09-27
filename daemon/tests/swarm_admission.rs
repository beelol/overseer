mod common;

use common::*;
use serde_json::{json, Value};
use std::time::{Duration, Instant};
use std::time::{SystemTime, UNIX_EPOCH};

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

fn snapshot(at: i64, remaining: i64) -> Value {
    json!({"version":1,"observed_ms":at-1000,"expires_ms":at+60000,
        "targets":[
            {"id":"codex-a","account_id":"account-a","pool_ids":["shared"],"capabilities":["code"],"health":"up","auth":"ok"},
            {"id":"opencode-a","account_id":"account-a","pool_ids":["shared"],"capabilities":["code"],"health":"up","auth":"ok"}
        ],"pools":[{"id":"shared","windows":[{"id":"week","unit":"points","remaining_milli":remaining,
            "protected_milli":0,"reserved_milli":0,"confidence":"exact","expires_ms":at+60000}]}]})
}

fn setup(d: &Daemon, category: &str, count: usize) -> String {
    let run = d.call(
        "swarm.create",
        json!({"category":category,"objective":"Audit","allowed_targets":["codex-a","opencode-a"]}),
    );
    let id = run["id"].as_str().unwrap().to_string();
    let jobs: Vec<_>=(0..count).map(|n|json!({"id":format!("j{n}"),"title":format!("J{n}"),"acceptance":"evidence","deps":[]})).collect();
    d.call(
        "swarm.plan",
        json!({"id":id,"generation":1,"revision":0,"jobs":jobs}),
    );
    id
}

#[test]
fn last_admission_hold_survives_restart_and_success_replaces_it() {
    let mut d=Daemon::start(&[]);
    let id=setup(&d,"Last admission readout",2);
    let at=now();
    let held=admit(&d,&id,"j0","missing-target","held-request",at,100000,1000).unwrap();
    assert_eq!(held["reason"],"unknown_target");
    let first=&d.call("swarm.get",json!({"id":id}))["capacity"]["last_admission"];
    assert_eq!((first["job_id"].as_str(),first["target_id"].as_str(),
        first["status"].as_str(),first["reason"].as_str()),
        (Some("j0"),Some("missing-target"),Some("blocked"),Some("unknown_target")));
    assert!(first["observed_ms"].as_i64().is_some());
    d.kill9();
    d.spawn();
    assert_eq!(d.call("swarm.get",json!({"id":id}))["capacity"]["last_admission"]["reason"],
        "unknown_target");
    let admitted=admit(&d,&id,"j0","codex-a","success-request",at,100000,1000).unwrap();
    assert_eq!(admitted["status"],"admitted","{admitted}");
    let latest=d.call("swarm.get",json!({"id":id}))["capacity"]["last_admission"].clone();
    assert_eq!((latest["job_id"].as_str(),latest["target_id"].as_str(),
        latest["status"].as_str(),latest["reason"].as_str()),
        (Some("j0"),Some("codex-a"),Some("admitted"),None));
    let replay=admit(&d,&id,"j0","codex-a","success-request",at,100000,1000).unwrap();
    assert_eq!(replay["status"],"already_admitted");
    assert_eq!(d.call("swarm.get",json!({"id":id}))["capacity"]["last_admission"],latest);
}

fn admit(
    d: &Daemon,
    run: &str,
    job: &str,
    target: &str,
    request: &str,
    at: i64,
    remaining: i64,
    estimate: i64,
) -> Result<Value, String> {
    d.try_call(
        "swarm.admit",
        json!({"run_id":run,"generation":1,"revision":1,"job_id":job,
        "target_id":target,"request_id":request,"snapshot":snapshot(at,remaining),"now_ms":at,
        "required_capabilities":["code"],"estimate_milli":{"points":estimate},"purpose":"worker"}),
    )
}

#[test]
fn audit_only_run_holds_native_worker_without_source_write_enforcement() {
    let d = Daemon::start(&[]);
    let at = now();
    let snapshot = json!({"version":1,"observed_ms":at-1000,"expires_ms":at+60000,
        "targets":[{"id":"claude-a","harness":"claude","profile_id":"approved-a",
            "model":"test-model","account_id":"account-a","pool_ids":["pool-a"],
            "capabilities":["code"],"health":"up","auth":"ok"}],
        "pools":[{"id":"pool-a","windows":[{"id":"week","unit":"points",
            "remaining_milli":100000,"protected_milli":0,"reserved_milli":0,
            "confidence":"exact","expires_ms":at+60000}]}]});
    let create = |category: &str, permission: &str| d.call("swarm.create", json!({
        "category":category,"objective":"Audit a local backend",
        "allowed_targets":["claude-a"],"source_change_permission":permission}));
    let audit = create("Audit scope", "none");
    let write = create("Granted source change", "isolated");
    for run in [&audit, &write] {
        d.call("swarm.plan", json!({"id":run["id"],"generation":1,"revision":0,
            "jobs":[{"id":"j1","title":"Inspect backend","acceptance":"Evidence",
                "deps":[]}]}));
    }
    let request = |run: &Value, request_id: &str| json!({
        "run_id":run["id"],"generation":1,"revision":1,"job_id":"j1",
        "target_id":"claude-a","request_id":request_id,"snapshot":snapshot,
        "now_ms":at,"required_capabilities":["code"],
        "estimate_milli":{"points":1000},"purpose":"worker"});
    let denied = d.call("swarm.admit", request(&audit,"audit-native"));
    assert_eq!(denied["status"], "blocked", "{denied}");
    assert_eq!(denied["reason"], "audit_source_boundary_unqualified");
    assert_eq!(d.call("swarm.jobs",json!({"id":audit["id"]}))["jobs"][0]["status"], "ready");
    let granted = d.call("swarm.admit", request(&write,"write-native"));
    assert_eq!(granted["status"], "admitted", "{granted}");
}

#[test]
fn dependency_chain_explains_serial_work_while_independent_jobs_admit() {
    let d = Daemon::start(&[]);
    let run = d.call("swarm.create", json!({"category":"Dependency and resource replay",
        "objective":"Audit the backend","allowed_targets":["codex-a"]}));
    let id = run["id"].as_str().unwrap();
    d.call("swarm.plan",json!({"id":id,"generation":1,"revision":0,"jobs":[
        {"id":"root","title":"Check contract","acceptance":"contract evidence","deps":[]},
        {"id":"child","title":"Check consumer","acceptance":"consumer evidence","deps":["root"]},
        {"id":"leaf","title":"Check integration","acceptance":"integration evidence","deps":["child"]},
        {"id":"ind-a","title":"Check API","acceptance":"API evidence","deps":[]},
        {"id":"ind-b","title":"Check queue","acceptance":"queue evidence","deps":[]},
        {"id":"ind-c","title":"Check storage","acceptance":"storage evidence","deps":[]},
        {"id":"writer-a","title":"Use fixture DB","acceptance":"DB evidence","deps":[],
            "resource_claims":[{"resource":"db:shared","mode":"write"}]},
        {"id":"writer-b","title":"Use same fixture DB","acceptance":"DB evidence","deps":[],
            "resource_claims":[{"resource":"db:shared","mode":"write"}]}
    ]}));
    commit_beneficial_batch(&d,id,&["root".into(),"ind-a".into(),"ind-b".into(),
        "ind-c".into(),"writer-a".into()]);
    let at = now();
    let child = admit(&d,id,"child","codex-a","early-child",at,100000,1000).unwrap();
    assert_eq!(child["reason"],"dependency_pending","{child}");
    assert_eq!(child["waiting_on"],json!(["root"]));
    let leaf = admit(&d,id,"leaf","codex-a","early-leaf",at,100000,1000).unwrap();
    assert_eq!(leaf["waiting_on"],json!(["child"]));
    for job in ["root","ind-a","ind-b","ind-c"] {
        let result = admit(&d,id,job,"codex-a",job,at,100000,1000).unwrap();
        assert_eq!(result["status"],"admitted","{job}: {result}");
    }
    let writer = admit(&d,id,"writer-a","codex-a","writer-a",at+5000,100000,1000).unwrap();
    assert_eq!(writer["status"],"admitted","{writer}");
    let conflict = admit(&d,id,"writer-b","codex-a","writer-b",at+5000,100000,1000).unwrap();
    assert_eq!(conflict["reason"],"resource_conflict","{conflict}");
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let attempts: i64 = db.query_row("SELECT COUNT(*) FROM swarm_attempts WHERE run_id=?1",
        [id],|r|r.get(0)).unwrap();
    assert_eq!(attempts,5,"blocked dependencies and writer must not consume attempts");
    let jobs = d.call("swarm.jobs",json!({"id":id}))["jobs"].as_array().unwrap().clone();
    for job in ["child","leaf"] {
        assert_eq!(jobs.iter().find(|item| item["id"]==job).unwrap()["status"],"planned");
    }

    let serial = Daemon::start(&[]);
    let chain = serial.call("swarm.create",json!({"category":"Serial chain replay",
        "objective":"Check contract then consumer then integration",
        "allowed_targets":["codex-a"]}));
    let chain_id = chain["id"].as_str().unwrap();
    serial.call("swarm.plan",json!({"id":chain_id,"generation":1,"revision":0,
        "jobs":[
            {"id":"root","title":"Contract","acceptance":"contract evidence","deps":[]},
            {"id":"child","title":"Consumer","acceptance":"consumer evidence","deps":["root"]},
            {"id":"leaf","title":"Integration","acceptance":"integration evidence","deps":["child"]}
        ]}));
    let finish = |job: &str, attempt: &Value| {
        let artifact = format!("{job}-evidence");
        serial.call("swarm.artifact.put",json!({"run_id":chain_id,"job_id":job,
            "attempt_id":attempt["attempt_id"],"token":attempt["token"],
            "artifact_id":artifact,"source_revision":1,"kind":"finding",
            "content":format!("checked {job}")}));
        serial.call("swarm.report",json!({"run_id":chain_id,"job_id":job,
            "attempt_id":attempt["attempt_id"],"token":attempt["token"],
            "message_id":format!("{job}-result"),"type":"result","revision":1,
            "payload":{"artifact_ids":[artifact]}}));
        serial.call("swarm.decide",json!({"run_id":chain_id,"generation":1,
            "revision":1,"job_id":job,"decision":"accept","evidence":[artifact]}));
        if let Some(next) = match job { "root" => Some("child"),
            "child" => Some("leaf"), _ => None } {
            let held = admit(&serial,chain_id,next,"codex-a",
                &format!("{next}-before-parent-exit"),at,100000,1000).unwrap();
            assert_eq!(held["reason"],"dependency_pending","{held}");
            assert_eq!(held["waiting_on"],json!([job]));
        }
        serial.call("swarm.attempt.confirm_exit",json!({"run_id":chain_id,
            "generation":1,"revision":1,"job_id":job,
            "attempt_id":attempt["attempt_id"]}));
    };
    for (index,job) in ["root","child","leaf"].into_iter().enumerate() {
        let attempt = admit(&serial,chain_id,job,"codex-a",job,at+index as i64*5000,
            100000,1000).unwrap();
        assert_eq!(attempt["status"],"admitted","{job}: {attempt}");
        finish(job,&attempt);
        let next = ["child","leaf"].get(index);
        if let Some(next) = next {
            let jobs = serial.call("swarm.jobs",json!({"id":chain_id}))["jobs"]
                .as_array().unwrap().clone();
            assert_eq!(jobs.iter().find(|item| item["id"]==*next).unwrap()["status"],"ready");
        }
    }
}

#[test]
fn one_run_freezes_allocation_and_dedupes_replayed_admission() {
    let d = Daemon::start(&[]);
    let id = setup(&d, "Backend admission", 3);
    let at = now();
    let first = admit(&d, &id, "j0", "codex-a", "req-0", at, 60000, 4000).unwrap();
    assert_eq!(first["status"], "admitted");
    assert_eq!(d.call("swarm.jobs",json!({"id":id}))["jobs"][0]["deadline_at_ms"],at+900_000);
    assert_eq!(first["allocation_milli"], 6000);
    let replay = admit(&d, &id, "j0", "codex-a", "req-0", at, 60000, 4000).unwrap();
    assert_eq!(replay["status"], "already_admitted");
    assert_eq!(replay["attempt_id"], first["attempt_id"]);
    let denied = admit(&d, &id, "j1", "opencode-a", "req-1", at, 100000, 1000).unwrap();
    assert_eq!(denied["status"], "blocked");
    assert_eq!(denied["reason"], "finishing_reserve");
}

#[test]
fn quota_window_reset_does_not_grant_a_second_run_allocation() {
    let d = Daemon::start(&[]);
    let id = setup(&d, "Quota reset", 2);
    let at = now();
    let mut before_reset = snapshot(at, 1000);
    before_reset["pools"][0]["windows"][0]["id"] = json!("week-old");
    let request = |job: &str, request_id: &str, snapshot: Value, estimate: i64| {
        json!({"run_id":id,"generation":1,"revision":1,"job_id":job,
            "target_id":"codex-a","request_id":request_id,"snapshot":snapshot,
            "now_ms":at,"required_capabilities":["code"],
            "estimate_milli":{"points":estimate},"purpose":"worker"})
    };
    let first = d.call("swarm.admit", request("j0", "before-reset", before_reset, 60));
    assert_eq!(first["status"], "admitted", "{first}");
    assert_eq!(first["allocation_milli"], 100);
    d.call("swarm.artifact.put", json!({"run_id":id,"job_id":"j0",
        "attempt_id":first["attempt_id"],"token":first["token"],
        "artifact_id":"first-evidence","source_revision":1,
        "kind":"finding","content":"checked"}));
    d.call("swarm.report", json!({"run_id":id,"job_id":"j0",
        "attempt_id":first["attempt_id"],"token":first["token"],
        "message_id":"first-result","type":"result","revision":1,
        "payload":{"artifact_ids":["first-evidence"]}}));
    d.call("swarm.decide", json!({"run_id":id,"generation":1,"revision":1,
        "job_id":"j0","decision":"accept","evidence":["first-evidence"]}));
    d.call("swarm.attempt.confirm_exit", json!({"run_id":id,"generation":1,
        "revision":1,"job_id":"j0","attempt_id":first["attempt_id"]}));
    let mut after_reset = snapshot(at, 2000);
    after_reset["pools"][0]["windows"][0]["id"] = json!("week-new");
    let held = d.call("swarm.admit", request("j1", "after-reset-large", after_reset.clone(), 70));
    assert_eq!(held["status"], "blocked", "{held}");
    assert_eq!(held["reason"], "finishing_reserve");
    let within_cap = d.call("swarm.admit", request("j1", "after-reset-small", after_reset, 20));
    assert_eq!(within_cap["status"], "admitted", "{within_cap}");
    assert_eq!(within_cap["allocation_milli"], 100);
}

#[test]
fn short_window_and_unlike_unit_each_bind_admission_without_conversion() {
    let d=Daemon::start(&[]);
    let run=setup(&d,"Binding windows",3);
    commit_beneficial_batch(&d,&run,&["j0".into(),"j1".into(),"j2".into()]);
    let at=now();
    let mut snap=snapshot(at,100000);
    let windows=snap["pools"][0]["windows"].as_array_mut().unwrap();
    windows.push(json!({"id":"day","unit":"points","remaining_milli":5000,
        "protected_milli":0,"reserved_milli":0,"confidence":"exact","expires_ms":at+60000}));
    windows.push(json!({"id":"requests","unit":"requests","remaining_milli":20000,
        "protected_milli":0,"reserved_milli":0,"confidence":"exact","expires_ms":at+60000}));
    let admit=|job:&str,key:&str,points:i64,requests:Option<i64>| {
        let mut estimate=json!({"points":points});
        if let Some(requests)=requests { estimate["requests"]=json!(requests); }
        d.call("swarm.admit",json!({"run_id":run,"generation":1,"revision":1,
            "job_id":job,"target_id":"codex-a","request_id":key,
            "snapshot":snap,"now_ms":at,"required_capabilities":["code"],
            "estimate_milli":estimate,"purpose":"worker"}))
    };
    let first=admit("j0","all-windows-first",300,Some(100));
    assert_eq!(first["status"],"admitted","{first}");
    assert_eq!(first["reservation_windows"],3);
    let day_limited=admit("j1","day-binds",150,Some(100));
    assert_eq!(day_limited["reason"],"finishing_reserve","{day_limited}");
    let no_conversion=admit("j1","missing-native-unit",50,None);
    assert_eq!(no_conversion["reason"],"missing_estimate","{no_conversion}");
    let requests_limited=admit("j1","requests-bind",50,Some(1700));
    assert_eq!(requests_limited["reason"],"finishing_reserve","{requests_limited}");
    let second=admit("j1","both-units-fit",50,Some(100));
    assert_eq!(second["status"],"admitted","{second}");
    assert_eq!(second["reservation_windows"],3);
    let db=rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let mut stmt=db.prepare("SELECT window_id,unit,allocation_milli FROM swarm_allocations
        WHERE run_id=?1 ORDER BY window_id").unwrap();
    let saved=stmt.query_map([&run],|row|Ok((row.get::<_,String>(0)?,
        row.get::<_,String>(1)?,row.get::<_,i64>(2)?))).unwrap()
        .collect::<rusqlite::Result<Vec<_>>>().unwrap();
    assert_eq!(saved,vec![("day".into(),"points".into(),500),
        ("requests".into(),"requests".into(),2000),
        ("week".into(),"points".into(),10000)]);
}

#[test]
fn estimated_quota_requires_run_permission_and_revocation_blocks_future_jobs() {
    let mut d=Daemon::start(&[]);
    let at=now();
    let mut estimated=snapshot(at,100000);
    estimated["pools"][0]["windows"][0]["confidence"]=json!("estimated");
    let create=|d:&Daemon, category:&str, policy:Value| {
        let run=d.call("swarm.create",json!({"category":category,"objective":"Audit",
            "allowed_targets":["codex-a"],"policy":policy}));
        let id=run["id"].as_str().unwrap().to_string();
        d.call("swarm.plan",json!({"id":id,"generation":1,"revision":0,"jobs":[
            {"id":"j0","title":"Inspect A","acceptance":"evidence","deps":[]},
            {"id":"j1","title":"Inspect B","acceptance":"evidence","deps":[]}]}));
        id
    };
    let request=|run:&str, job:&str, key:&str, snap:Value| json!({
        "run_id":run,"generation":1,"revision":1,"job_id":job,"target_id":"codex-a",
        "request_id":key,"snapshot":snap,"now_ms":at,"required_capabilities":["code"],
        "estimate_milli":{"points":100},"purpose":"worker"});

    let default=create(&d,"Estimated quota denied",json!({}));
    let missing={let mut s=estimated.clone();s["pools"][0]["windows"][0]["remaining_milli"]=Value::Null;s};
    let unknown=d.call("swarm.admit",request(&default,"j0","unknown",missing));
    assert_eq!(unknown["reason"],"unknown_quota","{unknown}");
    let stale={let mut s=estimated.clone();s["expires_ms"]=json!(at-1);s};
    let expired=d.call("swarm.admit",request(&default,"j0","stale",stale));
    assert_eq!(expired["reason"],"stale_snapshot","{expired}");
    let inferred=d.call("swarm.admit",request(&default,"j0","inferred",estimated.clone()));
    assert_eq!(inferred["reason"],"estimated_quota_requires_permission","{inferred}");
    let unknown_readout=d.call("swarm.get",json!({"id":default}));
    assert_eq!(unknown_readout["capacity"]["provider_usage_state"],"unknown");
    assert!(unknown_readout["capacity"]["windows"].as_array().unwrap().is_empty());

    let permitted=create(&d,"Estimated quota allowed",json!({"allow_estimated_quota":true}));
    commit_beneficial_batch(&d,&permitted,&["j0".into(),"j1".into()]);
    let observed=d.call("swarm.availability.observe",json!({"run_id":permitted,
        "snapshot":estimated,"now_ms":at,"required_capabilities":["code"],
        "estimate_milli":{"points":100},"purpose":"worker"}));
    assert_eq!(observed["state"],"eligible","{observed}");
    assert_eq!(observed["allowance_windows"][0]["confidence"],"estimated");
    let first=d.call("swarm.admit",request(&permitted,"j0","estimated-first",estimated.clone()));
    assert_eq!(first["status"],"admitted","{first}");
    assert_eq!(first["allocation_milli"],10000);
    let revoked=d.call("swarm.estimate.revoke",json!({"run_id":permitted,
        "request_id":"revoke-estimate","expected_control_revision":0}));
    assert_eq!(revoked["changed"],true,"{revoked}");
    d.kill9();d.spawn();
    assert_eq!(d.call("swarm.estimate.revoke",json!({"run_id":permitted,
        "request_id":"revoke-estimate","expected_control_revision":0}))["duplicate"],true);
    let after=d.call("swarm.get",json!({"id":permitted}));
    assert_eq!(after["policy"]["effective"]["allow_estimated_quota"],false);
    assert_eq!(after["availability"]["reason"],"estimated_permission_revoked");
    let held=d.call("swarm.admit",request(&permitted,"j1","after-revoke",estimated));
    assert_eq!(held["status"],"blocked","{held}");
    assert_eq!(held["reason"],"run_availability_blocked","{held}");
    let mut still_estimated=snapshot(at+2000,100000);
    still_estimated["version"]=json!(2);
    still_estimated["pools"][0]["windows"][0]["confidence"]=json!("estimated");
    let reassessed=d.call("swarm.availability.observe",json!({"run_id":permitted,
        "snapshot":still_estimated,"now_ms":at+2000,
        "required_capabilities":["code"],"estimate_milli":{"points":100},
        "purpose":"worker"}));
    assert_eq!(reassessed["reason"],"estimated_quota_requires_permission","{reassessed}");
    let mut exact=snapshot(at+4000,100000);
    exact["version"]=json!(3);
    let recovered=d.call("swarm.availability.observe",json!({"run_id":permitted,
        "snapshot":exact,"now_ms":at+4000,
        "required_capabilities":["code"],"estimate_milli":{"points":100},
        "purpose":"worker"}));
    assert_eq!(recovered["state"],"eligible","{recovered}");
    let next=d.call("swarm.admit",json!({"run_id":permitted,"generation":1,
        "revision":1,"job_id":"j1","target_id":"codex-a",
        "request_id":"exact-after-revoke","snapshot":exact,"now_ms":at+4000,
        "required_capabilities":["code"],"estimate_milli":{"points":100},
        "purpose":"worker"}));
    assert_eq!(next["status"],"admitted","{next}");
    let db=rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let attempts:i64=db.query_row("SELECT COUNT(*) FROM swarm_attempts WHERE run_id=?1",
        [&permitted],|row|row.get(0)).unwrap();
    assert_eq!(attempts,2);
}

#[test]
fn newly_selected_account_cannot_create_allocation_after_first_admission() {
    let d=Daemon::start(&[]);
    let run=setup(&d,"New account after work starts",2);
    let at=now();
    let first=admit(&d,&run,"j0","codex-a","initial-account",at,100000,100).unwrap();
    assert_eq!(first["status"],"admitted","{first}");
    d.call("swarm.targets.set",json!({"run_id":run,"request_id":"select-new-account",
        "owner_confirmed":true,"expected_control_revision":0,
        "allowed_targets":["new-account"]}));
    let mut next=snapshot(at+2000,1000000);
    next["targets"].as_array_mut().unwrap().push(json!({"id":"new-account",
        "account_id":"new-account","pool_ids":["new-pool"],"capabilities":["code"],
        "health":"up","auth":"ok"}));
    next["pools"].as_array_mut().unwrap().push(json!({"id":"new-pool","windows":[{
        "id":"week","unit":"points","remaining_milli":1000000,
        "protected_milli":0,"reserved_milli":0,"confidence":"exact",
        "expires_ms":at+62000}]}));
    let observed=d.call("swarm.availability.observe",json!({"run_id":run,
        "snapshot":next,"now_ms":at+2000,"required_capabilities":["code"],
        "estimate_milli":{"points":100},"purpose":"worker"}));
    assert_eq!(observed["state"],"eligible","{observed}");
    let held=d.call("swarm.admit",json!({"run_id":run,"generation":1,"revision":1,
        "job_id":"j1","target_id":"new-account","request_id":"new-account-admission",
        "snapshot":next,"now_ms":at+2000,"required_capabilities":["code"],
        "estimate_milli":{"points":100},"purpose":"worker"}));
    assert_eq!(held["reason"],"allocation_not_frozen","{held}");
    assert_eq!(d.call("swarm.get",json!({"id":run}))["registered_attempts"],1);
}

#[test]
fn shared_pool_reservation_blocks_stale_capacity_across_categories() {
    let d = Daemon::start(&[]);
    let first = setup(&d, "Backend pool", 1);
    let second = setup(&d, "QA pool", 1);
    let at = now();
    let admitted = admit(&d, &first, "j0", "codex-a", "first", at, 60000, 4000).unwrap();
    assert_eq!(admitted["status"], "admitted");
    let denied = admit(&d, &second, "j0", "opencode-a", "second", at, 5000, 1000).unwrap();
    assert_eq!(denied["status"], "blocked");
    assert!(denied["reason"] == "finishing_reserve" || denied["reason"] == "shared_pool_headroom");
    d.call("swarm.artifact.put", json!({"run_id":first,"job_id":"j0",
        "attempt_id":admitted["attempt_id"],"token":admitted["token"],
        "artifact_id":"first-evidence","source_revision":1,
        "kind":"finding","content":"checked"}));
    d.call("swarm.report", json!({"run_id":first,"job_id":"j0",
        "attempt_id":admitted["attempt_id"],"token":admitted["token"],
        "message_id":"first-result","type":"result","revision":1,
        "payload":{"artifact_ids":["first-evidence"]}}));
    d.call("swarm.decide", json!({"run_id":first,"generation":1,"revision":1,
        "job_id":"j0","decision":"accept","evidence":["first-evidence"]}));
    d.call("swarm.attempt.confirm_exit", json!({"run_id":first,"generation":1,
        "revision":1,"job_id":"j0","attempt_id":admitted["attempt_id"]}));
    let still_held = admit(&d, &second, "j0", "opencode-a", "after-exit", at, 5000, 1000).unwrap();
    assert_eq!(still_held["status"], "blocked", "{still_held}");
    assert!(still_held["reason"] == "finishing_reserve" || still_held["reason"] == "shared_pool_headroom");
    assert_eq!(
        d.call("swarm.jobs", json!({"id":second}))["jobs"][0]["status"],
        "ready"
    );
}

#[test]
fn newer_allowance_observation_fences_stale_admission_snapshot() {
    let d=Daemon::start(&[]);
    let owner=setup(&d,"Observed capacity owner",1);
    let candidate=setup(&d,"Observed capacity candidate",1);
    let at=now();
    let held=admit(&d,&owner,"j0","codex-a","owner-reservation",at,100000,1200).unwrap();
    assert_eq!(held["status"],"admitted","{held}");
    let reduced=snapshot(at+2000,1250);
    let observed=d.call("swarm.availability.observe",json!({"run_id":candidate,
        "snapshot":reduced.clone(),"now_ms":at+2000,
        "required_capabilities":["code"],"estimate_milli":{"points":100},
        "purpose":"worker"}));
    assert_eq!(observed["state"],"eligible","{observed}");
    assert_eq!(observed["changed"],true);
    assert!(d.try_call("swarm.availability.observe",json!({"run_id":candidate,
        "snapshot":snapshot(at+2000,1260),"now_ms":at+2000,
        "required_capabilities":["code"],"estimate_milli":{"points":100},
        "purpose":"worker"})).unwrap_err().contains("conflicting availability observation"));
    let request=|id:&str,snapshot:Value,when:i64|json!({"run_id":candidate,
        "generation":1,"revision":1,"job_id":"j0","target_id":"opencode-a",
        "request_id":id,"snapshot":snapshot,"now_ms":when,
        "required_capabilities":["code"],"estimate_milli":{"points":100},
        "purpose":"worker"});
    let stale=d.call("swarm.admit",request("stale-larger",snapshot(at,100000),at+2000));
    assert_eq!(stale["status"],"blocked","{stale}");
    assert_eq!(stale["reason"],"snapshot_superseded");
    let fresh=d.call("swarm.admit",request("current-smaller",reduced,at+2000));
    assert_eq!(fresh["status"],"blocked","{fresh}");
    assert_eq!(fresh["reason"],"shared_pool_headroom");
    let newer=snapshot(at+3000,3000);
    let recovered=d.call("swarm.availability.observe",json!({"run_id":candidate,
        "snapshot":newer.clone(),"now_ms":at+3000,
        "required_capabilities":["code"],"estimate_milli":{"points":100},
        "purpose":"worker"}));
    assert_eq!(recovered["changed"],true);
    assert_eq!(recovered["woken"],false);
    let resumed=d.call("swarm.admit",request("recovered",newer,at+3000));
    assert_eq!(resumed["status"],"admitted","{resumed}");
}

#[test]
fn lowering_one_run_worker_ceiling_drains_without_discarding_active_attempts() {
    let mut d=Daemon::start(&[]);
    let run=setup(&d,"Run ceiling change",3);
    commit_beneficial_batch(&d,&run,&["j0".into(),"j1".into(),"j2".into()]);
    let at=now();
    let first=admit(&d,&run,"j0","codex-a","run-limit-first",at,100000,100).unwrap();
    let second=admit(&d,&run,"j1","codex-a","run-limit-second",at,100000,100).unwrap();
    assert_eq!(first["status"],"admitted","{first}");
    assert_eq!(second["status"],"admitted","{second}");
    let change=json!({"run_id":run,"request_id":"lower-run-ceiling",
        "expected_limit_revision":0,"max_workers":1});
    let lowered=d.call("swarm.limit.set",change.clone());
    assert_eq!(lowered["limit_revision"],1,"{lowered}");
    assert_eq!(lowered["max_workers"],1);
    assert_eq!(d.call("swarm.limit.set",change.clone())["duplicate"],true);
    let mut reused=change;
    reused["max_workers"]=json!(2);
    assert!(d.try_call("swarm.limit.set",reused).unwrap_err().contains("reused"));
    assert!(d.try_call("swarm.limit.set",json!({"run_id":run,
        "request_id":"stale-run-ceiling","expected_limit_revision":0,
        "max_workers":3})).unwrap_err().contains("stale limit revision"));
    d.kill9();
    d.spawn();
    assert_eq!(d.call("swarm.limit.set",json!({"run_id":run,
        "request_id":"lower-run-ceiling","expected_limit_revision":0,
        "max_workers":1}))["duplicate"],true);
    let current=d.call("swarm.get",json!({"id":run}));
    assert_eq!(current["limit_revision"],1);
    assert_eq!(current["policy"]["effective"]["max_workers"],1);
    assert_eq!(current["policy"]["sources"]["max_workers"],"run_update");
    let held=admit(&d,&run,"j2","codex-a","run-limit-held",at,100000,100).unwrap();
    assert_eq!(held["reason"],"worker_limit","{held}");
    for (job,attempt) in [("j0",&first),("j1",&second)] {
        d.call("swarm.attempt.confirm_exit",json!({"run_id":run,"generation":1,
            "revision":1,"job_id":job,"attempt_id":attempt["attempt_id"]}));
        if job=="j0" {
            assert_eq!(admit(&d,&run,"j2","codex-a","run-limit-still-held",at,100000,100)
                .unwrap()["reason"],"worker_limit");
        }
    }
    let resumed=admit(&d,&run,"j2","codex-a","run-limit-resumed",at,100000,100).unwrap();
    assert_eq!(resumed["status"],"admitted","{resumed}");
    let db=rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let events:i64=db.query_row("SELECT COUNT(*) FROM swarm_limit_events WHERE run_id=?1",
        [&run],|r|r.get(0)).unwrap();
    assert_eq!(events,1);
}

#[test]
fn ordinary_run_occupies_global_slot_until_confirmed_exit() {
    let d = Daemon::start(&[]);
    d.call("agents.limit.set",json!({"max_active":2}));
    let temp = tmp();
    let checkout = repo(&temp.path().join("ordinary"));
    let ordinary = d.generic(&checkout, "worktree", "/bin/sleep", &["2"]);
    assert!(ordinary["launch_error"].is_null());
    let ordinary_id = run_id(&ordinary);
    assert!(["starting", "running"].contains(&d.run(&ordinary_id)["status"].as_str().unwrap()));
    let swarm = d.call("swarm.create", json!({"category":"Shared global slots",
        "objective":"Audit", "allowed_targets":["codex-a"],
        "policy":{"max_workers":1}}));
    let id = swarm["id"].as_str().unwrap();
    d.call("swarm.plan",json!({"id":id,"generation":1,"revision":0,"jobs":[
        {"id":"j0","title":"Inspect","acceptance":"evidence","deps":[]}
    ]}));
    let at = now();
    let held = admit(&d,id,"j0","codex-a","ordinary-active",at,100000,100).unwrap();
    assert_eq!(held["status"],"blocked");
    assert_eq!(held["reason"],"global_agent_limit");
    d.wait_done(&ordinary_id,5);
    let admitted = admit(&d,id,"j0","codex-a","ordinary-finished",now(),100000,100).unwrap();
    assert_eq!(admitted["status"],"admitted");
}

#[test]
fn app_agent_limit_serializes_manual_launches_and_ignores_native_children() {
    let mut d = Daemon::start(&[]);
    assert_eq!(d.call("agents.limit.get",json!({}))["max_active"],9);
    assert_eq!(d.call("agents.limit.set",json!({"max_active":2}))["max_active"],2);
    assert!(d.try_call("agents.limit.set",json!({"max_active":0})).is_err());
    d.kill9();
    d.spawn();
    assert_eq!(d.call("agents.limit.get",json!({}))["max_active"],2);
    let temp = tmp();
    let checkout = repo(&temp.path().join("app-limit"));
    let first = d.generic(&checkout,"worktree","/bin/sleep", &["8"]);
    let first_id = run_id(&first);
    assert!(first["launch_error"].is_null());
    let child_id = "r-synthetic-native-child";
    rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap().execute(
        "INSERT INTO runs(id,task_id,parent_run_id,harness,harness_version,profile_id,model,
         workspace_id,native_id,status,exit_reason,created_ms,ended_ms,title,relation_source,
         relation_confidence,capabilities,process_generation)
         SELECT ?2,task_id,id,harness,harness_version,profile_id,model,workspace_id,
         'synthetic-child','running',NULL,created_ms,NULL,'Native child','fixture','exact',
         capabilities,0 FROM runs WHERE id=?1",
        rusqlite::params![first_id,child_id]).unwrap();
    let second = d.generic(&checkout,"worktree","/bin/sleep", &["8"]);
    assert!(second["launch_error"].is_null());
    let request = json!({"id":7,"method":"task.create","params":{"repo":checkout,
        "harness":"generic","workspace_mode":"worktree","program":"/bin/sleep",
        "args":["8"],"prompt":"","title":"over limit"}});
    let reply: Value = serde_json::from_str(&d.raw(format!("{request}\n").as_bytes())).unwrap();
    assert_eq!(reply["error"]["code"],"agent_limit","{reply}");
    assert_eq!(reply["error"]["active"],2);
    assert_eq!(reply["error"]["limit"],2);
    d.call("run.interrupt",json!({"run_id":first_id}));
    d.wait_done(&first_id,5);
    let third = d.generic(&checkout,"worktree","/bin/sleep", &["1"]);
    assert!(third["launch_error"].is_null());
}

#[test]
fn app_agent_limit_holds_swarm_admission_until_a_slot_frees() {
    let d = Daemon::start(&[]);
    d.call("agents.limit.set",json!({"max_active":2}));
    let temp = tmp();
    let checkout = repo(&temp.path().join("swarm-limit"));
    let ordinary = d.generic(&checkout,"worktree","/bin/sleep", &["8"]);
    let ordinary_id = run_id(&ordinary);
    let id = setup(&d,"Shared app limit",1);
    let at = now();
    let held = admit(&d,&id,"j0","codex-a","at-cap",at,100000,100).unwrap();
    assert_eq!(held["status"],"blocked","{held}");
    assert_eq!(held["reason"],"global_agent_limit");
    d.call("run.interrupt",json!({"run_id":ordinary_id}));
    d.wait_done(&ordinary_id,5);
    let admitted = admit(&d,&id,"j0","codex-a","slot-freed",now(),100000,100).unwrap();
    assert_eq!(admitted["status"],"admitted","{admitted}");
    let request = json!({"id":8,"method":"task.create","params":{"repo":checkout,
        "harness":"generic","workspace_mode":"worktree","program":"/bin/sleep",
        "args":["1"],"prompt":"","title":"manual after swarm"}});
    let reply: Value = serde_json::from_str(&d.raw(format!("{request}\n").as_bytes())).unwrap();
    assert_eq!(reply["error"]["code"],"agent_limit","{reply}");
    assert_eq!(reply["error"]["running_agents"].as_array().unwrap().len(),2);
}

#[test]
fn concurrent_manual_starts_cannot_claim_the_same_last_slot() {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixStream;
    let d = Daemon::start(&[]);
    d.call("agents.limit.set", json!({"max_active":1}));
    let temp = tmp();
    let checkout = repo(&temp.path().join("concurrent-limit"));
    let socket = d.socket();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
    let mut handles = Vec::new();
    for n in 0..2 {
        let socket = socket.clone();
        let checkout = checkout.clone();
        let barrier = barrier.clone();
        handles.push(std::thread::spawn(move || {
            let mut conn = UnixStream::connect(socket).unwrap();
            conn.set_read_timeout(Some(std::time::Duration::from_secs(15))).unwrap();
            let request = json!({"id":n,"method":"task.create","params":{
                "repo":checkout,"harness":"generic","workspace_mode":"worktree",
                "program":"/bin/sleep","args":["5"],"prompt":"","title":format!("parallel-{n}")}});
            barrier.wait();
            conn.write_all(format!("{request}\n").as_bytes()).unwrap();
            let mut line = String::new();
            BufReader::new(conn).read_line(&mut line).unwrap();
            serde_json::from_str::<Value>(&line).unwrap()
        }));
    }
    barrier.wait();
    let replies: Vec<Value> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    assert_eq!(replies.iter().filter(|r| r["result"].is_object()).count(),1,"{replies:?}");
    assert_eq!(replies.iter().filter(|r| r["error"]["code"]=="agent_limit").count(),1,"{replies:?}");
    assert_eq!(d.call("agents.limit.get",json!({}))["active"],1);
}

#[test]
fn stopped_swarm_releases_director_slot_after_attempt_exit() {
    let d = Daemon::start(&[]);
    d.call("agents.limit.set", json!({"max_active":2}));
    let first = setup(&d,"First director",1);
    let second = setup(&d,"Second director",1);
    let attempt = admit(&d,&first,"j0","codex-a","first-director",now(),100000,100).unwrap();
    assert_eq!(attempt["status"],"admitted");
    let held = admit(&d,&second,"j0","codex-a","second-held",now(),100000,100).unwrap();
    assert_eq!(held["reason"],"global_agent_limit");
    let stopping = d.call("swarm.stop",json!({"run_id":first,"generation":1,"revision":1}));
    assert_eq!(stopping["status"],"stopping");
    assert_eq!(d.call("agents.limit.get",json!({}))["active"],2);
    d.call("swarm.attempt.confirm_exit",json!({"run_id":first,"generation":1,
        "revision":1,"job_id":"j0","attempt_id":attempt["attempt_id"]}));
    assert_eq!(d.call("swarm.get",json!({"id":first}))["status"],"stopped");
    assert_eq!(d.call("agents.limit.get",json!({}))["active"],0);
    let admitted = admit(&d,&second,"j0","codex-a","second-started",now(),100000,100).unwrap();
    assert_eq!(admitted["status"],"admitted","{admitted}");
}

#[test]
fn three_slot_limit_runs_director_and_two_workers_then_reuses_confirmed_slot() {
    let d = Daemon::start(&[]);
    d.call("agents.limit.set",json!({"max_active":3}));
    let temp = tmp();
    let checkout = repo(&temp.path().join("three-slot-director"));
    let run = setup(&d,"Three slot director",2);
    commit_beneficial_batch(&d,&run,&["j0".into(),"j1".into()]);
    let at = now();
    let first = admit(&d,&run,"j0","codex-a","three-slot-0",at,100000,100).unwrap();
    let second = admit(&d,&run,"j1","codex-a","three-slot-1",at,100000,100).unwrap();
    assert_eq!(first["status"],"admitted","{first}");
    assert_eq!(second["status"],"admitted","{second}");
    let launched = d.call("swarm.director.launch",json!({"run_id":run,
        "generation":1,"repo":checkout,"program":"/bin/sleep","args":["30"],
        "prompt":"Direct the audit","title":"Three slot director"}));
    let director = launched["overseer_run_id"].as_str().unwrap();
    assert_eq!(launched["status"],"launched");
    assert_eq!(d.call("agents.limit.get",json!({}))["active"],3);
    assert!(d.try_call("task.create",json!({"repo":checkout,"harness":"generic",
        "workspace_mode":"worktree","program":"/bin/sleep","args":["30"],
        "prompt":"","title":"fourth agent"})).unwrap_err().contains("agent limit reached"));
    d.call("swarm.attempt.confirm_exit",json!({"run_id":run,"generation":1,
        "revision":1,"job_id":"j0","attempt_id":first["attempt_id"]}));
    assert_eq!(d.call("agents.limit.get",json!({}))["active"],2);
    let ordinary = d.generic(&checkout,"worktree","/bin/sleep", &["30"]);
    assert!(ordinary["launch_error"].is_null(),"{ordinary}");
    assert_eq!(d.call("agents.limit.get",json!({}))["active"],3);
    d.call("run.interrupt",json!({"run_id":run_id(&ordinary)}));
    d.wait_done(&run_id(&ordinary),8);
    d.call("swarm.stop",json!({"run_id":run}));
    d.call("swarm.attempt.confirm_exit",json!({"run_id":run,"generation":1,
        "revision":1,"job_id":"j1","attempt_id":second["attempt_id"]}));
    d.wait_done(director,8);
    let deadline = Instant::now()+Duration::from_secs(3);
    while d.call("swarm.get",json!({"id":run}))["status"] != "stopped" {
        assert!(Instant::now()<deadline,"director exit did not finish Stop");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(d.call("agents.limit.get",json!({}))["active"],0);
}

#[test]
fn three_slot_limit_counts_live_director_and_workers_until_confirmed_exit() {
    let d = Daemon::start(&[]);
    d.call("agents.limit.set",json!({"max_active":3}));
    let temp = tmp();
    let checkout = repo(&temp.path().join("three-live-slots"));
    let run = setup(&d,"Three live slots",2);
    commit_beneficial_batch(&d,&run,&["j0".into(),"j1".into()]);
    let at = now();
    let first = admit(&d,&run,"j0","codex-a","live-slot-0",at,100000,100).unwrap();
    let second = admit(&d,&run,"j1","codex-a","live-slot-1",at,100000,100).unwrap();
    assert_eq!(first["status"],"admitted","{first}");
    assert_eq!(second["status"],"admitted","{second}");

    let director = d.call("swarm.director.launch",json!({"run_id":run,
        "generation":1,"repo":checkout,"program":"/bin/sleep","args":["30"],
        "prompt":"Direct the audit","title":"Live three-slot director"}));
    assert_eq!(director["status"],"launched","{director}");
    let director_run = director["overseer_run_id"].as_str().unwrap();
    let launch = |job: &str, admitted: &Value| d.call("swarm.worker.launch",json!({
        "run_id":run,"job_id":job,"attempt_id":admitted["attempt_id"],
        "token":admitted["token"],"repo":checkout,"harness":"generic",
        "program":"/bin/sleep","args":["30"],"prompt":"Audit the route",
        "title":format!("Live worker {job}")}));
    let worker0 = launch("j0",&first);
    let worker1 = launch("j1",&second);
    assert_eq!(worker0["status"],"launched","{worker0}");
    assert_eq!(worker1["status"],"launched","{worker1}");
    let first_run = worker0["overseer_run_id"].as_str().unwrap();
    let second_run = worker1["overseer_run_id"].as_str().unwrap();
    let deadline = Instant::now() + Duration::from_secs(4);
    while [director_run, first_run, second_run]
        .iter().any(|id| d.run(id)["status"] != "running") {
        assert!(Instant::now() < deadline,"all three supervised processes must be live");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(d.call("agents.limit.get",json!({}))["active"],3);
    assert!(d.try_call("task.create",json!({"repo":checkout,"harness":"generic",
        "workspace_mode":"worktree","program":"/bin/sleep","args":["30"],
        "prompt":"","title":"fourth agent"})).unwrap_err().contains("agent limit reached"));
    assert!(d.try_call("swarm.attempt.confirm_exit",json!({"run_id":run,
        "generation":1,"revision":1,"job_id":"j0","attempt_id":first["attempt_id"]}))
        .unwrap_err().contains("exit is not confirmed"));
    d.call("run.interrupt",json!({"run_id":first_run}));
    d.wait_done(first_run,8);
    let active = d.call("agents.limit.get",json!({}))["active"].as_i64().unwrap();
    if active == 2 {
        // The daemon's background reconciler may confirm this terminal
        // supervisor before the explicit fixture call below reaches it.
        let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
        let status: String = db.query_row("SELECT status FROM swarm_attempts WHERE id=?1",
            [first["attempt_id"].as_str().unwrap()],|r|r.get(0)).unwrap();
        let ended: Option<i64> = db.query_row("SELECT ended_ms FROM runs WHERE id=?1",
            [first_run],|r|r.get(0)).unwrap();
        assert_eq!(status,"finished","slot may release only after daemon confirmation");
        assert!(ended.is_some(),"the linked process must have a terminal receipt");
    } else {
        assert_eq!(active,3,"an unconfirmed attempt must retain its slot");
    }
    d.call("swarm.attempt.confirm_exit",json!({"run_id":run,
        "generation":1,"revision":1,"job_id":"j0","attempt_id":first["attempt_id"]}));
    assert_eq!(d.call("agents.limit.get",json!({}))["active"],2);
    let ordinary = d.generic(&checkout,"worktree","/bin/sleep", &["30"]);
    assert!(ordinary["launch_error"].is_null(),"{ordinary}");
    assert_eq!(d.call("agents.limit.get",json!({}))["active"],3);

    d.call("run.interrupt",json!({"run_id":run_id(&ordinary)}));
    d.wait_done(&run_id(&ordinary),8);
    d.call("swarm.stop",json!({"run_id":run}));
    d.wait_done(second_run,8);
    d.call("swarm.attempt.confirm_exit",json!({"run_id":run,
        "generation":1,"revision":1,"job_id":"j1","attempt_id":second["attempt_id"]}));
    d.wait_done(director_run,8);
    let deadline = Instant::now()+Duration::from_secs(3);
    while d.call("swarm.get",json!({"id":run}))["status"] != "stopped" {
        assert!(Instant::now()<deadline,"director exit did not finish Stop");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(d.call("agents.limit.get",json!({}))["active"],0);
}

#[test]
fn lowering_app_limit_holds_new_workers_until_existing_work_drains() {
    let d = Daemon::start(&[]);
    d.call("agents.limit.set",json!({"max_active":3}));
    let id = setup(&d,"Lower while active",3);
    let benefit=commit_beneficial_batch(&d,&id,&["j0".into(),"j1".into(),"j2".into()]);
    assert_eq!(benefit["max_parallel_workers"],2);
    let at = now();
    let first = admit(&d,&id,"j0","codex-a","before-lower-0",at,100000,100).unwrap();
    let second = admit(&d,&id,"j1","codex-a","before-lower-1",at,100000,100).unwrap();
    assert_eq!(first["status"],"admitted");
    assert_eq!(second["status"],"admitted","{second}");
    assert_eq!(d.call("agents.limit.get",json!({}))["active"],3);
    d.call("agents.limit.set",json!({"max_active":2}));
    assert_eq!(admit(&d,&id,"j2","codex-a","lower-held",at,100000,100).unwrap()["reason"],"global_agent_limit");
    d.call("swarm.attempt.confirm_exit",json!({"run_id":id,"generation":1,
        "revision":1,"job_id":"j0","attempt_id":first["attempt_id"]}));
    assert_eq!(d.call("agents.limit.get",json!({}))["active"],2);
    assert_eq!(admit(&d,&id,"j2","codex-a","still-held",at,100000,100).unwrap()["reason"],"global_agent_limit");
    d.call("swarm.attempt.confirm_exit",json!({"run_id":id,"generation":1,
        "revision":1,"job_id":"j1","attempt_id":second["attempt_id"]}));
    assert_eq!(d.call("agents.limit.get",json!({}))["active"],1);
    assert_eq!(admit(&d,&id,"j2","codex-a","drained",at,100000,100).unwrap()["status"],"admitted");
}

#[test]
fn ordinary_launches_take_priority_over_active_swarm_capacity() {
    let d = Daemon::start(&[]);
    d.call("agents.limit.set",json!({"max_active":3}));
    let temp = tmp();
    let checkout = repo(&temp.path().join("ordinary-after-swarm"));
    let swarm = d.call("swarm.create", json!({"category":"Reverse shared slots",
        "objective":"Audit", "allowed_targets":["codex-a"],
        "policy":{"max_workers":2}}));
    let id = swarm["id"].as_str().unwrap();
    d.call("swarm.plan",json!({"id":id,"generation":1,"revision":0,"jobs":[
        {"id":"j0","title":"Inspect","acceptance":"evidence","deps":[]},
        {"id":"j1","title":"Inspect again","acceptance":"evidence","deps":[]}
    ]}));
    let first = admit(&d,id,"j0","codex-a","reserve-first",now(),100000,100).unwrap();
    assert_eq!(first["status"],"admitted");
    let manual = d.call("task.create",json!({"repo":checkout,
        "harness":"generic","workspace_mode":"worktree", "program":"/bin/sleep",
        "args":["30"],"prompt":"","title":"ordinary when full"}));
    assert!(manual["launch_error"].is_null(),"{manual}");
    let held = admit(&d,id,"j1","codex-a","manual-priority",now(),100000,100).unwrap();
    assert_eq!(held["reason"],"global_agent_limit","{held}");

    d.call("swarm.attempt.confirm_exit",json!({"run_id":id,"generation":1,
        "revision":1,"job_id":"j0","attempt_id":first["attempt_id"]}));
    d.call("agents.limit.set",json!({"max_active":4}));
    let barrier = std::sync::Barrier::new(3);
    let launches = std::thread::scope(|scope| {
        let first = scope.spawn(|| {
            barrier.wait();
            d.try_call("task.create",json!({"repo":checkout,
                "harness":"generic","workspace_mode":"worktree","program":"/bin/sleep",
                "args":["30"],"prompt":"","title":"ordinary a"}))
        });
        let second = scope.spawn(|| {
            barrier.wait();
            d.try_call("task.create",json!({"repo":checkout,
                "harness":"generic","workspace_mode":"worktree","program":"/bin/sleep",
                "args":["30"],"prompt":"","title":"ordinary b"}))
        });
        barrier.wait();
        [first.join().unwrap(),second.join().unwrap()]
    });
    assert_eq!(launches.iter().filter(|r| r.as_ref().is_ok_and(|v| v["launch_error"].is_null())).count(),2,"{launches:?}");
    let still_held = admit(&d,id,"j1","codex-a","manual-priority-concurrent",now(),100000,100).unwrap();
    assert_eq!(still_held["reason"],"global_agent_limit","{still_held}");
}

#[test]
fn full_director_inbox_holds_new_admissions_but_keeps_terminal_reports() {
    let d = Daemon::start(&[]);
    let id = setup(&d, "Inbox pressure", 2);
    let at = now();
    let first = admit(&d, &id, "j0", "codex-a", "inbox-first", at, 1000000, 100)
        .unwrap();
    assert_eq!(first["status"], "admitted");
    for n in 0..1000 {
        d.call(
            "swarm.report",
            json!({"run_id":id,"job_id":"j0","attempt_id":first["attempt_id"],
                "token":first["token"],"message_id":format!("progress-{n}"),
                "type":"progress","revision":1,"payload":{"n":n}}),
        );
    }
    let held = admit(&d, &id, "j1", "codex-a", "inbox-second", at, 1000000, 100)
        .unwrap();
    assert_eq!(held["status"], "blocked");
    assert_eq!(held["reason"], "director_inbox_full");
    d.call("swarm.report", json!({"run_id":id,"job_id":"j0",
        "attempt_id":first["attempt_id"],"token":first["token"],
        "message_id":"terminal-at-capacity","type":"result","revision":1,
        "payload":{"artifact_ids":[]}}));
    assert_eq!(d.call("swarm.jobs",json!({"id":id}))["jobs"][0]["status"], "submitted");
}

#[test]
fn mutable_resource_conflict_is_rejected_before_reserving_an_attempt() {
    let d=Daemon::start(&[]);
    let first=setup(&d,"DB writer A",1);
    let second=setup(&d,"DB writer B",1);
    let at=now();
    let request=|run:&str,request_id:&str,mode:&str|json!({"run_id":run,"generation":1,
        "revision":1,"job_id":"j0","target_id":"codex-a","request_id":request_id,
        "snapshot":snapshot(at,1000000),"now_ms":at,"required_capabilities":["code"],
        "estimate_milli":{"points":100},"purpose":"worker",
        "resource_claims":[{"resource":"db:shared-test","mode":mode}]
    });
    let admitted=d.call("swarm.admit",request(&first,"writer-a","write"));
    assert_eq!(admitted["status"],"admitted");
    let mut invalid=request(&second,"invalid-claims","write");
    invalid["resource_claims"]=json!([
        {"resource":"db:shared-test","mode":"write"},
        {"resource":"db:shared-test","mode":"read"}
    ]);
    assert!(d.try_call("swarm.admit",invalid).unwrap_err().contains("duplicate resource"));
    let held=d.call("swarm.admit",request(&second,"writer-b","write"));
    assert_eq!(held["reason"],"resource_conflict");
    assert_eq!(d.call("swarm.jobs",json!({"id":second}))["jobs"][0]["status"],"ready");
    let db=rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let attempts:i64=db.query_row("SELECT COUNT(*) FROM swarm_attempts WHERE run_id=?1",[&second],|r|r.get(0)).unwrap();
    assert_eq!(attempts,0);
    assert_eq!(d.call("swarm.admit",request(&second,"reader-b","read"))["reason"],"resource_conflict");
    d.call("swarm.artifact.put",json!({"run_id":first,"job_id":"j0",
        "attempt_id":admitted["attempt_id"],"token":admitted["token"],
        "artifact_id":"writer-a-result","source_revision":1,"kind":"finding","content":"checked"}));
    d.call("swarm.report",json!({"run_id":first,"job_id":"j0",
        "attempt_id":admitted["attempt_id"],"token":admitted["token"],
        "message_id":"writer-a-done","type":"result","revision":1,
        "payload":{"artifact_ids":["writer-a-result"]}}));
    d.call("swarm.decide",json!({"run_id":first,"generation":1,"revision":1,
        "job_id":"j0","decision":"reject","evidence":["writer-a-result"]}));
    d.call("swarm.attempt.confirm_exit",json!({"run_id":first,"generation":1,
        "revision":1,"job_id":"j0","attempt_id":admitted["attempt_id"]}));
    assert_eq!(d.call("swarm.admit",request(&second,"writer-b","write"))["status"],"admitted");
}

#[test]
fn late_shared_database_use_quarantines_evidence_and_bounds_retries() {
    let d=Daemon::start(&[]);
    let first=setup(&d,"Late DB A",1);
    let second=setup(&d,"Late DB B",1);
    let at=now();
    let a=admit(&d,&first,"j0","codex-a","late-a",at,1000000,100).unwrap();
    let b=admit(&d,&second,"j0","codex-a","late-b",at,1000000,100).unwrap();
    for (run,attempt,label) in [(&first,&a,"a"),(&second,&b,"b")] {
        d.call("swarm.artifact.put",json!({"run_id":run,"job_id":"j0",
            "attempt_id":attempt["attempt_id"],"token":attempt["token"],
            "artifact_id":format!("evidence-{label}"),"source_revision":1,
            "kind":"finding","content":"db row changed"}));
        d.call("swarm.report",json!({"run_id":run,"job_id":"j0",
            "attempt_id":attempt["attempt_id"],"token":attempt["token"],
            "message_id":format!("result-{label}"),"type":"result","revision":1,
            "payload":{"artifact_ids":[format!("evidence-{label}")]}}));
    }
    let observed=|run:&str,attempt:&Value,resource:&str|json!({"run_id":run,"job_id":"j0",
        "attempt_id":attempt["attempt_id"],"token":attempt["token"],
        "generation":1,"revision":1,"resource":resource,"mode":"write",
        "after_use":true});
    assert_eq!(d.call("swarm.claim",observed(&first,&a,"db:shared-late"))["duplicate"],false);
    let conflict=d.call("swarm.claim",observed(&second,&b,"db:shared-late"));
    assert_eq!(conflict["status"],"contaminated", "{conflict}");
    assert_eq!(d.call("swarm.claim",observed(&second,&b,"db:shared-late"))["duplicate"],true);
    for (run,attempt,label) in [(&first,&a,"a"),(&second,&b,"b")] {
        assert_eq!(d.call("swarm.jobs",json!({"id":run}))["jobs"][0]["status"],"cancel_requested");
        assert_eq!(d.call("swarm.coverage",json!({"run_id":run}))["rows"][0]["coverage_state"],"contaminated");
        assert!(d.try_call("swarm.decide",json!({"run_id":run,"generation":1,"revision":1,
            "job_id":"j0","decision":"accept","evidence":[format!("evidence-{label}")]})).is_err());
        let db=rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
        let retained:i64=db.query_row("SELECT COUNT(*) FROM swarm_artifacts WHERE run_id=?1",[run],|r|r.get(0)).unwrap();
        assert_eq!(retained,1);
        let batch=d.call("swarm.director.claim_batch",json!({"run_id":run,
            "generation":1,"revision":1,"now_ms":now()+6000}));
        assert_eq!(batch["status"],"claimed","{batch}");
        d.call("swarm.director.complete_batch",json!({"run_id":run,
            "generation":1,"turn_id":batch["turn_id"],"token":batch["token"],
            "outcome":"progress"}));
        let pending:i64=db.query_row("SELECT COUNT(*) FROM swarm_messages WHERE run_id=?1
            AND recipient='director' AND phase!='applied'",[run],|r|r.get(0)).unwrap();
        assert_eq!(pending,0,"quarantined result must not block an isolated retry");
        d.call("swarm.attempt.confirm_exit",json!({"run_id":run,"generation":1,"revision":1,
            "job_id":"j0","attempt_id":attempt["attempt_id"]}));
        assert_eq!(d.call("swarm.jobs",json!({"id":run}))["jobs"][0]["status"],"ready");
    }
    assert_eq!(d.call("swarm.claim",observed(&second,&b,"db:shared-late"))["duplicate"],true);
    let retried=admit(&d,&first,"j0","codex-a","late-a-retry",at,1000000,100).unwrap();
    assert_eq!(retried["status"],"admitted");
    assert_ne!(retried["attempt_id"],a["attempt_id"]);
    let retried_peer=admit(&d,&second,"j0","codex-a","late-b-retry",at,1000000,100).unwrap();
    d.call("swarm.claim",observed(&first,&retried,"db:shared-late-reset"));
    assert_eq!(d.call("swarm.claim",observed(&second,&retried_peer,"db:shared-late-reset"))["status"],"contaminated");
    for (run,attempt) in [(&first,&retried),(&second,&retried_peer)] {
        d.call("swarm.attempt.confirm_exit",json!({"run_id":run,"generation":1,"revision":1,
            "job_id":"j0","attempt_id":attempt["attempt_id"]}));
        assert_eq!(d.call("swarm.jobs",json!({"id":run}))["jobs"][0]["status"],"failed");
        let third=admit(&d,run,"j0","codex-a","forbidden-third",at,1000000,100);
        assert!(third.is_err() || third.unwrap()["status"]!="admitted");
    }
}

#[test]
fn unconfirmed_planned_owner_preserves_late_use_and_holds_both_jobs() {
    let mut d = Daemon::start(&[]);
    let owner = setup(&d, "Planned DB owner", 1);
    let observer = setup(&d, "Unplanned DB observer", 1);
    d.call("swarm.claim", json!({"run_id":owner,"job_id":"j0",
        "generation":1,"revision":1,"resource":"db:unconfirmed",
        "mode":"write"}));
    let at = now();
    let attempt = admit(&d,&observer,"j0","codex-a","unconfirmed-use",at,1000000,100).unwrap();
    d.call("swarm.artifact.put",json!({"run_id":observer,"job_id":"j0",
        "attempt_id":attempt["attempt_id"],"token":attempt["token"],
        "artifact_id":"unconfirmed-evidence","source_revision":1,
        "kind":"finding","content":"database row observed"}));
    d.call("swarm.report",json!({"run_id":observer,"job_id":"j0",
        "attempt_id":attempt["attempt_id"],"token":attempt["token"],
        "message_id":"unconfirmed-result","type":"result","revision":1,
        "payload":{"artifact_ids":["unconfirmed-evidence"]}}));
    let observation = json!({"run_id":observer,"job_id":"j0",
        "attempt_id":attempt["attempt_id"],"token":attempt["token"],
        "generation":1,"revision":1,"resource":"db:unconfirmed",
        "mode":"write","after_use":true});
    let result = d.call("swarm.claim",observation.clone());
    assert_eq!(result["status"],"contaminated");
    assert_eq!(result["unconfirmed_planned_owners"],1);
    assert_eq!(d.call("swarm.jobs",json!({"id":observer}))["jobs"][0]["status"],"cancel_requested");
    assert_eq!(d.call("swarm.jobs",json!({"id":owner}))["jobs"][0]["status"],"blocked");
    assert_eq!(d.call("swarm.coverage",json!({"run_id":observer}))["rows"][0]["coverage_state"],"contaminated");
    assert!(d.try_call("swarm.decide",json!({"run_id":observer,
        "generation":1,"revision":1,"job_id":"j0","decision":"accept",
        "evidence":["unconfirmed-evidence"]})).is_err());
    d.kill9();
    d.spawn();
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let observations: i64 = db.query_row("SELECT COUNT(*) FROM swarm_resource_observations
        WHERE run_id=?1 AND job_id='j0' AND resource='db:unconfirmed'",
        [&observer],|r|r.get(0)).unwrap();
    assert_eq!(observations,1);
    assert_eq!(d.call("swarm.claim",observation)["duplicate"],true);
    assert_eq!(d.call("swarm.jobs",json!({"id":owner}))["jobs"][0]["status"],"blocked");
    d.call("swarm.attempt.confirm_exit",json!({"run_id":observer,
        "generation":1,"revision":1,"job_id":"j0","attempt_id":attempt["attempt_id"]}));
    assert_eq!(d.call("swarm.jobs",json!({"id":observer}))["jobs"][0]["status"],"ready");
}

#[test]
fn late_write_after_declared_read_quarantines_both_attempts() {
    let d=Daemon::start(&[]);
    let first=setup(&d,"Late upgrade A",1);
    let second=setup(&d,"Late upgrade B",1);
    for run in [&first,&second] {
        d.call("swarm.claim",json!({"run_id":run,"job_id":"j0","generation":1,
            "revision":1,"resource":"db:upgrade","mode":"read"}));
    }
    let at=now();
    let a=admit(&d,&first,"j0","codex-a","upgrade-a",at,1000000,100).unwrap();
    let b=admit(&d,&second,"j0","codex-a","upgrade-b",at,1000000,100).unwrap();
    let conflict=d.call("swarm.claim",json!({"run_id":first,"job_id":"j0",
        "attempt_id":a["attempt_id"],"token":a["token"],"generation":1,"revision":1,
        "resource":"db:upgrade","mode":"write","after_use":true}));
    assert_eq!(conflict["status"],"contaminated", "{conflict}");
    for (run,attempt) in [(&first,&a),(&second,&b)] {
        assert_eq!(d.call("swarm.jobs",json!({"id":run}))["jobs"][0]["status"],"cancel_requested");
        d.call("swarm.attempt.confirm_exit",json!({"run_id":run,"generation":1,"revision":1,
            "job_id":"j0","attempt_id":attempt["attempt_id"]}));
    }
}

#[test]
fn late_shared_read_only_snapshot_remains_parallel() {
    let d=Daemon::start(&[]);
    let first=setup(&d,"Read A",1);
    let second=setup(&d,"Read B",1);
    let at=now();
    let a=admit(&d,&first,"j0","codex-a","read-a",at,1000000,100).unwrap();
    let b=admit(&d,&second,"j0","codex-a","read-b",at,1000000,100).unwrap();
    for (run,attempt) in [(&first,&a),(&second,&b)] {
        let result=d.call("swarm.claim",json!({"run_id":run,"job_id":"j0",
            "attempt_id":attempt["attempt_id"],"token":attempt["token"],
            "generation":1,"revision":1,"resource":"db:snapshot","mode":"read",
            "after_use":true}));
        assert_ne!(result["status"],"contaminated", "{result}");
        assert_ne!(d.call("swarm.jobs",json!({"id":run}))["jobs"][0]["status"],"cancel_requested");
    }
}

#[test]
fn historical_shared_database_use_revokes_accepted_evidence_after_exit() {
    let mut d=Daemon::start(&[]);
    let first=setup(&d,"Historical DB A",1);
    let second=setup(&d,"Historical DB B",1);
    let at=now();
    let a=admit(&d,&first,"j0","codex-a","historical-a",at,1000000,100).unwrap();
    let observed=|run:&str,attempt:&Value|json!({"run_id":run,"job_id":"j0",
        "attempt_id":attempt["attempt_id"],"token":attempt["token"],
        "generation":1,"revision":1,"resource":"db:historical","mode":"write",
        "after_use":true});
    d.call("swarm.claim",observed(&first,&a));
    d.call("swarm.artifact.put",json!({"run_id":first,"job_id":"j0",
        "attempt_id":a["attempt_id"],"token":a["token"],
        "artifact_id":"historical-evidence","source_revision":1,
        "kind":"finding","content":"row changed"}));
    d.call("swarm.report",json!({"run_id":first,"job_id":"j0",
        "attempt_id":a["attempt_id"],"token":a["token"],
        "message_id":"historical-result","type":"result","revision":1,
        "payload":{"artifact_ids":["historical-evidence"],"audit_outcome":"negative"}}));
    d.call("swarm.decide",json!({"run_id":first,"generation":1,"revision":1,
        "job_id":"j0","decision":"accept","evidence":["historical-evidence"]}));
    d.call("swarm.attempt.confirm_exit",json!({"run_id":first,"generation":1,
        "revision":1,"job_id":"j0","attempt_id":a["attempt_id"]}));
    assert_eq!(d.call("swarm.coverage",json!({"run_id":first}))["rows"][0]["coverage_state"],
        "checked_negative");
    let batch=d.call("swarm.director.claim_batch",json!({"run_id":first,
        "generation":1,"revision":1,"now_ms":now()+6000}));
    d.call("swarm.director.complete_batch",json!({"run_id":first,"generation":1,
        "turn_id":batch["turn_id"],"token":batch["token"],"outcome":"no_progress"}));
    let completion=json!({"run_id":first,"generation":1,"revision":1,
        "request_id":"historical-complete","summary":"Checked the path",
        "verification":"Fixture evidence reviewed",
        "checks":[{"job_id":"j0","outcome":"passed","evidence":["historical-evidence"]}]});
    assert_eq!(d.call("swarm.complete",completion.clone())["status"],"completed");
    assert_eq!(d.call("swarm.get",json!({"id":first}))["completion"]["valid"],true);
    d.kill9();
    d.spawn();

    let b=admit(&d,&second,"j0","codex-a","historical-b",at,1000000,100).unwrap();
    let conflict=d.call("swarm.claim",observed(&second,&b));
    assert_eq!(conflict["status"],"contaminated", "{conflict}");
    assert_eq!(d.call("swarm.coverage",json!({"run_id":first}))["rows"][0]["coverage_state"],
        "contaminated");
    let prior=d.call("swarm.get",json!({"id":first}));
    assert_eq!(prior["status"],"invalidated");
    assert_eq!(prior["completion"]["valid"],false);
    assert_eq!(prior["completion"]["summary"],"Checked the path");
    assert_eq!(prior["completion"]["invalidation"]["reason"],"resource_contamination");
    assert_eq!(prior["completion"]["invalidation"]["resource"],"db:historical");
    assert_ne!(d.call("swarm.jobs",json!({"id":first}))["jobs"][0]["status"],"accepted");
    assert_ne!(d.call("swarm.jobs",json!({"id":second}))["jobs"][0]["status"],"accepted");
    d.kill9();
    d.spawn();
    let replay=d.call("swarm.get",json!({"id":first}));
    assert_eq!(replay["status"],"invalidated");
    assert_eq!(replay["completion"]["valid"],false);
    assert_eq!(d.call("swarm.director.claim_batch",json!({"run_id":first,
        "generation":1,"revision":1,"now_ms":now()+7000}))["status"],"halted");
    assert!(d.try_call("swarm.stop",json!({"run_id":first})).is_err());
    assert!(d.try_call("swarm.complete",completion).is_err());
}

#[test]
fn accepted_but_active_worker_can_report_late_shared_resource_use() {
    let d=Daemon::start(&[]);
    let first=setup(&d,"Accepted late A",1);
    let second=setup(&d,"Accepted late B",1);
    let at=now();
    let a=admit(&d,&first,"j0","codex-a","accepted-a",at,1000000,100).unwrap();
    let b=admit(&d,&second,"j0","codex-a","accepted-b",at,1000000,100).unwrap();
    d.call("swarm.artifact.put",json!({"run_id":first,"job_id":"j0",
        "attempt_id":a["attempt_id"],"token":a["token"],
        "artifact_id":"accepted-evidence","source_revision":1,
        "kind":"finding","content":"checked"}));
    d.call("swarm.report",json!({"run_id":first,"job_id":"j0",
        "attempt_id":a["attempt_id"],"token":a["token"],
        "message_id":"accepted-result","type":"result","revision":1,
        "payload":{"artifact_ids":["accepted-evidence"],"audit_outcome":"negative"}}));
    d.call("swarm.decide",json!({"run_id":first,"generation":1,"revision":1,
        "job_id":"j0","decision":"accept","evidence":["accepted-evidence"]}));
    let observed=|run:&str,attempt:&Value|json!({"run_id":run,"job_id":"j0",
        "attempt_id":attempt["attempt_id"],"token":attempt["token"],
        "generation":1,"revision":1,"resource":"db:accepted-late","mode":"write",
        "after_use":true});
    d.call("swarm.claim",observed(&second,&b));
    assert_eq!(d.call("swarm.claim",observed(&first,&a))["status"],"contaminated");
    assert_eq!(d.call("swarm.coverage",json!({"run_id":first}))["rows"][0]["coverage_state"],
        "contaminated");
    assert_ne!(d.call("swarm.jobs",json!({"id":first}))["jobs"][0]["status"],"accepted");
}

#[test]
fn run_percentage_overrides_change_frozen_allocation_and_finishing_reserve() {
    let d=Daemon::start(&[]);
    let run=d.call("swarm.create",json!({"category":"Budget override","objective":"Audit",
        "allowed_targets":["codex-a"],
        "policy":{"run_allocation_percent":20,"finishing_reserve_percent":30}}));
    let id=run["id"].as_str().unwrap();
    d.call("swarm.plan",json!({"id":id,"generation":1,"revision":0,"jobs":[
        {"id":"first","title":"First","acceptance":"evidence","deps":[]},
        {"id":"second","title":"Second","acceptance":"evidence","deps":[]}
    ]}));
    let at=now();
    let request=|job:&str,estimate:i64|json!({"run_id":id,"generation":1,"revision":1,
        "job_id":job,"target_id":"codex-a","request_id":job,"snapshot":snapshot(at,60000),
        "now_ms":at,"required_capabilities":["code"],
        "estimate_milli":{"points":estimate},"purpose":"worker"});
    let first=d.call("swarm.admit",request("first",8000));
    assert_eq!(first["status"],"admitted");
    assert_eq!(first["allocation_milli"],12000);
    let second=d.call("swarm.admit",request("second",500));
    assert_eq!(second["reason"],"finishing_reserve");
    let db=rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let saved:(i64,i64)=db.query_row("SELECT allocation_milli,reserve_milli FROM swarm_allocations WHERE run_id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?))).unwrap();
    assert_eq!(saved,(12000,3600));
}

#[test]
fn worker_cannot_claim_finishing_purpose_to_spend_the_completion_reserve() {
    let mut d=Daemon::start(&[]);
    let run=d.call("swarm.create",json!({"category":"Protected synthesis budget",
        "objective":"Audit and synthesize","allowed_targets":["codex-a"]}));
    let id=run["id"].as_str().unwrap();
    d.call("swarm.plan",json!({"id":id,"generation":1,"revision":0,"jobs":[
        {"id":"seed","title":"Initial inspection","acceptance":"evidence","deps":[]},
        {"id":"probe","title":"Probe routes","acceptance":"evidence","deps":[]},
        {"id":"extra","title":"Extra probe","acceptance":"evidence","deps":[]},
        {"id":"synthesis","title":"Synthesize findings","acceptance":"report",
            "budget_role":"finishing","deps":[]}
    ]}));
    commit_beneficial_batch(&d,id,&["seed".into(),"probe".into(),"extra".into(),"synthesis".into()]);
    let at=now();
    let request=|job:&str,purpose:&str,estimate:i64|json!({"run_id":id,
        "generation":1,"revision":1,"job_id":job,"target_id":"codex-a",
        "request_id":format!("{job}-{purpose}"),"snapshot":snapshot(at,1000000),
        "now_ms":at,"required_capabilities":["code"],
        "estimate_milli":{"points":estimate},"purpose":purpose});
    let seed=d.call("swarm.admit",request("seed","worker",10000));
    assert_eq!(seed["status"],"admitted","{seed}");
    let probe=d.call("swarm.admit",request("probe","worker",60000));
    assert_eq!(probe["status"],"admitted","{probe}");
    let normal=d.call("swarm.admit",request("extra","worker",11000));
    assert_eq!(normal["reason"],"finishing_reserve","{normal}");
    let forged=d.call("swarm.admit",request("extra","finishing",20000));
    assert_eq!(forged["reason"],"finishing_purpose_not_authorized","{forged}");
    d.kill9(); d.spawn();
    let synthesis=d.call("swarm.admit",request("synthesis","finishing",20000));
    assert_eq!(synthesis["status"],"admitted","{synthesis}");
    assert_eq!(synthesis["allocation_milli"],100000);

    let other_d=Daemon::start(&[]);
    let other=setup(&other_d,"Larger finishing estimate",2);
    commit_beneficial_batch(&other_d,&other,&["j0".into(),"j1".into()]);
    let mut first=request("seed","worker",10000);
    first["run_id"]=json!(other);
    first["job_id"]=json!("j0");
    first["request_id"]=json!("estimate-35-seed");
    first["finishing_estimate_milli"]=json!({"points":35000});
    let admitted=other_d.call("swarm.admit",first);
    assert_eq!(admitted["status"],"admitted","{admitted}");
    assert_eq!(admitted["allocation_milli"],100000);
    let mut second=request("probe","worker",60000);
    second["run_id"]=json!(other);
    second["job_id"]=json!("j1");
    second["request_id"]=json!("estimate-35-worker");
    let held=other_d.call("swarm.admit",second);
    assert_eq!(held["reason"],"finishing_reserve","{held}");
}

#[test]
fn explicit_run_deadline_extension_survives_restart_without_new_account_allocation() {
    let mut d = Daemon::start(&[]);
    let run = d.call("swarm.create",json!({"category":"Extended audit","objective":"Audit",
        "allowed_targets":["codex-a"],"policy":{"deadline_ms":60000}}));
    let id = run["id"].as_str().unwrap();
    let created = run["created_ms"].as_i64().unwrap();
    d.call("swarm.plan",json!({"id":id,"generation":1,"revision":0,"jobs":[
        {"id":"first","title":"First","acceptance":"evidence","deps":[]},
        {"id":"second","title":"Second","acceptance":"evidence","deps":[]},
        {"id":"third","title":"Third","acceptance":"evidence","deps":[]}
    ]}));
    assert_eq!(admit(&d,id,"first","codex-a","before-extension",now(),60000,100)
        .unwrap()["status"],"admitted");
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let before: (i64,i64) = db.query_row(
        "SELECT allocation_milli,reserve_milli FROM swarm_allocations WHERE run_id=?1",
        [id],|r|Ok((r.get(0)?,r.get(1)?))).unwrap();
    let request = json!({"run_id":id,"request_id":"owner-extension-1",
        "expected_deadline_at_ms":created+60000,"additional_ms":60000});
    let extended = d.call("swarm.deadline.extend",request.clone());
    assert_eq!(extended["deadline_at_ms"],created+120000);
    assert_eq!(extended["duplicate"],false);
    assert_eq!(d.call("swarm.deadline.extend",request.clone())["duplicate"],true);
    let mut changed = request.clone();
    changed["additional_ms"] = json!(30000);
    assert!(d.try_call("swarm.deadline.extend",changed).unwrap_err()
        .contains("request id reused"));
    assert!(d.try_call("swarm.deadline.extend",json!({"run_id":id,
        "request_id":"stale-extension","expected_deadline_at_ms":created+60000,
        "additional_ms":1000})).unwrap_err().contains("stale deadline"));
    d.kill9();
    d.spawn();
    let persisted = d.call("swarm.get",json!({"id":id}));
    assert_eq!(persisted["policy"]["effective"]["deadline_ms"],120000);
    let event: (i64,i64) = db.query_row(
        "SELECT old_deadline_at_ms,new_deadline_at_ms FROM swarm_deadline_extensions WHERE run_id=?1",
        [id],|r|Ok((r.get(0)?,r.get(1)?))).unwrap();
    assert_eq!(event,(created+60000,created+120000));
    let after_old = admit(&d,id,"second","codex-a","after-old-deadline",
        created+60001,60000,100).unwrap();
    assert_ne!(after_old["reason"],"run_deadline","{after_old}");
    assert_ne!(d.call("swarm.get",json!({"id":id}))["stop_reason"],"deadline");
    let after: (i64,i64) = db.query_row(
        "SELECT allocation_milli,reserve_milli FROM swarm_allocations WHERE run_id=?1",
        [id],|r|Ok((r.get(0)?,r.get(1)?))).unwrap();
    assert_eq!(after,before,"time extension must not mint quota allocation");
    let expired = admit(&d,id,"third","codex-a","after-new-deadline",
        created+120000,60000,100).unwrap();
    assert_eq!(expired["reason"],"run_deadline");
    assert_eq!(d.call("swarm.get",json!({"id":id}))["stop_reason"],"deadline");
    assert!(d.try_call("swarm.deadline.extend",json!({"run_id":id,
        "request_id":"too-late","expected_deadline_at_ms":created+120000,
        "additional_ms":60000})).unwrap_err().contains("cannot extend"));
    assert_eq!(d.call("swarm.deadline.extend",request)["duplicate"],true,
        "replaying the earlier request must not add more time after Stop");
}

#[test]
fn default_worker_ceiling_and_four_per_wave_are_admission_bounds() {
    let d = Daemon::start(&[]);
    let id = setup(&d, "Scale admission", 9);
    commit_beneficial_batch(&d, &id, &(0..8).map(|n| format!("j{n}")).collect::<Vec<_>>());
    let at = now();
    for n in 0..4 {
        assert_eq!(
            admit(
                &d,
                &id,
                &format!("j{n}"),
                "codex-a",
                &format!("r{n}"),
                at,
                1000000,
                100
            )
            .unwrap()["status"],
            "admitted"
        );
    }
    assert_eq!(
        admit(&d, &id, "j4", "codex-a", "r4", at, 1000000, 100).unwrap()["reason"],
        "growth_wave_full"
    );
    for n in 4..8 {
        assert_eq!(
            admit(
                &d,
                &id,
                &format!("j{n}"),
                "codex-a",
                &format!("r{n}"),
                at + 5000,
                1000000,
                100
            )
            .unwrap()["status"],
            "admitted"
        );
    }
    assert_eq!(
        admit(&d, &id, "j8", "codex-a", "r8", at + 10000, 1000000, 100).unwrap()["reason"],
        "worker_limit"
    );
}

#[test]
fn concurrent_requests_cannot_both_claim_one_run_allocation() {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixStream;
    use std::sync::{Arc, Barrier};
    let d = Daemon::start(&[]);
    let id = setup(&d, "Concurrent admission", 2);
    let at = now();
    let path = d.socket();
    let barrier = Arc::new(Barrier::new(3));
    let mut handles = Vec::new();
    for n in 0..2 {
        let path = path.clone();
        let barrier = barrier.clone();
        let run = id.clone();
        handles.push(std::thread::spawn(move || {
            let mut conn=UnixStream::connect(path).unwrap();
            conn.set_read_timeout(Some(std::time::Duration::from_secs(10))).unwrap();
            let params=json!({"run_id":run,"generation":1,"revision":1,"job_id":format!("j{n}"),
                "target_id":if n==0 {"codex-a"} else {"opencode-a"},"request_id":format!("parallel-{n}"),
                "snapshot":snapshot(at,60000),"now_ms":at,"required_capabilities":["code"],
                "estimate_milli":{"points":4000},"purpose":"worker"});
            barrier.wait();
            conn.write_all(format!("{}\n",json!({"id":n,"method":"swarm.admit","params":params})).as_bytes()).unwrap();
            let mut line=String::new();
            BufReader::new(conn).read_line(&mut line).unwrap();
            let response: Value=serde_json::from_str(&line).unwrap();
            response["result"].clone()
        }));
    }
    barrier.wait();
    let results: Vec<Value> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    assert_eq!(
        results.iter().filter(|r| r["status"] == "admitted").count(),
        1
    );
    assert_eq!(
        results.iter().filter(|r| r["status"] == "blocked").count(),
        1
    );
}

#[test]
fn review_backlog_holds_admissions_until_it_drains_below_four() {
    let d = Daemon::start(&[]);
    d.call("agents.limit.set", json!({"max_active":33}));
    let run = d.call(
        "swarm.create",
        json!({"category":"Review pressure","objective":"Audit many checks",
        "allowed_targets":["codex-a"],"policy":{"max_workers":32}}),
    );
    let id = run["id"].as_str().unwrap();
    let jobs: Vec<_>=(0..10).map(|n|json!({"id":format!("j{n}"),"title":format!("J{n}"),"acceptance":"evidence","deps":[]})).collect();
    d.call(
        "swarm.plan",
        json!({"id":id,"generation":1,"revision":0,"jobs":jobs}),
    );
    commit_beneficial_batch(&d, id, &(0..10).map(|n| format!("j{n}")).collect::<Vec<_>>());
    let at = now();
    let mut attempts = Vec::new();
    for n in 0..8 {
        let result = admit(
            &d,
            id,
            &format!("j{n}"),
            "codex-a",
            &format!("review-{n}"),
            at + (n / 4) as i64 * 5000,
            1000000,
            100,
        )
        .unwrap();
        assert_eq!(result["status"], "admitted");
        let aid = result["attempt_id"].as_str().unwrap().to_owned();
        let token = result["token"].as_str().unwrap().to_owned();
        let artifact = format!("artifact-{n}");
        d.call(
            "swarm.artifact.put",
            json!({"run_id":id,"job_id":format!("j{n}"),"attempt_id":aid,"token":token,
            "artifact_id":artifact,"source_revision":1,"kind":"finding","content":"checked"}),
        );
        d.call("swarm.report",json!({"run_id":id,"job_id":format!("j{n}"),"attempt_id":aid,"token":token,
            "message_id":format!("result-{n}"),"type":"result","revision":1,"payload":{"artifact_ids":[artifact]}}));
        attempts.push((aid, artifact));
    }
    let held = admit(
        &d,
        id,
        "j8",
        "codex-a",
        "review-8",
        at + 10000,
        1000000,
        100,
    )
    .unwrap();
    assert_eq!(held["reason"], "review_backlog");
    for (n, (aid, artifact)) in attempts.iter().take(5).enumerate() {
        d.call("swarm.decide",json!({"run_id":id,"generation":1,"revision":1,"job_id":format!("j{n}"),"decision":"accept","evidence":[artifact]}));
        d.call("swarm.attempt.confirm_exit",json!({"run_id":id,"generation":1,"revision":1,"job_id":format!("j{n}"),"attempt_id":aid}));
    }
    assert_eq!(
        admit(
            &d,
            id,
            "j8",
            "codex-a",
            "review-8",
            at + 10000,
            1000000,
            100
        )
        .unwrap()["status"],
        "admitted"
    );
}

#[test]
fn already_admitted_results_can_overflow_review_threshold_without_loss() {
    let mut d = Daemon::start(&[]);
    d.call("agents.limit.set", json!({"max_active":33}));
    let run = d.call("swarm.create", json!({"category":"Review overflow",
        "objective":"Audit many checks","allowed_targets":["codex-a"],
        "policy":{"max_workers":32}}));
    let id = run["id"].as_str().unwrap();
    let jobs: Vec<_> = (0..11).map(|n| json!({"id":format!("j{n}"),
        "title":format!("J{n}"),"acceptance":"evidence","deps":[]})).collect();
    d.call("swarm.plan",json!({"id":id,"generation":1,"revision":0,"jobs":jobs}));
    commit_beneficial_batch(&d, id, &(0..10).map(|n| format!("j{n}")).collect::<Vec<_>>());
    let at = now();
    let mut attempts = Vec::new();
    for n in 0..10 {
        let admitted = admit(&d,id,&format!("j{n}"),"codex-a",
            &format!("overflow-{n}"),at+(n/4) as i64*5000,1000000,100).unwrap();
        assert_eq!(admitted["status"],"admitted","job {n}: {admitted}");
        attempts.push(admitted);
    }
    for (n, admitted) in attempts.iter().enumerate() {
        let artifact = format!("overflow-artifact-{n}");
        d.call("swarm.artifact.put",json!({"run_id":id,"job_id":format!("j{n}"),
            "attempt_id":admitted["attempt_id"],"token":admitted["token"],
            "artifact_id":artifact,"source_revision":1,"kind":"finding","content":"checked"}));
        let report = d.call("swarm.report",json!({"run_id":id,"job_id":format!("j{n}"),
            "attempt_id":admitted["attempt_id"],"token":admitted["token"],
            "message_id":format!("overflow-result-{n}"),"type":"result","revision":1,
            "payload":{"artifact_ids":[artifact]}}));
        assert_eq!(report["duplicate"],false,"result {n}: {report}");
    }
    let jobs = d.call("swarm.jobs",json!({"id":id}))["jobs"].as_array().unwrap().clone();
    assert_eq!(jobs.iter().filter(|job| job["status"]=="submitted").count(),10);
    assert_eq!(admit(&d,id,"j10","codex-a","overflow-10",at+15000,1000000,100)
        .unwrap()["reason"],"review_backlog");
    d.kill9();
    d.spawn();
    let jobs = d.call("swarm.jobs",json!({"id":id}))["jobs"].as_array().unwrap().clone();
    assert_eq!(jobs.iter().filter(|job| job["status"]=="submitted").count(),10);
    let messages = d.call("swarm.messages",json!({"run_id":id,"recipient":"director","limit":100}));
    assert_eq!(messages["messages"].as_array().unwrap().iter()
        .filter(|message| message["type"]=="result").count(),10);
}

#[test]
fn explicit_ceiling_admits_thirty_two_fixture_workers_without_hidden_eight_cap() {
    let d = Daemon::start(&[]);
    d.call("agents.limit.set", json!({"max_active":33}));
    let run = d.call(
        "swarm.create",
        json!({"category":"Large qualification","objective":"Inspect modules",
        "allowed_targets":["codex-a"],"policy":{"max_workers":32}}),
    );
    let id = run["id"].as_str().unwrap();
    let jobs: Vec<_>=(0..33).map(|n|json!({"id":format!("j{n}"),"title":format!("J{n}"),"acceptance":"evidence","deps":[]})).collect();
    d.call(
        "swarm.plan",
        json!({"id":id,"generation":1,"revision":0,"jobs":jobs}),
    );
    commit_beneficial_batch(&d, id, &(0..32).map(|n| format!("j{n}")).collect::<Vec<_>>());
    let at = now();
    for n in 0..32 {
        let result = admit(
            &d,
            id,
            &format!("j{n}"),
            "codex-a",
            &format!("large-{n}"),
            at + (n / 4) as i64 * 5000,
            1000000,
            100,
        )
        .unwrap();
        assert_eq!(result["status"], "admitted", "worker {n}: {result}");
    }
    assert_eq!(
        admit(
            &d,
            id,
            "j32",
            "codex-a",
            "large-32",
            at + 40000,
            1000000,
            100
        )
        .unwrap()["reason"],
        "worker_limit"
    );
}

#[test]
fn planned_write_claim_cannot_be_omitted_at_admission() {
    let d = Daemon::start(&[]);
    let planned = |category: &str| {
        let run = d.call("swarm.create", json!({"category":category,"objective":"Audit",
            "allowed_targets":["codex-a"]}));
        let id = run["id"].as_str().unwrap().to_string();
        d.call("swarm.plan", json!({"id":id,"generation":1,"revision":0,
            "jobs":[{"id":"j0","title":"Inspect","acceptance":"evidence","deps":[],
                "resource_claims":[{"resource":"db:tenant-fixture","mode":"write"}]}]}));
        id
    };
    let first = planned("Plan claims A");
    let second = planned("Plan claims B");
    let at = now();
    let downgraded = json!({"run_id":first,"generation":1,"revision":1,
        "job_id":"j0","target_id":"codex-a","request_id":"downgraded",
        "snapshot":snapshot(at,1_000_000),"now_ms":at,
        "required_capabilities":["code"],"estimate_milli":{"points":100},
        "purpose":"worker","resource_claims":[{"resource":"db:tenant-fixture","mode":"read"}]});
    assert!(d.try_call("swarm.admit",downgraded).unwrap_err().contains("cannot change a planned resource claim"));
    assert_eq!(admit(&d,&first,"j0","codex-a","first",at,1_000_000,100)
        .unwrap()["status"],"admitted");
    let held = admit(&d,&second,"j0","codex-a","second",at,1_000_000,100).unwrap();
    assert_eq!(held["status"],"blocked");
    assert_eq!(held["reason"],"resource_conflict");
}

#[test]
fn hundred_jobs_cycle_through_thirty_two_slots_and_accept_once() {
    let d = Daemon::start(&[]);
    d.call("agents.limit.set", json!({"max_active":33}));
    let run = d.call("swarm.create", json!({"category":"Hundred job qualification",
        "objective":"Audit one hundred independent paths","allowed_targets":["codex-a"],
        "policy":{"max_workers":32}}));
    let id = run["id"].as_str().unwrap();
    let jobs: Vec<_> = (0..100).map(|n| json!({"id":format!("j{n:03}"),
        "title":format!("Inspect path {n}"),"acceptance":"evidence","deps":[]})).collect();
    d.call("swarm.plan",json!({"id":id,"generation":1,"revision":0,"jobs":jobs}));
    let at=now();
    for start in (0..100).step_by(32) {
        let end=(start+32).min(100);
        let batch:Vec<String>=(start..end).map(|n|format!("j{n:03}")).collect();
        let benefit=commit_beneficial_batch(&d,id,&batch);
        assert_eq!(benefit["max_parallel_workers"],(end-start) as i64);
        let mut admitted=Vec::new();
        for n in start..end {
            let job=format!("j{n:03}");
            let result=admit(&d,id,&job,"codex-a",&format!("hundred-{n}"),
                at+(n/4) as i64*5000,1_000_000,100).unwrap();
            assert_eq!(result["status"],"admitted","{job}: {result}");
            admitted.push((job,result));
        }
        if start==0 {
            let held=admit(&d,id,"j032","codex-a","overflow-32",
                at+40_000,1_000_000,100).unwrap();
            assert_eq!(held["reason"],"worker_limit");
            let db=rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
            let active:i64=db.query_row("SELECT COUNT(*) FROM swarm_attempts WHERE run_id=?1 AND status='registered'",[id],|r|r.get(0)).unwrap();
            assert_eq!(active,32);
            assert_eq!(d.call("swarm.get",json!({"id":id}))["status"],"running");
        }
        for (job,result) in admitted {
            let artifact=format!("checked-{job}");
            d.call("swarm.artifact.put",json!({"run_id":id,"job_id":job,
                "attempt_id":result["attempt_id"],"token":result["token"],
                "artifact_id":artifact,"source_revision":1,"kind":"finding","content":"checked"}));
            d.call("swarm.report",json!({"run_id":id,"job_id":job,
                "attempt_id":result["attempt_id"],"token":result["token"],
                "message_id":format!("result-{job}"),"type":"result","revision":1,
                "payload":{"artifact_ids":[artifact]}}));
            d.call("swarm.decide",json!({"run_id":id,"generation":1,"revision":1,
                "job_id":job,"decision":"accept","evidence":[artifact]}));
            d.call("swarm.attempt.confirm_exit",json!({"run_id":id,"generation":1,
                "revision":1,"job_id":job,"attempt_id":result["attempt_id"]}));
        }
    }
    let db=rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let counts:(i64,i64,i64)=db.query_row("SELECT COUNT(*),
        SUM(CASE WHEN status='accepted' AND attempt_count=1 THEN 1 ELSE 0 END),
        COUNT(DISTINCT id) FROM swarm_jobs WHERE run_id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap();
    assert_eq!(counts,(100,100,100));
    let accepted:i64=db.query_row("SELECT COUNT(*) FROM swarm_decisions WHERE run_id=?1 AND decision='accept'",[id],|r|r.get(0)).unwrap();
    assert_eq!(accepted,100);
    assert_eq!(admit(&d,id,"j000","codex-a","hundred-0",at,1_000_000,100)
        .unwrap()["status"],"already_admitted");
}

#[test]
fn quota_headroom_explains_smaller_pool_than_worker_ceiling() {
    let d=Daemon::start(&[]);
    d.call("agents.limit.set", json!({"max_active":33}));
    let run=d.call("swarm.create",json!({"category":"Quota-constrained pool",
        "objective":"Inspect three paths","allowed_targets":["codex-a"],
        "policy":{"max_workers":32}}));
    let id=run["id"].as_str().unwrap();
    let jobs:Vec<_>=(0..3).map(|n|json!({"id":format!("j{n}"),
        "title":format!("Inspect {n}"),"acceptance":"evidence","deps":[]})).collect();
    d.call("swarm.plan",json!({"id":id,"generation":1,"revision":0,"jobs":jobs}));
    commit_beneficial_batch(&d,id,&["j0".into(),"j1".into(),"j2".into()]);
    let at=now();
    for n in 0..2 {
        assert_eq!(admit(&d,id,&format!("j{n}"),"codex-a",
            &format!("small-{n}"),at,3000,100).unwrap()["status"],"admitted");
    }
    let third=admit(&d,id,"j2","codex-a","small-2",at,3000,100).unwrap();
    assert_eq!(third["status"],"blocked");
    assert_eq!(third["reason"],"finishing_reserve");
}
