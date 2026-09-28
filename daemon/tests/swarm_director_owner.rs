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
    // Acceptance now waits for the worker to apply the director's advisory (9759b031).
    for phase in ["delivered","applied"] {
        d.call("swarm.ack",json!({"run_id":id,"message_id":"focus",
            "recipient":attempt["id"],"token":attempt["token"],"revision":1,"phase":phase}));
    }
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

/// SWARM-30: after a director is replaced, the stale generation can neither
/// dispatch nor accept. Director 1 plans two jobs and admits one worker; the
/// worker submits its result while director 1 dies (confirmed). Director 1's
/// admission of the second job and its acceptance of the result are refused,
/// with its old generation and token and with its token under the new
/// generation. The worker's result and reservation are kept; director 2
/// accepts that result and admits the second job.
#[test]
fn replaced_director_cannot_dispatch_or_accept_and_worker_results_survive() {
    let mut d = Daemon::start(&[]);
    let run = d.call("swarm.create", json!({"category":"Stale director fence",
        "objective":"Audit","allowed_targets":["fixture"]}));
    let id = run["id"].as_str().unwrap().to_string();
    let first = d.call("swarm.director.owner.begin", json!({"run_id":id,"generation":1}));
    let old = first["owner_token"].as_str().unwrap().to_string();
    d.call("swarm.plan", json!({"id":id,"generation":1,"revision":0,"owner_token":old,"jobs":[
        {"id":"a","title":"Tasks","acceptance":"evidence","deps":[]},
        {"id":"b","title":"Attachments","acceptance":"evidence","deps":[]}]}));
    let at = now();
    let snapshot = json!({"version":1,"observed_ms":at-1000,"expires_ms":at+600000,
        "targets":[{"id":"fixture","account_id":"account","pool_ids":["pool"],
            "capabilities":["audit"],"health":"up","auth":"ok"}],
        "pools":[{"id":"pool","windows":[{"id":"week","unit":"points",
            "remaining_milli":1000000,"protected_milli":0,"reserved_milli":0,
            "confidence":"exact","expires_ms":at+600000}]}]});
    let admit = |d: &Daemon, job: &str, generation: i64, token: &str| d.try_call("swarm.admit",
        json!({"run_id":id,"generation":generation,"revision":1,"job_id":job,"target_id":"fixture",
            "request_id":format!("admit-{job}-{generation}-{}", &token[..6]),"snapshot":snapshot,
            "now_ms":at,"required_capabilities":["audit"],"estimate_milli":{"points":100},
            "purpose":"worker","owner_token":token}));
    let workers: Vec<serde_json::Value> = ["a","b"].iter().map(|job| json!({"id":job,
        "elapsed_ms":100,"usage_milli":{"points":10}})).collect();
    let cost = json!({"elapsed_ms":10,"usage_milli":{"points":1}});
    let serial = json!({"planning":cost,"context":cost,"integration":cost,"review":cost,
        "retries":cost,"workers":workers});
    let mut parallel = serial.clone();
    parallel["context"]["elapsed_ms"] = json!(20);
    let decision = d.call("swarm.benefit.commit", json!({"run_id":id,"generation":1,"revision":1,
        "owner_token":old,"estimate":{"independent":true,"max_workers":2,
        "allocation_milli":{"points":100000},"finishing_reserve_milli":{"points":20000},
        "serial":serial,"parallel":parallel}}));
    assert_eq!(decision["decision"], "parallel", "{decision}");
    let a = admit(&d, "a", 1, &old).unwrap();
    assert_eq!(a["status"], "admitted", "{a}");
    d.call("swarm.artifact.put", json!({"run_id":id,"job_id":"a","attempt_id":a["attempt_id"],
        "token":a["token"],"artifact_id":"a-proof","source_revision":1,"kind":"finding",
        "content":"checked"}));
    d.call("swarm.report", json!({"run_id":id,"job_id":"a","attempt_id":a["attempt_id"],
        "token":a["token"],"message_id":"a-result","type":"result","revision":1,
        "payload":{"artifact_ids":["a-proof"]}}));
    d.call("swarm.director.recover", json!({"run_id":id,"generation":1,"revision":1,
        "termination":"unknown"}));
    // While termination is uncertain, nothing new is admitted.
    let held = admit(&d, "b", 1, &old);
    let held = held.unwrap();
    assert_eq!((held["status"].as_str(), held["reason"].as_str()), (Some("blocked"), Some("run_not_admitting")));
    d.kill9();
    d.spawn();
    let replaced = d.call("swarm.director.recover", json!({"run_id":id,"generation":1,"revision":1,
        "termination":"confirmed_dead"}));
    assert_eq!(replaced["generation"], 2);
    let second = d.call("swarm.director.owner.begin", json!({"run_id":id,"generation":2}));
    let new = second["owner_token"].as_str().unwrap().to_string();
    // The stale director: its dispatch and its acceptance are refused.
    for (generation, token) in [(1, old.as_str()), (2, old.as_str())] {
        let dispatch = admit(&d, "b", generation, token);
        let want = if generation == 1 { "stale or inactive director owner generation" } else { "invalid director owner identity" };
        assert_eq!(dispatch.unwrap_err(), want, "stale dispatch g{generation}");
        let accept = d.try_call("swarm.decide", json!({"run_id":id,"generation":generation,
            "revision":1,"job_id":"a","decision":"accept","evidence":["a-proof"],"owner_token":token}));
        assert_eq!(accept.unwrap_err(), want, "stale acceptance g{generation}");
    }
    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let (attempts_b, decisions): (i64, i64) = db.query_row("SELECT
        (SELECT COUNT(*) FROM swarm_attempts WHERE run_id=?1 AND job_id='b'),
        (SELECT COUNT(*) FROM swarm_decisions WHERE run_id=?1)", [&id], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
    assert_eq!((attempts_b, decisions), (0, 0), "the stale director changed nothing");
    let reserved: i64 = db.query_row("SELECT COUNT(*) FROM swarm_reservations WHERE run_id=?1
        AND attempt_id=?2 AND status!='released'", [&id, &a["attempt_id"].as_str().unwrap().to_string()],
        |r| r.get(0)).unwrap();
    assert_eq!(reserved, 1, "the worker's reservation is kept across the replacement");
    // The replacement uses the durable plan: it reviews the kept result and dispatches b.
    let batch = d.call("swarm.director.claim_batch", json!({"run_id":id,"generation":2,
        "revision":1,"now_ms":now()+6000,"owner_token":new}));
    assert_eq!(batch["status"], "claimed", "{batch}");
    assert!(batch["messages"].as_array().unwrap().iter().any(|m| m["message_id"] == "a-result"), "{batch}");
    assert_eq!(d.call("swarm.decide", json!({"run_id":id,"generation":2,"revision":1,"job_id":"a",
        "decision":"accept","evidence":["a-proof"],"owner_token":new}))["status"], "accepted");
    d.call("swarm.director.complete_batch", json!({"run_id":id,"generation":2,
        "turn_id":batch["turn_id"],"token":batch["token"],"owner_token":new}));
    let b = admit(&d, "b", 2, &new).unwrap();
    assert_eq!(b["status"], "admitted", "{b}");
}
