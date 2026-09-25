//! Visible background agents (AC-45). Agents and the daemon keep running after VS Code closes,
//! but never silently: when the last VS Code window disconnects while runs are active, the
//! daemon posts an OS notification naming them and how to stop them. `stop_all` interrupts
//! every active run, forces any process that does not exit, and lets the daemon exit.

use crate::daemon::{now, Daemon, ACTIVE};
use crate::shim;
use crate::store::Run;
use anyhow::Result;
use serde_json::{json, Value};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Grace period before notifying, so a window reload (disconnect then reconnect) stays quiet.
fn grace() -> Duration {
    let ms = std::env::var("OVERSEER_BACKGROUND_NOTICE_MS").ok().and_then(|v| v.parse().ok()).unwrap_or(15_000u64);
    Duration::from_millis(ms)
}

impl Daemon {
    pub fn active_roots(&self) -> Result<Vec<Run>> {
        Ok(self.store.lock().unwrap().runs()?.into_iter().filter(|r| r.parent_run_id.is_none() && ACTIVE.contains(&r.status.as_str())).collect())
    }

    pub fn ui_connected(&self) {
        self.ui_clients.fetch_add(1, Ordering::SeqCst);
        self.ui_epoch.fetch_add(1, Ordering::SeqCst);
    }

    /// Called when a VS Code connection closes. Schedules the background notice if it was the last.
    pub fn ui_disconnected(self: &Arc<Self>) {
        let left = self.ui_clients.fetch_sub(1, Ordering::SeqCst).saturating_sub(1);
        let epoch = self.ui_epoch.fetch_add(1, Ordering::SeqCst) + 1;
        if left > 0 {
            return;
        }
        let daemon = self.clone();
        tokio::spawn(async move {
            tokio::time::sleep(grace()).await;
            if daemon.ui_clients.load(Ordering::SeqCst) > 0 || daemon.ui_epoch.load(Ordering::SeqCst) != epoch {
                return;
            }
            if let Err(e) = daemon.background_notice() {
                crate::log(&format!("background notice failed: {e}"));
            }
        });
    }

    /// Posts the "agents still running" notification if anything is active. Returns what was sent.
    pub fn background_notice(&self) -> Result<Option<Value>> {
        let runs = self.active_roots()?;
        if runs.is_empty() {
            crate::log("last VS Code window closed; no active agents, no notice");
            return Ok(None);
        }
        let names: Vec<String> = runs.iter().map(|r| format!("{}: {}", r.harness, r.title.chars().take(40).collect::<String>())).collect();
        let title = format!("Overseer: {} agent{} still running", runs.len(), if runs.len() == 1 { "" } else { "s" });
        let body = format!(
            "{}. They keep running with VS Code closed. Reopen VS Code to watch them, or run \u{201c}Overseer: Stop Agents and Daemon\u{201d}.",
            names.join("; ")
        );
        let via = notify(&title, &body);
        crate::log(&format!("background notice ({via}): {title} — {body}"));
        let payload = json!({"title": title, "body": body, "runs": runs.iter().map(|r| json!({"id": r.id, "harness": r.harness, "title": r.title, "status": r.status})).collect::<Vec<_>>(), "delivered_via": via});
        self.emit(None, None, "background_notice", "daemon", "exact", payload.clone())?;
        Ok(Some(payload))
    }

    /// Interrupts every active run, waits for their processes, forces stragglers, and reports.
    /// The caller exits the daemon afterwards.
    pub fn stop_all(self: &Arc<Self>) -> Result<Value> {
        let runs = self.active_roots()?;
        let mut interrupted = Vec::new();
        for run in &runs {
            match self.interrupt(&run.id) {
                Ok(_) => interrupted.push(run.id.clone()),
                Err(e) => crate::log(&format!("stop_all: interrupt {} failed: {e}", run.id)),
            }
        }
        let alive = |run: &Run| self.control_socket(run).ok().map(|s| shim::control(&s, &json!({"op": "ping"})).is_ok()).unwrap_or(false);
        let wait = |secs: u64| {
            let end = Instant::now() + Duration::from_secs(secs);
            while Instant::now() < end && runs.iter().any(|r| alive(r)) {
                std::thread::sleep(Duration::from_millis(200));
            }
        };
        wait(8);
        let mut forced = Vec::new();
        for (sig, secs) in [(libc::SIGTERM, 3), (libc::SIGKILL, 2)] {
            let left: Vec<&Run> = runs.iter().filter(|r| alive(r)).collect();
            if left.is_empty() {
                break;
            }
            for run in left {
                if let Ok(sock) = self.control_socket(run) {
                    let _ = shim::control(&sock, &json!({"op": "signal", "sig": sig}));
                }
                if !forced.contains(&run.id) {
                    forced.push(run.id.clone());
                }
            }
            wait(secs);
        }
        let remaining: Vec<String> = runs.iter().filter(|r| alive(r)).map(|r| r.id.clone()).collect();
        for run in &runs {
            // Anything that never got a process (queued) or whose tail has not finalized yet.
            let store = self.store.lock().unwrap();
            let status = store.run(&run.id).ok().flatten().map(|r| r.status).unwrap_or_default();
            if ACTIVE.contains(&status.as_str()) {
                store.conn.execute("UPDATE runs SET status='interrupted', exit_reason=COALESCE(exit_reason, 'stopped with the daemon'), ended_ms=COALESCE(ended_ms, ?2) WHERE id=?1", rusqlite::params![run.id, now()])?;
            }
        }
        let result = json!({"stopped": runs.iter().map(|r| r.id.clone()).collect::<Vec<_>>(), "interrupted": interrupted, "forced": forced, "remaining": remaining});
        self.emit(None, None, "daemon_stopping", "user", "exact", result.clone())?;
        crate::log(&format!("stop_all: {result}"));
        Ok(result)
    }
}

/// Sends the OS notification. `OVERSEER_NOTIFY_COMMAND` (an executable taking title and body)
/// replaces the platform notifier; tests use it to observe notices without desktop banners.
fn notify(title: &str, body: &str) -> String {
    if let Ok(cmd) = std::env::var("OVERSEER_NOTIFY_COMMAND") {
        let ok = std::process::Command::new(&cmd).arg(title).arg(body).status().map(|s| s.success()).unwrap_or(false);
        return format!("{cmd} ({})", if ok { "ok" } else { "failed" });
    }
    #[cfg(target_os = "macos")]
    {
        let quote = |s: &str| format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""));
        let script = format!("display notification {} with title {}", quote(body), quote(title));
        let ok = std::process::Command::new("/usr/bin/osascript").arg("-e").arg(script).status().map(|s| s.success()).unwrap_or(false);
        format!("osascript ({})", if ok { "ok" } else { "failed" })
    }
    #[cfg(not(target_os = "macos"))]
    {
        let ok = std::process::Command::new("notify-send").arg(title).arg(body).status().map(|s| s.success()).unwrap_or(false);
        format!("notify-send ({})", if ok { "ok" } else { "failed" })
    }
}
