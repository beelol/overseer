//! AUTO-AC-17 with Gate S present: Overseer-started agents take the same
//! admission as manual starts and Auto launches.

mod common;
use common::*;
use serde_json::{json, Value};
use std::time::Duration;

fn fixture(name: &str) -> String {
    repo_root().join("fixtures").join(name).display().to_string()
}

/// With one app slot left, two Overseer start proposals confirmed at the
/// same moment admit exactly one agent; the other's card reports the agent
/// limit. While that Overseer-started agent holds the slot, a manual start,
/// an Auto root and an Auto child are all refused by the same count. Once it
/// ends, the slot is free for the next admission. (Watchers launch through
/// the same `create_task` path as Overseer starts.)
#[test]
fn overseer_starts_manual_and_auto_launches_share_one_admission() {
    let r = tmp();
    let checkout = repo(&r.path().join("repo"));
    let mode_file = r.path().join("mode");
    std::fs::write(&mode_file, "permission").unwrap();
    let d = Daemon::start(&[("OVERSEER_CLAUDE_PATH", fixture("fake-harness/claude-fixture.js").as_str()),
        ("OVERSEER_CODEX_PATH", fixture("fake-harness/codex-app-fixture.js").as_str()),
        ("CLAUDE_FIXTURE_MODE_FILE", mode_file.to_str().unwrap()), ("FIXTURE_MODE", "managed-models"),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "CLAUDE_FIXTURE_MODE_FILE,FIXTURE_MODE")]);
    d.call("auto.mode.set", json!({"enabled":true}));
    let parent = run_id(&d.call("task.create", json!({"repo":checkout,"harness":"codex-app",
        "model":"gpt-6-astra","effort":"high","prompt":"seed context","approval_policy":"never"})));
    assert_eq!(d.wait_done(&parent, 15)["status"], "completed");
    d.call("agents.limit.set", json!({"max_active":1}));
    d.call("overseer.session", json!({}));
    rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap()
        .execute("UPDATE overseer_sessions SET last_cause='owner'", []).unwrap();
    let propose = |title: &str| -> String {
        d.call("overseer.propose", json!({"actions":[{"action":"start","repo":checkout,
            "harness":"claude","prompt":"hold for review","title":title}],"source":"ctl"}))
            ["proposal"].as_str().unwrap().to_string()
    };
    let (first, second) = (propose("overseer-one"), propose("overseer-two"));
    let answers: Vec<Value> = std::thread::scope(|scope| {
        let d = &d;
        let handles: Vec<_> = [first, second].into_iter().map(|id| scope.spawn(move ||
            d.call("overseer.answer", json!({"id":id,"yes":true,"surface":"ctl","by":"owner"}))))
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    let started: Vec<Value> = d.runs().into_iter()
        .filter(|run| run["title"].as_str().is_some_and(|t| t.starts_with("overseer-"))).collect();
    assert_eq!(started.len(), 1, "one slot admits one Overseer start: {answers:?}");
    assert_eq!(answers.iter().filter(|a| a["result"].as_str().unwrap_or("").contains("agent limit")).count(), 1,
        "{answers:?}");
    let holder = started[0]["id"].as_str().unwrap().to_string();
    d.wait_status(&holder, |s| s == "waiting_for_user", 20);

    let manual = d.try_call("task.create", json!({"repo":checkout,"harness":"codex-app",
        "model":"gpt-6-sol","effort":"medium","prompt":"manual","title":"manual"}));
    assert!(manual.unwrap_err().contains("agent limit"));
    let root = d.try_call("auto.start", json!({"work_unit_id":"root-1","repo":checkout,
        "workspace_mode":"worktree","prompt":"root","title":"root","allowed_profiles":["system-codex"],
        "min_tier":"general","required_tools":[],"sandbox":"workspace_write"}));
    assert!(root.unwrap_err().contains("agent limit"));
    let child = d.try_call("auto.dispatch", json!({"work_unit_id":"child-1","parent_run_id":parent,
        "min_tier":"general","required_tools":[],"prompt":"child","title":"child"}));
    assert!(child.unwrap_err().contains("agent limit"));
    let runs = d.runs().len();

    d.call("run.interrupt", json!({"run_id":holder}));
    d.wait_status(&holder, |s| !matches!(s, "queued" | "starting" | "running" | "waiting_for_user"), 20);
    std::thread::sleep(Duration::from_millis(300));
    let admitted = d.call("auto.dispatch", json!({"work_unit_id":"child-2","parent_run_id":parent,
        "min_tier":"general","required_tools":[],"prompt":"child","title":"child"}));
    assert_eq!(admitted["state"], "dispatched", "{admitted}");
    assert_eq!(d.runs().len(), runs + 1);
    assert_eq!(d.wait_done(&run_id(&admitted), 20)["status"], "completed");
}
