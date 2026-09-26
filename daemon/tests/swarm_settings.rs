mod common;

use common::*;
use serde_json::json;

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
    assert_eq!(first["policy"]["effective"]["max_executing"], 9);
    assert_eq!(first["policy"]["sources"]["max_executing"], "built_in");
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
