//! AC-235: every surface names the account an agent runs on, as its harness reports it: the
//! provider and plan, the email with its local part shortened, and whose login it is. Uses the
//! SYNTHETIC account CLI (fixtures/fake-harness/account-cli.js); no real login is read.

mod common;
use common::*;
use serde_json::{json, Value};

fn fixture(name: &str) -> String {
    repo_root().join("fixtures").join(name).display().to_string()
}

struct Lab { d: Daemon, _t: tempfile::TempDir, next: std::path::PathBuf }

fn lab() -> Lab {
    let t = tmp();
    let sys = t.path().join("desktop-home");
    std::fs::create_dir_all(&sys).unwrap();
    let next = t.path().join("next-login");
    let cli = fixture("fake-harness/account-cli.js");
    let d = Daemon::start(&[("OVERSEER_CODEX_PATH", &cli), ("OVERSEER_CLAUDE_PATH", &cli), ("OVERSEER_TEST_SYSTEM_HOME", sys.to_str().unwrap()),
        ("OVERSEER_HARNESS_ENV_PASSTHROUGH", "FIXTURE_LOGIN_ACCOUNT_FILE,OVERSEER_TEST_SYSTEM_HOME"), ("FIXTURE_LOGIN_ACCOUNT_FILE", next.to_str().unwrap())]);
    Lab { d, _t: t, next }
}

impl Lab {
    fn sign_in(&self, id: &str, who: &str) {
        std::fs::write(&self.next, who).unwrap();
        let cmd = self.d.call("profile.login_command", json!({"id": id}));
        let mut c = std::process::Command::new(cmd["program"].as_str().unwrap());
        c.args(cmd["args"].as_array().unwrap().iter().map(|a| a.as_str().unwrap())).env("FIXTURE_LOGIN_ACCOUNT_FILE", &self.next);
        for (k, v) in cmd["env"].as_object().unwrap() { c.env(k, v.as_str().unwrap()); }
        assert!(c.status().unwrap().success());
    }
    fn account(&self, id: &str) -> Value {
        self.d.call("state", json!({}))["profiles"].as_array().unwrap().iter().find(|p| p["id"] == id).unwrap()["account"].clone()
    }
}

#[test]
fn ac235_the_default_login_and_a_named_account_show_provider_plan_and_shortened_email() {
    let lab = lab();
    let d = &lab.d;
    // Before any read: the provider and whose login it is, never "Your login".
    assert_eq!(lab.account("system-claude")["label"], "Claude · Mac's default login");
    let work = d.call("account.create", json!({"provider": "openai", "name": "Work ChatGPT"}))["account"]["id"].as_str().unwrap().to_string();
    assert_eq!(lab.account(&work)["label"], "ChatGPT · Work ChatGPT");

    lab.sign_in("system-claude", "bilal:max:bilal@testbox.com");
    lab.sign_in(&work, "worker:team:worker@acme.example");
    let cursor = d.call("state", json!({}))["cursor"].as_i64().unwrap();
    assert_eq!(d.call("profile.status", json!({"id": "system-claude"}))["logged_in"], true);
    assert_eq!(d.call("profile.status", json!({"id": work}))["logged_in"], true);

    let mac = lab.account("system-claude");
    assert_eq!(mac["label"], "Claude Max · bil…@testbox.com · Mac's default login");
    assert_eq!(mac["short"], "Claude Max · bil…@testbox.com");
    assert_eq!((mac["provider"].as_str(), mac["plan"].as_str(), mac["email"].as_str(), mac["default"].as_bool()), (Some("Claude"), Some("Max"), Some("bil…@testbox.com"), Some(true)));
    // The Codex account comes from Codex's own account read (app-server `account/read`).
    let named = lab.account(&work);
    assert_eq!(named["label"], "ChatGPT Team · wor…@acme.example · Work ChatGPT");
    assert_eq!(named["default"], false);
    // account.list carries the same, and the full address is never stored or sent.
    let list = d.call("account.list", json!({}));
    let listed = list["accounts"].as_array().unwrap().iter().find(|a| a["id"] == "system-claude").unwrap();
    assert_eq!(listed["account"], mac);
    let everything = format!("{}{}{}", d.call("state", json!({})), list, d.call("profile.list", json!({})));
    assert!(!everything.contains("bilal@testbox.com") && !everything.contains("worker@acme.example"), "a full address left the daemon");
    assert!(!everything.contains("Your login"));
    // The change reached every client as a profile event; reading it again changes nothing.
    let events = d.call("events.list", json!({"after": cursor}));
    let changed = |events: &Value| events.as_array().map(|e| e.iter().filter(|e| e["kind"] == "profile" && e["payload"]["action"] == "account").count()).unwrap_or(0);
    assert_eq!(changed(&events["events"]), 2, "{events}");
    let again = d.call("state", json!({}))["cursor"].as_i64().unwrap();
    d.call("profile.status", json!({"id": "system-claude"}));
    assert_eq!(changed(&d.call("events.list", json!({"after": again}))["events"]), 0);

    // The Mac's default login switches accounts: the label follows; signing out leaves no email.
    lab.sign_in("system-claude", "ana:pro:ana.silva@testbox.com");
    d.call("profile.status", json!({"id": "system-claude"}));
    assert_eq!(lab.account("system-claude")["label"], "Claude Pro · ana…@testbox.com · Mac's default login");
    d.call("profile.logout", json!({"id": work}));
    d.call("profile.status", json!({"id": work}));
    assert_eq!(lab.account(&work)["label"], "ChatGPT · Work ChatGPT");
    // Signed in again as the same account: named again at once.
    lab.sign_in(&work, "worker:team:worker@acme.example");
    d.call("profile.status", json!({"id": work}));
    assert_eq!(lab.account(&work)["label"], "ChatGPT Team · wor…@acme.example · Work ChatGPT");
}
