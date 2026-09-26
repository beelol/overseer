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
        "targets":[{"id":"fixture","account_id":"account","pool_ids":["pool"],
            "capabilities":["code"],"health":"up","auth":"ok"}],
        "pools":[{"id":"pool","windows":[{"id":"week","unit":"points",
            "remaining_milli":1000,"protected_milli":0,"reserved_milli":0,
            "confidence":"exact","expires_ms":at+60000}]}]})
}

fn run(d: &Daemon, category: &str) -> String {
    let created = d.call(
        "swarm.create",
        json!({"category":category,
        "objective":"Audit backend","allowed_targets":["fixture"]}),
    );
    let id = created["id"].as_str().unwrap().to_string();
    d.call(
        "swarm.plan",
        json!({"id":id,"generation":1,"revision":0,
        "jobs":[{"id":"a","title":"A","acceptance":"evidence","deps":[]},
                {"id":"b","title":"B","acceptance":"evidence","deps":[]},
                {"id":"c","title":"C","acceptance":"evidence","deps":[]},
                {"id":"d","title":"D","acceptance":"evidence","deps":[]}]}),
    );
    id
}

fn admit(d: &Daemon, run_id: &str, job: &str, at: i64) -> Value {
    d.call(
        "swarm.admit",
        json!({"run_id":run_id,"generation":1,"revision":1,
        "job_id":job,"target_id":"fixture","request_id":format!("{run_id}-{job}"),
        "snapshot":snapshot(at),"now_ms":at,"required_capabilities":["code"],
        "estimate_milli":{"points":10},"finishing_estimate_milli":{"points":10},
        "purpose":"worker"}),
    )
}

fn cost(elapsed: i64, points: i64) -> Value {
    json!({"elapsed_ms":elapsed,"usage_milli":{"points":points}})
}

fn pair() -> Value {
    json!({
        "independent":true,"max_workers":8,
        "allocation_milli":{"points":40},"finishing_reserve_milli":{"points":10},
        "serial":{
            "planning":cost(10,1),"context":cost(10,1),
            "integration":cost(10,2),"review":cost(10,2),"retries":cost(0,1),
            "workers":[{"id":"a","elapsed_ms":100,"usage_milli":{"points":10}},
                       {"id":"b","elapsed_ms":100,"usage_milli":{"points":10}}]
        },
        "parallel":{
            "planning":cost(10,2),"context":cost(50,3),
            "integration":cost(30,4),"review":cost(20,4),"retries":cost(0,2),
            "workers":[{"id":"a","elapsed_ms":100,"usage_milli":{"points":10}},
                       {"id":"b","elapsed_ms":100,"usage_milli":{"points":10}}]
        }
    })
}

#[test]
fn paired_estimates_admit_only_affordable_time_benefit() {
    let d = Daemon::start(&[]);
    let input = pair();
    let benefit = d.call("swarm.benefit.preview", input.clone());
    assert_eq!(benefit["decision"], "parallel");
    assert_eq!(benefit["reason"], "beneficial");
    assert_eq!(benefit["serial"]["elapsed_ms"], 240);
    assert_eq!(benefit["parallel"]["elapsed_ms"], 210);
    assert_eq!(benefit["expected_time_benefit_ms"], 30);
    assert_eq!(benefit["parallel"]["usage_milli"]["points"], 35);
    assert_eq!(benefit["parallel"]["finishing_usage_milli"]["points"], 10);
    assert_eq!(benefit["authoritative_admission"], false);

    let mut slow = input.clone();
    slow["parallel"]["context"]["elapsed_ms"] = json!(90);
    assert_eq!(
        d.call("swarm.benefit.preview", slow)["reason"],
        "no_time_benefit"
    );
    let mut tie = input.clone();
    tie["parallel"]["context"]["elapsed_ms"] = json!(80);
    assert_eq!(d.call("swarm.benefit.preview", tie)["decision"], "serial");

    let mut no_finish = input.clone();
    no_finish["finishing_reserve_milli"]["points"] = json!(9);
    assert_eq!(
        d.call("swarm.benefit.preview", no_finish)["reason"],
        "finishing_unaffordable"
    );

    let mut over_budget = input.clone();
    over_budget["allocation_milli"]["points"] = json!(34);
    assert_eq!(
        d.call("swarm.benefit.preview", over_budget)["reason"],
        "allocation_exceeded"
    );

    let mut dependent = input.clone();
    dependent["independent"] = json!(false);
    assert_eq!(
        d.call("swarm.benefit.preview", dependent)["reason"],
        "dependent_jobs"
    );

    let mut neither = input;
    neither["independent"] = json!(false);
    neither["allocation_milli"]["points"] = json!(26);
    assert_eq!(
        d.call("swarm.benefit.preview", neither)["decision"],
        "blocked"
    );
}

#[test]
fn uncalibrated_or_mismatched_paired_estimates_are_rejected() {
    let d = Daemon::start(&[]);
    let mut missing = pair();
    missing["parallel"]["review"]["usage_milli"] = json!({});
    assert!(d
        .try_call("swarm.benefit.preview", missing)
        .unwrap_err()
        .contains("incomplete"));

    let mut changed_job = pair();
    changed_job["parallel"]["workers"][1]["id"] = json!("c");
    assert!(d
        .try_call("swarm.benefit.preview", changed_job)
        .unwrap_err()
        .contains("matching jobs"));

    let mut zero = pair();
    zero["parallel"]["workers"][0]["usage_milli"]["points"] = json!(0);
    assert!(d
        .try_call("swarm.benefit.preview", zero)
        .unwrap_err()
        .contains("uncalibrated"));
}

#[test]
fn committed_benefit_bounds_admission_and_survives_restart() {
    let mut d = Daemon::start(&[]);
    let unproven = run(&d, "Unproven fanout");
    let at = now();
    assert_eq!(admit(&d, &unproven, "a", at)["status"], "admitted");
    assert_eq!(admit(&d, &unproven, "b", at)["reason"], "benefit_unproven");

    let id = run(&d, "Proven fanout");
    let committed = d.call(
        "swarm.benefit.commit",
        json!({"run_id":id,
        "generation":1,"revision":1,"estimate":pair()}),
    );
    assert_eq!(committed["decision"], "parallel");
    assert_eq!(committed["max_parallel_workers"], 2);
    d.kill9();
    d.spawn();
    assert_eq!(
        d.call("swarm.get", json!({"id":id}))["benefit"]["decision"],
        "parallel"
    );
    let first = admit(&d, &id, "a", at);
    let second = admit(&d, &id, "b", at);
    assert_eq!(first["status"], "admitted");
    assert_eq!(second["status"], "admitted");
    assert_eq!(
        admit(&d, &id, "c", at)["reason"],
        "benefit_job_not_estimated"
    );
    let replay = d.call(
        "swarm.benefit.commit",
        json!({"run_id":id,
        "generation":1,"revision":1,"estimate":pair()}),
    );
    assert_eq!(replay["replay"], true);
    let mut changed = pair();
    changed["parallel"]["context"]["elapsed_ms"] = json!(80);
    assert!(d
        .try_call(
            "swarm.benefit.commit",
            json!({"run_id":id,
        "generation":1,"revision":1,"estimate":changed})
        )
        .unwrap_err()
        .contains("before admitting a batch"));
    for (job, admitted) in [("a", first), ("b", second)] {
        let artifact = format!("evidence-{job}");
        d.call(
            "swarm.artifact.put",
            json!({"run_id":id,"job_id":job,
            "attempt_id":admitted["attempt_id"],"token":admitted["token"],
            "artifact_id":artifact,"source_revision":1,"kind":"finding","content":"checked"}),
        );
        d.call(
            "swarm.report",
            json!({"run_id":id,"job_id":job,
            "attempt_id":admitted["attempt_id"],"token":admitted["token"],
            "message_id":format!("result-{job}"),"type":"result","revision":1,
            "payload":{"artifact_ids":[artifact]}}),
        );
        d.call(
            "swarm.decide",
            json!({"run_id":id,"generation":1,"revision":1,
            "job_id":job,"decision":"accept","evidence":[artifact]}),
        );
        d.call(
            "swarm.attempt.confirm_exit",
            json!({"run_id":id,"generation":1,
            "revision":1,"job_id":job,"attempt_id":admitted["attempt_id"]}),
        );
    }
    let next_wave = commit_beneficial_batch(&d, &id, &["c".into(), "d".into()]);
    assert_eq!(next_wave["wave"], 2);
    assert_eq!(d.call("swarm.get", json!({"id":id}))["benefit"]["wave"], 2);
    assert_eq!(admit(&d, &id, "c", at + 5000)["status"], "admitted");
    assert_eq!(admit(&d, &id, "d", at + 5000)["status"], "admitted");
}

#[test]
fn committed_serial_choice_holds_second_worker() {
    let d = Daemon::start(&[]);
    let id = run(&d, "No parallel gain");
    let mut estimate = pair();
    estimate["parallel"]["context"]["elapsed_ms"] = json!(80);
    let committed = d.call(
        "swarm.benefit.commit",
        json!({"run_id":id,
        "generation":1,"revision":1,"estimate":estimate}),
    );
    assert_eq!(committed["decision"], "serial");
    let at = now();
    assert_eq!(admit(&d, &id, "a", at)["status"], "admitted");
    assert_eq!(admit(&d, &id, "b", at)["reason"], "benefit_serial");
}

#[test]
fn planned_exclusive_claim_overrides_director_parallel_estimate() {
    let mut d = Daemon::start(&[]);
    let created = d.call(
        "swarm.create",
        json!({"category":"Shared fixture database","objective":"Audit two routes",
            "allowed_targets":["fixture"]}),
    );
    let id = created["id"].as_str().unwrap();
    d.call(
        "swarm.plan",
        json!({"id":id,"generation":1,"revision":0,"jobs":[
            {"id":"a","title":"Route A","acceptance":"evidence","deps":[],
             "resource_claims":[{"resource":"db:shared","mode":"write"}]},
            {"id":"b","title":"Route B","acceptance":"evidence","deps":[],
             "resource_claims":[{"resource":"db:shared","mode":"write"}]}
        ]}),
    );
    d.kill9();
    d.spawn();
    let decision = d.call(
        "swarm.benefit.commit",
        json!({"run_id":id,"generation":1,"revision":1,"estimate":pair()}),
    );
    assert_eq!(decision["decision"], "serial");
    assert_eq!(decision["reason"], "resource_conflict");
}

#[test]
fn supervised_exit_retains_measured_elapsed_but_not_invented_usage() {
    let mut d = Daemon::start(&[]);
    let id = run(&d, "Measured worker");
    d.call(
        "swarm.benefit.commit",
        json!({"run_id":id,
        "generation":1,"revision":1,"estimate":pair()}),
    );
    let admitted = admit(&d, &id, "a", now());
    assert_eq!(admitted["status"], "admitted");
    let temp = tmp();
    let checkout = repo(&temp.path().join("benefit-worker"));
    let launched = d.call(
        "swarm.worker.launch",
        json!({"run_id":id,"job_id":"a",
        "attempt_id":admitted["attempt_id"],"token":admitted["token"],
        "repo":checkout,"program":"/bin/sleep","args":["1"],
        "prompt":"Check one path","title":"Benefit worker"}),
    );
    let worker = launched["overseer_run_id"].as_str().unwrap();
    assert_eq!(d.wait_done(worker, 5)["status"], "completed");
    d.call(
        "swarm.worker.reconcile",
        json!({"run_id":id,"job_id":"a",
        "attempt_id":admitted["attempt_id"],"generation":1,"revision":1}),
    );
    d.kill9();
    d.spawn();
    let state = d.call("swarm.get", json!({"id":id}));
    assert!(
        state["benefit"]["outcomes"][0]["actual_elapsed_ms"]
            .as_i64()
            .unwrap()
            >= 1000
    );
    assert!(state["benefit"]["outcomes"][0]["actual_usage_milli"].is_null());
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let outcome: (i64, String, i64, Option<String>, String) = db
        .query_row(
            "SELECT estimate_elapsed_ms,estimate_usage_milli,actual_elapsed_ms,
                actual_usage_milli,actual_source FROM swarm_benefit_attempt_outcomes
         WHERE attempt_id=?1",
            [admitted["attempt_id"].as_str().unwrap()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .unwrap();
    assert_eq!(outcome.0, 100);
    assert_eq!(
        serde_json::from_str::<Value>(&outcome.1).unwrap()["points"],
        10
    );
    assert!(outcome.2 >= 1000);
    assert_eq!(outcome.3, None);
    assert_eq!(outcome.4, "supervised_run_wall");
}
