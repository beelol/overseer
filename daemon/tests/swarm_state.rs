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
fn earlier_swarm_database_gains_recovery_columns_without_losing_its_run() {
    let mut d = Daemon::start(&[]);
    let made = d.call("swarm.create",json!({"category":"Migration","objective":"Audit",
        "allowed_targets":["system-codex"]}));
    let id = made["id"].as_str().unwrap();
    d.kill9();
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    db.execute_batch("ALTER TABLE swarm_runs DROP COLUMN stop_reason; ALTER TABLE swarm_runs DROP COLUMN stalled_from;").unwrap();
    drop(db);
    d.spawn();
    assert_eq!(d.call("swarm.get",json!({"id":id}))["objective"],"Audit");
    assert!(d.call("swarm.get",json!({"id":id}))["stop_reason"].is_null());
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let version: String = db.query_row("SELECT value FROM meta WHERE key='schema_version'",[],|row|row.get(0)).unwrap();
    assert_eq!(version,"3");
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
    assert!(d
        .try_call(
            "swarm.plan",
            json!({"id":id,"generation":1,"revision":0,"jobs":[
                {"id":"orphan","title":"Orphan","acceptance":"evidence","deps":["missing"]}
            ]})
        )
        .unwrap_err()
        .contains("unknown dependency"));
    let planned = d.call(
        "swarm.plan",
        json!({"id":id,"generation":1,"revision":0,"jobs":[
            {"id":"routes","title":"Inspect routes","acceptance":"route matrix","deps":[]},
            {"id":"verify","title":"Verify findings","acceptance":"reproduction","deps":["routes"]}
        ]}),
    );
    assert_eq!(planned["revision"], 1);
    let jobs = d.call("swarm.jobs", json!({"id":id,"limit":10}));
    assert_eq!(jobs["jobs"].as_array().unwrap().len(), 2);
    assert_eq!(jobs["jobs"][0]["status"], "ready");
    assert_eq!(jobs["jobs"][1]["status"], "planned");
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
