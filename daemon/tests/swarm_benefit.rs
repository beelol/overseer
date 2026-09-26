mod common;

use common::*;
use serde_json::{json, Value};

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
