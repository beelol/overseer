//! Gate S governed text changes: authored contracts, not execution evidence.
//! Only isolated daemon homes and synthetic bundles; no owner library or paid model.
mod common;
use common::*;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

fn db(d: &Daemon) -> rusqlite::Connection {
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    db.busy_timeout(Duration::from_secs(5)).unwrap();
    db
}
fn rows(d: &Daemon, table: &str) -> Vec<Vec<String>> {
    let db = db(d);
    let mut s = db
        .prepare(&format!("SELECT * FROM {table} ORDER BY rowid"))
        .unwrap();
    let columns = s.column_count();
    let rows = s
        .query_map([], |r| {
            (0..columns)
                .map(|i| Ok(format!("{:?}", r.get_ref(i)?)))
                .collect()
        })
        .unwrap()
        .map(Result::unwrap)
        .collect();
    rows
}
fn mods_snapshot(d: &Daemon) -> Value {
    let db = db(d);
    let revision: String = db
        .query_row(
            "SELECT value FROM meta WHERE key='mods_revision'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let events: i64 = db
        .query_row(
            "SELECT count(*) FROM events WHERE kind='mods_changed'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    json!({"revision":revision,"events":events,"bindings":rows(d,"mod_bindings"),"versions":rows(d,"mod_versions"),"previews":rows(d,"mod_previews")})
}
fn controls(d: &Daemon) -> Value {
    json!({"roles":rows(d,"run_roles"),"runs":rows(d,"runs"),"turns":rows(d,"turns"),"tokens":rows(d,"overseer_tokens"),"profiles":rows(d,"profiles"),"guardrails":rows(d,"guardrails"),"repository_policy":d.call("overseer.auto_repos",json!({}))})
}
fn bindings(d: &Daemon) -> Vec<Value> {
    let db = db(d);
    let mut s = db
        .prepare("SELECT content FROM mod_bindings ORDER BY id")
        .unwrap();
    let bindings = s
        .query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .map(|r| serde_json::from_str(&r.unwrap()).unwrap())
        .collect();
    bindings
}
fn revision(d: &Daemon) -> Value {
    d.call("mods.list", json!({}))["revision"].clone()
}
fn source(root: &Path, id: &str, version: &str) -> PathBuf {
    let path = root.join(format!("{id}-{version}"));
    std::fs::create_dir_all(&path).unwrap();
    std::fs::write(path.join("mod.toml"), format!("schema_version=1\nid='{id}'\nname='Synthetic text'\nversion='{version}'\nsummary='Governance fixture'\nsource='local'\n[rules]\nfiles=['rules.md']\n")).unwrap();
    std::fs::write(path.join("rules.md"), "PRIVATE_GOVERNED_TEXT_BODY").unwrap();
    path
}
fn install(d: &Daemon, root: &Path, id: &str) -> Value {
    let preview = d.call(
        "mods.preview",
        json!({"source":source(root,id,"1"),"operation":"install"}),
    );
    d.call(
        "mods.install",
        json!({"preview_id":preview["id"],"confirm":true}),
    )["version"]
        .clone()
}
fn input(version: &Value, scope: Value) -> Value {
    json!({"mod_id":version["id"],"version":version["version"],"fingerprint":version["fingerprint"],"scope":scope,"enabled":true,"required":false,"locked":false,"filters":{"harnesses":[],"accounts":[],"models":[]}})
}
fn action(d: &Daemon, binding: Value) -> Value {
    json!({"action":"mod","operation":"bind","expected_revision":revision(d),"binding":binding})
}
fn propose(d: &Daemon, action: Value) -> Value {
    d.call(
        "overseer.propose",
        json!({"actions":[action],"source":"api"}),
    )
}
fn id(proposal: &Value) -> &str {
    proposal["proposal"]
        .as_str()
        .expect("accepted proposal has a daemon ID")
}
fn answer(d: &Daemon, proposal: &Value, yes: bool, surface: &str) -> Value {
    d.call(
        "overseer.answer",
        json!({"id":id(proposal),"yes":yes,"surface":surface,"by":"owner"}),
    )
}
fn assert_applied(reply: &Value) {
    assert_eq!(
        reply["state"], "yes",
        "governed change did not apply: {reply}"
    );
    assert!(
        !reply["result"].as_str().unwrap_or("").contains("failed"),
        "failed operation claimed completion: {reply}"
    );
}
fn rejects(d: &Daemon, actions: Value) {
    let before = mods_snapshot(d);
    let proposals = rows(d, "overseer_proposals");
    assert!(d
        .try_call(
            "overseer.propose",
            json!({"actions":actions,"source":"api"})
        )
        .is_err());
    assert_eq!(mods_snapshot(d), before);
    assert_eq!(rows(d, "overseer_proposals"), proposals);
}
fn common_key(path: &Path) -> String {
    let output = std::process::Command::new("git")
        .args([
            "-C",
            path.to_str().unwrap(),
            "rev-parse",
            "--path-format=absolute",
            "--git-common-dir",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    std::fs::canonicalize(String::from_utf8(output.stdout).unwrap().trim())
        .unwrap()
        .to_string_lossy()
        .into()
}
fn ordinary(d: &Daemon, repo: &Path, title: &str) -> String {
    let created = d.generic(repo, "worktree", "/usr/bin/true", &[]);
    let run = run_id(&created);
    assert_eq!(
        d.wait_done(&run, 20)["status"],
        "completed",
        "{title} fixture did not complete"
    );
    run
}
fn wait_idle(d: &Daemon) -> Value {
    let until = Instant::now() + Duration::from_secs(30);
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
            "Overseer fixture did not finish: {session}"
        );
        std::thread::sleep(Duration::from_millis(30));
    }
}
struct Fixture {
    d: Daemon,
    repo: PathBuf,
    forced: PathBuf,
    gate: PathBuf,
    session: Value,
}
impl Fixture {
    fn new(root: &Path) -> Self {
        let repo = repo(&root.join("repo"));
        let mode = root.join("mode");
        let forced = root.join("overseer-mode");
        let gate = root.join("native-gate");
        std::fs::create_dir(&gate).unwrap();
        std::fs::write(&mode, "overseer").unwrap();
        std::fs::write(&forced, "overseer").unwrap();
        let executable = repo_root().join("fixtures/fake-harness/claude-fixture.js");
        let d = Daemon::start(&[("OVERSEER_TEST_AUTO_DISABLED","1"),("OVERSEER_CLAUDE_PATH",executable.to_str().unwrap()),("CLAUDE_FIXTURE_MODE_FILE",mode.to_str().unwrap()),("CLAUDE_FIXTURE_OVERSEER_MODE_FILE",forced.to_str().unwrap()),("CLAUDE_FIXTURE_PROPOSE_GATE_DIR",gate.to_str().unwrap()),("OVERSEER_HARNESS_ENV_PASSTHROUGH","CLAUDE_FIXTURE_MODE_FILE,CLAUDE_FIXTURE_OVERSEER_MODE_FILE,CLAUDE_FIXTURE_PROPOSE_GATE_DIR")]);
        d.call("agent.cadence", json!({"cadence":"off","by":"owner"}));
        d.call("overseer.level", json!({"level":"ask_first"}));
        d.call(
            "overseer.send",
            json!({"text":"Report synthetic state only.","surface":"ctl","harness":"claude"}),
        );
        let session = wait_idle(&d);
        Self {
            d,
            repo,
            forced,
            gate,
            session,
        }
    }
    fn arm(&self, calls: Value) {
        for name in ["reached", "release", "reply.json", "replies.json"] {
            let p = self.gate.join(name);
            if p.exists() {
                std::fs::remove_file(p).unwrap();
            }
        }
        std::fs::write(self.gate.join("calls.json"), calls.to_string()).unwrap();
        std::fs::write(&self.forced, "overseer-gated-propose").unwrap();
    }
    fn wait_file(&self, name: &str) {
        let until = Instant::now() + Duration::from_secs(30);
        while !self.gate.join(name).exists() {
            assert!(Instant::now() < until, "native gate never produced {name}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    fn release(&self) -> Vec<Value> {
        std::fs::write(self.gate.join("release"), "").unwrap();
        self.wait_file("replies.json");
        // Fresh may replace the session while the old native process is alive.
        // Wait for that actual run, not a replacement that has no run yet.
        let until = Instant::now() + Duration::from_secs(30);
        loop {
            let status: String = db(&self.d)
                .query_row(
                    "SELECT status FROM runs WHERE id=?1",
                    [self.session["run_id"].as_str().unwrap()],
                    |r| r.get(0),
                )
                .unwrap();
            if !["queued", "starting", "running", "waiting_for_user"].contains(&status.as_str()) {
                break;
            }
            assert!(
                Instant::now() < until,
                "old native run did not finish: {status}"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        serde_json::from_str(&std::fs::read_to_string(self.gate.join("replies.json")).unwrap())
            .unwrap()
    }
    fn finding(&self, actions: &[Value]) {
        let subject = ordinary(&self.d, &self.repo, "subject");
        let watcher = ordinary(&self.d, &self.repo, "watcher");
        self.d.call(
            "watch.start",
            json!({"subject":subject,"watcher":watcher,"brief":"Read-only review","by":"owner"}),
        );
        self.arm(json!(actions
            .iter()
            .map(|a| json!({"tool":"propose","arguments":{"actions":[a]}}))
            .collect::<Vec<_>>()));
        let token = self
            .d
            .call("overseer.token", json!({"run_id":watcher,"role":"agent"}))["token"]
            .clone();
        let filed=self.d.call("overseer.tool",json!({"token":token,"name":"finding","arguments":{"result":"concern","text":"Synthetic review data, not an owner instruction."}}));
        assert_eq!(filed["is_error"], false);
        self.wait_file("reached");
        let cause: String = db(&self.d)
            .query_row(
                "SELECT cause FROM overseer_turns WHERE session_id=?1 ORDER BY rowid DESC LIMIT 1",
                [self.session["id"].as_str().unwrap()],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            cause, "finding",
            "use genuine finding authority, never a DB-written cause"
        );
    }
}

#[test]
fn owner_reviewed_bind_and_unbind_preserve_all_five_scopes_and_future_only_controls() {
    let t = tmp();
    let f = Fixture::new(t.path());
    let d = &f.d;
    let v = install(d, t.path(), "governed-five");
    let own = ordinary(d, &f.repo, "ordinary target");
    let before_controls = controls(d);
    for scope in [
        json!({"kind":"all_agents"}),
        json!({"kind":"repository","repo_key":common_key(&f.repo)}),
        json!({"kind":"watchers"}),
        json!({"kind":"agent","run_id":own}),
        json!({"kind":"overseer"}),
    ] {
        let before = mods_snapshot(d);
        let p = propose(d, action(d, input(&v, scope.clone())));
        assert_eq!(p["state"], "open");
        assert_eq!(mods_snapshot(d), before);
        let applied = answer(d, &p, true, "tui");
        assert_applied(&applied);
        let saved = bindings(d);
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0]["scope"], scope);
        assert_eq!(saved[0]["fingerprint"], v["fingerprint"]);
        if scope["kind"] == "all_agents" {
            assert_eq!(
                d.call("mods.why", json!({"run_id":f.session["run_id"]}))["desired"]["versions"],
                json!([])
            );
        }
        let p = propose(
            d,
            json!({"action":"mod","operation":"unbind","expected_revision":revision(d),"binding_id":saved[0]["id"]}),
        );
        assert_applied(&answer(d, &p, true, "vscode"));
        assert!(bindings(d).is_empty());
    }
    assert_eq!(
        controls(d),
        before_controls,
        "binding changes must not launch/resume agents or modify roles/auth/policy"
    );
}
#[test]
fn owner_steer_and_auto_binding_changes_settle_without_bypassing_cancellation() {
    let t = tmp();
    let f = Fixture::new(t.path());
    let v = install(&f.d, t.path(), "governed-settle");
    for level in ["steer", "auto"] {
        f.d.call("overseer.level", json!({"level":level}));
        let before = mods_snapshot(&f.d);
        let p = propose(&f.d, action(&f.d, input(&v, json!({"kind":"overseer"}))));
        assert_eq!(
            p["state"], "settling",
            "owner Steer retains existing settle window"
        );
        f.d.call("overseer.cancel", json!({"id":id(&p),"by":"owner"}));
        assert_eq!(mods_snapshot(&f.d), before);
    }
}
#[test]
fn strict_mod_actions_reject_unknown_authority_private_references_and_temporary_batches() {
    let t = tmp();
    let f = Fixture::new(t.path());
    let v = install(&f.d, t.path(), "governed-strict");
    let good = action(&f.d, input(&v, json!({"kind":"overseer"})));
    let p = propose(&f.d, good.clone());
    answer(&f.d, &p, false, "tui");
    for key in [
        "class",
        "confirm",
        "actor",
        "role",
        "env",
        "approval",
        "command_ref",
        "private_command_id",
        "proposal_id",
        "action_index",
        "unknown",
    ] {
        let mut bad = good.clone();
        bad[key] = json!("owner");
        rejects(&f.d, json!([bad]));
    }
    for key in ["actor", "role", "class", "command_ref"] {
        let mut bad = good.clone();
        bad["binding"][key] = json!("owner");
        rejects(&f.d, json!([bad]));
    }
    let mut bad = good.clone();
    bad["binding"]["filters"]["regex"] = json!(".*");
    rejects(&f.d, json!([bad]));
    let mut bad = good.clone();
    bad["binding"]["id"] = json!("unknown-binding");
    rejects(&f.d, json!([bad]));
    for operation in ["execute", "lock", "unlock", "refresh", "unknown"] {
        let mut bad = good.clone();
        bad["operation"] = json!(operation);
        rejects(&f.d, json!([bad]));
    }
    let mut bad = good.clone();
    bad["binding"]["scope"] = json!({"kind":"global"});
    rejects(&f.d, json!([bad]));
    let mut bad = good.clone();
    bad["binding"]["fingerprint"] = json!("0".repeat(64));
    rejects(&f.d, json!([bad]));
    rejects(&f.d, json!([good.clone(), good.clone()]));
    rejects(&f.d, json!([good,{"action":"pin","agent":"made-up"}]));
}
#[test]
fn owner_lock_is_never_a_governed_capability_even_with_yes_or_auto() {
    let t = tmp();
    let f = Fixture::new(t.path());
    let d = &f.d;
    let v = install(d, t.path(), "governed-lock");
    let good = action(d, input(&v, json!({"kind":"overseer"})));
    let p = propose(d, good.clone());
    answer(d, &p, false, "tui");
    let mut locked = input(&v, json!({"kind":"overseer"}));
    locked["locked"] = json!(true);
    rejects(d, json!([action(d, locked.clone())]));
    for enabled in [true, false] {
        locked["enabled"] = json!(enabled);
        let saved = d.call(
            "mods.bind",
            json!({"expected_revision":revision(d),"binding":locked}),
        )["binding"]
            .clone();
        f.d.call("overseer.level", json!({"level":"auto"}));
        let mut edit = input(&v, json!({"kind":"overseer"}));
        edit["id"] = saved["id"].clone();
        edit["enabled"] = json!(!enabled);
        rejects(d, json!([action(d, edit.clone())]));
        rejects(
            d,
            json!([{"action":"mod","operation":"unbind","expected_revision":revision(d),"binding_id":saved["id"]}]),
        );
        // The existing local-owner surface may unlock; Gate S cannot inherit it.
        f.d.call("overseer.level", json!({"level":"ask_first"}));
        d.call(
            "mods.bind",
            json!({"expected_revision":revision(d),"binding":edit}),
        );
        let saved = bindings(d);
        let p = propose(
            d,
            json!({"action":"mod","operation":"unbind","expected_revision":revision(d),"binding_id":saved[0]["id"]}),
        );
        assert_applied(&answer(d, &p, true, "tui"));
    }
}
#[test]
fn concurrent_revision_and_later_lock_leave_reviewed_proposals_stale_without_rebasing() {
    let t = tmp();
    let f = Fixture::new(t.path());
    let d = &f.d;
    let v = install(d, t.path(), "governed-stale");
    let saved = d.call(
        "mods.bind",
        json!({"expected_revision":revision(d),"binding":input(&v,json!({"kind":"overseer"}))}),
    )["binding"]
        .clone();
    let mut edit = input(&v, json!({"kind":"overseer"}));
    edit["id"] = saved["id"].clone();
    edit["enabled"] = json!(false);
    let p = propose(d, action(d, edit.clone()));
    edit["locked"] = json!(true);
    d.call(
        "mods.bind",
        json!({"expected_revision":revision(d),"binding":edit}),
    );
    let before = mods_snapshot(d);
    let result = answer(d, &p, true, "vscode");
    assert_eq!(result["state"], "stale");
    assert_eq!(mods_snapshot(d), before);
    assert!(!result["result"].as_str().unwrap().starts_with("Done:"));
    assert!(d
        .try_call(
            "overseer.answer",
            json!({"id":id(&p),"yes":true,"surface":"tui","by":"owner"})
        )
        .is_err());
    assert_eq!(mods_snapshot(d), before);
}
#[test]
fn removed_version_pin_cannot_be_replaced_when_a_waiting_binding_is_approved() {
    let t = tmp();
    let f = Fixture::new(t.path());
    let d = &f.d;
    let v = install(d, t.path(), "governed-removed");
    let p = propose(d, action(d, input(&v, json!({"kind":"overseer"}))));
    d.call("mods.remove",json!({"mod_id":v["id"],"fingerprint":v["fingerprint"],"expected_revision":revision(d),"confirm":true}));
    let before = mods_snapshot(d);
    let result = answer(d, &p, true, "tui");
    assert_eq!(result["state"], "stale");
    assert_eq!(mods_snapshot(d), before);
    assert!(bindings(d).is_empty());
}
#[test]
fn public_redaction_never_rewrites_private_canonical_path_or_escaped_filters() {
    let t = tmp();
    let f = Fixture::new(t.path());
    let d = &f.d;
    let v = install(d, t.path(), "governed-private");
    let secret = "sk-governedFixturePrivatePath0001";
    let private_repo = repo(&t.path().join(secret));
    let run = ordinary(d, &private_repo, "private exact repository");
    let key = common_key(&private_repo);
    let p = propose(
        d,
        action(d, input(&v, json!({"kind":"repository","repo_key":key}))),
    );
    let card = d.call("overseer.card", json!({"id":id(&p)}));
    assert!(!card.to_string().contains(secret));
    assert!(!p.to_string().contains(secret));
    assert_applied(&answer(d, &p, true, "tui"));
    assert_eq!(
        bindings(d)[0]["scope"]["repo_key"],
        key,
        "private canonical bytes are execution authority"
    );
    assert_eq!(
        d.call("mods.why", json!({"run_id":run}))["desired"]["versions"][0]["fingerprint"],
        v["fingerprint"]
    );
    let escaped = "quoted\\\"sk-governedExactFilter00001\\suffix";
    let mut b = input(&v, json!({"kind":"agent","run_id":run}));
    b["filters"]["models"] = json!([escaped]);
    let p = propose(d, action(d, b));
    assert!(!d
        .call("overseer.card", json!({"id":id(&p)}))
        .to_string()
        .contains("sk-governedExactFilter00001"));
    assert_applied(&answer(d, &p, true, "vscode"));
    assert!(bindings(d)
        .iter()
        .any(|b| b["filters"]["models"] == json!([escaped])));
    for public in [
        d.call("overseer.session", json!({})),
        d.call("overseer.messages", json!({"after":0,"limit":500})),
        d.call("events.list", json!({"limit":5000})),
    ] {
        assert!(!public.to_string().contains(secret));
        assert!(!public.to_string().contains("sk-governedExactFilter00001"));
        assert!(!public.to_string().contains("PRIVATE_GOVERNED_TEXT_BODY"));
    }
}
#[test]
fn two_private_commands_remain_distinct_and_declined_action_cannot_execute_after_restart() {
    let t = tmp();
    let mut f = Fixture::new(t.path());
    let v = install(&f.d, t.path(), "governed-identity");
    let a = propose(&f.d, action(&f.d, input(&v, json!({"kind":"all_agents"}))));
    let b = propose(&f.d, action(&f.d, input(&v, json!({"kind":"overseer"}))));
    assert_ne!(id(&a), id(&b));
    let before = mods_snapshot(&f.d);
    assert_eq!(answer(&f.d, &a, false, "vscode")["state"], "no");
    assert_eq!(mods_snapshot(&f.d), before);
    f.d.shutdown();
    f.d.spawn();
    assert_applied(&answer(&f.d, &b, true, "tui"));
    assert_eq!(bindings(&f.d).len(), 1);
    assert_eq!(bindings(&f.d)[0]["scope"]["kind"], "overseer");
    let applied = mods_snapshot(&f.d);
    assert!(f
        .d
        .try_call(
            "overseer.answer",
            json!({"id":id(&a),"yes":true,"surface":"tui","by":"owner"})
        )
        .is_err());
    assert_eq!(mods_snapshot(&f.d), applied);
}
#[test]
fn shared_owner_answers_claim_once_across_surface_labels_without_new_execution_parameters() {
    let t = tmp();
    let f = Fixture::new(t.path());
    let d = &f.d;
    let v = install(d, t.path(), "governed-once");
    let p = propose(d, action(d, input(&v, json!({"kind":"overseer"}))));
    let before = revision(d).as_i64().unwrap();
    let events = mods_snapshot(d)["events"].as_i64().unwrap();
    // This is the shared daemon answer boundary, not real device/terminal UI.
    let replies = std::thread::scope(|s| {
        let a = s.spawn(|| {
            d.try_call(
                "overseer.answer",
                json!({"id":id(&p),"yes":true,"surface":"vscode","by":"owner"}),
            )
        });
        let b = s.spawn(|| {
            d.try_call(
                "overseer.answer",
                json!({"id":id(&p),"yes":true,"surface":"tui","by":"owner"}),
            )
        });
        vec![a.join().unwrap(), b.join().unwrap()]
    });
    assert_eq!(replies.iter().filter(|r| r.is_ok()).count(), 1);
    for result in replies {
        match result {
            Ok(result) => assert_applied(&result),
            Err(error) => assert!(error.contains("already_answered")),
        }
    }
    assert_eq!(bindings(d).len(), 1);
    assert_eq!(revision(d).as_i64(), Some(before + 1));
    assert_eq!(mods_snapshot(d)["events"].as_i64(), Some(events + 1));
    let before = mods_snapshot(d);
    rejects(
        d,
        json!([{"action":"mod","operation":"bind","command_ref":id(&p),"actor":"owner"}]),
    );
    assert_eq!(mods_snapshot(d), before);
}
#[test]
fn real_native_finding_keeps_steer_policy_and_cannot_confirm_library_or_borrow_local_owner_lock() {
    for level in ["steer", "auto"] {
        let t = tmp();
        let f = Fixture::new(t.path());
        let d = &f.d;
        let v = install(d, t.path(), "governed-finding");
        let mut locked = input(&v, json!({"kind":"overseer"}));
        locked["locked"] = json!(true);
        let saved = d.call(
            "mods.bind",
            json!({"expected_revision":revision(d),"binding":locked}),
        )["binding"]
            .clone();
        let mut edit = input(&v, json!({"kind":"overseer"}));
        edit["id"] = saved["id"].clone();
        edit["enabled"] = json!(false);
        d.call("overseer.level", json!({"level":level}));
        // Test lock denial at the current revision before the legitimate Auto
        // bind can advance it; stale revision must not mask inherited authority.
        f.finding(&[action(d,edit),json!({"action":"mod","operation":"preview","expected_revision":revision(d),"source":source(t.path(),"finding-library","1")}),action(d,input(&v,json!({"kind":"all_agents"})))]);
        let before = mods_snapshot(d);
        let replies = f.release();
        assert_eq!(replies.len(), 3);
        assert_eq!(
            replies[2]["isError"], false,
            "genuine finding Steer remains legitimate: {replies:?}"
        );
        assert_eq!(
            replies[1]["isError"], true,
            "finding cannot authorize library Confirm"
        );
        assert_eq!(
            replies[0]["isError"], true,
            "native Unix transport must not supply local-owner lock authority"
        );
        let stored: (String, String, String) = db(d)
            .query_row(
                "SELECT id,cause,state FROM overseer_proposals ORDER BY rowid DESC LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(stored.1, "finding");
        if level == "steer" {
            assert_eq!(stored.2, "open");
            assert_eq!(mods_snapshot(d), before);
            d.call(
                "overseer.answer",
                json!({"id":stored.0,"yes":false,"surface":"tui","by":"owner"}),
            );
        } else {
            assert_eq!(stored.2, "yes");
            assert!(bindings(d)
                .iter()
                .any(|b| b["scope"]["kind"] == "all_agents"));
        }
        assert!(bindings(d)
            .iter()
            .any(|b| b["id"] == saved["id"] && b["locked"] == true && b["enabled"] == true));
    }
}
#[test]
fn archived_native_finding_cannot_publish_governed_command_into_fresh_owner_session() {
    let t = tmp();
    let f = Fixture::new(t.path());
    let d = &f.d;
    let v = install(d, t.path(), "governed-archived");
    d.call("overseer.level", json!({"level":"auto"}));
    f.finding(&[action(d, input(&v, json!({"kind":"all_agents"})))]);
    let old = f.session["id"].clone();
    let fresh = d.call("overseer.fresh", json!({}));
    assert_eq!(fresh["archived"], old);
    let before = mods_snapshot(d);
    let proposals = rows(d, "overseer_proposals");
    let replies = f.release();
    assert_eq!(replies[0]["isError"], true);
    assert_eq!(mods_snapshot(d), before);
    assert_eq!(rows(d, "overseer_proposals"), proposals);
}
#[test]
fn owner_library_operations_remain_confirm_at_auto_and_no_has_no_mod_side_effect() {
    let t = tmp();
    let f = Fixture::new(t.path());
    let d = &f.d;
    let v = install(d, t.path(), "governed-library");
    let next = source(t.path(), "governed-library", "2");
    let update = d.call("mods.preview", json!({"source":next,"operation":"update"}));
    let other = source(t.path(), "governed-new", "1");
    let new = d.call(
        "mods.preview",
        json!({"source":other,"operation":"install"}),
    );
    d.call("overseer.level", json!({"level":"auto"}));
    for operation in [
        json!({"action":"mod","operation":"preview","source":other,"expected_revision":revision(d)}),
        json!({"action":"mod","operation":"install","preview_id":new["id"],"fingerprint":new["fingerprint"],"expected_revision":revision(d)}),
        json!({"action":"mod","operation":"update","preview_id":update["id"],"fingerprint":update["fingerprint"],"expected_revision":revision(d)}),
        json!({"action":"mod","operation":"remove","mod_id":v["id"],"fingerprint":v["fingerprint"],"expected_revision":revision(d)}),
    ] {
        let before = mods_snapshot(d);
        let p = propose(d, operation);
        assert_eq!(p["state"], "open");
        assert_eq!(mods_snapshot(d), before);
        let events = d.events(f.session["run_id"].as_str().unwrap());
        let published = events
            .iter()
            .find(|e| e["kind"] == "proposal" && e["payload"]["id"] == id(&p))
            .unwrap();
        assert_eq!(
            published["payload"]["confirm"], true,
            "operation class is daemon-owned"
        );
        answer(d, &p, false, "tui");
        assert_eq!(mods_snapshot(d), before);
    }
}
