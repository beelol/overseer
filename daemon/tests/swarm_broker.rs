mod common;

use common::*;
use serde_json::json;

fn planned(d: &Daemon) -> (String, String, String) {
    let run = d.call(
        "swarm.create",
        json!({"category":"Backend","objective":"Audit routes","allowed_targets":["system-codex"]}),
    );
    let id = run["id"].as_str().unwrap().to_owned();
    d.call(
        "swarm.plan",
        json!({"id":id,"generation":1,"revision":0,"jobs":[
            {"id":"routes","title":"Inspect routes","acceptance":"route table","deps":[]}
        ]}),
    );
    let attempt = d.call(
        "swarm.attempt.register",
        json!({"run_id":id,"job_id":"routes","generation":1,"revision":1}),
    );
    (
        id,
        attempt["id"].as_str().unwrap().to_owned(),
        attempt["token"].as_str().unwrap().to_owned(),
    )
}

#[test]
fn unfinished_runtime_transitions_are_disabled_without_fixture_opt_in() {
    let d=Daemon::start(&[("OVERSEER_SWARM_FIXTURE_API","0")]);
    let create=json!({"category":"Safe default","objective":"Audit","allowed_targets":["system-codex"]});
    // No Swarm run exists outside the fixture API while its setting is off (AC-204).
    assert!(d.try_call("swarm.create",create.clone()).unwrap_err().contains("swarm.native_director"));
    d.call("swarm.native_director.set",json!({"enabled":true}));
    let run=d.call("swarm.create",create);
    let id=run["id"].as_str().unwrap();
    let plan=json!({"id":id,"generation":1,"revision":0,"jobs":[{"id":"audit","title":"Audit","acceptance":"report","deps":[]}]});
    assert!(d.try_call("swarm.plan",plan).unwrap_err().contains("fixture-only"));
    for method in ["swarm.messages","swarm.direct","swarm.claim","swarm.decide","swarm.revise","swarm.director.recover"] {
        assert!(d.try_call(method,json!({"run_id":id,"recipient":"director"})).unwrap_err().contains("fixture-only"),"{method}");
    }
    assert!(d.try_call("swarm.ack",json!({"run_id":id,"recipient":"director"})).unwrap_err().contains("fixture-only"));
    let error=d.try_call("swarm.attempt.register",json!({"run_id":id,"job_id":"audit","generation":1,"revision":1})).unwrap_err();
    assert!(error.contains("fixture-only"));
    let error=d.try_call("swarm.attempt.confirm_exit",json!({"run_id":id,"job_id":"audit","attempt_id":"fake","generation":1,"revision":1})).unwrap_err();
    assert!(error.contains("fixture-only"));
}

#[test]
fn result_submission_is_durable_with_awaiting_review_state() {
    let d=Daemon::start(&[]);
    let (run_id,attempt_id,token)=planned(&d);
    d.call("swarm.report",json!({"run_id":run_id,"job_id":"routes","attempt_id":attempt_id,"token":token,
        "message_id":"submitted","type":"result","revision":1,"payload":{"artifact_ids":[]}}));
    let jobs=d.call("swarm.jobs",json!({"id":run_id}));
    assert_eq!(jobs["jobs"][0]["status"],"submitted");
}

#[test]
fn audit_coverage_distinguishes_negative_environment_and_defect_results() {
    let mut d = Daemon::start(&[]);
    let run = d.call(
        "swarm.create",
        json!({"category":"LedgerPay audit",
        "objective":"Check signature, queue and duplicate entitlement behavior",
        "allowed_targets":["fixture"]}),
    );
    let run_id = run["id"].as_str().unwrap();
    d.call(
        "swarm.plan",
        json!({"id":run_id,"generation":1,"revision":0,"jobs":[
            {"id":"negative","title":"Invalid signatures","acceptance":"Rejection trace","deps":[]},
            {"id":"environment","title":"Queue retries","acceptance":"Queue trace","deps":[]},
            {"id":"defect","title":"Duplicate grants","acceptance":"Reproducer","deps":[]}
        ]}),
    );
    for (job, outcome, kind) in [
        ("negative", "negative", "finding"),
        ("environment", "environment_failure", "log"),
        ("defect", "confirmed_defect", "reproduction"),
    ] {
        let attempt = d.call(
            "swarm.attempt.register",
            json!({"run_id":run_id,
            "job_id":job,"generation":1,"revision":1}),
        );
        let artifact = format!("proof-{job}");
        d.call(
            "swarm.artifact.put",
            json!({"run_id":run_id,"job_id":job,
            "attempt_id":attempt["id"],"token":attempt["token"],
            "artifact_id":artifact,"kind":kind,"content":format!("fixture evidence for {job}"),
            "source_revision":1}),
        );
        let payload = if job == "environment" {
            json!({"audit_outcome":outcome,"artifact_ids":[artifact],
                "unavailable_resource":"queue"})
        } else {
            json!({"audit_outcome":outcome,"artifact_ids":[artifact]})
        };
        d.call(
            "swarm.report",
            json!({"run_id":run_id,"job_id":job,
            "attempt_id":attempt["id"],"token":attempt["token"],
            "message_id":format!("result-{job}"),"type":"result","revision":1,
            "payload":payload}),
        );
        let decision = json!({"run_id":run_id,"generation":1,"revision":1,"job_id":job,
            "decision":"accept","evidence":[artifact]});
        if job == "environment" {
            assert!(d
                .try_call("swarm.decide", decision)
                .unwrap_err()
                .contains("environment"));
        } else {
            assert_eq!(d.call("swarm.decide", decision)["status"], "accepted");
        }
    }
    let coverage = d.call("swarm.coverage", json!({"run_id":run_id}));
    let rows = coverage["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 3);
    assert_eq!(
        rows.iter().find(|r| r["job_id"] == "negative").unwrap()["coverage_state"],
        "checked_negative"
    );
    let blocked = rows.iter().find(|r| r["job_id"] == "environment").unwrap();
    assert_eq!(blocked["coverage_state"], "environment_blocked");
    assert_eq!(blocked["unavailable_resource"], "queue");
    assert_ne!(blocked["job_status"], "accepted");
    assert_eq!(
        rows.iter().find(|r| r["job_id"] == "defect").unwrap()["coverage_state"],
        "confirmed_application_defect"
    );
    d.kill9();
    d.spawn();
    assert_eq!(d.call("swarm.coverage", json!({"run_id":run_id})), coverage);
}

#[test]
fn reported_defect_needs_reproducer_evidence_before_confirmation() {
    let d = Daemon::start(&[]);
    let (run_id, attempt_id, token) = planned(&d);
    d.call(
        "swarm.artifact.put",
        json!({"run_id":run_id,"job_id":"routes",
        "attempt_id":attempt_id,"token":token,"artifact_id":"claim",
        "kind":"finding","content":"The worker suspects an authorization defect",
        "source_revision":1}),
    );
    d.call(
        "swarm.report",
        json!({"run_id":run_id,"job_id":"routes",
        "attempt_id":attempt_id,"token":token,"message_id":"claim-result",
        "type":"result","revision":1,"payload":{"audit_outcome":"confirmed_defect",
            "artifact_ids":["claim"]}}),
    );
    let decision = json!({"run_id":run_id,"generation":1,"revision":1,
        "job_id":"routes","decision":"accept","evidence":["claim"]});
    assert!(d
        .try_call("swarm.decide", decision)
        .unwrap_err()
        .contains("reproduction"));
    assert_eq!(
        d.call("swarm.coverage", json!({"run_id":run_id}))["rows"][0]["coverage_state"],
        "defect_awaiting_review"
    );
    d.call(
        "swarm.artifact.put",
        json!({"run_id":run_id,"job_id":"routes",
        "attempt_id":attempt_id,"token":token,"artifact_id":"repro",
        "kind":"reproduction","content":"Request returned 200 and changed another tenant row",
        "source_revision":1}),
    );
    d.call(
        "swarm.report",
        json!({"run_id":run_id,"job_id":"routes",
        "attempt_id":attempt_id,"token":token,"message_id":"repro-result",
        "type":"result","revision":1,"payload":{"audit_outcome":"confirmed_defect",
            "artifact_ids":["claim","repro"]}}),
    );
    assert_eq!(
        d.call(
            "swarm.decide",
            json!({"run_id":run_id,"generation":1,"revision":1,
        "job_id":"routes","decision":"accept","evidence":["claim","repro"]})
        )["status"],
        "accepted"
    );
    assert_eq!(
        d.call("swarm.coverage", json!({"run_id":run_id}))["rows"][0]["coverage_state"],
        "confirmed_application_defect"
    );
}

#[test]
fn late_environment_failure_cannot_be_hidden_by_an_earlier_acceptance() {
    let d = Daemon::start(&[]);
    let (run_id, attempt_id, token) = planned(&d);
    // This fixture registers an attempt directly, bypassing admission's running transition.
    rusqlite::Connection::open(d.home.path().join("overseer.sqlite"))
        .unwrap()
        .execute(
            "UPDATE swarm_runs SET status='running' WHERE id=?1",
            rusqlite::params![run_id],
        )
        .unwrap();
    d.call(
        "swarm.artifact.put",
        json!({"run_id":run_id,"job_id":"routes",
        "attempt_id":attempt_id,"token":token,"artifact_id":"checked",
        "kind":"finding","content":"Routes checked with fixture database",
        "source_revision":1}),
    );
    d.call(
        "swarm.report",
        json!({"run_id":run_id,"job_id":"routes",
        "attempt_id":attempt_id,"token":token,"message_id":"initial-result",
        "type":"result","revision":1,"payload":{"audit_outcome":"negative",
            "artifact_ids":["checked"]}}),
    );
    d.call(
        "swarm.decide",
        json!({"run_id":run_id,"generation":1,"revision":1,
        "job_id":"routes","decision":"accept","evidence":["checked"]}),
    );
    d.call(
        "swarm.attempt.confirm_exit",
        json!({"run_id":run_id,"job_id":"routes",
        "attempt_id":attempt_id,"generation":1,"revision":1}),
    );
    d.call(
        "swarm.report",
        json!({"run_id":run_id,"job_id":"routes",
        "attempt_id":attempt_id,"token":token,"message_id":"late-queue-failure",
        "type":"result","revision":1,"payload":{"audit_outcome":"environment_failure",
            "artifact_ids":["checked"],"unavailable_resource":"queue"}}),
    );
    d.call(
        "swarm.report",
        json!({"run_id":run_id,"job_id":"routes",
        "attempt_id":attempt_id,"token":token,"message_id":"later-all-clear",
        "type":"result","revision":1,"payload":{"audit_outcome":"negative",
            "artifact_ids":["checked"]}}),
    );
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    let batch = d.call(
        "swarm.director.claim_batch",
        json!({"run_id":run_id,
        "generation":1,"revision":1,"now_ms":now+6000}),
    );
    assert_eq!(batch["status"], "claimed", "{batch}");
    d.call(
        "swarm.director.complete_batch",
        json!({"run_id":run_id,"generation":1,
        "turn_id":batch["turn_id"],"token":batch["token"],"outcome":"no_progress"}),
    );
    assert_eq!(
        d.call("swarm.get", json!({"id":run_id}))["status"],
        "running"
    );
    assert_eq!(
        d.call("swarm.coverage", json!({"run_id":run_id}))["rows"][0]["coverage_state"],
        "environment_blocked"
    );
    let error = d
        .try_call(
            "swarm.complete",
            json!({"run_id":run_id,"generation":1,
        "revision":1,"request_id":"complete-after-queue-failure",
        "summary":"Routes checked","verification":"Fixture route checks",
        "checks":[{"job_id":"routes","outcome":"passed","evidence":["checked"]}]}),
        )
        .unwrap_err();
    assert!(error.contains("environment failure"), "{error}");
}

#[test]
fn late_result_invalidates_the_accepted_review_before_completion() {
    let mut d = Daemon::start(&[]);
    let (run_id, attempt_id, token) = planned(&d);
    rusqlite::Connection::open(d.home.path().join("overseer.sqlite"))
        .unwrap()
        .execute(
            "UPDATE swarm_runs SET status='running' WHERE id=?1",
            rusqlite::params![run_id],
        )
        .unwrap();
    d.call("swarm.artifact.put", json!({"run_id":run_id,"job_id":"routes",
        "attempt_id":attempt_id,"token":token,"artifact_id":"checked",
        "kind":"finding","content":"The route rejected cross-tenant access",
        "source_revision":1}));
    let original_result = json!({"run_id":run_id,"job_id":"routes",
        "attempt_id":attempt_id,"token":token,"message_id":"negative-result",
        "type":"result","revision":1,"payload":{"audit_outcome":"negative",
            "artifact_ids":["checked"]}});
    d.call("swarm.report", original_result.clone());
    d.call("swarm.decide", json!({"run_id":run_id,"generation":1,"revision":1,
        "job_id":"routes","decision":"accept","evidence":["checked"]}));
    d.call("swarm.attempt.confirm_exit", json!({"run_id":run_id,"job_id":"routes",
        "attempt_id":attempt_id,"generation":1,"revision":1}));
    d.kill9();
    d.spawn();
    assert_eq!(d.call("swarm.report", original_result)["duplicate"], true);
    assert_eq!(d.call("swarm.coverage", json!({"run_id":run_id}))["rows"][0]["coverage_state"],
        "checked_negative");
    d.call("swarm.report", json!({"run_id":run_id,"job_id":"routes",
        "attempt_id":attempt_id,"token":token,"message_id":"late-defect-result",
        "type":"result","revision":1,"payload":{"audit_outcome":"confirmed_defect",
            "artifact_ids":["checked"]}}));
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64;
    let batch = d.call("swarm.director.claim_batch", json!({"run_id":run_id,
        "generation":1,"revision":1,"now_ms":now+6000}));
    d.call("swarm.director.complete_batch", json!({"run_id":run_id,"generation":1,
        "turn_id":batch["turn_id"],"token":batch["token"],"outcome":"no_progress"}));
    assert_eq!(d.call("swarm.get", json!({"id":run_id}))["status"], "running");
    assert_eq!(d.call("swarm.coverage", json!({"run_id":run_id}))["rows"][0]["coverage_state"],
        "review_stale");
    let error = d.try_call("swarm.complete", json!({"run_id":run_id,"generation":1,
        "revision":1,"request_id":"complete-after-new-result",
        "summary":"Routes checked","verification":"Fixture route checks",
        "checks":[{"job_id":"routes","outcome":"passed","evidence":["checked"]}]}))
        .unwrap_err();
    assert!(error.contains("new result after review"), "{error}");
}

#[test]
fn report_is_durable_before_ack_and_replay_is_idempotent() {
    let mut d = Daemon::start(&[]);
    let (run_id, attempt_id, token) = planned(&d);
    let report = json!({"run_id":run_id,"job_id":"routes","attempt_id":attempt_id,"token":token,
        "message_id":"discovery-1","type":"discovery","revision":1,"payload":{"finding":"missing tenant filter"}});
    let receipt = d.call("swarm.report", report.clone());
    assert_eq!(receipt["duplicate"], false);
    assert_eq!(d.call("swarm.report", report.clone())["duplicate"], true);
    assert!(d.try_call("swarm.report",json!({"run_id":run_id,"job_id":"routes","attempt_id":attempt_id,"token":"wrong","message_id":"bad","type":"discovery","revision":1,"payload":{}})).is_err());
    assert!(d.try_call("swarm.report",json!({"run_id":run_id,"job_id":"routes","attempt_id":attempt_id,"token":token,"message_id":"spoof","type":"redirect","revision":1,"payload":{}})).is_err());
    assert!(d.try_call("swarm.report",json!({"run_id":run_id,"job_id":"routes","attempt_id":attempt_id,"token":token,"message_id":"huge","type":"progress","revision":1,"payload":{"text":"x".repeat(33_000)}})).is_err());
    d.kill9();
    d.spawn();
    let inbox = d.call(
        "swarm.messages",
        json!({"run_id":run_id,"recipient":"director","limit":20}),
    );
    assert_eq!(inbox["messages"].as_array().unwrap().len(), 1);
    assert_eq!(inbox["messages"][0]["message_id"], "discovery-1");
    assert_eq!(d.call("swarm.report", report)["duplicate"], true);
    // One envelope in the daemon's broker ledger, received once across the restart.
    let envelopes = d.call("broker.envelopes", json!({"origin":"swarm","scope":run_id}))["envelopes"].clone();
    assert_eq!(envelopes.as_array().unwrap().len(), 1, "{envelopes}");
    assert_eq!(envelopes[0]["id"], format!("swarm/{run_id}/discovery-1"));
    assert_eq!(envelopes[0]["phase"], "queued");
}

#[test]
fn directive_delivery_and_application_are_distinct() {
    let d = Daemon::start(&[]);
    let (run_id, attempt_id, token) = planned(&d);
    let sent=d.call("swarm.direct",json!({"run_id":run_id,"job_id":"routes","attempt_id":attempt_id,
        "message_id":"redirect-2","generation":1,"revision":1,"type":"redirect","payload":{"focus":"pagination"}}));
    assert_eq!(sent["phase"], "queued");
    assert!(d.try_call("swarm.messages",json!({"run_id":run_id,
        "recipient":attempt_id})).is_err(),"an attempt id alone must not reveal directives");
    assert!(d.try_call("swarm.messages",json!({"run_id":run_id,
        "recipient":attempt_id,"token":"wrong"})).is_err());
    let inbox = d.call(
        "swarm.messages",
        json!({"run_id":run_id,"recipient":attempt_id,"token":token}),
    );
    assert_eq!(inbox["messages"].as_array().unwrap().len(), 1);
    let delivered=d.call("swarm.ack",json!({"run_id":run_id,"message_id":"redirect-2","recipient":attempt_id,"token":token,"phase":"delivered","revision":1}));
    assert_eq!(delivered["phase"], "delivered");
    let ledger=d.call("broker.envelopes",json!({"origin":"swarm","scope":run_id}))["envelopes"].clone();
    let row=ledger.as_array().unwrap().iter().find(|e| e["message_id"]=="redirect-2").unwrap().clone();
    assert_eq!((row["phase"].as_str(),row["applied_ms"].is_null()),(Some("delivered"),true),"{row}");
    let applied=d.call("swarm.ack",json!({"run_id":run_id,"message_id":"redirect-2","recipient":attempt_id,"token":token,"phase":"applied","revision":1}));
    assert_eq!(applied["phase"], "applied");
    assert_eq!(d.call("swarm.redirect.persist_due",json!({"now_ms":i64::MAX-1}))["timed_out"],0);
    let replay=d.call("swarm.direct",json!({"run_id":run_id,"job_id":"routes","attempt_id":attempt_id,
        "message_id":"redirect-2","generation":1,"revision":1,"type":"redirect","payload":{"focus":"pagination"}}));
    assert_eq!(replay["phase"], "applied");
    assert_eq!(replay["duplicate"], true);
    // SWARM-60: the daemon's one broker ledger has this directive once, under a
    // stable id, with delivery and application kept apart.
    let envelopes=d.call("broker.envelopes",json!({"origin":"swarm","scope":run_id}))["envelopes"].clone();
    let mine: Vec<&serde_json::Value>=envelopes.as_array().unwrap().iter()
        .filter(|e| e["message_id"]=="redirect-2").collect();
    assert_eq!(mine.len(),1,"{envelopes}");
    assert_eq!(mine[0]["id"],format!("swarm/{run_id}/redirect-2"));
    assert_eq!((mine[0]["sender"].as_str(),mine[0]["recipient"].as_str(),mine[0]["phase"].as_str()),
        (Some("director"),Some(attempt_id.as_str()),Some("applied")));
    assert!(mine[0]["delivered_ms"].as_i64().unwrap()<=mine[0]["applied_ms"].as_i64().unwrap());
    assert!(d.try_call("swarm.ack",json!({"run_id":run_id,"message_id":"redirect-2","recipient":attempt_id,"token":token,"phase":"applied","revision":0})).is_err());
    assert!(d.try_call("swarm.direct",json!({"run_id":run_id,"job_id":"routes","attempt_id":attempt_id,"message_id":"stale","generation":0,"revision":1,"type":"redirect","payload":{}})).is_err());
}

#[test]
fn finished_attempt_cannot_receive_a_new_directive_but_can_replay_one() {
    let mut d = Daemon::start(&[]);
    let (run, attempt, token) = planned(&d);
    let prior = json!({"run_id":run,"job_id":"routes","attempt_id":attempt,
        "message_id":"before-exit","generation":1,"revision":1,
        "type":"advisory","payload":{"focus":"check the route"}});
    let sent = d.call("swarm.direct", prior.clone());
    assert_eq!(sent["phase"], "queued");
    d.call("swarm.artifact.put",json!({"run_id":run,"job_id":"routes",
        "attempt_id":attempt,"token":token,"artifact_id":"route-proof",
        "source_revision":1,"kind":"finding","content":"route evidence"}));
    d.call("swarm.report",json!({"run_id":run,"job_id":"routes",
        "attempt_id":attempt,"token":token,"message_id":"result-before-exit",
        "type":"result","revision":1,"payload":{"artifact_ids":["route-proof"]}}));
    d.call("swarm.decide",json!({"run_id":run,"generation":1,"revision":1,
        "job_id":"routes","decision":"reject","evidence":["route-proof"]}));
    d.call("swarm.attempt.confirm_exit",json!({"run_id":run,"job_id":"routes",
        "attempt_id":attempt,"generation":1,"revision":1}));
    d.kill9();
    d.spawn();

    assert_eq!(d.call("swarm.direct",prior)["duplicate"],true);
    let error = d.try_call("swarm.direct",json!({"run_id":run,"job_id":"routes",
        "attempt_id":attempt,"message_id":"after-exit","generation":1,
        "revision":1,"type":"redirect","payload":{"focus":"new work"}}))
        .unwrap_err();
    assert!(error.contains("finished attempt"),"{error}");
    let inbox = d.call("swarm.messages",json!({"run_id":run,"recipient":attempt,
        "token":token}));
    assert_eq!(inbox["messages"].as_array().unwrap().len(),1);
}

#[test]
fn acceptance_waits_for_directive_application_before_unlocking_dependents() {
    for kind in ["redirect","advisory","retract"] {
        let mut d = Daemon::start(&[]);
        let made = d.call("swarm.create",json!({"category":"Applied assignment",
            "objective":"Audit the route before its dependent review",
            "allowed_targets":["system-codex"]}));
        let run = made["id"].as_str().unwrap();
        d.call("swarm.plan",json!({"id":run,"generation":1,"revision":0,"jobs":[
            {"id":"routes","title":"Audit route","acceptance":"route proof"},
            {"id":"review","title":"Review route","acceptance":"review proof","deps":["routes"]}
        ]}));
        let attempt = d.call("swarm.attempt.register",json!({"run_id":run,
            "job_id":"routes","generation":1,"revision":1}));
        d.call("swarm.direct",json!({"run_id":run,"job_id":"routes",
            "attempt_id":attempt["id"],"message_id":"focus-change",
            "generation":1,"revision":1,"type":kind,"payload":{"focus":"check the revised boundary"}}));
        d.call("swarm.artifact.put",json!({"run_id":run,"job_id":"routes",
            "attempt_id":attempt["id"],"token":attempt["token"],
            "artifact_id":"route-proof","source_revision":1,"kind":"finding","content":"route evidence"}));
        d.call("swarm.report",json!({"run_id":run,"job_id":"routes",
            "attempt_id":attempt["id"],"token":attempt["token"],"message_id":"route-result",
            "type":"result","revision":1,"payload":{"artifact_ids":["route-proof"]}}));
        let decision = json!({"run_id":run,"generation":1,"revision":1,
            "job_id":"routes","decision":"accept","evidence":["route-proof"]});
        for phase in ["queued","delivered"] {
            if phase == "delivered" {
                d.call("swarm.ack",json!({"run_id":run,"message_id":"focus-change",
                    "recipient":attempt["id"],"token":attempt["token"],
                    "revision":1,"phase":"delivered"}));
                d.kill9();
                d.spawn();
            }
            let error = d.try_call("swarm.decide",decision.clone()).unwrap_err();
            assert!(error.contains("unapplied directive"),"{phase}: {error}");
            let jobs = d.call("swarm.jobs",json!({"id":run}));
            assert_eq!(jobs["jobs"].as_array().unwrap().iter()
                .find(|job| job["id"] == "review").unwrap()["status"],"planned");
        }
        d.call("swarm.ack",json!({"run_id":run,"message_id":"focus-change",
            "recipient":attempt["id"],"token":attempt["token"],"revision":1,"phase":"applied"}));
        assert_eq!(d.call("swarm.decide",decision)["status"],"accepted");
        d.call("swarm.attempt.confirm_exit",json!({"run_id":run,"job_id":"routes",
            "attempt_id":attempt["id"],"generation":1,"revision":1}));
        let jobs = d.call("swarm.jobs",json!({"id":run}));
        assert_eq!(jobs["jobs"].as_array().unwrap().iter()
            .find(|job| job["id"] == "review").unwrap()["status"],"ready");
    }
}

#[test]
fn completion_does_not_hide_a_directive_sent_after_review() {
    for applied in [false,true] {
        let d = Daemon::start(&[]);
        let (run,attempt,token) = planned(&d);
        d.call("swarm.artifact.put",json!({"run_id":run,"job_id":"routes",
            "attempt_id":attempt,"token":token,"artifact_id":"proof",
            "source_revision":1,"kind":"finding","content":"route evidence"}));
        d.call("swarm.report",json!({"run_id":run,"job_id":"routes",
            "attempt_id":attempt,"token":token,"message_id":"result",
            "type":"result","revision":1,"payload":{"artifact_ids":["proof"]}}));
        d.call("swarm.decide",json!({"run_id":run,"generation":1,"revision":1,
            "job_id":"routes","decision":"accept","evidence":["proof"]}));
        d.call("swarm.direct",json!({"run_id":run,"job_id":"routes",
            "attempt_id":attempt,"message_id":"after-review","generation":1,
            "revision":1,"type":"advisory","payload":{"focus":"confirm the shared discovery"}}));
        if applied {
            for phase in ["delivered","applied"] {
                d.call("swarm.ack",json!({"run_id":run,"message_id":"after-review",
                    "recipient":attempt,"token":token,"revision":1,"phase":phase}));
            }
        }
        d.call("swarm.attempt.confirm_exit",json!({"run_id":run,"job_id":"routes",
            "attempt_id":attempt,"generation":1,"revision":1}));
        d.call("swarm.ack",json!({"run_id":run,"message_id":"result",
            "recipient":"director","generation":1,"revision":1,"phase":"applied"}));
        rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap()
            .execute("UPDATE swarm_runs SET status='running' WHERE id=?1",[&run]).unwrap();
        let completion = json!({"run_id":run,"generation":1,"revision":1,
            "request_id":"complete-directive","summary":"Route checked",
            "verification":"Accepted route proof","checks":[
                {"job_id":"routes","outcome":"passed","evidence":["proof"]}
            ]});
        if applied {
            assert_eq!(d.call("swarm.complete",completion)["status"],"completed");
        } else {
            let error = d.try_call("swarm.complete",completion).unwrap_err();
            assert!(error.contains("unapplied directive"),"{error}");
            assert_eq!(d.call("swarm.get",json!({"id":run}))["status"],"running");
        }
    }
}

#[test]
fn unapplied_redirect_times_out_and_holds_dependent_work() {
    let mut d = Daemon::start(&[]);
    let run = d.call("swarm.create", json!({"category":"Redirect timeout",
        "objective":"Audit Atlas","allowed_targets":["system-codex"]}));
    let id = run["id"].as_str().unwrap();
    d.call("swarm.plan", json!({"id":id,"generation":1,"revision":0,"jobs":[
        {"id":"j4","title":"Attachments","acceptance":"signed URL evidence","deps":[]},
        {"id":"dependent","title":"Review","acceptance":"review evidence","deps":["j4"]}
    ]}));
    let attempt = d.call("swarm.attempt.register", json!({"run_id":id,"job_id":"j4",
        "generation":1,"revision":1}));
    let message = json!({"run_id":id,"job_id":"j4","attempt_id":attempt["id"],
        "message_id":"j4-redirect","generation":1,"revision":1,"type":"redirect",
        "payload":{"focus":"signed URL boundary"}});
    d.call("swarm.direct", message.clone());
    assert!(d.try_call("swarm.ack", json!({"run_id":id,"message_id":"j4-redirect",
        "recipient":attempt["id"],"token":attempt["token"],"phase":"applied",
        "revision":1})).is_err());
    d.call("swarm.ack", json!({"run_id":id,"message_id":"j4-redirect",
        "recipient":attempt["id"],"token":attempt["token"],"phase":"delivered",
        "revision":1}));
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let delivered_at: i64 = db.query_row("SELECT updated_ms FROM swarm_messages
        WHERE run_id=?1 AND message_id='j4-redirect'",[id],|row|row.get(0)).unwrap();
    d.call("swarm.ack", json!({"run_id":id,"message_id":"j4-redirect",
        "recipient":attempt["id"],"token":attempt["token"],"phase":"delivered",
        "revision":1}));
    let replay_at: i64 = db.query_row("SELECT updated_ms FROM swarm_messages
        WHERE run_id=?1 AND message_id='j4-redirect'",[id],|row|row.get(0)).unwrap();
    assert_eq!(replay_at,delivered_at);
    let before = d.call("swarm.redirect.persist_due", json!({"now_ms":delivered_at+29_999}));
    assert_eq!(before["timed_out"],0);
    assert_eq!(d.call("swarm.jobs",json!({"id":id}))["jobs"].as_array().unwrap()
        .iter().find(|j|j["id"]=="j4").unwrap()["status"],"reserved");
    d.kill9();
    d.spawn();
    let timed_out = d.call("swarm.redirect.persist_due", json!({"now_ms":delivered_at+30_000}));
    assert_eq!(timed_out["timed_out"],1);
    let jobs = d.call("swarm.jobs",json!({"id":id}));
    let j4 = jobs["jobs"].as_array().unwrap().iter().find(|j|j["id"]=="j4").unwrap();
    assert_eq!(j4["status"],"cancel_requested");
    assert_eq!(j4["stop_reason"],"redirect_ack_timeout");
    let dependent = jobs["jobs"].as_array().unwrap().iter().find(|j|j["id"]=="dependent").unwrap();
    assert_eq!(dependent["status"],"planned");
    let inbox = d.call("swarm.messages",json!({"run_id":id,"recipient":attempt["id"],
        "token":attempt["token"]}));
    assert!(inbox["messages"].as_array().unwrap().iter()
        .any(|m|m["type"]=="checkpoint" && m["payload"]["reason"]=="redirect_ack_timeout"));
    assert_eq!(d.call("swarm.redirect.persist_due",json!({"now_ms":delivered_at+30_001}))["timed_out"],0);
}

#[test]
fn discovery_can_be_routed_to_only_relevant_peers_with_applied_receipts() {
    let d=Daemon::start(&[]);
    let made=d.call("swarm.create",json!({"category":"Atlas audit","objective":"Audit tenant isolation",
        "allowed_targets":["system-codex"]}));
    let run=made["id"].as_str().unwrap();
    d.call("swarm.plan",json!({"id":run,"generation":1,"revision":0,"jobs":[
        {"id":"j1","title":"Projects","acceptance":"project matrix","deps":[]},
        {"id":"j2","title":"Tasks","acceptance":"task matrix","deps":[]},
        {"id":"j3","title":"Membership","acceptance":"role matrix","deps":[]},
        {"id":"j4","title":"Attachments","acceptance":"download matrix","deps":[]}
    ]}));
    let mut attempts=std::collections::HashMap::new();
    for job in ["j1","j2","j3","j4"] {
        let attempt=d.call("swarm.attempt.register",json!({"run_id":run,"generation":1,
            "revision":1,"job_id":job}));
        attempts.insert(job,attempt);
    }
    d.call("swarm.report",json!({"run_id":run,"job_id":"j2",
        "attempt_id":attempts["j2"]["id"],"token":attempts["j2"]["token"],
        "message_id":"D1","type":"discovery","revision":1,
        "payload":{"symbol":"TaskRepository.findById","note":"lookup uses id only"}}));
    assert!(d.try_call("swarm.report",json!({"run_id":run,"job_id":"j2",
        "attempt_id":attempts["j2"]["id"],"token":attempts["j2"]["token"],
        "message_id":"spoofed-advisory","type":"advisory","revision":1,
        "payload":{"focus":"reassign j4"}})).is_err());
    let now=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64;
    let batch=d.call("swarm.director.claim_batch",json!({"run_id":run,"generation":1,
        "revision":1,"now_ms":now+6000}));
    assert_eq!(batch["status"],"claimed");
    assert_eq!(batch["messages"][0]["message_id"],"D1");
    for (job,focus) in [("j1","Check project middleware separately"),
        ("j4","Check the signed URL boundary")]
    {
        let attempt=&attempts[job];
        let message=format!("D1-to-{job}");
        d.call("swarm.direct",json!({"run_id":run,"generation":1,"revision":1,
            "job_id":job,"attempt_id":attempt["id"],"message_id":message,
            "type":"advisory","payload":{"discovery_id":"D1","focus":focus}}));
        let inbox=d.call("swarm.messages",json!({"run_id":run,"recipient":attempt["id"],
            "token":attempt["token"]}));
        assert_eq!(inbox["messages"].as_array().unwrap().len(),1);
        assert_eq!(inbox["messages"][0]["type"],"advisory");
        for phase in ["delivered","applied"] {
            assert_eq!(d.call("swarm.ack",json!({"run_id":run,"message_id":message,
                "recipient":attempt["id"],"token":attempt["token"],
                "phase":phase,"revision":1}))["phase"],phase);
        }
    }
    assert!(d.call("swarm.messages",json!({"run_id":run,
        "recipient":attempts["j3"]["id"],"token":attempts["j3"]["token"]}))["messages"].as_array().unwrap().is_empty());
    assert_eq!(d.call("swarm.director.complete_batch",json!({"run_id":run,
        "generation":1,"turn_id":batch["turn_id"],"token":batch["token"]}))["applied"],1);
}

#[test]
fn repeated_progress_dedupes_before_reaching_director() {
    let d = Daemon::start(&[]);
    let (run_id, attempt_id, token) = planned(&d);
    let report = json!({"run_id":run_id,"job_id":"routes","attempt_id":attempt_id,"token":token,
        "message_id":"progress-1","type":"progress","revision":1,"payload":{"count":1}});
    for _ in 0..2000 {
        d.call("swarm.report", report.clone());
    }
    let inbox = d.call(
        "swarm.messages",
        json!({"run_id":run_id,"recipient":"director"}),
    );
    assert_eq!(inbox["messages"].as_array().unwrap().len(), 1);
}

#[test]
fn stop_is_not_starved_by_two_thousand_duplicate_progress_replays() {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixStream;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    let d = Daemon::start(&[]);
    let (run, attempt, token) = planned(&d);
    let report = json!({"run_id":run,"job_id":"routes","attempt_id":attempt,
        "token":token,"message_id":"replayed-progress","type":"progress",
        "revision":1,"payload":{"step":"checking routes"}});
    assert_eq!(d.call("swarm.report", report.clone())["duplicate"], false);
    let socket = d.socket();
    let (started_tx, started_rx) = mpsc::channel();
    let flood = std::thread::spawn(move || {
        for index in 0..2000 {
            let mut conn = UnixStream::connect(&socket).unwrap();
            conn.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            writeln!(conn, "{}", json!({"id":index,"method":"swarm.report","params":report})).unwrap();
            let mut line = String::new();
            BufReader::new(conn).read_line(&mut line).unwrap();
            let reply: serde_json::Value = serde_json::from_str(&line).unwrap();
            assert_eq!(reply["result"]["duplicate"], true, "{reply}");
            if index == 20 { started_tx.send(()).unwrap(); }
            std::thread::sleep(Duration::from_millis(1));
        }
    });
    started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let begin = Instant::now();
    let stopped = d.call("swarm.stop", json!({"run_id":run,"generation":1,"revision":1}));
    assert!(begin.elapsed() < Duration::from_secs(2), "Stop was starved by duplicate traffic");
    assert_eq!(stopped["status"], "stopping");
    flood.join().unwrap();
    let inbox = d.call("swarm.messages", json!({"run_id":run,"recipient":"director"}));
    assert_eq!(inbox["messages"].as_array().unwrap().len(), 1, "{inbox}");
    assert_eq!(inbox["messages"][0]["message_id"], "replayed-progress");
}

#[test]
fn malformed_and_unauthorized_reports_return_bounded_errors_without_inbox_effects() {
    let d = Daemon::start(&[]);
    let (run, attempt, token) = planned(&d);
    let base = json!({"run_id":run,"job_id":"routes","attempt_id":attempt,
        "token":token,"message_id":"valid","type":"progress","revision":1,
        "payload":{"note":"ordinary"}});
    let secret = "DO-NOT-ECHO-THIS-PAYLOAD";
    let mut invalid = Vec::new();
    let mut wrong_token = base.clone();
    wrong_token["token"] = json!("wrong-secret-token");
    invalid.push(wrong_token);
    let mut missing_id = base.clone();
    missing_id.as_object_mut().unwrap().remove("message_id");
    invalid.push(missing_id);
    let mut long_id = base.clone();
    long_id["message_id"] = json!("i".repeat(129));
    invalid.push(long_id);
    let mut spoofed_direction = base.clone();
    spoofed_direction["type"] = json!("redirect");
    invalid.push(spoofed_direction);
    let mut wrong_revision = base.clone();
    wrong_revision["revision"] = json!(0);
    invalid.push(wrong_revision);
    let mut non_object = base.clone();
    non_object["payload"] = json!([secret]);
    invalid.push(non_object);
    let mut oversized = base.clone();
    oversized["payload"] = json!({"note":format!("{}{}",secret,"x".repeat(33_000))});
    invalid.push(oversized);
    for report in invalid {
        let error = d.try_call("swarm.report", report).unwrap_err();
        assert!(error.len() <= 160, "unbounded diagnostic: {error}");
        assert!(!error.contains(secret), "payload leaked in diagnostic");
        assert!(!error.contains("wrong-secret-token"), "token leaked in diagnostic");
    }
    let inbox = d.call("swarm.messages", json!({"run_id":run,"recipient":"director"}));
    assert!(inbox["messages"].as_array().unwrap().is_empty());
    assert_eq!(d.call("swarm.report", base)["duplicate"], false);
}

#[test]
fn terminal_run_replays_a_saved_result_receipt_without_accepting_new_work() {
    let mut d = Daemon::start(&[]);
    let (run, attempt, token) = planned(&d);
    let result = json!({"run_id":run,"job_id":"routes","attempt_id":attempt,
        "token":token,"message_id":"final-result","type":"result","revision":1,
        "payload":{"artifact_ids":[]}});
    let first = d.call("swarm.report", result.clone());
    assert_eq!(first["duplicate"], false);
    // Simulate finalization after the broker committed the result but before its receipt arrived.
    rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap()
        .execute("UPDATE swarm_runs SET status='completed' WHERE id=?1", [&run]).unwrap();
    d.kill9();
    d.spawn();
    let replay = d.call("swarm.report", result.clone());
    assert_eq!(replay["duplicate"], true);
    assert_eq!(replay["seq"], first["seq"]);
    let mut different = result.clone();
    different["payload"] = json!({"artifact_ids":["different"]});
    assert!(d.try_call("swarm.report", different).is_err());
    let mut new_message = result;
    new_message["message_id"] = json!("new-after-completion");
    assert!(d.try_call("swarm.report", new_message).is_err());
    let inbox = d.call("swarm.messages", json!({"run_id":run,"recipient":"director"}));
    assert_eq!(inbox["messages"].as_array().unwrap().len(), 1);
}

#[test]
fn full_inbox_rejects_routine_progress_but_keeps_terminal_result() {
    let d = Daemon::start(&[]);
    let (run_id, attempt_id, token) = planned(&d);
    for n in 0..1000 {
        d.call(
            "swarm.report",
            json!({"run_id":run_id,"job_id":"routes","attempt_id":attempt_id,"token":token,
            "message_id":format!("event-{n}"),"type":"progress","revision":1,"payload":{"n":n}}),
        );
    }
    let rejected = d
        .try_call(
            "swarm.report",
            json!({"run_id":run_id,"job_id":"routes","attempt_id":attempt_id,"token":token,
        "message_id":"overflow","type":"progress","revision":1,"payload":{}}),
        )
        .unwrap_err();
    assert!(rejected.contains("inbox is full"));
    d.call("swarm.report", json!({"run_id":run_id,"job_id":"routes","attempt_id":attempt_id,"token":token,
        "message_id":"terminal-result","type":"result","revision":1,"payload":{"artifact":"evidence"}}));
}

#[test]
fn terminal_inbox_bypass_is_bounded_per_attempt_and_replays_stay_idempotent() {
    let d = Daemon::start(&[]);
    let (run, attempt, token) = planned(&d);
    let message = |n: usize| json!({"run_id":run,"job_id":"routes",
        "attempt_id":attempt,"token":token,"message_id":format!("terminal-{n}"),
        "type":match n % 3 { 0 => "result", 1 => "submit", _ => "blocker" },
        "revision":1,"payload":{"n":n}});
    let mut first = serde_json::Value::Null;
    for n in 0..16 {
        let receipt = d.call("swarm.report", message(n));
        if n == 0 { first = receipt; }
    }
    assert_eq!(d.call("swarm.report", message(0))["seq"], first["seq"]);
    let rejected = d.try_call("swarm.report", message(16)).unwrap_err();
    assert!(rejected.contains("terminal report limit"), "{rejected}");
    let inbox = d.call("swarm.messages", json!({"run_id":run,"recipient":"director"}));
    assert_eq!(inbox["messages"].as_array().unwrap().len(), 16);
    d.call("swarm.ack", json!({"run_id":run,"message_id":"terminal-0",
        "recipient":"director","generation":1,"revision":1,"phase":"applied"}));
    assert_eq!(d.call("swarm.report", message(16))["duplicate"], false);
}

#[test]
fn stop_blocks_new_attempts_without_discarding_late_evidence() {
    let d = Daemon::start(&[]);
    let (run_id, attempt_id, token) = planned(&d);
    d.call(
        "swarm.stop",
        json!({"run_id":run_id,"generation":1,"revision":1}),
    );
    assert!(d
        .try_call(
            "swarm.attempt.register",
            json!({"run_id":run_id,"job_id":"routes","generation":1,"revision":1})
        )
        .is_err());
    d.call(
        "swarm.report",
        json!({"run_id":run_id,"job_id":"routes","attempt_id":attempt_id,"token":token,
        "message_id":"late-result","type":"result","revision":1,"payload":{"artifact":"partial"}}),
    );
    let inbox = d.call(
        "swarm.messages",
        json!({"run_id":run_id,"recipient":"director"}),
    );
    assert_eq!(inbox["messages"][0]["message_id"], "late-result");
    assert_eq!(
        d.call("swarm.get", json!({"id":run_id}))["status"],
        "stopping"
    );
    let jobs = d.call("swarm.jobs", json!({"id":run_id}));
    assert_eq!(jobs["jobs"][0]["status"], "cancel_requested");
    let control = d.call(
        "swarm.messages",
        json!({"run_id":run_id,"recipient":attempt_id,"token":token}),
    );
    assert_eq!(control["messages"][0]["type"], "stop");
}

#[test]
fn finished_attempt_late_result_cannot_submit_a_newer_attempt() {
    let d=Daemon::start(&[]);
    let (run,first,first_token)=planned(&d);
    d.call("swarm.artifact.put",json!({"run_id":run,"job_id":"routes",
        "attempt_id":first,"token":first_token,"artifact_id":"first-proof",
        "source_revision":1,"kind":"finding","content":"first attempt"}));
    d.call("swarm.report",json!({"run_id":run,"job_id":"routes",
        "attempt_id":first,"token":first_token,"message_id":"first-result",
        "type":"result","revision":1,"payload":{"artifact_ids":["first-proof"]}}));
    d.call("swarm.decide",json!({"run_id":run,"generation":1,"revision":1,
        "job_id":"routes","decision":"reject","evidence":["first-proof"]}));
    d.call("swarm.attempt.confirm_exit",json!({"run_id":run,"job_id":"routes",
        "attempt_id":first,"generation":1,"revision":1}));
    let second=d.call("swarm.attempt.register",json!({"run_id":run,"job_id":"routes",
        "generation":1,"revision":1}));
    assert_eq!(d.call("swarm.jobs",json!({"id":run}))["jobs"][0]["status"],"reserved");
    d.call("swarm.report",json!({"run_id":run,"job_id":"routes",
        "attempt_id":first,"token":first_token,"message_id":"late-first-result",
        "type":"result","revision":1,"payload":{"artifact_ids":["first-proof"]}}));
    assert_eq!(d.call("swarm.jobs",json!({"id":run}))["jobs"][0]["status"],"reserved");
    assert_eq!(d.call("swarm.messages",json!({"run_id":run,"recipient":"director"}))["messages"]
        .as_array().unwrap().iter().filter(|m|m["message_id"]=="late-first-result").count(),1);
    d.call("swarm.report",json!({"run_id":run,"job_id":"routes",
        "attempt_id":second["id"],"token":second["token"],"message_id":"second-result",
        "type":"result","revision":1,"payload":{"artifact_ids":[]}}));
    assert_eq!(d.call("swarm.jobs",json!({"id":run}))["jobs"][0]["status"],"submitted");
}

/// SWARM-55: an unavailable database and missing access are environment
/// blockers like a missing queue. Neither can be accepted as a passed check,
/// neither reads as an application defect in the coverage report (also when
/// the worker labels its artifact a reproduction), and the run cannot
/// complete by claiming them passed.
#[test]
fn unavailable_database_or_access_is_blocked_coverage_not_a_pass_or_defect() {
    let d = Daemon::start(&[]);
    let run = d.call("swarm.create", json!({"category":"Unavailable services",
        "objective":"Audit tenant isolation","allowed_targets":["fixture"]}));
    let run_id = run["id"].as_str().unwrap();
    d.call("swarm.plan", json!({"id":run_id,"generation":1,"revision":0,"jobs":[
        {"id":"db","title":"Row ownership","acceptance":"before/after rows","deps":[]},
        {"id":"access","title":"Token scopes","acceptance":"token matrix","deps":[]}
    ]}));
    for (job, resource, kind) in [("db", "database", "reproduction"), ("access", "access", "log")] {
        let attempt = d.call("swarm.attempt.register", json!({"run_id":run_id,
            "job_id":job,"generation":1,"revision":1}));
        let artifact = format!("proof-{job}");
        d.call("swarm.artifact.put", json!({"run_id":run_id,"job_id":job,
            "attempt_id":attempt["id"],"token":attempt["token"],"artifact_id":artifact,
            "kind":kind,"content":format!("{resource} unavailable: connection refused"),
            "source_revision":1}));
        d.call("swarm.report", json!({"run_id":run_id,"job_id":job,
            "attempt_id":attempt["id"],"token":attempt["token"],
            "message_id":format!("result-{job}"),"type":"result","revision":1,
            "payload":{"audit_outcome":"environment_failure","artifact_ids":[artifact],
                "unavailable_resource":resource}}));
        let refused = d.try_call("swarm.decide", json!({"run_id":run_id,"generation":1,
            "revision":1,"job_id":job,"decision":"accept","evidence":[artifact]})).unwrap_err();
        assert!(refused.contains("environment"), "{job}: {refused}");
    }
    let coverage = d.call("swarm.coverage", json!({"run_id":run_id}));
    for row in coverage["rows"].as_array().unwrap() {
        assert_eq!(row["coverage_state"], "environment_blocked", "{row}");
        assert_ne!(row["job_status"], "accepted", "{row}");
    }
    let resources: Vec<&str> = coverage["rows"].as_array().unwrap().iter()
        .map(|r| r["unavailable_resource"].as_str().unwrap()).collect();
    assert_eq!(resources, vec!["access", "database"]);
    let completed = d.try_call("swarm.complete", json!({"run_id":run_id,"generation":1,
        "revision":1,"request_id":"claim-pass","summary":"All clear","verification":"none",
        "checks":[{"job_id":"db","outcome":"passed","evidence":["proof-db"]},
            {"job_id":"access","outcome":"passed","evidence":["proof-access"]}]}));
    assert!(completed.is_err(), "{completed:?}");
    assert_ne!(d.call("swarm.get", json!({"id":run_id}))["status"], "completed");
}

/// SWARM-42: duplicated, delayed and reordered progress, discovery, result
/// and acknowledgement messages each have one effect. The worker's result
/// arrives before its earlier progress and discovery; the applied
/// acknowledgement arrives before the delivered one; every message is sent
/// twice, and the result a third time after a daemon kill. One message row
/// per ID, one submission, one acceptance, one reservation, the directive
/// stays applied, and the late progress changes no job state. Unknown
/// identities, a message for another run and a stale director generation are
/// refused.
#[test]
fn duplicated_delayed_and_reordered_messages_have_one_effect_each() {
    let mut d = Daemon::start(&[]);
    let run = d.call("swarm.create", json!({"category":"Reordered","objective":"Audit routes",
        "allowed_targets":["fixture"]}));
    let id = run["id"].as_str().unwrap().to_string();
    d.call("swarm.plan", json!({"id":id,"generation":1,"revision":0,"jobs":[
        {"id":"routes","title":"Inspect routes","acceptance":"route table","deps":[]}]}));
    let other = d.call("swarm.create", json!({"category":"Other run","objective":"Audit",
        "allowed_targets":["fixture"]}));
    let other_id = other["id"].as_str().unwrap().to_string();
    d.call("swarm.plan", json!({"id":other_id,"generation":1,"revision":0,"jobs":[
        {"id":"routes","title":"Inspect routes","acceptance":"route table","deps":[]}]}));
    let at = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64;
    let admitted = d.call("swarm.admit", json!({"run_id":id,"generation":1,"revision":1,
        "job_id":"routes","target_id":"fixture","request_id":"routes-once","now_ms":at,
        "snapshot":{"version":1,"observed_ms":at-1000,"expires_ms":at+60000,
            "targets":[{"id":"fixture","account_id":"a","pool_ids":["p"],"capabilities":["audit"],
                "health":"up","auth":"ok"}],
            "pools":[{"id":"p","windows":[{"id":"w","unit":"points","remaining_milli":1000000,
                "protected_milli":0,"reserved_milli":0,"confidence":"exact","expires_ms":at+60000}]}]},
        "required_capabilities":["audit"],"estimate_milli":{"points":100},"purpose":"worker"}));
    assert_eq!(admitted["status"], "admitted", "{admitted}");
    let (attempt, token) = (admitted["attempt_id"].as_str().unwrap().to_string(),
        admitted["token"].as_str().unwrap().to_string());
    let directive = json!({"run_id":id,"job_id":"routes","attempt_id":attempt,"message_id":"focus-1",
        "generation":1,"revision":1,"type":"advisory","payload":{"focus":"pagination"}});
    d.call("swarm.direct", directive.clone());
    d.call("swarm.artifact.put", json!({"run_id":id,"job_id":"routes","attempt_id":attempt,"token":token,
        "artifact_id":"table","source_revision":1,"kind":"finding","content":"route table"}));
    let report = |message: &str, kind: &str, payload: serde_json::Value| json!({"run_id":id,"job_id":"routes",
        "attempt_id":attempt,"token":token,"message_id":message,"type":kind,"revision":1,"payload":payload});
    let result = report("result-1", "result", json!({"artifact_ids":["table"]}));
    let progress = report("progress-1", "progress", json!({"step":"reading routes"}));
    let discovery = report("discovery-1", "discovery", json!({"symbol":"findRoute"}));
    let ack = |phase: &str| json!({"run_id":id,"message_id":"focus-1","recipient":attempt,"token":token,
        "phase":phase,"revision":1});
    // Reordered and duplicated: the applied acknowledgement arrives before the
    // delivered one. It is refused without a receipt, so the worker retries it
    // after the delivered one; then a delayed duplicate of the delivered one
    // arrives last. The result arrives before the progress and discovery that
    // preceded it.
    let early = d.try_call("swarm.ack", ack("applied")).unwrap_err();
    assert!(early.contains("delivered before it is applied"), "{early}");
    for _ in 0..2 {
        d.call("swarm.ack", ack("delivered"));
        d.call("swarm.report", result.clone());
    }
    for _ in 0..2 {
        d.call("swarm.ack", ack("applied"));
    }
    let late = d.try_call("swarm.ack", ack("delivered"));
    let phase: String = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap()
        .query_row("SELECT phase FROM swarm_messages WHERE run_id=?1 AND message_id='focus-1'", [&id],
            |r| r.get(0)).unwrap();
    assert_eq!(phase, "applied", "a delayed delivered acknowledgement does not regress it: {late:?}");
    for _ in 0..2 {
        d.call("swarm.report", progress.clone());
        d.call("swarm.report", discovery.clone());
    }
    assert_eq!(d.call("swarm.jobs", json!({"id":id}))["jobs"][0]["status"], "submitted",
        "late progress changes no job state");
    // The result's receipt is lost in a crash; the worker replays it.
    d.kill9();
    d.spawn();
    assert_eq!(d.call("swarm.report", result.clone())["duplicate"], true);
    assert_eq!(d.call("swarm.direct", directive)["duplicate"], true);
    assert_eq!(d.call("swarm.decide", json!({"run_id":id,"generation":1,"revision":1,"job_id":"routes",
        "decision":"accept","evidence":["table"]}))["status"], "accepted");
    let again = d.try_call("swarm.decide", json!({"run_id":id,"generation":1,"revision":1,"job_id":"routes",
        "decision":"accept","evidence":["table"]}));
    assert!(again.as_ref().map(|r| r["duplicate"] == true).unwrap_or(true), "{again:?}");
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    for message in ["result-1", "progress-1", "discovery-1", "focus-1"] {
        let n: i64 = db.query_row("SELECT COUNT(*) FROM swarm_messages WHERE run_id=?1 AND message_id=?2",
            [&id, &message.to_string()], |r| r.get(0)).unwrap();
        assert_eq!(n, 1, "{message}");
    }
    let (decisions, reservations, attempts): (i64, i64, i64) = db.query_row("SELECT
        (SELECT COUNT(*) FROM swarm_decisions WHERE run_id=?1),
        (SELECT COUNT(*) FROM swarm_reservations WHERE run_id=?1),
        (SELECT COUNT(*) FROM swarm_attempts WHERE run_id=?1)", [&id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap();
    assert_eq!((decisions, reservations, attempts), (1, 1, 1), "one acceptance, one reservation, one attempt");
    let results: i64 = db.query_row("SELECT COUNT(*) FROM swarm_operation_order WHERE run_id=?1 AND kind='result'",
        [&id], |r| r.get(0)).unwrap();
    assert_eq!(results, 1, "one submission");
    // Unknown identities, another run's attempt and a stale director generation.
    let mut unknown = progress.clone();
    unknown["attempt_id"] = json!("sa-unknown");
    unknown["message_id"] = json!("unknown-1");
    assert!(d.try_call("swarm.report", unknown).is_err());
    let mut forged = progress.clone();
    forged["token"] = json!("0".repeat(32));
    forged["message_id"] = json!("forged-1");
    assert!(d.try_call("swarm.report", forged).is_err());
    let mut cross = progress.clone();
    cross["run_id"] = json!(other_id);
    cross["message_id"] = json!("cross-1");
    assert!(d.try_call("swarm.report", cross).is_err(), "another run's attempt is refused");
    assert!(d.try_call("swarm.direct", json!({"run_id":id,"job_id":"routes","attempt_id":attempt,
        "message_id":"stale-gen","generation":2,"revision":1,"type":"advisory","payload":{}})).is_err());
    let n: i64 = db.query_row("SELECT COUNT(*) FROM swarm_messages WHERE message_id IN
        ('unknown-1','forged-1','cross-1','stale-gen')", [], |r| r.get(0)).unwrap();
    assert_eq!(n, 0);
}

/// SWARM-46 (S1's D1 retraction): once the director withdraws a discovery,
/// every attempt that was told of it must apply the correction before its
/// work counts. D1 was routed to J1 and J4. The director retracts D1 to J4
/// only: J1 cannot be accepted (the omission is caught, not silently used)
/// until the retraction also reaches J1 and J1 applies it. An accepted J6
/// that was told of D1 before the retraction blocks completion the same way.
/// J3, never told of D1, is unaffected.
#[test]
fn a_withdrawn_discovery_must_be_retracted_to_every_prior_recipient() {
    let d = Daemon::start(&[]);
    let run = d.call("swarm.create", json!({"category":"Retraction","objective":"Audit tenant isolation",
        "allowed_targets":["fixture"]}));
    let id = run["id"].as_str().unwrap().to_string();
    d.call("swarm.plan", json!({"id":id,"generation":1,"revision":0,"jobs":[
        {"id":"j1","title":"Projects","acceptance":"evidence","deps":[]},
        {"id":"j2","title":"Tasks","acceptance":"evidence","deps":[]},
        {"id":"j3","title":"Membership","acceptance":"evidence","deps":[]},
        {"id":"j4","title":"Attachments","acceptance":"evidence","deps":[]},
        {"id":"j6","title":"Tokens","acceptance":"evidence","deps":[]}]}));
    let mut attempts = std::collections::BTreeMap::new();
    for job in ["j1","j2","j3","j4","j6"] {
        attempts.insert(job, d.call("swarm.attempt.register", json!({"run_id":id,"generation":1,"revision":1,"job_id":job})));
    }
    let a = |job: &str| attempts[job].clone();
    d.call("swarm.report", json!({"run_id":id,"job_id":"j2","attempt_id":a("j2")["id"],"token":a("j2")["token"],
        "message_id":"D1","type":"discovery","revision":1,"payload":{"symbol":"TaskRepository.findById"}}));
    let direct = |job: &str, message: &str, kind: &str| d.call("swarm.direct", json!({"run_id":id,"generation":1,
        "revision":1,"job_id":job,"attempt_id":a(job)["id"],"message_id":message,"type":kind,
        "payload":{"discovery_id":"D1","note":"see the shared task lookup"}}));
    let ack = |job: &str, message: &str, phase: &str| d.call("swarm.ack", json!({"run_id":id,"message_id":message,
        "recipient":a(job)["id"],"token":a(job)["token"],"phase":phase,"revision":1}));
    let finish = |job: &str| {
        d.call("swarm.artifact.put", json!({"run_id":id,"job_id":job,"attempt_id":a(job)["id"],"token":a(job)["token"],
            "artifact_id":format!("{job}-proof"),"source_revision":1,"kind":"finding","content":format!("{job} checked")}));
        d.call("swarm.report", json!({"run_id":id,"job_id":job,"attempt_id":a(job)["id"],"token":a(job)["token"],
            "message_id":format!("{job}-result"),"type":"result","revision":1,"payload":{"artifact_ids":[format!("{job}-proof")]}}));
    };
    let accept = |job: &str| d.try_call("swarm.decide", json!({"run_id":id,"generation":1,"revision":1,"job_id":job,
        "decision":"accept","evidence":[format!("{job}-proof")]}));
    for job in ["j1","j4","j6"] {
        direct(job, &format!("D1-to-{job}"), "advisory");
        ack(job, &format!("D1-to-{job}"), "delivered");
        ack(job, &format!("D1-to-{job}"), "applied");
    }
    // J6 is accepted while D1 still stands.
    finish("j6");
    assert_eq!(accept("j6").unwrap()["status"], "accepted");
    // The director withdraws D1, but tells only J4.
    direct("j4", "retract-D1-j4", "retract");
    ack("j4", "retract-D1-j4", "delivered");
    ack("j4", "retract-D1-j4", "applied");
    finish("j1");
    let silent = accept("j1").unwrap_err();
    assert!(silent.contains("withdrawn discovery"), "J1 would silently use D1: {silent}");
    direct("j1", "retract-D1-j1", "retract");
    ack("j1", "retract-D1-j1", "delivered");
    assert!(accept("j1").unwrap_err().contains("unapplied directive"));
    ack("j1", "retract-D1-j1", "applied");
    assert_eq!(accept("j1").unwrap()["status"], "accepted");
    for job in ["j2","j3","j4"] { finish(job); assert_eq!(accept(job).unwrap()["status"], "accepted", "{job}"); }
    for (job, attempt) in &attempts {
        d.call("swarm.attempt.confirm_exit", json!({"run_id":id,"generation":1,"revision":1,
            "job_id":job,"attempt_id":attempt["id"]}));
    }
    let inbox = d.call("swarm.messages", json!({"run_id":id,"recipient":"director"}));
    for m in inbox["messages"].as_array().unwrap() {
        d.call("swarm.ack", json!({"run_id":id,"message_id":m["message_id"],"recipient":"director",
            "generation":1,"revision":m["revision"],"phase":"applied"}));
    }
    // Attempts registered directly (no admission) leave the run in `planning`;
    // completion needs `running`, as in the integration fixtures.
    rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap()
        .execute("UPDATE swarm_runs SET status='running' WHERE id=?1", [&id]).unwrap();
    let checks: Vec<serde_json::Value> = ["j1","j2","j3","j4","j6"].iter()
        .map(|j| json!({"job_id":j,"outcome":"passed","evidence":[format!("{j}-proof")]})).collect();
    let complete = |request: &str| d.try_call("swarm.complete", json!({"run_id":id,"generation":1,"revision":1,
        "request_id":request,"summary":"audit","verification":"fixture checks","checks":checks}));
    let blocked = complete("complete-1").unwrap_err();
    assert!(blocked.contains("withdrawn discovery"), "J6 was accepted on D1 and never told: {blocked}");
    assert_ne!(d.call("swarm.get", json!({"id":id}))["status"], "completed");
}
