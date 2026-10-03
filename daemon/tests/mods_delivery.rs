mod common;
use common::*;
use serde_json::{json, Value};
use std::{
    path::Path,
    time::{Duration, Instant},
};

fn install(d: &Daemon) -> Value {
    let preview = d.call(
        "mods.preview",
        json!({"source":"bundled:clear-prose","operation":"install"}),
    );
    d.call(
        "mods.install",
        json!({"preview_id":preview["id"],"confirm":true}),
    )["version"]
        .clone()
}
fn bind(d: &Daemon, version: &Value, kind: &str, id: Option<&Value>, enabled: bool) -> Value {
    let revision = d.call("mods.list", json!({}))["revision"].clone();
    d.call("mods.bind",json!({"expected_revision":revision,"binding":{
        "id":id,"mod_id":version["id"],"version":version["version"],"fingerprint":version["fingerprint"],
        "scope":{"kind":kind},"enabled":enabled}}))["binding"].clone()
}
fn captures(path: &Path, count: usize) -> Vec<Value> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let rows: Vec<Value> = std::fs::read_to_string(path)
            .unwrap_or_default()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        if rows.len() >= count {
            return rows;
        }
        assert!(
            Instant::now() < deadline,
            "fixture capture missing: {rows:?}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}
fn done(d: &Daemon, run: &str) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let state = d.call("state", json!({"include_hidden":true}));
        let saved = state["runs"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["id"] == run)
            .unwrap();
        if !["queued", "starting", "running", "waiting_for_user"]
            .contains(&saved["status"].as_str().unwrap())
        {
            assert_eq!(saved["status"], "completed", "{saved}");
            return;
        }
        assert!(
            Instant::now() < deadline,
            "fixture run did not complete: {saved}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}
fn create(d: &Daemon, repo: &Path, capture: &Path, prompt: &str) -> String {
    run_id(&d.call("task.create",json!({"repo":repo,"harness":"generic","workspace_mode":"worktree",
        "program":"node","args":[repo_root().join("fixtures/fake-harness/mods-cli.js"),capture],"prompt":prompt})))
}
fn rules() -> String {
    std::fs::read_to_string(repo_root().join("mods/clear-prose/style.md")).unwrap()
}

#[test]
fn no_mod_transport_is_unchanged_and_first_follow_up_snapshots_capture_full_text() {
    let d = Daemon::start(&[("OVERSEER_TEST_AUTO_DISABLED", "1")]);
    let t = tmp();
    let repo = repo(&t.path().join("repo"));
    let baseline_path = t.path().join("baseline.jsonl");
    let baseline = create(
        &d,
        &repo,
        &baseline_path,
        "Preserve the exact owner prompt.",
    );
    done(&d, &baseline);
    let saved = d.call("run.turns", json!({"run_id":baseline}));
    assert_eq!(
        captures(&baseline_path, 1)[0]["text"],
        format!("{}\n", saved[0]["prompt"].as_str().unwrap())
    );
    let version = install(&d);
    bind(&d, &version, "all_agents", None, true);
    let path = t.path().join("enabled.jsonl");
    let agent = create(&d, &repo, &path, "Keep the warning and its source.");
    done(&d, &agent);
    assert!(captures(&path, 1)[0]["text"]
        .as_str()
        .unwrap()
        .contains(&rules()));
    let why = d.call("mods.why", json!({"run_id":agent}));
    assert_eq!(why["last_turn"]["delivery"], "message_text");
    assert_eq!(why["last_turn"]["outcome"], "transport_accepted");
    assert_eq!(
        why["last_turn"]["plan"]["versions"][0]["fingerprint"],
        version["fingerprint"]
    );
    assert_eq!(why["pending"], false);
    d.call(
        "run.follow_up",
        json!({"run_id":agent,"prompt":"Retain that warning in the final answer."}),
    );
    done(&d, &agent);
    assert!(captures(&path, 2)[1]["text"]
        .as_str()
        .unwrap()
        .contains(&rules()));
    let second = d.call("mods.why", json!({"run_id":agent}));
    assert_ne!(why["last_turn"]["turn_id"], second["last_turn"]["turn_id"]);
    assert_eq!(second["last_turn"]["children"], "unknown");
    assert_eq!(second["support"]["global_text_suppression"], "unsupported");
}

#[test]
fn shared_overseer_session_delivers_separate_binding_on_first_and_subsequent_turns() {
    let t = tmp();
    let path = t.path().join("overseer.jsonl");
    let fixture = repo_root()
        .join("fixtures/fake-harness/mods-cli.js")
        .display()
        .to_string();
    let capture = path.display().to_string();
    let d = Daemon::start(&[
        ("OVERSEER_TEST_AUTO_DISABLED", "1"),
        ("OVERSEER_CLAUDE_PATH", &fixture),
        ("MODS_CAPTURE_FILE", &capture),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "MODS_CAPTURE_FILE"),
    ]);
    let version = install(&d);
    bind(&d, &version, "all_agents", None, true);
    let first = d.call(
        "overseer.send",
        json!({"text":"Explain the saved plan clearly.","harness":"claude","model":"fixture"}),
    );
    let id = first["run_id"].as_str().unwrap();
    done(&d, id);
    assert!(!captures(&path, 1)[0]["text"]
        .as_str()
        .unwrap()
        .contains(&rules()));
    let scoped = bind(&d, &version, "overseer", None, true);
    d.call("overseer.fresh", json!({}));
    let next=d.call("overseer.send",json!({"text":"Explain this plan as complete sentences.","harness":"claude","model":"fixture"}));
    let id = next["run_id"].as_str().unwrap();
    done(&d, id);
    assert!(captures(&path, 2)[1]["text"]
        .as_str()
        .unwrap()
        .contains(&rules()));
    assert_eq!(
        d.call("mods.why", json!({"run_id":id}))["last_turn"]["delivery"],
        "message_text"
    );
    d.call(
        "overseer.send",
        json!({"text":"Preserve every necessary fact in the follow-up."}),
    );
    done(&d, id);
    assert!(captures(&path, 3)[2]["text"]
        .as_str()
        .unwrap()
        .contains(&rules()));
    bind(&d, &version, "overseer", Some(&scoped["id"]), false);
    let pending = d.call("mods.why", json!({"run_id":id}));
    assert_eq!(pending["pending"], true);
    assert_eq!(
        pending["last_turn"]["plan"]["versions"][0]["fingerprint"],
        version["fingerprint"]
    );
    d.call(
        "overseer.send",
        json!({"text":"Keep the original facts after disabling the style."}),
    );
    done(&d, id);
    assert!(!captures(&path, 4)[3]["text"]
        .as_str()
        .unwrap()
        .contains(&rules()));
    assert_eq!(d.call("mods.why", json!({"run_id":id}))["pending"], false);
}

fn local_rules(d: &Daemon, folder: &Path, text: &str) -> Value {
    std::fs::create_dir_all(folder).unwrap();
    std::fs::write(folder.join("mod.toml"), "schema_version=1\nid='large-rules'\nname='Bounded rules'\nversion='1'\nsummary='Delivery limit fixture'\nsource='local'\n[rules]\nfiles=['rules.md']\n").unwrap();
    std::fs::write(folder.join("rules.md"), text).unwrap();
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

#[test]
fn live_stdin_changes_next_turn_only_and_removal_keeps_saved_history() {
    let d = Daemon::start(&[("OVERSEER_TEST_AUTO_DISABLED", "1")]);
    let t = tmp();
    let repo = repo(&t.path().join("repo"));
    let path = t.path().join("live.jsonl");
    let version = install(&d);
    let binding = bind(&d, &version, "all_agents", None, true);
    let id = run_id(&d.call("task.create",json!({"repo":repo,"harness":"generic","workspace_mode":"worktree",
        "program":"node","args":[repo_root().join("fixtures/fake-harness/mods-cli.js"),path,"--stay"],"prompt":"Keep this original turn."})));
    assert!(captures(&path, 1)[0]["text"]
        .as_str()
        .unwrap()
        .contains(&rules()));
    let first = d.call("mods.why", json!({"run_id":id}));
    bind(&d, &version, "overseer", None, true);
    assert_eq!(
        d.call("mods.why", json!({"run_id":id}))["pending"],
        false,
        "An unrelated scope revision is not pending on this agent"
    );
    bind(&d, &version, "all_agents", Some(&binding["id"]), false);
    let changed = d.call("mods.why", json!({"run_id":id}));
    assert_eq!(changed["pending"], true);
    assert_eq!(
        changed["last_turn"], first["last_turn"],
        "Mid-turn settings do not rewrite the snapshot"
    );
    d.call(
        "run.follow_up",
        json!({"run_id":id,"prompt":"Only the new turn uses disabled guidance."}),
    );
    assert!(!captures(&path, 2)[1]["text"]
        .as_str()
        .unwrap()
        .contains(&rules()));
    assert_eq!(d.call("mods.why", json!({"run_id":id}))["pending"], false);
    let removed = d.call(
        "mods.remove",
        json!({"mod_id":version["id"],"fingerprint":version["fingerprint"],"confirm":true,
        "expected_revision":d.call("mods.list",json!({}))["revision"]}),
    );
    assert_eq!(removed["removed"], true, "{removed}");
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let copied: String = db
        .query_row(
            "SELECT content FROM turn_mods WHERE turn_id=?1",
            [first["last_turn"]["turn_id"].as_str().unwrap()],
            |r| r.get(0),
        )
        .unwrap();
    assert!(serde_json::from_str::<Value>(&copied).unwrap()["text"]
        .as_str()
        .unwrap()
        .contains(&rules()));
    assert!(d.call("run.turns", json!({"run_id":id}))[0]["prompt"]
        .as_str()
        .unwrap()
        .contains(&rules()));
    d.call("run.interrupt", json!({"run_id":id}));
}

#[test]
fn oversize_optional_text_is_bypassed_whole_and_required_text_refuses_before_spawn() {
    let d = Daemon::start(&[("OVERSEER_TEST_AUTO_DISABLED", "1")]);
    let t = tmp();
    let repo = repo(&t.path().join("repo"));
    let full = format!(
        "Important opening.\n{}\nImportant final evidence.",
        "x".repeat(17 * 1024)
    );
    let version = local_rules(&d, &t.path().join("large"), &full);
    let scoped = bind(&d, &version, "all_agents", None, true);
    let path = t.path().join("optional.jsonl");
    let id = create(&d, &repo, &path, "Preserve the real request.");
    done(&d, &id);
    let captured = captures(&path, 1);
    let text = captured[0]["text"].as_str().unwrap();
    assert!(!text.contains("Important opening."));
    assert!(!text.contains("Important final evidence."));
    let why = d.call("mods.why", json!({"run_id":id}));
    assert_eq!(why["last_turn"]["delivery"], "unsupported");
    assert_eq!(why["last_turn"]["added_bytes"], 0);
    assert!(why["last_turn"]["plan"]["rules_text"]
        .as_str()
        .unwrap()
        .contains(&full));
    assert!(why["desired"]["decisions"][0]["reason"]
        .as_str()
        .unwrap()
        .contains("without truncation"));
    let mut required = scoped;
    required.as_object_mut().unwrap().remove("actor");
    required.as_object_mut().unwrap().remove("changed_ms");
    required["required"] = json!(true);
    d.call(
        "mods.bind",
        json!({"expected_revision":d.call("mods.list",json!({}))["revision"],"binding":required}),
    );
    let blocked = t.path().join("required.jsonl");
    let result=d.call("task.create",json!({"repo":repo,"harness":"generic","workspace_mode":"worktree","program":"node",
        "args":[repo_root().join("fixtures/fake-harness/mods-cli.js"),blocked],"prompt":"Must retain complete guidance."}));
    assert!(
        result["launch_error"]
            .as_str()
            .unwrap()
            .contains("Required mod text cannot be delivered"),
        "{result}"
    );
    assert_eq!(result["run"]["status"], "failed");
    assert!(!blocked.exists());
    assert!(d
        .call("run.turns", json!({"run_id":result["run"]["id"]}))
        .as_array()
        .unwrap()
        .is_empty());
    // Inspection remains available for required unsupported guidance.
    let why = d.call("mods.why", json!({"run_id":result["run"]["id"]}));
    assert_eq!(why["desired"]["decisions"][0]["delivery"], "unsupported");
}

#[test]
fn pre_effect_failure_records_unknown_consumption_and_legacy_latest_turn_is_unknown() {
    let d = Daemon::start(&[("OVERSEER_TEST_AUTO_DISABLED", "1")]);
    let t = tmp();
    let repo = repo(&t.path().join("repo"));
    let path = t.path().join("history.jsonl");
    let version = install(&d);
    bind(&d, &version, "all_agents", None, true);
    let id = create(&d, &repo, &path, "Preserve this completed turn.");
    done(&d, &id);
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let launch: String = db
        .query_row("SELECT launch FROM runs WHERE id=?1", [&id], |r| r.get(0))
        .unwrap();
    let mut launch: Value = serde_json::from_str(&launch).unwrap();
    launch["generic"]["extra_args"] = json!(["--api-key=synthetic-fixture"]);
    db.execute(
        "UPDATE runs SET launch=?2 WHERE id=?1",
        rusqlite::params![id, launch.to_string()],
    )
    .unwrap();
    assert!(d
        .try_call(
            "run.follow_up",
            json!({"run_id":id,"prompt":"This adapter validation must fail."})
        )
        .is_err());
    let why = d.call("mods.why", json!({"run_id":id}));
    assert_eq!(why["last_turn"]["outcome"], "failed_before_effect");
    assert!(
        why["last_turn"]["applied_fingerprints"]
            .as_array()
            .unwrap()
            .is_empty(),
        "Pre-effect failure cannot advertise applied Mods"
    );
    assert_eq!(why["pending"], true);
    assert_eq!(
        captures(&path, 1).len(),
        1,
        "Refused input must not reach the fixture"
    );
    let turns = d.call("run.turns", json!({"run_id":id}));
    assert_eq!(turns[1]["status"], "failed");
    // A historical turn with no recorded snapshot cannot inherit the preceding turn's proof.
    db.execute(
        "DELETE FROM turn_mods WHERE turn_id=?1",
        [turns[1]["id"].as_str().unwrap()],
    )
    .unwrap();
    assert!(d.call("mods.why", json!({"run_id":id}))["last_turn"].is_null());
}

#[test]
fn retry_after_restart_and_removal_reuses_original_snapshot_without_duplicate_text() {
    let t = tmp();
    let repo = repo(&t.path().join("repo"));
    let path = t.path().join("retry.jsonl");
    let net = t.path().join("net.json");
    let memory = t.path().join("memory.json");
    std::fs::write(&net,r#"{"system":"connected","baseline":{"by_name":true,"by_ip":true},"providers":{"openai":true,"anthropic":true}}"#).unwrap();
    std::fs::write(
        &memory,
        r#"{"total":137438953472,"available":123695058124,"pressure":"normal"}"#,
    )
    .unwrap();
    let mut d = Daemon::start(&[
        ("OVERSEER_TEST_AUTO_DISABLED", "1"),
        ("OVERSEER_TEST_NET", net.to_str().unwrap()),
        ("OVERSEER_TEST_MEMORY", memory.to_str().unwrap()),
        ("OVERSEER_TEST_CONTINUITY_TICK_MS", "600000"),
    ]);
    let version = install(&d);
    bind(&d, &version, "all_agents", None, true);
    let id = create(
        &d,
        &repo,
        &path,
        "Never duplicate optional guidance on retry.",
    );
    done(&d, &id);
    let first = d.call("mods.why", json!({"run_id":id}))["last_turn"].clone();
    d.call(
        "mods.remove",
        json!({"mod_id":version["id"],"fingerprint":version["fingerprint"],"confirm":true,
        "expected_revision":d.call("mods.list",json!({}))["revision"]}),
    );
    d.kill9();
    d.spawn();
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let ms = first["outcome_ms"].as_i64().unwrap();
    db.execute(
        "UPDATE runs SET status='waiting_for_memory',ended_ms=NULL WHERE id=?1",
        [&id],
    )
    .unwrap();
    db.execute(
        "UPDATE turns SET status='waiting',ended_ms=NULL WHERE id=?1",
        [first["turn_id"].as_str().unwrap()],
    )
    .unwrap();
    db.execute("INSERT INTO continuity_waits(run_id,turn_id,kind,provider,reason,started_ms,next_ms,scheduled_ms) VALUES(?1,?2,'memory','generic','synthetic fixture wait',?3,9223372036854775807,?3)",rusqlite::params![id,first["turn_id"].as_str().unwrap(),ms]).unwrap();
    d.call("run.retry_now", json!({"run_id":id}));
    let input = captures(&path, 2);
    done(&d, &id);
    assert_eq!(input[0]["text"], input[1]["text"]);
    assert_eq!(
        input[1]["text"]
            .as_str()
            .unwrap()
            .matches("[Optional Overseer Mods: text guidance]")
            .count(),
        1
    );
    let last = d.call("mods.why", json!({"run_id":id}));
    assert_eq!(last["last_turn"]["turn_id"], first["turn_id"]);
    assert_eq!(last["last_turn"]["plan"], first["plan"]);
    assert_eq!(last["last_turn"]["digest"], first["digest"]);
    assert_eq!(
        last["pending"], true,
        "Removed guidance remains in the retry; next new turn uses desired settings"
    );
    assert_eq!(
        d.call("run.turns", json!({"run_id":id}))
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn failed_live_stdin_attempt_is_reported_uncertain_and_preserves_open_occupancy() {
    let d = Daemon::start(&[("OVERSEER_TEST_AUTO_DISABLED", "1")]);
    let t = tmp();
    let repo = repo(&t.path().join("repo"));
    let path = t.path().join("uncertain.jsonl");
    let version = install(&d);
    bind(&d, &version, "all_agents", None, true);
    let id=run_id(&d.call("task.create",json!({"repo":repo,"harness":"generic","workspace_mode":"worktree","program":"node",
        "args":[repo_root().join("fixtures/fake-harness/mods-cli.js"),path,"--stay"],"prompt":"Start a live fixture."})));
    captures(&path, 1);
    let launch_path = d.home.path().join("runs").join(&id).join("p1/launch.json");
    let original = std::fs::read(&launch_path).unwrap();
    let mut launch: Value = serde_json::from_slice(&original).unwrap();
    launch["control_socket"] = json!(t.path().join("missing-control.sock"));
    std::fs::write(&launch_path, launch.to_string()).unwrap();
    let refused = d.try_call(
        "run.follow_up",
        json!({"run_id":id,"prompt":"A failed control attempt cannot prove delivery."}),
    );
    std::fs::write(&launch_path, &original).unwrap();
    assert!(refused.is_err());
    let why = d.call("mods.why", json!({"run_id":id}));
    assert_eq!(why["last_turn"]["outcome"], "uncertain_after_effect");
    assert!(
        why["last_turn"]["applied_fingerprints"]
            .as_array()
            .unwrap()
            .is_empty(),
        "An uncertain attempt cannot advertise confirmed applied Mods"
    );
    assert_eq!(why["pending"], true);
    let turns = d.call("run.turns", json!({"run_id":id}));
    assert!(turns[1]["ended_ms"].is_null());
    assert_eq!(turns[1]["status"], "running");
    d.call("run.interrupt", json!({"run_id":id}));
}

#[test]
fn snapshot_admission_is_atomic_and_held_turn_uses_settings_at_release() {
    let d = Daemon::start(&[("OVERSEER_TEST_AUTO_DISABLED", "1")]);
    let t = tmp();
    let repo = repo(&t.path().join("repo"));
    let path = t.path().join("held.jsonl");
    let id = create(&d, &repo, &path, "A turn with no enabled mods.");
    done(&d, &id);
    d.call(
        "agent.hold",
        json!({"run_id":id,"reason":"Keep the saved turn intact","by":"owner"}),
    );
    let version = install(&d);
    bind(&d, &version, "all_agents", None, true);
    assert!(d
        .try_call(
            "run.follow_up",
            json!({"run_id":id,"prompt":"Held work must not launch."})
        )
        .is_err());
    assert_eq!(
        d.call("run.turns", json!({"run_id":id}))
            .as_array()
            .unwrap()
            .len(),
        1
    );
    d.call(
        "run.follow_up",
        json!({"run_id":id,"prompt":"Release and apply current guidance.","release":true}),
    );
    done(&d, &id);
    assert!(captures(&path, 2)[1]["text"]
        .as_str()
        .unwrap()
        .contains(&rules()));
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    db.execute_batch("CREATE TRIGGER fixture_refuse_mod_snapshot BEFORE INSERT ON turn_mods BEGIN SELECT RAISE(ABORT,'snapshot refusal fixture'); END;").unwrap();
    assert!(d
        .try_call(
            "run.follow_up",
            json!({"run_id":id,"prompt":"Both records must roll back."})
        )
        .is_err());
    assert_eq!(
        d.call("run.turns", json!({"run_id":id}))
            .as_array()
            .unwrap()
            .len(),
        2,
        "Turn admission must roll back when snapshot insertion fails"
    );
    db.execute_batch("DROP TRIGGER fixture_refuse_mod_snapshot")
        .unwrap();
    assert_eq!(captures(&path, 2).len(), 2);
}

#[test]
fn native_child_controls_precede_mods_and_missing_proof_stays_unknown() {
    let d = Daemon::start(&[("OVERSEER_TEST_AUTO_DISABLED", "1")]);
    let t = tmp();
    let repo = repo(&t.path().join("repo"));
    let version = install(&d);
    bind(&d, &version, "all_agents", None, true);
    let parent = create(
        &d,
        &repo,
        &t.path().join("parent.jsonl"),
        "Keep parent authority.",
    );
    done(&d, &parent);
    let path = t.path().join("child.jsonl");
    let child = create(&d, &repo, &path, "This fixture becomes an observed child.");
    done(&d, &child);
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    db.execute(
        "UPDATE runs SET parent_run_id=?2,relation_source='native' WHERE id=?1",
        rusqlite::params![child, parent],
    )
    .unwrap();
    db.execute("DELETE FROM run_roles WHERE run_id=?1", [&child])
        .unwrap();
    let revision = d.call("mods.list", json!({}))["revision"].clone();
    d.call("mods.bind",json!({"expected_revision":revision,"binding":{"mod_id":version["id"],"version":version["version"],
        "fingerprint":version["fingerprint"],"scope":{"kind":"agent","run_id":child},"enabled":true,"required":true}}));
    let why = d.call("mods.why", json!({"run_id":child}));
    assert_eq!(why["context"]["role"], "child");
    assert_eq!(
        why["desired"]["decisions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["status"] == "selected")
            .unwrap()["delivery"],
        "unsupported"
    );
    let refused = d
        .try_call(
            "run.follow_up",
            json!({"run_id":child,"prompt":"Mods cannot widen child control."}),
        )
        .unwrap_err();
    assert!(
        refused.contains("native children are controlled by their parent harness"),
        "{refused}"
    );
    assert_eq!(
        d.call("run.turns", json!({"run_id":child}))
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(captures(&path, 1).len(), 1);
    assert_eq!(why["support"]["children"], "unknown");
    assert_eq!(
        why["support"]["installed_runtime_qualification"],
        "unverified"
    );
}

#[test]
fn prepared_snapshot_without_transport_acceptance_is_pending_after_restart() {
    let t = tmp();
    let repo = repo(&t.path().join("repo"));
    let path = t.path().join("prepared.jsonl");
    let mut d = Daemon::start(&[("OVERSEER_TEST_AUTO_DISABLED", "1")]);
    let version = install(&d);
    bind(&d, &version, "all_agents", None, true);
    let id = create(
        &d,
        &repo,
        &path,
        "An admitted snapshot is not transport proof.",
    );
    done(&d, &id);
    let first = d.call("mods.why", json!({"run_id":id}))["last_turn"].clone();
    d.kill9();
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let mut prepared = first;
    prepared["outcome"] = json!("prepared");
    prepared.as_object_mut().unwrap().remove("outcome_detail");
    db.execute(
        "UPDATE turn_mods SET content=?2 WHERE turn_id=?1",
        rusqlite::params![prepared["turn_id"].as_str().unwrap(), prepared.to_string()],
    )
    .unwrap();
    d.spawn();
    let why = d.call("mods.why", json!({"run_id":id}));
    assert_eq!(why["last_turn"]["outcome"], "prepared");
    assert_eq!(
        why["pending"], true,
        "A crash before transport acceptance cannot claim the desired text was applied"
    );
}

#[test]
fn dynamic_local_model_selection_is_explicitly_unsupported_before_local_effects() {
    let d = Daemon::start(&[("OVERSEER_TEST_AUTO_DISABLED", "1")]);
    let t = tmp();
    let repo = repo(&t.path().join("repo"));
    let path = t.path().join("local.jsonl");
    let id = create(&d, &repo, &path, "A completed synthetic source run.");
    done(&d, &id);
    let version = install(&d);
    let scoped = bind(&d, &version, "all_agents", None, true);
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    // Private state observation only. No local server, provider, account or model is invoked.
    db.execute("UPDATE runs SET harness='opencode-serve',model='ollama/previous',native_id='local-fixture',harness_version=NULL WHERE id=?1",[&id]).unwrap();
    let why = d.call("mods.why", json!({"run_id":id}));
    let selected = why["desired"]["decisions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["status"] == "selected")
        .unwrap();
    assert_eq!(
        selected["delivery"], "unsupported",
        "The local guard may select a different model after admission"
    );
    assert_eq!(why["context"]["local_model_selection"], true);
    assert!(selected["reason"]
        .as_str()
        .unwrap()
        .contains("local model selection"));
    // With no private local profile, the unchanged local guard refuses before
    // reading inventory. The admitted optional snapshot must already be a bypass.
    let refused = d
        .try_call(
            "run.follow_up",
            json!({"run_id":id,"prompt":"Optional local text must be bypassed whole."}),
        )
        .unwrap_err();
    assert!(refused.contains("no folder of its own"), "{refused}");
    let applied = d.call("mods.why", json!({"run_id":id}))["last_turn"].clone();
    assert_eq!(applied["delivery"], "unsupported");
    assert_eq!(applied["text"], "");
    assert_eq!(applied["added_bytes"], 0);
    assert!(applied["plan"]["style_text"]
        .as_str()
        .unwrap()
        .contains(&rules()));
    let mut required = scoped;
    required.as_object_mut().unwrap().remove("actor");
    required.as_object_mut().unwrap().remove("changed_ms");
    required["required"] = json!(true);
    d.call(
        "mods.bind",
        json!({"binding":required,"expected_revision":d.call("mods.list",json!({}))["revision"]}),
    );
    let turns = d.call("run.turns", json!({"run_id":id}));
    let refused = d
        .try_call(
            "run.follow_up",
            json!({"run_id":id,"prompt":"Required local text must refuse first."}),
        )
        .unwrap_err();
    assert!(
        refused.contains("Required mod text cannot be delivered"),
        "{refused}"
    );
    assert_eq!(d.call("run.turns", json!({"run_id":id})), turns);
    assert_eq!(captures(&path, 1).len(), 1);
}

#[test]
fn credential_text_keeps_private_delivery_bytes_but_public_views_and_replay_are_redacted() {
    use std::io::{BufRead, BufReader, Write};
    let d = Daemon::start(&[("OVERSEER_TEST_AUTO_DISABLED", "1")]);
    let t = tmp();
    let repo = repo(&t.path().join("repo"));
    let marker = "sk-abcdefghijklmnopqrstuv"; // Recognized synthetic fixture, never a credential.
    let escaped = r#"{"api_key":"opaque\"mods-secret-suffix\\private-tail"}"#;
    let text = format!("Preserve the necessary caveat.\nSynthetic credential {marker}.\n{escaped}\nKeep the source and uncertainty.");
    let folder = t.path().join("private-rules");
    let version = local_rules(&d, &folder, &text);
    let preview = d.call(
        "mods.preview",
        json!({"source":folder,"operation":"install"}),
    );
    bind(&d, &version, "all_agents", None, true);
    let path = t.path().join("private-capture.jsonl");
    let id = create(
        &d,
        &repo,
        &path,
        "Preserve private bytes, sanitize presentation.",
    );
    done(&d, &id);
    let captured = captures(&path, 1);
    assert!(captured[0]["text"].as_str().unwrap().contains(&text));
    let why = d.call("mods.why", json!({"run_id":id}));
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let private: String = db
        .query_row(
            "SELECT content FROM turn_mods WHERE turn_id=?1",
            [why["last_turn"]["turn_id"].as_str().unwrap()],
            |r| r.get(0),
        )
        .unwrap();
    let private: Value = serde_json::from_str(&private).unwrap();
    assert!(private["text"].as_str().unwrap().contains(&text));
    let events = d.events(&id);
    let applied: Vec<&Value> = events
        .iter()
        .filter(|e| e["kind"] == "mods_applied")
        .collect();
    assert!(!applied.is_empty());
    for event in &applied {
        assert!(
            !event.to_string().contains(marker),
            "Public event leaked the synthetic recognized credential: {event}"
        );
        assert!(
            !event.to_string().contains("mods-secret-suffix"),
            "Decoded escaped credential suffix leaked through the public event"
        );
        assert_eq!(event["payload"]["snapshot"]["text_redacted"], true);
        assert_eq!(event["payload"]["snapshot"]["digest"], private["digest"]);
    }
    assert!(
        !why.to_string().contains("mods-secret-suffix"),
        "Decoded escaped credential suffix leaked through Mods inspection"
    );
    assert!(
        !why.to_string().contains(marker),
        "Mods inspection leaked the synthetic recognized credential"
    );
    assert_eq!(why["last_turn"]["text_redacted"], true);
    assert_eq!(why["last_turn"]["digest"], private["digest"]);
    assert_eq!(why["last_turn"]["added_bytes"], private["added_bytes"]);
    assert!(
        !preview.to_string().contains(marker),
        "Preview must redact presentation without changing the installed bytes"
    );
    let stored: String=db.query_row("SELECT payload FROM events WHERE kind='mods_applied' AND run_id=?1 ORDER BY seq DESC LIMIT 1",[&id],|r|r.get(0)).unwrap();
    assert!(
        !stored.contains(marker),
        "Durable public event storage must be sanitized before replay"
    );
    let mut socket = std::os::unix::net::UnixStream::connect(d.socket()).unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    writeln!(
        socket,
        "{}",
        json!({"id":1,"method":"events.subscribe","params":{"after":0,"run_id":id}})
    )
    .unwrap();
    let mut replayed = 0;
    for line in BufReader::new(socket).lines() {
        let item: Value = serde_json::from_str(&line.unwrap()).unwrap();
        if item["method"] == "replayed" {
            break;
        }
        if item["method"] == "event" && item["params"]["kind"] == "mods_applied" {
            assert!(!item.to_string().contains(marker));
            replayed += 1;
        }
    }
    assert_eq!(replayed, applied.len());
}

#[test]
fn required_future_local_model_is_unqualified_without_inventory_or_admission() {
    let d = Daemon::start(&[("OVERSEER_TEST_AUTO_DISABLED", "1")]);
    let t = tmp();
    let repo = repo(&t.path().join("repo"));
    let path = t.path().join("future-local.jsonl");
    let id = create(
        &d,
        &repo,
        &path,
        "Stored model does not prove the future selection.",
    );
    done(&d, &id);
    let version = install(&d);
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    db.execute("UPDATE runs SET harness='opencode-serve',model='ollama/previous',native_id='local-fixture',harness_version=NULL WHERE id=?1",[&id]).unwrap();
    let mut input = json!({"mod_id":version["id"],"version":version["version"],"fingerprint":version["fingerprint"],
        "scope":{"kind":"all_agents"},"enabled":true,"required":true,"locked":true,
        "filters":{"harnesses":["opencode-serve"],"models":["ollama/future"],"accounts":[]}});
    let b = d.call(
        "mods.bind",
        json!({"expected_revision":d.call("mods.list",json!({}))["revision"],"binding":input}),
    )["binding"]
        .clone();
    let why = d.call("mods.why", json!({"run_id":id}));
    let decision = why["desired"]["decisions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["binding_id"] == b["id"])
        .unwrap();
    assert_eq!(
        decision["status"], "unqualified",
        "A different stored model cannot rule out future required applicability"
    );
    assert_eq!(decision["delivery"], "unsupported");
    let before = d.call("run.turns", json!({"run_id":id}));
    let refusal = d
        .try_call(
            "run.follow_up",
            json!({"run_id":id,"prompt":"Refuse before local effects."}),
        )
        .unwrap_err();
    assert!(
        refusal.contains("Required mod text cannot be delivered"),
        "{refusal}"
    );
    assert_eq!(d.call("run.turns", json!({"run_id":id})), before);
    assert_eq!(captures(&path, 1).len(), 1);
    // Exact harness/account mismatches are already known, unlike the future model.
    input["id"] = b["id"].clone();
    for filters in [
        json!({"harnesses":["claude"],"models":["ollama/future"]}),
        json!({"accounts":["unrelated-account"],"models":["ollama/future"]}),
    ] {
        input["filters"] = filters;
        d.call(
            "mods.bind",
            json!({"expected_revision":d.call("mods.list",json!({}))["revision"],"binding":input}),
        );
        let why = d.call("mods.why", json!({"run_id":id}));
        assert_eq!(why["desired"]["decisions"][0]["status"], "filtered");
    }
}

#[test]
fn unknown_future_local_model_preserves_required_and_owner_off_precedence() {
    let d = Daemon::start(&[("OVERSEER_TEST_AUTO_DISABLED", "1")]);
    let t = tmp();
    let repo = repo(&t.path().join("repo"));
    let path = t.path().join("unknown-local.jsonl");
    let id = create(&d, &repo, &path, "Model-independent owner choices hold.");
    done(&d, &id);
    let version = install(&d);
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    db.execute("UPDATE runs SET harness='opencode-serve',model='ollama/previous',native_id='local-fixture' WHERE id=?1",[&id]).unwrap();
    let parent = json!({"mod_id":version["id"],"version":version["version"],"fingerprint":version["fingerprint"],"scope":{"kind":"all_agents"},"enabled":true,"required":true});
    d.call(
        "mods.bind",
        json!({"expected_revision":d.call("mods.list",json!({}))["revision"],"binding":parent}),
    );
    let mut off = json!({"mod_id":version["id"],"version":version["version"],"fingerprint":version["fingerprint"],"scope":{"kind":"agent","run_id":id},"enabled":false,"filters":{"models":["ollama/previous"]}});
    let bound = d.call(
        "mods.bind",
        json!({"expected_revision":d.call("mods.list",json!({}))["revision"],"binding":off}),
    )["binding"]
        .clone();
    let why = d.call("mods.why", json!({"run_id":id}));
    assert!(
        why["desired"]["decisions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["required"] == true && v["status"] == "unqualified"),
        "An unlisted future model can select the unfiltered required parent: {why}"
    );
    off["id"] = bound["id"].clone();
    off["filters"] = json!({});
    off["locked"] = json!(true);
    d.call(
        "mods.bind",
        json!({"expected_revision":d.call("mods.list",json!({}))["revision"],"binding":off}),
    );
    db.execute("UPDATE runs SET model=NULL WHERE id=?1", [&id])
        .unwrap();
    let why = d.call("mods.why", json!({"run_id":id}));
    assert!(
        !why["desired"]["decisions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["status"] == "selected" || v["status"] == "unqualified"),
        "The owner's unconditional locked off binding still wins for an unknown model: {why}"
    );
}

#[test]
fn malformed_manifest_error_redacts_source_without_losing_error_code() {
    let d = Daemon::start(&[("OVERSEER_TEST_AUTO_DISABLED", "1")]);
    let t = tmp();
    let source = t.path().join("malformed-private-mod");
    std::fs::create_dir(&source).unwrap();
    for (marker, field) in [
        ("sk-abcdefghijklmnopqrstuv", "summary"),
        ("opaque-private-value", "api_key"),
    ] {
        std::fs::write(
            source.join("mod.toml"),
            format!("{field}='{marker}' invalid_tail\n"),
        )
        .unwrap();
        let request = format!(
            "{}\n",
            json!({"id":1,"method":"mods.preview","params":{"source":source,"operation":"install"}})
        );
        let response: Value = serde_json::from_str(&d.raw(request.as_bytes())).unwrap();
        assert_eq!(response["error"]["code"], "invalid_mod", "{response}");
        let message = response["error"]["message"].as_str().unwrap();
        assert!(
            message.contains("invalid mod manifest")
                && message.contains("line 1")
                && message.contains("column "),
            "{response}"
        );
        assert!(
            !response.to_string().contains(marker),
            "Public parser error leaked private manifest source: {response}"
        );
    }
}

#[test]
fn future_local_candidate_bound_refuses_required_uncertainty_actionably() {
    let d = Daemon::start(&[("OVERSEER_TEST_AUTO_DISABLED", "1")]);
    let t = tmp();
    let repo = repo(&t.path().join("repo"));
    let path = t.path().join("bounded-local.jsonl");
    let id = create(
        &d,
        &repo,
        &path,
        "Bound local qualification without loading models.",
    );
    done(&d, &id);
    let version = install(&d);
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    db.execute("UPDATE runs SET harness='opencode-serve',model='ollama/previous',native_id='local-fixture' WHERE id=?1",[&id]).unwrap();
    let repo_key = d.call("mods.why", json!({"run_id":id}))["context"]["repo_key"].clone();
    for (scope, enabled, required, models) in [
        (
            json!({"kind":"all_agents"}),
            true,
            true,
            vec!["ollama/future".to_string()],
        ),
        (
            json!({"kind":"repository","repo_key":repo_key}),
            false,
            false,
            (0..64).map(|i| format!("ollama/off-{i}")).collect(),
        ),
        (
            json!({"kind":"agent","run_id":id}),
            false,
            false,
            vec!["ollama/another".to_string()],
        ),
    ] {
        d.call("mods.bind",json!({"expected_revision":d.call("mods.list",json!({}))["revision"],"binding":{
            "mod_id":version["id"],"version":version["version"],"fingerprint":version["fingerprint"],
            "scope":scope,"enabled":enabled,"required":required,"filters":{"models":models}}}));
    }
    let why = d.call("mods.why", json!({"run_id":id}));
    let decision = why["desired"]["decisions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["required"] == true)
        .unwrap();
    assert_eq!(decision["status"], "unqualified");
    assert!(
        decision["reason"].as_str().unwrap().contains("64"),
        "{decision}"
    );
    let turns = d.call("run.turns", json!({"run_id":id}));
    let error = d
        .try_call(
            "run.follow_up",
            json!({"run_id":id,"prompt":"Do not probe an unbounded model set."}),
        )
        .unwrap_err();
    assert!(
        error.contains("64") && error.contains("Required mod text cannot be delivered"),
        "{error}"
    );
    assert_eq!(d.call("run.turns", json!({"run_id":id})), turns);
    // A model-independent locked off choice is proof even when declarations
    // exceed the probe bound. It must not be defeated by conservative refusal.
    let off = d.call("mods.list", json!({}))["bindings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["scope"]["kind"] == "agent")
        .unwrap()
        .clone();
    d.call("mods.bind",json!({"expected_revision":d.call("mods.list",json!({}))["revision"],"binding":{
        "id":off["id"],"mod_id":version["id"],"version":version["version"],"fingerprint":version["fingerprint"],
        "scope":{"kind":"agent","run_id":id},"enabled":false,"locked":true}}));
    let why = d.call("mods.why", json!({"run_id":id}));
    assert!(
        !why["desired"]["decisions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|b| b["status"] == "unqualified" || b["status"] == "selected"),
        "{why}"
    );
}
