mod common;

use common::*;
use serde_json::{json, Value};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as i64
}

fn atlas_probe(job: &str) -> Value {
    atlas_probe_variant(job, None)
}

fn atlas_probe_variant(job: &str, variant: Option<&str>) -> Value {
    let fixture = repo_root().join("fixtures/swarm/atlas-v1");
    let mut command = Command::new("node");
    command.arg("probe.mjs").arg(job);
    if let Some(variant) = variant { command.arg(variant); }
    let output = command.current_dir(fixture).output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let trace: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(trace["fixtureVersion"], 1);
    assert_eq!(trace["job"], job);
    trace["evidence"].clone()
}

fn register(d: &Daemon, run: &str, job: &str) -> Value {
    let attempt = d.call("swarm.attempt.register", json!({"run_id":run,
        "generation":1,"revision":1,"job_id":job}));
    json!({"attempt_id":attempt["id"],"token":attempt["token"]})
}

fn commit_wave(d: &Daemon, run: &str, revision: i64, jobs: &[&str]) {
    let workers: Vec<Value> = jobs.iter().map(|job| json!({"id":job,
        "elapsed_ms":100,"usage_milli":{"points":10}})).collect();
    let serial = json!({"planning":{"elapsed_ms":10,"usage_milli":{"points":1}},
        "context":{"elapsed_ms":10,"usage_milli":{"points":1}},
        "integration":{"elapsed_ms":10,"usage_milli":{"points":1}},
        "review":{"elapsed_ms":10,"usage_milli":{"points":1}},
        "retries":{"elapsed_ms":0,"usage_milli":{"points":1}},"workers":workers});
    let mut parallel = serial.clone();
    parallel["context"]["elapsed_ms"] = json!(20);
    let decision = d.call("swarm.benefit.commit", json!({"run_id":run,
        "generation":1,"revision":revision,"estimate":{"independent":true,
        "max_workers":4,"allocation_milli":{"points":100000},
        "finishing_reserve_milli":{"points":20000},
        "serial":serial,"parallel":parallel}}));
    assert_eq!(decision["decision"], "parallel", "{decision}");
}

fn admit(d: &Daemon, run: &str, revision: i64, job: &str, target: &str,
    snapshot: &Value, at: i64) -> Value {
    d.call("swarm.admit", json!({"run_id":run,"generation":1,
        "revision":revision,"job_id":job,"target_id":target,
        "request_id":format!("atlas-{job}"),"snapshot":snapshot,"now_ms":at,
        "required_capabilities":["audit"],"estimate_milli":{"points":100},
        "purpose":"worker"}))
}

fn submit(d: &Daemon, run: &str, job: &str, attempt: &Value, revision: i64,
    evidence: &Value, outcome: &str) -> String {
    let artifact = format!("atlas-{job}-evidence");
    let kind = if outcome == "confirmed_defect" { "reproduction" } else { "finding" };
    d.call("swarm.artifact.put", json!({"run_id":run,"job_id":job,
        "attempt_id":attempt["attempt_id"],"token":attempt["token"],
        "artifact_id":artifact,"source_revision":revision,"kind":kind,
        "content":evidence.to_string()}));
    d.call("swarm.report", json!({"run_id":run,"job_id":job,
        "attempt_id":attempt["attempt_id"],"token":attempt["token"],
        "message_id":format!("atlas-{job}-result"),"type":"result","revision":revision,
        "payload":{"audit_outcome":outcome,"artifact_ids":[artifact]}}));
    artifact
}

fn accept_and_exit(d: &Daemon, run: &str, run_revision: i64, job: &str,
    attempt: &Value, artifact: &str) {
    assert_eq!(d.call("swarm.decide", json!({"run_id":run,"generation":1,
        "revision":run_revision,"job_id":job,"decision":"accept",
        "evidence":[artifact]}))["status"], "accepted");
    d.call("swarm.attempt.confirm_exit", json!({"run_id":run,"generation":1,
        "revision":run_revision,"job_id":job,"attempt_id":attempt["attempt_id"]}));
}

// Explicit opt-in: a real local PostgreSQL fixture and Node 24 are required.
// The director's choices and target snapshot are scripted; no provider account is used.
#[test]
#[ignore = "requires Atlas PostgreSQL fixture, Node.js 24, and local socket permission"]
fn atlas_s1_backend_evidence_flows_through_swarm_review() {
    assert!(std::env::var("ATLAS_DATABASE_URL").is_ok());
    let d = Daemon::start(&[]);
    let created = d.call("swarm.create", json!({"category":"Backend security",
        "objective":"Audit Atlas tenant isolation; report bugs, do not change application code",
        "allowed_targets":["profile-a","profile-b"],"policy":{"max_workers":4}}));
    let run = created["id"].as_str().unwrap();
    assert_eq!(created["policy"]["effective"]["max_workers"], 4);
    assert_eq!(created["source_change_permission"], "none");
    let jobs: Vec<Value> = [
        ("j1", "Projects", "own and foreign project checks"),
        ("j2", "Tasks", "foreign patch and delete before-after rows"),
        ("j3", "Membership", "role change matrix"),
        ("j4", "Attachments", "signed foreign object retrieval"),
        ("j5", "Exports", "workspace-bound queue and query"),
        ("j6", "Tokens", "read-only revoked and removed-member checks"),
    ].into_iter().map(|(id,title,acceptance)| json!({"id":id,"title":title,
        "acceptance":acceptance,"deps":[],"resource_claims":[
            {"resource":format!("atlas-db-{id}"),"mode":"write"}]})).collect();
    d.call("swarm.plan", json!({"id":run,"generation":1,"revision":0,"jobs":jobs}));
    let at = now();
    let snapshot = json!({"version":1,"observed_ms":at-1000,"expires_ms":at+120000,
        "targets":[
            {"id":"profile-a","account_id":"account-a","pool_ids":["pool-a"],
                "capabilities":["audit"],"health":"up","auth":"ok"},
            {"id":"profile-b","account_id":"account-b","pool_ids":["pool-b"],
                "capabilities":["audit"],"health":"up","auth":"ok"}],
        "pools":[
            {"id":"pool-a","windows":[{"id":"week-a","unit":"points",
                "remaining_milli":1000000,"protected_milli":0,"reserved_milli":0,
                "confidence":"exact","expires_ms":at+120000}]},
            {"id":"pool-b","windows":[{"id":"week-b","unit":"points",
                "remaining_milli":1000000,"protected_milli":0,"reserved_milli":0,
                "confidence":"exact","expires_ms":at+120000}]}]});
    commit_wave(&d, run, 1, &["j1","j2","j3","j4"]);
    let a1 = admit(&d,run,1,"j1","profile-a",&snapshot,at);
    let a2 = admit(&d,run,1,"j2","profile-a",&snapshot,at);
    let a3 = admit(&d,run,1,"j3","profile-b",&snapshot,at);
    let a4 = admit(&d,run,1,"j4","profile-b",&snapshot,at);
    for admitted in [&a1,&a2,&a3,&a4] { assert_eq!(admitted["status"],"admitted","{admitted}"); }
    assert_eq!(admit(&d,run,1,"j5","profile-a",&snapshot,at)["status"],"blocked");

    let j2 = atlas_probe("j2");
    assert_eq!(j2["taskAfter"], "changed-by-alice");
    let discovery = json!({"run_id":run,"job_id":"j2","attempt_id":a2["attempt_id"],
        "token":a2["token"],"message_id":"D1","type":"discovery","revision":1,
        "payload":{"symbol":"TaskRepository.findById","source_revision":"atlas-v1",
            "callers":["PATCH /tasks/:id","DELETE /tasks/:id"],
            "note":"lookup filters by task id; ownership still needs endpoint checks"}});
    assert_eq!(d.call("swarm.report",discovery.clone())["duplicate"],false);
    assert_eq!(d.call("swarm.report",discovery)["duplicate"],true);
    let batch = d.call("swarm.director.claim_batch",json!({"run_id":run,"generation":1,
        "revision":1,"now_ms":at+6000}));
    assert_eq!(batch["status"],"claimed","{batch}");
    assert_eq!(batch["messages"][0]["message_id"],"D1");
    for (job,attempt,focus) in [
        ("j1",&a1,"Check project middleware separately"),
        ("j4",&a4,"Check signed URL boundary; reference D1"),
    ] {
        let message=format!("D1-to-{job}");
        d.call("swarm.direct",json!({"run_id":run,"generation":1,"revision":1,
            "job_id":job,"attempt_id":attempt["attempt_id"],"message_id":message,
            "type":"advisory","payload":{"discovery_id":"D1","focus":focus}}));
        for phase in ["delivered","applied"] {
            assert_eq!(d.call("swarm.ack",json!({"run_id":run,"message_id":message,
                "recipient":attempt["attempt_id"],"token":attempt["token"],
                "phase":phase,"revision":1}))["phase"],phase);
        }
    }
    d.call("swarm.director.complete_batch",json!({"run_id":run,"generation":1,
        "turn_id":batch["turn_id"],"token":batch["token"],"outcome":"progress"}));
    d.call("swarm.report",json!({"run_id":run,"job_id":"j4",
        "attempt_id":a4["attempt_id"],"token":a4["token"],
        "message_id":"j4-overlap","type":"claim","revision":1,
        "payload":{"overlap_with":"j2","symbol":"TaskRepository.findById"}}));
    d.call("swarm.direct",json!({"run_id":run,"generation":1,"revision":1,
        "job_id":"j4","attempt_id":a4["attempt_id"],"message_id":"j4-own-signed-url",
        "type":"redirect","payload":{"owner":"j2","focus":"signed URL boundary"}}));
    for phase in ["delivered","applied"] {
        d.call("swarm.ack",json!({"run_id":run,"message_id":"j4-own-signed-url",
            "recipient":a4["attempt_id"],"token":a4["token"],
            "phase":phase,"revision":1}));
    }

    let j1 = atlas_probe("j1");
    assert_eq!(j1["foreignStatus"],403);
    let j1_artifact=submit(&d,run,"j1",&a1,1,&j1,"negative");
    accept_and_exit(&d,run,1,"j1",&a1,&j1_artifact);
    commit_wave(&d,run,1,&["j5","j6"]);
    let a5=admit(&d,run,1,"j5","profile-a",&snapshot,at+5000);
    assert_eq!(a5["status"],"admitted","{a5}");
    let j5=atlas_probe("j5");
    let j5_artifact=submit(&d,run,"j5",&a5,1,&j5,"negative");

    let j2_artifact=submit(&d,run,"j2",&a2,1,&j2,"confirmed_defect");
    d.call("swarm.attempt.confirm_exit",json!({"run_id":run,"generation":1,
        "revision":1,"job_id":"j2","attempt_id":a2["attempt_id"]}));
    let mut revised_jobs=jobs;
    revised_jobs.push(json!({"id":"j7","title":"Independent task reproduction",
        "acceptance":"repeat foreign task mutation from fresh fixture",
        "deps":[],"resource_claims":[{"resource":"atlas-db-j7","mode":"write"}]}));
    assert_eq!(d.call("swarm.revise",json!({"id":run,"generation":1,
        "expected_revision":1,"reason":"J2 task finding needs independent reproduction",
        "jobs":revised_jobs}))["revision"],2);
    commit_wave(&d,run,2,&["j7","j6"]);
    let a7=admit(&d,run,2,"j7","profile-b",&snapshot,at+10000);
    assert_eq!(a7["status"],"admitted","{a7}");
    let j7=atlas_probe("j7");
    assert_ne!(j2["namespace"],j7["namespace"]);
    assert_eq!(j7["taskBefore"],"Bob task");
    let j7_artifact=submit(&d,run,"j7",&a7,2,&j7,"confirmed_defect");
    accept_and_exit(&d,run,2,"j7",&a7,&j7_artifact);
    accept_and_exit(&d,run,2,"j2",&a2,&j2_artifact);

    let j4=atlas_probe("j4");
    assert_eq!(j4["objectStatus"],200);
    let j4_artifact=submit(&d,run,"j4",&a4,1,&j4,"confirmed_defect");
    accept_and_exit(&d,run,2,"j4",&a4,&j4_artifact);
    let j3=atlas_probe("j3");
    let j3_artifact=submit(&d,run,"j3",&a3,1,&j3,"negative");
    accept_and_exit(&d,run,2,"j3",&a3,&j3_artifact);
    accept_and_exit(&d,run,2,"j5",&a5,&j5_artifact);
    let a6=admit(&d,run,2,"j6","profile-a",&snapshot,at+15000);
    assert_eq!(a6["status"],"admitted","{a6}");
    let j6=atlas_probe("j6");
    let j6_artifact=submit(&d,run,"j6",&a6,1,&j6,"negative");
    accept_and_exit(&d,run,2,"j6",&a6,&j6_artifact);

    let final_batch=d.call("swarm.director.claim_batch",json!({"run_id":run,
        "generation":1,"revision":2,"now_ms":at+20000}));
    assert_eq!(final_batch["status"],"claimed","{final_batch}");
    d.call("swarm.director.complete_batch",json!({"run_id":run,"generation":1,
        "turn_id":final_batch["turn_id"],"token":final_batch["token"],"outcome":"progress"}));
    let evidence=[j1_artifact,j2_artifact,j3_artifact,j4_artifact,j5_artifact,j6_artifact,j7_artifact];
    let checks: Vec<Value>=(1..=7).map(|n|json!({"job_id":format!("j{n}"),
        "outcome":"passed","evidence":[evidence[n-1]]})).collect();
    let completed=d.call("swarm.complete",json!({"run_id":run,"generation":1,
        "revision":2,"request_id":"atlas-s1-scripted-complete",
        "summary":"Confirmed foreign task mutation and foreign attachment download; projects, membership, exports and tokens checked separately",
        "verification":"Seven isolated Atlas PostgreSQL probes; J7 independently reproduced J2",
        "checks":checks}));
    assert_eq!(completed["status"],"completed","{completed}");
    let db=rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let admissions:i64=db.query_row("SELECT COUNT(*) FROM swarm_admissions WHERE run_id=?1",
        [run],|row|row.get(0)).unwrap();
    assert_eq!(admissions,7);
    let decisions:i64=db.query_row("SELECT COUNT(*) FROM swarm_decisions WHERE run_id=?1 AND decision='accept'",
        [run],|row|row.get(0)).unwrap();
    assert_eq!(decisions,7);
}

#[test]
#[ignore = "requires Atlas PostgreSQL fixture, Node.js 24, and local socket permission"]
fn atlas_s1_faults_quarantine_stale_and_missing_evidence() {
    assert!(std::env::var("ATLAS_DATABASE_URL").is_ok());
    let d = Daemon::start(&[]);
    let created = d.call("swarm.create", json!({"category":"Atlas fault replay",
        "objective":"Audit tenant isolation","allowed_targets":["fixture"]}));
    let run = created["id"].as_str().unwrap();
    let jobs = json!([
        {"id":"j2","title":"Tasks","acceptance":"foreign mutation evidence","deps":[]},
        {"id":"j4","title":"Attachments","acceptance":"original shared-helper trace","deps":[]}
    ]);
    d.call("swarm.plan", json!({"id":run,"generation":1,"revision":0,"jobs":jobs}));
    let a2=register(&d,run,"j2");
    let a4=register(&d,run,"j4");
    let j2=atlas_probe("j2");
    assert_eq!(j2["foreignPatchStatus"],200);
    let j2_artifact=submit(&d,run,"j2",&a2,1,&j2,"confirmed_defect");
    let db=rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    db.execute("DELETE FROM swarm_artifacts WHERE run_id=?1 AND id=?2",
        [run,j2_artifact.as_str()]).unwrap();
    assert!(d.try_call("swarm.decide",json!({"run_id":run,"generation":1,
        "revision":1,"job_id":"j2","decision":"accept","evidence":[j2_artifact]}))
        .unwrap_err().contains("missing artifact"));
    let j4=atlas_probe("j4");
    assert_eq!(j4["objectStatus"],200);
    let revised=json!([
        {"id":"j2","title":"Tasks","acceptance":"foreign mutation evidence","deps":[]},
        {"id":"j4","title":"Attachments","acceptance":"signed URL boundary only","deps":[]}
    ]);
    assert_eq!(d.call("swarm.revise",json!({"id":run,"generation":1,
        "expected_revision":1,"reason":"J2 owns the shared helper; J4 checks the URL boundary",
        "jobs":revised}))["revision"],2);
    let redirected=d.call("swarm.messages",json!({"run_id":run,
        "recipient":a4["attempt_id"],"token":a4["token"]}));
    assert!(redirected["messages"].as_array().unwrap().iter()
        .any(|message| message["type"] == "redirect"));
    let j4_artifact=submit(&d,run,"j4",&a4,1,&j4,"confirmed_defect");
    assert!(d.try_call("swarm.decide",json!({"run_id":run,"generation":1,
        "revision":2,"job_id":"j4","decision":"accept","evidence":[j4_artifact]}))
        .unwrap_err().contains("stale"));
    let jobs=d.call("swarm.jobs",json!({"id":run}));
    let j4_status=jobs["jobs"].as_array().unwrap().iter()
        .find(|job| job["id"] == "j4").unwrap();
    assert_ne!(j4_status["status"],"accepted");
}

#[test]
#[ignore = "requires Atlas PostgreSQL fixture, Node.js 24, and local socket permission"]
fn atlas_s1_contradictory_j7_retracts_claim_pending_environment_review() {
    assert!(std::env::var("ATLAS_DATABASE_URL").is_ok());
    let d=Daemon::start(&[]);
    let created=d.call("swarm.create",json!({"category":"Atlas contradiction",
        "objective":"Audit task isolation","allowed_targets":["fixture"]}));
    let run=created["id"].as_str().unwrap();
    d.call("swarm.plan",json!({"id":run,"generation":1,"revision":0,"jobs":[
        {"id":"j2","title":"Task finding","acceptance":"foreign mutation evidence","deps":[]},
        {"id":"j4","title":"Attachment boundary","acceptance":"independent path","deps":[]},
        {"id":"j7","title":"Independent reproduction","acceptance":"reproduce J2","deps":[]}
    ]}));
    let at=now();
    let snapshot=json!({"version":1,"observed_ms":at-1000,"expires_ms":at+120000,
        "targets":[{"id":"fixture","account_id":"account","pool_ids":["pool"],
            "capabilities":["audit"],"health":"up","auth":"ok"}],
        "pools":[{"id":"pool","windows":[{"id":"week","unit":"points",
            "remaining_milli":1000000,"protected_milli":0,"reserved_milli":0,
            "confidence":"exact","expires_ms":at+120000}]}]});
    commit_wave(&d,run,1,&["j2","j4","j7"]);
    let a2=admit(&d,run,1,"j2","fixture",&snapshot,at);
    let a4=admit(&d,run,1,"j4","fixture",&snapshot,at);
    let a7=admit(&d,run,1,"j7","fixture",&snapshot,at);
    for attempt in [&a2,&a4,&a7] { assert_eq!(attempt["status"],"admitted","{attempt}"); }
    let j2=atlas_probe("j2");
    let j7=atlas_probe_variant("j7",Some("task-guarded"));
    assert_eq!(j2["foreignPatchStatus"],200);
    assert_eq!(j7["foreignPatchStatus"],403);
    assert_eq!(j7["taskAfter"],"Bob task");
    assert_ne!(j2["namespace"],j7["namespace"]);
    let j2_artifact=submit(&d,run,"j2",&a2,1,&j2,"confirmed_defect");
    let j7_artifact=submit(&d,run,"j7",&a7,1,&j7,"negative");
    let j4=atlas_probe("j4");
    let j4_artifact=submit(&d,run,"j4",&a4,1,&j4,"confirmed_defect");
    let batch=d.call("swarm.director.claim_batch",json!({"run_id":run,
        "generation":1,"revision":1,"now_ms":now()+6000}));
    assert_eq!(batch["status"],"claimed","{batch}");
    let messages=batch["messages"].as_array().unwrap();
    assert!(messages.iter().any(|message| message["message_id"] == "atlas-j2-result"));
    assert!(messages.iter().any(|message| message["message_id"] == "atlas-j7-result"));
    for (job,attempt) in [("j2",&a2),("j4",&a4)] {
        let message_id=format!("retract-task-claim-{job}");
        d.call("swarm.direct",json!({"run_id":run,"generation":1,"revision":1,
            "job_id":job,"attempt_id":attempt["attempt_id"],"message_id":message_id,
            "type":"retract","payload":{"candidate":j2_artifact,
                "contradiction":j7_artifact,"reason":"J7 used a guarded task route; compare fixture middleware before accepting"}}));
        for phase in ["delivered","applied"] {
            assert_eq!(d.call("swarm.ack",json!({"run_id":run,"message_id":message_id,
                "recipient":attempt["attempt_id"],"token":attempt["token"],
                "phase":phase,"revision":1}))["phase"],phase);
        }
    }
    d.call("swarm.director.complete_batch",json!({"run_id":run,"generation":1,
        "turn_id":batch["turn_id"],"token":batch["token"],"outcome":"progress"}));
    let jobs=d.call("swarm.jobs",json!({"id":run}));
    assert!(jobs["jobs"].as_array().unwrap().iter()
        .all(|job| job["status"] != "accepted"));
    let checks=json!([
        {"job_id":"j2","outcome":"passed","evidence":[j2_artifact]},
        {"job_id":"j4","outcome":"passed","evidence":[j4_artifact]},
        {"job_id":"j7","outcome":"passed","evidence":[j7_artifact]}
    ]);
    let completion_error=d.try_call("swarm.complete",json!({"run_id":run,"generation":1,
        "revision":1,"request_id":"contradicted-atlas","summary":"Task finding",
        "verification":"Contradictory fixture variants","checks":checks})).unwrap_err();
    assert!(completion_error.contains("requires every planned job to be accepted"),"{completion_error}");
}

#[test]
#[ignore = "requires Atlas PostgreSQL fixture, Node.js 24, and local socket permission"]
fn atlas_s1_missing_export_queue_remains_blocked_coverage() {
    assert!(std::env::var("ATLAS_DATABASE_URL").is_ok());
    let d=Daemon::start(&[]);
    let created=d.call("swarm.create",json!({"category":"Atlas missing queue",
        "objective":"Audit export isolation","allowed_targets":["fixture"]}));
    let run=created["id"].as_str().unwrap();
    d.call("swarm.plan",json!({"id":run,"generation":1,"revision":0,"jobs":[
        {"id":"j5","title":"Exports","acceptance":"workspace-bound queued export","deps":[]}
    ]}));
    let at=now();
    let snapshot=json!({"version":1,"observed_ms":at-1000,"expires_ms":at+120000,
        "targets":[{"id":"fixture","account_id":"account","pool_ids":["pool"],
            "capabilities":["audit"],"health":"up","auth":"ok"}],
        "pools":[{"id":"pool","windows":[{"id":"week","unit":"points",
            "remaining_milli":1000000,"protected_milli":0,"reserved_milli":0,
            "confidence":"exact","expires_ms":at+120000}]}]});
    let attempt=admit(&d,run,1,"j5","fixture",&snapshot,at);
    assert_eq!(attempt["status"],"admitted","{attempt}");
    let evidence=atlas_probe_variant("j5",Some("export-queue-missing"));
    assert_eq!(evidence["foreignExportStatus"],403);
    assert_eq!(evidence["ownExportStatus"],503);
    assert_eq!(evidence["queueAvailable"],false);
    let artifact="atlas-j5-missing-queue";
    d.call("swarm.artifact.put",json!({"run_id":run,"job_id":"j5",
        "attempt_id":attempt["attempt_id"],"token":attempt["token"],
        "artifact_id":artifact,"source_revision":1,"kind":"log",
        "content":evidence.to_string()}));
    d.call("swarm.report",json!({"run_id":run,"job_id":"j5",
        "attempt_id":attempt["attempt_id"],"token":attempt["token"],
        "message_id":"atlas-j5-queue-unavailable","type":"result","revision":1,
        "payload":{"audit_outcome":"environment_failure","artifact_ids":[artifact],
            "unavailable_resource":"exports_queue"}}));
    assert!(d.try_call("swarm.decide",json!({"run_id":run,"generation":1,
        "revision":1,"job_id":"j5","decision":"accept","evidence":[artifact]}))
        .unwrap_err().contains("environment"));
    let coverage=d.call("swarm.coverage",json!({"run_id":run}));
    assert_eq!(coverage["rows"][0]["coverage_state"],"environment_blocked");
    assert_eq!(coverage["rows"][0]["unavailable_resource"],"exports_queue");
    assert_ne!(coverage["rows"][0]["job_status"],"accepted");
    assert!(d.try_call("swarm.complete",json!({"run_id":run,"generation":1,
        "revision":1,"request_id":"atlas-missing-queue-complete",
        "summary":"Exports checked","verification":"Queue unavailable",
        "checks":[{"job_id":"j5","outcome":"passed","evidence":[artifact]}]}))
        .unwrap_err().contains("requires every planned job to be accepted"));
}

// S5: the daemon commits the result, but the sender never receives its reply.
// A retry after restart must not add a second result or acceptance effect.
#[test]
#[ignore = "requires Atlas PostgreSQL fixture, Node.js 24, and local socket permission"]
fn atlas_s5_lost_result_receipt_replays_once_after_restart() {
    assert!(std::env::var("ATLAS_DATABASE_URL").is_ok());
    let mut d = Daemon::start(&[]);
    let created = d.call("swarm.create", json!({"category":"Atlas receipt fault",
        "objective":"Audit foreign task mutation","allowed_targets":["fixture"]}));
    let run = created["id"].as_str().unwrap();
    d.call("swarm.plan", json!({"id":run,"generation":1,"revision":0,"jobs":[
        {"id":"j2","title":"Tasks","acceptance":"foreign patch and before-after rows",
         "deps":[],"resource_claims":[{"resource":"atlas-db-j2","mode":"write"}]}
    ]}));
    let at = now();
    let snapshot = json!({"version":1,"observed_ms":at-1000,"expires_ms":at+120000,
        "targets":[{"id":"fixture","account_id":"account","pool_ids":["pool"],
            "capabilities":["audit"],"health":"up","auth":"ok"}],
        "pools":[{"id":"pool","windows":[{"id":"week","unit":"points",
            "remaining_milli":1000000,"protected_milli":0,"reserved_milli":0,
            "confidence":"exact","expires_ms":at+120000}]}]});
    let attempt = admit(&d,run,1,"j2","fixture",&snapshot,at);
    assert_eq!(attempt["status"],"admitted","{attempt}");

    let evidence = atlas_probe("j2");
    assert_eq!(evidence["foreignPatchStatus"],200);
    assert_eq!(evidence["taskBefore"],"Bob task");
    assert_eq!(evidence["taskAfter"],"changed-by-alice");
    let artifact = "atlas-s5-task-evidence";
    d.call("swarm.artifact.put",json!({"run_id":run,"job_id":"j2",
        "attempt_id":attempt["attempt_id"],"token":attempt["token"],
        "artifact_id":artifact,"source_revision":1,"kind":"reproduction",
        "content":evidence.to_string()}));
    let result = json!({"run_id":run,"job_id":"j2",
        "attempt_id":attempt["attempt_id"],"token":attempt["token"],
        "message_id":"atlas-s5-j2-result","type":"result","revision":1,
        "payload":{"audit_outcome":"confirmed_defect","artifact_ids":[artifact]}});
    assert_eq!(d.call("swarm.report",result.clone())["duplicate"],false);
    // Fault: the durable write committed but its response was lost.
    d.kill9();
    d.spawn();
    assert_eq!(d.call("swarm.report",result)["duplicate"],true);
    let inbox = d.call("swarm.messages",json!({"run_id":run,"recipient":"director"}));
    let copies = inbox["messages"].as_array().unwrap().iter()
        .filter(|message| message["message_id"] == "atlas-s5-j2-result").count();
    assert_eq!(copies,1);
    let decision = json!({"run_id":run,"generation":1,"revision":1,
        "job_id":"j2","decision":"accept","evidence":[artifact]});
    assert_eq!(d.call("swarm.decide",decision.clone())["duplicate"],false);
    assert_eq!(d.call("swarm.decide",decision)["duplicate"],true);
    d.call("swarm.attempt.confirm_exit",json!({"run_id":run,"generation":1,
        "revision":1,"job_id":"j2","attempt_id":attempt["attempt_id"]}));
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let admissions: i64 = db.query_row("SELECT COUNT(*) FROM swarm_admissions WHERE run_id=?1 AND job_id='j2'",[run],|r|r.get(0)).unwrap();
    let artifacts: i64 = db.query_row("SELECT COUNT(*) FROM swarm_artifacts WHERE run_id=?1 AND job_id='j2'",[run],|r|r.get(0)).unwrap();
    let results: i64 = db.query_row("SELECT COUNT(*) FROM swarm_messages WHERE run_id=?1 AND job_id='j2' AND kind='result'",[run],|r|r.get(0)).unwrap();
    let accepts: i64 = db.query_row("SELECT COUNT(*) FROM swarm_decisions WHERE run_id=?1 AND job_id='j2' AND decision='accept'",[run],|r|r.get(0)).unwrap();
    assert_eq!((admissions,artifacts,results,accepts),(1,1,1,1));
    assert_eq!(d.call("swarm.coverage",json!({"run_id":run}))["rows"][0]["coverage_state"],"confirmed_application_defect");
    let batch = d.call("swarm.director.claim_batch",json!({"run_id":run,
        "generation":1,"revision":1,"now_ms":at+6000}));
    assert_eq!(batch["status"],"claimed","{batch}");
    assert_eq!(batch["messages"].as_array().unwrap().iter()
        .filter(|message| message["message_id"] == "atlas-s5-j2-result").count(),1);
    d.call("swarm.director.complete_batch",json!({"run_id":run,"generation":1,
        "turn_id":batch["turn_id"],"token":batch["token"],"outcome":"progress"}));
    let completed = d.call("swarm.complete",json!({"run_id":run,"generation":1,
        "revision":1,"request_id":"atlas-s5-lost-receipt-complete",
        "summary":"Foreign task mutation reproduced","verification":"J2 Atlas PostgreSQL before-after probe",
        "checks":[{"job_id":"j2","outcome":"passed","evidence":[artifact]}]}));
    assert_eq!(completed["status"],"completed","{completed}");
}

// S5: a director disappears after the durable dispatch admission, but before
// the worker launch acknowledgement. The replacement must retain one worker.
#[test]
#[ignore = "requires Atlas PostgreSQL fixture, Node.js 24, and local socket permission"]
fn atlas_s5_director_death_recovers_one_dispatched_worker() {
    assert!(std::env::var("ATLAS_DATABASE_URL").is_ok());
    let mut d = Daemon::start(&[]);
    let temp = tmp();
    let checkout = repo(&temp.path().join("atlas-recovery"));
    let script = repo_root().join("fixtures/swarm/atlas-v1/swarm-j2-worker.mjs");
    let database_url_file = temp.path().join("disposable-database-url");
    std::fs::write(&database_url_file,std::env::var("ATLAS_DATABASE_URL").unwrap()).unwrap();
    let created = d.call("swarm.create", json!({"category":"Atlas dispatch fault",
        "objective":"Audit foreign task mutation","allowed_targets":["fixture"]}));
    let run = created["id"].as_str().unwrap();
    d.call("swarm.plan", json!({"id":run,"generation":1,"revision":0,"jobs":[
        {"id":"j2","title":"Tasks","acceptance":"foreign patch and before-after rows",
         "deps":[],"resource_claims":[{"resource":"atlas-db-j2","mode":"write"}]}
    ]}));
    let at = now();
    let request = json!({"request_id":"atlas-s5-dispatch","target_id":"fixture",
        "repo":checkout,"program":"/usr/bin/env","args":["node",script,database_url_file],
        "now_ms":at,"snapshot":{"version":1,"observed_ms":at-1000,
            "expires_ms":at+120000,
            "targets":[{"id":"fixture","account_id":"account",
                "pool_ids":["pool"],"capabilities":["audit"],
                "health":"up","auth":"ok"}],
            "pools":[{"id":"pool","windows":[{"id":"week","unit":"points",
                "remaining_milli":1000000,"protected_milli":0,"reserved_milli":0,
                "confidence":"exact","expires_ms":at+120000}]}]},
        "required_capabilities":["audit"],"estimate_milli":{"points":100},
        "purpose":"worker","inject_failure_after_admit_once":true});
    assert!(d.try_call("swarm.dispatch.next",request.clone())
        .unwrap_err().contains("injected failure after admission"));
    assert!(d.runs().is_empty());
    let recovered = d.call("swarm.director.recover", json!({"run_id":run,
        "generation":1,"revision":1,"termination":"confirmed_dead"}));
    assert_eq!(recovered["generation"],2);
    assert_eq!(recovered["workers_preserved"],1);
    assert!(d.try_call("swarm.director.claim_batch",json!({"run_id":run,
        "generation":1,"revision":1,"now_ms":at+6000})).is_err());
    assert!(d.try_call("swarm.revise",json!({"id":run,"generation":1,
        "expected_revision":1,"reason":"stale director edit","jobs":[]})).is_err());
    d.kill9();
    d.spawn();
    // Startup replays the persisted launch intent once, without a new admission.
    assert_eq!(d.runs().len(),1);
    let replay = d.call("swarm.dispatch.next",request);
    assert_eq!(replay["status"],"linked","{replay}");
    assert_eq!(replay["duplicate"],true);
    assert_eq!(replay["job_id"],"j2");
    let worker = replay["overseer_run_id"].as_str().unwrap();
    let attempt = replay["attempt_id"].as_str().unwrap();
    let finished = d.wait_done(worker,10);
    assert_eq!(finished["status"],"completed","{finished}; output: {}",
        d.call("run.raw_output",json!({"run_id":worker})));
    let artifact = format!("atlas-s5-recovered-{attempt}");
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let content: String = db.query_row("SELECT content FROM swarm_artifacts WHERE run_id=?1 AND id=?2",
        rusqlite::params![run,artifact],|r|r.get(0)).unwrap();
    let evidence: Value = serde_json::from_str(&content).unwrap();
    assert_eq!(evidence["foreignPatchStatus"],200);
    assert_eq!(evidence["taskBefore"],"Bob task");
    assert_eq!(evidence["taskAfter"],"changed-by-alice");
    let admissions: i64 = db.query_row("SELECT COUNT(*) FROM swarm_admissions WHERE run_id=?1",[run],|r|r.get(0)).unwrap();
    let launches: i64 = db.query_row("SELECT COUNT(*) FROM swarm_worker_launches WHERE run_id=?1 AND overseer_run_id IS NOT NULL",[run],|r|r.get(0)).unwrap();
    assert_eq!((admissions,launches),(1,1));
    drop(db);
    let terminal = d.call("swarm.worker.reconcile",json!({"run_id":run,"job_id":"j2",
        "attempt_id":attempt,"generation":2,"revision":1}));
    assert_eq!(terminal["status"],"terminal","{terminal}");
    assert!(d.try_call("swarm.direct",json!({"run_id":run,"generation":1,
        "revision":1,"job_id":"j2","attempt_id":attempt,
        "message_id":"stale-director","type":"redirect","payload":{}})).is_err());
    let batch = d.call("swarm.director.claim_batch",json!({"run_id":run,
        "generation":2,"revision":1,"now_ms":now()+6000}));
    assert_eq!(batch["status"],"claimed","{batch}");
    assert!(batch["messages"].as_array().unwrap().iter()
        .any(|message| message["message_id"] == format!("atlas-s5-result-{attempt}")));
    let decision = d.call("swarm.decide",json!({"run_id":run,"generation":2,
        "revision":1,"job_id":"j2","decision":"accept","evidence":[artifact]}));
    assert_eq!(decision["status"],"accepted");
    d.call("swarm.director.complete_batch",json!({"run_id":run,"generation":2,
        "turn_id":batch["turn_id"],"token":batch["token"],"outcome":"progress"}));
    let completed = d.call("swarm.complete",json!({"run_id":run,"generation":2,
        "revision":1,"request_id":"atlas-s5-director-recovery-complete",
        "summary":"Foreign task mutation reproduced",
        "verification":"Recovered J2 worker ran Atlas PostgreSQL before-after probe",
        "checks":[{"job_id":"j2","outcome":"passed","evidence":[artifact]}]}));
    assert_eq!(completed["status"],"completed","{completed}");
}

// S5: the J4 worker is inside a real Atlas PostgreSQL probe when a redirect
// arrives. Transport receipt precedes application; expiry holds dependent work.
#[test]
#[ignore = "requires Atlas PostgreSQL fixture, Node.js 24, and local socket permission"]
fn atlas_s5_redirect_during_long_probe_interrupts_and_holds_review() {
    assert!(std::env::var("ATLAS_DATABASE_URL").is_ok());
    let d=Daemon::start(&[]);
    let temp=tmp();
    let checkout=repo(&temp.path().join("atlas-redirect"));
    let script=repo_root().join("fixtures/swarm/atlas-v1/swarm-j4-long-worker.mjs");
    let database_url_file=temp.path().join("disposable-database-url");
    let probe_marker=temp.path().join("j4-probe-active");
    std::fs::write(&database_url_file,std::env::var("ATLAS_DATABASE_URL").unwrap()).unwrap();
    let created=d.call("swarm.create",json!({"category":"Atlas redirect fault",
        "objective":"Audit attachment isolation","allowed_targets":["fixture"]}));
    let run=created["id"].as_str().unwrap();
    d.call("swarm.plan",json!({"id":run,"generation":1,"revision":0,"jobs":[
        {"id":"j4","title":"Attachments","acceptance":"foreign download evidence",
            "deps":[],"resource_claims":[{"resource":"atlas-db-j4","mode":"write"}]},
        {"id":"review","title":"Review attachment finding","acceptance":"validated J4 evidence",
            "deps":["j4"]}
    ]}));
    let at=now();
    let snapshot=json!({"version":1,"observed_ms":at-1000,"expires_ms":at+120000,
        "targets":[{"id":"fixture","account_id":"account","pool_ids":["pool"],
            "capabilities":["audit"],"health":"up","auth":"ok"}],
        "pools":[{"id":"pool","windows":[{"id":"week","unit":"points",
            "remaining_milli":1000000,"protected_milli":0,"reserved_milli":0,
            "confidence":"exact","expires_ms":at+120000}]}]});
    let attempt=admit(&d,run,1,"j4","fixture",&snapshot,at);
    assert_eq!(attempt["status"],"admitted","{attempt}");
    let launched=d.call("swarm.worker.launch",json!({"run_id":run,"job_id":"j4",
        "attempt_id":attempt["attempt_id"],"token":attempt["token"],
        "repo":checkout,"program":"/usr/bin/env",
        "args":["node",script,database_url_file,probe_marker],
        "prompt":"Audit Atlas attachment authorization","title":"Atlas J4"}));
    let worker=launched["overseer_run_id"].as_str().unwrap();
    let until=std::time::Instant::now()+std::time::Duration::from_secs(8);
    while !probe_marker.exists() {
        assert!(std::time::Instant::now()<until,"J4 never entered its Atlas probe");
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let marker:Value=serde_json::from_slice(&std::fs::read(&probe_marker).unwrap()).unwrap();
    assert_eq!(marker["fixtureVersion"],1);
    assert_eq!(marker["job"],"j4");
    assert_eq!(marker["attachmentStatus"],200);
    assert!(["queued","starting","running"].contains(&d.run(worker)["status"].as_str().unwrap()));
    let revised=d.call("swarm.revise",json!({"id":run,"generation":1,
        "expected_revision":1,"reason":"J2 owns the shared helper; J4 checks the signed URL boundary",
        "jobs":[
            {"id":"j4","title":"Attachments","acceptance":"signed URL boundary only",
                "deps":[],"resource_claims":[{"resource":"atlas-db-j4","mode":"write"}]},
            {"id":"review","title":"Review attachment finding","acceptance":"validated J4 evidence",
                "deps":["j4"]}
        ]}));
    assert_eq!(revised["revision"],2);
    assert_eq!(revised["redirected"],1);
    let redirect_id=format!("revision-2-{}",attempt["attempt_id"].as_str().unwrap());
    let db=rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let delivered_by=std::time::Instant::now()+std::time::Duration::from_secs(4);
    loop {
        let phase:String=db.query_row("SELECT phase FROM swarm_messages
            WHERE run_id=?1 AND message_id=?2",[run,redirect_id.as_str()],|r|r.get(0)).unwrap();
        if phase=="delivered" { break; }
        assert!(std::time::Instant::now()<delivered_by,"J4 did not acknowledge delivery: {phase}");
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert!(probe_marker.exists());
    let run_dir:String=db.query_row("SELECT run_dir FROM runs WHERE id=?1",[worker],|r|r.get(0)).unwrap();
    let interrupt=std::path::Path::new(&run_dir).join("interrupt.requested");
    assert!(!interrupt.exists());
    db.execute("UPDATE swarm_messages SET updated_ms=?3 WHERE run_id=?1 AND message_id=?2",
        rusqlite::params![run,redirect_id,now()-30_001]).unwrap();
    let stopped_by=std::time::Instant::now()+std::time::Duration::from_secs(4);
    while !interrupt.exists() {
        assert!(std::time::Instant::now()<stopped_by,"J4 was not interrupted after redirect timeout");
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let phase:String=db.query_row("SELECT phase FROM swarm_messages
        WHERE run_id=?1 AND message_id=?2",[run,redirect_id.as_str()],|r|r.get(0)).unwrap();
    assert_eq!(phase,"delivered");
    let results:i64=db.query_row("SELECT COUNT(*) FROM swarm_messages WHERE run_id=?1
        AND job_id='j4' AND kind='result'",[run],|r|r.get(0)).unwrap();
    let accepts:i64=db.query_row("SELECT COUNT(*) FROM swarm_decisions WHERE run_id=?1
        AND job_id='j4' AND decision='accept'",[run],|r|r.get(0)).unwrap();
    assert_eq!((results,accepts),(0,0));
    let jobs=d.call("swarm.jobs",json!({"id":run}));
    let j4=jobs["jobs"].as_array().unwrap().iter().find(|job|job["id"]=="j4").unwrap();
    assert_eq!(j4["stop_reason"],"redirect_ack_timeout");
    assert_ne!(j4["status"],"accepted");
    let review=jobs["jobs"].as_array().unwrap().iter().find(|job|job["id"]=="review").unwrap();
    assert_eq!(review["status"],"planned");
    assert!(d.try_call("swarm.complete",json!({"run_id":run,"generation":1,
        "revision":2,"request_id":"s5-redirect-complete","summary":"Attachment audit passed",
        "verification":"J4 probe","checks":[]})).is_err());
    let messages=d.call("swarm.messages",json!({"run_id":run,"recipient":attempt["attempt_id"],
        "token":attempt["token"]}));
    assert!(messages["messages"].as_array().unwrap().iter().any(|message|
        message["type"]=="checkpoint" && message["payload"]["reason"]=="redirect_ack_timeout"));
}

// S5: a worker that repeatedly says "working" contributes no audit evidence.
// The original job deadline still interrupts the held backend probe.
#[test]
#[ignore = "requires Atlas PostgreSQL fixture, Node.js 24, and local socket permission"]
fn atlas_s5_progress_heartbeats_do_not_extend_job_deadline() {
    assert!(std::env::var("ATLAS_DATABASE_URL").is_ok());
    let d=Daemon::start(&[]);
    let temp=tmp();
    let checkout=repo(&temp.path().join("atlas-heartbeat"));
    let script=repo_root().join("fixtures/swarm/atlas-v1/swarm-j4-long-worker.mjs");
    let database_url_file=temp.path().join("disposable-database-url");
    let probe_marker=temp.path().join("j4-probe-active");
    std::fs::write(&database_url_file,std::env::var("ATLAS_DATABASE_URL").unwrap()).unwrap();
    let created=d.call("swarm.create",json!({"category":"Atlas heartbeat fault",
        "objective":"Audit attachment isolation","allowed_targets":["fixture"],
        "policy":{"deadline_ms":15000}}));
    let run=created["id"].as_str().unwrap();
    d.call("swarm.plan",json!({"id":run,"generation":1,"revision":0,"jobs":[
        {"id":"j4","title":"Attachments","acceptance":"foreign download evidence",
            "deps":[],"resource_claims":[{"resource":"atlas-db-j4","mode":"write"}]}
    ]}));
    let at=now();
    let attempt=d.call("swarm.admit",json!({"run_id":run,"generation":1,"revision":1,
        "job_id":"j4","target_id":"fixture","request_id":"atlas-s5-heartbeat",
        "job_deadline_ms":4000,"now_ms":at,
        "snapshot":{"version":1,"observed_ms":at-1000,"expires_ms":at+120000,
            "targets":[{"id":"fixture","account_id":"account","pool_ids":["pool"],
                "capabilities":["audit"],"health":"up","auth":"ok"}],
            "pools":[{"id":"pool","windows":[{"id":"week","unit":"points",
                "remaining_milli":1000000,"protected_milli":0,"reserved_milli":0,
                "confidence":"exact","expires_ms":at+120000}]}]},
        "required_capabilities":["audit"],"estimate_milli":{"points":100},
        "purpose":"worker"}));
    assert_eq!(attempt["status"],"admitted","{attempt}");
    let original_deadline=d.call("swarm.jobs",json!({"id":run}))["jobs"][0]
        ["deadline_at_ms"].as_i64().unwrap();
    let launched=d.call("swarm.worker.launch",json!({"run_id":run,"job_id":"j4",
        "attempt_id":attempt["attempt_id"],"token":attempt["token"],
        "repo":checkout,"program":"/usr/bin/env",
        "args":["node",script,database_url_file,probe_marker,"heartbeat"],
        "prompt":"Audit Atlas attachment authorization","title":"Atlas J4 heartbeat"}));
    let worker=launched["overseer_run_id"].as_str().unwrap();
    let db=rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let run_dir:String=db.query_row("SELECT run_dir FROM runs WHERE id=?1",[worker],|r|r.get(0)).unwrap();
    let marker=std::path::Path::new(&run_dir).join("interrupt.requested");
    let until=std::time::Instant::now()+std::time::Duration::from_secs(8);
    while !marker.exists() {
        assert!(std::time::Instant::now()<until,"heartbeat worker did not hit its job deadline");
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert!(probe_marker.exists(),"Atlas backend probe did not run");
    let probe:Value=serde_json::from_slice(&std::fs::read(&probe_marker).unwrap()).unwrap();
    assert_eq!(probe["attachmentStatus"],200);
    let progress:i64=db.query_row("SELECT COUNT(*) FROM swarm_messages WHERE run_id=?1
        AND job_id='j4' AND kind='progress'",[run],|r|r.get(0)).unwrap();
    assert!(progress>=2,"worker did not send repeated progress: {progress}");
    let job=&d.call("swarm.jobs",json!({"id":run}))["jobs"][0];
    assert_eq!(job["deadline_at_ms"],original_deadline);
    assert_eq!(job["stop_reason"],"job_deadline");
    assert_ne!(job["status"],"accepted");
    let results:i64=db.query_row("SELECT COUNT(*) FROM swarm_messages WHERE run_id=?1
        AND job_id='j4' AND kind='result'",[run],|r|r.get(0)).unwrap();
    let accepts:i64=db.query_row("SELECT COUNT(*) FROM swarm_decisions WHERE run_id=?1
        AND job_id='j4' AND decision='accept'",[run],|r|r.get(0)).unwrap();
    assert_eq!((results,accepts),(0,0));
    assert!(d.try_call("swarm.complete",json!({"run_id":run,"generation":1,
        "revision":1,"request_id":"s5-heartbeat-complete","summary":"Attachment audit passed",
        "verification":"Repeated progress","checks":[]})).is_err());
}

// S5: malformed director dependencies are rejected as a subgraph. A valid J2
// audit remains dispatchable and can still finish with real Atlas evidence.
#[test]
#[ignore = "requires Atlas PostgreSQL fixture, Node.js 24, and local socket permission"]
fn atlas_s5_invalid_dependency_subgraph_does_not_block_valid_audit() {
    assert!(std::env::var("ATLAS_DATABASE_URL").is_ok());
    let d=Daemon::start(&[]);
    let created=d.call("swarm.create",json!({"category":"Atlas malformed plan fault",
        "objective":"Audit foreign task mutation","allowed_targets":["fixture"]}));
    let run=created["id"].as_str().unwrap();
    let jobs=json!([
        {"id":"j2","title":"Tasks","acceptance":"foreign patch and before-after rows",
            "deps":[],"resource_claims":[{"resource":"atlas-db-j2","mode":"write"}]},
        {"id":"j8","title":"Cycle A","acceptance":"check","deps":["j9"]},
        {"id":"j9","title":"Cycle B","acceptance":"check","deps":["j8"]},
        {"id":"j10","title":"Unknown dependency","acceptance":"check","deps":["j99"]},
        {"id":"j11","title":"Depends on cycle","acceptance":"check","deps":["j8"]}
    ]);
    assert!(d.try_call("swarm.plan",json!({"id":run,"generation":1,
        "revision":0,"jobs":jobs})).is_err());
    assert_eq!(d.call("swarm.get",json!({"id":run}))["revision"],0);
    assert!(d.call("swarm.jobs",json!({"id":run}))["jobs"].as_array().unwrap().is_empty());
    let plan=d.call("swarm.plan",json!({"id":run,"generation":1,
        "revision":0,"allow_partial":true,"jobs":jobs}));
    assert_eq!(plan["revision"],1);
    assert_eq!(plan["job_count"],1);
    let rejected=plan["rejected"].as_array().unwrap();
    assert_eq!(rejected.len(),4,"{plan}");
    for id in ["j8","j9","j10","j11"] {
        assert!(rejected.iter().any(|entry|entry["id"]==id),"{plan}");
        assert!(d.try_call("swarm.attempt.register",json!({"run_id":run,
            "generation":1,"revision":1,"job_id":id})).is_err());
    }
    assert!(rejected.iter().any(|entry|entry["id"]=="j10" &&
        entry["reason"].as_str().unwrap().contains("j99")),"{plan}");
    let current=d.call("swarm.jobs",json!({"id":run}));
    assert_eq!(current["jobs"].as_array().unwrap().len(),1);
    assert_eq!(current["jobs"][0]["id"],"j2");

    let at=now();
    let snapshot=json!({"version":1,"observed_ms":at-1000,"expires_ms":at+120000,
        "targets":[{"id":"fixture","account_id":"account","pool_ids":["pool"],
            "capabilities":["audit"],"health":"up","auth":"ok"}],
        "pools":[{"id":"pool","windows":[{"id":"week","unit":"points",
            "remaining_milli":1000000,"protected_milli":0,"reserved_milli":0,
            "confidence":"exact","expires_ms":at+120000}]}]});
    let attempt=admit(&d,run,1,"j2","fixture",&snapshot,at);
    assert_eq!(attempt["status"],"admitted","{attempt}");
    let evidence=atlas_probe("j2");
    assert_eq!(evidence["foreignPatchStatus"],200);
    assert_eq!(evidence["taskBefore"],"Bob task");
    assert_eq!(evidence["taskAfter"],"changed-by-alice");
    let artifact=submit(&d,run,"j2",&attempt,1,&evidence,"confirmed_defect");
    accept_and_exit(&d,run,1,"j2",&attempt,&artifact);
    let db=rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let admissions:i64=db.query_row("SELECT COUNT(*) FROM swarm_admissions WHERE run_id=?1",
        [run],|r|r.get(0)).unwrap();
    assert_eq!(admissions,1);
    let batch=d.call("swarm.director.claim_batch",json!({"run_id":run,
        "generation":1,"revision":1,"now_ms":at+6000}));
    assert_eq!(batch["status"],"claimed","{batch}");
    assert!(batch["messages"].as_array().unwrap().iter()
        .any(|message|message["job_id"]=="j2" && message["type"]=="result"));
    d.call("swarm.director.complete_batch",json!({"run_id":run,"generation":1,
        "turn_id":batch["turn_id"],"token":batch["token"],"outcome":"progress"}));
    let completed=d.call("swarm.complete",json!({"run_id":run,"generation":1,
        "revision":1,"request_id":"atlas-s5-valid-subgraph-complete",
        "summary":"Foreign task mutation reproduced; invalid dependency subgraph excluded",
        "verification":"J2 Atlas PostgreSQL before-after probe",
        "checks":[{"job_id":"j2","outcome":"passed","evidence":[artifact]}]}));
    assert_eq!(completed["status"],"completed","{completed}");
}

// S5: two admitted jobs really mutate one Atlas PostgreSQL schema despite
// distinct planned claims. Late observation quarantines both evidence chains.
#[test]
#[ignore = "requires Atlas PostgreSQL fixture, Node.js 24, and local socket permission"]
fn atlas_s5_shared_database_contamination_quarantines_and_retries() {
    assert!(std::env::var("ATLAS_DATABASE_URL").is_ok());
    let d=Daemon::start(&[]);
    let created=d.call("swarm.create",json!({"category":"Atlas shared DB fault",
        "objective":"Independently audit foreign task mutation","allowed_targets":["fixture"],
        "policy":{"max_workers":2}}));
    let run=created["id"].as_str().unwrap();
    d.call("swarm.plan",json!({"id":run,"generation":1,"revision":0,"jobs":[
        {"id":"j2","title":"Tasks","acceptance":"foreign patch and before-after rows",
            "deps":[],"resource_claims":[{"resource":"atlas-db-j2","mode":"write"}]},
        {"id":"j7","title":"Independent task reproduction",
            "acceptance":"fresh foreign patch before-after rows","deps":[],
            "resource_claims":[{"resource":"atlas-db-j7","mode":"write"}]}
    ]}));
    let at=now();
    let snapshot=json!({"version":1,"observed_ms":at-1000,"expires_ms":at+120000,
        "targets":[{"id":"fixture","account_id":"account","pool_ids":["pool"],
            "capabilities":["audit"],"health":"up","auth":"ok"}],
        "pools":[{"id":"pool","windows":[{"id":"week","unit":"points",
            "remaining_milli":1000000,"protected_milli":0,"reserved_milli":0,
            "confidence":"exact","expires_ms":at+120000}]}]});
    commit_wave(&d,run,1,&["j2","j7"]);
    let a2=admit(&d,run,1,"j2","fixture",&snapshot,at);
    let a7=admit(&d,run,1,"j7","fixture",&snapshot,at);
    assert_eq!(a2["status"],"admitted","{a2}");
    assert_eq!(a7["status"],"admitted","{a7}");
    let output=Command::new("node").arg("probe-shared.mjs")
        .current_dir(repo_root().join("fixtures/swarm/atlas-v1")).output().unwrap();
    assert!(output.status.success(),"{}",String::from_utf8_lossy(&output.stderr));
    let shared:Value=serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(shared["fixtureVersion"],1);
    assert_eq!(shared["fault"],"shared-schema");
    assert_eq!(shared["j2"]["taskBefore"],"Bob task");
    assert_eq!(shared["j2"]["foreignPatchStatus"],200);
    assert_eq!(shared["j7"]["taskInitially"],"Bob task");
    assert_eq!(shared["j7"]["taskBefore"],"changed-by-alice");
    assert_eq!(shared["j7"]["foreignPatchStatus"],200);
    assert_eq!(shared["j7"]["taskAfter"],"changed-by-j7");
    assert_eq!(shared["j2"]["taskAtEnd"],"changed-by-j7");
    assert_eq!(shared["j2"]["namespace"],shared["j7"]["namespace"]);
    for (job,attempt) in [("j2",&a2),("j7",&a7)] {
        let artifact=format!("atlas-shared-{job}");
        d.call("swarm.artifact.put",json!({"run_id":run,"job_id":job,
            "attempt_id":attempt["attempt_id"],"token":attempt["token"],
            "artifact_id":artifact,"source_revision":1,"kind":"reproduction",
            "content":shared[job].to_string()}));
        d.call("swarm.report",json!({"run_id":run,"job_id":job,
            "attempt_id":attempt["attempt_id"],"token":attempt["token"],
            "message_id":format!("atlas-shared-{job}-result"),"type":"result",
            "revision":1,"payload":{"artifact_ids":[artifact]}}));
    }
    let observe=|job:&str,attempt:&Value|json!({"run_id":run,"job_id":job,
        "attempt_id":attempt["attempt_id"],"token":attempt["token"],
        "generation":1,"revision":1,"resource":shared["resource"],
        "mode":"write","after_use":true});
    d.call("swarm.claim",observe("j2",&a2));
    let conflict=d.call("swarm.claim",observe("j7",&a7));
    assert_eq!(conflict["status"],"contaminated","{conflict}");
    for (job,attempt) in [("j2",&a2),("j7",&a7)] {
        let row=d.call("swarm.jobs",json!({"id":run}))["jobs"]
            .as_array().unwrap().iter().find(|row|row["id"]==job).unwrap().clone();
        assert_eq!(row["status"],"cancel_requested");
        let coverage=d.call("swarm.coverage",json!({"run_id":run}));
        assert!(coverage["rows"].as_array().unwrap().iter()
            .any(|row|row["job_id"]==job && row["coverage_state"]=="contaminated"));
        assert!(d.try_call("swarm.decide",json!({"run_id":run,"generation":1,
            "revision":1,"job_id":job,"decision":"accept",
            "evidence":[format!("atlas-shared-{job}")]})).is_err());
        d.call("swarm.attempt.confirm_exit",json!({"run_id":run,"generation":1,
            "revision":1,"job_id":job,"attempt_id":attempt["attempt_id"]}));
    }
    let db=rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let retained:i64=db.query_row("SELECT COUNT(*) FROM swarm_artifacts WHERE run_id=?1",
        [run],|r|r.get(0)).unwrap();
    assert_eq!(retained,2);
    let mut retry_namespaces=Vec::new();
    for job in ["j2","j7"] {
        let retry=d.call("swarm.admit",json!({"run_id":run,"generation":1,
            "revision":1,"job_id":job,"target_id":"fixture",
            "request_id":format!("atlas-shared-{job}-retry"),
            "snapshot":snapshot,"now_ms":now(),"required_capabilities":["audit"],
            "estimate_milli":{"points":100},"purpose":"worker"}));
        assert_eq!(retry["status"],"admitted","{retry}");
        let clean=atlas_probe(job);
        assert_eq!(clean["taskBefore"],"Bob task");
        assert_eq!(clean["foreignPatchStatus"],200);
        assert_ne!(clean["namespace"],shared["namespace"]);
        retry_namespaces.push(clean["namespace"].as_str().unwrap().to_owned());
        let claim=d.call("swarm.claim",json!({"run_id":run,"job_id":job,
            "attempt_id":retry["attempt_id"],"token":retry["token"],
            "generation":1,"revision":1,
            "resource":format!("atlas-db:{}",clean["namespace"].as_str().unwrap()),
            "mode":"write","after_use":true}));
        assert_ne!(claim["status"],"contaminated","{claim}");
        let artifact=format!("atlas-{job}-retry-evidence");
        d.call("swarm.artifact.put",json!({"run_id":run,"job_id":job,
            "attempt_id":retry["attempt_id"],"token":retry["token"],
            "artifact_id":artifact,"source_revision":1,"kind":"reproduction",
            "content":clean.to_string()}));
        d.call("swarm.report",json!({"run_id":run,"job_id":job,
            "attempt_id":retry["attempt_id"],"token":retry["token"],
            "message_id":format!("atlas-{job}-retry-result"),"type":"result",
            "revision":1,"payload":{"artifact_ids":[artifact]}}));
        accept_and_exit(&d,run,1,job,&retry,&artifact);
    }
    assert_ne!(retry_namespaces[0],retry_namespaces[1]);
    let admissions:i64=db.query_row("SELECT COUNT(*) FROM swarm_admissions WHERE run_id=?1",
        [run],|r|r.get(0)).unwrap();
    assert_eq!(admissions,4);
    for _ in 0..8 {
        let pending:i64=db.query_row("SELECT COUNT(*) FROM swarm_messages WHERE run_id=?1
            AND recipient='director' AND phase!='applied'",[run],|r|r.get(0)).unwrap();
        if pending==0 { break; }
        let batch=d.call("swarm.director.claim_batch",json!({"run_id":run,
            "generation":1,"revision":1,"now_ms":now()+12000}));
        assert_eq!(batch["status"],"claimed","{batch}; pending={pending}");
        d.call("swarm.director.complete_batch",json!({"run_id":run,"generation":1,
            "turn_id":batch["turn_id"],"token":batch["token"],"outcome":"progress"}));
    }
    let pending:i64=db.query_row("SELECT COUNT(*) FROM swarm_messages WHERE run_id=?1
        AND recipient='director' AND phase!='applied'",[run],|r|r.get(0)).unwrap();
    assert_eq!(pending,0);
    let completed=d.call("swarm.complete",json!({"run_id":run,"generation":1,
        "revision":1,"request_id":"atlas-s5-shared-db-complete",
        "summary":"Foreign task mutation reproduced in isolated retry namespaces",
        "verification":"Shared namespace evidence quarantined; two fresh Atlas probes",
        "checks":[{"job_id":"j2","outcome":"passed","evidence":["atlas-j2-retry-evidence"]},
            {"job_id":"j7","outcome":"passed","evidence":["atlas-j7-retry-evidence"]}]}));
    assert_eq!(completed["status"],"completed","{completed}");
}

// S5: duplicate progress from a worker with real Atlas probe evidence must
// collapse to one director envelope while a user Stop cuts through the flood.
#[test]
#[ignore = "requires Atlas PostgreSQL fixture, Node.js 24, and local socket permission"]
fn atlas_s5_two_thousand_duplicate_progress_messages_do_not_starve_stop() {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixStream;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    assert!(std::env::var("ATLAS_DATABASE_URL").is_ok());
    let d=Daemon::start(&[]);
    let created=d.call("swarm.create",json!({"category":"Atlas duplicate progress fault",
        "objective":"Audit foreign task mutation","allowed_targets":["fixture"]}));
    let run=created["id"].as_str().unwrap();
    d.call("swarm.plan",json!({"id":run,"generation":1,"revision":0,"jobs":[
        {"id":"j2","title":"Tasks","acceptance":"foreign patch before-after rows",
            "deps":[],"resource_claims":[{"resource":"atlas-db-j2","mode":"write"}]}
    ]}));
    let attempt=register(&d,run,"j2");
    let evidence=atlas_probe("j2");
    assert_eq!(evidence["foreignPatchStatus"],200);
    assert_eq!(evidence["taskBefore"],"Bob task");
    assert_eq!(evidence["taskAfter"],"changed-by-alice");
    d.call("swarm.artifact.put",json!({"run_id":run,"job_id":"j2",
        "attempt_id":attempt["attempt_id"],"token":attempt["token"],
        "artifact_id":"atlas-j2-progress-probe","source_revision":1,
        "kind":"reproduction","content":evidence.to_string()}));
    let report=json!({"run_id":run,"job_id":"j2","attempt_id":attempt["attempt_id"],
        "token":attempt["token"],"message_id":"atlas-j2-replayed-progress",
        "type":"progress","revision":1,
        "payload":{"step":"backend probe complete","artifact_id":"atlas-j2-progress-probe"}});
    assert_eq!(d.call("swarm.report",report.clone())["duplicate"],false);
    let socket=d.socket();
    let (started_tx,started_rx)=mpsc::channel();
    let flood=std::thread::spawn(move || {
        for index in 0..2000 {
            let mut conn=UnixStream::connect(&socket).unwrap();
            conn.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            writeln!(conn,"{}",json!({"id":index,"method":"swarm.report",
                "params":report})).unwrap();
            let mut line=String::new();
            BufReader::new(conn).read_line(&mut line).unwrap();
            let reply:Value=serde_json::from_str(&line).unwrap();
            assert_eq!(reply["result"]["duplicate"],true,"{reply}");
            if index==20 { started_tx.send(()).unwrap(); }
            std::thread::sleep(Duration::from_millis(1));
        }
    });
    started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let begin=Instant::now();
    let stopped=d.call("swarm.stop",json!({"run_id":run}));
    let stop_elapsed=begin.elapsed();
    eprintln!("Atlas duplicate-flood Stop acknowledged in {} ms",stop_elapsed.as_millis());
    assert!(stop_elapsed<Duration::from_secs(2),"Stop was starved by duplicate updates");
    assert_eq!(stopped["status"],"stopping","{stopped}");
    flood.join().unwrap();
    let db=rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let counts:(i64,i64,i64)=db.query_row("SELECT
        (SELECT COUNT(*) FROM swarm_messages WHERE run_id=?1 AND kind='progress'),
        (SELECT COUNT(*) FROM swarm_artifacts WHERE run_id=?1),
        (SELECT COUNT(*) FROM swarm_decisions WHERE run_id=?1)",
        [run],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap();
    assert_eq!(counts,(1,1,0));
    let inbox=d.call("swarm.messages",json!({"run_id":run,"recipient":"director"}));
    assert_eq!(inbox["messages"].as_array().unwrap().len(),1);
    assert_eq!(inbox["messages"][0]["message_id"],"atlas-j2-replayed-progress");
    assert_ne!(d.call("swarm.jobs",json!({"id":run}))["jobs"][0]["status"],"accepted");
    assert!(d.try_call("swarm.complete",json!({"run_id":run,"generation":1,
        "revision":1,"request_id":"atlas-duplicate-progress-complete",
        "summary":"Task audit complete","verification":"Backend probe","checks":[]})).is_err());
    d.call("swarm.attempt.confirm_exit",json!({"run_id":run,"generation":1,
        "revision":1,"job_id":"j2","attempt_id":attempt["attempt_id"]}));
    assert_eq!(d.call("swarm.get",json!({"id":run}))["status"],"stopped");
}

// S5: the final Atlas probe and user Stop can commit in either order. The
// artifact survives both orderings, but neither may publish a final verdict.
#[test]
#[ignore = "requires Atlas PostgreSQL fixture, Node.js 24, and local socket permission"]
fn atlas_s5_stop_as_last_result_arrives_preserves_evidence_without_completion() {
    assert!(std::env::var("ATLAS_DATABASE_URL").is_ok());
    for stop_first in [true, false] {
        let d=Daemon::start(&[]);
        let created=d.call("swarm.create",json!({"category":"Atlas final-result Stop fault",
            "objective":"Audit foreign task mutation","allowed_targets":["fixture"]}));
        let run=created["id"].as_str().unwrap();
        d.call("swarm.plan",json!({"id":run,"generation":1,"revision":0,"jobs":[
            {"id":"j2","title":"Tasks","acceptance":"foreign patch before-after rows",
                "deps":[],"resource_claims":[{"resource":"atlas-db-j2","mode":"write"}]}
        ]}));
        let attempt=register(&d,run,"j2");
        let evidence=atlas_probe("j2");
        assert_eq!(evidence["foreignPatchStatus"],200);
        assert_eq!(evidence["taskBefore"],"Bob task");
        assert_eq!(evidence["taskAfter"],"changed-by-alice");
        let artifact="atlas-j2-final-stop-proof";
        d.call("swarm.artifact.put",json!({"run_id":run,"job_id":"j2",
            "attempt_id":attempt["attempt_id"],"token":attempt["token"],
            "artifact_id":artifact,"source_revision":1,"kind":"reproduction",
            "content":evidence.to_string()}));
        let result=json!({"run_id":run,"job_id":"j2",
            "attempt_id":attempt["attempt_id"],"token":attempt["token"],
            "message_id":"atlas-j2-final-stop-result","type":"result","revision":1,
            "payload":{"audit_outcome":"confirmed_defect","artifact_ids":[artifact]}});
        if stop_first {
            assert_eq!(d.call("swarm.stop",json!({"run_id":run}))["status"],"stopping");
        }
        assert_eq!(d.call("swarm.report",result.clone())["duplicate"],false);
        if !stop_first {
            assert_eq!(d.call("swarm.stop",json!({"run_id":run}))["status"],"stopping");
        }
        assert_eq!(d.call("swarm.report",result)["duplicate"],true);
        assert!(d.try_call("swarm.decide",json!({"run_id":run,"generation":1,
            "revision":1,"job_id":"j2","decision":"accept",
            "evidence":[artifact]})).is_err());
        assert!(d.try_call("swarm.complete",json!({"run_id":run,"generation":1,
            "revision":1,"request_id":"atlas-last-result-stop-complete",
            "summary":"Task audit complete","verification":"Atlas PostgreSQL probe",
            "checks":[{"job_id":"j2","outcome":"passed","evidence":[artifact]}]})).is_err());
        let db=rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
        let counts:(i64,i64,i64,i64)=db.query_row("SELECT
            (SELECT COUNT(*) FROM swarm_artifacts WHERE run_id=?1),
            (SELECT COUNT(*) FROM swarm_messages WHERE run_id=?1 AND kind='result'),
            (SELECT COUNT(*) FROM swarm_decisions WHERE run_id=?1),
            (SELECT COUNT(*) FROM swarm_completions WHERE run_id=?1)",
            [run],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).unwrap();
        assert_eq!(counts,(1,1,0,0),"stop_first={stop_first}");
        let mut order=db.prepare("SELECT kind FROM swarm_operation_order
            WHERE run_id=?1 AND kind IN ('stop','result') ORDER BY seq").unwrap();
        let order:Vec<String>=order.query_map([run],|r|r.get(0)).unwrap()
            .collect::<rusqlite::Result<_>>().unwrap();
        assert_eq!(order,if stop_first { vec!["stop","result"] }
            else { vec!["result","stop"] },"stop_first={stop_first}");
        d.call("swarm.attempt.confirm_exit",json!({"run_id":run,"generation":1,
            "revision":1,"job_id":"j2","attempt_id":attempt["attempt_id"]}));
        assert_eq!(d.call("swarm.get",json!({"id":run}))["status"],"stopped");
    }
}

// S5: a full durable store must not acknowledge an Atlas reproduction
// artifact. The sender keeps the same artifact and result IDs across restart.
#[test]
#[ignore = "requires Atlas PostgreSQL fixture, Node.js 24, and local socket permission"]
fn atlas_s5_full_storage_replays_unacknowledged_evidence_after_recovery() {
    assert!(std::env::var("ATLAS_DATABASE_URL").is_ok());
    let mut d=Daemon::start(&[]);
    let created=d.call("swarm.create",json!({"category":"Atlas full-storage fault",
        "objective":"Audit foreign task mutation","allowed_targets":["fixture"]}));
    let run=created["id"].as_str().unwrap();
    d.call("swarm.plan",json!({"id":run,"generation":1,"revision":0,"jobs":[
        {"id":"j2","title":"Tasks","acceptance":"foreign patch before-after rows",
            "deps":[],"resource_claims":[{"resource":"atlas-db-j2","mode":"write"}]},
        {"id":"j4","title":"Attachments","acceptance":"foreign signed URL",
            "deps":[],"resource_claims":[{"resource":"atlas-db-j4","mode":"write"}]}
    ]}));
    let attempt=register(&d,run,"j2");
    let evidence=atlas_probe("j2");
    assert_eq!(evidence["foreignPatchStatus"],200);
    assert_eq!(evidence["taskBefore"],"Bob task");
    assert_eq!(evidence["taskAfter"],"changed-by-alice");
    let at=now();
    let snapshot=json!({"version":1,"observed_ms":at-1000,"expires_ms":at+120000,
        "targets":[{"id":"fixture","account_id":"account","pool_ids":["pool"],
            "capabilities":["audit"],"health":"up","auth":"ok"}],
        "pools":[{"id":"pool","windows":[{"id":"week","unit":"points",
            "remaining_milli":1000000,"protected_milli":0,"reserved_milli":0,
            "confidence":"exact","expires_ms":at+120000}]}]});
    let artifact="atlas-j2-full-storage-proof";
    let artifact_request=json!({"run_id":run,"job_id":"j2",
        "attempt_id":attempt["attempt_id"],"token":attempt["token"],
        "artifact_id":artifact,"source_revision":1,"kind":"reproduction",
        "content":json!({"probe":evidence,"padding":"x".repeat(28_000)}).to_string()});
    d.call("swarm.storage.limit_pages",json!({"mode":"current"}));
    assert!(d.try_call("swarm.artifact.put",artifact_request.clone()).is_err());
    assert_eq!(d.call("swarm.storage.status",json!({}))["state"],"blocked");
    let db=rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let saved:i64=db.query_row("SELECT COUNT(*) FROM swarm_artifacts WHERE run_id=?1",
        [run],|r|r.get(0)).unwrap();
    assert_eq!(saved,0,"failed durable artifact write was acknowledged");
    let admission=json!({"run_id":run,"generation":1,
        "revision":1,"job_id":"j4","target_id":"fixture",
        "request_id":"atlas-full-storage-held","snapshot":snapshot,
        "now_ms":at,"required_capabilities":["audit"],
        "estimate_milli":{"points":100},"purpose":"worker"});
    assert!(d.try_call("swarm.admit",admission.clone()).unwrap_err().contains("storage"));
    d.env.push(("OVERSEER_TEST_SWARM_STORAGE_PAGE_LIMIT".into(),"current".into()));
    d.kill9();
    d.spawn();
    assert_eq!(d.call("swarm.storage.status",json!({}))["state"],"blocked");
    assert!(d.try_call("swarm.admit",admission.clone()).unwrap_err().contains("storage"));
    assert!(d.try_call("swarm.storage.recover",json!({})).is_err());
    d.call("swarm.storage.limit_pages",json!({"mode":"unlimited"}));
    assert_eq!(d.call("swarm.storage.recover",json!({}))["state"],"ready");
    assert_eq!(d.call("swarm.artifact.put",artifact_request.clone())["duplicate"],false);
    assert_eq!(d.call("swarm.artifact.put",artifact_request)["duplicate"],true);
    let result=json!({"run_id":run,"job_id":"j2",
        "attempt_id":attempt["attempt_id"],"token":attempt["token"],
        "message_id":"atlas-j2-after-full-result","type":"result","revision":1,
        "payload":{"audit_outcome":"confirmed_defect","artifact_ids":[artifact]}});
    assert_eq!(d.call("swarm.report",result.clone())["duplicate"],false);
    assert_eq!(d.call("swarm.report",result)["duplicate"],true);
    let counts:(i64,i64)=db.query_row("SELECT
        (SELECT COUNT(*) FROM swarm_artifacts WHERE run_id=?1),
        (SELECT COUNT(*) FROM swarm_messages WHERE run_id=?1 AND kind='result')",
        [run],|r|Ok((r.get(0)?,r.get(1)?))).unwrap();
    assert_eq!(counts,(1,1));
    assert_eq!(d.call("swarm.decide",json!({"run_id":run,"generation":1,
        "revision":1,"job_id":"j2","decision":"accept",
        "evidence":[artifact]}))["status"],"accepted");
    d.call("swarm.attempt.confirm_exit",json!({"run_id":run,"generation":1,
        "revision":1,"job_id":"j2","attempt_id":attempt["attempt_id"]}));
    assert_eq!(d.call("swarm.admit",admission)["status"],"admitted");
}

// S5: an external allowance observation removes headroom while an Atlas probe
// is active. New work must stay held, but its existing evidence is reviewable.
#[test]
#[ignore = "requires Atlas PostgreSQL fixture, Node.js 24, and local socket permission"]
fn atlas_s5_external_allowance_drop_holds_new_work_without_losing_evidence() {
    assert!(std::env::var("ATLAS_DATABASE_URL").is_ok());
    let d=Daemon::start(&[]);
    let created=d.call("swarm.create",json!({"category":"Atlas allowance-drop fault",
        "objective":"Audit cross-workspace endpoints","allowed_targets":["fixture"]}));
    let run=created["id"].as_str().unwrap();
    d.call("swarm.plan",json!({"id":run,"generation":1,"revision":0,"jobs":[
        {"id":"j2","title":"Tasks","acceptance":"foreign task before-after rows",
            "deps":[],"resource_claims":[{"resource":"atlas-db-j2","mode":"write"}]},
        {"id":"j4","title":"Attachments","acceptance":"foreign signed URL",
            "deps":[],"resource_claims":[{"resource":"atlas-db-j4","mode":"write"}]}
    ]}));
    let at=now();
    let snapshot=|observed:i64,remaining:i64|json!({"version":1,
        "observed_ms":observed,"expires_ms":observed+120000,
        "targets":[{"id":"fixture","account_id":"account","pool_ids":["pool"],
            "capabilities":["audit"],"health":"up","auth":"ok"}],
        "pools":[{"id":"pool","windows":[{"id":"week","unit":"points",
            "remaining_milli":remaining,"protected_milli":0,"reserved_milli":0,
            "confidence":"exact","expires_ms":observed+120000}]}]});
    let observe=|when:i64,remaining:i64|d.call("swarm.availability.observe",json!({
        "run_id":run,"snapshot":snapshot(when-1000,remaining),"now_ms":when,
        "required_capabilities":["audit"],"estimate_milli":{"points":100},
        "purpose":"worker"}));
    assert_eq!(observe(at,100000)["state"],"eligible");
    let first=admit(&d,run,1,"j2","fixture",&snapshot(at-1000,100000),at);
    assert_eq!(first["status"],"admitted","{first}");
    let reduced=observe(at+2000,500);
    assert_eq!(reduced["state"],"blocked","{reduced}");
    assert_eq!(reduced["reason"],"finishing_reserve");
    assert_eq!(reduced["woken"],false);
    let readout=d.call("swarm.get",json!({"id":run}));
    assert_eq!(readout["availability"]["observed_ms"],at+1000);
    assert_eq!(readout["availability"]["reason"],"finishing_reserve");
    let next=admit(&d,run,1,"j4","fixture",&snapshot(at-1000,100000),at+2000);
    assert_eq!(next["status"],"blocked","{next}");
    assert_eq!(next["reason"],"run_availability_blocked");
    let evidence=atlas_probe("j2");
    assert_eq!(evidence["foreignPatchStatus"],200);
    assert_eq!(evidence["taskBefore"],"Bob task");
    assert_eq!(evidence["taskAfter"],"changed-by-alice");
    let artifact=submit(&d,run,"j2",&first,1,&evidence,"confirmed_defect");
    accept_and_exit(&d,run,1,"j2",&first,&artifact);
    assert_eq!(d.call("swarm.coverage",json!({"run_id":run}))["rows"][0]["coverage_state"],
        "confirmed_application_defect");
    assert_eq!(d.call("swarm.get",json!({"id":run}))["availability"]["state"],"blocked");
    let db=rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let counts:(i64,i64,i64)=db.query_row("SELECT
        (SELECT COUNT(*) FROM swarm_artifacts WHERE run_id=?1),
        (SELECT COUNT(*) FROM swarm_decisions WHERE run_id=?1 AND decision='accept'),
        (SELECT COUNT(*) FROM swarm_admissions WHERE run_id=?1 AND job_id='j4')",
        [run],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap();
    assert_eq!(counts,(1,1,0));
    assert!(d.try_call("swarm.complete",json!({"run_id":run,"generation":1,
        "revision":1,"request_id":"atlas-allowance-drop-complete",
        "summary":"Audit complete","verification":"Atlas PostgreSQL probe",
        "checks":[{"job_id":"j2","outcome":"passed","evidence":[artifact]}]})).is_err());
}

// S5: the original run clock keeps advancing while all allowed accounts are
// unavailable. Deadline control checkpoints and interrupts the active probe.
#[test]
#[ignore = "requires Atlas PostgreSQL fixture, Node.js 24, and local socket permission"]
fn atlas_s5_run_deadline_expires_while_all_targets_are_blocked() {
    assert!(std::env::var("ATLAS_DATABASE_URL").is_ok());
    let d=Daemon::start(&[]);
    let temp=tmp();
    let checkout=repo(&temp.path().join("atlas-blocked-deadline"));
    let script=repo_root().join("fixtures/swarm/atlas-v1/swarm-j4-long-worker.mjs");
    let database_url_file=temp.path().join("disposable-database-url");
    let probe_marker=temp.path().join("j4-probe-active");
    std::fs::write(&database_url_file,std::env::var("ATLAS_DATABASE_URL").unwrap()).unwrap();
    let created=d.call("swarm.create",json!({"category":"Atlas blocked deadline fault",
        "objective":"Audit foreign attachment and task access","allowed_targets":["fixture"],
        "policy":{"deadline_ms":8000}}));
    let run=created["id"].as_str().unwrap();
    d.call("swarm.plan",json!({"id":run,"generation":1,"revision":0,"jobs":[
        {"id":"j4","title":"Attachments","acceptance":"foreign object response",
            "deps":[],"resource_claims":[{"resource":"atlas-db-j4","mode":"write"}]},
        {"id":"j2","title":"Tasks","acceptance":"foreign task before-after rows",
            "deps":[],"resource_claims":[{"resource":"atlas-db-j2","mode":"write"}]}
    ]}));
    let at=now();
    let snapshot=|observed:i64,available:bool|json!({"version":1,
        "observed_ms":observed,"expires_ms":observed+120000,
        "targets":if available {json!([{"id":"fixture","account_id":"account",
            "pool_ids":["pool"],"capabilities":["audit"],"health":"up","auth":"ok"}])}
            else {json!([])},
        "pools":[{"id":"pool","windows":[{"id":"week","unit":"points",
            "remaining_milli":1000000,"protected_milli":0,"reserved_milli":0,
            "confidence":"exact","expires_ms":observed+120000}]}]});
    let observe=|when:i64,available:bool|d.call("swarm.availability.observe",json!({
        "run_id":run,"snapshot":snapshot(when-1000,available),"now_ms":when,
        "required_capabilities":["audit"],"estimate_milli":{"points":100},
        "purpose":"worker"}));
    assert_eq!(observe(at,true)["state"],"eligible");
    let attempt=admit(&d,run,1,"j4","fixture",&snapshot(at-1000,true),at);
    assert_eq!(attempt["status"],"admitted","{attempt}");
    let launched=d.call("swarm.worker.launch",json!({"run_id":run,"job_id":"j4",
        "attempt_id":attempt["attempt_id"],"token":attempt["token"],
        "repo":checkout,"program":"/usr/bin/env",
        "args":["node",script,database_url_file,probe_marker],
        "prompt":"Audit Atlas attachment authorization","title":"Atlas J4 blocked deadline"}));
    let worker=launched["overseer_run_id"].as_str().unwrap();
    let until=std::time::Instant::now()+std::time::Duration::from_secs(6);
    while !probe_marker.exists() {
        assert!(std::time::Instant::now()<until,"Atlas backend probe did not start");
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let probe:Value=serde_json::from_slice(&std::fs::read(&probe_marker).unwrap()).unwrap();
    assert_eq!(probe["attachmentStatus"],200);
    let blocked=observe(now(),false);
    assert_eq!(blocked["state"],"blocked","{blocked}");
    assert_eq!(blocked["reason"],"allowed_target_missing");
    let held=admit(&d,run,1,"j2","fixture",&snapshot(at-1000,true),now());
    assert_eq!(held["status"],"blocked","{held}");
    assert_eq!(held["reason"],"run_availability_blocked");
    let db=rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let run_dir:String=db.query_row("SELECT run_dir FROM runs WHERE id=?1",[worker],|r|r.get(0)).unwrap();
    let interrupt=std::path::Path::new(&run_dir).join("interrupt.requested");
    let until=std::time::Instant::now()+std::time::Duration::from_secs(12);
    while !interrupt.exists() {
        assert!(std::time::Instant::now()<until,"blocked run was not interrupted at deadline");
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let state=d.call("swarm.get",json!({"id":run}));
    assert_eq!(state["stop_reason"],"deadline","{state}");
    assert!(state["status"]=="stopping" || state["status"]=="stopped","{state}");
    assert_eq!(state["availability"]["state"],"blocked");
    let messages=d.call("swarm.messages",json!({"run_id":run,
        "recipient":attempt["attempt_id"],"token":attempt["token"]}));
    let messages=messages["messages"].as_array().unwrap();
    assert!(messages.iter().any(|m|m["type"]=="checkpoint"
        && m["payload"]["reason"]=="run_deadline"));
    assert!(messages.iter().any(|m|m["type"]=="stop"));
    let jobs=d.call("swarm.jobs",json!({"id":run}));
    assert_eq!(jobs["jobs"].as_array().unwrap().iter().find(|j|j["id"]=="j2").unwrap()["status"],"cancelled");
    let counts:(i64,i64,i64)=db.query_row("SELECT
        (SELECT COUNT(*) FROM swarm_admissions WHERE run_id=?1 AND job_id='j2'),
        (SELECT COUNT(*) FROM swarm_messages WHERE run_id=?1 AND job_id='j4' AND kind='result'),
        (SELECT COUNT(*) FROM swarm_decisions WHERE run_id=?1 AND decision='accept')",
        [run],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap();
    assert_eq!(counts,(0,0,0));
    assert!(d.try_call("swarm.complete",json!({"run_id":run,"generation":1,
        "revision":1,"request_id":"atlas-blocked-deadline-complete",
        "summary":"Audit complete","verification":"Atlas attachment probe",
        "checks":[]})).is_err());
}

// S5: lowering a run's worker ceiling below its current activity drains the
// existing supervised workers and holds a third without losing their evidence.
#[test]
#[ignore = "requires Atlas PostgreSQL fixture, Node.js 24, and local socket permission"]
fn atlas_s5_lower_worker_ceiling_drains_existing_backend_probes() {
    assert!(std::env::var("ATLAS_DATABASE_URL").is_ok());
    let d=Daemon::start(&[]);
    let temp=tmp();
    let checkout=repo(&temp.path().join("atlas-lower-ceiling"));
    let j2_script=repo_root().join("fixtures/swarm/atlas-v1/swarm-j2-worker.mjs");
    let j4_script=repo_root().join("fixtures/swarm/atlas-v1/swarm-j4-long-worker.mjs");
    let database_url_file=temp.path().join("disposable-database-url");
    let release_j2=temp.path().join("release-j2");
    let probe_j4=temp.path().join("j4-probe-active");
    std::fs::write(&database_url_file,std::env::var("ATLAS_DATABASE_URL").unwrap()).unwrap();
    let created=d.call("swarm.create",json!({"category":"Atlas lower-ceiling fault",
        "objective":"Audit tasks, attachments and tokens","allowed_targets":["fixture"],
        "policy":{"max_workers":3}}));
    let run=created["id"].as_str().unwrap();
    d.call("swarm.plan",json!({"id":run,"generation":1,"revision":0,"jobs":[
        {"id":"j2","title":"Tasks","acceptance":"foreign task mutation",
            "deps":[],"resource_claims":[{"resource":"atlas-db-j2","mode":"write"}]},
        {"id":"j4","title":"Attachments","acceptance":"foreign object retrieval",
            "deps":[],"resource_claims":[{"resource":"atlas-db-j4","mode":"write"}]},
        {"id":"j6","title":"Tokens","acceptance":"revoked token response",
            "deps":[],"resource_claims":[{"resource":"atlas-db-j6","mode":"write"}]}
    ]}));
    commit_wave(&d,run,1,&["j2","j4","j6"]);
    let at=now();
    let snapshot=json!({"version":1,"observed_ms":at-1000,"expires_ms":at+120000,
        "targets":[{"id":"fixture","account_id":"account","pool_ids":["pool"],
            "capabilities":["audit"],"health":"up","auth":"ok"}],
        "pools":[{"id":"pool","windows":[{"id":"week","unit":"points",
            "remaining_milli":1000000,"protected_milli":0,"reserved_milli":0,
            "confidence":"exact","expires_ms":at+120000}]}]});
    let a2=admit(&d,run,1,"j2","fixture",&snapshot,at);
    let a4=admit(&d,run,1,"j4","fixture",&snapshot,at);
    assert_eq!(a2["status"],"admitted","{a2}");
    assert_eq!(a4["status"],"admitted","{a4}");
    let j2=d.call("swarm.worker.launch",json!({"run_id":run,"job_id":"j2",
        "attempt_id":a2["attempt_id"],"token":a2["token"],
        "repo":checkout,"program":"/usr/bin/env",
        "args":["node",j2_script,database_url_file,release_j2],
        "prompt":"Audit Atlas task mutation","title":"Atlas J2 ceiling drain"}));
    let j4=d.call("swarm.worker.launch",json!({"run_id":run,"job_id":"j4",
        "attempt_id":a4["attempt_id"],"token":a4["token"],
        "repo":checkout,"program":"/usr/bin/env",
        "args":["node",j4_script,database_url_file,probe_j4],
        "prompt":"Audit Atlas attachments","title":"Atlas J4 ceiling drain"}));
    let db=rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let until=std::time::Instant::now()+std::time::Duration::from_secs(8);
    loop {
        let result_count:i64=db.query_row("SELECT COUNT(*) FROM swarm_messages
            WHERE run_id=?1 AND job_id='j2' AND kind='result'",[run],|r|r.get(0)).unwrap();
        if result_count==1 && probe_j4.exists() { break; }
        assert!(std::time::Instant::now()<until,"two Atlas backend probes did not become active");
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let probe:Value=serde_json::from_slice(&std::fs::read(&probe_j4).unwrap()).unwrap();
    assert_eq!(probe["attachmentStatus"],200);
    let active:i64=db.query_row("SELECT COUNT(*) FROM swarm_worker_launches l
        JOIN runs r ON r.id=l.overseer_run_id
        WHERE l.run_id=?1 AND r.status IN ('starting','running','waiting_for_user')",
        [run],|r|r.get(0)).unwrap();
    assert_eq!(active,2);
    let lowered=d.call("swarm.limit.set",json!({"run_id":run,
        "request_id":"atlas-lower-to-one","expected_limit_revision":0,"max_workers":1}));
    assert_eq!(lowered["limit_revision"],1);
    assert_eq!(lowered["previous_max_workers"],3);
    assert_eq!(admit(&d,run,1,"j6","fixture",&snapshot,now())["reason"],"worker_limit");
    let worker_ids=[j2["overseer_run_id"].as_str().unwrap(),j4["overseer_run_id"].as_str().unwrap()];
    for worker in worker_ids {
        let run_dir:String=db.query_row("SELECT run_dir FROM runs WHERE id=?1",[worker],|r|r.get(0)).unwrap();
        assert!(!std::path::Path::new(&run_dir).join("interrupt.requested").exists());
    }
    std::fs::write(&release_j2,"continue").unwrap();
    let until=std::time::Instant::now()+std::time::Duration::from_secs(8);
    loop {
        let finished:i64=db.query_row("SELECT COUNT(*) FROM swarm_attempts
            WHERE id=?1 AND status='finished'",[a2["attempt_id"].as_str().unwrap()],|r|r.get(0)).unwrap();
        if finished==1 { break; }
        assert!(std::time::Instant::now()<until,"J2 did not drain after its release");
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert_eq!(admit(&d,run,1,"j6","fixture",&snapshot,now())["reason"],"worker_limit");
    let until=std::time::Instant::now()+std::time::Duration::from_secs(18);
    loop {
        let finished:i64=db.query_row("SELECT COUNT(*) FROM swarm_attempts
            WHERE id=?1 AND status='finished'",[a4["attempt_id"].as_str().unwrap()],|r|r.get(0)).unwrap();
        if finished==1 { break; }
        assert!(std::time::Instant::now()<until,"J4 did not finish its held backend probe");
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let resumed=admit(&d,run,1,"j6","fixture",&snapshot,now());
    assert_eq!(resumed["status"],"admitted","{resumed}");
    let counts:(i64,i64,i64)=db.query_row("SELECT
        (SELECT COUNT(*) FROM swarm_artifacts WHERE run_id=?1 AND job_id='j2'),
        (SELECT COUNT(*) FROM swarm_artifacts WHERE run_id=?1 AND job_id='j4'),
        (SELECT COUNT(*) FROM swarm_admissions WHERE run_id=?1 AND job_id='j6')",
        [run],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap();
    assert_eq!(counts,(1,1,1));
}
