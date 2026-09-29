//! Claude Code's quota readings as the daemon records them, and the owner's
//! decisions of 2026-09-28 that make them a calibration source for the
//! qualified upper draw: the identity read's plan stands for the readings,
//! an `allowed` event is an explicit allowance, a run is bracketed by its
//! neighbours' readings, and a reading backs a booking for up to 15 minutes.
//! Synthetic Claude stream-json fixture and a synthetic account meter only:
//! no provider allowance is spent, read or measured.

mod common;
use common::*;
use serde_json::{json, Value};
use std::time::Duration;

const HOLDING: &[&str] = &["queued","starting","running","waiting_for_user",
    "waiting_for_connection","waiting_for_memory","unknown","disconnected"];

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64
}

/// Serial Claude runs on one account, each emitting the two native
/// `rate_limit_event`s a live turn emits (after its first model response and
/// before its result), after an identity read that reported plan `max`.
/// Every reading lies inside its own run, carries the identity read's plan
/// and an explicit allowance. With six runs, four are bracketed by their
/// neighbours and a booked start is refused (`upper_draw_unknown`); the
/// seventh run makes five, and a booked start is priced on the qualified
/// draw from the neighbours' readings. A new identity read with another plan
/// refuses the next booking (`plan_changed`); an ordinary start still runs.
#[test]
fn claude_neighbour_readings_price_a_booking_after_the_identity_read() {
    let temp = tmp();
    let checkout = repo(&temp.path().join("claude-readings"));
    let meter = temp.path().join("meter.json");
    let plan = temp.path().join("plan.txt");
    std::fs::write(&plan, "max").unwrap();
    let resets_at = now_ms() / 1000 + 3 * 3600;
    std::fs::write(&meter, json!({"used":0.10,"weekly":0.05,"resets_at":resets_at,
        "first":0.003,"last":0.008,"step":0.01}).to_string()).unwrap();
    let claude = repo_root().join("fixtures/fake-harness/claude-fixture.js").display().to_string();
    let d = Daemon::start(&[("OVERSEER_CLAUDE_PATH", claude.as_str()),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE,CLAUDE_FIXTURE_METER_FILE,CLAUDE_FIXTURE_PLAN_FILE"),
        ("FIXTURE_MODE", "echo"), ("CLAUDE_FIXTURE_METER_FILE", meter.to_str().unwrap()),
        ("CLAUDE_FIXTURE_PLAN_FILE", plan.to_str().unwrap()),
        ("OVERSEER_TEST_DRAW_SETTLE_MS", "300")]);
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    db.busy_timeout(Duration::from_secs(10)).unwrap();

    // The identity read (local, no model turn) records the account and its plan.
    let identity = d.call("auto.quota.refresh", json!({"profile_id":"system-claude"}));
    assert_eq!((identity["identity"].as_str(), identity["plan"].as_str()), (Some("recorded"), Some("max")), "{identity}");
    assert!(!identity.to_string().contains("fixture@example.test"), "{identity}");

    let run_one = |n: usize| -> String {
        let run = d.call("task.create", json!({"repo":checkout,"harness":"claude",
            "profile_id":"system-claude","model":"sonnet","effort":"medium",
            "prompt":format!("sample {n}"),"title":format!("sample {n}")}))["run"]["id"]
            .as_str().unwrap().to_string();
        let ended = d.wait_status(&run, |status| !HOLDING.contains(&status), 30);
        assert_eq!(ended["status"], "completed", "{ended}");
        std::thread::sleep(Duration::from_millis(400));
        run
    };
    let booked_start = |id: &str| d.try_call("task.create", json!({"repo":checkout,"harness":"claude",
        "profile_id":"system-claude","model":"sonnet","effort":"medium","prompt":"priced",
        "title":"priced","shared_booking":{"work_unit_id":id,"draw":"qualified"}}));

    let mut runs: Vec<String> = (0..6).map(run_one).collect();
    for run in &runs {
        let (created, ended): (i64, i64) = db.query_row(
            "SELECT created_ms,ended_ms FROM runs WHERE id=?1", [run], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        let readings: Vec<(i64, String, String)> = {
            let mut stmt = db.prepare("SELECT o.observed_ms,o.source,o.snapshot FROM auto_quota_observations o
                JOIN events e ON e.seq=o.event_seq WHERE e.run_id=?1 AND o.pool_id='system-claude'
                ORDER BY o.observed_ms").unwrap();
            let rows = stmt.query_map([run], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap()
                .map(Result::unwrap).collect();
            rows
        };
        assert_eq!(readings.len(), 2, "one reading after the first response, one before the result");
        for (observed, source, snapshot) in &readings {
            assert_eq!(source, "claude/native-rate-limit-event");
            assert!(*observed > created && *observed <= ended,
                "a Claude reading lies inside its run: {created} < {observed} <= {ended}");
            let snapshot: Value = serde_json::from_str(snapshot).unwrap();
            assert_eq!(snapshot["ordinary_usage_allowed"], true, "an allowed event is an allowance: {snapshot}");
            assert_eq!(snapshot["windows"].as_array().unwrap().len(), 2, "{snapshot}");
            assert!(snapshot["windows"].as_array().unwrap().iter().all(|w| w["plan_type"] == "max"),
                "the identity read's plan stands for the reading: {snapshot}");
        }
        let booked: i64 = db.query_row("SELECT COUNT(*) FROM shared_booking_intents WHERE run_id=?1",
            [run], |r| r.get(0)).unwrap();
        assert_eq!(booked, 0, "an uncalibrated ordinary start runs unbooked");
    }
    let outside: i64 = db.query_row("SELECT COUNT(*) FROM auto_quota_observations o
        WHERE o.pool_id='system-claude' AND NOT EXISTS(SELECT 1 FROM runs r
          WHERE r.profile_id='system-claude' AND o.observed_ms>r.created_ms AND o.observed_ms<=r.ended_ms)",
        [], |r| r.get(0)).unwrap();
    assert_eq!(outside, 0, "Claude has no reading between runs");

    // Six runs: runs 1-4 have both neighbours, four samples, below five.
    let refused = booked_start("claude-priced-early").unwrap_err();
    assert!(refused.contains("upper_draw_unknown"), "{refused}");

    // The seventh run gives run 5 its next neighbour: five samples.
    runs.push(run_one(6));
    let priced = booked_start("claude-priced").unwrap();
    let priced_run = priced["run"]["id"].as_str().unwrap().to_string();
    let (source, provenance): (String, String) = db.query_row(
        "SELECT draw_source,draw_provenance FROM shared_booking_intents WHERE work_unit_id='claude-priced'",
        [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
    assert_eq!(source, "qualified");
    let provenance: Value = serde_json::from_str(&provenance).unwrap();
    assert_eq!((provenance["sample_count"].as_i64(), provenance["plan_type"].as_str()), (Some(5), Some("max")),
        "{provenance}");
    let sampled: Vec<&str> = provenance["samples"].as_array().unwrap().iter()
        .map(|s| s["run_id"].as_str().unwrap()).collect();
    assert_eq!(sampled, runs[1..6].iter().rev().map(String::as_str).collect::<Vec<_>>());
    for (sample, n) in provenance["samples"].as_array().unwrap().iter().zip((1..6).rev()) {
        assert_eq!(sample["neighbours"], json!([runs[n - 1], runs[n + 1]]), "{sample}");
    }
    // Each sample moved 1.5 points (the run's point with its neighbours'
    // tail and head) plus two reading errors; never below the run's one point.
    let bound: Vec<i64> = provenance["windows"].as_array().unwrap().iter()
        .map(|w| w["upper_milli"].as_i64().unwrap()).collect();
    assert_eq!(bound, [3_500, 2_000], "{provenance}");
    d.wait_status(&priced_run, |status| !HOLDING.contains(&status), 30);

    // A new identity read reports another plan: the latest reading is of the
    // old plan and cannot back a booking; an ordinary start is unaffected.
    std::fs::write(&plan, "pro").unwrap();
    assert_eq!(d.call("auto.quota.refresh", json!({"profile_id":"system-claude"}))["plan"], "pro");
    let changed = booked_start("claude-after-plan-change").unwrap_err();
    assert!(changed.contains("plan_changed"), "{changed}");
    let plain = d.call("task.create", json!({"repo":checkout,"harness":"claude",
        "profile_id":"system-claude","model":"sonnet","effort":"medium","prompt":"plain",
        "title":"plain"}))["run"]["id"].as_str().unwrap().to_string();
    let booking: i64 = db.query_row("SELECT COUNT(*) FROM shared_booking_intents WHERE run_id=?1",
        [&plain], |r| r.get(0)).unwrap();
    assert_eq!(booking, 0);
    d.wait_status(&plain, |status| !HOLDING.contains(&status), 30);
}
