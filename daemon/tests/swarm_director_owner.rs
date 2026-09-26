mod common;

use common::*;
use serde_json::json;
use std::time::{SystemTime, UNIX_EPOCH};

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as i64
}

#[test]
fn durable_owner_fences_director_actions_after_restart_and_replacement() {
    let mut d = Daemon::start(&[]);
    let run = d.call("swarm.create", json!({"category":"Owner fencing",
        "objective":"Audit","allowed_targets":["system-codex"]}));
    let id = run["id"].as_str().unwrap();
    let owner = d.call("swarm.director.owner.begin", json!({"run_id":id,"generation":1}));
    let secret = owner["owner_token"].as_str().unwrap();
    assert!(!d.call("swarm.get",json!({"id":id})).to_string().contains(secret));
    let jobs = json!([{"id":"j","title":"Inspect","acceptance":"evidence","deps":[]}]);
    let plan = json!({"id":id,"generation":1,"revision":0,"jobs":jobs});
    assert!(d.try_call("swarm.plan",plan.clone()).is_err());
    assert!(d.try_call("swarm.plan",json!({"id":id,"generation":1,"revision":0,
        "jobs":jobs,"owner_token":"wrong"})).is_err());
    d.call("swarm.plan",json!({"id":id,"generation":1,"revision":0,
        "jobs":jobs,"owner_token":secret}));
    let attempt = d.call("swarm.attempt.register",json!({"run_id":id,
        "generation":1,"revision":1,"job_id":"j","owner_token":secret}));
    d.call("swarm.report",json!({"run_id":id,"job_id":"j",
        "attempt_id":attempt["id"],"token":attempt["token"],
        "message_id":"finding","type":"discovery","revision":1,
        "payload":{"symbol":"TaskRepository.findById"}}));
    let claim = json!({"run_id":id,"generation":1,"revision":1,"now_ms":now()+6000});
    assert!(d.try_call("swarm.director.claim_batch",claim.clone()).is_err());
    let turn = d.call("swarm.director.claim_batch",json!({"run_id":id,
        "generation":1,"revision":1,"now_ms":now()+6000,"owner_token":secret}));
    assert_eq!(turn["status"],"claimed");
    let directive = json!({"run_id":id,"generation":1,"revision":1,
        "job_id":"j","attempt_id":attempt["id"],"message_id":"focus",
        "type":"advisory","payload":{"focus":"authorization"}});
    assert!(d.try_call("swarm.direct",directive.clone()).is_err());
    d.call("swarm.direct",json!({"run_id":id,"generation":1,"revision":1,
        "job_id":"j","attempt_id":attempt["id"],"message_id":"focus",
        "type":"advisory","payload":{"focus":"authorization"},"owner_token":secret}));
    assert!(d.try_call("swarm.director.complete_batch",json!({"run_id":id,
        "generation":1,"turn_id":turn["turn_id"],"token":turn["token"]})).is_err());
    d.call("swarm.director.complete_batch",json!({"run_id":id,
        "generation":1,"turn_id":turn["turn_id"],"token":turn["token"],
        "owner_token":secret}));
    d.call("swarm.artifact.put",json!({"run_id":id,"job_id":"j",
        "attempt_id":attempt["id"],"token":attempt["token"],
        "artifact_id":"proof","source_revision":1,"kind":"finding",
        "content":"verified"}));
    d.call("swarm.report",json!({"run_id":id,"job_id":"j",
        "attempt_id":attempt["id"],"token":attempt["token"],
        "message_id":"result","type":"result","revision":1,
        "payload":{"artifact_ids":["proof"]}}));
    assert!(d.try_call("swarm.decide",json!({"run_id":id,"generation":1,
        "revision":1,"job_id":"j","decision":"accept","evidence":["proof"]})).is_err());
    assert_eq!(d.call("swarm.decide",json!({"run_id":id,"generation":1,
        "revision":1,"job_id":"j","decision":"accept","evidence":["proof"],
        "owner_token":secret}))["status"],"accepted");
    d.kill9();
    d.spawn();
    assert!(d.try_call("swarm.revise",json!({"id":id,"generation":1,
        "expected_revision":1,"reason":"No change","jobs":jobs})).is_err());
    assert_eq!(d.call("swarm.revise",json!({"id":id,"generation":1,
        "expected_revision":1,"reason":"No change","jobs":jobs,
        "owner_token":secret}))["unchanged"],true);
    d.call("swarm.director.recover",json!({"run_id":id,"generation":1,
        "revision":1,"termination":"unknown"}));
    let replaced = d.call("swarm.director.recover",json!({"run_id":id,"generation":1,
        "revision":1,"termination":"confirmed_dead"}));
    assert_eq!(replaced["generation"],2);
    let awaiting = d.call("swarm.get",json!({"id":id}));
    assert_eq!(awaiting["status"],"stalled");
    assert_eq!(awaiting["stall_reason"],"director_replacement_pending");
    assert!(d.try_call("swarm.revise",json!({"id":id,"generation":2,
        "expected_revision":1,"reason":"No change","jobs":jobs,
        "owner_token":secret})).is_err());
    let next = d.call("swarm.director.owner.begin",json!({"run_id":id,"generation":2}));
    assert_ne!(next["owner_token"],owner["owner_token"]);
    assert_eq!(d.call("swarm.get",json!({"id":id}))["status"],"running");
    assert_eq!(d.call("swarm.revise",json!({"id":id,"generation":2,
        "expected_revision":1,"reason":"No change","jobs":jobs,
        "owner_token":next["owner_token"]}))["unchanged"],true);
}

#[test]
fn owner_lease_expiry_blocks_mutation_without_authorizing_a_second_owner() {
    let d = Daemon::start(&[]);
    let run = d.call("swarm.create",json!({"category":"Lease expiry",
        "objective":"Audit","allowed_targets":["system-codex"]}));
    let id = run["id"].as_str().unwrap();
    let owner = d.call("swarm.director.owner.begin",json!({"run_id":id,"generation":1}));
    assert!(d.try_call("swarm.director.owner.begin",json!({"run_id":id,
        "generation":1})).is_err());
    assert!(d.try_call("swarm.director.owner.renew",json!({"run_id":id,
        "generation":1,"owner_token":"wrong"})).is_err());
    let renewed = d.call("swarm.director.owner.renew",json!({"run_id":id,
        "generation":1,"owner_token":owner["owner_token"]}));
    assert!(renewed["lease_expires_ms"].as_i64().unwrap()
        >= owner["lease_expires_ms"].as_i64().unwrap());
    d.call("swarm.plan",json!({"id":id,"generation":1,"revision":0,
        "owner_token":owner["owner_token"],"jobs":[{"id":"j","title":"J",
        "acceptance":"evidence","deps":[]}]}));
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    db.execute("UPDATE swarm_director_owners SET lease_expires_ms=1 WHERE run_id=?1",[id]).unwrap();
    d.call("swarm.director.owner.expire_due",json!({}));
    let uncertain = d.call("swarm.get",json!({"id":id}));
    assert_eq!(uncertain["status"],"stalled");
    assert_eq!(uncertain["stall_reason"],"director_termination_unknown");
    let error = d.try_call("swarm.attempt.register",json!({"run_id":id,
        "generation":1,"revision":1,"job_id":"j",
        "owner_token":owner["owner_token"]})).unwrap_err();
    assert!(error.contains("lease expired"),"{error}");
    let at = now();
    let scheduled = d.call("swarm.schedule.next",json!({"request_id":"expired-owner",
        "target_id":"system-codex","now_ms":at,
        "snapshot":{"version":1,"observed_ms":at-1000,"expires_ms":at+60000,
            "targets":[{"id":"system-codex","account_id":"account",
                "pool_ids":["pool"],"capabilities":["code"],
                "health":"up","auth":"ok"}],
            "pools":[{"id":"pool","windows":[{"id":"week","unit":"points",
                "remaining_milli":100000,"protected_milli":0,"reserved_milli":0,
                "confidence":"exact","expires_ms":at+60000}]}]},
        "required_capabilities":["code"],"estimate_milli":{"points":100},
        "purpose":"worker"}));
    assert_eq!(scheduled["status"],"blocked","{scheduled}");
    assert_eq!(scheduled["reason"],"no_ready_category","{scheduled}");
    assert!(d.try_call("swarm.director.owner.begin",json!({"run_id":id,
        "generation":1})).is_err());
    let attempts: i64 = db.query_row("SELECT COUNT(*) FROM swarm_attempts WHERE run_id=?1",
        [id],|row|row.get(0)).unwrap();
    assert_eq!(attempts,0);
}

#[test]
fn binding_cannot_take_over_an_existing_unowned_director_turn() {
    let d = Daemon::start(&[]);
    let run = d.call("swarm.create",json!({"category":"Unowned turn",
        "objective":"Audit","allowed_targets":["system-codex"]}));
    let id = run["id"].as_str().unwrap();
    d.call("swarm.plan",json!({"id":id,"generation":1,"revision":0,
        "jobs":[{"id":"j","title":"J","acceptance":"evidence","deps":[]}]}));
    let attempt = d.call("swarm.attempt.register",json!({"run_id":id,
        "generation":1,"revision":1,"job_id":"j"}));
    d.call("swarm.report",json!({"run_id":id,"job_id":"j",
        "attempt_id":attempt["id"],"token":attempt["token"],
        "message_id":"discovery","type":"discovery","revision":1,
        "payload":{"finding":"inspect"}}));
    let turn = d.call("swarm.director.claim_batch",json!({"run_id":id,
        "generation":1,"revision":1,"now_ms":now()+6000}));
    assert_eq!(turn["status"],"claimed");
    assert!(d.try_call("swarm.director.owner.begin",json!({"run_id":id,
        "generation":1})).is_err());
    d.call("swarm.director.complete_batch",json!({"run_id":id,"generation":1,
        "turn_id":turn["turn_id"],"token":turn["token"]}));
    assert!(d.try_call("swarm.director.owner.begin",json!({"run_id":id,
        "generation":1})).is_ok());
}
