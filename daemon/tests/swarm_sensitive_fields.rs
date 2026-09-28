mod common;

use common::*;
use serde_json::json;

fn run(d: &Daemon) -> (String, String, String) {
    let created = d.call(
        "swarm.create",
        json!({"category":"Sensitive field fixture","objective":"Audit a local route",
            "allowed_targets":["fixture"]}),
    );
    let run = created["id"].as_str().unwrap().to_string();
    d.call(
        "swarm.plan",
        json!({"id":run,"generation":1,"revision":0,
            "jobs":[{"id":"route","title":"Check route","acceptance":"evidence","deps":[]}]}),
    );
    let attempt = d.call(
        "swarm.attempt.register",
        json!({"run_id":run,"job_id":"route","generation":1,"revision":1}),
    );
    (
        run,
        attempt["id"].as_str().unwrap().to_string(),
        attempt["token"].as_str().unwrap().to_string(),
    )
}

#[test]
fn secret_shaped_artifact_and_message_ids_never_enter_shared_storage() {
    let d = Daemon::start(&[]);
    let (run, attempt, token) = run(&d);
    let secret = "sk-abcdefghijklmnopqrstuv";
    let artifact_error = d
        .try_call(
            "swarm.artifact.put",
            json!({"run_id":run,"job_id":"route","attempt_id":attempt,"token":token,
                "artifact_id":secret,"source_revision":1,"kind":"finding",
                "content":"Route checked"}),
        )
        .unwrap_err();
    assert!(artifact_error.contains("sensitive"), "{artifact_error}");
    assert!(!artifact_error.contains(secret));
    let kind_error = d
        .try_call(
            "swarm.artifact.put",
            json!({"run_id":run,"job_id":"route","attempt_id":attempt,"token":token,
                "artifact_id":"safe-id","source_revision":1,"kind":secret,
                "content":"Route checked"}),
        )
        .unwrap_err();
    assert!(kind_error.contains("sensitive"), "{kind_error}");
    assert!(!kind_error.contains(secret));
    let message_error = d
        .try_call(
            "swarm.report",
            json!({"run_id":run,"job_id":"route","attempt_id":attempt,"token":token,
                "message_id":secret,"type":"progress","revision":1,
                "payload":{"status":"working"}}),
        )
        .unwrap_err();
    assert!(message_error.contains("sensitive"), "{message_error}");
    assert!(!message_error.contains(secret));
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let artifact_count: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM swarm_artifacts WHERE run_id=?1",
            [&run],
            |r| r.get(0),
        )
        .unwrap();
    let message_count: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM swarm_messages WHERE run_id=?1",
            [&run],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!((artifact_count, message_count), (0, 0));
}

#[test]
fn completion_rejects_unrecognized_check_fields_and_secret_request_ids() {
    let d = Daemon::start(&[]);
    let (run, attempt, token) = run(&d);
    let secret = "sk-abcdefghijklmnopqrstuv";
    d.call(
        "swarm.artifact.put",
        json!({"run_id":run,"job_id":"route","attempt_id":attempt,"token":token,
            "artifact_id":"route-proof","source_revision":1,"kind":"finding",
            "content":"Route checked"}),
    );
    d.call(
        "swarm.report",
        json!({"run_id":run,"job_id":"route","attempt_id":attempt,"token":token,
            "message_id":"route-result","type":"result","revision":1,
            "payload":{"artifact_ids":["route-proof"]}}),
    );
    d.call(
        "swarm.decide",
        json!({"run_id":run,"generation":1,"revision":1,"job_id":"route",
            "decision":"accept","evidence":["route-proof"]}),
    );
    d.call(
        "swarm.attempt.confirm_exit",
        json!({"run_id":run,"generation":1,"revision":1,"job_id":"route",
            "attempt_id":attempt}),
    );
    d.call(
        "swarm.ack",
        json!({"run_id":run,"message_id":"route-result","recipient":"director",
            "generation":1,"revision":1,"phase":"applied"}),
    );
    rusqlite::Connection::open(d.home.path().join("overseer.sqlite"))
        .unwrap()
        .execute("UPDATE swarm_runs SET status='running' WHERE id=?1", [&run])
        .unwrap();
    let mut complete = json!({"run_id":run,"generation":1,"revision":1,
        "request_id":"finish-route","summary":"Route checked","verification":"Local evidence",
        "checks":[{"job_id":"route","outcome":"passed","evidence":["route-proof"]}]});
    complete["checks"][0]["raw_secret"] = json!(secret);
    let error = d.try_call("swarm.complete", complete.clone()).unwrap_err();
    assert!(error.contains("unknown completion check field"), "{error}");
    assert!(!error.contains(secret));
    complete["checks"][0]
        .as_object_mut()
        .unwrap()
        .remove("raw_secret");
    complete["checks"][0]["evidence"] = json!([secret]);
    let error = d.try_call("swarm.complete", complete.clone()).unwrap_err();
    assert!(error.contains("sensitive"), "{error}");
    assert!(!error.contains(secret));
    complete["checks"][0]["evidence"] = json!(["route-proof"]);
    complete["request_id"] = json!(secret);
    let error = d.try_call("swarm.complete", complete.clone()).unwrap_err();
    assert!(error.contains("sensitive"), "{error}");
    assert!(!error.contains(secret));
    complete["request_id"] = json!("finish-route");
    assert_eq!(d.call("swarm.complete", complete)["status"], "completed");
    let saved = d.call("swarm.get", json!({"id":run}))["completion"].clone();
    assert_eq!(saved["checks"][0]["evidence"], json!(["route-proof"]));
    assert!(!saved.to_string().contains(secret));
}
