//! Bounded metadata-only reads. This module never starts a model turn.

use anyhow::{anyhow, Context, Result};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddrV4, TcpListener, TcpStream};
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
    Ok(ClaudeAuth { fingerprint:format!("{:x}", digest.finalize()), observed_ms })
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

fn opencode_bounded_read(stream: &mut TcpStream, chunk: &mut [u8], deadline: Instant) -> Result<usize> {
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() { return Err(OpenCodeReadTimeout.into()); }
        stream.set_read_timeout(Some(remaining.max(Duration::from_millis(10))))
            .context("setting OpenCode metadata read timeout")?;
        match stream.read(chunk) {
            Ok(count) => return Ok(count),
            Err(error) if matches!(error.kind(), std::io::ErrorKind::Interrupted
                | std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock) => continue,
            Err(error) => return Err(anyhow!("OpenCode metadata stream failed: {error}")),
        }
    }
}

fn opencode_metadata_response(port: u16, password: &str, deadline: Instant) -> Result<Value> {
    // An OpenCode server can accept a connection before its first metadata
    // request is ready. Retry only read timeouts, with one overall deadline.
    for attempt in 0..3 {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() { return Err(OpenCodeReadTimeout.into()); }
        let attempt_deadline = match attempt {
            0 => Instant::now() + remaining.min(Duration::from_millis(750)),
            1 => Instant::now() + remaining.min(Duration::from_millis(1500)),
            _ => deadline,
        };
        match opencode_metadata_response_once(port, password, attempt_deadline) {
            Err(error) if error.downcast_ref::<OpenCodeReadTimeout>().is_some() && attempt < 2 => continue,
            outcome => return outcome,
        }
    }
    Err(OpenCodeReadTimeout.into())
}

fn opencode_metadata_response_once(port: u16, password: &str, deadline: Instant) -> Result<Value> {
    use base64::Engine;
    let address = SocketAddrV4::new(Ipv4Addr::LOCALHOST, port);
    let mut stream = loop {
        if Instant::now() >= deadline { return Err(OpenCodeReadTimeout.into()); }
        match TcpStream::connect_timeout(&address.into(), Duration::from_millis(100)) {
            Ok(stream) => break stream,
            Err(_) => std::thread::sleep(Duration::from_millis(20)),
        }
    };
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() { return Err(OpenCodeReadTimeout.into()); }
    stream.set_write_timeout(Some(remaining.max(Duration::from_millis(10))))
        .context("setting OpenCode metadata write timeout")?;
    let basic = base64::engine::general_purpose::STANDARD.encode(format!("overseer:{password}"));
    stream.write_all(format!("GET /config/providers HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Basic {basic}\r\nAccept: application/json\r\nAccept-Encoding: identity\r\nConnection: close\r\n\r\n").as_bytes())
        .context("writing OpenCode metadata request")?;
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 8192];
    let header_end = loop {
        let count = opencode_bounded_read(&mut stream, &mut chunk, deadline)?;
        if count == 0 { return Err(anyhow!("OpenCode metadata response ended early")); }
        bytes.extend_from_slice(&chunk[..count]);
        if bytes.len() > MAX_FRAME + 4096 { return Err(anyhow!("OpenCode metadata response exceeded its bound")); }
        if let Some(end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") { break end + 4 }
    };
    let headers = std::str::from_utf8(&bytes[..header_end])
        .map_err(|_| anyhow!("invalid OpenCode metadata headers"))?;
    if !headers.starts_with("HTTP/1.1 200 ") && !headers.starts_with("HTTP/1.0 200 ") {
        return Err(anyhow!("OpenCode metadata request was rejected"));
    }
    let mut content_length = None;
    for line in headers.lines() {
        if let Some((name, value)) = line.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                content_length = value.trim().parse::<usize>().ok();
            }
        }
    }
    let length = content_length.filter(|len| *len <= MAX_FRAME)
        .ok_or_else(|| anyhow!("OpenCode metadata response has no bounded length"))?;
    while bytes.len().saturating_sub(header_end) < length {
        let count = opencode_bounded_read(&mut stream, &mut chunk, deadline)?;
        if count == 0 { return Err(anyhow!("OpenCode metadata response ended early")); }
        bytes.extend_from_slice(&chunk[..count]);
        if bytes.len() > MAX_FRAME + 4096 { return Err(anyhow!("OpenCode metadata response exceeded its bound")); }
    }
    serde_json::from_slice(&bytes[header_end..header_end + length])
        .map_err(|_| anyhow!("invalid OpenCode metadata response"))
}

/// One authenticated loopback metadata server, no session or prompt. The
/// server is always killed/reaped; only normalized explicit local providers
/// leave this function. The random Basic secret protects this temporary server
/// and is never an account credential or persisted observation.
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
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    let port = listener.local_addr()?.port();
    drop(listener);
    let password = uuid::Uuid::new_v4().to_string();
    let mut child = Command::new(program)
        .args(["serve", "--pure", "--hostname", "127.0.0.1", "--port", &port.to_string()])
        .current_dir(cwd).env_clear().envs(env)
        .env("OPENCODE_DISABLE_AUTOUPDATE", "1")
        .env("OPENCODE_DISABLE_MODELS_FETCH", "1")
        .env("OPENCODE_SERVER_USERNAME", "overseer")
        .env("OPENCODE_SERVER_PASSWORD", &password)
        .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn()?;
    let result = (|| {
        let response = opencode_metadata_response(port, &password, Instant::now() + timeout)?;
        crate::auto_opencode::parse_local_catalog(&config, &response, observed_ms)
    })();
    let _ = child.kill();
    let _ = child.wait();
    result
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

    #[test]
    fn stalled_first_connection_retries_the_read_only_metadata_request() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (first, _) = listener.accept().unwrap();
            std::thread::sleep(Duration::from_millis(900));
            drop(first);
            let (mut second, _) = listener.accept().unwrap();
            let body = r#"{"providers":[]}"#;
            write!(second, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
            second.flush().unwrap();
            std::thread::sleep(Duration::from_millis(100));
        });
        let result = opencode_metadata_response(port, "test-secret", Instant::now() + Duration::from_secs(3)).unwrap();
        server.join().unwrap();
        assert_eq!(result, json!({"providers":[]}));
    }

    #[test]
    fn stalled_local_server_reports_a_bounded_timeout_without_a_secret() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            std::thread::sleep(Duration::from_millis(120));
            drop(stream);
        });
        let error = opencode_metadata_response(port, "secret-sentinel", Instant::now() + Duration::from_millis(60))
            .unwrap_err().to_string();
        server.join().unwrap();
        assert!(error.contains("timed out"), "{error}");
        assert!(!error.contains("secret-sentinel"));
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
            request(
                stdin,
                4,
                "account/usage/read",
                json!({"threadId":thread_id}),
            )?;
            let usage = reader.response(stdout, 4, deadline)?;
            Ok(CodexThreadUsageRead { rate_limits, usage })
        },
    )
}
