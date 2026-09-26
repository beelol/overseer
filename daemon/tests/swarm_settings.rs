mod common;

use common::*;
use serde_json::json;

#[test]
fn app_agent_limit_is_not_a_swarm_policy_override() {
    let d = Daemon::start(&[]);
    d.call("agents.limit.set",json!({"max_active":3}));
    assert!(d.try_call("swarm.create",json!({"category":"No duplicate ceiling",
        "objective":"Audit","policy":{"max_executing":2}}))
        .unwrap_err().contains("unknown swarm policy setting max_executing"));
    assert_eq!(d.call("agents.limit.get",json!({}))["max_active"],3);
}

#[test]
fn legacy_saved_execution_ceiling_is_ignored_without_losing_worker_policy() {
    let d = Daemon::start(&[]);
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    db.execute("INSERT INTO swarm_policy_settings(scope,scope_key,policy,allowed_targets,updated_ms)
        VALUES('application','*',?1,'[\"system-codex\"]',1)",
        [r#"{"max_executing":2,"max_workers":4}"#]).unwrap();
    let run = d.call("swarm.create",json!({"category":"Migrated policy","objective":"Audit"}));
    assert_eq!(run["policy"]["effective"]["max_workers"],4);
    assert!(run["policy"]["effective"].get("max_executing").is_none());
    assert_eq!(run["allowed_targets"],json!(["system-codex"]));
}

#[test]
fn policy_precedence_and_approved_targets_are_snapshotted_per_run() {
    let d = Daemon::start(&[]);
    d.call("swarm.policy.set",json!({"scope":"application","policy":{"max_workers":6,"deadline_ms":900000},"allowed_targets":["system-codex"]}));
    d.call("swarm.policy.set",json!({"scope":"category","category":"Backend","policy":{"max_workers":4},"allowed_targets":["system-claude"]}));
    let first = d.call(
        "swarm.create",
        json!({"category":"Backend","objective":"Audit routes","policy":{"max_workers":3}}),
    );
    let id = first["id"].as_str().unwrap();
    assert_eq!(first["policy"]["effective"]["max_workers"], 3);
    assert_eq!(first["policy"]["sources"]["max_workers"], "run");
    assert_eq!(first["policy"]["effective"]["deadline_ms"], 900000);
    assert_eq!(first["policy"]["sources"]["deadline_ms"], "application");
    assert!(first["policy"]["effective"].get("max_executing").is_none());
    assert_eq!(d.call("agents.limit.get",json!({}))["max_active"],9);
    assert_eq!(first["allowed_targets"], json!(["system-claude"]));
    assert_eq!(first["needs_account_selection"], false);
    d.call("swarm.policy.set",json!({"scope":"category","category":"Backend","policy":{"max_workers":2},"allowed_targets":["system-opencode"]}));
    let unchanged = d.call("swarm.get", json!({"id":id}));
    assert_eq!(unchanged["policy"]["effective"]["max_workers"], 3);
    assert_eq!(unchanged["allowed_targets"], json!(["system-claude"]));
    let other = d.call(
        "swarm.create",
        json!({"category":"QA","objective":"Audit checks"}),
    );
    assert_eq!(other["policy"]["effective"]["max_workers"], 6);
    assert_eq!(other["allowed_targets"], json!(["system-codex"]));
}

#[test]
fn empty_approval_requires_one_selection_and_invalid_overrides_fail() {
    let d = Daemon::start(&[]);
    let run = d.call(
        "swarm.create",
        json!({"category":"Research","objective":"Compare systems"}),
    );
    assert_eq!(run["allowed_targets"], json!([]));
    assert_eq!(run["needs_account_selection"], true);
    assert_eq!(run["policy"]["effective"]["max_workers"], 8);
    assert!(d
        .try_call(
            "swarm.policy.set",
            json!({"scope":"application","policy":{"max_workers":0}})
        )
        .is_err());
    assert!(d.try_call("swarm.policy.set",json!({"scope":"application",
        "policy":{"run_allocation_percent":101}})).is_err());
    assert!(d
        .try_call(
            "swarm.create",
            json!({"category":"Bad","objective":"No","policy":{"unknown_switch":true}})
        )
        .is_err());
}

#[test]
fn category_limit_without_account_override_keeps_application_approval() {
    let d = Daemon::start(&[]);
    d.call(
        "swarm.policy.set",
        json!({"scope":"application","allowed_targets":["system-codex"],"policy":{}}),
    );
    d.call(
        "swarm.policy.set",
        json!({"scope":"category","category":"QA","policy":{"max_workers":2}}),
    );
    let run = d.call(
        "swarm.create",
        json!({"category":"QA","objective":"Review"}),
    );
    assert_eq!(run["allowed_targets"], json!(["system-codex"]));
    assert_eq!(run["policy"]["allowed_targets_source"], "application");
}
