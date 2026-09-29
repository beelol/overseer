//! Scenario S0: a normal "start a swarm" run. Category, objective and Start;
//! one compact read-back; the owner's yes commits the run and the daemon
//! launches the director through the one launch path. The director is the
//! scripted fixture in `fixtures/swarm/s0-start-v1` (no model harness is a
//! qualified director yet) and its workers are supervised fixture processes;
//! no provider account is used.

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

fn world(director: bool, extra: &[(&str, &str)]) -> World {
    let temp = tmp();
    let checkout = repo(&temp.path().join("atlas"));
    let trace = temp.path().join("director-trace.jsonl");
    let gate = temp.path().join("gate");
    let fixture = repo_root().join("fixtures/swarm/s0-start-v1");
    let config = json!({"program":"/usr/bin/python3","args":[fixture.join("director.py"),trace,gate]})
        .to_string();
    let mut env: Vec<(&str, &str)> = extra.to_vec();
    if director {
        env.push(("OVERSEER_SWARM_FIXTURE_DIRECTOR", config.as_str()));
    }
    let d = Daemon::start(&env);
    World { d, checkout, trace, gate, _temp: temp }
}

fn db(d: &Daemon) -> rusqlite::Connection {
    let conn = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    conn.busy_timeout(Duration::from_secs(10)).unwrap();
    conn
}

fn count(d: &Daemon, sql: &str, run: &str) -> i64 {
    db(d).query_row(sql, [run], |r| r.get(0)).unwrap()
}

fn trace(path: &Path) -> Vec<Value> {
    std::fs::read_to_string(path).unwrap_or_default().lines()
        .map(|line| serde_json::from_str(line).unwrap()).collect()
}

fn wait_trace(path: &Path, step: &str, secs: u64) -> Value {
    let deadline = Instant::now() + Duration::from_secs(secs);
    loop {
        if let Some(found) = trace(path).into_iter().find(|t| t["step"] == step) {
            return found;
        }
        assert!(Instant::now() < deadline, "director never reached {step}: {:?}", trace(path));
        std::thread::sleep(Duration::from_millis(50));
    }
}

const CATEGORY: &str = "Backend security";
const OBJECTIVE: &str = "Audit Atlas tenant isolation; report bugs, don't change application code";

fn start_params(w: &World) -> Value {
    json!({"category":CATEGORY,"objective":OBJECTIVE,"repositories":[w.checkout]})
}

/// Saved permissions allow two targets (the fixture target and a Claude
/// account profile); the app allows five agents, so four workers.
fn approve(w: &World) {
    w.d.call("agents.limit.set", json!({"max_active":5}));
    w.d.call("swarm.policy.set", json!({"scope":"application",
        "allowed_targets":["fixture-local","system-claude"]}));
}

/// Read back, confirm once, return (run id, director run id, digest).
fn start(w: &World) -> (String, String, String) {
    let back = w.d.call("swarm.start", start_params(w));
    let digest = back["readback_sha256"].as_str().unwrap().to_string();
    let mut confirm = start_params(w);
    confirm["request_id"] = json!("s0-start");
    confirm["confirm_readback_sha256"] = json!(digest);
    let started = w.d.call("swarm.start", confirm);
    assert_eq!(started["status"], "started", "{started}");
    assert_eq!(started["director"]["status"], "launched", "{started}");
    (started["run"]["id"].as_str().unwrap().to_string(),
        started["director"]["overseer_run_id"].as_str().unwrap().to_string(), digest)
}

fn wait_completed(w: &World, run: &str, director: &str) {
    let finished = w.d.wait_done(director, 60);
    assert_eq!(finished["status"], "completed", "{finished}; trace={:?}; output={}",
        trace(&w.trace), w.d.call("run.raw_output", json!({"run_id":director})));
    let deadline = Instant::now() + Duration::from_secs(10);
    while w.d.call("agents.limit.get", json!({}))["active"] != 0 {
        assert!(Instant::now() < deadline, "slots still held after the run completed");
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(w.d.call("swarm.get", json!({"id":run}))["status"], "completed");
}

/// The communication, decisions and completion every S0 run must show.
fn assert_s0_outcome(w: &World, run: &str) {
    let d = &w.d;
    assert_eq!(count(d, "SELECT COUNT(*) FROM swarm_worker_launches WHERE run_id=?1", run), 3);
    assert_eq!(count(d, "SELECT COUNT(*) FROM swarm_attempts WHERE run_id=?1", run), 3,
        "one attempt per job: the refused Claude offer admitted nothing");
    assert_eq!(count(d, "SELECT COUNT(*) FROM swarm_decisions WHERE run_id=?1", run), 3);
    assert_eq!(count(d, "SELECT COUNT(*) FROM swarm_completions WHERE run_id=?1", run), 1);
    assert_eq!(count(d, "SELECT COUNT(*) FROM swarm_director_owners WHERE run_id=?1", run), 1);
    let offers: Vec<Value> = trace(&w.trace).into_iter().filter(|t| t["step"] == "offered").collect();
    assert_eq!((offers[0]["target"].as_str(), offers[0]["status"].as_str(), offers[0]["reason"].as_str()),
        (Some("unapproved-local"), Some("blocked"), Some("not_allowed")),
        "a manual swarm stays within its approved pool: {offers:?}");
    let offered = &offers[1];
    assert_eq!(offered["target"], "system-claude");
    // A Claude worker may join an audit read-only (the owner's decision 3 of
    // 2026-09-28), so the audit boundary no longer refuses it; this account
    // has no qualified draw, so the booking refuses it before any effect.
    assert_eq!((offered["status"].as_str(), offered["reason"].as_str()),
        (Some("blocked"), Some("upper_draw_unknown")),
        "an unpriced account is refused before any booking: {offered}");
    let booked: i64 = db(d).query_row("SELECT COUNT(*) FROM shared_booking_intents WHERE caller='swarm'",
        [], |r| r.get(0)).unwrap();
    assert_eq!(booked, 0, "no account was booked");
    let db = db(d);
    let seq = |id: &str| -> i64 { db.query_row("SELECT seq FROM swarm_messages WHERE run_id=?1 AND message_id=?2",
        [run, id], |r| r.get(0)).unwrap() };
    assert!(seq("D1") < seq("tasks-result"), "the discovery arrives before the result");
    let phase: String = db.query_row("SELECT phase FROM swarm_messages WHERE run_id=?1
        AND message_id='D1-to-members'", [run], |r| r.get(0)).unwrap();
    assert_eq!(phase, "applied");
    assert!(seq("D1-to-members") < seq("members-result"));
    let accepted: i64 = db.query_row("SELECT COUNT(*) FROM swarm_jobs WHERE run_id=?1 AND status='accepted'",
        [run], |r| r.get(0)).unwrap();
    assert_eq!(accepted, 3);
}

/// S0 end to end: category + objective + Start, one read-back of the pool,
/// allocation and ceiling, one confirmation; the daemon launches the
/// director, whose linked run takes the category's one slot; the director
/// plans, offers a job to the Claude account (refused in an audit run before
/// booking), dispatches three supervised workers through Swarm admission,
/// routes a discovery, accepts three evidence-backed results and completes.
#[test]
fn s0_normal_start_reads_back_once_and_runs_end_to_end() {
    let w = world(true, &[]);
    approve(&w);
    let back = w.d.call("swarm.start", start_params(&w));
    assert_eq!(back["status"], "readback", "{back}");
    let r = &back["readback"];
    assert_eq!(r["summary"], "Auto · up to 4 workers · 60 min", "{r}");
    assert_eq!(r["account_pool"]["targets"], json!(["fixture-local","system-claude"]));
    assert_eq!(r["account_pool"]["source"], "application", "inherited, not asked");
    assert_eq!(r["account_pool"]["needs_account_selection"], false);
    let accounts = r["account_pool"]["accounts"].as_array().unwrap();
    assert_eq!(accounts[0]["quota"], "fixture");
    assert_eq!((accounts[1]["kind"].as_str(), accounts[1]["quota"].as_str()), (Some("account"), Some("unknown")),
        "{r}");
    assert_eq!(r["ceiling"], json!({"max_workers":8,"agents_max_active":5,"workers":4,"growth_per_wave":4}));
    assert_eq!(r["deadline_ms"], 3_600_000);
    assert_eq!(r["allocation"]["run_allocation_percent"], 10);
    assert_eq!(r["allocation"]["finishing_reserve_percent"], 20);
    assert_eq!(r["director"]["state"], "qualified");
    assert_eq!(w.d.call("swarm.list", json!({}))["runs"], json!([]), "a read-back commits nothing");

    // A stale or different yes commits nothing.
    let mut stale = start_params(&w);
    stale["request_id"] = json!("s0-stale");
    stale["confirm_readback_sha256"] = json!("0".repeat(64));
    let refused = w.d.call("swarm.start", stale);
    assert_eq!(refused["status"], "readback_changed", "{refused}");
    assert_eq!(w.d.call("swarm.list", json!({}))["runs"], json!([]));

    let (run, director, digest) = start(&w);
    let state = w.d.call("swarm.get", json!({"id":run}));
    assert_eq!(state["start"]["readback_sha256"], digest.as_str());
    assert_eq!(state["start"]["summary"], "Auto · up to 4 workers · 60 min");
    assert_eq!(state["repositories"].as_array().unwrap().len(), 1);
    assert_eq!(state["allowed_targets"], json!(["fixture-local","system-claude"]));
    // A repeated yes (a client retry) returns the same run and director.
    let mut again = start_params(&w);
    again["request_id"] = json!("s0-start");
    again["confirm_readback_sha256"] = json!(digest);
    let replay = w.d.call("swarm.start", again);
    assert_eq!((replay["run"]["id"].as_str(), replay["director"]["overseer_run_id"].as_str(), replay["duplicate"].as_bool()),
        (Some(run.as_str()), Some(director.as_str()), Some(true)));

    // The director's linked run is the category's one slot: with three
    // workers running, four slots are in use, not five.
    let dispatched = wait_trace(&w.trace, "dispatched", 30);
    assert_eq!(dispatched["active"], 4, "{dispatched}");
    let harness: String = db(&w.d).query_row("SELECT harness FROM runs WHERE id=?1", [&director], |r| r.get(0)).unwrap();
    assert_eq!(harness, "generic");
    std::fs::write(&w.gate, "open").unwrap();
    wait_completed(&w, &run, &director);
    assert_s0_outcome(&w, &run);
}

/// S0 with the daemon killed mid-run: the director and three workers are
/// running (waiting on the test's gate) when the daemon is killed with
/// SIGKILL and restarted. The same supervised director and workers are
/// reattached (no second director, no second launch), the slots are held
/// once, a client retry of the start returns the same run, and the run
/// completes with the same outcome.
#[test]
fn s0_restart_mid_run_reattaches_one_director_and_its_workers() {
    let mut w = world(true, &[]);
    approve(&w);
    let (run, director, digest) = start(&w);
    wait_trace(&w.trace, "dispatched", 30);
    let generation = |d: &Daemon| -> i64 { db(d).query_row("SELECT process_generation FROM runs WHERE id=?1",
        [&director], |r| r.get(0)).unwrap() };
    let before = generation(&w.d);
    assert_eq!(w.d.call("agents.limit.get", json!({}))["active"], 4);
    w.d.kill9();
    w.d.spawn();
    assert_eq!(w.d.call("agents.limit.get", json!({}))["active"], 4, "each occupant counted once after restart");
    assert_eq!(generation(&w.d), before, "the director was reattached, not relaunched");
    let mut again = start_params(&w);
    again["request_id"] = json!("s0-start");
    again["confirm_readback_sha256"] = json!(digest);
    let replay = w.d.call("swarm.start", again);
    assert_eq!((replay["run"]["id"].as_str(), replay["director"]["overseer_run_id"].as_str()),
        (Some(run.as_str()), Some(director.as_str())), "{replay}");
    std::fs::write(&w.gate, "open").unwrap();
    wait_completed(&w, &run, &director);
    assert_eq!(generation(&w.d), before);
    assert_s0_outcome(&w, &run);
    let directors: i64 = db(&w.d).query_row("SELECT COUNT(*) FROM runs WHERE title=?1",
        [format!("{CATEGORY} director")], |r| r.get(0)).unwrap();
    assert_eq!(directors, 1, "one director process for the run");
}

/// S0's variants, none of which commits a run without the owner's yes to
/// what it would do: no approved accounts asks only for the one-time
/// selection (then remembers it); an account with no fresh reading gives a
/// serial read-back; a fresh reading shows the run's share of it; with no
/// qualified director the start is blocked and nothing weaker runs.
#[test]
fn s0_variants_ask_once_fall_back_or_block_without_committing() {
    let w = world(true, &[]);
    // No approved accounts: only the initial selection is needed.
    let back = w.d.call("swarm.start", start_params(&w));
    assert_eq!(back["readback"]["account_pool"]["needs_account_selection"], true, "{back}");
    assert_eq!(back["readback"]["summary"], "Choose the accounts this category may use");
    let mut confirm = start_params(&w);
    confirm["request_id"] = json!("no-accounts");
    confirm["confirm_readback_sha256"] = back["readback_sha256"].clone();
    assert_eq!(w.d.call("swarm.start", confirm)["status"], "needs_account_selection");
    assert_eq!(w.d.call("swarm.list", json!({}))["runs"], json!([]));

    // An approved account with no reading: one agent at a time.
    let mut serial = start_params(&w);
    serial["allowed_targets"] = json!(["system-claude"]);
    let back = w.d.call("swarm.start", serial.clone());
    assert_eq!(back["readback"]["fanout"], "serial", "{back}");
    assert_eq!(back["readback"]["account_pool"]["source"], "selection");
    assert_eq!(back["readback"]["summary"], "Auto · one agent at a time (usage unknown) · 60 min");
    // A fresh reading (35% used): the run's share is 10% of the remaining
    // 65 points, with a 20% finishing reserve, per window.
    fixture_account_booking(&w.d, "system-claude", "claude-account", 35.0, 1);
    let back = w.d.call("swarm.start", serial.clone());
    let account = &back["readback"]["account_pool"]["accounts"][0];
    assert_eq!(account["quota"], "measured", "{back}");
    let windows: Vec<(i64, i64, i64)> = account["windows"].as_array().unwrap().iter()
        .map(|w| (w["remaining_milli"].as_i64().unwrap(), w["allocation_milli"].as_i64().unwrap(),
            w["reserve_milli"].as_i64().unwrap())).collect();
    assert_eq!(windows, vec![(65_000, 6_500, 1_300), (65_000, 6_500, 1_300)]);
    assert_eq!(back["readback"]["fanout"], "bounded");

    // No free slot for the director: the yes is refused before any commit.
    w.d.call("agents.limit.set", json!({"max_active":1}));
    let occupying = run_id(&w.d.generic(&w.checkout, "worktree", "/bin/sleep", &["30"]));
    let mut full = start_params(&w);
    full["allowed_targets"] = json!(["fixture-local"]);
    let back = w.d.call("swarm.start", full.clone());
    full["request_id"] = json!("no-slot");
    full["confirm_readback_sha256"] = back["readback_sha256"].clone();
    let refused = w.d.call("swarm.start", full);
    assert_eq!((refused["status"].as_str(), refused["reason"].as_str()),
        (Some("blocked"), Some("global_agent_limit")), "{refused}");
    assert_eq!(w.d.call("swarm.list", json!({}))["runs"], json!([]));
    w.d.call("run.interrupt", json!({"run_id":occupying}));
    w.d.wait_done(&occupying, 10);
    w.d.call("agents.limit.set", json!({"max_active":9}));

    // Confirming a selection starts the run and remembers the selection for
    // the category; the next read-back inherits it without asking.
    let mut selected = start_params(&w);
    selected["allowed_targets"] = json!(["fixture-local","system-claude"]);
    let back = w.d.call("swarm.start", selected.clone());
    selected["request_id"] = json!("with-selection");
    selected["confirm_readback_sha256"] = back["readback_sha256"].clone();
    let started = w.d.call("swarm.start", selected);
    assert_eq!(started["status"], "started", "{started}");
    let run = started["run"]["id"].as_str().unwrap().to_string();
    let inherited = w.d.call("swarm.start", start_params(&w));
    assert_eq!(inherited["readback"]["account_pool"]["source"], "category", "{inherited}");
    assert_eq!(inherited["readback"]["account_pool"]["targets"], json!(["fixture-local","system-claude"]));
    // One active run per category: a second start is refused, not queued silently.
    let mut second = start_params(&w);
    second["request_id"] = json!("second");
    second["confirm_readback_sha256"] = inherited["readback_sha256"].clone();
    assert!(w.d.try_call("swarm.start", second).unwrap_err().contains("already has an active swarm run"));
    std::fs::write(&w.gate, "open").unwrap();
    wait_completed(&w, &run, started["director"]["overseer_run_id"].as_str().unwrap());

    // No qualified director: the read-back says so and the yes is refused.
    // The native director is on by default, and a pool with no Claude account
    // has none to run it on.
    let plain = world(false, &[]);
    plain.d.call("swarm.policy.set", json!({"scope":"application","allowed_targets":["fixture-local"]}));
    let back = plain.d.call("swarm.start", start_params(&plain));
    assert_eq!(back["readback"]["director"], json!({"state":"blocked","reason":"no_director_account"}));
    assert_eq!(back["readback"]["summary"], "Blocked · no qualified director · 60 min");
    let mut confirm = start_params(&plain);
    confirm["request_id"] = json!("blocked");
    confirm["confirm_readback_sha256"] = back["readback_sha256"].clone();
    let blocked = plain.d.call("swarm.start", confirm);
    assert_eq!((blocked["status"].as_str(), blocked["reason"].as_str()), (Some("blocked"), Some("no_director_account")));
    assert_eq!(plain.d.call("swarm.list", json!({}))["runs"], json!([]), "no weaker substitute runs");
    assert_eq!(plain.d.call("agents.limit.get", json!({}))["active"], 0);
}

/// Outside the fixture API a configured scripted director is not a
/// qualified director: the product start stays blocked.
#[test]
fn product_start_without_fixture_api_is_blocked_for_the_director() {
    let w = world(true, &[("OVERSEER_SWARM_FIXTURE_API", "0")]);
    w.d.call("swarm.policy.set", json!({"scope":"application","allowed_targets":["system-claude"]}));
    let back = w.d.call("swarm.start", start_params(&w));
    assert_eq!(back["readback"]["director"]["state"], "blocked", "{back}");
    assert_eq!(back["readback"]["account_pool"]["accounts"][0]["quota"], "unknown");
    let mut confirm = start_params(&w);
    confirm["request_id"] = json!("product");
    confirm["confirm_readback_sha256"] = back["readback_sha256"].clone();
    assert_eq!(w.d.call("swarm.start", confirm)["status"], "blocked");
    assert_eq!(w.d.call("swarm.list", json!({}))["runs"], json!([]));
}
