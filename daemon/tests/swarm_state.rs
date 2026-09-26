//! Durable swarm plan tests using the real daemon protocol and SQLite store.
mod common;

use common::*;
use serde_json::json;

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
