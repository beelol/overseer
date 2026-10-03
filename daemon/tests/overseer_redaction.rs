//! AC-200: redaction must preserve safe action semantics without exposing credentials.
mod common;
use common::*;
use serde_json::json;

const TOKEN: &str = "sk-proj-syntheticSYNTHETICabcdefghijkl012345";

fn message_round_trip(text: &str, password: Option<&str>, expected_text: &str, secrets: &[&str]) {
    let r = tmp();
    let repo = repo(&r.path().join("repo"));
    let d = Daemon::start(&[]);
    let run = run_id(&d.call("task.create", json!({"repo": repo, "harness": "generic", "workspace_mode": "worktree", "program": "/bin/sh", "args": ["-c", "IFS= read -r line; printf '%s\n' \"$line\""], "prompt": "Initial safe input", "title": "Notes"})));
    assert_eq!(d.wait_done(&run, 20)["status"], "completed");
    d.call("overseer.level", json!({"level": "ask_first"}));
    let mut action = json!({"action": "message", "agent": run, "text": text, "context": {"notes": ["Keep the useful context", 7, false]}});
    if let Some(password) = password {
        action["password"] = json!(password);
    }
    let proposed = d.call(
        "overseer.propose",
        json!({"actions": [action], "source": "ctl"}),
    );
    assert_eq!(proposed["state"], "open", "{proposed}");
    let id = proposed["proposal"].as_str().unwrap();
    let card_before = d.call("overseer.card", json!({"id": id}));
    let session_before = d.call("overseer.session", json!({}));
    let conn = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let stored: String = conn
        .query_row(
            "SELECT actions FROM overseer_proposals WHERE id=?1",
            [id],
            |row| row.get(0),
        )
        .unwrap();
    let yes = d.call(
        "overseer.answer",
        json!({"id": id, "yes": true, "surface": "ctl"}),
    );
    assert_eq!(yes["state"], "yes", "{yes}");
    assert_eq!(d.wait_done(&run, 20)["status"], "completed");
    let turns = d.call("run.turns", json!({"run_id": run}));
    let card_after = d.call("overseer.card", json!({"id": id}));
    let session_after = d.call("overseer.session", json!({}));
    let again = d
        .try_call(
            "overseer.answer",
            json!({"id": id, "yes": true, "surface": "ctl"}),
        )
        .unwrap_err();
    assert!(again.contains("already_answered"), "{again}");
    assert_eq!(
        d.call("run.turns", json!({"run_id": run}))
            .as_array()
            .unwrap()
            .len(),
        2,
        "confirm executes one follow-up"
    );
    for (surface, value) in [
        ("stored proposal", stored),
        ("open card", card_before.to_string()),
        ("session before", session_before.to_string()),
        ("delivered input", turns.to_string()),
        ("answered card", card_after.to_string()),
        ("session after", session_after.to_string()),
    ] {
        for secret in secrets {
            assert!(
                !value.contains(secret),
                "credential {secret:?} leaked in {surface}: {value}"
            );
        }
    }
    assert_eq!(card_before["actions"][0]["action"], "message");
    assert_eq!(card_before["actions"][0]["agent"], run);
    assert_eq!(
        card_before["actions"][0]["context"],
        json!({"notes": ["Keep the useful context", 7, false]})
    );
    assert_eq!(card_before["actions"][0]["text"], expected_text);
    if password.is_some() {
        assert_eq!(card_before["actions"][0]["password"], "[redacted]");
    }
    assert_eq!(
        turns[1]["prompt"],
        format!("From Overseer: {expected_text}")
    );
}

#[test]
fn ac200_escaped_secret_metadata_cannot_restore_the_original_action() {
    message_round_trip(
        &format!("Keep the notes complete. Token {TOKEN}."),
        Some("synthetic\"metadata-tail"),
        "Keep the notes complete. Token [redacted].",
        &[TOKEN, "metadata-tail"],
    );
}

#[test]
fn ac200_credentials_in_ordinary_message_text_are_redacted_before_delivery() {
    message_round_trip(
        r#"Keep the notes complete: {"password":"synthetic-ordinary-secret"}."#,
        None,
        r#"Keep the notes complete: {"password": "[redacted]"}."#,
        &["synthetic-ordinary-secret"],
    );
}

#[test]
fn ac200_escaped_credential_text_does_not_leak_its_suffix() {
    message_round_trip(
        r#"Keep the notes complete: {"password":"synthetic\"escaped-tail"}."#,
        None,
        r#"Keep the notes complete: {"password": "[redacted]"}."#,
        &["synthetic", "escaped-tail"],
    );
}

#[test]
fn ac200_legacy_card_read_redacts_decoded_credentials_without_mutating_history() {
    let d = Daemon::start(&[]);
    let session = d.call("overseer.session", json!({}));
    let conn = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let actions = json!([{"action":"message", "agent":"r-legacy", "text":format!(r#"Keep {TOKEN} and {{"secret":"synthetic\"legacy-tail"}} useful."#),
        "password":"synthetic\"metadata-tail", "context":{"notes":["Preserve this",7,false]}}]);
    let stored = actions.to_string();
    conn.execute("INSERT INTO overseer_proposals(id, session_id, ts, actions, state, source) VALUES(?1, ?2, 1, ?3, 'cancelled', 'fixture')",
        rusqlite::params!["p-legacy", session["id"].as_str().unwrap(), stored]).unwrap();
    let card = d.call("overseer.card", json!({"id":"p-legacy"}));
    assert_eq!(card["state"], "cancelled");
    assert_eq!(
        card["actions"][0],
        json!({"action":"message", "agent":"r-legacy",
        "text":r#"Keep [redacted] and {"secret": "[redacted]"} useful."#,
        "password":"[redacted]", "context":{"notes":["Preserve this",7,false]}})
    );
    let after: String = conn
        .query_row(
            "SELECT actions FROM overseer_proposals WHERE id='p-legacy'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        after, stored,
        "read sanitization does not rewrite historical records"
    );
}

#[test]
fn ac200_credential_shaped_metadata_keys_do_not_reach_cards() {
    let d = Daemon::start(&[]);
    d.call("overseer.level", json!({"level":"ask_first"}));
    let mut context = json!({"notes":["Keep this context",7,false]});
    context[TOKEN] = json!("safe value");
    let proposed = d.call("overseer.propose", json!({"actions":[{"action":"cadence", "cadence":"off", "context":context}], "source":"ctl"}));
    let id = proposed["proposal"].as_str().unwrap();
    let card = d.call("overseer.card", json!({"id":id}));
    let session = d.call("overseer.session", json!({}));
    let conn = rusqlite::Connection::open(d.home.path().join("overseer.sqlite")).unwrap();
    let stored: String = conn
        .query_row(
            "SELECT actions FROM overseer_proposals WHERE id=?1",
            [id],
            |row| row.get(0),
        )
        .unwrap();
    for value in [stored, card.to_string(), session.to_string()] {
        assert!(
            !value.contains(TOKEN),
            "credential-shaped key persisted: {value}"
        );
    }
    assert_eq!(
        card["actions"][0]["context"],
        json!({"notes":["Keep this context",7,false],"[redacted]":"safe value"})
    );
    let yes = d.call(
        "overseer.answer",
        json!({"id":id,"yes":true,"surface":"ctl"}),
    );
    assert_eq!(yes["state"], "yes", "{yes}");
}
