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

fn snapshot(at: i64, remaining: i64) -> Value {
    json!({"version":1,"observed_ms":at-1000,"expires_ms":at+60000,
        "targets":[
            {"id":"codex-a","account_id":"account-a","pool_ids":["shared"],"capabilities":["code"],"health":"up","auth":"ok"},
            {"id":"opencode-a","account_id":"account-a","pool_ids":["shared"],"capabilities":["code"],"health":"up","auth":"ok"}
        ],"pools":[{"id":"shared","windows":[{"id":"week","unit":"points","remaining_milli":remaining,
            "protected_milli":0,"reserved_milli":0,"confidence":"exact","expires_ms":at+60000}]}]})
}

fn setup(d: &Daemon, category: &str, count: usize) -> String {
    let run = d.call(
        "swarm.create",
        json!({"category":category,"objective":"Audit","allowed_targets":["codex-a","opencode-a"]}),
    );
    let id = run["id"].as_str().unwrap().to_string();
    let jobs: Vec<_>=(0..count).map(|n|json!({"id":format!("j{n}"),"title":format!("J{n}"),"acceptance":"evidence","deps":[]})).collect();
    d.call(
        "swarm.plan",
        json!({"id":id,"generation":1,"revision":0,"jobs":jobs}),
    );
    id
}

fn admit(
    d: &Daemon,
    run: &str,
    job: &str,
    target: &str,
    request: &str,
    at: i64,
    remaining: i64,
    estimate: i64,
) -> Result<Value, String> {
    d.try_call(
        "swarm.admit",
        json!({"run_id":run,"generation":1,"revision":1,"job_id":job,
        "target_id":target,"request_id":request,"snapshot":snapshot(at,remaining),"now_ms":at,
        "required_capabilities":["code"],"estimate_milli":{"points":estimate},"purpose":"worker"}),
    )
}

#[test]
fn one_run_freezes_allocation_and_dedupes_replayed_admission() {
    let d = Daemon::start(&[]);
    let id = setup(&d, "Backend admission", 3);
    let at = now();
    let first = admit(&d, &id, "j0", "codex-a", "req-0", at, 60000, 4000).unwrap();
    assert_eq!(first["status"], "admitted");
    assert_eq!(first["allocation_milli"], 6000);
    let replay = admit(&d, &id, "j0", "codex-a", "req-0", at, 60000, 4000).unwrap();
    assert_eq!(replay["status"], "already_admitted");
    assert_eq!(replay["attempt_id"], first["attempt_id"]);
    let denied = admit(&d, &id, "j1", "opencode-a", "req-1", at, 100000, 1000).unwrap();
    assert_eq!(denied["status"], "blocked");
    assert_eq!(denied["reason"], "finishing_reserve");
}

#[test]
fn shared_pool_reservation_blocks_stale_capacity_across_categories() {
    let d = Daemon::start(&[]);
    let first = setup(&d, "Backend pool", 1);
    let second = setup(&d, "QA pool", 1);
    let at = now();
    assert_eq!(
        admit(&d, &first, "j0", "codex-a", "first", at, 60000, 4000).unwrap()["status"],
        "admitted"
    );
    let denied = admit(&d, &second, "j0", "opencode-a", "second", at, 5000, 1000).unwrap();
    assert_eq!(denied["status"], "blocked");
    assert!(denied["reason"] == "finishing_reserve" || denied["reason"] == "shared_pool_headroom");
    assert_eq!(
        d.call("swarm.jobs", json!({"id":second}))["jobs"][0]["status"],
        "ready"
    );
}

#[test]
fn default_worker_ceiling_and_four_per_wave_are_admission_bounds() {
    let d = Daemon::start(&[]);
    let id = setup(&d, "Scale admission", 9);
    let at = now();
    for n in 0..4 {
        assert_eq!(
            admit(
                &d,
                &id,
                &format!("j{n}"),
                "codex-a",
                &format!("r{n}"),
                at,
                1000000,
                100
            )
            .unwrap()["status"],
            "admitted"
        );
    }
    assert_eq!(
        admit(&d, &id, "j4", "codex-a", "r4", at, 1000000, 100).unwrap()["reason"],
        "growth_wave_full"
    );
    for n in 4..8 {
        assert_eq!(
            admit(
                &d,
                &id,
                &format!("j{n}"),
                "codex-a",
                &format!("r{n}"),
                at + 5000,
                1000000,
                100
            )
            .unwrap()["status"],
            "admitted"
        );
    }
    assert_eq!(
        admit(&d, &id, "j8", "codex-a", "r8", at + 10000, 1000000, 100).unwrap()["reason"],
        "worker_limit"
    );
}

#[test]
fn concurrent_requests_cannot_both_claim_one_run_allocation() {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixStream;
    use std::sync::{Arc, Barrier};
    let d = Daemon::start(&[]);
    let id = setup(&d, "Concurrent admission", 2);
    let at = now();
    let path = d.socket();
    let barrier = Arc::new(Barrier::new(3));
    let mut handles = Vec::new();
    for n in 0..2 {
        let path = path.clone();
        let barrier = barrier.clone();
        let run = id.clone();
        handles.push(std::thread::spawn(move || {
            let mut conn=UnixStream::connect(path).unwrap();
            conn.set_read_timeout(Some(std::time::Duration::from_secs(10))).unwrap();
            let params=json!({"run_id":run,"generation":1,"revision":1,"job_id":format!("j{n}"),
                "target_id":if n==0 {"codex-a"} else {"opencode-a"},"request_id":format!("parallel-{n}"),
                "snapshot":snapshot(at,60000),"now_ms":at,"required_capabilities":["code"],
                "estimate_milli":{"points":4000},"purpose":"worker"});
            barrier.wait();
            conn.write_all(format!("{}\n",json!({"id":n,"method":"swarm.admit","params":params})).as_bytes()).unwrap();
            let mut line=String::new();
            BufReader::new(conn).read_line(&mut line).unwrap();
            let response: Value=serde_json::from_str(&line).unwrap();
            response["result"].clone()
        }));
    }
    barrier.wait();
    let results: Vec<Value> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    assert_eq!(
        results.iter().filter(|r| r["status"] == "admitted").count(),
        1
    );
    assert_eq!(
        results.iter().filter(|r| r["status"] == "blocked").count(),
        1
    );
}

#[test]
fn review_backlog_holds_admissions_until_it_drains_below_four() {
    let d = Daemon::start(&[]);
    let run = d.call(
        "swarm.create",
        json!({"category":"Review pressure","objective":"Audit many checks",
        "allowed_targets":["codex-a"],"policy":{"max_workers":32,"max_executing":33}}),
    );
    let id = run["id"].as_str().unwrap();
    let jobs: Vec<_>=(0..10).map(|n|json!({"id":format!("j{n}"),"title":format!("J{n}"),"acceptance":"evidence","deps":[]})).collect();
    d.call(
        "swarm.plan",
        json!({"id":id,"generation":1,"revision":0,"jobs":jobs}),
    );
    let at = now();
    let mut attempts = Vec::new();
    for n in 0..8 {
        let result = admit(
            &d,
            id,
            &format!("j{n}"),
            "codex-a",
            &format!("review-{n}"),
            at + (n / 4) as i64 * 5000,
            1000000,
            100,
        )
        .unwrap();
        assert_eq!(result["status"], "admitted");
        let aid = result["attempt_id"].as_str().unwrap().to_owned();
        let token = result["token"].as_str().unwrap().to_owned();
        let artifact = format!("artifact-{n}");
        d.call(
            "swarm.artifact.put",
            json!({"run_id":id,"job_id":format!("j{n}"),"attempt_id":aid,"token":token,
            "artifact_id":artifact,"source_revision":1,"kind":"finding","content":"checked"}),
        );
        d.call("swarm.report",json!({"run_id":id,"job_id":format!("j{n}"),"attempt_id":aid,"token":token,
            "message_id":format!("result-{n}"),"type":"result","revision":1,"payload":{"artifact_ids":[artifact]}}));
        attempts.push((aid, artifact));
    }
    let held = admit(
        &d,
        id,
        "j8",
        "codex-a",
        "review-8",
        at + 10000,
        1000000,
        100,
    )
    .unwrap();
    assert_eq!(held["reason"], "review_backlog");
    for (n, (aid, artifact)) in attempts.iter().take(5).enumerate() {
        d.call("swarm.decide",json!({"run_id":id,"generation":1,"revision":1,"job_id":format!("j{n}"),"decision":"accept","evidence":[artifact]}));
        d.call("swarm.attempt.confirm_exit",json!({"run_id":id,"generation":1,"revision":1,"job_id":format!("j{n}"),"attempt_id":aid}));
    }
    assert_eq!(
        admit(
            &d,
            id,
            "j8",
            "codex-a",
            "review-8",
            at + 10000,
            1000000,
            100
        )
        .unwrap()["status"],
        "admitted"
    );
}

#[test]
fn explicit_ceiling_admits_thirty_two_fixture_workers_without_hidden_eight_cap() {
    let d = Daemon::start(&[]);
    let run = d.call(
        "swarm.create",
        json!({"category":"Large qualification","objective":"Inspect modules",
        "allowed_targets":["codex-a"],"policy":{"max_workers":32,"max_executing":33}}),
    );
    let id = run["id"].as_str().unwrap();
    let jobs: Vec<_>=(0..33).map(|n|json!({"id":format!("j{n}"),"title":format!("J{n}"),"acceptance":"evidence","deps":[]})).collect();
    d.call(
        "swarm.plan",
        json!({"id":id,"generation":1,"revision":0,"jobs":jobs}),
    );
    let at = now();
    for n in 0..32 {
        let result = admit(
            &d,
            id,
            &format!("j{n}"),
            "codex-a",
            &format!("large-{n}"),
            at + (n / 4) as i64 * 5000,
            1000000,
            100,
        )
        .unwrap();
        assert_eq!(result["status"], "admitted", "worker {n}: {result}");
    }
    assert_eq!(
        admit(
            &d,
            id,
            "j32",
            "codex-a",
            "large-32",
            at + 40000,
            1000000,
            100
        )
        .unwrap()["reason"],
        "worker_limit"
    );
}
