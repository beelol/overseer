mod common;

use common::*;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::time::{SystemTime, UNIX_EPOCH};

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

#[test]
fn independent_jobs_execute_together_while_conflicting_writer_waits() {
    let d = Daemon::start(&[]);
    let temp = tmp();
    let checkout = repo(&temp.path().join("parallel-source"));
    let run = d.call("swarm.create", json!({"category":"Parallel backend replay",
        "objective":"Check contract, API, queue and storage independently",
        "allowed_targets":["fixture-local"]}));
    let id = run["id"].as_str().unwrap();
    d.call("swarm.plan",json!({"id":id,"generation":1,"revision":0,"jobs":[
        {"id":"root","title":"Contract","acceptance":"contract evidence","deps":[]},
        {"id":"ind-a","title":"API","acceptance":"API evidence","deps":[]},
        {"id":"ind-b","title":"Queue","acceptance":"queue evidence","deps":[]},
        {"id":"ind-c","title":"Storage","acceptance":"storage evidence","deps":[]},
        {"id":"writer-a","title":"Database writer A","acceptance":"DB evidence","deps":[],
            "resource_claims":[{"resource":"db:shared","mode":"write"}]},
        {"id":"writer-b","title":"Database writer B","acceptance":"DB evidence","deps":[],
            "resource_claims":[{"resource":"db:shared","mode":"write"}]}
    ]}));
    commit_beneficial_batch(&d,id,&["root".into(),"ind-a".into(),"ind-b".into(),
        "ind-c".into(),"writer-a".into()]);
    let at=now();
    let snapshot=json!({"version":1,"observed_ms":at-1000,"expires_ms":at+60000,
        "targets":[{"id":"fixture-local","account_id":"fixture",
            "pool_ids":["pool"],"capabilities":["code"],"health":"up","auth":"ok"}],
        "pools":[{"id":"pool","windows":[{"id":"run","unit":"points",
            "remaining_milli":200000,"protected_milli":0,"reserved_milli":0,
            "confidence":"exact","expires_ms":at+60000}]}]});
    let mut workers=Vec::new();
    for job in ["root","ind-a","ind-b","ind-c","writer-a"] {
        if job=="writer-a" {
            let early=d.call("swarm.admit",json!({"run_id":id,"generation":1,
                "revision":1,"job_id":job,"target_id":"fixture-local",
                "request_id":"parallel-writer-early","snapshot":snapshot,"now_ms":at,
                "required_capabilities":["code"],"estimate_milli":{"points":1000},
                "purpose":"worker"}));
            assert_eq!(early["reason"],"growth_wave_full","{early}");
        }
        let wave_at=if job=="writer-a" { at+5000 } else { at };
        let admitted=d.call("swarm.admit",json!({"run_id":id,"generation":1,
            "revision":1,"job_id":job,"target_id":"fixture-local",
            "request_id":format!("parallel-{job}"),"snapshot":snapshot,"now_ms":wave_at,
            "required_capabilities":["code"],"estimate_milli":{"points":1000},
            "purpose":"worker"}));
        assert_eq!(admitted["status"],"admitted","{job}: {admitted}");
        let launched=d.call("swarm.worker.launch",json!({"run_id":id,"job_id":job,
            "attempt_id":admitted["attempt_id"],"token":admitted["token"],
            "repo":checkout,"program":"/bin/sleep","args":["30"],
            "prompt":format!("Check {job}"),"title":format!("Worker {job}")}));
        assert_eq!(launched["status"],"launched","{job}: {launched}");
        workers.push(launched["overseer_run_id"].as_str().unwrap().to_owned());
    }
    for worker in &workers {
        assert_eq!(d.wait_status(worker,|s|s=="running",5)["status"],"running");
    }
    assert!(workers.iter().all(|worker|d.run(worker)["status"]=="running"),
        "all five supervised workers must overlap in real process time");
    let held=d.call("swarm.admit",json!({"run_id":id,"generation":1,
        "revision":1,"job_id":"writer-b","target_id":"fixture-local",
        "request_id":"parallel-writer-b","snapshot":snapshot,"now_ms":at+5000,
        "required_capabilities":["code"],"estimate_milli":{"points":1000},
        "purpose":"worker"}));
    assert_eq!(held["reason"],"resource_conflict","{held}");
    let db=rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let attempts:i64=db.query_row("SELECT COUNT(*) FROM swarm_attempts WHERE run_id=?1",
        [id],|row|row.get(0)).unwrap();
    assert_eq!(attempts,5,"held writer must not reserve a sixth attempt");
    d.call("swarm.stop",json!({"run_id":id}));
    for worker in &workers {
        assert_eq!(d.wait_done(worker,8)["status"],"interrupted");
    }
}

#[test]
fn supervised_dependency_chain_waits_for_accepted_result_and_exit() {
    let d=Daemon::start(&[]);
    let temp=tmp();
    let checkout=repo(&temp.path().join("serial-source"));
    let run=d.call("swarm.create",json!({"category":"Serial backend replay",
        "objective":"Check contract, consumer, then integration",
        "allowed_targets":["fixture-local"]}));
    let id=run["id"].as_str().unwrap();
    d.call("swarm.plan",json!({"id":id,"generation":1,"revision":0,"jobs":[
        {"id":"root","title":"Contract","acceptance":"contract evidence","deps":[]},
        {"id":"child","title":"Consumer","acceptance":"consumer evidence","deps":["root"]},
        {"id":"leaf","title":"Integration","acceptance":"integration evidence","deps":["child"]}
    ]}));
    let at=now();
    let snapshot=json!({"version":1,"observed_ms":at-1000,"expires_ms":at+60000,
        "targets":[{"id":"fixture-local","account_id":"fixture","pool_ids":["pool"],
            "capabilities":["code"],"health":"up","auth":"ok"}],
        "pools":[{"id":"pool","windows":[{"id":"run","unit":"points",
            "remaining_milli":100000,"protected_milli":0,"reserved_milli":0,
            "confidence":"exact","expires_ms":at+60000}]}]});
    let mut previous:Option<&str>=None;
    for job in ["root","child","leaf"] {
        let admitted=d.call("swarm.admit",json!({"run_id":id,"generation":1,
            "revision":1,"job_id":job,"target_id":"fixture-local",
            "request_id":format!("serial-{job}"),"snapshot":snapshot,"now_ms":at,
            "required_capabilities":["code"],"estimate_milli":{"points":1000},
            "purpose":"worker"}));
        assert_eq!(admitted["status"],"admitted","{job}: {admitted}");
        let marker=temp.path().join(format!("release-{job}"));
        let launched=d.call("swarm.worker.launch",json!({"run_id":id,"job_id":job,
            "attempt_id":admitted["attempt_id"],"token":admitted["token"],
            "repo":checkout,"program":"/bin/sh",
            "args":["-c","while [ ! -f \"$1\" ]; do sleep 0.1; done","worker",marker],
            "prompt":format!("Check {job}"),"title":format!("Worker {job}")}));
        let worker=launched["overseer_run_id"].as_str().unwrap();
        d.wait_status(worker,|s|s=="running",5);
        if let Some(parent)=previous {
            assert_eq!(d.call("swarm.jobs",json!({"id":id}))["jobs"].as_array().unwrap()
                .iter().find(|entry|entry["id"]==parent).unwrap()["status"],"accepted");
        }
        let artifact=format!("proof-{job}");
        d.call("swarm.artifact.put",json!({"run_id":id,"job_id":job,
            "attempt_id":admitted["attempt_id"],"token":admitted["token"],
            "artifact_id":artifact,"source_revision":1,"kind":"finding",
            "content":format!("checked {job}")}));
        d.call("swarm.report",json!({"run_id":id,"job_id":job,
            "attempt_id":admitted["attempt_id"],"token":admitted["token"],
            "message_id":format!("result-{job}"),"type":"result","revision":1,
            "payload":{"artifact_ids":[artifact]}}));
        if let Some(next)=match job {"root"=>Some("child"),"child"=>Some("leaf"),_=>None} {
            let before=d.call("swarm.admit",json!({"run_id":id,"generation":1,
                "revision":1,"job_id":next,"target_id":"fixture-local",
                "request_id":format!("before-accept-{next}"),"snapshot":snapshot,"now_ms":at,
                "required_capabilities":["code"],"estimate_milli":{"points":1000},
                "purpose":"worker"}));
            assert_eq!((before["reason"].as_str(),before["waiting_on"].clone()),
                (Some("dependency_pending"),json!([job])));
        }
        d.call("swarm.decide",json!({"run_id":id,"generation":1,"revision":1,
            "job_id":job,"decision":"accept","evidence":[artifact]}));
        if let Some(next)=match job {"root"=>Some("child"),"child"=>Some("leaf"),_=>None} {
            let before=d.call("swarm.admit",json!({"run_id":id,"generation":1,
                "revision":1,"job_id":next,"target_id":"fixture-local",
                "request_id":format!("before-exit-{next}"),"snapshot":snapshot,"now_ms":at,
                "required_capabilities":["code"],"estimate_milli":{"points":1000},
                "purpose":"worker"}));
            assert_eq!((before["reason"].as_str(),before["waiting_on"].clone()),
                (Some("dependency_pending"),json!([job])));
        }
        std::fs::write(&marker,"release").unwrap();
        assert_eq!(d.wait_done(worker,8)["status"],"completed");
        d.call("swarm.attempt.confirm_exit",json!({"run_id":id,"generation":1,
            "revision":1,"job_id":job,"attempt_id":admitted["attempt_id"]}));
        previous=Some(job);
    }
    assert!(d.call("swarm.jobs",json!({"id":id}))["jobs"].as_array().unwrap()
        .iter().all(|entry|entry["status"]=="accepted"));
}

#[test]
fn repository_scope_blocks_director_and_worker_launch_outside_approved_repo() {
    let d = Daemon::start(&[]);
    let temp = tmp();
    let approved = repo(&temp.path().join("approved-source"));
    let unrelated = repo(&temp.path().join("unrelated-source"));
    let run = d.call("swarm.create", json!({"category":"Scoped backend",
        "objective":"Inspect approved repository", "allowed_targets":["fixture-local"],
        "repositories":[approved]}));
    let id = run["id"].as_str().unwrap();
    assert_eq!(run["repositories"].as_array().unwrap().len(),1);
    let denied = d.try_call("swarm.director.launch",json!({"run_id":id,
        "generation":1,"repo":unrelated,"program":"/bin/sleep","args":["1"],
        "prompt":"Inspect","title":"Out of scope director"})).unwrap_err();
    assert!(denied.contains("repository is outside the approved Swarm scope"),"{denied}");
    d.call("swarm.plan",json!({"id":id,"generation":1,"revision":0,"jobs":[
        {"id":"inspect","title":"Inspect","acceptance":"evidence","deps":[]}
    ]}));
    let at=now();
    let admitted=d.call("swarm.admit",json!({"run_id":id,"generation":1,"revision":1,
        "job_id":"inspect","target_id":"fixture-local","request_id":"scoped-worker",
        "now_ms":at,"snapshot":{"version":1,"observed_ms":at-1000,"expires_ms":at+60000,
            "targets":[{"id":"fixture-local","account_id":"fixture","pool_ids":["pool"],
                "capabilities":["code"],"health":"up","auth":"ok"}],
            "pools":[{"id":"pool","windows":[{"id":"run","unit":"points",
                "remaining_milli":100000,"protected_milli":0,"reserved_milli":0,
                "confidence":"exact","expires_ms":at+60000}]}]},
        "required_capabilities":["code"],"estimate_milli":{"points":100},"purpose":"worker"}));
    assert_eq!(admitted["status"],"admitted","{admitted}");
    let brief=d.call("swarm.worker.brief",json!({"run_id":id,"job_id":"inspect",
        "attempt_id":admitted["attempt_id"],"token":admitted["token"]}));
    assert_eq!(brief["repositories"],run["repositories"]);
    let denied=d.try_call("swarm.worker.launch",json!({"run_id":id,"job_id":"inspect",
        "attempt_id":admitted["attempt_id"],"token":admitted["token"],
        "repo":unrelated,"harness":"generic","program":"/bin/sleep","args":["1"],
        "prompt":"Inspect","title":"Out of scope worker"})).unwrap_err();
    assert!(denied.contains("repository is outside the approved Swarm scope"),"{denied}");
    let db=rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let owners:i64=db.query_row("SELECT COUNT(*) FROM swarm_director_owners WHERE run_id=?1",
        [id],|r|r.get(0)).unwrap();
    let launches:i64=db.query_row("SELECT COUNT(*) FROM swarm_worker_launches WHERE run_id=?1",
        [id],|r|r.get(0)).unwrap();
    assert_eq!((owners,launches),(0,0));
    let sibling=temp.path().join("approved-sibling");
    git(&approved,&["worktree","add","--detach",sibling.to_str().unwrap(),"HEAD"]);
    let launched=d.call("swarm.worker.launch",json!({"run_id":id,"job_id":"inspect",
        "attempt_id":admitted["attempt_id"],"token":admitted["token"],
        "repo":sibling,"harness":"generic","program":"/bin/sleep","args":["1"],
        "prompt":"Inspect","title":"Approved worker"}));
    assert_eq!(launched["status"],"launched","{launched}");
    let worker = launched["overseer_run_id"].as_str().unwrap();
    for method in ["run.targets", "run.handoff", "run.retry_now"] {
        let refused=d.try_call(method,json!({"run_id":worker,"to":"local"})).unwrap_err();
        assert!(refused.contains("Swarm owns this run"),"{method}: {refused}");
    }
    assert_eq!(d.call("swarm.jobs",json!({"id":id}))["jobs"][0]["attempt_count"],1);
    let fork_commit:String=db.query_row(
        "SELECT t.fork_commit FROM tasks t JOIN runs r ON r.task_id=t.id WHERE r.id=?1",
        [launched["overseer_run_id"].as_str().unwrap()],|r|r.get(0)).unwrap();
    assert_eq!(fork_commit,run["repositories"][0]["source_commit"]);
}

#[test]
fn repository_scope_pins_source_revision_across_daemon_restart() {
    let mut d=Daemon::start(&[]);
    let temp=tmp();
    let approved=repo(&temp.path().join("pinned-source"));
    let initial=git(&approved,&["rev-parse","HEAD"]);
    let run=d.call("swarm.create",json!({"category":"Pinned audit",
        "objective":"Inspect pinned source","allowed_targets":["fixture-local"],
        "repositories":[approved]}));
    let id=run["id"].as_str().unwrap();
    assert_eq!(run["repositories"][0]["source_commit"],initial);
    std::fs::write(approved.join("changed.txt"),"new source\n").unwrap();
    git(&approved,&["add","changed.txt"]);
    git(&approved,&["commit","-q","-m","advance source"]);
    d.kill9();
    d.spawn();
    assert_eq!(d.call("swarm.get",json!({"id":id}))["repositories"][0]["source_commit"],initial);
    let denied=d.try_call("swarm.director.launch",json!({"run_id":id,
        "generation":1,"repo":approved,"program":"/bin/sleep","args":["1"],
        "prompt":"Inspect","title":"Changed source director"})).unwrap_err();
    assert!(denied.contains("source revision changed since Swarm approval"),"{denied}");
}

#[test]
fn uncontrolled_native_delegation_blocks_admission_before_reserving_or_launching() {
    let d = Daemon::start(&[]);
    let run = d.call("swarm.create", json!({"category":"Uncontrolled delegation",
        "objective":"Inspect backend","allowed_targets":["fixture-codex"]}));
    let id = run["id"].as_str().unwrap();
    d.call("swarm.plan",json!({"id":id,"generation":1,"revision":0,"jobs":[
        {"id":"inspect","title":"Inspect","acceptance":"evidence","deps":[]}
    ]}));
    let at = now();
    let result = d.call("swarm.admit",json!({"run_id":id,"generation":1,"revision":1,
        "job_id":"inspect","target_id":"fixture-codex","request_id":"uncontrolled",
        "now_ms":at,"snapshot":{"version":1,"observed_ms":at-1000,"expires_ms":at+60000,
            "targets":[{"id":"fixture-codex","harness":"codex",
                "profile_id":"system-codex","model":"gpt-5.6-luna","account_id":"fixture",
                "pool_ids":["pool"],"capabilities":["code","native_child_control"],
                "health":"up","auth":"ok"}],
            "pools":[{"id":"pool","windows":[{"id":"run","unit":"points",
                "remaining_milli":100000,"protected_milli":0,"reserved_milli":0,
                "confidence":"exact","expires_ms":at+60000}]}]},
        "required_capabilities":["code"],"estimate_milli":{"points":100},"purpose":"worker"}));
    assert_eq!(result["status"],"blocked", "{result}");
    assert_eq!(result["reason"],"uncontrolled_native_delegation");
    let db=rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let attempts: i64 = db.query_row("SELECT COUNT(*) FROM swarm_attempts WHERE run_id=?1",
        [id],|r|r.get(0)).unwrap();
    assert_eq!(attempts,0);
}

#[test]
fn admitted_target_harness_cannot_be_changed_at_worker_launch() {
    let d = Daemon::start(&[("OVERSEER_SHARED_BOOKING_FIXTURE_API","1")]);
    let temp = tmp();
    let checkout = repo(&temp.path().join("route-binding-source"));
    let run = d.call("swarm.create", json!({"category":"Route binding",
        "objective":"Inspect backend","allowed_targets":["fixture-claude"],
        "source_change_permission":"isolated"}));
    let id = run["id"].as_str().unwrap();
    d.call("swarm.plan",json!({"id":id,"generation":1,"revision":0,"jobs":[
        {"id":"inspect","title":"Inspect","acceptance":"evidence","deps":[]}
    ]}));
    let at = now();
    // A native (account) target books its account windows and app slot in
    // the shared booking; the fixture observation stands in for Auto's feed.
    let booking = fixture_account_booking(&d, "system-claude", "fixture-claude-account", 0.0, 1000);
    let admitted = d.call("swarm.admit",json!({"shared_booking":booking,"run_id":id,"generation":1,"revision":1,
        "job_id":"inspect","target_id":"fixture-claude","request_id":"route-binding",
        "now_ms":at,"snapshot":{"version":1,"observed_ms":at-1000,"expires_ms":at+60000,
            "targets":[{"id":"fixture-claude","harness":"claude",
                "profile_id":"system-claude","model":"sonnet","effort":"medium","account_id":"fixture",
                "pool_ids":["pool"],"capabilities":["code"],"health":"up","auth":"ok"}],
            "pools":[{"id":"pool","windows":[{"id":"run","unit":"points",
                "remaining_milli":100000,"protected_milli":0,"reserved_milli":0,
                "confidence":"exact","expires_ms":at+60000}]}]},
        "required_capabilities":["code"],"estimate_milli":{"points":100},"purpose":"worker"}));
    assert_eq!(admitted["status"],"admitted", "{admitted}");
    let wrong = json!({"run_id":id,"job_id":"inspect",
        "attempt_id":admitted["attempt_id"],"token":admitted["token"],
        "repo":checkout,"harness":"generic","program":"/bin/sleep","args":["30"],
        "prompt":"Inspect","title":"Wrong harness"});
    let error = d.try_call("swarm.worker.launch",wrong).unwrap_err();
    assert!(error.contains("admitted target harness"), "{error}");
    let db=rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let launch_count: i64 = db.query_row("SELECT COUNT(*) FROM swarm_worker_launches
        WHERE attempt_id=?1",[admitted["attempt_id"].as_str().unwrap()],|row|row.get(0)).unwrap();
    assert_eq!(launch_count,0,"wrong harness must not create a launch intent");
    assert_eq!(d.call("swarm.jobs",json!({"id":id}))["jobs"][0]["status"],"reserved");
}

#[test]
fn previously_admitted_native_worker_cannot_launch_after_audit_scope_is_restored() {
    let d = Daemon::start(&[("OVERSEER_SHARED_BOOKING_FIXTURE_API","1")]);
    let temp = tmp();
    let checkout = repo(&temp.path().join("legacy-audit-source"));
    let run = d.call("swarm.create", json!({"category":"Legacy audit admission",
        "objective":"Inspect backend","allowed_targets":["fixture-claude"],
        "source_change_permission":"isolated"}));
    let id = run["id"].as_str().unwrap();
    d.call("swarm.plan",json!({"id":id,"generation":1,"revision":0,"jobs":[
        {"id":"inspect","title":"Inspect","acceptance":"evidence","deps":[]}
    ]}));
    let at = now();
    // A native (account) target books its account windows and app slot in
    // the shared booking; the fixture observation stands in for Auto's feed.
    let booking = fixture_account_booking(&d, "system-claude", "fixture-claude-account", 0.0, 1000);
    let admitted = d.call("swarm.admit",json!({"shared_booking":booking,"run_id":id,"generation":1,"revision":1,
        "job_id":"inspect","target_id":"fixture-claude","request_id":"old-admission",
        "now_ms":at,"snapshot":{"version":1,"observed_ms":at-1000,"expires_ms":at+60000,
            "targets":[{"id":"fixture-claude","harness":"claude",
                "profile_id":"system-claude","model":"sonnet","account_id":"fixture",
                "pool_ids":["pool"],"capabilities":["code"],"health":"up","auth":"ok"}],
            "pools":[{"id":"pool","windows":[{"id":"run","unit":"points",
                "remaining_milli":100000,"protected_milli":0,"reserved_milli":0,
                "confidence":"exact","expires_ms":at+60000}]}]},
        "required_capabilities":["code"],"estimate_milli":{"points":100},"purpose":"worker"}));
    assert_eq!(admitted["status"],"admitted","{admitted}");
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    db.execute("UPDATE swarm_runs SET source_change_permission='none' WHERE id=?1",[id]).unwrap();
    let error = d.try_call("swarm.worker.launch",json!({"run_id":id,"job_id":"inspect",
        "attempt_id":admitted["attempt_id"],"token":admitted["token"],
        "repo":checkout,"harness":"claude","args":[],
        "prompt":"Inspect","title":"Unsafe old reservation"})).unwrap_err();
    assert!(error.contains("audit source boundary"),"{error}");
    let count: i64 = db.query_row("SELECT COUNT(*) FROM swarm_worker_launches WHERE attempt_id=?1",
        [admitted["attempt_id"].as_str().unwrap()],|r|r.get(0)).unwrap();
    assert_eq!(count,0,"unsafe launch must not write an intent");
}

#[test]
fn admitted_profile_model_and_effort_cannot_be_changed_at_worker_launch() {
    let fixture_path = repo_root().join("fixtures/fake-harness/claude-fixture.js")
        .display().to_string();
    let d = Daemon::start(&[("OVERSEER_CLAUDE_PATH", &fixture_path),
        ("OVERSEER_SHARED_BOOKING_FIXTURE_API","1")]);
    let temp = tmp();
    let checkout = repo(&temp.path().join("route-options-source"));
    let run = d.call("swarm.create", json!({"category":"Route options",
        "objective":"Inspect backend","allowed_targets":["claude-sonnet"],
        "source_change_permission":"isolated"}));
    let id = run["id"].as_str().unwrap();
    d.call("swarm.plan",json!({"id":id,"generation":1,"revision":0,"jobs":[
        {"id":"inspect","title":"Inspect","acceptance":"evidence","deps":[]}
    ]}));
    let at = now();
    // A native (account) target books its account windows and app slot in
    // the shared booking; the fixture observation stands in for Auto's feed.
    let booking = fixture_account_booking(&d, "system-claude", "fixture-claude-account", 0.0, 1000);
    let admitted = d.call("swarm.admit",json!({"shared_booking":booking,"run_id":id,"generation":1,"revision":1,
        "job_id":"inspect","target_id":"claude-sonnet","request_id":"route-options",
        "now_ms":at,"snapshot":{"version":1,"observed_ms":at-1000,"expires_ms":at+60000,
            "targets":[{"id":"claude-sonnet","harness":"claude","profile_id":"system-claude",
                "model":"sonnet","effort":"medium","account_id":"fixture","pool_ids":["pool"],
                "capabilities":["code"],"health":"up","auth":"ok"}],
            "pools":[{"id":"pool","windows":[{"id":"run","unit":"points",
                "remaining_milli":100000,"protected_milli":0,"reserved_milli":0,
                "confidence":"exact","expires_ms":at+60000}]}]},
        "required_capabilities":["code"],"estimate_milli":{"points":100},"purpose":"worker"}));
    assert_eq!(admitted["status"],"admitted", "{admitted}");
    let wrong = json!({"run_id":id,"job_id":"inspect",
        "attempt_id":admitted["attempt_id"],"token":admitted["token"],
        "repo":checkout,"harness":"claude","profile_id":"system-codex",
        "model":"opus","effort":"high","args":[],
        "prompt":"Inspect","title":"Wrong route options"});
    let error = d.try_call("swarm.worker.launch",wrong).unwrap_err();
    assert!(error.contains("admitted target route"), "{error}");
    let db=rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let launch_count: i64 = db.query_row("SELECT COUNT(*) FROM swarm_worker_launches
        WHERE attempt_id=?1",[admitted["attempt_id"].as_str().unwrap()],|row|row.get(0)).unwrap();
    assert_eq!(launch_count,0,"route changes must not create a launch intent");
    db.execute("UPDATE swarm_admissions SET target_model=NULL WHERE attempt_id=?1",
        [admitted["attempt_id"].as_str().unwrap()]).unwrap();
    let incomplete = json!({"run_id":id,"job_id":"inspect",
        "attempt_id":admitted["attempt_id"],"token":admitted["token"],
        "repo":checkout,"harness":"claude","args":[],
        "prompt":"Inspect","title":"Incomplete old route"});
    let error = d.try_call("swarm.worker.launch",incomplete).unwrap_err();
    assert!(error.contains("admitted target route"), "{error}");
    let launch_count: i64 = db.query_row("SELECT COUNT(*) FROM swarm_worker_launches
        WHERE attempt_id=?1",[admitted["attempt_id"].as_str().unwrap()],|row|row.get(0)).unwrap();
    assert_eq!(launch_count,0,"incomplete route must not create a launch intent");
}

#[test]
fn delivered_redirect_interrupts_a_long_running_worker_after_restart() {
    let mut d = Daemon::start(&[]);
    let temp = tmp();
    let checkout = repo(&temp.path().join("redirect-source"));
    let run = d.call("swarm.create", json!({"category":"Atlas redirect",
        "objective":"Audit attachments","allowed_targets":["fixture-local"]}));
    let id = run["id"].as_str().unwrap();
    d.call("swarm.plan",json!({"id":id,"generation":1,"revision":0,"jobs":[
        {"id":"j4","title":"Attachments","acceptance":"signed URL evidence","deps":[]}
    ]}));
    let at=now();
    let attempt=d.call("swarm.admit",json!({"run_id":id,"generation":1,"revision":1,
        "job_id":"j4","target_id":"fixture-local","request_id":"redirect-worker",
        "now_ms":at,"snapshot":{"version":1,"observed_ms":at-1000,"expires_ms":at+60000,
            "targets":[{"id":"fixture-local","account_id":"fixture","pool_ids":["fixture-pool"],
                "capabilities":["code"],"health":"up","auth":"ok"}],
            "pools":[{"id":"fixture-pool","windows":[{"id":"run","unit":"points",
                "remaining_milli":100000,"protected_milli":0,"reserved_milli":0,
                "confidence":"exact","expires_ms":at+60000}]}]},
        "required_capabilities":["code"],"estimate_milli":{"points":100},"purpose":"worker"}));
    assert_eq!(attempt["status"],"admitted", "{attempt}");
    let launched=d.call("swarm.worker.launch",json!({"run_id":id,"job_id":"j4",
        "attempt_id":attempt["attempt_id"],"token":attempt["token"],
        "repo":checkout,"program":"/bin/sleep","args":["30"],
        "prompt":"Inspect attachments","title":"J4"}));
    let worker=launched["overseer_run_id"].as_str().unwrap();
    d.call("swarm.direct",json!({"run_id":id,"job_id":"j4",
        "attempt_id":attempt["attempt_id"],"message_id":"redirect-j4",
        "generation":1,"revision":1,"type":"redirect",
        "payload":{"focus":"signed URL boundary"}}));
    d.call("swarm.ack",json!({"run_id":id,"message_id":"redirect-j4",
        "recipient":attempt["attempt_id"],"token":attempt["token"],
        "phase":"delivered","revision":1}));
    let db=rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let run_dir:String=db.query_row("SELECT run_dir FROM runs WHERE id=?1",[worker],|r|r.get(0)).unwrap();
    let marker=std::path::Path::new(&run_dir).join("interrupt.requested");
    assert!(!marker.exists());
    d.kill9();
    db.execute("UPDATE swarm_messages SET updated_ms=?3 WHERE run_id=?1 AND message_id=?2",
        rusqlite::params![id,"redirect-j4",now()-30_001]).unwrap();
    d.spawn();
    let until=std::time::Instant::now()+std::time::Duration::from_secs(4);
    while !marker.exists() {
        assert!(std::time::Instant::now()<until,"redirect timeout did not interrupt linked worker");
        std::thread::sleep(std::time::Duration::from_millis(30));
    }
    let job=&d.call("swarm.jobs",json!({"id":id}))["jobs"][0];
    assert_eq!(job["status"],"cancel_requested");
    assert_eq!(job["stop_reason"],"redirect_ack_timeout");
    let inbox=d.call("swarm.messages",json!({"run_id":id,"recipient":attempt["attempt_id"],
        "token":attempt["token"]}));
    assert_eq!(inbox["messages"].as_array().unwrap().iter()
        .filter(|m|m["type"]=="checkpoint" && m["payload"]["reason"]=="redirect_ack_timeout").count(),1);
}

#[test]
fn late_resource_conflict_interrupts_only_affected_supervised_workers() {
    let d = Daemon::start(&[]);
    let temp = tmp();
    let checkout = repo(&temp.path().join("contamination-source"));
    let run = d.call("swarm.create", json!({"category":"Late resource collision",
        "objective":"Inspect backend","allowed_targets":["fixture-local"]}));
    let id = run["id"].as_str().unwrap();
    d.call("swarm.plan",json!({"id":id,"generation":1,"revision":0,"jobs":[
        {"id":"a","title":"A","acceptance":"evidence","deps":[]},
        {"id":"b","title":"B","acceptance":"evidence","deps":[]},
        {"id":"unrelated","title":"Unrelated","acceptance":"evidence","deps":[]}
    ]}));
    commit_beneficial_batch(&d,id,&["a".into(),"b".into(),"unrelated".into()]);
    let at=now();
    let mut attempts=Vec::new();
    for job in ["a","b","unrelated"] {
        let admitted=d.call("swarm.admit",json!({"run_id":id,"generation":1,"revision":1,
            "job_id":job,"target_id":"fixture-local","request_id":format!("admit-{job}"),
            "now_ms":at,"snapshot":{"version":1,"observed_ms":at-1000,"expires_ms":at+60000,
                "targets":[{"id":"fixture-local","account_id":"fixture","pool_ids":["pool"],
                    "capabilities":["code"],"health":"up","auth":"ok"}],
                "pools":[{"id":"pool","windows":[{"id":"run","unit":"points",
                    "remaining_milli":1000000,"protected_milli":0,"reserved_milli":0,
                    "confidence":"exact","expires_ms":at+60000}]}]},
            "required_capabilities":["code"],"estimate_milli":{"points":100},"purpose":"worker"}));
        assert_eq!(admitted["status"],"admitted", "{admitted}");
        let launched=d.call("swarm.worker.launch",json!({"run_id":id,"job_id":job,
            "attempt_id":admitted["attempt_id"],"token":admitted["token"],
            "repo":checkout,"program":"/bin/sleep","args":["30"],
            "prompt":"Inspect","title":format!("Worker {job}")}));
        let worker=launched["overseer_run_id"].as_str().unwrap().to_string();
        attempts.push((job,admitted,worker));
    }
    let observed=|job:&str,attempt:&serde_json::Value|json!({"run_id":id,"job_id":job,
        "attempt_id":attempt["attempt_id"],"token":attempt["token"],
        "generation":1,"revision":1,"resource":"db:late","mode":"write","after_use":true});
    d.call("swarm.claim",observed("a",&attempts[0].1));
    assert_eq!(d.call("swarm.claim",observed("b",&attempts[1].1))["status"],"contaminated");
    let db=rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    for (_,_,worker) in &attempts[..2] {
        let run_dir:String=db.query_row("SELECT run_dir FROM runs WHERE id=?1",[worker],|r|r.get(0)).unwrap();
        let marker=std::path::Path::new(&run_dir).join("interrupt.requested");
        let until=std::time::Instant::now()+std::time::Duration::from_secs(3);
        while !marker.exists() {
            assert!(std::time::Instant::now()<until,"affected worker was not interrupted");
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }
    let unrelated=&attempts[2].2;
    let run_dir:String=db.query_row("SELECT run_dir FROM runs WHERE id=?1",[unrelated],|r|r.get(0)).unwrap();
    assert!(!std::path::Path::new(&run_dir).join("interrupt.requested").exists());
    assert!(["queued","starting","running"].contains(&d.run(unrelated)["status"].as_str().unwrap()));
    d.call("swarm.stop",json!({"run_id":id,"generation":1,"revision":1}));
}

#[test]
fn job_deadline_interrupt_retries_after_daemon_crash() {
    let mut d = Daemon::start(&[]);
    let temp = tmp();
    let checkout = repo(&temp.path().join("deadline-crash-source"));
    let run = d.call("swarm.create", json!({"category":"Crash during job deadline",
        "objective":"Inspect backend","allowed_targets":["fixture-local"],
        "policy":{"deadline_ms":60000}}));
    let id = run["id"].as_str().unwrap();
    d.call("swarm.plan", json!({"id":id,"generation":1,"revision":0,"jobs":[
        {"id":"inspect","title":"Inspect","acceptance":"evidence","deps":[]}
    ]}));
    let at = now();
    let admitted = d.call("swarm.admit", json!({"run_id":id,"generation":1,"revision":1,
        "job_id":"inspect","target_id":"fixture-local","request_id":"deadline-crash-worker",
        "job_deadline_ms":30000,"now_ms":at,
        "snapshot":{"version":1,"observed_ms":at-1000,"expires_ms":at+60000,
            "targets":[{"id":"fixture-local","account_id":"fixture","pool_ids":["fixture-pool"],
                "capabilities":["code"],"health":"up","auth":"ok"}],
            "pools":[{"id":"fixture-pool","windows":[{"id":"run","unit":"points",
                "remaining_milli":100000,"protected_milli":0,"reserved_milli":0,
                "confidence":"exact","expires_ms":at+60000}]}]},
        "required_capabilities":["code"],"estimate_milli":{"points":100},"purpose":"worker"}));
    let launched = d.call("swarm.worker.launch", json!({"run_id":id,"job_id":"inspect",
        "attempt_id":admitted["attempt_id"],"token":admitted["token"],"repo":checkout,
        "program":"/bin/sleep","args":["30"],"prompt":"Inspect",
        "title":"Deadline crash worker"}));
    let worker = launched["overseer_run_id"].as_str().unwrap();
    let deadline = d.call("swarm.jobs", json!({"id":id}))["jobs"][0]["deadline_at_ms"].as_i64().unwrap();
    let persisted = d.call("swarm.job.deadline.persist_due", json!({"now_ms":deadline}));
    assert_eq!(persisted["interrupt_pending"], json!([worker]));
    let job = &d.call("swarm.jobs", json!({"id":id}))["jobs"][0];
    assert_eq!(job["status"], "cancel_requested");
    assert_eq!(job["stop_reason"], "job_deadline");
    assert_eq!(job["attempt_count"], 1);
    let db_probe = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let run_dir: String = db_probe.query_row("SELECT run_dir FROM runs WHERE id=?1",
        [worker], |row| row.get(0)).unwrap();
    assert!(!std::path::Path::new(&run_dir).join("interrupt.requested").exists());
    assert!(["queued","starting","running"].contains(&d.run(worker)["status"].as_str().unwrap()));

    d.kill9();
    db_probe.execute("UPDATE swarm_jobs SET deadline_at_ms=?1 WHERE run_id=?2 AND id='inspect'",
        rusqlite::params![now()-1,id]).unwrap();
    d.spawn();
    assert_ne!(d.wait_done(worker, 8)["status"], "completed");
    let until = std::time::Instant::now() + std::time::Duration::from_secs(3);
    loop {
        let job = &d.call("swarm.jobs", json!({"id":id}))["jobs"][0];
        if job["status"] == "failed" {
            assert_eq!(job["stop_reason"], "job_deadline");
            assert_eq!(job["attempt_count"], 1);
            break;
        }
        assert!(std::time::Instant::now() < until, "timeout did not reconcile: {job}");
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    assert!(std::path::Path::new(&run_dir).join("interrupt.requested").exists());
    let stop_messages: i64 = db_probe.query_row(
        "SELECT COUNT(*) FROM swarm_messages WHERE run_id=?1 AND job_id='inspect' AND kind='stop' AND message_id=?2",
        rusqlite::params![id, format!("job-deadline-{}", admitted["attempt_id"].as_str().unwrap())],
        |row| row.get(0),
    ).unwrap();
    assert_eq!(stop_messages, 1);
}

#[test]
fn job_deadline_never_accepts_a_worker_that_ignores_interrupt() {
    let d = Daemon::start(&[]);
    let temp = tmp();
    let checkout = repo(&temp.path().join("deadline-ignore-source"));
    let run = d.call("swarm.create", json!({"category":"Ignored interrupt",
        "objective":"Inspect backend","allowed_targets":["fixture-local"],
        "policy":{"deadline_ms":15000}}));
    let id = run["id"].as_str().unwrap();
    d.call("swarm.plan", json!({"id":id,"generation":1,"revision":0,"jobs":[
        {"id":"inspect","title":"Inspect","acceptance":"evidence","deps":[]}
    ]}));
    let at = now();
    let admitted = d.call("swarm.admit", json!({"run_id":id,"generation":1,"revision":1,
        "job_id":"inspect","target_id":"fixture-local","request_id":"deadline-ignore-worker",
        "job_deadline_ms":7000,"now_ms":at,
        "snapshot":{"version":1,"observed_ms":at-1000,"expires_ms":at+60000,
            "targets":[{"id":"fixture-local","account_id":"fixture","pool_ids":["fixture-pool"],
                "capabilities":["code"],"health":"up","auth":"ok"}],
            "pools":[{"id":"fixture-pool","windows":[{"id":"run","unit":"points",
                "remaining_milli":100000,"protected_milli":0,"reserved_milli":0,
                "confidence":"exact","expires_ms":at+60000}]}]},
        "required_capabilities":["code"],"estimate_milli":{"points":100},"purpose":"worker"}));
    let ready=temp.path().join("handler-ready");
    let launched = d.call("swarm.worker.launch", json!({"run_id":id,"job_id":"inspect",
        "attempt_id":admitted["attempt_id"],"token":admitted["token"],"repo":checkout,
        "program":"/usr/bin/python3","args":["-c",
            format!("import pathlib,signal,time;signal.signal(signal.SIGINT,signal.SIG_IGN);pathlib.Path({:?}).write_text('ready');time.sleep(30)",ready.to_string_lossy())],
        "prompt":"Inspect","title":"Ignoring worker"}));
    let worker = launched["overseer_run_id"].as_str().unwrap();
    let ready_by=std::time::Instant::now()+std::time::Duration::from_secs(3);
    while !ready.exists() {
        assert!(std::time::Instant::now()<ready_by,"worker did not install its interrupt handler");
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let db_probe = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let run_dir: String = db_probe.query_row("SELECT run_dir FROM runs WHERE id=?1",
        [worker], |row| row.get(0)).unwrap();
    let marker = std::path::Path::new(&run_dir).join("interrupt.requested");
    let until = std::time::Instant::now() + std::time::Duration::from_secs(12);
    while !marker.exists() {
        assert!(std::time::Instant::now() < until, "job deadline did not request interrupt");
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    std::thread::sleep(std::time::Duration::from_millis(600));
    let job = &d.call("swarm.jobs", json!({"id":id}))["jobs"][0];
    assert_eq!(job["status"], "cancel_requested");
    assert_eq!(job["stop_reason"], "job_deadline");
    assert_eq!(job["attempt_count"], 1);
    assert_eq!(d.run(worker)["status"], "running");
    let shim: serde_json::Value = serde_json::from_slice(&std::fs::read(
        std::path::Path::new(&run_dir).join("shim.json")).unwrap()).unwrap();
    signal(shim["child_pid"].as_i64().unwrap(), 9);
    assert_ne!(d.wait_done(worker, 5)["status"], "completed");
    let until = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while d.call("swarm.jobs", json!({"id":id}))["jobs"][0]["status"] != "failed" {
        assert!(std::time::Instant::now() < until, "confirmed exit did not fail the job");
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

#[test]
fn explicit_ceiling_runs_thirty_two_supervised_workers() {
    let d = Daemon::start(&[]);
    d.call("agents.limit.set", json!({"max_active":33}));
    let temp = tmp();
    let checkout = repo(&temp.path().join("large-swarm-source"));
    let run = d.call("swarm.create", json!({"category":"Large local swarm",
        "objective":"Inspect 32 modules","allowed_targets":["fixture-local"],
        "policy":{"max_workers":32,"deadline_ms":240000}}));
    let id = run["id"].as_str().unwrap();
    let jobs: Vec<_> = (0..33).map(|n| json!({"id":format!("j{n}"),
        "title":format!("Inspect {n}"),"acceptance":"evidence","deps":[]})).collect();
    d.call("swarm.plan",json!({"id":id,"generation":1,"revision":0,"jobs":jobs}));
    commit_beneficial_batch(&d, id, &(0..32).map(|n| format!("j{n}")).collect::<Vec<_>>());
    let at = now();
    let admit = |n: usize| d.call("swarm.admit", json!({"run_id":id,"generation":1,"revision":1,
        "job_id":format!("j{n}"),"target_id":"fixture-local",
        "request_id":format!("large-runtime-{n}"),
        "now_ms":at + (n / 4) as i64 * 5000,
        "snapshot":{"version":1,"observed_ms":at-1000,"expires_ms":at+240000,
            "targets":[{"id":"fixture-local","account_id":"fixture","pool_ids":["fixture-pool"],
                "capabilities":["code"],"health":"up","auth":"ok"}],
            "pools":[{"id":"fixture-pool","windows":[{"id":"run","unit":"points",
                "remaining_milli":1000000,"protected_milli":0,"reserved_milli":0,
                "confidence":"exact","expires_ms":at+240000}]}]},
        "required_capabilities":["code"],"estimate_milli":{"points":100},"purpose":"worker"}));
    let mut workers = Vec::new();
    let mut first_attempt = None;
    for n in 0..32 {
        let admitted = admit(n);
        assert_eq!(admitted["status"], "admitted", "job {n}: {admitted}");
        if n == 0 {
            first_attempt = Some(admitted.clone());
        }
        let launched = d.call("swarm.worker.launch", json!({"run_id":id,
            "job_id":format!("j{n}"),"attempt_id":admitted["attempt_id"],
            "token":admitted["token"],"repo":checkout,
            "program":"/bin/sleep","args":["180"],"prompt":"Inspect",
            "title":format!("Fixture worker {n}")}));
        assert_eq!(launched["status"], "launched", "job {n}: {launched}");
        workers.push(launched["overseer_run_id"].as_str().unwrap().to_owned());
    }
    assert_eq!(workers.len(), 32);
    assert_eq!(workers.iter().collect::<std::collections::HashSet<_>>().len(), 32);
    let until = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        let running = workers.iter().filter(|worker| d.run(worker)["status"] == "running").count();
        if running == 32 { break; }
        assert!(std::time::Instant::now() < until, "only {running}/32 workers running");
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    let page = d.call("swarm.jobs", json!({"id":id,"limit":40}));
    let rows = page["jobs"].as_array().unwrap();
    for (n, worker) in workers.iter().enumerate() {
        let linked = &rows.iter().find(|row| row["id"] == format!("j{n}"))
            .unwrap()["worker_runs"];
        assert_eq!(linked.as_array().unwrap().len(), 1, "job {n} should show its worker");
        assert_eq!(linked[0]["overseer_run_id"], *worker);
        assert_eq!(linked[0]["status"], "running");
        assert!(linked[0].get("token").is_none(), "worker detail must not expose broker credentials");
    }
    assert_eq!(rows.iter().find(|row| row["id"] == "j32").unwrap()["worker_runs"]
        .as_array().unwrap().len(), 0);
    let state = d.call("state", json!({}));
    let worker_state = state["runs"].as_array().unwrap().iter()
        .find(|row| row["id"] == workers[0]).unwrap();
    assert_eq!(worker_state["swarm_membership"]["role"], "worker");
    assert_eq!(worker_state["swarm_membership"]["run_id"], id);
    assert_eq!(worker_state["swarm_membership"]["job_id"], "j0");
    let db_probe = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    for worker in &workers {
        let run_dir: String = db_probe.query_row("SELECT run_dir FROM runs WHERE id=?1",
            [worker], |row| row.get(0)).unwrap();
        let shim: serde_json::Value = serde_json::from_slice(&std::fs::read(
            std::path::Path::new(&run_dir).join("shim.json")).unwrap()).unwrap();
        assert!(pid_alive(shim["child_pid"].as_i64().unwrap()),
            "worker {worker} has no live supervised process");
    }
    let first_attempt = first_attempt.unwrap();
    d.call("swarm.report", json!({"run_id":id,"job_id":"j0",
        "attempt_id":first_attempt["attempt_id"],"token":first_attempt["token"],
        "message_id":"scale-discovery","type":"discovery","revision":1,
        "payload":{"finding":"module zero checked"}}));
    let director = d.call("swarm.director.claim_batch", json!({"run_id":id,
        "generation":1,"revision":1,"now_ms":now()+6000}));
    assert_eq!(director["status"], "claimed");
    assert_eq!(director["messages"][0]["message_id"], "scale-discovery");
    assert_eq!(admit(32)["reason"], "worker_limit");
    d.call("swarm.director.complete_batch", json!({"run_id":id,"generation":1,
        "turn_id":director["turn_id"],"token":director["token"]}));
    assert_eq!(d.call("swarm.get", json!({"id":id}))["status"], "running");
    d.call("swarm.stop", json!({"run_id":id,"generation":1,"revision":1}));
    for worker in &workers {
        assert_ne!(d.wait_done(worker, 10)["status"], "completed");
    }
}

#[test]
fn swarm_writers_keep_conflicting_changes_out_of_a_dirty_source_checkout() {
    let d = Daemon::start(&[]);
    let temp = tmp();
    let checkout = repo(&temp.path().join("dirty-swarm-source"));
    std::fs::write(checkout.join("a.txt"), "user edit\n").unwrap();
    let source_before = fingerprint(&checkout);
    let run = d.call("swarm.create", json!({"category":"Isolated writers",
        "objective":"Inspect independent modules","allowed_targets":["fixture-local"]}));
    let id = run["id"].as_str().unwrap();
    d.call("swarm.plan", json!({"id":id,"generation":1,"revision":0,"jobs":[
        {"id":"first","title":"First writer","acceptance":"evidence","deps":[]},
        {"id":"second","title":"Second writer","acceptance":"evidence","deps":[]}
    ]}));
    commit_beneficial_batch(&d, id, &["first".into(), "second".into()]);
    let at = now();
    let mut workers = Vec::new();
    for (job, content) in [("first", "first worker\n"), ("second", "second worker\n")] {
        let admitted = d.call("swarm.admit", json!({"run_id":id,"generation":1,
            "revision":1,"job_id":job,"target_id":"fixture-local",
            "request_id":format!("isolated-{job}"),"now_ms":at,
            "snapshot":{"version":1,"observed_ms":at-1000,"expires_ms":at+60000,
                "targets":[{"id":"fixture-local","account_id":"fixture",
                    "pool_ids":["fixture-pool"],"capabilities":["code"],
                    "health":"up","auth":"ok"}],
                "pools":[{"id":"fixture-pool","windows":[{"id":"run",
                    "unit":"points","remaining_milli":100000,
                    "protected_milli":0,"reserved_milli":0,"confidence":"exact",
                    "expires_ms":at+60000}]}]},
            "required_capabilities":["code"],
            "estimate_milli":{"points":100},"purpose":"worker"}));
        assert_eq!(admitted["status"], "admitted", "{admitted}");
        let launched = d.call("swarm.worker.launch", json!({"run_id":id,"job_id":job,
            "attempt_id":admitted["attempt_id"],"token":admitted["token"],
            "repo":checkout,"program":"/bin/sleep","args":["30"],
            "prompt":"Inspect","title":format!("Fixture {job}")}));
        assert_eq!(launched["status"], "launched", "{launched}");
        let worker = launched["overseer_run_id"].as_str().unwrap().to_string();
        let state = d.call("state", json!({}));
        let run_state = state["runs"].as_array().unwrap().iter()
            .find(|r| r["id"] == worker).unwrap();
        let workspace = state["workspaces"].as_array().unwrap().iter()
            .find(|w| w["id"] == run_state["workspace_id"]).unwrap();
        assert_eq!(workspace["kind"], "worktree");
        let path = std::path::PathBuf::from(workspace["path"].as_str().unwrap());
        assert_eq!(std::fs::read_to_string(path.join("a.txt")).unwrap(), "a\n");
        std::fs::write(path.join("a.txt"), content).unwrap();
        workers.push((worker, path, content));
    }
    assert_ne!(workers[0].1, workers[1].1);
    for (_, path, expected) in &workers {
        assert_eq!(std::fs::read_to_string(path.join("a.txt")).unwrap(), *expected);
    }
    assert_eq!(fingerprint(&checkout), source_before);
    d.call("swarm.stop", json!({"run_id":id,"generation":1,"revision":1}));
    for (worker, _, _) in &workers {
        assert_ne!(d.wait_done(worker, 10)["status"], "completed");
    }
    assert_eq!(fingerprint(&checkout), source_before);
}

#[test]
fn job_deadline_interrupts_only_its_worker_despite_progress() {
    let d = Daemon::start(&[]);
    let temp = tmp();
    let checkout = repo(&temp.path().join("job-deadline-source"));
    let run = d.call("swarm.create",json!({"category":"Job deadline",
        "objective":"Inspect two paths","allowed_targets":["fixture-local"],
        "policy":{"deadline_ms":45000}}));
    let id = run["id"].as_str().unwrap();
    d.call("swarm.plan",json!({"id":id,"generation":1,"revision":0,"jobs":[
        {"id":"inspect","title":"Inspect","acceptance":"evidence","deps":[]},
        {"id":"followup","title":"Follow up","acceptance":"evidence","deps":[]}
    ]}));
    commit_beneficial_batch(&d, id, &["inspect".into(), "followup".into()]);
    let at=now();
    let admission_request=json!({"run_id":id,"generation":1,"revision":1,
        "job_id":"inspect","target_id":"fixture-local","request_id":"job-deadline-worker",
        "job_deadline_ms":3000,"now_ms":at,
        "snapshot":{"version":1,"observed_ms":at-1000,"expires_ms":at+60000,
            "targets":[{"id":"fixture-local","account_id":"fixture","pool_ids":["fixture-pool"],
                "capabilities":["code"],"health":"up","auth":"ok"}],
            "pools":[{"id":"fixture-pool","windows":[{"id":"run","unit":"points",
                "remaining_milli":100000,"protected_milli":0,"reserved_milli":0,
                "confidence":"exact","expires_ms":at+60000}]}]},
        "required_capabilities":["code"],"estimate_milli":{"points":100},"purpose":"worker"});
    let admitted=d.call("swarm.admit",admission_request.clone());
    assert_eq!(admitted["status"],"admitted","{admitted}");
    let mut other_request=admission_request;
    other_request["job_id"]=json!("followup");
    other_request["request_id"]=json!("job-deadline-other-worker");
    other_request["job_deadline_ms"]=json!(30000);
    let other=d.call("swarm.admit",other_request);
    assert_eq!(other["status"],"admitted","{other}");
    let launched=d.call("swarm.worker.launch",json!({"run_id":id,"job_id":"inspect",
        "attempt_id":admitted["attempt_id"],"token":admitted["token"],"repo":checkout,
        "program":"/bin/sleep","args":["30"],"prompt":"Inspect",
        "title":"Job deadline worker"}));
    let worker=launched["overseer_run_id"].as_str().unwrap();
    let other_launched=d.call("swarm.worker.launch",json!({"run_id":id,"job_id":"followup",
        "attempt_id":other["attempt_id"],"token":other["token"],"repo":checkout,
        "program":"/bin/sleep","args":["30"],"prompt":"Follow up",
        "title":"Other job worker"}));
    let other_worker=other_launched["overseer_run_id"].as_str().unwrap();
    let original_deadline=d.call("swarm.jobs",json!({"id":id}))["jobs"]
        .as_array().unwrap().iter().find(|j|j["id"]=="inspect").unwrap()["deadline_at_ms"].as_i64().unwrap();
    for seq in 0..5 {
        d.call("swarm.report",json!({"run_id":id,"job_id":"inspect",
            "attempt_id":admitted["attempt_id"],"token":admitted["token"],
            "message_id":format!("job-deadline-progress-{seq}"),"type":"progress",
            "revision":1,"payload":{"note":"working"}}));
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    let until=std::time::Instant::now()+std::time::Duration::from_secs(8);
    loop {
        let jobs=d.call("swarm.jobs",json!({"id":id}))["jobs"].as_array().unwrap().clone();
        let inspect=jobs.iter().find(|j|j["id"]=="inspect").unwrap();
        assert_eq!(inspect["deadline_at_ms"],original_deadline);
        if inspect["status"]=="failed" { break; }
        assert!(std::time::Instant::now()<until,"job deadline did not fail job: {inspect}");
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    assert_ne!(d.wait_done(worker,5)["status"],"completed");
    assert_eq!(d.call("swarm.get",json!({"id":id}))["status"],"running");
    let jobs=d.call("swarm.jobs",json!({"id":id}))["jobs"].as_array().unwrap().clone();
    assert_eq!(jobs.iter().find(|j|j["id"]=="followup").unwrap()["status"],"reserved");
    assert!(["queued","starting","running"].contains(&d.run(other_worker)["status"].as_str().unwrap()));
    assert_eq!(jobs.iter().find(|j|j["id"]=="inspect").unwrap()["attempt_count"],1);
    d.call("swarm.stop",json!({"run_id":id,"generation":1,"revision":1}));
    d.wait_done(other_worker,5);
}

#[test]
fn deadline_interrupts_a_linked_worker_without_another_admission() {
    let d = Daemon::start(&[]);
    let temp = tmp();
    let checkout = repo(&temp.path().join("deadline-source"));
    let run = d.call("swarm.create", json!({"category":"Deadline worker",
        "objective":"Inspect backend","allowed_targets":["fixture-local"],
        "policy":{"deadline_ms":2500}}));
    let id = run["id"].as_str().unwrap();
    d.call("swarm.plan",json!({"id":id,"generation":1,"revision":0,"jobs":[
        {"id":"inspect","title":"Inspect","acceptance":"evidence","deps":[]}
    ]}));
    let at = now();
    let admitted = d.call("swarm.admit",json!({"run_id":id,"generation":1,"revision":1,
        "job_id":"inspect","target_id":"fixture-local","request_id":"deadline-worker",
        "now_ms":at,"snapshot":{"version":1,"observed_ms":at-1000,"expires_ms":at+60000,
            "targets":[{"id":"fixture-local","account_id":"fixture","pool_ids":["fixture-pool"],
                "capabilities":["code"],"health":"up","auth":"ok"}],
            "pools":[{"id":"fixture-pool","windows":[{"id":"run","unit":"points",
                "remaining_milli":100000,"protected_milli":0,"reserved_milli":0,
                "confidence":"exact","expires_ms":at+60000}]}]},
        "required_capabilities":["code"],"estimate_milli":{"points":100},"purpose":"worker"}));
    let launched = d.call("swarm.worker.launch",json!({"run_id":id,"job_id":"inspect",
        "attempt_id":admitted["attempt_id"],"token":admitted["token"],"repo":checkout,
        "program":"/bin/sleep","args":["30"],"prompt":"Inspect",
        "title":"Deadline worker"}));
    assert_eq!(launched["status"],"launched");
    let worker = launched["overseer_run_id"].as_str().unwrap();
    let created_ms = run["created_ms"].as_i64().unwrap();
    for seq in 0..10 {
        d.call("swarm.report",json!({"run_id":id,"job_id":"inspect",
            "attempt_id":admitted["attempt_id"],"token":admitted["token"],
            "message_id":format!("deadline-progress-{seq}"),"type":"progress",
            "revision":1,"payload":{"note":"still working"}}));
        std::thread::sleep(std::time::Duration::from_millis(120));
    }
    let after_progress = d.call("swarm.get",json!({"id":id}));
    assert_eq!(after_progress["created_ms"],created_ms);
    assert_eq!(after_progress["policy"]["effective"]["deadline_ms"],2500);
    let deadline = std::time::Instant::now()+std::time::Duration::from_secs(5);
    while !["stopping","stopped"].contains(&d.call("swarm.get",json!({"id":id}))["status"].as_str().unwrap_or("")) {
        assert!(std::time::Instant::now()<deadline,"deadline did not stop run: {}",d.call("swarm.get",json!({"id":id})));
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    assert_eq!(d.call("swarm.get",json!({"id":id}))["stop_reason"],"deadline");
    assert_ne!(d.wait_done(worker,5)["status"],"completed");
    let until = std::time::Instant::now()+std::time::Duration::from_secs(5);
    loop {
        let inbox = d.call("swarm.messages",json!({"run_id":id,"recipient":"director"}));
        if inbox["messages"].as_array().unwrap().iter().any(|m|m["type"]=="terminal") {
            break;
        }
        assert!(std::time::Instant::now()<until,"terminal event never reached the director");
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    let job=&d.call("swarm.jobs",json!({"id":id}))["jobs"][0];
    assert_eq!(job["status"],"cancelled");
    assert_eq!(job["attempt_count"],1);
}

#[test]
fn admitted_worker_launch_replays_to_one_supervised_run_after_daemon_restart() {
    let mut d = Daemon::start(&[]);
    let temp = tmp();
    let checkout = repo(&temp.path().join("worker-source"));
    let run = d.call(
        "swarm.create",
        json!({"category":"Runtime bridge",
        "objective":"Inspect backend", "allowed_targets":["fixture-local"]}),
    );
    let id = run["id"].as_str().unwrap();
    d.call(
        "swarm.plan",
        json!({"id":id,"generation":1,"revision":0,"jobs":[
            {"id":"inspect","title":"Inspect","acceptance":"report evidence","deps":[]},
            {"id":"followup","title":"Follow up","acceptance":"report evidence","deps":[]}
        ]}),
    );
    commit_beneficial_batch(&d, id, &["inspect".into(), "followup".into()]);
    let at = now();
    let admitted = d.call(
        "swarm.admit",
        json!({"run_id":id,"generation":1,"revision":1,
        "job_id":"inspect","target_id":"fixture-local","request_id":"worker-one",
        "now_ms":at,"snapshot":{"version":1,"observed_ms":at-1000,"expires_ms":at+60000,
            "targets":[{"id":"fixture-local","account_id":"fixture","pool_ids":["fixture-pool"],
                "capabilities":["code"],"health":"up","auth":"ok"}],
            "pools":[{"id":"fixture-pool","windows":[{"id":"run","unit":"points",
                "remaining_milli":100000,"protected_milli":0,"reserved_milli":0,
                "confidence":"exact","expires_ms":at+60000}]}]},
        "required_capabilities":["code"],"estimate_milli":{"points":100},"purpose":"worker"}),
    );
    assert_eq!(admitted["status"], "admitted");
    let request = json!({"run_id":id,"job_id":"inspect","attempt_id":admitted["attempt_id"],
        "token":admitted["token"],"repo":checkout,"program":"/bin/sleep",
        "args":["30"],"prompt":"Inspect the backend","title":"Swarm inspect"});
    let launched = d.call("swarm.worker.launch", request.clone());
    assert_eq!(launched["status"], "launched");
    let worker_run = launched["overseer_run_id"].as_str().unwrap();
    for prefix in ["terminal-", "stop-", "deadline-checkpoint-", "contamination-"] {
        assert!(d.try_call("swarm.report",json!({"run_id":id,"job_id":"inspect",
            "attempt_id":admitted["attempt_id"],"token":admitted["token"],
            "message_id":format!("{prefix}{}",admitted["attempt_id"].as_str().unwrap()),
            "type":"progress","revision":1,"payload":{"note":"still running"}})).is_err(),
            "worker could claim daemon-owned {prefix} message id");
    }
    assert!(
        ["queued", "starting", "running"].contains(&d.run(worker_run)["status"].as_str().unwrap())
    );
    assert!(d
        .try_call(
            "swarm.attempt.confirm_exit",
            json!({"run_id":id,"generation":1,
        "revision":1,"job_id":"inspect","attempt_id":admitted["attempt_id"]})
        )
        .is_err());
    let sampled_by = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let first_sample = loop {
        let observation = d.call("swarm.worker.liveness",json!({"run_id":id,
            "job_id":"inspect","attempt_id":admitted["attempt_id"]}));
        if observation["state"] == "reachable" {
            break observation;
        }
        assert!(std::time::Instant::now() < sampled_by, "daemon did not sample worker reachability: {observation}");
        std::thread::sleep(std::time::Duration::from_millis(100));
    };
    assert_eq!(
        d.call("swarm.worker.reconcile",json!({"run_id":id,"generation":1,
            "revision":1,"job_id":"inspect","attempt_id":admitted["attempt_id"]}))["status"],
        "active"
    );
    let db_probe = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let run_dir: String = db_probe.query_row("SELECT run_dir FROM runs WHERE id=?1",
        rusqlite::params![worker_run], |r|r.get(0)).unwrap();
    let launch: serde_json::Value = serde_json::from_slice(&std::fs::read(
        std::path::Path::new(&run_dir).join("launch.json")).unwrap()).unwrap();
    let socket = std::path::PathBuf::from(launch["control_socket"].as_str().unwrap());
    let hidden_socket = socket.with_extension("unreachable");
    std::fs::rename(&socket, &hidden_socket).unwrap();
    let poll_at = first_sample["last_sample_ms"].as_i64().unwrap() + 15_000;
    for step in 0..=4 {
        let polled = d.call("swarm.worker.liveness.poll",json!({"now_ms":poll_at+step*15_000}));
        assert_eq!(polled["sampled"],1,"{polled}");
        let state = d.call("swarm.worker.liveness",json!({"run_id":id,
            "job_id":"inspect","attempt_id":admitted["attempt_id"]}));
        assert_eq!(state["state"], if step == 4 { "unknown" } else { "suspect" }, "{state}");
    }
    assert_eq!(d.call("swarm.worker.reconcile",json!({"run_id":id,"generation":1,
        "revision":1,"job_id":"inspect","attempt_id":admitted["attempt_id"]}))["status"], "unknown");
    std::fs::rename(&hidden_socket, &socket).unwrap();
    let recovered = d.call("swarm.worker.liveness.poll",json!({"now_ms":poll_at+75_000}));
    assert_eq!(recovered["sampled"],1,"{recovered}");
    assert_eq!(d.call("swarm.worker.liveness",json!({"run_id":id,
        "job_id":"inspect","attempt_id":admitted["attempt_id"]}))["state"], "reachable");
    let probe_at = poll_at + 75_001;
    let sample = |time, reachable| d.call("swarm.worker.liveness.sample", json!({
        "run_id":id,"job_id":"inspect","attempt_id":admitted["attempt_id"],
        "now_ms":time,"reachable":reachable}));
    assert_eq!(sample(probe_at, true)["state"], "reachable");
    assert_eq!(sample(probe_at + 1, false)["state"], "suspect");
    d.call("swarm.report", json!({"run_id":id,"job_id":"inspect",
        "attempt_id":admitted["attempt_id"],"token":admitted["token"],
        "message_id":"progress-does-not-reset-liveness","type":"progress",
        "revision":1,"payload":{"note":"still working"}}));
    assert_eq!(sample(probe_at + 60_000, false)["state"], "suspect");
    let unknown_after_threshold = sample(probe_at + 60_001, false);
    assert_eq!(unknown_after_threshold["state"], "unknown");
    assert_eq!(unknown_after_threshold["unreachable_since_ms"], probe_at + 1);
    assert!(d.try_call("swarm.worker.liveness.sample",json!({"run_id":id,
        "job_id":"inspect","attempt_id":admitted["attempt_id"],
        "now_ms":probe_at + 60_000,"reachable":true})).is_err());
    assert_eq!(d.call("swarm.worker.liveness",json!({"run_id":id,
        "job_id":"inspect","attempt_id":admitted["attempt_id"]}))["state"], "unknown");
    assert_eq!(d.call("swarm.worker.reconcile",json!({"run_id":id,"generation":1,
        "revision":1,"job_id":"inspect","attempt_id":admitted["attempt_id"]}))["status"], "unknown");
    assert_eq!(sample(probe_at + 60_002, true)["state"], "reachable");
    assert_eq!(d.call("swarm.worker.reconcile",json!({"run_id":id,"generation":1,
        "revision":1,"job_id":"inspect","attempt_id":admitted["attempt_id"]}))["status"], "active");
    let suspect = sample(probe_at + 60_003, false);
    assert_eq!(suspect["state"], "suspect");
    assert_eq!(suspect["sample_interval_ms"], 1_000);
    let retried = d.call("swarm.worker.liveness.poll",json!({"now_ms":probe_at + 61_003}));
    assert_eq!(retried["sampled"], 1, "suspect worker was not retried promptly: {retried}");
    assert_eq!(d.call("swarm.worker.liveness",json!({"run_id":id,
        "job_id":"inspect","attempt_id":admitted["attempt_id"]}))["state"], "reachable");
    let attempts: i64 = rusqlite::Connection::open(d.home.path().join("overseer.sqlite"))
        .unwrap().query_row("SELECT attempt_count FROM swarm_jobs WHERE run_id=?1 AND id='inspect'",
        rusqlite::params![id], |r|r.get(0)).unwrap();
    assert_eq!(attempts, 1);
    // A lost supervisor/transport is not proof that its child process exited.
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    db.execute("UPDATE runs SET status='disconnected',ended_ms=?2 WHERE id=?1",
        rusqlite::params![worker_run, now()]).unwrap();
    let unknown = d.call("swarm.worker.reconcile",json!({"run_id":id,"generation":1,
        "revision":1,"job_id":"inspect","attempt_id":admitted["attempt_id"]}));
    assert_eq!(unknown["status"], "unknown", "{unknown}");
    assert!(d.try_call("swarm.attempt.confirm_exit",json!({"run_id":id,
        "generation":1,"revision":1,"job_id":"inspect",
        "attempt_id":admitted["attempt_id"]})).is_err());
    std::thread::sleep(std::time::Duration::from_millis(1200));
    assert_eq!(d.call("swarm.worker.reconcile",json!({"run_id":id,"generation":1,
        "revision":1,"job_id":"inspect","attempt_id":admitted["attempt_id"]}))["status"], "unknown");
    let attempt_status: String = db.query_row("SELECT status FROM swarm_attempts WHERE id=?1",
        rusqlite::params![admitted["attempt_id"].as_str().unwrap()], |r|r.get(0)).unwrap();
    assert_eq!(attempt_status, "registered");
    let reservations: i64 = db.query_row("SELECT COUNT(*) FROM swarm_reservations WHERE attempt_id=?1 AND status='active'",
        rusqlite::params![admitted["attempt_id"].as_str().unwrap()], |r|r.get(0)).unwrap();
    assert!(reservations > 0);
    db.execute("UPDATE runs SET status='running',ended_ms=NULL WHERE id=?1",
        rusqlite::params![worker_run]).unwrap();
    let jobs = d.call("swarm.jobs", json!({"id":id}))["jobs"]
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(
        jobs.iter().find(|job| job["id"] == "inspect").unwrap()["status"],
        "reserved"
    );
    let second = d.call(
        "swarm.admit",
        json!({"run_id":id,"generation":1,"revision":1,
        "job_id":"followup","target_id":"fixture-local","request_id":"worker-two",
        "now_ms":at,"snapshot":{"version":1,"observed_ms":at-1000,"expires_ms":at+60000,
            "targets":[{"id":"fixture-local","account_id":"fixture","pool_ids":["fixture-pool"],
                "capabilities":["code"],"health":"up","auth":"ok"}],
            "pools":[{"id":"fixture-pool","windows":[{"id":"run","unit":"points",
                "remaining_milli":100000,"protected_milli":0,"reserved_milli":0,
                "confidence":"exact","expires_ms":at+60000}]}]},
        "required_capabilities":["code"],"estimate_milli":{"points":100},"purpose":"worker"}),
    );
    assert_eq!(second["status"], "admitted", "{second}");
    d.kill9();
    d.spawn();
    assert_eq!(d.call("swarm.worker.liveness",json!({"run_id":id,
        "job_id":"inspect","attempt_id":admitted["attempt_id"]}))["state"], "reachable");
    let replay = d.call("swarm.worker.launch", request.clone());
    assert_eq!(replay["overseer_run_id"], worker_run);
    assert_eq!(replay["duplicate"], true);
    assert_eq!(
        d.runs()
            .iter()
            .filter(|run| run["title"] == "Swarm inspect")
            .count(),
        1
    );
    let mut changed = request;
    changed["args"] = json!(["1"]);
    assert!(d.try_call("swarm.worker.launch", changed).is_err());
    d.call("swarm.revise",json!({"id":id,"generation":1,"expected_revision":1,
        "reason":"Inspect a newly identified authorization path", "jobs":[
            {"id":"inspect","title":"Inspect","acceptance":"report both paths","deps":[]},
            {"id":"followup","title":"Follow up","acceptance":"report evidence","deps":[]}
        ]}));
    d.call(
        "swarm.stop",
        json!({"run_id":id,"generation":1,"revision":2}),
    );
    d.wait_done(worker_run, 5);
    let reconciled = d.call(
        "swarm.worker.reconcile",
        json!({"run_id":id,"generation":1,
        "revision":2,"job_id":"inspect","attempt_id":admitted["attempt_id"]}),
    );
    assert_eq!(reconciled["status"], "terminal");
    assert_eq!(
        d.call(
            "swarm.worker.reconcile",
            json!({"run_id":id,"generation":1,
        "revision":2,"job_id":"inspect","attempt_id":admitted["attempt_id"]})
        )["duplicate"],
        true
    );
    let inbox = d.call(
        "swarm.messages",
        json!({"run_id":id,"recipient":"director"}),
    )["messages"]
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(
        inbox
            .iter()
            .filter(|message| message["type"] == "terminal")
            .count(),
        1
    );
    assert_eq!(
        d.call("swarm.jobs", json!({"id":id}))["jobs"]
            .as_array()
            .unwrap()
            .iter()
            .find(|job| job["id"] == "inspect")
            .unwrap()["status"],
        "cancelled"
    );
}

#[test]
fn pending_worker_launch_cannot_resume_after_stop() {
    let d = Daemon::start(&[]);
    let temp = tmp();
    let checkout = repo(&temp.path().join("stopped-source"));
    let run = d.call(
        "swarm.create",
        json!({"category":"Stopped pending launch",
        "objective":"Inspect backend","allowed_targets":["fixture-local"]}),
    );
    let id = run["id"].as_str().unwrap();
    d.call(
        "swarm.plan",
        json!({"id":id,"generation":1,"revision":0,"jobs":[
            {"id":"inspect","title":"Inspect","acceptance":"evidence","deps":[]}
        ]}),
    );
    let at = now();
    let admitted = d.call(
        "swarm.admit",
        json!({"run_id":id,"generation":1,"revision":1,
        "job_id":"inspect","target_id":"fixture-local","request_id":"stopped-worker",
        "now_ms":at,"snapshot":{"version":1,"observed_ms":at-1000,"expires_ms":at+60000,
            "targets":[{"id":"fixture-local","account_id":"fixture","pool_ids":["fixture-pool"],
                "capabilities":["code"],"health":"up","auth":"ok"}],
            "pools":[{"id":"fixture-pool","windows":[{"id":"run","unit":"points",
                "remaining_milli":100000,"protected_milli":0,"reserved_milli":0,
                "confidence":"exact","expires_ms":at+60000}]}]},
        "required_capabilities":["code"],"estimate_milli":{"points":100},"purpose":"worker"}),
    );
    let request = json!({"run_id":id,"job_id":"inspect","attempt_id":admitted["attempt_id"],
        "token":admitted["token"],"repo":checkout,"program":"/bin/sleep",
        "args":["30"],"prompt":"Inspect the backend","title":"Stopped worker"});
    let digest = format!("{:x}", Sha256::digest(request.to_string().as_bytes()));
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    db.execute(
        "INSERT INTO swarm_worker_launches(attempt_id,run_id,job_id,request_sha256,created_ms)
        VALUES(?1,?2,'inspect',?3,?4)",
        rusqlite::params![admitted["attempt_id"].as_str().unwrap(), id, digest, at],
    )
    .unwrap();
    d.call(
        "swarm.stop",
        json!({"run_id":id,"generation":1,"revision":1}),
    );
    assert!(d.try_call("swarm.worker.launch", request).is_err());
    assert!(d.runs().is_empty());
}

#[test]
fn parent_exit_waits_for_native_descendant_receipts_before_finishing_attempt() {
    let d = Daemon::start(&[]);
    let temp = tmp();
    let checkout = repo(&temp.path().join("descendant-order"));
    let run = d.call("swarm.create", json!({"category":"Descendant ordering",
        "objective":"Inspect an authorized backend","allowed_targets":["fixture-local"]}));
    let id = run["id"].as_str().unwrap();
    d.call("swarm.plan",json!({"id":id,"generation":1,"revision":0,"jobs":[
        {"id":"inspect","title":"Inspect","acceptance":"report evidence","deps":[]},
        {"id":"other","title":"Other","acceptance":"report evidence","deps":[]}
    ]}));
    commit_beneficial_batch(&d,id,&["inspect".into(),"other".into()]);
    let at = now();
    let admitted = d.call("swarm.admit",json!({"run_id":id,"generation":1,"revision":1,
        "job_id":"inspect","target_id":"fixture-local","request_id":"descendant-order",
        "now_ms":at,"snapshot":{"version":1,"observed_ms":at-1000,"expires_ms":at+60000,
            "targets":[{"id":"fixture-local","account_id":"fixture","pool_ids":["pool"],
                "capabilities":["code"],"health":"up","auth":"ok"}],
            "pools":[{"id":"pool","windows":[{"id":"run","unit":"points",
                "remaining_milli":100000,"protected_milli":0,"reserved_milli":0,
                "confidence":"exact","expires_ms":at+60000}]}]},
        "required_capabilities":["code"],"estimate_milli":{"points":100},"purpose":"worker"}));
    assert_eq!(admitted["status"],"admitted");
    let launched = d.call("swarm.worker.launch",json!({"run_id":id,"job_id":"inspect",
        "attempt_id":admitted["attempt_id"],"token":admitted["token"],
        "repo":checkout,"program":"/bin/sleep","args":["2"],
        "prompt":"Inspect","title":"Descendant order worker"}));
    let worker = launched["overseer_run_id"].as_str().unwrap();
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    for (parent,child,native) in [(worker,"r-native-child","native-child"),
        ("r-native-child","r-native-grandchild","native-grandchild")] {
        db.execute("INSERT INTO runs(id,task_id,parent_run_id,harness,harness_version,
            profile_id,model,workspace_id,native_id,status,exit_reason,created_ms,ended_ms,
            title,relation_source,relation_confidence,capabilities,process_generation)
            SELECT ?2,task_id,id,harness,harness_version,profile_id,model,workspace_id,
            ?3,'running',NULL,?4,NULL,'Native descendant','fixture','exact',capabilities,0
            FROM runs WHERE id=?1",rusqlite::params![parent,child,native,now()]).unwrap();
    }
    d.wait_done(worker, 6);
    let reconcile = || d.call("swarm.worker.reconcile",json!({"run_id":id,"generation":1,
        "revision":1,"job_id":"inspect","attempt_id":admitted["attempt_id"]}));
    assert_eq!(reconcile()["status"],"descendants_unconfirmed");
    let attempt = admitted["attempt_id"].as_str().unwrap();
    assert!(d.try_call("swarm.attempt.confirm_exit",json!({"run_id":id,
        "generation":1,"revision":1,"job_id":"inspect",
        "attempt_id":attempt})).is_err());
    let status: String = db.query_row("SELECT status FROM swarm_attempts WHERE id=?1",
        [attempt],|r|r.get(0)).unwrap();
    assert_eq!(status,"registered");
    let active_reservations: i64 = db.query_row("SELECT COUNT(*) FROM swarm_reservations
        WHERE attempt_id=?1 AND status='active'",[attempt],|r|r.get(0)).unwrap();
    assert!(active_reservations > 0);
    db.execute("UPDATE runs SET status='completed',ended_ms=?1 WHERE id='r-native-child'",
        rusqlite::params![now()]).unwrap();
    assert_eq!(reconcile()["status"],"descendants_unconfirmed");
    db.execute("UPDATE runs SET status='completed',ended_ms=?1 WHERE id='r-native-grandchild'",
        rusqlite::params![now()]).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let status: String = db.query_row("SELECT status FROM swarm_attempts WHERE id=?1",
            [attempt],|r|r.get(0)).unwrap();
        if status == "finished" { break; }
        assert!(std::time::Instant::now() < deadline,
            "background reconciliation did not finish descendant-confirmed attempt: {status}");
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert_eq!(reconcile()["duplicate"],true);
    let uncertain_reservations: i64 = db.query_row("SELECT COUNT(*) FROM swarm_reservations
        WHERE attempt_id=?1 AND status='uncertain'",[attempt],|r|r.get(0)).unwrap();
    assert!(uncertain_reservations > 0);
    let count: i64 = db.query_row("SELECT COUNT(*) FROM swarm_messages WHERE run_id=?1
        AND attempt_id=?2 AND kind='terminal'",rusqlite::params![id,attempt],|r|r.get(0)).unwrap();
    assert_eq!(count,1);

    let later = now();
    let failed_child_attempt = d.call("swarm.admit",json!({"run_id":id,"generation":1,
        "revision":1,"job_id":"other","target_id":"fixture-local",
        "request_id":"descendant-failure","now_ms":later,
        "snapshot":{"version":2,"observed_ms":later-1000,"expires_ms":later+60000,
            "targets":[{"id":"fixture-local","account_id":"fixture","pool_ids":["pool"],
                "capabilities":["code"],"health":"up","auth":"ok"}],
            "pools":[{"id":"pool","windows":[{"id":"run","unit":"points",
                "remaining_milli":100000,"protected_milli":0,"reserved_milli":0,
                "confidence":"exact","expires_ms":later+60000}]}]},
        "required_capabilities":["code"],"estimate_milli":{"points":100},"purpose":"worker"}));
    assert_eq!(failed_child_attempt["status"],"admitted");
    let failed_parent = d.call("swarm.worker.launch",json!({"run_id":id,"job_id":"other",
        "attempt_id":failed_child_attempt["attempt_id"],"token":failed_child_attempt["token"],
        "repo":checkout,"program":"/bin/sleep","args":["2"],
        "prompt":"Inspect","title":"Failed descendant worker"}));
    let failed_parent_run = failed_parent["overseer_run_id"].as_str().unwrap();
    db.execute("INSERT INTO runs(id,task_id,parent_run_id,harness,harness_version,profile_id,
        model,workspace_id,native_id,status,exit_reason,created_ms,ended_ms,title,
        relation_source,relation_confidence,capabilities,process_generation)
        SELECT 'r-failed-native-child',task_id,id,harness,harness_version,profile_id,model,
        workspace_id,'failed-native-child','failed','native child failed',?2,?2,
        'Failed native child','fixture','exact',capabilities,0 FROM runs WHERE id=?1",
        rusqlite::params![failed_parent_run,now()]).unwrap();
    d.call("swarm.artifact.put",json!({"run_id":id,"job_id":"other",
        "attempt_id":failed_child_attempt["attempt_id"],"token":failed_child_attempt["token"],
        "artifact_id":"other-evidence","source_revision":1,"kind":"finding",
        "content":"parent reported a result before native child failure was considered"}));
    d.call("swarm.report",json!({"run_id":id,"job_id":"other",
        "attempt_id":failed_child_attempt["attempt_id"],"token":failed_child_attempt["token"],
        "message_id":"other-result","type":"result","revision":1,
        "payload":{"artifact_ids":["other-evidence"]}}));
    d.call("swarm.decide",json!({"run_id":id,"generation":1,"revision":1,
        "job_id":"other","decision":"accept","evidence":["other-evidence"]}));
    d.wait_done(failed_parent_run,6);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let jobs = d.call("swarm.jobs",json!({"id":id}));
        let other = jobs["jobs"].as_array().unwrap().iter()
            .find(|job| job["id"]=="other").unwrap();
        if other["status"]=="blocked" {
            assert_eq!(other["stop_reason"],"native_descendant_failed");
            break;
        }
        assert!(std::time::Instant::now()<deadline,
            "failed native descendant did not block its job: {other}");
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let failed_attempt = failed_child_attempt["attempt_id"].as_str().unwrap();
    let payload: String = db.query_row("SELECT payload FROM swarm_messages
        WHERE run_id=?1 AND attempt_id=?2 AND kind='terminal'",
        rusqlite::params![id,failed_attempt],|r|r.get(0)).unwrap();
    assert_eq!(serde_json::from_str::<serde_json::Value>(&payload).unwrap()
        ["native_descendant_failures"],1);
    let evidence_count: i64 = db.query_row("SELECT COUNT(*) FROM swarm_artifacts
        WHERE run_id=?1 AND job_id='other' AND id='other-evidence'",
        rusqlite::params![id],|r|r.get(0)).unwrap();
    assert_eq!(evidence_count,1);
}

#[test]
fn synthetic_claude_background_child_does_not_finish_swarm_attempt_at_launch_stub() {
    let fixture_path = repo_root().join("fixtures/fake-harness/claude-fixture.js")
        .display().to_string();
    let d = Daemon::start(&[("OVERSEER_CLAUDE_PATH", &fixture_path),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE"),
        ("FIXTURE_MODE", "background"), ("OVERSEER_SHARED_BOOKING_FIXTURE_API","1")]);
    let temp = tmp();
    let checkout = repo(&temp.path().join("claude-background-source"));
    let run = d.call("swarm.create",json!({"category":"Synthetic child ordering",
        "objective":"Inspect backend","allowed_targets":["fixture-claude"],
        "source_change_permission":"isolated"}));
    let id = run["id"].as_str().unwrap();
    d.call("swarm.plan",json!({"id":id,"generation":1,"revision":0,"jobs":[
        {"id":"inspect","title":"Inspect","acceptance":"evidence","deps":[]}
    ]}));
    let at = now();
    // A native (account) target books its account windows and app slot in
    // the shared booking; the fixture observation stands in for Auto's feed.
    let booking = fixture_account_booking(&d, "system-claude", "fixture-claude-account", 0.0, 1000);
    let admitted = d.call("swarm.admit",json!({"shared_booking":booking,"run_id":id,"generation":1,"revision":1,
        "job_id":"inspect","target_id":"fixture-claude","request_id":"recorded-claude-child",
        "now_ms":at,"snapshot":{"version":1,"observed_ms":at-1000,"expires_ms":at+60000,
            "targets":[{"id":"fixture-claude","harness":"claude",
                "profile_id":"system-claude","model":"sonnet","effort":"medium",
                "account_id":"fixture","pool_ids":["pool"],
                "capabilities":["code"],"health":"up","auth":"ok"}],
            "pools":[{"id":"pool","windows":[{"id":"run","unit":"points",
                "remaining_milli":100000,"protected_milli":0,"reserved_milli":0,
                "confidence":"exact","expires_ms":at+60000}]}]},
        "required_capabilities":["code"],"estimate_milli":{"points":100},"purpose":"worker"}));
    assert_eq!(admitted["status"],"admitted");
    let launched = d.call("swarm.worker.launch",json!({"run_id":id,"job_id":"inspect",
        "attempt_id":admitted["attempt_id"],"token":admitted["token"],
        "repo":checkout,"harness":"claude","args":[],
        "prompt":"Inspect","title":"Synthetic Claude worker"}));
    assert_eq!(launched["status"],"launched", "{launched}");
    let worker = launched["overseer_run_id"].as_str().unwrap();
    let run_dir: String = rusqlite::Connection::open(d.home.path().join("overseer.sqlite"))
        .unwrap().query_row("SELECT run_dir FROM runs WHERE id=?1",[worker],|row|row.get(0)).unwrap();
    let launch_file = std::fs::read_to_string(std::path::Path::new(&run_dir).join("launch.json")).unwrap();
    assert_eq!(d.run(worker)["profile_id"],"system-claude");
    assert_eq!(d.run(worker)["model"],"sonnet");
    assert!(launch_file.contains("--model") && launch_file.contains("sonnet")
        && launch_file.contains("--effort") && launch_file.contains("medium"),
        "admitted route options did not reach the harness: {launch_file}");
    assert!(launch_file.contains("--disallowedTools") && launch_file.contains("Agent,Task"),
        "Swarm worker launch did not disable native Claude delegation: {launch_file}");
    assert!(!launch_file.contains(admitted["token"].as_str().unwrap()),
        "synthetic harness must not receive the worker broker credential");
    let until = std::time::Instant::now() + std::time::Duration::from_secs(15);
    let waiting = loop {
        let state = d.run(worker);
        if state["status"]=="waiting_for_user" { break state; }
        assert!(state["status"]!="failed" && std::time::Instant::now()<until,
            "synthetic harness did not reach permission pause: {state}; events: {:?}",d.events(worker));
        std::thread::sleep(std::time::Duration::from_millis(50));
    };
    let child = d.runs().into_iter().find(|row| row["parent_run_id"]==worker)
        .expect("synthetic Agent event created a native child");
    assert_eq!(child["native_id"],"toolu_bg");
    let interim = d.call("swarm.worker.reconcile",json!({"run_id":id,"generation":1,
        "revision":1,"job_id":"inspect","attempt_id":admitted["attempt_id"]}));
    assert!(["active","suspect","unknown"].contains(&interim["status"].as_str().unwrap()),
        "an async launch stub cannot finish the attempt: {interim}");
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let attempt_status: String = db.query_row("SELECT status FROM swarm_attempts WHERE id=?1",
        [admitted["attempt_id"].as_str().unwrap()],|row|row.get(0)).unwrap();
    assert_eq!(attempt_status,"registered");
    d.call("run.permission",json!({"run_id":worker,
        "request_id":waiting["attention"]["request_id"],"allow":false}));
    assert_eq!(d.wait_done(worker,15)["status"],"completed");
    let reconciled = d.call("swarm.worker.reconcile",json!({"run_id":id,"generation":1,
        "revision":1,"job_id":"inspect","attempt_id":admitted["attempt_id"]}));
    assert_eq!(reconciled["status"],"terminal", "{reconciled}");
    assert_eq!(d.call("swarm.jobs",json!({"id":id}))["jobs"][0]["status"],"reserved");
    let duplicate = d.call("swarm.worker.reconcile",json!({"run_id":id,"generation":1,
        "revision":1,"job_id":"inspect","attempt_id":admitted["attempt_id"]}));
    assert_eq!(duplicate["duplicate"],true);
    let terminal_count: i64 = rusqlite::Connection::open(d.home.path().join("overseer.sqlite"))
        .unwrap().query_row("SELECT COUNT(*) FROM swarm_messages WHERE run_id=?1
            AND attempt_id=?2 AND kind='terminal'",
            rusqlite::params![id,admitted["attempt_id"].as_str().unwrap()],|row|row.get(0)).unwrap();
    assert_eq!(terminal_count,1);
}

#[test]
fn failed_worker_program_releases_its_execution_attempt() {
    let d=Daemon::start(&[]);
    let temp=tmp();
    let checkout=repo(&temp.path().join("missing-worker-source"));
    let made=d.call("swarm.create",json!({"category":"Missing worker program",
        "objective":"Inspect backend","allowed_targets":["fixture-local"]}));
    let id=made["id"].as_str().unwrap();
    d.call("swarm.plan",json!({"id":id,"generation":1,"revision":0,"jobs":[
        {"id":"inspect","title":"Inspect","acceptance":"evidence","deps":[]}]}));
    let at=now();
    let admitted=d.call("swarm.admit",json!({"run_id":id,"generation":1,"revision":1,
        "job_id":"inspect","target_id":"fixture-local","request_id":"missing-program",
        "now_ms":at,"snapshot":{"version":1,"observed_ms":at-1000,"expires_ms":at+60000,
            "targets":[{"id":"fixture-local","account_id":"fixture","pool_ids":["fixture-pool"],
                "capabilities":["code"],"health":"up","auth":"ok"}],
            "pools":[{"id":"fixture-pool","windows":[{"id":"run","unit":"points",
                "remaining_milli":100000,"protected_milli":0,"reserved_milli":0,
                "confidence":"exact","expires_ms":at+60000}]}]},
        "required_capabilities":["code"],"estimate_milli":{"points":100},"purpose":"worker"}));
    let launched=d.call("swarm.worker.launch",json!({"run_id":id,"job_id":"inspect",
        "attempt_id":admitted["attempt_id"],"token":admitted["token"],"repo":checkout,
        "program":"/nonexistent/overseer-swarm-worker","args":[],"prompt":"Inspect",
        "title":"Missing worker"}));
    let worker=launched["overseer_run_id"].as_str().unwrap();
    assert_eq!(d.wait_done(worker,5)["status"],"failed");
    let deadline=std::time::Instant::now()+std::time::Duration::from_secs(3);
    loop {
        let db=rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
        let status:String=db.query_row("SELECT status FROM swarm_attempts WHERE id=?1",
            [admitted["attempt_id"].as_str().unwrap()],|r|r.get(0)).unwrap();
        if status=="finished" { break; }
        assert!(std::time::Instant::now()<deadline,"failed worker kept attempt {status}");
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert_eq!(d.call("swarm.jobs",json!({"id":id}))["jobs"][0]["status"],"ready");
}

#[test]
fn stop_retries_an_initially_unreachable_worker_after_daemon_restart() {
    let mut d = Daemon::start(&[]);
    let temp = tmp();
    let checkout = repo(&temp.path().join("stop-retry-source"));
    let run = d.call("swarm.create", json!({"category":"Stop retry",
        "objective":"Inspect backend","allowed_targets":["fixture-local"]}));
    let id = run["id"].as_str().unwrap();
    d.call("swarm.plan", json!({"id":id,"generation":1,"revision":0,"jobs":[
        {"id":"inspect","title":"Inspect","acceptance":"evidence","deps":[]}
    ]}));
    let at = now();
    let admitted = d.call("swarm.admit", json!({"run_id":id,"generation":1,"revision":1,
        "job_id":"inspect","target_id":"fixture-local","request_id":"stop-retry-worker",
        "now_ms":at,"snapshot":{"version":1,"observed_ms":at-1000,"expires_ms":at+60000,
            "targets":[{"id":"fixture-local","account_id":"fixture","pool_ids":["fixture-pool"],
                "capabilities":["code"],"health":"up","auth":"ok"}],
            "pools":[{"id":"fixture-pool","windows":[{"id":"run","unit":"points",
                "remaining_milli":100000,"protected_milli":0,"reserved_milli":0,
                "confidence":"exact","expires_ms":at+60000}]}]},
        "required_capabilities":["code"],"estimate_milli":{"points":100},"purpose":"worker"}));
    let launched = d.call("swarm.worker.launch", json!({"run_id":id,"job_id":"inspect",
        "attempt_id":admitted["attempt_id"],"token":admitted["token"],"repo":checkout,
        "program":"/bin/sleep","args":["30"],"prompt":"Inspect",
        "title":"Stop retry worker"}));
    let worker = launched["overseer_run_id"].as_str().unwrap();
    d.wait_status(worker, |status| status == "running", 10);
    let stopped = d.call("swarm.stop", json!({"run_id":id,
        "fault_interrupt_once":true}));
    assert_eq!(stopped["workers"]["unconfirmed"], json!([worker]));
    assert_eq!(d.run(worker)["status"], "running");
    assert_eq!(d.call("swarm.jobs", json!({"id":id}))["jobs"][0]["status"],
        "cancel_requested");
    let pending = d.call("swarm.get", json!({"id":id}));
    assert_eq!(pending["unconfirmed_exit_count"], 1);
    assert_eq!(pending["unconfirmed_exits"][0]["overseer_run_id"], worker);
    assert_eq!(pending["unconfirmed_exits"][0]["last_signal_outcome"], "unconfirmed");
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let first: (i64,String) = db.query_row(
        "SELECT attempts,last_outcome FROM swarm_stop_signals WHERE run_id=?1 AND overseer_run_id=?2",
        rusqlite::params![id,worker], |r| Ok((r.get(0)?,r.get(1)?))).unwrap();
    assert_eq!(first, (1,"unconfirmed".into()));
    let reservation_before: String = db.query_row(
        "SELECT status FROM swarm_reservations WHERE attempt_id=?1",
        [admitted["attempt_id"].as_str().unwrap()], |r| r.get(0)).unwrap();
    assert_eq!(reservation_before, "active");
    d.kill9();
    d.spawn();
    assert_eq!(d.wait_done(worker, 12)["status"], "interrupted");
    d.call("swarm.attempt.confirm_exit", json!({"run_id":id,"job_id":"inspect",
        "attempt_id":admitted["attempt_id"],"generation":1,"revision":1}));
    let reservation_after: String = db.query_row(
        "SELECT status FROM swarm_reservations WHERE attempt_id=?1",
        [admitted["attempt_id"].as_str().unwrap()], |r| r.get(0)).unwrap();
    assert_eq!(reservation_after, "uncertain");
    let later: (i64,String) = db.query_row(
        "SELECT attempts,last_outcome FROM swarm_stop_signals WHERE run_id=?1 AND overseer_run_id=?2",
        rusqlite::params![id,worker], |r| Ok((r.get(0)?,r.get(1)?))).unwrap();
    assert!(later.0 >= 2, "stop interrupt was not retried: {later:?}");
    assert_eq!(later.1, "requested");
    let after = d.call("swarm.get", json!({"id":id}));
    assert_eq!(after["status"], "stopped");
    assert_eq!(after["unconfirmed_exit_count"], 0);
    assert_eq!(d.call("swarm.jobs", json!({"id":id}))["jobs"][0]["attempt_count"], 1);
}

#[test]
fn slow_worker_stop_signal_does_not_hold_other_swarm_admissions() {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixStream;
    use std::time::{Duration, Instant};

    struct ResumeShim(i64);
    impl Drop for ResumeShim {
        fn drop(&mut self) { signal(self.0, libc::SIGCONT); }
    }

    let d = Daemon::start(&[]);
    let temp = tmp();
    let checkout = repo(&temp.path().join("slow-stop-source"));
    let made = d.call("swarm.create",json!({"category":"Slow stop socket",
        "objective":"Audit", "allowed_targets":["fixture-local"]}));
    let id = made["id"].as_str().unwrap();
    d.call("swarm.plan",json!({"id":id,"generation":1,"revision":0,"jobs":[
        {"id":"inspect","title":"Inspect","acceptance":"evidence","deps":[]}]}));
    let at = now();
    let admitted = d.call("swarm.admit",json!({"run_id":id,"generation":1,"revision":1,
        "job_id":"inspect","target_id":"fixture-local","request_id":"slow-stop",
        "now_ms":at,"snapshot":{"version":1,"observed_ms":at-1000,"expires_ms":at+60000,
            "targets":[{"id":"fixture-local","account_id":"fixture","pool_ids":["fixture-pool"],
                "capabilities":["code"],"health":"up","auth":"ok"}],
            "pools":[{"id":"fixture-pool","windows":[{"id":"run","unit":"points",
                "remaining_milli":100000,"protected_milli":0,"reserved_milli":0,
                "confidence":"exact","expires_ms":at+60000}]}]},
        "required_capabilities":["code"],"estimate_milli":{"points":100},"purpose":"worker"}));
    let launched = d.call("swarm.worker.launch",json!({"run_id":id,"job_id":"inspect",
        "attempt_id":admitted["attempt_id"],"token":admitted["token"],"repo":checkout,
        "program":"/bin/sleep","args":["30"],"prompt":"Inspect","title":"Slow socket worker"}));
    let worker = launched["overseer_run_id"].as_str().unwrap();
    d.wait_status(worker, |status| status == "running", 10);
    let (shim, run_dir) = launch_info(&d, worker);
    let control: serde_json::Value = serde_json::from_slice(
        &std::fs::read(run_dir.join("launch.json")).unwrap()).unwrap();
    let mut ping = UnixStream::connect(control["control_socket"].as_str().unwrap()).unwrap();
    ping.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
    ping.write_all(b"{\"op\":\"ping\"}\n").unwrap();
    let mut reply = String::new();
    BufReader::new(ping).read_line(&mut reply).unwrap();
    assert!(reply.contains("\"ok\":true"),"{reply}");

    let shim_pid = shim["shim_pid"].as_i64().unwrap();
    signal(shim_pid, libc::SIGSTOP);
    let resume = ResumeShim(shim_pid);
    let socket = d.socket();
    let id_for_stop = id.to_string();
    let stop = std::thread::spawn(move || {
        let mut conn = UnixStream::connect(socket).unwrap();
        conn.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        conn.write_all(format!("{}\n",json!({"id":1,"method":"swarm.stop",
            "params":{"run_id":id_for_stop}})).as_bytes()).unwrap();
        let mut reply = String::new();
        BufReader::new(conn).read_line(&mut reply).unwrap();
        serde_json::from_str::<serde_json::Value>(&reply).unwrap()
    });
    let ready_by = Instant::now() + Duration::from_secs(3);
    while d.call("swarm.get",json!({"id":id}))["status"] != "stopping" {
        assert!(Instant::now() < ready_by,"Stop did not commit before worker control I/O");
        std::thread::sleep(Duration::from_millis(20));
    }
    let started = Instant::now();
    assert!(d.try_call("swarm.admit",json!({})).unwrap_err().contains("run_id"));
    assert!(started.elapsed() < Duration::from_secs(3),
        "a blocked worker control socket held the launch lock for another request");
    drop(resume);
    let stopped = stop.join().unwrap();
    assert_eq!(stopped["result"]["status"],"stopping","{stopped}");
}

#[test]
fn finished_attempt_cannot_launch_a_worker() {
    let d = Daemon::start(&[]);
    let temp = tmp();
    let checkout = repo(&temp.path().join("finished-source"));
    let run = d.call(
        "swarm.create",
        json!({"category":"Finished launch",
        "objective":"Inspect backend","allowed_targets":["fixture-local"]}),
    );
    let id = run["id"].as_str().unwrap();
    d.call(
        "swarm.plan",
        json!({"id":id,"generation":1,"revision":0,"jobs":[
            {"id":"inspect","title":"Inspect","acceptance":"evidence","deps":[]}
        ]}),
    );
    let attempt = d.call(
        "swarm.attempt.register",
        json!({"run_id":id,"generation":1,
        "revision":1,"job_id":"inspect"}),
    );
    let aid = attempt["id"].as_str().unwrap();
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    db.execute("INSERT INTO swarm_admissions(run_id,request_id,request_sha256,job_id,attempt_id,target_id,created_ms)
        VALUES(?1,'fixture-admission','fixture','inspect',?2,'fixture-local',?3)",
        rusqlite::params![id,aid,now()]).unwrap();
    d.call(
        "swarm.attempt.confirm_exit",
        json!({"run_id":id,"generation":1,
        "revision":1,"job_id":"inspect","attempt_id":aid}),
    );
    let request = json!({"run_id":id,"job_id":"inspect","attempt_id":aid,
        "token":attempt["token"],"repo":checkout,"program":"/bin/sleep",
        "args":["30"],"prompt":"Inspect the backend","title":"Finished worker"});
    assert!(d.try_call("swarm.worker.launch", request).is_err());
    assert!(d.runs().is_empty());
}

/// SWARM-62 with the default deadline. Two runs use the built-in 60-minute
/// deadline (nothing set): one active, with a supervised worker that ignores
/// SIGINT and a queued job; one blocked with no allowed target. Their start
/// times are moved back 60 minutes and one millisecond (the clock is the only
/// thing changed). The daemon's own timer then stops both for `deadline`:
/// the queued job is cancelled, the active worker gets a checkpoint request,
/// a Stop and an interrupt, and while it ignores the interrupt the run shows
/// it as an unconfirmed exit. An ordinary agent and a third Swarm category on
/// the same daemon keep running. Once the worker is killed its exit is
/// confirmed and the run stops.
#[test]
fn default_sixty_minute_deadline_stops_active_and_blocked_runs_but_nothing_else() {
    let d = Daemon::start(&[]);
    let temp = tmp();
    let checkout = repo(&temp.path().join("default-deadline-source"));
    let active = d.call("swarm.create", json!({"category":"Default deadline active",
        "objective":"Inspect backend","allowed_targets":["fixture-local"]}));
    let active_id = active["id"].as_str().unwrap().to_string();
    assert_eq!(active["policy"]["effective"]["deadline_ms"], 3_600_000, "{active}");
    d.call("swarm.plan", json!({"id":active_id,"generation":1,"revision":0,"jobs":[
        {"id":"inspect","title":"Inspect","acceptance":"evidence","deps":[]},
        {"id":"queued","title":"Queued","acceptance":"evidence","deps":["inspect"]}]}));
    let blocked = d.call("swarm.create", json!({"category":"Default deadline blocked",
        "objective":"Inspect backend","allowed_targets":[]}));
    let blocked_id = blocked["id"].as_str().unwrap().to_string();
    d.call("swarm.plan", json!({"id":blocked_id,"generation":1,"revision":0,"jobs":[
        {"id":"waiting","title":"Waiting","acceptance":"evidence","deps":[]}]}));
    let other = d.call("swarm.create", json!({"category":"Unrelated category",
        "objective":"Inspect backend","allowed_targets":["fixture-local"]}));
    let other_id = other["id"].as_str().unwrap().to_string();
    d.call("swarm.plan", json!({"id":other_id,"generation":1,"revision":0,"jobs":[
        {"id":"later","title":"Later","acceptance":"evidence","deps":[]}]}));
    let at = now();
    let admitted = d.call("swarm.admit", json!({"run_id":active_id,"generation":1,"revision":1,
        "job_id":"inspect","target_id":"fixture-local","request_id":"default-deadline-worker","now_ms":at,
        "snapshot":{"version":1,"observed_ms":at-1000,"expires_ms":at+60000,
            "targets":[{"id":"fixture-local","account_id":"fixture","pool_ids":["fixture-pool"],
                "capabilities":["code"],"health":"up","auth":"ok"}],
            "pools":[{"id":"fixture-pool","windows":[{"id":"run","unit":"points",
                "remaining_milli":100000,"protected_milli":0,"reserved_milli":0,
                "confidence":"exact","expires_ms":at+60000}]}]},
        "required_capabilities":["code"],"estimate_milli":{"points":100},"purpose":"worker"}));
    assert_eq!(admitted["status"], "admitted", "{admitted}");
    let ready = temp.path().join("handler-ready");
    let launched = d.call("swarm.worker.launch", json!({"run_id":active_id,"job_id":"inspect",
        "attempt_id":admitted["attempt_id"],"token":admitted["token"],"repo":checkout,
        "program":"/usr/bin/python3","args":["-c",
            format!("import pathlib,signal,time;signal.signal(signal.SIGINT,signal.SIG_IGN);pathlib.Path({:?}).write_text('ready');time.sleep(60)",ready.to_string_lossy())],
        "prompt":"Inspect","title":"Deadline worker"}));
    let worker = launched["overseer_run_id"].as_str().unwrap().to_string();
    let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !ready.exists() {
        assert!(std::time::Instant::now() < until, "worker did not install its handler");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let ordinary = run_id(&d.generic(&checkout, "worktree", "/bin/sleep", &["60"]));
    d.wait_status(&ordinary, |s| s == "running", 10);
    // Only the clock moves: both runs started 60 minutes and 1 ms ago.
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    db.busy_timeout(std::time::Duration::from_secs(10)).unwrap();
    for run in [&active_id, &blocked_id] {
        db.execute("UPDATE swarm_runs SET created_ms=created_ms-3600001 WHERE id=?1", [run]).unwrap();
    }
    let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while d.call("swarm.get", json!({"id":active_id}))["status"] != "stopping"
        || d.call("swarm.get", json!({"id":blocked_id}))["status"] != "stopped" {
        assert!(std::time::Instant::now() < until, "the default deadline did not stop both runs: {} / {}",
            d.call("swarm.get", json!({"id":active_id})), d.call("swarm.get", json!({"id":blocked_id})));
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    for run in [&active_id, &blocked_id] {
        assert_eq!(d.call("swarm.get", json!({"id":run}))["stop_reason"], "deadline");
    }
    let jobs = d.call("swarm.jobs", json!({"id":active_id}));
    let status = |job: &str| jobs["jobs"].as_array().unwrap().iter().find(|j| j["id"] == job).unwrap()["status"].clone();
    assert_eq!((status("inspect"), status("queued")), (json!("cancel_requested"), json!("cancelled")));
    assert_eq!(d.call("swarm.jobs", json!({"id":blocked_id}))["jobs"][0]["status"], "cancelled");
    let inbox = d.call("swarm.messages", json!({"run_id":active_id,"recipient":admitted["attempt_id"],
        "token":admitted["token"]}));
    let kinds: Vec<&str> = inbox["messages"].as_array().unwrap().iter().filter_map(|m| m["type"].as_str()).collect();
    assert!(kinds.contains(&"checkpoint") && kinds.contains(&"stop"), "{kinds:?}");
    let run_dir: String = db.query_row("SELECT run_dir FROM runs WHERE id=?1", [&worker], |r| r.get(0)).unwrap();
    let marker = std::path::Path::new(&run_dir).join("interrupt.requested");
    let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !marker.exists() {
        assert!(std::time::Instant::now() < until, "the deadline did not interrupt the worker");
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    std::thread::sleep(std::time::Duration::from_millis(500));
    // The worker ignores the interrupt: its exit is shown as unconfirmed.
    let stopping = d.call("swarm.get", json!({"id":active_id}));
    assert_eq!(stopping["unconfirmed_exit_count"], 1, "{stopping}");
    assert_eq!(stopping["unconfirmed_exits"][0]["overseer_run_id"], worker.as_str(), "{stopping}");
    assert_eq!(d.run(&worker)["status"], "running");
    // Nothing unrelated is touched.
    assert_eq!(d.run(&ordinary)["status"], "running", "an ordinary agent keeps running");
    assert!(matches!(d.call("swarm.get", json!({"id":other_id}))["status"].as_str(), Some("planning" | "running")));
    let shim: serde_json::Value = serde_json::from_slice(&std::fs::read(
        std::path::Path::new(&run_dir).join("shim.json")).unwrap()).unwrap();
    signal(shim["child_pid"].as_i64().unwrap(), 9);
    assert_ne!(d.wait_done(&worker, 10)["status"], "completed");
    let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while d.call("swarm.get", json!({"id":active_id}))["status"] != "stopped" {
        assert!(std::time::Instant::now() < until, "confirmed exit did not finish the stop");
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    assert_eq!(d.call("swarm.get", json!({"id":active_id}))["unconfirmed_exit_count"], 0);
    d.call("run.interrupt", json!({"run_id":ordinary}));
    d.wait_done(&ordinary, 10);
}
