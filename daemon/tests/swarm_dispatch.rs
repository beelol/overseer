mod common;

use common::*;
use serde_json::json;
use std::time::{SystemTime, UNIX_EPOCH};

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

#[test]
fn dispatch_recovers_admitted_but_unlaunched_worker_without_duplicate_attempt() {
    let mut d = Daemon::start(&[]);
    let temp = tmp();
    let checkout = repo(&temp.path().join("dispatch-source"));
    let run = d.call(
        "swarm.create",
        json!({"category":"Dispatch backend","objective":"Audit API",
        "allowed_targets":["fixture"]}),
    );
    let run_id = run["id"].as_str().unwrap();
    d.call(
        "swarm.plan",
        json!({"id":run_id,"generation":1,"revision":0,"jobs":[
            {"id":"routes","title":"Inspect routes","acceptance":"Route evidence","deps":[]},
            {"id":"db","title":"Inspect database","acceptance":"DB evidence","deps":[]},
            {"id":"verify","title":"Verify backend","acceptance":"Verification evidence","deps":[]}
        ]}),
    );
    let at = now();
    let base = json!({"request_id":"dispatch-one","target_id":"fixture","repo":checkout,
        "program":"/bin/sleep","args":["30"],"now_ms":at,
        "snapshot":{"version":1,"observed_ms":at-1000,"expires_ms":at+60000,
            "targets":[{"id":"fixture","account_id":"fixture","pool_ids":["pool"],
                "capabilities":["code"],"health":"up","auth":"ok"}],
            "pools":[{"id":"pool","windows":[{"id":"run","unit":"points",
                "remaining_milli":100000,"protected_milli":0,"reserved_milli":0,
                "confidence":"exact","expires_ms":at+60000}]}]},
        "required_capabilities":["code"],"estimate_milli":{"points":100},
        "purpose":"worker","inject_failure_after_admit_once":true});
    assert!(d
        .try_call("swarm.dispatch.next", base.clone())
        .unwrap_err()
        .contains("injected"));
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let attempts: i64 = db
        .query_row("SELECT COUNT(*) FROM swarm_attempts", [], |r| r.get(0))
        .unwrap();
    assert_eq!(attempts, 1);
    assert!(d.runs().is_empty());
    drop(db);
    d.kill9();
    d.spawn();
    // Startup reconciles only the already-admitted intent. No caller retry is needed.
    assert_eq!(d.runs().len(), 1);
    let first = d.call("swarm.dispatch.next", base.clone());
    assert_eq!(first["status"], "linked", "{first}");
    assert_eq!(first["run_id"], run_id);
    assert_eq!(first["job_id"], "db");
    let worker = first["overseer_run_id"].as_str().unwrap();
    d.kill9();
    d.spawn();
    let replay = d.call("swarm.dispatch.next", base.clone());
    assert_eq!(replay["overseer_run_id"], worker);
    assert_eq!(replay["duplicate"], true);
    assert!(d
        .try_call(
            "swarm.dispatch.next",
            json!({"request_id":"dispatch-one",
        "target_id":"other","repo":checkout,"program":"/bin/sleep","args":["30"],
        "now_ms":at,"snapshot":base["snapshot"],"required_capabilities":["code"],
        "estimate_milli":{"points":100},"purpose":"worker",
        "inject_failure_after_admit_once":true})
        )
        .is_err());
    let shared_snapshot = base["snapshot"].clone();
    let mut second = base;
    second["request_id"] = json!("dispatch-two");
    second["inject_failure_after_admit_once"] = json!(false);
    let next = d.call("swarm.dispatch.next", second);
    assert_eq!(next["status"], "launched", "{next}");
    assert_eq!(next["job_id"], "routes");
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let attempts: i64 = db
        .query_row("SELECT COUNT(*) FROM swarm_attempts", [], |r| r.get(0))
        .unwrap();
    assert_eq!(attempts, 2);
    let launches: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM swarm_worker_launches WHERE overseer_run_id IS NOT NULL",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(launches, 2);
    drop(db);
    let pending = json!({"request_id":"dispatch-three","target_id":"fixture","repo":checkout,
        "program":"/bin/sleep","args":["30"],"now_ms":at,
        "snapshot":shared_snapshot,"required_capabilities":["code"],
        "estimate_milli":{"points":100},"purpose":"worker",
        "inject_failure_after_admit_once":true});
    assert!(d
        .try_call("swarm.dispatch.next", pending.clone())
        .unwrap_err()
        .contains("injected"));
    d.call(
        "swarm.stop",
        json!({"run_id":run_id,"generation":1,"revision":1}),
    );
    assert!(d.try_call("swarm.dispatch.next", pending).is_err());
    assert_eq!(d.runs().len(), 2);
    d.wait_done(worker, 5);
}

#[test]
fn startup_does_not_launch_an_admitted_worker_from_an_expired_snapshot() {
    let mut d = Daemon::start(&[]);
    let temp = tmp();
    let checkout = repo(&temp.path().join("expired-dispatch-source"));
    let run = d.call(
        "swarm.create",
        json!({"category":"Expired dispatch","objective":"Inspect backend",
            "allowed_targets":["fixture"]}),
    );
    let run_id = run["id"].as_str().unwrap();
    d.call(
        "swarm.plan",
        json!({"id":run_id,"generation":1,"revision":0,"jobs":[
            {"id":"inspect","title":"Inspect backend","acceptance":"Evidence","deps":[]}
        ]}),
    );
    let at = now();
    let request = json!({"request_id":"expiring-dispatch","target_id":"fixture",
        "repo":checkout,"program":"/bin/sleep","args":["30"],"now_ms":at,
        "snapshot":{"version":1,"observed_ms":at-1000,"expires_ms":at+150,
            "targets":[{"id":"fixture","account_id":"fixture","pool_ids":["pool"],
                "capabilities":["code"],"health":"up","auth":"ok"}],
            "pools":[{"id":"pool","windows":[{"id":"run","unit":"points",
                "remaining_milli":100000,"protected_milli":0,"reserved_milli":0,
                "confidence":"exact","expires_ms":at+150}]}]},
        "required_capabilities":["code"],"estimate_milli":{"points":100},
        "purpose":"worker","inject_failure_after_admit_once":true});
    assert!(d.try_call("swarm.dispatch.next", request.clone()).is_err());
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    db.execute(
        "UPDATE swarm_runs SET allowed_targets='[]' WHERE id=?1",
        [run_id],
    )
    .unwrap();
    assert!(d
        .try_call("swarm.dispatch.next", request.clone())
        .unwrap_err()
        .contains("permission revoked"));
    db.execute(
        "UPDATE swarm_runs SET allowed_targets='[\"fixture\"]' WHERE id=?1",
        [run_id],
    )
    .unwrap();
    drop(db);
    std::thread::sleep(std::time::Duration::from_millis(200));
    d.kill9();
    d.spawn();
    assert!(d.runs().is_empty());
    assert!(d
        .try_call("swarm.dispatch.next", request)
        .unwrap_err()
        .contains("expired"));
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let attempts: i64 = db
        .query_row("SELECT COUNT(*) FROM swarm_attempts", [], |r| r.get(0))
        .unwrap();
    assert_eq!(attempts, 1);
}

#[test]
fn supervised_scripted_workers_report_evidence_that_unlocks_dependent_work() {
    let mut d = Daemon::start(&[]);
    let temp = tmp();
    let checkout = repo(&temp.path().join("worker-report-source"));
    let script = temp.path().join("report-worker.sh");
    std::fs::write(&script, r#"#!/bin/sh
set -eu
test -n "$OVERSEER_SWARM_RUN_ID"
test -n "$OVERSEER_SWARM_JOB_ID"
test -n "$OVERSEER_SWARM_ATTEMPT_ID"
test -n "$OVERSEER_SWARM_TOKEN"
test -n "$OVERSEER_SWARM_REVISION"
artifact="proof-$OVERSEER_SWARM_ATTEMPT_ID"
written=$("$OVERSEER_BIN" ctl swarm.artifact.put "{\"run_id\":\"$OVERSEER_SWARM_RUN_ID\",\"job_id\":\"$OVERSEER_SWARM_JOB_ID\",\"attempt_id\":\"$OVERSEER_SWARM_ATTEMPT_ID\",\"token\":\"$OVERSEER_SWARM_TOKEN\",\"artifact_id\":\"$artifact\",\"source_revision\":$OVERSEER_SWARM_REVISION,\"kind\":\"finding\",\"content\":\"scripted evidence\"}")
case "$written" in *'"error"'*) exit 2;; esac
reported=$("$OVERSEER_BIN" ctl swarm.report "{\"run_id\":\"$OVERSEER_SWARM_RUN_ID\",\"job_id\":\"$OVERSEER_SWARM_JOB_ID\",\"attempt_id\":\"$OVERSEER_SWARM_ATTEMPT_ID\",\"token\":\"$OVERSEER_SWARM_TOKEN\",\"message_id\":\"result-$OVERSEER_SWARM_ATTEMPT_ID\",\"type\":\"result\",\"revision\":$OVERSEER_SWARM_REVISION,\"payload\":{\"artifact_ids\":[\"$artifact\"]}}")
case "$reported" in *'"error"'*) exit 3;; esac
"#).unwrap();
    let run = d.call(
        "swarm.create",
        json!({"category":"Fixture endpoint work",
        "objective":"Set contract then verify endpoint","allowed_targets":["fixture"]}),
    );
    let run_id = run["id"].as_str().unwrap();
    d.call("swarm.plan",json!({"id":run_id,"generation":1,"revision":0,"jobs":[
        {"id":"contract","title":"Define endpoint contract","acceptance":"Contract evidence","deps":[]},
        {"id":"endpoint","title":"Verify endpoint","acceptance":"Endpoint evidence","deps":["contract"]}
    ]}));
    let at = now();
    let snapshot = json!({"version":1,"observed_ms":at-1000,"expires_ms":at+60000,
        "targets":[{"id":"fixture","account_id":"fixture","pool_ids":["pool"],
            "capabilities":["code"],"health":"up","auth":"ok"}],
        "pools":[{"id":"pool","windows":[{"id":"run","unit":"points",
            "remaining_milli":100000,"protected_milli":0,"reserved_milli":0,
            "confidence":"exact","expires_ms":at+60000}]}]});
    let mut checks = Vec::new();
    for (request_id, job_id) in [
        ("contract-launch", "contract"),
        ("endpoint-launch", "endpoint"),
    ] {
        let dispatched = d.call(
            "swarm.dispatch.next",
            json!({"request_id":request_id,
            "target_id":"fixture","repo":checkout,"program":"/bin/sh","args":[script],
            "now_ms":at,"snapshot":snapshot,"required_capabilities":["code"],
            "estimate_milli":{"points":100},"purpose":"worker"}),
        );
        assert_eq!(dispatched["job_id"], job_id, "{dispatched}");
        let worker = dispatched["overseer_run_id"].as_str().unwrap();
        let attempt = dispatched["attempt_id"].as_str().unwrap();
        assert_eq!(d.wait_done(worker, 5)["status"], "completed");
        let messages = d.call(
            "swarm.messages",
            json!({"run_id":run_id,"recipient":"director"}),
        );
        let artifact = format!("proof-{attempt}");
        assert!(
            messages["messages"]
                .as_array()
                .unwrap()
                .iter()
                .any(|m| m["type"] == "result"
                    && m["job_id"] == job_id
                    && m["payload"]["artifact_ids"][0] == artifact),
            "{messages}"
        );
        d.call(
            "swarm.worker.reconcile",
            json!({"run_id":run_id,"job_id":job_id,
            "attempt_id":attempt,"generation":1,"revision":1}),
        );
        let batch = d.call(
            "swarm.director.claim_batch",
            json!({"run_id":run_id,
            "generation":1,"revision":1,"now_ms":now()+6000}),
        );
        assert_eq!(batch["status"], "claimed", "{batch}");
        d.call(
            "swarm.decide",
            json!({"run_id":run_id,"generation":1,"revision":1,
            "job_id":job_id,"decision":"accept","evidence":[artifact]}),
        );
        d.call(
            "swarm.director.complete_batch",
            json!({"run_id":run_id,"generation":1,
            "turn_id":batch["turn_id"],"token":batch["token"],"outcome":"progress"}),
        );
        checks.push(json!({"job_id":job_id,"outcome":"passed","evidence":[artifact]}));
        if job_id == "contract" {
            assert!(d.try_call("swarm.complete",json!({"run_id":run_id,"generation":1,
                "revision":1,"request_id":"finish-before-endpoint",
                "summary":"Contract checked; endpoint pending","verification":"Endpoint still pending",
                "checks":checks})).is_err());
        }
    }
    let jobs = d.call("swarm.jobs", json!({"id":run_id}));
    assert_eq!(jobs["jobs"][0]["status"], "accepted");
    assert_eq!(jobs["jobs"][1]["status"], "accepted");
    let final_request = json!({"run_id":run_id,"generation":1,"revision":1,
        "request_id":"finish-verified-backend",
        "summary":"Contract and endpoint checks are complete.",
        "verification":"The scripted endpoint check used the accepted contract finding.",
        "checks":checks});
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    db.execute("UPDATE swarm_artifacts SET content='tampered after review' WHERE run_id=?1 AND job_id='contract'",
        rusqlite::params![run_id]).unwrap();
    assert!(d
        .try_call("swarm.complete", final_request.clone())
        .unwrap_err()
        .contains("integrity"));
    assert_eq!(
        d.call("swarm.get", json!({"id":run_id}))["status"],
        "running"
    );
    db.execute("UPDATE swarm_artifacts SET content='scripted evidence' WHERE run_id=?1 AND job_id='contract'",
        rusqlite::params![run_id]).unwrap();
    let completed = d.call("swarm.complete", final_request.clone());
    assert_eq!(completed["status"], "completed");
    assert_eq!(
        d.call("swarm.complete", final_request.clone())["duplicate"],
        true
    );
    let mut changed = final_request;
    changed["summary"] = json!("An unreviewed alternative summary");
    assert!(d.try_call("swarm.complete", changed).is_err());
    d.kill9();
    d.spawn();
    let recovered = d.call("swarm.get", json!({"id":run_id}));
    assert_eq!(recovered["status"], "completed");
    assert_eq!(
        recovered["completion"]["checks"].as_array().unwrap().len(),
        2
    );
}

#[test]
fn supervised_scripted_worker_receives_and_applies_targeted_director_advisory() {
    let d = Daemon::start(&[]);
    let temp = tmp();
    let checkout = repo(&temp.path().join("worker-advisory-source"));
    let script = temp.path().join("receive-advisory.sh");
    std::fs::write(&script, r#"#!/bin/sh
set -eu
count=0
while [ "$count" -lt 100 ]; do
    inbox=$("$OVERSEER_BIN" ctl swarm.messages "{\"run_id\":\"$OVERSEER_SWARM_RUN_ID\",\"recipient\":\"$OVERSEER_SWARM_ATTEMPT_ID\"}")
    case "$inbox" in *'"message_id":"targeted-note"'*) break;; esac
    count=$((count + 1))
    sleep 0.05
done
test "$count" -lt 100
delivered=$("$OVERSEER_BIN" ctl swarm.ack "{\"run_id\":\"$OVERSEER_SWARM_RUN_ID\",\"message_id\":\"targeted-note\",\"recipient\":\"$OVERSEER_SWARM_ATTEMPT_ID\",\"token\":\"$OVERSEER_SWARM_TOKEN\",\"revision\":$OVERSEER_SWARM_REVISION,\"phase\":\"delivered\"}")
case "$delivered" in *'"error"'*) exit 2;; esac
applied=$("$OVERSEER_BIN" ctl swarm.ack "{\"run_id\":\"$OVERSEER_SWARM_RUN_ID\",\"message_id\":\"targeted-note\",\"recipient\":\"$OVERSEER_SWARM_ATTEMPT_ID\",\"token\":\"$OVERSEER_SWARM_TOKEN\",\"revision\":$OVERSEER_SWARM_REVISION,\"phase\":\"applied\"}")
case "$applied" in *'"error"'*) exit 3;; esac
"#).unwrap();
    let run = d.call(
        "swarm.create",
        json!({"category":"Fixture advisory",
        "objective":"Inspect API routes","allowed_targets":["fixture"]}),
    );
    let run_id = run["id"].as_str().unwrap();
    d.call(
        "swarm.plan",
        json!({"id":run_id,"generation":1,"revision":0,"jobs":[
            {"id":"inspect","title":"Inspect routes","acceptance":"Finding","deps":[]}
        ]}),
    );
    let at = now();
    let dispatched = d.call(
        "swarm.dispatch.next",
        json!({"request_id":"advisory-worker",
        "target_id":"fixture","repo":checkout,"program":"/bin/sh","args":[script],
        "now_ms":at,"snapshot":{"version":1,"observed_ms":at-1000,
            "expires_ms":at+60000,"targets":[{"id":"fixture","account_id":"fixture",
                "pool_ids":["pool"],"capabilities":["code"],"health":"up","auth":"ok"}],
            "pools":[{"id":"pool","windows":[{"id":"run","unit":"points",
                "remaining_milli":100000,"protected_milli":0,"reserved_milli":0,
                "confidence":"exact","expires_ms":at+60000}]}]},
        "required_capabilities":["code"],"estimate_milli":{"points":100},"purpose":"worker"}),
    );
    let worker = dispatched["overseer_run_id"].as_str().unwrap();
    let attempt = dispatched["attempt_id"].as_str().unwrap();
    let sent = d.call(
        "swarm.direct",
        json!({"run_id":run_id,"job_id":"inspect",
        "attempt_id":attempt,"generation":1,"revision":1,"message_id":"targeted-note",
        "type":"advisory","payload":{"question":"Check the retry handler"}}),
    );
    assert_eq!(sent["phase"], "queued");
    assert_eq!(d.wait_done(worker, 8)["status"], "completed");
    let messages = d.call(
        "swarm.messages",
        json!({"run_id":run_id,"recipient":attempt}),
    );
    assert_eq!(messages["messages"][0]["phase"], "applied");
    assert_eq!(
        messages["messages"][0]["payload"]["question"],
        "Check the retry handler"
    );
}
