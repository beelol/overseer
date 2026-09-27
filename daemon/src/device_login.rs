//! Signing in from a phone (Gate N, AC-127): the provider's device-code flow. The daemon starts
//! the harness's own sign-in on the Mac and hands the phone the address and the code it prints;
//! the person finishes in the phone's browser. No credential passes through the phone.
//! A provider without such a flow is signed in on the Mac.

use crate::adapters;
use crate::daemon::Daemon;
use crate::server::ProtoError;
use anyhow::Result;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

/// How long the daemon waits for the harness to print the address and the code.
const PRINT_WAIT: Duration = Duration::from_secs(20);
/// How long the person has to finish in the browser.
const FINISH_WAIT: Duration = Duration::from_secs(15 * 60);

fn strip_ansi(text: &str) -> String {
    regex::Regex::new(r"\x1b\[[0-9;?]*[ -/]*[@-~]").unwrap().replace_all(text, "").to_string()
}

/// The address and the one-time code in what a harness printed.
pub fn find_code(lines: &[String]) -> (Option<String>, Option<String>) {
    let url = regex::Regex::new(r"https://[^\s\x22\x27<>)]+").unwrap();
    let code = regex::Regex::new(r"\b[A-Z0-9]{4,}-[A-Z0-9]{4,}\b").unwrap();
    let mut found_url = None;
    let mut found_code = None;
    for line in lines {
        let line = strip_ansi(line);
        if found_url.is_none() {
            found_url = url.find(&line).map(|m| m.as_str().trim_end_matches(['.', ',']).to_string());
        }
        // The code is looked for outside addresses, so a code-like part of a link is not taken.
        let without_urls = url.replace_all(&line, " ");
        if found_code.is_none() {
            found_code = code.find(&without_urls).map(|m| m.as_str().to_string());
        }
    }
    (found_url, found_code)
}

impl Daemon {
    pub fn device_login(self: &Arc<Self>, id: &str) -> Result<Value> {
        let profile = self.profile(id)?;
        let args: Vec<&str> = match profile.harness.as_str() {
            "codex" => vec!["login", "--device-auth"],
            other => {
                return Err(ProtoError::new("mac_only", format!("Sign in on the Mac: {other} has no sign-in with a code.")).into());
            }
        };
        let program = adapters::resolve_program(&profile.harness).ok_or_else(|| ProtoError::new("mac_setup", format!("{} is not installed on the Mac", profile.harness)))?;
        let mut cmd = std::process::Command::new(&program);
        cmd.args(&args).env_clear();
        cmd.current_dir(adapters::neutral_dir());
        for (k, v) in adapters::base_env(&program.display().to_string()) {
            cmd.env(k, v);
        }
        for (k, v) in Self::profile_env(&profile) {
            cmd.env(k, v);
        }
        let mut child = cmd.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).spawn()?;
        let (tx, rx) = mpsc::channel::<String>();
        for stream in [child.stdout.take().map(|s| Box::new(s) as Box<dyn std::io::Read + Send>), child.stderr.take().map(|s| Box::new(s) as Box<dyn std::io::Read + Send>)].into_iter().flatten() {
            let tx = tx.clone();
            std::thread::spawn(move || {
                for line in BufReader::new(stream).lines().map_while(|l| l.ok()) {
                    if tx.send(line).is_err() {
                        break;
                    }
                }
            });
        }
        drop(tx);
        let mut lines: Vec<String> = Vec::new();
        let started = Instant::now();
        let (mut url, mut code) = (None, None);
        let mut ended_early = None;
        while started.elapsed() < PRINT_WAIT && (url.is_none() || code.is_none()) {
            match rx.recv_timeout(Duration::from_millis(200)) {
                Ok(line) => {
                    lines.push(line);
                    (url, code) = find_code(&lines);
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    ended_early = child.wait().ok().and_then(|s| s.code());
                    break;
                }
            }
        }
        if let Some(exit) = ended_early {
            // It finished without asking for a code: already signed in, or it failed.
            let status = self.profile_status(id)?;
            self.emit(None, None, "profile", "daemon", "exact", json!({"profile_id": id, "action": "login", "exit": exit, "logged_in": status["logged_in"]}))?;
            return Ok(json!({"profile_id": id, "finished": true, "exit": exit, "logged_in": status["logged_in"], "output": crate::redact::redact(&strip_ansi(&lines.join("\n")))}));
        }
        let (Some(url), Some(code)) = (url, code) else {
            let _ = child.kill();
            let _ = child.wait();
            return Err(ProtoError::new("mac_only", format!("Sign in on the Mac: {} did not offer a code.", profile.harness)).into());
        };
        // The person finishes in the browser; the harness notices and ends by itself.
        let daemon = self.clone();
        let profile_id = id.to_string();
        std::thread::spawn(move || {
            let deadline = Instant::now() + FINISH_WAIT;
            let exit = loop {
                match child.try_wait() {
                    Ok(Some(status)) => break status.code(),
                    Ok(None) if Instant::now() > deadline => {
                        let _ = child.kill();
                        let _ = child.wait();
                        break None;
                    }
                    Ok(None) => std::thread::sleep(Duration::from_millis(300)),
                    Err(_) => break None,
                }
            };
            let logged_in = daemon.profile_status(&profile_id).ok().map(|s| s["logged_in"].clone()).unwrap_or(Value::Null);
            let _ = daemon.emit(None, None, "profile", "daemon", "exact", json!({"profile_id": profile_id, "action": "login", "exit": exit, "logged_in": logged_in, "timed_out": exit.is_none()}));
        });
        Ok(json!({"profile_id": id, "finished": false, "url": url, "code": code, "valid_ms": FINISH_WAIT.as_millis() as u64}))
    }
}

#[cfg(test)]
mod tests {
    use super::find_code;

    #[test]
    fn the_address_and_the_code_are_found_in_what_a_harness_prints() {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        let printed = s(&["Follow these steps to sign in with ChatGPT using device code authorization:", "", "1. Open this link in your browser and sign in to your account", "   \u{1b}[94mhttps://auth.openai.com/codex/device\u{1b}[0m", "", "2. Enter this one-time code (expires in 15 minutes)", "   \u{1b}[1mABCD-EFGH1\u{1b}[0m"]);
        assert_eq!(find_code(&printed), (Some("https://auth.openai.com/codex/device".into()), Some("ABCD-EFGH1".into())));
        assert_eq!(find_code(&s(&["Open https://example.invalid/activate?user_code=WXYZ-1234 to continue."])), (Some("https://example.invalid/activate?user_code=WXYZ-1234".into()), None));
        assert_eq!(find_code(&s(&["Logged in."])), (None, None));
    }
}
