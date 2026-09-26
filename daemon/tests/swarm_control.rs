mod common;

use common::*;
use serde_json::json;

#[test]
fn daemon_stop_all_cancels_swarm_without_a_supervised_worker() {
    let mut d = Daemon::start(&[]);
    let run = d.call("swarm.create", json!({"category":"Global stop","objective":"Audit backend",
        "allowed_targets":["fixture-local"]}));
    let id = run["id"].as_str().unwrap();
    d.call("swarm.plan", json!({"id":id,"generation":1,"revision":0,"jobs":[
        {"id":"queued","title":"Queued audit","acceptance":"evidence","deps":[]}
    ]}));

    let stopped = d.call("daemon.stop_all", json!({}));
    assert_eq!(stopped["swarms"], json!([id]));
    assert!(d.child.as_mut().unwrap().wait().unwrap().success());
    d.child = None;
    d.spawn();
    assert_eq!(d.call("swarm.get", json!({"id":id}))["status"], "stopped");
    assert_eq!(d.call("swarm.jobs", json!({"id":id}))["jobs"][0]["status"], "cancelled");
    assert!(d.try_call("swarm.attempt.register", json!({"run_id":id,"generation":1,
        "revision":1,"job_id":"queued"})).is_err());
}

#[test]
fn daemon_stop_all_interrupts_linked_swarm_worker_and_preserves_attempt() {
    let mut d = Daemon::start(&[]);
    let temp = tmp();
    let checkout = repo(&temp.path().join("global-stop-source"));
    let run = d.call("swarm.create", json!({"category":"Global active stop",
        "objective":"Audit backend","allowed_targets":["fixture-local"]}));
    let id = run["id"].as_str().unwrap();
    d.call("swarm.plan", json!({"id":id,"generation":1,"revision":0,"jobs":[
        {"id":"active","title":"Inspect","acceptance":"evidence","deps":[]},
        {"id":"queued","title":"Queued","acceptance":"evidence","deps":[]}
    ]}));
    let at = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64;
    let admitted = d.call("swarm.admit", json!({"run_id":id,"generation":1,"revision":1,
        "job_id":"active","target_id":"fixture-local","request_id":"global-stop-worker",
        "now_ms":at,"snapshot":{"version":1,"observed_ms":at-1000,"expires_ms":at+60000,
            "targets":[{"id":"fixture-local","account_id":"fixture","pool_ids":["fixture-pool"],
                "capabilities":["code"],"health":"up","auth":"ok"}],
            "pools":[{"id":"fixture-pool","windows":[{"id":"run","unit":"points",
                "remaining_milli":100000,"protected_milli":0,"reserved_milli":0,
                "confidence":"exact","expires_ms":at+60000}]}]},
        "required_capabilities":["code"],"estimate_milli":{"points":100},"purpose":"worker"}));
    let launched = d.call("swarm.worker.launch", json!({"run_id":id,"job_id":"active",
        "attempt_id":admitted["attempt_id"],"token":admitted["token"],"repo":checkout,
        "program":"/bin/sleep","args":["30"],"prompt":"Inspect","title":"Global stop worker"}));
    let worker = launched["overseer_run_id"].as_str().unwrap();
    d.wait_status(worker, |status| status == "running", 10);

    let stopped = d.call("daemon.stop_all", json!({}));
    assert_eq!(stopped["swarms"], json!([id]));
    assert_eq!(stopped["remaining"], json!([]));
    assert!(d.child.as_mut().unwrap().wait().unwrap().success());
    d.child = None;
    d.spawn();
    assert_eq!(d.call("swarm.jobs", json!({"id":id}))["jobs"][1]["status"], "cancelled");
    assert_ne!(d.run(worker)["status"], "running");
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let attempts: i64 = db.query_row("SELECT COUNT(*) FROM swarm_attempts WHERE run_id=?1 AND job_id='active'",
        [id], |row| row.get(0)).unwrap();
    assert_eq!(attempts, 1);
}

#[test]
fn pause_resume_and_off_keep_active_evidence_but_stop_new_delegation() {
    let d = Daemon::start(&[]);
    let run = d.call(
        "swarm.create",
        json!({"category":"Controls","objective":"Audit","allowed_targets":["system-codex"]}),
    );
    let id = run["id"].as_str().unwrap();
    d.call(
        "swarm.plan",
        json!({"id":id,"generation":1,"revision":0,"jobs":[
            {"id":"active","title":"Active","acceptance":"evidence","deps":[]},
            {"id":"queued","title":"Queued","acceptance":"evidence","deps":[]}
        ]}),
    );
    let attempt = d.call(
        "swarm.attempt.register",
        json!({"run_id":id,"generation":1,"revision":1,"job_id":"active"}),
    );
    let aid = attempt["id"].as_str().unwrap();
    let token = attempt["token"].as_str().unwrap();
    assert_eq!(
        d.call(
            "swarm.pause",
            json!({"run_id":id,"generation":1,"revision":1})
        )["status"],
        "paused"
    );
    assert!(d
        .try_call(
            "swarm.attempt.register",
            json!({"run_id":id,"generation":1,"revision":1,"job_id":"queued"})
        )
        .is_err());
    let pending = d.call("swarm.messages", json!({"run_id":id,"recipient":aid}));
    assert!(pending["messages"]
        .as_array()
        .unwrap()
        .iter()
        .any(|m| m["type"] == "checkpoint"));
    assert_eq!(
        d.call(
            "swarm.resume",
            json!({"run_id":id,"generation":1,"revision":1})
        )["status"],
        "running"
    );
    assert_eq!(
        d.call(
            "swarm.off",
            json!({"run_id":id,"generation":1,"revision":1})
        )["status"],
        "draining"
    );
    let jobs = d.call("swarm.jobs", json!({"id":id}));
    assert_eq!(
        jobs["jobs"]
            .as_array()
            .unwrap()
            .iter()
            .find(|j| j["id"] == "queued")
            .unwrap()["status"],
        "cancelled"
    );
    assert_eq!(
        jobs["jobs"]
            .as_array()
            .unwrap()
            .iter()
            .find(|j| j["id"] == "active")
            .unwrap()["status"],
        "reserved"
    );
    d.call(
        "swarm.artifact.put",
        json!({"run_id":id,"job_id":"active","attempt_id":aid,"token":token,
            "artifact_id":"drained-evidence","source_revision":1,
            "kind":"finding","content":"inspected active job"}),
    );
    d.call(
        "swarm.report",
        json!({"run_id":id,"job_id":"active","attempt_id":aid,"token":token,
        "message_id":"late-after-off","type":"result","revision":1,
        "payload":{"artifact_ids":["drained-evidence"]}}),
    );
    assert_eq!(
        d.call("swarm.jobs", json!({"id":id}))["jobs"]
            .as_array()
            .unwrap()
            .iter()
            .find(|j| j["id"] == "active")
            .unwrap()["status"],
        "submitted"
    );
    assert!(d
        .try_call(
            "swarm.attempt.register",
            json!({"run_id":id,"generation":1,"revision":1,"job_id":"queued"})
        )
        .is_err());
    d.call("swarm.attempt.confirm_exit",json!({"run_id":id,"generation":1,
        "revision":1,"job_id":"active","attempt_id":aid}));
    assert_eq!(d.call("swarm.get",json!({"id":id}))["status"],"draining");
    assert_eq!(d.call("swarm.decide",json!({"run_id":id,"generation":1,"revision":1,
        "job_id":"active","decision":"accept","evidence":["drained-evidence"]}))["status"],"accepted");
    assert_eq!(d.call("swarm.get",json!({"id":id}))["status"],"stopped");
}

#[test]
fn deadline_on_admission_stops_queued_work_without_claiming_completion() {
    let d = Daemon::start(&[]);
    let run=d.call("swarm.create",json!({"category":"Deadline","objective":"Audit","allowed_targets":["target"],"policy":{"deadline_ms":60000}}));
    let id = run["id"].as_str().unwrap();
    d.call("swarm.plan",json!({"id":id,"generation":1,"revision":0,"jobs":[{"id":"j","title":"J","acceptance":"evidence","deps":[]}]}));
    let at = run["created_ms"].as_i64().unwrap() + 60000;
    let snapshot = json!({"version":1,"observed_ms":at-1000,"expires_ms":at+60000,
        "targets":[{"id":"target","account_id":"account","pool_ids":["pool"],"capabilities":["code"],"health":"up","auth":"ok"}],
        "pools":[{"id":"pool","windows":[{"id":"week","unit":"points","remaining_milli":1000000,"protected_milli":0,"reserved_milli":0,"confidence":"exact","expires_ms":at+60000}]}]});
    let result=d.call("swarm.admit",json!({"run_id":id,"generation":1,"revision":1,"job_id":"j","target_id":"target","request_id":"expired","snapshot":snapshot,"now_ms":at,"required_capabilities":["code"],"estimate_milli":{"points":100},"purpose":"worker"}));
    assert_eq!(result["reason"], "run_deadline");
    assert_eq!(d.call("swarm.get", json!({"id":id}))["status"], "stopped");
    assert_eq!(d.call("swarm.get", json!({"id":id}))["stop_reason"], "deadline");
    assert_eq!(
        d.call("swarm.jobs", json!({"id":id}))["jobs"][0]["status"],
        "cancelled"
    );
}

#[test]
fn deadline_expires_without_another_admission_while_active_or_blocked() {
    let d = Daemon::start(&[]);
    let active = d.call("swarm.create", json!({"category":"Timed active","objective":"Audit",
        "allowed_targets":["system-codex"],"policy":{"deadline_ms":1500}}));
    let active_id = active["id"].as_str().unwrap();
    d.call("swarm.plan",json!({"id":active_id,"generation":1,"revision":0,"jobs":[
        {"id":"working","title":"Working","acceptance":"evidence","deps":[]},
        {"id":"queued","title":"Queued","acceptance":"evidence","deps":[]}
    ]}));
    let attempt=d.call("swarm.attempt.register",json!({"run_id":active_id,"job_id":"working",
        "generation":1,"revision":1}));
    let blocked = d.call("swarm.create", json!({"category":"Timed blocked","objective":"Audit",
        "allowed_targets":[],"policy":{"deadline_ms":1500}}));
    let blocked_id = blocked["id"].as_str().unwrap();
    d.call("swarm.plan",json!({"id":blocked_id,"generation":1,"revision":0,"jobs":[
        {"id":"blocked","title":"Blocked","acceptance":"evidence","deps":[]}
    ]}));
    let deadline=std::time::Instant::now()+std::time::Duration::from_secs(5);
    while std::time::Instant::now()<deadline {
        if d.call("swarm.get",json!({"id":active_id}))["status"]=="stopping"
            && d.call("swarm.get",json!({"id":blocked_id}))["status"]=="stopped" {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    assert_eq!(d.call("swarm.get",json!({"id":active_id}))["status"],"stopping");
    assert_eq!(d.call("swarm.get",json!({"id":blocked_id}))["status"],"stopped");
    assert_eq!(d.call("swarm.get",json!({"id":active_id}))["stop_reason"],"deadline");
    assert_eq!(d.call("swarm.get",json!({"id":blocked_id}))["stop_reason"],"deadline");
    let jobs=d.call("swarm.jobs",json!({"id":active_id}));
    assert_eq!(jobs["jobs"].as_array().unwrap().iter().find(|j|j["id"]=="working").unwrap()["status"],"cancel_requested");
    assert_eq!(jobs["jobs"].as_array().unwrap().iter().find(|j|j["id"]=="queued").unwrap()["status"],"cancelled");
    assert_eq!(d.call("swarm.jobs",json!({"id":blocked_id}))["jobs"][0]["status"],"cancelled");
    assert!(d.call("swarm.messages",json!({"run_id":active_id,"recipient":attempt["id"]}))["messages"]
        .as_array().unwrap().iter().any(|m|m["type"]=="stop"));
}
