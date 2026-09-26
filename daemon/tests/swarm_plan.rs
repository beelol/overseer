mod common;

use common::*;
use serde_json::json;

fn run_with_job(d: &Daemon, category: &str, job: &str) -> String {
    let made=d.call("swarm.create",json!({"category":category,"objective":"Audit behavior","allowed_targets":["system-codex"]}));
    let id = made["id"].as_str().unwrap().to_owned();
    d.call("swarm.plan",json!({"id":id,"generation":1,"revision":0,"jobs":[{"id":job,"title":"Inspect","acceptance":"evidence","deps":[]}]}));
    id
}

#[test]
fn shared_reads_and_exclusive_claims_span_categories() {
    let d = Daemon::start(&[]);
    let backend = run_with_job(&d, "Backend", "routes");
    let qa = run_with_job(&d, "QA", "verify");
    let claim = |run_id: &str, job_id: &str, resource: &str, mode: &str| json!({"run_id":run_id,"generation":1,"revision":1,"job_id":job_id,"resource":resource,"mode":mode});
    d.call(
        "swarm.claim",
        claim(&backend, "routes", "file:routes.rs", "read"),
    );
    d.call(
        "swarm.claim",
        claim(&qa, "verify", "file:routes.rs", "read"),
    );
    d.call(
        "swarm.claim",
        claim(&backend, "routes", "db:test-tenant", "write"),
    );
    assert!(d
        .try_call(
            "swarm.claim",
            claim(&qa, "verify", "db:test-tenant", "write")
        )
        .unwrap_err()
        .contains("conflict"));
    assert!(d
        .try_call(
            "swarm.claim",
            claim(&qa, "verify", "db:test-tenant", "read")
        )
        .unwrap_err()
        .contains("conflict"));
    assert!(d
        .try_call(
            "swarm.claim",
            claim(&qa, "verify", "file:routes.rs", "write")
        )
        .unwrap_err()
        .contains("conflict"));
    assert_eq!(
        d.call(
            "swarm.claim",
            claim(&backend, "routes", "db:test-tenant", "write")
        )["duplicate"],
        true
    );
    assert!(d
        .try_call(
            "swarm.claim",
            claim(&backend, "routes", "file:routes.rs", "write")
        )
        .is_err());
    assert!(d.try_call("swarm.claim",json!({"run_id":backend,"generation":0,"revision":1,"job_id":"routes","resource":"x","mode":"read"})).is_err());
}

#[test]
fn one_hypothesis_owner_with_explicit_reproducer() {
    let d = Daemon::start(&[]);
    let audit = run_with_job(&d, "Security", "investigate");
    let repro = run_with_job(&d, "Reproduction", "reproduce");
    d.call("swarm.claim",json!({"run_id":audit,"generation":1,"revision":1,"job_id":"investigate","resource":"hypothesis:tenant-leak","mode":"write"}));
    assert!(d.try_call("swarm.claim",json!({"run_id":repro,"generation":1,"revision":1,"job_id":"reproduce","resource":"hypothesis:tenant-leak","mode":"write"})).is_err());
    d.call("swarm.claim",json!({"run_id":repro,"generation":1,"revision":1,"job_id":"reproduce","resource":"reproduction:tenant-leak:independent","mode":"write"}));
}

#[test]
fn evidence_review_and_confirmed_exit_gate_dependent_work() {
    let d = Daemon::start(&[]);
    let run=d.call("swarm.create",json!({"category":"Catalog","objective":"Change paging","allowed_targets":["system-codex"]}));
    let run_id = run["id"].as_str().unwrap();
    d.call("swarm.plan",json!({"id":run_id,"generation":1,"revision":0,"jobs":[
        {"id":"interface","title":"Change interface","acceptance":"contract diff","deps":[]},
        {"id":"consumer","title":"Change consumer","acceptance":"consumer test","deps":["interface"]}
    ]}));
    let attempt = d.call(
        "swarm.attempt.register",
        json!({"run_id":run_id,"generation":1,"revision":1,"job_id":"interface"}),
    );
    let attempt_id = attempt["id"].as_str().unwrap();
    let token = attempt["token"].as_str().unwrap();
    assert!(d.try_call("swarm.decide",json!({"run_id":run_id,"generation":1,"revision":1,"job_id":"interface","decision":"accept","evidence":["missing"]})).is_err());
    d.call("swarm.artifact.put",json!({"run_id":run_id,"job_id":"interface","attempt_id":attempt_id,"token":token,"artifact_id":"contract-v1","source_revision":1,"kind":"contract","content":"GET /items returns cursor"}));
    d.call("swarm.report",json!({"run_id":run_id,"job_id":"interface","attempt_id":attempt_id,"token":token,"message_id":"result-1","type":"result","revision":1,"payload":{"artifact_ids":["contract-v1"]}}));
    d.call("swarm.decide",json!({"run_id":run_id,"generation":1,"revision":1,"job_id":"interface","decision":"reject","evidence":["contract-v1"]}));
    let jobs = d.call("swarm.jobs", json!({"id":run_id}));
    let consumer = jobs["jobs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|j| j["id"] == "consumer")
        .unwrap();
    assert_eq!(consumer["status"], "planned");
    d.call("swarm.attempt.confirm_exit",json!({"run_id":run_id,"job_id":"interface","attempt_id":attempt_id,"generation":1,"revision":1}));
    let retry = d.call(
        "swarm.attempt.register",
        json!({"run_id":run_id,"generation":1,"revision":1,"job_id":"interface"}),
    );
    let retry_id = retry["id"].as_str().unwrap();
    let retry_token = retry["token"].as_str().unwrap();
    assert!(d.try_call("swarm.decide",json!({"run_id":run_id,"generation":1,"revision":1,"job_id":"interface","decision":"accept","evidence":["contract-v1"]})).is_err());
    d.call("swarm.artifact.put",json!({"run_id":run_id,"job_id":"interface","attempt_id":retry_id,"token":retry_token,"artifact_id":"contract-v2","source_revision":1,"kind":"contract","content":"GET /items returns checked cursor"}));
    d.call("swarm.report",json!({"run_id":run_id,"job_id":"interface","attempt_id":retry_id,"token":retry_token,"message_id":"result-2","type":"result","revision":1,"payload":{"artifact_ids":["contract-v2"]}}));
    d.call("swarm.decide",json!({"run_id":run_id,"generation":1,"revision":1,"job_id":"interface","decision":"accept","evidence":["contract-v2"]}));
    let jobs = d.call("swarm.jobs", json!({"id":run_id}));
    let consumer = jobs["jobs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|j| j["id"] == "consumer")
        .unwrap();
    assert_eq!(consumer["status"], "planned");
    d.call("swarm.attempt.confirm_exit",json!({"run_id":run_id,"job_id":"interface","attempt_id":retry_id,"generation":1,"revision":1}));
    let jobs = d.call("swarm.jobs", json!({"id":run_id}));
    let consumer = jobs["jobs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|j| j["id"] == "consumer")
        .unwrap();
    assert_eq!(consumer["status"], "ready");
    d.call("swarm.report",json!({"run_id":run_id,"job_id":"interface","attempt_id":retry_id,"token":retry_token,
        "message_id":"late-tail","type":"progress","revision":1,"payload":{"note":"tool log arrived after exit"}}));
}
