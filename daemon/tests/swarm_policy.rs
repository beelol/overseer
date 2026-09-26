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
