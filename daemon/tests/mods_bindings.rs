mod common;
use common::*;
use serde_json::{json, Value};

fn install(d: &Daemon, source: Value) -> Value {
    let preview = d.call(
        "mods.preview",
        json!({"source":source,"operation":"install"}),
    );
    d.call(
        "mods.install",
        json!({"preview_id":preview["id"],"confirm":true}),
    )["version"]
        .clone()
}

fn binding(version: &Value, scope: Value, enabled: bool) -> Value {
    json!({"mod_id":version["id"],"version":version["version"],
        "fingerprint":version["fingerprint"],"scope":scope,"enabled":enabled,
        "required":false,"locked":false,"filters":{"harnesses":[],"accounts":[],"models":[]}})
}

fn bind(d: &Daemon, input: Value) -> Value {
    let revision = d.call("mods.list", json!({}))["revision"].clone();
    d.call(
        "mods.bind",
        json!({"binding":input,"expected_revision":revision}),
    )
}

fn unbind(d: &Daemon, id: &Value) {
    let revision = d.call("mods.list", json!({}))["revision"].clone();
    d.call(
        "mods.unbind",
        json!({"binding_id":id,"expected_revision":revision}),
    );
}

fn run(d: &Daemon, repo: &std::path::Path, role: &str) -> String {
    let created = d.call(
        "task.create",
        json!({"repo":repo,"harness":"generic",
        "workspace_mode":"worktree","program":"/usr/bin/true","args":[],
        "prompt":"Keep the task facts.","title":"Mods scope fixture","role":role}),
    );
    let id = run_id(&created);
    // Overseer's run is deliberately absent from the ordinary agents list.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let state = d.call("state", json!({"include_hidden":true}));
        let saved = state["runs"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["id"] == id)
            .unwrap();
        if !["queued", "starting", "running", "waiting_for_user"]
            .contains(&saved["status"].as_str().unwrap())
        {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "generic Mods fixture did not finish: {saved}"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    id
}

fn desired(d: &Daemon, id: &str) -> Value {
    d.call("mods.why", json!({"run_id":id}))["desired"].clone()
}

fn fingerprints(plan: &Value) -> Vec<String> {
    plan["versions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["fingerprint"].as_str().unwrap().to_string())
        .collect()
}

fn local(d: &Daemon, folder: &std::path::Path, id: &str, kind: &str, text: &str) -> Value {
    std::fs::create_dir_all(folder).unwrap();
    let section = if kind == "style" {
        "[style]\nfile='text.md'"
    } else {
        "[rules]\nfiles=['text.md']"
    };
    std::fs::write(folder.join("mod.toml"), format!("schema_version=1\nid='{id}'\nname='Fixture'\nversion='1'\nsummary='A text fixture'\nsource='local'\n{section}\n")).unwrap();
    std::fs::write(folder.join("text.md"), text).unwrap();
    install(d, json!(folder))
}

#[test]
fn all_agents_excludes_watchers_and_overseer_with_separate_overrides() {
    let d = Daemon::start(&[("OVERSEER_TEST_AUTO_DISABLED", "1")]);
    let t = tmp();
    let repo = repo(&t.path().join("repo"));
    let version = install(&d, json!("bundled:clear-prose"));
    bind(&d, binding(&version, json!({"kind":"all_agents"}), true));
    let agent = run(&d, &repo, "agent");
    let watcher = run(&d, &repo, "watcher");
    let overseer = run(&d, &repo, "overseer");
    assert_eq!(
        fingerprints(&desired(&d, &agent)),
        vec![version["fingerprint"].as_str().unwrap()]
    );
    assert!(fingerprints(&desired(&d, &watcher)).is_empty());
    assert!(fingerprints(&desired(&d, &overseer)).is_empty());
    bind(&d, binding(&version, json!({"kind":"watchers"}), true));
    bind(&d, binding(&version, json!({"kind":"overseer"}), true));
    assert_eq!(fingerprints(&desired(&d, &watcher)).len(), 1);
    assert_eq!(fingerprints(&desired(&d, &overseer)).len(), 1);
    let off = bind(
        &d,
        binding(&version, json!({"kind":"agent","run_id":agent}), false),
    );
    assert!(fingerprints(&desired(&d, &agent)).is_empty());
    assert_eq!(fingerprints(&desired(&d, &overseer)).len(), 1);
    unbind(&d, &off["binding"]["id"]);
    assert_eq!(fingerprints(&desired(&d, &agent)).len(), 1);
    assert!(d.try_call("mods.why", json!({"run_id":"missing"})).is_err());
}

#[test]
fn repository_identity_is_shared_by_linked_worktrees_and_override_is_narrow() {
    let d = Daemon::start(&[("OVERSEER_TEST_AUTO_DISABLED", "1")]);
    let t = tmp();
    let root = repo(&t.path().join("root"));
    let other = repo(&t.path().join("other"));
    let linked = t.path().join("linked");
    git(
        &root,
        &[
            "worktree",
            "add",
            "-q",
            "--detach",
            linked.to_str().unwrap(),
            "HEAD",
        ],
    );
    let a = run(&d, &root, "agent");
    let b = run(&d, &linked, "agent");
    let c = run(&d, &other, "agent");
    let common = std::fs::canonicalize(root.join(".git")).unwrap();
    let version = install(&d, json!("bundled:clear-prose"));
    bind(
        &d,
        binding(
            &version,
            json!({"kind":"repository","repo_key":common}),
            true,
        ),
    );
    assert_eq!(fingerprints(&desired(&d, &a)).len(), 1);
    assert_eq!(fingerprints(&desired(&d, &b)).len(), 1);
    assert!(fingerprints(&desired(&d, &c)).is_empty());
    let off = bind(
        &d,
        binding(&version, json!({"kind":"agent","run_id":b}), false),
    );
    assert!(fingerprints(&desired(&d, &b)).is_empty());
    assert_eq!(fingerprints(&desired(&d, &a)).len(), 1);
    unbind(&d, &off["binding"]["id"]);
    assert_eq!(fingerprints(&desired(&d, &b)).len(), 1);
}

#[test]
fn binding_revisions_metadata_locks_and_targets_fail_without_mutation() {
    let d = Daemon::start(&[("OVERSEER_TEST_AUTO_DISABLED", "1")]);
    let t = tmp();
    let repo = repo(&t.path().join("repo"));
    let agent = run(&d, &repo, "agent");
    let version = install(&d, json!("bundled:clear-prose"));
    let mut parent = binding(&version, json!({"kind":"all_agents"}), true);
    parent["locked"] = json!(true);
    let old_revision = d.call("mods.list", json!({}))["revision"].clone();
    let saved = bind(&d, parent);
    let before = d.call("mods.list", json!({}));
    assert!(d.try_call("mods.bind", json!({"binding":binding(&version,json!({"kind":"agent","run_id":agent}),false),"expected_revision":old_revision})).is_err());
    for mut input in [
        binding(&version, json!({"kind":"agent","run_id":agent}), false),
        binding(&version, json!({"kind":"swarm"}), true),
        binding(&version, json!({"kind":"agent","run_id":"missing"}), true),
    ] {
        assert!(d
            .try_call(
                "mods.bind",
                json!({"binding":input,"expected_revision":before["revision"]})
            )
            .is_err());
        input["actor"] = json!("owner");
        assert!(d
            .try_call(
                "mods.bind",
                json!({"binding":input,"expected_revision":before["revision"]})
            )
            .is_err());
    }
    for key in ["actor", "role", "changed_ms", "env", "extra_args"] {
        let mut input = binding(&version, json!({"kind":"watchers"}), true);
        input[key] = json!("forged");
        assert!(
            d.try_call(
                "mods.bind",
                json!({"binding":input,"expected_revision":before["revision"]})
            )
            .is_err(),
            "accepted caller-controlled {key}"
        );
    }
    let mut missing = binding(&version, json!({"kind":"watchers"}), true);
    missing["fingerprint"] = json!("f".repeat(64));
    assert!(d
        .try_call(
            "mods.bind",
            json!({"binding":missing,"expected_revision":before["revision"]})
        )
        .is_err());
    assert_eq!(d.call("mods.list", json!({})), before);
    let mut unlock = binding(&version, json!({"kind":"all_agents"}), true);
    unlock["id"] = saved["binding"]["id"].clone();
    bind(&d, unlock); // a direct local-owner request can unlock its existing binding
    bind(
        &d,
        binding(&version, json!({"kind":"agent","run_id":agent}), false),
    );
    assert!(fingerprints(&desired(&d, &agent)).is_empty());
}

#[test]
fn same_named_version_update_keeps_binding_pinned_after_restart() {
    let mut d = Daemon::start(&[("OVERSEER_TEST_AUTO_DISABLED", "1")]);
    let t = tmp();
    let repo = repo(&t.path().join("repo"));
    let agent = run(&d, &repo, "agent");
    let folder = t.path().join("mod");
    let first = local(
        &d,
        &folder,
        "pinned",
        "rules",
        "Preserve the original warning.",
    );
    bind(&d, binding(&first, json!({"kind":"all_agents"}), true));
    let second = local(&d, &folder, "pinned", "rules", "Preserve the new warning.");
    assert_ne!(first["fingerprint"], second["fingerprint"]);
    assert_eq!(
        fingerprints(&desired(&d, &agent)),
        vec![first["fingerprint"].as_str().unwrap()]
    );
    d.kill9();
    d.spawn();
    let plan = desired(&d, &agent);
    assert_eq!(
        fingerprints(&plan),
        vec![first["fingerprint"].as_str().unwrap()]
    );
    assert!(plan["rules_text"]
        .as_str()
        .unwrap()
        .contains("original warning"));
    assert!(!plan["rules_text"].as_str().unwrap().contains("new warning"));
    assert_eq!(
        d.call("mods.list", json!({}))["support"]["native_configuration"],
        "unverified"
    );
}

#[test]
fn overlapping_styles_are_rejected_before_any_run_and_rules_order_is_stable() {
    let d = Daemon::start(&[("OVERSEER_TEST_AUTO_DISABLED", "1")]);
    let t = tmp();
    let first = local(&d, &t.path().join("a"), "a-style", "style", "First style.");
    let second = local(&d, &t.path().join("b"), "b-style", "style", "Second style.");
    bind(&d, binding(&first, json!({"kind":"all_agents"}), true));
    let before = d.call("mods.list", json!({}));
    let error = d.try_call("mods.bind", json!({"binding":binding(&second,json!({"kind":"all_agents"}),true),"expected_revision":before["revision"]})).unwrap_err();
    assert!(
        error.contains("a-style") && error.contains("b-style"),
        "{error}"
    );
    assert_eq!(d.call("mods.list", json!({})), before);
    let z = local(&d, &t.path().join("z"), "z-rule", "rules", "Last rule.");
    let a = local(
        &d,
        &t.path().join("rule-a"),
        "a-rule",
        "rules",
        "First rule.",
    );
    bind(&d, binding(&z, json!({"kind":"all_agents"}), true));
    bind(&d, binding(&a, json!({"kind":"all_agents"}), true));
    let repo = repo(&t.path().join("repo"));
    let agent = run(&d, &repo, "agent");
    let plan = desired(&d, &agent);
    let text = plan["rules_text"].as_str().unwrap();
    assert!(text.find("First rule.").unwrap() < text.find("Last rule.").unwrap());
    assert!(plan["style_text"]
        .as_str()
        .unwrap()
        .contains("First style."));
    assert_eq!(plan, desired(&d, &agent));
}

#[test]
fn filters_match_exact_harness_model_and_shared_account_not_profile_name() {
    let d = Daemon::start(&[("OVERSEER_TEST_AUTO_DISABLED", "1")]);
    let t = tmp();
    let repo = repo(&t.path().join("repo"));
    let one = run(&d, &repo, "agent");
    let two = run(&d, &repo, "agent");
    let unknown = run(&d, &repo, "agent");
    // Synthetic account observations in this test's private DB; no auth or provider process.
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    for (run_id, profile) in [(&one, "fixture-one"), (&two, "fixture-two")] {
        db.execute(
            "UPDATE runs SET profile_id=?2,model='fixture-model' WHERE id=?1",
            rusqlite::params![run_id, profile],
        )
        .unwrap();
        db.execute("INSERT INTO auto_account_identity(profile_id,fingerprint,generation,observed_ms) VALUES(?1,?2,1,1)", rusqlite::params![profile,"a".repeat(64)]).unwrap();
    }
    db.execute(
        "UPDATE runs SET model='fixture-model' WHERE id=?1",
        [&unknown],
    )
    .unwrap();
    drop(db);
    let version = install(&d, json!("bundled:clear-prose"));
    let mut input = binding(&version, json!({"kind":"all_agents"}), true);
    input["filters"] = json!({"harnesses":["generic"],"accounts":[format!("account/{}","a".repeat(64))],"models":["fixture-model"]});
    let saved = bind(&d, input);
    assert_eq!(fingerprints(&desired(&d, &one)).len(), 1);
    assert_eq!(fingerprints(&desired(&d, &two)).len(), 1);
    assert!(fingerprints(&desired(&d, &unknown)).is_empty());
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    // Scope filtering is independent of transport qualification: only private stored
    // fixture metadata changes here, and no native harness is started.
    db.execute("UPDATE runs SET harness='codex' WHERE id=?1", [&one])
        .unwrap();
    db.execute("UPDATE runs SET harness='opencode' WHERE id=?1", [&two])
        .unwrap();
    assert!(fingerprints(&desired(&d, &one)).is_empty());
    assert!(fingerprints(&desired(&d, &two)).is_empty());
    db.execute(
        "UPDATE runs SET harness='generic' WHERE id IN (?1,?2)",
        rusqlite::params![one, two],
    )
    .unwrap();
    db.execute("UPDATE runs SET model='fixture-other' WHERE id=?1", [&one])
        .unwrap();
    assert!(fingerprints(&desired(&d, &one)).is_empty());
    assert_eq!(fingerprints(&desired(&d, &two)).len(), 1);
    db.execute("UPDATE runs SET model='fixture-model' WHERE id=?1", [&one])
        .unwrap();
    db.execute(
        "UPDATE runs SET profile_id='fixture-three' WHERE id=?1",
        [&unknown],
    )
    .unwrap();
    db.execute("INSERT INTO auto_account_identity(profile_id,fingerprint,generation,observed_ms) VALUES('fixture-three',?1,1,1)", ["b".repeat(64)]).unwrap();
    drop(db);
    unbind(&d, &saved["binding"]["id"]);
    for filters in [
        json!({"harnesses":["codex"],"accounts":[],"models":[]}),
        json!({"harnesses":[],"accounts":[format!("account/{}","b".repeat(64))],"models":[]}),
        json!({"harnesses":[],"accounts":[],"models":["fixture"]}),
    ] {
        let mut input = binding(&version, json!({"kind":"all_agents"}), true);
        input["filters"] = filters.clone();
        let saved = bind(&d, input);
        assert!(fingerprints(&desired(&d, &one)).is_empty());
        if !filters["accounts"].as_array().unwrap().is_empty() {
            assert_eq!(fingerprints(&desired(&d, &unknown)).len(), 1);
        }
        unbind(&d, &saved["binding"]["id"]);
    }
}

#[test]
fn concurrent_clients_cannot_overwrite_each_others_revision() {
    let d = Daemon::start(&[("OVERSEER_TEST_AUTO_DISABLED", "1")]);
    let version = install(&d, json!("bundled:clear-prose"));
    let before = d.call("mods.list", json!({}));
    let barrier = std::sync::Barrier::new(2);
    let submit = |kind: &str| {
        barrier.wait();
        d.try_call("mods.bind", json!({"binding":binding(&version,json!({"kind":kind}),true),"expected_revision":before["revision"]}))
    };
    let results = std::thread::scope(|scope| {
        let first = scope.spawn(|| submit("all_agents"));
        let second = scope.spawn(|| submit("watchers"));
        [first.join().unwrap(), second.join().unwrap()]
    });
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert!(results
        .iter()
        .find(|r| r.is_err())
        .unwrap()
        .as_ref()
        .unwrap_err()
        .contains("Mods changed"));
    let after = d.call("mods.list", json!({}));
    assert_eq!(after["bindings"].as_array().unwrap().len(), 1);
    assert_eq!(
        after["revision"].as_i64().unwrap(),
        before["revision"].as_i64().unwrap() + 1
    );
}

#[test]
fn group_precedence_disjoint_styles_and_locked_off_parent_are_explicit() {
    let d = Daemon::start(&[("OVERSEER_TEST_AUTO_DISABLED", "1")]);
    let t = tmp();
    let repo = repo(&t.path().join("repo"));
    let agent = run(&d, &repo, "agent");
    let watcher = run(&d, &repo, "watcher");
    let first = local(
        &d,
        &t.path().join("style-a"),
        "a-style",
        "style",
        "Generic prose.",
    );
    let second = local(
        &d,
        &t.path().join("style-b"),
        "b-style",
        "style",
        "Codex prose.",
    );
    let mut generic = binding(&first, json!({"kind":"all_agents"}), true);
    generic["filters"]["harnesses"] = json!(["generic"]);
    let saved = bind(&d, generic);
    let mut codex = binding(&second, json!({"kind":"all_agents"}), true);
    codex["filters"]["harnesses"] = json!(["codex"]);
    bind(&d, codex); // disjoint filters cannot conflict on a current or future turn
    assert_eq!(desired(&d, &agent)["style_text"], "Generic prose.");
    let common = std::fs::canonicalize(repo.join(".git")).unwrap();
    bind(
        &d,
        binding(
            &first,
            json!({"kind":"repository","repo_key":common}),
            false,
        ),
    );
    bind(&d, binding(&first, json!({"kind":"watchers"}), true));
    assert!(fingerprints(&desired(&d, &agent)).is_empty());
    assert_eq!(desired(&d, &watcher)["style_text"], "Generic prose.");
    bind(
        &d,
        binding(&first, json!({"kind":"agent","run_id":watcher}), false),
    );
    assert!(fingerprints(&desired(&d, &watcher)).is_empty());
    // A one-run style takes precedence over styles selected at a broader scope.
    bind(
        &d,
        binding(&second, json!({"kind":"agent","run_id":agent}), true),
    );
    assert_eq!(desired(&d, &agent)["style_text"], "Codex prose.");
    unbind(&d, &saved["binding"]["id"]);
    let third = local(
        &d,
        &t.path().join("rule"),
        "locked-rule",
        "rules",
        "Preserve evidence.",
    );
    let mut off = binding(&third, json!({"kind":"all_agents"}), false);
    off["locked"] = json!(true);
    bind(&d, off);
    let before = d.call("mods.list", json!({}));
    assert!(d.try_call("mods.bind", json!({"binding":binding(&third,json!({"kind":"agent","run_id":agent}),true),"expected_revision":before["revision"]})).unwrap_err().contains("Locked binding"));
    assert_eq!(d.call("mods.list", json!({})), before);
}

#[test]
fn existing_agent_enters_and_leaves_watcher_scope_with_actual_watch_membership() {
    let d = Daemon::start(&[("OVERSEER_TEST_AUTO_DISABLED", "1")]);
    let t = tmp();
    let repo = repo(&t.path().join("repo"));
    let subject = run(&d, &repo, "agent");
    let watcher = run(&d, &repo, "agent");
    let version = install(&d, json!("bundled:clear-prose"));
    bind(&d, binding(&version, json!({"kind":"all_agents"}), true));
    bind(&d, binding(&version, json!({"kind":"watchers"}), false));
    assert_eq!(fingerprints(&desired(&d, &watcher)).len(), 1);
    let watch = d.call(
        "watch.start",
        json!({"subject":subject,"watcher":watcher,"brief":"Keep warnings visible","by":"owner"}),
    );
    assert_eq!(
        d.call("mods.why", json!({"run_id":watcher}))["context"]["role"],
        "watcher"
    );
    assert!(fingerprints(&desired(&d, &watcher)).is_empty());
    assert_eq!(fingerprints(&desired(&d, &subject)).len(), 1);
    d.call("watch.end", json!({"id":watch["id"],"by":"owner"}));
    assert_eq!(
        d.call("mods.why", json!({"run_id":watcher}))["context"]["role"],
        "agent"
    );
    assert_eq!(fingerprints(&desired(&d, &watcher)).len(), 1);
}

#[test]
fn locked_style_holds_when_watch_end_changes_applicable_scope() {
    let d = Daemon::start(&[("OVERSEER_TEST_AUTO_DISABLED", "1")]);
    let t = tmp();
    let repo = repo(&t.path().join("repo"));
    let subject = run(&d, &repo, "agent");
    let watcher = run(&d, &repo, "agent");
    let broad = local(
        &d,
        &t.path().join("broad"),
        "broad-style",
        "style",
        "The locked style.",
    );
    let narrow = local(
        &d,
        &t.path().join("narrow"),
        "narrow-style",
        "style",
        "The one-run style.",
    );
    let mut input = binding(&broad, json!({"kind":"all_agents"}), true);
    input["locked"] = json!(true);
    let saved = bind(&d, input);
    let watch = d.call(
        "watch.start",
        json!({"subject":subject,"watcher":watcher,"brief":"Keep facts visible","by":"owner"}),
    );
    bind(
        &d,
        binding(&narrow, json!({"kind":"agent","run_id":watcher}), true),
    );
    assert_eq!(desired(&d, &watcher)["style_text"], "The one-run style.");
    d.call("watch.end", json!({"id":watch["id"],"by":"owner"}));
    let plan = desired(&d, &watcher);
    assert_eq!(
        plan["style_text"], "The locked style.",
        "ending a watch cannot unlock the broad style"
    );
    assert_eq!(
        fingerprints(&plan),
        vec![broad["fingerprint"].as_str().unwrap()]
    );
    let mut unlock = binding(&broad, json!({"kind":"all_agents"}), true);
    unlock["id"] = saved["binding"]["id"].clone();
    bind(&d, unlock);
    assert_eq!(desired(&d, &watcher)["style_text"], "The one-run style.");
}
