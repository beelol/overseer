//! Installing and running Ollama, only when the owner allows it (Continuity, AC-90).
//!
//! With `allowOllamaInstall` off (the default) a machine without Ollama is reported so and
//! nothing is installed or started. With it on:
//!
//! - **Install.** Homebrew when present (`brew install --cask ollama`); otherwise the official
//!   macOS archive, unpacked into Overseer's own folder and used only after its code signature
//!   verifies as Ollama's Developer ID and Gatekeeper accepts it. A download that fails either
//!   check is deleted and reported. Nothing else from a download is ever executed.
//! - **Run.** `ollama serve` is started only when nothing answers on the loopback address,
//!   bound to that address, and stopped after the idle time without local work. Overseer only
//!   ever stops the process it started itself: an Ollama the user runs is used as it is.
//!
//! The server is kept by its process id, recorded in the daemon's store, so that it outlives a
//! restart of the daemon as the agents do.

use crate::continuity::{self, Conn};
use crate::daemon::{now, Daemon};
use crate::local;
use crate::paths;
use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Ollama's Developer ID (Infra Technologies, Inc), as its signed application states it.
pub const TEAM_ID: &str = "3MU9H2V9Y9";
pub const ARCHIVE_URL: &str = "https://ollama.com/download/Ollama-darwin.zip";
const SERVER_KEY: &str = "continuity.ollama_server";

pub fn own_dir() -> PathBuf {
    paths::data_dir().join("ollama")
}

pub fn own_app() -> PathBuf {
    own_dir().join("Ollama.app")
}

/// The program inside Overseer's own copy.
pub fn own_program() -> PathBuf {
    own_app().join("Contents/Resources/ollama")
}

#[derive(Default)]
struct State {
    /// An install is going on: what it is doing now.
    installing: Option<Value>,
    last_error: Option<String>,
    /// When local work last needed the server.
    used_ms: i64,
}

static STATE: Mutex<State> = Mutex::new(State { installing: None, last_error: None, used_ms: 0 });

fn state() -> std::sync::MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(|e| e.into_inner())
}

fn emit(d: &Daemon, kind: &str, payload: Value) {
    let _ = d.emit(None, None, kind, "daemon", "exact", payload);
}

/// Homebrew, when it is there. `OVERSEER_TEST_BREW` names another program, or none when empty.
pub fn brew() -> Option<PathBuf> {
    if let Ok(v) = std::env::var("OVERSEER_TEST_BREW") {
        return Some(PathBuf::from(v)).filter(|p| p.is_file());
    }
    ["/opt/homebrew/bin/brew", "/usr/local/bin/brew"].iter().map(PathBuf::from).find(|p| p.is_file())
}

pub fn method() -> &'static str {
    if !cfg!(target_os = "macos") {
        "the distribution's package (not built yet on this platform, AC-41)"
    } else if brew().is_some() {
        "homebrew"
    } else {
        "archive"
    }
}

// ------------------------------------------------------------------ the archive

/// Fetches the archive to `to`, reporting the bytes as they arrive. Only over HTTPS.
/// `OVERSEER_TEST_OLLAMA_ARCHIVE` names a local file taken instead (tests).
fn download(d: &Daemon, to: &Path) -> Result<u64> {
    if let Some(fixture) = std::env::var_os("OVERSEER_TEST_OLLAMA_ARCHIVE") {
        let bytes = std::fs::copy(&fixture, to).map_err(|e| anyhow!("the archive {} could not be read: {e}", Path::new(&fixture).display()))?;
        emit(d, "ollama_install", json!({"step": "downloading", "bytes": bytes, "total": bytes, "source": "fixture"}));
        return Ok(bytes);
    }
    let url = ARCHIVE_URL;
    if !url.starts_with("https://") {
        bail!("Ollama is only downloaded over HTTPS");
    }
    let agent = ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(15)).timeout_read(Duration::from_secs(60)).redirects(8).build();
    let response = agent.get(url).call().map_err(|e| anyhow!("Ollama could not be downloaded from {url}: {e}"))?;
    let landed = response.get_url().to_string();
    if !landed.starts_with("https://") {
        bail!("the download of Ollama was sent to {landed}, which is not HTTPS");
    }
    let total: Option<u64> = response.header("content-length").and_then(|v| v.parse().ok());
    let mut reader = response.into_reader();
    let mut file = std::fs::File::create(to)?;
    let (mut bytes, mut told) = (0u64, Instant::now());
    let mut buffer = vec![0u8; 1 << 16];
    emit(d, "ollama_install", json!({"step": "downloading", "bytes": 0, "total": total, "source": url}));
    loop {
        let n = reader.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        file.write_all(&buffer[..n])?;
        bytes += n as u64;
        if told.elapsed() > Duration::from_secs(2) {
            told = Instant::now();
            state().installing = Some(json!({"step": "downloading", "bytes": bytes, "total": total}));
            emit(d, "ollama_install", json!({"step": "downloading", "bytes": bytes, "total": total}));
        }
    }
    file.flush()?;
    if total.is_some_and(|t| t != bytes) {
        bail!("the download of Ollama ended early ({bytes} of {} bytes)", total.unwrap_or(0));
    }
    Ok(bytes)
}

fn run(program: &str, args: &[&str]) -> Result<(bool, String)> {
    let out = std::process::Command::new(program).args(args).stdin(std::process::Stdio::null()).output().map_err(|e| anyhow!("{program} could not be run: {e}"))?;
    Ok((out.status.success(), format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)).trim().to_string()))
}

/// The code signature of `app` must be intact, made with Ollama's Developer ID, and accepted by
/// Gatekeeper. Returns what was found; an error says which check failed.
pub fn verify(app: &Path) -> Result<Value> {
    if !cfg!(target_os = "macos") {
        bail!("the archive is for macOS");
    }
    let path = app.to_string_lossy().to_string();
    let (intact, said) = run("/usr/bin/codesign", &["--verify", "--deep", "--strict", "--verbose=2", &path])?;
    if !intact {
        bail!("its code signature does not verify ({})", last_line(&said));
    }
    // Apple's own form of "signed with a Developer ID of this team".
    let requirement = format!("=anchor apple generic and certificate leaf[subject.OU] = \"{TEAM_ID}\" and certificate 1[field.1.2.840.113635.100.6.2.6] exists and certificate leaf[field.1.2.840.113635.100.6.1.13] exists");
    let (ours, said) = run("/usr/bin/codesign", &["--verify", "--deep", "--strict", "-R", &requirement, &path])?;
    if !ours {
        bail!("it is not signed with Ollama's Developer ID, team {TEAM_ID} ({})", last_line(&said));
    }
    let (_, described) = run("/usr/bin/codesign", &["-dv", "--verbose=2", &path])?;
    let field = |name: &str| described.lines().find_map(|l| l.strip_prefix(name)).map(str::to_string);
    let (accepted, assessed) = run("/usr/sbin/spctl", &["--assess", "--type", "execute", "-vv", &path])?;
    if !accepted {
        bail!("Gatekeeper does not accept it ({})", last_line(&assessed));
    }
    Ok(json!({"authority": field("Authority="), "team": field("TeamIdentifier="), "identifier": field("Identifier="), "gatekeeper": assessed.lines().find_map(|l| l.strip_prefix("source=")).unwrap_or("accepted")}))
}

fn last_line(text: &str) -> String {
    text.lines().last().unwrap_or("no reason given").chars().take(200).collect()
}

/// Downloads, unpacks, verifies and only then keeps the application, in Overseer's own folder.
fn install_archive(d: &Daemon) -> Result<Value> {
    let dir = own_dir();
    paths::ensure_private_dir(&dir)?;
    let archive = dir.join("Ollama-darwin.zip.part");
    let staging = dir.join(format!("staging-{}", now()));
    let clean = |archive: &Path, staging: &Path| {
        let _ = std::fs::remove_file(archive);
        let _ = std::fs::remove_dir_all(staging);
    };
    clean(&archive, &staging);
    let result = (|| -> Result<Value> {
        let bytes = download(d, &archive)?;
        state().installing = Some(json!({"step": "verifying"}));
        emit(d, "ollama_install", json!({"step": "verifying", "bytes": bytes}));
        std::fs::create_dir_all(&staging)?;
        let (unpacked, said) = run("/usr/bin/ditto", &["-x", "-k", &archive.to_string_lossy(), &staging.to_string_lossy()])?;
        let app = staging.join("Ollama.app");
        if !unpacked || !app.is_dir() {
            bail!("the download is not an archive of Ollama.app ({})", last_line(&said));
        }
        let signature = verify(&app).map_err(|e| anyhow!("the downloaded Ollama was not opened: {e}"))?;
        if !app.join("Contents/Resources/ollama").is_file() {
            bail!("the downloaded Ollama has no `ollama` program inside");
        }
        let _ = std::fs::remove_dir_all(own_app());
        std::fs::rename(&app, own_app())?;
        Ok(json!({"method": "archive", "bytes": bytes, "signature": signature, "program": own_program()}))
    })();
    // Whatever happened, the download and the staging folder do not stay.
    clean(&archive, &staging);
    result.map_err(|e| anyhow!("{e}; the download was deleted"))
}

fn install_homebrew(d: &Daemon, brew: &Path) -> Result<Value> {
    emit(d, "ollama_install", json!({"step": "homebrew", "command": "brew install --cask ollama"}));
    state().installing = Some(json!({"step": "homebrew"}));
    let out = std::process::Command::new(brew).args(["install", "--cask", "ollama"]).env("HOMEBREW_NO_AUTO_UPDATE", "1").env("HOMEBREW_NO_ENV_HINTS", "1").stdin(std::process::Stdio::null()).output()?;
    if !out.status.success() {
        bail!("Homebrew did not install Ollama: {}", last_line(&String::from_utf8_lossy(&out.stderr)));
    }
    let program = local::ollama_program().ok_or_else(|| anyhow!("Homebrew reported success, but Ollama is not where it is expected"))?;
    Ok(json!({"method": "homebrew", "program": program}))
}

/// Installs Ollama now, if that is allowed and needed. Blocking.
pub fn install(d: &Daemon) -> Result<Value> {
    let settings = continuity::settings();
    if let Some(program) = local::ollama_program() {
        return Ok(json!({"installed": true, "already": true, "program": program}));
    }
    if !settings.allow_ollama_install {
        bail!("Ollama is not installed, and installing it is off (overseer.continuity.allowOllamaInstall)");
    }
    if continuity::status().is_some_and(|s| s.state == Conn::Offline) {
        bail!("Ollama is not installed, and installing it needs a connection");
    }
    if !cfg!(target_os = "macos") {
        bail!("Ollama is not installed; installing it on this platform is not built yet (AC-41)");
    }
    {
        let mut s = state();
        if s.installing.is_some() {
            bail!("Ollama is already being installed");
        }
        s.installing = Some(json!({"step": "starting"}));
        s.last_error = None;
    }
    let method = method();
    emit(d, "ollama_install", json!({"step": "starting", "method": method}));
    let result = match brew() {
        Some(b) => install_homebrew(d, &b),
        None => install_archive(d),
    };
    let mut s = state();
    s.installing = None;
    match result {
        Ok(v) => {
            drop(s);
            emit(d, "ollama_install", json!({"step": "installed", "detail": v}));
            crate::log(&format!("continuity: Ollama installed ({method})"));
            Ok(json!({"installed": true, "already": false, "detail": v}))
        }
        Err(e) => {
            s.last_error = Some(e.to_string());
            drop(s);
            emit(d, "ollama_install", json!({"step": "failed", "method": method, "reason": e.to_string()}));
            crate::log(&format!("continuity: Ollama was not installed: {e}"));
            Err(e)
        }
    }
}

// ------------------------------------------------------------------ the server

/// The server Overseer started, if it is still that process.
pub fn own_server(d: &Daemon) -> Option<Value> {
    let v: Value = serde_json::from_str(&continuity::meta_get(d, SERVER_KEY)?).ok()?;
    let pid = v["pid"].as_i64()?;
    let program = v["program"].as_str()?;
    // The process id may have been given to something else since: the command must be ours.
    let (alive, command) = run("/bin/ps", &["-p", &pid.to_string(), "-o", "command="]).ok()?;
    (alive && command.contains(&format!("{program} serve"))).then_some(v)
}

fn host() -> Result<String> {
    Ok(local::ollama_url()?.trim_start_matches("http://").to_string())
}

/// Starts `ollama serve` on the loopback address, unless something answers there already.
pub fn start(d: &Daemon) -> Result<Value> {
    if local::get("/api/version", 3).is_ok() {
        return Ok(json!({"running": true, "started": false, "ours": own_server(d).is_some()}));
    }
    if !continuity::settings().allow_ollama_install {
        bail!("Ollama is not running, and starting it is off (overseer.continuity.allowOllamaInstall)");
    }
    let program = local::ollama_program().ok_or_else(|| anyhow!("Ollama is not installed"))?;
    let host = host()?;
    paths::ensure_private_dir(&own_dir())?;
    let log = std::fs::OpenOptions::new().create(true).append(true).open(own_dir().join("serve.log"))?;
    let mut command = std::process::Command::new(&program);
    command.arg("serve").env("OLLAMA_HOST", &host).stdin(std::process::Stdio::null()).stdout(log.try_clone()?).stderr(log);
    {
        // Its own process group: a signal meant for the daemon is not one for the server.
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command.spawn().map_err(|e| anyhow!("{} could not be started: {e}", program.display()))?;
    let pid = child.id();
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    let record = json!({"pid": pid, "program": program, "host": host, "started_ms": now()});
    continuity::meta_set(d, SERVER_KEY, &record.to_string())?;
    state().used_ms = now();
    let began = Instant::now();
    // A first start answers late: Ollama looks for the machine's GPUs before it serves (19 s, live).
    while began.elapsed() < Duration::from_secs(90) {
        if local::get("/api/version", 1).is_ok() {
            emit(d, "ollama_server", json!({"action": "started", "pid": pid, "program": program, "host": host, "after_ms": began.elapsed().as_millis() as u64}));
            crate::log(&format!("continuity: started ollama serve on {host} (pid {pid})"));
            return Ok(json!({"running": true, "started": true, "ours": true, "pid": pid, "host": host}));
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    let _ = stop(d, "it did not answer");
    bail!("Ollama was started but did not answer on {host} within 90 s (see {})", own_dir().join("serve.log").display())
}

/// Stops the server Overseer started. An Ollama that Overseer did not start is never touched.
pub fn stop(d: &Daemon, why: &str) -> Result<bool> {
    let Some(server) = own_server(d) else {
        let _ = d.store.lock().unwrap().conn.execute("DELETE FROM meta WHERE key=?1", [SERVER_KEY]);
        return Ok(false);
    };
    let pid = server["pid"].as_i64().unwrap_or(0) as libc::pid_t;
    unsafe { libc::kill(pid, libc::SIGTERM) };
    let began = Instant::now();
    while began.elapsed() < Duration::from_secs(5) && unsafe { libc::kill(pid, 0) } == 0 {
        std::thread::sleep(Duration::from_millis(100));
    }
    if unsafe { libc::kill(pid, 0) } == 0 {
        unsafe { libc::kill(pid, libc::SIGKILL) };
    }
    d.store.lock().unwrap().conn.execute("DELETE FROM meta WHERE key=?1", [SERVER_KEY])?;
    emit(d, "ollama_server", json!({"action": "stopped", "pid": pid, "why": why}));
    crate::log(&format!("continuity: stopped ollama serve (pid {pid}): {why}"));
    Ok(true)
}

fn idle_ms() -> i64 {
    std::env::var("OVERSEER_TEST_OLLAMA_IDLE_MS").ok().and_then(|v| v.parse().ok()).unwrap_or(continuity::settings().ollama_idle_minutes as i64 * 60_000)
}

/// Called at every check of the connection: the server Overseer started is stopped when no
/// local work has needed it for the idle time.
pub fn maintain(d: &Daemon) {
    let Some(server) = own_server(d) else { return };
    let working = d.store.lock().unwrap().runs().map(|runs| runs.iter().any(|r| continuity::is_local(r) && crate::daemon::ACTIVE.contains(&r.status.as_str()))).unwrap_or(false);
    let mut s = state();
    if s.used_ms == 0 {
        s.used_ms = server["started_ms"].as_i64().unwrap_or_else(now);
    }
    if working || s.installing.is_some() || crate::downloads::busy() {
        s.used_ms = now();
        return;
    }
    let idle = now() - s.used_ms;
    drop(s);
    if idle >= idle_ms() {
        let _ = stop(d, &format!("no local work for {} s", idle / 1000));
    }
}

/// Local work is about to need Ollama. Running: nothing to do. Installed: it is started. Not
/// installed: the install begins in the background, and the caller waits and looks again.
/// With the setting off nothing is installed or started, and the error says what is missing.
pub fn ensure_running(d: &Arc<Daemon>) -> Result<()> {
    let status = local::ollama_status();
    if status.running {
        state().used_ms = now();
        return Ok(());
    }
    if !continuity::settings().allow_ollama_install {
        bail!("{}", status.detail);
    }
    if status.installed.is_some() {
        return start(d).map(|_| ());
    }
    if let Some(step) = state().installing.clone() {
        bail!("Ollama is being installed ({}); the work goes on when it is ready", step["step"].as_str().unwrap_or("working"));
    }
    if continuity::status().is_some_and(|s| s.state == Conn::Offline) {
        bail!("Ollama is not installed, and installing it needs a connection");
    }
    let daemon = d.clone();
    std::thread::spawn(move || {
        if install(&daemon).is_ok() {
            let _ = start(&daemon);
        }
    });
    bail!("Ollama is being installed; the work goes on when it is ready")
}

pub fn status(d: &Daemon) -> Value {
    let ollama = local::ollama_status();
    let s = state();
    json!({
        "ollama": ollama,
        "allowed": continuity::settings().allow_ollama_install,
        "method": method(),
        "own_copy": own_program().is_file().then(own_program),
        "server": own_server(d),
        "installing": s.installing,
        "last_error": s.last_error,
        "idle_stop_minutes": continuity::settings().ollama_idle_minutes,
    })
}

pub fn handles(method: &str) -> bool {
    matches!(method, "ollama.status" | "ollama.install" | "ollama.start" | "ollama.stop")
}

pub fn dispatch(d: &Arc<Daemon>, method: &str, _p: &Value) -> Result<Value> {
    Ok(match method {
        "ollama.status" => status(d),
        "ollama.install" => install(d)?,
        "ollama.start" => start(d)?,
        "ollama.stop" => json!({"stopped": stop(d, "asked by the user")?, "note": "only a server Overseer started is ever stopped"}),
        other => bail!("unknown method {other}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "macos")]
    #[test]
    fn an_unsigned_application_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let app = dir.path().join("Ollama.app");
        std::fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
        std::fs::write(app.join("Contents/Info.plist"), "<?xml version=\"1.0\"?><plist version=\"1.0\"><dict><key>CFBundleIdentifier</key><string>com.electron.ollama</string><key>CFBundleExecutable</key><string>Ollama</string></dict></plist>").unwrap();
        std::fs::write(app.join("Contents/MacOS/Ollama"), "#!/bin/sh\necho not ollama\n").unwrap();
        let refused = verify(&app).unwrap_err().to_string();
        assert!(refused.starts_with("its code signature does not verify"), "{refused}");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_program_signed_by_someone_else_is_refused() {
        // Signed by Apple, intact, and not Ollama's.
        let refused = verify(Path::new("/System/Applications/Calculator.app")).unwrap_err().to_string();
        assert!(refused.starts_with("it is not signed with Ollama's Developer ID"), "{refused}");
    }

    #[test]
    fn the_own_copy_lives_in_overseers_folder() {
        assert!(own_program().starts_with(paths::data_dir()));
        assert!(own_program().ends_with("ollama/Ollama.app/Contents/Resources/ollama"));
    }
}
