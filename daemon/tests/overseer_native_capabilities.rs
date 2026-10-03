//! AC-200: native capabilities retain exact durable turn identity across restart,
//! and fail closed when absent, revoked, legacy/unbound or superseded.
mod common;
use common::*;
use serde_json::{json, Value};
use std::path::Path;
use std::time::{Duration, Instant};

fn wait_idle(d: &Daemon) -> Value {
    let until = Instant::now() + Duration::from_secs(25);
    loop {
        let session = d.call("overseer.session", json!({}));
        if session["run_id"].is_string()
            && !["queued", "starting", "running", "waiting_for_user"]
                .contains(&session["run_status"].as_str().unwrap_or(""))
        {
            return session;
        }
        assert!(
            Instant::now() < until,
            "setup: native turn did not end: {session}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn fixture() -> Daemon {
    let d = Daemon::start(&[
        (
            "OVERSEER_CLAUDE_PATH",
            &repo_root()
                .join("fixtures/fake-harness/claude-fixture.js")
                .display()
                .to_string(),
        ),
        ("OVERSEER_TEST_AUTO_DISABLED", "1"),
    ]);
    d.call("agent.cadence", json!({"cadence":"off", "by":"owner"}));
    d.call(
        "overseer.send",
        json!({"text":"Summarize current work.", "surface":"ctl", "harness":"claude"}),
    );
    wait_idle(&d);
    d
}

fn archive(d: &Daemon, token: &str, subject: &str) -> Value {
    d.call("overseer.tool", json!({"token":token,"name":"propose", "arguments":{"actions":[{"action":"archive", "agent":subject}]}}))
}

fn subject(d: &Daemon, root: &Path) -> String {
    let repository = repo(&root.join("repo"));
    let id = run_id(&d.generic(
        &repository,
        "worktree",
        "/bin/sh",
        &["-c", "echo synthetic capability subject"],
    ));
    d.wait_done(&id, 20);
    id
}

fn assert_refused(reply: &Value) {
    assert_eq!(
        reply["is_error"], true,
        "protected native call must refuse: {reply}"
    );
    assert!(
        reply["text"].as_str().unwrap().contains("refused"),
        "{reply}"
    );
}

#[test]
fn ac200_native_capability_restart_retains_exact_origin_and_owner_control() {
    let root = tmp();
    let mut d = fixture();
    let original = wait_idle(&d);
    let run = original["run_id"].as_str().unwrap();
    let token = native_capability(&d, run);
    let target = subject(&d, root.path());
    d.kill9();
    d.spawn();
    let current = d.call("overseer.session", json!({}));
    assert_eq!(current["id"], original["id"]);
    assert_eq!(current["run_id"], original["run_id"]);
    assert_eq!(
        native_capability(&d, run),
        token,
        "restart must preserve the original private file"
    );
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let binding: (String,String) = db.query_row(
        "SELECT c.native_turn_id,o.cause FROM overseer_tokens c JOIN overseer_turns o ON o.turn_id=c.native_turn_id WHERE c.run_id=?1 AND c.revoked_ms IS NULL",
        [run], |r| Ok((r.get(0)?,r.get(1)?))).unwrap();
    assert_eq!(binding.1, "owner");
    let reply = archive(&d, &token, &target);
    assert_eq!(
        reply["is_error"], false,
        "valid current owner capability survives restart: {reply}"
    );
    let card = d.call("overseer.session", json!({}))["proposals"]
        .as_array()
        .unwrap()
        .last()
        .unwrap()
        .clone();
    assert_eq!(card["cause"], "owner");
    assert_eq!(card["state"], "open");
    assert!(
        !d.events(&target)
            .iter()
            .any(|e| matches!(e["kind"].as_str(), Some("archived" | "task_archived"))),
        "no Yes/effect"
    );
}

#[test]
fn ac200_unbound_direct_and_revoked_native_capabilities_cannot_propose() {
    let root = tmp();
    let d = fixture();
    let session = wait_idle(&d);
    let run = session["run_id"].as_str().unwrap();
    let target = subject(&d, root.path());
    let unbound = d.call("overseer.token", json!({"run_id":run,"role":"overseer"}))["token"]
        .as_str()
        .unwrap()
        .to_string();
    let tools = d.call("overseer.tools", json!({"token":unbound}));
    assert!(
        tools["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|tool| tool["name"] == "roster"),
        "unbound legacy/direct read compatibility"
    );
    assert_refused(&archive(&d, &unbound, &target));
    let token = native_capability(&d, run);
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    // Injected revocation state only; no claim that this is a production owner API.
    assert_eq!(db.execute("UPDATE overseer_tokens SET revoked_ms=1 WHERE run_id=?1 AND native_turn_id IS NOT NULL", [run]).unwrap(),1);
    assert_refused(&archive(&d, &token, &target));
    assert!(d
        .try_call("overseer.tools", json!({"token":token}))
        .unwrap_err()
        .contains("revoked"));
    assert!(d.call("overseer.session", json!({}))["proposals"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(
        d.events(run)
            .iter()
            .filter(|event| event["kind"] == "overseer_tool_call"
                && event["payload"]["name"] == "propose"
                && event["payload"]["role"] == "overseer"
                && event["payload"]["refused"].is_string())
            .count()
            >= 2,
        "protected refusal retains actual caller run/role telemetry"
    );
}

#[test]
fn ac200_failed_native_successor_launch_never_revives_predecessor_capability() {
    let root = tmp();
    let mut d = fixture();
    let original = wait_idle(&d);
    let run = original["run_id"].as_str().unwrap().to_string();
    let old = native_capability(&d, &run);
    let target = subject(&d, root.path());
    d.kill9();
    d.env
        .iter_mut()
        .find(|(key, _)| key == "OVERSEER_CLAUDE_PATH")
        .unwrap()
        .1 = "/nonexistent/synthetic-native-launch-failure".into();
    d.spawn();
    // One real new launch attempt, with its real durable turn and adapter failure.
    let attempted = d.try_call(
        "overseer.send",
        json!({"text":"Summarize current work again.","surface":"ctl"}),
    );
    let failed = wait_idle(&d);
    assert_eq!(failed["run_id"], run);
    assert_eq!(
        failed["run_status"], "failed",
        "actual successor must fail: {attempted:?}; {failed}"
    );
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let bound: (i64,i64) = db.query_row("SELECT COUNT(*),SUM(revoked_ms IS NOT NULL) FROM overseer_tokens WHERE run_id=?1 AND native_turn_id IS NOT NULL", [&run], |r|Ok((r.get(0)?,r.get(1)?))).unwrap();
    assert_eq!(
        bound,
        (2, 1),
        "failure must retain new binding and predecessor revocation"
    );
    assert_refused(&archive(&d, &old, &target));
    assert!(d.call("overseer.session", json!({}))["proposals"]
        .as_array()
        .unwrap()
        .is_empty());
}

#[test]
fn ac200_exact_legacy_scratch_migration_keeps_unrelated_arguments() {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let d = fixture();
    let session = wait_idle(&d);
    let run = session["run_id"].as_str().unwrap();
    let old = d.call("overseer.token", json!({"run_id":run,"role":"overseer"}))["token"]
        .as_str()
        .unwrap()
        .to_string();
    let config = d.home.path().join("overseer/scratch/mcp.json");
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&config)
        .unwrap();
    // Inject only the exact old daemon-owned launch record/config shape, using
    // the actual isolated run's genuine unbound compatibility token.
    write!(file,"{}",json!({"mcpServers":{"overseer":{"type":"stdio","command":BIN,"args":["mcp","--socket",d.socket()],"env":{"OVERSEER_MCP_TOKEN":old}}}})).unwrap();
    drop(file);
    let allowed = d.call("overseer.tools", json!({"token":old}))["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| format!("mcp__overseer__{}", tool["name"].as_str().unwrap()))
        .collect::<Vec<_>>()
        .join(",");
    let args = vec!["--verbose".to_string(),"--mcp-config".into(),config.display().to_string(),"--strict-mcp-config".into(),"--allowedTools".into(),allowed,"--disallowedTools".into(),"Bash,Edit,Write,MultiEdit,NotebookEdit,WebFetch,WebSearch,Agent,Task,TodoWrite,KillShell,BashOutput,ToolSearch,AskUserQuestion,EnterPlanMode,ExitPlanMode".into()];
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
    let sent = d.call(
        "overseer.send",
        json!({"text":"Summarize current work again.","surface":"ctl"}),
    );
    assert_eq!(sent["queued"], false, "real migrated native launch: {sent}");
    wait_idle(&d);
    let dir: String = db
        .query_row("SELECT run_dir FROM runs WHERE id=?1", [run], |r| r.get(0))
        .unwrap();
    let actual: Value =
        serde_json::from_slice(&std::fs::read(Path::new(&dir).join("launch.json")).unwrap())
            .unwrap();
    let actual_args = actual["args"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a.as_str().unwrap())
        .collect::<Vec<_>>();
    assert!(
        actual_args.contains(&"--verbose"),
        "unrelated argument retained"
    );
    assert_eq!(
        actual_args.iter().filter(|a| **a == "--mcp-config").count(),
        1
    );
    assert!(
        !actual_args.contains(&config.to_str().unwrap()),
        "shared legacy config is replaced"
    );
    assert_ne!(
        native_capability(&d, run),
        old,
        "the actual new launch receives a bound capability"
    );
}

#[test]
fn ac200_missing_or_unprivate_capability_file_never_uses_environment_fallback() {
    use std::io::Read;
    use std::os::unix::fs::PermissionsExt;
    use std::process::{Child, Command, Stdio};
    struct Reap(Child);
    impl Drop for Reap {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let root = tmp();
    let d = fixture();
    let session = wait_idle(&d);
    let valid = native_capability(&d, session["run_id"].as_str().unwrap());
    let missing = root.path().join("missing-native.cap");
    let public = root.path().join("public-native.cap");
    std::fs::write(&public, &valid).unwrap();
    std::fs::set_permissions(&public, std::fs::Permissions::from_mode(0o644)).unwrap();
    let malformed = root.path().join("malformed-native.cap");
    std::fs::write(&malformed, "not-a-capability").unwrap();
    std::fs::set_permissions(&malformed, std::fs::Permissions::from_mode(0o600)).unwrap();
    for path in [missing, public, malformed] {
        let mut child = Reap(
            Command::new(BIN)
                .args(["mcp", "--socket"])
                .arg(d.socket())
                .arg("--capability-file")
                .arg(&path)
                .env("OVERSEER_MCP_TOKEN", &valid)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap(),
        );
        let until = Instant::now() + Duration::from_secs(5);
        let status = loop {
            if let Some(status) = child.0.try_wait().unwrap() {
                break status;
            }
            assert!(
                Instant::now() < until,
                "MCP startup failure must be bounded"
            );
            std::thread::sleep(Duration::from_millis(10));
        };
        let mut error = String::new();
        child
            .0
            .stderr
            .take()
            .unwrap()
            .read_to_string(&mut error)
            .unwrap();
        assert!(
            !status.success(),
            "invalid private file must fail even with a valid legacy environment token"
        );
        assert!(
            !error.contains(&valid),
            "refusal must not reveal the capability"
        );
    }
}

#[test]
fn ac200_modified_legacy_config_refuses_without_discarding_added_policy() {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let d = fixture();
    let session = wait_idle(&d);
    let run = session["run_id"].as_str().unwrap();
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let before: (i64, String) = db
        .query_row(
            "SELECT process_generation,run_dir FROM runs WHERE id=?1",
            [run],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    let old = d.call("overseer.token", json!({"run_id":run,"role":"overseer"}))["token"]
        .as_str()
        .unwrap()
        .to_string();
    let config = d.home.path().join("overseer/scratch/mcp.json");
    let mut body = json!({"mcpServers":{"overseer":{"type":"stdio","command":BIN,"args":["mcp","--socket",d.socket()],"env":{"OVERSEER_MCP_TOKEN":old}}}});
    // Test-injected additions are deliberately outside the generated shape.
    // A migration must not silently delete another server or environment/policy.
    body["mcpServers"]["owner-extra"] =
        json!({"type":"stdio","command":"/nonexistent/synthetic-extra-server"});
    body["mcpServers"]["overseer"]["env"]["SYNTHETIC_ADDED_SETTING"] = json!("retain");
    body["mcpServers"]["overseer"]["policy"] = json!({"synthetic_rule":"retain"});
    let original = serde_json::to_vec(&body).unwrap();
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&config)
        .unwrap();
    file.write_all(&original).unwrap();
    drop(file);
    let allowed = d.call("overseer.tools", json!({"token":old}))["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| format!("mcp__overseer__{}", tool["name"].as_str().unwrap()))
        .collect::<Vec<_>>()
        .join(",");
    let args = vec!["--mcp-config".to_string(),config.display().to_string(),"--strict-mcp-config".into(),"--allowedTools".into(),allowed,"--disallowedTools".into(),"Bash,Edit,Write,MultiEdit,NotebookEdit,WebFetch,WebSearch,Agent,Task,TodoWrite,KillShell,BashOutput,ToolSearch,AskUserQuestion,EnterPlanMode,ExitPlanMode".into()];
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
    // Exercise the real launch boundary once directly, avoiding the conversation
    // retry queue: this fixture qualifies migration refusal, not action authority.
    let error = d
        .try_call(
            "run.follow_up",
            json!({"run_id":run,"prompt":"Summarize current work again."}),
        )
        .unwrap_err();
    assert!(
        error.contains("exact daemon-generated configuration"),
        "modified legacy configuration must visibly refuse: {error}"
    );
    let after: (i64, String) = db
        .query_row(
            "SELECT process_generation,run_dir FROM runs WHERE id=?1",
            [run],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(after, before, "no replacement native process launched");
    assert_eq!(
        std::fs::read(&config).unwrap(),
        original,
        "added server/environment/policy remains intact"
    );
    let capabilities: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM overseer_tokens WHERE run_id=?1 AND native_turn_id IS NOT NULL",
            [run],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        capabilities, 1,
        "ambiguous migration refuses before minting a replacement capability"
    );
    let persisted: String = db
        .query_row("SELECT launch FROM runs WHERE id=?1", [run], |r| r.get(0))
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&persisted).unwrap()["generic"]["extra_args"],
        json!(args),
        "saved policy is not dropped"
    );
}
