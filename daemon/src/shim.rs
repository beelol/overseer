//! Per-run supervisor process. The daemon starts one shim per harness process.
//! The shim owns the harness pipes, records output to append-only segment files,
//! and accepts stdin/signal commands on a private control socket. Because the shim
//! runs in its own session, harness work survives a daemon crash or restart and the
//! daemon can reattach by reading the run directory.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::sync::mpsc::{self, Sender};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const SEGMENT_BYTES: u64 = 8 * 1024 * 1024;
pub const MAX_LINE_BYTES: usize = 256 * 1024;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct LaunchFile {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: String,
    pub env: BTreeMap<String, String>,
    /// Text written to stdin immediately after spawn (for stream-json harnesses).
    #[serde(default)]
    pub initial_stdin: Option<String>,
    /// Close stdin after the initial write (one-shot CLIs).
    #[serde(default)]
    pub close_stdin: bool,
    pub control_socket: String,
    /// Absolute deadline for an Auto-managed child. The supervisor enforces it
    /// even when its owning daemon is unavailable.
    #[serde(default)]
    pub auto_execution_deadline_ms: Option<u64>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ShimInfo {
    pub shim_pid: u32,
    pub child_pid: u32,
    pub started_ms: u64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ExitInfo {
    pub code: Option<i32>,
    pub signal: Option<i32>,
    pub spawn_error: Option<String>,
    pub ended_ms: u64,
}

pub fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

pub fn segment_path(dir: &Path, n: u64) -> PathBuf {
    dir.join(format!("output-{n:06}.log"))
}

struct SegmentWriter {
    dir: PathBuf,
    n: u64,
    file: std::fs::File,
    len: u64,
}

impl SegmentWriter {
    fn open(dir: &Path) -> std::io::Result<Self> {
        let mut n = 0;
        while segment_path(dir, n + 1).exists() {
            n += 1;
        }
        let file = std::fs::OpenOptions::new().create(true).append(true).open(segment_path(dir, n))?;
        let len = file.metadata()?.len();
        Ok(Self { dir: dir.to_path_buf(), n, file, len })
    }

    fn record(&mut self, stream: &str, data: &[u8]) {
        let mut truncated = false;
        let slice = if data.len() > MAX_LINE_BYTES {
            truncated = true;
            &data[..MAX_LINE_BYTES]
        } else {
            data
        };
        let text = String::from_utf8_lossy(slice);
        let text = text.trim_end_matches(['\n', '\r']);
        let mut rec = serde_json::json!({"t": now_ms(), "s": stream, "d": text});
        if truncated {
            rec["trunc"] = serde_json::Value::Bool(true);
        }
        let mut line = rec.to_string();
        line.push('\n');
        if self.len + line.len() as u64 > SEGMENT_BYTES && self.len > 0 {
            if let Ok(file) = std::fs::OpenOptions::new().create(true).append(true).open(segment_path(&self.dir, self.n + 1)) {
                self.n += 1;
                self.file = file;
                self.len = 0;
            }
        }
        if self.file.write_all(line.as_bytes()).is_ok() {
            self.len += line.len() as u64;
            let _ = self.file.flush();
        }
    }
}

fn write_json(path: &Path, value: &impl Serialize) {
    let tmp = path.with_extension("tmp");
    if let Ok(text) = serde_json::to_vec(value) {
        if std::fs::write(&tmp, text).is_ok() {
            let _ = std::fs::rename(&tmp, path);
        }
    }
}

type MetadataReplies = Arc<Mutex<HashMap<String, Sender<Result<serde_json::Value, String>>>>>;

fn private_metadata_response(line: &[u8], pending: &MetadataReplies) -> bool {
    const PREFIX: &str = "ovs-auto-meta-";
    // A timed-out response is still private. Never let a late metadata frame
    // enter the append-only transcript just because its waiter has gone away.
    if !line.windows(PREFIX.len()).any(|window| window == PREFIX.as_bytes()) {
        return false;
    }
    let parsed = if line.len() <= 1024 * 1024 {
        serde_json::from_slice::<serde_json::Value>(line).ok()
    } else { None };
    if let Some(id) = parsed.as_ref().and_then(|value| value["id"].as_str()).map(str::to_owned) {
        if id.starts_with(PREFIX) {
            if let Some(sender) = pending.lock().unwrap().remove(&id) {
                let answer = parsed.ok_or_else(|| "metadata response exceeded its bound".to_string());
                let _ = sender.send(answer);
            }
            return true;
        }
    }
    // Malformed private output is discarded as well; the waiting request
    // expires at its bounded deadline and cannot leak account data to logs.
    true
}

fn pump<R: Read + Send + 'static>(reader: R, stream: &'static str,
    out: Arc<Mutex<SegmentWriter>>, pending: Option<MetadataReplies>) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut reader = BufReader::new(reader);
        let mut buf = Vec::new();
        loop {
            buf.clear();
            match reader.read_until(b'\n', &mut buf) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    if pending.as_ref().is_some_and(|pending|
                        private_metadata_response(&buf, pending)) { continue; }
                    out.lock().unwrap().record(stream, &buf);
                }
            }
        }
    })
}

fn watch_auto_deadline(dir: PathBuf, child_pid: u32, deadline_ms: u64,
    out: Arc<Mutex<SegmentWriter>>) -> std::sync::mpsc::Sender<()> {
    use std::sync::mpsc::{channel, RecvTimeoutError};
    let (done, stopped) = channel();
    std::thread::spawn(move || {
        let wait_ms = deadline_ms.saturating_sub(now_ms());
        if !matches!(stopped.recv_timeout(Duration::from_millis(wait_ms)), Err(RecvTimeoutError::Timeout)) {
            return;
        }
        // The marker is written before the signal so a daemon that reattaches
        // can distinguish this stop from a user interrupt or harness failure.
        let _ = std::fs::write(dir.join("auto-budget.requested"), deadline_ms.to_string());
        out.lock().unwrap().record("x", b"auto execution budget elapsed");
        for (signal, next_wait) in [(libc::SIGINT, Some(10_000)),
            (libc::SIGTERM, Some(5_000)), (libc::SIGKILL, None)] {
            let _ = unsafe { libc::kill(-(child_pid as i32), signal) };
            if let Some(wait_ms) = next_wait {
                if !matches!(stopped.recv_timeout(Duration::from_millis(wait_ms)), Err(RecvTimeoutError::Timeout)) {
                    return;
                }
            }
        }
    });
    done
}

/// Entry point for `overseerd shim <run-dir>`.
pub fn run(dir: PathBuf) -> anyhow::Result<()> {
    let launch: LaunchFile = serde_json::from_slice(&std::fs::read(dir.join("launch.json"))?)?;
    unsafe {
        libc::signal(libc::SIGHUP, libc::SIG_IGN);
        libc::signal(libc::SIGINT, libc::SIG_IGN);
        libc::signal(libc::SIGPIPE, libc::SIG_IGN);
    }
    let out = Arc::new(Mutex::new(SegmentWriter::open(&dir)?));
    let mut cmd = Command::new(&launch.program);
    cmd.args(&launch.args)
        .current_dir(&launch.cwd)
        .env_clear()
        .envs(&launch.env)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    unsafe {
        cmd.pre_exec(|| {
            libc::signal(libc::SIGINT, libc::SIG_DFL);
            libc::signal(libc::SIGHUP, libc::SIG_DFL);
            libc::signal(libc::SIGPIPE, libc::SIG_DFL);
            Ok(())
        });
    }
    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(error) => {
            out.lock().unwrap().record("x", format!("spawn failed: {error}").as_bytes());
            write_json(&dir.join("exit.json"), &ExitInfo { code: None, signal: None, spawn_error: Some(error.to_string()), ended_ms: now_ms() });
            return Ok(());
        }
    };
    let child_pid = child.id();
    write_json(&dir.join("shim.json"), &ShimInfo { shim_pid: std::process::id(), child_pid, started_ms: now_ms() });
    let deadline_watch = launch.auto_execution_deadline_ms.map(|deadline|
        watch_auto_deadline(dir.clone(), child_pid, deadline, out.clone()));
    let stdin = Arc::new(Mutex::new(child.stdin.take()));
    let metadata_replies: MetadataReplies = Arc::new(Mutex::new(HashMap::new()));
    let t_out = pump(child.stdout.take().unwrap(), "o", out.clone(), Some(metadata_replies.clone()));
    let t_err = pump(child.stderr.take().unwrap(), "e", out.clone(), None);
    if let Some(text) = &launch.initial_stdin {
        if let Some(pipe) = stdin.lock().unwrap().as_mut() {
            let _ = pipe.write_all(text.as_bytes());
            let _ = pipe.flush();
        }
        out.lock().unwrap().record("i", text.as_bytes());
    }
    if launch.close_stdin {
        stdin.lock().unwrap().take();
    }

    let sock_path = PathBuf::from(&launch.control_socket);
    let _ = std::fs::remove_file(&sock_path);
    let listener = UnixListener::bind(&sock_path)?;
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&sock_path, std::fs::Permissions::from_mode(0o600));
    }
    let uid = unsafe { libc::getuid() };
    {
        let stdin = stdin.clone();
        let out = out.clone();
        let metadata_replies = metadata_replies.clone();
        std::thread::spawn(move || {
            for conn in listener.incoming().flatten() {
                if peer_uid(&conn) != Some(uid) {
                    continue;
                }
                let stdin = stdin.clone();
                let out = out.clone();
                let metadata_replies = metadata_replies.clone();
                std::thread::spawn(move || handle_control(conn, stdin, out, metadata_replies, child_pid));
            }
        });
    }

    let status = child.wait();
    if let Some(done) = deadline_watch { let _ = done.send(()); }
    // Grandchildren may keep the pipes open; do not block exit on them forever.
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    while (!t_out.is_finished() || !t_err.is_finished()) && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    let (code, signal) = match status {
        Ok(status) => {
            use std::os::unix::process::ExitStatusExt;
            (status.code(), status.signal())
        }
        Err(_) => (None, None),
    };
    write_json(&dir.join("exit.json"), &ExitInfo { code, signal, spawn_error: None, ended_ms: now_ms() });
    let _ = std::fs::remove_file(&sock_path);
    Ok(())
}

/// Effective UID of the connected peer (portable boundary: getpeereid on macOS/BSD,
/// SO_PEERCRED on Linux).
pub fn peer_uid(conn: &UnixStream) -> Option<u32> {
    peer_uid_fd(std::os::unix::io::AsRawFd::as_raw_fd(conn))
}

#[cfg(not(target_os = "linux"))]
pub fn peer_uid_fd(fd: i32) -> Option<u32> {
    let mut uid: libc::uid_t = 0;
    let mut gid: libc::gid_t = 0;
    let rc = unsafe { libc::getpeereid(fd, &mut uid, &mut gid) };
    if rc == 0 { Some(uid) } else { None }
}

#[cfg(target_os = "linux")]
pub fn peer_uid_fd(fd: i32) -> Option<u32> {
    let mut cred: libc::ucred = unsafe { std::mem::zeroed() };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    let rc = unsafe { libc::getsockopt(fd, libc::SOL_SOCKET, libc::SO_PEERCRED, &mut cred as *mut _ as *mut libc::c_void, &mut len) };
    if rc == 0 { Some(cred.uid) } else { None }
}

fn handle_control(conn: UnixStream, stdin: Arc<Mutex<Option<std::process::ChildStdin>>>,
    out: Arc<Mutex<SegmentWriter>>, pending: MetadataReplies, child_pid: u32) {
    let mut writer = match conn.try_clone() {
        Ok(w) => w,
        Err(_) => return,
    };
    let reader = BufReader::new(conn.take(1024 * 1024));
    for line in reader.lines() {
        let Ok(line) = line else { break };
        let reply = match serde_json::from_str::<serde_json::Value>(&line) {
            Ok(msg) => match msg["op"].as_str() {
                Some("ping") => serde_json::json!({"ok": true, "child_pid": child_pid}),
                Some("metadata_rpc") => {
                    let method = msg["method"].as_str().unwrap_or_default();
                    let params = &msg["params"];
                    let allowed = match method {
                        "account/read" => params == &serde_json::json!({"refreshToken":false}),
                        "account/rateLimits/read" => params == &serde_json::json!({}),
                        "model/list" => params == &serde_json::json!({})
                            || params["cursor"].as_str().is_some_and(|cursor|
                                !cursor.is_empty() && cursor.len() <= 512
                                    && cursor.bytes().all(|byte| byte.is_ascii_graphic())
                                    && params.as_object().is_some_and(|object| object.len() == 1)),
                        "mcpServerStatus/list" => params == &serde_json::json!({"detail":"toolsAndAuthOnly"})
                            || params["cursor"].as_str().is_some_and(|cursor|
                                !cursor.is_empty() && cursor.len() <= 512
                                    && cursor.bytes().all(|byte| byte.is_ascii_graphic())
                                    && params["detail"] == "toolsAndAuthOnly"
                                    && params.as_object().is_some_and(|object| object.len() == 2)),
                        _ => false,
                    };
                    let timeout_ms = msg["timeout_ms"].as_u64().unwrap_or(0);
                    if !allowed || !(1..=4000).contains(&timeout_ms) {
                        serde_json::json!({"ok":false,"error":"unsupported metadata request"})
                    } else {
                        let id = format!("ovs-auto-meta-{}", uuid::Uuid::new_v4().simple());
                        let (sender, receiver) = mpsc::channel();
                        let registered = {
                            let mut waiting = pending.lock().unwrap();
                            if waiting.len() >= 8 { false } else {
                                waiting.insert(id.clone(), sender);
                                true
                            }
                        };
                        if !registered {
                            serde_json::json!({"ok":false,"error":"metadata request limit reached"})
                        } else {
                            let request = serde_json::json!({"id":id,"method":method,"params":params});
                            let written = stdin.lock().unwrap().as_mut().is_some_and(|pipe|
                                pipe.write_all(format!("{request}\n").as_bytes())
                                    .and_then(|_| pipe.flush()).is_ok());
                            let response = if written {
                                receiver.recv_timeout(Duration::from_millis(timeout_ms)).ok()
                            } else { None };
                            pending.lock().unwrap().remove(&id);
                            match response {
                                Some(Ok(value)) if value["error"].is_null() && !value["result"].is_null() =>
                                    serde_json::json!({"ok":true,"result":value["result"]}),
                                Some(Ok(_)) => serde_json::json!({"ok":false,"error":"metadata method failed"}),
                                Some(Err(_)) => serde_json::json!({"ok":false,"error":"metadata response invalid"}),
                                None => serde_json::json!({"ok":false,"error":"metadata response unavailable"}),
                            }
                        }
                    }
                }
                Some("stdin") => {
                    let data = msg["data"].as_str().unwrap_or_default();
                    let mut guard = stdin.lock().unwrap();
                    match guard.as_mut() {
                        Some(pipe) => {
                            let ok = pipe.write_all(data.as_bytes()).and_then(|_| pipe.flush()).is_ok();
                            drop(guard);
                            if ok {
                                out.lock().unwrap().record("i", data.as_bytes());
                            }
                            serde_json::json!({"ok": ok})
                        }
                        None => serde_json::json!({"ok": false, "error": "stdin closed"}),
                    }
                }
                Some("close_stdin") => {
                    stdin.lock().unwrap().take();
                    serde_json::json!({"ok": true})
                }
                Some("signal") => {
                    let sig = msg["sig"].as_i64().unwrap_or(libc::SIGINT as i64) as i32;
                    if ![libc::SIGINT, libc::SIGTERM, libc::SIGKILL].contains(&sig) {
                        serde_json::json!({"ok": false, "error": "signal not allowed"})
                    } else {
                        let rc = unsafe { libc::kill(-(child_pid as i32), sig) };
                        out.lock().unwrap().record("x", format!("signal {sig} sent to process group").as_bytes());
                        serde_json::json!({"ok": rc == 0})
                    }
                }
                _ => serde_json::json!({"ok": false, "error": "unknown op"}),
            },
            Err(_) => serde_json::json!({"ok": false, "error": "malformed"}),
        };
        let mut text = reply.to_string();
        text.push('\n');
        if writer.write_all(text.as_bytes()).is_err() {
            break;
        }
    }
}

/// Client helper used by the daemon.
pub fn control(socket: &Path, msg: &serde_json::Value) -> anyhow::Result<serde_json::Value> {
    let mut conn = UnixStream::connect(socket)?;
    conn.set_read_timeout(Some(Duration::from_secs(5)))?;
    let mut text = msg.to_string();
    text.push('\n');
    conn.write_all(text.as_bytes())?;
    let mut reader = BufReader::new(conn);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    Ok(serde_json::from_str(&line)?)
}
