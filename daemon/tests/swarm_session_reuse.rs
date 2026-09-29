//! SWARM-35's last clause: reuse a valid related worker session, and never
//! carry unrelated context into a new job. A worker launch may name an earlier
//! attempt whose native session it continues. The daemon allows it only when
//! that attempt is of the same swarm run, has exited, is related (an earlier
//! attempt of the same logical job, or of a job this one depends on), is not
//! stale (a dependency's session from before its contract changed), is not
//! contaminated or disputed, ran on the same admitted route (a session stays
//! on its account), and has a session to continue. Without reuse a job starts
//! a fresh session with only its own brief.
//!
//! Native Claude workers on the synthetic Claude fixture (echo mode reports
//! the arguments it was started with); the director is scripted. No provider
//! account is used.

mod common;
use common::*;
use serde_json::{json, Value};
use std::path::Path;

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64
}

fn daemon() -> Daemon {
    let fixture = repo_root().join("fixtures/fake-harness/claude-fixture.js").display().to_string();
    Daemon::start(&[("OVERSEER_SHARED_BOOKING_FIXTURE_API", "1"), ("OVERSEER_CLAUDE_PATH", fixture.as_str()),
        ("CLAUDE_FIXTURE_MODE", "echo"), ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "CLAUDE_FIXTURE_MODE")])
}

fn create(d: &Daemon, category: &str, jobs: Value) -> String {
    let run = d.call("swarm.create", json!({"category":category,"objective":"Migrate the catalog to cursor pagination",
        "allowed_targets":["fixture-claude"],"source_change_permission":"isolated"}));
    let id = run["id"].as_str().unwrap().to_string();
    d.call("swarm.plan", json!({"id":id,"generation":1,"revision":0,"jobs":jobs}));
    id
}

fn revision(d: &Daemon, run: &str) -> i64 {
    d.call("swarm.get", json!({"id":run}))["revision"].as_i64().unwrap()
}

fn admit(d: &Daemon, run: &str, job: &str, request: &str) -> Value {
    let at = now();
    let booking = fixture_account_booking(d, "system-claude", "fixture-claude-account", 0.0, 1000);
    let admitted = d.call("swarm.admit", json!({"shared_booking":booking,"run_id":run,"generation":1,
        "revision":revision(d, run),"job_id":job,"target_id":"fixture-claude","request_id":request,"now_ms":at,
        "snapshot":{"version":1,"observed_ms":at-1000,"expires_ms":at+60000,
            "targets":[{"id":"fixture-claude","harness":"claude","profile_id":"system-claude","model":"sonnet",
                "account_id":"fixture","pool_ids":["pool"],"capabilities":["code"],"health":"up","auth":"ok"}],
            "pools":[{"id":"pool","windows":[{"id":"run","unit":"points","remaining_milli":100000,
                "protected_milli":0,"reserved_milli":0,"confidence":"exact","expires_ms":at+60000}]}]},
        "required_capabilities":["code"],"estimate_milli":{"points":100},"purpose":"worker"}));
    assert_eq!(admitted["status"], "admitted", "{job}: {admitted}");
    admitted
}

fn launch(d: &Daemon, checkout: &Path, run: &str, job: &str, attempt: &Value, prompt: &str, reuse: Option<&Value>)
    -> Result<Value, String> {
    let mut p = json!({"run_id":run,"job_id":job,"attempt_id":attempt["attempt_id"],"token":attempt["token"],
        "repo":checkout,"harness":"claude","args":[],"prompt":prompt,"title":format!("worker {job}")});
    if let Some(prior) = reuse {
        p["reuse_session_of"] = prior["attempt_id"].clone();
    }
    d.try_call("swarm.worker.launch", p)
}

/// The arguments and prompt the fixture worker was started with.
fn echo(d: &Daemon, worker_run: &str) -> Value {
    d.wait_done(worker_run, 30);
    let text = d.events(worker_run).iter().find_map(|e| e["payload"]["text"].as_str()
        .filter(|t| t.starts_with("ECHO ")).map(|t| t[5..].to_string())).expect("echo output");
    serde_json::from_str(&text).unwrap()
}

fn finish(d: &Daemon, run: &str, job: &str, attempt: &Value, accept: bool) {
    let artifact = format!("{job}-{}", attempt["attempt_id"].as_str().unwrap());
    let revision_of_attempt = d.call("swarm.jobs", json!({"id":run}))["jobs"].as_array().unwrap().iter()
        .find(|j| j["id"] == job).unwrap()["plan_revision"].as_i64().unwrap();
    d.call("swarm.artifact.put", json!({"run_id":run,"job_id":job,"attempt_id":attempt["attempt_id"],
        "token":attempt["token"],"artifact_id":artifact,"source_revision":revision_of_attempt,"kind":"finding",
        "content":format!("{job} evidence")}));
    d.call("swarm.report", json!({"run_id":run,"job_id":job,"attempt_id":attempt["attempt_id"],
        "token":attempt["token"],"message_id":format!("{artifact}-result"),"type":"result",
        "revision":revision_of_attempt,"payload":{"artifact_ids":[artifact]}}));
    let decision = if accept { "accept" } else { "reject" };
    d.call("swarm.decide", json!({"run_id":run,"generation":1,"revision":revision(d, run),"job_id":job,
        "decision":decision,"evidence":[artifact]}));
    d.call("swarm.attempt.confirm_exit", json!({"run_id":run,"generation":1,"revision":revision(d, run),
        "job_id":job,"attempt_id":attempt["attempt_id"]}));
}

fn job(id: &str, deps: &[&str]) -> Value {
    json!({"id":id,"title":format!("Job {id}"),"acceptance":format!("{id} behaviour tests"),"deps":deps})
}

#[test]
fn a_related_session_is_reused_and_unrelated_context_never_is() {
    let temp = tmp();
    let checkout = repo(&temp.path().join("catalog"));
    let d = daemon();
    d.call("agents.limit.set", json!({"max_active":6}));
    let run = create(&d, "Catalog migration", json!([job("contract", &[]), job("module-a", &["contract"]),
        job("module-b", &["contract"]), job("docs", &[])]));
    let other = create(&d, "Docs", json!([job("k1", &[])]));
    commit_beneficial_batch(&d, &run, &["contract", "docs"].map(String::from));

    // The contract worker starts fresh: no session to continue.
    let c1 = admit(&d, &run, "contract", "contract-1");
    let launched = launch(&d, &checkout, &run, "contract", &c1, "Write the cursor contract (timestamp only)", None).unwrap();
    let c1_run = launched["overseer_run_id"].as_str().unwrap().to_string();
    let first = echo(&d, &c1_run);
    assert!(!first["argv"].as_array().unwrap().iter().any(|a| a == "--resume"), "{first}");
    // A still-running attempt cannot be continued by another.
    let docs = admit(&d, &run, "docs", "docs-1");
    // (the contract worker has finished its turn but not exited as an attempt yet)
    let err = launch(&d, &checkout, &run, "docs", &docs, "Document the API", Some(&c1)).unwrap_err();
    assert!(err.contains("has not exited"), "{err}");
    finish(&d, &run, "contract", &c1, true);
    commit_beneficial_batch(&d, &run, &["module-a", "module-b"].map(String::from));

    // Unrelated work never continues the contract worker's session: not a job
    // without the dependency, not another category's swarm.
    let err = launch(&d, &checkout, &run, "docs", &docs, "Document the API", Some(&c1)).unwrap_err();
    assert!(err.contains("related"), "{err}");
    let k1 = admit(&d, &other, "k1", "k1-1");
    let err = launch(&d, &checkout, &other, "k1", &k1, "Docs category work", Some(&c1)).unwrap_err();
    assert!(err.contains("another swarm"), "{err}");
    // A fresh launch carries only its own brief.
    let docs_run = launch(&d, &checkout, &run, "docs", &docs, "Document the API", None).unwrap()["overseer_run_id"]
        .as_str().unwrap().to_string();
    let fresh = echo(&d, &docs_run);
    assert!(!fresh["argv"].as_array().unwrap().iter().any(|a| a == "--resume"), "{fresh}");
    assert!(!fresh["text"].as_str().unwrap().contains("cursor contract"), "no unrelated context: {fresh}");

    // A dependent job continues the contract worker's session.
    let a1 = admit(&d, &run, "module-a", "module-a-1");
    let reused = launch(&d, &checkout, &run, "module-a", &a1, "Migrate module A to the contract", Some(&c1)).unwrap();
    assert_eq!(reused["reused_session"]["from_attempt"], c1["attempt_id"], "{reused}");
    let continued = echo(&d, reused["overseer_run_id"].as_str().unwrap());
    let argv: Vec<String> = continued["argv"].as_array().unwrap().iter().map(|a| a.as_str().unwrap().to_string()).collect();
    let at = argv.iter().position(|a| a == "--resume").expect("resumed");
    assert_eq!(argv[at + 1], "fixture-session-1", "{argv:?}");
    // One session continues in one place at a time.
    let b1 = admit(&d, &run, "module-b", "module-b-1");
    let err = launch(&d, &checkout, &run, "module-b", &b1, "Migrate module B", Some(&c1)).unwrap_err();
    assert!(err.contains("already continued"), "{err}");
    d.call("swarm.stop", json!({"run_id":other}));
    let _ = b1;
}

#[test]
fn a_session_from_before_a_contract_change_is_stale_for_dependents() {
    let temp = tmp();
    let checkout = repo(&temp.path().join("catalog"));
    let d = daemon();
    d.call("agents.limit.set", json!({"max_active":6}));
    let run = create(&d, "Catalog migration", json!([job("contract", &[]), job("module-a", &["contract"])]));
    let c1 = admit(&d, &run, "contract", "contract-1");
    let c1_run = launch(&d, &checkout, &run, "contract", &c1, "Write the cursor contract (timestamp only)", None)
        .unwrap()["overseer_run_id"].as_str().unwrap().to_string();
    echo(&d, &c1_run);
    finish(&d, &run, "contract", &c1, true);
    // Rows with tied timestamps are lost: the director changes the contract.
    d.call("swarm.revise", json!({"id":run,"generation":1,"expected_revision":1,
        "reason":"tie-breaker on the primary key","jobs":[
            {"id":"contract","title":"Job contract","acceptance":"cursor is (created_at, id); tie tests","deps":[]},
            job("module-a", &["contract"])]}));
    // The repair is attempt 2 of the same logical job, and may continue attempt 1's session.
    let c2 = admit(&d, &run, "contract", "contract-2");
    let repaired = launch(&d, &checkout, &run, "contract", &c2, "Add the primary-key tie-breaker", Some(&c1)).unwrap();
    assert_eq!(repaired["reused_session"]["from_attempt"], c1["attempt_id"], "{repaired}");
    echo(&d, repaired["overseer_run_id"].as_str().unwrap());
    finish(&d, &run, "contract", &c2, true);
    // A dependent may not continue the revision-1 session (it knows the old
    // contract), only the current one.
    let a1 = admit(&d, &run, "module-a", "module-a-1");
    let err = launch(&d, &checkout, &run, "module-a", &a1, "Migrate module A", Some(&c1)).unwrap_err();
    assert!(err.contains("stale"), "{err}");
    let ok = launch(&d, &checkout, &run, "module-a", &a1, "Migrate module A", Some(&c2)).unwrap();
    assert_eq!(ok["reused_session"]["from_attempt"], c2["attempt_id"], "{ok}");
}

#[test]
fn a_rejected_or_contaminated_session_is_not_reused_by_dependents() {
    let temp = tmp();
    let checkout = repo(&temp.path().join("catalog"));
    let d = daemon();
    d.call("agents.limit.set", json!({"max_active":6}));
    let run = create(&d, "Catalog migration", json!([job("contract", &[]), job("module-a", &["contract"])]));
    let c1 = admit(&d, &run, "contract", "contract-1");
    let c1_run = launch(&d, &checkout, &run, "contract", &c1, "Write the cursor contract", None)
        .unwrap()["overseer_run_id"].as_str().unwrap().to_string();
    echo(&d, &c1_run);
    finish(&d, &run, "contract", &c1, false);
    // The retry of the same job may continue the rejected attempt's session
    // (it carries the rejection), but a dependent never builds on it.
    let c2 = admit(&d, &run, "contract", "contract-2");
    let retry = launch(&d, &checkout, &run, "contract", &c2, "Write the cursor contract again", Some(&c1)).unwrap();
    assert_eq!(retry["reused_session"]["from_attempt"], c1["attempt_id"], "{retry}");
    echo(&d, retry["overseer_run_id"].as_str().unwrap());
    finish(&d, &run, "contract", &c2, true);
    let a1 = admit(&d, &run, "module-a", "module-a-1");
    let err = launch(&d, &checkout, &run, "module-a", &a1, "Migrate module A", Some(&c1)).unwrap_err();
    assert!(err.contains("rejected"), "{err}");
}
