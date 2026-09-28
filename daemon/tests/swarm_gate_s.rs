//! Swarm with Gate S (Overseer) present: S5's Overseer faults. Overseer sees a
//! swarm as one agent, its director. A message, redirect or hold Overseer aims
//! at a Swarm worker is refused and offered as an advisory to the director,
//! which alone assigns that worker's job (SWARM-60, S5).
//!
//! The director and workers are the scripted S0 fixture launched through the
//! normal start (`fixtures/swarm/s0-start-v1`); no provider account is used.

mod common;
use common::*;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

struct World {
    d: Daemon,
    checkout: PathBuf,
    trace: PathBuf,
    gate: PathBuf,
    _temp: tempfile::TempDir,
}

fn world() -> World {
    let temp = tmp();
    let checkout = repo(&temp.path().join("atlas"));
    let trace = temp.path().join("director-trace.jsonl");
    let gate = temp.path().join("gate");
    let fixture = repo_root().join("fixtures/swarm/s0-start-v1");
    let config = json!({"program":"/usr/bin/python3","args":[fixture.join("director.py"),trace,gate]})
        .to_string();
    let d = Daemon::start(&[("OVERSEER_SWARM_FIXTURE_DIRECTOR", config.as_str())]);
    World { d, checkout, trace, gate, _temp: temp }
}

fn db(d: &Daemon) -> rusqlite::Connection {
    let conn = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    conn.busy_timeout(Duration::from_secs(10)).unwrap();
    conn
}

fn wait_trace(path: &Path, step: &str, secs: u64) {
    let deadline = Instant::now() + Duration::from_secs(secs);
    loop {
        let text = std::fs::read_to_string(path).unwrap_or_default();
        if text.lines().any(|line| serde_json::from_str::<Value>(line).unwrap()["step"] == step) {
            return;
        }
        assert!(Instant::now() < deadline, "director never reached {step}: {text}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Start the S0 swarm through the normal start and wait until its three
/// workers run. Returns (swarm run, director's run, one worker's run).
fn running_swarm(w: &World) -> (String, String, String) {
    w.d.call("agents.limit.set", json!({"max_active":5}));
    w.d.call("swarm.policy.set", json!({"scope":"application","allowed_targets":["fixture-local"]}));
    let params = json!({"category":"Backend security",
        "objective":"Audit Atlas tenant isolation; report bugs, don't change application code",
        "repositories":[w.checkout]});
    let back = w.d.call("swarm.start", params.clone());
    let mut confirm = params;
    confirm["request_id"] = json!("gate-s-start");
    confirm["confirm_readback_sha256"] = back["readback_sha256"].clone();
    let started = w.d.call("swarm.start", confirm);
    assert_eq!(started["status"], "started", "{started}");
    let run = started["run"]["id"].as_str().unwrap().to_string();
    let director = started["director"]["overseer_run_id"].as_str().unwrap().to_string();
    wait_trace(&w.trace, "dispatched", 30);
    let worker: String = db(&w.d).query_row("SELECT overseer_run_id FROM swarm_worker_launches
        WHERE run_id=?1 AND job_id='tasks'", [&run], |r| r.get(0)).unwrap();
    (run, director, worker)
}

/// The owner is the one who asked (a Confirm or owner-asked action is allowed).
fn owner_asked(d: &Daemon) {
    d.call("overseer.session", json!({}));
    db(d).execute("UPDATE overseer_sessions SET last_cause='owner'", []).unwrap();
}

/// S5 "Overseer tries to redirect or hold J2 directly": each action that
/// would steer a Swarm worker (message, redirect, hold, release, stop,
/// guardrail, area, report, cadence, and a watch that would hold it) is
/// refused before any proposal, at every level, and the refusal names the
/// director to send it to. Nothing reaches the worker: no queued message,
/// hold, guardrail or area row, and it keeps running. Pinning it, and a
/// read-only watch, are not steering. The same words to the director go
/// out as Overseer's message with their source (the proposal and who said
/// yes) recorded.
#[test]
fn overseer_actions_aimed_at_a_swarm_worker_are_refused_and_offered_to_the_director() {
    let w = world();
    let (run, director, worker) = running_swarm(&w);
    owner_asked(&w.d);
    let director_title: String = db(&w.d).query_row("SELECT title FROM runs WHERE id=?1", [&director],
        |r| r.get(0)).unwrap();
    for level in ["ask_first", "steer", "auto"] {
        w.d.call("overseer.level", json!({"level":level}));
        for action in [
            json!({"action":"message","agent":worker,"text":"also check DELETE /tasks"}),
            json!({"action":"redirect","agent":worker,"text":"switch to attachments"}),
            json!({"action":"hold","agent":worker,"reason":"wait for the attachment trace"}),
            json!({"action":"release","agent":worker}),
            json!({"action":"stop","agent":worker}),
            json!({"action":"guardrail","agent":worker,"words":"stay in routes/","allow":["routes"]}),
            json!({"action":"area","agent":worker,"paths":["routes"]}),
            json!({"action":"report","agent":worker}),
            json!({"action":"cadence","agent":worker,"cadence":"5m"}),
            json!({"action":"watch","agent":worker,"brief":"watch it","hold_on_stop":true}),
        ] {
            let err = w.d.try_call("overseer.propose", json!({"actions":[action],"source":"test"}))
                .unwrap_err();
            assert!(err.contains("Swarm worker"), "{level} {action}: {err}");
            assert!(err.contains(&director) && err.contains(&director_title),
                "the refusal offers the director as the destination: {err}");
        }
    }
    let db = db(&w.d);
    for (table, column) in [("queued_messages", "run_id"), ("holds", "run_id"), ("areas", "run_id"),
        ("guardrails", "run_id")] {
        let n: i64 = db.query_row(&format!("SELECT COUNT(*) FROM {table} WHERE {column}=?1"), [&worker],
            |r| r.get(0)).unwrap();
        assert_eq!(n, 0, "nothing reached the worker through {table}");
    }
    let proposals: i64 = db.query_row("SELECT COUNT(*) FROM overseer_proposals", [], |r| r.get(0)).unwrap();
    assert_eq!(proposals, 0, "a refused action makes no proposal");
    assert_eq!(w.d.run(&worker)["status"], "running");
    // Looking is not steering.
    w.d.call("overseer.level", json!({"level":"ask_first"}));
    let pin = w.d.call("overseer.propose", json!({"actions":[{"action":"pin","agent":worker}],"source":"test"}));
    assert_eq!(pin["state"], "open", "{pin}");

    // The advisory goes to the director, with its source: an envelope in the
    // swarm's durable director inbox from `overseer`, naming the proposal and
    // who said yes, and no job or attempt. It is not an ordinary follow-up and
    // changes no assignment. Answering the same proposal again adds nothing.
    let jobs_before = w.d.call("swarm.jobs", json!({"id":run}));
    let revision_before = w.d.call("swarm.get", json!({"id":run}))["revision"].clone();
    let proposed = w.d.call("overseer.propose", json!({"actions":[{"action":"message","agent":director,
        "text":"Advisory about the tasks worker: also check DELETE /tasks"}],"source":"test"}));
    let answered = w.d.call("overseer.answer", json!({"id":proposed["proposal"],"yes":true,
        "surface":"ctl","by":"owner"}));
    assert_eq!(answered["state"], "yes", "{answered}");
    assert!(answered["result"].as_str().unwrap().contains("as an advisory"), "{answered}");
    assert!(w.d.try_call("overseer.answer", json!({"id":proposed["proposal"],"yes":true,
        "surface":"ctl","by":"owner"})).unwrap_err().contains("already_answered"));
    let rows: Vec<(String, Option<String>, Option<String>, String, String)> = {
        let mut stmt = db.prepare("SELECT sender,job_id,attempt_id,kind,payload FROM swarm_messages
            WHERE run_id=?1 AND sender='overseer'").unwrap();
        let rows = stmt.query_map([&run], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))
            .unwrap().map(Result::unwrap).collect();
        rows
    };
    assert_eq!(rows.len(), 1, "{rows:?}");
    let (sender, job, attempt, kind, payload) = &rows[0];
    assert_eq!((sender.as_str(), job, attempt, kind.as_str()), ("overseer", &None, &None, "advisory"));
    let payload: Value = serde_json::from_str(payload).unwrap();
    assert_eq!((payload["source"].as_str(), payload["proposal"].as_str(), payload["approved_by"].as_str()),
        (Some("overseer"), proposed["proposal"].as_str(), Some("owner")), "{payload}");
    let queued: i64 = db.query_row("SELECT COUNT(*) FROM queued_messages WHERE run_id=?1", [&director],
        |r| r.get(0)).unwrap();
    assert_eq!(queued, 0, "not an ordinary follow-up");
    assert_eq!(w.d.call("swarm.jobs", json!({"id":run})), jobs_before, "the advisory changes no job or assignment");
    assert_eq!(w.d.call("swarm.get", json!({"id":run}))["revision"], revision_before);
    let advisory_events = w.d.events(&director).iter().filter(|e| e["kind"] == "swarm_advisory").count();
    assert_eq!(advisory_events, 1);
    std::fs::write(&w.gate, "open").unwrap();
    let finished = w.d.wait_done(&director, 60);
    assert_eq!(finished["status"], "completed", "{finished}");
    assert_eq!(w.d.call("swarm.get", json!({"id":run}))["status"], "completed");
}

/// SWARM-20 and S5's "Overseer asks to raise this one's active limit while on
/// Auto": Overseer controls a swarm only through the swarm's own controls,
/// with the same daemon methods as the owner. At the Auto level a pause that
/// Overseer starts by itself happens at once; resume, a raised worker limit and
/// changed requirements are refused unless the owner asked, and when the owner
/// asks they wait for the owner's yes. A lowered limit and Stop are Steer.
/// Starting a swarm is not an Overseer action. Each carried-out action is
/// recorded with its proposal and who said yes.
#[test]
fn overseer_controls_a_swarm_only_through_its_controls_and_confirms_what_commits_more() {
    let w = world();
    let (run, director, _worker) = running_swarm(&w);
    w.d.call("overseer.session", json!({}));
    w.d.call("overseer.level", json!({"level":"auto"}));
    let cause = |who: &str| db(&w.d).execute("UPDATE overseer_sessions SET last_cause=?1", [who]).unwrap();
    let propose = |action: Value| w.d.try_call("overseer.propose", json!({"actions":[action],"source":"test"}));
    let status = || w.d.call("swarm.get", json!({"id":run}))["status"].as_str().unwrap().to_string();
    let ceiling = w.d.call("swarm.get", json!({"id":run}))["policy"]["effective"]["max_workers"].as_i64().unwrap();
    // Overseer by itself, at Auto: pausing reduces work and happens at once.
    cause("check_in");
    let paused = propose(json!({"action":"swarm","swarm":run,"op":"pause"})).unwrap();
    assert_eq!(paused["done"], true, "{paused}");
    assert_eq!(status(), "paused");
    // What commits more is refused unless the owner asked.
    for action in [json!({"action":"swarm","swarm":run,"op":"resume"}),
        json!({"action":"swarm","swarm":run,"op":"limit","max_workers":ceiling + 1}),
        json!({"action":"swarm","swarm":run,"op":"requirements","text":"also audit exports"})] {
        let err = propose(action.clone()).unwrap_err();
        assert!(err.contains("only when the owner asks"), "{action}: {err}");
    }
    assert!(propose(json!({"action":"swarm","swarm":run,"op":"start"})).unwrap_err().contains("does not start a swarm"));
    assert_eq!(status(), "paused");
    // The owner asks: still a read-back that waits for the owner's yes, even at Auto.
    cause("owner");
    let resume = propose(json!({"action":"swarm","swarm":run,"op":"resume"})).unwrap();
    assert_eq!((resume["done"].as_bool(), resume["state"].as_str()), (Some(false), Some("open")), "{resume}");
    assert_eq!(status(), "paused", "nothing happens before the yes");
    let yes = w.d.call("overseer.answer", json!({"id":resume["proposal"],"yes":true,"surface":"ctl","by":"owner"}));
    assert_eq!(yes["state"], "yes", "{yes}");
    assert_eq!(status(), "running");
    let before = w.d.call("swarm.get", json!({"id":run}))["policy"]["effective"]["max_workers"].as_i64().unwrap();
    let raise = propose(json!({"action":"swarm","swarm":run,"op":"limit","max_workers":before + 2})).unwrap();
    assert_eq!(raise["state"], "open", "raising the limit waits for a yes: {raise}");
    assert_eq!(w.d.call("swarm.get", json!({"id":run}))["policy"]["effective"]["max_workers"], before);
    w.d.call("overseer.answer", json!({"id":raise["proposal"],"yes":false,"surface":"ctl","by":"owner"}));
    assert_eq!(w.d.call("swarm.get", json!({"id":run}))["policy"]["effective"]["max_workers"], before, "a no changes nothing");
    // Lowering the limit reduces work: Steer.
    cause("check_in");
    let lower = propose(json!({"action":"swarm","swarm":run,"op":"limit","max_workers":2})).unwrap();
    assert_eq!(lower["done"], true, "{lower}");
    assert_eq!(w.d.call("swarm.get", json!({"id":run}))["policy"]["effective"]["max_workers"], 2);
    // The owner's changed requirements through Overseer: recorded after the yes.
    cause("owner");
    let change = propose(json!({"action":"swarm","swarm":run,"op":"requirements","text":"Also check the exports route"})).unwrap();
    let yes = w.d.call("overseer.answer", json!({"id":change["proposal"],"yes":true,"surface":"ctl","by":"owner"}));
    assert_eq!(yes["state"], "yes", "{yes}");
    let recorded = w.d.call("swarm.get", json!({"id":run}))["requirement_changes"][0].clone();
    assert_eq!(recorded["request_id"], format!("overseer-{}", change["proposal"].as_str().unwrap()), "{recorded}");
    assert_eq!(w.d.call("swarm.get", json!({"id":run}))["generation"], 1, "the director generation is unchanged");
    // Stop through Overseer uses the owner's Stop: the director is interrupted.
    cause("check_in");
    let stop = propose(json!({"action":"swarm","swarm":run,"op":"stop"})).unwrap();
    assert_eq!(stop["done"], true, "{stop}");
    assert!(matches!(status().as_str(), "stopping" | "stopped"));
    assert_ne!(w.d.wait_done(&director, 30)["status"], "running");
    // Every carried-out swarm action is recorded with its proposal.
    let recorded: i64 = db(&w.d).query_row("SELECT COUNT(*) FROM dispatches WHERE action='swarm' AND run_id=?1",
        [&run], |r| r.get(0)).unwrap();
    assert_eq!(recorded, 5, "pause, resume, lower limit, requirements, stop");
    std::fs::write(&w.gate, "open").unwrap();
}
