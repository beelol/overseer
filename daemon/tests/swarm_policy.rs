mod common;

use common::*;
use serde_json::{json, Value};

fn snapshot(remaining: Option<i64>, exact: bool) -> Value {
    json!({
        "version":4,"observed_ms":1000,"expires_ms":2000,
        "targets":[
            {"id":"cheap","account_id":"shared","pool_ids":["shared-pool"],"capabilities":["read"],"health":"up","auth":"ok","provider":"alpha"},
            {"id":"qualified","account_id":"shared","pool_ids":["shared-pool"],"capabilities":["read","write"],"health":"up","auth":"ok","provider":"beta"},
            {"id":"independent","account_id":"other","pool_ids":["other-pool"],"capabilities":["read","write"],"health":"up","auth":"ok","provider":"gamma"}
        ],
        "pools":[
            {"id":"shared-pool","windows":[{"id":"week","unit":"points","remaining_milli":remaining,"protected_milli":0,"reserved_milli":0,"confidence":if exact {"exact"} else {"estimated"},"expires_ms":2000}]},
            {"id":"other-pool","windows":[{"id":"week","unit":"points","remaining_milli":100000,"protected_milli":0,"reserved_milli":0,"confidence":"exact","expires_ms":2000}]}
        ]
    })
}

fn preview(d: &Daemon, snapshot: Value, allowed: &[&str], estimate: i64, finishing: i64) -> Value {
    d.call(
        "swarm.policy.preview",
        json!({"snapshot":snapshot,"request":{
            "now_ms":1200,"allowed_targets":allowed,"required_capabilities":["write"],
            "purpose":"worker","estimate_milli":{"points":estimate},
            "finishing_estimate_milli":{"points":finishing}
        }}),
    )
}

#[test]
fn preview_preserves_admitted_harness_identity_and_rejects_unknown_harness() {
    let d = Daemon::start(&[]);
    let mut target_snapshot = snapshot(Some(60000), true);
    target_snapshot["targets"][1]["harness"] = json!("claude");
    target_snapshot["targets"][1]["profile_id"] = json!("system-claude");
    target_snapshot["targets"][1]["model"] = json!("sonnet");
    target_snapshot["targets"][1]["effort"] = json!("medium");
    let result = preview(&d, target_snapshot.clone(), &["qualified"], 1000, 0);
    assert_eq!(result["targets"]["qualified"]["harness"], "claude");
    assert_eq!(result["targets"]["qualified"]["profile_id"], "system-claude");
    assert_eq!(result["targets"]["qualified"]["model"], "sonnet");
    assert_eq!(result["targets"]["qualified"]["effort"], "medium");
    assert_eq!(result["targets"]["cheap"]["harness"], "generic");
    let mut no_effort = target_snapshot.clone();
    no_effort["targets"][1]["effort"] = Value::Null;
    let result = preview(&d, no_effort, &["qualified"], 1000, 0);
    assert_eq!(result["targets"]["qualified"]["eligible"], true);
    assert!(result["targets"]["qualified"]["effort"].is_null());
    let mut incomplete = target_snapshot.clone();
    incomplete["targets"][1]["model"] = Value::Null;
    let error = d.try_call("swarm.policy.preview",json!({"snapshot":incomplete,
        "request":{"now_ms":1200,"allowed_targets":["qualified"],
            "required_capabilities":["write"],"purpose":"worker",
            "estimate_milli":{"points":1000}}})).unwrap_err();
    assert!(error.contains("incomplete or invalid target route"), "{error}");
    target_snapshot["targets"][1]["harness"] = json!("unknown");
    let error = d.try_call("swarm.policy.preview",json!({"snapshot":target_snapshot,
        "request":{"now_ms":1200,"allowed_targets":["qualified"],
            "required_capabilities":["write"],"purpose":"worker",
            "estimate_milli":{"points":1000}}})).unwrap_err();
    assert!(error.contains("invalid target harness"), "{error}");
}

#[test]
fn snapshot_and_allowed_target_identifiers_cannot_expose_credentials_in_status() {
    let d = Daemon::start(&[]);
    let secret = "sk-abcdefghijklmnopqrstuv";
    let request = |snapshot: Value| {
        let mut input = json!({"snapshot":snapshot,"request":{
            "now_ms":1200,"allowed_targets":["qualified"],
            "required_capabilities":["write"],"purpose":"worker",
            "estimate_milli":{"points":1000}}});
        input["request"]["estimate_milli"][secret] = json!(1000);
        input
    };
    for field in ["target", "account", "endpoint", "pool", "window", "unit", "model"] {
        let mut candidate = snapshot(Some(60000), true);
        match field {
            "target" => candidate["targets"][1]["id"] = json!(secret),
            "account" => candidate["targets"][1]["account_id"] = json!(secret),
            "endpoint" => candidate["targets"][1]["endpoint_id"] = json!(secret),
            "pool" => {
                candidate["targets"][1]["pool_ids"] = json!([secret]);
                candidate["pools"][0]["id"] = json!(secret);
            }
            "window" => candidate["pools"][0]["windows"][0]["id"] = json!(secret),
            "unit" => candidate["pools"][0]["windows"][0]["unit"] = json!(secret),
            "model" => {
                candidate["targets"][1]["harness"] = json!("claude");
                candidate["targets"][1]["profile_id"] = json!("profile");
                candidate["targets"][1]["model"] = json!(secret);
            }
            _ => unreachable!(),
        }
        let error = d.try_call("swarm.policy.preview", request(candidate)).unwrap_err();
        assert!(error.contains("invalid") && !error.contains(secret), "{field}: {error}");
    }
    let error = d.try_call("swarm.create", json!({"category":"Credential guard",
        "objective":"Audit backend","allowed_targets":[secret]})).unwrap_err();
    assert!(error.contains("invalid") && !error.contains(secret), "{error}");
    assert_eq!(d.call("swarm.list", json!({}))["runs"].as_array().unwrap().len(), 0);

    let created = d.call("swarm.create", json!({"category":"Credential guard",
        "objective":"Audit backend","allowed_targets":["qualified"]}));
    let mut contaminated = snapshot(Some(60000), true);
    contaminated["targets"][1]["pool_ids"] = json!([secret]);
    contaminated["pools"][0]["id"] = json!(secret);
    let error = d.try_call("swarm.availability.observe", json!({"run_id":created["id"],
        "snapshot":contaminated,"now_ms":1200,
        "required_capabilities":["write"],"purpose":"worker",
        "estimate_milli":{"points":1000}})).unwrap_err();
    assert!(error.contains("invalid") && !error.contains(secret), "{error}");
    assert!(d.call("swarm.get", json!({"id":created["id"]}))["availability"].is_null());
}

#[test]
fn policy_uses_capability_allowed_pool_and_finishing_headroom() {
    let d = Daemon::start(&[]);
    let result = preview(
        &d,
        snapshot(Some(60000), true),
        &["cheap", "qualified"],
        4000,
        0,
    );
    assert_eq!(result["defaults"]["max_workers"], 8);
    assert!(result["defaults"].get("max_executing").is_none());
    assert_eq!(result["targets"]["qualified"]["eligible"], true);
    assert_eq!(
        result["targets"]["qualified"]["windows"][0]["allocation_milli"],
        6000
    );
    assert_eq!(
        result["targets"]["qualified"]["windows"][0]["finishing_reserve_milli"],
        1200
    );
    assert_eq!(result["targets"]["cheap"]["reason"], "missing_capability");
    assert_eq!(result["targets"]["independent"]["reason"], "not_allowed");
    let too_large = preview(&d, snapshot(Some(60000), true), &["qualified"], 5000, 0);
    assert_eq!(
        too_large["targets"]["qualified"]["reason"],
        "finishing_reserve"
    );
    let costly_finish = preview(&d, snapshot(Some(60000), true), &["qualified"], 4000, 3500);
    assert_eq!(
        costly_finish["targets"]["qualified"]["reason"],
        "finishing_reserve"
    );
}

#[test]
fn unknown_stale_or_incompatible_units_do_not_enable_fanout() {
    let d = Daemon::start(&[]);
    let missing = preview(&d, snapshot(None, true), &["qualified"], 1000, 0);
    assert_eq!(missing["targets"]["qualified"]["reason"], "unknown_quota");
    let mut stale = snapshot(Some(60000), true);
    stale["expires_ms"] = json!(1100);
    let result = preview(&d, stale, ["qualified"].as_slice(), 1000, 0);
    assert_eq!(result["targets"]["qualified"]["reason"], "stale_snapshot");
    let mut unlike = snapshot(Some(60000), true);
    unlike["pools"][0]["windows"].as_array_mut().unwrap().push(json!({"id":"day","unit":"requests","remaining_milli":10000,"protected_milli":0,"reserved_milli":0,"confidence":"exact","expires_ms":2000}));
    let result = preview(&d, unlike, ["qualified"].as_slice(), 1000, 0);
    assert_eq!(result["targets"]["qualified"]["reason"], "missing_estimate");
    let estimated = preview(&d, snapshot(Some(60000), false), &["qualified"], 1000, 0);
    assert_eq!(
        estimated["targets"]["qualified"]["reason"],
        "estimated_quota_requires_permission"
    );
}

#[test]
fn each_window_binds_and_provider_label_does_not_change_policy() {
    let d = Daemon::start(&[]);
    let mut base = snapshot(Some(60000), true);
    base["pools"][0]["windows"].as_array_mut().unwrap().push(json!({"id":"hour","unit":"points","remaining_milli":20000,"protected_milli":0,"reserved_milli":0,"confidence":"exact","expires_ms":2000}));
    let result = preview(&d, base.clone(), &["qualified"], 2000, 0);
    assert_eq!(
        result["targets"]["qualified"]["reason"],
        "finishing_reserve"
    );
    base["targets"][1]["provider"] = json!("renamed-provider");
    let replay = preview(&d, base, &["qualified"], 2000, 0);
    assert_eq!(result, replay);
}

#[test]
fn affected_targets_fail_independently_and_estimated_permission_is_explicit() {
    let d = Daemon::start(&[]);
    let mut input = snapshot(Some(60000), true);
    input["targets"][0]["health"] = json!("down");
    input["targets"][1]["auth"] = json!("expired");
    let result = preview(
        &d,
        input,
        ["cheap", "qualified", "independent"].as_slice(),
        1000,
        0,
    );
    assert_eq!(result["targets"]["cheap"]["reason"], "target_unhealthy");
    assert_eq!(result["targets"]["qualified"]["reason"], "auth_unavailable");
    assert_eq!(result["targets"]["independent"]["eligible"], true);

    let estimated = d.call(
        "swarm.policy.preview",
        json!({
            "snapshot":snapshot(Some(60000),false),
            "request":{"now_ms":1200,"allowed_targets":["qualified"],
                "required_capabilities":["write"],"purpose":"worker",
                "estimate_milli":{"points":1000},"allow_estimated":true}
        }),
    );
    assert_eq!(estimated["targets"]["qualified"]["eligible"], true);
    assert_eq!(
        estimated["targets"]["qualified"]["windows"][0]["confidence"],
        "estimated"
    );
    assert_eq!(estimated["authoritative_reservation"], false);
}

#[test]
fn zero_upper_estimate_cannot_authorize_free_fanout() {
    let d=Daemon::start(&[]);
    let result=preview(&d,snapshot(Some(60000),true), &["qualified"],0,0);
    assert_eq!(result["targets"]["qualified"]["reason"],"uncalibrated_estimate");
}

#[test]
fn one_account_without_a_common_verified_pool_cannot_double_its_allowance() {
    let d=Daemon::start(&[]);
    let mut conflicting=snapshot(Some(60000),true);
    conflicting["targets"][1]["pool_ids"]=json!(["other-pool"]);
    let result=preview(&d,conflicting,&["qualified","independent"],1000,0);
    assert_eq!(result["targets"]["qualified"]["reason"],"account_pool_conflict");
    assert_eq!(result["targets"]["independent"]["eligible"],true);
    let mut overlapping=snapshot(Some(60000),true);
    overlapping["targets"][1]["pool_ids"]=json!(["shared-pool","other-pool"]);
    let result=preview(&d,overlapping,&["qualified"],1000,0);
    assert_eq!(result["targets"]["qualified"]["eligible"],true);
    assert_eq!(result["targets"]["qualified"]["windows"].as_array().unwrap().len(),2);
}

#[test]
fn revoked_account_blocks_all_of_its_target_aliases() {
    let d=Daemon::start(&[]);
    let mut input=snapshot(Some(60000),true);
    input["targets"][0]["auth"]=json!("revoked");
    let result=preview(&d,input.clone(),&["cheap","qualified","independent"],1000,0);
    assert_eq!(result["targets"]["cheap"]["reason"],"auth_unavailable");
    assert_eq!(result["targets"]["qualified"]["reason"],"auth_unavailable");
    assert_eq!(result["targets"]["independent"]["eligible"],true);
    let selected_alias=preview(&d,input,&["qualified","independent"],1000,0);
    assert_eq!(selected_alias["targets"]["qualified"]["reason"],"auth_unavailable");
    assert_eq!(selected_alias["targets"]["independent"]["eligible"],true);
}

#[test]
fn rate_limit_and_local_harness_failure_leave_another_provider_eligible() {
    let d=Daemon::start(&[]);
    let mut input=snapshot(Some(60000),true);
    input["targets"].as_array_mut().unwrap().truncate(2);
    input["targets"][0]["id"]=json!("opencode-provider-a");
    input["targets"][0]["account_id"]=json!("account-a");
    input["targets"][0]["capabilities"]=json!(["write"]);
    input["targets"][0]["health"]=json!("rate_limited");
    input["targets"][1]["id"]=json!("opencode-provider-b");
    input["targets"][1]["account_id"]=json!("account-b");
    input["targets"][1]["pool_ids"]=json!(["other-pool"]);
    let allowed=["opencode-provider-a","opencode-provider-b"];
    for (health,expected) in [
        ("rate_limited","rate_limited"),
        ("local_unavailable","local_harness_unavailable"),
        ("down","target_unhealthy"),
    ] {
        input["targets"][0]["health"]=json!(health);
        let result=preview(&d,input.clone(),&allowed,1000,0);
        assert_eq!(result["targets"][allowed[0]]["reason"],expected);
        assert_eq!(result["targets"][allowed[1]]["eligible"],true);
    }
    input["targets"][0]["health"]=json!("up");
    input["targets"][0]["auth"]=json!("expired");
    let auth=preview(&d,input.clone(),&allowed,1000,0);
    assert_eq!(auth["targets"][allowed[0]]["reason"],"auth_unavailable");
    assert_eq!(auth["targets"][allowed[1]]["eligible"],true);
    input["targets"][0]["auth"]=json!("ok");
    input["pools"][0]["windows"][0]["remaining_milli"]=json!(0);
    let quota=preview(&d,input,&allowed,1000,0);
    assert_eq!(quota["targets"][allowed[0]]["reason"],"quota_exhausted");
    assert_eq!(quota["targets"][allowed[1]]["eligible"],true);
}

#[test]
fn failures_follow_declared_account_endpoint_and_harness_scope() {
    let d=Daemon::start(&[]);
    let mut base=snapshot(Some(60000),true);
    for (index,endpoint) in [(0,"provider-a"),(1,"provider-a"),(2,"provider-b")] {
        base["targets"][index]["endpoint_id"]=json!(endpoint);
        base["targets"][index]["harness"]=json!("opencode");
        base["targets"][index]["profile_id"]=json!(format!("profile-{index}"));
        base["targets"][index]["model"]=json!("fixture-model");
    }
    let allowed=["cheap","qualified","independent"];
    let mut outage=base.clone();
    outage["targets"][0]["health"]=json!("down");
    outage["targets"][0]["health_scope"]=json!("endpoint");
    let result=preview(&d,outage,&allowed,1000,0);
    assert_eq!(result["targets"]["cheap"]["reason"],"target_unhealthy");
    assert_eq!(result["targets"]["qualified"]["reason"],"target_unhealthy");
    assert_eq!(result["targets"]["independent"]["eligible"],true,
        "healthy OpenCode provider B remains usable");

    let mut auth=base.clone();
    auth["targets"][0]["auth"]=json!("expired");
    let result=preview(&d,auth,&allowed,1000,0);
    assert_eq!(result["targets"]["qualified"]["reason"],"auth_unavailable");
    assert_eq!(result["targets"]["independent"]["eligible"],true);

    let mut limited=base.clone();
    limited["targets"][0]["health"]=json!("rate_limited");
    limited["targets"][0]["health_scope"]=json!("endpoint");
    let result=preview(&d,limited,&allowed,1000,0);
    assert_eq!(result["targets"]["qualified"]["reason"],"rate_limited");
    assert_eq!(result["targets"]["independent"]["eligible"],true);

    let mut account_limit=base.clone();
    account_limit["targets"][0]["health"]=json!("rate_limited");
    account_limit["targets"][0]["health_scope"]=json!("account");
    let result=preview(&d,account_limit,&allowed,1000,0);
    assert_eq!(result["targets"]["qualified"]["reason"],"rate_limited");
    assert_eq!(result["targets"]["independent"]["eligible"],true);

    let mut local=base.clone();
    local["targets"][0]["health"]=json!("local_unavailable");
    local["targets"][0]["health_scope"]=json!("harness");
    let result=preview(&d,local,&allowed,1000,0);
    assert_eq!(result["targets"]["qualified"]["reason"],"local_harness_unavailable");
    assert_eq!(result["targets"]["independent"]["reason"],"local_harness_unavailable",
        "a missing OpenCode binary blocks every OpenCode provider");

    let mut empty=base.clone();
    empty["pools"][0]["windows"][0]["remaining_milli"]=json!(0);
    let result=preview(&d,empty,&allowed,1000,0);
    assert_eq!(result["targets"]["qualified"]["reason"],"quota_exhausted");
    assert_eq!(result["targets"]["independent"]["eligible"],true);

    let mut malformed=base;
    malformed["targets"][0]["health"]=json!("down");
    malformed["targets"][0]["health_scope"]=json!("endpoint");
    malformed["targets"][0]["endpoint_id"]=Value::Null;
    assert!(d.try_call("swarm.policy.preview",json!({"snapshot":malformed,
        "request":{"now_ms":1200,"allowed_targets":allowed,
            "required_capabilities":["write"],"purpose":"worker",
            "estimate_milli":{"points":1000}}})).unwrap_err()
        .contains("invalid target health scope"));
}

#[test]
fn preview_uses_explicit_percentage_bounds_instead_of_builtin_values() {
    let d=Daemon::start(&[]);
    let result=d.call("swarm.policy.preview",json!({
        "snapshot":snapshot(Some(60000),true),
        "request":{"now_ms":1200,"allowed_targets":["qualified"],
            "required_capabilities":["write"],"purpose":"worker",
            "estimate_milli":{"points":8000},"allocation_percent":20,
            "finishing_reserve_percent":30}
    }));
    assert_eq!(result["targets"]["qualified"]["eligible"],true);
    assert_eq!(result["applied_percentages"]["run_allocation_percent"],20);
    assert_eq!(result["targets"]["qualified"]["windows"][0]["allocation_milli"],12000);
    assert_eq!(result["targets"]["qualified"]["windows"][0]["finishing_reserve_milli"],3600);
}
