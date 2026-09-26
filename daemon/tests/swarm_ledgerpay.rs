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

fn probe(job: &str) -> Value {
    let fixture = repo_root().join("fixtures/swarm/ledgerpay-v1");
    let output = Command::new(fixture.join(".venv/bin/python"))
        .arg("probe.py")
        .arg(job)
        .current_dir(fixture)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["fixtureVersion"], 1);
    assert_eq!(result["job"], job);
    result["evidence"].clone()
}

fn session_command(action: &str, namespace: Option<&str>) -> Value {
    let fixture = repo_root().join("fixtures/swarm/ledgerpay-v1");
    let mut command = Command::new(fixture.join(".venv/bin/python"));
    command.arg("session.py").arg(action).current_dir(&fixture);
    if let Some(namespace) = namespace {
        command.arg(namespace);
    }
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

struct EffectSession {
    namespace: String,
}

impl EffectSession {
    fn new() -> Self {
        let initialized = session_command("init", None);
        assert_eq!(initialized["queued"], json!(["evt-42", "evt-42"]));
        Self {
            namespace: initialized["namespace"].as_str().unwrap().to_string(),
        }
    }

    fn call(&self, action: &str) -> Value {
        session_command(action, Some(&self.namespace))
    }
}

impl Drop for EffectSession {
    fn drop(&mut self) {
        let fixture = repo_root().join("fixtures/swarm/ledgerpay-v1");
        let _ = Command::new(fixture.join(".venv/bin/python"))
            .arg("session.py")
            .arg("cleanup")
            .arg(&self.namespace)
            .current_dir(&fixture)
            .output();
    }
}

fn benefit(d: &Daemon, run: &str, jobs: &[&str]) {
    let workers: Vec<Value> = jobs
        .iter()
        .map(|id| {
            json!({"id":id,
        "elapsed_ms":100,"usage_milli":{"points":10}})
        })
        .collect();
    let serial = json!({"planning":{"elapsed_ms":10,"usage_milli":{"points":1}},
        "context":{"elapsed_ms":10,"usage_milli":{"points":1}},
        "integration":{"elapsed_ms":10,"usage_milli":{"points":1}},
        "review":{"elapsed_ms":10,"usage_milli":{"points":1}},
        "retries":{"elapsed_ms":0,"usage_milli":{"points":1}},"workers":workers});
    let mut parallel = serial.clone();
    parallel["context"]["elapsed_ms"] = json!(20);
    let decision = d.call(
        "swarm.benefit.commit",
        json!({"run_id":run,
        "generation":1,"revision":1,"estimate":{"independent":true,
        "max_workers":3,"allocation_milli":{"points":100000},
        "finishing_reserve_milli":{"points":20000},
        "serial":serial,"parallel":parallel}}),
    );
    assert_eq!(
        decision["decision"],
        if jobs.len() == 1 {
            "serial"
        } else {
            "parallel"
        },
        "{decision}"
    );
}

fn admit(d: &Daemon, run: &str, job: &str, snapshot: &Value, at: i64) -> Value {
    d.call(
        "swarm.admit",
        json!({"run_id":run,"generation":1,"revision":1,
        "job_id":job,"target_id":"local-audit","request_id":format!("s2-{job}"),
        "snapshot":snapshot,"now_ms":at,"required_capabilities":["audit"],
        "estimate_milli":{"points":100},"purpose":"worker"}),
    )
}

fn submit(
    d: &Daemon,
    run: &str,
    job: &str,
    attempt: &Value,
    evidence: &Value,
    outcome: &str,
) -> String {
    let artifact = format!("s2-{job}-evidence");
    let kind = if outcome == "confirmed_defect" {
        "reproduction"
    } else {
        "finding"
    };
    d.call(
        "swarm.artifact.put",
        json!({"run_id":run,"job_id":job,
        "attempt_id":attempt["attempt_id"],"token":attempt["token"],
        "artifact_id":artifact,"source_revision":1,"kind":kind,
        "content":evidence.to_string()}),
    );
    d.call(
        "swarm.report",
        json!({"run_id":run,"job_id":job,
        "attempt_id":attempt["attempt_id"],"token":attempt["token"],
        "message_id":format!("s2-{job}-result"),"type":"result","revision":1,
        "payload":{"audit_outcome":outcome,"artifact_ids":[artifact]}}),
    );
    artifact
}

fn accept(d: &Daemon, run: &str, job: &str, attempt: &Value, artifact: &str) {
    assert_eq!(
        d.call(
            "swarm.decide",
            json!({"run_id":run,"generation":1,
        "revision":1,"job_id":job,"decision":"accept", "evidence":[artifact]})
        )["status"],
        "accepted"
    );
    d.call(
        "swarm.attempt.confirm_exit",
        json!({"run_id":run,"generation":1,
        "revision":1,"job_id":job,"attempt_id":attempt["attempt_id"]}),
    );
}

// Opt-in: local PostgreSQL and Redis are required; no provider or paid account is contacted.
#[test]
#[ignore = "requires disposable LedgerPay PostgreSQL/Redis fixture and local socket permission"]
fn ledgerpay_s2_backend_evidence_reaches_director_review() {
    assert!(std::env::var("LEDGERPAY_DATABASE_URL").is_ok());
    assert!(std::env::var("LEDGERPAY_REDIS_URL").is_ok());
    let d = Daemon::start(&[]);
    let created = d.call(
        "swarm.create",
        json!({"category":"LedgerPay billing audit",
        "objective":"Audit whether retries and reordered events double-apply subscriptions",
        "allowed_targets":["local-audit"],"policy":{"max_workers":3}}),
    );
    let run = created["id"].as_str().unwrap();
    let jobs = json!([
        {"id":"k1","title":"Signature and ingress","acceptance":"invalid signature rejected","deps":[],"resource_claims":[{"resource":"s2-db-k1","mode":"write"}]},
        {"id":"k2","title":"Queue retries and crash point","acceptance":"duplicate queue IDs and lost-ack outcome","deps":[],"resource_claims":[{"resource":"s2-db-k2","mode":"write"}]},
        {"id":"k3","title":"Entitlement transactions","acceptance":"seeded and protected duplicate replay","deps":[],"resource_claims":[{"resource":"s2-db-k3","mode":"write"}]},
        {"id":"k4","title":"Event order","acceptance":"stale activation denied","deps":[],"resource_claims":[{"resource":"s2-db-k4","mode":"write"}]}
    ]);
    d.call(
        "swarm.plan",
        json!({"id":run,"generation":1,"revision":0,"jobs":jobs}),
    );
    let at = now();
    let snapshot = json!({"version":1,"observed_ms":at-1000,"expires_ms":at+120000,
        "targets":[{"id":"local-audit","account_id":"fixture-a","pool_ids":["fixture-pool"],
            "capabilities":["audit"],"health":"up","auth":"ok"}],
        "pools":[{"id":"fixture-pool","windows":[{"id":"week","unit":"points",
            "remaining_milli":1000000,"protected_milli":0,"reserved_milli":0,
            "confidence":"exact","expires_ms":at+120000}]}]});
    benefit(&d, run, &["k1", "k2", "k3"]);
    let k1 = admit(&d, run, "k1", &snapshot, at);
    let k2 = admit(&d, run, "k2", &snapshot, at);
    let k3 = admit(&d, run, "k3", &snapshot, at);
    for item in [&k1, &k2, &k3] {
        assert_eq!(item["status"], "admitted", "{item}");
    }
    assert_eq!(admit(&d, run, "k4", &snapshot, at)["status"], "blocked");

    let k2_evidence = probe("k2");
    assert_eq!(k2_evidence["queuedEventIds"], json!(["evt-42", "evt-42"]));
    assert_eq!(
        k2_evidence["outcomeProbe"]["status"],
        "unknown_do_not_retry"
    );
    d.call(
        "swarm.report",
        json!({"run_id":run,"job_id":"k2",
        "attempt_id":k2["attempt_id"],"token":k2["token"],
        "message_id":"s2-duplicate-discovery","type":"discovery","revision":1,
        "payload":{"event_id":"evt-42","delivery_count":2,
            "question":"Do event receipt and grant update share one transaction?"}}),
    );
    let batch = d.call(
        "swarm.director.claim_batch",
        json!({"run_id":run,
        "generation":1,"revision":1,"now_ms":at+6000}),
    );
    assert_eq!(batch["status"], "claimed", "{batch}");
    assert_eq!(batch["messages"][0]["message_id"], "s2-duplicate-discovery");
    d.call(
        "swarm.direct",
        json!({"run_id":run,"generation":1,"revision":1,
        "job_id":"k3","attempt_id":k3["attempt_id"],
        "message_id":"s2-check-transaction-gap","type":"advisory",
        "payload":{"event_id":"evt-42",
            "focus":"Check whether receipt insert and entitlement update share one transaction"}}),
    );
    for phase in ["delivered", "applied"] {
        assert_eq!(
            d.call(
                "swarm.ack",
                json!({"run_id":run,
            "message_id":"s2-check-transaction-gap","recipient":k3["attempt_id"],
            "token":k3["token"],"phase":phase,"revision":1})
            )["phase"],
            phase
        );
    }
    d.call(
        "swarm.director.complete_batch",
        json!({"run_id":run,"generation":1,
        "turn_id":batch["turn_id"],"token":batch["token"],"outcome":"progress"}),
    );

    let k1_evidence = probe("k1");
    assert_eq!(k1_evidence["invalidSignatureStatus"], 401);
    let a1 = submit(&d, run, "k1", &k1, &k1_evidence, "negative");
    accept(&d, run, "k1", &k1, &a1);
    benefit(&d, run, &["k4"]);
    let k4 = admit(&d, run, "k4", &snapshot, at + 5000);
    assert_eq!(k4["status"], "admitted", "{k4}");

    let k3_evidence = probe("k3");
    let protected = probe("k3-protected");
    assert_eq!(k3_evidence["finalState"]["grant_count"], 2);
    assert_eq!(protected["finalState"]["grant_count"], 1);
    let transaction_evidence = json!({"seeded":k3_evidence,"protected":protected});
    let a3 = submit(
        &d,
        run,
        "k3",
        &k3,
        &transaction_evidence,
        "confirmed_defect",
    );
    let a2 = submit(&d, run, "k2", &k2, &k2_evidence, "confirmed_defect");
    let k4_evidence = probe("k4");
    assert_eq!(k4_evidence["staleActivation"]["reason"], "stale_version");
    let a4 = submit(&d, run, "k4", &k4, &k4_evidence, "negative");
    let final_batch = d.call(
        "swarm.director.claim_batch",
        json!({"run_id":run,
        "generation":1,"revision":1,"now_ms":at+20000}),
    );
    assert_eq!(final_batch["status"], "claimed", "{final_batch}");
    for (job, attempt, artifact) in [("k2", &k2, &a2), ("k3", &k3, &a3), ("k4", &k4, &a4)] {
        accept(&d, run, job, attempt, artifact);
    }
    d.call(
        "swarm.director.complete_batch",
        json!({"run_id":run,"generation":1,
        "turn_id":final_batch["turn_id"],"token":final_batch["token"],
        "outcome":"progress"}),
    );
    let checks = vec![
        json!({"job_id":"k1","outcome":"passed","evidence":[a1]}),
        json!({"job_id":"k2","outcome":"passed","evidence":[a2]}),
        json!({"job_id":"k3","outcome":"passed","evidence":[a3]}),
        json!({"job_id":"k4","outcome":"passed","evidence":[a4]}),
    ];
    let before_complete = d.call("swarm.get", json!({"id":run}));
    assert_eq!(before_complete["status"], "running", "{before_complete}");
    let complete=d.call("swarm.complete",json!({"run_id":run,"generation":1,
        "revision":1,"request_id":"s2-scripted-complete",
        "summary":"evt-42 duplicate deliveries grant twice in the seeded fixture; protected transaction grants once; stale activation and invalid signatures denied",
        "verification":"Four isolated PostgreSQL/Redis job probes; protected replay isolated separately",
        "checks":checks}));
    assert_eq!(complete["status"], "completed", "{complete}");
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let admissions: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM swarm_admissions WHERE run_id=?1",
            [run],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(admissions, 4);
}

#[test]
#[ignore = "requires disposable LedgerPay PostgreSQL/Redis fixture and local socket permission"]
fn ledgerpay_s2_missing_redis_remains_blocked_coverage() {
    assert!(std::env::var("LEDGERPAY_DATABASE_URL").is_ok());
    assert!(std::env::var("LEDGERPAY_REDIS_URL").is_ok());
    let d = Daemon::start(&[]);
    let created = d.call(
        "swarm.create",
        json!({"category":"LedgerPay missing queue",
        "objective":"Audit retry safety","allowed_targets":["local-audit"]}),
    );
    let run = created["id"].as_str().unwrap();
    d.call(
        "swarm.plan",
        json!({"id":run,"generation":1,"revision":0,"jobs":[
            {"id":"k2","title":"Queue retry safety",
             "acceptance":"signed event queued and delivered","deps":[]}
        ]}),
    );
    let at = now();
    let snapshot = json!({"version":1,"observed_ms":at-1000,"expires_ms":at+120000,
        "targets":[{"id":"local-audit","account_id":"fixture-a",
            "pool_ids":["fixture-pool"],"capabilities":["audit"],
            "health":"up","auth":"ok"}],
        "pools":[{"id":"fixture-pool","windows":[{"id":"week","unit":"points",
            "remaining_milli":1000000,"protected_milli":0,"reserved_milli":0,
            "confidence":"exact","expires_ms":at+120000}]}]});
    let attempt = admit(&d, run, "k2", &snapshot, at);
    assert_eq!(attempt["status"], "admitted", "{attempt}");
    let evidence = probe("k2-missing-redis");
    assert_eq!(evidence["webhookStatus"], 503);
    assert_eq!(evidence["unavailableResource"], "redis_queue");
    let artifact = "s2-k2-missing-redis";
    d.call(
        "swarm.artifact.put",
        json!({"run_id":run,"job_id":"k2",
        "attempt_id":attempt["attempt_id"],"token":attempt["token"],
        "artifact_id":artifact,"source_revision":1,"kind":"log",
        "content":evidence.to_string()}),
    );
    d.call(
        "swarm.report",
        json!({"run_id":run,"job_id":"k2",
        "attempt_id":attempt["attempt_id"],"token":attempt["token"],
        "message_id":"s2-k2-redis-unavailable","type":"result","revision":1,
        "payload":{"audit_outcome":"environment_failure","artifact_ids":[artifact],
            "unavailable_resource":"redis_queue"}}),
    );
    let coverage = d.call("swarm.coverage", json!({"run_id":run}));
    assert_eq!(coverage["rows"][0]["coverage_state"], "environment_blocked");
    assert_eq!(coverage["rows"][0]["unavailable_resource"], "redis_queue");
    assert!(d
        .try_call(
            "swarm.decide",
            json!({"run_id":run,"generation":1,
        "revision":1,"job_id":"k2","decision":"accept","evidence":[artifact]})
        )
        .unwrap_err()
        .contains("environment"));
    assert!(d
        .try_call(
            "swarm.complete",
            json!({"run_id":run,"generation":1,
        "revision":1,"request_id":"s2-missing-redis-complete",
        "summary":"Retry safety checked","verification":"Redis unavailable",
        "checks":[{"job_id":"k2","outcome":"passed","evidence":[artifact]}]})
        )
        .unwrap_err()
        .contains("requires every planned job to be accepted"));
}

#[test]
#[ignore = "requires disposable LedgerPay PostgreSQL/Redis fixture and local socket permission"]
fn ledgerpay_s2_lost_ack_probes_real_db_after_daemon_restart() {
    assert!(std::env::var("LEDGERPAY_DATABASE_URL").is_ok());
    assert!(std::env::var("LEDGERPAY_REDIS_URL").is_ok());
    let fixture = EffectSession::new();
    let mut d = Daemon::start(&[]);
    let created = d.call(
        "swarm.create",
        json!({"category":"LedgerPay effect reconciliation",
        "objective":"Audit duplicate evt-42 application after a lost acknowledgement",
        "allowed_targets":["local-audit"]}),
    );
    let run = created["id"].as_str().unwrap();
    d.call(
        "swarm.plan",
        json!({"id":run,"generation":1,"revision":0,"jobs":[
            {"id":"k2","title":"Queue delivery","acceptance":"one applied grant",
                "deps":[],"resource_claims":[{"resource":"ledgerpay-sub-1","mode":"write"}]},
            {"id":"k1","title":"Signature ingress","acceptance":"invalid signature denied",
                "deps":[]}
        ]}),
    );
    let k2 = d.call(
        "swarm.attempt.register",
        json!({"run_id":run,"generation":1,
        "revision":1,"job_id":"k2"}),
    );
    let operation = format!("fixture:ledgerpay:{}:evt-42:grant", fixture.namespace);
    let intent = json!({"run_id":run,"job_id":"k2","attempt_id":k2["id"],
        "token":k2["token"],"effect_id":"evt-42-grant","operation_id":operation,
        "revision":1,"fixture_drop_ack_after_commit":true});
    assert!(d
        .try_call("swarm.effect.begin", intent.clone())
        .unwrap_err()
        .contains("injected"));
    assert_eq!(fixture.call("deliver")["status"], "ack_lost");
    d.kill9();
    d.spawn();
    let mut replay = intent;
    replay
        .as_object_mut()
        .unwrap()
        .remove("fixture_drop_ack_after_commit");
    let recovered = d.call("swarm.effect.begin", replay.clone());
    assert_eq!(recovered["outcome"], "unknown");
    assert_eq!(recovered["may_execute"], false);
    assert_eq!(recovered["duplicate"], true);

    let observed = fixture.call("outcome");
    assert_eq!(observed["event_id"], "evt-42");
    assert_eq!(observed["grant_count"], 1);
    assert_eq!(observed["receipt"], false);
    assert_eq!(observed["queued"], json!(["evt-42"]));
    assert_eq!(observed["status"], "unknown_do_not_retry");

    d.call(
        "swarm.artifact.put",
        json!({"run_id":run,"job_id":"k2",
        "attempt_id":k2["id"],"token":k2["token"],
        "artifact_id":"k2-lost-ack","source_revision":1,"kind":"finding",
        "content":"delivery acknowledgement lost after evt-42 grant"}),
    );
    d.call(
        "swarm.report",
        json!({"run_id":run,"job_id":"k2",
        "attempt_id":k2["id"],"token":k2["token"],
        "message_id":"k2-lost-ack-result","type":"result","revision":1,
        "payload":{"artifact_ids":["k2-lost-ack"]}}),
    );
    assert!(d
        .try_call(
            "swarm.decide",
            json!({"run_id":run,"generation":1,
        "revision":1,"job_id":"k2","decision":"accept",
        "evidence":["k2-lost-ack"]})
        )
        .unwrap_err()
        .contains("unreconciled side effect"));

    let k1 = d.call(
        "swarm.attempt.register",
        json!({"run_id":run,"generation":1,
        "revision":1,"job_id":"k1"}),
    );
    let ingress = probe("k1");
    assert_eq!(ingress["invalidSignatureStatus"], 401);
    d.call(
        "swarm.artifact.put",
        json!({"run_id":run,"job_id":"k1",
        "attempt_id":k1["id"],"token":k1["token"],
        "artifact_id":"k1-signature","source_revision":1,"kind":"finding",
        "content":ingress.to_string()}),
    );
    d.call(
        "swarm.report",
        json!({"run_id":run,"job_id":"k1",
        "attempt_id":k1["id"],"token":k1["token"],
        "message_id":"k1-signature-result","type":"result","revision":1,
        "payload":{"audit_outcome":"negative","artifact_ids":["k1-signature"]}}),
    );
    assert_eq!(
        d.call(
            "swarm.decide",
            json!({"run_id":run,"generation":1,
        "revision":1,"job_id":"k1","decision":"accept",
        "evidence":["k1-signature"]})
        )["status"],
        "accepted"
    );
    d.call(
        "swarm.attempt.confirm_exit",
        json!({"run_id":run,"generation":1,
        "revision":1,"job_id":"k1","attempt_id":k1["id"]}),
    );

    d.call(
        "swarm.artifact.put",
        json!({"run_id":run,"job_id":"k2",
        "attempt_id":k2["id"],"token":k2["token"],
        "artifact_id":"k2-db-outcome","source_revision":1,"kind":"effect_probe",
        "content":observed.to_string()}),
    );
    assert!(d
        .try_call(
            "swarm.effect.reconcile",
            json!({"run_id":run,
        "generation":1,"revision":1,"job_id":"k2","effect_id":"evt-42-grant",
        "outcome":"applied","proof_artifact_id":"k2-db-outcome"})
        )
        .unwrap_err()
        .contains("not submitted"));
    d.call(
        "swarm.report",
        json!({"run_id":run,"job_id":"k2",
        "attempt_id":k2["id"],"token":k2["token"],
        "message_id":"k2-db-outcome-report","type":"discovery","revision":1,
        "payload":{"artifact_ids":["k2-db-outcome"]}}),
    );
    let reconciled = d.call(
        "swarm.effect.reconcile",
        json!({"run_id":run,
        "generation":1,"revision":1,"job_id":"k2","effect_id":"evt-42-grant",
        "outcome":"applied","proof_artifact_id":"k2-db-outcome"}),
    );
    assert_eq!(reconciled["outcome"], "applied");
    assert_eq!(reconciled["may_execute"], false);
    assert_eq!(d.call("swarm.effect.begin", replay)["may_execute"], false);
    let after = fixture.call("outcome");
    assert_eq!(after["grant_count"], 1);
    assert_eq!(after["queued"], json!(["evt-42"]));
}
