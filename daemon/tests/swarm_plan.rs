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

#[test]
fn acceptance_revalidates_present_revisioned_untampered_evidence() {
    let d=Daemon::start(&[]);
    let run=run_with_job(&d,"Evidence integrity","j");
    let attempt=d.call("swarm.attempt.register",json!({"run_id":run,"job_id":"j",
        "generation":1,"revision":1}));
    let artifact=json!({"run_id":run,"job_id":"j","attempt_id":attempt["id"],
        "token":attempt["token"],"artifact_id":"proof","source_revision":1,
        "kind":"finding","content":"reproduced against source revision 1"});
    d.call("swarm.artifact.put",artifact.clone());
    d.call("swarm.report",json!({"run_id":run,"job_id":"j",
        "attempt_id":attempt["id"],"token":attempt["token"],
        "message_id":"submitted-proof","type":"result","revision":1,
        "payload":{"artifact_ids":["proof"]}}));
    let decision=json!({"run_id":run,"generation":1,"revision":1,"job_id":"j",
        "decision":"accept","evidence":["proof"]});
    let db=rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    db.execute("DELETE FROM swarm_artifacts WHERE run_id=?1 AND id='proof'",[&run]).unwrap();
    assert!(d.try_call("swarm.decide",decision.clone()).unwrap_err().contains("missing artifact"));
    d.call("swarm.artifact.put",artifact);
    db.execute("UPDATE swarm_artifacts SET source_revision=0 WHERE run_id=?1 AND id='proof'",[&run]).unwrap();
    assert!(d.try_call("swarm.decide",decision.clone()).unwrap_err().contains("stale artifact"));
    db.execute("UPDATE swarm_artifacts SET source_revision=1,content='tampered' WHERE run_id=?1 AND id='proof'",[&run]).unwrap();
    assert!(d.try_call("swarm.decide",decision.clone()).unwrap_err().contains("artifact integrity"));
    db.execute("UPDATE swarm_artifacts SET content='reproduced against source revision 1' WHERE run_id=?1 AND id='proof'",[&run]).unwrap();
    assert_eq!(d.call("swarm.decide",decision)["status"],"accepted");
}

#[test]
fn revision_invalidates_affected_work_and_preserves_unrelated_acceptance() {
    let d = Daemon::start(&[]);
    let run = d.call("swarm.create", json!({"category":"Catalog revision","objective":"Change paging","allowed_targets":["system-codex"]}));
    let run_id = run["id"].as_str().unwrap();
    d.call(
        "swarm.plan",
        json!({"id":run_id,"generation":1,"revision":0,"jobs":[
            {"id":"interface","title":"Interface","acceptance":"original contract","deps":[]},
            {"id":"consumer","title":"Consumer","acceptance":"consumer test","deps":["interface"]},
            {"id":"unrelated","title":"Unrelated","acceptance":"separate check","deps":[]}
        ]}),
    );
    let finish = |job: &str, artifact: &str| {
        let attempt = d.call(
            "swarm.attempt.register",
            json!({"run_id":run_id,"generation":1,"revision":1,"job_id":job}),
        );
        let aid = attempt["id"].as_str().unwrap();
        let token = attempt["token"].as_str().unwrap();
        d.call("swarm.artifact.put",json!({"run_id":run_id,"job_id":job,"attempt_id":aid,"token":token,"artifact_id":artifact,"source_revision":1,"kind":"finding","content":"verified"}));
        d.call("swarm.report",json!({"run_id":run_id,"job_id":job,"attempt_id":aid,"token":token,"message_id":format!("result-{job}"),"type":"result","revision":1,"payload":{"artifact_ids":[artifact]}}));
        d.call("swarm.decide",json!({"run_id":run_id,"generation":1,"revision":1,"job_id":job,"decision":"accept","evidence":[artifact]}));
        d.call(
            "swarm.attempt.confirm_exit",
            json!({"run_id":run_id,"job_id":job,"attempt_id":aid,"generation":1,"revision":1}),
        );
    };
    finish("interface", "original-interface");
    finish("unrelated", "unrelated-proof");
    let consumer = d.call(
        "swarm.attempt.register",
        json!({"run_id":run_id,"generation":1,"revision":1,"job_id":"consumer"}),
    );
    let consumer_id = consumer["id"].as_str().unwrap();
    let consumer_token = consumer["token"].as_str().unwrap();
    let revised=d.call("swarm.revise",json!({"id":run_id,"generation":1,"expected_revision":1,"reason":"cursor contract changed","jobs":[
        {"id":"interface","title":"Interface","acceptance":"new cursor contract","deps":[]},
        {"id":"consumer","title":"Consumer","acceptance":"consumer test","deps":["interface"]},
        {"id":"unrelated","title":"Unrelated","acceptance":"separate check","deps":[]}
    ]}));
    assert_eq!(revised["revision"], 2);
    let jobs = d.call("swarm.jobs", json!({"id":run_id}));
    let find = |id: &str| {
        jobs["jobs"]
            .as_array()
            .unwrap()
            .iter()
            .find(|j| j["id"] == id)
            .unwrap()
            .clone()
    };
    assert_eq!(find("interface")["status"], "ready");
    assert_eq!(find("interface")["plan_revision"], 2);
    assert_eq!(find("consumer")["status"], "cancel_requested");
    assert_eq!(find("consumer")["plan_revision"], 2);
    assert_eq!(find("unrelated")["status"], "accepted");
    assert_eq!(find("unrelated")["plan_revision"], 1);
    let msg = d.call(
        "swarm.messages",
        json!({"run_id":run_id,"recipient":consumer_id}),
    );
    assert!(msg["messages"]
        .as_array()
        .unwrap()
        .iter()
        .any(|m| m["type"] == "redirect"));
    d.call("swarm.artifact.put",json!({"run_id":run_id,"job_id":"consumer","attempt_id":consumer_id,"token":consumer_token,"artifact_id":"old-consumer","source_revision":1,"kind":"finding","content":"old contract result"}));
    d.call("swarm.report",json!({"run_id":run_id,"job_id":"consumer","attempt_id":consumer_id,"token":consumer_token,"message_id":"late-old","type":"result","revision":1,"payload":{"artifact_ids":["old-consumer"]}}));
    assert!(d.try_call("swarm.decide",json!({"run_id":run_id,"generation":1,"revision":2,"job_id":"consumer","decision":"accept","evidence":["old-consumer"]})).unwrap_err().contains("stale"));
    d.call("swarm.attempt.confirm_exit",json!({"run_id":run_id,"job_id":"consumer","attempt_id":consumer_id,"generation":1,"revision":2}));
    let jobs = d.call("swarm.jobs", json!({"id":run_id}));
    let consumer_job = jobs["jobs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|j| j["id"] == "consumer")
        .unwrap();
    assert_eq!(consumer_job["status"], "planned");
}

#[test]
fn unchanged_ready_job_keeps_its_assignment_revision_after_another_job_changes() {
    let d = Daemon::start(&[]);
    let run=d.call("swarm.create",json!({"category":"Split work","objective":"Check two independent paths","allowed_targets":["system-codex"]}));
    let id = run["id"].as_str().unwrap();
    d.call(
        "swarm.plan",
        json!({"id":id,"generation":1,"revision":0,"jobs":[
            {"id":"a","title":"A","acceptance":"old A","deps":[]},
            {"id":"b","title":"B","acceptance":"B check","deps":[]}
        ]}),
    );
    d.call(
        "swarm.revise",
        json!({"id":id,"generation":1,"expected_revision":1,"reason":"A changed","jobs":[
            {"id":"a","title":"A","acceptance":"new A","deps":[]},
            {"id":"b","title":"B","acceptance":"B check","deps":[]}
        ]}),
    );
    let attempt = d.call(
        "swarm.attempt.register",
        json!({"run_id":id,"generation":1,"revision":2,"job_id":"b"}),
    );
    assert_eq!(attempt["revision"], 1);
    let aid = attempt["id"].as_str().unwrap();
    let token = attempt["token"].as_str().unwrap();
    d.call("swarm.artifact.put",json!({"run_id":id,"job_id":"b","attempt_id":aid,"token":token,"artifact_id":"b-proof","source_revision":1,"kind":"finding","content":"B is verified"}));
    d.call("swarm.report",json!({"run_id":id,"job_id":"b","attempt_id":aid,"token":token,"message_id":"b-result","type":"result","revision":1,"payload":{"artifact_ids":["b-proof"]}}));
    d.call("swarm.decide",json!({"run_id":id,"generation":1,"revision":2,"job_id":"b","decision":"accept","evidence":["b-proof"]}));
}
