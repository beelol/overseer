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
    let run=d.call("swarm.create",json!({"category":"Safe default","objective":"Audit","allowed_targets":["system-codex"]}));
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
}

#[test]
fn directive_delivery_and_application_are_distinct() {
    let d = Daemon::start(&[]);
    let (run_id, attempt_id, token) = planned(&d);
    let sent=d.call("swarm.direct",json!({"run_id":run_id,"job_id":"routes","attempt_id":attempt_id,
        "message_id":"redirect-2","generation":1,"revision":1,"type":"redirect","payload":{"focus":"pagination"}}));
    assert_eq!(sent["phase"], "queued");
    let inbox = d.call(
        "swarm.messages",
        json!({"run_id":run_id,"recipient":attempt_id}),
    );
    assert_eq!(inbox["messages"].as_array().unwrap().len(), 1);
    let delivered=d.call("swarm.ack",json!({"run_id":run_id,"message_id":"redirect-2","recipient":attempt_id,"token":token,"phase":"delivered","revision":1}));
    assert_eq!(delivered["phase"], "delivered");
    let applied=d.call("swarm.ack",json!({"run_id":run_id,"message_id":"redirect-2","recipient":attempt_id,"token":token,"phase":"applied","revision":1}));
    assert_eq!(applied["phase"], "applied");
    let replay=d.call("swarm.direct",json!({"run_id":run_id,"job_id":"routes","attempt_id":attempt_id,
        "message_id":"redirect-2","generation":1,"revision":1,"type":"redirect","payload":{"focus":"pagination"}}));
    assert_eq!(replay["phase"], "applied");
    assert_eq!(replay["duplicate"], true);
    assert!(d.try_call("swarm.ack",json!({"run_id":run_id,"message_id":"redirect-2","recipient":attempt_id,"token":token,"phase":"applied","revision":0})).is_err());
    assert!(d.try_call("swarm.direct",json!({"run_id":run_id,"job_id":"routes","attempt_id":attempt_id,"message_id":"stale","generation":0,"revision":1,"type":"redirect","payload":{}})).is_err());
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
        let inbox=d.call("swarm.messages",json!({"run_id":run,"recipient":attempt["id"]}));
        assert_eq!(inbox["messages"].as_array().unwrap().len(),1);
        assert_eq!(inbox["messages"][0]["type"],"advisory");
        for phase in ["delivered","applied"] {
            assert_eq!(d.call("swarm.ack",json!({"run_id":run,"message_id":message,
                "recipient":attempt["id"],"token":attempt["token"],
                "phase":phase,"revision":1}))["phase"],phase);
        }
    }
    assert!(d.call("swarm.messages",json!({"run_id":run,
        "recipient":attempts["j3"]["id"]}))["messages"].as_array().unwrap().is_empty());
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
        json!({"run_id":run_id,"recipient":attempt_id}),
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
