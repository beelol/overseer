mod common;

use common::*;
use serde_json::{json, Value};
use std::time::{SystemTime, UNIX_EPOCH};

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

fn admit(d: &Daemon, run: &str, job: &str, target: &str) -> Value {
    let at = now();
    let result = d.call("swarm.admit", json!({"run_id":run,"generation":1,"revision":1,
        "job_id":job,"target_id":target,"request_id":format!("admit-{job}"),"now_ms":at,
        "snapshot":{"version":1,"observed_ms":at-1000,"expires_ms":at+60000,
            "targets":[
                {"id":"account-a","account_id":"a","pool_ids":["pool-a"],"capabilities":["code"],"health":"up","auth":"ok"},
                {"id":"account-b","account_id":"b","pool_ids":["pool-b"],"capabilities":["code"],"health":"up","auth":"ok"}],
            "pools":[
                {"id":"pool-a","windows":[{"id":"run","unit":"points","remaining_milli":100000,"protected_milli":0,"reserved_milli":0,"confidence":"exact","expires_ms":at+60000}]},
                {"id":"pool-b","windows":[{"id":"run","unit":"points","remaining_milli":100000,"protected_milli":0,"reserved_milli":0,"confidence":"exact","expires_ms":at+60000}]}]},
        "required_capabilities":["code"],"estimate_milli":{"points":100},"purpose":"worker"}));
    assert_eq!(result["status"], "admitted", "{result}");
    result
}

#[test]
fn hundred_job_summary_and_scoped_large_artifact_context() {
    let mut d = Daemon::start(&[]);
    let run = d.call(
        "swarm.create",
        json!({"category":"Context fixture", "objective":"Audit backend",
        "allowed_targets":["account-a","account-b"]}),
    );
    let id = run["id"].as_str().unwrap();
    let mut jobs = vec![
        json!({"id":"parent","title":"Inspect API contract","acceptance":"Save checked evidence","deps":[]}),
        json!({"id":"child-a","title":"Check client A","acceptance":"Verify parent contract","deps":["parent"]}),
        json!({"id":"child-b","title":"Check client B","acceptance":"Verify parent contract","deps":["parent"]}),
    ];
    for n in 0..97 {
        jobs.push(json!({"id":format!("j{n:02}"),"title":format!("Independent {n}"),"acceptance":"Evidence","deps":[]}));
    }
    d.call(
        "swarm.plan",
        json!({"id":id,"generation":1,"revision":0,"jobs":jobs}),
    );
    let summary = d.call(
        "swarm.director.summary",
        json!({"run_id":id,"generation":1,"revision":1,"max_inline_bytes":4096}),
    );
    assert!(summary.to_string().len() <= 4096, "{summary}");
    assert_eq!(summary["counts"]["total"], 100);
    assert!(summary["next_cursor"].is_string());

    let parent = admit(&d, id, "parent", "account-a");
    let aid = parent["attempt_id"].as_str().unwrap();
    let token = parent["token"].as_str().unwrap();
    let evidence = "é".repeat(40_000);
    d.call("swarm.artifact.put",json!({"run_id":id,"job_id":"parent","attempt_id":aid,
        "token":token,"artifact_id":"contract-v1","source_revision":1,"kind":"contract","content":evidence}));
    d.call("swarm.report",json!({"run_id":id,"job_id":"parent","attempt_id":aid,"token":token,
        "message_id":"parent-result","type":"result","revision":1,"payload":{"artifact_ids":["contract-v1"]}}));
    d.call(
        "swarm.decide",
        json!({"run_id":id,"generation":1,"revision":1,"job_id":"parent",
        "decision":"accept","evidence":["contract-v1"]}),
    );
    d.call(
        "swarm.attempt.confirm_exit",
        json!({"run_id":id,"generation":1,"revision":1,
        "job_id":"parent","attempt_id":aid}),
    );
    d.call("swarm.artifact.put", json!({"run_id":id,"job_id":"parent",
        "attempt_id":aid,"token":token,"artifact_id":"unreviewed",
        "source_revision":1,"kind":"contract","content":"unreviewed note"}));
    commit_beneficial_batch(&d, id, &["child-a".into(), "child-b".into()]);
    let same = admit(&d, id, "child-a", "account-a");
    let other = admit(&d, id, "child-b", "account-b");
    let brief = d.call(
        "swarm.worker.brief",
        json!({"run_id":id,"job_id":"child-a",
        "attempt_id":same["attempt_id"],"token":same["token"],"max_inline_bytes":4096}),
    );
    assert!(brief.to_string().len() <= 4096, "{brief}");
    assert_eq!(brief["job"]["acceptance"], "Verify parent contract");
    assert_eq!(brief["artifacts"][0]["id"], "contract-v1");
    assert_eq!(brief["artifacts"].as_array().unwrap().len(), 1);
    assert!(!brief.to_string().contains("Independent 42"));
    let denied = d.call(
        "swarm.worker.brief",
        json!({"run_id":id,"job_id":"child-b",
        "attempt_id":other["attempt_id"],"token":other["token"]}),
    );
    assert!(denied["artifacts"].as_array().unwrap().is_empty());
    let chunk = d.call(
        "swarm.context.get",
        json!({"run_id":id,"job_id":"child-a",
        "attempt_id":same["attempt_id"],"token":same["token"],
        "artifact_id":"contract-v1","offset_bytes":0,"max_bytes":4096}),
    );
    assert!(chunk["content"].as_str().unwrap().len() <= 4096);
    assert_eq!(chunk["next_offset_bytes"], 4096);
    let end = d.call(
        "swarm.context.get",
        json!({"run_id":id,"job_id":"child-a",
        "attempt_id":same["attempt_id"],"token":same["token"],
        "artifact_id":"contract-v1","offset_bytes":4096,"max_bytes":16384}),
    );
    assert_eq!(end["next_offset_bytes"], 20480);
    assert!(d
        .try_call(
            "swarm.context.get",
            json!({"run_id":id,"job_id":"child-b",
        "attempt_id":other["attempt_id"],"token":other["token"],"artifact_id":"contract-v1"})
        )
        .is_err());
    assert!(d.try_call("swarm.context.get", json!({"run_id":id,"job_id":"child-a",
        "attempt_id":same["attempt_id"],"token":same["token"],
        "artifact_id":"unreviewed"})).is_err());
    assert!(d.try_call("swarm.context.grant", json!({"run_id":id,
        "generation":1,"revision":1,"artifact_id":"unreviewed",
        "target_id":"account-b"})).is_err());
    let grant = json!({"run_id":id,"generation":1,"revision":1,
        "artifact_id":"contract-v1","target_id":"account-b"});
    assert!(d.try_call("swarm.context.grant", json!({"run_id":id,
        "generation":0,"revision":1,"artifact_id":"contract-v1",
        "target_id":"account-b"})).is_err());
    assert!(d.try_call("swarm.context.grant", json!({"run_id":id,
        "generation":1,"revision":1,"artifact_id":"contract-v1",
        "target_id":"account-c"})).is_err());
    assert_eq!(d.call("swarm.context.grant", grant.clone())["status"], "granted");
    assert_eq!(d.call("swarm.context.grant", grant.clone())["duplicate"], true);
    d.kill9();
    d.spawn();
    let transferred = d.call("swarm.context.get", json!({"run_id":id,"job_id":"child-b",
        "attempt_id":other["attempt_id"],"token":other["token"],
        "artifact_id":"contract-v1","max_bytes":4096}));
    assert_eq!(transferred["content"].as_str().unwrap().len(), 4096);
    let other_brief = d.call("swarm.worker.brief", json!({"run_id":id,"job_id":"child-b",
        "attempt_id":other["attempt_id"],"token":other["token"],"max_inline_bytes":4096}));
    assert_eq!(other_brief["artifacts"][0]["id"], "contract-v1");
    d.call("swarm.context.revoke", grant.clone());
    assert!(d.try_call("swarm.context.get", json!({"run_id":id,"job_id":"child-b",
        "attempt_id":other["attempt_id"],"token":other["token"],
        "artifact_id":"contract-v1"})).is_err());
    assert!(d.try_call("swarm.context.grant", grant).is_err());
    assert_eq!(d.call("swarm.context.get", json!({"run_id":id,"job_id":"child-a",
        "attempt_id":same["attempt_id"],"token":same["token"],
        "artifact_id":"contract-v1","max_bytes":4096}))["content"].as_str().unwrap().len(),4096);
    assert!(d
        .try_call(
            "swarm.context.get",
            json!({"run_id":id,"job_id":"child-a",
        "attempt_id":same["attempt_id"],"token":other["token"],"artifact_id":"contract-v1"})
        )
        .is_err());
    assert!(d.try_call("swarm.context.get",json!({"run_id":id,"job_id":"child-a",
        "attempt_id":same["attempt_id"],"token":same["token"],"artifact_id":"contract-v1","offset_bytes":1})).is_err());
    let temp = tmp();
    let checkout = repo(&temp.path().join("context-worker"));
    let launched = d.call(
        "swarm.worker.launch",
        json!({"run_id":id,"job_id":"child-a",
        "attempt_id":same["attempt_id"],"token":same["token"],"repo":checkout,
        "program":"/bin/true","args":[],"prompt":"Check the client","title":"Context worker"}),
    );
    assert_eq!(launched["status"], "launched");
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let saved_prompt: String = db
        .query_row(
            "SELECT t.prompt FROM tasks t JOIN runs r ON r.task_id=t.id WHERE r.id=?1",
            [launched["overseer_run_id"].as_str().unwrap()],
            |r| r.get(0),
        )
        .unwrap();
    assert!(saved_prompt.contains("contract-v1"));
    assert!(saved_prompt.contains("Verify parent contract"));
    assert!(!saved_prompt.contains(&evidence));
    assert!(saved_prompt.len() <= 32 * 1024);
    d.call("swarm.report", json!({"run_id":id,"job_id":"parent",
        "attempt_id":aid,"token":token,"message_id":"late-source-result",
        "type":"result","revision":1,
        "payload":{"artifact_ids":["unreviewed"]}}));
    assert!(d.try_call("swarm.context.get", json!({"run_id":id,"job_id":"child-a",
        "attempt_id":same["attempt_id"],"token":same["token"],
        "artifact_id":"contract-v1"})).is_err());
}

#[test]
fn revoked_artifact_stops_dependent_delivery_and_worker_but_not_unrelated_work() {
    let d = Daemon::start(&[]);
    let temp = tmp();
    let checkout = repo(&temp.path().join("revocation-source"));
    let run = d.call("swarm.create", json!({"category":"Revoked context",
        "objective":"Inspect backend","allowed_targets":["account-a"]}));
    let id = run["id"].as_str().unwrap();
    d.call("swarm.plan", json!({"id":id,"generation":1,"revision":0,"jobs":[
        {"id":"contract","title":"Contract","acceptance":"evidence","deps":[]},
        {"id":"dependent","title":"Dependent","acceptance":"evidence","deps":["contract"]},
        {"id":"later","title":"Later dependent","acceptance":"evidence","deps":["contract"]},
        {"id":"unrelated","title":"Unrelated","acceptance":"evidence","deps":[]}
    ]}));
    let parent = admit(&d, id, "contract", "account-a");
    d.call("swarm.artifact.put", json!({"run_id":id,"job_id":"contract",
        "attempt_id":parent["attempt_id"],"token":parent["token"],
        "artifact_id":"contract-evidence","source_revision":1,
        "kind":"contract","content":"checked interface"}));
    d.call("swarm.report", json!({"run_id":id,"job_id":"contract",
        "attempt_id":parent["attempt_id"],"token":parent["token"],
        "message_id":"contract-result","type":"result","revision":1,
        "payload":{"artifact_ids":["contract-evidence"]}}));
    d.call("swarm.decide", json!({"run_id":id,"generation":1,"revision":1,
        "job_id":"contract","decision":"accept","evidence":["contract-evidence"]}));
    d.call("swarm.attempt.confirm_exit", json!({"run_id":id,"generation":1,
        "revision":1,"job_id":"contract","attempt_id":parent["attempt_id"]}));
    commit_beneficial_batch(&d, id, &["dependent".into(), "later".into(), "unrelated".into()]);
    let dependent = admit(&d, id, "dependent", "account-a");
    let context = json!({"run_id":id,"job_id":"dependent",
        "attempt_id":dependent["attempt_id"],"token":dependent["token"],
        "artifact_id":"contract-evidence"});
    assert_eq!(d.call("swarm.context.get", context.clone())["content"], "checked interface");
    let launched = d.call("swarm.worker.launch", json!({"run_id":id,
        "job_id":"dependent","attempt_id":dependent["attempt_id"],
        "token":dependent["token"],"repo":checkout,
        "program":"/bin/sleep","args":["30"],"prompt":"Use contract",
        "title":"Dependent worker"}));
    assert_eq!(launched["status"], "launched");
    let worker = launched["overseer_run_id"].as_str().unwrap();
    d.wait_status(worker, |status| status == "running", 10);
    let revoked = d.call("swarm.context.revoke", json!({"run_id":id,
        "generation":1,"revision":1,"artifact_id":"contract-evidence",
        "target_id":"account-a"}));
    assert_eq!(revoked["status"], "revoked");
    assert!(revoked["interrupt_requested"].as_array().unwrap().iter().any(|r| r == worker));
    assert!(d.try_call("swarm.context.get", context).is_err());
    assert_ne!(d.wait_done(worker, 10)["status"], "completed");
    let repeated = d.call("swarm.context.revoke", json!({"run_id":id,
        "generation":1,"revision":1,"artifact_id":"contract-evidence",
        "target_id":"account-a"}));
    assert_eq!(repeated["duplicate"], true);
    assert!(repeated["interrupt_requested"].as_array().unwrap().is_empty());
    assert!(d.try_call("swarm.context.revoke", json!({"run_id":id,
        "generation":0,"revision":1,"artifact_id":"contract-evidence",
        "target_id":"account-a"})).is_err());
    assert_eq!(d.call("swarm.admit", json!({"run_id":id,"generation":1,
        "revision":1,"job_id":"later","target_id":"account-a",
        "request_id":"revoked-later","now_ms":now(),
        "snapshot":{"version":1,"observed_ms":now()-1000,"expires_ms":now()+60000,
            "targets":[{"id":"account-a","account_id":"a","pool_ids":["pool-a"],
                "capabilities":["code"],"health":"up","auth":"ok"}],
            "pools":[{"id":"pool-a","windows":[{"id":"run","unit":"points",
                "remaining_milli":100000,"protected_milli":0,"reserved_milli":0,
                "confidence":"exact","expires_ms":now()+60000}]}]},
        "required_capabilities":["code"],"estimate_milli":{"points":100},
        "purpose":"worker"}))["reason"], "artifact_permission_revoked");
    assert_eq!(admit(&d, id, "unrelated", "account-a")["status"], "admitted");
}

#[test]
fn revoked_artifact_interrupt_retries_after_daemon_crash() {
    let mut d = Daemon::start(&[]);
    let temp = tmp();
    let checkout = repo(&temp.path().join("revocation-crash-source"));
    let run = d.call("swarm.create", json!({"category":"Revocation crash",
        "objective":"Inspect backend","allowed_targets":["account-a"]}));
    let id = run["id"].as_str().unwrap();
    d.call("swarm.plan", json!({"id":id,"generation":1,"revision":0,"jobs":[
        {"id":"source","title":"Contract","acceptance":"evidence","deps":[]},
        {"id":"child","title":"Check dependent","acceptance":"evidence","deps":["source"]},
        {"id":"sibling","title":"Independent work","acceptance":"evidence","deps":[]}
    ]}));
    let source = d.call("swarm.attempt.register", json!({"run_id":id,"job_id":"source",
        "generation":1,"revision":1}));
    d.call("swarm.artifact.put", json!({"run_id":id,"job_id":"source",
        "attempt_id":source["id"],"token":source["token"],
        "artifact_id":"contract","source_revision":1,"kind":"contract",
        "content":"checked contract"}));
    d.call("swarm.report", json!({"run_id":id,"job_id":"source",
        "attempt_id":source["id"],"token":source["token"],
        "message_id":"source-result","type":"result","revision":1,
        "payload":{"artifact_ids":["contract"]}}));
    d.call("swarm.decide", json!({"run_id":id,"generation":1,"revision":1,
        "job_id":"source","decision":"accept","evidence":["contract"]}));
    d.call("swarm.attempt.confirm_exit", json!({"run_id":id,"generation":1,
        "revision":1,"job_id":"source","attempt_id":source["id"]}));
    commit_beneficial_batch(&d,id,&["child".into(),"sibling".into()]);
    let child = admit(&d,id,"child","account-a");
    let launched = d.call("swarm.worker.launch", json!({"run_id":id,
        "job_id":"child","attempt_id":child["attempt_id"],
        "token":child["token"],"repo":checkout,
        "program":"/bin/sleep","args":["30"],"prompt":"Check contract",
        "title":"Revoked worker"}));
    let worker = launched["overseer_run_id"].as_str().unwrap();
    d.wait_status(worker, |status| status == "running", 10);
    let sibling = admit(&d,id,"sibling","account-a");
    let sibling_launch = d.call("swarm.worker.launch", json!({"run_id":id,
        "job_id":"sibling","attempt_id":sibling["attempt_id"],
        "token":sibling["token"],"repo":repo(&temp.path().join("revocation-sibling")),
        "program":"/bin/sleep","args":["30"],"prompt":"Independent work",
        "title":"Unrelated worker"}));
    let sibling_worker = sibling_launch["overseer_run_id"].as_str().unwrap();
    d.wait_status(sibling_worker, |status| status == "running", 10);
    let revoked = d.call("swarm.context.revoke", json!({"run_id":id,
        "generation":1,"revision":1,"artifact_id":"contract",
        "target_id":"account-a","fault_persist_only":true}));
    assert_eq!(revoked["status"], "revoked");
    assert!(revoked["interrupt_requested"].as_array().unwrap().is_empty());
    assert_eq!(d.run(worker)["status"], "running");
    d.kill9();
    d.spawn();
    assert_eq!(d.wait_done(worker,10)["status"], "interrupted");
    assert_eq!(d.run(sibling_worker)["status"], "running");
    assert!(d.try_call("swarm.context.get", json!({"run_id":id,"job_id":"child",
        "attempt_id":child["attempt_id"],"token":child["token"],
        "artifact_id":"contract"})).is_err());
    d.call("swarm.stop", json!({"run_id":id,"generation":1,"revision":1}));
    assert_eq!(d.wait_done(sibling_worker,10)["status"], "interrupted");
}

#[test]
fn stop_revocation_result_and_acceptance_keep_one_durable_order() {
    for steps in [
        ["result","accept","revoke","stop"],
        ["result","revoke","accept","stop"],
        ["stop","result","revoke","accept"],
        ["revoke","stop","result","accept"],
    ] {
        let d = Daemon::start(&[]);
        let temp = tmp();
        let checkout = repo(&temp.path().join("race-source"));
        let run = d.call("swarm.create", json!({"category":format!("Race {steps:?}"),
            "objective":"Inspect backend","allowed_targets":["account-a"]}));
        let id = run["id"].as_str().unwrap();
        d.call("swarm.plan", json!({"id":id,"generation":1,"revision":0,"jobs":[
            {"id":"source","title":"Contract","acceptance":"evidence","deps":[]},
            {"id":"child","title":"Dependent","acceptance":"evidence","deps":["source"]}
        ]}));
        let source = d.call("swarm.attempt.register", json!({"run_id":id,"job_id":"source",
            "generation":1,"revision":1}));
        d.call("swarm.artifact.put", json!({"run_id":id,"job_id":"source",
            "attempt_id":source["id"],"token":source["token"],
            "artifact_id":"contract","source_revision":1,"kind":"contract",
            "content":"checked contract"}));
        d.call("swarm.report", json!({"run_id":id,"job_id":"source",
            "attempt_id":source["id"],"token":source["token"],
            "message_id":"source-result","type":"result","revision":1,
            "payload":{"artifact_ids":["contract"]}}));
        d.call("swarm.decide", json!({"run_id":id,"generation":1,"revision":1,
            "job_id":"source","decision":"accept","evidence":["contract"]}));
        d.call("swarm.attempt.confirm_exit", json!({"run_id":id,"generation":1,
            "revision":1,"job_id":"source","attempt_id":source["id"]}));
        let child = admit(&d,id,"child","account-a");
        d.call("swarm.artifact.put", json!({"run_id":id,"job_id":"child",
            "attempt_id":child["attempt_id"],"token":child["token"],
            "artifact_id":"child-proof","source_revision":1,"kind":"finding",
            "content":"local reproduction"}));
        let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
        let before: i64 = db.query_row("SELECT COALESCE(MAX(seq),0) FROM swarm_operation_order",
            [], |r| r.get(0)).unwrap();
        let mut accepted = false;
        for step in steps {
            match step {
                "result" => { d.call("swarm.report", json!({"run_id":id,"job_id":"child",
                    "attempt_id":child["attempt_id"],"token":child["token"],
                    "message_id":"child-result","type":"result","revision":1,
                    "payload":{"artifact_ids":["child-proof"]}})); }
                "accept" => {
                    accepted = d.try_call("swarm.decide", json!({"run_id":id,
                        "generation":1,"revision":1,"job_id":"child",
                        "decision":"accept","evidence":["child-proof"]})).is_ok();
                }
                "revoke" => { d.call("swarm.context.revoke", json!({"run_id":id,
                    "generation":1,"revision":1,"artifact_id":"contract",
                    "target_id":"account-a"})); }
                "stop" => { d.call("swarm.stop", json!({"run_id":id,
                    "generation":1,"revision":1})); }
                _ => unreachable!(),
            }
        }
        assert_eq!(accepted, steps == ["result","accept","revoke","stop"]);
        let mut stmt = db.prepare("SELECT kind FROM swarm_operation_order WHERE run_id=?1 AND seq>?2 ORDER BY seq").unwrap();
        let observed: Vec<String> = stmt.query_map(rusqlite::params![id,before], |r| r.get(0))
            .unwrap().collect::<rusqlite::Result<_>>().unwrap();
        let expected: Vec<String> = steps.iter().filter(|step| **step != "accept" || accepted)
            .map(|step| step.to_string()).collect();
        assert_eq!(observed, expected, "{steps:?}");
        assert_eq!(d.call("swarm.get", json!({"id":id}))["status"], "stopping");
        let jobs = d.call("swarm.jobs", json!({"id":id}));
        assert_eq!(jobs["jobs"].as_array().unwrap().iter()
            .find(|job| job["id"] == "child").unwrap()["status"],
            if steps[0] == "stop" { "cancel_requested" } else { "blocked" });
        let inbox = d.call("swarm.messages", json!({"run_id":id,"recipient":"director"}));
        assert!(inbox["messages"].as_array().unwrap().iter()
            .any(|m| m["message_id"] == "child-result"));
        assert!(d.try_call("swarm.complete", json!({"run_id":id,
            "generation":1,"revision":1,"summary":"finished",
            "verification":"fixture","checks":[]})).is_err());
        assert!(d.try_call("swarm.worker.launch", json!({"run_id":id,
            "job_id":"child","attempt_id":child["attempt_id"],
            "token":child["token"],"repo":checkout,
            "program":"/bin/true","args":[],"prompt":"Inspect",
            "title":"Late worker"})).is_err());
        assert_eq!(d.call("swarm.stop", json!({"run_id":id,
            "generation":1,"revision":1}))["duplicate"], true);
        assert_eq!(d.call("swarm.context.revoke", json!({"run_id":id,
            "generation":1,"revision":1,"artifact_id":"contract",
            "target_id":"account-a"}))["duplicate"], true);
        assert_eq!(d.call("swarm.report", json!({"run_id":id,"job_id":"child",
            "attempt_id":child["attempt_id"],"token":child["token"],
            "message_id":"child-result","type":"result","revision":1,
            "payload":{"artifact_ids":["child-proof"]}}))["duplicate"], true);
        let after: i64 = db.query_row("SELECT COALESCE(MAX(seq),0) FROM swarm_operation_order",
            [], |r| r.get(0)).unwrap();
        assert_eq!(after - before, observed.len() as i64);
    }
}
