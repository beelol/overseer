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
