//! AUTO-AC-26: what Auto retains exposes no credential or account identity.

mod common;
use common::*;
use serde_json::json;
use std::path::{Path, PathBuf};

fn fixture(name: &str) -> String {
    repo_root().join("fixtures").join(name).display().to_string()
}

fn files_under(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() { files_under(&path, out); } else { out.push(path); }
    }
}

fn containing(files: &[PathBuf], needle: &str) -> Vec<String> {
    files.iter().filter(|file| std::fs::read(file).map(|bytes|
        bytes.windows(needle.len()).any(|window| window == needle.as_bytes())).unwrap_or(false))
        .map(|file| file.display().to_string()).collect()
}

/// Credential files planted in each Auto profile's harness home, and the
/// fixtures' account e-mails and raw account ids, never reach anything the
/// daemon keeps: its database and write-ahead log, the separate learning
/// file, the decision traces and events, handoff records, the user-triggered
/// usage export, or the daemon's logs and run directories. The flow covers
/// quota and identity reads, Codex and Claude managed children, a checkpoint
/// handoff, an account change (whose earlier observations are dropped), and
/// export.
#[test]
fn auto_retained_state_holds_no_credential_or_account_identity() {
    const TOKEN: &str = "SENTINEL-ACCESS-TOKEN-7f3a";
    const KEY: &str = "sk-SENTINEL-KEY-7f3a";
    const REFRESH: &str = "CLAUDE-SENTINEL-REFRESH-7f3a";
    const ACCOUNT: &str = "acct-private-alpha-7f3a";
    const SWITCHED: &str = "acct-private-beta-7f3a";
    let r = tmp();
    let checkout = repo(&r.path().join("repo"));
    let accounts = r.path().join("account-ids");
    let meters = r.path().join("quota-modes");
    std::fs::create_dir_all(&accounts).unwrap();
    std::fs::create_dir_all(&meters).unwrap();
    let mut d = Daemon::start(&[
        ("OVERSEER_CODEX_PATH", fixture("fake-harness/codex-app-fixture.js").as_str()),
        ("OVERSEER_CLAUDE_PATH", fixture("fake-harness/claude-fixture.js").as_str()),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH",
            "FIXTURE_MODE,CLAUDE_FIXTURE_MODE,FIXTURE_ACCOUNT_IDS_DIR,FIXTURE_QUOTA_MODES_DIR"),
        ("FIXTURE_MODE", "managed-models"), ("CLAUDE_FIXTURE_MODE", "prose"),
        ("FIXTURE_ACCOUNT_IDS_DIR", accounts.to_str().unwrap()),
        ("FIXTURE_QUOTA_MODES_DIR", meters.to_str().unwrap()),
    ]);
    d.call("auto.mode.set", json!({"enabled":true}));
    let codex = d.call("profile.create", json!({"name":"alpha","harness":"codex"}));
    let codex_id = codex["id"].as_str().unwrap().to_string();
    let codex_home = PathBuf::from(codex["home"].as_str().unwrap());
    std::fs::write(accounts.join(&codex_id), ACCOUNT).unwrap();
    std::fs::write(meters.join(&codex_id), "available").unwrap();
    std::fs::create_dir_all(codex_home.join("codex")).unwrap();
    std::fs::write(codex_home.join("codex/auth.json"), json!({"OPENAI_API_KEY":KEY,
        "tokens":{"access_token":TOKEN,"refresh_token":TOKEN}}).to_string()).unwrap();
    let claude = d.call("profile.create", json!({"name":"claude-alpha","harness":"claude"}));
    let claude_id = claude["id"].as_str().unwrap().to_string();
    let claude_home = PathBuf::from(claude["home"].as_str().unwrap());
    std::fs::create_dir_all(claude_home.join("claude")).unwrap();
    std::fs::write(claude_home.join("claude/.credentials.json"), json!({"claudeAiOauth":{
        "accessToken":TOKEN,"refreshToken":REFRESH}}).to_string()).unwrap();

    let parent = run_id(&d.call("task.create", json!({"repo":checkout,"harness":"codex-app",
        "model":"gpt-6-astra","effort":"high","prompt":"seed context","approval_policy":"never"})));
    assert_eq!(d.wait_done(&parent, 15)["status"], "completed");
    d.call("auto.quota.refresh", json!({"profile_id":codex_id}));
    for (unit, profile) in [("codex-child-1", &codex_id), ("claude-child-2", &claude_id)] {
        let child = d.call("auto.dispatch", json!({"work_unit_id":unit,"parent_run_id":parent,
            "min_tier":"general","required_tools":[],"allowed_profiles":[profile],
            "task_class":"general","prompt":"summarize","title":"summary"}));
        assert_eq!(child["state"], "dispatched", "{child}");
        assert_eq!(d.wait_done(&run_id(&child), 20)["status"], "completed");
    }
    let handoff = d.call("run.handoff", json!({"source_run_id":parent,"harness":"codex-app",
        "model":"gpt-6-sol","effort":"medium","handoff":{"corrections":["keep the file"],
        "completed":["seeded"],"remaining":["check"],"tests":["none"],"limitations":["none"],
        "unresolved_actions":[]}}));
    assert_eq!(d.wait_done(&run_id(&handoff), 20)["status"], "completed");
    // Account change: the earlier account's observations are dropped.
    std::fs::write(accounts.join(&codex_id), SWITCHED).unwrap();
    d.call("auto.quota.refresh", json!({"profile_id":codex_id}));
    let generations: Vec<i64> = {
        let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
        let generation: i64 = db.query_row("SELECT generation FROM auto_account_identity WHERE profile_id=?1",
            [&codex_id], |row| row.get(0)).unwrap();
        let mut stmt = db.prepare("SELECT COUNT(*) FROM auto_quota_observations WHERE pool_id=?1").unwrap();
        vec![generation, stmt.query_row([&codex_id], |row| row.get(0)).unwrap()]
    };
    assert_eq!(generations, vec![2, 1], "a new generation keeps only its own reading");
    let export = r.path().join("usage-export.json");
    d.call("auto.usage.export", json!({"path":export}));
    d.shutdown();

    let mut retained = Vec::new();
    files_under(d.home.path(), &mut retained);
    retained.retain(|file| !file.starts_with(&codex_home) && !file.starts_with(&claude_home));
    retained.push(export);
    assert!(retained.iter().any(|f| f.ends_with("overseer.sqlite")), "{retained:?}");
    for needle in [TOKEN, KEY, REFRESH] {
        assert_eq!(containing(&retained, needle), Vec::<String>::new(), "credential {needle} retained");
    }
    // Identities: neither the daemon's records nor the run transcripts name
    // the account (the supervisor records the daemon's own account reads
    // with a fingerprint in place of the raw id, and without the e-mail or
    // credit balance).
    for needle in ["private@example.invalid", "fixture@example.test", "fixture-org", ACCOUNT, SWITCHED,
        "secret-credit-sentinel"] {
        assert_eq!(containing(&retained, needle), Vec::<String>::new(), "identity {needle} retained");
    }
}
