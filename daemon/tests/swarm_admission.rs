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
    assert_eq!(d.call("swarm.jobs",json!({"id":id}))["jobs"][0]["deadline_at_ms"],at+900_000);
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
fn ordinary_run_occupies_global_slot_until_confirmed_exit() {
    let d = Daemon::start(&[]);
    let temp = tmp();
    let checkout = repo(&temp.path().join("ordinary"));
    let ordinary = d.generic(&checkout, "worktree", "/bin/sleep", &["2"]);
    assert!(ordinary["launch_error"].is_null());
    let ordinary_id = run_id(&ordinary);
    assert!(["starting", "running"].contains(&d.run(&ordinary_id)["status"].as_str().unwrap()));
    let swarm = d.call("swarm.create", json!({"category":"Shared global slots",
        "objective":"Audit", "allowed_targets":["codex-a"],
        "policy":{"max_executing":2,"max_workers":1}}));
    let id = swarm["id"].as_str().unwrap();
    d.call("swarm.plan",json!({"id":id,"generation":1,"revision":0,"jobs":[
        {"id":"j0","title":"Inspect","acceptance":"evidence","deps":[]}
    ]}));
    let at = now();
    let held = admit(&d,id,"j0","codex-a","ordinary-active",at,100000,100).unwrap();
    assert_eq!(held["status"],"blocked");
    assert_eq!(held["reason"],"global_agent_limit");
    d.wait_done(&ordinary_id,5);
    let admitted = admit(&d,id,"j0","codex-a","ordinary-finished",now(),100000,100).unwrap();
    assert_eq!(admitted["status"],"admitted");
}

#[test]
fn full_director_inbox_holds_new_admissions_but_keeps_terminal_reports() {
    let d = Daemon::start(&[]);
    let id = setup(&d, "Inbox pressure", 2);
    let at = now();
    let first = admit(&d, &id, "j0", "codex-a", "inbox-first", at, 1000000, 100)
        .unwrap();
    assert_eq!(first["status"], "admitted");
    for n in 0..1000 {
        d.call(
            "swarm.report",
            json!({"run_id":id,"job_id":"j0","attempt_id":first["attempt_id"],
                "token":first["token"],"message_id":format!("progress-{n}"),
                "type":"progress","revision":1,"payload":{"n":n}}),
        );
    }
    let held = admit(&d, &id, "j1", "codex-a", "inbox-second", at, 1000000, 100)
        .unwrap();
    assert_eq!(held["status"], "blocked");
    assert_eq!(held["reason"], "director_inbox_full");
    d.call("swarm.report", json!({"run_id":id,"job_id":"j0",
        "attempt_id":first["attempt_id"],"token":first["token"],
        "message_id":"terminal-at-capacity","type":"result","revision":1,
        "payload":{"artifact_ids":[]}}));
    assert_eq!(d.call("swarm.jobs",json!({"id":id}))["jobs"][0]["status"], "submitted");
}

#[test]
fn mutable_resource_conflict_is_rejected_before_reserving_an_attempt() {
    let d=Daemon::start(&[]);
    let first=setup(&d,"DB writer A",1);
    let second=setup(&d,"DB writer B",1);
    let at=now();
    let request=|run:&str,request_id:&str,mode:&str|json!({"run_id":run,"generation":1,
        "revision":1,"job_id":"j0","target_id":"codex-a","request_id":request_id,
        "snapshot":snapshot(at,1000000),"now_ms":at,"required_capabilities":["code"],
        "estimate_milli":{"points":100},"purpose":"worker",
        "resource_claims":[{"resource":"db:shared-test","mode":mode}]
    });
    let admitted=d.call("swarm.admit",request(&first,"writer-a","write"));
    assert_eq!(admitted["status"],"admitted");
    let mut invalid=request(&second,"invalid-claims","write");
    invalid["resource_claims"]=json!([
        {"resource":"db:shared-test","mode":"write"},
        {"resource":"db:shared-test","mode":"read"}
    ]);
    assert!(d.try_call("swarm.admit",invalid).unwrap_err().contains("duplicate resource"));
    let held=d.call("swarm.admit",request(&second,"writer-b","write"));
    assert_eq!(held["reason"],"resource_conflict");
    assert_eq!(d.call("swarm.jobs",json!({"id":second}))["jobs"][0]["status"],"ready");
    let db=rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let attempts:i64=db.query_row("SELECT COUNT(*) FROM swarm_attempts WHERE run_id=?1",[&second],|r|r.get(0)).unwrap();
    assert_eq!(attempts,0);
    assert_eq!(d.call("swarm.admit",request(&second,"reader-b","read"))["reason"],"resource_conflict");
    d.call("swarm.artifact.put",json!({"run_id":first,"job_id":"j0",
        "attempt_id":admitted["attempt_id"],"token":admitted["token"],
        "artifact_id":"writer-a-result","source_revision":1,"kind":"finding","content":"checked"}));
    d.call("swarm.report",json!({"run_id":first,"job_id":"j0",
        "attempt_id":admitted["attempt_id"],"token":admitted["token"],
        "message_id":"writer-a-done","type":"result","revision":1,
        "payload":{"artifact_ids":["writer-a-result"]}}));
    d.call("swarm.decide",json!({"run_id":first,"generation":1,"revision":1,
        "job_id":"j0","decision":"reject","evidence":["writer-a-result"]}));
    d.call("swarm.attempt.confirm_exit",json!({"run_id":first,"generation":1,
        "revision":1,"job_id":"j0","attempt_id":admitted["attempt_id"]}));
    assert_eq!(d.call("swarm.admit",request(&second,"writer-b","write"))["status"],"admitted");
}

#[test]
fn run_percentage_overrides_change_frozen_allocation_and_finishing_reserve() {
    let d=Daemon::start(&[]);
    let run=d.call("swarm.create",json!({"category":"Budget override","objective":"Audit",
        "allowed_targets":["codex-a"],
        "policy":{"run_allocation_percent":20,"finishing_reserve_percent":30}}));
    let id=run["id"].as_str().unwrap();
    d.call("swarm.plan",json!({"id":id,"generation":1,"revision":0,"jobs":[
        {"id":"first","title":"First","acceptance":"evidence","deps":[]},
        {"id":"second","title":"Second","acceptance":"evidence","deps":[]}
    ]}));
    let at=now();
    let request=|job:&str,estimate:i64|json!({"run_id":id,"generation":1,"revision":1,
        "job_id":job,"target_id":"codex-a","request_id":job,"snapshot":snapshot(at,60000),
        "now_ms":at,"required_capabilities":["code"],
        "estimate_milli":{"points":estimate},"purpose":"worker"});
    let first=d.call("swarm.admit",request("first",8000));
    assert_eq!(first["status"],"admitted");
    assert_eq!(first["allocation_milli"],12000);
    let second=d.call("swarm.admit",request("second",500));
    assert_eq!(second["reason"],"finishing_reserve");
    let db=rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let saved:(i64,i64)=db.query_row("SELECT allocation_milli,reserve_milli FROM swarm_allocations WHERE run_id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?))).unwrap();
    assert_eq!(saved,(12000,3600));
}

#[test]
fn default_worker_ceiling_and_four_per_wave_are_admission_bounds() {
    let d = Daemon::start(&[]);
    let id = setup(&d, "Scale admission", 9);
    commit_beneficial_batch(&d, &id, &(0..8).map(|n| format!("j{n}")).collect::<Vec<_>>());
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
    commit_beneficial_batch(&d, id, &(0..10).map(|n| format!("j{n}")).collect::<Vec<_>>());
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
fn already_admitted_results_can_overflow_review_threshold_without_loss() {
    let mut d = Daemon::start(&[]);
    let run = d.call("swarm.create", json!({"category":"Review overflow",
        "objective":"Audit many checks","allowed_targets":["codex-a"],
        "policy":{"max_workers":32,"max_executing":33}}));
    let id = run["id"].as_str().unwrap();
    let jobs: Vec<_> = (0..11).map(|n| json!({"id":format!("j{n}"),
        "title":format!("J{n}"),"acceptance":"evidence","deps":[]})).collect();
    d.call("swarm.plan",json!({"id":id,"generation":1,"revision":0,"jobs":jobs}));
    commit_beneficial_batch(&d, id, &(0..10).map(|n| format!("j{n}")).collect::<Vec<_>>());
    let at = now();
    let mut attempts = Vec::new();
    for n in 0..10 {
        let admitted = admit(&d,id,&format!("j{n}"),"codex-a",
            &format!("overflow-{n}"),at+(n/4) as i64*5000,1000000,100).unwrap();
        assert_eq!(admitted["status"],"admitted","job {n}: {admitted}");
        attempts.push(admitted);
    }
    for (n, admitted) in attempts.iter().enumerate() {
        let artifact = format!("overflow-artifact-{n}");
        d.call("swarm.artifact.put",json!({"run_id":id,"job_id":format!("j{n}"),
            "attempt_id":admitted["attempt_id"],"token":admitted["token"],
            "artifact_id":artifact,"source_revision":1,"kind":"finding","content":"checked"}));
        let report = d.call("swarm.report",json!({"run_id":id,"job_id":format!("j{n}"),
            "attempt_id":admitted["attempt_id"],"token":admitted["token"],
            "message_id":format!("overflow-result-{n}"),"type":"result","revision":1,
            "payload":{"artifact_ids":[artifact]}}));
        assert_eq!(report["duplicate"],false,"result {n}: {report}");
    }
    let jobs = d.call("swarm.jobs",json!({"id":id}))["jobs"].as_array().unwrap().clone();
    assert_eq!(jobs.iter().filter(|job| job["status"]=="submitted").count(),10);
    assert_eq!(admit(&d,id,"j10","codex-a","overflow-10",at+15000,1000000,100)
        .unwrap()["reason"],"review_backlog");
    d.kill9();
    d.spawn();
    let jobs = d.call("swarm.jobs",json!({"id":id}))["jobs"].as_array().unwrap().clone();
    assert_eq!(jobs.iter().filter(|job| job["status"]=="submitted").count(),10);
    let messages = d.call("swarm.messages",json!({"run_id":id,"recipient":"director","limit":100}));
    assert_eq!(messages["messages"].as_array().unwrap().iter()
        .filter(|message| message["type"]=="result").count(),10);
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
    commit_beneficial_batch(&d, id, &(0..32).map(|n| format!("j{n}")).collect::<Vec<_>>());
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

#[test]
fn planned_write_claim_cannot_be_omitted_at_admission() {
    let d = Daemon::start(&[]);
    let planned = |category: &str| {
        let run = d.call("swarm.create", json!({"category":category,"objective":"Audit",
            "allowed_targets":["codex-a"]}));
        let id = run["id"].as_str().unwrap().to_string();
        d.call("swarm.plan", json!({"id":id,"generation":1,"revision":0,
            "jobs":[{"id":"j0","title":"Inspect","acceptance":"evidence","deps":[],
                "resource_claims":[{"resource":"db:tenant-fixture","mode":"write"}]}]}));
        id
    };
    let first = planned("Plan claims A");
    let second = planned("Plan claims B");
    let at = now();
    let downgraded = json!({"run_id":first,"generation":1,"revision":1,
        "job_id":"j0","target_id":"codex-a","request_id":"downgraded",
        "snapshot":snapshot(at,1_000_000),"now_ms":at,
        "required_capabilities":["code"],"estimate_milli":{"points":100},
        "purpose":"worker","resource_claims":[{"resource":"db:tenant-fixture","mode":"read"}]});
    assert!(d.try_call("swarm.admit",downgraded).unwrap_err().contains("cannot change a planned resource claim"));
    assert_eq!(admit(&d,&first,"j0","codex-a","first",at,1_000_000,100)
        .unwrap()["status"],"admitted");
    let held = admit(&d,&second,"j0","codex-a","second",at,1_000_000,100).unwrap();
    assert_eq!(held["status"],"blocked");
    assert_eq!(held["reason"],"resource_conflict");
}

#[test]
fn hundred_jobs_cycle_through_thirty_two_slots_and_accept_once() {
    let d = Daemon::start(&[]);
    let run = d.call("swarm.create", json!({"category":"Hundred job qualification",
        "objective":"Audit one hundred independent paths","allowed_targets":["codex-a"],
        "policy":{"max_workers":32,"max_executing":33}}));
    let id = run["id"].as_str().unwrap();
    let jobs: Vec<_> = (0..100).map(|n| json!({"id":format!("j{n:03}"),
        "title":format!("Inspect path {n}"),"acceptance":"evidence","deps":[]})).collect();
    d.call("swarm.plan",json!({"id":id,"generation":1,"revision":0,"jobs":jobs}));
    let at=now();
    for start in (0..100).step_by(32) {
        let end=(start+32).min(100);
        let batch:Vec<String>=(start..end).map(|n|format!("j{n:03}")).collect();
        let benefit=commit_beneficial_batch(&d,id,&batch);
        assert_eq!(benefit["max_parallel_workers"],(end-start) as i64);
        let mut admitted=Vec::new();
        for n in start..end {
            let job=format!("j{n:03}");
            let result=admit(&d,id,&job,"codex-a",&format!("hundred-{n}"),
                at+(n/4) as i64*5000,1_000_000,100).unwrap();
            assert_eq!(result["status"],"admitted","{job}: {result}");
            admitted.push((job,result));
        }
        if start==0 {
            let held=admit(&d,id,"j032","codex-a","overflow-32",
                at+40_000,1_000_000,100).unwrap();
            assert_eq!(held["reason"],"worker_limit");
            let db=rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
            let active:i64=db.query_row("SELECT COUNT(*) FROM swarm_attempts WHERE run_id=?1 AND status='registered'",[id],|r|r.get(0)).unwrap();
            assert_eq!(active,32);
            assert_eq!(d.call("swarm.get",json!({"id":id}))["status"],"running");
        }
        for (job,result) in admitted {
            let artifact=format!("checked-{job}");
            d.call("swarm.artifact.put",json!({"run_id":id,"job_id":job,
                "attempt_id":result["attempt_id"],"token":result["token"],
                "artifact_id":artifact,"source_revision":1,"kind":"finding","content":"checked"}));
            d.call("swarm.report",json!({"run_id":id,"job_id":job,
                "attempt_id":result["attempt_id"],"token":result["token"],
                "message_id":format!("result-{job}"),"type":"result","revision":1,
                "payload":{"artifact_ids":[artifact]}}));
            d.call("swarm.decide",json!({"run_id":id,"generation":1,"revision":1,
                "job_id":job,"decision":"accept","evidence":[artifact]}));
            d.call("swarm.attempt.confirm_exit",json!({"run_id":id,"generation":1,
                "revision":1,"job_id":job,"attempt_id":result["attempt_id"]}));
        }
    }
    let db=rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let counts:(i64,i64,i64)=db.query_row("SELECT COUNT(*),
        SUM(CASE WHEN status='accepted' AND attempt_count=1 THEN 1 ELSE 0 END),
        COUNT(DISTINCT id) FROM swarm_jobs WHERE run_id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap();
    assert_eq!(counts,(100,100,100));
    let accepted:i64=db.query_row("SELECT COUNT(*) FROM swarm_decisions WHERE run_id=?1 AND decision='accept'",[id],|r|r.get(0)).unwrap();
    assert_eq!(accepted,100);
    assert_eq!(admit(&d,id,"j000","codex-a","hundred-0",at,1_000_000,100)
        .unwrap()["status"],"already_admitted");
}

#[test]
fn quota_headroom_explains_smaller_pool_than_worker_ceiling() {
    let d=Daemon::start(&[]);
    let run=d.call("swarm.create",json!({"category":"Quota-constrained pool",
        "objective":"Inspect three paths","allowed_targets":["codex-a"],
        "policy":{"max_workers":32,"max_executing":33}}));
    let id=run["id"].as_str().unwrap();
    let jobs:Vec<_>=(0..3).map(|n|json!({"id":format!("j{n}"),
        "title":format!("Inspect {n}"),"acceptance":"evidence","deps":[]})).collect();
    d.call("swarm.plan",json!({"id":id,"generation":1,"revision":0,"jobs":jobs}));
    commit_beneficial_batch(&d,id,&["j0".into(),"j1".into(),"j2".into()]);
    let at=now();
    for n in 0..2 {
        assert_eq!(admit(&d,id,&format!("j{n}"),"codex-a",
            &format!("small-{n}"),at,3000,100).unwrap()["status"],"admitted");
    }
    let third=admit(&d,id,"j2","codex-a","small-2",at,3000,100).unwrap();
    assert_eq!(third["status"],"blocked");
    assert_eq!(third["reason"],"finishing_reserve");
}
