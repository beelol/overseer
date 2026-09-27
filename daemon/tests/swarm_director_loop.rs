mod common;

use common::*;
use serde_json::json;
use std::time::{Duration, Instant};

#[test]
fn supervised_director_plans_routes_reviews_and_completes_two_workers() {
    let d = Daemon::start(&[]);
    let temp = tmp();
    let checkout = repo(&temp.path().join("director-loop-source"));
    let fixture = repo_root().join("fixtures/swarm/director-loop-v1");
    let discovery_routed = temp.path().join("discovery-routed");
    let created = d.call("swarm.create",json!({"category":"Director loop fixture",
        "objective":"Audit two local routes; coordinate the shared lookup before final results",
        "allowed_targets":["fixture-local"],"policy":{"max_workers":2,"deadline_ms":60000}}));
    let run = created["id"].as_str().unwrap();
    let launched = d.call("swarm.director.launch",json!({"run_id":run,
        "generation":1,"repo":checkout,"program":"/usr/bin/python3",
        "args":[fixture.join("director.py"),checkout,discovery_routed],
        "prompt":"Plan, coordinate, review and finish the two route audits",
        "title":"Fixture director loop"}));
    assert_eq!(launched["status"],"launched","{launched}");
    let process = launched["overseer_run_id"].as_str().unwrap();
    let finished = d.wait_done(process,25);
    assert_eq!(finished["status"],"completed","{finished}");
    let state = d.call("swarm.get",json!({"id":run}));
    assert_eq!(state["status"],"completed","{state}");
    assert!(discovery_routed.exists(),"J2's discovery never reached J4");
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let count = |table: &str| -> i64 {
        db.query_row(&format!("SELECT COUNT(*) FROM {table} WHERE run_id=?1"),
            [run],|row|row.get(0)).unwrap()
    };
    assert_eq!(count("swarm_worker_launches"),2);
    assert_eq!(count("swarm_decisions"),2);
    assert_eq!(count("swarm_completions"),1);
    let (discovery,result): (i64,i64) = (
        db.query_row("SELECT seq FROM swarm_messages WHERE run_id=?1 AND message_id='D1'",
            [run],|r|r.get(0)).unwrap(),
        db.query_row("SELECT seq FROM swarm_messages WHERE run_id=?1 AND message_id='J2-result'",
            [run],|r|r.get(0)).unwrap(),
    );
    assert!(discovery<result,"J2 must report D1 before completing");
    let phase: String = db.query_row("SELECT phase FROM swarm_messages WHERE run_id=?1 AND message_id='D1-to-J4'",
        [run],|r|r.get(0)).unwrap();
    assert_eq!(phase,"applied");
}

#[test]
fn one_slot_director_executes_and_accepts_a_job_without_spawning_a_worker() {
    let d = Daemon::start(&[]);
    d.call("agents.limit.set",json!({"max_active":1}));
    let temp = tmp();
    let checkout = repo(&temp.path().join("director-serial-source"));
    let fixture = repo_root().join("fixtures/swarm/director-loop-v1/serial.py");
    let created = d.call("swarm.create",json!({"category":"Serial director fixture",
        "objective":"Inspect a.txt with one available agent", "allowed_targets":["fixture-local"]}));
    let run = created["id"].as_str().unwrap();
    let launched = d.call("swarm.director.launch",json!({"run_id":run,
        "generation":1,"repo":checkout,"program":"/usr/bin/python3",
        "args":[fixture,checkout],"prompt":"Inspect the file and record evidence",
        "title":"Serial fixture director"}));
    assert_eq!(launched["status"],"launched","{launched}");
    let process = launched["overseer_run_id"].as_str().unwrap();
    let finished = d.wait_done(process,25);
    assert_eq!(finished["status"],"completed","{finished}; output={}",
        d.call("run.raw_output",json!({"run_id":process})));
    let state = d.call("swarm.get",json!({"id":run}));
    assert_eq!(state["status"],"completed","{state}");
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let (executor,attempts,worker_launches): (String,i64,i64) = (
        db.query_row("SELECT executor FROM swarm_attempts WHERE run_id=?1",[run],|r|r.get(0)).unwrap(),
        db.query_row("SELECT COUNT(*) FROM swarm_attempts WHERE run_id=?1",[run],|r|r.get(0)).unwrap(),
        db.query_row("SELECT COUNT(*) FROM swarm_worker_launches WHERE run_id=?1",[run],|r|r.get(0)).unwrap(),
    );
    assert_eq!(executor,"director");
    assert_eq!(attempts,1);
    assert_eq!(worker_launches,0);
    assert_eq!(d.call("agents.limit.get",json!({}))["active"],0);
}

#[test]
fn stop_closes_unreviewed_director_self_attempt_only_after_process_exit() {
    let d = Daemon::start(&[]);
    d.call("agents.limit.set",json!({"max_active":1}));
    let temp = tmp();
    let checkout = repo(&temp.path().join("director-self-stop-source"));
    let ready = temp.path().join("self-attempt-ready");
    let fixture = repo_root().join("fixtures/swarm/director-loop-v1/serial.py");
    let created = d.call("swarm.create",json!({"category":"Stop self attempt",
        "objective":"Inspect a.txt", "allowed_targets":["fixture-local"]}));
    let run = created["id"].as_str().unwrap();
    let launched = d.call("swarm.director.launch",json!({"run_id":run,
        "generation":1,"repo":checkout,"program":"/usr/bin/python3",
        "args":[fixture,checkout,ready],"prompt":"Inspect the file",
        "title":"Self attempt to stop"}));
    let process = launched["overseer_run_id"].as_str().unwrap();
    let deadline = Instant::now()+Duration::from_secs(8);
    while !ready.exists() {
        assert!(Instant::now()<deadline,"director did not admit its self attempt");
        std::thread::sleep(Duration::from_millis(20));
    }
    let attempt = std::fs::read_to_string(&ready).unwrap();
    assert_eq!(d.call("swarm.stop",json!({"run_id":run}))["status"],"stopping");
    d.wait_done(process,8);
    let deadline = Instant::now()+Duration::from_secs(5);
    while d.call("swarm.get",json!({"id":run}))["status"] != "stopped" {
        assert!(Instant::now()<deadline,"confirmed exit left self attempt registered");
        std::thread::sleep(Duration::from_millis(20));
    }
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let (attempt_state,executor): (String,String) = db.query_row(
        "SELECT status,executor FROM swarm_attempts WHERE id=?1",[&attempt],
        |r| Ok((r.get(0)?,r.get(1)?))).unwrap();
    assert_eq!(attempt_state,"finished");
    assert_eq!(executor,"director");
    let reservation: String = db.query_row(
        "SELECT status FROM swarm_reservations WHERE attempt_id=?1",[&attempt],
        |r|r.get(0)).unwrap();
    assert_eq!(reservation,"uncertain");
    let job: String = db.query_row("SELECT status FROM swarm_jobs WHERE run_id=?1 AND id='inspect'",
        [run],|r|r.get(0)).unwrap();
    assert_eq!(job,"cancelled");
    let decisions: i64 = db.query_row("SELECT COUNT(*) FROM swarm_decisions WHERE run_id=?1",
        [run],|r|r.get(0)).unwrap();
    assert_eq!(decisions,0,"process exit is not an acceptance decision");
}

#[test]
fn confirmed_director_death_requeues_unsubmitted_self_job_for_replacement() {
    let d = Daemon::start(&[]);
    d.call("agents.limit.set",json!({"max_active":1}));
    let temp = tmp();
    let checkout = repo(&temp.path().join("director-self-crash-source"));
    let ready = temp.path().join("self-attempt-ready");
    let fixture = repo_root().join("fixtures/swarm/director-loop-v1/serial.py");
    let created = d.call("swarm.create",json!({"category":"Recover self attempt",
        "objective":"Inspect a.txt", "allowed_targets":["fixture-local"]}));
    let run = created["id"].as_str().unwrap();
    let launched = d.call("swarm.director.launch",json!({"run_id":run,
        "generation":1,"repo":checkout,"program":"/usr/bin/python3",
        "args":[fixture,checkout,ready],"prompt":"Inspect the file",
        "title":"Self attempt before crash"}));
    let process = launched["overseer_run_id"].as_str().unwrap();
    let deadline = Instant::now()+Duration::from_secs(8);
    while !ready.exists() {
        assert!(Instant::now()<deadline,"director did not admit its self attempt");
        std::thread::sleep(Duration::from_millis(20));
    }
    let attempt = std::fs::read_to_string(&ready).unwrap();
    d.call("run.interrupt",json!({"run_id":process}));
    d.wait_done(process,8);
    let recovered = d.call("swarm.director.recover",json!({"run_id":run,
        "generation":1,"revision":1,"termination":"confirmed_dead"}));
    assert_eq!(recovered["generation"],2);
    assert_eq!(recovered["replacement_pending"],true);
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let attempt_status: String = db.query_row("SELECT status FROM swarm_attempts WHERE id=?1",
        [&attempt],|r|r.get(0)).unwrap();
    assert_eq!(attempt_status,"finished","dead director must not hold a job forever");
    let reservation: String = db.query_row(
        "SELECT status FROM swarm_reservations WHERE attempt_id=?1",[&attempt],
        |r|r.get(0)).unwrap();
    assert_eq!(reservation,"uncertain","exit does not measure account usage");
    let (job_status,count): (String,i64) = db.query_row(
        "SELECT status,attempt_count FROM swarm_jobs WHERE run_id=?1 AND id='inspect'",
        [run],|r|Ok((r.get(0)?,r.get(1)?))).unwrap();
    assert_eq!(job_status,"ready");
    assert_eq!(count,1,"replacement must consume the second attempt, not reset the cap");
    let (kind,payload): (String,String) = db.query_row(
        "SELECT kind,payload FROM swarm_messages WHERE run_id=?1 AND message_id=?2",
        rusqlite::params![run,format!("terminal-self-{attempt}")],
        |r|Ok((r.get(0)?,r.get(1)?))).unwrap();
    assert_eq!(kind,"terminal");
    assert_eq!(serde_json::from_str::<serde_json::Value>(&payload).unwrap()["result_submitted"],false);
    assert_eq!(d.call("agents.limit.get",json!({}))["active"],1,
        "replacement-pending category still owns its director slot");
}

#[test]
fn replacement_reviews_submitted_self_result_without_reexecuting_job() {
    let d = Daemon::start(&[]);
    d.call("agents.limit.set",json!({"max_active":1}));
    let temp = tmp();
    let checkout = repo(&temp.path().join("director-self-submitted-source"));
    let ready = temp.path().join("submitted-self-attempt-ready");
    let fixture = repo_root().join("fixtures/swarm/director-loop-v1");
    let created = d.call("swarm.create",json!({"category":"Recover submitted self result",
        "objective":"Inspect a.txt", "allowed_targets":["fixture-local"]}));
    let run = created["id"].as_str().unwrap();
    let launched = d.call("swarm.director.launch",json!({"run_id":run,
        "generation":1,"repo":checkout,"program":"/usr/bin/python3",
        "args":[fixture.join("serial.py"),checkout,ready,"after_report"],
        "prompt":"Inspect the file","title":"Submitted self result"}));
    let process = launched["overseer_run_id"].as_str().unwrap();
    let deadline = Instant::now()+Duration::from_secs(8);
    while !ready.exists() {
        assert!(Instant::now()<deadline,"director did not submit its result");
        std::thread::sleep(Duration::from_millis(20));
    }
    let attempt = std::fs::read_to_string(&ready).unwrap();
    d.call("run.interrupt",json!({"run_id":process}));
    d.wait_done(process,8);
    d.call("swarm.director.recover",json!({"run_id":run,
        "generation":1,"revision":1,"termination":"confirmed_dead"}));
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let (attempt_status,job_status): (String,String) = db.query_row(
        "SELECT a.status,j.status FROM swarm_attempts a JOIN swarm_jobs j
         ON j.run_id=a.run_id AND j.id=a.job_id WHERE a.id=?1",[&attempt],
        |r|Ok((r.get(0)?,r.get(1)?))).unwrap();
    assert_eq!((attempt_status.as_str(),job_status.as_str()),("registered","submitted"),
        "submitted evidence must wait for replacement review, not be retried");
    let payload: String = db.query_row(
        "SELECT payload FROM swarm_messages WHERE run_id=?1 AND message_id=?2",
        rusqlite::params![run,format!("terminal-self-{attempt}")],
        |r|r.get(0)).unwrap();
    assert_eq!(serde_json::from_str::<serde_json::Value>(&payload).unwrap()["result_submitted"],true);
    let replacement = d.call("swarm.director.launch",json!({"run_id":run,
        "generation":2,"repo":checkout,"program":"/usr/bin/python3",
        "args":[fixture.join("serial_review.py"),attempt],
        "prompt":"Review the preserved result","title":"Replacement reviewer"}));
    let replacement_run = replacement["overseer_run_id"].as_str().unwrap();
    let finished = d.wait_done(replacement_run,8);
    assert_eq!(finished["status"],"completed","{finished}; output={}",
        d.call("run.raw_output",json!({"run_id":replacement_run})));
    let (attempt_status,job_status,attempt_count): (String,String,i64) = db.query_row(
        "SELECT a.status,j.status,j.attempt_count FROM swarm_attempts a JOIN swarm_jobs j
         ON j.run_id=a.run_id AND j.id=a.job_id WHERE a.id=?1",[&attempt],
        |r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap();
    assert_eq!((attempt_status.as_str(),job_status.as_str(),attempt_count),
        ("finished","accepted",1));
}

#[test]
fn director_death_with_unknown_effect_blocks_self_job_retry() {
    let d = Daemon::start(&[]);
    d.call("agents.limit.set",json!({"max_active":1}));
    let temp = tmp();
    let checkout = repo(&temp.path().join("director-self-effect-source"));
    let ready = temp.path().join("self-effect-ready");
    let fixture = repo_root().join("fixtures/swarm/director-loop-v1/serial.py");
    let created = d.call("swarm.create",json!({"category":"Recover uncertain effect",
        "objective":"Inspect a.txt", "allowed_targets":["fixture-local"]}));
    let run = created["id"].as_str().unwrap();
    let launched = d.call("swarm.director.launch",json!({"run_id":run,
        "generation":1,"repo":checkout,"program":"/usr/bin/python3",
        "args":[fixture,checkout,ready,"after_effect_begin"],
        "prompt":"Inspect the file","title":"Self effect before crash"}));
    let process = launched["overseer_run_id"].as_str().unwrap();
    let deadline = Instant::now()+Duration::from_secs(8);
    while !ready.exists() {
        assert!(Instant::now()<deadline,"director did not journal its effect");
        std::thread::sleep(Duration::from_millis(20));
    }
    let attempt = std::fs::read_to_string(&ready).unwrap();
    d.call("run.interrupt",json!({"run_id":process}));
    d.wait_done(process,8);
    d.call("swarm.director.recover",json!({"run_id":run,
        "generation":1,"revision":1,"termination":"confirmed_dead"}));
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let (attempt_status,job_status): (String,String) = db.query_row(
        "SELECT a.status,j.status FROM swarm_attempts a JOIN swarm_jobs j
         ON j.run_id=a.run_id AND j.id=a.job_id WHERE a.id=?1",[&attempt],
        |r|Ok((r.get(0)?,r.get(1)?))).unwrap();
    assert_eq!((attempt_status.as_str(),job_status.as_str()),("finished","blocked"));
    let effect: String = db.query_row(
        "SELECT outcome FROM swarm_effects WHERE run_id=?1 AND effect_id='inspect-effect'",
        [run],|r|r.get(0)).unwrap();
    assert_eq!(effect,"unknown");
    let reservation: String = db.query_row(
        "SELECT status FROM swarm_reservations WHERE attempt_id=?1",[&attempt],
        |r|r.get(0)).unwrap();
    assert_eq!(reservation,"uncertain");
}

#[test]
fn director_death_after_scope_narrowing_supersedes_self_job() {
    let d = Daemon::start(&[]);
    d.call("agents.limit.set",json!({"max_active":1}));
    let temp = tmp();
    let checkout = repo(&temp.path().join("director-self-scope-source"));
    let ready = temp.path().join("self-scope-ready");
    let fixture = repo_root().join("fixtures/swarm/director-loop-v1/serial.py");
    let created = d.call("swarm.create",json!({"category":"Recover narrowed scope",
        "objective":"Inspect a.txt", "allowed_targets":["fixture-local"]}));
    let run = created["id"].as_str().unwrap();
    let launched = d.call("swarm.director.launch",json!({"run_id":run,
        "generation":1,"repo":checkout,"program":"/usr/bin/python3",
        "args":[fixture,checkout,ready,"after_scope_narrowed"],
        "prompt":"Inspect the file","title":"Self job before scope revision"}));
    let process = launched["overseer_run_id"].as_str().unwrap();
    let deadline = Instant::now()+Duration::from_secs(8);
    while !ready.exists() {
        assert!(Instant::now()<deadline,"director did not narrow the plan");
        std::thread::sleep(Duration::from_millis(20));
    }
    let attempt = std::fs::read_to_string(&ready).unwrap();
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let before: String = db.query_row("SELECT status FROM swarm_jobs WHERE run_id=?1 AND id='inspect'",
        [run],|r|r.get(0)).unwrap();
    assert_eq!(before,"cancel_requested");
    d.call("run.interrupt",json!({"run_id":process}));
    d.wait_done(process,8);
    d.call("swarm.director.recover",json!({"run_id":run,
        "generation":1,"revision":2,"termination":"confirmed_dead"}));
    let (after,reason): (String,Option<String>) = db.query_row(
        "SELECT status,stop_reason FROM swarm_jobs WHERE run_id=?1 AND id='inspect'",
        [run],|r|Ok((r.get(0)?,r.get(1)?))).unwrap();
    assert_eq!((after.as_str(),reason.as_deref()),("superseded",Some("scope_narrowed")));
    let status: String = db.query_row("SELECT status FROM swarm_attempts WHERE id=?1",
        [&attempt],|r|r.get(0)).unwrap();
    assert_eq!(status,"finished");
    let claims: i64 = db.query_row("SELECT COUNT(*) FROM swarm_claims WHERE run_id=?1 AND job_id='inspect' AND status='active'",
        [run],|r|r.get(0)).unwrap();
    assert_eq!(claims,0);
}

#[test]
fn director_self_job_deadline_retries_interrupt_after_daemon_restart() {
    let mut d = Daemon::start(&[]);
    d.call("agents.limit.set",json!({"max_active":1}));
    let temp = tmp();
    let checkout = repo(&temp.path().join("director-self-deadline-source"));
    let ready = temp.path().join("self-deadline-ready");
    let fixture = repo_root().join("fixtures/swarm/director-loop-v1/serial.py");
    let created = d.call("swarm.create",json!({"category":"Recover expired self job",
        "objective":"Inspect a.txt", "allowed_targets":["fixture-local"]}));
    let run = created["id"].as_str().unwrap();
    let launched = d.call("swarm.director.launch",json!({"run_id":run,
        "generation":1,"repo":checkout,"program":"/usr/bin/python3",
        "args":[fixture,checkout,ready],"prompt":"Inspect the file",
        "title":"Self job before deadline"}));
    let process = launched["overseer_run_id"].as_str().unwrap();
    let deadline = Instant::now()+Duration::from_secs(8);
    while !ready.exists() {
        assert!(Instant::now()<deadline,"director did not admit its self job");
        std::thread::sleep(Duration::from_millis(20));
    }
    let attempt = std::fs::read_to_string(&ready).unwrap();
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .unwrap().as_millis() as i64;
    db.execute("UPDATE swarm_jobs SET deadline_at_ms=?2 WHERE run_id=?1 AND id='inspect'",
        rusqlite::params![run,now-1]).unwrap();
    let persisted = d.call("swarm.job.deadline.persist_due",json!({"now_ms":now}));
    let pending = persisted["interrupt_pending"].as_array().unwrap();
    if !pending.iter().any(|id|id == process) {
        d.call("run.interrupt",json!({"run_id":process}));
        d.wait_done(process,8);
    }
    assert!(pending.iter().any(|id|id == process),
        "deadline transition must retain the director process to interrupt: {persisted}");
    d.kill9();
    d.spawn();
    let finished = d.wait_done(process,8);
    assert_eq!(finished["status"],"interrupted","{finished}");
    d.call("swarm.director.recover",json!({"run_id":run,
        "generation":1,"revision":1,"termination":"confirmed_dead"}));
    let (job_status,reason): (String,Option<String>) = db.query_row(
        "SELECT status,stop_reason FROM swarm_jobs WHERE run_id=?1 AND id='inspect'",
        [run],|r|Ok((r.get(0)?,r.get(1)?))).unwrap();
    assert_eq!((job_status.as_str(),reason.as_deref()),("failed",Some("job_deadline")));
    let attempt_status: String = db.query_row("SELECT status FROM swarm_attempts WHERE id=?1",
        [&attempt],|r|r.get(0)).unwrap();
    assert_eq!(attempt_status,"finished");
}
