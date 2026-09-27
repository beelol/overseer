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

fn snapshot(at: i64) -> Value {
    json!({"version":1,"observed_ms":at-1000,"expires_ms":at+60000,
        "targets":[{"id":"fixture","account_id":"shared","pool_ids":["shared-pool"],
            "capabilities":["code"],"health":"up","auth":"ok"}],
        "pools":[{"id":"shared-pool","windows":[{"id":"run","unit":"points",
            "remaining_milli":1000000,"protected_milli":0,"reserved_milli":0,
            "confidence":"exact","expires_ms":at+60000}]}]})
}

fn make_run(d: &Daemon, category: &str, jobs: usize) -> String {
    let backlog = (0..jobs)
        .map(|n| {
            json!({"id":format!("j{n:03}"),"title":format!("Inspect {n}"),
        "acceptance":"Record evidence","deps":[]})
        })
        .collect::<Vec<_>>();
    make_run_with_jobs(d, category, backlog)
}

fn make_run_with_jobs(d: &Daemon, category: &str, backlog: Vec<Value>) -> String {
    let run = d.call(
        "swarm.create",
        json!({"category":category,"objective":"Audit backend",
        "allowed_targets":["fixture"]}),
    );
    let id = run["id"].as_str().unwrap().to_string();
    d.call(
        "swarm.plan",
        json!({"id":id,"generation":1,"revision":0,"jobs":backlog}),
    );
    id
}

fn next(d: &Daemon, id: &str, at: i64) -> Value {
    d.call(
        "swarm.schedule.next",
        json!({"request_id":id,"target_id":"fixture",
        "now_ms":at,"snapshot":snapshot(at),"required_capabilities":["code"],
        "estimate_milli":{"points":100},"purpose":"worker"}),
    )
}

#[test]
fn round_robin_admission_is_durable_and_replay_safe_across_unequal_categories() {
    let mut d = Daemon::start(&[]);
    let long = make_run(&d, "A very long backlog", 100);
    let short = make_run(&d, "B two jobs", 2);
    commit_beneficial_batch(&d, &long, &["j000".into(), "j001".into()]);
    commit_beneficial_batch(&d, &short, &["j000".into(), "j001".into()]);
    let at = now();
    let one = next(&d, "step-1", at);
    let two = next(&d, "step-2", at);
    let three = next(&d, "step-3", at);
    assert_eq!(one["status"], "admitted", "{one}");
    assert_eq!(two["status"], "admitted", "{two}");
    assert_eq!(three["status"], "admitted", "{three}");
    assert_eq!(one["run_id"], long);
    assert_eq!(two["run_id"], short);
    assert_eq!(three["run_id"], long);
    assert_eq!(one["job_id"], "j000");
    assert_eq!(three["job_id"], "j001");
    assert_eq!(next(&d, "step-2", at)["attempt_id"], two["attempt_id"]);
    assert!(d
        .try_call(
            "swarm.schedule.next",
            json!({"request_id":"step-2",
        "target_id":"other","now_ms":at,"snapshot":snapshot(at),
        "required_capabilities":["code"],"estimate_milli":{"points":100},
        "purpose":"worker"})
        )
        .is_err());
    d.kill9();
    d.spawn();
    let four = next(&d, "step-4", at);
    assert_eq!(four["status"], "admitted", "{four}");
    assert_eq!(four["run_id"], short);
    assert_eq!(four["job_id"], "j001");
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let attempts: i64 = db
        .query_row("SELECT COUNT(*) FROM swarm_attempts", [], |r| r.get(0))
        .unwrap();
    assert_eq!(attempts, 4);
}

#[test]
fn blocked_first_job_does_not_skip_an_eligible_job_in_the_same_category() {
    let mut d = Daemon::start(&[]);
    let jobs = || vec![
        json!({"id":"j000","title":"Shared database","acceptance":"Evidence",
            "deps":[],"resource_claims":[{"resource":"db:shared","mode":"write"}]}),
        json!({"id":"j001","title":"Independent route","acceptance":"Evidence",
            "deps":[],"resource_claims":[{"resource":"route:independent","mode":"read"}]}),
    ];
    let first = make_run_with_jobs(&d, "A first category", jobs());
    let second = make_run_with_jobs(&d, "B second category", jobs());
    commit_beneficial_batch(&d, &first, &["j000".into(), "j001".into()]);
    commit_beneficial_batch(&d, &second, &["j000".into(), "j001".into()]);
    let at = now();
    let initial = next(&d, "first-shared-claim", at);
    assert_eq!(initial["status"], "admitted", "{initial}");
    assert_eq!(initial["run_id"], first);
    assert_eq!(initial["job_id"], "j000");
    d.kill9();
    d.spawn();
    let independent = next(&d, "second-independent", at);
    assert_eq!(independent["status"], "admitted", "{independent}");
    assert_eq!(independent["run_id"], second);
    assert_eq!(independent["job_id"], "j001");
    assert_eq!(next(&d, "second-independent", at)["attempt_id"], independent["attempt_id"]);
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let attempts: i64 = db.query_row("SELECT COUNT(*) FROM swarm_attempts", [], |r| r.get(0)).unwrap();
    let blocked_job_status: String = db.query_row(
        "SELECT status FROM swarm_jobs WHERE run_id=?1 AND id='j000'",
        [&second], |r| r.get(0),
    ).unwrap();
    assert_eq!(attempts, 2, "the conflicting job must not consume an attempt");
    assert_eq!(blocked_job_status, "ready");
}

#[test]
fn planned_job_capabilities_survive_restart_and_constrain_every_admission_path() {
    let mut d = Daemon::start(&[]);
    let run = make_run_with_jobs(&d, "Capability-specific work", vec![
        json!({"id":"j000","title":"Inspect browser flow","acceptance":"Evidence",
            "deps":[],"required_capabilities":["browser"]}),
        json!({"id":"j001","title":"Inspect source","acceptance":"Evidence",
            "deps":[],"required_capabilities":["code"]}),
    ]);
    commit_beneficial_batch(&d, &run, &["j000".into(), "j001".into()]);
    d.kill9();
    d.spawn();
    let at = now();
    let direct = d.call("swarm.admit", json!({
        "run_id":run,"generation":1,"revision":1,"job_id":"j000",
        "target_id":"fixture","request_id":"browser-on-code",
        "snapshot":snapshot(at),"now_ms":at,"required_capabilities":["code"],
        "estimate_milli":{"points":100},"purpose":"worker"}));
    assert_eq!(direct["status"], "blocked", "{direct}");
    assert_eq!(direct["reason"], "missing_capability");
    let scheduled = next(&d, "matching-job", at);
    assert_eq!(scheduled["status"], "admitted", "{scheduled}");
    assert_eq!(scheduled["job_id"], "j001");
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let saved: String = db.query_row(
        "SELECT required_capabilities FROM swarm_jobs WHERE run_id=?1 AND id='j000'",
        [&run], |r| r.get(0),
    ).unwrap();
    assert_eq!(saved, r#"["browser"]"#);
}

#[test]
fn distinct_allowed_targets_admit_only_the_jobs_they_can_perform() {
    let d = Daemon::start(&[]);
    let created = d.call("swarm.create", json!({"category":"Mixed capabilities",
        "objective":"Check source and browser","allowed_targets":["fixture","browser-profile"]}));
    let run = created["id"].as_str().unwrap();
    d.call("swarm.plan", json!({"id":run,"generation":1,"revision":0,"jobs":[
        {"id":"j000","title":"Browser flow","acceptance":"Evidence","deps":[],
            "required_capabilities":["browser"]},
        {"id":"j001","title":"Source path","acceptance":"Evidence","deps":[],
            "required_capabilities":["code"]}
    ]}));
    commit_beneficial_batch(&d, run, &["j000".into(), "j001".into()]);
    let at = now();
    let mut routes = snapshot(at);
    routes["targets"].as_array_mut().unwrap().push(json!({
        "id":"browser-profile","account_id":"browser-account",
        "pool_ids":["browser-pool"],"capabilities":["browser","code"],
        "health":"up","auth":"ok"
    }));
    routes["pools"].as_array_mut().unwrap().push(json!({
        "id":"browser-pool","windows":[{"id":"run","unit":"points",
            "remaining_milli":1000000,"protected_milli":0,"reserved_milli":0,
            "confidence":"exact","expires_ms":at+60000}]
    }));
    let schedule = |request_id: &str, target_id: &str| d.call("swarm.schedule.next", json!({
        "request_id":request_id,"target_id":target_id,"now_ms":at,"snapshot":routes,
        "required_capabilities":[],"estimate_milli":{"points":100},"purpose":"worker"
    }));
    let code = schedule("code-route", "fixture");
    assert_eq!(code["status"], "admitted", "{code}");
    assert_eq!(code["job_id"], "j001");
    assert_eq!(code["target_id"], "fixture");
    let browser = schedule("browser-route", "browser-profile");
    assert_eq!(browser["status"], "admitted", "{browser}");
    assert_eq!(browser["job_id"], "j000");
    assert_eq!(browser["target_id"], "browser-profile");
}
