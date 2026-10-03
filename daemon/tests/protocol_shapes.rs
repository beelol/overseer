//! Gate N, AC-134: the protocol description (protocol/protocol.json) is what the daemon really
//! sends. Real results and real events are checked against its shapes, and the app's types are
//! generated from the same file, so a method cannot change on one side only.

mod common;

use common::*;
use serde_json::{json, Value};

fn description() -> Value {
    serde_json::from_str(&std::fs::read_to_string(repo_root().join("protocol/protocol.json")).unwrap()).unwrap()
}

/// Checks `value` against `shape`. Every field of an object must be described, every field that
/// is not marked `?` must be there, and every type must match. Returns what is wrong, with its path.
fn check(doc: &Value, shape: &Value, value: &Value, at: &str, wrong: &mut Vec<String>) {
    if let Some(fields) = shape.as_object() {
        let Some(actual) = value.as_object() else {
            wrong.push(format!("{at}: expected an object, got {value}"));
            return;
        };
        for (name, s) in fields {
            let optional = s.as_str().is_some_and(|t| t.trim_end().ends_with('?'));
            match actual.get(name) {
                None | Some(Value::Null) if optional => {}
                None => wrong.push(format!("{at}.{name}: missing")),
                Some(v) => check(doc, s, v, &format!("{at}.{name}"), wrong),
            }
        }
        for name in actual.keys() {
            if !fields.contains_key(name) {
                wrong.push(format!("{at}.{name}: not in the description"));
            }
        }
        return;
    }
    let mut text = shape.as_str().unwrap_or_else(|| panic!("{at}: a shape is text or an object")).trim();
    if let Some(inner) = text.strip_suffix('?') {
        if value.is_null() {
            return;
        }
        text = inner;
    }
    if let Some(inner) = text.strip_suffix("[]") {
        match value.as_array() {
            Some(items) => items.iter().enumerate().for_each(|(i, v)| check(doc, &json!(inner), v, &format!("{at}[{i}]"), wrong)),
            None => wrong.push(format!("{at}: expected a list, got {value}")),
        }
        return;
    }
    if let Some(inner) = text.strip_prefix("record<").and_then(|t| t.strip_suffix('>')) {
        match value.as_object() {
            Some(map) => map.iter().for_each(|(k, v)| check(doc, &json!(inner), v, &format!("{at}.{k}"), wrong)),
            None => wrong.push(format!("{at}: expected an object, got {value}")),
        }
        return;
    }
    if text.contains('|') && !text.contains('\'') {
        for variant in text.split('|') {
            let mut variant_wrong = Vec::new();
            check(doc, &json!(variant.trim()), value, at, &mut variant_wrong);
            if variant_wrong.is_empty() { return; }
        }
        wrong.push(format!("{at}: expected one of {text}, got {value}"));
        return;
    }
    if text.contains('\'') {
        let allowed: Vec<&str> = text.split('|').map(|s| s.trim().trim_matches('\'')).collect();
        if !value.as_str().is_some_and(|v| allowed.contains(&v)) {
            wrong.push(format!("{at}: expected one of {allowed:?}, got {value}"));
        }
        return;
    }
    let ok = match text {
        "json" => true,
        "string" => value.is_string(),
        "number" => value.is_number(),
        "boolean" => value.is_boolean(),
        "null" => value.is_null(),
        name => {
            let named = &doc["types"][name];
            assert!(!named.is_null(), "{at}: the description has no type {name}");
            check(doc, named, value, at, wrong);
            true
        }
    };
    if !ok {
        wrong.push(format!("{at}: expected {text}, got {value}"));
    }
}

#[test]
fn ac134_the_daemon_sends_what_the_description_says() {
    let doc = description();
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    std::fs::write(repo.join("README.md"), "# fixture\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "readme"]);
    let mode = r.path().join("mode");
    let claude = repo_root().join("fixtures/fake-harness/claude-fixture.js").display().to_string();
    let d = Daemon::start(&[("OVERSEER_CLAUDE_PATH", &claude), ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "CLAUDE_FIXTURE_MODE_FILE"), ("CLAUDE_FIXTURE_MODE_FILE", mode.to_str().unwrap()), ("OVERSEER_GATEWAY_MDNS", "off")]);
    let mut wrong = Vec::new();
    let mut checked = Vec::new();
    let mut call = |method: &str, params: Value| -> Value {
        let m = &doc["methods"][method];
        assert!(!m.is_null(), "{method} is not in the description");
        let mut bad = Vec::new();
        check(&doc, &m["params"], &params, &format!("{method} params"), &mut bad);
        // Optional parameters may be left out, so only what was sent is compared with the description.
        wrong.extend(bad.into_iter().filter(|w| !w.ends_with(": missing")));
        let result = d.call(method, params);
        check(&doc, &m["result"], &result, &format!("{method} result"), &mut wrong);
        checked.push(method.to_string());
        result
    };
    // A realistic session: tools, edits, a permission request, a child, errors, a follow-up.
    std::fs::write(&mode, "showcase-permission").unwrap();
    let created = call("task.create", json!({"repo": repo, "harness": "claude", "prompt": "do the showcase", "title": "showcase"}));
    let (run, ws, task) = (run_id(&created), created["workspace"]["id"].as_str().unwrap().to_string(), created["task"]["id"].as_str().unwrap().to_string());
    let waiting = d.wait_status(&run, |s| s == "waiting_for_user", 20);
    call("state", json!({}));
    call("run.active", json!({}));
    call("run.permission", json!({"run_id": run, "request_id": waiting["attention"]["request_id"], "allow": true}));
    d.wait_done(&run, 20);
    for mode_name in ["nested", "auth", "ratelimit", "echo"] {
        std::fs::write(&mode, mode_name).unwrap();
        let c = d.call("task.create", json!({"repo": repo, "harness": "claude", "prompt": mode_name, "title": mode_name}));
        d.wait_done(&run_id(&c), 20);
    }
    let base = option(&d, &run, "task_start", None)["base"].as_str().unwrap().to_string();
    call("run.follow_up", json!({"run_id": run, "prompt": "again"}));
    d.wait_done(&run, 20);
    call("run.interrupt", json!({"run_id":run}));
    let paused = call("run.follow_up", json!({"run_id":run,"prompt":"wait for explicit resume"}));
    assert_eq!(paused, json!({"delivery":"queued"}));
    call("state", json!({}));
    d.call("run.resume_queue", json!({"run_id":run}));
    d.wait_done(&run, 20);
    for (method, params) in [
        ("harness.list", json!({})), ("profile.list", json!({})), ("profile.status", json!({"id": "system-claude"})), ("repo.inspect", json!({"path": repo})), ("repo.known", json!({})),
        ("repo.files", json!({"workspace_id": ws, "query": "read"})), ("run.turns", json!({"run_id": run})), ("run.raw_output", json!({"run_id": run, "max_bytes": 2000})),
        ("events.list", json!({"run_id": run, "limit": 50})), ("comparison.options", json!({"run_id": run})), ("workspace.diff", json!({"workspace_id": ws, "base": base})),
        ("workspace.diff", json!({"workspace_id": ws, "base": base, "status": false})), ("workspace.status", json!({"workspace_id": ws})), ("workspace.changes", json!({"workspace_id": ws})),
        ("workspace.tree", json!({"workspace_id": ws, "dir": ""})), ("workspace.tree", json!({"workspace_id": ws, "dir": "src"})), ("workspace.file", json!({"workspace_id": ws, "path": "README.md", "base": base})),
        ("workspace.file", json!({"workspace_id": ws, "path": "nothing-here.txt"})), ("workspace.hunks", json!({"workspace_id": ws, "path": "README.md", "base": base, "run_id": run})),
        ("workspace.cleanup_plan", json!({"workspace_id": ws})), ("workspace.pr_plan", json!({"workspace_id": ws})), ("workspace.merge_plan", json!({"workspace_id": ws})),
        ("review.marks", json!({"run_id": run})), ("account.list", json!({})), ("account.usage", json!({"id": "system-claude"})), ("search", json!({"query": "showcase"})),
        ("daemon.last_notice", json!({})), ("daemon.clients", json!({})), ("task.archive", json!({"task_id": task})), ("task.archive", json!({"task_id": task, "archived": false})), ("runs.stop_all", json!({})),
    ] {
        call(method, params);
    }
    let hunks = d.call("workspace.hunks", json!({"workspace_id": ws, "path": "README.md", "base": base}));
    let first = hunks["hunks"][0].clone();
    call("review.accept", json!({"run_id": run, "path": "README.md", "key": first["key"], "modified_start": first["modified_start"], "modified_lines": first["modified_lines"], "base_lines": first["base_lines"]}));
    call("review.marks", json!({"run_id": run}));
    call("review.unaccept", json!({"run_id": run, "key": first["key"]}));
    call("review.reject", json!({"workspace_id": ws, "path": "README.md", "base": base, "key": first["key"]}));
    call("account.create", json!({"provider": "anthropic", "name": "described"}));
    assert!(wrong.is_empty(), "results that differ from protocol/protocol.json:\n{}", wrong.join("\n"));
    checked.sort();
    checked.dedup();
    assert!(checked.len() >= 36, "{} methods were checked: {checked:?}", checked.len());

    // Every event of those sessions, by its kind.
    let mut events = Vec::new();
    let mut after = 0;
    loop {
        let page = d.call("events.list", json!({"after": after, "limit": 5000}))["events"].as_array().unwrap().clone();
        if page.is_empty() {
            break;
        }
        after = page.last().unwrap()["seq"].as_i64().unwrap();
        events.extend(page);
    }
    let mut kinds = std::collections::BTreeMap::new();
    let mut not_described = std::collections::BTreeSet::new();
    for e in &events {
        check(&doc, &json!("Event"), e, &format!("event {}", e["seq"]), &mut wrong);
        let kind = e["kind"].as_str().unwrap();
        match doc["events"].get(kind) {
            Some(shape) => {
                check(&doc, shape, &e["payload"], &format!("event {} ({kind})", e["seq"]), &mut wrong);
                *kinds.entry(kind.to_string()).or_insert(0) += 1;
            }
            None => {
                not_described.insert(kind.to_string());
            }
        }
    }
    assert!(wrong.is_empty(), "events that differ from protocol/protocol.json:\n{}", wrong.join("\n"));
    for kind in ["task_created", "turn_started", "turn_done", "status", "output", "tool", "tool_result", "file_activity", "permission", "permission_answered", "usage", "error", "review_mark", "review_reject", "task_archived", "child", "child_reparented"] {
        assert!(kinds.contains_key(kind), "{kind} was checked ({kinds:?})");
    }
    // What the app does not read is allowed to be there, and is named here so it is a choice.
    // overseer_message is Overseer's own conversation (Gate S): the phone reads it with Talk to Overseer (AC-128).
    let quiet: std::collections::BTreeSet<String> = ["daemon_started", "interrupt_requested", "reattached", "daemon_stopping", "background_notice", "workspace_removed", "daemon_error", "merge_back", "overseer_message"].iter().map(|s| s.to_string()).collect();
    let surprising: Vec<&String> = not_described.difference(&quiet).collect();
    assert!(surprising.is_empty(), "event kinds with no description: {surprising:?}");
    println!("{} methods and {} events of {} kinds match the description", checked.len(), events.len(), kinds.len());
}

#[test]
fn ac134_the_apps_types_are_generated_from_the_description() {
    let root = repo_root();
    let out = std::process::Command::new("node").arg(root.join("protocol/gen-ts.mjs")).arg("--check").output().expect("node");
    assert!(out.status.success(), "{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    // A method changed in the description without regenerating is caught.
    let file = root.join("protocol/protocol.json");
    let original = std::fs::read_to_string(&file).unwrap();
    let changed = original.replacen("\"now_ms\": \"number\"", "\"now_ms\": \"string\"", 1);
    assert_ne!(changed, original);
    std::fs::write(&file, &changed).unwrap();
    let stale = std::process::Command::new("node").arg(root.join("protocol/gen-ts.mjs")).arg("--check").output();
    std::fs::write(&file, &original).unwrap();
    assert!(!stale.unwrap().status.success(), "a changed description must fail the check until the types are generated again");
    // Every method the daemon has is in the generated file, with its class.
    let generated = std::fs::read_to_string(root.join("phone/protocol/protocol.generated.ts")).unwrap();
    let doc = description();
    for (name, m) in doc["methods"].as_object().unwrap() {
        if m["planned"].as_bool() == Some(true) {
            continue;
        }
        assert!(generated.contains(&format!("  \"{name}\": '{}',", m["class"].as_str().unwrap())), "{name} is in the generated types with its class");
    }
}

#[test]
fn ac265_follow_up_description_rejects_false_queued_and_turn_acknowledgements() {
    let doc = description();
    for value in [json!({"delivery":"sent"}), json!({"delivery":"queued","started_ms":1}), json!({"id":"not-a-turn"})] {
        let mut wrong = Vec::new();
        check(&doc, &doc["methods"]["run.follow_up"]["result"], &value, "run.follow_up", &mut wrong);
        assert!(!wrong.is_empty(), "invalid result passed validation: {value}");
    }
}
