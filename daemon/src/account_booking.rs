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
              AND t.status='running' AND t.ended_ms IS NULL)))",
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

impl Store {
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
            "UPDATE shared_booking_intents SET phase='released',updated_ms=?2
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
        let snapshot = QuotaSnapshot {
            ordinary_usage_allowed: Some(true),
            observed_ms: 1000,
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
                    observed_ms: 1000,
                    expires_ms: 60_000,
                })
                .collect(),
        };
        let event = store
            .insert_event(
                1000,
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
}
