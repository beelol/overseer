//! SWARM-45: independent reproduction and duplicate findings. The director
//! creates J7 explicitly as an independent reproduction of a recorded finding,
//! with its own bounded job and budget; duplicate discovery messages cannot
//! create repeated reproducers. The director merges duplicate findings without
//! losing endpoint-specific evidence, and agreement between workers never
//! confirms a finding: only accepted reproduction evidence does (from the
//! reproducer, once one exists).
//!
//! Scripted director and workers on a fixture swarm (S1's Atlas shape, no
//! backend); no provider account is used.

mod common;
use common::*;
use serde_json::{json, Value};

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64
}

fn snapshot(at: i64) -> Value {
    json!({"version":1,"observed_ms":at-1000,"expires_ms":at+600000,
        "targets":[{"id":"fixture","account_id":"account-a","pool_ids":["pool-a"],
            "capabilities":["audit"],"health":"up","auth":"ok"}],
        "pools":[{"id":"pool-a","windows":[{"id":"week","unit":"points","remaining_milli":10000000,
            "protected_milli":0,"reserved_milli":0,"confidence":"exact","expires_ms":at+600000}]}]})
}

fn job(id: &str) -> Value {
    json!({"id":id,"title":format!("Audit {id}"),"acceptance":"evidence","deps":[],
        "resource_claims":[{"resource":format!("atlas-db-{id}"),"mode":"write"}]})
}

fn setup(d: &Daemon, max_workers: i64, jobs: &[&str]) -> String {
    let created = d.call("swarm.create", json!({"category":"Backend security",
        "objective":"Audit Atlas tenant isolation; report bugs, do not change application code",
        "allowed_targets":["fixture"],"policy":{"max_workers":max_workers}}));
    let run = created["id"].as_str().unwrap().to_string();
    d.call("swarm.plan", json!({"id":run,"generation":1,"revision":0,
        "jobs":jobs.iter().map(|j| job(j)).collect::<Vec<_>>()}));
    run
}

fn benefit(d: &Daemon, run: &str, revision: i64, jobs: &[&str], max: i64) {
    let workers: Vec<Value> = jobs.iter().map(|id| json!({"id":id,"elapsed_ms":100,"usage_milli":{"points":10}})).collect();
    let serial = json!({"planning":{"elapsed_ms":10,"usage_milli":{"points":1}},
        "context":{"elapsed_ms":10,"usage_milli":{"points":1}},
        "integration":{"elapsed_ms":10,"usage_milli":{"points":1}},
        "review":{"elapsed_ms":10,"usage_milli":{"points":1}},
        "retries":{"elapsed_ms":0,"usage_milli":{"points":1}},"workers":workers});
    let mut parallel = serial.clone();
    parallel["context"]["elapsed_ms"] = json!(20);
    let decision = d.call("swarm.benefit.commit", json!({"run_id":run,"generation":1,"revision":revision,
        "estimate":{"independent":true,"max_workers":max,"allocation_milli":{"points":1000000},
            "finishing_reserve_milli":{"points":20000},"serial":serial,"parallel":parallel}}));
    assert_eq!(decision["decision"], "parallel", "{decision}");
}

fn admit(d: &Daemon, run: &str, revision: i64, job: &str, request: &str, points: i64) -> Value {
    let at = now();
    d.call("swarm.admit", json!({"run_id":run,"generation":1,"revision":revision,"job_id":job,
        "target_id":"fixture","request_id":request,"snapshot":snapshot(at),"now_ms":at,
        "required_capabilities":["audit"],"estimate_milli":{"points":points},"purpose":"worker"}))
}

fn put(d: &Daemon, run: &str, job: &str, attempt: &Value, revision: i64, artifact: &str, kind: &str, content: &str) {
    d.call("swarm.artifact.put", json!({"run_id":run,"job_id":job,"attempt_id":attempt["attempt_id"],
        "token":attempt["token"],"artifact_id":artifact,"source_revision":revision,"kind":kind,"content":content}));
}

fn result(d: &Daemon, run: &str, job: &str, attempt: &Value, revision: i64, message: &str, outcome: &str, artifacts: &[&str]) {
    d.call("swarm.report", json!({"run_id":run,"job_id":job,"attempt_id":attempt["attempt_id"],
        "token":attempt["token"],"message_id":message,"type":"result","revision":revision,
        "payload":{"audit_outcome":outcome,"artifact_ids":artifacts}}));
}

fn discovery(d: &Daemon, run: &str, job: &str, attempt: &Value, revision: i64, message: &str) {
    d.call("swarm.report", json!({"run_id":run,"job_id":job,"attempt_id":attempt["attempt_id"],
        "token":attempt["token"],"message_id":message,"type":"discovery","revision":revision,
        "payload":{"symbol":"TaskRepository.findById","note":"lookup filters by task id only"}}));
}

fn accept_and_exit(d: &Daemon, run: &str, revision: i64, job: &str, attempt: &Value, evidence: &[&str]) {
    let decided = d.call("swarm.decide", json!({"run_id":run,"generation":1,"revision":revision,"job_id":job,
        "decision":"accept","evidence":evidence}));
    assert_eq!(decided["status"], "accepted", "{decided}");
    d.call("swarm.attempt.confirm_exit", json!({"run_id":run,"generation":1,"revision":revision,
        "job_id":job,"attempt_id":attempt["attempt_id"]}));
}

fn revision(d: &Daemon, run: &str) -> i64 {
    d.call("swarm.get", json!({"id":run}))["revision"].as_i64().unwrap()
}

fn findings(d: &Daemon, run: &str) -> Vec<Value> {
    d.call("swarm.findings", json!({"run_id":run}))["findings"].as_array().unwrap().clone()
}

fn finding(d: &Daemon, run: &str, id: &str) -> Value {
    findings(d, run).into_iter().find(|f| f["finding_id"] == id).unwrap_or_else(|| panic!("no finding {id}"))
}

fn job_row(d: &Daemon, run: &str, id: &str) -> Option<Value> {
    d.call("swarm.jobs", json!({"id":run}))["jobs"].as_array().unwrap().iter().find(|j| j["id"] == id).cloned()
}

#[test]
fn a_reproducer_is_an_explicit_budgeted_job_and_duplicate_discoveries_make_one() {
    let d = Daemon::start(&[]);
    let run = setup(&d, 2, &["j2", "j4", "j6"]);
    benefit(&d, &run, 1, &["j2", "j4"], 2);
    let a2 = admit(&d, &run, 1, "j2", "adm-j2", 100);
    let a4 = admit(&d, &run, 1, "j4", "adm-j4", 100);
    assert_eq!((a2["status"].as_str(), a4["status"].as_str()), (Some("admitted"), Some("admitted")));
    // J2 reports D1 and its local reproduction; J4 reports the same discovery.
    discovery(&d, &run, "j2", &a2, 1, "D1");
    discovery(&d, &run, "j4", &a4, 1, "D1-from-j4");
    put(&d, &run, "j2", &a2, 1, "j2-repro", "reproduction", "alice PATCH /tasks/task-b-7 -> 200; Bob's row changed");
    result(&d, &run, "j2", &a2, 1, "j2-result", "confirmed_defect", &["j2-repro"]);
    let recorded = d.call("swarm.finding.record", json!({"run_id":run,"generation":1,"revision":1,
        "finding_id":"task-foreign-mutation","title":"Foreign task mutation","root_cause":"D1",
        "evidence":[{"job_id":"j2","artifact_id":"j2-repro","endpoint":"PATCH /tasks/:id"}]}));
    assert_eq!(recorded["status"], "candidate", "{recorded}");

    // J7: an explicit independent reproduction with its own bounded budget.
    let j7 = json!({"id":"j7","title":"Independent task reproduction",
        "acceptance":"repeat the foreign task mutation from a fresh fixture",
        "resource_claims":[{"resource":"atlas-db-j7","mode":"write"}],"required_capabilities":["audit"]});
    let made = d.call("swarm.reproduce", json!({"run_id":run,"generation":1,"revision":1,
        "finding_id":"task-foreign-mutation","source_message_id":"D1","job":j7,"budget_milli":{"points":200}}));
    assert_eq!((made["job_id"].as_str(), made["duplicate"].as_bool()), (Some("j7"), Some(false)), "{made}");
    assert_eq!(revision(&d, &run), 2, "a reproducer is a plan revision");
    let row = job_row(&d, &run, "j7").unwrap();
    assert_eq!(row["status"], "ready", "{row}");
    let f = finding(&d, &run, "task-foreign-mutation");
    assert_eq!(f["reproducer"]["job_id"], "j7", "{f}");
    assert_eq!(f["reproducer"]["budget_milli"], json!({"points":200}));
    // Duplicate discoveries, replays and other findings citing the same
    // discovery make no second reproducer.
    for (source, finding_id, job_id) in [("D1", "task-foreign-mutation", "j7"), ("D1-from-j4", "task-foreign-mutation", "j8"),
        ("D1", "task-foreign-mutation", "j9")] {
        let mut again = j7.clone();
        again["id"] = json!(job_id);
        again["resource_claims"] = json!([{"resource":format!("atlas-db-{job_id}"),"mode":"write"}]);
        let dup = d.call("swarm.reproduce", json!({"run_id":run,"generation":1,"revision":2,
            "finding_id":finding_id,"source_message_id":source,"job":again,"budget_milli":{"points":200}}));
        assert_eq!((dup["job_id"].as_str(), dup["duplicate"].as_bool()), (Some("j7"), Some(true)), "{source}: {dup}");
    }
    d.call("swarm.finding.record", json!({"run_id":run,"generation":1,"revision":2,
        "finding_id":"attachment-download","title":"Foreign attachment download","root_cause":"D1",
        "evidence":[{"job_id":"j2","artifact_id":"j2-repro","endpoint":"PATCH /tasks/:id"}]}));
    let other = d.call("swarm.reproduce", json!({"run_id":run,"generation":1,"revision":2,
        "finding_id":"attachment-download","source_message_id":"D1","job":json!({"id":"j10","title":"Repro",
            "acceptance":"repeat","resource_claims":[{"resource":"atlas-db-j10","mode":"write"}]}),
        "budget_milli":{"points":100}}));
    assert_eq!((other["job_id"].as_str(), other["duplicate"].as_bool()), (Some("j7"), Some(true)),
        "a discovery already reproduced cannot start another reproducer: {other}");
    for id in ["j8", "j9", "j10"] {
        assert!(job_row(&d, &run, id).is_none(), "no job {id}");
    }
    assert_eq!(revision(&d, &run), 2);
    // Independence: a reproducer cannot build on the reproduced work.
    d.call("swarm.finding.record", json!({"run_id":run,"generation":1,"revision":2,
        "finding_id":"task-delete","title":"Foreign task delete",
        "evidence":[{"job_id":"j2","artifact_id":"j2-repro","endpoint":"DELETE /tasks/:id"}]}));
    let dependent = d.try_call("swarm.reproduce", json!({"run_id":run,"generation":1,"revision":2,
        "finding_id":"task-delete","source_message_id":"j2-result","job":{"id":"j11","title":"Repro",
            "acceptance":"repeat","deps":["j2"]},"budget_milli":{"points":100}}));
    assert!(dependent.unwrap_err().contains("independent"));
    let shared = d.try_call("swarm.reproduce", json!({"run_id":run,"generation":1,"revision":2,
        "finding_id":"task-delete","source_message_id":"j2-result","job":{"id":"j11","title":"Repro",
            "acceptance":"repeat","resource_claims":[{"resource":"atlas-db-j2","mode":"write"}]},
        "budget_milli":{"points":100}}));
    assert!(shared.unwrap_err().contains("independent"));
    assert!(d.try_call("swarm.reproduce", json!({"run_id":run,"generation":1,"revision":2,
        "finding_id":"no-such-finding","source_message_id":"j2-result","job":{"id":"j11","title":"Repro",
            "acceptance":"repeat"},"budget_milli":{"points":100}})).is_err());
    assert!(d.try_call("swarm.reproduce", json!({"run_id":run,"generation":1,"revision":2,
        "finding_id":"task-delete","source_message_id":"j2-result","job":{"id":"j11","title":"Repro",
            "acceptance":"repeat"},"budget_milli":{"points":0}})).is_err(), "a budget is bounded and positive");

    // J7 is an ordinary worker in the queue: not a secret extra worker.
    benefit(&d, &run, 2, &["j7", "j6"], 2);
    let blocked = admit(&d, &run, 2, "j7", "adm-j7-a", 150);
    assert_eq!(blocked["reason"], "worker_limit", "{blocked}");
    accept_and_exit(&d, &run, 2, "j2", &a2, &["j2-repro"]);
    // Its own budget bounds it: 150 of 200 points on the first attempt.
    let over = admit(&d, &run, 2, "j7", "adm-j7-big", 250);
    assert_eq!(over["reason"], "reproduction_budget", "{over}");
    let a7 = admit(&d, &run, 2, "j7", "adm-j7-b", 150);
    assert_eq!(a7["status"], "admitted", "{a7}");
    put(&d, &run, "j7", &a7, 2, "j7-inconclusive", "finding", "fixture did not start");
    result(&d, &run, "j7", &a7, 2, "j7-result-1", "negative", &["j7-inconclusive"]);
    d.call("swarm.decide", json!({"run_id":run,"generation":1,"revision":2,"job_id":"j7","decision":"reject",
        "evidence":["j7-inconclusive"]}));
    d.call("swarm.attempt.confirm_exit", json!({"run_id":run,"generation":1,"revision":2,"job_id":"j7",
        "attempt_id":a7["attempt_id"]}));
    let second = admit(&d, &run, 2, "j7", "adm-j7-c", 100);
    assert_eq!(second["reason"], "reproduction_budget", "150 + 100 exceeds 200: {second}");
    let within = admit(&d, &run, 2, "j7", "adm-j7-d", 50);
    assert_eq!(within["status"], "admitted", "{within}");
    let f = finding(&d, &run, "task-foreign-mutation");
    assert_eq!(f["reproducer"]["spent_milli"], 200, "{f}");
}

#[test]
fn merged_findings_keep_endpoint_evidence_and_agreement_is_not_proof() {
    let d = Daemon::start(&[]);
    let run = setup(&d, 4, &["j2", "j4", "j5", "j6"]);
    benefit(&d, &run, 1, &["j2", "j4", "j5"], 3);
    let a2 = admit(&d, &run, 1, "j2", "adm-j2", 100);
    let a4 = admit(&d, &run, 1, "j4", "adm-j4", 100);
    let a5 = admit(&d, &run, 1, "j5", "adm-j5", 100);
    discovery(&d, &run, "j2", &a2, 1, "D1");
    put(&d, &run, "j2", &a2, 1, "j2-patch", "finding", "PATCH /tasks/task-b-7 as alice -> 200");
    put(&d, &run, "j2", &a2, 1, "j2-delete", "finding", "DELETE /tasks/task-b-8 as alice -> 204");
    result(&d, &run, "j2", &a2, 1, "j2-result", "confirmed_defect", &["j2-patch", "j2-delete"]);
    put(&d, &run, "j5", &a5, 1, "j5-patch", "finding", "PATCH /tasks/task-b-7 as alice -> 200 (second worker)");
    result(&d, &run, "j5", &a5, 1, "j5-result", "confirmed_defect", &["j5-patch"]);
    put(&d, &run, "j4", &a4, 1, "j4-download", "reproduction", "GET /attachments/att-b-1/download as alice -> signed URL, object fetched");
    result(&d, &run, "j4", &a4, 1, "j4-result", "confirmed_defect", &["j4-download"]);

    // Two workers report the same PATCH defect: recorded separately, then merged.
    let record = |finding_id: &str, title: &str, evidence: Value| d.call("swarm.finding.record", json!({"run_id":run,
        "generation":1,"revision":revision(&d, &run),"finding_id":finding_id,"title":title,"root_cause":"D1","evidence":evidence}));
    record("task-mutation", "Foreign task mutation", json!([
        {"job_id":"j2","artifact_id":"j2-patch","endpoint":"PATCH /tasks/:id"},
        {"job_id":"j2","artifact_id":"j2-delete","endpoint":"DELETE /tasks/:id"}]));
    record("task-mutation-j5", "Foreign task mutation (J5)", json!([
        {"job_id":"j5","artifact_id":"j5-patch","endpoint":"PATCH /tasks/:id"}]));
    record("attachment-download", "Foreign attachment download", json!([
        {"job_id":"j4","artifact_id":"j4-download","endpoint":"GET /attachments/:id/download"}]));
    let merged = d.call("swarm.finding.merge", json!({"run_id":run,"generation":1,"revision":revision(&d, &run),
        "into":"task-mutation","from":["task-mutation-j5"]}));
    assert_eq!(merged["finding_id"], "task-mutation", "{merged}");
    let f = finding(&d, &run, "task-mutation");
    let endpoints = f["endpoints"].as_array().unwrap();
    let patch = endpoints.iter().find(|e| e["endpoint"] == "PATCH /tasks/:id").unwrap();
    let delete = endpoints.iter().find(|e| e["endpoint"] == "DELETE /tasks/:id").unwrap();
    let jobs = |e: &Value| e["evidence"].as_array().unwrap().iter().map(|x| format!("{}:{}", x["job_id"].as_str().unwrap(),
        x["artifact_id"].as_str().unwrap())).collect::<Vec<_>>();
    assert_eq!(jobs(patch), ["j2:j2-patch", "j5:j5-patch"], "{f}");
    assert_eq!(jobs(delete), ["j2:j2-delete"], "{f}");
    // Two agreeing workers are not proof.
    assert_eq!(f["agreeing_jobs"], 2);
    assert_eq!(f["status"], "candidate", "{f}");
    assert_eq!(patch["confirmed"], false);
    let old = finding(&d, &run, "task-mutation-j5");
    assert_eq!((old["status"].as_str(), old["merged_into"].as_str()), (Some("merged"), Some("task-mutation")), "{old}");
    // Recording again never drops evidence; unknown or unsubmitted evidence is refused.
    record("task-mutation", "Foreign task mutation", json!([{"job_id":"j5","artifact_id":"j5-patch","endpoint":"PATCH /tasks/:id"}]));
    assert_eq!(finding(&d, &run, "task-mutation")["endpoints"].as_array().unwrap().iter()
        .map(|e| e["evidence"].as_array().unwrap().len()).sum::<usize>(), 3);
    assert!(d.try_call("swarm.finding.record", json!({"run_id":run,"generation":1,"revision":1,"finding_id":"task-mutation",
        "title":"Foreign task mutation","evidence":[{"job_id":"j5","artifact_id":"j2-patch","endpoint":"PATCH /tasks/:id"}]})).is_err());
    assert!(d.try_call("swarm.finding.record", json!({"run_id":run,"generation":1,"revision":1,"finding_id":"task-mutation-j5",
        "title":"Foreign task mutation (J5)","evidence":[{"job_id":"j5","artifact_id":"j5-patch","endpoint":"PATCH /tasks/:id"}]}))
        .unwrap_err().contains("merged"));
    // The attachment finding is linked to D1 but stays its own finding with its own evidence.
    let attachment = finding(&d, &run, "attachment-download");
    assert_eq!((attachment["root_cause"].as_str(), attachment["merged_into"].as_str()), (Some("D1"), None));
    assert_eq!(attachment["status"], "candidate", "not yet accepted");
    accept_and_exit(&d, &run, 1, "j4", &a4, &["j4-download"]);
    assert_eq!(finding(&d, &run, "attachment-download")["status"], "confirmed");

    // The independent reproducer confirms the PATCH endpoint only.
    let made = d.call("swarm.reproduce", json!({"run_id":run,"generation":1,"revision":1,
        "finding_id":"task-mutation","source_message_id":"j2-result","job":{"id":"j7",
            "title":"Independent task reproduction","acceptance":"repeat PATCH from a fresh fixture",
            "resource_claims":[{"resource":"atlas-db-j7","mode":"write"}],"required_capabilities":["audit"]},
        "budget_milli":{"points":300}}));
    assert_eq!(made["job_id"], "j7");
    benefit(&d, &run, 2, &["j7", "j6"], 3);
    let a7 = admit(&d, &run, 2, "j7", "adm-j7", 100);
    assert_eq!(a7["status"], "admitted", "{a7}");
    put(&d, &run, "j7", &a7, 2, "j7-patch", "reproduction", "fresh fixture: PATCH /tasks/task-b-7 as alice -> 200");
    result(&d, &run, "j7", &a7, 2, "j7-result", "confirmed_defect", &["j7-patch"]);
    record("task-mutation", "Foreign task mutation", json!([{"job_id":"j7","artifact_id":"j7-patch","endpoint":"PATCH /tasks/:id"}]));
    assert_eq!(finding(&d, &run, "task-mutation")["status"], "candidate", "submitted, not accepted");
    accept_and_exit(&d, &run, 2, "j7", &a7, &["j7-patch"]);
    let f = finding(&d, &run, "task-mutation");
    let endpoint = |name: &str| f["endpoints"].as_array().unwrap().iter().find(|e| e["endpoint"] == name).unwrap().clone();
    assert_eq!(endpoint("PATCH /tasks/:id")["confirmed"], true, "{f}");
    assert_eq!(endpoint("DELETE /tasks/:id")["confirmed"], false, "{f}");
    assert_eq!(f["status"], "partially_confirmed", "{f}");
    assert_eq!(endpoint("PATCH /tasks/:id")["evidence"].as_array().unwrap().len(), 3, "J2, J5 and J7 all kept");
}
