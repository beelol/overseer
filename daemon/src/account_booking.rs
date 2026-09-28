//! One daemon-owned account commitment boundary for ordinary, Auto and Swarm work.

use crate::auto_quota::QuotaSnapshot;
use crate::store::Store;
use anyhow::{anyhow, Result};
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use sha2::{Digest, Sha256};
use std::collections::HashSet;

/// Where a booking's upper draw comes from. It is in thousandths of a
/// reported percentage point for every window of the cited structured
/// observation; a token count or estimated credit amount is never one.
#[derive(Clone, Copy)]
pub enum BookingDraw<'a> {
    /// Supplied by a fixture caller (behind `OVERSEER_SHARED_BOOKING_FIXTURE_API`).
    Fixture(&'a [i64]),
    /// Computed in the booking's own transaction from the account's recorded
    /// observations around isolated runs of this bucket
    /// (`upper_draw::qualified_upper_draw_in_tx`); unknown refuses the booking.
    Qualified(&'a crate::upper_draw::DrawBucket),
}

pub struct AccountBookingRequest<'a> {
    pub id: &'a str,
    pub request_hash: &'a str,
    pub caller: &'a str,
    pub route_id: &'a str,
    pub profile_id: &'a str,
    pub quota_profile_id: &'a str,
    pub account_generation: i64,
    pub quota_event_seq: i64,
    pub now_ms: i64,
    pub draw: BookingDraw<'a>,
    pub allocation_remaining_milli: Option<&'a [i64]>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum BookingDecision {
    Booked,
    Replayed,
    Blocked(&'static str),
}

pub struct LaunchBookingRequest<'a> {
    pub account: &'a AccountBookingRequest<'a>,
    pub workspace_path: Option<&'a str>,
    pub consume_agent_slot: bool,
    /// Binds the caller's durable permission/launch inputs. The shared
    /// service also hashes the resource fields; this is not a spawn command.
    pub launch_hash: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct SharedLaunchIntent {
    pub work_unit_id: String,
    pub route_id: String,
    pub profile_id: String,
    pub account_generation: i64,
    pub phase: String,
    pub workspace_path: Option<String>,
    pub slot_held: bool,
    pub writer_held: bool,
    pub effects_claimed_ms: Option<i64>,
    /// The run this launch became. From binding on, that run is counted
    /// through this intent's holds and never again as an unbound run.
    pub run_id: Option<String>,
    pub settled_ms: Option<i64>,
    /// `released_unclaimed`, `effects_uncertain`, `not_started`, `settled`
    /// or `observed_after_settlement`; None while the launch is in progress.
    pub outcome: Option<String>,
}

/// Statuses in which a run may still own a process, its workspace and an
/// app slot. `unknown` and `disconnected` stay held until the daemon has
/// confirmed that the supervisor and the harness are gone.
const HOLDING_STATUSES: &str = "'queued','starting','running','waiting_for_user',
    'waiting_for_connection','waiting_for_memory','unknown','disconnected'";

/// An intent still holds its flagged slot/writer: it is unsettled and either
/// not yet bound, or bound to a run that may still own a process.
fn held_intent_sql(alias: &str) -> String {
    format!(
        "{alias}.settled_ms IS NULL AND ({alias}.run_id IS NULL OR EXISTS(SELECT 1 FROM runs hr
        WHERE hr.id={alias}.run_id AND (hr.status IN ({HOLDING_STATUSES})
            OR EXISTS(SELECT 1 FROM turns ht WHERE ht.run_id=hr.id
                AND ht.status='running' AND ht.ended_ms IS NULL))))"
    )
}

/// A run bound to a booking is counted through that booking only. After a
/// confirmed settlement, a lost (`disconnected`/`unknown`) bound run holds
/// nothing; a later follow-up that reactivates it counts as ordinary work.
fn unbound_run_sql(alias: &str) -> String {
    format!(
        "NOT EXISTS(SELECT 1 FROM shared_booking_intents bb WHERE bb.run_id={alias}.id
        AND (bb.settled_ms IS NULL OR {alias}.status IN ('disconnected','unknown')))"
    )
}

/// The app-wide agent ceiling (`agents.max_active`, default 9).
pub fn app_slot_limit(conn: &Connection) -> Result<i64> {
    let setting: Option<String> = conn
        .query_row(
            "SELECT value FROM meta WHERE key='agents.max_active'",
            [],
            |row| row.get(0),
        )
        .optional()?;
    match setting {
        None => Ok(9),
        Some(value) => value
            .parse::<i64>()
            .ok()
            .filter(|n| (1..=256).contains(n))
            .ok_or_else(|| anyhow!("invalid app agent limit")),
    }
}

/// The work unit id of the shared booking that holds a Swarm worker
/// attempt's account commitment and app slot.
pub fn swarm_attempt_booking_id(attempt_id: &str) -> String {
    format!("swarm/{attempt_id}")
}

/// The one count of app slots in use, consulted by every admission path
/// (ordinary starts and follow-ups, Auto roots and children, Swarm directors
/// and workers, booked starts) inside its own admission transaction. Each
/// occupant is counted exactly once, by the first rule that holds it:
///
/// 1. a durable slot hold taken before an ordinary start's or a follow-up's
///    run row exists (`app_slot_holds`, deleted in the commit that inserts
///    the run, and cleared at startup: the old daemon's requests are gone);
/// 2. a shared booking that holds a slot (`slot_held`), bound or not, until
///    its run is confirmed to have no process;
/// 3. an Auto child admitted but not yet recorded as a managed child;
/// 4. a registered Swarm worker attempt whose booking does not hold a slot
///    (an unbooked fixture worker, or a booking already settled);
/// 5. an active Swarm category's director slot;
/// 6. any other top-level or managed-delegation run that may still own a
///    process (including `unknown`/`disconnected`), except Overseer's own
///    coordinating run, which by the Swarm/Auto contract holds no slot.
///
/// A director run bound to a booking that took no slot
/// (`consume_agent_slot: false`) is counted by rule 5 while its category is
/// active and by rule 6 while it is still planning, never twice.
pub fn app_slots_in_use(conn: &Connection) -> Result<i64> {
    let held = held_intent_sql("b");
    let swarm_held = held_intent_sql("sb");
    Ok(conn.query_row(
        &format!(
            "SELECT
            (SELECT COUNT(*) FROM app_slot_holds)
          + (SELECT COUNT(*) FROM shared_booking_intents b WHERE b.slot_held=1 AND {held})
          + (SELECT COUNT(*) FROM auto_launch_intents i
               JOIN auto_pool_claims c ON c.work_unit_id=i.work_unit_id
               WHERE c.state IN ('active','uncertain')
               AND NOT EXISTS(SELECT 1 FROM managed_work_units m WHERE m.work_unit_id=i.work_unit_id))
          + (SELECT COUNT(*) FROM swarm_attempts a
               WHERE a.status='registered' AND a.executor='worker'
               AND NOT EXISTS(SELECT 1 FROM shared_booking_intents sb
                   WHERE sb.work_unit_id='swarm/'||a.id AND sb.slot_held=1 AND {swarm_held}))
          + (SELECT COUNT(*) FROM swarm_runs WHERE status IN ('running','paused','stalled','stopping'))
          + (SELECT COUNT(*) FROM runs r
               WHERE (r.parent_run_id IS NULL OR r.relation_source='managed-delegation')
               AND (r.status IN ({HOLDING_STATUSES})
                   OR EXISTS(SELECT 1 FROM turns t WHERE t.run_id=r.id
                       AND t.status='running' AND t.ended_ms IS NULL))
               AND NOT EXISTS(SELECT 1 FROM shared_booking_intents bb WHERE bb.run_id=r.id
                   AND ((bb.settled_ms IS NULL AND bb.slot_held=1)
                     OR (bb.settled_ms IS NOT NULL AND r.status IN ('disconnected','unknown'))))
               AND NOT EXISTS(SELECT 1 FROM run_roles rr WHERE rr.run_id=r.id AND rr.role='overseer')
               AND NOT EXISTS(SELECT 1 FROM swarm_worker_launches l
                   JOIN swarm_attempts wa ON wa.id=l.attempt_id
                   WHERE l.overseer_run_id=r.id AND wa.status='registered')
               AND NOT EXISTS(SELECT 1 FROM swarm_director_owners o
                   JOIN swarm_runs s ON s.id=o.run_id
                   WHERE o.overseer_run_id=r.id
                   AND s.status IN ('running','paused','stalled','stopping')))"
        ),
        [],
        |row| row.get(0),
    )?)
}

#[derive(Debug, PartialEq, Eq)]
pub enum SlotHold {
    Held,
    Full { active: i64, limit: i64 },
}

/// Take a durable app-slot hold for an admission whose run row does not
/// exist yet (an ordinary start before its workspace is prepared, a
/// follow-up that reactivates an ended run). Call inside an IMMEDIATE
/// transaction; the hold is deleted in the commit that inserts the run.
pub fn hold_app_slot_in_tx(conn: &Connection, id: &str, kind: &str, now_ms: i64) -> Result<SlotHold> {
    if conn.is_autocommit() {
        return Err(anyhow!("an app slot hold requires an admission transaction"));
    }
    let active = app_slots_in_use(conn)?;
    let limit = app_slot_limit(conn)?;
    if active >= limit {
        return Ok(SlotHold::Full { active, limit });
    }
    conn.execute(
        "INSERT INTO app_slot_holds(id,kind,created_ms) VALUES(?1,?2,?3)",
        params![id, kind, now_ms],
    )?;
    Ok(SlotHold::Held)
}

/// A booked Swarm worker whose attempt is no longer registered (cancelled,
/// failed or finished before its launch claimed effects) never requested an
/// effect: release its booking with its slot and account commitment. Called
/// in the admission and startup transactions, so a Swarm attempt and its
/// booking never disagree for longer than one admission.
pub fn release_orphaned_swarm_bookings_in_tx(conn: &Connection, now_ms: i64) -> Result<usize> {
    let orphaned: Vec<String> = {
        let mut stmt = conn.prepare(
            "SELECT b.work_unit_id FROM shared_booking_intents b
            WHERE b.caller='swarm' AND b.phase='booked' AND b.effects_claimed_ms IS NULL
              AND b.launch_hash IS NOT NULL
              AND NOT EXISTS(SELECT 1 FROM swarm_attempts a
                  WHERE 'swarm/'||a.id=b.work_unit_id AND a.status='registered')
            ORDER BY b.work_unit_id",
        )?;
        let rows = stmt.query_map([], |row| row.get(0))?;
        rows.collect::<rusqlite::Result<_>>()?
    };
    for id in &orphaned {
        conn.execute(
            "UPDATE shared_booking_intents SET phase='released',slot_held=0,writer_held=0,
            outcome='released_unclaimed',updated_ms=?2 WHERE work_unit_id=?1",
            params![id, now_ms],
        )?;
        conn.execute(
            "UPDATE auto_pool_claims SET state='released',released_ms=?2
            WHERE work_unit_id=?1 AND state IN ('active','uncertain')",
            params![id, now_ms],
        )?;
    }
    Ok(orphaned.len())
}

/// One window of the structured observation a booking must cite: its pool,
/// its window key and its reported remaining allowance in thousandths of a
/// percentage point. None when the cited observation is not the latest for
/// that profile's account (the booking would refuse it anyway).
pub fn cited_windows_in_tx(
    conn: &Connection,
    quota_profile_id: &str,
    quota_event_seq: i64,
) -> Result<Option<Vec<(String, String, i64)>>> {
    let fingerprint: Option<String> = conn
        .query_row(
            "SELECT fingerprint FROM auto_account_identity WHERE profile_id=?1",
            [quota_profile_id],
            |row| row.get(0),
        )
        .optional()?;
    let Some(fingerprint) = fingerprint else {
        return Ok(None);
    };
    let observation: Option<(i64, String)> = conn
        .query_row(
            "SELECT event_seq,snapshot FROM auto_quota_observations WHERE pool_id=?1
         ORDER BY observed_ms DESC,event_seq DESC LIMIT 1",
            [quota_profile_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let Some((event_seq, encoded)) = observation else {
        return Ok(None);
    };
    if event_seq != quota_event_seq {
        return Ok(None);
    }
    let quota: QuotaSnapshot = serde_json::from_str(&encoded)?;
    let pool = format!("account/{fingerprint}");
    let mut windows = Vec::with_capacity(quota.windows.len());
    for window in &quota.windows {
        if !window.used_percent.is_finite() || !(0.0..=100.0).contains(&window.used_percent) {
            return Ok(None);
        }
        windows.push((
            pool.clone(),
            window_key(window)?,
            ((100.0 - window.used_percent) * 1000.0).floor() as i64,
        ));
    }
    Ok(Some(windows))
}

#[derive(Debug, PartialEq, Eq)]
pub enum LaunchBookingDecision {
    Booked(SharedLaunchIntent),
    Replayed(SharedLaunchIntent),
    Blocked(&'static str),
}

fn launch_intent(conn: &Connection, id: &str) -> Result<Option<SharedLaunchIntent>> {
    Ok(conn
        .query_row(
            "SELECT work_unit_id,route_id,profile_id,account_generation,phase,
        workspace_path,slot_held,writer_held,effects_claimed_ms,run_id,settled_ms,outcome
        FROM shared_booking_intents WHERE work_unit_id=?1 AND launch_hash IS NOT NULL",
            [id],
            |row| {
                Ok(SharedLaunchIntent {
                    work_unit_id: row.get(0)?,
                    route_id: row.get(1)?,
                    profile_id: row.get(2)?,
                    account_generation: row.get(3)?,
                    phase: row.get(4)?,
                    workspace_path: row.get(5)?,
                    slot_held: row.get(6)?,
                    writer_held: row.get(7)?,
                    effects_claimed_ms: row.get(8)?,
                    run_id: row.get(9)?,
                    settled_ms: row.get(10)?,
                    outcome: row.get(11)?,
                })
            },
        )
        .optional()?)
}

/// Reserve account windows, an app slot and the planned workspace writer in
/// one transaction. The returned intent is inspectable state, not permission
/// to attempt effects: the launch worker must win claim_shared_launch_effects.
pub fn book_shared_launch_in_tx(
    conn: &Connection,
    req: &LaunchBookingRequest<'_>,
) -> Result<LaunchBookingDecision> {
    if conn.is_autocommit() {
        return Err(anyhow!(
            "shared launch booking requires an admission transaction"
        ));
    }
    if !valid_label(req.launch_hash, 128)
        || req.workspace_path.is_some_and(|path| {
            !valid_label(path, 4096)
                || !std::path::Path::new(path).is_absolute()
                || std::path::Path::new(path).components().any(|part| {
                    matches!(
                        part,
                        std::path::Component::ParentDir | std::path::Component::CurDir
                    )
                })
        })
    {
        return Err(anyhow!("invalid shared launch resource identity"));
    }
    let resource_hash = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&(
            req.launch_hash,
            req.workspace_path,
            req.consume_agent_slot
        ))?)
    );
    let prior: Option<Option<String>> = conn
        .query_row(
            "SELECT launch_hash FROM shared_booking_intents WHERE work_unit_id=?1",
            [req.account.id],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(prior_hash) = prior {
        if prior_hash.as_deref() != Some(&resource_hash) {
            return Err(anyhow!(
                "shared launch id reused with different resource inputs"
            ));
        }
        match book_shared_account_in_tx(conn, req.account)? {
            BookingDecision::Replayed => {
                return Ok(LaunchBookingDecision::Replayed(
                    launch_intent(conn, req.account.id)?
                        .ok_or_else(|| anyhow!("shared launch intent disappeared"))?,
                ))
            }
            _ => return Err(anyhow!("shared launch replay lost its account booking")),
        }
    }
    if let Some(path) = req.workspace_path {
        let held: bool = conn.query_row(
            &format!(
                "SELECT EXISTS(SELECT 1 FROM shared_booking_intents b
            WHERE b.workspace_path=?1 AND b.writer_held=1 AND {})",
                held_intent_sql("b")
            ),
            [path],
            |row| row.get(0),
        )?;
        let active_writer: bool = conn.query_row(
            &format!(
                "SELECT EXISTS(SELECT 1 FROM runs r
            JOIN workspaces w ON w.id=r.workspace_id WHERE w.path=?1 AND w.removed_ms IS NULL
            AND (r.status IN ({HOLDING_STATUSES})
                OR EXISTS(SELECT 1 FROM turns t WHERE t.run_id=r.id
                    AND t.status='running' AND t.ended_ms IS NULL)) AND {})",
                unbound_run_sql("r")
            ),
            [path],
            |row| row.get(0),
        )?;
        let planned_writer: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM auto_launch_intents i
            JOIN auto_pool_claims c ON c.work_unit_id=i.work_unit_id
            WHERE i.planned_path=?1 AND c.state IN ('active','uncertain'))",
            [path],
            |row| row.get(0),
        )?;
        if held || active_writer || planned_writer {
            return Ok(LaunchBookingDecision::Blocked("workspace_writer_busy"));
        }
    }
    if req.consume_agent_slot && app_slots_in_use(conn)? >= app_slot_limit(conn)? {
        return Ok(LaunchBookingDecision::Blocked("global_agent_limit"));
    }
    match book_shared_account_in_tx(conn, req.account)? {
        BookingDecision::Blocked(reason) => return Ok(LaunchBookingDecision::Blocked(reason)),
        BookingDecision::Replayed => {
            return Err(anyhow!(
                "account booking cannot be promoted to a launch implicitly"
            ))
        }
        BookingDecision::Booked => {}
    }
    conn.execute(
        "UPDATE shared_booking_intents SET launch_hash=?2,workspace_path=?3,
        slot_held=?4,writer_held=?5 WHERE work_unit_id=?1",
        params![
            req.account.id,
            resource_hash,
            req.workspace_path,
            req.consume_agent_slot,
            req.workspace_path.is_some()
        ],
    )?;
    Ok(LaunchBookingDecision::Booked(
        launch_intent(conn, req.account.id)?
            .ok_or_else(|| anyhow!("shared launch intent was not recorded"))?,
    ))
}

fn window_key(window: &crate::auto_quota::QuotaWindow) -> Result<String> {
    let encoded = serde_json::to_vec(&(
        &window.bucket_id,
        &window.window,
        &window.model,
        &window.model_family,
        &window.plan_type,
        window.reset_ms,
    ))?;
    Ok(format!("{:x}", Sha256::digest(&encoded)))
}

fn valid_label(label: &str, max: usize) -> bool {
    !label.is_empty() && label.len() <= max && !label.chars().any(char::is_control)
}

/// A run on this account that may be drawing with no known-window booking:
/// an ordinary unbooked run, or an Auto unit on an unknown-draw claim. Its
/// draw is unknown, so no known-window booking can share the account.
pub fn unbooked_run_on_account(conn: &Connection, fingerprint: &str) -> Result<bool> {
    Ok(conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM runs r JOIN auto_account_identity a ON a.profile_id=r.profile_id
         WHERE a.fingerprint=?1 AND (r.status IN ('queued','starting','running','waiting_for_user')
           OR EXISTS(SELECT 1 FROM turns t WHERE t.run_id=r.id
              AND t.status='running' AND t.ended_ms IS NULL))
           -- A bound run's draw is already committed in its windows.
           AND NOT EXISTS(SELECT 1 FROM shared_booking_intents b
              WHERE b.run_id=r.id AND b.settled_ms IS NULL)
           -- So is an Auto root's or child's, booked under its work unit.
           AND NOT EXISTS(SELECT 1 FROM shared_booking_intents b
              WHERE b.caller='auto' AND b.settled_ms IS NULL AND b.work_unit_id IN (
                  SELECT work_unit_id FROM auto_root_intents WHERE run_id=r.id
                  UNION SELECT work_unit_id FROM managed_work_units WHERE child_run_id=r.id)))",
        [fingerprint], |row| row.get(0))?)
}

/// The room a window leaves for new work, in thousandths of a reported
/// percentage point. A reading may be rounded by up to one whole point
/// until an adapter proves its meter's precision (the qualified upper
/// draw's reading error), so one point is held back: coarse precision can
/// only make a booking refuse, never admit work the meter could not take.
pub fn window_headroom_milli(used_percent: f64) -> i64 {
    ((100.0 - used_percent) * 1000.0).floor() as i64 - crate::upper_draw::READING_ERROR_MILLI
}

/// Draw already committed to one window of an account by live bookings.
fn committed_window_milli(conn: &Connection, account_pool: &str, key: &str) -> Result<i64> {
    Ok(conn.query_row(
        "SELECT COALESCE(SUM(w.amount_milli),0) FROM shared_booking_windows w
         JOIN auto_pool_claims c ON c.work_unit_id=w.work_unit_id
         WHERE w.pool_id=?1 AND w.window_key=?2 AND c.state IN ('active','uncertain')",
        params![account_pool, key],
        |row| row.get(0),
    )?)
}

/// One window of a selection-time fit preview, in thousandths of a
/// reported percentage point. Recorded in the decision trace so a replay
/// recomputes the same fit from the same numbers.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct WindowFitInput {
    pub bucket_id: String,
    pub window: String,
    /// [`window_headroom_milli`] of the cited reading.
    pub headroom_milli: i64,
    /// Live known-window bookings on the account in this window.
    pub committed_milli: i64,
    /// The bucket's qualified upper draw in this window.
    pub upper_draw_milli: i64,
}

#[derive(Debug, PartialEq)]
pub enum AccountFitPreview {
    Windows {
        quota_event_seq: i64,
        observed_ms: i64,
        sample_count: usize,
        windows: Vec<WindowFitInput>,
    },
    Unknown(&'static str),
}

/// What the one booking would compare for a route of `bucket` on
/// `profile_id`'s account, without writing anything: the latest reading's
/// per-window headroom, the live commitments and the qualified upper draw.
/// Selection uses it to exclude a route whose known draw cannot fit and to
/// prefer one that fits; admission books (and rechecks) in its own
/// transaction. Anything the booking would refuse before comparing windows
/// is `Unknown`, never a fit.
pub fn preview_account_fit(
    conn: &Connection,
    profile_id: &str,
    bucket: &crate::upper_draw::DrawBucket,
    now_ms: i64,
) -> Result<AccountFitPreview> {
    preview_account_fit_with(conn, profile_id, PreviewDraw::Qualified(bucket), now_ms)
}

/// The draw a preview compares: the qualified draw of a bucket, or a fixture
/// caller's draw citing one observation (behind the booking's fixture API).
pub enum PreviewDraw<'a> {
    Qualified(&'a crate::upper_draw::DrawBucket),
    Fixture { quota_event_seq: i64, upper_draw_milli: &'a [i64] },
}

/// [`preview_account_fit`] for either kind of draw.
pub fn preview_account_fit_with(
    conn: &Connection,
    profile_id: &str,
    draw: PreviewDraw<'_>,
    now_ms: i64,
) -> Result<AccountFitPreview> {
    let fingerprint: Option<String> = conn
        .query_row(
            "SELECT fingerprint FROM auto_account_identity WHERE profile_id=?1",
            [profile_id],
            |row| row.get(0),
        )
        .optional()?;
    let Some(fingerprint) = fingerprint else {
        return Ok(AccountFitPreview::Unknown("account_identity_unknown"));
    };
    let observation: Option<(i64, String)> = conn
        .query_row(
            "SELECT event_seq,snapshot FROM auto_quota_observations WHERE pool_id=?1
         ORDER BY observed_ms DESC,event_seq DESC LIMIT 1",
            [profile_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let Some((event_seq, encoded)) = observation else {
        return Ok(AccountFitPreview::Unknown("quota_unknown"));
    };
    let quota: QuotaSnapshot = serde_json::from_str(&encoded)?;
    if quota.needs_refresh(now_ms)
        || quota
            .native_uncertain_until_ms
            .is_some_and(|until| now_ms < until)
    {
        return Ok(AccountFitPreview::Unknown("snapshot_expired"));
    }
    match quota.ordinary_usage_allowed {
        Some(true) => {}
        Some(false) => return Ok(AccountFitPreview::Unknown("account_exhausted")),
        None => return Ok(AccountFitPreview::Unknown("account_allowance_unknown")),
    }
    let (upper_draw_milli, sample_count) = match draw {
        PreviewDraw::Qualified(bucket) =>
            match crate::upper_draw::qualified_upper_draw_in_tx(conn, profile_id, event_seq, bucket, now_ms)? {
                crate::upper_draw::UpperDraw::Qualified { upper_draw_milli, provenance } => {
                    (upper_draw_milli, provenance.sample_count)
                }
                crate::upper_draw::UpperDraw::Unknown { .. } => {
                    return Ok(AccountFitPreview::Unknown("upper_draw_unknown"))
                }
            },
        PreviewDraw::Fixture { quota_event_seq, upper_draw_milli } => {
            if quota_event_seq != event_seq {
                return Ok(AccountFitPreview::Unknown("snapshot_superseded"));
            }
            (upper_draw_milli.to_vec(), 0)
        }
    };
    if quota.windows.is_empty() || quota.windows.len() > 32 || upper_draw_milli.len() != quota.windows.len() {
        return Ok(AccountFitPreview::Unknown("upper_draw_unknown"));
    }
    let account_pool = format!("account/{fingerprint}");
    let mut windows = Vec::with_capacity(quota.windows.len());
    for (n, window) in quota.windows.iter().enumerate() {
        if !(1..=100_000).contains(&upper_draw_milli[n])
            || !window.used_percent.is_finite()
            || !(0.0..=100.0).contains(&window.used_percent)
        {
            return Ok(AccountFitPreview::Unknown("upper_draw_unknown"));
        }
        let key = window_key(window)?;
        windows.push(WindowFitInput {
            bucket_id: window.bucket_id.clone(),
            window: window.window.clone(),
            headroom_milli: window_headroom_milli(window.used_percent),
            committed_milli: committed_window_milli(conn, &account_pool, &key)?,
            upper_draw_milli: upper_draw_milli[n],
        });
    }
    Ok(AccountFitPreview::Windows {
        quota_event_seq: event_seq,
        observed_ms: quota.observed_ms,
        sample_count,
        windows,
    })
}

/// Called inside the *caller's* IMMEDIATE SQLite transaction. This is the
/// integration seam for Swarm's existing admission transaction. The same
/// auto_pool_claims row is the account booking for all caller types; no
/// second balance or allowance ledger is introduced.
pub fn book_shared_account_in_tx(
    conn: &Connection,
    req: &AccountBookingRequest<'_>,
) -> Result<BookingDecision> {
    if conn.is_autocommit() {
        return Err(anyhow!(
            "shared account booking requires an admission transaction"
        ));
    }
    if !valid_label(req.id, 128)
        || !valid_label(req.request_hash, 128)
        || !valid_label(req.caller, 32)
        || !valid_label(req.route_id, 256)
        || !valid_label(req.profile_id, 128)
        || !valid_label(req.quota_profile_id, 128)
        || req.account_generation < 1
        || req.quota_event_seq < 1
    {
        return Err(anyhow!("invalid shared booking identity"));
    }
    // A fixture draw is caller input and binds the booking. A qualified draw
    // and the observation it cites are derived when the booking is made, so
    // a replay of the same request binds to its bucket, not to a newer
    // reading that the replay happens to see.
    let request_hash = format!(
        "{:x}",
        Sha256::digest(match req.draw {
            BookingDraw::Fixture(upper_draw_milli) => serde_json::to_vec(&(
                req.request_hash,
                req.caller,
                req.route_id,
                req.profile_id,
                req.quota_profile_id,
                req.account_generation,
                req.quota_event_seq,
                upper_draw_milli,
                req.allocation_remaining_milli
            ))?,
            BookingDraw::Qualified(bucket) => serde_json::to_vec(&(
                req.request_hash,
                req.caller,
                req.route_id,
                req.profile_id,
                req.quota_profile_id,
                req.account_generation,
                "qualified",
                bucket
            ))?,
        })
    );
    let previous: Option<String> = conn
        .query_row(
            "SELECT request_hash FROM shared_booking_intents WHERE work_unit_id=?1",
            [req.id],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(hash) = previous {
        if hash != request_hash {
            return Err(anyhow!("shared booking id reused with different input"));
        }
        return Ok(BookingDecision::Replayed);
    }
    settle_finished_shared_launches_in_tx(conn, req.now_ms)?;
    let profile_identity: Option<(String, i64)> = conn
        .query_row(
            "SELECT fingerprint,generation FROM auto_account_identity WHERE profile_id=?1",
            [req.profile_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let quota_identity: Option<String> = conn
        .query_row(
            "SELECT fingerprint FROM auto_account_identity WHERE profile_id=?1",
            [req.quota_profile_id],
            |row| row.get(0),
        )
        .optional()?;
    let Some((fingerprint, generation)) = profile_identity else {
        return Ok(BookingDecision::Blocked("account_identity_unknown"));
    };
    if generation != req.account_generation || quota_identity.as_deref() != Some(&fingerprint) {
        return Ok(BookingDecision::Blocked("account_identity_changed"));
    }
    let account_pool = format!("account/{fingerprint}");
    let observation: Option<(i64, String)> = conn
        .query_row(
            "SELECT event_seq,snapshot FROM auto_quota_observations WHERE pool_id=?1
         ORDER BY observed_ms DESC,event_seq DESC LIMIT 1",
            [req.quota_profile_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let Some((event_seq, encoded)) = observation else {
        return Ok(BookingDecision::Blocked("quota_unknown"));
    };
    if event_seq != req.quota_event_seq {
        return Ok(BookingDecision::Blocked("snapshot_superseded"));
    }
    let quota: QuotaSnapshot = serde_json::from_str(&encoded)?;
    if quota.needs_refresh(req.now_ms)
        || quota
            .native_uncertain_until_ms
            .is_some_and(|until| req.now_ms < until)
    {
        return Ok(BookingDecision::Blocked("snapshot_expired"));
    }
    // Only an explicit allowance books. A reading that reports none (a
    // Claude `allowed` rate-limit event) is unknown, not exhausted.
    match quota.ordinary_usage_allowed {
        Some(true) => {}
        Some(false) => return Ok(BookingDecision::Blocked("account_exhausted")),
        None => return Ok(BookingDecision::Blocked("account_allowance_unknown")),
    }
    let (upper_draw_milli, draw_source, draw_provenance) = match req.draw {
        BookingDraw::Fixture(draws) => (draws.to_vec(), "fixture", None),
        BookingDraw::Qualified(bucket) => match crate::upper_draw::qualified_upper_draw_in_tx(
            conn,
            req.quota_profile_id,
            req.quota_event_seq,
            bucket,
            req.now_ms,
        )? {
            crate::upper_draw::UpperDraw::Qualified {
                upper_draw_milli,
                provenance,
            } => (
                upper_draw_milli,
                "qualified",
                Some(serde_json::to_string(&provenance)?),
            ),
            crate::upper_draw::UpperDraw::Unknown { .. } => {
                return Ok(BookingDecision::Blocked("upper_draw_unknown"))
            }
        },
    };
    if quota.windows.is_empty()
        || quota.windows.len() > 32
        || upper_draw_milli.len() != quota.windows.len()
        || req
            .allocation_remaining_milli
            .is_some_and(|a| a.len() != quota.windows.len())
    {
        return Ok(BookingDecision::Blocked("upper_draw_unknown"));
    }
    let legacy_claim: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM auto_pool_claims c
         WHERE (c.pool_id=?1 OR c.pool_id='legacy/unresolved')
           AND c.state IN ('active','uncertain')
           AND NOT EXISTS(SELECT 1 FROM shared_booking_intents i WHERE i.work_unit_id=c.work_unit_id))",
        [&account_pool], |row| row.get(0))?;
    if legacy_claim {
        return Ok(BookingDecision::Blocked("account_pool_busy"));
    }
    if unbooked_run_on_account(conn, &fingerprint)? {
        return Ok(BookingDecision::Blocked("account_pool_busy"));
    }
    let mut windows = Vec::with_capacity(quota.windows.len());
    let mut seen = HashSet::new();
    for (n, window) in quota.windows.iter().enumerate() {
        let key = window_key(window)?;
        if !seen.insert(key.clone()) {
            return Ok(BookingDecision::Blocked("duplicate_window"));
        }
        let amount = upper_draw_milli[n];
        if !(1..=100_000).contains(&amount)
            || !window.used_percent.is_finite()
            || !(0.0..=100.0).contains(&window.used_percent)
        {
            return Ok(BookingDecision::Blocked("upper_draw_unknown"));
        }
        if req
            .allocation_remaining_milli
            .is_some_and(|a| amount > a[n])
        {
            return Ok(BookingDecision::Blocked("allocation_exhausted"));
        }
        let remaining = window_headroom_milli(window.used_percent);
        let committed = committed_window_milli(conn, &account_pool, &key)?;
        if amount > remaining.saturating_sub(committed) {
            return Ok(BookingDecision::Blocked("shared_pool_headroom"));
        }
        windows.push((key, amount));
    }
    conn.execute(
        "INSERT INTO shared_booking_intents(work_unit_id,request_hash,caller,route_id,
        profile_id,quota_profile_id,account_generation,quota_event_seq,phase,created_ms,updated_ms,
        draw_source,draw_provenance)
        VALUES(?1,?2,?3,?4,?5,?6,?7,?8,'booked',?9,?9,?10,?11)",
        params![
            req.id,
            request_hash,
            req.caller,
            req.route_id,
            req.profile_id,
            req.quota_profile_id,
            req.account_generation,
            req.quota_event_seq,
            req.now_ms,
            draw_source,
            draw_provenance
        ],
    )?;
    conn.execute(
        "INSERT INTO auto_pool_claims(work_unit_id,pool_id,account_generation,state,created_ms)
        VALUES(?1,?2,?3,'active',?4)",
        params![req.id, account_pool, req.account_generation, req.now_ms],
    )?;
    for (key, amount) in windows {
        conn.execute(
            "INSERT INTO shared_booking_windows(work_unit_id,pool_id,window_key,amount_milli)
            VALUES(?1,?2,?3,?4)",
            params![req.id, account_pool, key, amount],
        )?;
    }
    Ok(BookingDecision::Booked)
}

/// Bind a claimed launch to the run it became, in the transaction that
/// commits that run's row. From then on the intent's slot and writer holds
/// *are* the run's occupancy: the run is excluded from unbound-run counts,
/// so a held booking that becomes a process is counted once. The run must
/// still be queued with no supervisor identity: a caller spawns only after
/// this commit, so an unbound claimed intent has no model process.
pub fn bind_shared_launch_run_in_tx(
    conn: &Connection,
    id: &str,
    run_id: &str,
    now_ms: i64,
) -> Result<()> {
    if conn.is_autocommit() {
        return Err(anyhow!(
            "shared launch binding requires the run transaction"
        ));
    }
    let intent = launch_intent(conn, id)?.ok_or_else(|| anyhow!("shared launch is not booked"))?;
    let claim: Option<String> = conn
        .query_row(
            "SELECT state FROM auto_pool_claims WHERE work_unit_id=?1",
            [id],
            |row| row.get(0),
        )
        .optional()?;
    if intent.phase != "uncertain"
        || intent.effects_claimed_ms.is_none()
        || intent.run_id.is_some()
        || intent.outcome.is_some()
        || intent.settled_ms.is_some()
        || claim.as_deref() != Some("uncertain")
    {
        return Err(anyhow!(
            "shared launch cannot bind a run: effects are not claimed by this worker"
        ));
    }
    let run: Option<(Option<String>, String, Option<String>, String)> = conn
        .query_row(
            "SELECT r.profile_id,r.status,r.run_dir,w.path FROM runs r
            JOIN workspaces w ON w.id=r.workspace_id WHERE r.id=?1",
            [run_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()?;
    let Some((profile, status, run_dir, path)) = run else {
        return Err(anyhow!("shared launch run is not recorded"));
    };
    if profile.as_deref() != Some(intent.profile_id.as_str())
        || status != "queued"
        || run_dir.is_some()
        || intent
            .workspace_path
            .as_deref()
            .is_some_and(|held| held != path)
    {
        return Err(anyhow!(
            "shared launch run does not match its booked profile or workspace"
        ));
    }
    let changed = conn.execute(
        "UPDATE shared_booking_intents SET run_id=?2,bound_ms=?3,updated_ms=?3
        WHERE work_unit_id=?1 AND run_id IS NULL",
        params![id, run_id, now_ms],
    )?;
    if changed != 1 {
        return Err(anyhow!("shared launch was bound concurrently"));
    }
    Ok(())
}

#[derive(Debug, PartialEq, Eq)]
pub enum LaunchSettlement {
    /// No supervisor was ever recorded: no model process could have run, so
    /// the account commitment is released with the slot and writer.
    NotStarted,
    /// A process ran and is confirmed gone: slot and writer are released;
    /// the account commitment is retained until a provider observation
    /// taken after settlement can include the work's actual draw.
    Retained,
}

/// Called in the run's terminal transaction. `process_gone` is the caller's
/// confirmation that neither the supervisor nor the harness can still act;
/// without it (a lost supervisor whose harness may live) every hold stays.
pub fn settle_shared_launch_run_in_tx(
    conn: &Connection,
    run_id: &str,
    process_gone: bool,
    now_ms: i64,
) -> Result<Option<LaunchSettlement>> {
    if conn.is_autocommit() {
        return Err(anyhow!(
            "shared launch settlement requires the run transaction"
        ));
    }
    if !process_gone {
        return Ok(None);
    }
    // A refused first spawn clears run_dir after advancing the generation
    // to 1; a refused later spawn (a Continuity retry) also clears it, but
    // an earlier generation may already have spent allowance.
    let bound: Option<(String, bool)> = conn
        .query_row(
            "SELECT i.work_unit_id,r.run_dir IS NULL AND r.process_generation<=1
            FROM shared_booking_intents i
            JOIN runs r ON r.id=i.run_id WHERE i.run_id=?1 AND i.settled_ms IS NULL",
            [run_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let Some((id, never_started)) = bound else {
        return Ok(None);
    };
    if never_started {
        conn.execute(
            "UPDATE shared_booking_intents SET phase='released',slot_held=0,writer_held=0,
            settled_ms=?2,outcome='not_started',updated_ms=?2 WHERE work_unit_id=?1",
            params![id, now_ms],
        )?;
        conn.execute(
            "UPDATE auto_pool_claims SET state='released',released_ms=?2
            WHERE work_unit_id=?1 AND state IN ('active','uncertain')",
            params![id, now_ms],
        )?;
        return Ok(Some(LaunchSettlement::NotStarted));
    }
    conn.execute(
        "UPDATE shared_booking_intents SET slot_held=0,writer_held=0,
        settled_ms=?2,outcome='settled',updated_ms=?2 WHERE work_unit_id=?1",
        params![id, now_ms],
    )?;
    Ok(Some(LaunchSettlement::Retained))
}

/// A bound run that left every holding status by any path (including a
/// daemon stop that marks runs interrupted directly) no longer owns a
/// process. Settle its holds; lost `disconnected`/`unknown` runs are left
/// for the caller that can check their processes.
pub fn settle_finished_shared_launches_in_tx(conn: &Connection, now_ms: i64) -> Result<usize> {
    let finished: Vec<String> = {
        let mut stmt = conn.prepare(&format!(
            "SELECT i.run_id FROM shared_booking_intents i JOIN runs r ON r.id=i.run_id
            WHERE i.settled_ms IS NULL AND NOT (r.status IN ({HOLDING_STATUSES})
                OR EXISTS(SELECT 1 FROM turns t WHERE t.run_id=r.id
                    AND t.status='running' AND t.ended_ms IS NULL))"
        ))?;
        let rows = stmt.query_map([], |row| row.get(0))?;
        rows.collect::<rusqlite::Result<_>>()?
    };
    for run_id in &finished {
        settle_shared_launch_run_in_tx(conn, run_id, true, now_ms)?;
    }
    Ok(finished.len())
}

/// A retained commitment is released once a structured observation for the
/// same account was taken, in every window, after the run settled. That
/// observation is the one a later booking must cite, so the actual draw is
/// counted in its reported usage instead of in this reservation. This is
/// not a qualified attribution of the draw to this work.
pub fn release_observed_settled_bookings(
    conn: &Connection,
    quota_profile_id: &str,
    snapshot: &QuotaSnapshot,
    now_ms: i64,
) -> Result<usize> {
    if snapshot.windows.is_empty()
        || snapshot
            .native_uncertain_until_ms
            .is_some_and(|until| now_ms < until)
    {
        return Ok(0);
    }
    let observed = snapshot
        .windows
        .iter()
        .map(|window| window.observed_ms)
        .min()
        .unwrap_or(i64::MIN)
        .min(snapshot.observed_ms);
    let fingerprint: Option<String> = conn
        .query_row(
            "SELECT fingerprint FROM auto_account_identity WHERE profile_id=?1",
            [quota_profile_id],
            |row| row.get(0),
        )
        .optional()?;
    let Some(fingerprint) = fingerprint else {
        return Ok(0);
    };
    let pool = format!("account/{fingerprint}");
    conn.execute_batch("SAVEPOINT shared_booking_observed")?;
    let released = (|| -> Result<usize> {
        let released = conn.execute(
            "UPDATE auto_pool_claims SET state='released',released_ms=?3
            WHERE pool_id=?1 AND state IN ('active','uncertain') AND work_unit_id IN
              (SELECT work_unit_id FROM shared_booking_intents
               WHERE outcome='settled' AND settled_ms < ?2)",
            params![pool, observed, now_ms],
        )?;
        conn.execute(
            "UPDATE shared_booking_intents SET phase='released',
            outcome='observed_after_settlement',updated_ms=?3
            WHERE outcome='settled' AND settled_ms < ?2 AND work_unit_id IN
              (SELECT work_unit_id FROM auto_pool_claims WHERE pool_id=?1 AND state='released')",
            params![pool, observed, now_ms],
        )?;
        Ok(released)
    })();
    match released {
        Ok(count) => {
            conn.execute_batch("RELEASE shared_booking_observed")?;
            Ok(count)
        }
        Err(error) => {
            let _ = conn.execute_batch(
                "ROLLBACK TO shared_booking_observed; RELEASE shared_booking_observed",
            );
            Err(error)
        }
    }
}

/// A claimed launch that never bound a run cannot have started a model
/// process, but its Git effect is unknown. Release the account commitment
/// and the app slot; keep the workspace writer and the intent paused for
/// reconciliation. A replay never re-attempts it.
fn release_unbound_shared_launch_in_tx(conn: &Connection, id: &str, now_ms: i64) -> Result<bool> {
    let changed = conn.execute(
        "UPDATE shared_booking_intents SET slot_held=0,outcome='effects_uncertain',updated_ms=?2
        WHERE work_unit_id=?1 AND phase='uncertain' AND launch_hash IS NOT NULL
          AND run_id IS NULL AND outcome IS NULL",
        params![id, now_ms],
    )? == 1;
    if changed {
        conn.execute(
            "UPDATE auto_pool_claims SET state='released',released_ms=?2
            WHERE work_unit_id=?1 AND state IN ('active','uncertain')",
            params![id, now_ms],
        )?;
    }
    Ok(changed)
}

impl Store {
    pub fn book_shared_launch(
        &self,
        req: &LaunchBookingRequest<'_>,
    ) -> Result<LaunchBookingDecision> {
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        let result = book_shared_launch_in_tx(&tx, req)?;
        tx.commit()?;
        Ok(result)
    }

    /// Exclusive durable permission for this launch worker to attempt effects.
    /// Record uncertainty *before* Git/spawn/stdin, so reconnect/restart cannot
    /// turn an unobserved outcome into a second attempt.
    pub fn claim_shared_launch_effects(&self, id: &str) -> Result<bool> {
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        let changed = tx.execute(
            "UPDATE shared_booking_intents SET phase='uncertain',
            effects_claimed_ms=?2,updated_ms=?2 WHERE work_unit_id=?1 AND phase='booked'
            AND launch_hash IS NOT NULL AND effects_claimed_ms IS NULL",
            params![id, crate::daemon::now()],
        )? == 1;
        if changed {
            tx.execute(
                "UPDATE auto_pool_claims SET state='uncertain'
                WHERE work_unit_id=?1 AND state='active'",
                [id],
            )?;
        }
        tx.commit()?;
        Ok(changed)
    }
    pub fn book_shared_account(&self, req: &AccountBookingRequest<'_>) -> Result<BookingDecision> {
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        let result = book_shared_account_in_tx(&tx, req)?;
        tx.commit()?;
        Ok(result)
    }

    /// A confirmed pre-effect failure can release the held capacity. Once a
    /// launch is uncertain, this path cannot assert that no allowance was used.
    pub fn release_shared_booking_pre_effect(&self, id: &str) -> Result<bool> {
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        let changed = tx.execute(
            "UPDATE shared_booking_intents SET phase='released',updated_ms=?2,slot_held=0,writer_held=0
            WHERE work_unit_id=?1 AND phase='booked'",
            params![id, crate::daemon::now()],
        )? == 1;
        if changed {
            tx.execute(
                "UPDATE auto_pool_claims SET state='released',released_ms=?2
                WHERE work_unit_id=?1 AND state='active'",
                params![id, crate::daemon::now()],
            )?;
        }
        tx.commit()?;
        Ok(changed)
    }

    /// Settle a bound run inside the caller's terminal-run savepoint.
    pub fn settle_shared_launch_run(
        &self,
        run_id: &str,
        process_gone: bool,
    ) -> Result<Option<LaunchSettlement>> {
        settle_shared_launch_run_in_tx(&self.conn, run_id, process_gone, crate::daemon::now())
    }

    /// A run lost by an earlier daemon, whose supervisor and harness the
    /// caller has now confirmed gone.
    pub fn settle_lost_shared_launch(&self, run_id: &str) -> Result<bool> {
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        let settled = settle_shared_launch_run_in_tx(&tx, run_id, true, crate::daemon::now())?;
        tx.commit()?;
        Ok(settled.is_some())
    }

    pub fn shared_launch_intent(&self, id: &str) -> Result<Option<SharedLaunchIntent>> {
        launch_intent(&self.conn, id)
    }

    /// Where a booking's upper draw came from (`fixture` or `qualified`,
    /// None for a booking made before this was recorded) and, when
    /// qualified, its provenance: bucket, samples and their observations.
    pub fn shared_booking_draw(
        &self,
        id: &str,
    ) -> Result<Option<(Option<String>, Option<crate::upper_draw::DrawProvenance>)>> {
        let row: Option<(Option<String>, Option<String>)> = self
            .conn
            .query_row(
                "SELECT draw_source,draw_provenance FROM shared_booking_intents WHERE work_unit_id=?1",
                [id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        row.map(|(source, provenance)| {
            Ok((
                source,
                provenance
                    .map(|encoded| serde_json::from_str(&encoded))
                    .transpose()?,
            ))
        })
        .transpose()
    }

    /// True while a run is the unsettled result of a shared launch booking:
    /// its admission was that booking, and its supervisor must be recorded
    /// before spawn so recovery can tell an unstarted run from a lost one.
    pub fn shared_launch_bound_unsettled(&self, run_id: &str) -> Result<bool> {
        Ok(self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM shared_booking_intents
            WHERE run_id=?1 AND settled_ms IS NULL)",
            [run_id],
            |row| row.get(0),
        )?)
    }

    /// The shared launch that holds this path's single writer, if any,
    /// other than the caller's own booking or bound run.
    pub fn shared_writer_hold(
        &self,
        path: &str,
        except_work_unit: Option<&str>,
        except_run: Option<&str>,
    ) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row(
                &format!(
                    "SELECT b.work_unit_id FROM shared_booking_intents b
                WHERE b.workspace_path=?1 AND b.writer_held=1 AND {}
                  AND (?2 IS NULL OR b.work_unit_id<>?2)
                  AND (?3 IS NULL OR b.run_id IS NULL OR b.run_id<>?3)
                ORDER BY b.work_unit_id LIMIT 1",
                    held_intent_sql("b")
                ),
                params![path, except_work_unit, except_run],
                |row| row.get(0),
            )
            .optional()?)
    }

    /// The claiming worker stopped before binding a run (an error or a
    /// dropped worker). See release_unbound_shared_launch_in_tx.
    pub fn release_unbound_shared_launch(&self, id: &str) -> Result<bool> {
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        let changed = release_unbound_shared_launch_in_tx(&tx, id, crate::daemon::now())?;
        tx.commit()?;
        Ok(changed)
    }

    /// Startup reconciliation, before any supervisor is reattached. The old
    /// daemon's launch workers are gone. A booked but unclaimed intent never
    /// requested an effect and is released. A claimed intent with no bound
    /// run keeps its writer for reconciliation. A bound run that already
    /// left every holding status is settled. Bound runs that may still own a
    /// process are left to supervisor reconciliation, which reattaches them
    /// (the binding stays) or settles them once their processes are gone.
    pub fn reconcile_shared_launches_on_start(&self) -> Result<Vec<serde_json::Value>> {
        let now = crate::daemon::now();
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        let mut report = Vec::new();
        // The daemon's own launch requests are gone: their slot holds go.
        tx.execute("DELETE FROM app_slot_holds", [])?;
        release_orphaned_swarm_bookings_in_tx(&tx, now)?;
        // A booked Swarm worker belongs to its durable, still registered
        // attempt, which dispatch recovery launches again: it keeps its
        // booking. Every other unclaimed booking had only a request.
        let unclaimed: Vec<String> = {
            let mut stmt = tx.prepare(
                "SELECT work_unit_id FROM shared_booking_intents
                WHERE phase='booked' AND launch_hash IS NOT NULL AND effects_claimed_ms IS NULL
                  AND NOT (caller='swarm' AND EXISTS(SELECT 1 FROM swarm_attempts a
                      WHERE 'swarm/'||a.id=work_unit_id AND a.status='registered'))
                ORDER BY work_unit_id",
            )?;
            let rows = stmt.query_map([], |row| row.get(0))?;
            rows.collect::<rusqlite::Result<_>>()?
        };
        for id in unclaimed {
            tx.execute(
                "UPDATE shared_booking_intents SET phase='released',slot_held=0,writer_held=0,
                outcome='released_unclaimed',updated_ms=?2 WHERE work_unit_id=?1",
                params![id, now],
            )?;
            tx.execute(
                "UPDATE auto_pool_claims SET state='released',released_ms=?2
                WHERE work_unit_id=?1 AND state IN ('active','uncertain')",
                params![id, now],
            )?;
            report.push(serde_json::json!({"work_unit_id": id, "result": "released_unclaimed"}));
        }
        let unbound: Vec<String> = {
            let mut stmt = tx.prepare(
                "SELECT work_unit_id FROM shared_booking_intents
                WHERE phase='uncertain' AND launch_hash IS NOT NULL AND run_id IS NULL
                  AND outcome IS NULL ORDER BY work_unit_id",
            )?;
            let rows = stmt.query_map([], |row| row.get(0))?;
            rows.collect::<rusqlite::Result<_>>()?
        };
        for id in unbound {
            if release_unbound_shared_launch_in_tx(&tx, &id, now)? {
                report.push(serde_json::json!({"work_unit_id": id, "result": "effects_uncertain"}));
            }
        }
        let settled = settle_finished_shared_launches_in_tx(&tx, now)?;
        if settled > 0 {
            report.push(serde_json::json!({"result": "settled_finished_runs", "count": settled}));
        }
        tx.commit()?;
        Ok(report)
    }

    pub fn mark_shared_booking_uncertain(&self, id: &str) -> Result<()> {
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        let changed = tx.execute(
            "UPDATE shared_booking_intents SET phase='uncertain',updated_ms=?2
            WHERE work_unit_id=?1 AND phase='booked'",
            params![id, crate::daemon::now()],
        )? == 1;
        if changed {
            tx.execute(
                "UPDATE auto_pool_claims SET state='uncertain'
            WHERE work_unit_id=?1 AND state='active'",
                [id],
            )?;
        }
        tx.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auto_quota::{QuotaSnapshot, QuotaWindow};
    use crate::store::Store;
    use std::path::Path;

    fn observed(store: &Store, profile: &str, used: [f64; 2]) -> i64 {
        observed_at(store, profile, used, 1000)
    }

    fn observed_at(store: &Store, profile: &str, used: [f64; 2], at_ms: i64) -> i64 {
        let snapshot = QuotaSnapshot {
            ordinary_usage_allowed: Some(true),
            observed_ms: at_ms,
            expires_ms: 60_000,
            native_uncertain_until_ms: None,
            windows: ["short", "long"]
                .into_iter()
                .zip(used)
                .map(|(name, used_percent)| QuotaWindow {
                    pool_id: profile.into(),
                    bucket_id: name.into(),
                    window: name.into(),
                    model: None,
                    model_family: None,
                    plan_type: Some("pro".into()),
                    used_percent,
                    reset_ms: Some(90_000),
                    duration_mins: Some(60),
                    observed_ms: at_ms,
                    expires_ms: 60_000,
                })
                .collect(),
        };
        let event = store
            .insert_event(
                at_ms,
                None,
                None,
                "quota",
                "fixture",
                "exact",
                &serde_json::json!({}),
            )
            .unwrap();
        store
            .insert_auto_quota(event.seq, profile, "fixture/structured", &snapshot)
            .unwrap();
        event.seq
    }

    #[test]
    fn linked_profiles_share_each_window_and_replay_one_booking() {
        let store = Store::open(Path::new(":memory:")).unwrap();
        let fingerprint = "a".repeat(64);
        store
            .record_auto_account_identity("codex", &fingerprint)
            .unwrap();
        store
            .record_auto_account_identity("alias", &fingerprint)
            .unwrap();
        let first_observation = observed(&store, "codex", [90.0, 92.0]);
        let alias_observation = observed(&store, "alias", [90.0, 92.0]);
        let first = AccountBookingRequest {
            id: "swarm/job-1",
            request_hash: "first",
            caller: "swarm",
            route_id: "codex/sol",
            profile_id: "codex",
            quota_profile_id: "codex",
            account_generation: 1,
            quota_event_seq: first_observation,
            now_ms: 2000,
            draw: BookingDraw::Fixture(&[4_000, 4_000]),
            allocation_remaining_milli: None,
        };
        assert!(
            book_shared_account_in_tx(&store.conn, &first).is_err(),
            "the composable primitive must refuse a call outside a transaction"
        );
        assert_eq!(
            store.book_shared_account(&first).unwrap(),
            BookingDecision::Booked
        );
        assert_eq!(
            store.book_shared_account(&first).unwrap(),
            BookingDecision::Replayed
        );
        let changed_route = AccountBookingRequest {
            route_id: "codex/astra",
            ..first
        };
        assert!(
            store.book_shared_account(&changed_route).is_err(),
            "a logical ID cannot replay a different route with the same caller hash"
        );
        let second = AccountBookingRequest {
            id: "auto/job-2",
            request_hash: "second",
            caller: "auto",
            route_id: "codex/sol",
            profile_id: "alias",
            quota_profile_id: "alias",
            account_generation: 1,
            quota_event_seq: alias_observation,
            now_ms: 2000,
            draw: BookingDraw::Fixture(&[5_000, 5_000]),
            allocation_remaining_milli: None,
        };
        assert_eq!(
            store.book_shared_account(&second).unwrap(),
            BookingDecision::Blocked("shared_pool_headroom")
        );
        store.mark_shared_booking_uncertain(first.id).unwrap();
        assert_eq!(
            store.book_shared_account(&second).unwrap(),
            BookingDecision::Blocked("shared_pool_headroom")
        );
        assert!(
            !store.release_shared_booking_pre_effect(first.id).unwrap(),
            "an uncertain effect cannot release its account commitment"
        );
    }

    #[test]
    fn two_connections_cannot_commit_the_last_account_window_twice() {
        use std::sync::{Arc, Barrier};
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("booking.sqlite");
        let store = Store::open(&db).unwrap();
        store
            .record_auto_account_identity("codex", &"b".repeat(64))
            .unwrap();
        let event = observed(&store, "codex", [92.0, 92.0]);
        drop(store);
        let barrier = Arc::new(Barrier::new(2));
        let handles: Vec<_> = (0..2)
            .map(|n| {
                let db = db.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    let store = Store::open(&db).unwrap();
                    let id = format!("client/{n}");
                    let req = AccountBookingRequest {
                        id: &id,
                        request_hash: "same-work",
                        caller: "swarm",
                        route_id: "codex/sol",
                        profile_id: "codex",
                        quota_profile_id: "codex",
                        account_generation: 1,
                        quota_event_seq: event,
                        now_ms: 2000,
                        draw: BookingDraw::Fixture(&[6_000, 6_000]),
                        allocation_remaining_milli: None,
                    };
                    barrier.wait();
                    store.book_shared_account(&req).unwrap()
                })
            })
            .collect();
        let outcomes: Vec<_> = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect();
        assert_eq!(
            outcomes
                .iter()
                .filter(|outcome| **outcome == BookingDecision::Booked)
                .count(),
            1
        );
        assert_eq!(
            outcomes
                .iter()
                .filter(|outcome| **outcome == BookingDecision::Blocked("shared_pool_headroom"))
                .count(),
            1
        );
        let reopened = Store::open(&db).unwrap();
        let active: i64 = reopened
            .conn
            .query_row(
                "SELECT COUNT(*) FROM auto_pool_claims
            WHERE state='active'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            active, 1,
            "restart retains exactly one durable account claim"
        );
    }

    #[test]
    fn stale_identity_snapshot_and_pre_effect_release_have_scoped_results() {
        let store = Store::open(Path::new(":memory:")).unwrap();
        store
            .record_auto_account_identity("codex", &"c".repeat(64))
            .unwrap();
        let event = observed(&store, "codex", [90.0, 92.0]);
        let req = AccountBookingRequest {
            id: "ordinary/turn",
            request_hash: "ordinary",
            caller: "ordinary",
            route_id: "codex/sol",
            profile_id: "codex",
            quota_profile_id: "codex",
            account_generation: 1,
            quota_event_seq: event,
            now_ms: 2000,
            draw: BookingDraw::Fixture(&[4_000, 4_000]),
            allocation_remaining_milli: None,
        };
        let stale_generation = AccountBookingRequest {
            account_generation: 2,
            ..req
        };
        assert_eq!(
            store.book_shared_account(&stale_generation).unwrap(),
            BookingDecision::Blocked("account_identity_changed")
        );
        let expired = AccountBookingRequest {
            now_ms: 60_000,
            ..req
        };
        assert_eq!(
            store.book_shared_account(&expired).unwrap(),
            BookingDecision::Blocked("snapshot_expired")
        );
        let tiny_allocation = AccountBookingRequest {
            allocation_remaining_milli: Some(&[5_000, 3_000]),
            ..req
        };
        assert_eq!(
            store.book_shared_account(&tiny_allocation).unwrap(),
            BookingDecision::Blocked("allocation_exhausted")
        );
        assert_eq!(
            store.book_shared_account(&req).unwrap(),
            BookingDecision::Booked
        );
        assert_eq!(
            store.release_stale_unstarted_auto_pool_claims().unwrap(),
            0,
            "Auto restart reconciliation must not release a shared booking it does not own"
        );
        assert!(store.release_shared_booking_pre_effect(req.id).unwrap());
        assert!(!store.release_shared_booking_pre_effect(req.id).unwrap());
        let old_snapshot = AccountBookingRequest {
            id: "ordinary/next",
            ..req
        };
        let newer = observed(&store, "codex", [95.0, 95.0]);
        assert_eq!(
            store.book_shared_account(&old_snapshot).unwrap(),
            BookingDecision::Blocked("snapshot_superseded")
        );
        let current = AccountBookingRequest {
            quota_event_seq: newer,
            ..old_snapshot
        };
        assert_eq!(
            store.book_shared_account(&current).unwrap(),
            BookingDecision::Booked
        );
    }

    #[test]
    fn launch_booking_holds_writer_slot_and_claims_effects_once() {
        let store = Store::open(Path::new(":memory:")).unwrap();
        for (profile, fingerprint) in [("first", "d".repeat(64)), ("second", "e".repeat(64))] {
            store
                .record_auto_account_identity(profile, &fingerprint)
                .unwrap();
        }
        let first_event = observed(&store, "first", [90.0, 90.0]);
        let second_event = observed(&store, "second", [90.0, 90.0]);
        store
            .conn
            .execute(
                "INSERT INTO meta(key,value) VALUES('agents.max_active','1')",
                [],
            )
            .unwrap();
        let first_account = AccountBookingRequest {
            id: "launch/first",
            request_hash: "first",
            caller: "swarm",
            route_id: "codex/sol",
            profile_id: "first",
            quota_profile_id: "first",
            account_generation: 1,
            quota_event_seq: first_event,
            now_ms: 2000,
            draw: BookingDraw::Fixture(&[4_000, 4_000]),
            allocation_remaining_milli: None,
        };
        let first = LaunchBookingRequest {
            account: &first_account,
            workspace_path: Some("/repo/shared"),
            consume_agent_slot: true,
            launch_hash: "first-launch",
        };
        let admitted = store.book_shared_launch(&first).unwrap();
        assert!(
            matches!(admitted, LaunchBookingDecision::Booked(ref intent) if intent.phase == "booked")
        );
        let second_account = AccountBookingRequest {
            id: "launch/second",
            request_hash: "second",
            profile_id: "second",
            quota_profile_id: "second",
            quota_event_seq: second_event,
            ..first_account
        };
        let competing_writer = LaunchBookingRequest {
            account: &second_account,
            ..first
        };
        assert_eq!(
            store.book_shared_launch(&competing_writer).unwrap(),
            LaunchBookingDecision::Blocked("workspace_writer_busy")
        );
        let independent_writer = LaunchBookingRequest {
            workspace_path: Some("/repo/other"),
            ..competing_writer
        };
        assert_eq!(
            store.book_shared_launch(&independent_writer).unwrap(),
            LaunchBookingDecision::Blocked("global_agent_limit")
        );
        assert!(store.claim_shared_launch_effects(first_account.id).unwrap());
        assert!(
            !store.claim_shared_launch_effects(first_account.id).unwrap(),
            "only one reconnecting caller may attempt launch effects"
        );
        assert!(!store
            .release_shared_booking_pre_effect(first_account.id)
            .unwrap());
        let replay = store.book_shared_launch(&first).unwrap();
        assert!(
            matches!(replay, LaunchBookingDecision::Replayed(ref intent) if intent.phase == "uncertain")
        );
        let changed = LaunchBookingRequest {
            workspace_path: Some("/repo/changed"),
            ..first
        };
        assert!(store.book_shared_launch(&changed).is_err());
        let claims: i64 = store
            .conn
            .query_row("SELECT COUNT(*) FROM auto_pool_claims", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(
            claims, 1,
            "blocked writer/slot requests cannot retain account claims"
        );
    }

    fn account<'a>(
        id: &'a str,
        profile: &'a str,
        event: i64,
        draw: &'a [i64],
    ) -> AccountBookingRequest<'a> {
        AccountBookingRequest {
            id,
            request_hash: id,
            caller: "ordinary",
            route_id: "codex/sol",
            profile_id: profile,
            quota_profile_id: profile,
            account_generation: 1,
            quota_event_seq: event,
            now_ms: 2000,
            draw: BookingDraw::Fixture(draw),
            allocation_remaining_milli: None,
        }
    }

    fn launch<'a>(
        account: &'a AccountBookingRequest<'a>,
        path: &'a str,
    ) -> LaunchBookingRequest<'a> {
        LaunchBookingRequest {
            account,
            workspace_path: Some(path),
            consume_agent_slot: true,
            launch_hash: account.id,
        }
    }

    fn queued_run(store: &Store, run: &str, profile: &str, path: &str) {
        store
            .conn
            .execute(
                "INSERT INTO workspaces(id,path,repo_root,common_dir,kind,initial_dirty,created_ms)
                VALUES('w-'||?1,?2,?2,?2,'current','{}',0)",
                params![run, path],
            )
            .unwrap();
        store
            .conn
            .execute(
                "INSERT INTO tasks(id,title,prompt,repo_root,workspace_id,created_ms)
                VALUES('t-'||?1,'task','prompt',?2,'w-'||?1,0)",
                params![run, path],
            )
            .unwrap();
        store
            .conn
            .execute(
                "INSERT INTO runs(id,task_id,harness,profile_id,workspace_id,status,created_ms,title,capabilities)
                VALUES(?1,'t-'||?1,'codex-app',?2,'w-'||?1,'queued',0,'run','{}')",
                params![run, profile],
            )
            .unwrap();
    }

    fn bind(store: &Store, id: &str, run: &str) -> Result<()> {
        let tx = Transaction::new_unchecked(&store.conn, TransactionBehavior::Immediate)?;
        bind_shared_launch_run_in_tx(&tx, id, run, 2500)?;
        tx.commit()?;
        Ok(())
    }

    fn start(store: &Store, run: &str) {
        store
            .conn
            .execute(
                "UPDATE runs SET status='running',run_dir='/tmp/'||?1,process_generation=1 WHERE id=?1",
                [run],
            )
            .unwrap();
    }

    fn settle(store: &Store, run: &str, status: &str, at: i64) -> Option<LaunchSettlement> {
        store
            .conn
            .execute(
                "UPDATE runs SET status=?2,ended_ms=?3 WHERE id=?1",
                params![run, status, at],
            )
            .unwrap();
        let tx = Transaction::new_unchecked(&store.conn, TransactionBehavior::Immediate).unwrap();
        let settled = settle_shared_launch_run_in_tx(&tx, run, true, at).unwrap();
        tx.commit().unwrap();
        settled
    }

    fn claim_state(store: &Store, id: &str) -> String {
        store
            .conn
            .query_row(
                "SELECT state FROM auto_pool_claims WHERE work_unit_id=?1",
                [id],
                |row| row.get(0),
            )
            .unwrap()
    }

    fn identities(store: &Store, profiles: &[(&str, char)]) {
        for (profile, digit) in profiles {
            store
                .record_auto_account_identity(profile, &digit.to_string().repeat(64))
                .unwrap();
        }
    }

    fn agent_limit(store: &Store, limit: i64) {
        store
            .conn
            .execute(
                "INSERT INTO meta(key,value) VALUES('agents.max_active',?1)
                ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                [limit.to_string()],
            )
            .unwrap();
    }

    #[test]
    fn bound_run_is_counted_once_and_settlement_releases_its_slot_and_writer() {
        let store = Store::open(Path::new(":memory:")).unwrap();
        identities(
            &store,
            &[
                ("first", 'a'),
                ("alias", 'a'),
                ("second", 'b'),
                ("third", 'c'),
            ],
        );
        let first_event = observed(&store, "first", [90.0, 90.0]);
        let alias_event = observed(&store, "alias", [90.0, 90.0]);
        let second_event = observed(&store, "second", [90.0, 90.0]);
        let third_event = observed(&store, "third", [90.0, 90.0]);
        agent_limit(&store, 2);
        let a = account("launch/a", "first", first_event, &[4_000, 4_000]);
        assert!(matches!(
            store.book_shared_launch(&launch(&a, "/repo/a")).unwrap(),
            LaunchBookingDecision::Booked(_)
        ));
        assert!(store.claim_shared_launch_effects("launch/a").unwrap());
        queued_run(&store, "run-a", "first", "/repo/a");
        bind(&store, "launch/a", "run-a").unwrap();
        start(&store, "run-a");

        // One running run plus its own hold must count as one of two slots.
        let b = account("launch/b", "second", second_event, &[4_000, 4_000]);
        assert!(
            matches!(
                store.book_shared_launch(&launch(&b, "/repo/b")).unwrap(),
                LaunchBookingDecision::Booked(_)
            ),
            "a held booking that became a running process is counted once"
        );
        let c = account("launch/c", "third", third_event, &[4_000, 4_000]);
        assert_eq!(
            store.book_shared_launch(&launch(&c, "/repo/c")).unwrap(),
            LaunchBookingDecision::Blocked("global_agent_limit")
        );
        let same_writer = account("launch/writer", "third", third_event, &[4_000, 4_000]);
        assert_eq!(
            store
                .book_shared_launch(&launch(&same_writer, "/repo/a"))
                .unwrap(),
            LaunchBookingDecision::Blocked("workspace_writer_busy")
        );
        assert_eq!(
            store
                .shared_writer_hold("/repo/a", None, None)
                .unwrap()
                .as_deref(),
            Some("launch/a")
        );
        assert_eq!(
            store
                .shared_writer_hold("/repo/a", None, Some("run-a"))
                .unwrap(),
            None,
            "the bound run itself is the writer its booking holds"
        );

        // The bound run's draw is in its committed windows, so it does not
        // also occupy the whole account as if it were unmetered work.
        store.release_shared_booking_pre_effect("launch/b").unwrap();
        let alias = account("launch/alias", "alias", alias_event, &[5_000, 5_000]);
        assert!(matches!(
            store.book_shared_account(&alias).unwrap(),
            BookingDecision::Booked
        ));
        let too_much = account("launch/too-much", "alias", alias_event, &[2_000, 2_000]);
        assert_eq!(
            store.book_shared_account(&too_much).unwrap(),
            BookingDecision::Blocked("shared_pool_headroom"),
            "the running run's committed draw still counts in every window"
        );
        assert!(
            !store
                .auto_claim_conflicts_with_run("first", "run-a")
                .unwrap(),
            "the bound run's own turn is admitted by its booking"
        );
        assert!(
            store.auto_claim_conflicts_with_run("first", "").unwrap(),
            "unbooked manual work still sees the committed account"
        );
        store
            .release_shared_booking_pre_effect("launch/alias")
            .unwrap();

        assert_eq!(
            settle(&store, "run-a", "completed", 3000),
            Some(LaunchSettlement::Retained)
        );
        assert_eq!(
            store.shared_writer_hold("/repo/a", None, None).unwrap(),
            None
        );
        assert!(
            matches!(
                store.book_shared_launch(&launch(&c, "/repo/c")).unwrap(),
                LaunchBookingDecision::Booked(_)
            ),
            "settlement frees the slot"
        );
        assert_eq!(
            claim_state(&store, "launch/a"),
            "uncertain",
            "a process ran: its account draw stays committed after exit"
        );
        let after = account("launch/after", "alias", alias_event, &[7_000, 7_000]);
        assert_eq!(
            store.book_shared_account(&after).unwrap(),
            BookingDecision::Blocked("shared_pool_headroom")
        );
        observed_at(&store, "alias", [95.0, 95.0], 2900);
        assert_eq!(
            claim_state(&store, "launch/a"),
            "uncertain",
            "an observation taken before settlement cannot include the draw"
        );
        let fresh = observed_at(&store, "alias", [95.0, 95.0], 4000);
        assert_eq!(claim_state(&store, "launch/a"), "released");
        let intent = store.shared_launch_intent("launch/a").unwrap().unwrap();
        assert_eq!(intent.outcome.as_deref(), Some("observed_after_settlement"));
        let next = AccountBookingRequest {
            id: "launch/next",
            request_hash: "launch/next",
            quota_event_seq: fresh,
            now_ms: 5000,
            draw: BookingDraw::Fixture(&[4_000, 4_000]),
            ..after
        };
        assert_eq!(
            store.book_shared_account(&next).unwrap(),
            BookingDecision::Booked
        );
    }

    #[test]
    fn binding_needs_the_effects_claim_and_the_booked_profile_and_workspace() {
        let store = Store::open(Path::new(":memory:")).unwrap();
        identities(&store, &[("first", 'a'), ("other", 'b')]);
        let event = observed(&store, "first", [50.0, 50.0]);
        let a = account("launch/a", "first", event, &[1_000, 1_000]);
        store.book_shared_launch(&launch(&a, "/repo/a")).unwrap();
        queued_run(&store, "run-a", "first", "/repo/a");
        queued_run(&store, "run-elsewhere", "first", "/repo/elsewhere");
        queued_run(&store, "run-other", "other", "/repo/a2");
        assert!(
            bind(&store, "launch/a", "run-a").is_err(),
            "no binding before the effects claim"
        );
        assert!(store.claim_shared_launch_effects("launch/a").unwrap());
        assert!(bind(&store, "launch/a", "run-elsewhere").is_err());
        assert!(bind(&store, "launch/a", "run-other").is_err());
        assert!(
            bind_shared_launch_run_in_tx(&store.conn, "launch/a", "run-a", 1).is_err(),
            "binding must share the run row's transaction"
        );
        bind(&store, "launch/a", "run-a").unwrap();
        queued_run(&store, "run-second", "first", "/repo/a3");
        assert!(
            bind(&store, "launch/a", "run-second").is_err(),
            "one launch binds one run"
        );
        match store.book_shared_launch(&launch(&a, "/repo/a")).unwrap() {
            LaunchBookingDecision::Replayed(intent) => {
                assert_eq!(intent.run_id.as_deref(), Some("run-a"));
                assert!(!intent.slot_held || intent.settled_ms.is_none());
            }
            other => panic!("expected replay, got {other:?}"),
        }
        assert!(store.shared_launch_bound_unsettled("run-a").unwrap());
    }

    #[test]
    fn a_bound_run_that_never_got_a_supervisor_releases_its_account_commitment() {
        let store = Store::open(Path::new(":memory:")).unwrap();
        identities(&store, &[("first", 'a')]);
        let event = observed(&store, "first", [50.0, 50.0]);
        let a = account("launch/a", "first", event, &[1_000, 1_000]);
        store.book_shared_launch(&launch(&a, "/repo/a")).unwrap();
        assert!(store.claim_shared_launch_effects("launch/a").unwrap());
        queued_run(&store, "run-a", "first", "/repo/a");
        bind(&store, "launch/a", "run-a").unwrap();
        let tx = Transaction::new_unchecked(&store.conn, TransactionBehavior::Immediate).unwrap();
        assert_eq!(
            settle_shared_launch_run_in_tx(&tx, "run-a", false, 3000).unwrap(),
            None,
            "an unconfirmed process keeps every hold"
        );
        tx.commit().unwrap();
        assert_eq!(
            settle(&store, "run-a", "failed", 3000),
            Some(LaunchSettlement::NotStarted)
        );
        assert_eq!(claim_state(&store, "launch/a"), "released");
        let intent = store.shared_launch_intent("launch/a").unwrap().unwrap();
        assert_eq!(
            (intent.phase.as_str(), intent.slot_held, intent.writer_held),
            ("released", false, false)
        );
        assert_eq!(
            settle(&store, "run-a", "failed", 3100),
            None,
            "settlement happens once"
        );

        // A later generation whose spawn was refused also has no run_dir,
        // but its first generation may have spent allowance.
        let b = account("launch/b", "first", event, &[1_000, 1_000]);
        store.book_shared_launch(&launch(&b, "/repo/b")).unwrap();
        assert!(store.claim_shared_launch_effects("launch/b").unwrap());
        queued_run(&store, "run-b", "first", "/repo/b");
        bind(&store, "launch/b", "run-b").unwrap();
        store
            .conn
            .execute("UPDATE runs SET process_generation=2 WHERE id='run-b'", [])
            .unwrap();
        assert_eq!(
            settle(&store, "run-b", "failed", 3200),
            Some(LaunchSettlement::Retained)
        );
        assert_eq!(claim_state(&store, "launch/b"), "uncertain");
    }

    #[test]
    fn startup_recovery_releases_unclaimed_keeps_uncertain_writers_and_settles_finished_runs() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("recovery.sqlite");
        {
            let store = Store::open(&db).unwrap();
            identities(
                &store,
                &[
                    ("p1", '1'),
                    ("p2", '2'),
                    ("p3", '3'),
                    ("p4", '4'),
                    ("p5", '5'),
                    ("p6", '6'),
                ],
            );
            agent_limit(&store, 4);
            for (id, profile, path, claim, run, running) in [
                ("unclaimed", "p1", "/repo/unclaimed", false, None, false),
                ("unbound", "p2", "/repo/unbound", true, None, false),
                (
                    "interrupted",
                    "p3",
                    "/repo/interrupted",
                    true,
                    Some("run-interrupted"),
                    true,
                ),
                (
                    "running",
                    "p4",
                    "/repo/running",
                    true,
                    Some("run-running"),
                    true,
                ),
            ] {
                let event = observed(&store, profile, [10.0, 10.0]);
                let request = account(id, profile, event, &[1_000, 1_000]);
                assert!(
                    matches!(
                        store.book_shared_launch(&launch(&request, path)).unwrap(),
                        LaunchBookingDecision::Booked(_)
                    ),
                    "{id}"
                );
                if claim {
                    assert!(store.claim_shared_launch_effects(id).unwrap());
                }
                if let Some(run) = run {
                    queued_run(&store, run, profile, path);
                    bind(&store, id, run).unwrap();
                    if running {
                        start(&store, run);
                    }
                }
            }
            // A daemon stop marks runs interrupted without the settle hook.
            store
                .conn
                .execute(
                    "UPDATE runs SET status='interrupted',ended_ms=3000 WHERE id='run-interrupted'",
                    [],
                )
                .unwrap();
        }
        let store = Store::open(&db).unwrap();
        agent_limit(&store, 3);
        let report = store.reconcile_shared_launches_on_start().unwrap();
        assert!(
            report
                .iter()
                .any(|entry| entry["work_unit_id"] == "unclaimed"
                    && entry["result"] == "released_unclaimed"),
            "{report:?}"
        );
        assert!(
            report.iter().any(|entry| entry["work_unit_id"] == "unbound"
                && entry["result"] == "effects_uncertain"),
            "{report:?}"
        );

        let unclaimed = store.shared_launch_intent("unclaimed").unwrap().unwrap();
        assert_eq!(
            (
                unclaimed.phase.as_str(),
                unclaimed.slot_held,
                unclaimed.writer_held
            ),
            ("released", false, false)
        );
        assert_eq!(claim_state(&store, "unclaimed"), "released");

        let unbound = store.shared_launch_intent("unbound").unwrap().unwrap();
        assert_eq!(
            (
                unbound.phase.as_str(),
                unbound.slot_held,
                unbound.writer_held
            ),
            ("uncertain", false, true),
            "its Git effect is unknown: the writer stays held"
        );
        assert_eq!(
            claim_state(&store, "unbound"),
            "released",
            "no bound run means no model process could have spent allowance"
        );
        assert_eq!(
            store
                .shared_writer_hold("/repo/unbound", None, None)
                .unwrap()
                .as_deref(),
            Some("unbound")
        );
        assert!(
            !store.claim_shared_launch_effects("unbound").unwrap(),
            "an uncertain launch is never attempted again"
        );
        let event = observed(&store, "p2", [10.0, 10.0]);
        let replay = account("unbound", "p2", event, &[1_000, 1_000]);
        assert!(
            store
                .book_shared_launch(&launch(&replay, "/repo/unbound"))
                .is_err(),
            "a replay citing another observation is a different request"
        );

        let interrupted = store.shared_launch_intent("interrupted").unwrap().unwrap();
        assert_eq!(interrupted.outcome.as_deref(), Some("settled"));
        assert!(!interrupted.slot_held && !interrupted.writer_held);
        assert_eq!(
            claim_state(&store, "interrupted"),
            "uncertain",
            "its process ran: the account draw stays committed"
        );

        let running = store.shared_launch_intent("running").unwrap().unwrap();
        assert_eq!(running.run_id.as_deref(), Some("run-running"));
        assert!(
            running.slot_held && running.writer_held && running.settled_ms.is_none(),
            "a run that may still own a process keeps its binding for reattachment"
        );
        assert_eq!(claim_state(&store, "running"), "uncertain");

        assert!(
            Store::open(&db)
                .unwrap()
                .reconcile_shared_launches_on_start()
                .unwrap()
                .is_empty(),
            "a second restart finds nothing more to reconcile"
        );

        // Only the running launch occupies a slot now: two more fit under three.
        for (id, profile, path) in [
            ("new-1", "p5", "/repo/new-1"),
            ("new-2", "p6", "/repo/new-2"),
        ] {
            let event = observed(&store, profile, [10.0, 10.0]);
            let request = account(id, profile, event, &[1_000, 1_000]);
            assert!(
                matches!(
                    store.book_shared_launch(&launch(&request, path)).unwrap(),
                    LaunchBookingDecision::Booked(_)
                ),
                "{id}"
            );
        }
        let event = observed(&store, "p1", [10.0, 10.0]);
        let over = account("over", "p1", event, &[1_000, 1_000]);
        assert_eq!(
            store
                .book_shared_launch(&launch(&over, "/repo/over"))
                .unwrap(),
            LaunchBookingDecision::Blocked("global_agent_limit")
        );
    }

    #[test]
    fn two_connections_claim_one_booked_launch_exactly_once() {
        use std::sync::{Arc, Barrier};
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("claims.sqlite");
        {
            let store = Store::open(&db).unwrap();
            identities(&store, &[("first", 'a')]);
            let event = observed(&store, "first", [10.0, 10.0]);
            let request = account("launch/raced", "first", event, &[1_000, 1_000]);
            store
                .book_shared_launch(&launch(&request, "/repo/raced"))
                .unwrap();
        }
        {
            let barrier = Arc::new(Barrier::new(2));
            let handles: Vec<_> = (0..2)
                .map(|_| {
                    let db = db.clone();
                    let barrier = barrier.clone();
                    std::thread::spawn(move || {
                        let store = Store::open(&db).unwrap();
                        barrier.wait();
                        store.claim_shared_launch_effects("launch/raced").unwrap()
                    })
                })
                .collect();
            let won = handles
                .into_iter()
                .map(|h| h.join().unwrap())
                .filter(|won| *won)
                .count();
            assert_eq!(
                won, 1,
                "exactly one of two racing workers may claim the launch"
            );
        }
        let store = Store::open(&db).unwrap();
        let intent = store.shared_launch_intent("launch/raced").unwrap().unwrap();
        assert_eq!(intent.phase, "uncertain");
        assert!(intent.effects_claimed_ms.is_some());
        let claimed_once: i64 = store
            .conn
            .query_row(
                "SELECT COUNT(*) FROM shared_booking_intents WHERE work_unit_id='launch/raced'
            AND effects_claimed_ms IS NOT NULL",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(claimed_once, 1);
    }

    fn swarm_run(store: &Store, id: &str, status: &str) {
        store.conn.execute(
            "INSERT INTO swarm_runs(id,category,category_key,objective,status,generation,revision,
            allowed_targets,policy,created_ms,updated_ms) VALUES(?1,?1,?1,'objective',?2,1,1,'[]','{}',0,0)",
            params![id, status]).unwrap();
        store.conn.execute(
            "INSERT INTO swarm_jobs(run_id,id,plan_revision,title,acceptance,deps,status,created_ms,updated_ms)
            VALUES(?1,'j',1,'job','evidence','[]','reserved',0,0)", [id]).unwrap();
    }

    fn swarm_attempt(store: &Store, run: &str, attempt: &str) {
        store.conn.execute(
            "INSERT INTO swarm_attempts(id,run_id,job_id,revision,token_sha256,status,executor,created_ms)
            VALUES(?1,?2,'j',1,'hash','registered','worker',0)", params![attempt, run]).unwrap();
    }

    #[test]
    fn one_count_takes_each_occupant_once_and_a_director_booking_takes_no_second_slot() {
        let store = Store::open(Path::new(":memory:")).unwrap();
        identities(&store, &[("first", 'a'), ("second", 'b')]);
        let first = observed(&store, "first", [10.0, 10.0]);
        let second = observed(&store, "second", [10.0, 10.0]);
        let count = || app_slots_in_use(&store.conn).unwrap();
        assert_eq!(count(), 0);

        // An ordinary start's durable hold, before its run row exists.
        let hold = store.hold_app_slot("start").unwrap().unwrap();
        assert_eq!(count(), 1);
        // A booked start, not yet bound.
        let booked = account("launch/ordinary", "first", first, &[1_000, 1_000]);
        assert!(matches!(store.book_shared_launch(&launch(&booked, "/repo/ordinary")).unwrap(),
            LaunchBookingDecision::Booked(_)));
        assert_eq!(count(), 2);
        // Overseer's own coordinating run holds no slot (Swarm/Auto contract).
        queued_run(&store, "r-overseer", "overseer-profile", "/repo/overseer");
        store.conn.execute("INSERT INTO run_roles(run_id,role) VALUES('r-overseer','overseer')", []).unwrap();
        assert_eq!(count(), 2);
        // An active category: its director slot, and an unbooked (fixture) worker attempt.
        swarm_run(&store, "s-active", "running");
        swarm_attempt(&store, "s-active", "att-fixture");
        assert_eq!(count(), 4);
        // A booked Swarm worker attempt is counted through its booking only.
        swarm_attempt(&store, "s-active", "att-booked");
        let worker_id = swarm_attempt_booking_id("att-booked");
        let worker = AccountBookingRequest { caller: "swarm", ..account(&worker_id, "first", first, &[1_000, 1_000]) };
        let worker_launch = LaunchBookingRequest { account: &worker, workspace_path: None,
            consume_agent_slot: true, launch_hash: &worker_id };
        assert!(matches!(store.book_shared_launch(&worker_launch).unwrap(), LaunchBookingDecision::Booked(_)));
        assert_eq!(count(), 5, "a booked worker attempt is one slot, not two");

        // A director in a planning category, bound to a booking that took no
        // slot: its run is the one occupant while planning, and the category's
        // director slot is the one occupant once the category runs.
        swarm_run(&store, "s-plan", "planning");
        let director = account("director/s-plan", "second", second, &[1_000, 1_000]);
        let director_launch = LaunchBookingRequest { account: &director, workspace_path: None,
            consume_agent_slot: false, launch_hash: "director/s-plan" };
        match store.book_shared_launch(&director_launch).unwrap() {
            LaunchBookingDecision::Booked(intent) => assert!(!intent.slot_held),
            other => panic!("{other:?}"),
        }
        assert_eq!(count(), 5, "a booking without a slot takes none");
        queued_run(&store, "r-director", "second", "/repo/director");
        store.conn.execute(
            "INSERT INTO swarm_director_owners(run_id,generation,token_sha256,status,created_ms,renewed_ms,
            lease_expires_ms,overseer_run_id,supervised_launch,launch_phase)
            VALUES('s-plan',1,'hash','active',0,0,999999999999,'r-director',1,'linked')", []).unwrap();
        assert!(store.claim_shared_launch_effects("director/s-plan").unwrap());
        bind(&store, "director/s-plan", "r-director").unwrap();
        assert_eq!(count(), 6, "the planning director's run is counted once");
        store.conn.execute("UPDATE swarm_runs SET status='running' WHERE id='s-plan'", []).unwrap();
        assert_eq!(count(), 6, "a running category's director slot replaces its run, not adds to it");

        // Releasing the ordinary hold gives back exactly one slot.
        store.release_app_slot_hold(&hold).unwrap();
        assert_eq!(count(), 5);
        // A cancelled attempt's unlaunched booking is released with it.
        store.conn.execute("UPDATE swarm_attempts SET status='cancelled' WHERE id='att-booked'", []).unwrap();
        assert_eq!(count(), 5, "the booking still holds until reconciled");
        let tx = Transaction::new_unchecked(&store.conn, TransactionBehavior::Immediate).unwrap();
        assert_eq!(release_orphaned_swarm_bookings_in_tx(&tx, 3_000).unwrap(), 1);
        tx.commit().unwrap();
        assert_eq!(count(), 4);
        assert_eq!(claim_state(&store, &worker_id), "released");
    }

    #[test]
    fn four_admission_paths_race_for_the_last_slot_and_exactly_one_wins() {
        use std::sync::{Arc, Barrier};
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("slots.sqlite");
        {
            let store = Store::open(&db).unwrap();
            identities(&store, &[("first", 'a'), ("second", 'b')]);
            observed(&store, "first", [10.0, 10.0]);
            store.set_auto_mode_enabled(true).unwrap();
            // Two slots: an active category holds one; one is left.
            agent_limit(&store, 2);
            swarm_run(&store, "s-active", "running");
            store.conn.execute_batch("INSERT INTO workspaces(id,path,repo_root,common_dir,kind,initial_dirty,created_ms)
                VALUES('w-parent','/repo/parent','/repo','/repo','current','{}',0);
                INSERT INTO tasks(id,title,prompt,repo_root,workspace_id,created_ms)
                VALUES('t-parent','parent','prompt','/repo','w-parent',0);
                INSERT INTO runs(id,task_id,harness,workspace_id,status,created_ms,title,capabilities)
                VALUES('parent','t-parent','codex-app','w-parent','completed',0,'parent','{}');").unwrap();
            assert_eq!(app_slots_in_use(&store.conn).unwrap(), 1);
        }
        let event: i64 = Store::open(&db).unwrap().conn.query_row(
            "SELECT MAX(event_seq) FROM auto_quota_observations WHERE pool_id='first'", [], |r| r.get(0)).unwrap();
        let barrier = Arc::new(Barrier::new(4));
        let handles: Vec<_> = (0..4).map(|path| {
            let db = db.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || -> (&'static str, bool, String) {
                let store = Store::open(&db).unwrap();
                barrier.wait();
                match path {
                    0 => match store.hold_app_slot("start").unwrap() {
                        Ok(_) => ("ordinary", true, String::new()),
                        Err((active, limit)) => ("ordinary", false, format!("limit {active}/{limit}")),
                    },
                    1 => {
                        let request = account("launch/booked", "first", event, &[1_000, 1_000]);
                        match store.book_shared_launch(&launch(&request, "/repo/booked")).unwrap() {
                            LaunchBookingDecision::Booked(_) => ("booked", true, String::new()),
                            other => ("booked", false, format!("{other:?}")),
                        }
                    }
                    2 => {
                        let parent = store.run("parent").unwrap().unwrap();
                        match store.insert_auto_selected_decision("child-1", &parent, "hash", "route",
                            "account/other-child", None, 300_000, &serde_json::json!({})) {
                            Ok(Some(_)) => ("auto_child", true, String::new()),
                            Ok(None) => ("auto_child", false, "pool".into()),
                            Err(error) => ("auto_child", false, error.to_string()),
                        }
                    }
                    _ => {
                        let (ws, task, run) = auto_root_rows();
                        match store.insert_auto_root_selected("root-1", &"h".repeat(64), "route",
                            "account/other-root", None, &ws, &task, &run, &serde_json::json!({}),
                            &serde_json::json!({})) {
                            Ok(Some(_)) => ("auto_root", true, String::new()),
                            Ok(None) => ("auto_root", false, "pool".into()),
                            Err(error) => ("auto_root", false, error.to_string()),
                        }
                    }
                }
            })
        }).collect();
        let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        let winners: Vec<_> = results.iter().filter(|(_, won, _)| *won).collect();
        assert_eq!(winners.len(), 1, "exactly one path takes the last slot: {results:?}");
        for (path, won, why) in &results {
            if !won {
                assert!(why.contains("limit") || why.contains("global_agent_limit"),
                    "{path} was refused for the agent limit, not something else: {why}");
            }
        }
        // The winner's slot is durable: a fresh connection sees the limit
        // reached, and a late ordinary start and booked start are refused.
        let store = Store::open(&db).unwrap();
        assert_eq!(app_slots_in_use(&store.conn).unwrap(), 2, "{results:?}");
        assert!(store.hold_app_slot("start").unwrap().is_err());
        let late = account("launch/late", "first", event, &[1_000, 1_000]);
        assert_eq!(store.book_shared_launch(&launch(&late, "/repo/late")).unwrap(),
            LaunchBookingDecision::Blocked("global_agent_limit"));
    }

    fn auto_root_rows() -> (crate::store::Workspace, crate::store::Task, crate::store::Run) {
        let ws = crate::store::Workspace { id: "w-root".into(), path: "/repo/root".into(),
            repo_root: "/repo".into(), common_dir: "/repo/.git".into(), kind: "worktree".into(),
            branch: Some("root".into()), owner_run_id: None, initial_dirty: serde_json::json!({}),
            created_ms: 0, removed_ms: None };
        let task = crate::store::Task { id: "t-root".into(), title: "root".into(), prompt: "prompt".into(),
            repo_root: "/repo".into(), target_ref: None, workspace_id: "w-root".into(), start_snapshot: None,
            fork_commit: None, fork_provenance: None, created_ms: 0, archived_ms: None };
        let run = crate::store::Run { id: "r-root".into(), task_id: "t-root".into(), parent_run_id: None,
            harness: "codex-app".into(), harness_version: None, profile_id: Some("second".into()),
            model: Some("model".into()), effort: None, workspace_id: "w-root".into(), native_id: None,
            status: "queued".into(), exit_reason: None, created_ms: 0, ended_ms: None, title: "root".into(),
            relation_source: None, relation_confidence: None, capabilities: serde_json::json!({}),
            process_generation: 0, attention: None };
        (ws, task, run)
    }
}
