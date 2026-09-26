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
        if self.ui_clients.fetch_add(1, Ordering::SeqCst) == 0 {
            self.ui_session.lock().unwrap().0 = Some(Instant::now());
        }
        self.ui_epoch.fetch_add(1, Ordering::SeqCst);
    }

    /// Called when a VS Code connection closes. Schedules the background notice if it was the last.
    pub fn ui_disconnected(self: &Arc<Self>) {
        let left = self.ui_clients.fetch_sub(1, Ordering::SeqCst).saturating_sub(1);
        let epoch = self.ui_epoch.fetch_add(1, Ordering::SeqCst) + 1;
        if left > 0 {
            return;
        }
        {
            // A window that stayed open past the grace period was a real session: its quit may
            // notify again about the same agents. A shorter visit does not repeat the last notice.
            let mut session = self.ui_session.lock().unwrap();
            if session.0.take().is_some_and(|since| since.elapsed() >= grace()) {
                session.1 = None;
            }
        }
        let daemon = self.clone();
        tokio::spawn(async move {
            tokio::time::sleep(grace()).await;
            if daemon.ui_clients.load(Ordering::SeqCst) > 0 || daemon.ui_epoch.load(Ordering::SeqCst) != epoch {
                return;
            }
            if let Err(e) = daemon.background_notice_once() {
                crate::log(&format!("background notice failed: {e}"));
            }
        });
    }

    /// One notice per quit: skipped when the previous notice named the same agents and no VS Code
    /// session happened since.
    fn background_notice_once(&self) -> Result<Option<Value>> {
        let mut ids: Vec<String> = self.active_roots()?.into_iter().map(|r| r.id).collect();
        ids.sort();
        if !ids.is_empty() && self.ui_session.lock().unwrap().1.as_ref() == Some(&ids) {
            crate::log("last VS Code window closed again; these agents were already announced, no notice");
            return Ok(None);
        }
        let sent = self.background_notice()?;
        if sent.is_some() {
            self.ui_session.lock().unwrap().1 = Some(ids);
        }
        Ok(sent)
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
        // Commit the Swarm control transition before interrupting processes. A category
        // may have queued work but no Overseer run yet, so active_roots alone misses it.
        // Keep the launch lock through this transition so an admitted worker cannot
        // start between the snapshot and Stop.
        let swarms = {
            let _serial = self.swarm_launch_lock.lock().unwrap();
            let rows = {
                let store = self.store.lock().unwrap();
                let mut stmt = store.conn.prepare(
                    "SELECT id,generation,revision FROM swarm_runs
                     WHERE status IN ('planning','running','paused','stalled','draining','stopping')
                     ORDER BY id",
                )?;
                let rows = stmt.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?, row.get::<_, i64>(2)?)))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                rows
            };
            for (id, generation, revision) in &rows {
                crate::swarm::stop(&mut self.store.lock().unwrap(),
                    &json!({"run_id":id,"generation":generation,"revision":revision}))?;
            }
            rows.into_iter().map(|(id, _, _)| id).collect::<Vec<_>>()
        };
        for run in &swarms {
            if let Err(error) = crate::swarm::interrupt_workers(self, run) {
                crate::log(&format!("stop_all: swarm interrupt {run} failed: {error}"));
            }
        }
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
        let result = json!({"stopped": runs.iter().map(|r| r.id.clone()).collect::<Vec<_>>(), "swarms": swarms, "interrupted": interrupted, "forced": forced, "remaining": remaining});
        self.emit(None, None, "daemon_stopping", "user", "exact", result.clone())?;
        crate::log(&format!("stop_all: {result}"));
        Ok(result)
    }
}

/// Where a notification click takes the user: VS Code's Overseer view (extension URI handler).
pub const OPEN_URL: &str = "vscode://beelol.overseer/open-center";

/// The bundled notifier app (a `.app` directory): `OVERSEER_NOTIFIER_APP`, else
/// `Overseer Notifier.app` next to this daemon binary (the extension's `bin/`).
fn notifier_app() -> Option<std::path::PathBuf> {
    let app = std::env::var_os("OVERSEER_NOTIFIER_APP").map(std::path::PathBuf::from).or_else(|| {
        std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join("Overseer Notifier.app")))
    })?;
    app.join("Contents/MacOS/notifier").is_file().then_some(app)
}

/// Runs the notifier and returns its outcome code (0 posted, 3 denied, 5 no answer, other: failed).
/// macOS only lets an app use notifications when LaunchServices launched it, so it is started
/// with `open -n -W` and reports through a result file. Tests with fake helpers set
/// `OVERSEER_TEST_NOTIFIER_DIRECT` to execute the fake directly instead.
fn run_notifier(app: &std::path::Path, title: &str, body: &str) -> Result<i32, String> {
    let args = ["--title", title, "--body", body, "--open", OPEN_URL];
    if std::env::var_os("OVERSEER_TEST_NOTIFIER_DIRECT").is_some() {
        return std::process::Command::new(app.join("Contents/MacOS/notifier")).args(args).output().map(|o| o.status.code().unwrap_or(-1)).map_err(|e| e.to_string());
    }
    let result = crate::paths::runtime_dir().join(format!("notify-{}.result", std::process::id()));
    let _ = std::fs::remove_file(&result);
    let status = std::process::Command::new("/usr/bin/open").arg("-n").arg("-W").arg(app).arg("--args").args(args).arg("--result").arg(&result).status().map_err(|e| e.to_string())?;
    let text = std::fs::read_to_string(&result).unwrap_or_default();
    let _ = std::fs::remove_file(&result);
    if !status.success() {
        return Err(format!("open exited {:?}", status.code()));
    }
    text.split_whitespace().next().and_then(|c| c.parse().ok()).ok_or_else(|| "no result reported".to_string())
}

/// Sends the OS notification and says how it was delivered.
/// - `OVERSEER_NOTIFY_COMMAND` (an executable taking title and body) replaces everything (tests).
/// - macOS: the bundled Overseer notifier app (shows as Overseer; a click opens the Overseer view).
///   If it is missing, denied, or gets no answer to the first permission prompt, fall back to
///   `osascript` (shows as Script Editor), or to `OVERSEER_NOTIFY_FALLBACK` in tests.
pub fn notify(title: &str, body: &str) -> String {
    if let Ok(cmd) = std::env::var("OVERSEER_NOTIFY_COMMAND") {
        let ok = std::process::Command::new(&cmd).arg(title).arg(body).status().map(|s| s.success()).unwrap_or(false);
        return format!("{cmd} ({})", if ok { "ok" } else { "failed" });
    }
    #[cfg(target_os = "macos")]
    {
        let mut note = String::new();
        match notifier_app() {
            Some(app) => match run_notifier(&app, title, body) {
                Ok(0) => return "overseer-notifier (ok)".into(),
                Ok(3) => note = "overseer-notifier (denied); ".into(),
                Ok(5) => note = "overseer-notifier (permission not answered yet); ".into(),
                Ok(code) => note = format!("overseer-notifier (failed, exit {code}); "),
                Err(e) => note = format!("overseer-notifier (could not start: {e}); "),
            },
            None => note = "overseer-notifier (not installed); ".into(),
        }
        if let Ok(cmd) = std::env::var("OVERSEER_NOTIFY_FALLBACK") {
            let ok = std::process::Command::new(&cmd).arg(title).arg(body).status().map(|s| s.success()).unwrap_or(false);
            return format!("{note}fell back to {cmd} ({})", if ok { "ok" } else { "failed" });
        }
        let quote = |s: &str| format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""));
        let script = format!("display notification {} with title {}", quote(body), quote(title));
        let ok = std::process::Command::new("/usr/bin/osascript").arg("-e").arg(script).status().map(|s| s.success()).unwrap_or(false);
        format!("{note}fell back to osascript ({})", if ok { "ok" } else { "failed" })
    }
    #[cfg(not(target_os = "macos"))]
    {
        let ok = std::process::Command::new("notify-send").arg(title).arg(body).status().map(|s| s.success()).unwrap_or(false);
        format!("notify-send ({})", if ok { "ok" } else { "failed" })
    }
}
