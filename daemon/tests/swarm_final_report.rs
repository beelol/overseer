//! S1's expected artifact: one final report with an entry per confirmed
//! finding (endpoint and method, caller permission, object ownership, source
//! revision, reproduction command, expected denial, observed response and DB
//! effect, root cause, fix direction) and a coverage matrix that keeps
//! confirmed findings, checked paths and unresolved areas apart. The seeded
//! Atlas variant (`fixtures/swarm/atlas-v1/manifest.json`) has two confirmed
//! findings and four protected paths; the missing-queue variant leaves
//! exports blocked, never passed.
//!
//! Scripted director and workers; the evidence text is the S1 specification's
//! expected artifact, not a live backend run (the joined PostgreSQL replay is
//! `swarm_atlas.rs`). No provider account is used.

mod common;
use common::*;
use serde_json::{json, Value};

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64
}

fn snapshot(at: i64) -> Value {
    json!({"version":1,"observed_ms":at-1000,"expires_ms":at+600000,
        "targets":[{"id":"fixture","account_id":"account-a","pool_ids":["pool-a"],
            "capabilities":["audit"],"health":"up","auth":"ok"}],
        "pools":[{"id":"pool-a","windows":[{"id":"week","unit":"points","remaining_milli":10000000,
            "protected_milli":0,"reserved_milli":0,"confidence":"exact","expires_ms":at+600000}]}]})
}

struct S1 {
    d: Daemon,
    run: String,
}

impl S1 {
    fn revision(&self) -> i64 {
        self.d.call("swarm.get", json!({"id":self.run}))["revision"].as_i64().unwrap()
    }

    fn benefit(&self, jobs: &[&str]) {
        let workers: Vec<Value> = jobs.iter().map(|id| json!({"id":id,"elapsed_ms":100,"usage_milli":{"points":10}})).collect();
        let serial = json!({"planning":{"elapsed_ms":10,"usage_milli":{"points":1}},
            "context":{"elapsed_ms":10,"usage_milli":{"points":1}},
            "integration":{"elapsed_ms":10,"usage_milli":{"points":1}},
            "review":{"elapsed_ms":10,"usage_milli":{"points":1}},
            "retries":{"elapsed_ms":0,"usage_milli":{"points":1}},"workers":workers});
        let mut parallel = serial.clone();
        parallel["context"]["elapsed_ms"] = json!(20);
        let decision = self.d.call("swarm.benefit.commit", json!({"run_id":self.run,"generation":1,
            "revision":self.revision(),"estimate":{"independent":true,"max_workers":jobs.len(),
            "allocation_milli":{"points":1000000},"finishing_reserve_milli":{"points":20000},
            "serial":serial,"parallel":parallel}}));
        if jobs.len() > 1 {
            assert_eq!(decision["decision"], "parallel", "{decision}");
        }
    }

    fn admit(&self, job: &str, later_ms: i64) -> Value {
        let at = now() + later_ms;
        let admitted = self.d.call("swarm.admit", json!({"run_id":self.run,"generation":1,"revision":self.revision(),
            "job_id":job,"target_id":"fixture","request_id":format!("s1-{job}"),"snapshot":snapshot(at),"now_ms":at,
            "required_capabilities":["audit"],"estimate_milli":{"points":100},"purpose":"worker"}));
        assert_eq!(admitted["status"], "admitted", "{job}: {admitted}");
        admitted
    }

    /// The worker's evidence and result; `unavailable` makes it an environment failure.
    fn submit(&self, job: &str, attempt: &Value, kind: &str, outcome: &str, content: &str, unavailable: Option<&str>) -> String {
        let revision = self.d.call("swarm.jobs", json!({"id":self.run}))["jobs"].as_array().unwrap().iter()
            .find(|j| j["id"] == job).unwrap()["plan_revision"].as_i64().unwrap();
        let artifact = format!("{job}-evidence");
        self.d.call("swarm.artifact.put", json!({"run_id":self.run,"job_id":job,"attempt_id":attempt["attempt_id"],
            "token":attempt["token"],"artifact_id":artifact,"source_revision":revision,"kind":kind,"content":content}));
        let mut payload = json!({"audit_outcome":outcome,"artifact_ids":[artifact]});
        if let Some(resource) = unavailable {
            payload["unavailable_resource"] = json!(resource);
        }
        self.d.call("swarm.report", json!({"run_id":self.run,"job_id":job,"attempt_id":attempt["attempt_id"],
            "token":attempt["token"],"message_id":format!("{job}-result"),"type":"result","revision":revision,
            "payload":payload}));
        artifact
    }

    fn accept(&self, job: &str, attempt: &Value, artifact: &str) {
        let decided = self.d.call("swarm.decide", json!({"run_id":self.run,"generation":1,"revision":self.revision(),
            "job_id":job,"decision":"accept","evidence":[artifact]}));
        assert_eq!(decided["status"], "accepted", "{job}: {decided}");
        self.d.call("swarm.attempt.confirm_exit", json!({"run_id":self.run,"generation":1,"revision":self.revision(),
            "job_id":job,"attempt_id":attempt["attempt_id"]}));
    }
}

/// S1 through J7, with J5's queue present or missing.
fn s1(export_queue_missing: bool) -> S1 {
    let d = Daemon::start(&[]);
    let created = d.call("swarm.create", json!({"category":"Backend security",
        "objective":"Audit cross-workspace access and privilege changes. Reproduce findings locally. Do not change application code.",
        "allowed_targets":["fixture"],"policy":{"max_workers":6}}));
    let run = created["id"].as_str().unwrap().to_string();
    let jobs: Vec<Value> = [("j1","Projects"),("j2","Tasks"),("j3","Membership"),("j4","Attachments"),("j5","Exports"),("j6","API tokens")]
        .iter().map(|(id,title)| json!({"id":id,"title":title,"acceptance":"evidence","deps":[],
            "resource_claims":[{"resource":format!("atlas-db-{id}"),"mode":"write"}]})).collect();
    d.call("swarm.plan", json!({"id":run,"generation":1,"revision":0,"jobs":jobs}));
    let s = S1 { d, run };
    s.benefit(&["j1","j2","j3","j4","j5","j6"]);
    // The first wave is J1-J4; J5 and J6 wait for the next one (S1's plan).
    let attempts: Vec<(String, Value)> = ["j1","j2","j3","j4","j5","j6"].iter().enumerate()
        .map(|(n, j)| (j.to_string(), s.admit(j, if n < 4 { 0 } else { 6000 }))).collect();
    let a = |job: &str| attempts.iter().find(|(j, _)| j == job).unwrap().1.clone();
    let d1 = s.d.call("swarm.report", json!({"run_id":s.run,"job_id":"j2","attempt_id":a("j2")["attempt_id"],
        "token":a("j2")["token"],"message_id":"D1","type":"discovery","revision":1,
        "payload":{"symbol":"TaskRepository.findById","source_revision":"atlas-v1"}}));
    assert_eq!(d1["duplicate"], false);
    for (job, evidence) in [("j1","Alice->A 200, Alice->B 403: requireProjectMember follows findProject"),
        ("j3","member PATCH /workspaces/:id/members/:userId -> 403 same and foreign workspace"),
        ("j6","read-only token PATCH /tasks -> 403; removed member token -> 401")] {
        let artifact = s.submit(job, &a(job), "finding", "negative", evidence, None);
        s.accept(job, &a(job), &artifact);
    }
    if export_queue_missing {
        let artifact = s.submit("j5", &a("j5"), "finding", "environment_failure",
            "POST /exports own workspace -> 503: relation export_jobs does not exist", Some("export-queue"));
        let _ = artifact;
        s.d.call("swarm.attempt.confirm_exit", json!({"run_id":s.run,"generation":1,"revision":s.revision(),
            "job_id":"j5","attempt_id":a("j5")["attempt_id"]}));
    } else {
        let artifact = s.submit("j5", &a("j5"), "finding", "negative", "queued payload bound to workspace A; worker query scoped", None);
        s.accept("j5", &a("j5"), &artifact);
    }
    let j2 = s.submit("j2", &a("j2"), "reproduction", "confirmed_defect",
        "alice-test PATCH /tasks/task-b-7 {\"title\":\"changed-by-alice\"} -> 200; tasks row task-b-7 title changed", None);
    let j4 = s.submit("j4", &a("j4"), "reproduction", "confirmed_defect",
        "alice-test GET /attachments/att-b-1/download -> 200 signed URL; emulator served workspace B object", None);
    let record = |finding: &str, title: &str, job: &str, artifact: &str, endpoint: &str, entry: Value| {
        let mut p = json!({"run_id":s.run,"generation":1,"revision":s.revision(),"finding_id":finding,"title":title,
            "root_cause":"D1","evidence":[{"job_id":job,"artifact_id":artifact,"endpoint":endpoint}]});
        if !entry.is_null() {
            p["entries"] = json!([entry]);
        }
        s.d.call("swarm.finding.record", p)
    };
    record("foreign-task-mutation", "Foreign task mutation", "j2", &j2, "PATCH /tasks/:id", json!({
        "endpoint":"PATCH /tasks/:id","caller_permission":"Alice, member of workspace A only (token alice-test)",
        "object_ownership":"task-b-7 belongs to Bob's project in workspace B","source_revision":"atlas-v1",
        "reproduction_command":"curl -X PATCH -H 'Authorization: Bearer alice-test' -d '{\"title\":\"changed-by-alice\"}' /tasks/task-b-7",
        "expected":"403 with the row unchanged","observed":"200 and tasks.task-b-7.title = changed-by-alice",
        "root_cause":"routes/tasks.ts -> services/tasks.ts -> repositories/tasks.ts::findById looks up by id with no ownership check",
        "fix_direction":"scope the task lookup to the caller's workspace membership before mutation"}));
    record("foreign-attachment-download", "Foreign attachment download", "j4", &j4, "GET /attachments/:id/download", json!({
        "endpoint":"GET /attachments/:id/download","caller_permission":"Alice, member of workspace A only",
        "object_ownership":"att-b-1 belongs to a task in workspace B","source_revision":"atlas-v1",
        "reproduction_command":"curl -H 'Authorization: Bearer alice-test' /attachments/att-b-1/download",
        "expected":"403 before any URL is signed","observed":"200 with a signed URL; the object was fetched"}));
    s.accept("j4", &a("j4"), &j4);
    // J7: J2's finding is confirmed by an independent reproduction only.
    let made = s.d.call("swarm.reproduce", json!({"run_id":s.run,"generation":1,"revision":s.revision(),
        "finding_id":"foreign-task-mutation","source_message_id":"D1","job":{"id":"j7","title":"Independent task reproduction",
            "acceptance":"repeat from a fresh fixture","resource_claims":[{"resource":"atlas-db-j7","mode":"write"}],
            "required_capabilities":["audit"]},"budget_milli":{"points":300}}));
    assert_eq!(made["job_id"], "j7");
    s.accept("j2", &a("j2"), &j2);
    s.benefit(&["j7"]);
    let a7 = s.admit("j7", 12000);
    let j7 = s.submit("j7", &a7, "reproduction", "confirmed_defect",
        "fresh schema: alice-test PATCH /tasks/task-b-7 -> 200; Bob's row changed", None);
    s.accept("j7", &a7, &j7);
    record("foreign-task-mutation", "Foreign task mutation", "j7", &j7, "PATCH /tasks/:id", Value::Null);
    s
}

#[test]
fn s1_final_report_has_one_entry_per_confirmed_finding_and_a_coverage_matrix() {
    let manifest: Value = serde_json::from_slice(&std::fs::read(repo_root().join("fixtures/swarm/atlas-v1/manifest.json")).unwrap()).unwrap();
    let s = s1(false);
    let report = s.d.call("swarm.report.final", json!({"run_id":s.run}));
    // One entry per confirmed finding, with every field S1 asks for.
    let entries = report["entries"].as_array().unwrap();
    let mut confirmed: Vec<&str> = entries.iter().map(|e| e["finding_id"].as_str().unwrap()).collect();
    confirmed.sort();
    let mut seeded: Vec<&str> = manifest["seeded_findings"].as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
    seeded.sort();
    assert_eq!(confirmed, seeded, "{report}");
    let task = entries.iter().find(|e| e["finding_id"] == "foreign-task-mutation").unwrap();
    for field in ["endpoint","caller_permission","object_ownership","source_revision","reproduction_command",
        "expected","observed","root_cause","fix_direction"] {
        assert!(task[field].as_str().is_some_and(|v| !v.is_empty()), "{field}: {task}");
    }
    assert_eq!(task["expected"], "403 with the row unchanged");
    assert_eq!(task["reproducer"]["job_id"], "j7");
    let evidence: Vec<&str> = task["evidence"].as_array().unwrap().iter().map(|e| e["job_id"].as_str().unwrap()).collect();
    assert_eq!(evidence, ["j2", "j7"]);
    // An entry missing fields is named as incomplete, not filled in.
    let attachment = entries.iter().find(|e| e["finding_id"] == "foreign-attachment-download").unwrap();
    assert!(attachment["root_cause"].is_null() && attachment["fix_direction"].is_null(), "{attachment}");
    let incomplete = report["incomplete_entries"].as_array().unwrap();
    assert_eq!(incomplete.len(), 1, "{report}");
    assert_eq!(incomplete[0]["missing"], json!(["root_cause","fix_direction"]));
    // The matrix keeps confirmed findings, checked paths and unresolved areas apart.
    let matrix = &report["coverage_matrix"];
    assert_eq!(matrix["confirmed_findings"].as_array().unwrap().len(), 2, "{matrix}");
    let mut checked: Vec<&str> = matrix["checked_paths"].as_array().unwrap().iter().map(|c| c["job_id"].as_str().unwrap()).collect();
    checked.sort();
    assert_eq!(checked, ["j1","j3","j5","j6"], "{matrix}");
    assert_eq!(checked.len(), manifest["protected_paths"].as_array().unwrap().len());
    assert_eq!(matrix["unresolved"], json!([]), "{matrix}");
    // One artifact: the same state gives the same report and digest.
    let again = s.d.call("swarm.report.final", json!({"run_id":s.run}));
    assert_eq!(again["sha256"], report["sha256"]);
    assert_eq!(report["sha256"].as_str().unwrap().len(), 64);
}

#[test]
fn s1_missing_export_queue_is_unresolved_never_a_checked_path() {
    let s = s1(true);
    let report = s.d.call("swarm.report.final", json!({"run_id":s.run}));
    let matrix = &report["coverage_matrix"];
    let checked: Vec<&str> = matrix["checked_paths"].as_array().unwrap().iter().map(|c| c["job_id"].as_str().unwrap()).collect();
    assert!(!checked.contains(&"j5"), "{matrix}");
    let unresolved = matrix["unresolved"].as_array().unwrap();
    let exports = unresolved.iter().find(|u| u["job_id"] == "j5").expect("exports unresolved");
    assert_eq!((exports["reason"].as_str(), exports["unavailable_resource"].as_str()),
        (Some("environment_blocked"), Some("export-queue")), "{exports}");
    assert_eq!(matrix["confirmed_findings"].as_array().unwrap().len(), 2);
    // A finding with no accepted reproduction is unresolved too, never an entry.
    s.d.call("swarm.finding.record", json!({"run_id":s.run,"generation":1,"revision":s.d.call("swarm.get",
        json!({"id":s.run}))["revision"],"finding_id":"export-binding","title":"Export binding (unverified)",
        "evidence":[{"job_id":"j5","artifact_id":"j5-evidence","endpoint":"POST /exports"}]}));
    let report = s.d.call("swarm.report.final", json!({"run_id":s.run}));
    assert!(!report["entries"].as_array().unwrap().iter().any(|e| e["finding_id"] == "export-binding"));
    assert!(report["coverage_matrix"]["unresolved"].as_array().unwrap().iter()
        .any(|u| u["finding_id"] == "export-binding" && u["reason"] == "unconfirmed_finding"), "{report}");
}
