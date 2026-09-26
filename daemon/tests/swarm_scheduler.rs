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
    let run = d.call(
        "swarm.create",
        json!({"category":category,"objective":"Audit backend",
        "allowed_targets":["fixture"],"policy":{"max_executing":9}}),
    );
    let id = run["id"].as_str().unwrap().to_string();
    let backlog = (0..jobs)
        .map(|n| {
            json!({"id":format!("j{n:03}"),"title":format!("Inspect {n}"),
        "acceptance":"Record evidence","deps":[]})
        })
        .collect::<Vec<_>>();
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
