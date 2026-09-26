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
        "recipient":a4["attempt_id"]}));
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
        "generation":1,"revision":1,"now_ms":at+6000}));
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
