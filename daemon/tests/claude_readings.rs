//! Claude Code's quota readings as the daemon records them (Auto Mode's
//! observation contract; the qualified upper draw's calibration source).
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

/// Six serial Claude runs on one account, each emitting the two native
/// `rate_limit_event`s a live turn emits (after its first model response and
/// before its result). The daemon records every one as a structured quota
/// observation of the run's profile. Each lies inside its own run's span, so
/// no run is bracketed by a reading taken before it began and one taken
/// after it ended; the readings report no plan and no explicit allowance.
/// A booked start on the qualified draw is refused, and an ordinary start
/// still runs unbooked.
#[test]
fn claude_readings_arrive_inside_runs_and_cannot_price_a_booking() {
    let temp = tmp();
    let checkout = repo(&temp.path().join("claude-readings"));
    let meter = temp.path().join("meter.json");
    let resets_at = now_ms() / 1000 + 3 * 3600;
    std::fs::write(&meter, json!({"used":0.10,"weekly":0.05,"resets_at":resets_at,
        "first":0.003,"last":0.008,"step":0.01}).to_string()).unwrap();
    let claude = repo_root().join("fixtures/fake-harness/claude-fixture.js").display().to_string();
    let d = Daemon::start(&[("OVERSEER_CLAUDE_PATH", claude.as_str()),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE,CLAUDE_FIXTURE_METER_FILE"),
        ("FIXTURE_MODE", "echo"), ("CLAUDE_FIXTURE_METER_FILE", meter.to_str().unwrap()),
        ("OVERSEER_TEST_DRAW_SETTLE_MS", "300")]);
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    db.busy_timeout(Duration::from_secs(10)).unwrap();
    // The account identity, as the identity read records it; it is not the
    // question here.
    db.execute("INSERT INTO auto_account_identity(profile_id,fingerprint,generation,observed_ms)
        VALUES('system-claude',?1,1,?2)", rusqlite::params!["c".repeat(64), now_ms()]).unwrap();

    let mut runs = Vec::new();
    for n in 0..6 {
        let run = d.call("task.create", json!({"repo":checkout,"harness":"claude",
            "profile_id":"system-claude","model":"sonnet","effort":"medium",
            "prompt":format!("sample {n}"),"title":format!("sample {n}")}))["run"]["id"]
            .as_str().unwrap().to_string();
        let ended = d.wait_status(&run, |status| !HOLDING.contains(&status), 30);
        assert_eq!(ended["status"], "completed", "{ended}");
        std::thread::sleep(Duration::from_millis(400));
        runs.push(run);
    }

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
            assert!(snapshot["ordinary_usage_allowed"].is_null(), "{snapshot}");
            assert_eq!(snapshot["windows"].as_array().unwrap().len(), 2, "{snapshot}");
            assert!(snapshot["windows"].as_array().unwrap().iter().all(|w| w["plan_type"].is_null()),
                "Claude's event reports no plan: {snapshot}");
        }
    }
    // No reading of the account exists outside the runs' spans.
    let outside: i64 = db.query_row("SELECT COUNT(*) FROM auto_quota_observations o
        WHERE o.pool_id='system-claude' AND NOT EXISTS(SELECT 1 FROM runs r
          WHERE r.profile_id='system-claude' AND o.observed_ms>r.created_ms AND o.observed_ms<=r.ended_ms)",
        [], |r| r.get(0)).unwrap();
    assert_eq!(outside, 0, "Claude has no reading between runs");

    // The latest reading is fresh (seconds old), yet it cannot book: it
    // reports no explicit allowance, and no run has a bracketing pair.
    let booked = d.try_call("task.create", json!({"repo":checkout,"harness":"claude",
        "profile_id":"system-claude","model":"sonnet","effort":"medium","prompt":"priced",
        "title":"priced","shared_booking":{"work_unit_id":"claude-priced","draw":"qualified"}}));
    let error = booked.unwrap_err();
    assert!(error.contains("account_allowance_unknown"), "{error}");
    // An ordinary start is unaffected: it falls back to an unbooked start.
    let plain = d.call("task.create", json!({"repo":checkout,"harness":"claude",
        "profile_id":"system-claude","model":"sonnet","effort":"medium","prompt":"plain",
        "title":"plain"}))["run"]["id"].as_str().unwrap().to_string();
    let booking: i64 = db.query_row("SELECT COUNT(*) FROM shared_booking_intents WHERE run_id=?1",
        [&plain], |r| r.get(0)).unwrap();
    assert_eq!(booking, 0);
    d.wait_status(&plain, |status| !HOLDING.contains(&status), 30);
}
