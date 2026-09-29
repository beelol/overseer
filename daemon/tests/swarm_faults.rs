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
fn full_storage_blocks_new_admissions_until_write_capacity_recovers() {
    let mut d = Daemon::start(&[]);
    let run = planned(&d, "Full storage");
    let attempt = d.call("swarm.attempt.register",
        json!({"run_id":run,"job_id":"audit","generation":1,"revision":1}));
    let other = planned(&d, "Waiting category");
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as i64;
    let admission = json!({"run_id":other,"generation":1,"revision":1,"job_id":"audit",
        "target_id":"codex-a","request_id":"after-full","now_ms":now,
        "snapshot":{"version":1,"observed_ms":now-1000,"expires_ms":now+60000,
            "targets":[{"id":"codex-a","account_id":"account-a","pool_ids":["shared"],
                "capabilities":["code"],"health":"up","auth":"ok"}],
            "pools":[{"id":"shared","windows":[{"id":"week","unit":"points",
                "remaining_milli":60000,"protected_milli":0,"reserved_milli":0,
                "confidence":"exact","expires_ms":now+60000}]}]},
        "required_capabilities":["code"],"estimate_milli":{"points":1000},"purpose":"worker"});
    d.call("swarm.storage.limit_pages", json!({"mode":"current"}));
    let report = json!({"run_id":run,"job_id":"audit","attempt_id":attempt["id"],
        "token":attempt["token"],"message_id":"full-result","type":"result",
        "revision":1,"payload":{"artifact_ids":[],"note":"x".repeat(28_000)}});
    assert!(d.try_call("swarm.report", report.clone()).is_err());
    assert_eq!(d.call("swarm.storage.status", json!({}))["state"], "blocked");
    assert_eq!(d.call("state", json!({}))["daemon"]["swarm_storage"], "blocked");
    assert!(d.try_call("swarm.admit", admission.clone()).unwrap_err().contains("storage"));
    assert!(d.try_call("swarm.storage.recover", json!({})).is_err());
    assert_eq!(d.call("swarm.storage.status", json!({}))["state"], "blocked");
    // SQLite's page ceiling is connection-local; reapply it on restart to
    // emulate a disk that is still full after the daemon process dies.
    d.env.push(("OVERSEER_TEST_SWARM_STORAGE_PAGE_LIMIT".into(), "current".into()));
    d.kill9();
    d.spawn();
    assert_eq!(d.call("swarm.storage.status", json!({}))["state"], "blocked");
    assert!(d.try_call("swarm.admit", admission.clone()).unwrap_err().contains("storage"));
    d.call("swarm.storage.limit_pages", json!({"mode":"unlimited"}));
    assert_eq!(d.call("swarm.storage.recover", json!({}))["state"], "ready");
    assert_eq!(d.call("swarm.report", report.clone())["duplicate"], false);
    assert_eq!(d.call("swarm.report", report)["duplicate"], true);
    assert_eq!(d.call("swarm.admit", admission)["status"], "admitted");
}

#[test]
fn failed_discovery_write_replays_once_after_restart() {
    let mut d = Daemon::start(&[]);
    let run = planned(&d, "Discovery write fault");
    let attempt = d.call("swarm.attempt.register",
        json!({"run_id":run,"job_id":"audit","generation":1,"revision":1}));
    let report = json!({"run_id":run,"job_id":"audit","attempt_id":attempt["id"],
        "token":attempt["token"],"message_id":"discovery-once","type":"discovery",
        "revision":1,"payload":{"symbol":"TaskRepository.findById"}});
    let db = database(&d);
    db.execute_batch("CREATE TRIGGER fail_discovery BEFORE INSERT ON swarm_messages
        WHEN NEW.kind='discovery' BEGIN SELECT RAISE(FAIL,'injected discovery fault'); END;").unwrap();
    assert!(d.try_call("swarm.report", report.clone()).is_err());
    let before: i64 = db.query_row("SELECT COUNT(*) FROM swarm_messages WHERE run_id=?1",
        [&run], |r| r.get(0)).unwrap();
    assert_eq!(before, 0);
    db.execute_batch("DROP TRIGGER fail_discovery;").unwrap();
    d.kill9();
    d.spawn();
    assert_eq!(d.call("swarm.report", report.clone())["duplicate"], false);
    assert_eq!(d.call("swarm.report", report)["duplicate"], true);
    let after: i64 = db.query_row("SELECT COUNT(*) FROM swarm_messages WHERE run_id=?1",
        [&run], |r| r.get(0)).unwrap();
    assert_eq!(after, 1);
}

#[test]
fn failed_dispatch_intent_cannot_admit_or_launch_and_replays_once() {
    let mut d = Daemon::start(&[]);
    let run = planned(&d, "Dispatch intent fault");
    let temp = tmp();
    let checkout = repo(&temp.path().join("dispatch-intent-source"));
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as i64;
    let request = json!({"request_id":"dispatch-after-fault","target_id":"codex-a",
        "repo":checkout,"program":"/bin/sleep","args":["30"],"now_ms":now,
        "snapshot":{"version":1,"observed_ms":now-1000,"expires_ms":now+60000,
            "targets":[{"id":"codex-a","account_id":"account-a","pool_ids":["shared"],
                "capabilities":["code"],"health":"up","auth":"ok"}],
            "pools":[{"id":"shared","windows":[{"id":"week","unit":"points",
                "remaining_milli":60000,"protected_milli":0,"reserved_milli":0,
                "confidence":"exact","expires_ms":now+60000}]}]},
        "required_capabilities":["code"],"estimate_milli":{"points":1000},"purpose":"worker"});
    let db = database(&d);
    db.execute_batch("CREATE TRIGGER fail_dispatch_intent BEFORE INSERT ON swarm_dispatch_intents
        BEGIN SELECT RAISE(FAIL,'injected intent fault'); END;").unwrap();
    assert!(d.try_call("swarm.dispatch.next", request.clone()).is_err());
    for table in ["swarm_dispatch_intents", "swarm_admissions", "swarm_attempts", "swarm_worker_launches"] {
        let count: i64 = db.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0)).unwrap();
        assert_eq!(count, 0, "{table} changed before dispatch intent committed");
    }
    db.execute_batch("DROP TRIGGER fail_dispatch_intent;").unwrap();
    d.kill9();
    d.spawn();
    let launched = d.call("swarm.dispatch.next", request.clone());
    assert_eq!(launched["status"], "launched", "{launched}");
    let replay = d.call("swarm.dispatch.next", request);
    assert_eq!(replay["overseer_run_id"], launched["overseer_run_id"]);
    assert_eq!(replay["duplicate"], true);
    let count: i64 = db.query_row("SELECT COUNT(*) FROM swarm_attempts WHERE run_id=?1",
        [&run], |r| r.get(0)).unwrap();
    assert_eq!(count, 1);
    d.call("run.interrupt", json!({"run_id":launched["overseer_run_id"]}));
    d.wait_done(launched["overseer_run_id"].as_str().unwrap(), 8);
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

/// SWARM-50: storage-full (a real `SQLITE_FULL` from the fixture page ceiling)
/// during discovery persistence. The discovery gets no receipt and leaves no
/// row; Swarm storage reads blocked and new admission fails closed; after
/// capacity returns and recovery clears the block, the sender's identical
/// discovery commits once and its replay is recognized.
#[test]
fn full_storage_during_discovery_withholds_its_receipt_and_replays_once() {
    let d = Daemon::start(&[]);
    let run = planned(&d, "Full storage discovery");
    let attempt = d.call("swarm.attempt.register",
        json!({"run_id":run,"job_id":"audit","generation":1,"revision":1}));
    let other = planned(&d, "Waiting behind full storage");
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as i64;
    let admission = json!({"run_id":other,"generation":1,"revision":1,"job_id":"audit",
        "target_id":"codex-a","request_id":"after-full-discovery","now_ms":now,
        "snapshot":{"version":1,"observed_ms":now-1000,"expires_ms":now+60000,
            "targets":[{"id":"codex-a","account_id":"account-a","pool_ids":["shared"],
                "capabilities":["code"],"health":"up","auth":"ok"}],
            "pools":[{"id":"shared","windows":[{"id":"week","unit":"points",
                "remaining_milli":60000,"protected_milli":0,"reserved_milli":0,
                "confidence":"exact","expires_ms":now+60000}]}]},
        "required_capabilities":["code"],"estimate_milli":{"points":1000},"purpose":"worker"});
    // An event acknowledged before the disk fills must survive it.
    let earlier = json!({"run_id":run,"job_id":"audit","attempt_id":attempt["id"],
        "token":attempt["token"],"message_id":"before-full","type":"progress",
        "revision":1,"payload":{"step":"reading routes"}});
    assert_eq!(d.call("swarm.report", earlier)["duplicate"], false);
    d.call("swarm.storage.limit_pages", json!({"mode":"current"}));
    let discovery = json!({"run_id":run,"job_id":"audit","attempt_id":attempt["id"],
        "token":attempt["token"],"message_id":"full-discovery","type":"discovery",
        "revision":1,"payload":{"symbol":"TaskRepository.findById","trace":"x".repeat(28_000)}});
    let refused = d.try_call("swarm.report", discovery.clone()).unwrap_err();
    assert!(!refused.contains("duplicate"), "{refused}");
    let db = database(&d);
    let stored: i64 = db.query_row("SELECT COUNT(*) FROM swarm_messages WHERE run_id=?1",
        [&run], |r| r.get(0)).unwrap();
    assert_eq!(stored, 1, "only the earlier acknowledged event: no partial discovery survives");
    assert_eq!(d.call("swarm.storage.status", json!({}))["state"], "blocked");
    assert!(d.try_call("swarm.admit", admission.clone()).unwrap_err().contains("storage"));
    d.call("swarm.storage.limit_pages", json!({"mode":"unlimited"}));
    assert_eq!(d.call("swarm.storage.recover", json!({}))["state"], "ready");
    assert_eq!(d.call("swarm.report", discovery.clone())["duplicate"], false);
    assert_eq!(d.call("swarm.report", discovery)["duplicate"], true);
    let stored: Vec<String> = {
        let mut stmt = db.prepare("SELECT message_id FROM swarm_messages WHERE run_id=?1 ORDER BY seq").unwrap();
        let rows = stmt.query_map([&run], |r| r.get(0)).unwrap().map(Result::unwrap).collect();
        rows
    };
    assert_eq!(stored, vec!["before-full".to_string(), "full-discovery".to_string()]);
    assert_eq!(d.call("swarm.admit", admission)["status"], "admitted");
}

/// SWARM-50: storage-full during the dispatch intent. Nothing is admitted or
/// launched from the uncommitted intent (no intent, admission, attempt or
/// worker launch row, no worker process); storage reads blocked and a second
/// dispatch is refused before it writes; after recovery the identical request
/// launches one worker and its replay returns that worker.
#[test]
fn full_storage_during_dispatch_intent_launches_nothing_until_recovery() {
    let d = Daemon::start(&[]);
    let run = planned(&d, "Full storage dispatch");
    let temp = tmp();
    let checkout = repo(&temp.path().join("full-dispatch-source"));
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as i64;
    let request = json!({"request_id":"dispatch-after-full","target_id":"codex-a",
        "repo":checkout,"program":"/bin/sleep","args":["30"],"now_ms":now,
        "note":"x".repeat(28_000),
        "snapshot":{"version":1,"observed_ms":now-1000,"expires_ms":now+60000,
            "targets":[{"id":"codex-a","account_id":"account-a","pool_ids":["shared"],
                "capabilities":["code"],"health":"up","auth":"ok"}],
            "pools":[{"id":"shared","windows":[{"id":"week","unit":"points",
                "remaining_milli":60000,"protected_milli":0,"reserved_milli":0,
                "confidence":"exact","expires_ms":now+60000}]}]},
        "required_capabilities":["code"],"estimate_milli":{"points":1000},"purpose":"worker"});
    let runs_before = d.runs().len();
    d.call("swarm.storage.limit_pages", json!({"mode":"current"}));
    assert!(d.try_call("swarm.dispatch.next", request.clone()).is_err());
    let db = database(&d);
    for table in ["swarm_dispatch_intents", "swarm_admissions", "swarm_attempts", "swarm_worker_launches"] {
        let count: i64 = db.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0)).unwrap();
        assert_eq!(count, 0, "{table} changed although the intent never committed");
    }
    assert_eq!(d.runs().len(), runs_before, "no worker process from an uncommitted intent");
    assert_eq!(d.call("swarm.storage.status", json!({}))["state"], "blocked");
    assert!(d.try_call("swarm.dispatch.next", request.clone()).unwrap_err().contains("storage"));
    d.call("swarm.storage.limit_pages", json!({"mode":"unlimited"}));
    assert_eq!(d.call("swarm.storage.recover", json!({}))["state"], "ready");
    let launched = d.call("swarm.dispatch.next", request.clone());
    assert_eq!(launched["status"], "launched", "{launched}");
    let replay = d.call("swarm.dispatch.next", request);
    assert_eq!((replay["overseer_run_id"].clone(), replay["duplicate"].clone()),
        (launched["overseer_run_id"].clone(), json!(true)));
    let attempts: i64 = db.query_row("SELECT COUNT(*) FROM swarm_attempts WHERE run_id=?1",
        [&run], |r| r.get(0)).unwrap();
    assert_eq!(attempts, 1);
    d.call("run.interrupt", json!({"run_id":launched["overseer_run_id"]}));
    d.wait_done(launched["overseer_run_id"].as_str().unwrap(), 8);
}
