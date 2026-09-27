mod common;

use common::*;
use serde_json::{json, Value};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

fn estimate(max_workers: usize) -> Value {
    let cost = |elapsed, units| json!({"elapsed_ms":elapsed,"usage_milli":{"work_units":units}});
    let workers = json!([
        {"id":"a","elapsed_ms":1500,"usage_milli":{"work_units":100}},
        {"id":"b","elapsed_ms":1500,"usage_milli":{"work_units":100}}
    ]);
    json!({"independent":true,"max_workers":max_workers,
        "allocation_milli":{"work_units":300},
        "finishing_reserve_milli":{"work_units":20},
        "serial":{"planning":cost(30,5),"context":cost(20,5),
            "integration":cost(30,5),"review":cost(20,5),"retries":cost(0,0),
            "workers":workers},
        "parallel":{"planning":cost(30,5),"context":cost(40,10),
            "integration":cost(30,5),"review":cost(30,10),"retries":cost(0,0),
            "workers":workers}})
}

fn snapshot(at: i64) -> Value {
    json!({"version":1,"observed_ms":at-1000,"expires_ms":at+60000,
        "targets":[{"id":"fixture","account_id":"local","pool_ids":["pool"],
            "capabilities":["code"],"health":"up","auth":"ok"}],
        "pools":[{"id":"pool","windows":[{"id":"run","unit":"work_units",
            "remaining_milli":100000,"protected_milli":0,"reserved_milli":0,
            "confidence":"exact","expires_ms":at+60000}]}]})
}

#[test]
fn fixed_decision_suite_covers_serial_conflict_budget_and_independent_work() {
    let fixture = repo_root().join("fixtures/swarm/evaluation-v1");
    let manifest: Value =
        serde_json::from_slice(&std::fs::read(fixture.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(manifest["version"], 1);
    assert_eq!(manifest["worker_delay_ms"], 1500);
    assert_eq!(manifest["worker_usage_milli"], 100);
    assert_eq!(
        manifest["cases"],
        json!([
            "serial_dependency",
            "independent",
            "exclusive_conflict",
            "constrained_budget"
        ])
    );
    let d = Daemon::start(&[]);
    let independent = d.call("swarm.benefit.preview", estimate(2));
    assert_eq!(independent["decision"], "parallel");
    assert!(
        independent["serial"]["elapsed_ms"].as_i64().unwrap()
            > independent["parallel"]["elapsed_ms"].as_i64().unwrap()
    );
    let mut dependent = estimate(2);
    dependent["independent"] = json!(false);
    let serial = d.call("swarm.benefit.preview", dependent);
    assert_eq!(serial["decision"], "serial");
    assert_eq!(serial["reason"], "dependent_jobs");
    let mut constrained = estimate(2);
    constrained["allocation_milli"]["work_units"] = json!(225);
    let budget = d.call("swarm.benefit.preview", constrained);
    assert_eq!(budget["decision"], "serial");
    assert_eq!(budget["reason"], "allocation_exceeded");
    assert_eq!(budget["serial"]["usage_milli"]["work_units"], 220);
    assert_eq!(budget["parallel"]["usage_milli"]["work_units"], 230);
    let created = d.call(
        "swarm.create",
        json!({"category":"Evaluation write conflict",
        "objective":"Audit two writes","allowed_targets":["fixture"]}),
    );
    let run = created["id"].as_str().unwrap();
    d.call(
        "swarm.plan",
        json!({"id":run,"generation":1,"revision":0,"jobs":[
            {"id":"a","title":"A","acceptance":"evidence","deps":[],
                "resource_claims":[{"resource":"db:shared","mode":"write"}]},
            {"id":"b","title":"B","acceptance":"evidence","deps":[],
                "resource_claims":[{"resource":"db:shared","mode":"write"}]}
        ]}),
    );
    let conflict = d.call(
        "swarm.benefit.commit",
        json!({"run_id":run,
        "generation":1,"revision":1,"estimate":estimate(2)}),
    );
    assert_eq!(conflict["decision"], "serial");
    assert_eq!(conflict["reason"], "resource_conflict");
}

#[derive(Clone, Copy, Debug)]
enum Case {
    SerialDependency,
    Independent,
    ExclusiveConflict,
    ConstrainedBudget,
}

impl Case {
    fn name(self) -> &'static str {
        match self {
            Self::SerialDependency => "serial_dependency",
            Self::Independent => "independent",
            Self::ExclusiveConflict => "exclusive_conflict",
            Self::ConstrainedBudget => "constrained_budget",
        }
    }

    fn estimate(self, max_workers: usize) -> Value {
        let mut result = estimate(max_workers);
        if matches!(self, Self::SerialDependency) {
            result["independent"] = json!(false);
        }
        if matches!(self, Self::ConstrainedBudget) {
            result["allocation_milli"]["work_units"] = json!(225);
        }
        result
    }

    fn ready_estimate(self, max_workers: usize, job: &str) -> Value {
        let mut result = self.estimate(max_workers);
        if matches!(self, Self::SerialDependency) {
            for mode in ["serial", "parallel"] {
                result[mode]["workers"] = json!([{
                    "id":job,"elapsed_ms":1500,"usage_milli":{"work_units":100}
                }]);
            }
        }
        result
    }

    fn jobs(self) -> Value {
        let claims = if matches!(self, Self::ExclusiveConflict) {
            json!([{"resource":"db:shared","mode":"write"}])
        } else {
            json!([])
        };
        json!([
            {"id":"a","title":"A","acceptance":"A evidence","deps":[],
                "resource_claims":claims},
            {"id":"b","title":"B","acceptance":"B evidence",
                "deps":if matches!(self, Self::SerialDependency) { json!(["a"]) } else { json!([]) },
                "resource_claims":claims}
        ])
    }
}

struct ResultRow {
    elapsed_ms: u128,
    accepted: usize,
    work_units_milli: i64,
    decision: String,
    reason: String,
}

fn run_pair(case: Case, candidate: bool) -> ResultRow {
    let d = Daemon::start(&[]);
    let temp = tmp();
    let checkout = repo(&temp.path().join("evaluation-source"));
    let fixture = repo_root().join("fixtures/swarm/evaluation-v1");
    let max_workers = if candidate { 2 } else { 1 };
    let created = d.call(
        "swarm.create",
        json!({"category":format!("{} {} evaluation", case.name(), if candidate {"candidate"} else {"baseline"}),
        "objective":"Produce two checked local artifacts",
        "allowed_targets":["fixture"],"policy":{"max_workers":max_workers}}),
    );
    let run = created["id"].as_str().unwrap();
    d.call(
        "swarm.plan",
        json!({"id":run,"generation":1,"revision":0,"jobs":case.jobs()}),
    );
    let decision = d.call(
        "swarm.benefit.commit",
        json!({"run_id":run,"generation":1,
        "revision":1,"estimate":case.ready_estimate(max_workers, "a")}),
    );
    let should_parallel = candidate && matches!(case, Case::Independent);
    assert_eq!(
        decision["decision"],
        if should_parallel {
            "parallel"
        } else {
            "serial"
        }
    );
    let at = now();
    let snapshot = snapshot(at);
    let start = Instant::now();
    let mut workers = Vec::new();
    let launch = |job: &str| {
        let admitted = d.call(
            "swarm.admit",
            json!({"run_id":run,"generation":1,
            "revision":1,"job_id":job,"target_id":"fixture",
            "request_id":format!("evaluation-{job}"),"snapshot":snapshot,
            "now_ms":at,"required_capabilities":["code"],
            "estimate_milli":{"work_units":100},"purpose":"worker"}),
        );
        assert_eq!(admitted["status"], "admitted", "{admitted}");
        let launched = d.call(
            "swarm.worker.launch",
            json!({"run_id":run,
            "job_id":job,"attempt_id":admitted["attempt_id"],"token":admitted["token"],
            "repo":checkout,"program":"/usr/bin/python3",
            "args":[fixture.join("worker.py"),"1500"],
            "prompt":"Produce the assigned local evidence","title":format!("Evaluation {job}")}),
        );
        assert_eq!(launched["status"], "launched", "{launched}");
        (
            launched["overseer_run_id"].as_str().unwrap().to_string(),
            admitted["attempt_id"].as_str().unwrap().to_string(),
        )
    };
    workers.push(("a", launch("a")));
    if should_parallel {
        workers.push(("b", launch("b")));
    } else {
        let held = d.call(
            "swarm.admit",
            json!({"run_id":run,"generation":1,"revision":1,"job_id":"b",
                "target_id":"fixture","request_id":"evaluation-b-before-a",
                "snapshot":snapshot,"now_ms":at,"required_capabilities":["code"],
                "estimate_milli":{"work_units":100},"purpose":"worker"}),
        );
        assert_eq!(held["status"], "blocked", "{held}");
        assert_eq!(
            held["reason"],
            match case {
                Case::SerialDependency => "dependency_pending",
                Case::ExclusiveConflict => "resource_conflict",
                Case::Independent | Case::ConstrainedBudget if !candidate => "worker_limit",
                Case::Independent | Case::ConstrainedBudget => "benefit_serial",
            },
            "{held}"
        );
    }
    for (job, (worker, attempt)) in &workers {
        assert_eq!(d.wait_done(worker, 10)["status"], "completed");
        d.call(
            "swarm.worker.reconcile",
            json!({"run_id":run,"job_id":job,
            "attempt_id":attempt,"generation":1,"revision":1}),
        );
    }
    let mut accepted = 0;
    if !should_parallel {
        let decided = d.call(
            "swarm.decide",
            json!({"run_id":run,"generation":1,
            "revision":1,"job_id":"a","decision":"accept",
            "evidence":["evaluation-proof-a"]}),
        );
        assert_eq!(decided["status"], "accepted", "{decided}");
        accepted += 1;
        if matches!(case, Case::SerialDependency) {
            let second_wave = d.call(
                "swarm.benefit.commit",
                json!({"run_id":run,"generation":1,"revision":1,
                    "estimate":case.ready_estimate(max_workers, "b")}),
            );
            assert_eq!(second_wave["decision"], "serial", "{second_wave}");
            assert_eq!(second_wave["wave"], 2);
        }
        workers.push(("b", launch("b")));
        let (_, (worker, attempt)) = &workers[1];
        assert_eq!(d.wait_done(worker, 10)["status"], "completed");
        d.call(
            "swarm.worker.reconcile",
            json!({"run_id":run,"job_id":"b",
            "attempt_id":attempt,"generation":1,"revision":1}),
        );
    }
    for job in if should_parallel {
        vec!["a", "b"]
    } else {
        vec!["b"]
    } {
        let decided = d.call(
            "swarm.decide",
            json!({"run_id":run,"generation":1,
            "revision":1,"job_id":job,"decision":"accept",
            "evidence":[format!("evaluation-proof-{job}")]}),
        );
        assert_eq!(decided["status"], "accepted", "{decided}");
        accepted += 1;
    }
    let inbox = d.call(
        "swarm.messages",
        json!({"run_id":run,"recipient":"director","limit":100}),
    );
    let results: Vec<_> = inbox["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|message| message["type"] == "result")
        .collect();
    assert_eq!(results.len(), 2);
    let work_units_milli = results
        .iter()
        .map(|message| {
            message["payload"]["fixture_work_units_milli"]
                .as_i64()
                .unwrap()
        })
        .sum();
    let state = d.call("swarm.get", json!({"id":run}));
    assert!(state["benefit"]["outcomes"]
        .as_array()
        .unwrap()
        .iter()
        .all(|outcome| outcome["actual_usage_milli"].is_null()));
    for _ in 0..4 {
        let batch = d.call(
            "swarm.director.claim_batch",
            json!({"run_id":run,
            "generation":1,"revision":1,"now_ms":now()+5000}),
        );
        if batch["status"] == "idle" {
            break;
        }
        assert_eq!(batch["status"], "claimed", "{batch}");
        d.call(
            "swarm.director.complete_batch",
            json!({"run_id":run,
            "generation":1,"revision":1,"turn_id":batch["turn_id"],
            "token":batch["token"],"outcome":"progress"}),
        );
    }
    let completed = d.call(
        "swarm.complete",
        json!({"run_id":run,"generation":1,
        "revision":1,"request_id":"evaluation-complete",
        "summary":"Both local checks accepted",
        "verification":"Two scripted evidence-producing workers exited",
        "checks":[{"job_id":"a","outcome":"passed","evidence":["evaluation-proof-a"]},
            {"job_id":"b","outcome":"passed","evidence":["evaluation-proof-b"]}]}),
    );
    assert_eq!(completed["status"], "completed", "{completed}");
    let elapsed_ms = start.elapsed().as_millis();
    ResultRow {
        elapsed_ms,
        accepted,
        work_units_milli,
        decision: decision["decision"].as_str().unwrap().to_string(),
        reason: decision["reason"].as_str().unwrap().to_string(),
    }
}

#[test]
fn local_parallel_pair_finishes_faster_at_equal_fixture_acceptance() {
    let serial = run_pair(Case::Independent, false);
    let parallel = run_pair(Case::Independent, true);
    eprintln!("evaluation-v1 serial_ms={} parallel_ms={} serial_overhead_ms={} parallel_overhead_ms={} serial_accepted={} parallel_accepted={} serial_fixture_work_units_milli={} parallel_fixture_work_units_milli={}",
        serial.elapsed_ms,parallel.elapsed_ms,
        serial.elapsed_ms.saturating_sub(3000),parallel.elapsed_ms.saturating_sub(1500),
        serial.accepted,parallel.accepted,
        serial.work_units_milli,parallel.work_units_milli);
    assert_eq!((serial.accepted, parallel.accepted), (2, 2));
    assert_eq!(
        (serial.work_units_milli, parallel.work_units_milli),
        (200, 200)
    );
    assert!(
        parallel.elapsed_ms < serial.elapsed_ms,
        "parallel {} ms did not beat serial {} ms",
        parallel.elapsed_ms,
        serial.elapsed_ms
    );
}

#[test]
fn local_serial_cases_keep_second_worker_held_until_first_is_accepted() {
    for case in [
        Case::SerialDependency,
        Case::ExclusiveConflict,
        Case::ConstrainedBudget,
    ] {
        let baseline = run_pair(case, false);
        let candidate = run_pair(case, true);
        eprintln!("evaluation-v1 case={} baseline_ms={} candidate_ms={} baseline_accepted={} candidate_accepted={} baseline_fixture_work_units_milli={} candidate_fixture_work_units_milli={}",
            case.name(),baseline.elapsed_ms,candidate.elapsed_ms,
            baseline.accepted,candidate.accepted,
            baseline.work_units_milli,candidate.work_units_milli);
        assert_eq!(
            (baseline.decision.as_str(), candidate.decision.as_str()),
            ("serial", "serial")
        );
        let reason = match case {
            Case::SerialDependency => "dependent_jobs",
            Case::ExclusiveConflict => "resource_conflict",
            Case::ConstrainedBudget => "allocation_exceeded",
            Case::Independent => unreachable!(),
        };
        assert_eq!(candidate.reason, reason);
        assert_eq!((baseline.accepted, candidate.accepted), (2, 2));
        assert_eq!(
            (baseline.work_units_milli, candidate.work_units_milli),
            (200, 200)
        );
    }
}
