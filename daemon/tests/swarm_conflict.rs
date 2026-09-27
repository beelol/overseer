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
