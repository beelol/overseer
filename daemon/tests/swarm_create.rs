mod common;

use common::*;
use serde_json::json;

#[test]
fn start_request_replays_once_after_restart_and_cannot_change_its_payload() {
    let mut d = Daemon::start(&[]);
    let request = json!({"request_scope":"window-1","request_id":"start-1",
        "category":"Backend security","objective":"Audit tenant isolation",
        "allowed_targets":["approved-a"]});
    let first = d.call("swarm.create", request.clone());
    let id = first["id"].as_str().unwrap().to_string();
    assert_eq!(first["duplicate"], false);
    let replay = d.call("swarm.create", request.clone());
    assert_eq!(replay["id"], id);
    assert_eq!(replay["duplicate"], true);
    d.kill9();
    d.spawn();
    assert_eq!(d.call("swarm.create", request.clone())["id"], id);

    let db = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let count: i64 = db.query_row("SELECT COUNT(*) FROM swarm_runs WHERE category_key='backend security'",
        [], |row| row.get(0)).unwrap();
    assert_eq!(count, 1);
    let changed = d.try_call("swarm.create", json!({"request_scope":"window-1",
        "request_id":"start-1","category":"Backend security",
        "objective":"Change the objective","allowed_targets":["approved-a"]})).unwrap_err();
    assert!(changed.contains("reused with different"), "{changed}");
    assert!(d.try_call("swarm.create", json!({"request_scope":"window-1",
        "request_id":"start-2","category":"Backend security",
        "objective":"Audit tenant isolation","allowed_targets":["approved-a"]})).is_err());
}

#[test]
fn start_request_ids_are_scoped_to_the_originating_client() {
    let d = Daemon::start(&[]);
    let one = d.call("swarm.create", json!({"request_scope":"phone-a","request_id":"7",
        "category":"API audit","objective":"Inspect API"}));
    let two = d.call("swarm.create", json!({"request_scope":"phone-b","request_id":"7",
        "category":"Queue audit","objective":"Inspect queue"}));
    assert_ne!(one["id"], two["id"]);
    assert_eq!(d.call("swarm.create", json!({"request_scope":"phone-a","request_id":"7",
        "category":"API audit","objective":"Inspect API"}))["id"], one["id"]);

    let local = d.call("swarm.create", json!({"request_id":"local-start",
        "category":"Local audit","objective":"Inspect locally"}));
    assert_eq!(d.call("swarm.create", json!({"request_scope":"local",
        "request_id":"local-start","category":"Local audit",
        "objective":"Inspect locally"}))["id"], local["id"]);
}
