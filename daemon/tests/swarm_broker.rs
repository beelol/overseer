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
