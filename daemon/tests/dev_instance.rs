//! AC-212: a dev daemon never opens the standard instance, and the standard daemon beside it
//! keeps its pid, socket, state, clients and data, with no trace of the dev ones.
//!
//! The "standard" daemon runs under a temporary HOME (no OVERSEER_HOME), so its data folder and
//! socket are the platform defaults for that HOME: the owner's own daemon is never involved.

mod common;

use common::{repo, BIN};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

const LEAKY: [&str; 3] = ["OVERSEER_HOME", "OVERSEER_SOCKET", "OVERSEER_INSTANCE"];

/// A daemon command with a clean environment: the given HOME, fixture-free harnesses, no probes.
fn cmd(bin: &Path, home: &Path, args: &[&str], env: &[(&str, &str)]) -> Command {
    let mut c = Command::new(bin);
    c.args(args).env("HOME", home).env("OVERSEER_CONTINUITY_PROBES", "off");
    for key in LEAKY {
        c.env_remove(key);
    }
    for key in ["OVERSEER_CODEX_PATH", "OVERSEER_CLAUDE_PATH", "OVERSEER_OPENCODE_PATH"] {
        c.env(key, "/nonexistent/harness-disabled-in-tests");
    }
    for (k, v) in env {
        c.env(k, v);
    }
    c
}

struct Proc(Option<Child>);
impl Drop for Proc {
    fn drop(&mut self) {
        if let Some(mut c) = self.0.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

fn call(socket: &Path, method: &str, params: Value) -> Result<Value, String> {
    let mut conn = UnixStream::connect(socket).map_err(|e| e.to_string())?;
    conn.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
    conn.write_all(format!("{}\n", json!({"id": 1, "method": method, "params": params})).as_bytes()).map_err(|e| e.to_string())?;
    let mut line = String::new();
    BufReader::new(conn).read_line(&mut line).map_err(|e| e.to_string())?;
    let msg: Value = serde_json::from_str(&line).map_err(|e| format!("{e}: {line}"))?;
    match msg.get("error") {
        Some(err) => Err(err["message"].as_str().unwrap_or_default().to_string()),
        None => Ok(msg["result"].clone()),
    }
}

fn socket_of(bin: &Path, home: &Path, env: &[(&str, &str)]) -> PathBuf {
    let out = cmd(bin, home, &["socket-path"], env).output().unwrap();
    assert!(out.status.success(), "socket-path: {}", String::from_utf8_lossy(&out.stderr));
    PathBuf::from(String::from_utf8(out.stdout).unwrap().trim())
}

fn serve(bin: &Path, home: &Path, env: &[(&str, &str)]) -> (Proc, PathBuf) {
    let socket = socket_of(bin, home, env);
    let child = cmd(bin, home, &["serve"], env).stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    while call(&socket, "hello", json!({})).is_err() {
        assert!(Instant::now() < deadline, "daemon did not start at {}", socket.display());
        std::thread::sleep(Duration::from_millis(50));
    }
    (Proc(Some(child)), socket)
}

/// Runs `serve` expecting the dev guard's refusal (exit 4) with `says` in its message.
fn refused(bin: &Path, home: &Path, env: &[(&str, &str)], says: &str) -> Output {
    let mut child = cmd(bin, home, &["serve"], env).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while child.try_wait().unwrap().is_none() {
        if Instant::now() > deadline {
            let _ = child.kill();
            panic!("a dev daemon with {env:?} kept running instead of refusing");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let out = child.wait_with_output().unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(4), "{env:?}: exit {:?}, stderr {err}", out.status.code());
    assert!(err.contains(says), "{env:?}: expected {says:?} in {err}");
    out
}

/// Every file and folder under `dir` (SQLite's transient journal files left out).
fn listing(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            let rel = p.strip_prefix(dir).unwrap().display().to_string();
            if rel.ends_with("-wal") || rel.ends_with("-shm") || rel.ends_with("-journal") {
                continue;
            }
            if p.is_dir() {
                stack.push(p);
            }
            out.push(rel);
        }
    }
    out.sort();
    out
}

fn snapshot(socket: &Path) -> Value {
    let hello = call(socket, "hello", json!({})).unwrap();
    let state = call(socket, "state", json!({"include_hidden": true})).unwrap();
    json!({
        "pid": hello["pid"], "socket": hello["socket"], "data_dir": hello["data_dir"], "instance": hello["instance"],
        "tasks": state["tasks"], "runs": state["runs"], "profiles": call(socket, "profile.list", json!({})).unwrap(),
        "clients": call(socket, "daemon.clients", json!({})).unwrap(),
    })
}

fn wait_exit(p: &mut Proc) {
    let child = p.0.as_mut().unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    while child.try_wait().unwrap().is_none() {
        assert!(Instant::now() < deadline, "dev daemon did not stop");
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn ac212_dev_daemons_refuse_the_standard_instance_and_leave_it_untouched() {
    let tmp = tempfile::Builder::new().prefix("ovs-dv").tempdir_in("/tmp").unwrap();
    let t = std::fs::canonicalize(tmp.path()).unwrap();
    let home = t.join("h");
    std::fs::create_dir_all(&home).unwrap();

    // The standard daemon: no OVERSEER_HOME, so the platform default under this HOME.
    let (mut standard, std_socket) = serve(Path::new(BIN), &home, &[]);
    let std_data = PathBuf::from(call(&std_socket, "hello", json!({})).unwrap()["data_dir"].as_str().unwrap());
    if cfg!(target_os = "macos") {
        assert_eq!(std_data, home.join("Library/Application Support/Overseer"));
    }
    let before = snapshot(&std_socket);
    assert!(before["instance"].is_null(), "the standard daemon reports no instance");
    let files_before = listing(&std_data);

    // A dev build: the binary copied with the marker file next to it (what scripts/dev does).
    let bin_dir = t.join("b/bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    let marked = bin_dir.join("overseerd");
    std::fs::copy(BIN, &marked).unwrap();
    std::fs::write(bin_dir.join("overseer-dev-instance"), "dev-b\n").unwrap();

    let std_socket_s = std_socket.display().to_string();
    let std_data_s = std_data.display().to_string();
    let long = t.join("a-folder-name-long-enough-that-its-socket-path-would-not-fit-in-a-unix-socket-address/home");
    let long_s = long.display().to_string();
    let fresh = t.join("fresh-home");
    let fresh_s = fresh.display().to_string();
    // Marked by the environment.
    refused(Path::new(BIN), &home, &[("OVERSEER_INSTANCE", "dev-x")], "needs its own OVERSEER_HOME");
    refused(Path::new(BIN), &home, &[("OVERSEER_INSTANCE", "dev-x"), ("OVERSEER_HOME", &std_data_s)], "refuses the standard data folder");
    refused(Path::new(BIN), &home, &[("OVERSEER_INSTANCE", "dev-x"), ("OVERSEER_HOME", &fresh_s), ("OVERSEER_SOCKET", &std_socket_s)], "refuses the standard socket");
    refused(Path::new(BIN), &home, &[("OVERSEER_INSTANCE", "dev-x"), ("OVERSEER_HOME", &long_s)], "standard socket folder");
    refused(Path::new(BIN), &home, &[("OVERSEER_INSTANCE", "Prod"), ("OVERSEER_HOME", &fresh_s)], "not a dev instance name");
    // Marked by the file: never the standard instance, even with nothing in the environment.
    refused(&marked, &home, &[], "is a dev build");
    refused(&marked, &home, &[("OVERSEER_INSTANCE", "dev-z"), ("OVERSEER_HOME", &fresh_s)], "does not match");
    let out = cmd(&marked, &home, &["socket-path"], &[]).output().unwrap();
    assert_eq!(out.status.code(), Some(4), "a dev build does not even name the standard socket");
    assert!(!fresh.exists() && !long.exists(), "a refused dev daemon creates nothing");

    // Proper dev daemons beside it: A marked by the environment, B a marked copy.
    let a_home = t.join("a/home");
    let a_sock = t.join("a/overseerd.sock");
    let b_home = t.join("b/home");
    let b_sock = t.join("b/overseerd.sock");
    let (a_home_s, a_sock_s, b_home_s, b_sock_s) = (a_home.display().to_string(), a_sock.display().to_string(), b_home.display().to_string(), b_sock.display().to_string());
    let (mut a, a_socket) = serve(Path::new(BIN), &home, &[("OVERSEER_INSTANCE", "dev-a"), ("OVERSEER_HOME", &a_home_s), ("OVERSEER_SOCKET", &a_sock_s)]);
    let (mut b, b_socket) = serve(&marked, &home, &[("OVERSEER_INSTANCE", "dev-b"), ("OVERSEER_HOME", &b_home_s), ("OVERSEER_SOCKET", &b_sock_s)]);
    assert_eq!(a_socket, a_sock);
    assert_eq!(b_socket, b_sock);
    let ha = call(&a_socket, "hello", json!({})).unwrap();
    let hb = call(&b_socket, "hello", json!({})).unwrap();
    assert_eq!(ha["instance"], "dev-a");
    assert_eq!(hb["instance"], "dev-b");
    assert_ne!(ha["pid"], before["pid"]);

    // Busy: an agent in each dev instance, then both stopped with their agents.
    for (socket, name) in [(&a_socket, "a"), (&b_socket, "b")] {
        let r = repo(&t.join(format!("repo-{name}")));
        call(socket, "task.create", json!({"repo": r, "harness": "generic", "program": "/bin/sh", "args": ["-c", "echo busy; sleep 30"], "prompt": "", "title": format!("dev agent {name}")})).unwrap();
        let tasks = call(socket, "state", json!({})).unwrap()["tasks"].as_array().unwrap().len();
        assert_eq!(tasks, 1);
    }
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(snapshot(&std_socket), before, "the standard daemon while dev instances run");
    let _ = call(&a_socket, "daemon.stop_all", json!({}));
    let _ = call(&b_socket, "daemon.stop_all", json!({}));
    wait_exit(&mut a);
    wait_exit(&mut b);

    // The standard daemon: same pid, socket, state, clients and data, and no trace of dev.
    let after = snapshot(&std_socket);
    assert_eq!(after, before);
    assert_eq!(listing(&std_data), files_before);
    let log = std::fs::read_to_string(std_data.join("overseerd.log")).unwrap_or_default();
    for trace in [t.join("a").display().to_string(), t.join("b").display().to_string(), "dev-a".into(), "dev-b".into(), "dev-x".into()] {
        assert!(!log.contains(&trace), "the standard daemon's log mentions {trace}");
    }
    let _ = call(&std_socket, "daemon.shutdown", json!({}));
    wait_exit(&mut standard);
    let _ = Command::new("pkill").arg("-9").arg("-f").arg(&t).status();
}

#[test]
fn ac212_hello_reports_no_instance_for_a_test_daemon() {
    // Test daemons (OVERSEER_HOME, no marker) stay standard: every other suite relies on it.
    let d = common::Daemon::start(&[]);
    assert!(d.call("hello", json!({}))["instance"].is_null());
}
