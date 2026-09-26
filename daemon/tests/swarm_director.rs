mod common;

use common::*;
use serde_json::json;
use std::time::{SystemTime, UNIX_EPOCH};

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

#[test]
fn director_batches_twenty_events_and_never_claims_two_active_turns() {
    let mut d = Daemon::start(&[]);
    let run = d.call(
        "swarm.create",
        json!({"category":"Batch","objective":"Audit","allowed_targets":["system-codex"]}),
    );
    let id = run["id"].as_str().unwrap();
    d.call("swarm.plan",json!({"id":id,"generation":1,"revision":0,"jobs":[{"id":"j","title":"J","acceptance":"evidence","deps":[]}]}));
    let attempt = d.call(
        "swarm.attempt.register",
        json!({"run_id":id,"generation":1,"revision":1,"job_id":"j"}),
    );
    let aid = attempt["id"].as_str().unwrap();
    let token = attempt["token"].as_str().unwrap();
    for n in 0..21 {
        d.call(
            "swarm.report",
            json!({"run_id":id,"job_id":"j","attempt_id":aid,"token":token,
            "message_id":format!("event-{n}"),"type":"progress","revision":1,"payload":{"step":n}}),
        );
    }
    let first = d.call(
        "swarm.director.claim_batch",
        json!({"run_id":id,"generation":1,"revision":1,"now_ms":now()}),
    );
    assert_eq!(first["status"], "claimed");
    assert_eq!(first["messages"].as_array().unwrap().len(), 20);
    assert_eq!(
        d.call(
            "swarm.director.claim_batch",
            json!({"run_id":id,"generation":1,"revision":1,"now_ms":now()+6000})
        )["status"],
        "busy"
    );
    d.kill9();
    d.spawn();
    assert_eq!(
        d.call(
            "swarm.director.claim_batch",
            json!({"run_id":id,"generation":1,"revision":1,"now_ms":now()+6000})
        )["status"],
        "busy"
    );
    let done = d.call(
        "swarm.director.complete_batch",
        json!({"run_id":id,"generation":1,"turn_id":first["turn_id"],"token":first["token"]}),
    );
    assert_eq!(done["applied"], 20);
    assert_eq!(
        d.call(
            "swarm.director.complete_batch",
            json!({"run_id":id,"generation":1,"turn_id":first["turn_id"],"token":first["token"]})
        )["duplicate"],
        true
    );
    let next = d.call(
        "swarm.director.claim_batch",
        json!({"run_id":id,"generation":1,"revision":1,"now_ms":now()+6000}),
    );
    assert_eq!(next["messages"].as_array().unwrap().len(), 1);
}

#[test]
fn director_waits_for_age_or_byte_limit_without_background_inference() {
    let d = Daemon::start(&[]);
    let run = d.call(
        "swarm.create",
        json!({"category":"Byte batch","objective":"Audit","allowed_targets":["system-codex"]}),
    );
    let id = run["id"].as_str().unwrap();
    d.call("swarm.plan",json!({"id":id,"generation":1,"revision":0,"jobs":[{"id":"j","title":"J","acceptance":"evidence","deps":[]}]}));
    let attempt = d.call(
        "swarm.attempt.register",
        json!({"run_id":id,"generation":1,"revision":1,"job_id":"j"}),
    );
    let aid = attempt["id"].as_str().unwrap();
    let token = attempt["token"].as_str().unwrap();
    assert_eq!(
        d.call(
            "swarm.director.claim_batch",
            json!({"run_id":id,"generation":1,"revision":1,"now_ms":now()})
        )["status"],
        "idle"
    );
    d.call(
        "swarm.report",
        json!({"run_id":id,"job_id":"j","attempt_id":aid,"token":token,
        "message_id":"one","type":"discovery","revision":1,"payload":{"finding":"x"}}),
    );
    assert_eq!(
        d.call(
            "swarm.director.claim_batch",
            json!({"run_id":id,"generation":1,"revision":1,"now_ms":now()})
        )["status"],
        "waiting"
    );
    let aged = d.call(
        "swarm.director.claim_batch",
        json!({"run_id":id,"generation":1,"revision":1,"now_ms":now()+6000}),
    );
    assert_eq!(aged["status"], "claimed");
    assert_eq!(aged["messages"].as_array().unwrap().len(), 1);
}

#[test]
fn inline_batch_limit_defers_excess_and_stop_halts_future_turns() {
    let d = Daemon::start(&[]);
    let run = d.call(
        "swarm.create",
        json!({"category":"Large updates","objective":"Audit","allowed_targets":["system-codex"]}),
    );
    let id = run["id"].as_str().unwrap();
    d.call("swarm.plan",json!({"id":id,"generation":1,"revision":0,"jobs":[{"id":"j","title":"J","acceptance":"evidence","deps":[]}]}));
    let attempt = d.call(
        "swarm.attempt.register",
        json!({"run_id":id,"generation":1,"revision":1,"job_id":"j"}),
    );
    let aid = attempt["id"].as_str().unwrap();
    let token = attempt["token"].as_str().unwrap();
    for n in 0..2 {
        d.call("swarm.report",json!({"run_id":id,"job_id":"j","attempt_id":aid,"token":token,
            "message_id":format!("large-{n}"),"type":"progress","revision":1,"payload":{"text":"x".repeat(20_000)}}));
    }
    let first = d.call(
        "swarm.director.claim_batch",
        json!({"run_id":id,"generation":1,"revision":1,"now_ms":now()}),
    );
    assert_eq!(first["messages"].as_array().unwrap().len(), 1);
    assert_eq!(first["more_pending"], true);
    assert!(first["inline_bytes"].as_i64().unwrap() <= 32 * 1024);
    d.call(
        "swarm.director.complete_batch",
        json!({"run_id":id,"generation":1,"turn_id":first["turn_id"],"token":first["token"]}),
    );
    d.call(
        "swarm.stop",
        json!({"run_id":id,"generation":1,"revision":1}),
    );
    let halted = d.call(
        "swarm.director.claim_batch",
        json!({"run_id":id,"generation":1,"revision":1,"now_ms":now()+6000}),
    );
    assert_eq!(halted["status"], "halted");
}

#[test]
fn uncertain_director_stalls_and_confirmed_replacement_replays_unapplied_batch() {
    let mut d = Daemon::start(&[]);
    let run=d.call("swarm.create",json!({"category":"Director recovery","objective":"Audit",
        "allowed_targets":["codex-a"]}));
    let id=run["id"].as_str().unwrap();
    d.call("swarm.plan",json!({"id":id,"generation":1,"revision":0,"jobs":[
        {"id":"j","title":"Inspect","acceptance":"evidence","deps":[]}
    ]}));
    let at=now();
    let attempt=d.call("swarm.admit",json!({"run_id":id,"generation":1,"revision":1,
        "job_id":"j","target_id":"codex-a","request_id":"director-recovery",
        "now_ms":at,"snapshot":{"version":1,"observed_ms":at-1000,"expires_ms":at+60000,
            "targets":[{"id":"codex-a","account_id":"account-a","pool_ids":["shared"],
                "capabilities":["code"],"health":"up","auth":"ok"}],
            "pools":[{"id":"shared","windows":[{"id":"week","unit":"points",
                "remaining_milli":100000,"protected_milli":0,"reserved_milli":0,
                "confidence":"exact","expires_ms":at+60000}]}]},
        "required_capabilities":["code"],"estimate_milli":{"points":1000},"purpose":"worker"}));
    assert_eq!(attempt["status"],"admitted");
    d.call("swarm.report",json!({"run_id":id,"job_id":"j","attempt_id":attempt["attempt_id"],
        "token":attempt["token"],"message_id":"discovery","type":"discovery",
        "revision":1,"payload":{"finding":"trace"}}));
    let first=d.call("swarm.director.claim_batch",json!({"run_id":id,"generation":1,
        "revision":1,"now_ms":at+6000}));
    assert_eq!(first["status"],"claimed");
    d.kill9();
    d.spawn();
    let uncertain=d.call("swarm.director.recover",json!({"run_id":id,"generation":1,
        "revision":1,"termination":"unknown"}));
    assert_eq!(uncertain["status"],"stalled");
    assert!(d.try_call("swarm.pause",json!({"run_id":id,"generation":1,"revision":1})).is_err());
    assert!(d.try_call("swarm.resume",json!({"run_id":id,"generation":1,"revision":1})).is_err());
    assert_eq!(d.call("swarm.director.claim_batch",json!({"run_id":id,"generation":1,
        "revision":1,"now_ms":at+7000}))["status"],"stalled");
    assert!(d.try_call("swarm.direct",json!({"run_id":id,"generation":1,"revision":1,
        "job_id":"j","attempt_id":attempt["attempt_id"],"message_id":"uncertain-director",
        "type":"redirect","payload":{}})).is_err());
    d.call("swarm.report",json!({"run_id":id,"job_id":"j","attempt_id":attempt["attempt_id"],
        "token":attempt["token"],"message_id":"late-result","type":"result",
        "revision":1,"payload":{"artifact_ids":[]}}));
    let db=rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let active: i64=db.query_row("SELECT COUNT(*) FROM swarm_reservations WHERE run_id=?1 AND status='active'",[id],|r|r.get(0)).unwrap();
    assert_eq!(active,1);
    let replaced=d.call("swarm.director.recover",json!({"run_id":id,"generation":1,
        "revision":1,"termination":"confirmed_dead"}));
    assert_eq!(replaced["generation"],2);
    assert!(d.try_call("swarm.director.complete_batch",json!({"run_id":id,"generation":1,
        "turn_id":first["turn_id"],"token":first["token"]})).is_err());
    assert!(d.try_call("swarm.direct",json!({"run_id":id,"generation":1,"revision":1,
        "job_id":"j","attempt_id":attempt["attempt_id"],"message_id":"old-director",
        "type":"redirect","payload":{}})).is_err());
    let replay=d.call("swarm.director.claim_batch",json!({"run_id":id,"generation":2,
        "revision":1,"now_ms":at+12000}));
    assert_eq!(replay["status"],"claimed");
    assert_eq!(replay["messages"].as_array().unwrap().len(),2);
    assert_eq!(replay["messages"][0]["message_id"],"discovery");
    assert_eq!(replay["messages"][1]["message_id"],"late-result");
}
