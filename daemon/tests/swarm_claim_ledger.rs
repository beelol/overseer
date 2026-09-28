//! One daemon claim ledger for Swarm jobs and ordinary agents (SWARM-44; S5's
//! "Ordinary agent and J2 claim the same exclusive path"; Gate S's "Areas and
//! conflicts" rule). An ordinary agent's area (Gate S's `claim`, or an area the
//! owner sets) and a Swarm job's exclusive write claim can never both own one
//! path: whichever is first owns it and the other is refused. Independent
//! read-only analysis is allowed. The refusal is recorded once and both sides'
//! coordinators see it: the director in its durable inbox (it alone decides
//! for its job), Overseer in its conversation. Nobody is reassigned.
//!
//! Scripted fixtures: a fixture swarm (no director process, no provider
//! account) and a generic ordinary agent that sleeps; its claims go through
//! the agent channel tool with its own token.

mod common;
use common::*;
use serde_json::{json, Value};
use std::path::Path;
use std::time::Duration;

fn db(d: &Daemon) -> rusqlite::Connection {
    let conn = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    conn.busy_timeout(Duration::from_secs(10)).unwrap();
    conn
}

fn swarm(d: &Daemon, category: &str, repo: &Path, jobs: Value) -> String {
    let created = d.call("swarm.create", json!({"category":category,
        "objective":"Audit Atlas tenant isolation; report bugs","allowed_targets":["fixture"],
        "repositories":[repo],"policy":{"max_workers":4}}));
    let run = created["id"].as_str().unwrap().to_string();
    d.call("swarm.plan", json!({"id":run,"generation":1,"revision":0,"jobs":jobs}));
    run
}

fn job(id: &str, claims: Value) -> Value {
    json!({"id":id,"title":format!("Job {id}"),"acceptance":"evidence","deps":[],"resource_claims":claims})
}

/// A live ordinary agent in `repo`, with its channel on and a token of its own.
fn agent(d: &Daemon, repo: &Path) -> (String, String) {
    let created = d.generic(repo, "worktree", "/bin/sh", &["-c", "sleep 120"]);
    let run = run_id(&created);
    d.wait_status(&run, |s| s == "running", 20);
    d.call("agent.channel", json!({"run_id":run,"briefing":true,"channel":true,"by":"owner"}));
    let token = d.call("overseer.token", json!({"run_id":run,"role":"agent"}))["token"].as_str().unwrap().to_string();
    (run, token)
}

fn agent_claim(d: &Daemon, token: &str, paths: &[&str]) -> Value {
    d.call("overseer.tool", json!({"token":token,"name":"claim","arguments":{"paths":paths}}))
}

fn swarm_claim(d: &Daemon, run: &str, job: &str, resource: &str, mode: &str) -> Result<Value, String> {
    d.try_call("swarm.claim", json!({"run_id":run,"job_id":job,"resource":resource,"mode":mode,
        "generation":1,"revision":1}))
}

fn areas(d: &Daemon, run: &str) -> Vec<String> {
    let db = db(d);
    let mut stmt = db.prepare("SELECT path FROM areas WHERE run_id=?1 ORDER BY path").unwrap();
    let rows = stmt.query_map([run], |r| r.get(0)).unwrap().map(Result::unwrap).collect();
    rows
}

fn owners(ledger: &Value, resource: &str) -> Vec<Value> {
    ledger["claims"].as_array().unwrap().iter()
        .filter(|c| c["mode"] == "write" && (c["resource"] == resource
            || resource.starts_with(&format!("{}/", c["resource"].as_str().unwrap()))
            || c["resource"].as_str().unwrap().starts_with(&format!("{resource}/"))))
        .cloned().collect()
}

fn director_inbox(d: &Daemon, run: &str) -> Vec<Value> {
    d.call("swarm.messages", json!({"run_id":run,"recipient":"director","limit":100}))["messages"]
        .as_array().unwrap().clone()
}

#[test]
fn an_ordinary_agent_and_a_swarm_job_never_both_own_one_path() {
    let temp = tmp();
    let atlas = repo(&temp.path().join("atlas"));
    let other = repo(&temp.path().join("other"));
    let d = Daemon::start(&[]);
    let run = swarm(&d, "Backend security", &atlas, json!([
        job("j1", json!([])),
        job("j2", json!([])),
        job("j3", json!([{"resource":"web/page.ts","mode":"write"}])),
    ]));
    let docs = swarm(&d, "Docs", &atlas, json!([job("k1", json!([]))]));
    let (ordinary, token) = agent(&d, &atlas);

    // J2 first: its exclusive claim owns routes/tasks.ts. The ordinary agent's
    // claim of the directory holding it is refused, names the Swarm job and its
    // director, and writes no area. Another category's swarm is refused too.
    assert_eq!(swarm_claim(&d, &run, "j2", "routes/tasks.ts", "write").unwrap()["mode"], "write");
    let refused = agent_claim(&d, &token, &["routes"]);
    assert_eq!(refused["is_error"], true, "{refused}");
    let text = refused["text"].as_str().unwrap();
    assert!(text.contains("j2") && text.contains("director"), "{text}");
    assert!(areas(&d, &ordinary).is_empty(), "a refused claim leaves no area");
    assert!(swarm_claim(&d, &docs, "k1", "routes/tasks.ts", "write").unwrap_err().contains("claim conflict"));
    // The owner's own area for the agent is refused the same way: one ledger.
    let owner_set = d.try_call("agent.area", json!({"run_id":ordinary,"paths":["routes/tasks.ts"],"by":"owner"}));
    assert!(owner_set.unwrap_err().contains("j2"));
    assert!(areas(&d, &ordinary).is_empty());

    // The agent first: its area owns web/. J2's exclusive claim inside it is
    // refused and names the agent; J3's planned write claim blocks its
    // admission; J1 may still read there (independent read-only analysis of
    // its pinned revision is not ownership).
    let claimed = agent_claim(&d, &token, &["web"]);
    assert_eq!(claimed["is_error"], false, "{claimed}");
    assert_eq!(areas(&d, &ordinary), ["web"]);
    let err = swarm_claim(&d, &run, "j2", "web/login.ts", "write").unwrap_err();
    assert!(err.contains("claim conflict") && err.contains(&ordinary), "{err}");
    let at = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64;
    let admitted = d.call("swarm.admit", json!({"run_id":run,"generation":1,"revision":1,"job_id":"j3",
        "target_id":"fixture","request_id":"ledger-j3","now_ms":at,"required_capabilities":[],
        "estimate_milli":{"points":100},"purpose":"worker","snapshot":{"version":1,"observed_ms":at-1000,
        "expires_ms":at+120000,"targets":[{"id":"fixture","account_id":"account-a","pool_ids":["pool-a"],
            "capabilities":[],"health":"up","auth":"ok"}],
        "pools":[{"id":"pool-a","windows":[{"id":"week","unit":"points","remaining_milli":1000000,
            "protected_milli":0,"reserved_milli":0,"confidence":"exact","expires_ms":at+120000}]}]}}));
    assert_eq!((admitted["status"].as_str(), admitted["reason"].as_str()), (Some("blocked"), Some("resource_conflict")), "{admitted}");
    assert_eq!(swarm_claim(&d, &run, "j1", "web/login.ts", "read").unwrap()["mode"], "read");

    // One ledger, one owner per path.
    let ledger = d.call("claims.ledger", json!({}));
    let tasks = owners(&ledger, "routes/tasks.ts");
    assert_eq!(tasks.len(), 1, "{ledger}");
    assert_eq!((tasks[0]["holder"].as_str(), tasks[0]["job_id"].as_str()), (Some("swarm"), Some("j2")));
    let web = owners(&ledger, "web/login.ts");
    assert_eq!(web.len(), 1, "{ledger}");
    assert_eq!((web[0]["holder"].as_str(), web[0]["run_id"].as_str()), (Some("agent"), Some(ordinary.as_str())));

    // Both sides see each refusal, once. The director hears it in its durable
    // inbox as a routine event that names no attempt and assigns nothing; the
    // ordinary agent's side is Overseer's conversation and the agent's events.
    let refusals = ledger["refusals"].as_array().unwrap();
    assert!(refusals.iter().any(|r| r["claimant"]["run_id"] == ordinary && r["holder"]["job_id"] == "j2"), "{ledger}");
    assert!(refusals.iter().any(|r| r["claimant"]["job_id"] == "j2" && r["holder"]["run_id"] == ordinary), "{ledger}");
    let inbox = director_inbox(&d, &run);
    let told: Vec<&Value> = inbox.iter().filter(|m| m["sender"] == "ledger").collect();
    assert_eq!(told.len(), 3, "the agent's claim, the owner's area and J2's claim: {inbox:?}");
    for m in &told {
        assert_eq!((m["type"].as_str(), m["attempt_id"].as_str()), (Some("claim"), None), "{m}");
        assert_eq!(m["payload"]["agent_run_id"], ordinary.as_str(), "{m}");
    }
    let before = d.call("swarm.jobs", json!({"id":run}));
    let again = agent_claim(&d, &token, &["routes"]);
    assert_eq!(again["is_error"], true);
    assert_eq!(director_inbox(&d, &run).iter().filter(|m| m["sender"] == "ledger").count(), 3,
        "a repeated refusal is one event");
    assert_eq!(d.call("swarm.jobs", json!({"id":run})), before, "a refusal assigns nothing");
    let session = d.call("overseer.session", json!({}));
    let cards: Vec<&Value> = session["messages"].as_array().unwrap().iter()
        .filter(|m| m["card"]["kind"] == "claim_refused").collect();
    assert_eq!(cards.len(), 3, "{cards:?}");
    assert_eq!(d.events(&ordinary).iter().filter(|e| e["kind"] == "claim_refused").count(), 3);

    // Another repository is another ledger scope: the swarm is scoped to Atlas.
    let (elsewhere, other_token) = agent(&d, &other);
    assert_eq!(agent_claim(&d, &other_token, &["routes"])["is_error"], false);
    assert_eq!(areas(&d, &elsewhere), ["routes"]);

    // When J2 lets go (the director revises its job), the path is free.
    d.call("swarm.revise", json!({"id":run,"generation":1,"expected_revision":1,
        "reason":"J2 narrows to the service layer","jobs":[job("j1", json!([])),
        json!({"id":"j2","title":"Job j2 (services)","acceptance":"evidence","deps":[],"resource_claims":[]}),
        job("j3", json!([{"resource":"web/page.ts","mode":"write"}]))]}));
    assert_eq!(agent_claim(&d, &token, &["routes"])["is_error"], false);
    assert_eq!(areas(&d, &ordinary), ["routes", "web"]);
}

/// Claims racing from both sides for the same paths: each path ends with
/// exactly one owner, and the loser is refused.
#[test]
fn racing_claims_from_an_agent_and_a_swarm_job_admit_one_owner() {
    let temp = tmp();
    let atlas = repo(&temp.path().join("atlas"));
    let d = Daemon::start(&[]);
    let run = swarm(&d, "Backend security", &atlas, json!([job("j2", json!([]))]));
    let (ordinary, token) = agent(&d, &atlas);
    let d = std::sync::Arc::new(d);
    for n in 0..12 {
        let path = format!("src/module{n}.ts");
        let (da, db_) = (d.clone(), d.clone());
        let (pa, pb, tok, r) = (path.clone(), path.clone(), token.clone(), run.clone());
        let a = std::thread::spawn(move || agent_claim(&da, &tok, &[pa.as_str()])["is_error"] == false);
        let b = std::thread::spawn(move || swarm_claim(&db_, &r, "j2", &pb, "write").is_ok());
        let (agent_won, swarm_won) = (a.join().unwrap(), b.join().unwrap());
        assert!(agent_won ^ swarm_won, "{path}: agent {agent_won}, swarm {swarm_won}");
        let ledger = d.call("claims.ledger", json!({}));
        let held = owners(&ledger, &path);
        assert_eq!(held.len(), 1, "{path}: {ledger}");
        assert_eq!(held[0]["holder"], if agent_won { "agent" } else { "swarm" });
    }
    let _ = ordinary;
}
