mod common;

use common::*;
use rusqlite::Connection;
use serde_json::json;
use std::time::{SystemTime, UNIX_EPOCH};

fn database(d: &Daemon) -> Connection {
    Connection::open(d.home.path().join("overseer.sqlite")).unwrap()
}

fn planned(d: &Daemon, category: &str) -> String {
    let run = d.call(
        "swarm.create",
        json!({"category":category,"objective":"Audit writes","allowed_targets":["codex-a"]}),
    );
    let id = run["id"].as_str().unwrap().to_owned();
    d.call(
        "swarm.plan",
        json!({"id":id,"generation":1,"revision":0,"jobs":[
            {"id":"audit","title":"Audit","acceptance":"evidence","deps":[]}
        ]}),
    );
    id
}

#[test]
fn failed_result_write_is_not_acknowledged_or_partially_submitted() {
    let d = Daemon::start(&[]);
    let run = planned(&d, "Result write fault");
    let attempt = d.call(
        "swarm.attempt.register",
        json!({"run_id":run,"job_id":"audit","generation":1,"revision":1}),
    );
    let db = database(&d);
    db.execute_batch(
        "CREATE TRIGGER fail_result_state BEFORE UPDATE OF status ON swarm_jobs
         WHEN NEW.status='submitted' BEGIN SELECT RAISE(FAIL,'injected write fault'); END;",
    )
    .unwrap();
    let report = json!({"run_id":run,"job_id":"audit","attempt_id":attempt["id"],
        "token":attempt["token"],"message_id":"terminal-result","type":"result",
        "revision":1,"payload":{"artifact_ids":[]}});
    assert!(d.try_call("swarm.report", report.clone()).is_err());
    assert_eq!(
        d.call(
            "swarm.messages",
            json!({"run_id":run,"recipient":"director"})
        )["messages"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
    assert_eq!(
        d.call("swarm.jobs", json!({"id":run}))["jobs"][0]["status"],
        "reserved"
    );
    db.execute_batch("DROP TRIGGER fail_result_state;").unwrap();
    let receipt = d.call("swarm.report", report.clone());
    assert_eq!(receipt["duplicate"], false);
    assert_eq!(d.call("swarm.report", report)["duplicate"], true);
    assert_eq!(
        d.call("swarm.jobs", json!({"id":run}))["jobs"][0]["status"],
        "submitted"
    );
}

#[test]
fn failed_reservation_rolls_back_attempt_and_retry_succeeds_once() {
    let mut d = Daemon::start(&[]);
    let run = planned(&d, "Reservation write fault");
    let db = database(&d);
    db.execute_batch(
        "CREATE TRIGGER fail_reservation BEFORE INSERT ON swarm_reservations
         BEGIN SELECT RAISE(FAIL,'injected write fault'); END;",
    )
    .unwrap();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    let request = json!({"run_id":run,"generation":1,"revision":1,"job_id":"audit",
        "target_id":"codex-a","request_id":"admit-once","now_ms":now,
        "snapshot":{"version":1,"observed_ms":now-1000,"expires_ms":now+60000,
            "targets":[{"id":"codex-a","account_id":"account-a","pool_ids":["shared"],
                "capabilities":["code"],"health":"up","auth":"ok"}],
            "pools":[{"id":"shared","windows":[{"id":"week","unit":"points",
                "remaining_milli":60000,"protected_milli":0,"reserved_milli":0,
                "confidence":"exact","expires_ms":now+60000}]}]},
        "required_capabilities":["code"],"estimate_milli":{"points":1000},"purpose":"worker"});
    assert!(d.try_call("swarm.admit", request.clone()).is_err());
    assert_eq!(
        d.call("swarm.jobs", json!({"id":run}))["jobs"][0]["status"],
        "ready"
    );
    let attempts: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM swarm_attempts WHERE run_id=?1",
            [&run],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(attempts, 0);
    for table in [
        "swarm_admissions",
        "swarm_reservations",
        "swarm_allocations",
    ] {
        let count: i64 = db
            .query_row(
                &format!("SELECT COUNT(*) FROM {table} WHERE run_id=?1"),
                [&run],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0, "{table} retained a partial admission");
    }
    db.execute_batch("DROP TRIGGER fail_reservation;").unwrap();
    d.kill9();
    d.spawn();
    let admitted = d.call("swarm.admit", request.clone());
    assert_eq!(admitted["status"], "admitted");
    let replay = d.call("swarm.admit", request);
    assert_eq!(replay["status"], "already_admitted");
    assert_eq!(replay["attempt_id"], admitted["attempt_id"]);
}
