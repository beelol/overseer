//! The qualified upper draw (Auto Mode RFC, "Measuring model and reasoning
//! consumption"): a conservative bound, per quota window and in thousandths
//! of a reported percentage point, on what one more work unit of a bucket
//! draws from an account. It is learned only from that account's own
//! recorded structured quota observations around isolated completed runs.
//! Nothing is converted from tokens, estimated credits or prices; with too
//! few attributable samples the draw stays unknown and booking refuses.
//!
//! Attribution (one sample per run):
//! - the run and its native descendants have ended with their processes
//!   confirmed gone (`completed`, `failed`, `interrupted`), with no open turn;
//! - a structured observation of the run's own profile was taken at or
//!   before the run was created, and another at least
//!   [`REPORTING_SETTLE_MS`] after its last turn or descendant ended. A
//!   profile's observations are deleted when its account changes, so both
//!   readings are of the account the run used;
//! - no other run that may share the account (a profile with the same
//!   account fingerprint, or one whose identity is unknown) was active at
//!   any time between the two readings. External work on another machine
//!   cannot be excluded; it can only enlarge a sample, never shrink it;
//! - both readings report the cited plan, every cited window is present
//!   with the same scope and the same reset (no reset was crossed), and no
//!   meter decreased. A visible change of zero is kept as the interval
//!   [0, 2 x the reading error], never as free work.
//!
//! Each sample's upper draw in a window is its visible movement plus twice
//! [`READING_ERROR_MILLI`] (each reading may be rounded by up to one whole
//! point). The bound across at least [`MIN_SAMPLES`] samples is the larger
//! of their maximum and mean + [`NOISE_SIGMAS`] standard deviations.

use crate::auto_quota::{QuotaSnapshot, QuotaWindow};
use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const METHOD: &str = "isolated_run_window_delta_upper_v1";
/// The same minimum Auto's fit check requires of a predictive estimate.
pub const MIN_SAMPLES: usize = 5;
/// The newest attributable samples used for one bound.
pub const MAX_SAMPLES: usize = 50;
/// Candidate runs inspected per request, newest first (bounded work inside
/// an admission transaction).
const MAX_CANDIDATES: usize = 100;
/// Matches the retention of detailed learning and quota observations.
pub const MAX_SAMPLE_AGE_MS: i64 = 30 * 86_400_000;
/// An after-reading taken sooner than this after the work ended may not yet
/// include all of its draw.
pub const REPORTING_SETTLE_MS: i64 = 60_000;

/// Protocol tests may shorten (never lengthen past, never remove from the
/// product) the settle wait with `OVERSEER_TEST_DRAW_SETTLE_MS`; it changes
/// only how long a test waits for an after-reading, not any draw value.
fn reporting_settle_ms() -> i64 {
    std::env::var("OVERSEER_TEST_DRAW_SETTLE_MS")
        .ok()
        .and_then(|value| value.parse::<i64>().ok())
        .filter(|value| (0..=REPORTING_SETTLE_MS).contains(value))
        .unwrap_or(REPORTING_SETTLE_MS)
}
/// The largest error of one reading when the meter's rounding is unproven:
/// one whole reported percentage point.
pub const READING_ERROR_MILLI: i64 = 1_000;
pub const NOISE_SIGMAS: f64 = 3.0;
/// Ordinary agent work: manual starts, their follow-ups and Swarm workers.
pub const AGENT_CLASS: &str = "agent";

const MAX_DESCENDANTS: usize = 64;
const SETTLED_STATUSES: [&str; 3] = ["completed", "failed", "interrupted"];

/// Samples are comparable only within one bucket, on one account generation
/// and plan, for the newest recorded version of the harness.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DrawBucket {
    pub harness: String,
    pub model: String,
    pub effort: String,
    pub task_class: String,
}

impl DrawBucket {
    pub fn agent(harness: &str, model: Option<&str>, effort: Option<&str>) -> Self {
        Self {
            harness: harness.into(),
            model: model.unwrap_or_default().into(),
            effort: effort.unwrap_or_default().into(),
            task_class: AGENT_CLASS.into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SampleRef {
    pub run_id: String,
    /// Event sequence ids of the two structured observations.
    pub before_seq: i64,
    pub after_seq: i64,
    pub after_observed_ms: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WindowBound {
    pub bucket_id: String,
    pub window: String,
    pub max_milli: i64,
    pub mean_milli: f64,
    pub sd_milli: f64,
    pub upper_milli: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DrawProvenance {
    pub method: String,
    pub bucket: DrawBucket,
    pub account_pool: String,
    pub account_generation: i64,
    pub plan_type: String,
    pub harness_version: String,
    pub cited_quota_event_seq: i64,
    pub sample_count: usize,
    pub samples: Vec<SampleRef>,
    pub oldest_sample_ms: i64,
    pub newest_sample_ms: i64,
    /// Age of the newest sample when the bound was computed.
    pub age_ms: i64,
    pub windows: Vec<WindowBound>,
}

#[derive(Debug, PartialEq)]
pub enum UpperDraw {
    /// One amount per window of the cited observation, in its order.
    Qualified {
        upper_draw_milli: Vec<i64>,
        provenance: DrawProvenance,
    },
    Unknown {
        reason: &'static str,
        samples: usize,
        rejected: BTreeMap<&'static str, usize>,
    },
}

impl UpperDraw {
    fn unknown(reason: &'static str) -> Self {
        UpperDraw::Unknown { reason, samples: 0, rejected: BTreeMap::new() }
    }

    /// A content-free summary for a refusal or a preview.
    pub fn summary(&self) -> serde_json::Value {
        match self {
            UpperDraw::Qualified { upper_draw_milli, provenance } => serde_json::json!({
                "state":"qualified","method":provenance.method,
                "upper_draw_milli":upper_draw_milli,"samples":provenance.sample_count,
                "age_ms":provenance.age_ms}),
            UpperDraw::Unknown { reason, samples, rejected } => serde_json::json!({
                "state":"unknown","reason":reason,"samples":samples,
                "min_samples":MIN_SAMPLES,"rejected":rejected}),
        }
    }
}

/// A window of a later observation is the same meter as a cited one when
/// its scope, plan and duration match; the reset is compared per sample.
fn same_meter(left: &QuotaWindow, right: &QuotaWindow) -> bool {
    left.bucket_id == right.bucket_id
        && left.window == right.window
        && left.model == right.model
        && left.model_family == right.model_family
        && left.plan_type == right.plan_type
        && left.duration_mins == right.duration_mins
}

/// Attribute each cited window's movement between two observations of the
/// same account around one isolated piece of work (`start_ms` to `end_ms`).
/// Returns the sample's upper draw per cited window, or why it is not
/// attributable.
pub fn attribute_window_movement(
    before: &QuotaSnapshot,
    after: &QuotaSnapshot,
    cited: &QuotaSnapshot,
    plan: &str,
    start_ms: i64,
    end_ms: i64,
) -> std::result::Result<Vec<i64>, &'static str> {
    if before.observed_ms > start_ms
        || before.windows.iter().any(|window| window.observed_ms > start_ms)
    {
        return Err("before_reading_not_prior");
    }
    let settled = end_ms.saturating_add(reporting_settle_ms());
    if after.observed_ms < settled || after.windows.iter().any(|window| window.observed_ms < settled) {
        return Err("reporting_not_settled");
    }
    if before.native_uncertain_until_ms.is_some() || after.native_uncertain_until_ms.is_some() {
        return Err("native_order_uncertain");
    }
    if before.ordinary_usage_allowed == Some(false) || after.ordinary_usage_allowed == Some(false) {
        return Err("account_denied");
    }
    if before.reported_plan_type() != Some(plan) || after.reported_plan_type() != Some(plan) {
        return Err("plan_changed");
    }
    if cited.windows.is_empty() {
        return Err("window_scope_changed");
    }
    let mut draws = Vec::with_capacity(cited.windows.len());
    for window in &cited.windows {
        let earlier: Vec<_> = before.windows.iter().filter(|w| same_meter(w, window)).collect();
        let later: Vec<_> = after.windows.iter().filter(|w| same_meter(w, window)).collect();
        if earlier.len() != 1 || later.len() != 1 {
            return Err("window_scope_changed");
        }
        let (old, new) = (earlier[0], later[0]);
        let (Some(old_reset), Some(new_reset)) = (old.reset_ms, new.reset_ms) else {
            return Err("window_reset_unknown");
        };
        if old_reset != new_reset || old_reset <= after.observed_ms {
            return Err("window_reset_crossed");
        }
        if !old.used_percent.is_finite()
            || !new.used_percent.is_finite()
            || !(0.0..=100.0).contains(&old.used_percent)
            || !(0.0..=100.0).contains(&new.used_percent)
        {
            return Err("meter_invalid");
        }
        let visible = new.used_percent - old.used_percent;
        if visible < 0.0 {
            return Err("meter_decreased");
        }
        // Thousandths of a point, rounded up after removing float noise.
        let visible_milli = ((visible * 1_000_000.0).round() / 1_000.0).ceil() as i64;
        draws.push(
            visible_milli
                .saturating_add(2 * READING_ERROR_MILLI)
                .min(100_000),
        );
    }
    Ok(draws)
}

/// The bound for one window across samples: never below any sample, and
/// wider when the samples are noisy.
pub fn window_bound(values: &[i64]) -> Option<(i64, f64, f64, i64)> {
    if values.is_empty() {
        return None;
    }
    let n = values.len() as f64;
    let max = *values.iter().max()?;
    let mean = values.iter().map(|v| *v as f64).sum::<f64>() / n;
    let sd = if values.len() > 1 {
        (values.iter().map(|v| (*v as f64 - mean).powi(2)).sum::<f64>() / (n - 1.0)).sqrt()
    } else {
        0.0
    };
    let noisy = (mean + NOISE_SIGMAS * sd - 1e-6).ceil() as i64;
    Some((max, mean, sd, max.max(noisy).min(100_000)))
}

fn load_snapshot(encoded: &str) -> Option<QuotaSnapshot> {
    serde_json::from_str(encoded).ok()
}

/// The comparability class a run's work was recorded under.
fn run_task_class(conn: &Connection, run_id: &str) -> Result<String> {
    let bound: Option<Option<String>> = conn
        .query_row(
            "SELECT draw_provenance FROM shared_booking_intents WHERE run_id=?1",
            [run_id],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(provenance) = bound {
        // A booking priced from samples records its class; a fixture-priced
        // booking was an ordinary agent start.
        return Ok(provenance
            .and_then(|encoded| serde_json::from_str::<DrawProvenance>(&encoded).ok())
            .map(|p| p.bucket.task_class)
            .unwrap_or_else(|| AGENT_CLASS.into()));
    }
    let overseer: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM run_roles WHERE run_id=?1 AND role='overseer')",
        [run_id],
        |row| row.get(0),
    )?;
    if overseer {
        return Ok("overseer".into());
    }
    let director: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM swarm_director_owners WHERE overseer_run_id=?1)",
        [run_id],
        |row| row.get(0),
    )?;
    if director {
        return Ok("swarm/director".into());
    }
    let auto: Option<Option<i64>> = conn
        .query_row(
            "SELECT i.decision_event_seq FROM managed_work_units m
             JOIN auto_launch_intents i ON i.work_unit_id=m.work_unit_id WHERE m.child_run_id=?1
             UNION ALL
             SELECT decision_event_seq FROM auto_root_intents WHERE run_id=?1 LIMIT 1",
            [run_id],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(seq) = auto {
        let class = match seq {
            Some(seq) => conn
                .query_row("SELECT payload FROM events WHERE seq=?1", [seq], |row| {
                    row.get::<_, String>(0)
                })
                .optional()?
                .and_then(|payload| serde_json::from_str::<serde_json::Value>(&payload).ok())
                .and_then(|payload| {
                    payload["selection_input"]["work"]["task_class"]
                        .as_str()
                        .map(str::to_string)
                }),
            None => None,
        };
        return Ok(format!("auto/{}", class.as_deref().unwrap_or("unclassified")));
    }
    Ok(AGENT_CLASS.into())
}

const WORK_SET: &str = "WITH RECURSIVE w(id) AS (SELECT ?1 UNION
    SELECT r.id FROM runs r JOIN w ON r.parent_run_id=w.id
    WHERE COALESCE(r.relation_source,'')<>'managed-delegation')";

/// Compute the qualified upper draw for a booking on `quota_profile_id`'s
/// account that cites observation `quota_event_seq`. Call inside the
/// admission transaction, so the samples and the cited reading agree.
pub fn qualified_upper_draw_in_tx(
    conn: &Connection,
    quota_profile_id: &str,
    quota_event_seq: i64,
    bucket: &DrawBucket,
    now_ms: i64,
) -> Result<UpperDraw> {
    let valid = |value: &str| !value.is_empty() && value.len() <= 128 && !value.chars().any(char::is_control);
    if !valid(&bucket.harness) || !valid(&bucket.task_class) {
        return Ok(UpperDraw::unknown("bucket_incomplete"));
    }
    if !valid(&bucket.model) {
        return Ok(UpperDraw::unknown("model_unknown"));
    }
    if !valid(&bucket.effort) {
        return Ok(UpperDraw::unknown("effort_unknown"));
    }
    let identity: Option<(String, i64)> = conn
        .query_row(
            "SELECT fingerprint,generation FROM auto_account_identity WHERE profile_id=?1",
            [quota_profile_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let Some((fingerprint, generation)) = identity else {
        return Ok(UpperDraw::unknown("account_identity_unknown"));
    };
    let cited: Option<String> = conn
        .query_row(
            "SELECT snapshot FROM auto_quota_observations WHERE event_seq=?1 AND pool_id=?2",
            params![quota_event_seq, quota_profile_id],
            |row| row.get(0),
        )
        .optional()?;
    let Some(cited) = cited.as_deref().and_then(load_snapshot) else {
        return Ok(UpperDraw::unknown("quota_unknown"));
    };
    if cited.windows.is_empty() || cited.windows.len() > 32 {
        return Ok(UpperDraw::unknown("quota_unknown"));
    }
    let Some(plan) = cited.reported_plan_type().map(str::to_string) else {
        return Ok(UpperDraw::unknown("plan_unknown"));
    };
    // A harness upgrade starts a new bucket: only samples recorded under the
    // newest version seen for this harness are comparable.
    let version: Option<String> = conn
        .query_row(
            "SELECT harness_version FROM runs WHERE harness=?1
             AND harness_version IS NOT NULL AND harness_version<>''
             ORDER BY created_ms DESC,rowid DESC LIMIT 1",
            [&bucket.harness],
            |row| row.get(0),
        )
        .optional()?;
    let Some(version) = version else {
        return Ok(UpperDraw::unknown("harness_version_unknown"));
    };
    let candidates: Vec<(String, String, i64, Option<String>)> = {
        let mut stmt = conn.prepare(
            "SELECT r.id,r.profile_id,r.created_ms,r.harness_version FROM runs r
             JOIN auto_account_identity a ON a.profile_id=r.profile_id
             WHERE a.fingerprint=?1 AND r.harness=?2 AND r.model=?3 AND r.effort=?4
               AND r.status IN ('completed','failed','interrupted') AND r.ended_ms IS NOT NULL
               AND r.ended_ms>?5 AND r.ended_ms<=?6
               AND (r.parent_run_id IS NULL OR r.relation_source='managed-delegation')
             ORDER BY r.ended_ms DESC,r.id DESC LIMIT ?7",
        )?;
        let rows = stmt.query_map(
            params![
                fingerprint,
                bucket.harness,
                bucket.model,
                bucket.effort,
                now_ms.saturating_sub(MAX_SAMPLE_AGE_MS),
                now_ms,
                MAX_CANDIDATES as i64
            ],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )?;
        rows.collect::<rusqlite::Result<_>>()?
    };
    let mut rejected = BTreeMap::<&'static str, usize>::new();
    let mut samples: Vec<(SampleRef, Vec<i64>)> = Vec::new();
    for (run_id, profile_id, created_ms, run_version) in candidates {
        if samples.len() >= MAX_SAMPLES {
            break;
        }
        let verdict = attribute_run(
            conn, &run_id, &profile_id, created_ms, run_version.as_deref(), &version,
            &fingerprint, bucket, &cited, &plan, now_ms,
        )?;
        match verdict {
            Ok(sample) => samples.push(sample),
            Err(reason) => *rejected.entry(reason).or_default() += 1,
        }
    }
    if samples.len() < MIN_SAMPLES {
        return Ok(UpperDraw::Unknown {
            reason: "upper_draw_uncalibrated",
            samples: samples.len(),
            rejected,
        });
    }
    let mut upper_draw_milli = Vec::with_capacity(cited.windows.len());
    let mut windows = Vec::with_capacity(cited.windows.len());
    for (n, window) in cited.windows.iter().enumerate() {
        let values: Vec<i64> = samples.iter().map(|(_, draws)| draws[n]).collect();
        let Some((max, mean, sd, upper)) = window_bound(&values) else {
            return Ok(UpperDraw::unknown("upper_draw_uncalibrated"));
        };
        upper_draw_milli.push(upper);
        windows.push(WindowBound {
            bucket_id: window.bucket_id.clone(),
            window: window.window.clone(),
            max_milli: max,
            mean_milli: mean,
            sd_milli: sd,
            upper_milli: upper,
        });
    }
    let newest = samples.iter().map(|(s, _)| s.after_observed_ms).max().unwrap_or(now_ms);
    let oldest = samples.iter().map(|(s, _)| s.after_observed_ms).min().unwrap_or(now_ms);
    Ok(UpperDraw::Qualified {
        upper_draw_milli,
        provenance: DrawProvenance {
            method: METHOD.into(),
            bucket: bucket.clone(),
            account_pool: format!("account/{fingerprint}"),
            account_generation: generation,
            plan_type: plan,
            harness_version: version,
            cited_quota_event_seq: quota_event_seq,
            sample_count: samples.len(),
            samples: samples.into_iter().map(|(sample, _)| sample).collect(),
            oldest_sample_ms: oldest,
            newest_sample_ms: newest,
            age_ms: now_ms.saturating_sub(newest),
            windows,
        },
    })
}

#[allow(clippy::too_many_arguments)]
fn attribute_run(
    conn: &Connection,
    run_id: &str,
    profile_id: &str,
    created_ms: i64,
    run_version: Option<&str>,
    version: &str,
    fingerprint: &str,
    bucket: &DrawBucket,
    cited: &QuotaSnapshot,
    plan: &str,
    now_ms: i64,
) -> Result<std::result::Result<(SampleRef, Vec<i64>), &'static str>> {
    if run_version != Some(version) {
        return Ok(Err("harness_version_changed"));
    }
    if run_task_class(conn, run_id)? != bucket.task_class {
        return Ok(Err("task_class_differs"));
    }
    // The run and its native descendants are one inclusive piece of work.
    let work: Vec<(String, String, Option<i64>)> = {
        let mut stmt = conn.prepare(&format!(
            "{WORK_SET} SELECT r.id,r.status,r.ended_ms FROM runs r JOIN w ON w.id=r.id LIMIT ?2"
        ))?;
        let rows = stmt.query_map(params![run_id, MAX_DESCENDANTS as i64 + 2], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })?;
        rows.collect::<rusqlite::Result<_>>()?
    };
    if work.len() > MAX_DESCENDANTS + 1 {
        return Ok(Err("too_many_descendants"));
    }
    if work
        .iter()
        .any(|(_, status, ended)| !SETTLED_STATUSES.contains(&status.as_str()) || ended.is_none())
    {
        return Ok(Err("work_not_settled"));
    }
    let (open_turns, last_turn): (i64, Option<i64>) = conn.query_row(
        &format!(
            "{WORK_SET} SELECT COALESCE(SUM(t.ended_ms IS NULL),0),MAX(t.ended_ms)
             FROM turns t JOIN w ON w.id=t.run_id"
        ),
        [run_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    if open_turns > 0 {
        return Ok(Err("work_not_settled"));
    }
    let end_ms = work
        .iter()
        .filter_map(|(_, _, ended)| *ended)
        .chain(last_turn)
        .max()
        .unwrap_or(created_ms);
    let reading = |sql: &str, at: i64| -> Result<Option<(i64, i64, String)>> {
        Ok(conn
            .query_row(sql, params![profile_id, at, now_ms], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })
            .optional()?)
    };
    let Some((before_seq, before_ms, before)) = reading(
        "SELECT event_seq,observed_ms,snapshot FROM auto_quota_observations
         WHERE pool_id=?1 AND observed_ms<=?2 AND observed_ms<=?3
         ORDER BY observed_ms DESC,event_seq DESC LIMIT 1",
        created_ms,
    )?
    else {
        return Ok(Err("before_reading_missing"));
    };
    let Some((after_seq, after_ms, after)) = reading(
        "SELECT event_seq,observed_ms,snapshot FROM auto_quota_observations
         WHERE pool_id=?1 AND observed_ms>=?2 AND observed_ms<=?3
         ORDER BY observed_ms ASC,event_seq ASC LIMIT 1",
        end_ms.saturating_add(reporting_settle_ms()),
    )?
    else {
        return Ok(Err("after_reading_missing"));
    };
    // Any other run that may share this account and was active between the
    // two readings makes the movement ambiguous.
    let overlap: bool = conn.query_row(
        &format!(
            "{WORK_SET} SELECT EXISTS(SELECT 1 FROM runs o
             LEFT JOIN auto_account_identity a ON a.profile_id=o.profile_id
             WHERE o.id NOT IN (SELECT id FROM w) AND o.harness<>'generic'
               AND (a.fingerprint IS NULL OR a.fingerprint=?2)
               AND o.created_ms<=?4
               AND (o.ended_ms IS NULL OR o.ended_ms>=?3
                    OR o.status NOT IN ('completed','failed','interrupted')
                    OR EXISTS(SELECT 1 FROM turns t WHERE t.run_id=o.id
                        AND (t.ended_ms IS NULL OR t.ended_ms>=?3))))"
        ),
        params![run_id, fingerprint, before_ms, after_ms],
        |row| row.get(0),
    )?;
    if overlap {
        return Ok(Err("overlapping_work"));
    }
    let (Some(before), Some(after)) = (load_snapshot(&before), load_snapshot(&after)) else {
        return Ok(Err("meter_invalid"));
    };
    Ok(
        attribute_window_movement(&before, &after, cited, plan, created_ms, end_ms).map(|draws| {
            (
                SampleRef {
                    run_id: run_id.into(),
                    before_seq,
                    after_seq,
                    after_observed_ms: after_ms,
                },
                draws,
            )
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;
    use std::path::Path;

    const PROFILE: &str = "codex-a";
    const MINUTE: i64 = 60_000;

    struct Fixture {
        store: Store,
        base: i64,
        reset: i64,
    }

    impl Fixture {
        fn new() -> Self {
            let store = Store::open(Path::new(":memory:")).unwrap();
            store.record_auto_account_identity(PROFILE, &"a".repeat(64)).unwrap();
            let base = crate::daemon::now() - 20 * 60 * MINUTE;
            Self { store, base, reset: base + 48 * 60 * MINUTE }
        }

        fn at(&self, minutes: i64) -> i64 {
            self.base + minutes * MINUTE
        }

        /// A structured observation through the real recording path.
        fn observe_on(&self, profile: &str, minutes: i64, used: [f64; 2], plan: &str) -> i64 {
            self.observe_at(profile, self.at(minutes), used, plan)
        }

        fn observe_at(&self, profile: &str, at: i64, used: [f64; 2], plan: &str) -> i64 {
            let snapshot = QuotaSnapshot {
                ordinary_usage_allowed: Some(true),
                observed_ms: at,
                expires_ms: at + MINUTE,
                native_uncertain_until_ms: None,
                windows: [("five_hour", 300), ("weekly", 10_080)]
                    .into_iter()
                    .zip(used)
                    .map(|((name, minutes), used_percent)| QuotaWindow {
                        pool_id: profile.into(),
                        bucket_id: name.into(),
                        window: name.into(),
                        model: None,
                        model_family: None,
                        plan_type: Some(plan.into()),
                        used_percent,
                        reset_ms: Some(self.reset),
                        duration_mins: Some(minutes),
                        observed_ms: at,
                        expires_ms: at + MINUTE,
                    })
                    .collect(),
            };
            let event = self
                .store
                .insert_event(at, None, None, "quota", "fixture", "reported",
                    &serde_json::json!({"profile_id":profile}))
                .unwrap();
            assert!(self
                .store
                .insert_auto_quota(event.seq, profile, "fixture/structured", &snapshot)
                .unwrap());
            event.seq
        }

        fn observe(&self, minutes: i64, used: [f64; 2]) -> i64 {
            self.observe_on(PROFILE, minutes, used, "pro")
        }

        #[allow(clippy::too_many_arguments)]
        fn run_full(&self, id: &str, profile: Option<&str>, harness: &str, model: &str,
            effort: Option<&str>, version: &str, from: i64, to: Option<i64>, status: &str) {
            let conn = &self.store.conn;
            conn.execute(
                "INSERT INTO workspaces(id,path,repo_root,common_dir,kind,initial_dirty,created_ms)
                 VALUES('w-'||?1,'/repo/'||?1,'/repo','/repo','worktree','{}',?2)",
                params![id, self.at(from)],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO tasks(id,title,prompt,repo_root,workspace_id,created_ms)
                 VALUES('t-'||?1,'task','prompt','/repo','w-'||?1,?2)",
                params![id, self.at(from)],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO runs(id,task_id,harness,harness_version,profile_id,model,effort,
                 workspace_id,status,created_ms,ended_ms,title,capabilities)
                 VALUES(?1,'t-'||?1,?2,?3,?4,?5,?6,'w-'||?1,?7,?8,?9,'run','{}')",
                params![id, harness, version, profile, model, effort, status, self.at(from),
                    to.map(|m| self.at(m))],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO turns(id,run_id,n,prompt,started_ms,ended_ms,status)
                 VALUES('turn-'||?1,?1,1,'prompt',?2,?3,?4)",
                params![id, self.at(from), to.map(|m| self.at(m)),
                    if to.is_some() { "completed" } else { "running" }],
            )
            .unwrap();
        }

        fn run(&self, id: &str, from: i64, to: i64) {
            self.run_full(id, Some(PROFILE), "codex-app", "gpt-6-sol", Some("medium"), "0.155",
                from, Some(to), "completed");
        }

        /// One isolated run: a reading, the run, a settled reading.
        fn sample(&self, id: &str, start: i64, used: [f64; 2], moved: [f64; 2]) {
            self.observe(start, used);
            self.run(id, start + 1, start + 5);
            self.observe(start + 7, [used[0] + moved[0], used[1] + moved[1]]);
        }

        /// One isolated Auto root of `class`, recorded as Auto records it
        /// (a root intent and its decision event).
        fn auto_sample(&self, id: &str, start: i64, class: &str) {
            self.observe(start, [10.0, 1.0]);
            self.run(id, start + 1, start + 5);
            let event = self.store.insert_event(self.at(start + 1), Some(&format!("t-{id}")), Some(id),
                "auto_decision", "daemon", "exact",
                &serde_json::json!({"selection_input":{"work":{"task_class":class}}})).unwrap();
            self.store.conn.execute(
                "INSERT INTO auto_root_intents(work_unit_id,requirements_hash,repo_root,workspace_mode,
                 route_id,account_generation,phase,task_id,run_id,workspace_id,created_ms,decision_event_seq)
                 VALUES('unit-'||?1,?2,'/repo','worktree','codex-app/gpt-6-sol/medium',1,'queued',
                 't-'||?1,?1,'w-'||?1,?3,?4)",
                params![id, "h".repeat(64), self.at(start + 1), event.seq]).unwrap();
            self.observe(start + 7, [11.0, 1.0]);
        }

        fn draw(&self, bucket: &DrawBucket) -> UpperDraw {
            let cited = self.store.latest_auto_quota(PROFILE).unwrap().unwrap().event_seq;
            qualified_upper_draw_in_tx(&self.store.conn, PROFILE, cited, bucket, crate::daemon::now())
                .unwrap()
        }
    }

    fn bucket() -> DrawBucket {
        DrawBucket::agent("codex-app", Some("gpt-6-sol"), Some("medium"))
    }

    fn sampled(draw: &UpperDraw) -> Vec<String> {
        match draw {
            UpperDraw::Qualified { provenance, .. } => {
                provenance.samples.iter().map(|s| s.run_id.clone()).collect()
            }
            UpperDraw::Unknown { .. } => Vec::new(),
        }
    }

    fn unknown(draw: &UpperDraw) -> (&'static str, usize, BTreeMap<&'static str, usize>) {
        match draw {
            UpperDraw::Unknown { reason, samples, rejected } => (reason, *samples, rejected.clone()),
            other => panic!("expected an unknown draw, got {other:?}"),
        }
    }

    #[test]
    fn one_isolated_run_is_attributed_and_overlapping_runs_are_not() {
        let f = Fixture::new();
        for n in 0..5 {
            f.sample(&format!("lone-{n}"), n * 20, [10.0 + n as f64, 1.0], [2.0, 1.0]);
        }
        // Two runs on the same account between one pair of readings: the
        // movement cannot be split between them.
        f.observe(200, [30.0, 5.0]);
        f.run("pair-a", 201, 205);
        f.run("pair-b", 202, 204);
        f.observe(207, [34.0, 6.0]);
        // A run on another profile of the same account overlaps too.
        f.store.record_auto_account_identity("codex-b", &"a".repeat(64)).unwrap();
        f.observe(220, [34.0, 6.0]);
        f.run("linked", 221, 225);
        f.run_full("linked-other", Some("codex-b"), "claude", "sonnet", Some("high"), "2.1",
            222, Some(223), "completed");
        f.observe(227, [36.0, 6.0]);
        // A profile whose account is unknown might share it.
        f.observe(240, [36.0, 6.0]);
        f.run("unknown-id", 241, 245);
        f.run_full("unknown-other", Some("unrecorded"), "codex-app", "gpt-6-sol", Some("medium"),
            "0.155", 243, Some(244), "completed");
        f.observe(247, [38.0, 6.0]);
        let draw = f.draw(&bucket());
        let mut runs = sampled(&draw);
        runs.sort();
        assert_eq!(runs, ["lone-0", "lone-1", "lone-2", "lone-3", "lone-4"], "{draw:?}");
        let UpperDraw::Qualified { upper_draw_milli, provenance } = &draw else { unreachable!() };
        assert_eq!(upper_draw_milli, &vec![4_000, 3_000], "2 and 1 visible points plus two readings' error");
        assert_eq!(provenance.samples.len(), 5);

        // A native child is part of its parent's work, not overlapping work.
        f.observe(260, [38.0, 6.0]);
        f.run("parent", 261, 263);
        f.run_full("native-child", Some(PROFILE), "codex-app", "gpt-6-sol", Some("medium"),
            "0.155", 262, Some(265), "completed");
        f.store.conn.execute("UPDATE runs SET parent_run_id='parent',relation_source='native' WHERE id='native-child'",
            []).unwrap();
        f.observe(267, [39.0, 6.0]);
        // A managed delegation child is its own work unit: it overlaps.
        f.observe(280, [39.0, 6.0]);
        f.run("delegating", 281, 283);
        f.run("managed-child", 282, 284);
        f.store.conn.execute("UPDATE runs SET parent_run_id='delegating',relation_source='managed-delegation'
            WHERE id='managed-child'", []).unwrap();
        f.observe(287, [41.0, 6.0]);
        // A still-running run anywhere on the account is active through both
        // readings of any later run.
        f.run_full("still-running", Some("codex-b"), "codex-app", "gpt-6-sol", Some("medium"), "0.155",
            300, None, "running");
        f.observe(299, [41.0, 6.0]);
        f.run("during-open-run", 301, 303);
        f.observe(305, [42.0, 6.0]);
        let draw = f.draw(&bucket());
        let runs = sampled(&draw);
        assert!(runs.contains(&"parent".to_string()), "{draw:?}");
        for excluded in ["pair-a", "pair-b", "linked", "unknown-id", "delegating", "managed-child",
            "during-open-run", "native-child"] {
            assert!(!runs.contains(&excluded.to_string()), "{excluded} must not be a sample: {draw:?}");
        }
    }

    #[test]
    fn ambiguous_movement_is_rejected_and_zero_visible_movement_is_not_free() {
        let f = Fixture::new();
        let reading = |minutes: i64, used: [f64; 2], reset: i64, plan: &str| {
            let at = f.at(minutes);
            QuotaSnapshot {
                ordinary_usage_allowed: Some(true),
                observed_ms: at,
                expires_ms: at + MINUTE,
                native_uncertain_until_ms: None,
                windows: [("five_hour", 300), ("weekly", 10_080)].into_iter().zip(used)
                    .map(|((name, minutes), used_percent)| QuotaWindow {
                        pool_id: PROFILE.into(), bucket_id: name.into(), window: name.into(),
                        model: None, model_family: None, plan_type: Some(plan.into()), used_percent,
                        reset_ms: Some(reset), duration_mins: Some(minutes), observed_ms: at,
                        expires_ms: at + MINUTE,
                    }).collect(),
            }
        };
        let cited = reading(100, [50.0, 10.0], f.reset, "pro");
        let before = reading(0, [40.0, 9.0], f.reset, "pro");
        let (start, end) = (f.at(1), f.at(5));
        let after = reading(7, [42.5, 9.0], f.reset, "pro");
        assert_eq!(attribute_window_movement(&before, &after, &cited, "pro", start, end),
            Ok(vec![4_500, 2_000]), "zero visible movement keeps its interval's upper end, not zero");
        let early = reading(5, [42.5, 9.0], f.reset, "pro");
        assert_eq!(attribute_window_movement(&before, &early, &cited, "pro", start, end),
            Err("reporting_not_settled"));
        let reset = reading(7, [2.0, 9.0], f.reset + MINUTE, "pro");
        assert_eq!(attribute_window_movement(&before, &reset, &cited, "pro", start, end),
            Err("window_reset_crossed"));
        let expired = reading(7, [42.5, 9.0], f.at(6), "pro");
        let expired_before = reading(0, [40.0, 9.0], f.at(6), "pro");
        assert_eq!(attribute_window_movement(&expired_before, &expired, &cited, "pro", start, end),
            Err("window_reset_crossed"));
        let lower = reading(7, [39.0, 9.0], f.reset, "pro");
        assert_eq!(attribute_window_movement(&before, &lower, &cited, "pro", start, end),
            Err("meter_decreased"));
        let plan = reading(7, [42.5, 9.0], f.reset, "plus");
        assert_eq!(attribute_window_movement(&before, &plan, &cited, "pro", start, end),
            Err("plan_changed"));
        let late_before = reading(2, [40.0, 9.0], f.reset, "pro");
        assert_eq!(attribute_window_movement(&late_before, &after, &cited, "pro", start, end),
            Err("before_reading_not_prior"));
        let mut narrowed = after.clone();
        narrowed.windows.pop();
        assert_eq!(attribute_window_movement(&before, &narrowed, &cited, "pro", start, end),
            Err("window_scope_changed"));
        let mut uncertain = after.clone();
        uncertain.native_uncertain_until_ms = Some(f.at(60));
        assert_eq!(attribute_window_movement(&before, &uncertain, &cited, "pro", start, end),
            Err("native_order_uncertain"));
        let mut denied = after.clone();
        denied.ordinary_usage_allowed = Some(false);
        assert_eq!(attribute_window_movement(&before, &denied, &cited, "pro", start, end),
            Err("account_denied"));
    }

    #[test]
    fn samples_are_bucketed_by_harness_model_effort_class_plan_and_version() {
        let f = Fixture::new();
        for n in 0..5 {
            f.sample(&format!("sol-medium-{n}"), n * 20, [10.0, 1.0], [1.0, 0.0]);
        }
        // Different model, effort, harness, missing effort: other buckets.
        let mut minute = 100;
        let mut other = |id: &str, harness: &str, model: &str, effort: Option<&str>, version: &str| {
            f.observe(minute, [20.0, 2.0]);
            f.run_full(id, Some(PROFILE), harness, model, effort, version, minute + 1,
                Some(minute + 5), "completed");
            f.observe(minute + 7, [30.0, 3.0]);
            minute += 20;
        };
        other("astra-medium", "codex-app", "gpt-6-astra", Some("medium"), "0.155");
        other("sol-high", "codex-app", "gpt-6-sol", Some("high"), "0.155");
        other("sol-no-effort", "codex-app", "gpt-6-sol", None, "0.155");
        other("sol-codex-exec", "codex", "gpt-6-sol", Some("medium"), "0.155");
        let draw = f.draw(&bucket());
        let UpperDraw::Qualified { upper_draw_milli, .. } = &draw else { panic!("{draw:?}") };
        assert_eq!(upper_draw_milli, &vec![3_000, 2_000], "only same-bucket samples priced it");
        assert_eq!(unknown(&f.draw(&DrawBucket::agent("codex-app", Some("gpt-6-sol"), Some("high")))).1, 1);
        assert_eq!(unknown(&f.draw(&DrawBucket::agent("codex-app", Some("gpt-6-sol"), None))).0,
            "effort_unknown", "missing effort cannot be priced");
        // An Auto work unit's class is its own bucket.
        let mut auto = bucket();
        auto.task_class = "auto/browser_check".into();
        assert_eq!(unknown(&f.draw(&auto)).1, 0);
        // Another plan in the cited reading: none of the samples compare.
        f.observe_on(PROFILE, 300, [1.0, 1.0], "plus");
        let (_, samples, rejected) = unknown(&f.draw(&bucket()));
        assert_eq!(samples, 0);
        assert_eq!(rejected.get("plan_changed"), Some(&5), "{rejected:?}");
        f.observe(320, [1.0, 1.0]);
        assert!(matches!(f.draw(&bucket()), UpperDraw::Qualified { .. }));
        // A newer harness version starts a new bucket.
        f.observe(340, [1.0, 1.0]);
        f.run_full("upgraded", Some(PROFILE), "codex-app", "gpt-6-sol", Some("medium"), "0.156",
            341, Some(345), "completed");
        f.observe(347, [2.0, 1.0]);
        let (reason, samples, rejected) = unknown(&f.draw(&bucket()));
        assert_eq!((reason, samples), ("upper_draw_uncalibrated", 1));
        assert_eq!(rejected.get("harness_version_changed"), Some(&5), "{rejected:?}");
    }

    #[test]
    fn the_upper_bound_widens_under_noise_and_is_never_below_a_sample() {
        assert_eq!(window_bound(&[3_000; 5]).unwrap().3, 3_000, "quiet samples: the largest one");
        let (max, mean, sd, upper) = window_bound(&[2_000, 2_500, 3_000, 2_000, 9_000]).unwrap();
        assert_eq!(max, 9_000);
        assert!((mean - 3_700.0).abs() < 1e-9);
        assert!(upper > max && upper as f64 >= mean + 3.0 * sd - 1.0, "{upper} {mean} {sd}");
        assert_eq!(window_bound(&[100_000, 1_000, 1_000]).unwrap().3, 100_000, "capped at a whole window");
        // The same through recorded observations: one noisy sample widens
        // the bound above every sample.
        let f = Fixture::new();
        for (n, moved) in [1.0, 1.5, 1.0, 1.0, 6.0].into_iter().enumerate() {
            f.sample(&format!("noisy-{n}"), n as i64 * 20, [10.0, 1.0], [moved, 0.0]);
        }
        let draw = f.draw(&bucket());
        let UpperDraw::Qualified { upper_draw_milli, provenance } = &draw else { panic!("{draw:?}") };
        assert_eq!(provenance.windows[0].max_milli, 8_000);
        assert!(upper_draw_milli[0] > 8_000, "{draw:?}");
        assert_eq!(upper_draw_milli[0], window_bound(&[3_000, 3_500, 3_000, 3_000, 8_000]).unwrap().3);
    }

    #[test]
    fn cold_start_refuses_until_five_attributable_samples() {
        let f = Fixture::new();
        f.observe(0, [1.0, 1.0]);
        assert_eq!(unknown(&f.draw(&bucket())), ("harness_version_unknown", 0, BTreeMap::new()));
        for n in 0..4 {
            f.sample(&format!("s-{n}"), n * 20, [10.0, 1.0], [1.0, 0.0]);
        }
        let (reason, samples, _) = unknown(&f.draw(&bucket()));
        assert_eq!((reason, samples), ("upper_draw_uncalibrated", 4));
        f.sample("s-4", 80, [10.0, 1.0], [1.0, 0.0]);
        let draw = f.draw(&bucket());
        assert!(matches!(draw, UpperDraw::Qualified { .. }), "{draw:?}");
        assert_eq!(unknown(&f.draw(&DrawBucket::agent("codex-app", Some("other"), Some("medium")))).1, 0);
        // No identity or no observation: unknown, not zero.
        let cited = f.store.latest_auto_quota(PROFILE).unwrap().unwrap().event_seq;
        assert_eq!(unknown(&qualified_upper_draw_in_tx(&f.store.conn, "unrecorded", cited, &bucket(),
            crate::daemon::now()).unwrap()).0, "account_identity_unknown");
        assert_eq!(unknown(&qualified_upper_draw_in_tx(&f.store.conn, PROFILE, cited + 1000, &bucket(),
            crate::daemon::now()).unwrap()).0, "quota_unknown");
        // Samples older than the retention horizon no longer count.
        let later = crate::daemon::now() + MAX_SAMPLE_AGE_MS;
        let stale = qualified_upper_draw_in_tx(&f.store.conn, PROFILE, cited, &bucket(), later).unwrap();
        assert_eq!(unknown(&stale).1, 0);
    }

    #[test]
    fn a_qualified_booking_records_its_provenance_and_cold_start_refuses() {
        use crate::account_booking::{AccountBookingRequest, BookingDecision, BookingDraw};
        let f = Fixture::new();
        let now = crate::daemon::now();
        let book = |id: &str, seq: i64, draw: BookingDraw<'_>| {
            f.store.book_shared_account(&AccountBookingRequest {
                id, request_hash: id, caller: "ordinary", route_id: "codex-app/gpt-6-sol",
                profile_id: PROFILE, quota_profile_id: PROFILE, account_generation: 1,
                quota_event_seq: seq, now_ms: crate::daemon::now(), draw,
                allocation_remaining_milli: None,
            }).unwrap()
        };
        for n in 0..4 {
            f.sample(&format!("s-{n}"), n * 20, [10.0, 1.0], [1.0, 0.0]);
        }
        // A fresh cited reading (the booking refuses a stale one).
        let fresh = f.observe(1200, [20.0, 2.0]);
        assert!((f.at(1200) - now).abs() < MINUTE);
        let bucket = bucket();
        assert_eq!(book("cold", fresh, BookingDraw::Qualified(&bucket)),
            BookingDecision::Blocked("upper_draw_unknown"), "four samples: uncalibrated");
        assert!(f.store.shared_booking_draw("cold").unwrap().is_none(), "a refusal records nothing");
        f.sample("s-4", 80, [10.0, 1.0], [1.0, 0.0]);
        let fresh = f.observe(1200, [20.0, 2.0]);
        assert_eq!(book("warm", fresh, BookingDraw::Qualified(&bucket)), BookingDecision::Booked);
        let (source, provenance) = f.store.shared_booking_draw("warm").unwrap().unwrap();
        assert_eq!(source.as_deref(), Some("qualified"));
        let provenance = provenance.expect("a qualified booking keeps its provenance");
        assert_eq!(provenance.method, METHOD);
        assert_eq!(provenance.bucket, bucket);
        assert_eq!(provenance.sample_count, 5);
        assert_eq!(provenance.cited_quota_event_seq, fresh);
        assert_eq!(provenance.account_generation, 1);
        assert_eq!(provenance.plan_type, "pro");
        assert_eq!(provenance.harness_version, "0.155");
        let mut runs: Vec<_> = provenance.samples.iter().map(|s| s.run_id.as_str()).collect();
        runs.sort();
        assert_eq!(runs, ["s-0", "s-1", "s-2", "s-3", "s-4"]);
        assert!(provenance.samples.iter().all(|s| s.before_seq < s.after_seq && s.after_seq < fresh));
        assert_eq!(provenance.newest_sample_ms, f.at(87));
        assert_eq!(provenance.oldest_sample_ms, f.at(7));
        assert!(provenance.age_ms >= f.at(1200) - f.at(87) - MINUTE, "{}", provenance.age_ms);
        let amounts: Vec<i64> = {
            let mut stmt = f.store.conn.prepare(
                "SELECT amount_milli FROM shared_booking_windows WHERE work_unit_id='warm' ORDER BY window_key").unwrap();
            let rows = stmt.query_map([], |row| row.get(0)).unwrap().map(Result::unwrap).collect();
            rows
        };
        let mut expected = provenance.windows.iter().map(|w| w.upper_milli).collect::<Vec<_>>();
        expected.sort();
        let mut sorted = amounts.clone();
        sorted.sort();
        assert_eq!(sorted, expected);
        assert_eq!(sorted, vec![2_000, 3_000], "1 and 0 visible points plus two readings' error");
        // A replay after a newer reading is the same booking, not a new draw.
        let newer = f.observe(1200, [21.0, 2.0]);
        assert_eq!(book("warm", newer, BookingDraw::Qualified(&bucket)), BookingDecision::Replayed);
        // A fixture draw is recorded as such, without provenance.
        f.store.release_shared_booking_pre_effect("warm").unwrap();
        assert_eq!(book("fixture", newer, BookingDraw::Fixture(&[1_000, 1_000])), BookingDecision::Booked);
        assert_eq!(f.store.shared_booking_draw("fixture").unwrap(), Some((Some("fixture".into()), None)));
    }

    #[test]
    fn an_auto_root_books_known_windows_once_its_class_is_calibrated() {
        use crate::account_booking::{AccountBookingRequest, BookingDecision, BookingDraw};
        use crate::store::{Run, Task, Workspace};
        let f = Fixture::new();
        f.store.set_auto_mode_enabled(true).unwrap();
        let pool = format!("account/{}", "a".repeat(64));
        for n in 0..5 {
            f.auto_sample(&format!("auto-{n}"), n * 20, "browser_check");
        }
        f.observe(1200, [20.0, 2.0]);
        let at = f.at(1200);
        let admit = |id: &str, class: &str| -> bool {
            let workspace = Workspace { id:format!("w-{id}"), path:format!("/repo/{id}"),
                repo_root:"/repo".into(), common_dir:"/repo/.git".into(), kind:"worktree".into(),
                branch:Some(format!("codex/{id}")), owner_run_id:None,
                initial_dirty:serde_json::json!({"clean":true}), created_ms:at, removed_ms:None };
            let task = Task { id:format!("t-{id}"), title:"root".into(), prompt:"prompt".into(),
                repo_root:"/repo".into(), target_ref:None, workspace_id:workspace.id.clone(),
                start_snapshot:None, fork_commit:None, fork_provenance:None, created_ms:at, archived_ms:None };
            let run = Run { id:id.into(), task_id:task.id.clone(), parent_run_id:None,
                harness:"codex-app".into(), harness_version:Some("0.155".into()),
                profile_id:Some(PROFILE.into()), model:Some("gpt-6-sol".into()),
                effort:Some("medium".into()), workspace_id:workspace.id.clone(), native_id:None,
                status:"queued".into(), exit_reason:None, created_ms:at, ended_ms:None,
                title:"root".into(), relation_source:None, relation_confidence:None,
                capabilities:serde_json::json!({}), process_generation:0, attention:None };
            let trace = serde_json::json!({"selected_route":{"harness":"codex-app","profile_id":PROFILE,
                "model":"gpt-6-sol","effort":"medium"},"selection_input":{"work":{"task_class":class}}});
            f.store.insert_auto_root_selected(&format!("unit-{id}"), &"h".repeat(64),
                "codex-app/gpt-6-sol/medium", &pool, Some(1), &workspace, &task, &run,
                &serde_json::json!({"generic":{}}), &trace).unwrap().is_some()
        };
        let book = |id: &str| {
            let cited = f.store.latest_auto_quota(PROFILE).unwrap().unwrap().event_seq;
            f.store.book_shared_account(&AccountBookingRequest {
                id, request_hash: id, caller: "swarm", route_id: "claude/sonnet",
                profile_id: PROFILE, quota_profile_id: PROFILE, account_generation: 1,
                quota_event_seq: cited, now_ms: crate::daemon::now(),
                draw: BookingDraw::Fixture(&[1_000, 1_000]), allocation_remaining_milli: None,
            }).unwrap()
        };
        let finish = |id: &str| {
            f.store.conn.execute("UPDATE runs SET status='completed',ended_ms=?2 WHERE id=?1",
                params![id, crate::daemon::now()]).unwrap();
            f.store.release_settled_auto_pool_claim(id).unwrap();
        };
        let claim = |unit: &str| -> String {
            f.store.conn.query_row("SELECT state FROM auto_pool_claims WHERE work_unit_id=?1",
                [unit], |row| row.get(0)).unwrap()
        };

        let route = {
            use crate::auto_select::{Allowance, CapabilityTier, Fit, Health, Route, Sandbox};
            Route { id:"codex-app/gpt-6-sol/medium".into(), harness:"codex-app".into(),
                provider:"openai".into(), endpoint:"codex".into(), profile_id:PROFILE.into(),
                pool_id:pool.clone(), model:"gpt-6-sol".into(), resolved_model_version:None,
                effort:"medium".into(), tier:CapabilityTier::General, tools:Default::default(),
                context_limit:None, supports_approvals:true, sandbox:Sandbox::WorkspaceWrite,
                supported_sandboxes:None, recommended_default:true,
                quota:Allowance::ObservedNonExhausted, quota_blocks:Vec::new(), fit:Fit::Unknown,
                health:Health::Healthy, unresolved_quota_pool_identity:false,
                in_flight_pool_claim:false }
        };
        let open = |class: &str| f.store
            .auto_pool_open_to_known_windows(&route, Some(class), crate::daemon::now()).unwrap();
        // An uncalibrated class keeps the whole-account unknown-draw claim,
        // which leaves no room for a known-window booking beside it.
        assert!(admit("root-cold", "diagnosis"));
        assert!(f.store.shared_booking_draw("unit-root-cold").unwrap().is_none());
        assert_eq!(book("swarm/beside-cold"), BookingDecision::Blocked("account_pool_busy"));
        assert!(!open("browser_check"), "an unknown-draw claim holds the whole account for selection");
        finish("root-cold");
        assert_eq!(claim("unit-root-cold"), "released");

        // A calibrated class books its windows; a Swarm worker shares the
        // account beside the queued root.
        assert!(admit("root-warm", "browser_check"));
        let (source, provenance) = f.store.shared_booking_draw("unit-root-warm").unwrap().unwrap();
        assert_eq!(source.as_deref(), Some("qualified"));
        let provenance = provenance.unwrap();
        assert_eq!(provenance.bucket.task_class, "auto/browser_check");
        assert_eq!(provenance.sample_count, 5);
        assert_eq!(book("swarm/beside-warm"), BookingDecision::Booked);
        // Selection: the pool is taken, but only by known windows, so a
        // calibrated class may still choose it; an uncalibrated one may not.
        assert!(f.store.auto_pool_claimed(&pool).unwrap());
        assert!(open("browser_check"));
        assert!(!open("diagnosis"));
        // Another Auto root on the same account books beside it too.
        assert!(admit("root-second", "browser_check"));
        // Settled: the draw stays committed until a later reading, then goes.
        finish("root-warm");
        assert_eq!(claim("unit-root-warm"), "active", "retained until a reading includes it");
        f.store.conn.execute("UPDATE runs SET status='completed',ended_ms=?1 WHERE id='root-second'",
            [crate::daemon::now()]).unwrap();
        f.store.conn.execute("UPDATE auto_pool_claims SET state='released' WHERE work_unit_id IN
            ('unit-root-second','swarm/beside-warm')", []).unwrap();
        assert!(!f.store.auto_pool_claimed(&pool).unwrap(),
            "a settled booking's retained draw is no occupant for Auto admission");
        f.store.conn.execute("UPDATE runs SET status='queued',ended_ms=NULL WHERE id='root-second'", []).unwrap();
        f.store.conn.execute("UPDATE auto_pool_claims SET state='active' WHERE work_unit_id IN
            ('unit-root-second','swarm/beside-warm')", []).unwrap();
        let settled: Option<String> = f.store.conn.query_row(
            "SELECT outcome FROM shared_booking_intents WHERE work_unit_id='unit-root-warm'",
            [], |row| row.get(0)).unwrap();
        assert_eq!(settled.as_deref(), Some("settled"));
        std::thread::sleep(std::time::Duration::from_millis(5));
        f.observe_at(PROFILE, crate::daemon::now(), [21.0, 2.0], "pro");
        assert_eq!(claim("unit-root-warm"), "released");
        assert_eq!(claim("unit-root-second"), "active", "an unsettled root keeps its windows");

        // A child of a calibrated class books its own windows; if it never
        // starts, its booking goes with its claim.
        f.store.conn.execute("UPDATE runs SET status='running',process_generation=1 WHERE id='root-second'",
            []).unwrap();
        let parent = f.store.run("root-second").unwrap().unwrap();
        let trace = serde_json::json!({"selected_route":{"harness":"codex-app","profile_id":PROFILE,
            "model":"gpt-6-sol","effort":"medium"},"selection_input":{"work":{"task_class":"browser_check"}}});
        assert!(f.store.insert_auto_selected_decision("unit-child", &parent, "hash",
            "codex-app/gpt-6-sol/medium", &pool, Some(1), 300_000, &trace).unwrap().is_some());
        assert_eq!(f.store.shared_booking_draw("unit-child").unwrap().unwrap().0.as_deref(),
            Some("qualified"));
        assert!(f.store.release_unstarted_auto_pool_claim("unit-child").unwrap());
        let outcome: Option<String> = f.store.conn.query_row(
            "SELECT outcome FROM shared_booking_intents WHERE work_unit_id='unit-child'",
            [], |row| row.get(0)).unwrap();
        assert_eq!(outcome.as_deref(), Some("not_started"));
        assert_eq!(claim("unit-child"), "released");
    }

    /// Claude Code's native `rate_limit_event`s arrive only during a run's
    /// own turns: the first after its first model response, the last before
    /// its result. Recorded through the real parser, six serial Claude runs
    /// whose only readings are their own give no sample: the readings carry
    /// no plan, and even with one stamped on, no reading brackets a run. A
    /// run's first reading already counts part of its draw and its last
    /// misses the rest, so using them would under-state the draw.
    #[test]
    fn claude_in_run_readings_cannot_bracket_their_own_run() {
        let f = Fixture::new();
        const CLAUDE: &str = "claude-a";
        f.store.record_auto_account_identity(CLAUDE, &"c".repeat(64)).unwrap();
        let reading = |at: i64, five: f64, plan: Option<&str>| -> i64 {
            let event = serde_json::json!({"type":"rate_limit_event","rate_limit_info":{
                "status":"allowed","rateLimitType":"five_hour","resetsAt":f.reset / 1000,
                "unifiedWindows":{"five_hour":{"utilization":five,"resetsAt":f.reset / 1000},
                    "seven_day":{"utilization":0.05,"resetsAt":f.reset / 1000 + 86_400}}}});
            let mut snapshot = crate::auto_quota::parse_claude_rate_limit_event(&event, CLAUDE, at).unwrap();
            assert_eq!(snapshot.reported_plan_type(), None, "Claude's event reports no plan");
            assert_eq!(snapshot.ordinary_usage_allowed, None, "nor an explicit allowance");
            for window in &mut snapshot.windows {
                window.plan_type = plan.map(str::to_string);
            }
            let seq = f.store.insert_event(at, None, None, "auto_quota", "harness", "normalized",
                &serde_json::json!({"pool_id":CLAUDE})).unwrap().seq;
            assert!(f.store.insert_auto_quota(seq, CLAUDE, "claude/native-rate-limit-event", &snapshot).unwrap());
            seq
        };
        // True meter: 10% before run 0; each run draws one point, of which
        // 0.3 is counted by its first reading and 0.8 by its last.
        let record = |plan: Option<&str>, offset: i64| {
            for n in 0..6 {
                let start = offset + n * 10;
                let base = 0.10 + n as f64 * 0.01;
                f.run_full(&format!("claude-{offset}-{n}"), Some(CLAUDE), "claude", "sonnet",
                    Some("medium"), "2.1.246", start, Some(start + 5), "completed");
                reading(f.at(start + 1), base + 0.003, plan);
                reading(f.at(start + 4), base + 0.008, plan);
            }
        };
        let bucket = DrawBucket::agent("claude", Some("sonnet"), Some("medium"));
        let draw = |f: &Fixture| {
            let cited = f.store.latest_auto_quota(CLAUDE).unwrap().unwrap().event_seq;
            qualified_upper_draw_in_tx(&f.store.conn, CLAUDE, cited, &bucket, crate::daemon::now()).unwrap()
        };
        record(None, 0);
        assert_eq!(unknown(&draw(&f)).0, "plan_unknown", "as recorded today");

        // Even with a plan, the bracket rule rejects all twelve runs: the
        // first has no earlier reading, the last no later one, and every
        // other run's nearest readings fall inside its neighbours' spans.
        record(Some("max"), 100);
        let (reason, samples, rejected) = unknown(&draw(&f));
        assert_eq!((reason, samples), ("upper_draw_uncalibrated", 0));
        assert_eq!(rejected.get("overlapping_work"), Some(&10), "{rejected:?}");
        assert_eq!(rejected.get("after_reading_missing"), Some(&1), "{rejected:?}");
        assert_eq!(rejected.get("before_reading_missing"), Some(&1), "{rejected:?}");

        // A run's own first and last readings are not a before and an after:
        // the first is taken after the run began and the last before it ended.
        let snapshot = |seq: i64| -> QuotaSnapshot {
            let encoded: String = f.store.conn.query_row(
                "SELECT snapshot FROM auto_quota_observations WHERE event_seq=?1", [seq], |r| r.get(0)).unwrap();
            serde_json::from_str(&encoded).unwrap()
        };
        let first = reading(f.at(201), 0.203, Some("max"));
        let last = reading(f.at(204), 0.208, Some("max"));
        let (first, last) = (snapshot(first), snapshot(last));
        assert_eq!(attribute_window_movement(&first, &last, &last, "max", f.at(200), f.at(205)),
            Err("before_reading_not_prior"));
        assert_eq!(attribute_window_movement(&first, &last, &last, "max", f.at(201), f.at(205)),
            Err("reporting_not_settled"));
        // Were both rules waived, the visible movement (0.5 of a point) would
        // miss half of the run's one-point draw: an under-count, unsafe for
        // an upper bound.
        let visible = last.windows[0].used_percent - first.windows[0].used_percent;
        assert!((visible - 0.5).abs() < 1e-9 && visible < 1.0, "{visible}");
    }
}
