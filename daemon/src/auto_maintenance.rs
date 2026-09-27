//! Bounded local-learning expiry independent of the execution-store lock.

use crate::{daemon::Daemon, paths};
use anyhow::Result;
use rusqlite::{params, Connection, ErrorCode, OpenFlags};
use std::{path::Path, sync::Arc, time::Duration};

const DAY_MS: i64 = 86_400_000;
const BATCH: i64 = 64;

fn quiet_interval() -> Duration {
    // Protocol fixtures accelerate the fixed product interval without creating
    // a user-maintained retention or routing setting.
    if let Some(ms) = std::env::var("OVERSEER_TEST_AUTO_MAINTENANCE_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
    {
        return Duration::from_millis(ms.clamp(20, 1000));
    }
    Duration::from_secs(3600)
}

fn prune_one_batch(path: &Path, now_ms: i64) -> Result<bool> {
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
    conn.busy_timeout(Duration::from_millis(25))?;
    let tx = conn.unchecked_transaction()?;
    let detail = tx.execute(
        "DELETE FROM auto_measurements WHERE event_seq IN (
           SELECT event_seq FROM auto_measurements WHERE observed_ms < ?1
           ORDER BY observed_ms LIMIT ?2)",
        params![now_ms.saturating_sub(30 * DAY_MS), BATCH],
    )?;
    let summary = tx.execute(
        "DELETE FROM auto_daily_aggregates WHERE rowid IN (
           SELECT rowid FROM auto_daily_aggregates WHERE last_observed_ms < ?1
           ORDER BY last_observed_ms LIMIT ?2)",
        params![now_ms.saturating_sub(90 * DAY_MS), BATCH],
    )?;
    let thread = tx.execute(
        "DELETE FROM auto_thread_usage_observations WHERE id IN (
           SELECT id FROM auto_thread_usage_observations WHERE observed_ms < ?1
           ORDER BY observed_ms LIMIT ?2)",
        params![now_ms.saturating_sub(30 * DAY_MS), BATCH],
    )?;
    let work = tx.execute(
        "DELETE FROM auto_work_observations WHERE work_unit_id IN (
           SELECT work_unit_id FROM auto_work_observations WHERE observed_ms < ?1
           ORDER BY observed_ms LIMIT ?2)",
        params![now_ms.saturating_sub(30 * DAY_MS), BATCH],
    )?;
    let estimates = tx.execute(
        "DELETE FROM auto_allowance_estimates WHERE rowid IN (
           SELECT rowid FROM auto_allowance_estimates WHERE observed_ms < ?1
           ORDER BY observed_ms LIMIT ?2)",
        params![now_ms.saturating_sub(30 * DAY_MS), BATCH],
    )?;
    tx.commit()?;
    Ok([detail, summary, thread, work, estimates]
        .iter()
        .any(|count| *count as i64 == BATCH))
}

fn database_busy(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<rusqlite::Error>()
        .is_some_and(|sqlite| {
            matches!(sqlite, rusqlite::Error::SqliteFailure(code, _)
                if matches!(code.code, ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked))
        })
}

pub fn start(daemon: Arc<Daemon>) {
    if !daemon.store.lock().unwrap().learning_persistent {
        return;
    }
    let interval = quiet_interval();
    let path = paths::learning_db_path(&paths::db_path());
    let owner = daemon.clone();
    let worker = std::thread::Builder::new()
        .name("auto-learning-maintenance".into())
        .spawn(move || {
            let mut delay = interval;
            loop {
                std::thread::sleep(delay);
                match prune_one_batch(&path, crate::daemon::now()) {
                    Ok(more) => {
                        owner
                            .learning_maintenance_paused
                            .store(false, std::sync::atomic::Ordering::Relaxed);
                        delay = if more {
                            interval.min(Duration::from_millis(100))
                        } else {
                            interval
                        };
                    }
                    Err(error) if database_busy(&error) => {
                        delay = interval.min(Duration::from_millis(100));
                    }
                    Err(_) => {
                        owner
                            .learning_maintenance_paused
                            .store(true, std::sync::atomic::Ordering::Relaxed);
                        delay = interval.min(Duration::from_secs(60));
                    }
                }
            }
        });
    if worker.is_err() {
        daemon
            .learning_maintenance_paused
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }
}
