//! Shared launch booking bound to real runs and supervisors (Auto Mode's
//! shared account authority, handover step 2). Fixture harness and fixture
//! quota observations only: no provider allowance is spent or measured.

mod common;
use common::*;
use serde_json::{json, Value};
use std::path::Path;
use std::time::{Duration, Instant};

fn fixture(name: &str) -> String {
    repo_root()
        .join("fixtures")
        .join(name)
        .display()
        .to_string()
}

fn start_daemon(dir: &Path, extra: &[(&str, &str)]) -> Daemon {
    let accounts = dir.join("account-ids");
    std::fs::create_dir_all(&accounts).unwrap();
    let trace = dir.join("model-turns.txt");
    let effect = dir.join("external-effect.txt");
    let codex = fixture("fake-harness/codex-app-fixture.js");
    let mut env = vec![
        ("OVERSEER_CODEX_PATH", codex.as_str()),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH",
            "FIXTURE_MODE,FIXTURE_ACCOUNT_IDS_DIR,FIXTURE_TURN_DELAY_MS,FIXTURE_TRACE_FILE,FIXTURE_EXTERNAL_EFFECT_FILE"),
        ("FIXTURE_MODE", "managed-models"),
        ("FIXTURE_ACCOUNT_IDS_DIR", accounts.to_str().unwrap()),
        ("FIXTURE_TURN_DELAY_MS", "8000"),
        ("FIXTURE_TRACE_FILE", trace.to_str().unwrap()),
        ("FIXTURE_EXTERNAL_EFFECT_FILE", effect.to_str().unwrap()),
        ("OVERSEER_SHARED_BOOKING_FIXTURE_API", "1"),
    ];
    env.extend_from_slice(extra);
    Daemon::start(&env)
}

/// A profile on its own fixture account with a fresh structured observation
/// (one 5-hour window, 35% used). Returns (profile id, cited event seq).
fn account(d: &Daemon, dir: &Path, name: &str) -> (String, i64) {
    let profile = d.call("profile.create", json!({"name":name,"harness":"codex"}))["id"]
        .as_str()
        .unwrap()
        .to_string();
    std::fs::write(
        dir.join("account-ids").join(&profile),
        format!("account-{name}"),
    )
    .unwrap();
    let seq = refresh(d, &profile);
    (profile, seq)
}

fn refresh(d: &Daemon, profile: &str) -> i64 {
    d.call("auto.quota.refresh", json!({"profile_id":profile}));
    d.call(
        "auto.quota.state",
        json!({"profile_id":profile,"harness":"codex-app",
        "model":"gpt-6-sol"}),
    )["observation"]["event_seq"]
        .as_i64()
        .unwrap()
}

fn db(d: &Daemon) -> rusqlite::Connection {
    let conn = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    conn.busy_timeout(Duration::from_secs(5)).unwrap();
    conn
}

fn agent_limit(d: &Daemon, limit: i64) {
    db(d)
        .execute(
            "INSERT INTO meta(key,value) VALUES('agents.max_active',?1)
        ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            [limit.to_string()],
        )
        .unwrap();
}

fn booked(repo: &Path, profile: &str, id: &str, seq: i64, draw: i64, prompt: &str) -> Value {
    json!({"repo":repo,"harness":"codex-app","profile_id":profile,"model":"gpt-6-sol",
        "effort":"medium","prompt":prompt,"title":id,
        "shared_booking":{"work_unit_id":id,"account_generation":1,
            "quota_event_seq":seq,"upper_draw_milli":[draw]}})
}

/// (run_id, slot_held, writer_held, settled, outcome, claim state)
fn intent(d: &Daemon, id: &str) -> (Option<String>, bool, bool, bool, Option<String>, String) {
    db(d)
        .query_row(
            "SELECT i.run_id,i.slot_held,i.writer_held,i.settled_ms IS NOT NULL,i.outcome,c.state
        FROM shared_booking_intents i JOIN auto_pool_claims c ON c.work_unit_id=i.work_unit_id
        WHERE i.work_unit_id=?1",
            [id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            },
        )
        .unwrap()
}

fn model_turns(dir: &Path) -> usize {
    std::fs::read_to_string(dir.join("model-turns.txt"))
        .unwrap_or_default()
        .lines()
        .filter(|line| line.starts_with("turn_model:"))
        .count()
}

fn wait_for_turns(dir: &Path, count: usize) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while model_turns(dir) < count {
        assert!(Instant::now() < deadline, "model turns did not begin");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn booked_start_counts_its_slot_once_and_releases_holds_when_the_run_ends() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let d = start_daemon(r.path(), &[]);
    let (first, first_seq) = account(&d, r.path(), "first");
    let (second, second_seq) = account(&d, r.path(), "second");
    agent_limit(&d, 2);

    // Two clients send the same booked start at once: one books, claims and
    // launches; the other observes the same intent and returns its run.
    let request = booked(
        &repo,
        &first,
        "ordinary-first",
        first_seq,
        30_000,
        "hold parent",
    );
    let socket_calls: Vec<_> = (0..2)
        .map(|_| {
            let request = request.clone();
            let home = d.home.path().to_path_buf();
            std::thread::spawn(move || {
                let socket = std::process::Command::new(BIN)
                    .arg("socket-path")
                    .env("OVERSEER_HOME", &home)
                    .output()
                    .unwrap();
                let path = String::from_utf8(socket.stdout).unwrap();
                use std::io::{BufRead, BufReader, Write};
                let mut conn = std::os::unix::net::UnixStream::connect(path.trim()).unwrap();
                conn.set_read_timeout(Some(Duration::from_secs(60)))
                    .unwrap();
                conn.write_all(
                    format!(
                        "{}\n",
                        json!({"id":1,"method":"task.create","params":request})
                    )
                    .as_bytes(),
                )
                .unwrap();
                let mut line = String::new();
                BufReader::new(conn).read_line(&mut line).unwrap();
                serde_json::from_str::<Value>(&line).unwrap()
            })
        })
        .collect();
    let replies: Vec<Value> = socket_calls
        .into_iter()
        .map(|h| h.join().unwrap())
        .collect();
    let runs: Vec<String> = replies
        .iter()
        .map(|reply| {
            reply["result"]["run"]["id"]
                .as_str()
                .unwrap_or_else(|| panic!("{reply}"))
                .to_string()
        })
        .collect();
    assert_eq!(
        runs[0], runs[1],
        "concurrent requests share one launch: {replies:?}"
    );
    let first_run = runs[0].clone();
    assert_eq!(
        replies
            .iter()
            .filter(|reply| reply["result"]["replayed"] == true)
            .count(),
        1
    );
    d.wait_status(&first_run, |status| status == "running", 15);
    wait_for_turns(r.path(), 1);
    let (bound, slot, writer, settled, _, claim) = intent(&d, "ordinary-first");
    assert_eq!(bound.as_deref(), Some(first_run.as_str()));
    assert!(
        slot && !writer && !settled,
        "a worktree start holds a slot but no preexisting path"
    );
    assert_eq!(
        claim, "uncertain",
        "effects were claimed before the worktree and process"
    );

    // With a limit of two, a second booked start still fits: the running
    // run and its own booking hold are one slot, not two.
    let second_run = run_id(&d.call(
        "task.create",
        booked(
            &repo,
            &second,
            "ordinary-second",
            second_seq,
            30_000,
            "hold parent",
        ),
    ));
    let third = d.try_call(
        "task.create",
        booked(&repo, &first, "ordinary-third", first_seq, 30_000, "quick"),
    );
    assert!(
        third.as_ref().unwrap_err().contains("global_agent_limit"),
        "{third:?}"
    );
    assert_eq!(
        d.runs().len(),
        2,
        "a blocked booking has no run and no worktree"
    );

    let replay = d.call("task.create", request.clone());
    assert_eq!(run_id(&replay), first_run);
    assert_eq!(replay["replayed"], true);
    assert_eq!(d.runs().len(), 2);
    wait_for_turns(r.path(), 2);

    assert_eq!(d.wait_done(&first_run, 20)["status"], "completed");
    let (_, slot, writer, settled, outcome, claim) = intent(&d, "ordinary-first");
    assert!(
        !slot && !writer && settled,
        "the ended run releases its slot and writer"
    );
    assert_eq!(outcome.as_deref(), Some("settled"));
    assert_eq!(
        claim, "uncertain",
        "a process ran: its draw stays committed until a later observation"
    );
    // The account still carries the settled run's 30-point draw (35 + 30 + 40 > 100).
    let blocked = d.try_call(
        "task.create",
        booked(&repo, &first, "ordinary-fourth", first_seq, 40_000, "quick"),
    );
    assert!(
        blocked
            .as_ref()
            .unwrap_err()
            .contains("shared_pool_headroom"),
        "{blocked:?}"
    );
    let fresh = refresh(&d, &first);
    assert_eq!(
        intent(&d, "ordinary-first").5,
        "released",
        "a structured observation taken after settlement releases the retained draw"
    );
    let fourth = run_id(&d.call(
        "task.create",
        booked(
            &repo,
            &first,
            "ordinary-fourth-fresh",
            fresh,
            40_000,
            "quick",
        ),
    ));
    assert_eq!(d.wait_done(&fourth, 20)["status"], "completed");
    assert_eq!(d.wait_done(&second_run, 20)["status"], "completed");
    assert_eq!(
        model_turns(r.path()),
        3,
        "one model turn per admitted start"
    );
    let held: i64 = db(&d)
        .query_row(
            "SELECT COUNT(*) FROM shared_booking_intents
        WHERE slot_held=1 OR writer_held=1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(held, 0);
}

#[test]
fn restart_reattaches_a_running_booked_run_and_settles_one_whose_processes_are_gone() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let mut d = start_daemon(r.path(), &[]);
    let (first, first_seq) = account(&d, r.path(), "first");
    let (second, second_seq) = account(&d, r.path(), "second");
    let (third, third_seq) = account(&d, r.path(), "third");
    agent_limit(&d, 2);
    let running_request = booked(
        &repo,
        &first,
        "restart-running",
        first_seq,
        30_000,
        "fixture: external effect then wait",
    );
    let lost_request = booked(
        &repo,
        &second,
        "restart-lost",
        second_seq,
        30_000,
        "fixture: external effect then wait",
    );
    let running = run_id(&d.call("task.create", running_request.clone()));
    let lost = run_id(&d.call("task.create", lost_request.clone()));
    d.wait_status(&running, |status| status == "running", 15);
    d.wait_status(&lost, |status| status == "running", 15);
    wait_for_turns(r.path(), 2);

    let (running_process, _) = launch_info(&d, &running);
    let (lost_process, _) = launch_info(&d, &lost);
    d.kill9();
    signal(lost_process["child_pid"].as_i64().unwrap(), 9);
    signal(lost_process["shim_pid"].as_i64().unwrap(), 9);
    let deadline = Instant::now() + Duration::from_secs(5);
    while pid_alive(lost_process["child_pid"].as_i64().unwrap())
        || pid_alive(lost_process["shim_pid"].as_i64().unwrap())
    {
        assert!(
            Instant::now() < deadline,
            "lost run's processes did not exit"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    d.spawn();

    assert_eq!(
        d.run(&running)["status"],
        "running",
        "the live supervisor is reattached"
    );
    let (bound, slot, _, settled, _, claim) = intent(&d, "restart-running");
    assert_eq!(bound.as_deref(), Some(running.as_str()));
    assert!(
        slot && !settled,
        "a reattached run keeps its binding and its one slot"
    );
    assert_eq!(claim, "uncertain");
    assert_eq!(d.run(&lost)["status"], "disconnected");
    let (bound, slot, writer, settled, outcome, claim) = intent(&d, "restart-lost");
    assert_eq!(bound.as_deref(), Some(lost.as_str()));
    assert!(
        !slot && !writer && settled,
        "confirmed-gone processes release slot and writer"
    );
    assert_eq!(outcome.as_deref(), Some("settled"));
    assert_eq!(claim, "uncertain", "its effects may have spent allowance");

    // Reconnecting clients get the same runs; nothing is launched again.
    let replies: Vec<_> = [running_request.clone(), running_request, lost_request]
        .into_iter()
        .map(|request| {
            let socket = d.socket();
            std::thread::spawn(move || {
                use std::io::{BufRead, BufReader, Write};
                let mut conn = std::os::unix::net::UnixStream::connect(socket).unwrap();
                conn.set_read_timeout(Some(Duration::from_secs(60)))
                    .unwrap();
                conn.write_all(
                    format!(
                        "{}\n",
                        json!({"id":1,"method":"task.create","params":request})
                    )
                    .as_bytes(),
                )
                .unwrap();
                let mut line = String::new();
                BufReader::new(conn).read_line(&mut line).unwrap();
                serde_json::from_str::<Value>(&line).unwrap()
            })
        })
        .collect::<Vec<_>>()
        .into_iter()
        .map(|h| h.join().unwrap())
        .collect();
    assert_eq!(
        replies[0]["result"]["run"]["id"],
        running.as_str(),
        "{replies:?}"
    );
    assert_eq!(
        replies[1]["result"]["run"]["id"],
        running.as_str(),
        "{replies:?}"
    );
    assert_eq!(
        replies[2]["result"]["run"]["id"],
        lost.as_str(),
        "{replies:?}"
    );
    assert!(replies
        .iter()
        .all(|reply| reply["result"]["replayed"] == true));
    assert_eq!(d.runs().len(), 2);
    assert_eq!(d.run(&running)["process_generation"], 1);
    assert_eq!(
        model_turns(r.path()),
        2,
        "no second model request after restart or reconnect"
    );
    assert!(pid_alive(running_process["child_pid"].as_i64().unwrap()));

    // The running run is counted once and the lost run not at all: a third
    // booked start fits under a limit of two.
    let next = run_id(&d.call(
        "task.create",
        booked(&repo, &third, "restart-next", third_seq, 30_000, "quick"),
    ));
    assert_eq!(d.wait_done(&next, 20)["status"], "completed");
    d.call("run.interrupt", json!({"run_id":running}));
    d.wait_done(&running, 20);
    let (_, slot, writer, settled, _, _) = intent(&d, "restart-running");
    assert!(
        !slot && !writer && settled,
        "an interrupted run's exit releases its holds"
    );
}

#[test]
fn crash_before_the_effects_claim_releases_and_after_it_keeps_the_writer_without_retry() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let mut d = start_daemon(r.path(), &[]);
    let (first, seq) = account(&d, r.path(), "first");
    let current = |id: &str| {
        let mut request = booked(&repo, &first, id, seq, 10_000, "quick");
        request["workspace_mode"] = json!("current");
        request
    };
    let restart_with = |d: &mut Daemon, crash: Option<&str>| {
        d.kill9();
        d.env
            .retain(|(key, _)| key != "OVERSEER_TEST_SHARED_LAUNCH_CRASH");
        if let Some(point) = crash {
            d.env
                .push(("OVERSEER_TEST_SHARED_LAUNCH_CRASH".into(), point.into()));
        }
        d.spawn();
    };

    restart_with(&mut d, Some("after_book"));
    assert!(d
        .try_call("task.create", current("crash-after-book"))
        .is_err());
    restart_with(&mut d, None);
    let after_book = intent(&d, "crash-after-book");
    let (bound, slot, writer, _, outcome, claim) = after_book.clone();
    assert!(bound.is_none() && !slot && !writer, "{after_book:?}");
    assert_eq!(outcome.as_deref(), Some("released_unclaimed"));
    assert_eq!(claim, "released", "no effect was ever requested");
    let replay = d.try_call("task.create", current("crash-after-book"));
    assert!(replay.unwrap_err().contains("released before any effect"));
    assert!(d.runs().is_empty());

    restart_with(&mut d, Some("after_claim"));
    assert!(d
        .try_call("task.create", current("crash-after-claim"))
        .is_err());
    restart_with(&mut d, None);
    let (bound, slot, writer, _, outcome, claim) = intent(&d, "crash-after-claim");
    assert!(
        bound.is_none() && !slot && writer,
        "the checkout's writer stays held while the effect is unknown"
    );
    assert_eq!(outcome.as_deref(), Some("effects_uncertain"));
    assert_eq!(
        claim, "released",
        "no run was bound, so no model process could have run"
    );
    let replay = d.try_call("task.create", current("crash-after-claim"));
    assert!(replay.unwrap_err().contains("uncertain effects"));
    let manual = d.try_call(
        "task.create",
        json!({"repo":repo,"harness":"codex-app",
        "profile_id":first,"workspace_mode":"current","prompt":"quick"}),
    );
    assert!(
        manual
            .unwrap_err()
            .contains("held by shared launch crash-after-claim"),
        "an ordinary writer on the held checkout is refused"
    );
    assert!(d.runs().is_empty());
    assert_eq!(model_turns(r.path()), 0);
    let worktree = run_id(&d.call(
        "task.create",
        json!({"repo":repo,"harness":"codex-app",
        "profile_id":first,"prompt":"quick"}),
    ));
    assert_eq!(
        d.wait_done(&worktree, 20)["status"],
        "completed",
        "the account itself was released: separate work still runs"
    );
}

// ---------------------------------------------------------------- one app-slot authority

/// One request on its own socket connection; returns the raw reply
/// (`result` or `error`), so a refusal can be read without panicking.
fn raw_call(home: &Path, method: &str, params: Value) -> Value {
    use std::io::{BufRead, BufReader, Write};
    let socket = std::process::Command::new(BIN).arg("socket-path")
        .env("OVERSEER_HOME", home).output().unwrap();
    let path = String::from_utf8(socket.stdout).unwrap();
    let mut conn = std::os::unix::net::UnixStream::connect(path.trim()).unwrap();
    conn.set_read_timeout(Some(Duration::from_secs(120))).unwrap();
    conn.write_all(format!("{}\n", json!({"id":1,"method":method,"params":params})).as_bytes()).unwrap();
    let mut line = String::new();
    BufReader::new(conn).read_line(&mut line).unwrap();
    serde_json::from_str(&line).unwrap()
}

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64
}

fn fixture_local_snapshot(at: i64) -> Value {
    json!({"version":1,"observed_ms":at-1000,"expires_ms":at+120000,
        "targets":[{"id":"fixture-local","account_id":"fixture","pool_ids":["pool"],
            "capabilities":["code"],"health":"up","auth":"ok"}],
        "pools":[{"id":"pool","windows":[{"id":"run","unit":"points","remaining_milli":1000000,
            "protected_milli":0,"reserved_milli":0,"confidence":"exact","expires_ms":at+120000}]}]})
}

fn active_slots(d: &Daemon) -> i64 {
    d.call("agents.limit.get", json!({}))["active"].as_i64().unwrap()
}

const HOLDING: &[&str] = &["queued","starting","running","waiting_for_user",
    "waiting_for_connection","waiting_for_memory","unknown","disconnected"];

/// Handover step 3: one app-slot authority. An ordinary start, an Auto root,
/// a Swarm worker admission and a booked start race for the last slot of
/// `agents.max_active`: exactly one wins and every other path is refused for
/// the agent limit. After a daemon restart the winner still holds its slot
/// while its process or attempt lives, and it is released exactly once.
#[test]
fn ordinary_auto_swarm_and_booked_starts_race_for_the_last_slot_and_one_wins() {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let mut d = start_daemon(r.path(), &[]);
    let (booked_profile, booked_seq) = account(&d, r.path(), "booked");
    // A running category: its director slot and one admitted worker attempt.
    let swarm = d.call("swarm.create", json!({"category":"Slot race","objective":"Race for the last slot",
        "allowed_targets":["fixture-local"]}));
    let swarm_id = swarm["id"].as_str().unwrap().to_string();
    d.call("swarm.plan", json!({"id":swarm_id,"generation":1,"revision":0,"jobs":[
        {"id":"j0","title":"First","acceptance":"evidence","deps":[]},
        {"id":"j1","title":"Second","acceptance":"evidence","deps":[]}]}));
    commit_beneficial_batch(&d, &swarm_id, &["j0".into(), "j1".into()]);
    let at = now_ms();
    let admit = |job: &str, request: &str| json!({"run_id":swarm_id,"generation":1,"revision":1,
        "job_id":job,"target_id":"fixture-local","request_id":request,"snapshot":fixture_local_snapshot(at),
        "now_ms":at,"required_capabilities":["code"],"estimate_milli":{"points":1000},"purpose":"worker"});
    assert_eq!(d.call("swarm.admit", admit("j0", "race-j0"))["status"], "admitted");
    d.call("agents.limit.set", json!({"max_active":3}));
    assert_eq!(active_slots(&d), 2, "the category's director and its worker attempt");

    let requests = vec![
        ("ordinary", "task.create", json!({"repo":repo,"harness":"generic","workspace_mode":"worktree",
            "program":"/bin/sleep","args":["60"],"prompt":"","title":"ordinary racer"})),
        ("auto", "auto.start", json!({"work_unit_id":"race-root","repo":repo,"workspace_mode":"worktree",
            "prompt":"hold parent","title":"auto racer","allowed_profiles":["system-codex"],
            "min_tier":"general","required_tools":[],"sandbox":"read_only"})),
        ("swarm", "swarm.admit", admit("j1", "race-j1")),
        ("booked", "task.create", booked(&repo, &booked_profile, "race-booked", booked_seq, 10_000, "hold parent")),
    ];
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(requests.len()));
    let replies: Vec<(&str, Value)> = requests.into_iter().map(|(kind, method, params)| {
        let home = d.home.path().to_path_buf();
        let barrier = barrier.clone();
        (kind, std::thread::spawn(move || { barrier.wait(); raw_call(&home, method, params) }))
    }).collect::<Vec<_>>().into_iter().map(|(kind, h)| (kind, h.join().unwrap())).collect();
    let won = |kind: &str, reply: &Value| match kind {
        "swarm" => reply["result"]["status"] == "admitted",
        _ => reply["result"]["run"]["id"].is_string() && reply["result"]["launch_error"].is_null(),
    };
    let winners: Vec<&(&str, Value)> = replies.iter().filter(|(kind, reply)| won(kind, reply)).collect();
    assert_eq!(winners.len(), 1, "exactly one path takes the last slot: {replies:?}");
    for (kind, reply) in &replies {
        if won(kind, reply) { continue; }
        let text = reply.to_string();
        assert!(reply["error"]["code"] == "agent_limit" || text.contains("global_agent_limit"),
            "{kind} was refused for the agent limit, not something else: {reply}");
    }
    assert_eq!(active_slots(&d), 3);
    let (winner, reply) = (winners[0].0, winners[0].1.clone());
    eprintln!("race winner: {winner}");
    let winner_run = reply["result"]["run"]["id"].as_str().map(str::to_string);

    d.kill9();
    d.spawn();
    let holding = match &winner_run {
        Some(run) => HOLDING.contains(&d.run(run)["status"].as_str().unwrap()),
        None => true, // a Swarm attempt is durable until its job settles
    };
    assert_eq!(active_slots(&d), 2 + holding as i64, "{winner} after restart: {reply}");
    if holding {
        let late = raw_call(d.home.path(), "task.create", json!({"repo":repo,"harness":"generic",
            "workspace_mode":"worktree","program":"/bin/sleep","args":["1"],"prompt":"","title":"late"}));
        assert_eq!(late["error"]["code"], "agent_limit", "{winner} still holds the last slot: {late}");
    }
    if let Some(run) = winner_run {
        if HOLDING.contains(&d.run(&run)["status"].as_str().unwrap()) {
            let _ = d.try_call("run.interrupt", json!({"run_id":run}));
        }
        d.wait_status(&run, |status| !HOLDING.contains(&status), 30);
        assert_eq!(active_slots(&d), 2, "{winner}'s slot is released exactly once");
        let next = run_id(&d.call("task.create", json!({"repo":repo,"harness":"generic",
            "workspace_mode":"worktree","program":"/bin/sleep","args":["30"],"prompt":"","title":"next"})));
        assert_eq!(active_slots(&d), 3);
        d.call("run.interrupt", json!({"run_id":next}));
        d.wait_status(&next, |status| !HOLDING.contains(&status), 30);
    }
}

/// Handover step 3: Swarm admits an account (native) worker through the one
/// shared booking. The booking holds the worker's app slot (the attempt is
/// not counted again), draws the account windows in thousandths of a
/// reported percentage point, and is limited by the category's remaining
/// allocation in those same windows. The booking survives a restart with its
/// registered attempt; the worker's launch claims its effects before the
/// worktree and binds the run in the run's own commit; the run's end settles
/// it. The account headroom is shared with an ordinary booked start.
#[test]
fn swarm_worker_admission_books_the_shared_account_and_binds_its_run() {
    let fixture = repo_root().join("fixtures/fake-harness/claude-fixture.js").display().to_string();
    let mut d = Daemon::start(&[("OVERSEER_CLAUDE_PATH", &fixture),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE"), ("FIXTURE_MODE", "echo"),
        ("OVERSEER_SHARED_BOOKING_FIXTURE_API", "1")]);
    let temp = tmp();
    let checkout = repo(&temp.path().join("booked-workers"));
    let swarm = d.call("swarm.create", json!({"category":"Booked workers","objective":"Inspect backend",
        "allowed_targets":["claude-a"],"source_change_permission":"isolated"}));
    let id = swarm["id"].as_str().unwrap().to_string();
    d.call("swarm.plan", json!({"id":id,"generation":1,"revision":0,"jobs":[
        {"id":"j0","title":"First","acceptance":"evidence","deps":[]},
        {"id":"j1","title":"Second","acceptance":"evidence","deps":[]}]}));
    commit_beneficial_batch(&d, &id, &["j0".into(), "j1".into()]);
    // A fresh fixture observation: 0% used, so each window reports 100,000
    // thousandths of a point remaining; the category's allocation is 10% of
    // it (10,000) and its finishing reserve 20% of that (2,000).
    let booking = fixture_account_booking(&d, "system-claude", "swarm-account", 0.0, 5_000);
    let at = now_ms();
    let admit = |d: &Daemon, job: &str, request: &str, booking: Option<Value>| {
        let mut p = json!({"run_id":id,"generation":1,"revision":1,"job_id":job,"target_id":"claude-a",
            "request_id":request,"now_ms":at,"required_capabilities":["code"],
            "estimate_milli":{"points":100},"purpose":"worker",
            "snapshot":{"version":1,"observed_ms":at-1000,"expires_ms":at+120000,
                "targets":[{"id":"claude-a","harness":"claude","profile_id":"system-claude","model":"sonnet",
                    "account_id":"swarm-account","pool_ids":["pool"],"capabilities":["code"],
                    "health":"up","auth":"ok"}],
                "pools":[{"id":"pool","windows":[{"id":"run","unit":"points","remaining_milli":1000000,
                    "protected_milli":0,"reserved_milli":0,"confidence":"exact","expires_ms":at+120000}]}]}});
        if let Some(booking) = booking { p["shared_booking"] = booking; }
        d.call("swarm.admit", p)
    };
    let first = admit(&d, "j0", "booked-j0", Some(booking.clone()));
    assert_eq!(first["status"], "admitted", "{first}");
    let attempt = first["attempt_id"].as_str().unwrap().to_string();
    let booking_id = format!("swarm/{attempt}");
    assert_eq!(first["shared_booking"], booking_id.as_str());
    assert_eq!(first["booked_windows"], 2);
    assert_eq!(first["allocation_milli"], 10_000);
    let row = |d: &Daemon, key: &str| -> (String, String, bool, Option<String>, Option<i64>) {
        db(d).query_row("SELECT caller,phase,slot_held,run_id,effects_claimed_ms FROM shared_booking_intents
            WHERE work_unit_id=?1", [key], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))).unwrap()
    };
    assert_eq!(row(&d, &booking_id), ("swarm".into(), "booked".into(), true, None, None));
    let draws: Vec<i64> = {
        let db = db(&d);
        let mut stmt = db.prepare("SELECT amount_milli FROM shared_booking_windows WHERE work_unit_id=?1").unwrap();
        let rows = stmt.query_map([&booking_id], |r| r.get(0)).unwrap().map(Result::unwrap).collect();
        rows
    };
    assert_eq!(draws, vec![5_000, 5_000], "thousandths of a reported percentage point, per window");
    let swarm_reservations: i64 = db(&d).query_row("SELECT COUNT(*) FROM swarm_reservations WHERE attempt_id=?1",
        [&attempt], |r| r.get(0)).unwrap();
    assert_eq!(swarm_reservations, 0, "swarm_reservations is no account authority for a booked worker");
    assert_eq!(active_slots(&d), 2, "the director slot and the booking; the attempt is not counted twice");

    // No qualified upper draw, no account worker.
    let unbooked = admit(&d, "j1", "unbooked-j1", None);
    assert_eq!(unbooked["reason"], "upper_draw_unknown", "{unbooked}");
    // The category's remaining allocation limits the booking:
    // 10,000 - 5,000 booked - 2,000 finishing reserve = 3,000.
    let mut over = booking.clone();
    over["upper_draw_milli"] = json!([3_001, 3_001]);
    let over = admit(&d, "j1", "over-j1", Some(over));
    assert_eq!(over["reason"], "allocation_exhausted", "{over}");
    let mut fits = booking.clone();
    fits["upper_draw_milli"] = json!([3_000, 3_000]);
    let second = admit(&d, "j1", "fits-j1", Some(fits));
    assert_eq!(second["status"], "admitted", "{second}");
    let second_booking = format!("swarm/{}", second["attempt_id"].as_str().unwrap());
    assert_eq!(active_slots(&d), 3);

    // A restart keeps a registered attempt's booking (its dispatch can still
    // launch it) and holds the same slots.
    d.kill9();
    d.spawn();
    assert_eq!(row(&d, &booking_id).1, "booked");
    assert_eq!(row(&d, &second_booking).1, "booked");
    assert_eq!(active_slots(&d), 3);

    let launched = d.call("swarm.worker.launch", json!({"run_id":id,"job_id":"j0","attempt_id":attempt,
        "token":first["token"],"repo":checkout,"harness":"claude","args":[],
        "prompt":"Inspect","title":"Booked worker"}));
    assert_eq!(launched["status"], "launched", "{launched}");
    let worker = launched["overseer_run_id"].as_str().unwrap().to_string();
    let (_, phase, slot, bound, claimed) = row(&d, &booking_id);
    assert_eq!((phase.as_str(), slot, bound.as_deref()), ("uncertain", true, Some(worker.as_str())),
        "effects claimed before the worktree, the run bound in its own commit");
    assert!(claimed.is_some());
    assert_eq!(active_slots(&d), 3, "the bound worker run is its booking's slot, not another");
    d.wait_status(&worker, |status| !HOLDING.contains(&status), 30);
    let settled: (bool, Option<String>) = db(&d).query_row(
        "SELECT slot_held,outcome FROM shared_booking_intents WHERE work_unit_id=?1", [&booking_id],
        |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
    assert_eq!(settled, (false, Some("settled".into())), "the run's end settles its holds");
    // The ended worker's attempt holds one slot until Swarm settles it (by
    // then possibly already); never a second one beside its booking.
    let registered: bool = db(&d).query_row("SELECT status='registered' FROM swarm_attempts WHERE id=?1",
        [&attempt], |r| r.get(0)).unwrap();
    assert_eq!(active_slots(&d), 2 + registered as i64, "the attempt is counted once, not beside its booking");

    // The account headroom is one authority: an ordinary booked start on the
    // same account sees the Swarm draws (5,000 retained, 3,000 booked).
    let ordinary = d.try_call("task.create", json!({"repo":checkout,"harness":"claude",
        "profile_id":"system-claude","model":"sonnet","prompt":"x","title":"too big",
        "shared_booking":{"work_unit_id":"ordinary-too-big","account_generation":1,
            "quota_event_seq":booking["quota_event_seq"],"upper_draw_milli":[92_001, 92_001]}}));
    assert!(ordinary.as_ref().unwrap_err().contains("shared_pool_headroom"), "{ordinary:?}");
}
