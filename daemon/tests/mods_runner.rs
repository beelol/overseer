//! First internal runner baselines. Not installation, native-hook or AC271 proof.
#![cfg(target_os = "macos")]
#[path = "../src/mods/runner.rs"]
mod runner;

use runner::{Outcome, Program, Request};
use sha2::{Digest, Sha256};
use std::os::unix::fs::PermissionsExt;
use std::process::Command;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

fn fixture(mode: &str) -> (tempfile::TempDir, Request) {
    let home = tempfile::tempdir().unwrap();
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
    let built = Command::new("/usr/bin/cc")
        .args(["-O0", "-o"])
        .arg(&path)
        .arg(&source)
        .output()
        .unwrap();
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
    let (_home, request) = fixture("echo");
    let input = "warning: café\nsecond line\n\n".as_bytes();
    assert_eq!(
        runner::run(&request, input, &AtomicBool::new(false)),
        Outcome::Output(input.to_vec())
    );
}

#[test]
fn runner_silent_hang_returns_timeout_within_whole_deadline() {
    let (_home, request) = fixture("hang");
    let start = Instant::now();
    assert_eq!(
        runner::run(&request, b"", &AtomicBool::new(false)),
        Outcome::Bypass("timeout")
    );
    assert!(
        start.elapsed() < Duration::from_secs(4),
        "whole operation must be bounded"
    );
}

#[test]
fn runner_simultaneous_stdout_stderr_flood_is_bounded() {
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
