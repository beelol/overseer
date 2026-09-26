mod common;

use common::*;
use serde_json::{json, Value};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

fn snapshot(at: i64, a_up: bool, b_up: bool) -> Value {
    let mut targets = vec![
        json!({"id":"cheap","account_id":"cheap","pool_ids":["cheap-pool"],
        "capabilities":["text"],"health":"up","auth":"ok"}),
    ];
    if a_up {
        targets.push(
            json!({"id":"audit-a","account_id":"a","pool_ids":["pool-a"],
        "capabilities":["audit"],"health":"up","auth":"ok"}),
        );
    }
    if b_up {
        targets.push(
            json!({"id":"audit-b","account_id":"b","pool_ids":["pool-b"],
        "capabilities":["audit"],"health":"up","auth":"ok"}),
        );
    }
    targets.push(
        json!({"id":"not-selected","account_id":"c","pool_ids":["pool-c"],
        "capabilities":["audit"],"health":"up","auth":"ok"}),
    );
    let pools: Vec<Value> = ["cheap-pool", "pool-a", "pool-b", "pool-c"]
        .iter()
        .map(|id| {
            json!({
        "id":id,"windows":[{"id":"run","unit":"points","remaining_milli":1000000,
        "protected_milli":0,"reserved_milli":0,"confidence":"exact","expires_ms":at+60000}]})
        })
        .collect();
    json!({"version":1,"observed_ms":at-1000,"expires_ms":at+60000,
        "targets":targets,"pools":pools})
}

fn admit(
    d: &Daemon,
    run: &str,
    job: &str,
    target: &str,
    snap: &Value,
    at: i64,
    request: &str,
) -> Value {
    d.call(
        "swarm.admit",
        json!({"run_id":run,"generation":1,"revision":1,
        "job_id":job,"target_id":target,"request_id":request,"snapshot":snap,"now_ms":at,
        "required_capabilities":["audit"],"estimate_milli":{"points":100},"purpose":"worker"}),
    )
}

fn record(d: &Daemon, run: &str, job: &str, attempt: &Value, content: &Value) -> String {
    let id = format!("s4-{job}-evidence");
    d.call(
        "swarm.artifact.put",
        json!({"run_id":run,"job_id":job,
        "attempt_id":attempt["attempt_id"],"token":attempt["token"],
        "artifact_id":id,"source_revision":1,"kind":"finding","content":content.to_string()}),
    );
    d.call(
        "swarm.report",
        json!({"run_id":run,"job_id":job,
        "attempt_id":attempt["attempt_id"],"token":attempt["token"],
        "message_id":format!("s4-{job}-result"),"type":"result","revision":1,
        "payload":{"audit_outcome":"negative","artifact_ids":[id]}}),
    );
    id
}

fn accept(d: &Daemon, run: &str, job: &str, attempt: &Value, artifact: &str) {
    assert_eq!(
        d.call(
            "swarm.decide",
            json!({"run_id":run,"generation":1,"revision":1,
        "job_id":job,"decision":"accept","evidence":[artifact]})
        )["status"],
        "accepted"
    );
    d.call(
        "swarm.attempt.confirm_exit",
        json!({"run_id":run,"generation":1,
        "revision":1,"job_id":job,"attempt_id":attempt["attempt_id"]}),
    );
}

// Scripted account snapshots and director choices; no paid provider or live routing.
#[test]
#[ignore = "requires disposable Dispatch PostgreSQL fixture and local socket permission"]
fn shipment_incident_survives_sql_account_loss_with_two_attempts() {
    let fixture = repo_root().join("fixtures/swarm/dispatch-v1");
    let output = Command::new("go")
        .args(["run", "-mod=mod", "."])
        .current_dir(&fixture)
        .env("GOCACHE", "/private/tmp/overseer-swarm-gocache")
        .env("GOPROXY", "off")
        .env("GOSUMDB", "off")
        .env("GOTOOLCHAIN", "local")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let bundle: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(bundle["fixtureVersion"], 1);
    assert!(bundle["request"]["poolWaitMs"].as_i64().unwrap() >= 35);
    assert_eq!(bundle["sql"]["openTransactions"], 1);
    assert_eq!(bundle["queue"]["deliveries"], 2);

    let d = Daemon::start(&[]);
    let checkout_temp = tmp();
    let checkout = repo(&checkout_temp.path().join("dispatch-snapshot"));
    std::fs::copy(fixture.join("shipment.go"), checkout.join("shipment.go")).unwrap();
    git(&checkout, &["add", "shipment.go"]);
    git(
        &checkout,
        &["commit", "-q", "-m", "shipment fixture snapshot"],
    );
    let created = d.call("swarm.create",json!({"category":"Shipment incident audit",
        "objective":"Explain 10:00-10:15 timeout from sanitized bundle and repo snapshot; no service changes",
        "allowed_targets":["audit-a","audit-b","cheap"],"policy":{"max_workers":3}}));
    let run = created["id"].as_str().unwrap();
    assert_eq!(created["source_change_permission"], "none");
    d.call("swarm.plan",json!({"id":run,"generation":1,"revision":0,"jobs":[
        {"id":"l1","title":"Request traces","acceptance":"pool wait timeline","deps":[]},
        {"id":"l2","title":"SQL and pool","acceptance":"held transaction evidence","deps":[]},
        {"id":"l3","title":"Queue redelivery","acceptance":"same handler path evidence","deps":[]}
    ]}));
    commit_beneficial_batch(&d, run, &["l1".into(), "l2".into(), "l3".into()]);
    let at = now();
    let first = snapshot(at, true, true);
    let cheap = admit(&d, run, "l2", "cheap", &first, at, "s4-cheap");
    assert_eq!(cheap["status"], "blocked");
    let excluded = admit(&d, run, "l2", "not-selected", &first, at, "s4-excluded");
    assert_eq!(excluded["status"], "blocked");
    let l1 = admit(&d, run, "l1", "audit-a", &first, at, "s4-l1");
    let l2a = admit(&d, run, "l2", "audit-a", &first, at, "s4-l2a");
    let l3 = admit(&d, run, "l3", "audit-b", &first, at, "s4-l3");
    for admitted in [&l1, &l2a, &l3] {
        assert_eq!(admitted["status"], "admitted", "{admitted}");
    }
    let worker_note = json!({"traceId":bundle["request"]["traceId"],
        "source":"shipment.go:consumeWithRetry","question":"Does the retry hold the pool connection?"});
    let checkpoint_content = json!({"note":worker_note,"sanitized_bundle":bundle}).to_string();
    d.call(
        "swarm.report",
        json!({"run_id":run,"job_id":"l1",
        "attempt_id":l1["attempt_id"],"token":l1["token"],
        "message_id":"s4-pool-wait","type":"discovery","revision":1,
        "payload":{"trace_ids":[bundle["request"]["traceId"]],
            "pool_wait_ms":bundle["request"]["poolWaitMs"]}}),
    );
    let batch = d.call(
        "swarm.director.claim_batch",
        json!({"run_id":run,
        "generation":1,"revision":1,"now_ms":at+6000}),
    );
    assert_eq!(batch["status"], "claimed", "{batch}");
    assert_eq!(batch["messages"][0]["message_id"], "s4-pool-wait");
    d.call(
        "swarm.direct",
        json!({"run_id":run,"generation":1,"revision":1,
        "job_id":"l2","attempt_id":l2a["attempt_id"],"message_id":"s4-trace-to-l2",
        "type":"advisory","payload":{"trace_ids":[bundle["request"]["traceId"]]}}),
    );
    d.call(
        "swarm.report",
        json!({"run_id":run,"job_id":"l2",
        "attempt_id":l2a["attempt_id"],"token":l2a["token"],
        "message_id":"s4-sql-checkpoint","type":"progress","revision":1,
        "payload":worker_note}),
    );
    d.call(
        "swarm.artifact.put",
        json!({"run_id":run,"job_id":"l2",
        "attempt_id":l2a["attempt_id"],"token":l2a["token"],
        "artifact_id":"s4-checkpoint-bundle","source_revision":1,
        "kind":"checkpoint","content":checkpoint_content}),
    );
    let launched = d.call(
        "swarm.worker.launch",
        json!({"run_id":run,"job_id":"l2",
        "attempt_id":l2a["attempt_id"],"token":l2a["token"],"repo":checkout,
        "program":"/definitely/missing/shipment-audit-worker","args":[],
        "prompt":"Check SQL and pool evidence","title":"L2 account A"}),
    );
    let worker = launched["overseer_run_id"].as_str().unwrap();
    assert_eq!(d.wait_done(worker, 5)["status"], "failed");
    d.call(
        "swarm.worker.reconcile",
        json!({"run_id":run,"job_id":"l2",
        "attempt_id":l2a["attempt_id"],"generation":1,"revision":1}),
    );
    assert_eq!(
        d.call("swarm.jobs", json!({"id":run}))["jobs"][1]["status"],
        "ready"
    );
    let after = snapshot(at + 1000, false, true);
    let denied = admit(&d, run, "l2", "audit-a", &after, at + 1000, "s4-dead-a");
    assert_eq!(denied["status"], "blocked");
    let no_bundle = admit(
        &d,
        run,
        "l2",
        "audit-b",
        &after,
        at + 1000,
        "s4-l2b-before-grant",
    );
    assert_eq!(no_bundle["reason"], "checkpoint_permission_required");
    let granted = d.call(
        "swarm.context.grant",
        json!({"run_id":run,
        "generation":1,"revision":1,"artifact_id":"s4-checkpoint-bundle",
        "target_id":"audit-b"}),
    );
    assert_eq!(granted["status"], "granted");
    let l2b = admit(&d, run, "l2", "audit-b", &after, at + 1000, "s4-l2b");
    assert_eq!(l2b["status"], "admitted", "{l2b}");
    assert_ne!(l2b["attempt_id"], l2a["attempt_id"]);
    let brief = d.call(
        "swarm.worker.brief",
        json!({"run_id":run,
        "job_id":"l2","attempt_id":l2b["attempt_id"],"token":l2b["token"]}),
    );
    assert_eq!(brief["artifacts"][0]["id"], "s4-checkpoint-bundle");
    let handoff = d.call(
        "swarm.context.get",
        json!({"run_id":run,
        "job_id":"l2","attempt_id":l2b["attempt_id"],"token":l2b["token"],
        "artifact_id":"s4-checkpoint-bundle"}),
    );
    assert_eq!(handoff["content"], checkpoint_content);
    for (job, attempt, evidence) in [
        ("l1", &l1, &bundle["request"]),
        ("l2", &l2b, &bundle["sql"]),
        ("l3", &l3, &bundle["queue"]),
    ] {
        let artifact = record(&d, run, job, attempt, evidence);
        accept(&d, run, job, attempt, &artifact);
    }
    let jobs = d.call("swarm.jobs", json!({"id":run}));
    assert_eq!(jobs["jobs"][0]["status"], "accepted");
    assert_eq!(jobs["jobs"][1]["attempt_count"], 2);
    assert_eq!(jobs["jobs"][2]["status"], "accepted");
    assert_eq!(
        bundle["conclusion"],
        "correlated; causal mechanism plausible but not proven by exported logs"
    );
    d.call(
        "swarm.director.complete_batch",
        json!({"run_id":run,
        "generation":1,"turn_id":batch["turn_id"],
        "token":batch["token"],"outcome":"progress"}),
    );
    let final_batch = d.call(
        "swarm.director.claim_batch",
        json!({"run_id":run,
        "generation":1,"revision":1,"now_ms":at+12000}),
    );
    assert_eq!(final_batch["status"], "claimed", "{final_batch}");
    d.call(
        "swarm.director.complete_batch",
        json!({"run_id":run,
        "generation":1,"turn_id":final_batch["turn_id"],
        "token":final_batch["token"],"outcome":"progress"}),
    );
    let completion = d.call("swarm.complete", json!({"run_id":run,
        "generation":1,"revision":1,"request_id":"s4-reviewed-incident",
        "summary":format!("10:07:12 queue delivery; 10:07:13 transaction held by retry; 10:07:14 shipment request waited {} ms for pool and timed out. The shared trace supports a pool-starvation hypothesis. Alternative: upstream latency or other connection holders. Exported logs alone do not prove causality; production pool configuration and other traffic remain unverified.",bundle["request"]["poolWaitMs"]),
        "verification":"Three scoped findings from a disposable PostgreSQL replay; no live service changed.",
        "checks":[
            {"job_id":"l1","outcome":"passed","evidence":["s4-l1-evidence"]},
            {"job_id":"l2","outcome":"passed","evidence":["s4-l2-evidence"]},
            {"job_id":"l3","outcome":"passed","evidence":["s4-l3-evidence"]}
        ]}));
    assert_eq!(completion["status"], "completed", "{completion}");
    assert!(
        d.call("swarm.get", json!({"id":run}))["completion"]["summary"]
            .as_str()
            .unwrap()
            .contains("do not prove causality")
    );
}

#[test]
fn shipment_incident_stops_waking_when_no_qualified_selected_account_remains() {
    let mut d = Daemon::start(&[]);
    let created = d.call(
        "swarm.create",
        json!({"category":"Shipment incident outage",
        "objective":"Inspect sanitized shipment bundle",
        "allowed_targets":["audit-a","cheap"],"deadline_ms":15000}),
    );
    let run = created["id"].as_str().unwrap();
    d.call(
        "swarm.plan",
        json!({"id":run,"generation":1,"revision":0,"jobs":[
            {"id":"l1","title":"Request trace","acceptance":"pool wait note","deps":[]},
            {"id":"l2","title":"SQL path","acceptance":"held transaction note","deps":[]}
        ]}),
    );
    commit_beneficial_batch(&d, run, &["l1".into(), "l2".into()]);
    let at = now();
    let l1 = admit(
        &d,
        run,
        "l1",
        "audit-a",
        &snapshot(at, true, false),
        at,
        "s4-outage-l1",
    );
    assert_eq!(l1["status"], "admitted");
    d.call(
        "swarm.artifact.put",
        json!({"run_id":run,"job_id":"l1",
        "attempt_id":l1["attempt_id"],"token":l1["token"],
        "artifact_id":"s4-pool-note","source_revision":1,"kind":"finding",
        "content":"ship-trace-021: pool wait 50ms"}),
    );
    let blocked = d.call(
        "swarm.availability.observe",
        json!({"run_id":run,
        "snapshot":snapshot(at+1000,false,false),"now_ms":at+1000,
        "required_capabilities":["audit"],"estimate_milli":{"points":100},
        "purpose":"worker"}),
    );
    assert_eq!(blocked["state"], "blocked");
    assert_eq!(blocked["woken"], false);
    d.kill9();
    d.spawn();
    let same = d.call(
        "swarm.availability.observe",
        json!({"run_id":run,
        "snapshot":snapshot(at+1000,false,false),"now_ms":at+1000,
        "required_capabilities":["audit"],"estimate_milli":{"points":100},
        "purpose":"worker"}),
    );
    assert_eq!(same["changed"], false);
    assert_eq!(same["wake_count"], 0);
    assert_eq!(
        d.call(
            "swarm.director.claim_batch",
            json!({"run_id":run,
        "generation":1,"revision":1,"now_ms":at+7000})
        )["status"],
        "blocked"
    );
    let denied = admit(
        &d,
        run,
        "l2",
        "not-selected",
        &snapshot(at + 1000, false, false),
        at + 1000,
        "s4-denied",
    );
    assert_eq!(denied["status"], "blocked");
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let retained: i64 = db
        .query_row(
            "SELECT count(*) FROM swarm_artifacts WHERE run_id=?1 AND id='s4-pool-note'",
            [run],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(retained, 1);
}
