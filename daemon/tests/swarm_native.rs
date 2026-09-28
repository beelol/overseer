//! The proposed native director and worker path, behind the daemon setting
//! `swarm.native_director` (on by default since the owner's decision of
//! 2026-09-28; the owner can turn it off, never Overseer's conversation).
//! The director is the synthetic Claude fixture (`claude-fixture.js`, mode
//! `swarm`) launched through the one launch path with the director's Swarm
//! tools over MCP (Gate S's `overseerd mcp` shim); native workers are the
//! same fixture with a worker's tools. Every director and worker step goes
//! through those tools; the choices are scripted, not model reasoning. No
//! provider account is used and no paid turn is made.

mod common;
use common::*;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const DIRECTOR_TOOLS: [&str; 8] = ["swarm_status", "swarm_plan", "swarm_revise", "swarm_dispatch", "swarm_inbox",
    "swarm_message", "swarm_decide", "swarm_complete"];
const WORKER_TOOLS: [&str; 6] = ["swarm_progress", "swarm_ask", "swarm_discovery", "swarm_inbox", "swarm_applied",
    "swarm_result"];

struct World {
    d: Daemon,
    dir: tempfile::TempDir,
    checkout: PathBuf,
    trace: PathBuf,
    gate: PathBuf,
    script: PathBuf,
    workers: PathBuf,
}

fn world(extra: &[(&str, &str)]) -> World {
    world_in(tmp(), extra)
}

fn world_in(dir: tempfile::TempDir, extra: &[(&str, &str)]) -> World {
    let checkout = repo(&dir.path().join("atlas"));
    let trace = dir.path().join("trace.jsonl");
    let gate = dir.path().join("gate");
    let script = dir.path().join("director.json");
    let workers = dir.path().join("workers.json");
    let claude = repo_root().join("fixtures/fake-harness/claude-fixture.js").display().to_string();
    let mut env: Vec<(String, String)> = vec![
        ("OVERSEER_CLAUDE_PATH".into(), claude),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH".into(),
            "CLAUDE_FIXTURE_MODE,CLAUDE_FIXTURE_VERSION,CLAUDE_FIXTURE_HELP,CLAUDE_FIXTURE_SWARM_SCRIPT,CLAUDE_FIXTURE_SWARM_WORKERS".into()),
        ("CLAUDE_FIXTURE_MODE".into(), "swarm".into()),
        ("CLAUDE_FIXTURE_VERSION".into(), "2.1.246".into()),
        ("CLAUDE_FIXTURE_SWARM_SCRIPT".into(), script.display().to_string()),
        ("CLAUDE_FIXTURE_SWARM_WORKERS".into(), workers.display().to_string()),
    ];
    for (key, value) in extra {
        env.retain(|(k, _)| k != key);
        env.push((key.to_string(), value.to_string()));
    }
    let env: Vec<(&str, &str)> = env.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    let d = Daemon::start(&env);
    World { d, dir, checkout, trace, gate, script, workers }
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
        .filter_map(|line| serde_json::from_str(line).ok()).collect()
}

fn wait_trace(w: &World, pred: impl Fn(&Value) -> bool, what: &str, secs: u64) -> Value {
    let deadline = Instant::now() + Duration::from_secs(secs);
    loop {
        if let Some(found) = trace(&w.trace).into_iter().find(|t| pred(t)) {
            return found;
        }
        assert!(Instant::now() < deadline, "never reached {what}: {:?}", trace(&w.trace));
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn step<'a>(step: &'a str, role: &'a str) -> impl Fn(&Value) -> bool + 'a {
    move |t: &Value| t["step"] == step && t["role"] == role
}

fn active(d: &Daemon) -> i64 {
    d.call("agents.limit.get", json!({}))["active"].as_i64().unwrap()
}

/// The member's MCP token, from its private configuration in its run folder.
fn member_token(d: &Daemon, run: &str) -> String {
    let raw = std::fs::read(d.home.path().join("runs").join(run).join("mcp-swarm.json")).unwrap();
    let body: Value = serde_json::from_slice(&raw).unwrap();
    body["mcpServers"]["overseer"]["env"]["OVERSEER_MCP_TOKEN"].as_str().unwrap().to_string()
}

fn launch_file(d: &Daemon, run: &str) -> Value {
    let (_, dir) = launch_info(d, run);
    serde_json::from_slice(&std::fs::read(dir.join("launch.json")).unwrap()).unwrap()
}

fn start_params(w: &World, permission: &str) -> Value {
    json!({"category":"Backend security","objective":"Audit Atlas tenant isolation; report bugs, don't change application code",
        "repositories":[w.checkout],"source_change_permission":permission})
}

fn start(w: &World, permission: &str, request: &str) -> (String, String, Value) {
    let back = w.d.call("swarm.start", start_params(w, permission));
    let mut confirm = start_params(w, permission);
    confirm["request_id"] = json!(request);
    confirm["confirm_readback_sha256"] = back["readback_sha256"].clone();
    let started = w.d.call("swarm.start", confirm);
    assert_eq!(started["status"], "started", "{started}");
    assert_eq!(started["director"]["status"], "launched", "{started}");
    (started["run"]["id"].as_str().unwrap().to_string(),
        started["director"]["overseer_run_id"].as_str().unwrap().to_string(), back)
}

fn benefit(jobs: &[&str]) -> Value {
    let cost = json!({"elapsed_ms":10,"usage_milli":{"points":1}});
    let workers: Vec<Value> = jobs.iter().map(|job| json!({"id":job,"elapsed_ms":100,"usage_milli":{"points":10}})).collect();
    let serial = json!({"planning":cost,"context":cost,"integration":cost,"review":cost,"retries":cost,"workers":workers});
    let mut parallel = serial.clone();
    parallel["context"] = json!({"elapsed_ms":20,"usage_milli":{"points":1}});
    json!({"independent":true,"max_workers":jobs.len(),"allocation_milli":{"points":100000},
        "finishing_reserve_milli":{"points":20000},"serial":serial,"parallel":parallel})
}

/// S0's world with the gate on: the approved pool is the fixture target and
/// the Claude account (with a fresh reading, so it is a healthy target and
/// the audit boundary, not missing data, is what refuses it).
fn s0_world() -> World {
    let fixture = repo_root().join("fixtures/swarm/s0-start-v1");
    let dir = tmp();
    let gate = dir.path().join("gate");
    let targets = dir.path().join("targets.json");
    std::fs::write(&targets, json!({
        "targets":[
            {"id":"fixture-local","harness":"generic","account_id":"fixture-account","pool_ids":["fixture-pool"],
             "capabilities":["audit"],"health":"up","auth":"ok"},
            {"id":"unapproved-local","harness":"generic","account_id":"other-account","pool_ids":["fixture-pool"],
             "capabilities":["audit"],"health":"up","auth":"ok"}],
        "pools":[{"id":"fixture-pool","windows":[{"id":"run","unit":"points","remaining_milli":1000000,
            "protected_milli":0,"reserved_milli":0,"confidence":"exact","expires_ms":0}]}],
        "estimate_milli":{"points":100}}).to_string()).unwrap();
    // The scripted S0 workers (`worker.py`) wait on the same gate as the director.
    let worker = json!({"program":"/usr/bin/python3","args":[fixture.join("worker.py"), gate]}).to_string();
    let targets = targets.to_str().unwrap().to_string();
    let w = world_in(dir, &[("OVERSEER_SWARM_FIXTURE_TARGETS", targets.as_str()),
        ("OVERSEER_SWARM_FIXTURE_WORKER", worker.as_str())]);
    w.d.call("agents.limit.set", json!({"max_active":5}));
    w.d.call("swarm.policy.set", json!({"scope":"application","allowed_targets":["fixture-local","system-claude"]}));
    fixture_account_booking(&w.d, "system-claude", "claude-account", 0.0, 1);
    w.d.call("swarm.native_director.set", json!({"enabled":true}));
    std::fs::write(&w.script, json!({
        "trace":w.trace,"gate":w.gate,
        "jobs":[
            {"id":"projects","title":"Project route audit","acceptance":"checked path evidence","deps":[],"required_capabilities":["audit"]},
            {"id":"tasks","title":"Task mutation audit","acceptance":"local reproduction","deps":[],"required_capabilities":["audit"]},
            {"id":"members","title":"Membership role audit","acceptance":"role matrix evidence","deps":[],"required_capabilities":["audit"]}],
        "estimate":benefit(&["projects","tasks","members"]),
        "offers":[{"job":"projects","target":"unapproved-local"},{"job":"members"}],
        "dispatch":[{"job":"projects","target":"fixture-local"},{"job":"tasks","target":"fixture-local"},
            {"job":"members","target":"fixture-local"}],
        "route":{"D1":{"job":"members","message_id":"D1-to-members",
            "payload":{"discovery_id":"D1","focus":"Check whether role changes rely on the task lookup"}}},
        "complete":{"summary":"Tenant isolation audit: one confirmed task-mutation defect; project and membership paths checked",
            "verification":"Three supervised fixture workers submitted evidence and exited"}}).to_string()).unwrap();
    w
}

fn wait_completed(w: &World, run: &str, director: &str) {
    let deadline = Instant::now() + Duration::from_secs(90);
    while ["queued", "starting", "running", "waiting_for_user"].contains(&w.d.run(director)["status"].as_str().unwrap()) {
        assert!(Instant::now() < deadline, "director still running; trace={:?}; run={}",
            trace(&w.trace), w.d.call("swarm.get", json!({"id":run})));
        std::thread::sleep(Duration::from_millis(100));
    }
    let finished = w.d.run(director);
    if finished["status"] != "completed" {
        let errors: Vec<String> = w.d.events(director).iter().filter(|e| e["kind"] == "error"
            || (e["kind"] == "output" && e["payload"]["role"] == "stderr"))
            .map(|e| e["payload"].to_string().chars().take(600).collect()).collect();
        panic!("director {}: errors {errors:?}", finished["status"]);
    }
    let deadline = Instant::now() + Duration::from_secs(15);
    while active(&w.d) != 0 {
        assert!(Instant::now() < deadline, "slots still held after the run completed");
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(w.d.call("swarm.get", json!({"id":run}))["status"], "completed");
}

/// What every S0 run must show, whoever directs it.
fn assert_s0_outcome(w: &World, run: &str) {
    let d = &w.d;
    assert_eq!(count(d, "SELECT COUNT(*) FROM swarm_worker_launches WHERE run_id=?1", run), 3);
    assert_eq!(count(d, "SELECT COUNT(*) FROM swarm_attempts WHERE run_id=?1", run), 3,
        "one attempt per job: the refused offers admitted nothing");
    assert_eq!(count(d, "SELECT COUNT(*) FROM swarm_decisions WHERE run_id=?1 AND decision='accept'", run), 3);
    assert_eq!(count(d, "SELECT COUNT(*) FROM swarm_completions WHERE run_id=?1", run), 1);
    assert_eq!(count(d, "SELECT COUNT(*) FROM swarm_director_owners WHERE run_id=?1", run), 1);
    let offers: Vec<Value> = trace(&w.trace).into_iter().filter(|t| t["step"] == "offered").collect();
    assert_eq!((offers[0]["target"].as_str(), offers[0]["status"].as_str(), offers[0]["reason"].as_str()),
        (Some("unapproved-local"), Some("blocked"), Some("not_allowed")), "{offers:?}");
    // The director names no account: Auto's selector finds none eligible in
    // an audit, where no native worker has an audit-only source boundary.
    assert_eq!((offers[1]["status"].as_str(), offers[1]["reason"].as_str()),
        (Some("blocked"), Some("no_eligible_route")), "{offers:?}");
    let excluded = offers[1]["decision"]["exclusions"].as_array().unwrap();
    assert_eq!(excluded.len(), 2, "system-claude's two priors: {offers:?}");
    assert!(excluded.iter().all(|e| e["reason"] == "audit_source_boundary_unqualified"
        && e["route_id"].as_str().unwrap().starts_with("system-claude/")), "{offers:?}");
    let booked: i64 = db(d).query_row("SELECT COUNT(*) FROM shared_booking_intents WHERE caller='swarm'",
        [], |r| r.get(0)).unwrap();
    assert_eq!(booked, 0, "no worker account was booked in an audit");
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

/// The director's own launch: Claude on the approved account, its Swarm
/// tools over MCP, native delegation denied, no credential in its
/// environment or prompt.
fn assert_native_director(w: &World, director: &str) {
    let (harness, profile): (String, Option<String>) = db(&w.d).query_row(
        "SELECT harness,profile_id FROM runs WHERE id=?1", [director], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
    assert_eq!((harness.as_str(), profile.as_deref()), ("claude", Some("system-claude")));
    let launch = launch_file(&w.d, director);
    let args: Vec<String> = serde_json::from_value(launch["args"].clone()).unwrap();
    assert!(args.windows(2).any(|a| a == ["--disallowedTools", "Agent,Task"]), "{args:?}");
    assert!(args.iter().any(|a| a == "--strict-mcp-config"), "{args:?}");
    let allowed = &args[args.iter().position(|a| a == "--allowedTools").unwrap() + 1];
    assert_eq!(allowed, &DIRECTOR_TOOLS.iter().map(|t| format!("mcp__overseer__{t}")).collect::<Vec<_>>().join(","));
    let env = launch["env"].as_object().unwrap();
    assert!(!env.keys().any(|k| k.starts_with("OVERSEER_SWARM_") || k == "OVERSEER_MCP_TOKEN"),
        "no Swarm credential in the director's environment: {:?}", env.keys().collect::<Vec<_>>());
    let started = wait_trace(w, step("started", "director"), "director start", 30);
    let tools: Vec<String> = serde_json::from_value(started["tools"].clone()).unwrap();
    assert_eq!(tools, DIRECTOR_TOOLS.iter().map(|t| t.to_string()).collect::<Vec<_>>());
    assert_eq!((started["delegation_denied"].as_bool(), started["token_in_prompt"].as_bool()), (Some(true), Some(false)));
}

/// S0 end to end with the gate on: the read-back names the Claude director
/// (qualified by the daemon's check), the owner's yes launches it through
/// the one launch path, and it runs the whole scenario through its MCP tools:
/// it plans, is refused a target outside the pool and a native worker in an
/// audit, dispatches three workers through admission, routes D1, accepts
/// three evidence-backed results and completes.
#[test]
fn s0_native_director_runs_end_to_end_with_the_gate_on() {
    let w = s0_world();
    let back = w.d.call("swarm.start", start_params(&w, "none"));
    let r = &back["readback"];
    assert_eq!(r["director"], json!({"state":"qualified","kind":"claude_mcp","harness":"claude",
        "version":"2.1.246 (Claude Code)","profile_id":"system-claude","setting":"swarm.native_director",
        "model":"sonnet","effort":"medium","draw_class":"swarm/director"}), "{r}");
    assert_eq!(r["summary"], "Auto · up to 4 workers · 60 min", "{r}");
    let (run, director, _) = start(&w, "none", "s0-native");
    assert_native_director(&w, &director);
    let dispatched = wait_trace(&w, step("dispatched", "director"), "dispatch", 60);
    assert_eq!(dispatched["active"], 4, "the director and three workers: {dispatched}");
    assert_eq!(active(&w.d), 4);
    // The director draws from its approved account with its own class; with
    // no qualified `swarm/director` draw it runs unbooked, as an ordinary start does.
    let director_bookings = count(&w.d, "SELECT COUNT(*) FROM shared_booking_intents WHERE run_id=?1", &director);
    assert_eq!(director_bookings, 0);
    std::fs::write(&w.gate, "open").unwrap();
    wait_completed(&w, &run, &director);
    assert_s0_outcome(&w, &run);
    let completed = wait_trace(&w, step("completed", "director"), "completion", 5);
    assert_eq!(completed["status"], "completed", "{completed}");
    let calls = count(&w.d, "SELECT COUNT(*) FROM events WHERE kind='swarm_tool_call'
        AND json_extract(payload,'$.swarm_run_id')=?1 AND json_extract(payload,'$.role')='swarm_director'", &run);
    assert!(calls >= 10, "every director step was a tool call: {calls}");
}

/// The switch: on by default since the owner's decision of 2026-09-28, so a
/// start outside the fixture API reads back the qualified Claude director on
/// the recommended default route (sonnet, medium). The owner can turn it off
/// (then a start is blocked `no_qualified_director` and no Swarm run can be
/// created) and on again; Overseer's conversation can read it but never set
/// it. The daemon's qualification refuses an older version, a build without
/// the flags, a missing harness and a pool without a Claude account.
#[test]
fn native_director_is_on_by_default_and_the_owner_can_turn_it_off() {
    let w = world(&[("OVERSEER_SWARM_FIXTURE_API", "0")]);
    w.d.call("swarm.policy.set", json!({"scope":"application","allowed_targets":["system-claude"]}));
    let setting = w.d.call("swarm.native_director.get", json!({}));
    assert_eq!((setting["enabled"].as_bool(), setting["default"].as_bool(), setting["decision"].as_str()),
        (Some(true), Some(true), Some("made")), "{setting}");
    let on = w.d.call("swarm.start", start_params(&w, "none"));
    assert_eq!(on["readback"]["director"], json!({"state":"qualified","kind":"claude_mcp","harness":"claude",
        "version":"2.1.246 (Claude Code)","profile_id":"system-claude","setting":"swarm.native_director",
        "model":"sonnet","effort":"medium","draw_class":"swarm/director"}), "{on}");
    w.d.call("swarm.native_director.set", json!({"enabled":false}));
    let blocked = || {
        let back = w.d.call("swarm.start", start_params(&w, "none"));
        assert_eq!(back["readback"]["director"], json!({"state":"blocked","reason":"no_qualified_director"}), "{back}");
        assert_eq!(back["readback"]["summary"], "Blocked · no qualified director · 60 min");
        back
    };
    let back = blocked();
    let mut confirm = start_params(&w, "none");
    confirm["request_id"] = json!("gate-off");
    confirm["confirm_readback_sha256"] = back["readback_sha256"].clone();
    let refused = w.d.call("swarm.start", confirm);
    assert_eq!((refused["status"].as_str(), refused["reason"].as_str()), (Some("blocked"), Some("no_qualified_director")));
    assert_eq!(w.d.call("swarm.list", json!({}))["runs"], json!([]));
    assert_eq!(active(&w.d), 0);
    // Off, no Swarm run can be created either (AC-204: Swarm stays hidden).
    let created = w.d.try_call("swarm.create", json!({"category":"Backend security","objective":"x",
        "repositories":[w.checkout]}));
    assert!(created.unwrap_err().contains("swarm.native_director"), "swarm.create must be refused while off");
    assert_eq!(w.d.call("swarm.list", json!({}))["runs"], json!([]));

    w.d.call("swarm.native_director.set", json!({"enabled":true}));
    let on = w.d.call("swarm.start", start_params(&w, "none"));
    assert_eq!((on["readback"]["director"]["state"].as_str(), on["readback"]["director"]["kind"].as_str()),
        (Some("qualified"), Some("claude_mcp")), "{on}");
    w.d.call("swarm.native_director.set", json!({"enabled":false}));
    blocked();
    assert!(w.d.try_call("swarm.native_director.set", json!({"enabled":"yes"})).is_err());

    let reason = |extra: &[(&str, &str)], targets: Value| -> String {
        let mut env = vec![("OVERSEER_SWARM_FIXTURE_API", "0")];
        env.extend_from_slice(extra);
        let v = world(&env);
        v.d.call("swarm.policy.set", json!({"scope":"application","allowed_targets":targets}));
        v.d.call("swarm.native_director.set", json!({"enabled":true}));
        let back = v.d.call("swarm.start", start_params(&v, "none"));
        assert_eq!(back["readback"]["director"]["state"], "blocked", "{back}");
        back["readback"]["director"]["reason"].as_str().unwrap().to_string()
    };
    assert_eq!(reason(&[("CLAUDE_FIXTURE_VERSION", "2.1.245")], json!(["system-claude"])), "director_version_unqualified");
    assert_eq!(reason(&[("CLAUDE_FIXTURE_HELP", "bare")], json!(["system-claude"])), "director_tools_unqualified");
    assert_eq!(reason(&[("OVERSEER_CLAUDE_PATH", "/nonexistent/claude")], json!(["system-claude"])), "director_harness_missing");
    assert_eq!(reason(&[], json!(["system-codex"])), "no_director_account");
}

/// Native workers on the product path (the Swarm fixture API off): the
/// Claude director dispatches two Claude workers on an approved account;
/// admission books each through the shared booking (a fixture draw), and
/// each worker reports through its own attempt token over MCP: progress, a
/// question the director answers, and a result with evidence the director
/// accepts. A worker's token opens no director tool (through the fixture's
/// MCP client and directly), and no worker can report for another.
#[test]
fn native_workers_report_through_their_own_tokens_and_cannot_act_for_others() {
    let draws = tmp();
    let draw_file = draws.path().join("draws.json");
    let w = world(&[("OVERSEER_SWARM_FIXTURE_API", "0"), ("OVERSEER_SHARED_BOOKING_FIXTURE_API", "1"),
        ("OVERSEER_SWARM_FIXTURE_DRAW", draw_file.to_str().unwrap())]);
    let workers_profile = w.d.call("profile.create", json!({"name":"workers","harness":"claude"}))["id"]
        .as_str().unwrap().to_string();
    let booking = fixture_account_booking(&w.d, &workers_profile, "worker-account", 0.0, 3_000);
    std::fs::write(&draw_file, json!({workers_profile.clone(): booking}).to_string()).unwrap();
    w.d.call("agents.limit.set", json!({"max_active":5}));
    w.d.call("swarm.policy.set", json!({"scope":"application","allowed_targets":["system-claude", workers_profile]}));
    w.d.call("swarm.native_director.set", json!({"enabled":true}));
    let worker_gate = w.dir.path().join("worker-gate");
    std::fs::write(&w.script, json!({
        "trace":w.trace,"gate":w.gate,
        "jobs":[{"id":"a","title":"Tenant lookup","acceptance":"evidence","deps":[]},
            {"id":"b","title":"Role checks","acceptance":"evidence","deps":[]}],
        "estimate":benefit(&["a","b"]),
        "dispatch":[{"job":"a"},{"job":"b"}],
        "answer":"Use the tenant id from the session",
        "complete":{"summary":"Two native workers reported","verification":"Evidence accepted per job"}}).to_string()).unwrap();
    std::fs::write(&w.workers, json!({
        "trace":w.trace,"gate":worker_gate,
        "jobs":{
            "a":{"ask":"Which tenant id should the lookup use?",
                "evidence":[{"id":"a-proof","kind":"finding","content":"lookup scoped by tenant"}]},
            "b":{"probe":[
                    {"tool":"swarm_decide","args":{"job_id":"a","decision":"accept","evidence":["a-proof"]}},
                    {"tool":"swarm_result","args":{"job_id":"a","summary":"forged","evidence":[{"id":"forged","kind":"finding","content":"x"}]}}],
                "evidence":[{"id":"b-proof","kind":"finding","content":"roles checked"}]}}}).to_string()).unwrap();

    let (run, director, back) = start(&w, "isolated", "native-workers");
    let accounts = back["readback"]["account_pool"]["accounts"].as_array().unwrap();
    assert_eq!((accounts[1]["target"].as_str(), accounts[1]["quota"].as_str()), (Some(workers_profile.as_str()), Some("measured")));
    assert_native_director(&w, &director);
    let dispatched = wait_trace(&w, step("dispatched", "director"), "dispatch", 60);
    assert_eq!(dispatched["active"], 3, "the director and two workers: {dispatched}");
    let launched: Vec<Value> = trace(&w.trace).into_iter().filter(|t| t["step"] == "launched").collect();
    let worker = |job: &str| launched.iter().find(|t| t["job"] == job).unwrap().clone();
    let (a, b) = (worker("a"), worker("b"));
    let (a_run, b_run) = (a["worker"].as_str().unwrap().to_string(), b["worker"].as_str().unwrap().to_string());
    let (a_attempt, b_attempt) = (a["attempt"].as_str().unwrap().to_string(), b["attempt"].as_str().unwrap().to_string());
    for (job, attempt) in [("a", &a_attempt), ("b", &b_attempt)] {
        let started = wait_trace(&w, |t| t["step"] == "started" && t["role"] == "worker" && t["job"] == job, "worker start", 30);
        let tools: Vec<String> = serde_json::from_value(started["tools"].clone()).unwrap();
        assert_eq!(tools, WORKER_TOOLS.iter().map(|t| t.to_string()).collect::<Vec<_>>());
        assert_eq!((started["delegation_denied"].as_bool(), started["token_in_prompt"].as_bool()), (Some(true), Some(false)));
        // Booked through the one booking: effects claimed, the run bound.
        let (caller, bound): (String, Option<String>) = db(&w.d).query_row(
            "SELECT caller,run_id FROM shared_booking_intents WHERE work_unit_id=?1",
            [format!("swarm/{attempt}")], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!(caller, "swarm");
        assert_eq!(bound.as_deref(), Some(if job == "a" { a_run.as_str() } else { b_run.as_str() }));
    }
    assert_eq!(active(&w.d), 3, "each booked worker is its booking's slot, counted once");

    // Directly, over the daemon's tool call: who is speaking is the token.
    let a_token = member_token(&w.d, &a_run);
    let director_token = member_token(&w.d, &director);
    let tool = |token: &str, name: &str, arguments: Value| w.d.call("overseer.tool",
        json!({"token":token,"name":name,"arguments":arguments}));
    let listed: Vec<String> = w.d.call("overseer.tools", json!({"token":a_token}))["tools"].as_array().unwrap()
        .iter().map(|t| t["name"].as_str().unwrap().to_string()).collect();
    assert_eq!(listed, WORKER_TOOLS.iter().map(|t| t.to_string()).collect::<Vec<_>>());
    let refusal = tool(&a_token, "swarm_decide", json!({"job_id":"b","decision":"accept","evidence":["b-proof"]}));
    assert_eq!(refusal["is_error"], true);
    assert!(refusal["text"].as_str().unwrap().contains("swarm_worker members have no tool swarm_decide"), "{refusal}");
    for arguments in [json!({"job_id":"b","summary":"for b","evidence":[{"id":"x","kind":"finding","content":"x"}]}),
        json!({"attempt_id":b_attempt,"summary":"for b","evidence":[{"id":"x","kind":"finding","content":"x"}]})] {
        let refused = tool(&a_token, "swarm_result", arguments);
        assert!(refused["text"].as_str().unwrap().contains("a member cannot name another"), "{refused}");
    }
    let refused = tool(&director_token, "swarm_result", json!({"summary":"x","evidence":[{"kind":"finding","content":"x"}]}));
    assert!(refused["text"].as_str().unwrap().contains("swarm_director members have no tool swarm_result"), "{refused}");
    assert!(w.d.try_call("overseer.tool", json!({"token":"0".repeat(32),"name":"swarm_result","arguments":{}}))
        .unwrap_err().contains("unknown token"));

    std::fs::write(&worker_gate, "open").unwrap();
    std::fs::write(&w.gate, "open").unwrap();
    wait_completed(&w, &run, &director);
    // Through the fixture's own MCP client, worker b's director tool and its
    // report for a were refused.
    let probes: Vec<Value> = trace(&w.trace).into_iter().filter(|t| t["step"] == "probed").collect();
    assert_eq!(probes.len(), 2, "{probes:?}");
    assert!(probes[0]["error"].as_str().unwrap().contains("no tool swarm_decide"), "{probes:?}");
    assert!(probes[1]["error"].as_str().unwrap().contains("cannot name another"), "{probes:?}");
    let answer = wait_trace(&w, |t| t["step"] == "answer" && t["job"] == "a", "answer", 5);
    assert_eq!((answer["answer"].as_str(), answer["applied"].as_str()), (Some("Use the tenant id from the session"), Some("applied")), "{answer}");
    let db = db(&w.d);
    let sender = |id: &str| -> String { db.query_row("SELECT sender FROM swarm_messages WHERE run_id=?1 AND message_id=?2",
        [&run, &id.to_string()], |r| r.get(0)).unwrap() };
    assert_eq!(sender(&format!("result-{a_attempt}")), a_attempt);
    assert_eq!(sender(&format!("result-{b_attempt}")), b_attempt);
    let forged: i64 = db.query_row("SELECT COUNT(*) FROM swarm_artifacts WHERE id IN ('forged','x')", [], |r| r.get(0)).unwrap();
    assert_eq!(forged, 0, "no refused report left an artifact");
    let results: i64 = db.query_row("SELECT COUNT(*) FROM swarm_messages WHERE run_id=?1 AND kind='result'",
        [&run], |r| r.get(0)).unwrap();
    assert_eq!(results, 2, "one result per worker");
    let decisions: Vec<(String, String, String)> = {
        let mut stmt = db.prepare("SELECT job_id,attempt_id,decision FROM swarm_decisions WHERE run_id=?1 ORDER BY job_id").unwrap();
        let rows = stmt.query_map([&run], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap().map(Result::unwrap).collect();
        rows
    };
    assert_eq!(decisions, vec![("a".into(), a_attempt.clone(), "accept".into()), ("b".into(), b_attempt.clone(), "accept".into())]);
    for worker_run in [&a_run, &b_run] {
        let (harness, profile): (String, Option<String>) = db.query_row("SELECT harness,profile_id FROM runs WHERE id=?1",
            [worker_run], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!((harness.as_str(), profile.as_deref()), ("claude", Some(workers_profile.as_str())));
    }
}

/// S0 with the gate on and the daemon killed mid-run (SIGKILL) while the
/// Claude director and three workers wait on the gate. The same director is
/// reattached (its process generation unchanged), its MCP tools keep
/// working against the restarted daemon, a client retry of the start
/// returns the same run and director, and the run completes as before.
#[test]
fn s0_native_director_survives_a_daemon_restart_mid_run() {
    let mut w = s0_world();
    let (run, director, back) = start(&w, "none", "s0-native-restart");
    wait_trace(&w, step("dispatched", "director"), "dispatch", 60);
    let generation = |d: &Daemon| -> i64 { db(d).query_row("SELECT process_generation FROM runs WHERE id=?1",
        [&director], |r| r.get(0)).unwrap() };
    let before = generation(&w.d);
    assert_eq!(active(&w.d), 4);
    w.d.kill9();
    w.d.spawn();
    assert_eq!(active(&w.d), 4, "each occupant counted once after restart");
    assert_eq!(generation(&w.d), before, "the director was reattached, not relaunched");
    let mut again = start_params(&w, "none");
    again["request_id"] = json!("s0-native-restart");
    again["confirm_readback_sha256"] = back["readback_sha256"].clone();
    let replay = w.d.call("swarm.start", again);
    assert_eq!((replay["run"]["id"].as_str(), replay["director"]["overseer_run_id"].as_str()),
        (Some(run.as_str()), Some(director.as_str())), "{replay}");
    std::fs::write(&w.gate, "open").unwrap();
    wait_completed(&w, &run, &director);
    assert_eq!(generation(&w.d), before);
    assert_s0_outcome(&w, &run);
    let directors: i64 = db(&w.d).query_row("SELECT COUNT(*) FROM runs WHERE title='Backend security director'",
        [], |r| r.get(0)).unwrap();
    assert_eq!(directors, 1, "one director process for the run");
}

/// One cell of the four-way matrix: how the work was launched, what it
/// booked and how many app slots it held while running.
#[derive(Debug, PartialEq)]
struct Cell {
    path: String,
    bookings: Vec<String>,
    slots: i64,
    swarm_runs: i64,
    swarm_workers: i64,
}

fn swarm_totals(d: &Daemon) -> (i64, i64) {
    let db = db(d);
    let runs: i64 = db.query_row("SELECT COUNT(*) FROM swarm_runs", [], |r| r.get(0)).unwrap();
    let workers: i64 = db.query_row("SELECT COUNT(*) FROM swarm_worker_launches", [], |r| r.get(0)).unwrap();
    (runs, workers)
}

fn bound_bookings(d: &Daemon, run: &str) -> Vec<String> {
    let db = db(d);
    let mut stmt = db.prepare("SELECT caller||':'||CASE WHEN slot_held THEN 'slot' ELSE 'no_slot' END
        FROM shared_booking_intents WHERE run_id=?1").unwrap();
    let rows = stmt.query_map([run], |r| r.get(0)).unwrap().map(Result::unwrap).collect();
    rows
}

/// What launched a run, from the daemon's own records.
fn launch_path(d: &Daemon, run: &str) -> String {
    let db = db(d);
    let has = |sql: &str| -> bool { db.query_row(sql, [run], |r| r.get::<_, i64>(0)).unwrap() > 0 };
    if has("SELECT COUNT(*) FROM swarm_director_owners WHERE overseer_run_id=?1") { "swarm_director".into() }
    else if has("SELECT COUNT(*) FROM swarm_worker_launches WHERE overseer_run_id=?1") { "swarm_worker".into() }
    else if has("SELECT COUNT(*) FROM auto_root_intents WHERE run_id=?1") { "auto_root".into() }
    else { "ordinary".into() }
}

fn end_run(d: &Daemon, run: &str) {
    let _ = d.try_call("run.interrupt", json!({"run_id":run}));
    d.wait_done(run, 30);
    let deadline = Instant::now() + Duration::from_secs(15);
    while active(d) != 0 {
        assert!(Instant::now() < deadline, "slots still held after {run} ended");
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// SWARM-01's four combinations on one daemon, with the Swarm fixture API
/// off (the product path) and the native director on: Manual and Auto (Auto
/// Mode's root) with Swarm off, and a normal Swarm start with Auto Mode off
/// and on. Each cell records its launch path, what it booked and the app
/// slots it held; Swarm off creates no Swarm run or worker, a Swarm keeps to
/// its approved pool, and Auto Mode does not change how a Swarm launches:
/// with either setting, Auto's selector chooses each worker's route within
/// the approved pool (SWARM-24), and no worker is an Auto root.
#[test]
fn four_way_launch_matrix_auto_manual_by_swarm_on_off() {
    let draws = tmp();
    let draw_file = draws.path().join("draws.json");
    let codex = repo_root().join("fixtures/fake-harness/codex-app-fixture.js").display().to_string();
    let w = world(&[("OVERSEER_SWARM_FIXTURE_API", "0"), ("OVERSEER_SHARED_BOOKING_FIXTURE_API", "1"),
        ("OVERSEER_SWARM_FIXTURE_DRAW", draw_file.to_str().unwrap()),
        ("OVERSEER_CODEX_PATH", codex.as_str()), ("FIXTURE_MODE", "managed-models"), ("FIXTURE_TURN_DELAY_MS", "8000"),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH",
            "CLAUDE_FIXTURE_MODE,CLAUDE_FIXTURE_VERSION,CLAUDE_FIXTURE_HELP,CLAUDE_FIXTURE_SWARM_SCRIPT,CLAUDE_FIXTURE_SWARM_WORKERS,FIXTURE_MODE,FIXTURE_TURN_DELAY_MS")]);
    let d = &w.d;
    let workers_profile = d.call("profile.create", json!({"name":"workers","harness":"claude"}))["id"]
        .as_str().unwrap().to_string();
    let booking = fixture_account_booking(d, &workers_profile, "worker-account", 0.0, 3_000);
    std::fs::write(&draw_file, json!({workers_profile.clone(): booking}).to_string()).unwrap();
    d.call("agents.limit.set", json!({"max_active":5}));
    d.call("swarm.native_director.set", json!({"enabled":true}));

    // Manual × Swarm off: an ordinary start. Its automatic booking attempt
    // finds no qualified draw for the account, so it runs unbooked.
    d.call("auto.mode.set", json!({"enabled":false}));
    let manual = run_id(&d.call("task.create", json!({"repo":w.checkout,"harness":"codex-app","model":"gpt-6-sol",
        "effort":"medium","prompt":"hold","title":"manual"})));
    d.wait_status(&manual, |s| s == "running", 20);
    let (runs, workers) = swarm_totals(d);
    let manual_off = Cell { path: launch_path(d, &manual), bookings: bound_bookings(d, &manual), slots: active(d),
        swarm_runs: runs, swarm_workers: workers };
    end_run(d, &manual);

    // Auto × Swarm off: an Auto root. Its route is Auto's choice and its
    // account is held by Auto's unknown-draw claim on the whole pool.
    d.call("auto.mode.set", json!({"enabled":true}));
    let root = d.call("auto.start", json!({"work_unit_id":"matrix-root","repo":w.checkout,"workspace_mode":"worktree",
        "prompt":"hold parent","title":"auto","allowed_profiles":["system-codex"],"min_tier":"general",
        "required_tools":[],"sandbox":"read_only"}));
    let root_run = run_id(&root);
    d.wait_status(&root_run, |s| s == "running", 20);
    let claim: String = db(d).query_row("SELECT state FROM auto_pool_claims WHERE work_unit_id='matrix-root'",
        [], |r| r.get(0)).unwrap();
    let mut bookings = bound_bookings(d, &root_run);
    bookings.push(format!("auto_pool_claim:{claim}"));
    let (runs, workers) = swarm_totals(d);
    let auto_off = Cell { path: launch_path(d, &root_run), bookings, slots: active(d), swarm_runs: runs, swarm_workers: workers };
    assert_eq!(root["decision"]["selected"], "system-codex/gpt-6-sol/medium", "{root}");
    end_run(d, &root_run);

    // Swarm on, with Auto Mode off and then on: the normal start; the Claude
    // director on the approved Claude account and two native workers booked
    // through Swarm admission on the approved worker account.
    d.call("swarm.policy.set", json!({"scope":"application","allowed_targets":["system-claude", workers_profile]}));
    let swarm_cell = |auto: bool, category: &str| -> (Cell, Vec<String>) {
        d.call("auto.mode.set", json!({"enabled":auto}));
        let cell_trace = w.dir.path().join(format!("{category}-trace.jsonl"));
        let gate = w.dir.path().join(format!("{category}-gate"));
        std::fs::write(&w.script, json!({
            "trace":cell_trace,"gate":gate,
            "jobs":[{"id":"a","title":"First","acceptance":"evidence","deps":[]},
                {"id":"b","title":"Second","acceptance":"evidence","deps":[]}],
            "estimate":benefit(&["a","b"]),
            "dispatch":[{"job":"a"},{"job":"b"}],
            "complete":{"summary":"matrix cell","verification":"evidence accepted"}}).to_string()).unwrap();
        std::fs::write(&w.workers, json!({"trace":cell_trace,"gate":gate,"jobs":{
            "a":{"evidence":[{"id":"a-proof","kind":"finding","content":"a"}]},
            "b":{"evidence":[{"id":"b-proof","kind":"finding","content":"b"}]}}}).to_string()).unwrap();
        let params = json!({"category":category,"objective":"Matrix cell","repositories":[w.checkout],
            "source_change_permission":"isolated"});
        let back = d.call("swarm.start", params.clone());
        let mut confirm = params.clone();
        confirm["request_id"] = json!(category);
        confirm["confirm_readback_sha256"] = back["readback_sha256"].clone();
        let started = d.call("swarm.start", confirm);
        assert_eq!(started["director"]["status"], "launched", "{started}");
        let run = started["run"]["id"].as_str().unwrap().to_string();
        let director = started["director"]["overseer_run_id"].as_str().unwrap().to_string();
        let deadline = Instant::now() + Duration::from_secs(60);
        let launched = loop {
            let steps = trace(&cell_trace);
            if steps.iter().any(|t| t["step"] == "dispatched") {
                break steps.into_iter().filter(|t| t["step"] == "launched").collect::<Vec<_>>();
            }
            assert!(Instant::now() < deadline, "{category}: {steps:?}");
            std::thread::sleep(Duration::from_millis(50));
        };
        let mut bookings = vec![format!("director:{}", bound_bookings(d, &director).join("+"))];
        let mut paths = vec![launch_path(d, &director)];
        for worker in &launched {
            paths.push(launch_path(d, worker["worker"].as_str().unwrap()));
            assert_eq!(worker["target"], workers_profile.as_str(), "a Swarm keeps to its approved pool");
            bookings.push(format!("worker:{}", bound_bookings(d, worker["worker"].as_str().unwrap()).join("+")));
        }
        let (runs, workers) = swarm_totals(d);
        let slots = active(d);
        let routed_by_auto: i64 = db(d).query_row("SELECT COUNT(*) FROM auto_root_intents i JOIN swarm_worker_launches l
            ON l.overseer_run_id=i.run_id", [], |r| r.get(0)).unwrap();
        assert_eq!(routed_by_auto, 0, "no Swarm worker is an Auto root");
        std::fs::write(&gate, "open").unwrap();
        let finished = d.wait_done(&director, 60);
        assert_eq!(finished["status"], "completed", "{finished}");
        assert_eq!(d.call("swarm.get", json!({"id":run}))["status"], "completed");
        let deadline = Instant::now() + Duration::from_secs(15);
        while active(d) != 0 {
            assert!(Instant::now() < deadline, "slots still held after {category}");
            std::thread::sleep(Duration::from_millis(50));
        }
        let (harness, profile): (String, String) = db(d).query_row("SELECT harness,profile_id FROM runs WHERE id=?1",
            [&director], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        (Cell { path: paths.join("+"), bookings, slots, swarm_runs: runs, swarm_workers: workers },
            vec![harness, profile])
    };
    let (manual_on, manual_director) = swarm_cell(false, "Matrix manual");
    let (auto_on, auto_director) = swarm_cell(true, "Matrix auto");

    eprintln!("Manual x Swarm off: {manual_off:?}\nAuto x Swarm off: {auto_off:?}\nManual x Swarm on: {manual_on:?}\nAuto x Swarm on: {auto_on:?}");
    assert_eq!(manual_off, Cell { path: "ordinary".into(), bookings: vec![], slots: 1, swarm_runs: 0, swarm_workers: 0 },
        "an ordinary start: one slot, unbooked while its draw is unknown, no Swarm");
    assert_eq!(auto_off, Cell { path: "auto_root".into(), bookings: vec!["auto_pool_claim:active".into()], slots: 1,
        swarm_runs: 0, swarm_workers: 0 }, "an Auto root: one slot, Auto's unknown-draw claim, no Swarm");
    let swarm_expected = |runs: i64, workers: i64| Cell { path: "swarm_director+swarm_worker+swarm_worker".into(),
        bookings: vec!["director:".into(), "worker:swarm:slot".into(), "worker:swarm:slot".into()],
        slots: 3, swarm_runs: runs, swarm_workers: workers };
    assert_eq!(manual_on, swarm_expected(1, 2),
        "the director (unbooked, its swarm/director draw unknown) and two workers each booked with its slot: three slots");
    assert_eq!(auto_on, swarm_expected(2, 4), "Auto Mode does not change how a Swarm launches");
    assert_eq!(manual_director, vec!["claude".to_string(), "system-claude".to_string()]);
    assert_eq!(auto_director, manual_director);
}

/// A world whose director plans and then waits: the test drives each
/// dispatch through the director's own token (the same tool call its model
/// would make). Claude profiles `names` are created; `accounts` gives each a
/// fixture identity, a reading at `used` percent and a fixture draw of 3,000
/// (thousandths of a point) per window; `pool` is the approved pool.
struct Driven {
    w: World,
    worker_gate: PathBuf,
    ids: std::collections::BTreeMap<String, String>,
    _draws: tempfile::TempDir,
}

fn driven(names: &[&str], accounts: &[(&str, f64)], pool: &[&str], jobs: &[&str], worker_jobs: &[&str]) -> Driven {
    let draw_dir = tmp();
    let draw_file = draw_dir.path().join("draws.json");
    let w = world(&[("OVERSEER_SWARM_FIXTURE_API", "0"), ("OVERSEER_SHARED_BOOKING_FIXTURE_API", "1"),
        ("OVERSEER_SWARM_FIXTURE_DRAW", draw_file.to_str().unwrap())]);
    let mut ids = std::collections::BTreeMap::new();
    ids.insert("system-claude".to_string(), "system-claude".to_string());
    ids.insert("system-codex".to_string(), "system-codex".to_string());
    for name in names {
        let id = w.d.call("profile.create", json!({"name":name,"harness":"claude"}))["id"].as_str().unwrap().to_string();
        ids.insert(name.to_string(), id);
    }
    let mut draws = serde_json::Map::new();
    for (name, used) in accounts {
        let id = &ids[*name];
        draws.insert(id.clone(), fixture_account_booking(&w.d, id, &format!("{name}-account"), *used, 3_000));
    }
    std::fs::write(&draw_file, Value::Object(draws).to_string()).unwrap();
    w.d.call("agents.limit.set", json!({"max_active":6}));
    let approved: Vec<&String> = pool.iter().map(|name| &ids[*name]).collect();
    w.d.call("swarm.policy.set", json!({"scope":"application","allowed_targets":approved}));
    w.d.call("swarm.native_director.set", json!({"enabled":true}));
    let worker_gate = w.dir.path().join("worker-gate");
    let planned: Vec<Value> = jobs.iter().map(|job| json!({"id":job,"title":format!("Job {job}"),
        "acceptance":"evidence","deps":[]})).collect();
    std::fs::write(&w.script, json!({"trace":w.trace,"gate":w.gate,"jobs":planned,"estimate":benefit(jobs),
        "dispatch":[],"complete":{"summary":"driven","verification":"driven"}}).to_string()).unwrap();
    // A job missing from the worker script crashes its worker at once, before any effect.
    let mut scripted = serde_json::Map::new();
    for job in worker_jobs {
        scripted.insert(job.to_string(), json!({"evidence":[{"id":format!("{job}-proof"),"kind":"finding","content":job}]}));
    }
    std::fs::write(&w.workers, json!({"trace":w.trace,"gate":worker_gate,"jobs":scripted}).to_string()).unwrap();
    Driven { w, worker_gate, ids, _draws: draw_dir }
}

fn director_tool(d: &Daemon, director: &str, name: &str, arguments: Value) -> Value {
    d.call("overseer.tool", json!({"token":member_token(d, director),"name":name,"arguments":arguments}))
}

fn dispatch_job(d: &Daemon, director: &str, arguments: Value) -> Value {
    let reply = director_tool(d, director, "swarm_dispatch", arguments);
    assert_eq!(reply["is_error"], false, "{reply}");
    serde_json::from_str(reply["text"].as_str().unwrap()).unwrap()
}

/// Each admitted attempt of a job: (profile, model, effort).
fn admitted_routes(d: &Daemon, run: &str, job: &str) -> Vec<(String, String, String)> {
    let db = db(d);
    let mut stmt = db.prepare("SELECT target_profile_id,target_model,target_effort FROM swarm_admissions
        WHERE run_id=?1 AND job_id=?2 ORDER BY created_ms,rowid").unwrap();
    let rows = stmt.query_map([run, job], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap().map(Result::unwrap).collect();
    rows
}

/// The daemon's recorded route decisions for a job, oldest first.
fn route_decisions(d: &Daemon, run: &str, job: &str) -> Vec<Value> {
    let db = db(d);
    let mut stmt = db.prepare("SELECT payload FROM events WHERE kind='swarm_route_decision'
        AND json_extract(payload,'$.swarm_run_id')=?1 AND json_extract(payload,'$.job_id')=?2 ORDER BY seq").unwrap();
    let rows = stmt.query_map([run, job], |r| r.get::<_, String>(0)).unwrap()
        .map(|p| serde_json::from_str::<Value>(&p.unwrap()).unwrap()["trace"].clone()).collect();
    rows
}

fn exclusion_reasons(trace: &Value) -> std::collections::BTreeMap<String, String> {
    trace["decision"]["exclusions"].as_array().unwrap().iter()
        .map(|e| (e["route_id"].as_str().unwrap().to_string(), e["reason"].as_str().unwrap().to_string())).collect()
}

/// SWARM-24: with Swarm on, Auto's selector chooses each job's route within
/// the approved pool; the director only states requirements. The pool has
/// two healthy accounts, one nearly used up, one never identified and the
/// director's own (unidentified) profile; a sixth, healthy account is
/// outside the pool. Every job draws 3,000 (thousandths of a point) per
/// window; the category may use 10% of each account's remaining allowance
/// less a 20% finishing reserve (8,000 of an untouched account).
/// - Job a, no requirements: the general-tier default (Sonnet, medium) on
///   the first eligible account.
/// - Job b, `min_tier: frontier`: Opus, high, on the same account (5,000
///   of its category allowance left).
/// - Job c, no requirements: that account's category allowance (2,000 left)
///   cannot take the draw, so the other account's Sonnet.
/// The nearly used-up account is excluded on the account's own allowance,
/// the unidentified ones on identity, and the outside account is never a
/// candidate. A director that names an account is refused.
#[test]
fn auto_selects_each_jobs_route_within_the_approved_pool() {
    let t = driven(&["alpha", "beta", "full", "unread", "outside"],
        &[("alpha", 0.0), ("beta", 0.0), ("full", 97.0), ("outside", 0.0)],
        &["system-claude", "alpha", "beta", "full", "unread"], &["a", "b", "c"], &["a", "b", "c"]);
    let (w, ids) = (&t.w, &t.ids);
    let (run, director, _) = start(w, "isolated", "auto-routes");
    wait_trace(w, step("dispatched", "director"), "plan", 60);
    let effective = &w.d.call("swarm.get", json!({"id":run}))["policy"]["effective"];
    assert_eq!((effective["run_allocation_percent"].as_i64(), effective["finishing_reserve_percent"].as_i64()),
        (Some(10), Some(20)), "{effective}");
    let (first, second) = if ids["alpha"] < ids["beta"] { (&ids["alpha"], &ids["beta"]) } else { (&ids["beta"], &ids["alpha"]) };
    let route = |profile: &str, model: &str, effort: &str| (profile.to_string(), model.to_string(), effort.to_string());

    // A director that names an account is refused: Auto chooses it.
    let named = director_tool(&w.d, &director, "swarm_dispatch", json!({"job_id":"a","target":first,"brief":"Job a: a"}));
    assert_eq!(named["is_error"], true, "{named}");
    assert!(named["text"].as_str().unwrap().contains("unknown argument (target)"), "{named}");

    let a = dispatch_job(&w.d, &director, json!({"job_id":"a","brief":"Job a: a"}));
    assert_eq!(a["status"], "launched", "{a}");
    assert_eq!(admitted_routes(&w.d, &run, "a"), vec![route(first, "sonnet", "medium")]);
    let b = dispatch_job(&w.d, &director, json!({"job_id":"b","brief":"Job b: b","requirements":{"min_tier":"frontier"}}));
    assert_eq!(b["status"], "launched", "{b}");
    assert_eq!(admitted_routes(&w.d, &run, "b"), vec![route(first, "opus", "high")]);
    let c = dispatch_job(&w.d, &director, json!({"job_id":"c","brief":"Job c: c"}));
    assert_eq!(c["status"], "launched", "{c}");
    assert_eq!(admitted_routes(&w.d, &run, "c"), vec![route(second, "sonnet", "medium")]);

    let decided = |job: &str| { let d = route_decisions(&w.d, &run, job); assert_eq!(d.len(), 1, "{job}: {d:?}"); d[0].clone() };
    let (da, db_, dc) = (decided("a"), decided("b"), decided("c"));
    for trace in [&da, &db_, &dc] {
        assert_eq!(trace["selector_version"], "swarm-auto-route-v1");
        assert_eq!(trace["inference"]["state"], "not_used");
        let candidates = trace["selection_input"]["routes"].as_array().unwrap();
        assert!(candidates.iter().all(|r| r["profile_id"] != ids["outside"].as_str()), "the pool bounds the candidates: {trace}");
        let reasons = exclusion_reasons(trace);
        for model in ["sonnet/medium", "opus/high"] {
            assert_eq!(reasons[&format!("{}/{model}", ids["full"])], "estimated_draw_exceeds_allowance", "{trace}");
            assert_eq!(reasons[&format!("{}/{model}", ids["unread"])], "unresolved_quota_pool_identity", "{trace}");
            // The director's launch read its account's identity, and the
            // uncalibrated director runs unbooked on it: the account is busy.
            let fit = trace["fit"].as_array().unwrap().iter()
                .find(|f| f["route_id"] == format!("system-claude/{model}")).unwrap();
            assert_eq!(fit["reason"], "account_pool_busy", "{trace}");
            assert!(reasons.contains_key(&format!("system-claude/{model}")), "{trace}");
        }
    }
    assert_eq!(da["decision"]["selected"], format!("{first}/sonnet/medium"));
    assert_eq!(da["decision"]["reason"], "eligible_task_suitable_default");
    assert_eq!(db_["requirements"]["min_tier"], "frontier");
    assert_eq!(exclusion_reasons(&db_)[&format!("{first}/sonnet/medium")], "insufficient_capability");
    let reasons_c = exclusion_reasons(&dc);
    for model in ["sonnet/medium", "opus/high"] {
        assert_eq!(reasons_c[&format!("{first}/{model}")], "category_allocation_exceeded", "{dc}");
    }
    let fit_c = dc["fit"].as_array().unwrap().iter().find(|f| f["route_id"] == format!("{first}/sonnet/medium")).unwrap();
    assert_eq!(fit_c["windows"][0]["category_remaining_milli"], 2_000, "{fit_c}");
    assert_eq!(fit_c["windows"][0]["upper_draw_milli"], 3_000, "{fit_c}");
    // Each worker runs where it was booked.
    let db = db(&w.d);
    for (job, profile) in [("a", first), ("b", first), ("c", second)] {
        let (launched, bound): (String, Option<String>) = db.query_row(
            "SELECT r.profile_id,i.run_id FROM swarm_worker_launches l JOIN runs r ON r.id=l.overseer_run_id
             JOIN shared_booking_intents i ON i.work_unit_id='swarm/'||l.attempt_id
             WHERE l.run_id=?1 AND l.job_id=?2", [&run, &job.to_string()], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!(&launched, profile, "{job}");
        assert!(bound.is_some(), "{job}: its booking is bound to its run");
    }
    std::fs::write(&t.worker_gate, "open").unwrap();
}

/// CONTRACT-04 on the Swarm side: one logical job, at most two attempts
/// across routes and a daemon restart. A worker that fails before any
/// effect returns its job to ready; the next dispatch's route is another
/// eligible account. After the second failure the job is out of attempts:
/// a dispatch is refused before any selection (no route decision, no
/// launch), also after a restart. A job with an uncertain effect is
/// refused the same way (`side_effect_unreconciled`).
#[test]
fn one_job_falls_back_once_then_stops_and_an_uncertain_effect_pauses() {
    let mut t = driven(&["alpha", "beta"], &[("alpha", 0.0), ("beta", 0.0)],
        &["system-claude", "alpha", "beta"], &["x", "y"], &[]);
    let ids = t.ids.clone();
    // The workers of this test fail at once (their jobs are not scripted).
    std::fs::write(&t.worker_gate, "open").unwrap();
    let (run, director, _) = start(&t.w, "isolated", "fallback");
    wait_trace(&t.w, step("dispatched", "director"), "plan", 60);
    let (first, second) = if ids["alpha"] < ids["beta"] { (&ids["alpha"], &ids["beta"]) } else { (&ids["beta"], &ids["alpha"]) };
    let job_state = |d: &Daemon, job: &str| -> (String, i64) { db(d).query_row(
        "SELECT status,attempt_count FROM swarm_jobs WHERE run_id=?1 AND id=?2", [&run, &job.to_string()],
        |r| Ok((r.get(0)?, r.get(1)?))).unwrap() };
    let wait_job = |d: &Daemon, job: &str, want: &str| {
        let deadline = Instant::now() + Duration::from_secs(60);
        while job_state(d, job).0 != want {
            if Instant::now() >= deadline {
                let db = db(d);
                let mut stmt = db.prepare("SELECT r.id,r.status,r.ended_ms,a.status FROM swarm_worker_launches l
                    JOIN runs r ON r.id=l.overseer_run_id JOIN swarm_attempts a ON a.id=l.attempt_id WHERE l.job_id=?1").unwrap();
                let rows: Vec<(String, String, Option<i64>, String)> = stmt.query_map([job], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
                    .unwrap().map(Result::unwrap).collect();
                panic!("{job} never became {want}: {:?} workers {rows:?}", job_state(d, job));
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    };

    let one = dispatch_job(&t.w.d, &director, json!({"job_id":"x","brief":"Job x: x"}));
    assert_eq!(one["status"], "launched", "{one}");
    wait_job(&t.w.d, "x", "ready");
    assert_eq!(job_state(&t.w.d, "x"), ("ready".into(), 1));
    let two = dispatch_job(&t.w.d, &director, json!({"job_id":"x","brief":"Job x: x"}));
    assert_eq!(two["status"], "launched", "{two}");
    let routes = admitted_routes(&t.w.d, &run, "x");
    assert_eq!(routes.iter().map(|r| r.0.as_str()).collect::<Vec<_>>(), vec![first.as_str(), second.as_str()],
        "the second attempt is on another eligible account");
    let decisions = route_decisions(&t.w.d, &run, "x");
    assert_eq!(decisions.len(), 2);
    assert_eq!(exclusion_reasons(&decisions[1])[&format!("{first}/sonnet/medium")], "earlier_attempt_failed_on_route");
    wait_job(&t.w.d, "x", "failed");
    assert_eq!(job_state(&t.w.d, "x"), ("failed".into(), 2));

    let refused = dispatch_job(&t.w.d, &director, json!({"job_id":"x","brief":"Job x: x"}));
    assert_eq!((refused["status"].as_str(), refused["reason"].as_str()), (Some("blocked"), Some("attempt_limit")), "{refused}");
    assert_eq!(route_decisions(&t.w.d, &run, "x").len(), 2, "no selection for a job out of attempts");
    // Replanning the same logical job cannot reset its attempts (SWARM-15).
    let revised = director_tool(&t.w.d, &director, "swarm_revise", json!({"reason":"retry x with a narrower check",
        "jobs":[{"id":"x","title":"Job x","acceptance":"narrower evidence","deps":[]},
            {"id":"y","title":"Job y","acceptance":"evidence","deps":[]}]}));
    assert_eq!(revised["is_error"], false, "{revised}");
    assert_eq!(job_state(&t.w.d, "x"), ("failed".into(), 2), "a revision keeps the spent attempts");
    let replanned = dispatch_job(&t.w.d, &director, json!({"job_id":"x","brief":"Job x: x"}));
    assert_eq!(replanned["reason"], "attempt_limit", "{replanned}");
    assert_eq!(route_decisions(&t.w.d, &run, "x").len(), 2);

    // An uncertain effect on y (recorded by its first attempt) pauses it.
    let one = dispatch_job(&t.w.d, &director, json!({"job_id":"y","brief":"Job y: y"}));
    assert_eq!(one["status"], "launched", "{one}");
    wait_job(&t.w.d, "y", "ready");
    let attempt: String = db(&t.w.d).query_row("SELECT id FROM swarm_attempts WHERE run_id=?1 AND job_id='y'",
        [&run], |r| r.get(0)).unwrap();
    db(&t.w.d).execute("INSERT INTO swarm_effects(run_id,effect_id,job_id,attempt_id,revision,operation_sha256,outcome,created_ms,updated_ms)
        VALUES(?1,'y-push','y',?2,1,'00','unknown',1,1)", [&run, &attempt]).unwrap();
    let paused = dispatch_job(&t.w.d, &director, json!({"job_id":"y","brief":"Job y: y"}));
    assert_eq!((paused["status"].as_str(), paused["reason"].as_str()), (Some("blocked"), Some("side_effect_unreconciled")), "{paused}");
    assert_eq!(route_decisions(&t.w.d, &run, "y").len(), 1, "no selection while an effect is uncertain");

    // After a restart: the same answers, and still two attempts for x, one for y.
    t.w.d.kill9();
    t.w.d.spawn();
    let refused = dispatch_job(&t.w.d, &director, json!({"job_id":"x","brief":"Job x: x"}));
    assert_eq!(refused["reason"], "attempt_limit", "{refused}");
    let paused = dispatch_job(&t.w.d, &director, json!({"job_id":"y","brief":"Job y: y"}));
    assert_eq!(paused["reason"], "side_effect_unreconciled", "{paused}");
    let attempts = |job: &str| -> i64 { db(&t.w.d).query_row("SELECT COUNT(*) FROM swarm_attempts WHERE run_id=?1 AND job_id=?2",
        [&run, &job.to_string()], |r| r.get(0)).unwrap() };
    assert_eq!((attempts("x"), attempts("y")), (2, 1));
    assert_eq!(count(&t.w.d, "SELECT COUNT(*) FROM swarm_worker_launches WHERE run_id=?1", &run), 3);
}

/// SWARM-43: a harness with no qualified Swarm delivery and acknowledgement
/// path is excluded before launch, with the limitation shown. The approved
/// pool holds a Codex profile (no Swarm broker transport for Codex workers)
/// and Claude accounts (the daemon's Swarm tools). The route decision
/// records Codex as `swarm_worker_launch_unsupported`, the job is admitted
/// and launched on the Claude account, and nothing is admitted on Codex.
#[test]
fn a_harness_without_a_swarm_delivery_path_is_excluded_before_launch() {
    // The director runs on the first approved Claude account (system-claude).
    let t = driven(&["alpha"], &[("alpha", 0.0)], &["system-claude", "system-codex", "alpha"], &["a"], &["a"]);
    let (w, ids) = (&t.w, &t.ids);
    let (run, director, _) = start(w, "isolated", "delivery-path");
    wait_trace(w, step("dispatched", "director"), "plan", 60);
    let a = dispatch_job(&w.d, &director, json!({"job_id":"a","brief":"Job a: a"}));
    assert_eq!(a["status"], "launched", "{a}");
    assert_eq!(admitted_routes(&w.d, &run, "a"), vec![(ids["alpha"].clone(), "sonnet".to_string(), "medium".to_string())]);
    let decisions = route_decisions(&w.d, &run, "a");
    assert_eq!(decisions.len(), 1);
    assert_eq!(exclusion_reasons(&decisions[0]).get("system-codex").map(String::as_str),
        Some("swarm_worker_launch_unsupported"), "{}", decisions[0]);
    let codex: i64 = db(&w.d).query_row("SELECT COUNT(*) FROM swarm_admissions WHERE run_id=?1
        AND target_profile_id='system-codex'", [&run], |r| r.get(0)).unwrap();
    assert_eq!(codex, 0);
    std::fs::write(&t.worker_gate, "open").unwrap();
}

/// SWARM-21 on the proposed native path: the owner's changed requirements hold
/// the native director's dispatches (`requirements_pending`) until it revises
/// the plan through its own `swarm_revise` tool, naming the request; the
/// revision is recorded against the request and dispatch resumes.
#[test]
fn a_native_director_applies_the_owners_requirement_change_by_revising() {
    let t = driven(&["alpha"], &[("alpha", 0.0)], &["system-claude", "alpha"], &["a", "b"], &["a"]);
    let w = &t.w;
    let (run, director, _) = start(w, "isolated", "native-requirements");
    wait_trace(w, step("dispatched", "director"), "plan", 60);
    w.d.call("swarm.requirements.change", json!({"run_id":run,"request_id":"owner-drop-b",
        "text":"Drop job b; only job a is needed"}));
    let held = dispatch_job(&w.d, &director, json!({"job_id":"a","brief":"Job a: a"}));
    assert_eq!((held["status"].as_str(), held["reason"].as_str()), (Some("blocked"), Some("requirements_pending")), "{held}");
    let revised = director_tool(&w.d, &director, "swarm_revise", json!({"reason":"The owner dropped b",
        "jobs":[{"id":"a","title":"Job a","acceptance":"evidence","deps":[]}],"requirements":["owner-drop-b"]}));
    assert_eq!(revised["is_error"], false, "{revised}");
    let state = w.d.call("swarm.get", json!({"id":run}));
    assert_eq!(state["requirement_changes"][0]["applied_revision"], 2, "{state}");
    assert_eq!(w.d.call("swarm.jobs", json!({"id":run,"status":"superseded"}))["jobs"].as_array().unwrap().len(), 1);
    let a = dispatch_job(&w.d, &director, json!({"job_id":"a","brief":"Job a: a"}));
    assert_ne!(a["reason"], "requirements_pending", "{a}");
    std::fs::write(&t.worker_gate, "open").unwrap();
}

/// SWARM-06 through Auto's route selection: the cheaper general-tier route is
/// refused for a job needing the frontier tier (`insufficient_capability`) and
/// the costlier qualified route is chosen; a job needing a tool no approved
/// route has is blocked visibly (`no_eligible_route`, each route's reason
/// recorded) and nothing weaker is admitted in its place.
#[test]
fn an_unqualified_cheap_route_is_refused_and_no_qualified_route_blocks_visibly() {
    let t = driven(&["alpha"], &[("alpha", 0.0)], &["system-claude", "alpha"], &["frontier", "tooling"], &["frontier"]);
    let (w, ids) = (&t.w, &t.ids);
    let (run, director, _) = start(w, "isolated", "capability-floor");
    wait_trace(w, step("dispatched", "director"), "plan", 60);
    let frontier = dispatch_job(&w.d, &director, json!({"job_id":"frontier","brief":"Job frontier",
        "requirements":{"min_tier":"frontier"}}));
    assert_eq!(frontier["status"], "launched", "{frontier}");
    assert_eq!(admitted_routes(&w.d, &run, "frontier"), vec![(ids["alpha"].clone(), "opus".to_string(), "high".to_string())]);
    let chosen = &route_decisions(&w.d, &run, "frontier")[0];
    assert_eq!(exclusion_reasons(chosen)[&format!("{}/sonnet/medium", ids["alpha"])], "insufficient_capability", "{chosen}");
    let blocked = dispatch_job(&w.d, &director, json!({"job_id":"tooling","brief":"Job tooling",
        "requirements":{"required_tools":["browser/navigate"]}}));
    assert_eq!((blocked["status"].as_str(), blocked["reason"].as_str()), (Some("blocked"), Some("no_eligible_route")), "{blocked}");
    let decision = &route_decisions(&w.d, &run, "tooling")[0];
    let reasons = exclusion_reasons(decision);
    for model in ["sonnet/medium", "opus/high"] {
        assert!(reasons.contains_key(&format!("{}/{model}", ids["alpha"])), "every route has its reason: {decision}");
    }
    assert!(admitted_routes(&w.d, &run, "tooling").is_empty(), "nothing weaker was admitted");
    let job: String = db(&w.d).query_row("SELECT status FROM swarm_jobs WHERE run_id=?1 AND id='tooling'", [&run], |r| r.get(0)).unwrap();
    assert_eq!(job, "ready");
    std::fs::write(&t.worker_gate, "open").unwrap();
}

/// SWARM-04 through Auto's route selection: each recorded route decision
/// replays to the same route and reason from its recorded input alone (plan
/// requirements, account readings and draws, bookings and exclusions as
/// recorded), also after a daemon restart. The daemon's own reasons (category
/// allocation, the account's own allowance, unresolved identity) and Auto's
/// eligibility reasons are both part of the recorded decision.
#[test]
fn each_route_decision_replays_to_the_same_route_and_reason() {
    let mut t = driven(&["alpha", "beta", "full", "unread"], &[("alpha", 0.0), ("beta", 0.0), ("full", 97.0)],
        &["system-claude", "alpha", "beta", "full", "unread"], &["a", "b", "c"], &["a", "b", "c"]);
    let (run, director, _) = start(&t.w, "isolated", "replay");
    wait_trace(&t.w, step("dispatched", "director"), "plan", 60);
    for (job, needs) in [("a", json!({})), ("b", json!({"min_tier":"frontier"})), ("c", json!({}))] {
        let r = dispatch_job(&t.w.d, &director, json!({"job_id":job,"brief":format!("Job {job}: {job}"),"requirements":needs}));
        assert_eq!(r["status"], "launched", "{r}");
    }
    let seqs: Vec<i64> = {
        let db = db(&t.w.d);
        let mut stmt = db.prepare("SELECT seq FROM events WHERE kind='swarm_route_decision'
            AND json_extract(payload,'$.swarm_run_id')=?1 ORDER BY seq").unwrap();
        let rows = stmt.query_map([&run], |r| r.get(0)).unwrap().map(Result::unwrap).collect();
        rows
    };
    assert_eq!(seqs.len(), 3);
    let replay_all = |d: &Daemon| -> Vec<Value> { seqs.iter().map(|seq| d.call("swarm.route.replay", json!({"event_seq":seq}))).collect() };
    let first = replay_all(&t.w.d);
    for replay in &first {
        assert_eq!(replay["matches"], true, "{replay}");
        assert_eq!(replay["selected"], replay["recorded_selected"]);
        assert_eq!(replay["auto_exclusions_recorded"], true);
    }
    std::fs::write(&t.worker_gate, "open").unwrap();
    t.w.d.kill9();
    t.w.d.spawn();
    assert_eq!(replay_all(&t.w.d), first, "the same answers after a restart");
    // A decision that does not follow from its input is caught: record job a
    // as having chosen the frontier route instead of the default.
    {
        let db = db(&t.w.d);
        let payload: String = db.query_row("SELECT payload FROM events WHERE seq=?1", [seqs[0]], |r| r.get(0)).unwrap();
        let mut payload: Value = serde_json::from_str(&payload).unwrap();
        let selected = payload["trace"]["decision"]["selected"].as_str().unwrap().replace("sonnet/medium", "opus/high");
        payload["trace"]["decision"]["selected"] = json!(selected);
        db.execute("UPDATE events SET payload=?2 WHERE seq=?1", rusqlite::params![seqs[0], payload.to_string()]).unwrap();
    }
    assert_eq!(t.w.d.call("swarm.route.replay", json!({"event_seq":seqs[0]}))["matches"], false);
}

/// SWARM-02 on Auto's route selection: accounts the owner never selected are
/// never candidates, for a first attempt or for fallback. (1) The approved
/// pool is two accounts; a third, healthy account discovered on the machine is
/// not in it. A worker that fails before any effect falls back to the other
/// approved account, never to the discovered one. (2) With only the
/// director's account approved and two healthy discovered accounts, a dispatch
/// is blocked `no_eligible_route`: the discovered accounts are not even
/// candidates, and nothing is admitted.
#[test]
fn unselected_accounts_are_never_candidates_for_work_or_fallback() {
    let t = driven(&["alpha", "beta", "gamma"], &[("alpha", 0.0), ("beta", 0.0), ("gamma", 0.0)],
        &["system-claude", "alpha", "beta"], &["x"], &[]);
    let ids = t.ids.clone();
    std::fs::write(&t.worker_gate, "open").unwrap();
    let (run, director, _) = start(&t.w, "isolated", "unselected-fallback");
    wait_trace(&t.w, step("dispatched", "director"), "plan", 60);
    let status = |job: &str| -> String { db(&t.w.d).query_row("SELECT status FROM swarm_jobs WHERE run_id=?1 AND id=?2",
        [&run, &job.to_string()], |r| r.get(0)).unwrap() };
    let one = dispatch_job(&t.w.d, &director, json!({"job_id":"x","brief":"Job x: x"}));
    assert_eq!(one["status"], "launched", "{one}");
    let until = std::time::Instant::now() + Duration::from_secs(60);
    while status("x") != "ready" { assert!(std::time::Instant::now() < until); std::thread::sleep(Duration::from_millis(100)); }
    let two = dispatch_job(&t.w.d, &director, json!({"job_id":"x","brief":"Job x: x"}));
    assert_eq!(two["status"], "launched", "{two}");
    let used: Vec<String> = admitted_routes(&t.w.d, &run, "x").into_iter().map(|r| r.0).collect();
    assert!(used.iter().all(|p| p == &ids["alpha"] || p == &ids["beta"]), "{used:?}");
    assert!(!used.contains(&ids["gamma"]), "the discovered account is never used");
    for trace in route_decisions(&t.w.d, &run, "x") {
        let candidates = trace["selection_input"]["routes"].as_array().unwrap();
        assert!(candidates.iter().all(|r| r["profile_id"] != ids["gamma"].as_str()), "{trace}");
    }

    // Only the director's own (unidentified) account approved.
    let only = driven(&["alpha", "beta"], &[("alpha", 0.0), ("beta", 0.0)], &["system-claude"], &["y"], &["y"]);
    let (run2, director2, _) = start(&only.w, "isolated", "only-denied");
    wait_trace(&only.w, step("dispatched", "director"), "plan", 60);
    let blocked = dispatch_job(&only.w.d, &director2, json!({"job_id":"y","brief":"Job y: y"}));
    assert_eq!((blocked["status"].as_str(), blocked["reason"].as_str()), (Some("blocked"), Some("no_eligible_route")), "{blocked}");
    let trace = &route_decisions(&only.w.d, &run2, "y")[0];
    let candidates: Vec<&str> = trace["selection_input"]["routes"].as_array().unwrap().iter()
        .filter_map(|r| r["profile_id"].as_str()).collect();
    assert!(candidates.iter().all(|p| *p == "system-claude"), "only approved accounts are candidates: {candidates:?}");
    assert!(admitted_routes(&only.w.d, &run2, "y").is_empty());
    std::fs::write(&only.worker_gate, "open").unwrap();
}

/// The director's own qualified draw (class `swarm/director`) with the
/// Claude calibration of 2026-09-28, on the product path with the switch at
/// its default (on). The identity read records the plan; seven serial Claude
/// runs on the account (sonnet, medium: the director's route) leave five
/// neighbour-bracketed samples; they are recorded as director runs of
/// earlier Swarm runs (the class is the only seeded fact; readings, runs and
/// bracketing are real). The confirmed start's director then books its
/// account on that draw instead of running unbooked, and a booked start on
/// the same account is no longer refused `account_pool_busy` while it runs.
#[test]
fn a_calibrated_director_books_its_account_with_its_own_draw() {
    let dir = tmp();
    let meter = dir.path().join("meter.json");
    let plan = dir.path().join("plan.txt");
    std::fs::write(&plan, "max").unwrap();
    let resets_at = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() + 3 * 3600;
    std::fs::write(&meter, json!({"used":0.10,"weekly":0.05,"resets_at":resets_at,
        "first":0.003,"last":0.008,"step":0.01}).to_string()).unwrap();
    let passthrough = "CLAUDE_FIXTURE_MODE,CLAUDE_FIXTURE_VERSION,CLAUDE_FIXTURE_HELP,CLAUDE_FIXTURE_SWARM_SCRIPT,\
        CLAUDE_FIXTURE_SWARM_WORKERS,CLAUDE_FIXTURE_METER_FILE,CLAUDE_FIXTURE_PLAN_FILE";
    let (meter_path, plan_path) = (meter.display().to_string(), plan.display().to_string());
    let w = world_in(dir, &[("OVERSEER_SWARM_FIXTURE_API", "0"), ("OVERSEER_SHARED_BOOKING_FIXTURE_API", "1"),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", passthrough), ("CLAUDE_FIXTURE_METER_FILE", meter_path.as_str()),
        ("CLAUDE_FIXTURE_PLAN_FILE", plan_path.as_str()), ("OVERSEER_TEST_DRAW_SETTLE_MS", "300")]);
    let identity = w.d.call("auto.quota.refresh", json!({"profile_id":"system-claude"}));
    assert_eq!(identity["plan"], "max", "{identity}");
    let mut earlier = Vec::new();
    for n in 0..7 {
        let run = w.d.call("task.create", json!({"repo":w.checkout,"harness":"claude","profile_id":"system-claude",
            "model":"sonnet","effort":"medium","prompt":format!("earlier {n}"),"title":format!("earlier {n}")}))
            ["run"]["id"].as_str().unwrap().to_string();
        assert_eq!(w.d.wait_done(&run, 30)["status"], "completed");
        std::thread::sleep(Duration::from_millis(400));
        earlier.push(run);
    }
    {
        // The seeded fact: these were the directors of seven earlier Swarm
        // runs (their run rows are not needed for the class, so the seeding
        // connection does not enforce the reference).
        let db = db(&w.d);
        db.execute_batch("PRAGMA foreign_keys=OFF").unwrap();
        for (n, run) in earlier.iter().enumerate() {
            db.execute("INSERT INTO swarm_director_owners(run_id,generation,token_sha256,status,created_ms,
                renewed_ms,lease_expires_ms,overseer_run_id,supervised_launch,launch_phase)
                VALUES(?1,1,?2,'released',0,0,0,?3,1,'linked')",
                rusqlite::params![format!("earlier-swarm-{n}"), format!("{n:064}"), run]).unwrap();
        }
    }
    w.d.call("agents.limit.set", json!({"max_active":5}));
    w.d.call("swarm.policy.set", json!({"scope":"application","allowed_targets":["system-claude"]}));
    std::fs::write(&w.script, json!({"trace":w.trace,"gate":w.gate,
        "jobs":[{"id":"a","title":"Tenant lookup","acceptance":"evidence","deps":[]}],
        "estimate":benefit(&["a"]),"dispatch":[],
        "complete":{"summary":"none","verification":"none"}}).to_string()).unwrap();
    let (_run, director, back) = start(&w, "isolated", "calibrated-director");
    assert_eq!((back["readback"]["director"]["model"].as_str(), back["readback"]["director"]["effort"].as_str()),
        (Some("sonnet"), Some("medium")), "{back}");
    assert_native_director(&w, &director);
    let (source, provenance): (String, String) = db(&w.d).query_row(
        "SELECT draw_source,draw_provenance FROM shared_booking_intents WHERE run_id=?1", [&director],
        |r| Ok((r.get(0)?, r.get(1)?))).expect("the director booked its account");
    assert_eq!(source, "qualified");
    let provenance: Value = serde_json::from_str(&provenance).unwrap();
    assert_eq!((provenance["bucket"]["task_class"].as_str(), provenance["bucket"]["model"].as_str(),
        provenance["sample_count"].as_i64(), provenance["plan_type"].as_str()),
        (Some("swarm/director"), Some("sonnet"), Some(5), Some("max")), "{provenance}");
    let launch = launch_file(&w.d, &director);
    let args: Vec<String> = serde_json::from_value(launch["args"].clone()).unwrap();
    assert!(args.windows(2).any(|a| a == ["--model", "sonnet"]) && args.windows(2).any(|a| a == ["--effort", "medium"]),
        "{args:?}");
    wait_trace(&w, step("dispatched", "director"), "the director's plan", 30);

    // The booked director's draw is committed, so the account is not busy
    // for another booked start (the finding of the default-off build).
    let db = db(&w.d);
    let (seq, generation): (i64, i64) = db.query_row(
        "SELECT o.event_seq,a.generation FROM auto_quota_observations o JOIN auto_account_identity a
         ON a.profile_id=o.pool_id WHERE o.pool_id='system-claude' ORDER BY o.observed_ms DESC,o.event_seq DESC LIMIT 1",
        [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
    let beside = w.d.call("task.create", json!({"repo":w.checkout,"harness":"claude","profile_id":"system-claude",
        "model":"sonnet","effort":"medium","prompt":"beside","title":"beside",
        "shared_booking":{"work_unit_id":"beside-director","account_generation":generation,
            "quota_event_seq":seq,"upper_draw_milli":[1_000, 1_000]}}));
    let beside = beside["run"]["id"].as_str().unwrap().to_string();
    let bound: i64 = db.query_row("SELECT COUNT(*) FROM shared_booking_intents WHERE run_id=?1", [&beside],
        |r| r.get(0)).unwrap();
    assert_eq!(bound, 1, "booked beside the director");
    w.d.wait_done(&beside, 30);
    end_run(&w.d, &director);
}
