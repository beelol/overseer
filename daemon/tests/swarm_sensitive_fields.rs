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

/// SWARM-57: a credential that reaches Swarm through any free-text path
/// (worker reports of every kind, artifact content, a director directive, a
/// plan revision's reason, a conflict's reason, the completion summary and
/// verification) is never written in the clear. After the run completes, no
/// file in the daemon's home (the SQLite database, its WAL and every log)
/// contains the secret or the bearer value, and the readouts carry only the
/// redaction marker.
#[test]
fn no_file_in_the_daemon_home_holds_a_credential_sent_through_swarm_text() {
    let d = Daemon::start(&[]);
    let created = d.call("swarm.create", json!({"category":"Credential sweep","objective":"Audit a local route",
        "allowed_targets":["fixture"]}));
    let run = created["id"].as_str().unwrap().to_string();
    d.call("swarm.plan", json!({"id":run,"generation":1,"revision":0,
        "jobs":[{"id":"route","title":"Check route","acceptance":"evidence","deps":[]}]}));
    let at = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64;
    let snapshot = json!({"version":1,"observed_ms":at-1000,"expires_ms":at+60000,
        "targets":[{"id":"fixture","account_id":"a","pool_ids":["p"],"capabilities":["audit"],"health":"up","auth":"ok"}],
        "pools":[{"id":"p","windows":[{"id":"w","unit":"points","remaining_milli":100000,"protected_milli":0,
            "reserved_milli":0,"confidence":"exact","expires_ms":at+60000}]}]});
    let admit = |job: &str, revision: i64| d.call("swarm.admit", json!({"run_id":run,"generation":1,"revision":revision,
        "job_id":job,"target_id":"fixture","request_id":format!("{job}-sweep"),"now_ms":at,"snapshot":snapshot,
        "required_capabilities":["audit"],"estimate_milli":{"points":100},"purpose":"worker"}));
    let admitted = admit("route", 1);
    assert_eq!(admitted["status"], "admitted", "{admitted}");
    let (attempt, token) = (admitted["attempt_id"].as_str().unwrap().to_string(), admitted["token"].as_str().unwrap().to_string());
    let secret = "sk-ant-api03-SWARMSECRETabcdefghijklmnop";
    let bearer = "SWARMBEARERabcdefghijklmnopqrst";
    let leak = format!("key {secret}; Authorization: Bearer {bearer}");
    d.call("swarm.artifact.put", json!({"run_id":run,"job_id":"route","attempt_id":attempt,"token":token,
        "artifact_id":"route-proof","source_revision":1,"kind":"finding","content":format!("trace with {leak}")}));
    for (message, kind, payload) in [
        ("p1", "progress", json!({"step":format!("reading env {leak}")})),
        ("d1", "discovery", json!({"symbol":"loadConfig","note":leak.clone()})),
        ("q1", "question", json!({"question":format!("may I use {leak}?")})),
        ("r1", "result", json!({"artifact_ids":["route-proof"],"summary":leak.clone()})),
    ] {
        d.call("swarm.report", json!({"run_id":run,"job_id":"route","attempt_id":attempt,"token":token,
            "message_id":message,"type":kind,"revision":1,"payload":payload}));
    }
    d.call("swarm.direct", json!({"run_id":run,"job_id":"route","attempt_id":attempt,"message_id":"focus",
        "generation":1,"revision":1,"type":"advisory","payload":{"focus":format!("ignore {leak}")}}));
    for phase in ["delivered", "applied"] {
        d.call("swarm.ack", json!({"run_id":run,"message_id":"focus","recipient":attempt,"token":token,
            "phase":phase,"revision":1}));
    }
    d.call("swarm.decide", json!({"run_id":run,"generation":1,"revision":1,"job_id":"route",
        "decision":"accept","evidence":["route-proof"]}));
    d.call("swarm.attempt.confirm_exit", json!({"run_id":run,"generation":1,"revision":1,
        "job_id":"route","attempt_id":attempt}));
    let revised = d.try_call("swarm.revise", json!({"id":run,"generation":1,"expected_revision":1,
        "reason":format!("reviewer pasted {leak}"),"jobs":[
            {"id":"route","title":"Check route","acceptance":"evidence","deps":[]},
            {"id":"extra","title":"Extra","acceptance":"evidence","deps":[]}]}));
    let revision = revised.as_ref().unwrap()["revision"].as_i64().unwrap();
    let extra = admit("extra", revision);
    assert_eq!(extra["status"], "admitted", "{extra}");
    d.call("swarm.artifact.put", json!({"run_id":run,"job_id":"extra","attempt_id":extra["attempt_id"],
        "token":extra["token"],"artifact_id":"extra-proof","source_revision":revision,"kind":"finding",
        "content":"route denies foreign access"}));
    d.call("swarm.report", json!({"run_id":run,"job_id":"extra","attempt_id":extra["attempt_id"],"token":extra["token"],
        "message_id":"extra-result","type":"result","revision":revision,"payload":{"artifact_ids":["extra-proof"]}}));
    let conflict = d.try_call("swarm.conflict.open", json!({"run_id":run,"generation":1,"revision":revision,
        "conflict_id":"c1","left_job_id":"route","left_artifact_id":"route-proof","right_job_id":"extra",
        "right_artifact_id":"extra-proof","reason":format!("differs: {leak}")}));
    assert!(conflict.is_ok(), "{conflict:?}");
    d.call("swarm.conflict.resolve", json!({"run_id":run,"generation":1,"revision":revision,
        "conflict_id":"c1","outcome":"unresolved"}));
    let messages = d.call("swarm.messages", json!({"run_id":run,"recipient":"director"}));
    for m in messages["messages"].as_array().unwrap() {
        d.call("swarm.ack", json!({"run_id":run,"message_id":m["message_id"],"recipient":"director",
            "generation":1,"revision":m["revision"],"phase":"applied"}));
    }
    d.call("swarm.attempt.confirm_exit", json!({"run_id":run,"generation":1,"revision":revision,
        "job_id":"extra","attempt_id":extra["attempt_id"]}));
    // The run closes through the director's partial report (the disagreement stays
    // unresolved), whose summary and limitations are free text too.
    let control = d.call("swarm.get", json!({"id":run}))["control_revision"].as_i64().unwrap_or(0);
    let completed = d.try_call("swarm.partial", json!({"run_id":run,"generation":1,"revision":revision,
        "expected_revision":revision,"expected_control_revision":control,
        "request_id":"partial-sweep","incomplete_reason":"unresolved_conflict",
        "summary":format!("done; found {leak}"),"limitations":format!("checked with {leak}")}));
    assert!(completed.is_ok(), "{completed:?}");
    let readouts = [d.call("swarm.get", json!({"id":run})), d.call("swarm.coverage", json!({"run_id":run})),
        d.call("swarm.messages", json!({"run_id":run,"recipient":"director"})),
        d.call("swarm.conflicts", json!({"run_id":run}))];
    for readout in &readouts {
        let text = readout.to_string();
        assert!(!text.contains(secret) && !text.contains(bearer), "a readout carries the credential: {text}");
    }
    // Every byte on disk: the database, its journal files and the logs.
    let mut stack = vec![d.home.path().to_path_buf()];
    let mut scanned = 0;
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() { stack.push(path); continue; }
            if path.extension().is_some_and(|e| e == "sock") { continue; }
            let Ok(bytes) = std::fs::read(&path) else { continue };
            scanned += 1;
            let found = |needle: &str| bytes.windows(needle.len()).any(|w| w == needle.as_bytes());
            assert!(!found(secret) && !found(bearer), "{} holds the credential (revise {revised:?}, conflict {conflict:?}, complete {completed:?})", path.display());
        }
    }
    assert!(scanned >= 2, "the database and its files were scanned");
}
