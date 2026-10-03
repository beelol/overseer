//! First internal runner baselines. Not installation, native-hook or AC271 proof.
#![cfg(target_os = "macos")]
#[path = "../src/mods/runner.rs"]
mod runner;

use runner::{Outcome, Program, Request};
use sha2::{Digest, Sha256};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Output, Stdio};
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

// Each assertion runs in an owned subprocess, so a runner pipe/lock/wait
// deadlock cannot defeat the elapsed assertions or strand the whole test file.
const WORKER: &str = "OVERSEER_MODS_RUNNER_TEST_WORKER";
const WORKER_ROOT: &str = "OVERSEER_MODS_RUNNER_TEST_ROOT";
struct OwnedChild {
    child: Child,
    witness_root: Option<std::path::PathBuf>,
}
impl Drop for OwnedChild {
    fn drop(&mut self) {
        // Kill the group even after its leader exits: leaked descendants may
        // retain captured pipes. Only groups created by this fixture are used.
        let pid = self.child.id() as i32;
        unsafe {
            libc::kill(-pid, libc::SIGKILL);
        }
        let _ = self.child.kill();
        if let Some(root) = &self.witness_root {
            // Production runner children have a distinct owned process group.
            // The fixture records startup in a root only this worker can use.
            if let Ok(entries) = std::fs::read_dir(root) {
                for entry in entries.flatten() {
                    let witness = StartedWitness(entry.path().join("scratch/started.pid"));
                    drop(witness);
                }
            }
        }
        let _ = self.child.wait();
    }
}

fn bounded_output(
    mut command: Command,
    deadline: Duration,
    root: Option<&std::path::Path>,
) -> Output {
    use std::io::Read;
    command
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = OwnedChild {
        child: command.spawn().expect("fixture process spawn"),
        witness_root: root.map(std::path::Path::to_path_buf),
    };
    let end = Instant::now() + deadline;
    // Drain both pipes concurrently but keep only bounded diagnostic bytes.
    let reader = |mut stream: Box<dyn Read + Send>| {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let mut overflow = false;
            let mut buf = [0u8; 4096];
            loop {
                match stream.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        let keep = n.min((64 * 1024usize).saturating_sub(bytes.len()));
                        bytes.extend_from_slice(&buf[..keep]);
                        overflow |= keep != n;
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(e) => {
                        let _ = tx.send(Err(e));
                        return;
                    }
                }
            }
            let _ = tx.send(Ok((bytes, overflow)));
        });
        rx
    };
    let out = reader(Box::new(child.child.stdout.take().unwrap()));
    let err = reader(Box::new(child.child.stderr.take().unwrap()));
    let status = loop {
        if let Some(status) = child.child.try_wait().expect("fixture process wait") {
            break status;
        }
        assert!(
            Instant::now() < end,
            "fixture watchdog: child exceeded {deadline:?}"
        );
        std::thread::sleep(Duration::from_millis(5));
    };
    let read = |rx: std::sync::mpsc::Receiver<std::io::Result<(Vec<u8>, bool)>>| {
        let (bytes, overflow) = rx
            .recv_timeout(end.saturating_duration_since(Instant::now()))
            .expect("fixture watchdog: child pipe retained past deadline")
            .expect("fixture pipe read");
        assert!(!overflow, "fixture diagnostics exceeded64KiB");
        bytes
    };
    Output {
        status,
        stdout: read(out),
        stderr: read(err),
    }
}

fn isolated_worker(name: &str) -> bool {
    if std::env::var(WORKER).as_deref() == Ok(name) {
        return false;
    }
    let root = tempfile::Builder::new()
        .prefix("mods-runner-worker-")
        .tempdir_in("/private/tmp")
        .unwrap();
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", name, "--nocapture", "--test-threads=1"])
        .env(WORKER, name)
        .env(WORKER_ROOT, root.path());
    let result = bounded_output(command, Duration::from_secs(45), Some(root.path()));
    assert!(
        result.status.success(),
        "runner worker {name} failed: {}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    true
}

fn fixture(mode: &str) -> (tempfile::TempDir, Request) {
    let worker_root = std::env::var_os(WORKER_ROOT).expect("fixture must run in watchdog worker");
    let home = tempfile::Builder::new()
        .prefix("mods-runner-")
        .tempdir_in(worker_root)
        .unwrap();
    std::fs::set_permissions(home.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let program_dir = home.path().join("program");
    let scratch = home.path().join("scratch");
    std::fs::create_dir(&program_dir).unwrap();
    std::fs::create_dir(&scratch).unwrap();
    std::fs::set_permissions(&scratch, std::fs::Permissions::from_mode(0o700)).unwrap();
    let source = program_dir.join("helper.c");
    std::fs::write(
        &source,
        include_bytes!("../../fixtures/mods/runner-helper.c"),
    )
    .unwrap();
    let path = program_dir.join("helper");
    let mut compiler = Command::new("/usr/bin/cc");
    compiler.args(["-O0", "-o"]).arg(&path).arg(&source);
    let built = bounded_output(compiler, Duration::from_secs(15), None);
    assert!(
        built.status.success(),
        "fixture helper build: {}",
        String::from_utf8_lossy(&built.stderr)
    );
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o555)).unwrap();
    let sha256 = format!("{:x}", Sha256::digest(std::fs::read(&path).unwrap()));
    (
        home,
        Request {
            program: Program { path, sha256 },
            scratch,
            args: vec![mode.into()],
            deadline: Duration::from_secs(2),
            max_output_bytes: 16 * 1024,
        },
    )
}

#[test]
fn runner_preserves_exact_utf8_and_multiline_bytes() {
    if isolated_worker("runner_preserves_exact_utf8_and_multiline_bytes") {
        return;
    }
    let (_home, request) = fixture("echo");
    let input = "warning: café\nsecond line\n\n".as_bytes();
    assert_eq!(
        runner::run(&request, input, &AtomicBool::new(false)),
        Outcome::Output(input.to_vec())
    );
}

#[test]
fn runner_silent_hang_returns_timeout_within_whole_deadline() {
    if isolated_worker("runner_silent_hang_returns_timeout_within_whole_deadline") {
        return;
    }
    let (_home, request) = fixture("hang");
    let witness = StartedWitness(request.scratch.join("started.pid"));
    let start = Instant::now();
    assert_eq!(
        runner::run(&request, b"", &AtomicBool::new(false)),
        Outcome::Bypass("timeout")
    );
    assert!(
        start.elapsed() < Duration::from_secs(4),
        "whole operation must be bounded"
    );
    witness.assert_reaped();
}

#[test]
fn runner_simultaneous_stdout_stderr_flood_is_bounded() {
    if isolated_worker("runner_simultaneous_stdout_stderr_flood_is_bounded") {
        return;
    }
    let (_home, request) = fixture("flood");
    let start = Instant::now();
    assert_eq!(
        runner::run(&request, b"", &AtomicBool::new(false)),
        Outcome::Bypass("output_limit")
    );
    assert!(
        start.elapsed() < Duration::from_secs(4),
        "pipe limits must not deadlock"
    );
}

#[test]
fn runner_changed_program_is_refused_before_execution() {
    if isolated_worker("runner_changed_program_is_refused_before_execution") {
        return;
    }
    let (_home, request) = fixture("echo");
    std::fs::set_permissions(
        &request.program.path,
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    use std::io::Write;
    std::fs::OpenOptions::new()
        .append(true)
        .open(&request.program.path)
        .unwrap()
        .write_all(b"CHANGED_SYNTHETIC_PROGRAM")
        .unwrap();
    std::fs::set_permissions(
        &request.program.path,
        std::fs::Permissions::from_mode(0o555),
    )
    .unwrap();
    assert_eq!(
        runner::run(&request, b"must not execute", &AtomicBool::new(false)),
        Outcome::Bypass("program_changed")
    );
}

#[test]
fn runner_enforces_files_network_and_no_fork_with_positive_controls() {
    if isolated_worker("runner_enforces_files_network_and_no_fork_with_positive_controls") {
        return;
    }
    use std::net::TcpListener;
    use std::os::unix::{fs::symlink, net::UnixListener};
    let (home, mut request) = fixture("probe");
    let protected = home.path().join("synthetic-config");
    let other_run = home.path().join("other-run");
    std::fs::write(&protected, b"SYNTHETIC_CONFIG").unwrap();
    std::fs::write(&other_run, b"SYNTHETIC_OTHER_RUN").unwrap();
    let escape = request.scratch.join("escape");
    symlink(&protected, &escape).unwrap();
    let scratch_output = request.scratch.join("output");
    let tcp = TcpListener::bind("127.0.0.1:0").unwrap();
    tcp.set_nonblocking(true).unwrap();
    // Short, test-owned Unix path avoids the macOS sockaddr limit.
    let socket_home = tempfile::Builder::new()
        .prefix("mr-")
        .tempdir_in(std::env::var_os(WORKER_ROOT).unwrap())
        .unwrap();
    let socket = socket_home.path().join("socket");
    let unix = UnixListener::bind(&socket).unwrap();
    unix.set_nonblocking(true).unwrap();
    request.args.extend([
        protected.display().to_string(),
        other_run.display().to_string(),
        scratch_output.display().to_string(),
        escape.display().to_string(),
        tcp.local_addr().unwrap().port().to_string(),
        socket.display().to_string(),
    ]);
    // Identical0555 mode for both controls: helper first attempts chmod, then
    // write-open. A Unix mode denial cannot masquerade as sandbox enforcement.
    let mut control_command = Command::new(&request.program.path);
    control_command.args(&request.args);
    let control = bounded_output(control_command, Duration::from_secs(5), None);
    assert!(control.status.success());
    assert_eq!(String::from_utf8(control.stdout).unwrap(), concat!(
        "protected_read=allowed\nprotected_write=allowed\nother_run_read=allowed\nother_run_write=allowed\n",
        "program_write=allowed\nscratch_write=allowed\nsymlink_read=allowed\n",
        "tcp=allowed\nunix=allowed\nfork=allowed\nspawn=allowed\n"));
    drop(tcp.accept().expect("positive TCP connection"));
    drop(unix.accept().expect("positive Unix connection"));
    std::fs::remove_file(&scratch_output).unwrap();
    std::fs::set_permissions(
        &request.program.path,
        std::fs::Permissions::from_mode(0o555),
    )
    .unwrap();
    let output = runner::run(&request, b"", &AtomicBool::new(false));
    assert_eq!(output, Outcome::Output(concat!(
        "protected_read=denied\nprotected_write=denied\nother_run_read=denied\nother_run_write=denied\n",
        "program_write=denied\nscratch_write=allowed\nsymlink_read=denied\n",
        "tcp=denied\nunix=denied\nfork=denied\nspawn=denied\n").as_bytes().to_vec()));
    assert!(scratch_output.exists());
    assert_eq!(
        tcp.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
    assert_eq!(
        unix.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
    assert_eq!(std::fs::read(&protected).unwrap(), b"SYNTHETIC_CONFIG");
    assert_eq!(std::fs::read(&other_run).unwrap(), b"SYNTHETIC_OTHER_RUN");
}

#[test]
fn runner_environment_is_exact_private_allowlist() {
    if isolated_worker("runner_environment_is_exact_private_allowlist") {
        return;
    }
    let (_home, request) = fixture("env");
    let Outcome::Output(bytes) = runner::run(&request, b"", &AtomicBool::new(false)) else {
        panic!("runner must execute the environment witness");
    };
    let mut lines: Vec<String> = String::from_utf8(bytes)
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect();
    lines.sort();
    let private = request.scratch.canonicalize().unwrap();
    let mut expected = vec![
        format!("HOME={}", private.display()),
        "PATH=/usr/bin:/bin".to_string(),
        "RTK_TELEMETRY_DISABLED=1".to_string(),
        format!("TMPDIR={}", private.display()),
    ];
    expected.sort();
    assert_eq!(
        lines, expected,
        "no inherited credentials/provider environment"
    );
}

#[test]
fn runner_empty_success_is_distinct_from_failure_and_invalid_text() {
    if isolated_worker("runner_empty_success_is_distinct_from_failure_and_invalid_text") {
        return;
    }
    let (_home, mut request) = fixture("echo");
    assert_eq!(
        runner::run(&request, b"", &AtomicBool::new(false)),
        Outcome::Output(vec![])
    );
    request.args = vec!["fail".into()];
    assert_eq!(
        runner::run(&request, b"", &AtomicBool::new(false)),
        Outcome::Bypass("exit_failure")
    );
    request.args = vec!["invalid_utf8".into()];
    assert_eq!(
        runner::run(&request, b"", &AtomicBool::new(false)),
        Outcome::Bypass("invalid_text")
    );
}

#[test]
fn runner_blocked_stdin_and_separate_floods_are_bounded() {
    if isolated_worker("runner_blocked_stdin_and_separate_floods_are_bounded") {
        return;
    }
    let (_home, mut request) = fixture("hang");
    let witness = StartedWitness(request.scratch.join("started.pid"));
    let input = vec![b'I'; 1024 * 1024];
    let started = Instant::now();
    assert_eq!(
        runner::run(&request, &input, &AtomicBool::new(false)),
        Outcome::Bypass("timeout")
    );
    assert!(started.elapsed() < Duration::from_secs(4));
    witness.assert_reaped();
    for mode in ["stdout_flood", "stderr_flood"] {
        request.args = vec![mode.into()];
        let started = Instant::now();
        assert_eq!(
            runner::run(&request, b"", &AtomicBool::new(false)),
            Outcome::Bypass("output_limit")
        );
        assert!(started.elapsed() < Duration::from_secs(4));
    }
}

// The fixture owns this helper's isolated process group. On assertion failure,
// clean it before TempDir removal so a broken runner cannot strand a witness.
struct StartedWitness(std::path::PathBuf);
impl StartedWitness {
    fn pid(&self) -> Option<i32> {
        std::fs::read_to_string(&self.0)
            .ok()?
            .trim()
            .parse()
            .ok()
            .filter(|pid| *pid > 1)
    }
    fn assert_reaped(&self) {
        let pid = self.pid().expect("actual helper startup witness");
        let exists = unsafe { libc::kill(pid, 0) } == 0;
        assert!(
            !exists,
            "runner returned with owned helper {pid} still alive/unreaped"
        );
    }
}
impl Drop for StartedWitness {
    fn drop(&mut self) {
        if let Some(pid) = self.pid() {
            // All runner children must start their own process group. Never
            // signal the test process group if that invariant is broken.
            if unsafe { libc::getpgid(pid) } == pid {
                unsafe {
                    libc::kill(-pid, libc::SIGKILL);
                    libc::waitpid(pid, std::ptr::null_mut(), 0);
                }
            }
        }
    }
}

#[test]
fn runner_cancellation_covers_running_and_prelaunch_work() {
    if isolated_worker("runner_cancellation_covers_running_and_prelaunch_work") {
        return;
    }
    use std::sync::atomic::Ordering;
    let (_home, mut request) = fixture("hang");
    let witness = StartedWitness(request.scratch.join("started.pid"));
    let cancelled = AtomicBool::new(true);
    assert_eq!(
        runner::run(&request, b"", &cancelled),
        Outcome::Bypass("cancelled")
    );
    assert!(
        !witness.0.exists(),
        "prelaunch cancellation cannot execute helper"
    );
    cancelled.store(false, Ordering::SeqCst);
    request.deadline = Duration::from_secs(10);
    let started = Instant::now();
    let saw_start = AtomicBool::new(false);
    std::thread::scope(|scope| {
        scope.spawn(|| {
            let deadline = Instant::now() + Duration::from_secs(3);
            while witness.pid().is_none() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(5));
            }
            saw_start.store(witness.pid().is_some(), Ordering::SeqCst);
            cancelled.store(true, Ordering::SeqCst);
        });
        assert_eq!(
            runner::run(&request, b"", &cancelled),
            Outcome::Bypass("cancelled")
        );
    });
    assert!(
        saw_start.load(Ordering::SeqCst),
        "cancellation must cover an actually running helper"
    );
    assert!(started.elapsed() < Duration::from_secs(5));
    witness.assert_reaped();
}

#[test]
fn runner_refuses_symlinked_program_and_scratch_ancestors() {
    if isolated_worker("runner_refuses_symlinked_program_and_scratch_ancestors") {
        return;
    }
    use std::os::unix::fs::symlink;
    let (home, mut request) = fixture("echo");
    let original = request.program.path.clone();
    let alias = home.path().join("helper-alias");
    symlink(&original, &alias).unwrap();
    request.program.path = alias;
    assert_eq!(
        runner::run(&request, b"", &AtomicBool::new(false)),
        Outcome::Bypass("unsafe_path")
    );
    request.program.path = original;
    let actual_scratch = request.scratch.clone();
    let scratch_alias = home.path().join("scratch-alias");
    symlink(&actual_scratch, &scratch_alias).unwrap();
    request.scratch = scratch_alias;
    assert_eq!(
        runner::run(&request, b"", &AtomicBool::new(false)),
        Outcome::Bypass("unsafe_path")
    );
    request.scratch = actual_scratch;
    let parent_alias = home.path().join("program-parent-alias");
    symlink(request.program.path.parent().unwrap(), &parent_alias).unwrap();
    let program = request.program.path.clone();
    request.program.path = parent_alias.join("helper");
    assert_eq!(
        runner::run(&request, b"", &AtomicBool::new(false)),
        Outcome::Bypass("unsafe_path")
    );
    request.program.path = program;
    let scratch_parent_alias = home.path().join("scratch-parent-alias");
    symlink(home.path(), &scratch_parent_alias).unwrap();
    request.scratch = scratch_parent_alias.join("scratch");
    assert_eq!(
        runner::run(&request, b"", &AtomicBool::new(false)),
        Outcome::Bypass("unsafe_path")
    );
}
