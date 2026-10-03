//! AC-269 Gate S: authenticated native reads are projections, never management authority.
//! RED fixtures only so far; no new production code until an observed assertion RED.
mod common;
use common::*;
use serde_json::{json, Value};
use std::path::Path;
use std::time::{Duration, Instant};

fn db(d: &Daemon) -> rusqlite::Connection {
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    db.busy_timeout(Duration::from_secs(5)).unwrap();
    db
}

// Tool-call events are allowed. These private authority/configuration rows and
// admissions must stay byte-identical across a Look read or rejected request.
fn authority_snapshot(d: &Daemon) -> Value {
    let db = db(d);
    let tables = [
        "mod_versions",
        "mod_bindings",
        "mod_previews",
        "overseer_sessions",
        "overseer_proposals",
        "turns",
    ];
    let mut snapshot = serde_json::Map::new();
    for table in tables {
        let mut statement = db
            .prepare(&format!("SELECT * FROM {table} ORDER BY rowid"))
            .unwrap();
        let columns = statement.column_count();
        let rows: Vec<Vec<String>> = statement
            .query_map([], |row| {
                (0..columns)
                    .map(|i| Ok(format!("{:?}", row.get_ref(i)?)))
                    .collect()
            })
            .unwrap()
            .map(Result::unwrap)
            .collect();
        snapshot.insert(table.into(), json!(rows));
    }
    let revision: String = db
        .query_row(
            "SELECT value FROM meta WHERE key='mods_revision'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    snapshot.insert("mods_revision".into(), json!(revision));
    json!(snapshot)
}

fn run(d: &Daemon, repo: &Path, title: &str) -> String {
    let created = d.call("task.create", json!({"repo":repo,"harness":"generic", "workspace_mode":"worktree", "program":"/usr/bin/true", "args":[], "prompt":"Synthetic read fixture.", "title":title}));
    let id = run_id(&created);
    d.wait_done(&id, 20);
    id
}

fn token(d: &Daemon, run: &str, role: &str) -> String {
    d.call("overseer.token", json!({"run_id":run,"role":role}))["token"]
        .as_str()
        .unwrap()
        .into()
}

fn wait_own_done(d: &Daemon, run: &str) {
    // Ordinary state hides Overseer's run; use durable status without forging
    // run visibility or borrowing the ordinary helper's state assumption.
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let status: String = db(d)
            .query_row("SELECT status FROM runs WHERE id=?1", [run], |r| r.get(0))
            .unwrap();
        if !["queued", "starting", "running", "waiting_for_user"].contains(&status.as_str()) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "fixture Overseer did not finish: {status}"
        );
        std::thread::sleep(Duration::from_millis(30));
    }
}

fn tool(d: &Daemon, token: &str, args: Value) -> Value {
    let result = d.try_call(
        "overseer.tool",
        json!({"token":token,"name":"mods","arguments":args}),
    );
    assert!(
        result.is_ok(),
        "authenticated Mods read was unavailable: {result:?}"
    );
    let reply = result.unwrap();
    assert_eq!(reply["is_error"], false, "Mods read refused: {reply}");
    serde_json::from_str(reply["text"].as_str().unwrap())
        .expect("successful native Mods reads return JSON metadata")
}

fn refuses(d: &Daemon, token: &str, name: &str, args: Value) {
    let result = d.try_call(
        "overseer.tool",
        json!({"token":token,"name":name,"arguments":args}),
    );
    match result {
        Err(error) => assert!(!error.is_empty()),
        Ok(reply) => assert_eq!(
            reply["is_error"], true,
            "native request acquired authority: {reply}"
        ),
    }
}

fn install_local(d: &Daemon, folder: &Path, id: &str, private: &str) -> Value {
    std::fs::create_dir_all(folder).unwrap();
    std::fs::write(folder.join("mod.toml"), format!("schema_version=1\nid='{id}'\nname='{id}'\nversion='1'\nsummary='Synthetic metadata'\nsource='local'\n[rules]\nfiles=['rules.md']\n")).unwrap();
    std::fs::write(folder.join("rules.md"), private).unwrap();
    let preview = d.call(
        "mods.preview",
        json!({"source":folder,"operation":"install"}),
    );
    d.call(
        "mods.install",
        json!({"preview_id":preview["id"],"confirm":true}),
    )["version"]
        .clone()
}

fn bind(d: &Daemon, version: &Value, run: &str) -> String {
    let current = d.call("mods.list", json!({}));
    let saved = d.call("mods.bind", json!({"expected_revision":current["revision"],"binding":{"mod_id":version["id"],"version":version["version"],"fingerprint":version["fingerprint"],"scope":{"kind":"agent","run_id":run},"enabled":true,"required":false,"locked":false,"filters":{"harnesses":[],"accounts":[],"models":[]}}}));
    saved["binding"]["id"].as_str().unwrap().into()
}

fn has_private_text(value: &Value) -> bool {
    match value {
        Value::Object(fields) => fields.iter().any(|(key, value)| {
            [
                "rules_text",
                "style_text",
                "text",
                "contents",
                "binding_snapshot",
            ]
            .contains(&key.as_str())
                || has_private_text(value)
        }),
        Value::Array(values) => values.iter().any(has_private_text),
        _ => false,
    }
}

#[test]
fn native_overseer_reads_list_and_delivery_without_private_text_or_mutation() {
    // A real shared Overseer session/run with an inert local fixture; no fake
    // session cause, stored origin or fabricated caller ID is inserted.
    let t = tmp();
    let mode = t.path().join("mode");
    std::fs::write(&mode, "overseer").unwrap();
    let fixture = repo_root().join("fixtures/fake-harness/claude-fixture.js");
    let d = Daemon::start(&[
        ("OVERSEER_TEST_AUTO_DISABLED", "1"),
        ("OVERSEER_CLAUDE_PATH", fixture.to_str().unwrap()),
        ("CLAUDE_FIXTURE_MODE_FILE", mode.to_str().unwrap()),
        (
            "OVERSEER_HARNESS_ENV_PASSTHROUGH",
            "CLAUDE_FIXTURE_MODE_FILE",
        ),
    ]);
    d.call("agent.cadence", json!({"cadence":"off","by":"owner"}));
    d.call(
        "overseer.send",
        json!({"text":"Report synthetic state only.","surface":"ctl","harness":"claude"}),
    );
    let session = d.call("overseer.session", json!({}));
    let own = session["run_id"].as_str().unwrap().to_string();
    wait_own_done(&d, &own);
    let version = install_local(
        &d,
        &t.path().join("source"),
        "read-own",
        "PRIVATE_BODY_NO_NATIVE_READ",
    );
    let current = d.call("mods.list", json!({}));
    d.call("mods.bind", json!({"expected_revision":current["revision"],"binding":{"mod_id":version["id"],"version":version["version"],"fingerprint":version["fingerprint"],"scope":{"kind":"overseer"},"enabled":true}}));
    let token = token(&d, &own, "overseer");
    let before = authority_snapshot(&d);
    let list = tool(&d, &token, json!({"operation":"list"}));
    assert!(list["installed"]
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v["id"] == "read-own"));
    let why = tool(&d, &token, json!({"operation":"why","run_id":own}));
    assert_eq!(why["context"]["run_id"], own);
    assert_eq!(
        why["pending"], true,
        "binding enabled after last turn stays pending"
    );
    assert_eq!(
        why["desired"]["versions"][0]["fingerprint"],
        version["fingerprint"]
    );
    assert!(why["support"]["installed_runtime_qualification"] != "verified");
    for read in [list, why] {
        assert!(!read.to_string().contains("PRIVATE_BODY_NO_NATIVE_READ"));
        assert!(
            !has_private_text(&read),
            "native metadata exposed text or private snapshot: {read}"
        );
    }
    assert_eq!(
        authority_snapshot(&d),
        before,
        "Look changed saved authority/configuration or admitted a turn"
    );
}

#[test]
fn ordinary_native_reads_project_self_and_hide_unrelated_library() {
    let d = Daemon::start(&[("OVERSEER_TEST_AUTO_DISABLED", "1")]);
    let t = tmp();
    let repo = repo(&t.path().join("repo"));
    let own = run(&d, &repo, "Own synthetic task");
    let other = run(&d, &repo, "Unrelated synthetic task");
    let visible = install_local(
        &d,
        &t.path().join("visible-source"),
        "read-self",
        "PRIVATE_SELF_BODY",
    );
    let hidden_source = t.path().join("UNRELATED_SOURCE_FOLDER");
    let hidden = install_local(
        &d,
        &hidden_source,
        "unrelated-private-mod",
        "PRIVATE_UNRELATED_BODY",
    );
    let mine = bind(&d, &visible, &own);
    let theirs = bind(&d, &hidden, &other);
    // Populate an actual immutable last-turn snapshot with both private text
    // and out-of-scope decisions; the native projection must hide those too.
    d.call(
        "run.follow_up",
        json!({"run_id":own,"prompt":"Continue the synthetic task."}),
    );
    d.wait_done(&own, 20);
    let token = token(&d, &own, "agent");
    let before = authority_snapshot(&d);
    let list = tool(&d, &token, json!({"operation":"list"}));
    assert!(list["installed"]
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v["id"] == "read-self"));
    assert!(list["bindings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|b| b["id"] == mine));
    let why = tool(&d, &token, json!({"operation":"why","run_id":own}));
    assert_eq!(why["context"]["run_id"], own);
    assert_eq!(
        why["desired"]["versions"][0]["fingerprint"],
        visible["fingerprint"]
    );
    for read in [list, why] {
        let serialized = read.to_string();
        for private in [
            theirs.as_str(),
            "unrelated-private-mod",
            "UNRELATED_SOURCE_FOLDER",
            "PRIVATE_SELF_BODY",
            "PRIVATE_UNRELATED_BODY",
        ] {
            assert!(
                !serialized.contains(private),
                "self read disclosed unrelated/private data: {private}"
            );
        }
        assert!(!has_private_text(&read));
    }
    refuses(
        &d,
        &token,
        "mods",
        json!({"operation":"why","run_id":other}),
    );
    assert_eq!(authority_snapshot(&d), before);
    let sessions: i64 = db(&d)
        .query_row("SELECT count(*) FROM overseer_sessions", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        sessions, 0,
        "native self reads must not create an Overseer session"
    );
}

#[test]
fn existing_agent_watch_membership_changes_native_read_target_then_restores_self() {
    let d = Daemon::start(&[("OVERSEER_TEST_AUTO_DISABLED", "1")]);
    let t = tmp();
    let repo = repo(&t.path().join("repo"));
    let watcher = run(&d, &repo, "Named watcher");
    let subject = run(&d, &repo, "Actual subject");
    let unrelated = run(&d, &repo, "Other subject");
    let watched = install_local(
        &d,
        &t.path().join("watched-source"),
        "subject-visible",
        "PRIVATE_SUBJECT_BODY",
    );
    let own = install_local(
        &d,
        &t.path().join("WATCHER_OWN_UNRELATED_SOURCE"),
        "watcher-own-hidden",
        "PRIVATE_WATCHER_BODY",
    );
    bind(&d, &watched, &subject);
    bind(&d, &own, &watcher);
    let token = token(&d, &watcher, "agent");
    let watch = d.call(
        "watch.start",
        json!({"subject":subject,"watcher":watcher,"brief":"Read the subject only.","by":"owner"}),
    );
    let before = authority_snapshot(&d);
    let list = tool(&d, &token, json!({"operation":"list"}));
    assert!(list.to_string().contains("subject-visible"));
    assert!(!list.to_string().contains("watcher-own-hidden"));
    assert!(!list.to_string().contains("WATCHER_OWN_UNRELATED_SOURCE"));
    assert!(!has_private_text(&list));
    let why = tool(&d, &token, json!({"operation":"why","run_id":subject}));
    assert_eq!(why["context"]["run_id"], subject);
    refuses(
        &d,
        &token,
        "mods",
        json!({"operation":"why","run_id":watcher}),
    );
    refuses(
        &d,
        &token,
        "mods",
        json!({"operation":"why","run_id":unrelated}),
    );
    refuses(
        &d,
        &token,
        "propose",
        json!({"actions":[{"action":"mod","operation":"unbind","binding_id":"not-owner-authority"}]}),
    );
    assert_eq!(authority_snapshot(&d), before);
    d.call("watch.end", json!({"id":watch["id"],"by":"owner"}));
    let restored = tool(&d, &token, json!({"operation":"why","run_id":watcher}));
    assert_eq!(restored["context"]["run_id"], watcher);
    refuses(
        &d,
        &token,
        "mods",
        json!({"operation":"why","run_id":subject}),
    );
    let role: String = db(&d)
        .query_row(
            "SELECT COALESCE((SELECT role FROM run_roles WHERE run_id=?1),'agent')",
            [&watcher],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        role, "agent",
        "watch read capability must not change security role"
    );
}

#[test]
fn native_mods_read_rejects_authority_fields_and_every_mutation_operation() {
    let d = Daemon::start(&[("OVERSEER_TEST_AUTO_DISABLED", "1")]);
    let t = tmp();
    let repo = repo(&t.path().join("repo"));
    let own = run(&d, &repo, "Read authority boundary");
    let token = token(&d, &own, "agent");
    let before = authority_snapshot(&d);
    tool(&d, &token, json!({"operation":"list"}));
    let offered = d.call("overseer.tools", json!({"token":token}));
    assert!(
        !offered["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["name"].as_str().unwrap().starts_with("mods.")),
        "native tools must expose the governed read tool only"
    );
    for field in [
        "class",
        "confirm",
        "role",
        "actor",
        "env",
        "binding",
        "expected_revision",
        "preview_id",
        "command_ref",
        "unknown",
    ] {
        let mut args = json!({"operation":"list"});
        args[field] = json!(true);
        refuses(&d, &token, "mods", args);
    }
    for operation in [
        "bind", "unbind", "preview", "install", "update", "remove", "lock", "execute", "unknown",
        "",
    ] {
        refuses(&d, &token, "mods", json!({"operation":operation}));
    }
    for name in [
        "mods.bind",
        "mods.unbind",
        "mods.preview",
        "mods.install",
        "mods.update",
        "mods.remove",
    ] {
        refuses(
            &d,
            &token,
            name,
            json!({"confirm":true,"role":"overseer","class":"look"}),
        );
    }
    refuses(&d, &token, "mods", json!({}));
    refuses(&d, &token, "mods", json!({"operation":"why"}));
    refuses(
        &d,
        &token,
        "mods",
        json!({"operation":"list","run_id":"other"}),
    );
    assert_eq!(authority_snapshot(&d), before);
}

#[test]
fn watcher_without_active_subject_has_no_native_mods_read_fallback() {
    let d = Daemon::start(&[("OVERSEER_TEST_AUTO_DISABLED", "1")]);
    let t = tmp();
    let repo = repo(&t.path().join("repo"));
    let created = d.call("task.create", json!({"repo":repo,"harness":"generic","workspace_mode":"worktree","program":"/usr/bin/true","args":[],"prompt":"Synthetic watcher role.","title":"Unassigned watcher","role":"watcher"}));
    let run = run_id(&created);
    d.wait_done(&run, 20);
    let token = token(&d, &run, "watcher");
    // Tool availability does not supply a subject or mutation authority.
    let tools = d.call("overseer.tools", json!({"token":token}));
    assert!(
        tools["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["name"] == "mods"),
        "watcher metadata tool missing: {tools}"
    );
    let before = authority_snapshot(&d);
    refuses(&d, &token, "mods", json!({"operation":"list"}));
    refuses(&d, &token, "mods", json!({"operation":"why","run_id":run}));
    assert_eq!(authority_snapshot(&d), before);
}

#[test]
fn unknown_native_token_cannot_read_even_when_arguments_claim_owner() {
    let d = Daemon::start(&[("OVERSEER_TEST_AUTO_DISABLED", "1")]);
    let t = tmp();
    let repo = repo(&t.path().join("repo"));
    let own = run(&d, &repo, "Real reader");
    let good = token(&d, &own, "agent");
    tool(&d, &good, json!({"operation":"list"}));
    let before = authority_snapshot(&d);
    refuses(
        &d,
        "unrecognized-token",
        "mods",
        json!({"operation":"list","role":"overseer","actor":"owner"}),
    );
    assert_eq!(authority_snapshot(&d), before);
}

#[test]
fn daemon_created_watcher_reads_only_its_actual_subject() {
    let t = tmp();
    let repo = repo(&t.path().join("repo"));
    let mode = t.path().join("mode");
    std::fs::write(&mode, "slow").unwrap();
    let fixture = repo_root().join("fixtures/fake-harness/claude-fixture.js");
    let d = Daemon::start(&[
        ("OVERSEER_TEST_AUTO_DISABLED", "1"),
        ("OVERSEER_CLAUDE_PATH", fixture.to_str().unwrap()),
        ("CLAUDE_FIXTURE_MODE_FILE", mode.to_str().unwrap()),
        (
            "OVERSEER_HARNESS_ENV_PASSTHROUGH",
            "CLAUDE_FIXTURE_MODE_FILE",
        ),
    ]);
    d.call("agent.cadence", json!({"cadence":"off","by":"owner"}));
    // A generic quiet exit emits status, not an immediate turn_done wake.
    // Use the established local Claude fixture's genuine completion event.
    let created = d.call("task.create", json!({"repo":repo,"harness":"claude","prompt":"Synthetic watched turn.","title":"Actual watched subject"}));
    let subject = run_id(&created);
    d.wait_status(&subject, |status| status == "running", 20);
    let version = install_local(
        &d,
        &t.path().join("subject-source"),
        "auto-subject-visible",
        "PRIVATE_AUTOWATCH_SUBJECT",
    );
    bind(&d, &version, &subject);
    let watch = d.call("watch.start", json!({"subject":subject,"brief":"Inspect this subject only.","harness":"claude","by":"owner"}));
    assert!(
        watch["watcher"].is_null(),
        "the daemon creates the watcher on the genuine first wake"
    );
    std::fs::write(&mode, "watcher").unwrap();
    d.wait_done(&subject, 20);
    let deadline = Instant::now() + Duration::from_secs(30);
    let watcher = loop {
        let saved = d.call("watch.list", json!({"run_id":subject}));
        if let Some(run) = saved["watches"]
            .as_array()
            .unwrap()
            .iter()
            .find(|w| w["id"] == watch["id"])
            .and_then(|w| w["watcher"].as_str())
            .filter(|run| !run.is_empty())
        {
            break run.to_string();
        }
        assert!(
            Instant::now() < deadline,
            "daemon did not create an actual watcher: {saved}"
        );
        std::thread::sleep(Duration::from_millis(30));
    };
    d.wait_done(&watcher, 30);
    let role: String = db(&d)
        .query_row(
            "SELECT role FROM run_roles WHERE run_id=?1",
            [&watcher],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(role, "watcher");
    let token = token(&d, &watcher, "watcher");
    let before = d.call("mods.list", json!({}));
    let list = tool(&d, &token, json!({"operation":"list"}));
    assert!(list.to_string().contains("auto-subject-visible"));
    assert!(!has_private_text(&list));
    let why = tool(&d, &token, json!({"operation":"why","run_id":subject}));
    assert_eq!(why["context"]["run_id"], subject);
    assert!(!why.to_string().contains("PRIVATE_AUTOWATCH_SUBJECT"));
    assert!(!has_private_text(&why));
    refuses(
        &d,
        &token,
        "mods",
        json!({"operation":"why","run_id":watcher}),
    );
    refuses(
        &d,
        &token,
        "mods",
        json!({"operation":"why","run_id":"missing-subject"}),
    );
    refuses(
        &d,
        &token,
        "propose",
        json!({"actions":[{"action":"mod","operation":"bind","role":"overseer"}]}),
    );
    assert_eq!(d.call("mods.list", json!({})), before);
}
