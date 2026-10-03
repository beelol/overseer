//! AC200 file containment/read bounds. Only disposable synthetic files.
mod common;
use common::*;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const OUTSIDE: &str = "SYNTHETIC_OUTSIDE_SENTINEL_MUST_NOT_REACH_MODEL";
const SECRET: &str = "sk-proj-syntheticSYNTHETICabcdefghijkl012345";
struct Hold(PathBuf);
impl Hold {
    fn arm(&self) {
        std::fs::write(self.0.join("armed"), b"1").unwrap();
    }
    fn entered(&self) {
        let end = Instant::now() + Duration::from_secs(5);
        while !self.0.join("entered").exists() {
            assert!(
                Instant::now() < end,
                "file tool never reached actual validated boundary"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    fn release(&self) {
        std::fs::write(self.0.join("release"), b"1").unwrap();
    }
}
impl Drop for Hold {
    fn drop(&mut self) {
        let _ = std::fs::write(self.0.join("release"), b"1");
    }
}

fn setup(root: &Path, hold: &Hold) -> (Daemon, String, String, PathBuf) {
    let repo = repo(&root.join("repo"));
    let d = Daemon::start(&[
        ("OVERSEER_TEST_AUTO_DISABLED", "1"),
        ("OVERSEER_TEST_NET", "1"),
        ("OVERSEER_CONTINUITY_PROBES", "off"),
        ("OVERSEER_TEST_FILE_READ_GATE", hold.0.to_str().unwrap()),
    ]);
    let run = run_id(&d.generic(&repo, "worktree", "/usr/bin/true", &[]));
    d.wait_done(&run, 15);
    // Owner-issued authenticated read role on a real completed fixture run.
    // This does not claim a protected native action or installed harness route.
    let token = d.call("overseer.token", json!({"run_id":run,"role":"overseer"}))["token"]
        .as_str()
        .unwrap()
        .to_owned();
    let workspace = d.run(&run)["workspace_id"].clone();
    let state = d.call("state", json!({}));
    let path = state["workspaces"]
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w["id"] == workspace)
        .unwrap()["path"]
        .as_str()
        .unwrap();
    (d, run, token, PathBuf::from(path))
}
fn read(d: &Daemon, run: &str, token: &str, path: &str) -> Result<Value, String> {
    d.try_call(
        "overseer.tool",
        json!({"token":token,"name":"file","arguments":{"id":run,"path":path}}),
    )
}
fn assert_safe(result: Result<Value, String>) {
    match result {
        Ok(value) => {
            assert!(
                !value.to_string().contains(OUTSIDE),
                "outside bytes escaped the checked worktree"
            );
            assert!(
                value["text"]
                    .as_str()
                    .unwrap()
                    .contains("inside-before-swap"),
                "only a pinned original inode may succeed: {value}"
            );
        }
        Err(error) => assert!(!error.contains(OUTSIDE)),
    }
}
fn swap_case(parent: bool) {
    use std::os::unix::fs::symlink;
    let root = tmp();
    let gate = root.path().join("gate");
    std::fs::create_dir(&gate).unwrap();
    let hold = Hold(gate);
    let (d, run, token, ws) = setup(root.path(), &hold);
    let outside = root.path().join("outside");
    std::fs::create_dir(&outside).unwrap();
    std::fs::write(outside.join("note.txt"), OUTSIDE).unwrap();
    std::fs::create_dir(ws.join("notes")).unwrap();
    std::fs::write(ws.join("notes/note.txt"), "inside-before-swap\n").unwrap();
    hold.arm();
    let result = std::thread::scope(|scope| {
        let pending = scope.spawn(|| read(&d, &run, &token, "notes/note.txt"));
        hold.entered();
        if parent {
            std::fs::rename(ws.join("notes"), ws.join("original-notes")).unwrap();
            symlink(&outside, ws.join("notes")).unwrap();
        } else {
            std::fs::rename(ws.join("notes/note.txt"), ws.join("notes/original.txt")).unwrap();
            symlink(outside.join("note.txt"), ws.join("notes/note.txt")).unwrap();
        }
        hold.release();
        pending.join().unwrap()
    });
    assert_safe(result);
    std::fs::write(ws.join("control.txt"), "in-worktree control\n").unwrap();
    let control =
        read(&d, &run, &token, "control.txt").expect("valid in-worktree read remains usable");
    assert_eq!(control["text"], "in-worktree control\n");
}
#[test]
fn ac200_leaf_swap_cannot_read_outside_worktree() {
    swap_case(false);
}
#[test]
fn ac200_parent_swap_cannot_read_outside_worktree() {
    swap_case(true);
}
#[test]
fn ac200_large_file_is_rejected_with_bounded_actual_read() {
    let root = tmp();
    let gate = root.path().join("gate");
    std::fs::create_dir(&gate).unwrap();
    let hold = Hold(gate);
    let (d, run, token, ws) = setup(root.path(), &hold);
    std::fs::File::create(ws.join("large.txt"))
        .unwrap()
        .set_len(64 * 1024 * 1024)
        .unwrap();
    let result = read(&d, &run, &token, "large.txt");
    assert!(
        result.as_ref().is_err_and(|e| e.contains("too large")),
        "oversize refusal is visible: {result:?}"
    );
    let read_bytes: usize = std::fs::read_to_string(hold.0.join("read-bytes"))
        .expect("actual read receipt")
        .parse()
        .unwrap();
    assert!(
        read_bytes <= 4 * 1024 * 1024 + 1,
        "read {read_bytes} bytes before enforcing the4MiB limit"
    );
}
#[test]
fn ac200_successful_file_consumption_preserves_context_and_redacts_secret() {
    let root = tmp();
    let gate = root.path().join("gate");
    std::fs::create_dir(&gate).unwrap();
    let hold = Hold(gate);
    let (d, run, token, ws) = setup(root.path(), &hold);
    std::fs::write(
        ws.join("evidence.txt"),
        format!("The synthetic build needs2 tests.\ncafé evidence: {SECRET}\n"),
    )
    .unwrap();
    let answer =
        read(&d, &run, &token, "evidence.txt").expect("actual successful file consumption");
    assert_eq!(answer["is_error"], false);
    let text = answer["text"].as_str().unwrap();
    assert!(text.contains("The synthetic build needs2 tests.\n"));
    assert!(text.contains("café evidence: ") && text.contains("[redacted]"));
    assert!(!text.contains(SECRET));
    assert!(d
        .events(&run)
        .iter()
        .any(|e| e["kind"] == "overseer_tool_call" && e["payload"]["name"] == "file"));
}
