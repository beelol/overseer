//! Bounded metadata-only reads. This module never starts a model turn.

use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const MAX_FRAME: usize = 1024 * 1024;
const MAX_FRAMES: usize = 100;

#[derive(Clone, Debug)]
pub struct ClaudeAuth {
    pub fingerprint: String,
    pub observed_ms: i64,
    /// The account's `subscriptionType`, bounded to a plain identifier
    /// (anything else is unknown). It stands for the plan of the account's
    /// Claude quota readings until the next identity read (the owner's
    /// decision of 2026-09-28).
    pub plan: Option<String>,
}

/// A plan label as reported, if it is a short plain identifier.
pub fn bounded_plan(value: Option<&Value>) -> Option<String> {
    value.and_then(Value::as_str)
        .filter(|plan| !plan.is_empty() && plan.len() <= 40
            && plan.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')))
        .map(str::to_string)
}

fn parse_claude_auth_status(value: &Value, observed_ms: i64) -> Result<ClaudeAuth> {
    if value.get("loggedIn").and_then(Value::as_bool) != Some(true)
        || value.get("authMethod").and_then(Value::as_str) != Some("claude.ai") {
        return Err(anyhow!("Claude profile is not signed in with a Claude account"));
    }
    let email = value.get("email").and_then(Value::as_str).unwrap_or("");
    let org = value.get("orgId").and_then(Value::as_str).unwrap_or("");
    if (email.is_empty() && org.is_empty()) || email.len() > 256 || org.len() > 256 {
        return Err(anyhow!("Claude account identity is unavailable"));
    }
    let mut digest = Sha256::new();
    digest.update(b"overseer:auto:claude-account:v1\0");
    digest.update(email.to_ascii_lowercase().as_bytes());
    digest.update(b"\0");
    digest.update(org.as_bytes());
    Ok(ClaudeAuth { fingerprint:format!("{:x}", digest.finalize()), observed_ms,
        plan:bounded_plan(value.get("subscriptionType")) })
}

/// `claude auth status --json` is a local metadata read. The child has no API
/// keys, no stdin or repository cwd, a 16 KiB output cap and one deadline.
pub fn claude_auth_status(program: &Path, profile_env: &BTreeMap<String, String>,
    timeout: Duration, observed_ms: i64) -> Result<ClaudeAuth> {
    for key in profile_env.keys() {
        if crate::adapters::forbidden_env(key) { return Err(anyhow!("unsafe Claude auth environment")); }
    }
    let mut env = crate::adapters::base_env(&program.display().to_string());
    env.extend(profile_env.clone());
    let mut child = Command::new(program).args(["auth", "status", "--json"])
        .current_dir(crate::adapters::neutral_dir()).env_clear().envs(&env)
        .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn()?;
    let result = (|| -> Result<ClaudeAuth> {
        let mut stdout = child.stdout.take().ok_or_else(|| anyhow!("Claude auth stdout unavailable"))?;
        let deadline = Instant::now() + timeout;
        let mut bytes = Vec::new();
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() { return Err(anyhow!("Claude auth metadata timed out")); }
            let mut pollfd = libc::pollfd { fd:stdout.as_raw_fd(), events:libc::POLLIN | libc::POLLHUP, revents:0 };
            let ready = unsafe { libc::poll(&mut pollfd, 1, remaining.as_millis().min(i32::MAX as u128) as i32) };
            if ready == 0 { return Err(anyhow!("Claude auth metadata timed out")); }
            if ready < 0 {
                if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted { continue; }
                return Err(anyhow!("Claude auth metadata stream failed"));
            }
            let mut chunk = [0u8; 4096];
            let count = stdout.read(&mut chunk)?;
            if count == 0 { break; }
            bytes.extend_from_slice(&chunk[..count]);
            if bytes.len() > 16 * 1024 { return Err(anyhow!("Claude auth metadata exceeded its bound")); }
        }
        loop {
            if let Some(status) = child.try_wait()? {
                if !status.success() { return Err(anyhow!("Claude auth status command failed")); }
                break;
            }
            if Instant::now() >= deadline { return Err(anyhow!("Claude auth metadata timed out")); }
            std::thread::sleep(Duration::from_millis(10));
        }
        let value: Value = serde_json::from_slice(&bytes).map_err(|_| anyhow!("invalid Claude auth status JSON"))?;
        parse_claude_auth_status(&value, observed_ms)
    })();
    let _ = child.kill();
    let _ = child.wait();
    result
}

#[derive(Debug)]
struct OpenCodeReadTimeout;

impl std::fmt::Display for OpenCodeReadTimeout {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("OpenCode metadata read timed out")
    }
}

impl std::error::Error for OpenCodeReadTimeout {}

fn opencode_cli_output(program: &Path, env: &BTreeMap<String, String>, cwd: &Path,
    args: &[&str], deadline: Instant) -> Result<Vec<u8>> {
    let mut child = Command::new(program).args(args).current_dir(cwd)
        .env_clear().envs(env).env("OPENCODE_DISABLE_AUTOUPDATE", "1")
        .env("OPENCODE_DISABLE_MODELS_FETCH", "1")
        .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn()?;
    let result = (|| -> Result<Vec<u8>> {
        let mut stdout = child.stdout.take().ok_or_else(|| anyhow!("OpenCode metadata stdout unavailable"))?;
        let mut bytes = Vec::new();
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() { return Err(OpenCodeReadTimeout.into()); }
            let mut pollfd = libc::pollfd { fd:stdout.as_raw_fd(), events:libc::POLLIN | libc::POLLHUP, revents:0 };
            let ready = unsafe { libc::poll(&mut pollfd, 1,
                remaining.as_millis().min(i32::MAX as u128) as i32) };
            if ready == 0 { return Err(OpenCodeReadTimeout.into()); }
            if ready < 0 {
                if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted { continue; }
                return Err(anyhow!("OpenCode metadata stream failed"));
            }
            let mut chunk = [0u8; 8192];
            let count = stdout.read(&mut chunk)?;
            if count == 0 { break; }
            bytes.extend_from_slice(&chunk[..count]);
            if bytes.len() > MAX_FRAME { return Err(anyhow!("OpenCode metadata exceeded its output bound")); }
        }
        loop {
            if let Some(status) = child.try_wait()? {
                if !status.success() { return Err(anyhow!("OpenCode metadata command failed")); }
                break;
            }
            if Instant::now() >= deadline { return Err(OpenCodeReadTimeout.into()); }
            std::thread::sleep(Duration::from_millis(10));
        }
        Ok(bytes)
    })();
    let _ = child.kill();
    let _ = child.wait();
    result
}

fn parse_opencode_models_verbose(bytes: &[u8]) -> Result<Vec<(String, Value)>> {
    let mut offset = 0;
    let mut models = Vec::new();
    while offset < bytes.len() {
        while offset < bytes.len() && bytes[offset].is_ascii_whitespace() { offset += 1; }
        if offset == bytes.len() { break; }
        let end = bytes[offset..].iter().position(|byte| *byte == b'\n')
            .ok_or_else(|| anyhow!("invalid OpenCode model listing"))? + offset;
        let id = std::str::from_utf8(&bytes[offset..end])
            .map_err(|_| anyhow!("invalid OpenCode model identifier"))?;
        if id.is_empty() || id.len() > 256 || models.len() >= 2048 {
            return Err(anyhow!("OpenCode model listing exceeded its bound"));
        }
        offset = end + 1;
        let mut stream = serde_json::Deserializer::from_slice(&bytes[offset..]).into_iter::<Value>();
        let model = stream.next().ok_or_else(|| anyhow!("missing OpenCode model metadata"))?
            .map_err(|_| anyhow!("invalid OpenCode model metadata"))?;
        offset += stream.byte_offset();
        models.push((id.into(), model));
    }
    Ok(models)
}

/// Read resolved configuration and model metadata from the installed OpenCode
/// CLI without starting a server or model turn. Both reads share one deadline;
/// only normalized explicit local providers leave this function.
pub fn opencode_local_catalog(
    program: &Path,
    env: &BTreeMap<String, String>,
    cwd: &Path,
    timeout: Duration,
    observed_ms: i64,
) -> Result<crate::auto_opencode::LocalCatalog> {
    let config_path = cwd.join("opencode.json");
    let metadata = std::fs::symlink_metadata(&config_path)?;
    if !metadata.file_type().is_file() || metadata.len() > 128 * 1024 {
        return Err(anyhow!("OpenCode project configuration is unavailable or oversized"));
    }
    let config: Value = serde_json::from_slice(&std::fs::read(&config_path)?)?;
    if !config.get("provider").is_some_and(Value::is_object) {
        return Err(anyhow!("OpenCode project has no explicit provider map"));
    }
    let deadline = Instant::now() + timeout;
    // These independent CLI reads each pay OpenCode startup cost. Running them
    // sequentially can consume the whole metadata deadline before either
    // result is checked. A newly created profile may race its one-time local
    // database migration, so retry only a failed command once within the same
    // deadline after its peer has exited.
    let (resolved_read, model_read) = std::thread::scope(|scope| {
        let resolved = scope.spawn(|| opencode_cli_output(program, env, cwd,
            &["debug", "config", "--pure"], deadline));
        let models = scope.spawn(|| opencode_cli_output(program, env, cwd,
            &["models", "--pure", "--verbose"], deadline));
        (resolved.join(), models.join())
    });
    let resolved_read = resolved_read.map_err(|_| anyhow!("OpenCode config worker failed"))?;
    let model_read = model_read.map_err(|_| anyhow!("OpenCode model worker failed"))?;
    let resolved_bytes = match resolved_read {
        Err(error) if error.to_string() == "OpenCode metadata command failed" =>
            opencode_cli_output(program, env, cwd, &["debug", "config", "--pure"], deadline)?,
        other => other?,
    };
    let resolved: Value = serde_json::from_slice(&resolved_bytes)
        .map_err(|_| anyhow!("invalid OpenCode resolved configuration"))?;
    let model_bytes = match model_read {
        Err(error) if error.to_string() == "OpenCode metadata command failed" =>
            opencode_cli_output(program, env, cwd, &["models", "--pure", "--verbose"], deadline)?,
        other => other?,
    };
    let models = parse_opencode_models_verbose(&model_bytes)?;
    crate::auto_opencode::parse_local_cli_catalog(&config, &resolved, &models, observed_ms)
}

/// Public, credential-free status metadata. URLs are fixed in product code;
/// the response is bounded and later reduced to an advisory component state.
pub fn public_status_json(program: &Path, provider: &str, timeout: Duration) -> Result<Value> {
    let url = match provider {
        "openai" => "https://status.openai.com/api/v2/summary.json",
        "anthropic" => "https://status.claude.com/api/v2/summary.json",
        _ => return Err(anyhow!("unsupported public status provider")),
    };
    if timeout < Duration::from_millis(20) || timeout > Duration::from_secs(2) {
        return Err(anyhow!("public status deadline is out of bounds"));
    }
    let max_time = format!("{:.3}", timeout.as_secs_f64());
    let mut child = Command::new(program)
        .args(["-q", "--fail", "--silent", "--show-error", "--max-time", &max_time,
            "--connect-timeout", &max_time, "--max-filesize", "131072", "--noproxy", "*",
            "--proto", "=https", url])
        .current_dir(crate::adapters::neutral_dir()).env_clear()
        .env("PATH", "/usr/bin:/bin:/usr/local/bin").env("HOME", "/var/empty")
        .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn()?;
    let result = (|| -> Result<Value> {
        let deadline = Instant::now() + timeout;
        let mut stdout = child.stdout.take().ok_or_else(|| anyhow!("public status output unavailable"))?;
        let mut bytes = Vec::new();
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() { return Err(anyhow!("public status read timed out")); }
            let mut pollfd = libc::pollfd {fd:stdout.as_raw_fd(), events:libc::POLLIN | libc::POLLHUP, revents:0};
            let ready = unsafe { libc::poll(&mut pollfd, 1, remaining.as_millis().min(i32::MAX as u128) as i32) };
            if ready == 0 { return Err(anyhow!("public status read timed out")); }
            if ready < 0 {
                if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted { continue; }
                return Err(anyhow!("public status stream failed"));
            }
            let mut chunk = [0u8; 4096];
            let count = stdout.read(&mut chunk)?;
            if count == 0 { break; }
            bytes.extend_from_slice(&chunk[..count]);
            if bytes.len() > 128 * 1024 { return Err(anyhow!("public status response exceeded its bound")); }
        }
        loop {
            if let Some(status) = child.try_wait()? {
                if !status.success() { return Err(anyhow!("public status request failed")); }
                break;
            }
            if Instant::now() >= deadline { return Err(anyhow!("public status read timed out")); }
            std::thread::sleep(Duration::from_millis(10));
        }
        serde_json::from_slice(&bytes).map_err(|_| anyhow!("invalid public status JSON"))
    })();
    let _ = child.kill();
    let _ = child.wait();
    result
}

#[cfg(test)]
mod opencode_metadata_tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn independent_cli_metadata_reads_share_the_deadline() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("project");
        std::fs::create_dir(&project).unwrap();
        let config = json!({"provider":{"local":{"npm":"@ai-sdk/openai-compatible",
            "options":{"baseURL":"http://127.0.0.1:47811/v1"},
            "models":{"fixture":{"name":"Fixture","tool_call":true}}}},
            "model":"local/fixture"});
        std::fs::write(project.join("opencode.json"), serde_json::to_vec(&config).unwrap()).unwrap();
        let resolved = dir.path().join("resolved.json");
        std::fs::write(&resolved, serde_json::to_vec(&config).unwrap()).unwrap();
        let models = dir.path().join("models.txt");
        std::fs::write(&models, b"local/fixture\n{\"id\":\"fixture\",\"providerID\":\"local\",\"status\":\"active\",\"limit\":{\"context\":32000},\"capabilities\":{\"toolcall\":true},\"variants\":{}}\n").unwrap();
        let program = dir.path().join("fixture-opencode");
        std::fs::write(&program, "#!/bin/sh\ncase \"$1\" in\n  debug) touch \"$META_DEBUG_MARKER\"; while [ ! -e \"$META_MODELS_MARKER\" ]; do sleep 0.01; done; cat \"$META_RESOLVED_FILE\" ;;\n  models) touch \"$META_MODELS_MARKER\"; while [ ! -e \"$META_DEBUG_MARKER\" ]; do sleep 0.01; done; if [ ! -e \"$META_RETRY_MARKER\" ]; then touch \"$META_RETRY_MARKER\"; exit 1; fi; cat \"$META_MODELS_FILE\" ;;\n  *) exit 2 ;;\nesac\n").unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700)).unwrap();
        let env = BTreeMap::from([
            ("PATH".into(), "/usr/bin:/bin".into()),
            ("META_DEBUG_MARKER".into(), dir.path().join("debug.started").display().to_string()),
            ("META_MODELS_MARKER".into(), dir.path().join("models.started").display().to_string()),
            ("META_RESOLVED_FILE".into(), resolved.display().to_string()),
            ("META_MODELS_FILE".into(), models.display().to_string()),
            ("META_RETRY_MARKER".into(), dir.path().join("models.retried").display().to_string()),
        ]);
        let catalog = opencode_local_catalog(&program, &env, &project,
            Duration::from_secs(3), 1000).unwrap();
        assert_eq!(catalog.models.len(), 1);
        assert_eq!(catalog.models[0].model, "local/fixture");
        assert!(dir.path().join("models.retried").exists());
    }

    #[test]
    fn verbose_model_listing_is_structured_and_fail_closed() {
        let records = parse_opencode_models_verbose(
            b"local_a/fixture-a\n{\"id\":\"fixture-a\",\"providerID\":\"local_a\"}\nlocal_b/fixture-b\n{\"id\":\"fixture-b\",\"providerID\":\"local_b\"}\n"
        ).unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(records[1].0, "local_b/fixture-b");
        assert!(parse_opencode_models_verbose(b"local_a/fixture-a\n{bad json}").is_err());
        assert!(parse_opencode_models_verbose(b"local_a/fixture-a without metadata").is_err());
    }

    #[test]
    fn stalled_cli_metadata_is_killed_within_one_deadline() {
        let dir = tempfile::tempdir().unwrap();
        let program = dir.path().join("fixture-opencode");
        std::fs::write(&program, "#!/bin/sh\nexec sleep 2\n").unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700)).unwrap();
        let started = Instant::now();
        let error = opencode_cli_output(&program, &BTreeMap::new(), dir.path(),
            &["debug", "config", "--pure"], Instant::now() + Duration::from_millis(60))
            .unwrap_err().to_string();
        assert!(error.contains("timed out"), "{error}");
        assert!(started.elapsed() < Duration::from_millis(500));
    }
}

#[cfg(test)]
mod claude_auth_tests {
    use super::*;

    #[test]
    fn authenticated_claude_account_is_fingerprinted_without_retaining_identity() {
        let payload = json!({"loggedIn":true,"authMethod":"claude.ai",
            "email":"private@example.test","orgId":"private-org","subscriptionType":"max"});
        let auth = parse_claude_auth_status(&payload, 1000).unwrap();
        assert_eq!(auth.fingerprint.len(), 64);
        assert_eq!(auth.observed_ms, 1000);
        assert!(!format!("{auth:?}").contains("private@example.test"));
        assert!(!format!("{auth:?}").contains("private-org"));
        assert_eq!(auth.fingerprint, parse_claude_auth_status(&payload, 2000).unwrap().fingerprint);
        assert_ne!(auth.fingerprint, parse_claude_auth_status(&json!({"loggedIn":true,
            "authMethod":"claude.ai","email":"other@example.test","orgId":"private-org"}), 2000)
            .unwrap().fingerprint);
    }

    #[test]
    fn the_identity_read_carries_a_bounded_plan() {
        let read = |plan: Value| parse_claude_auth_status(&json!({"loggedIn":true,"authMethod":"claude.ai",
            "email":"a@example.test","subscriptionType":plan}), 1000).unwrap().plan;
        assert_eq!(read(json!("max")).as_deref(), Some("max"));
        assert_eq!(read(json!("team_premium")).as_deref(), Some("team_premium"));
        assert_eq!(read(Value::Null), None);
        assert_eq!(read(json!("Max plan (billed to someone@example.test)")), None, "free text is unknown");
        assert_eq!(read(json!(5)), None);
        assert_eq!(read(json!("x".repeat(41))), None);
    }

    #[test]
    fn claude_api_key_or_missing_identity_never_authorizes_auto() {
        for payload in [json!({"loggedIn":true,"authMethod":"api-key","email":"a@example.test"}),
            json!({"loggedIn":true,"authMethod":"claude.ai"}),
            json!({"loggedIn":false,"authMethod":"claude.ai","email":"a@example.test"})] {
            assert!(parse_claude_auth_status(&payload, 1000).is_err());
        }
    }
}

#[cfg(test)]
mod public_status_tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn public_status_read_uses_a_fixed_url_and_bounded_structured_output() {
        let dir = tempfile::tempdir().unwrap();
        let program = dir.path().join("fixture-curl");
        std::fs::write(&program, "#!/bin/sh\ncase \"$*\" in *https://status.claude.com/api/v2/summary.json*) printf '%s' '{\"components\":[{\"name\":\"Claude Code\",\"status\":\"operational\"}]}' ;; *) exit 2 ;; esac\n").unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700)).unwrap();
        let value = public_status_json(&program, "anthropic", Duration::from_secs(1)).unwrap();
        assert_eq!(value["components"][0]["name"], "Claude Code");
        assert!(public_status_json(&program, "unknown", Duration::from_secs(1)).is_err());
        std::fs::write(&program, "#!/bin/sh\nhead -c 140000 /dev/zero\n").unwrap();
        assert!(public_status_json(&program, "anthropic", Duration::from_secs(1)).is_err());
    }
}

struct Reader {
    pending: Vec<u8>,
    frames: usize,
}

impl Reader {
    fn response(
        &mut self,
        stdout: &mut std::process::ChildStdout,
        wanted: i64,
        deadline: Instant,
    ) -> Result<Value> {
        loop {
            if let Some(end) = self.pending.iter().position(|b| *b == b'\n') {
                let frame = self.pending.drain(..=end).collect::<Vec<_>>();
                self.frames += 1;
                if self.frames > MAX_FRAMES || frame.len() > MAX_FRAME {
                    return Err(anyhow!("Codex metadata response exceeded its bound"));
                }
                let value: Value = serde_json::from_slice(&frame)
                    .map_err(|_| anyhow!("invalid Codex metadata response"))?;
                if value.get("id").and_then(Value::as_i64) == Some(wanted) {
                    if value.get("error").is_some() {
                        return Err(anyhow!("Codex metadata request was rejected"));
                    }
                    return value
                        .get("result")
                        .filter(|v| v.is_object())
                        .cloned()
                        .ok_or_else(|| anyhow!("Codex metadata result was unavailable"));
                }
                if value.get("id").is_some() {
                    return Err(anyhow!("unexpected Codex metadata response ID"));
                }
                continue;
            }
            if self.pending.len() >= MAX_FRAME {
                return Err(anyhow!("Codex metadata frame exceeded its bound"));
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(anyhow!("Codex metadata read timed out"));
            }
            let mut pollfd = libc::pollfd {
                fd: stdout.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            let wait_ms = remaining.as_millis().min(i32::MAX as u128) as i32;
            let ready = unsafe { libc::poll(&mut pollfd, 1, wait_ms) };
            if ready == 0 {
                return Err(anyhow!("Codex metadata read timed out"));
            }
            if ready < 0 {
                if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(anyhow!("Codex metadata stream failed"));
            }
            let mut bytes = [0_u8; 8192];
            let count = stdout
                .read(&mut bytes)
                .map_err(|_| anyhow!("Codex metadata stream failed"))?;
            if count == 0 {
                return Err(anyhow!("Codex metadata process ended early"));
            }
            self.pending.extend_from_slice(&bytes[..count]);
        }
    }
}

fn request(
    stdin: &mut std::process::ChildStdin,
    id: i64,
    method: &str,
    params: Value,
) -> Result<()> {
    let line = json!({"id":id,"method":method,"params":params}).to_string();
    stdin.write_all(line.as_bytes())?;
    stdin.write_all(b"\n")?;
    stdin.flush()?;
    Ok(())
}

/// One profile-scoped metadata process with an account-login guard and one
/// overall deadline. The child is always killed/reaped; no model turn starts.
fn with_codex_account<T>(
    program: &Path,
    env: &BTreeMap<String, String>,
    cwd: &Path,
    timeout: Duration,
    operation: impl FnOnce(
        &mut std::process::ChildStdin,
        &mut std::process::ChildStdout,
        &mut Reader,
        Instant,
    ) -> Result<T>,
) -> Result<T> {
    with_codex_account_seen(program, env, cwd, timeout, None, operation)
}

/// The ChatGPT account a Codex profile is signed in to, as Codex's own `account/read` says it
/// (`email`, `planType`): what every surface shows of it (AC-235). Metadata only, no model turn;
/// the caller keeps the email shortened and nothing else.
pub fn codex_account(program: &Path, env: &BTreeMap<String, String>, cwd: &Path, timeout: Duration) -> Result<Value> {
    let mut seen = Value::Null;
    with_codex_account_seen(program, env, cwd, timeout, Some(&mut seen), |_, _, _, _| Ok(()))?;
    Ok(seen)
}

fn with_codex_account_seen<T>(
    program: &Path,
    env: &BTreeMap<String, String>,
    cwd: &Path,
    timeout: Duration,
    seen_account: Option<&mut Value>,
    operation: impl FnOnce(
        &mut std::process::ChildStdin,
        &mut std::process::ChildStdout,
        &mut Reader,
        Instant,
    ) -> Result<T>,
) -> Result<T> {
    let mut child = Command::new(program)
        .arg("app-server")
        .current_dir(cwd)
        .env_clear()
        .envs(env)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let outcome = (|| {
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow!("Codex metadata stdin unavailable"))?;
        let mut stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("Codex metadata stdout unavailable"))?;
        let mut reader = Reader {
            pending: Vec::new(),
            frames: 0,
        };
        let deadline = Instant::now() + timeout;
        request(
            &mut stdin,
            1,
            "initialize",
            json!({"clientInfo":{"name":"overseer","version":env!("CARGO_PKG_VERSION")},"capabilities":{"experimentalApi":true}}),
        )?;
        reader.response(&mut stdout, 1, deadline)?;
        stdin.write_all(b"{\"method\":\"initialized\"}\n")?;
        stdin.flush()?;
        request(&mut stdin, 2, "account/read", json!({"refreshToken":false}))?;
        let account = reader.response(&mut stdout, 2, deadline)?;
        if account["requiresOpenaiAuth"] != true || account["account"]["type"] != "chatgpt" {
            return Err(anyhow!(
                "Codex profile is not signed in with a ChatGPT account"
            ));
        }
        if let Some(seen) = seen_account { *seen = account["account"].clone(); }
        operation(&mut stdin, &mut stdout, &mut reader, deadline)
    })();
    let _ = child.kill();
    let _ = child.wait();
    outcome
}

/// Uses only account metadata RPCs. Raw values remain ephemeral and must be
/// normalized by the caller before persistence or protocol output.
pub fn codex_rate_limits(
    program: &Path,
    env: &BTreeMap<String, String>,
    cwd: &Path,
    timeout: Duration,
) -> Result<Value> {
    with_codex_account(
        program,
        env,
        cwd,
        timeout,
        |stdin, stdout, reader, deadline| {
            request(stdin, 3, "account/rateLimits/read", json!({}))?;
            reader.response(stdout, 3, deadline)
        },
    )
}

/// A complete, bounded model catalog and an account-identity-bearing quota
/// response read from the same authenticated metadata session.
pub struct CodexDiscovery {
    pub rate_limits: Value,
    pub rate_limits_observed_ms: i64,
    pub models: Value,
}

pub fn codex_model_list(
    program: &Path,
    env: &BTreeMap<String, String>,
    cwd: &Path,
    timeout: Duration,
) -> Result<CodexDiscovery> {
    with_codex_account(
        program,
        env,
        cwd,
        timeout,
        |stdin, stdout, reader, deadline| {
            request(stdin, 3, "account/rateLimits/read", json!({}))?;
            let rate_limits = reader.response(stdout, 3, deadline)?;
            // Model pagination may consume most of the discovery deadline.
            // Quota freshness begins when its own response arrives, not when
            // the later catalog has finished loading.
            let rate_limits_observed_ms = crate::daemon::now();
            let mut data = Vec::new();
            let mut cursor: Option<String> = None;
            let mut seen = std::collections::BTreeSet::new();
            for page in 0..8_i64 {
                let params = match &cursor {
                    Some(cursor) => json!({"cursor":cursor}),
                    None => json!({}),
                };
                request(stdin, 4 + page, "model/list", params)?;
                let result = reader.response(stdout, 4 + page, deadline)?;
                let page_models = result
                    .get("data")
                    .and_then(Value::as_array)
                    .ok_or_else(|| anyhow!("Codex model list is unavailable"))?;
                if page_models.len() > 128 || data.len() + page_models.len() > 128 {
                    return Err(anyhow!("Codex model list exceeded its bound"));
                }
                data.extend(page_models.iter().cloned());
                cursor = match result.get("nextCursor") {
                    None | Some(Value::Null) => None,
                    Some(Value::String(s))
                        if !s.is_empty()
                            && s.len() <= 512
                            && s.bytes().all(|b| b.is_ascii_graphic()) =>
                    {
                        Some(s.clone())
                    }
                    _ => return Err(anyhow!("invalid Codex model list cursor")),
                };
                let Some(next) = &cursor else {
                    return Ok(CodexDiscovery {
                        rate_limits,
                        rate_limits_observed_ms,
                        models: json!({"data": data, "nextCursor": null}),
                    });
                };
                if !seen.insert(next.clone()) {
                    return Err(anyhow!("Codex model pagination repeated"));
                }
            }
            Err(anyhow!("Codex model pagination exceeded its bound"))
        },
    )
}

/// Read the exact MCP tool catalog for a selected profile and project cwd.
/// This has its own bounded metadata process and never starts a model turn.
pub struct CodexToolDiscovery {
    pub rate_limits: Value,
    pub tools: Value,
}

pub fn codex_tool_inventory(
    program: &Path,
    env: &BTreeMap<String, String>,
    cwd: &Path,
    timeout: Duration,
) -> Result<CodexToolDiscovery> {
    with_codex_account(program, env, cwd, timeout, |stdin, stdout, reader, deadline| {
        request(stdin, 3, "account/rateLimits/read", json!({}))?;
        let rate_limits = reader.response(stdout, 3, deadline)?;
        let mut data = Vec::new();
        let mut cursor: Option<String> = None;
        let mut seen = std::collections::BTreeSet::new();
        for page in 0..8_i64 {
            let params = match &cursor {
                Some(cursor) => json!({"cursor":cursor,"detail":"toolsAndAuthOnly"}),
                None => json!({"detail":"toolsAndAuthOnly"}),
            };
            request(stdin, 4 + page, "mcpServerStatus/list", params)?;
            let result = reader.response(stdout, 4 + page, deadline)?;
            let servers = result.get("data").and_then(Value::as_array)
                .ok_or_else(|| anyhow!("Codex tool inventory is unavailable"))?;
            if servers.len() > 64 || data.len() + servers.len() > 64 {
                return Err(anyhow!("Codex tool inventory exceeded its bound"));
            }
            data.extend(servers.iter().cloned());
            cursor = match result.get("nextCursor") {
                None | Some(Value::Null) => None,
                Some(Value::String(value)) if !value.is_empty() && value.len() <= 512
                    && value.bytes().all(|byte| byte.is_ascii_graphic()) => Some(value.clone()),
                _ => return Err(anyhow!("invalid Codex tool inventory cursor")),
            };
            let Some(next) = &cursor else {
                return Ok(CodexToolDiscovery { rate_limits, tools: json!({"data":data,"nextCursor":null}) });
            };
            if !seen.insert(next.clone()) {
                return Err(anyhow!("Codex tool pagination repeated"));
            }
        }
        Err(anyhow!("Codex tool inventory pagination exceeded its bound"))
    })
}

/// Read an existing thread's provider-estimated usage without making a model
/// request. The selected account identity is checked in the same session.
pub struct CodexThreadUsageRead {
    pub rate_limits: Value,
    pub rate_limits_observed_ms: i64,
    pub usage: Value,
}

pub fn codex_thread_usage(
    program: &Path,
    env: &BTreeMap<String, String>,
    cwd: &Path,
    thread_id: &str,
    timeout: Duration,
) -> Result<CodexThreadUsageRead> {
    if thread_id.is_empty()
        || thread_id.len() > 120
        || !thread_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
    {
        return Err(anyhow!("invalid Codex thread identity"));
    }
    with_codex_account(
        program,
        env,
        cwd,
        timeout,
        |stdin, stdout, reader, deadline| {
            request(stdin, 3, "account/rateLimits/read", json!({}))?;
            let rate_limits = reader.response(stdout, 3, deadline)?;
            let rate_limits_observed_ms = crate::daemon::now();
            request(
                stdin,
                4,
                "account/usage/read",
                json!({"threadId":thread_id}),
            )?;
            let usage = reader.response(stdout, 4, deadline)?;
            Ok(CodexThreadUsageRead { rate_limits, rate_limits_observed_ms, usage })
        },
    )
}
