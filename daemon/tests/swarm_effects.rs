mod common;

use common::*;
use serde_json::json;

#[test]
fn narrowing_scope_cannot_erase_an_uncertain_external_effect() {
    let d=Daemon::start(&[]);
    let made=d.call("swarm.create",json!({"category":"Effect scope",
        "objective":"Investigate grant","allowed_targets":["fixture-local"]}));
    let run=made["id"].as_str().unwrap();
    d.call("swarm.plan",json!({"id":run,"generation":1,"revision":0,"jobs":[
        {"id":"keep","title":"Keep","acceptance":"proof","deps":[]},
        {"id":"grant","title":"Grant","acceptance":"proof","deps":[]}
    ]}));
    let attempt=d.call("swarm.attempt.register",json!({"run_id":run,
        "generation":1,"revision":1,"job_id":"grant"}));
    let effect=d.call("swarm.effect.begin",json!({"run_id":run,"job_id":"grant",
        "attempt_id":attempt["id"],"token":attempt["token"],
        "effect_id":"grant-1","operation_id":"fixture:grant:1","revision":1}));
    assert_eq!(effect["outcome"],"unknown");
    let error=d.try_call("swarm.revise",json!({"id":run,"generation":1,
        "expected_revision":1,"reason":"Owner narrowed scope",
        "jobs":[{"id":"keep","title":"Keep","acceptance":"proof","deps":[]}]}))
        .unwrap_err();
    assert!(error.contains("unreconciled side effect"),"{error}");
    assert_eq!(d.call("swarm.get",json!({"id":run}))["revision"],1);
    assert_eq!(d.call("swarm.jobs",json!({"id":run}))["jobs"].as_array().unwrap().iter()
        .find(|job|job["id"]=="grant").unwrap()["status"],"reserved");
}

#[test]
fn lost_side_effect_ack_blocks_retry_until_outcome_is_reconciled() {
    let mut d = Daemon::start(&[]);
    let temp = tmp();
    let external = temp.path().join("ledgerpay.sqlite");
    let billing = rusqlite::Connection::open(&external).unwrap();
    billing
        .execute_batch(
            "CREATE TABLE entitlements(event_id TEXT PRIMARY KEY, grant_count INTEGER NOT NULL);",
        )
        .unwrap();
    let run = d.call(
        "swarm.create",
        json!({"category":"LedgerPay effect",
        "objective":"Audit duplicate entitlement delivery","allowed_targets":["fixture-local"]}),
    );
    let id = run["id"].as_str().unwrap();
    d.call("swarm.plan",json!({"id":id,"generation":1,"revision":0,"jobs":[
        {"id":"retry","title":"Test duplicate delivery","acceptance":"event evidence","deps":[]},
        {"id":"independent","title":"Test invalid signature","acceptance":"evidence","deps":[]}
    ]}));
    let attempt = d.call(
        "swarm.attempt.register",
        json!({"run_id":id,"generation":1,
        "revision":1,"job_id":"retry"}),
    );
    let intent = json!({"run_id":id,"job_id":"retry","attempt_id":attempt["id"],
        "token":attempt["token"],"effect_id":"evt-42-grant",
        "operation_id":"fixture:ledgerpay:evt-42:grant","revision":1,
        "fixture_drop_ack_after_commit":true});
    assert!(d
        .try_call("swarm.effect.begin", intent.clone())
        .unwrap_err()
        .contains("injected"));
    billing
        .execute(
            "INSERT INTO entitlements(event_id,grant_count) VALUES('evt-42',1)",
            [],
        )
        .unwrap();
    d.kill9();
    d.spawn();
    let mut replay = intent.clone();
    replay
        .as_object_mut()
        .unwrap()
        .remove("fixture_drop_ack_after_commit");
    let recovered = d.call("swarm.effect.begin", replay.clone());
    assert_eq!(recovered["outcome"], "unknown");
    assert_eq!(recovered["may_execute"], false);
    assert_eq!(recovered["duplicate"], true);
    assert!(d
        .try_call(
            "swarm.effect.begin",
            json!({"run_id":id,"job_id":"retry",
        "attempt_id":attempt["id"],"token":attempt["token"],
        "effect_id":"evt-42-grant","operation_id":"different-command","revision":1})
        )
        .is_err());
    assert!(d
        .try_call(
            "swarm.effect.begin",
            json!({"run_id":id,"job_id":"retry",
        "attempt_id":attempt["id"],"token":attempt["token"],
        "effect_id":"renamed-effect","operation_id":"fixture:ledgerpay:evt-42:grant",
        "revision":1})
        )
        .unwrap_err()
        .contains("already journaled"));

    d.call(
        "swarm.artifact.put",
        json!({"run_id":id,"job_id":"retry",
        "attempt_id":attempt["id"],"token":attempt["token"],
        "artifact_id":"uncertain-effect","source_revision":1,"kind":"finding",
        "content":"evt-42 delivery acknowledgement was lost"}),
    );
    d.call(
        "swarm.report",
        json!({"run_id":id,"job_id":"retry",
        "attempt_id":attempt["id"],"token":attempt["token"],
        "message_id":"uncertain-result","type":"result","revision":1,
        "payload":{"artifact_ids":["uncertain-effect"]}}),
    );
    assert!(d
        .try_call(
            "swarm.decide",
            json!({"run_id":id,"generation":1,"revision":1,
        "job_id":"retry","decision":"accept","evidence":["uncertain-effect"]})
        )
        .unwrap_err()
        .contains("unreconciled side effect"));
    d.call(
        "swarm.decide",
        json!({"run_id":id,"generation":1,"revision":1,
        "job_id":"retry","decision":"reject","evidence":["uncertain-effect"]}),
    );
    d.call(
        "swarm.attempt.confirm_exit",
        json!({"run_id":id,"job_id":"retry",
        "attempt_id":attempt["id"],"generation":1,"revision":1}),
    );
    let jobs = d.call("swarm.jobs", json!({"id":id}))["jobs"]
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(
        jobs.iter().find(|j| j["id"] == "retry").unwrap()["status"],
        "blocked"
    );
    assert_eq!(
        jobs.iter().find(|j| j["id"] == "independent").unwrap()["status"],
        "ready"
    );
    assert!(d
        .try_call(
            "swarm.attempt.register",
            json!({"run_id":id,"generation":1,
        "revision":1,"job_id":"retry"})
        )
        .is_err());
    // Independent work continues while the uncertain job is blocked (SWARM-58).
    let independent = d.call("swarm.attempt.register", json!({"run_id":id,"generation":1,
        "revision":1,"job_id":"independent"}));
    d.call("swarm.artifact.put", json!({"run_id":id,"job_id":"independent",
        "attempt_id":independent["id"],"token":independent["token"],"artifact_id":"signature-check",
        "source_revision":1,"kind":"finding","content":"invalid signature rejected with 401"}));
    d.call("swarm.report", json!({"run_id":id,"job_id":"independent",
        "attempt_id":independent["id"],"token":independent["token"],"message_id":"independent-result",
        "type":"result","revision":1,"payload":{"artifact_ids":["signature-check"],"audit_outcome":"negative"}}));
    assert_eq!(d.call("swarm.decide", json!({"run_id":id,"generation":1,"revision":1,
        "job_id":"independent","decision":"accept","evidence":["signature-check"]}))["status"], "accepted");
    d.call("swarm.attempt.confirm_exit", json!({"run_id":id,"job_id":"independent",
        "attempt_id":independent["id"],"generation":1,"revision":1}));

    let counter: i64 = billing
        .query_row(
            "SELECT grant_count FROM entitlements WHERE event_id='evt-42'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(counter, 1);
    d.call(
        "swarm.artifact.put",
        json!({"run_id":id,"job_id":"retry",
        "attempt_id":attempt["id"],"token":attempt["token"],
        "artifact_id":"effect-probe","source_revision":1,"kind":"effect_probe",
        "content":format!("evt-42 grant_count={counter}")}),
    );
    assert!(d
        .try_call(
            "swarm.effect.reconcile",
            json!({"run_id":id,"generation":1,
        "revision":1,"job_id":"retry","effect_id":"evt-42-grant",
        "outcome":"absent","proof_artifact_id":"effect-probe"})
        )
        .is_err());
    assert!(d
        .try_call(
            "swarm.effect.reconcile",
            json!({"run_id":id,"generation":1,
        "revision":1,"job_id":"retry","effect_id":"evt-42-grant",
        "outcome":"applied","proof_artifact_id":"uncertain-effect"})
        )
        .is_err());
    assert!(d
        .try_call(
            "swarm.effect.reconcile",
            json!({"run_id":id,"generation":1,
        "revision":1,"job_id":"retry","effect_id":"evt-42-grant",
        "outcome":"applied","proof_artifact_id":"effect-probe"})
        )
        .unwrap_err()
        .contains("not submitted"));
    d.call(
        "swarm.report",
        json!({"run_id":id,"job_id":"retry",
        "attempt_id":attempt["id"],"token":attempt["token"],
        "message_id":"effect-probe-report","type":"discovery","revision":1,
        "payload":{"artifact_ids":["effect-probe"]}}),
    );
    let reconciled = d.call(
        "swarm.effect.reconcile",
        json!({"run_id":id,"generation":1,
        "revision":1,"job_id":"retry","effect_id":"evt-42-grant",
        "outcome":"applied","proof_artifact_id":"effect-probe"}),
    );
    assert_eq!(reconciled["outcome"], "applied");
    assert_eq!(reconciled["may_execute"], false);
    assert_eq!(d.call("swarm.effect.begin", replay)["may_execute"], false);
    assert_eq!(
        d.call("swarm.jobs", json!({"id":id}))["jobs"][1]["status"],
        "blocked"
    );
    assert_eq!(
        billing
            .query_row(
                "SELECT grant_count FROM entitlements WHERE event_id='evt-42'",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
        1
    );
    d.call("swarm.revise",json!({"id":id,"generation":1,"expected_revision":1,
        "reason":"Review the uncertain billing effect","jobs":[
            {"id":"retry","title":"Review duplicate delivery","acceptance":"event evidence","deps":[]},
            {"id":"independent","title":"Test invalid signature","acceptance":"evidence","deps":[]}
        ]}));
    let jobs = d.call("swarm.jobs", json!({"id":id}))["jobs"]
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(
        jobs.iter().find(|j| j["id"] == "retry").unwrap()["status"],
        "blocked"
    );
    assert!(d
        .try_call(
            "swarm.attempt.register",
            json!({"run_id":id,"generation":1,
        "revision":2,"job_id":"retry"})
        )
        .is_err());
}

#[test]
fn revision_during_an_uncertain_effect_cannot_requeue_after_worker_exit() {
    let d = Daemon::start(&[]);
    let run = d.call(
        "swarm.create",
        json!({"category":"Live effect revision",
        "objective":"Inspect a side effect","allowed_targets":["fixture-local"]}),
    );
    let id = run["id"].as_str().unwrap();
    d.call(
        "swarm.plan",
        json!({"id":id,"generation":1,"revision":0,"jobs":[
            {"id":"mutate","title":"Inspect event","acceptance":"evidence","deps":[]}
        ]}),
    );
    let attempt = d.call(
        "swarm.attempt.register",
        json!({"run_id":id,"generation":1,
        "revision":1,"job_id":"mutate"}),
    );
    d.call("swarm.claim", json!({"run_id":id,"job_id":"mutate",
        "resource":"ledgerpay:evt-43","mode":"write","generation":1,"revision":1}));
    assert_eq!(
        d.call(
            "swarm.effect.begin",
            json!({"run_id":id,"job_id":"mutate",
        "attempt_id":attempt["id"],"token":attempt["token"],
        "effect_id":"evt-43-grant","operation_id":"fixture:ledgerpay:evt-43:grant",
        "revision":1})
        )["may_execute"],
        true
    );
    d.call(
        "swarm.revise",
        json!({"id":id,"generation":1,"expected_revision":1,
        "reason":"Change the event check","jobs":[
            {"id":"mutate","title":"Inspect event again","acceptance":"evidence","deps":[]}
        ]}),
    );
    assert_eq!(
        d.call("swarm.jobs", json!({"id":id}))["jobs"][0]["status"],
        "cancel_requested"
    );
    d.call(
        "swarm.attempt.confirm_exit",
        json!({"run_id":id,"job_id":"mutate",
        "attempt_id":attempt["id"],"generation":1,"revision":2}),
    );
    assert_eq!(
        d.call("swarm.jobs", json!({"id":id}))["jobs"][0]["status"],
        "blocked"
    );
    assert!(d
        .try_call(
            "swarm.attempt.register",
            json!({"run_id":id,"generation":1,
        "revision":2,"job_id":"mutate"})
        )
        .is_err());
    let other = d.call("swarm.create", json!({"category":"Other effect revision",
        "objective":"Inspect evt-43","allowed_targets":["fixture-local"]}));
    let other_id = other["id"].as_str().unwrap();
    d.call("swarm.plan", json!({"id":other_id,"generation":1,"revision":0,"jobs":[
        {"id":"inspect","title":"Inspect event","acceptance":"evidence","deps":[]}
    ]}));
    assert!(d.try_call("swarm.claim", json!({"run_id":other_id,"job_id":"inspect",
        "resource":"ledgerpay:evt-43","mode":"write","generation":1,"revision":1}))
        .unwrap_err().contains("claim conflict"));
}

#[test]
fn stopping_an_uncertain_effect_keeps_its_resource_quarantined() {
    let d = Daemon::start(&[]);
    let run = d.call("swarm.create", json!({"category":"LedgerPay stop",
        "objective":"Inspect entitlement state","allowed_targets":["fixture-local"]}));
    let id = run["id"].as_str().unwrap();
    d.call("swarm.plan", json!({"id":id,"generation":1,"revision":0,"jobs":[
        {"id":"grant","title":"Grant entitlement","acceptance":"evidence","deps":[]}
    ]}));
    let attempt = d.call("swarm.attempt.register", json!({"run_id":id,
        "generation":1,"revision":1,"job_id":"grant"}));
    d.call("swarm.claim", json!({"run_id":id,"job_id":"grant",
        "resource":"ledgerpay:entitlements","mode":"write","generation":1,"revision":1}));
    d.call("swarm.effect.begin", json!({"run_id":id,"job_id":"grant",
        "attempt_id":attempt["id"],"token":attempt["token"],
        "effect_id":"evt-44-grant","operation_id":"fixture:ledgerpay:evt-44:grant",
        "revision":1}));
    d.call("swarm.stop", json!({"run_id":id,"generation":1,"revision":1}));
    d.call("swarm.attempt.confirm_exit", json!({"run_id":id,"job_id":"grant",
        "attempt_id":attempt["id"],"generation":1,"revision":1}));
    assert_eq!(d.call("swarm.jobs", json!({"id":id}))["jobs"][0]["status"], "blocked");

    let other = d.call("swarm.create", json!({"category":"LedgerPay followup",
        "objective":"Inspect same entitlement table","allowed_targets":["fixture-local"]}));
    let other_id = other["id"].as_str().unwrap();
    d.call("swarm.plan", json!({"id":other_id,"generation":1,"revision":0,"jobs":[
        {"id":"inspect","title":"Inspect entitlements","acceptance":"evidence","deps":[]}
    ]}));
    assert!(d.try_call("swarm.claim", json!({"run_id":other_id,"job_id":"inspect",
        "resource":"ledgerpay:entitlements","mode":"write","generation":1,"revision":1}))
        .unwrap_err().contains("claim conflict"));
}
