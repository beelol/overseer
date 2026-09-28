//! One daemon-owned account commitment boundary for ordinary, Auto and Swarm work.

use crate::auto_quota::QuotaSnapshot;
use crate::store::Store;
use anyhow::{anyhow, Result};
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use sha2::{Digest, Sha256};
use std::collections::HashSet;

/// The caller supplies a qualified upper draw in thousandths of a percentage
/// point for every binding window in the cited structured provider observation.
/// A token count or estimated credit amount is not a valid upper draw here.
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
    pub upper_draw_milli: &'a [i64],
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
    if req.consume_agent_slot {
        let setting: Option<String> = conn
            .query_row(
                "SELECT value FROM meta WHERE key='agents.max_active'",
                [],
                |row| row.get(0),
            )
            .optional()?;
        let limit = match setting {
            None => 9,
            Some(value) => value
                .parse::<i64>()
                .ok()
                .filter(|n| (1..=256).contains(n))
                .ok_or_else(|| anyhow!("invalid app agent limit"))?,
        };
        // A bound run is counted once, through its intent's slot hold.
        let active: i64 = conn.query_row(
            &format!(
                "SELECT COUNT(*) FROM runs r
            WHERE (r.parent_run_id IS NULL OR r.relation_source='managed-delegation')
            AND (r.status IN ({HOLDING_STATUSES})
                OR EXISTS(SELECT 1 FROM turns t WHERE t.run_id=r.id
                    AND t.status='running' AND t.ended_ms IS NULL)) AND {}",
                unbound_run_sql("r")
            ),
            [],
            |row| row.get(0),
        )?;
        let held: i64 = conn.query_row(
            &format!(
                "SELECT COUNT(*) FROM shared_booking_intents b
            WHERE b.slot_held=1 AND {}",
                held_intent_sql("b")
            ),
            [],
            |row| row.get(0),
        )?;
        let pending_auto: i64 = conn.query_row("SELECT COUNT(*) FROM auto_launch_intents i
            JOIN auto_pool_claims c ON c.work_unit_id=i.work_unit_id
            WHERE c.state IN ('active','uncertain')
              AND NOT EXISTS(SELECT 1 FROM managed_work_units m WHERE m.work_unit_id=i.work_unit_id)",
            [], |row| row.get(0))?;
        if active.saturating_add(held).saturating_add(pending_auto) >= limit {
            return Ok(LaunchBookingDecision::Blocked("global_agent_limit"));
        }
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
    let request_hash = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&(
            req.request_hash,
            req.caller,
            req.route_id,
            req.profile_id,
            req.quota_profile_id,
            req.account_generation,
            req.quota_event_seq,
            req.upper_draw_milli,
            req.allocation_remaining_milli
        ))?)
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
    if quota.ordinary_usage_allowed != Some(true) {
        return Ok(BookingDecision::Blocked("account_exhausted"));
    }
    if quota.windows.is_empty()
        || quota.windows.len() > 32
        || req.upper_draw_milli.len() != quota.windows.len()
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
    let manual_run: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM runs r JOIN auto_account_identity a ON a.profile_id=r.profile_id
         WHERE a.fingerprint=?1 AND (r.status IN ('queued','starting','running','waiting_for_user')
           OR EXISTS(SELECT 1 FROM turns t WHERE t.run_id=r.id
              AND t.status='running' AND t.ended_ms IS NULL))
           -- A bound run's draw is already committed in its windows.
           AND NOT EXISTS(SELECT 1 FROM shared_booking_intents b
              WHERE b.run_id=r.id AND b.settled_ms IS NULL))",
        [&fingerprint], |row| row.get(0))?;
    if manual_run {
        return Ok(BookingDecision::Blocked("account_pool_busy"));
    }
    let mut windows = Vec::with_capacity(quota.windows.len());
    let mut seen = HashSet::new();
    for (n, window) in quota.windows.iter().enumerate() {
        let key = window_key(window)?;
        if !seen.insert(key.clone()) {
            return Ok(BookingDecision::Blocked("duplicate_window"));
        }
        let amount = req.upper_draw_milli[n];
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
        let remaining = ((100.0 - window.used_percent) * 1000.0).floor() as i64;
        let committed: i64 = conn.query_row(
            "SELECT COALESCE(SUM(w.amount_milli),0) FROM shared_booking_windows w
             JOIN auto_pool_claims c ON c.work_unit_id=w.work_unit_id
             WHERE w.pool_id=?1 AND w.window_key=?2 AND c.state IN ('active','uncertain')",
            params![account_pool, key],
            |row| row.get(0),
        )?;
        if amount > remaining.saturating_sub(committed) {
            return Ok(BookingDecision::Blocked("shared_pool_headroom"));
        }
        windows.push((key, amount));
    }
    conn.execute(
        "INSERT INTO shared_booking_intents(work_unit_id,request_hash,caller,route_id,
        profile_id,quota_profile_id,account_generation,quota_event_seq,phase,created_ms,updated_ms)
        VALUES(?1,?2,?3,?4,?5,?6,?7,?8,'booked',?9,?9)",
        params![
            req.id,
            request_hash,
            req.caller,
            req.route_id,
            req.profile_id,
            req.quota_profile_id,
            req.account_generation,
            req.quota_event_seq,
            req.now_ms
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
        let unclaimed: Vec<String> = {
            let mut stmt = tx.prepare(
                "SELECT work_unit_id FROM shared_booking_intents
                WHERE phase='booked' AND launch_hash IS NOT NULL AND effects_claimed_ms IS NULL
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
            upper_draw_milli: &[4_000, 4_000],
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
            upper_draw_milli: &[5_000, 5_000],
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
                        upper_draw_milli: &[6_000, 6_000],
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
            upper_draw_milli: &[4_000, 4_000],
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
            upper_draw_milli: &[4_000, 4_000],
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
            upper_draw_milli: draw,
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
            upper_draw_milli: &[4_000, 4_000],
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
}
