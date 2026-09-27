mod common;

use common::*;
use serde_json::json;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

#[test]
fn running_director_can_dispatch_first_worker_into_second_app_slot() {
    let d = Daemon::start(&[]);
    d.call("agents.limit.set", json!({"max_active":2}));
    let temp = tmp();
    let checkout = repo(&temp.path().join("director-first-source"));
    let swarm = d.call("swarm.create", json!({"category":"Director-first dispatch",
        "objective":"Audit the backend","allowed_targets":["fixture"]}));
    let id = swarm["id"].as_str().unwrap();
    d.call("swarm.plan", json!({"id":id,"generation":1,"revision":0,"jobs":[
        {"id":"inspect","title":"Inspect backend","acceptance":"Evidence","deps":[]}
    ]}));
    let launched = d.call("swarm.director.launch", json!({"run_id":id,
        "generation":1,"repo":checkout,"program":"/bin/sleep","args":["30"],
        "prompt":"Direct the audit","title":"Director-first process"}));
    assert_eq!(launched["status"], "launched", "{launched}");
    let director = launched["overseer_run_id"].as_str().unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    while d.run(director)["status"] != "running" {
        assert!(Instant::now() < deadline, "director did not start");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(d.call("swarm.get",json!({"id":id}))["status"], "planning");
    assert_eq!(d.call("agents.limit.get",json!({}))["active"], 1);

    let at = now();
    let dispatched = d.call("swarm.dispatch.next", json!({"request_id":"director-first-worker",
        "target_id":"fixture","repo":checkout,"program":"/bin/sleep","args":["30"],
        "now_ms":at,"snapshot":{"version":1,"observed_ms":at-1000,
            "expires_ms":at+60000,"targets":[{"id":"fixture","account_id":"fixture",
                "pool_ids":["pool"],"capabilities":["code"],"health":"up","auth":"ok"}],
            "pools":[{"id":"pool","windows":[{"id":"run","unit":"points",
                "remaining_milli":100000,"protected_milli":0,"reserved_milli":0,
                "confidence":"exact","expires_ms":at+60000}]}]},
        "required_capabilities":["code"],"estimate_milli":{"points":100},
        "purpose":"worker"}));
    let active_after_dispatch = d.call("agents.limit.get",json!({}))["active"].clone();
    let third_refused = if dispatched["status"] == "launched" {
        d.try_call("task.create", json!({"repo":checkout,"harness":"generic",
            "workspace_mode":"worktree","program":"/bin/sleep","args":["30"],
            "prompt":"","title":"third agent"})).unwrap_err().contains("agent limit reached")
    } else { false };
    d.call("swarm.stop", json!({"run_id":id}));
    d.wait_done(director, 8);
    if let Some(worker) = dispatched["overseer_run_id"].as_str() {
        d.wait_done(worker, 8);
    }

    assert_eq!(dispatched["status"], "launched", "{dispatched}");
    assert_eq!(active_after_dispatch, 2);
    assert!(third_refused, "a third process must not exceed the two-slot cap");
}

#[test]
fn dispatch_skips_category_outside_requested_repository_before_reserving_attempt() {
    let d = Daemon::start(&[]);
    d.call("agents.limit.set", json!({"max_active":4}));
    let temp = tmp();
    let source_a = repo(&temp.path().join("category-a-source"));
    let source_b = repo(&temp.path().join("category-b-source"));
    let source_other = repo(&temp.path().join("unapproved-source"));
    let make_run = |category: &str, source: &std::path::Path, jobs: usize| {
        let created = d.call("swarm.create", json!({"category":category,
            "objective":"Audit this source","allowed_targets":["fixture"],
            "repositories":[source]}));
        let id = created["id"].as_str().unwrap().to_string();
        let backlog: Vec<_> = (0..jobs).map(|n| json!({"id":format!("j{n:03}"),
            "title":format!("Inspect {n}"),"acceptance":"Evidence","deps":[]})).collect();
        d.call("swarm.plan",json!({"id":id,"generation":1,"revision":0,"jobs":backlog}));
        let director = d.call("swarm.director.launch",json!({"run_id":id,
            "generation":1,"repo":source,"program":"/bin/sleep","args":["30"],
            "prompt":"Direct this category","title":format!("{category} director")}));
        assert_eq!(director["status"],"launched","{director}");
        (id,director["overseer_run_id"].as_str().unwrap().to_string())
    };
    let (long, long_director) = make_run("A long category", &source_a, 100);
    let (short, short_director) = make_run("B short category", &source_b, 2);
    let at = now();
    let request = |id: &str, source: &std::path::Path| json!({
        "request_id":id,"target_id":"fixture","repo":source,
        "program":"/bin/sleep","args":["30"],"now_ms":at,
        "snapshot":{"version":1,"observed_ms":at-1000,"expires_ms":at+60000,
            "targets":[{"id":"fixture","account_id":"shared","pool_ids":["pool"],
                "capabilities":["code"],"health":"up","auth":"ok"}],
            "pools":[{"id":"pool","windows":[{"id":"run","unit":"points",
                "remaining_milli":1000000,"protected_milli":0,"reserved_milli":0,
                "confidence":"exact","expires_ms":at+60000}]}]},
        "required_capabilities":["code"],"estimate_milli":{"points":100},
        "purpose":"worker"});
    let unapproved = d.call("swarm.dispatch.next",request("scoped-other",&source_other));
    let attempts_after_unapproved: i64 = rusqlite::Connection::open(
        d.home.path().join("overseer.sqlite")).unwrap().query_row(
        "SELECT COUNT(*) FROM swarm_attempts",[],|r|r.get(0)).unwrap();
    // B's repository is requested first even though A sorts first and has 100 jobs.
    let first_request = request("scoped-b", &source_b);
    let first = d.try_call("swarm.dispatch.next", first_request.clone());
    let long_attempts_after_first: i64 = rusqlite::Connection::open(
        d.home.path().join("overseer.sqlite")).unwrap().query_row(
        "SELECT COUNT(*) FROM swarm_attempts WHERE run_id=?1",[&long],|r|r.get(0)).unwrap();
    let second = if first.as_ref().is_ok_and(|result| result["status"] == "launched") {
        Some(d.try_call("swarm.dispatch.next",request("scoped-a", &source_a)))
    } else { None };
    let active = d.call("agents.limit.get",json!({}))["active"].clone();
    let replay = if second.as_ref().is_some_and(|result| result.is_ok()) {
        Some(d.try_call("swarm.dispatch.next", first_request))
    } else { None };
    let third_refused = if active == 4 {
        d.try_call("task.create",json!({"repo":source_a,"harness":"generic",
            "workspace_mode":"worktree","program":"/bin/sleep","args":["30"],
            "prompt":"","title":"fifth agent"})).unwrap_err().contains("agent limit reached")
    } else { false };
    d.call("swarm.stop",json!({"run_id":long}));
    d.call("swarm.stop",json!({"run_id":short}));
    d.wait_done(&long_director,8);
    d.wait_done(&short_director,8);
    for result in std::iter::once(&first).chain(second.iter()) {
        if let Ok(launched) = result {
            if let Some(worker) = launched["overseer_run_id"].as_str() {
                d.wait_done(worker,8);
            }
        }
    }

    let first = first.expect("B's approved source must be dispatchable");
    assert_eq!(unapproved["status"],"blocked","{unapproved}");
    assert_eq!(unapproved["reason"],"all_categories_blocked");
    assert_eq!(attempts_after_unapproved,0);
    assert_eq!(first["status"],"launched","{first}");
    assert_eq!(first["run_id"],short);
    assert_eq!(long_attempts_after_first,0,"A must not consume an attempt for B's source");
    let second = second.unwrap().expect("A's approved source must be dispatchable");
    assert_eq!(second["status"],"launched","{second}");
    assert_eq!(second["run_id"],long);
    assert_eq!(active,4);
    assert_eq!(replay.unwrap().unwrap()["overseer_run_id"],first["overseer_run_id"]);
    assert!(third_refused,"a fifth process must not exceed the four-slot cap");
}

#[test]
fn worker_spawn_record_failure_keeps_one_attempt_and_reattaches_after_restart() {
    let mut d = Daemon::start(&[]);
    let temp = tmp();
    let checkout = repo(&temp.path().join("orphan-worker-source"));
    let swarm = d.call("swarm.create", json!({"category":"Worker spawn window",
        "objective":"Inspect backend","allowed_targets":["fixture"]}));
    let id = swarm["id"].as_str().unwrap();
    d.call("swarm.plan",json!({"id":id,"generation":1,"revision":0,"jobs":[
        {"id":"inspect","title":"Orphan worker","acceptance":"Evidence","deps":[]}
    ]}));
    let at = now();
    let request = json!({"request_id":"orphan-worker","target_id":"fixture",
        "repo":checkout,"program":"/bin/sleep","args":["30"],"now_ms":at,
        "snapshot":{"version":1,"observed_ms":at-1000,"expires_ms":at+60000,
            "targets":[{"id":"fixture","account_id":"fixture","pool_ids":["pool"],
                "capabilities":["code"],"health":"up","auth":"ok"}],
            "pools":[{"id":"pool","windows":[{"id":"run","unit":"points",
                "remaining_milli":100000,"protected_milli":0,"reserved_milli":0,
                "confidence":"exact","expires_ms":at+60000}]}]},
        "required_capabilities":["code"],"estimate_milli":{"points":100},
        "purpose":"worker"});
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    db.execute_batch("CREATE TRIGGER abort_worker_process_record BEFORE UPDATE OF run_dir ON runs
        WHEN NEW.title='Orphan worker' BEGIN
        SELECT RAISE(ABORT, 'fixture orphaned worker supervisor'); END;").unwrap();
    let result = d.call("swarm.dispatch.next", request.clone());
    assert_eq!(result["status"],"launch_uncertain","{result}");
    let worker = result["overseer_run_id"].as_str().unwrap();
    assert_eq!(d.run(worker)["status"],"queued");
    let uncertain_replay=d.call("swarm.dispatch.next",request.clone());
    assert_eq!(uncertain_replay["status"],"launch_uncertain");
    assert_eq!(uncertain_replay["overseer_run_id"],worker);
    assert_eq!(uncertain_replay["duplicate"],true);
    let (phase, linked): (String,String) = db.query_row(
        "SELECT l.launch_phase,l.overseer_run_id FROM swarm_worker_launches l WHERE l.attempt_id=?1",
        [result["attempt_id"].as_str().unwrap()], |r| Ok((r.get(0)?,r.get(1)?))).unwrap();
    assert_eq!(phase,"spawn_requested");
    assert_eq!(linked,worker);
    let run_dir=d.home.path().join("runs").join(worker).join("p1");
    let deadline=Instant::now()+Duration::from_secs(3);
    while !run_dir.join("shim.json").exists() {
        assert!(Instant::now()<deadline,"orphan worker did not start");
        std::thread::sleep(Duration::from_millis(20));
    }
    d.kill9();
    d.spawn();
    assert_eq!(d.run(worker)["status"],"queued",
        "continued storage failure must retain the uncertain run");
    let unrecorded: Option<String>=db.query_row("SELECT run_dir FROM runs WHERE id=?1",[worker],|r|r.get(0)).unwrap();
    assert!(unrecorded.is_none());
    db.execute_batch("DROP TRIGGER abort_worker_process_record;").unwrap();
    db.execute("UPDATE swarm_worker_launches SET launch_phase=NULL WHERE overseer_run_id=?1",
        [worker]).unwrap(); // A historical linked row has no recorded spawn phase.
    let launch_file=run_dir.join("launch.json");
    let original=std::fs::read(&launch_file).unwrap();
    let mut altered: serde_json::Value=serde_json::from_slice(&original).unwrap();
    altered["env"]["OVERSEER_SWARM_TOKEN"]=json!("wrong-private-credential");
    std::fs::write(&launch_file,serde_json::to_vec(&altered).unwrap()).unwrap();
    d.kill9();
    d.spawn();
    assert_eq!(d.run(worker)["status"],"queued");
    let untrusted: Option<String>=db.query_row("SELECT run_dir FROM runs WHERE id=?1",[worker],|r|r.get(0)).unwrap();
    assert!(untrusted.is_none(),"mismatched launch identity must stay reserved");
    d.kill9();
    std::fs::write(&launch_file,original).unwrap();
    d.spawn();
    let recovered=d.run(worker);
    assert!(matches!(recovered["status"].as_str(),Some("running"|"starting")),"{recovered}");
    let dir: String=db.query_row("SELECT run_dir FROM runs WHERE id=?1",[worker],|r|r.get(0)).unwrap();
    assert_eq!(dir,run_dir.to_string_lossy());
    let replay=d.call("swarm.dispatch.next",request);
    assert_eq!(replay["overseer_run_id"],worker);
    assert_eq!(replay["duplicate"],true);
    let count: i64=db.query_row("SELECT COUNT(*) FROM swarm_attempts WHERE run_id=?1",[id],|r|r.get(0)).unwrap();
    assert_eq!(count,1);
    d.call("run.interrupt",json!({"run_id":worker}));
    d.wait_done(worker,8);
}

#[test]
fn exited_orphan_worker_replays_completion_once() {
    let mut d=Daemon::start(&[]);
    let temp=tmp();
    let checkout=repo(&temp.path().join("exited-orphan-worker-source"));
    let swarm=d.call("swarm.create",json!({"category":"Exited worker window",
        "objective":"Inspect backend","allowed_targets":["fixture"]}));
    let id=swarm["id"].as_str().unwrap();
    d.call("swarm.plan",json!({"id":id,"generation":1,"revision":0,"jobs":[
        {"id":"inspect","title":"Exited orphan worker","acceptance":"Evidence","deps":[]}
    ]}));
    let at=now();
    let request=json!({"request_id":"exited-orphan-worker","target_id":"fixture",
        "repo":checkout,"program":"/usr/bin/true","args":[],"now_ms":at,
        "snapshot":{"version":1,"observed_ms":at-1000,"expires_ms":at+60000,
            "targets":[{"id":"fixture","account_id":"fixture","pool_ids":["pool"],
                "capabilities":["code"],"health":"up","auth":"ok"}],
            "pools":[{"id":"pool","windows":[{"id":"run","unit":"points",
                "remaining_milli":100000,"protected_milli":0,"reserved_milli":0,
                "confidence":"exact","expires_ms":at+60000}]}]},
        "required_capabilities":["code"],"estimate_milli":{"points":100},
        "purpose":"worker"});
    let db=rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    db.execute_batch("CREATE TRIGGER abort_exited_worker_record BEFORE UPDATE OF run_dir ON runs
        WHEN NEW.title='Exited orphan worker' BEGIN
        SELECT RAISE(ABORT, 'fixture exited orphan worker'); END;").unwrap();
    let result=d.call("swarm.dispatch.next",request.clone());
    assert_eq!(result["status"],"launch_uncertain","{result}");
    let worker=result["overseer_run_id"].as_str().unwrap();
    let exit=d.home.path().join("runs").join(worker).join("p1").join("exit.json");
    let deadline=Instant::now()+Duration::from_secs(3);
    while !exit.exists() {
        assert!(Instant::now()<deadline,"orphan worker did not exit");
        std::thread::sleep(Duration::from_millis(20));
    }
    db.execute_batch("DROP TRIGGER abort_exited_worker_record;").unwrap();
    d.kill9();
    d.spawn();
    assert_eq!(d.wait_done(worker,5)["status"],"completed");
    let first: i64=db.query_row("SELECT COUNT(*) FROM events WHERE run_id=?1 AND kind='status'
        AND json_extract(payload,'$.status')='completed'",[worker],|r|r.get(0)).unwrap();
    assert_eq!(first,1);
    d.kill9();
    d.spawn();
    let again: i64=db.query_row("SELECT COUNT(*) FROM events WHERE run_id=?1 AND kind='status'
        AND json_extract(payload,'$.status')='completed'",[worker],|r|r.get(0)).unwrap();
    assert_eq!(again,1);
    let replay=d.call("swarm.dispatch.next",request);
    assert_eq!(replay["overseer_run_id"],worker);
    assert_eq!(replay["duplicate"],true);
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
    commit_beneficial_batch(&d, run_id, &["db".into(), "routes".into(), "verify".into()]);
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
        "snapshot":{"version":1,"observed_ms":at-1000,"expires_ms":at+1500,
            "targets":[{"id":"fixture","account_id":"fixture","pool_ids":["pool"],
                "capabilities":["code"],"health":"up","auth":"ok"}],
            "pools":[{"id":"pool","windows":[{"id":"run","unit":"points",
                "remaining_milli":100000,"protected_milli":0,"reserved_milli":0,
                "confidence":"exact","expires_ms":at+1500}]}]},
        "required_capabilities":["code"],"estimate_milli":{"points":100},
        "purpose":"worker","inject_failure_after_admit_once":true});
    assert!(d.try_call("swarm.dispatch.next", request.clone()).is_err());
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    db.execute(
        "UPDATE swarm_runs SET allowed_targets='[]' WHERE id=?1",
        [run_id],
    )
    .unwrap();
    let permission_error=d.try_call("swarm.dispatch.next", request.clone()).unwrap_err();
    assert!(permission_error.contains("permission revoked"),"{permission_error}");
    db.execute(
        "UPDATE swarm_runs SET allowed_targets='[\"fixture\"]' WHERE id=?1",
        [run_id],
    )
    .unwrap();
    drop(db);
    std::thread::sleep(std::time::Duration::from_millis(1600));
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

#[test]
fn supervised_scripted_workers_report_evidence_that_unlocks_dependent_work() {
    let mut d = Daemon::start(&[]);
    let temp = tmp();
    let checkout = repo(&temp.path().join("worker-report-source"));
    let script = temp.path().join("report-worker.sh");
    std::fs::write(&script, r#"#!/bin/sh
set -eu
test -n "$OVERSEER_SWARM_RUN_ID"
test -n "$OVERSEER_SWARM_JOB_ID"
test -n "$OVERSEER_SWARM_ATTEMPT_ID"
test -n "$OVERSEER_SWARM_TOKEN"
test -n "$OVERSEER_SWARM_REVISION"
artifact="proof-$OVERSEER_SWARM_ATTEMPT_ID"
written=$("$OVERSEER_BIN" ctl swarm.artifact.put "{\"run_id\":\"$OVERSEER_SWARM_RUN_ID\",\"job_id\":\"$OVERSEER_SWARM_JOB_ID\",\"attempt_id\":\"$OVERSEER_SWARM_ATTEMPT_ID\",\"token\":\"$OVERSEER_SWARM_TOKEN\",\"artifact_id\":\"$artifact\",\"source_revision\":$OVERSEER_SWARM_REVISION,\"kind\":\"finding\",\"content\":\"scripted evidence\"}")
case "$written" in *'"error"'*) exit 2;; esac
reported=$("$OVERSEER_BIN" ctl swarm.report "{\"run_id\":\"$OVERSEER_SWARM_RUN_ID\",\"job_id\":\"$OVERSEER_SWARM_JOB_ID\",\"attempt_id\":\"$OVERSEER_SWARM_ATTEMPT_ID\",\"token\":\"$OVERSEER_SWARM_TOKEN\",\"message_id\":\"result-$OVERSEER_SWARM_ATTEMPT_ID\",\"type\":\"result\",\"revision\":$OVERSEER_SWARM_REVISION,\"payload\":{\"artifact_ids\":[\"$artifact\"]}}")
case "$reported" in *'"error"'*) exit 3;; esac
"#).unwrap();
    let run = d.call(
        "swarm.create",
        json!({"category":"Fixture endpoint work",
        "objective":"Set contract then verify endpoint","allowed_targets":["fixture"]}),
    );
    let run_id = run["id"].as_str().unwrap();
    d.call("swarm.plan",json!({"id":run_id,"generation":1,"revision":0,"jobs":[
        {"id":"contract","title":"Define endpoint contract","acceptance":"Contract evidence","deps":[]},
        {"id":"endpoint","title":"Verify endpoint","acceptance":"Endpoint evidence","deps":["contract"]}
    ]}));
    let at = now();
    let snapshot = json!({"version":1,"observed_ms":at-1000,"expires_ms":at+60000,
        "targets":[{"id":"fixture","account_id":"fixture","pool_ids":["pool"],
            "capabilities":["code"],"health":"up","auth":"ok"}],
        "pools":[{"id":"pool","windows":[{"id":"run","unit":"points",
            "remaining_milli":100000,"protected_milli":0,"reserved_milli":0,
            "confidence":"exact","expires_ms":at+60000}]}]});
    let mut checks = Vec::new();
    for (request_id, job_id) in [
        ("contract-launch", "contract"),
        ("endpoint-launch", "endpoint"),
    ] {
        let dispatched = d.call(
            "swarm.dispatch.next",
            json!({"request_id":request_id,
            "target_id":"fixture","repo":checkout,"program":"/bin/sh","args":[script],
            "now_ms":at,"snapshot":snapshot,"required_capabilities":["code"],
            "estimate_milli":{"points":100},"purpose":"worker"}),
        );
        assert_eq!(dispatched["job_id"], job_id, "{dispatched}");
        let worker = dispatched["overseer_run_id"].as_str().unwrap();
        let attempt = dispatched["attempt_id"].as_str().unwrap();
        assert_eq!(d.wait_done(worker, 5)["status"], "completed");
        let messages = d.call(
            "swarm.messages",
            json!({"run_id":run_id,"recipient":"director"}),
        );
        let artifact = format!("proof-{attempt}");
        assert!(
            messages["messages"]
                .as_array()
                .unwrap()
                .iter()
                .any(|m| m["type"] == "result"
                    && m["job_id"] == job_id
                    && m["payload"]["artifact_ids"][0] == artifact),
            "{messages}"
        );
        d.call(
            "swarm.worker.reconcile",
            json!({"run_id":run_id,"job_id":job_id,
            "attempt_id":attempt,"generation":1,"revision":1}),
        );
        let batch = d.call(
            "swarm.director.claim_batch",
            json!({"run_id":run_id,
            "generation":1,"revision":1,"now_ms":now()+6000}),
        );
        assert_eq!(batch["status"], "claimed", "{batch}");
        d.call(
            "swarm.decide",
            json!({"run_id":run_id,"generation":1,"revision":1,
            "job_id":job_id,"decision":"accept","evidence":[artifact]}),
        );
        d.call(
            "swarm.director.complete_batch",
            json!({"run_id":run_id,"generation":1,
            "turn_id":batch["turn_id"],"token":batch["token"],"outcome":"progress"}),
        );
        checks.push(json!({"job_id":job_id,"outcome":"passed","evidence":[artifact]}));
        if job_id == "contract" {
            assert!(d.try_call("swarm.complete",json!({"run_id":run_id,"generation":1,
                "revision":1,"request_id":"finish-before-endpoint",
                "summary":"Contract checked; endpoint pending","verification":"Endpoint still pending",
                "checks":checks})).is_err());
        }
    }
    let jobs = d.call("swarm.jobs", json!({"id":run_id}));
    assert_eq!(jobs["jobs"][0]["status"], "accepted");
    assert_eq!(jobs["jobs"][1]["status"], "accepted");
    let final_request = json!({"run_id":run_id,"generation":1,"revision":1,
        "request_id":"finish-verified-backend",
        "summary":"Contract and endpoint checks are complete.",
        "verification":"The scripted endpoint check used the accepted contract finding.",
        "checks":checks});
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    db.execute("UPDATE swarm_artifacts SET content='tampered after review' WHERE run_id=?1 AND job_id='contract'",
        rusqlite::params![run_id]).unwrap();
    assert!(d
        .try_call("swarm.complete", final_request.clone())
        .unwrap_err()
        .contains("integrity"));
    assert_eq!(
        d.call("swarm.get", json!({"id":run_id}))["status"],
        "running"
    );
    db.execute("UPDATE swarm_artifacts SET content='scripted evidence' WHERE run_id=?1 AND job_id='contract'",
        rusqlite::params![run_id]).unwrap();
    let completed = d.call("swarm.complete", final_request.clone());
    assert_eq!(completed["status"], "completed");
    assert_eq!(
        d.call("swarm.complete", final_request.clone())["duplicate"],
        true
    );
    let mut changed = final_request;
    changed["summary"] = json!("An unreviewed alternative summary");
    assert!(d.try_call("swarm.complete", changed).is_err());
    d.kill9();
    d.spawn();
    let recovered = d.call("swarm.get", json!({"id":run_id}));
    assert_eq!(recovered["status"], "completed");
    assert_eq!(
        recovered["completion"]["checks"].as_array().unwrap().len(),
        2
    );
}

#[test]
fn supervised_scripted_worker_receives_and_applies_targeted_director_advisory() {
    let d = Daemon::start(&[]);
    let temp = tmp();
    let checkout = repo(&temp.path().join("worker-advisory-source"));
    let script = temp.path().join("receive-advisory.sh");
    std::fs::write(&script, r#"#!/bin/sh
set -eu
count=0
while [ "$count" -lt 100 ]; do
    inbox=$("$OVERSEER_BIN" ctl swarm.messages "{\"run_id\":\"$OVERSEER_SWARM_RUN_ID\",\"recipient\":\"$OVERSEER_SWARM_ATTEMPT_ID\",\"token\":\"$OVERSEER_SWARM_TOKEN\"}")
    case "$inbox" in *'"message_id":"targeted-note"'*) break;; esac
    count=$((count + 1))
    sleep 0.05
done
test "$count" -lt 100
delivered=$("$OVERSEER_BIN" ctl swarm.ack "{\"run_id\":\"$OVERSEER_SWARM_RUN_ID\",\"message_id\":\"targeted-note\",\"recipient\":\"$OVERSEER_SWARM_ATTEMPT_ID\",\"token\":\"$OVERSEER_SWARM_TOKEN\",\"revision\":$OVERSEER_SWARM_REVISION,\"phase\":\"delivered\"}")
case "$delivered" in *'"error"'*) exit 2;; esac
applied=$("$OVERSEER_BIN" ctl swarm.ack "{\"run_id\":\"$OVERSEER_SWARM_RUN_ID\",\"message_id\":\"targeted-note\",\"recipient\":\"$OVERSEER_SWARM_ATTEMPT_ID\",\"token\":\"$OVERSEER_SWARM_TOKEN\",\"revision\":$OVERSEER_SWARM_REVISION,\"phase\":\"applied\"}")
case "$applied" in *'"error"'*) exit 3;; esac
"#).unwrap();
    let run = d.call(
        "swarm.create",
        json!({"category":"Fixture advisory",
        "objective":"Inspect API routes","allowed_targets":["fixture"]}),
    );
    let run_id = run["id"].as_str().unwrap();
    d.call(
        "swarm.plan",
        json!({"id":run_id,"generation":1,"revision":0,"jobs":[
            {"id":"inspect","title":"Inspect routes","acceptance":"Finding","deps":[]}
        ]}),
    );
    let at = now();
    let dispatched = d.call(
        "swarm.dispatch.next",
        json!({"request_id":"advisory-worker",
        "target_id":"fixture","repo":checkout,"program":"/bin/sh","args":[script],
        "now_ms":at,"snapshot":{"version":1,"observed_ms":at-1000,
            "expires_ms":at+60000,"targets":[{"id":"fixture","account_id":"fixture",
                "pool_ids":["pool"],"capabilities":["code"],"health":"up","auth":"ok"}],
            "pools":[{"id":"pool","windows":[{"id":"run","unit":"points",
                "remaining_milli":100000,"protected_milli":0,"reserved_milli":0,
                "confidence":"exact","expires_ms":at+60000}]}]},
        "required_capabilities":["code"],"estimate_milli":{"points":100},"purpose":"worker"}),
    );
    let worker = dispatched["overseer_run_id"].as_str().unwrap();
    let attempt = dispatched["attempt_id"].as_str().unwrap();
    let sent = d.call(
        "swarm.direct",
        json!({"run_id":run_id,"job_id":"inspect",
        "attempt_id":attempt,"generation":1,"revision":1,"message_id":"targeted-note",
        "type":"advisory","payload":{"question":"Check the retry handler"}}),
    );
    assert_eq!(sent["phase"], "queued");
    assert_eq!(d.wait_done(worker, 8)["status"], "completed");
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let (phase,payload): (String,String) = db.query_row(
        "SELECT phase,payload FROM swarm_messages WHERE run_id=?1 AND recipient=?2 AND message_id='targeted-note'",
        rusqlite::params![run_id,attempt], |row| Ok((row.get(0)?,row.get(1)?)),
    ).unwrap();
    assert_eq!(phase,"applied");
    assert_eq!(serde_json::from_str::<serde_json::Value>(&payload).unwrap()["question"],
        "Check the retry handler");
}
