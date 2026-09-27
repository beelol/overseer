mod common;

use common::*;
use serde_json::{json, Value};
use std::time::{SystemTime, UNIX_EPOCH};

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as i64
}

fn submit(d: &Daemon, run: &str, job: &str, kind: &str, content: &str) -> Value {
    let attempt = d.call("swarm.attempt.register", json!({"run_id":run,
        "generation":1,"revision":1,"job_id":job}));
    let artifact = format!("{job}-evidence");
    d.call("swarm.artifact.put",json!({"run_id":run,"job_id":job,
        "attempt_id":attempt["id"],"token":attempt["token"],
        "artifact_id":artifact,"source_revision":1,"kind":kind,"content":content}));
    d.call("swarm.report",json!({"run_id":run,"job_id":job,
        "attempt_id":attempt["id"],"token":attempt["token"],
        "message_id":format!("{job}-result"),"type":"result","revision":1,
        "payload":{"artifact_ids":[artifact],"audit_outcome":
            if kind=="reproduction" {"confirmed_defect"} else {"negative"}}}));
    attempt
}

#[test]
fn resolving_a_conflict_during_a_director_turn_resets_no_progress_count() {
    let d = Daemon::start(&[]);
    let created = d.call("swarm.create",json!({"category":"Conflict progress",
        "objective":"Reconcile contradictory probes","allowed_targets":["fixture"]}));
    let run = created["id"].as_str().unwrap();
    d.call("swarm.plan",json!({"id":run,"generation":1,"revision":0,"jobs":[
        {"id":"left","title":"First probe","acceptance":"Evidence","deps":[]},
        {"id":"right","title":"Second probe","acceptance":"Evidence","deps":[]},
        {"id":"repro","title":"Independent repro","acceptance":"Evidence","deps":[]}
    ]}));
    submit(&d,run,"left","finding","foreign request returned 200");
    submit(&d,run,"right","finding","foreign request returned 403");
    d.call("swarm.conflict.open",json!({"run_id":run,"generation":1,"revision":1,
        "conflict_id":"response-disagreement","left_job_id":"left",
        "left_artifact_id":"left-evidence","right_job_id":"right",
        "right_artifact_id":"right-evidence","reason":"Different responses"}));
    let first = d.call("swarm.director.claim_batch",json!({"run_id":run,
        "generation":1,"revision":1,"now_ms":now()+6000}));
    assert_eq!(first["status"],"claimed");
    let first_done = d.call("swarm.director.complete_batch",json!({"run_id":run,
        "generation":1,"turn_id":first["turn_id"],"token":first["token"],
        "outcome":"no_progress"}));
    assert_eq!(first_done["no_progress_turns"],1);

    let repro = submit(&d,run,"repro","reproduction","fresh probe confirms the left response");
    d.call("swarm.decide",json!({"run_id":run,"generation":1,"revision":1,
        "job_id":"repro","decision":"accept","evidence":["repro-evidence"]}));
    d.call("swarm.attempt.confirm_exit",json!({"run_id":run,"generation":1,
        "revision":1,"job_id":"repro","attempt_id":repro["id"]}));
    let second = d.call("swarm.director.claim_batch",json!({"run_id":run,
        "generation":1,"revision":1,"now_ms":now()+6000}));
    assert_eq!(second["status"],"claimed");
    d.call("swarm.conflict.resolve",json!({"run_id":run,"generation":1,
        "revision":1,"conflict_id":"response-disagreement","outcome":"supports_left",
        "reproduction_job_id":"repro","reproduction_artifact_id":"repro-evidence"}));
    let done = d.call("swarm.director.complete_batch",json!({"run_id":run,
        "generation":1,"turn_id":second["turn_id"],"token":second["token"],
        "outcome":"progress"}));
    assert_eq!(done["material_progress"],true,"{done}");
    assert_eq!(done["no_progress_turns"],0,"{done}");
    assert_eq!(done["status"],"planning","{done}");
}

#[test]
fn contradictory_results_require_independent_accepted_reproduction_before_review() {
    let mut d=Daemon::start(&[]);
    let created=d.call("swarm.create",json!({"category":"Conflicting Atlas evidence",
        "objective":"Audit tenant isolation","allowed_targets":["fixture"]}));
    let run=created["id"].as_str().unwrap();
    d.call("swarm.plan",json!({"id":run,"generation":1,"revision":0,"jobs":[
        {"id":"j2","title":"Task mutation","acceptance":"foreign task response","deps":[]},
        {"id":"j7","title":"Independent task response","acceptance":"repeat task response","deps":[]},
        {"id":"j8","title":"Resolve middleware discrepancy","acceptance":"independent reproduction","deps":[]}
    ]}));
    let _j2=submit(&d,run,"j2","reproduction","foreign PATCH returned 200 and changed Bob's row");
    let _j7=submit(&d,run,"j7","finding","guarded route returned 403 and left Bob's row unchanged");
    let conflict=json!({"run_id":run,"generation":1,"revision":1,
        "conflict_id":"task-route-disagreement","left_job_id":"j2",
        "left_artifact_id":"j2-evidence","right_job_id":"j7",
        "right_artifact_id":"j7-evidence","reason":"The probes disagree on foreign task mutation"});
    assert_eq!(d.call("swarm.conflict.open",conflict.clone())["status"],"open");
    let coverage=d.call("swarm.coverage",json!({"run_id":run}));
    let rows=coverage["rows"].as_array().unwrap();
    for job in ["j2","j7"] {
        assert_eq!(rows.iter().find(|row|row["job_id"]==job).unwrap()["coverage_state"],
            "conflict_unresolved");
    }
    assert_eq!(d.call("swarm.conflicts",json!({"run_id":run}))["conflicts"][0]["status"],"open");
    for (job,evidence) in [("j2","j2-evidence"),("j7","j7-evidence")] {
        let error=d.try_call("swarm.decide",json!({"run_id":run,"generation":1,
            "revision":1,"job_id":job,"decision":"accept","evidence":[evidence]})).unwrap_err();
        assert!(error.contains("conflict"),"{error}");
    }
    d.kill9();
    d.spawn();
    assert_eq!(d.call("swarm.conflict.open",conflict)["duplicate"],true);
    let j8=submit(&d,run,"j8","reproduction","fresh unguarded route returns 200; guarded route returns 403");
    assert!(d.try_call("swarm.conflict.resolve",json!({"run_id":run,
        "generation":1,"revision":1,"conflict_id":"task-route-disagreement",
        "outcome":"supports_left","reproduction_job_id":"j2",
        "reproduction_artifact_id":"j2-evidence"})).unwrap_err().contains("independent"));
    assert!(d.try_call("swarm.conflict.resolve",json!({"run_id":run,
        "generation":1,"revision":1,"conflict_id":"task-route-disagreement",
        "outcome":"supports_left","reproduction_job_id":"j8",
        "reproduction_artifact_id":"j8-evidence"})).unwrap_err().contains("accepted"));
    d.call("swarm.decide",json!({"run_id":run,"generation":1,"revision":1,
        "job_id":"j8","decision":"accept","evidence":["j8-evidence"]}));
    d.call("swarm.attempt.confirm_exit",json!({"run_id":run,"generation":1,
        "revision":1,"job_id":"j8","attempt_id":j8["id"]}));
    let resolved=d.call("swarm.conflict.resolve",json!({"run_id":run,
        "generation":1,"revision":1,"conflict_id":"task-route-disagreement",
        "outcome":"supports_left","reproduction_job_id":"j8",
        "reproduction_artifact_id":"j8-evidence"}));
    assert_eq!(resolved["status"],"resolved");
    assert_eq!(d.call("swarm.decide",json!({"run_id":run,"generation":1,
        "revision":1,"job_id":"j2","decision":"accept",
        "evidence":["j2-evidence"]}))["status"],"accepted");
    let db=rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let count:i64=db.query_row("SELECT COUNT(*) FROM swarm_conflicts WHERE run_id=?1",
        [run],|row|row.get(0)).unwrap();
    assert_eq!(count,1);
}

#[test]
fn explicitly_unresolved_conflict_remains_visible_and_blocks_acceptance() {
    let d=Daemon::start(&[]);
    let created=d.call("swarm.create",json!({"category":"Unresolved disagreement",
        "objective":"Audit a disputed path","allowed_targets":["fixture"]}));
    let run=created["id"].as_str().unwrap();
    d.call("swarm.plan",json!({"id":run,"generation":1,"revision":0,"jobs":[
        {"id":"a","title":"First path","acceptance":"first evidence","deps":[]},
        {"id":"b","title":"Second path","acceptance":"second evidence","deps":[]}
    ]}));
    submit(&d,run,"a","finding","request returned 200");
    submit(&d,run,"b","finding","same request returned 403");
    d.call("swarm.conflict.open",json!({"run_id":run,"generation":1,"revision":1,
        "conflict_id":"different-responses","left_job_id":"a",
        "left_artifact_id":"a-evidence","right_job_id":"b",
        "right_artifact_id":"b-evidence","reason":"Incompatible responses"}));
    assert_eq!(d.call("swarm.conflict.resolve",json!({"run_id":run,
        "generation":1,"revision":1,"conflict_id":"different-responses",
        "outcome":"unresolved"}))["status"],"unresolved");
    assert_eq!(d.call("swarm.conflicts",json!({"run_id":run}))["conflicts"][0]["status"],
        "unresolved");
    let error=d.try_call("swarm.decide",json!({"run_id":run,"generation":1,
        "revision":1,"job_id":"a","decision":"accept",
        "evidence":["a-evidence"]})).unwrap_err();
    assert!(error.contains("conflict"),"{error}");
    assert_eq!(d.call("swarm.coverage",json!({"run_id":run}))["rows"][0]["coverage_state"],
        "conflict_unresolved");
}

#[test]
fn stopped_disagreement_reports_incomplete_coverage_after_restart() {
    let mut d=Daemon::start(&[]);
    let made=d.call("swarm.create",json!({"category":"Partial conflict report",
        "objective":"Audit disputed task responses","allowed_targets":["fixture"]}));
    let run=made["id"].as_str().unwrap();
    d.call("swarm.plan",json!({"id":run,"generation":1,"revision":0,"jobs":[
        {"id":"left","title":"First path","acceptance":"response proof"},
        {"id":"right","title":"Second path","acceptance":"response proof"},
        {"id":"independent","title":"Separate path","acceptance":"separate proof"}
    ]}));
    let left=submit(&d,run,"left","finding","foreign request returned 200");
    let right=submit(&d,run,"right","finding","same request returned 403");
    let independent=submit(&d,run,"independent","finding","separate path denied request");
    d.call("swarm.decide",json!({"run_id":run,"generation":1,"revision":1,
        "job_id":"independent","decision":"accept","evidence":["independent-evidence"]}));
    d.call("swarm.attempt.confirm_exit",json!({"run_id":run,"generation":1,
        "revision":1,"job_id":"independent","attempt_id":independent["id"]}));
    d.call("swarm.conflict.open",json!({"run_id":run,"generation":1,"revision":1,
        "conflict_id":"different-responses","left_job_id":"left",
        "left_artifact_id":"left-evidence","right_job_id":"right",
        "right_artifact_id":"right-evidence","reason":"Incompatible responses"}));
    d.call("swarm.conflict.resolve",json!({"run_id":run,"generation":1,
        "revision":1,"conflict_id":"different-responses","outcome":"unresolved"}));
    assert_eq!(d.call("swarm.stop",json!({"run_id":run}))["status"],"stopping");
    for (job,attempt) in [("left",left),("right",right)] {
        d.call("swarm.attempt.confirm_exit",json!({"run_id":run,"generation":1,
            "revision":1,"job_id":job,"attempt_id":attempt["id"]}));
    }
    assert_eq!(d.call("swarm.get",json!({"id":run}))["status"],"stopped");
    d.kill9();
    d.spawn();
    let report=d.call("swarm.coverage",json!({"run_id":run}));
    assert_eq!(report["outcome"],"incomplete","{report}");
    assert_eq!(report["run_status"],"stopped","{report}");
    assert_eq!(report["stop_reason"],"requested","{report}");
    assert_eq!(report["completion"],Value::Null,"{report}");
    let rows=report["rows"].as_array().unwrap();
    assert_eq!(rows.iter().find(|r|r["job_id"]=="independent").unwrap()["coverage_state"],
        "checked_negative");
    for job in ["left","right"] {
        assert_eq!(rows.iter().find(|r|r["job_id"]==job).unwrap()["coverage_state"],
            "conflict_unresolved");
    }
    let conflict=&report["conflicts"][0];
    assert_eq!(conflict["status"],"unresolved","{report}");
    assert_eq!(conflict["left_artifact_id"],"left-evidence");
    assert_eq!(conflict["right_artifact_id"],"right-evidence");
    assert!(d.try_call("swarm.complete",json!({"run_id":run,"generation":1,
        "revision":1,"request_id":"false-pass","summary":"Done",
        "verification":"No disagreement","checks":[
            {"job_id":"left","outcome":"passed","evidence":["left-evidence"]},
            {"job_id":"right","outcome":"passed","evidence":["right-evidence"]},
            {"job_id":"independent","outcome":"passed","evidence":["independent-evidence"]}
        ]})).is_err());
}

#[test]
fn late_conflict_holds_accepted_jobs_until_contradicted_review_is_revised() {
    let mut d = Daemon::start(&[]);
    let made = d.call("swarm.create",json!({"category":"Late contradiction",
        "objective":"Audit conflicting responses","allowed_targets":["fixture"]}));
    let run = made["id"].as_str().unwrap();
    d.call("swarm.plan",json!({"id":run,"generation":1,"revision":0,"jobs":[
        {"id":"left","title":"Left probe","acceptance":"response proof"},
        {"id":"right","title":"Right probe","acceptance":"response proof"},
        {"id":"repro","title":"Independent probe","acceptance":"fresh proof"}
    ]}));
    for (job,kind,content) in [
        ("left","finding","foreign request returned 200"),
        ("right","finding","foreign request returned 403"),
        ("repro","reproduction","fresh isolated probe supports left")
    ] {
        let attempt = submit(&d,run,job,kind,content);
        d.call("swarm.decide",json!({"run_id":run,"generation":1,"revision":1,
            "job_id":job,"decision":"accept","evidence":[format!("{job}-evidence")]}));
        d.call("swarm.attempt.confirm_exit",json!({"run_id":run,"generation":1,
            "revision":1,"job_id":job,"attempt_id":attempt["id"]}));
    }
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    db.execute("UPDATE swarm_runs SET status='running' WHERE id=?1",[run]).unwrap();
    let conflict = json!({"run_id":run,"generation":1,
        "revision":1,"conflict_id":"late-response","left_job_id":"left",
        "left_artifact_id":"left-evidence","right_job_id":"right",
        "right_artifact_id":"right-evidence","reason":"Accepted results disagree"});
    let opened = d.call("swarm.conflict.open",conflict.clone());
    assert_eq!(opened["status"],"open");
    d.kill9();
    d.spawn();
    assert_eq!(d.call("swarm.conflict.open",conflict)["duplicate"],true);
    let rows = d.call("swarm.coverage",json!({"run_id":run}))["rows"].as_array().unwrap().clone();
    for job in ["left","right"] {
        assert_eq!(rows.iter().find(|row|row["job_id"]==job).unwrap()["coverage_state"],
            "conflict_unresolved");
    }
    assert!(d.try_call("swarm.complete",json!({"run_id":run,"generation":1,
        "revision":1,"request_id":"late-conflict-complete","summary":"Done",
        "verification":"Local probes","checks":[
            {"job_id":"left","outcome":"passed","evidence":["left-evidence"]},
            {"job_id":"right","outcome":"passed","evidence":["right-evidence"]},
            {"job_id":"repro","outcome":"passed","evidence":["repro-evidence"]}
        ]})).unwrap_err().contains("conflict"));
    let resolution = json!({"run_id":run,"generation":1,"revision":1,
        "conflict_id":"late-response","outcome":"supports_left",
        "reproduction_job_id":"repro","reproduction_artifact_id":"repro-evidence"});
    assert!(d.try_call("swarm.conflict.resolve",resolution.clone()).unwrap_err()
        .contains("accepted contradictory review"));
    d.call("swarm.revise",json!({"id":run,"generation":1,"expected_revision":1,
        "reason":"Recheck the contradicted right result","jobs":[
            {"id":"left","title":"Left probe","acceptance":"response proof"},
            {"id":"right","title":"Right probe","acceptance":"fresh right-side proof"},
            {"id":"repro","title":"Independent probe","acceptance":"fresh proof"}
        ]}));
    let mut revised_resolution = resolution;
    revised_resolution["revision"] = json!(2);
    assert_eq!(d.call("swarm.conflict.resolve",revised_resolution)["status"],"resolved");
    let right = d.call("swarm.jobs",json!({"id":run}))["jobs"].as_array().unwrap()
        .iter().find(|job|job["id"]=="right").unwrap().clone();
    assert_ne!(right["status"],"accepted");
    assert!(d.try_call("swarm.complete",json!({"run_id":run,"generation":1,
        "revision":2,"request_id":"revised-conflict-complete","summary":"Done",
        "verification":"Local probes","checks":[
            {"job_id":"left","outcome":"passed","evidence":["left-evidence"]},
            {"job_id":"right","outcome":"passed","evidence":["right-evidence"]},
            {"job_id":"repro","outcome":"passed","evidence":["repro-evidence"]}
        ]})).unwrap_err().contains("accepted and checked"));
}

#[test]
fn late_conflict_holds_a_ready_transitive_dependent_before_admission() {
    let d = Daemon::start(&[]);
    let made = d.call("swarm.create",json!({"category":"Conflict dependent",
        "objective":"Audit route and consumer","allowed_targets":["fixture"]}));
    let run = made["id"].as_str().unwrap();
    d.call("swarm.plan",json!({"id":run,"generation":1,"revision":0,"jobs":[
        {"id":"left","title":"Left probe","acceptance":"response proof"},
        {"id":"right","title":"Right probe","acceptance":"response proof"},
        {"id":"middle","title":"Use right result","acceptance":"consumer proof","deps":["right"]},
        {"id":"leaf","title":"Use middle result","acceptance":"final proof","deps":["middle"]},
        {"id":"repro","title":"Independent probe","acceptance":"fresh proof"}
    ]}));
    for (job,content) in [
        ("right","foreign request returned 403"),
        ("middle","consumer relies on the denied response")
    ] {
        let attempt = submit(&d,run,job,"finding",content);
        d.call("swarm.decide",json!({"run_id":run,"generation":1,"revision":1,
            "job_id":job,"decision":"accept","evidence":[format!("{job}-evidence")]}));
        d.call("swarm.attempt.confirm_exit",json!({"run_id":run,"generation":1,
            "revision":1,"job_id":job,"attempt_id":attempt["id"]}));
    }
    submit(&d,run,"left","finding","foreign request returned 200");
    let leaf_before = d.call("swarm.jobs",json!({"id":run}))["jobs"].as_array().unwrap()
        .iter().find(|job|job["id"]=="leaf").unwrap().clone();
    assert_eq!(leaf_before["status"],"ready");
    d.call("swarm.conflict.open",json!({"run_id":run,"generation":1,
        "revision":1,"conflict_id":"ancestor-disagreement","left_job_id":"left",
        "left_artifact_id":"left-evidence","right_job_id":"right",
        "right_artifact_id":"right-evidence","reason":"Different route responses"}));
    let middle_held=d.call("swarm.jobs",json!({"id":run}))["jobs"].as_array().unwrap()
        .iter().find(|job|job["id"]=="middle").unwrap().clone();
    assert_eq!(middle_held["status"],"blocked");
    let leaf_after = d.call("swarm.jobs",json!({"id":run}))["jobs"].as_array().unwrap()
        .iter().find(|job|job["id"]=="leaf").unwrap().clone();
    assert_eq!(leaf_after["status"],"planned");
    let at = now();
    let admission = d.call("swarm.admit",json!({"run_id":run,"generation":1,
        "revision":1,"job_id":"leaf","target_id":"fixture",
        "request_id":"leaf-after-conflict","now_ms":at,
        "snapshot":{"version":1,"observed_ms":at-1000,"expires_ms":at+60000,
            "targets":[{"id":"fixture","account_id":"fixture-a","pool_ids":["pool-a"],
                "capabilities":["code"],"health":"up","auth":"ok"}],
            "pools":[{"id":"pool-a","windows":[{"id":"week","unit":"points",
                "remaining_milli":100000,"protected_milli":0,"reserved_milli":0,
                "confidence":"exact","expires_ms":at+60000}]}]},
        "required_capabilities":["code"],"estimate_milli":{"points":1000},
        "purpose":"worker"}));
    assert_eq!(admission["status"],"blocked","{admission}");
    assert_eq!(admission["reason"],"dependency_pending","{admission}");
    assert_eq!(admission["waiting_on"],json!(["middle"]));
    let repro = submit(&d,run,"repro","reproduction","fresh probe supports right");
    d.call("swarm.decide",json!({"run_id":run,"generation":1,"revision":1,
        "job_id":"repro","decision":"accept","evidence":["repro-evidence"]}));
    d.call("swarm.attempt.confirm_exit",json!({"run_id":run,"generation":1,
        "revision":1,"job_id":"repro","attempt_id":repro["id"]}));
    assert_eq!(d.call("swarm.conflict.resolve",json!({"run_id":run,"generation":1,
        "revision":1,"conflict_id":"ancestor-disagreement","outcome":"supports_right",
        "reproduction_job_id":"repro","reproduction_artifact_id":"repro-evidence"}))["status"],
        "resolved");
    let leaf_still_held = d.call("swarm.jobs",json!({"id":run}))["jobs"].as_array().unwrap()
        .iter().find(|job|job["id"]=="leaf").unwrap().clone();
    assert_eq!(leaf_still_held["status"],"planned");
    d.call("swarm.revise",json!({"id":run,"generation":1,"expected_revision":1,
        "reason":"Recheck the middle conclusion after independent reproduction","jobs":[
            {"id":"left","title":"Left probe","acceptance":"response proof"},
            {"id":"right","title":"Right probe","acceptance":"response proof"},
            {"id":"middle","title":"Use right result","acceptance":"rechecked consumer proof","deps":["right"]},
            {"id":"leaf","title":"Use middle result","acceptance":"final proof","deps":["middle"]},
            {"id":"repro","title":"Independent probe","acceptance":"fresh proof"}
        ]}));
    let middle=d.call("swarm.attempt.register",json!({"run_id":run,
        "generation":1,"revision":2,"job_id":"middle"}));
    d.call("swarm.artifact.put",json!({"run_id":run,"job_id":"middle",
        "attempt_id":middle["id"],"token":middle["token"],
        "artifact_id":"middle-rechecked","source_revision":2,"kind":"finding",
        "content":"fresh consumer proof after route reproduction"}));
    d.call("swarm.report",json!({"run_id":run,"job_id":"middle",
        "attempt_id":middle["id"],"token":middle["token"],
        "message_id":"middle-rechecked-result","type":"result","revision":2,
        "payload":{"artifact_ids":["middle-rechecked"],"audit_outcome":"negative"}}));
    d.call("swarm.decide",json!({"run_id":run,"generation":1,"revision":2,
        "job_id":"middle","decision":"accept","evidence":["middle-rechecked"]}));
    d.call("swarm.attempt.confirm_exit",json!({"run_id":run,"generation":1,
        "revision":2,"job_id":"middle","attempt_id":middle["id"]}));
    let leaf_restored=d.call("swarm.jobs",json!({"id":run}))["jobs"].as_array().unwrap()
        .iter().find(|job|job["id"]=="leaf").unwrap().clone();
    assert_eq!(leaf_restored["status"],"ready");
}

#[test]
fn late_conflict_checkpoints_an_active_dependent_until_replanned() {
    let mut d = Daemon::start(&[]);
    let made = d.call("swarm.create",json!({"category":"Active conflict dependent",
        "objective":"Audit route consumer","allowed_targets":["fixture"]}));
    let run = made["id"].as_str().unwrap();
    d.call("swarm.plan",json!({"id":run,"generation":1,"revision":0,"jobs":[
        {"id":"left","title":"Left probe","acceptance":"response proof"},
        {"id":"right","title":"Right probe","acceptance":"response proof"},
        {"id":"consumer","title":"Use right result","acceptance":"consumer proof","deps":["right"]},
        {"id":"repro","title":"Independent probe","acceptance":"fresh proof"}
    ]}));
    let right = submit(&d,run,"right","finding","foreign request returned 403");
    d.call("swarm.decide",json!({"run_id":run,"generation":1,"revision":1,
        "job_id":"right","decision":"accept","evidence":["right-evidence"]}));
    d.call("swarm.attempt.confirm_exit",json!({"run_id":run,"generation":1,
        "revision":1,"job_id":"right","attempt_id":right["id"]}));
    let active = d.call("swarm.attempt.register",json!({"run_id":run,
        "generation":1,"revision":1,"job_id":"consumer"}));
    submit(&d,run,"left","finding","foreign request returned 200");
    let conflict = json!({"run_id":run,"generation":1,"revision":1,
        "conflict_id":"consumer-disagreement","left_job_id":"left",
        "left_artifact_id":"left-evidence","right_job_id":"right",
        "right_artifact_id":"right-evidence","reason":"Route results disagree"});
    d.call("swarm.conflict.open",conflict.clone());
    let consumer = d.call("swarm.jobs",json!({"id":run}))["jobs"].as_array().unwrap()
        .iter().find(|job|job["id"]=="consumer").unwrap().clone();
    assert_eq!(consumer["status"],"cancel_requested");
    assert_eq!(consumer["stop_reason"],"evidence_conflict");
    d.kill9();
    d.spawn();
    assert_eq!(d.call("swarm.conflict.open",conflict)["duplicate"],true);
    let messages = d.call("swarm.messages",json!({"run_id":run,
        "recipient":active["id"],"token":active["token"]}));
    let checkpoints: Vec<_> = messages["messages"].as_array().unwrap().iter()
        .filter(|m|m["type"]=="checkpoint").collect();
    assert_eq!(checkpoints.len(),1);
    assert_eq!(checkpoints[0]["payload"]["reason"],"evidence_conflict");
    d.call("swarm.attempt.confirm_exit",json!({"run_id":run,"generation":1,
        "revision":1,"job_id":"consumer","attempt_id":active["id"]}));
    let after_exit = d.call("swarm.jobs",json!({"id":run}))["jobs"].as_array().unwrap()
        .iter().find(|job|job["id"]=="consumer").unwrap().clone();
    assert_eq!(after_exit["status"],"blocked");
}

#[test]
fn late_conflict_interrupts_only_the_linked_dependent_worker() {
    let d=Daemon::start(&[]);
    let temp=tmp();
    let checkout=repo(&temp.path().join("late-conflict-source"));
    let made=d.call("swarm.create",json!({"category":"Linked conflict dependent",
        "objective":"Audit route consumer","allowed_targets":["fixture-local"]}));
    let run=made["id"].as_str().unwrap();
    d.call("swarm.plan",json!({"id":run,"generation":1,"revision":0,"jobs":[
        {"id":"left","title":"Left probe","acceptance":"response proof"},
        {"id":"right","title":"Right probe","acceptance":"response proof"},
        {"id":"consumer","title":"Use right result","acceptance":"consumer proof","deps":["right"]},
        {"id":"unrelated","title":"Unrelated work","acceptance":"separate proof"}
    ]}));
    let right=submit(&d,run,"right","finding","foreign request returned 403");
    d.call("swarm.decide",json!({"run_id":run,"generation":1,"revision":1,
        "job_id":"right","decision":"accept","evidence":["right-evidence"]}));
    d.call("swarm.attempt.confirm_exit",json!({"run_id":run,"generation":1,
        "revision":1,"job_id":"right","attempt_id":right["id"]}));
    commit_beneficial_batch(&d,run,&["consumer".into(),"unrelated".into()]);
    let at=now();
    let mut workers=Vec::new();
    for job in ["consumer","unrelated"] {
        let admitted=d.call("swarm.admit",json!({"run_id":run,"generation":1,"revision":1,
            "job_id":job,"target_id":"fixture-local","request_id":format!("conflict-{job}"),
            "now_ms":at,"snapshot":{"version":1,"observed_ms":at-1000,"expires_ms":at+60000,
                "targets":[{"id":"fixture-local","account_id":"fixture","pool_ids":["pool"],
                    "capabilities":["code"],"health":"up","auth":"ok"}],
                "pools":[{"id":"pool","windows":[{"id":"run","unit":"points",
                    "remaining_milli":1000000,"protected_milli":0,"reserved_milli":0,
                    "confidence":"exact","expires_ms":at+60000}]}]},
            "required_capabilities":["code"],"estimate_milli":{"points":100},"purpose":"worker"}));
        assert_eq!(admitted["status"],"admitted","{admitted}");
        let launched=d.call("swarm.worker.launch",json!({"run_id":run,"job_id":job,
            "attempt_id":admitted["attempt_id"],"token":admitted["token"],"repo":checkout,
            "program":"/bin/sleep","args":["30"],"prompt":"Inspect",
            "title":format!("Worker {job}")}));
        workers.push((job.to_string(),admitted,launched["overseer_run_id"].as_str().unwrap().to_string()));
    }
    for (_,_,worker) in &workers { d.wait_status(worker,|s|s=="running",5); }
    submit(&d,run,"left","finding","foreign request returned 200");
    d.call("swarm.conflict.open",json!({"run_id":run,"generation":1,"revision":1,
        "conflict_id":"linked-disagreement","left_job_id":"left",
        "left_artifact_id":"left-evidence","right_job_id":"right",
        "right_artifact_id":"right-evidence","reason":"Route results disagree"}));
    assert_eq!(d.wait_done(&workers[0].2,8)["status"],"interrupted");
    assert_eq!(d.run(&workers[1].2)["status"],"running");
    let until=std::time::Instant::now()+std::time::Duration::from_secs(3);
    loop {
        let jobs=d.call("swarm.jobs",json!({"id":run}));
        let consumer=jobs["jobs"].as_array().unwrap().iter()
            .find(|j|j["id"]=="consumer").unwrap();
        assert_eq!(consumer["stop_reason"],"evidence_conflict");
        if consumer["status"]=="blocked" { break; }
        assert!(std::time::Instant::now()<until,"dependent exit did not reconcile: {consumer}");
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let db=rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let signal_count:i64=db.query_row("SELECT COUNT(*) FROM swarm_stop_signals
        WHERE run_id=?1 AND overseer_run_id=?2",
        rusqlite::params![run,workers[0].2],|r|r.get(0)).unwrap();
    assert_eq!(signal_count,1);
    d.call("swarm.stop",json!({"run_id":run}));
}

#[test]
fn late_conflict_invalidates_accepted_dependents_until_explicit_re_review() {
    let mut d=Daemon::start(&[]);
    let made=d.call("swarm.create",json!({"category":"Accepted dependent conflict",
        "objective":"Audit route-dependent results","allowed_targets":["fixture"]}));
    let run=made["id"].as_str().unwrap();
    d.call("swarm.plan",json!({"id":run,"generation":1,"revision":0,"jobs":[
        {"id":"left","title":"Left probe","acceptance":"response proof"},
        {"id":"right","title":"Right probe","acceptance":"response proof"},
        {"id":"consumer","title":"Use right result","acceptance":"consumer proof","deps":["right"]},
        {"id":"leaf","title":"Use consumer result","acceptance":"final proof","deps":["consumer"]},
        {"id":"unrelated","title":"Independent check","acceptance":"separate proof"},
        {"id":"repro","title":"Independent reproduction","acceptance":"fresh proof"}
    ]}));
    for (job,content) in [
        ("right","foreign request returned 403"),
        ("consumer","consumer concluded route denies foreign request"),
        ("leaf","summary adopted consumer's conclusion"),
        ("unrelated","separate authorization check passed")
    ] {
        let attempt=submit(&d,run,job,"finding",content);
        d.call("swarm.decide",json!({"run_id":run,"generation":1,"revision":1,
            "job_id":job,"decision":"accept","evidence":[format!("{job}-evidence")]}));
        d.call("swarm.attempt.confirm_exit",json!({"run_id":run,"generation":1,
            "revision":1,"job_id":job,"attempt_id":attempt["id"]}));
    }
    submit(&d,run,"left","finding","foreign request returned 200");
    let conflict=json!({"run_id":run,"generation":1,"revision":1,
        "conflict_id":"accepted-chain-disagreement","left_job_id":"left",
        "left_artifact_id":"left-evidence","right_job_id":"right",
        "right_artifact_id":"right-evidence","reason":"Route results disagree"});
    d.call("swarm.conflict.open",conflict.clone());
    d.kill9();
    d.spawn();
    assert_eq!(d.call("swarm.conflict.open",conflict)["duplicate"],true);
    let jobs=d.call("swarm.jobs",json!({"id":run}));
    for job in ["consumer","leaf"] {
        let row=jobs["jobs"].as_array().unwrap().iter().find(|j|j["id"]==job).unwrap();
        assert_eq!(row["status"],"blocked","{job}: {row}");
        assert_eq!(row["stop_reason"],"evidence_conflict");
        assert_eq!(row["attempt_count"],1);
    }
    let db=rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    for job in ["consumer","leaf"] {
        let history:i64=db.query_row("SELECT COUNT(*) FROM swarm_decisions
            WHERE run_id=?1 AND job_id=?2 AND decision='accept'",
            rusqlite::params![run,job],|r|r.get(0)).unwrap();
        assert_eq!(history,1);
    }
    let unrelated=jobs["jobs"].as_array().unwrap().iter()
        .find(|j|j["id"]=="unrelated").unwrap();
    assert_eq!(unrelated["status"],"accepted");
    let coverage=d.call("swarm.coverage",json!({"run_id":run}));
    for job in ["consumer","leaf"] {
        let row=coverage["rows"].as_array().unwrap().iter().find(|r|r["job_id"]==job).unwrap();
        assert_eq!(row["coverage_state"],"dependency_conflict","{job}: {row}");
    }
    let repro=submit(&d,run,"repro","reproduction","fresh independent probe supports right");
    d.call("swarm.decide",json!({"run_id":run,"generation":1,"revision":1,
        "job_id":"repro","decision":"accept","evidence":["repro-evidence"]}));
    d.call("swarm.attempt.confirm_exit",json!({"run_id":run,"generation":1,
        "revision":1,"job_id":"repro","attempt_id":repro["id"]}));
    d.call("swarm.conflict.resolve",json!({"run_id":run,"generation":1,"revision":1,
        "conflict_id":"accepted-chain-disagreement","outcome":"supports_right",
        "reproduction_job_id":"repro","reproduction_artifact_id":"repro-evidence"}));
    let after=d.call("swarm.jobs",json!({"id":run}));
    for job in ["consumer","leaf"] {
        assert_eq!(after["jobs"].as_array().unwrap().iter()
            .find(|j|j["id"]==job).unwrap()["status"],"blocked");
    }
    let revised=d.call("swarm.revise",json!({"id":run,"generation":1,
        "expected_revision":1,"reason":"Recheck conclusions after route disagreement",
        "jobs":[
            {"id":"left","title":"Left probe","acceptance":"response proof"},
            {"id":"right","title":"Right probe","acceptance":"response proof"},
            {"id":"consumer","title":"Use right result","acceptance":"recheck consumer proof","deps":["right"]},
            {"id":"leaf","title":"Use consumer result","acceptance":"final proof","deps":["consumer"]},
            {"id":"unrelated","title":"Independent check","acceptance":"separate proof"},
            {"id":"repro","title":"Independent reproduction","acceptance":"fresh proof"}
        ]}));
    assert_eq!(revised["affected"],2);
    let after_replan=d.call("swarm.jobs",json!({"id":run}));
    assert_eq!(after_replan["jobs"].as_array().unwrap().iter()
        .find(|j|j["id"]=="consumer").unwrap()["status"],"ready");
    assert_eq!(after_replan["jobs"].as_array().unwrap().iter()
        .find(|j|j["id"]=="leaf").unwrap()["status"],"planned");
    assert_eq!(after_replan["jobs"].as_array().unwrap().iter()
        .find(|j|j["id"]=="unrelated").unwrap()["status"],"accepted");
    let stale=d.try_call("swarm.decide",json!({"run_id":run,"generation":1,
        "revision":2,"job_id":"consumer","decision":"accept",
        "evidence":["consumer-evidence"]})).unwrap_err();
    assert!(stale.contains("stale artifact source revision"),"{stale}");
}
