mod common;

use common::*;
use serde_json::json;

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
