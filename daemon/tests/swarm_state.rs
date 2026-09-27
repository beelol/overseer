//! Durable swarm plan tests using the real daemon protocol and SQLite store.
mod common;

use common::*;
use serde_json::json;

#[test]
fn swarm_create_rejects_secret_shaped_category_and_objective_without_persisting_them() {
    let d = Daemon::start(&[]);
    let secret = "sk-abcdefghijklmnopqrstuv";
    for (category, objective) in [
        (format!("Audit {secret}"), "Inspect routes".to_string()),
        ("Backend audit".to_string(), format!("Inspect routes with {secret}")),
    ] {
        let error = d.try_call("swarm.create", json!({"category":category,
            "objective":objective,"allowed_targets":["system-codex"]})).unwrap_err();
        assert!(error.contains("sensitive text"), "{error}");
    }
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let count: i64 = db.query_row("SELECT COUNT(*) FROM swarm_runs", [], |r| r.get(0)).unwrap();
    assert_eq!(count, 0);
    let valid = d.call("swarm.create", json!({"category":"Backend audit",
        "objective":"Inspect routes","allowed_targets":["system-codex"]}));
    assert_eq!(valid["status"], "planning");
}

#[test]
fn swarm_plan_rejects_secret_shaped_job_text_before_persistence() {
    let d = Daemon::start(&[]);
    let made = d.call("swarm.create", json!({"category":"Safe plan",
        "objective":"Inspect routes","allowed_targets":["system-codex"]}));
    let run = made["id"].as_str().unwrap();
    let secret = "sk-abcdefghijklmnopqrstuv";
    let error = d.try_call("swarm.plan", json!({"id":run,"generation":1,"revision":0,
        "jobs":[{"id":"inspect","title":format!("Inspect {secret}"),
            "acceptance":"evidence","deps":[]}]})).unwrap_err();
    assert!(error.contains("sensitive text"), "{error}");
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let count: i64 = db.query_row("SELECT COUNT(*) FROM swarm_jobs WHERE run_id=?1", [run],
        |r| r.get(0)).unwrap();
    assert_eq!(count, 0);
}

#[test]
fn one_active_category_run_survives_restart() {
    let mut d = Daemon::start(&[]);
    let made = d.call(
        "swarm.create",
        json!({
            "category": "Backend security",
            "objective": "Audit tenant isolation",
            "allowed_targets": ["system-codex"]
        }),
    );
    let id = made["id"].as_str().unwrap();
    assert_eq!(made["status"], "planning");
    assert_eq!(made["generation"], 1);
    assert_eq!(made["revision"], 0);
    assert!(d
        .try_call(
            "swarm.create",
            json!({"category":"Backend security","objective":"Another audit"})
        )
        .is_err());
    d.kill9();
    d.spawn();
    let found = d.call("swarm.get", json!({"id": id}));
    assert_eq!(found["objective"], "Audit tenant isolation");
    assert_eq!(found["allowed_targets"], json!(["system-codex"]));
}

#[test]
fn swarm_list_pages_run_summaries_without_loading_job_rows() {
    let mut d = Daemon::start(&[]);
    let mut ids = Vec::new();
    for category in ["Backend", "QA", "Research"] {
        let made = d.call("swarm.create", json!({"category":category,
            "objective":"Inspect the fixture", "allowed_targets":[]}));
        ids.push(made["id"].as_str().unwrap().to_string());
    }
    d.call("swarm.plan", json!({"id":ids[0],"generation":1,"revision":0,
        "jobs":[{"id":"j1","title":"Inspect","acceptance":"Evidence","deps":[]}]}));
    d.kill9();
    d.spawn();

    let first = d.call("swarm.list", json!({"limit":2}));
    let first_rows = first["runs"].as_array().unwrap();
    assert_eq!(first_rows.len(), 2);
    assert!(first["next_cursor"].is_string());
    let second = d.call("swarm.list", json!({"limit":2,
        "cursor":first["next_cursor"]}));
    let second_rows = second["runs"].as_array().unwrap();
    assert_eq!(second_rows.len(), 1);
    assert!(second["next_cursor"].is_null());
    let seen = first_rows.iter().chain(second_rows).map(|r| r["id"].as_str().unwrap())
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(seen.len(), 3);
    for id in &ids { assert!(seen.contains(id.as_str())); }
    let backend = first_rows.iter().chain(second_rows)
        .find(|r| r["id"] == ids[0]).unwrap();
    assert_eq!(backend["job_counts"]["total"], 1);
    assert_eq!(backend["job_counts"]["by_status"]["ready"], 1);
    assert_eq!(backend["active_worker_processes"], 0);
    assert!(backend.get("jobs").is_none());
}

#[test]
fn stopped_empty_swarm_is_terminal_and_releases_category() {
    let d = Daemon::start(&[]);
    let made = d.call("swarm.create", json!({"category":"Reusable", "objective":"Audit",
        "allowed_targets":[]}));
    let id = made["id"].as_str().unwrap();
    let stopped = d.call("swarm.stop", json!({"run_id":id,"generation":1,"revision":0}));
    assert_eq!(stopped["status"],"stopped","{stopped}");
    assert_eq!(d.call("swarm.get",json!({"id":id}))["status"],"stopped");
    assert_eq!(d.call("swarm.get",json!({"id":id}))["stop_reason"],"requested");
    let again = d.call("swarm.create",json!({"category":"Reusable", "objective":"Next audit",
        "allowed_targets":[]}));
    assert_ne!(again["id"],id);
}

#[test]
fn user_stop_needs_only_the_run_id_even_after_a_plan_revision() {
    let d = Daemon::start(&[]);
    let made = d.call("swarm.create", json!({"category":"Stop from stale view",
        "objective":"Audit", "allowed_targets":[]}));
    let id = made["id"].as_str().unwrap();
    d.call("swarm.plan",json!({"id":id,"generation":1,"revision":0,"jobs":[
        {"id":"inspect","title":"Inspect","acceptance":"evidence","deps":[]}
    ]}));
    let stopped = d.call("swarm.stop",json!({"run_id":id}));
    assert_eq!(stopped["status"],"stopped","{stopped}");
    assert_eq!(stopped["stop_reason"],"requested");
    let duplicate = d.call("swarm.stop",json!({"run_id":id,
        "generation":0,"revision":0}));
    assert_eq!(duplicate["status"],"stopped");
    assert_eq!(duplicate["duplicate"],true);
}

#[test]
fn narrowing_scope_supersedes_queued_work_and_stops_an_active_excluded_attempt() {
    let d = Daemon::start(&[]);
    let made = d.call("swarm.create",json!({"category":"Catalog scope",
        "objective":"Migrate resource modules","allowed_targets":["system-codex"]}));
    let run = made["id"].as_str().unwrap();
    d.call("swarm.plan",json!({"id":run,"generation":1,"revision":0,"jobs":[
        {"id":"keep","title":"Keep module","acceptance":"cursor check","deps":[]},
        {"id":"omit_queued","title":"Omit queued","acceptance":"cursor check","deps":[]},
        {"id":"omit_active","title":"Omit active","acceptance":"cursor check","deps":[]}
    ]}));
    d.call("swarm.claim",json!({"run_id":run,"job_id":"omit_queued",
        "generation":1,"revision":1,"resource":"file:queued","mode":"write"}));
    let active=d.call("swarm.attempt.register",json!({"run_id":run,"generation":1,
        "revision":1,"job_id":"omit_active"}));
    d.call("swarm.artifact.put",json!({"run_id":run,"job_id":"omit_active",
        "attempt_id":active["id"],"token":active["token"],"artifact_id":"old-proof",
        "source_revision":1,"kind":"finding","content":"old work remains inspectable"}));
    d.call("swarm.report",json!({"run_id":run,"job_id":"omit_active",
        "attempt_id":active["id"],"token":active["token"],
        "message_id":"old-result","type":"result","revision":1,
        "payload":{"artifact_ids":["old-proof"]}}));
    let revised=d.call("swarm.revise",json!({"id":run,"generation":1,
        "expected_revision":1,"reason":"Owner narrowed the migration",
        "jobs":[{"id":"keep","title":"Keep module","acceptance":"cursor check","deps":[]}]}));
    assert_eq!(revised["revision"],2);
    assert_eq!(revised["superseded"],2);
    let jobs=d.call("swarm.jobs",json!({"id":run,"limit":10}));
    let status=|job:&str| jobs["jobs"].as_array().unwrap().iter()
        .find(|row| row["id"]==job).unwrap()["status"].clone();
    assert_eq!(status("keep"),"ready");
    assert_eq!(status("omit_queued"),"superseded");
    assert_eq!(status("omit_active"),"cancel_requested");
    assert_eq!(d.call("swarm.jobs",json!({"id":run,"status":"superseded"}))["jobs"]
        .as_array().unwrap().len(),1);
    let control=d.call("swarm.messages",json!({"run_id":run,"recipient":active["id"],
        "token":active["token"]}));
    assert!(control["messages"].as_array().unwrap().iter().any(|m|m["type"]=="stop"));
    assert!(d.try_call("swarm.decide",json!({"run_id":run,"generation":1,"revision":2,
        "job_id":"omit_active","decision":"accept","evidence":["old-proof"]})).is_err());
    d.call("swarm.attempt.confirm_exit",json!({"run_id":run,"job_id":"omit_active",
        "attempt_id":active["id"],"generation":1,"revision":2}));
    let after=d.call("swarm.jobs",json!({"id":run,"limit":10}));
    assert_eq!(after["jobs"].as_array().unwrap().iter()
        .find(|row|row["id"]=="omit_active").unwrap()["status"],"superseded");
    let coverage=d.call("swarm.coverage",json!({"run_id":run}));
    assert_eq!(coverage["rows"].as_array().unwrap().iter()
        .find(|row|row["job_id"]=="omit_active").unwrap()["coverage_state"],
        "excluded_by_scope");
    let db=rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let proof: String=db.query_row("SELECT content FROM swarm_artifacts WHERE run_id=?1 AND id='old-proof'",
        [run],|row|row.get(0)).unwrap();
    assert_eq!(proof,"old work remains inspectable");
    let claim: String=db.query_row("SELECT status FROM swarm_claims WHERE run_id=?1 AND job_id='omit_queued'",
        [run],|row|row.get(0)).unwrap();
    assert_eq!(claim,"released");
}

#[test]
fn narrowed_scope_completion_requires_only_retained_jobs() {
    let d = Daemon::start(&[]);
    let made = d.call("swarm.create",json!({"category":"Narrowed completion",
        "objective":"Inspect two modules","allowed_targets":["system-codex"]}));
    let run = made["id"].as_str().unwrap();
    d.call("swarm.plan",json!({"id":run,"generation":1,"revision":0,"jobs":[
        {"id":"keep","title":"Keep","acceptance":"proof","deps":[]},
        {"id":"omit","title":"Omit","acceptance":"proof","deps":[]}
    ]}));
    d.call("swarm.revise",json!({"id":run,"generation":1,"expected_revision":1,
        "reason":"Owner narrowed scope","jobs":[
            {"id":"keep","title":"Keep","acceptance":"proof","deps":[]}]}));
    let attempt=d.call("swarm.attempt.register",json!({"run_id":run,
        "generation":1,"revision":2,"job_id":"keep"}));
    d.call("swarm.artifact.put",json!({"run_id":run,"job_id":"keep",
        "attempt_id":attempt["id"],"token":attempt["token"],
        "artifact_id":"keep-proof","source_revision":1,"kind":"finding","content":"checked"}));
    d.call("swarm.report",json!({"run_id":run,"job_id":"keep",
        "attempt_id":attempt["id"],"token":attempt["token"],
        "message_id":"keep-result","type":"result","revision":1,
        "payload":{"artifact_ids":["keep-proof"]}}));
    d.call("swarm.decide",json!({"run_id":run,"generation":1,"revision":2,
        "job_id":"keep","decision":"accept","evidence":["keep-proof"]}));
    d.call("swarm.attempt.confirm_exit",json!({"run_id":run,"job_id":"keep",
        "attempt_id":attempt["id"],"generation":1,"revision":2}));
    d.call("swarm.ack",json!({"run_id":run,"message_id":"keep-result",
        "recipient":"director","generation":1,"revision":1,"phase":"applied"}));
    rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap()
        .execute("UPDATE swarm_runs SET status='running' WHERE id=?1",[run]).unwrap();
    let check=json!({"job_id":"keep","outcome":"passed","evidence":["keep-proof"]});
    let extra=json!({"job_id":"omit","outcome":"passed","evidence":["keep-proof"]});
    let base=json!({"run_id":run,"generation":1,"revision":2,
        "request_id":"narrow-complete","summary":"One module checked",
        "verification":"Retained module evidence reviewed"});
    let mut bad=base.clone();
    bad["checks"]=json!([check.clone(),extra]);
    assert!(d.try_call("swarm.complete",bad).unwrap_err()
        .contains("every planned job"));
    let mut good=base;
    good["checks"]=json!([check]);
    assert_eq!(d.call("swarm.complete",good)["status"],"completed");
}

#[test]
fn cancelled_jobs_release_claims_for_later_swarms() {
    let d = Daemon::start(&[]);
    for (category, action) in [("Stop claim", "swarm.stop"), ("Off claim", "swarm.off")] {
        let made = d.call("swarm.create", json!({"category":category,"objective":"Audit",
            "allowed_targets":[]}));
        let id = made["id"].as_str().unwrap();
        d.call("swarm.plan",json!({"id":id,"generation":1,"revision":0,"jobs":[
            {"id":"j","title":"Inspect","acceptance":"evidence","deps":[]}
        ]}));
        d.call("swarm.claim",json!({"run_id":id,"job_id":"j","generation":1,
            "revision":1,"resource":"db:shared-stop","mode":"write"}));
        let ended = d.call(action,json!({"run_id":id,"generation":1,"revision":1}));
        assert_eq!(ended["status"],"stopped","{ended}");
        let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
        let status: String = db.query_row(
            "SELECT status FROM swarm_claims WHERE run_id=?1 AND job_id='j'",[id],|r|r.get(0)).unwrap();
        assert_eq!(status,"released");
    }
}

#[test]
fn earlier_swarm_database_gains_recovery_columns_without_losing_its_run() {
    let mut d = Daemon::start(&[]);
    let made = d.call("swarm.create",json!({"category":"Migration","objective":"Audit",
        "allowed_targets":["system-codex"]}));
    let id = made["id"].as_str().unwrap();
    d.kill9();
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    db.execute_batch("ALTER TABLE swarm_runs DROP COLUMN stop_reason;
        ALTER TABLE swarm_runs DROP COLUMN stalled_from;
        ALTER TABLE swarm_runs DROP COLUMN stall_reason;
        ALTER TABLE swarm_runs DROP COLUMN no_progress_turns;
        ALTER TABLE swarm_runs DROP COLUMN failed_planning_turns;
        ALTER TABLE swarm_director_turns DROP COLUMN accepted_decision_id_at_claim;").unwrap();
    drop(db);
    d.spawn();
    assert_eq!(d.call("swarm.get",json!({"id":id}))["objective"],"Audit");
    assert!(d.call("swarm.get",json!({"id":id}))["stop_reason"].is_null());
    assert_eq!(d.call("swarm.get",json!({"id":id}))["failed_planning_turns"],0);
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let version: String = db.query_row("SELECT value FROM meta WHERE key='schema_version'",[],|row|row.get(0)).unwrap();
    assert_eq!(version,"5");
    d.call("swarm.stop",json!({"run_id":id,"generation":1,"revision":0}));
    assert_eq!(d.call("swarm.get",json!({"id":id}))["stop_reason"],"requested");
}

#[test]
fn plan_validates_dependencies_and_revision_before_dispatch() {
    let d = Daemon::start(&[]);
    let made = d.call("swarm.create", json!({"category":"Backend", "objective":"Check routes", "allowed_targets":["system-codex"]}));
    let id = made["id"].as_str().unwrap();
    let cycle = json!({"id":id,"generation":1,"revision":0,"jobs":[
        {"id":"a","title":"A","acceptance":"evidence","deps":["b"]},
        {"id":"b","title":"B","acceptance":"evidence","deps":["a"]}
    ]});
    assert!(d
        .try_call("swarm.plan", cycle)
        .unwrap_err()
        .contains("cycle"));
    assert_eq!(d.call("swarm.get",json!({"id":id}))["failed_planning_turns"],1);
    let planned = d.call(
        "swarm.plan",
        json!({"id":id,"generation":1,"revision":0,"jobs":[
            {"id":"routes","title":"Inspect routes","acceptance":"route matrix","deps":[]},
            {"id":"verify","title":"Verify findings","acceptance":"reproduction","deps":["routes"]}
        ]}),
    );
    assert_eq!(planned["revision"], 1);
    assert_eq!(d.call("swarm.get",json!({"id":id}))["failed_planning_turns"],0);
    let jobs = d.call("swarm.jobs", json!({"id":id,"limit":10}));
    assert_eq!(jobs["jobs"].as_array().unwrap().len(), 2);
    assert_eq!(jobs["jobs"][0]["status"], "ready");
    assert_eq!(jobs["jobs"][1]["status"], "planned");
    let unchanged=d.call("swarm.plan",json!({"id":id,"generation":1,"revision":1,"jobs":[
        {"id":"routes","title":"Inspect routes","acceptance":"route matrix","deps":[]},
        {"id":"verify","title":"Verify findings","acceptance":"reproduction","deps":["routes"]}
    ]}));
    assert_eq!(unchanged["unchanged"],true);
    assert_eq!(unchanged["revision"],1);
    assert!(d
        .try_call(
            "swarm.plan",
            json!({"id":id,"generation":1,"revision":0,"jobs":[]})
        )
        .unwrap_err()
        .contains("revision"));
    assert!(d
        .try_call(
            "swarm.plan",
            json!({"id":id,"generation":0,"revision":1,"jobs":[]})
        )
        .unwrap_err()
        .contains("generation"));
}

#[test]
fn two_invalid_planning_turns_stall_but_stale_calls_do_not_count() {
    let mut d=Daemon::start(&[]);
    let made=d.call("swarm.create",json!({"category":"Planning failures","objective":"Audit",
        "allowed_targets":["system-codex"]}));
    let id=made["id"].as_str().unwrap();
    let bad=json!({"id":id,"generation":1,"revision":0,"jobs":[
        {"id":"orphan","title":"Orphan","acceptance":"evidence","deps":["missing"]}
    ]});
    assert!(d.try_call("swarm.plan",json!({"id":id,"generation":0,"revision":0,
        "jobs":bad["jobs"]})).is_err());
    assert_eq!(d.call("swarm.get",json!({"id":id}))["failed_planning_turns"],0);
    assert!(d.try_call("swarm.plan",bad.clone()).unwrap_err().contains("unknown dependency"));
    assert_eq!(d.call("swarm.get",json!({"id":id}))["failed_planning_turns"],1);
    d.kill9();
    d.spawn();
    assert!(d.try_call("swarm.plan",bad).unwrap_err().contains("unknown dependency"));
    let stalled=d.call("swarm.get",json!({"id":id}));
    assert_eq!(stalled["status"],"stalled");
    assert_eq!(stalled["stall_reason"],"planning_failed");
    assert_eq!(stalled["failed_planning_turns"],2);
    assert!(d.try_call("swarm.plan",json!({"id":id,"generation":1,"revision":0,"jobs":[
        {"id":"valid","title":"Valid","acceptance":"evidence","deps":[]}
    ]})).is_err());
    assert!(d.try_call("swarm.director.recover",json!({"run_id":id,"generation":1,
        "revision":0,"termination":"confirmed_dead"})).is_err());
    assert_eq!(d.call("swarm.stop",json!({"run_id":id,"generation":1,
        "revision":0}))["status"],"stopped");
}

#[test]
fn two_invalid_repair_revisions_share_the_planning_stall_limit() {
    let d=Daemon::start(&[]);
    let made=d.call("swarm.create",json!({"category":"Repair failures","objective":"Audit",
        "allowed_targets":["system-codex"]}));
    let id=made["id"].as_str().unwrap();
    d.call("swarm.plan",json!({"id":id,"generation":1,"revision":0,"jobs":[
        {"id":"j","title":"Inspect","acceptance":"evidence","deps":[]}
    ]}));
    let unchanged=d.call("swarm.revise",json!({"id":id,"generation":1,
        "expected_revision":1,"reason":"repeat","jobs":[
        {"id":"j","title":"Inspect","acceptance":"evidence","deps":[]}
    ]}));
    assert_eq!(unchanged["unchanged"],true);
    assert_eq!(unchanged["revision"],1);
    let bad=json!({"id":id,"generation":1,"expected_revision":1,"reason":"repair",
        "jobs":[{"id":"j","title":"Inspect","acceptance":"evidence","deps":["missing"]}]});
    assert!(d.try_call("swarm.revise",json!({"id":id,"generation":0,
        "expected_revision":1,"reason":"repair","jobs":bad["jobs"]})).is_err());
    assert_eq!(d.call("swarm.get",json!({"id":id}))["failed_planning_turns"],0);
    for n in 1..=2 {
        assert!(d.try_call("swarm.revise",bad.clone()).unwrap_err().contains("unknown dependency"));
        assert_eq!(d.call("swarm.get",json!({"id":id}))["failed_planning_turns"],n);
    }
    let state=d.call("swarm.get",json!({"id":id}));
    assert_eq!(state["status"],"stalled");
    assert_eq!(state["stall_reason"],"planning_failed");
    assert_eq!(d.call("swarm.jobs",json!({"id":id}))["jobs"][0]["status"],"ready");
}

#[test]
fn partial_plan_keeps_independent_valid_subgraphs() {
    let d = Daemon::start(&[]);
    let run = d.call("swarm.create", json!({"category":"Partial graph","objective":"Audit",
        "allowed_targets":["system-codex"]}));
    let id = run["id"].as_str().unwrap();
    let planned = d.call("swarm.plan", json!({"id":id,"generation":1,"revision":0,
        "allow_partial":true,"jobs":[
            {"id":"root","title":"Root","acceptance":"evidence","deps":[]},
            {"id":"child","title":"Child","acceptance":"evidence","deps":["root"]},
            {"id":"independent","title":"Independent","acceptance":"evidence","deps":[]},
            {"id":"missing","title":"Missing","acceptance":"evidence","deps":["unknown"]},
            {"id":"depends-on-bad","title":"Dependent","acceptance":"evidence","deps":["missing"]},
            {"id":"cycle-a","title":"Cycle A","acceptance":"evidence","deps":["cycle-b"]},
            {"id":"cycle-b","title":"Cycle B","acceptance":"evidence","deps":["cycle-a"]},
            {"id":"no-check","title":"No check","deps":[]},
            {"id":"duplicate","title":"One","acceptance":"evidence","deps":[]},
            {"id":"duplicate","title":"Two","acceptance":"evidence","deps":[]}
        ]}));
    assert_eq!(planned["job_count"], 3);
    assert_eq!(planned["rejected"].as_array().unwrap().len(), 7);
    let jobs = d.call("swarm.jobs",json!({"id":id,"limit":100}))["jobs"].as_array().unwrap().clone();
    assert_eq!(jobs.len(), 3);
    assert_eq!(jobs.iter().filter(|job| job["status"] == "ready").count(), 2);
    assert_eq!(jobs.iter().filter(|job| job["status"] == "planned").count(), 1);
}

#[test]
fn large_plan_pages_without_loading_every_job() {
    let d = Daemon::start(&[]);
    let made = d.call("swarm.create", json!({"category":"Catalog", "objective":"Audit modules", "allowed_targets":["system-codex"]}));
    let id = made["id"].as_str().unwrap();
    let jobs: Vec<_> = (0..100).map(|n| json!({"id":format!("job-{n:03}"),"title":format!("Module {n}"),"acceptance":"findings and evidence","deps":[]})).collect();
    d.call(
        "swarm.plan",
        json!({"id":id,"generation":1,"revision":0,"jobs":jobs}),
    );
    let first = d.call("swarm.jobs", json!({"id":id,"limit":30}));
    assert_eq!(first["jobs"].as_array().unwrap().len(), 30);
    let cursor = first["next_cursor"].as_str().unwrap();
    let second = d.call("swarm.jobs", json!({"id":id,"limit":30,"cursor":cursor}));
    assert_eq!(second["jobs"].as_array().unwrap().len(), 30);
    assert_ne!(first["jobs"][0]["id"], second["jobs"][0]["id"]);
    assert_eq!(
        d.call("state", json!({}))["tasks"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
}

#[test]
fn large_run_readout_counts_states_without_inventing_live_workers() {
    let mut d = Daemon::start(&[]);
    let made = d.call("swarm.create", json!({"category":"Large readout",
        "objective":"Audit one hundred modules","allowed_targets":["system-codex"]}));
    let id = made["id"].as_str().unwrap();
    let jobs: Vec<_> = (0..100).map(|n| json!({"id":format!("job-{n:03}"),
        "title":format!("Module {n}"),"acceptance":"evidence","deps":[]})).collect();
    d.call("swarm.plan",json!({"id":id,"generation":1,"revision":0,"jobs":jobs}));
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    db.execute("UPDATE swarm_jobs SET status='running' WHERE run_id=?1 AND id<'job-032'",[id]).unwrap();
    db.execute("UPDATE swarm_jobs SET status='submitted' WHERE run_id=?1 AND id>='job-032' AND id<'job-040'",[id]).unwrap();
    db.execute("UPDATE swarm_jobs SET status='blocked' WHERE run_id=?1 AND id>='job-040' AND id<'job-044'",[id]).unwrap();
    let state = d.call("swarm.get",json!({"id":id}));
    assert_eq!(state["job_counts"]["total"],100);
    assert_eq!(state["job_counts"]["by_status"]["running"],32);
    assert_eq!(state["job_counts"]["by_status"]["submitted"],8);
    assert_eq!(state["job_counts"]["by_status"]["blocked"],4);
    assert_eq!(state["job_counts"]["by_status"]["ready"],56);
    assert_eq!(state["active_worker_processes"],0);
    assert_eq!(state["registered_attempts"],0);
    let first = d.call("swarm.jobs",json!({"id":id,"status":"running","limit":20}));
    assert_eq!(first["jobs"].as_array().unwrap().len(),20);
    let second = d.call("swarm.jobs",json!({"id":id,"status":"running","limit":20,
        "cursor":first["next_cursor"]}));
    assert_eq!(second["jobs"].as_array().unwrap().len(),12);
    assert!(second["next_cursor"].is_null());
    assert_eq!(d.call("swarm.jobs",json!({"id":id,"status":"blocked"}))["jobs"]
        .as_array().unwrap().len(),4);
    assert!(d.try_call("swarm.jobs",json!({"id":id,"status":"unknown"})).is_err());
    let owner = d.call("swarm.director.owner.begin",json!({"run_id":id,"generation":1}));
    let readout = d.call("swarm.get",json!({"id":id}));
    assert_eq!(readout["director"]["owner_status"],"active");
    assert!(readout["director"]["overseer_run_id"].is_null());
    assert!(!readout.to_string().contains(owner["owner_token"].as_str().unwrap()));
    d.kill9();
    d.spawn();
    assert_eq!(d.call("swarm.get",json!({"id":id}))["job_counts"],state["job_counts"]);
    assert_eq!(d.call("swarm.jobs",json!({"id":id,"status":"submitted"}))["jobs"]
        .as_array().unwrap().len(),8);
}

#[test]
fn ready_window_materializes_only_one_hundred_jobs_and_refills_after_admission() {
    let mut d = Daemon::start(&[]);
    let made = d.call("swarm.create", json!({"category":"Ready window",
        "objective":"Inspect independent modules","allowed_targets":["system-codex"]}));
    let run = made["id"].as_str().unwrap();
    let mut jobs: Vec<_> = (0..130).map(|n| json!({"id":format!("job-{n:03}"),
        "title":format!("Module {n}"),"acceptance":"evidence","deps":[]})).collect();
    d.call("swarm.plan", json!({"id":run,"generation":1,"revision":0,"jobs":jobs.clone()}));
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let counts = || -> (i64,i64) {
        let ready = db.query_row("SELECT COUNT(*) FROM swarm_jobs WHERE run_id=?1 AND status='ready'",
            [run], |r| r.get(0)).unwrap();
        let planned = db.query_row("SELECT COUNT(*) FROM swarm_jobs WHERE run_id=?1 AND status='planned'",
            [run], |r| r.get(0)).unwrap();
        (ready, planned)
    };
    assert_eq!(counts(), (100,30));
    d.call("swarm.attempt.register", json!({"run_id":run,"generation":1,
        "revision":1,"job_id":"job-000"}));
    assert_eq!(counts(), (100,29));
    let promoted: String = db.query_row("SELECT status FROM swarm_jobs WHERE run_id=?1 AND id='job-100'",
        [run], |r| r.get(0)).unwrap();
    assert_eq!(promoted, "ready");
    d.kill9();
    d.spawn();
    assert_eq!(counts(), (100,29));
    jobs.push(json!({"id":"job-130","title":"Module 130","acceptance":"evidence","deps":[]}));
    d.call("swarm.revise", json!({"id":run,"generation":1,"expected_revision":1,
        "reason":"Add another independent module","jobs":jobs}));
    assert_eq!(counts(), (100,30));
}

#[test]
fn configured_backlog_cap_counts_nonterminal_jobs_across_plan_and_revision() {
    let d = Daemon::start(&[]);
    let made = d.call("swarm.create", json!({"category":"Small backlog",
        "objective":"Inspect modules","allowed_targets":["system-codex"],
        "policy":{"backlog_max":2}}));
    let run = made["id"].as_str().unwrap();
    let job = |id: &str| json!({"id":id,"title":format!("Inspect {id}"),
        "acceptance":"evidence","deps":[]});
    let over = d.try_call("swarm.plan", json!({"id":run,"generation":1,"revision":0,
        "jobs":[job("j0"),job("j1"),job("j2")]})).unwrap_err();
    assert!(over.contains("backlog limit"), "{over}");
    assert_eq!(d.call("swarm.jobs", json!({"id":run}))["jobs"].as_array().unwrap().len(), 0);
    d.call("swarm.plan", json!({"id":run,"generation":1,"revision":0,
        "jobs":[job("j0"),job("j1")]}));
    let attempt = d.call("swarm.attempt.register", json!({"run_id":run,
        "generation":1,"revision":1,"job_id":"j0"}));
    d.call("swarm.artifact.put", json!({"run_id":run,"job_id":"j0",
        "attempt_id":attempt["id"],"token":attempt["token"],
        "artifact_id":"j0-proof","source_revision":1,"kind":"finding","content":"checked"}));
    d.call("swarm.report", json!({"run_id":run,"job_id":"j0",
        "attempt_id":attempt["id"],"token":attempt["token"],"message_id":"j0-result",
        "type":"result","revision":1,"payload":{"artifact_ids":["j0-proof"]}}));
    d.call("swarm.decide", json!({"run_id":run,"generation":1,"revision":1,
        "job_id":"j0","decision":"accept","evidence":["j0-proof"]}));
    d.call("swarm.attempt.confirm_exit", json!({"run_id":run,"generation":1,"revision":1,
        "job_id":"j0","attempt_id":attempt["id"]}));
    d.call("swarm.revise", json!({"id":run,"generation":1,"expected_revision":1,
        "reason":"Add one module after acceptance","jobs":[job("j0"),job("j1"),job("j2")]}));
    let over = d.try_call("swarm.revise", json!({"id":run,"generation":1,
        "expected_revision":2,"reason":"Try one more module",
        "jobs":[job("j0"),job("j1"),job("j2"),job("j3")]})).unwrap_err();
    assert!(over.contains("backlog limit"), "{over}");
    assert_eq!(d.call("swarm.jobs", json!({"id":run}))["jobs"].as_array().unwrap().len(), 3);
}
