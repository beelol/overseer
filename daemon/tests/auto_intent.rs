//! AUTO-AC-23: the owner's intent survives route picking.

mod common;
use common::*;
use serde_json::json;
use std::time::Duration;

fn fixture(name: &str) -> String {
    repo_root().join("fixtures").join(name).display().to_string()
}

fn decisions(d: &Daemon) -> i64 {
    rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap()
        .query_row("SELECT COUNT(*) FROM events WHERE kind='auto_decision'", [], |row| row.get(0)).unwrap()
}

/// A Gate S redirect of an Auto-selected root keeps its route: the new
/// direction runs as the same run, on the same account, model and effort,
/// with no new routing decision. A separately admitted new unit is still
/// assessed on its own (a frontier diagnosis goes to Astra/high). A child
/// that fails ordinarily stays failed: no successor starts by itself (Auto
/// has no automatic continuation; a handoff is only ever the owner's
/// explicit request), and asking for the same unit again pauses.
#[test]
fn auto_redirect_keeps_the_route_and_no_successor_starts_by_itself() {
    let r = tmp();
    let checkout = repo(&r.path().join("repo"));
    let trace = r.path().join("trace.txt");
    let d = Daemon::start(&[("OVERSEER_CODEX_PATH", fixture("fake-harness/codex-app-fixture.js").as_str()),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_MODE,FIXTURE_TRACE_FILE"),
        ("FIXTURE_MODE", "managed-models"), ("FIXTURE_TRACE_FILE", trace.to_str().unwrap())]);
    d.call("auto.mode.set", json!({"enabled":true}));
    let root = d.call("auto.start", json!({"work_unit_id":"root-1","repo":checkout,
        "workspace_mode":"worktree","prompt":"seed context","title":"root",
        "allowed_profiles":["system-codex"],"min_tier":"general","required_tools":[],
        "sandbox":"workspace_write"}));
    assert_eq!(root["decision"]["selected"], "system-codex/gpt-6-sol/medium", "{root}");
    let run = run_id(&root);
    assert_eq!(d.wait_done(&run, 15)["status"], "completed");
    let before = decisions(&d);

    let redirected = d.call("agent.redirect", json!({"run_id":run,"text":"look at the other file instead"}));
    assert_eq!(redirected["delivery"], "sent", "{redirected}");
    let after = d.wait_done(&run, 15);
    assert_eq!(after["status"], "completed");
    assert_eq!((after["profile_id"].as_str(), after["model"].as_str(), after["effort"].as_str()),
        (Some("system-codex"), Some("gpt-6-sol"), Some("medium")));
    assert_eq!(d.runs().len(), 1, "a redirect is the same agent, not a new route");
    assert_eq!(decisions(&d), before, "a redirect makes no routing decision");
    let turns = std::fs::read_to_string(&trace).unwrap();
    assert_eq!(turns.matches("turn_model:gpt-6-sol").count(), 2, "{turns}");
    assert_eq!(turns.matches("turn_effort:medium").count(), 2, "{turns}");
    assert!(!turns.contains("turn_model:gpt-6-astra"));

    // A separately admitted unit is assessed on its own.
    let diagnosis = d.call("auto.dispatch", json!({"work_unit_id":"diagnosis-2","parent_run_id":run,
        "min_tier":"frontier","required_tools":[],"prompt":"diagnose","title":"diagnosis"}));
    assert_eq!(diagnosis["decision"]["selected"], "system-codex/gpt-6-astra/high", "{diagnosis}");
    assert_eq!(d.wait_done(&run_id(&diagnosis), 20)["status"], "completed");

    // An ordinary failure: no successor by itself, and a paused replay.
    let failing = d.call("auto.dispatch", json!({"work_unit_id":"failing-3","parent_run_id":run,
        "min_tier":"general","required_tools":[],"prompt":"simulate direct 503","title":"fails"}));
    let child = run_id(&failing);
    assert_eq!(d.wait_done(&child, 20)["status"], "failed");
    let runs = d.runs().len();
    std::thread::sleep(Duration::from_secs(2));
    assert_eq!(d.runs().len(), runs, "no continuation starts without the owner");
    let replay = d.call("auto.dispatch", json!({"work_unit_id":"failing-3","parent_run_id":run,
        "min_tier":"general","required_tools":[],"prompt":"simulate direct 503","title":"fails"}));
    assert_eq!(replay["state"], "paused", "{replay}");
    assert_eq!(replay["run"]["id"], child);
    assert_eq!(d.runs().len(), runs);
    assert!(d.try_call("run.handoff", json!({"source_run_id":child,"harness":"codex-app",
        "model":"gpt-6-sol","effort":"medium","handoff":{"corrections":[],"completed":[],
        "remaining":["retry"],"tests":[],"limitations":[],"unresolved_actions":[]}})).is_err(),
        "a failed child is not a checkpoint to continue from");
}
