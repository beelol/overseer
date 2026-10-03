//! Frozen pre-Mods launch vectors: exact generated epochs, never a live tool-list subset.
//! Native harnesses and injected legacy launch records are synthetic; no installed-model claim.
mod common;
use common::*;
use serde_json::{json, Value};
use std::path::Path;
use std::time::{Duration, Instant};

// Literal ordering read independently from these committed generators:
// 4c371ecd1f57affd6b5328a3ba9cf4eba2ee82b6..24c3c245bb8e8ac4df7ccf2373fc9728de792135.
const GATE_S_13: &[&str] = &[
    "roster",
    "agent",
    "conflicts",
    "conversation",
    "changes",
    "diff",
    "file",
    "search",
    "usage",
    "check_in",
    "rally",
    "answer",
    "propose",
];
// 3ab9c1f738a5a177f46f79e7fa4bf03daac35618..55ebf1d22760280ba750007c9d49cd315ddda919.
const ACCOUNTS_14: &[&str] = &[
    "roster",
    "agent",
    "conflicts",
    "conversation",
    "changes",
    "diff",
    "file",
    "search",
    "usage",
    "accounts",
    "check_in",
    "rally",
    "answer",
    "propose",
];
const DENIED: &str = "Bash,Edit,Write,MultiEdit,NotebookEdit,WebFetch,WebSearch,Agent,Task,TodoWrite,KillShell,BashOutput,ToolSearch,AskUserQuestion,EnterPlanMode,ExitPlanMode";

fn idle(d: &Daemon) -> Value {
    let end = Instant::now() + Duration::from_secs(25);
    loop {
        let s = d.call("overseer.session", json!({}));
        if s["run_id"].is_string()
            && !["queued", "starting", "running", "waiting_for_user"]
                .contains(&s["run_status"].as_str().unwrap_or(""))
        {
            return s;
        }
        assert!(
            Instant::now() < end,
            "setup: native fixture did not settle: {s}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn start(harness: &str) -> Daemon {
    let executable = repo_root().join("fixtures/fake-harness/continuity-harness.js");
    let key = if harness == "claude" {
        "OVERSEER_CLAUDE_PATH"
    } else {
        "OVERSEER_CODEX_PATH"
    };
    let d = Daemon::start(&[
        (key, executable.to_str().unwrap()),
        ("OVERSEER_TEST_AUTO_DISABLED", "1"),
    ]);
    d.call("agent.cadence", json!({"cadence":"off","by":"owner"}));
    d.call(
        "overseer.send",
        json!({"text":"Summarize this synthetic fixture.","surface":"ctl","harness":harness}),
    );
    idle(&d);
    d
}

fn saved_args(d: &Daemon, run: &str, harness: &str, names: &[&str]) -> (Vec<String>, String) {
    let token = d.call("overseer.token", json!({"run_id":run,"role":"overseer"}))["token"]
        .as_str()
        .unwrap()
        .to_string();
    let mut args = vec!["--synthetic-unrelated-option".to_string()];
    if harness == "claude" {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let config = d.home.path().join("overseer/scratch/mcp.json");
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&config)
            .unwrap();
        // Only paths and the genuine disposable unbound credential are substituted.
        write!(
            file,
            "{}",
            json!({"mcpServers":{"overseer":{"type":"stdio","command":BIN,
            "args":["mcp","--socket",d.socket()],"env":{"OVERSEER_MCP_TOKEN":token}}}})
        )
        .unwrap();
        args.extend([
            "--mcp-config".into(),
            config.display().to_string(),
            "--strict-mcp-config".into(),
            "--allowedTools".into(),
            names
                .iter()
                .map(|n| format!("mcp__overseer__{n}"))
                .collect::<Vec<_>>()
                .join(","),
            "--disallowedTools".into(),
            DENIED.into(),
        ]);
    } else {
        args.extend([
            "-c".into(),
            format!("mcp_servers.overseer.command={}", json!(BIN)),
            "-c".into(),
            format!(
                "mcp_servers.overseer.args=[\"mcp\",\"--socket\",{}]",
                json!(d.socket())
            ),
            "-c".into(),
            format!(
                "mcp_servers.overseer.env={{ OVERSEER_MCP_TOKEN = {} }}",
                json!(token)
            ),
        ]);
        for name in names {
            args.extend([
                "-c".into(),
                format!("mcp_servers.overseer.tools.{name}.approval_mode=\"approve\""),
            ]);
        }
    }
    (args, token)
}

fn inject(d: &Daemon, run: &str, args: &[String]) {
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let raw: String = db
        .query_row("SELECT launch FROM runs WHERE id=?1", [run], |r| r.get(0))
        .unwrap();
    let mut meta: Value = serde_json::from_str(&raw).unwrap();
    meta["generic"]["extra_args"] = json!(args);
    db.execute(
        "UPDATE runs SET launch=?2 WHERE id=?1",
        rusqlite::params![run, meta.to_string()],
    )
    .unwrap();
}

fn migration(harness: &str, names: &[&str]) {
    let d = start(harness);
    let original = idle(&d);
    let run = original["run_id"].as_str().unwrap();
    let (args, old) = saved_args(&d, run, harness, names);
    inject(&d, run, &args);
    // One real launch attempt. No queue retries or fabricated Continuity status.
    let follow = d.try_call(
        "run.follow_up",
        json!({"run_id":run,"prompt":"Summarize again."}),
    );
    assert!(
        follow.is_ok(),
        "exact frozen {harness} {}-tool group must migrate: {follow:?}",
        names.len()
    );
    let after = idle(&d);
    assert_eq!(after["id"], original["id"]);
    assert_eq!(after["run_id"], original["run_id"]);
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let dir: String = db
        .query_row("SELECT run_dir FROM runs WHERE id=?1", [run], |r| r.get(0))
        .unwrap();
    let launch: Value =
        serde_json::from_slice(&std::fs::read(Path::new(&dir).join("launch.json")).unwrap())
            .unwrap();
    let actual: Vec<&str> = launch["args"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a.as_str().unwrap())
        .collect();
    assert!(
        actual.contains(&"--synthetic-unrelated-option"),
        "unrelated option retained"
    );
    assert!(
        !actual.iter().any(|a| a.contains(&old)),
        "old plaintext credential not carried forward"
    );
    if harness == "claude" {
        assert_eq!(actual.iter().filter(|a| **a == "--mcp-config").count(), 1);
        let at = actual.iter().position(|a| *a == "--mcp-config").unwrap();
        assert_ne!(
            actual[at + 1],
            d.home
                .path()
                .join("overseer/scratch/mcp.json")
                .to_str()
                .unwrap()
        );
        assert!(actual.contains(&"--strict-mcp-config"));
        let denied = actual
            .iter()
            .position(|a| *a == "--disallowedTools")
            .unwrap();
        assert_eq!(actual[denied + 1], DENIED);
    } else {
        assert_eq!(
            actual
                .iter()
                .filter(|a| a.starts_with("mcp_servers.overseer.command="))
                .count(),
            1
        );
        assert!(!actual
            .iter()
            .any(|a| a.starts_with("mcp_servers.overseer.env=")));
        assert!(actual.iter().any(|a| a.contains("--capability-file")));
        assert!(
            actual.contains(&"sandbox_mode=\"read-only\""),
            "native Codex remains read-only"
        );
    }
    let actual_owned: Vec<String> = actual.iter().map(|a| (*a).to_string()).collect();
    let new = native_capability_from_args(&actual_owned, harness);
    assert_ne!(new, old, "new process gets a private bound credential");
    let counts: (i64,i64)=db.query_row("SELECT COUNT(*),SUM(revoked_ms IS NOT NULL) FROM overseer_tokens WHERE run_id=?1 AND native_turn_id IS NOT NULL",[run],|r|Ok((r.get(0)?,r.get(1)?))).unwrap();
    assert_eq!(
        counts,
        (2, 1),
        "one new bound capability, predecessor revoked"
    );
    // This direct follow-up does not publish overseer_turns cause: read/config
    // compatibility only, not protected Continuity-origin qualification.
}

#[test]
fn ac200_claude_frozen_gate_s_13_migrates() {
    migration("claude", GATE_S_13);
}
#[test]
fn ac200_claude_frozen_accounts_14_migrates() {
    migration("claude", ACCOUNTS_14);
}
#[test]
fn ac200_codex_frozen_gate_s_13_migrates() {
    migration("codex", GATE_S_13);
}
#[test]
fn ac200_codex_frozen_accounts_14_migrates() {
    migration("codex", ACCOUNTS_14);
}

fn refuse_changed_group(harness: &str, change: &str) {
    let d = start(harness);
    let run = idle(&d)["run_id"].as_str().unwrap().to_string();
    let mut names = ACCOUNTS_14.to_vec();
    match change {
        "unknown" => names.push("synthetic_unknown_authority"),
        "reordered" => names.swap(1, 2),
        "removed" => {
            names.remove(1);
        }
        _ => unreachable!(),
    }
    let (args, _) = saved_args(&d, &run, harness, &names);
    inject(&d, &run, &args);
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let before: (i64, String) = db
        .query_row(
            "SELECT process_generation,run_dir FROM runs WHERE id=?1",
            [&run],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    let config = d.home.path().join("overseer/scratch/mcp.json");
    let bytes = (harness == "claude").then(|| std::fs::read(&config).unwrap());
    let attempt = d.try_call(
        "run.follow_up",
        json!({"run_id":run,"prompt":"Summarize again."}),
    );
    if attempt.is_ok() {
        idle(&d);
    }
    let error = attempt.expect_err("changed list must not be treated as a historical subset");
    assert!(
        error.contains("legacy"),
        "visible migration refusal: {error}"
    );
    let after: (i64, String) = db
        .query_row(
            "SELECT process_generation,run_dir FROM runs WHERE id=?1",
            [&run],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(after, before, "no new process");
    let count: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM overseer_tokens WHERE run_id=?1 AND native_turn_id IS NOT NULL",
            [&run],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 1, "no replacement capability minted");
    let raw: String = db
        .query_row("SELECT launch FROM runs WHERE id=?1", [&run], |r| r.get(0))
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&raw).unwrap()["generic"]["extra_args"],
        json!(args),
        "saved policy retained"
    );
    if let Some(bytes) = bytes {
        assert_eq!(std::fs::read(config).unwrap(), bytes);
    }
}
#[test]
fn ac200_claude_modified_historical_groups_refuse() {
    for change in ["unknown", "reordered", "removed"] {
        refuse_changed_group("claude", change);
    }
}
#[test]
fn ac200_codex_modified_historical_groups_refuse() {
    for change in ["unknown", "reordered", "removed"] {
        refuse_changed_group("codex", change);
    }
}
