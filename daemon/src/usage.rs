//! Usage and rate limits per account (AC-62), only as the harness reports them:
//! - Codex writes `token_count` events with `rate_limits` (5-hour and weekly windows) into the
//!   account's own session logs (`$CODEX_HOME/sessions/**/rollout-*.jsonl`); the newest one counts.
//! - Claude streams `rate_limit_event`s, which the adapter records as usage events on the run;
//!   the newest one from a run on the account counts.
//! Anything else is "not reported"; nothing is estimated or invented.

use crate::daemon::Daemon;
use anyhow::Result;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

fn window_label(minutes: i64) -> String {
    match minutes {
        300 => "5 hours".into(),
        10080 => "week".into(),
        1440 => "day".into(),
        m if m % 1440 == 0 => format!("{} days", m / 1440),
        m if m % 60 == 0 => format!("{} hours", m / 60),
        m => format!("{m} min"),
    }
}

/// Newest rollout log under `sessions` (walks year/month/day directories newest first).
fn newest_rollout(codex_home: &Path) -> Option<PathBuf> {
    let mut best: Option<(std::time::SystemTime, PathBuf)> = None;
    let mut stack = vec![codex_home.join("sessions")];
    let mut visited = 0;
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            visited += 1;
            if visited > 20000 {
                break;
            }
            let p = e.path();
            let Ok(meta) = e.metadata() else { continue };
            if meta.is_dir() {
                stack.push(p);
            } else if p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("rollout-") && n.ends_with(".jsonl")) {
                let m = meta.modified().ok()?;
                if best.as_ref().map_or(true, |(t, _)| m > *t) {
                    best = Some((m, p));
                }
            }
        }
    }
    best.map(|(_, p)| p)
}

/// The last `rate_limits` a Codex session log recorded (reads only token_count lines).
pub fn codex_limits(codex_home: &Path) -> Option<Value> {
    let file = newest_rollout(codex_home)?;
    let text = std::fs::read_to_string(&file).ok()?;
    let line = text.lines().rev().find(|l| l.contains("\"token_count\"") && l.contains("\"rate_limits\""))?;
    let v: Value = serde_json::from_str(line).ok()?;
    let rl = &v["payload"]["rate_limits"];
    let mut windows = Vec::new();
    for key in ["primary", "secondary"] {
        let w = &rl[key];
        if let Some(pct) = w["used_percent"].as_f64() {
            windows.push(json!({"label": window_label(w["window_minutes"].as_i64().unwrap_or(0)), "used": pct / 100.0, "resets_at_ms": w["resets_at"].as_i64().map(|s| s * 1000)}));
        }
    }
    let observed = std::fs::metadata(&file).ok().and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_millis() as i64);
    Some(json!({"reported": true, "source": "Codex session log", "plan": rl["plan_type"], "windows": windows, "observed_ms": observed,
        "limited": rl["rate_limit_reached_type"].is_string()}))
}

/// Normalizes a Claude `rate_limit_info` (as recorded by the adapter).
pub fn claude_limits(info: &Value, observed_ms: i64) -> Value {
    let mut windows = Vec::new();
    if let Some(map) = info["unifiedWindows"].as_object() {
        for (key, w) in map {
            let label = match key.as_str() { "five_hour" => "5 hours".to_string(), "seven_day" => "week".to_string(), "seven_day_opus" => "week (Opus)".to_string(), k => k.replace('_', " ") };
            if let Some(u) = w["utilization"].as_f64() {
                windows.push(json!({"label": label, "used": u, "resets_at_ms": w["resetsAt"].as_i64().map(|s| s * 1000)}));
            }
        }
    }
    json!({"reported": true, "source": "Claude rate_limit_event", "windows": windows, "observed_ms": observed_ms, "status": info["status"], "limited": info["status"] != "allowed" && info["status"].is_string()})
}

impl Daemon {
    pub fn account_usage(&self, id: &str) -> Result<Value> {
        let profile = self.profile(id)?;
        let env = Self::profile_env(&profile);
        let usage = match profile.harness.as_str() {
            "codex" => {
                let home = env.get("CODEX_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".codex")));
                home.and_then(|h| codex_limits(&h))
            }
            "claude" => {
                let store = self.store.lock().unwrap();
                let row: Option<(String, i64)> = store.conn.query_row(
                    "SELECT e.payload, e.ts FROM events e JOIN runs r ON r.id = e.run_id WHERE r.profile_id = ?1 AND e.kind = 'usage' AND e.payload LIKE '%\"claude_rate_limit\"%' ORDER BY e.seq DESC LIMIT 1",
                    [id], |r| Ok((r.get(0)?, r.get(1)?)),
                ).ok();
                row.and_then(|(p, ts)| serde_json::from_str::<Value>(&p).ok().map(|v| claude_limits(&v["rate_limits"]["claude_rate_limit"], ts)))
            }
            _ => None,
        };
        Ok(usage.unwrap_or_else(|| json!({"reported": false})))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn codex_session_log_limits_are_read_from_the_newest_token_count() {
        let dir = std::env::temp_dir().join(format!("ovs-usage-{}", std::process::id()));
        let day = dir.join("sessions/2026/09/25");
        std::fs::create_dir_all(&day).unwrap();
        std::fs::write(day.join("rollout-a.jsonl"), concat!(
            "{\"type\":\"event_msg\",\"payload\":{\"type\":\"agent_message\",\"message\":\"secret words\"}}\n",
            "{\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"rate_limits\":{\"primary\":{\"used_percent\":12.0,\"window_minutes\":300,\"resets_at\":1790413619},\"secondary\":{\"used_percent\":61.5,\"window_minutes\":10080,\"resets_at\":1790972190},\"plan_type\":\"team\"}}}\n",
        )).unwrap();
        let u = codex_limits(&dir).unwrap();
        assert_eq!(u["plan"], "team");
        assert_eq!(u["windows"][0]["label"], "5 hours");
        assert!((u["windows"][0]["used"].as_f64().unwrap() - 0.12).abs() < 1e-9);
        assert_eq!(u["windows"][1]["label"], "week");
        assert_eq!(u["windows"][1]["resets_at_ms"], 1790972190000i64);
        assert!(!u.to_string().contains("secret"), "only limits are read");
        std::fs::remove_dir_all(&dir).ok();
    }
    #[test]
    fn claude_rate_limit_info_is_normalized() {
        let info = json!({"status": "allowed", "unifiedWindows": {"five_hour": {"utilization": 0.1, "resetsAt": 1790429400}, "seven_day": {"utilization": 0.46, "resetsAt": 1790568000}}});
        let u = claude_limits(&info, 5);
        assert_eq!(u["windows"].as_array().unwrap().len(), 2);
        assert!(u["windows"].as_array().unwrap().iter().any(|w| w["label"] == "week" && w["used"] == 0.46));
        assert_eq!(u["limited"], false);
    }
}
