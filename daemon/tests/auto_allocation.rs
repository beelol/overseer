//! Auto Mode continuous allocation through the real daemon: which route a
//! work unit gets when the account's known draw does or does not fit, and
//! what Auto's selector may choose at all (never a Swarm).

mod common;
use common::*;
use serde_json::{json, Value};
use std::path::Path;
use std::time::Duration;

fn fixture(name: &str) -> String {
    repo_root().join("fixtures").join(name).display().to_string()
}

fn refresh(d: &Daemon, profile: &str) {
    d.call("auto.quota.refresh", json!({"profile_id":profile}));
}

fn db(d: &Daemon) -> rusqlite::Connection {
    let conn = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    conn.busy_timeout(Duration::from_secs(5)).unwrap();
    conn
}

fn decision_event(d: &Daemon, parent: &str, unit: &str) -> Value {
    d.events(parent).into_iter().rev()
        .find(|event| event["kind"] == "auto_decision"
            && event["payload"]["decision"]["work_unit_id"] == unit)
        .unwrap_or_else(|| panic!("no decision recorded for {unit}"))
}

fn excluded(outcome: &Value, route: &str) -> Option<String> {
    outcome["decision"]["exclusions"].as_array().unwrap().iter()
        .find(|entry| entry["route_id"] == route)
        .map(|entry| entry["reason"].as_str().unwrap().to_string())
}

fn codex_world(dir: &Path) -> (Daemon, std::path::PathBuf, std::path::PathBuf) {
    let accounts = dir.join("account-ids");
    let meters = dir.join("quota-modes");
    // Absent until a test upgrades the harness (`codex-app-fixture.js`).
    let version_file = dir.join("codex-version");
    std::fs::create_dir_all(&accounts).unwrap();
    std::fs::create_dir_all(&meters).unwrap();
    let codex = fixture("fake-harness/codex-app-fixture.js");
    let d = Daemon::start(&[
        ("OVERSEER_CODEX_PATH", codex.as_str()),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE,FIXTURE_ACCOUNT_IDS_DIR,FIXTURE_QUOTA_MODES_DIR,FIXTURE_VERSION_FILE"),
        ("FIXTURE_VERSION_FILE", version_file.to_str().unwrap()),
        ("FIXTURE_MODE", "managed-models"),
        ("FIXTURE_ACCOUNT_IDS_DIR", accounts.to_str().unwrap()),
        ("FIXTURE_QUOTA_MODES_DIR", meters.to_str().unwrap()),
        ("OVERSEER_TEST_DRAW_SETTLE_MS", "300"),
    ]);
    d.call("auto.mode.set", json!({"enabled":true}));
    (d, accounts, meters)
}

/// AUTO-AC-13 and 16 through `auto.dispatch`: five isolated browser checks
/// calibrate Sol/medium on account A (one visible point each, so 3,000 per
/// window with the reading error). With room, that calibrated route fits and
/// is preferred over an uncalibrated account and booked on its known
/// windows. When A's meter leaves less room than the draw (less one reading
/// error), the same route is excluded with its reason and account B runs
/// instead; pinned to it, the unit pauses with no child. The decision
/// replays from its recorded inputs.
#[test]
fn auto_dispatch_excludes_a_calibrated_route_that_cannot_fit_and_books_one_that_does() {
    let r = tmp();
    let checkout = repo(&r.path().join("repo"));
    let (d, accounts, meters) = codex_world(r.path());
    let profile = |name: &str| -> String {
        let id = d.call("profile.create", json!({"name":name,"harness":"codex"}))["id"]
            .as_str().unwrap().to_string();
        std::fs::write(accounts.join(&id), format!("account-{name}")).unwrap();
        id
    };
    let (a, b) = (profile("alpha"), profile("beta"));
    let meter = |id: &str, used: f64| std::fs::write(meters.join(id), format!("{used}")).unwrap();
    meter(&b, 40.0);
    refresh(&d, &b);
    let parent = run_id(&d.call("task.create", json!({"repo":checkout,"harness":"codex-app",
        "model":"gpt-6-astra","effort":"high","prompt":"seed context","approval_policy":"never"})));
    assert_eq!(d.wait_done(&parent, 15)["status"], "completed");
    let a_sol = format!("{a}/gpt-6-sol/medium");
    let b_sol = format!("{b}/gpt-6-sol/medium");
    let dispatch = |unit: &str, allowed: Vec<&str>, pin: Option<&str>| {
        let mut request = json!({"work_unit_id":unit,"parent_run_id":parent,"min_tier":"general",
            "required_tools":[],"allowed_profiles":allowed,"task_class":"browser_check",
            "prompt":"check the page","title":"browser check"});
        if let Some(pin) = pin { request["pinned_route"] = json!(pin); }
        d.call("auto.dispatch", request)
    };

    // Calibrate: a reading, one isolated child, the meter moves one point,
    // a settled reading.
    let mut used = 30.0;
    meter(&a, used);
    refresh(&d, &a);
    for n in 0..5 {
        let child = dispatch(&format!("calibrate-{n}"), vec![a.as_str()], None);
        assert_eq!(child["state"], "dispatched", "{child}");
        assert_eq!(child["decision"]["selected"], a_sol, "{child}");
        assert_eq!(d.wait_done(&run_id(&child), 20)["status"], "completed");
        used += 1.0;
        meter(&a, used);
        std::thread::sleep(Duration::from_millis(400));
        refresh(&d, &a);
    }

    // Room on A: the calibrated route fits and wins over B's unknown draw,
    // and admission books its known windows.
    let fits = dispatch("room-1", vec![a.as_str(), b.as_str()], None);
    assert_eq!(fits["state"], "dispatched", "{fits}");
    assert_eq!(fits["decision"]["selected"], a_sol, "{fits}");
    let event = decision_event(&d, &parent, "room-1");
    let fit_of = |event: &Value, route: &str| event["payload"]["estimator"]["routes"].as_array().unwrap()
        .iter().find(|entry| entry["route_id"] == route).cloned().unwrap();
    assert_eq!(fit_of(&event, &a_sol)["fit"], "fits", "{event}");
    assert_eq!(fit_of(&event, &a_sol)["reason"], "qualified_draw_fits");
    assert_eq!(fit_of(&event, &b_sol)["fit"], "unknown");
    let (source, amounts): (String, String) = db(&d).query_row(
        "SELECT i.draw_source, (SELECT group_concat(amount_milli) FROM shared_booking_windows w
            WHERE w.work_unit_id=i.work_unit_id) FROM shared_booking_intents i WHERE i.work_unit_id='room-1'",
        [], |row| Ok((row.get(0)?, row.get(1)?))).unwrap();
    assert_eq!((source.as_str(), amounts.as_str()), ("qualified", "3000"));
    assert_eq!(d.wait_done(&run_id(&fits), 20)["status"], "completed");
    used += 1.0;
    meter(&a, used);
    std::thread::sleep(Duration::from_millis(400));

    // 97.5% used: 2,500 displayed, 1,500 after one reading error, less than
    // the 3,000 draw. The calibrated route is excluded; B runs instead.
    meter(&a, 97.5);
    refresh(&d, &a);
    let tight = dispatch("tight-2", vec![a.as_str(), b.as_str()], None);
    assert_eq!(tight["state"], "dispatched", "{tight}");
    assert_eq!(tight["decision"]["selected"], b_sol, "{tight}");
    assert_eq!(excluded(&tight, &a_sol).as_deref(), Some("estimated_draw_exceeds_allowance"), "{tight}");
    let event = decision_event(&d, &parent, "tight-2");
    assert_eq!(fit_of(&event, &a_sol)["reason"], "qualified_draw_exceeds_allowance");
    let replay = d.call("auto.decision.replay", json!({"event_seq":event["seq"]}));
    assert_eq!(replay["matches_recorded"], true, "{replay}");
    assert_eq!(replay["estimator_matches_recorded"], true, "{replay}");
    assert_eq!(d.wait_done(&run_id(&tight), 20)["status"], "completed");

    // Pinned to the route that cannot fit: pause, with the reason and the
    // owner's actions, and no child.
    let runs_before = d.runs().len();
    let pinned = dispatch("pinned-3", vec![a.as_str()], Some(&a_sol));
    assert_eq!(pinned["state"], "paused", "{pinned}");
    assert!(pinned["decision"]["selected"].is_null());
    assert_eq!(pinned["decision"]["reason"], "no_eligible_route");
    assert_eq!(excluded(&pinned, &a_sol).as_deref(), Some("estimated_draw_exceeds_allowance"));
    assert_eq!(pinned["actions"], json!(["refresh","choose_manual_route"]));
    assert_eq!(d.runs().len(), runs_before, "an unaffordable unit starts nothing");
}

/// The Swarm RFC keeps Swarm a separate switch ("One combined Auto mode" is
/// rejected: opting into route picking must not also authorize fan-out).
/// Auto's selector therefore never routes a unit to a Swarm: a Swarm-shaped
/// constraint is refused before any selection, its candidates are single
/// agents (a harness, account, model and effort), and a dispatched unit
/// creates no Swarm run or worker.
#[test]
fn auto_selection_has_no_swarm_route_and_refuses_swarm_constraints() {
    let r = tmp();
    let checkout = repo(&r.path().join("repo"));
    let (d, _, _) = codex_world(r.path());
    let parent = run_id(&d.call("task.create", json!({"repo":checkout,"harness":"codex-app",
        "model":"gpt-6-astra","effort":"high","prompt":"seed context","approval_policy":"never"})));
    assert_eq!(d.wait_done(&parent, 15)["status"], "completed");
    let base = json!({"work_unit_id":"broad-1","parent_run_id":parent,"min_tier":"general",
        "required_tools":[],"prompt":"split this objective across many agents","title":"broad"});
    for (field, value) in [("swarm", json!(true)), ("category", json!("backend")),
        ("objective", json!("finish the backlog")), ("max_workers", json!(8))] {
        let mut request = base.clone();
        request[field] = value;
        let error = d.try_call("auto.dispatch", request).unwrap_err();
        assert!(error.contains(&format!("unsupported automatic work constraint: {field}")), "{error}");
        let mut root = json!({"repo":checkout,"work_unit_id":"broad-root"});
        root[field] = json!(true);
        let error = d.try_call("auto.root.preview", root).unwrap_err();
        assert!(error.contains("unsupported automatic root preview field"), "{error}");
    }
    let outcome = d.call("auto.dispatch", base);
    assert_eq!(outcome["state"], "dispatched", "{outcome}");
    let event = decision_event(&d, &parent, "broad-1");
    for route in event["payload"]["selection_input"]["routes"].as_array().unwrap() {
        assert!(matches!(route["harness"].as_str(), Some("codex-app" | "claude" | "opencode")),
            "a candidate is one agent route: {route}");
        assert!(route["model"].is_string() && route["effort"].is_string() && route["profile_id"].is_string());
    }
    assert_eq!(d.wait_done(&run_id(&outcome), 20)["status"], "completed");
    let (runs, workers): (i64, i64) = db(&d).query_row(
        "SELECT (SELECT COUNT(*) FROM swarm_runs), (SELECT COUNT(*) FROM swarm_worker_launches)",
        [], |row| Ok((row.get(0)?, row.get(1)?))).unwrap();
    assert_eq!((runs, workers), (0, 0), "Auto routing created a Swarm");
}

/// AUTO-AC-16: with no eligible capable route inside the unit's bounds,
/// nothing launches. Each case (a route unavailable after a structured 503,
/// an exhausted account, a missing required tool, a capability tier the
/// pinned route lacks) pauses with its exclusion reason and the refresh and
/// manual-route actions; the decision is recorded without a chosen route
/// and no routing inference runs. Asking again with unchanged evidence
/// pauses again without a child; once the evidence changes, the same work
/// unit runs. (The credibly unaffordable case is in the calibrated test
/// above.)
#[test]
fn auto_no_suitable_route_pauses_with_reasons_and_actions_and_starts_nothing() {
    let r = tmp();
    let checkout = repo(&r.path().join("repo"));
    let (d, accounts, meters) = codex_world(r.path());
    let account = |name: &str, meter: &str| -> String {
        let id = d.call("profile.create", json!({"name":name,"harness":"codex"}))["id"]
            .as_str().unwrap().to_string();
        std::fs::write(accounts.join(&id), format!("account-{name}")).unwrap();
        std::fs::write(meters.join(&id), meter).unwrap();
        id
    };
    let spent = account("spent", "exhausted");
    let fresh = account("fresh", "available");
    let parent = run_id(&d.call("task.create", json!({"repo":checkout,"harness":"codex-app",
        "model":"gpt-6-astra","effort":"high","prompt":"seed context","approval_policy":"never"})));
    assert_eq!(d.wait_done(&parent, 15)["status"], "completed");
    let pause_twice = |unit: &str, extra: Value, route: &str, reason: &str| {
        let mut request = json!({"work_unit_id":unit,"parent_run_id":parent,"min_tier":"general",
            "required_tools":[],"prompt":"next unit","title":"next"});
        request.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        let runs_before = d.runs().len();
        for attempt in 0..2 {
            let outcome = d.call("auto.dispatch", request.clone());
            assert_eq!(outcome["state"], "paused", "{unit} attempt {attempt}: {outcome}");
            assert!(outcome["decision"]["selected"].is_null());
            assert_eq!(outcome["decision"]["reason"], "no_eligible_route", "{outcome}");
            assert_eq!(excluded(&outcome, route).as_deref(), Some(reason), "{unit}: {outcome}");
            assert_eq!(outcome["actions"], json!(["refresh","choose_manual_route"]));
        }
        let event = decision_event(&d, &parent, unit);
        assert!(event["payload"]["selected_route"].is_null());
        assert_eq!(event["payload"]["inference"]["state"], "not_used");
        assert_eq!(d.runs().len(), runs_before, "{unit}: a paused unit starts nothing");
    };
    let sol = |profile: &str| format!("{profile}/gpt-6-sol/medium");
    pause_twice("exhausted-1", json!({"allowed_profiles":[spent]}), &sol(&spent), "quota_exhausted");
    pause_twice("missing-tool-2", json!({"allowed_profiles":[fresh],
        "required_tools":["browser/screenshot"]}), &format!("{fresh}/gpt-6-astra/high"), "missing_tool");
    pause_twice("incapable-3", json!({"allowed_profiles":[fresh],"min_tier":"frontier",
        "pinned_route":sol(&fresh)}), &sol(&fresh), "insufficient_capability");
    // New evidence: the spent account reports room again, and the same
    // work unit now runs once.
    std::fs::write(meters.join(&spent), "available").unwrap();
    d.call("auto.quota.refresh", json!({"profile_id":spent}));
    let resumed = d.call("auto.dispatch", json!({"work_unit_id":"exhausted-1","parent_run_id":parent,
        "min_tier":"general","required_tools":[],"allowed_profiles":[spent],
        "prompt":"next unit","title":"next"}));
    assert_eq!(resumed["state"], "dispatched", "{resumed}");
    assert_eq!(d.wait_done(&run_id(&resumed), 20)["status"], "completed");
    // A structured 503 makes the Codex endpoint's routes unavailable for
    // its cooldown; a unit pinned to one pauses.
    let failing = run_id(&d.call("task.create", json!({"repo":checkout,"harness":"codex-app",
        "model":"gpt-6-sol","effort":"medium","prompt":"simulate direct 503","approval_policy":"never"})));
    assert_eq!(d.wait_done(&failing, 15)["status"], "failed");
    pause_twice("unavailable-4", json!({"allowed_profiles":[fresh],"pinned_route":sol(&fresh)}),
        &sol(&fresh), "route_unavailable");
}

/// AUTO-AC-20's shared recovery check across accounts: a structured 503
/// with a short Retry-After on one account marks the Codex endpoint (the
/// cooldown itself is covered in `protocol.rs`). Once the cooldown passes, two concurrent units on two different
/// accounts (so no account claim serializes them) do not both probe the
/// endpoint: one is admitted as the recovery check and the other pauses as
/// `endpoint_recovery_in_progress`. The check's success clears the endpoint,
/// and the next unit on the other account runs.
#[test]
fn auto_one_shared_recovery_check_per_endpoint_across_accounts() {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixStream;
    use std::sync::{Arc, Barrier};
    let r = tmp();
    let checkout = repo(&r.path().join("repo"));
    let accounts = r.path().join("account-ids");
    std::fs::create_dir_all(&accounts).unwrap();
    let codex = fixture("fake-harness/codex-app-fixture.js");
    let d = Daemon::start(&[("OVERSEER_CODEX_PATH", codex.as_str()),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE,FIXTURE_ACCOUNT_IDS_DIR,FIXTURE_TURN_DELAY_MS"),
        ("FIXTURE_MODE", "managed-models"), ("FIXTURE_ACCOUNT_IDS_DIR", accounts.to_str().unwrap()),
        ("FIXTURE_TURN_DELAY_MS", "3000")]);
    d.call("auto.mode.set", json!({"enabled":true}));
    let profile = |name: &str| -> String {
        let id = d.call("profile.create", json!({"name":name,"harness":"codex"}))["id"]
            .as_str().unwrap().to_string();
        std::fs::write(accounts.join(&id), format!("account-{name}")).unwrap();
        id
    };
    let (a, b) = (profile("alpha"), profile("beta"));
    let parent = run_id(&d.call("task.create", json!({"repo":checkout,"harness":"codex-app",
        "model":"gpt-6-astra","effort":"high","prompt":"seed context","approval_policy":"never"})));
    assert_eq!(d.wait_done(&parent, 20)["status"], "completed");
    let failed = run_id(&d.call("task.create", json!({"repo":checkout,"harness":"codex-app",
        "profile_id":a,"model":"gpt-6-sol","effort":"medium",
        "prompt":"simulate direct 503 with short Retry-After","approval_policy":"never"})));
    assert_eq!(d.wait_done(&failed, 20)["status"], "failed");
    std::thread::sleep(Duration::from_millis(400));

    let socket = d.socket();
    let barrier = Arc::new(Barrier::new(2));
    let results: Vec<Value> = [&a, &b].into_iter().enumerate().map(|(n, profile)| {
        let (socket, barrier) = (socket.clone(), barrier.clone());
        let params = json!({"work_unit_id":format!("recover-{n}"),"parent_run_id":parent,
            "min_tier":"general","required_tools":[],"allowed_profiles":[profile],
            "prompt":"browser check","title":"check"});
        std::thread::spawn(move || {
            barrier.wait();
            let mut conn = UnixStream::connect(socket).unwrap();
            conn.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
            conn.write_all(format!("{}\n", json!({"id":1,"method":"auto.dispatch","params":params})).as_bytes()).unwrap();
            let mut line = String::new();
            BufReader::new(conn).read_line(&mut line).unwrap();
            let reply: Value = serde_json::from_str(&line).unwrap();
            assert!(reply.get("error").is_none(), "{reply}");
            reply["result"].clone()
        })
    }).collect::<Vec<_>>().into_iter().map(|h| h.join().unwrap()).collect();
    let admitted: Vec<&Value> = results.iter().filter(|r| r["state"] != "paused").collect();
    assert_eq!(admitted.len(), 1, "one recovery check, not a stampede: {results:?}");
    let waiting = results.iter().find(|r| r["state"] == "paused").unwrap();
    assert!(waiting["decision"]["exclusions"].as_array().unwrap().iter()
        .any(|e| e["reason"] == "endpoint_recovery_in_progress"), "{waiting}");
    assert_eq!(d.wait_done(&run_id(admitted[0]), 20)["status"], "completed");
    let next = d.call("auto.dispatch", json!({"work_unit_id":"after-2","parent_run_id":parent,
        "min_tier":"general","required_tools":[],"allowed_profiles":[a, b],"prompt":"x","title":"x"}));
    assert_eq!(next["state"], "dispatched", "the check's success clears the endpoint: {next}");
    assert_eq!(d.wait_done(&run_id(&next), 20)["status"], "completed");
}

/// AUTO-AC-36 through `auto.dispatch`: measured history steers later work
/// with no routing file, does not churn, and is forgotten on an upgrade.
/// Five isolated checks calibrate Sol/medium on account A (as above).
/// - Three more units with the same kind of evidence (each moves A's meter
///   one point) all get A's Sol on its qualified draw: no churn.
/// - The harness is upgraded. The next unit still sees the old version as
///   the newest recorded one and is booked on the draw; its own run records
///   the new version. From then on the old samples are not comparable:
///   A's Sol has no qualified draw (fit unknown) and nothing is booked on it.
#[test]
fn auto_adapts_from_measured_draw_without_churn_and_forgets_it_on_upgrade() {
    let r = tmp();
    let checkout = repo(&r.path().join("repo"));
    let (d, accounts, meters) = codex_world(r.path());
    let profile = |name: &str| -> String {
        let id = d.call("profile.create", json!({"name":name,"harness":"codex"}))["id"]
            .as_str().unwrap().to_string();
        std::fs::write(accounts.join(&id), format!("account-{name}")).unwrap();
        id
    };
    let (a, b) = (profile("alpha"), profile("beta"));
    let meter = |id: &str, used: f64| std::fs::write(meters.join(id), format!("{used}")).unwrap();
    meter(&b, 40.0);
    refresh(&d, &b);
    let parent = run_id(&d.call("task.create", json!({"repo":checkout,"harness":"codex-app",
        "model":"gpt-6-astra","effort":"high","prompt":"seed context","approval_policy":"never"})));
    assert_eq!(d.wait_done(&parent, 15)["status"], "completed");
    let a_sol = format!("{a}/gpt-6-sol/medium");
    let dispatch = |unit: &str, allowed: Vec<&str>| d.call("auto.dispatch", json!({"work_unit_id":unit,
        "parent_run_id":parent,"min_tier":"general","required_tools":[],"allowed_profiles":allowed,
        "task_class":"browser_check","prompt":"check the page","title":"browser check"}));
    let mut used = 30.0;
    meter(&a, used);
    refresh(&d, &a);
    let mut run_unit = |unit: &str, allowed: Vec<&str>| -> Value {
        let out = dispatch(unit, allowed);
        assert_eq!(out["state"], "dispatched", "{out}");
        assert_eq!(d.wait_done(&run_id(&out), 20)["status"], "completed");
        used += 1.0;
        meter(&a, used);
        std::thread::sleep(Duration::from_millis(400));
        refresh(&d, &a);
        out
    };
    for n in 0..5 {
        let child = run_unit(&format!("calibrate-{n}"), vec![a.as_str()]);
        assert_eq!(child["decision"]["selected"], a_sol, "{child}");
    }
    let fit_of = |unit: &str| decision_event(&d, &parent, unit)["payload"]["estimator"]["routes"]
        .as_array().unwrap().iter().find(|entry| entry["route_id"] == a_sol).cloned().unwrap();
    let draw_source = |unit: &str| -> Option<String> { db(&d).query_row(
        "SELECT draw_source FROM shared_booking_intents WHERE work_unit_id=?1", [unit], |row| row.get(0)).ok() };

    for n in 0..3 {
        let unit = format!("steady-{n}");
        let out = run_unit(&unit, vec![a.as_str(), b.as_str()]);
        assert_eq!(out["decision"]["selected"], a_sol, "{unit}: {out}");
        assert_eq!(fit_of(&unit)["reason"], "qualified_draw_fits", "{unit}");
        assert_eq!(draw_source(&unit).as_deref(), Some("qualified"), "{unit}");
    }

    std::fs::write(r.path().join("codex-version"), "codex-app-fixture 0.1.0 (synthetic)").unwrap();
    let bridge = run_unit("upgrade-1", vec![a.as_str(), b.as_str()]);
    assert_eq!(bridge["decision"]["selected"], a_sol, "{bridge}");
    assert_eq!(draw_source("upgrade-1").as_deref(), Some("qualified"));
    let version: String = db(&d).query_row("SELECT harness_version FROM runs WHERE id=?1",
        [run_id(&bridge)], |row| row.get(0)).unwrap();
    assert_eq!(version, "codex-app-fixture 0.1.0 (synthetic)");
    let after = dispatch("upgrade-2", vec![a.as_str(), b.as_str()]);
    assert_eq!(after["state"], "dispatched", "{after}");
    assert_eq!(fit_of("upgrade-2")["fit"], "unknown", "{}", fit_of("upgrade-2"));
    assert_ne!(fit_of("upgrade-2")["reason"], "qualified_draw_fits");
    assert_ne!(draw_source("upgrade-2").as_deref(), Some("qualified"), "no booking on the old samples");
    assert_eq!(d.wait_done(&run_id(&after), 20)["status"], "completed");
}
