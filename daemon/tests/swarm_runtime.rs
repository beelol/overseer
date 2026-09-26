mod common;

use common::*;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::time::{SystemTime, UNIX_EPOCH};

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

#[test]
fn deadline_interrupts_a_linked_worker_without_another_admission() {
    let d = Daemon::start(&[]);
    let temp = tmp();
    let checkout = repo(&temp.path().join("deadline-source"));
    let run = d.call("swarm.create", json!({"category":"Deadline worker",
        "objective":"Inspect backend","allowed_targets":["fixture-local"],
        "policy":{"deadline_ms":2500}}));
    let id = run["id"].as_str().unwrap();
    d.call("swarm.plan",json!({"id":id,"generation":1,"revision":0,"jobs":[
        {"id":"inspect","title":"Inspect","acceptance":"evidence","deps":[]}
    ]}));
    let at = now();
    let admitted = d.call("swarm.admit",json!({"run_id":id,"generation":1,"revision":1,
        "job_id":"inspect","target_id":"fixture-local","request_id":"deadline-worker",
        "now_ms":at,"snapshot":{"version":1,"observed_ms":at-1000,"expires_ms":at+60000,
            "targets":[{"id":"fixture-local","account_id":"fixture","pool_ids":["fixture-pool"],
                "capabilities":["code"],"health":"up","auth":"ok"}],
            "pools":[{"id":"fixture-pool","windows":[{"id":"run","unit":"points",
                "remaining_milli":100000,"protected_milli":0,"reserved_milli":0,
                "confidence":"exact","expires_ms":at+60000}]}]},
        "required_capabilities":["code"],"estimate_milli":{"points":100},"purpose":"worker"}));
    let launched = d.call("swarm.worker.launch",json!({"run_id":id,"job_id":"inspect",
        "attempt_id":admitted["attempt_id"],"token":admitted["token"],"repo":checkout,
        "program":"/bin/sleep","args":["30"],"prompt":"Inspect",
        "title":"Deadline worker"}));
    assert_eq!(launched["status"],"launched");
    let worker = launched["overseer_run_id"].as_str().unwrap();
    let created_ms = run["created_ms"].as_i64().unwrap();
    for seq in 0..10 {
        d.call("swarm.report",json!({"run_id":id,"job_id":"inspect",
            "attempt_id":admitted["attempt_id"],"token":admitted["token"],
            "message_id":format!("deadline-progress-{seq}"),"type":"progress",
            "revision":1,"payload":{"note":"still working"}}));
        std::thread::sleep(std::time::Duration::from_millis(120));
    }
    let after_progress = d.call("swarm.get",json!({"id":id}));
    assert_eq!(after_progress["created_ms"],created_ms);
    assert_eq!(after_progress["policy"]["effective"]["deadline_ms"],2500);
    let deadline = std::time::Instant::now()+std::time::Duration::from_secs(5);
    while d.call("swarm.get",json!({"id":id}))["status"]!="stopping" {
        assert!(std::time::Instant::now()<deadline,"deadline did not stop run");
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    assert_eq!(d.call("swarm.get",json!({"id":id}))["stop_reason"],"deadline");
    assert_ne!(d.wait_done(worker,5)["status"],"completed");
    let until = std::time::Instant::now()+std::time::Duration::from_secs(5);
    loop {
        let inbox = d.call("swarm.messages",json!({"run_id":id,"recipient":"director"}));
        if inbox["messages"].as_array().unwrap().iter().any(|m|m["type"]=="terminal") {
            break;
        }
        assert!(std::time::Instant::now()<until,"terminal event never reached the director");
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    let job=&d.call("swarm.jobs",json!({"id":id}))["jobs"][0];
    assert_eq!(job["status"],"cancelled");
    assert_eq!(job["attempt_count"],1);
}

#[test]
fn admitted_worker_launch_replays_to_one_supervised_run_after_daemon_restart() {
    let mut d = Daemon::start(&[]);
    let temp = tmp();
    let checkout = repo(&temp.path().join("worker-source"));
    let run = d.call(
        "swarm.create",
        json!({"category":"Runtime bridge",
        "objective":"Inspect backend", "allowed_targets":["fixture-local"],
        "policy":{"max_executing":3}}),
    );
    let id = run["id"].as_str().unwrap();
    d.call(
        "swarm.plan",
        json!({"id":id,"generation":1,"revision":0,"jobs":[
            {"id":"inspect","title":"Inspect","acceptance":"report evidence","deps":[]},
            {"id":"followup","title":"Follow up","acceptance":"report evidence","deps":[]}
        ]}),
    );
    let at = now();
    let admitted = d.call(
        "swarm.admit",
        json!({"run_id":id,"generation":1,"revision":1,
        "job_id":"inspect","target_id":"fixture-local","request_id":"worker-one",
        "now_ms":at,"snapshot":{"version":1,"observed_ms":at-1000,"expires_ms":at+60000,
            "targets":[{"id":"fixture-local","account_id":"fixture","pool_ids":["fixture-pool"],
                "capabilities":["code"],"health":"up","auth":"ok"}],
            "pools":[{"id":"fixture-pool","windows":[{"id":"run","unit":"points",
                "remaining_milli":100000,"protected_milli":0,"reserved_milli":0,
                "confidence":"exact","expires_ms":at+60000}]}]},
        "required_capabilities":["code"],"estimate_milli":{"points":100},"purpose":"worker"}),
    );
    assert_eq!(admitted["status"], "admitted");
    let request = json!({"run_id":id,"job_id":"inspect","attempt_id":admitted["attempt_id"],
        "token":admitted["token"],"repo":checkout,"program":"/bin/sleep",
        "args":["30"],"prompt":"Inspect the backend","title":"Swarm inspect"});
    let launched = d.call("swarm.worker.launch", request.clone());
    assert_eq!(launched["status"], "launched");
    let worker_run = launched["overseer_run_id"].as_str().unwrap();
    assert!(
        ["queued", "starting", "running"].contains(&d.run(worker_run)["status"].as_str().unwrap())
    );
    assert!(d
        .try_call(
            "swarm.attempt.confirm_exit",
            json!({"run_id":id,"generation":1,
        "revision":1,"job_id":"inspect","attempt_id":admitted["attempt_id"]})
        )
        .is_err());
    assert_eq!(
        d.call(
            "swarm.worker.reconcile",
            json!({"run_id":id,"generation":1,
        "revision":1,"job_id":"inspect","attempt_id":admitted["attempt_id"]})
        )["status"],
        "active"
    );
    let sampled_by = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let first_sample = loop {
        let observation = d.call("swarm.worker.liveness",json!({"run_id":id,
            "job_id":"inspect","attempt_id":admitted["attempt_id"]}));
        if observation["state"] == "reachable" {
            break observation;
        }
        assert!(std::time::Instant::now() < sampled_by, "daemon did not sample worker reachability: {observation}");
        std::thread::sleep(std::time::Duration::from_millis(100));
    };
    let db_probe = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let run_dir: String = db_probe.query_row("SELECT run_dir FROM runs WHERE id=?1",
        rusqlite::params![worker_run], |r|r.get(0)).unwrap();
    let launch: serde_json::Value = serde_json::from_slice(&std::fs::read(
        std::path::Path::new(&run_dir).join("launch.json")).unwrap()).unwrap();
    let socket = std::path::PathBuf::from(launch["control_socket"].as_str().unwrap());
    let hidden_socket = socket.with_extension("unreachable");
    std::fs::rename(&socket, &hidden_socket).unwrap();
    let poll_at = first_sample["last_sample_ms"].as_i64().unwrap() + 15_000;
    for step in 0..=4 {
        let polled = d.call("swarm.worker.liveness.poll",json!({"now_ms":poll_at+step*15_000}));
        assert_eq!(polled["sampled"],1,"{polled}");
        let state = d.call("swarm.worker.liveness",json!({"run_id":id,
            "job_id":"inspect","attempt_id":admitted["attempt_id"]}));
        assert_eq!(state["state"], if step == 4 { "unknown" } else { "suspect" }, "{state}");
    }
    assert_eq!(d.call("swarm.worker.reconcile",json!({"run_id":id,"generation":1,
        "revision":1,"job_id":"inspect","attempt_id":admitted["attempt_id"]}))["status"], "unknown");
    std::fs::rename(&hidden_socket, &socket).unwrap();
    let recovered = d.call("swarm.worker.liveness.poll",json!({"now_ms":poll_at+75_000}));
    assert_eq!(recovered["sampled"],1,"{recovered}");
    assert_eq!(d.call("swarm.worker.liveness",json!({"run_id":id,
        "job_id":"inspect","attempt_id":admitted["attempt_id"]}))["state"], "reachable");
    let probe_at = poll_at + 75_001;
    let sample = |time, reachable| d.call("swarm.worker.liveness.sample", json!({
        "run_id":id,"job_id":"inspect","attempt_id":admitted["attempt_id"],
        "now_ms":time,"reachable":reachable}));
    assert_eq!(sample(probe_at, true)["state"], "reachable");
    assert_eq!(sample(probe_at + 1, false)["state"], "suspect");
    d.call("swarm.report", json!({"run_id":id,"job_id":"inspect",
        "attempt_id":admitted["attempt_id"],"token":admitted["token"],
        "message_id":"progress-does-not-reset-liveness","type":"progress",
        "revision":1,"payload":{"note":"still working"}}));
    assert_eq!(sample(probe_at + 60_000, false)["state"], "suspect");
    let unknown_after_threshold = sample(probe_at + 60_001, false);
    assert_eq!(unknown_after_threshold["state"], "unknown");
    assert_eq!(unknown_after_threshold["unreachable_since_ms"], probe_at + 1);
    assert!(d.try_call("swarm.worker.liveness.sample",json!({"run_id":id,
        "job_id":"inspect","attempt_id":admitted["attempt_id"],
        "now_ms":probe_at + 60_000,"reachable":true})).is_err());
    assert_eq!(d.call("swarm.worker.liveness",json!({"run_id":id,
        "job_id":"inspect","attempt_id":admitted["attempt_id"]}))["state"], "unknown");
    assert_eq!(d.call("swarm.worker.reconcile",json!({"run_id":id,"generation":1,
        "revision":1,"job_id":"inspect","attempt_id":admitted["attempt_id"]}))["status"], "unknown");
    assert_eq!(sample(probe_at + 60_002, true)["state"], "reachable");
    assert_eq!(d.call("swarm.worker.reconcile",json!({"run_id":id,"generation":1,
        "revision":1,"job_id":"inspect","attempt_id":admitted["attempt_id"]}))["status"], "active");
    let attempts: i64 = rusqlite::Connection::open(d.home.path().join("overseer.sqlite"))
        .unwrap().query_row("SELECT attempt_count FROM swarm_jobs WHERE run_id=?1 AND id='inspect'",
        rusqlite::params![id], |r|r.get(0)).unwrap();
    assert_eq!(attempts, 1);
    // A lost supervisor/transport is not proof that its child process exited.
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    db.execute("UPDATE runs SET status='disconnected',ended_ms=?2 WHERE id=?1",
        rusqlite::params![worker_run, now()]).unwrap();
    let unknown = d.call("swarm.worker.reconcile",json!({"run_id":id,"generation":1,
        "revision":1,"job_id":"inspect","attempt_id":admitted["attempt_id"]}));
    assert_eq!(unknown["status"], "unknown", "{unknown}");
    assert!(d.try_call("swarm.attempt.confirm_exit",json!({"run_id":id,
        "generation":1,"revision":1,"job_id":"inspect",
        "attempt_id":admitted["attempt_id"]})).is_err());
    std::thread::sleep(std::time::Duration::from_millis(1200));
    assert_eq!(d.call("swarm.worker.reconcile",json!({"run_id":id,"generation":1,
        "revision":1,"job_id":"inspect","attempt_id":admitted["attempt_id"]}))["status"], "unknown");
    let attempt_status: String = db.query_row("SELECT status FROM swarm_attempts WHERE id=?1",
        rusqlite::params![admitted["attempt_id"].as_str().unwrap()], |r|r.get(0)).unwrap();
    assert_eq!(attempt_status, "registered");
    let reservations: i64 = db.query_row("SELECT COUNT(*) FROM swarm_reservations WHERE attempt_id=?1 AND status='active'",
        rusqlite::params![admitted["attempt_id"].as_str().unwrap()], |r|r.get(0)).unwrap();
    assert!(reservations > 0);
    db.execute("UPDATE runs SET status='running',ended_ms=NULL WHERE id=?1",
        rusqlite::params![worker_run]).unwrap();
    let jobs = d.call("swarm.jobs", json!({"id":id}))["jobs"]
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(
        jobs.iter().find(|job| job["id"] == "inspect").unwrap()["status"],
        "reserved"
    );
    let second = d.call(
        "swarm.admit",
        json!({"run_id":id,"generation":1,"revision":1,
        "job_id":"followup","target_id":"fixture-local","request_id":"worker-two",
        "now_ms":at,"snapshot":{"version":1,"observed_ms":at-1000,"expires_ms":at+60000,
            "targets":[{"id":"fixture-local","account_id":"fixture","pool_ids":["fixture-pool"],
                "capabilities":["code"],"health":"up","auth":"ok"}],
            "pools":[{"id":"fixture-pool","windows":[{"id":"run","unit":"points",
                "remaining_milli":100000,"protected_milli":0,"reserved_milli":0,
                "confidence":"exact","expires_ms":at+60000}]}]},
        "required_capabilities":["code"],"estimate_milli":{"points":100},"purpose":"worker"}),
    );
    assert_eq!(second["status"], "admitted", "{second}");
    d.kill9();
    d.spawn();
    assert_eq!(d.call("swarm.worker.liveness",json!({"run_id":id,
        "job_id":"inspect","attempt_id":admitted["attempt_id"]}))["state"], "reachable");
    let replay = d.call("swarm.worker.launch", request.clone());
    assert_eq!(replay["overseer_run_id"], worker_run);
    assert_eq!(replay["duplicate"], true);
    assert_eq!(
        d.runs()
            .iter()
            .filter(|run| run["title"] == "Swarm inspect")
            .count(),
        1
    );
    let mut changed = request;
    changed["args"] = json!(["1"]);
    assert!(d.try_call("swarm.worker.launch", changed).is_err());
    d.call("swarm.revise",json!({"id":id,"generation":1,"expected_revision":1,
        "reason":"Inspect a newly identified authorization path", "jobs":[
            {"id":"inspect","title":"Inspect","acceptance":"report both paths","deps":[]},
            {"id":"followup","title":"Follow up","acceptance":"report evidence","deps":[]}
        ]}));
    d.call(
        "swarm.stop",
        json!({"run_id":id,"generation":1,"revision":2}),
    );
    d.wait_done(worker_run, 5);
    let reconciled = d.call(
        "swarm.worker.reconcile",
        json!({"run_id":id,"generation":1,
        "revision":2,"job_id":"inspect","attempt_id":admitted["attempt_id"]}),
    );
    assert_eq!(reconciled["status"], "terminal");
    assert_eq!(
        d.call(
            "swarm.worker.reconcile",
            json!({"run_id":id,"generation":1,
        "revision":2,"job_id":"inspect","attempt_id":admitted["attempt_id"]})
        )["duplicate"],
        true
    );
    let inbox = d.call(
        "swarm.messages",
        json!({"run_id":id,"recipient":"director"}),
    )["messages"]
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(
        inbox
            .iter()
            .filter(|message| message["type"] == "terminal")
            .count(),
        1
    );
    assert_eq!(
        d.call("swarm.jobs", json!({"id":id}))["jobs"]
            .as_array()
            .unwrap()
            .iter()
            .find(|job| job["id"] == "inspect")
            .unwrap()["status"],
        "cancelled"
    );
}

#[test]
fn pending_worker_launch_cannot_resume_after_stop() {
    let d = Daemon::start(&[]);
    let temp = tmp();
    let checkout = repo(&temp.path().join("stopped-source"));
    let run = d.call(
        "swarm.create",
        json!({"category":"Stopped pending launch",
        "objective":"Inspect backend","allowed_targets":["fixture-local"]}),
    );
    let id = run["id"].as_str().unwrap();
    d.call(
        "swarm.plan",
        json!({"id":id,"generation":1,"revision":0,"jobs":[
            {"id":"inspect","title":"Inspect","acceptance":"evidence","deps":[]}
        ]}),
    );
    let at = now();
    let admitted = d.call(
        "swarm.admit",
        json!({"run_id":id,"generation":1,"revision":1,
        "job_id":"inspect","target_id":"fixture-local","request_id":"stopped-worker",
        "now_ms":at,"snapshot":{"version":1,"observed_ms":at-1000,"expires_ms":at+60000,
            "targets":[{"id":"fixture-local","account_id":"fixture","pool_ids":["fixture-pool"],
                "capabilities":["code"],"health":"up","auth":"ok"}],
            "pools":[{"id":"fixture-pool","windows":[{"id":"run","unit":"points",
                "remaining_milli":100000,"protected_milli":0,"reserved_milli":0,
                "confidence":"exact","expires_ms":at+60000}]}]},
        "required_capabilities":["code"],"estimate_milli":{"points":100},"purpose":"worker"}),
    );
    let request = json!({"run_id":id,"job_id":"inspect","attempt_id":admitted["attempt_id"],
        "token":admitted["token"],"repo":checkout,"program":"/bin/sleep",
        "args":["30"],"prompt":"Inspect the backend","title":"Stopped worker"});
    let digest = format!("{:x}", Sha256::digest(request.to_string().as_bytes()));
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    db.execute(
        "INSERT INTO swarm_worker_launches(attempt_id,run_id,job_id,request_sha256,created_ms)
        VALUES(?1,?2,'inspect',?3,?4)",
        rusqlite::params![admitted["attempt_id"].as_str().unwrap(), id, digest, at],
    )
    .unwrap();
    d.call(
        "swarm.stop",
        json!({"run_id":id,"generation":1,"revision":1}),
    );
    assert!(d.try_call("swarm.worker.launch", request).is_err());
    assert!(d.runs().is_empty());
}

#[test]
fn finished_attempt_cannot_launch_a_worker() {
    let d = Daemon::start(&[]);
    let temp = tmp();
    let checkout = repo(&temp.path().join("finished-source"));
    let run = d.call(
        "swarm.create",
        json!({"category":"Finished launch",
        "objective":"Inspect backend","allowed_targets":["fixture-local"]}),
    );
    let id = run["id"].as_str().unwrap();
    d.call(
        "swarm.plan",
        json!({"id":id,"generation":1,"revision":0,"jobs":[
            {"id":"inspect","title":"Inspect","acceptance":"evidence","deps":[]}
        ]}),
    );
    let attempt = d.call(
        "swarm.attempt.register",
        json!({"run_id":id,"generation":1,
        "revision":1,"job_id":"inspect"}),
    );
    let aid = attempt["id"].as_str().unwrap();
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    db.execute("INSERT INTO swarm_admissions(run_id,request_id,request_sha256,job_id,attempt_id,target_id,created_ms)
        VALUES(?1,'fixture-admission','fixture','inspect',?2,'fixture-local',?3)",
        rusqlite::params![id,aid,now()]).unwrap();
    d.call(
        "swarm.attempt.confirm_exit",
        json!({"run_id":id,"generation":1,
        "revision":1,"job_id":"inspect","attempt_id":aid}),
    );
    let request = json!({"run_id":id,"job_id":"inspect","attempt_id":aid,
        "token":attempt["token"],"repo":checkout,"program":"/bin/sleep",
        "args":["30"],"prompt":"Inspect the backend","title":"Finished worker"});
    assert!(d.try_call("swarm.worker.launch", request).is_err());
    assert!(d.runs().is_empty());
}
