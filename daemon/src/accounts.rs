//! Simple account governance (AC-46, docs/rfcs/account-governance.md). Accounts are the
//! existing profiles viewed by provider: a provider issues the login, an account is a named
//! login with its own credential folder (fixed) or the desktop app's login (follows the app).
//! Account login only: no API keys, and API-key logins show as not usable.

use crate::adapters;
use crate::daemon::{now, Daemon, ACTIVE};
use crate::paths;
use crate::store::Profile;
use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};

pub fn provider_of(harness: &str) -> &'static str {
    match harness {
        "codex" | "codex-app" => "openai",
        "claude" => "anthropic",
        "opencode" | "opencode-serve" => "local",
        _ => "none",
    }
}

fn harness_for(provider: &str) -> Option<&'static str> {
    match provider {
        "openai" => Some("codex"),
        "anthropic" => Some("claude"),
        "local" => Some("opencode"),
        _ => None,
    }
}

pub fn providers() -> Value {
    let installed = |h: &str| adapters::resolve_program(h).is_some();
    json!([
        {"id": "openai", "label": "OpenAI / ChatGPT", "harnesses": ["codex", "codex-app"], "available": installed("codex"),
         "sign_in": "ChatGPT sign-in in the browser, or a device code", "why": if installed("codex") { Value::Null } else { json!("Codex CLI is not installed") }},
        {"id": "anthropic", "label": "Anthropic / Claude", "harnesses": ["claude"], "available": installed("claude"),
         "sign_in": "Claude account sign-in (claude auth login)", "why": if installed("claude") { Value::Null } else { json!("Claude Code is not installed") }},
        {"id": "local", "label": "OpenCode (local models)", "harnesses": ["opencode", "opencode-serve"], "available": installed("opencode"),
         "sign_in": "none: local providers and mocks (owner decision 2026-09-25)", "why": if installed("opencode") { Value::Null } else { json!("OpenCode is not installed") }},
        {"id": "devin", "label": "Devin", "harnesses": [], "available": false, "why": "Devin has no account-login CLI yet (only API keys, which Overseer does not use)"},
    ])
}

/// What the default login is called on every surface (AC-235): never "Your login".
pub const DEFAULT_LOGIN: &str = "Mac's default login";

/// An email as every surface shows it (AC-235): the local part cut to three letters,
/// "bil…@testbox.com". Anything that is not a plain address is not shown at all.
pub fn short_email(email: &str) -> Option<String> {
    let email = email.trim();
    let (local, domain) = email.rsplit_once('@')?;
    if local.is_empty() || domain.is_empty() || email.len() > 256 || !domain.contains('.')
        || email.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return None;
    }
    let kept: String = local.chars().take(3).collect();
    let cut = local.chars().count() > 3;
    Some(format!("{kept}{}@{}", if cut { "…" } else { "" }, domain.to_ascii_lowercase()))
}

/// A plan as the owner reads it: "Max", "Team", "Pro Lite", never `pro_lite`.
pub fn plan_words(plan: &str) -> Option<String> {
    let words: Vec<String> = plan.split(|c: char| c == '_' || c == '-' || c.is_whitespace()).filter(|w| !w.is_empty())
        .map(|w| { let mut c = w.chars(); c.next().map(|f| f.to_uppercase().collect::<String>() + &c.as_str().to_ascii_lowercase()).unwrap_or_default() })
        .collect();
    (!words.is_empty() && plan.len() <= 40).then(|| words.join(" "))
}

/// The provider an account signs in to, as its own name: Claude, ChatGPT.
pub fn provider_words(harness: &str) -> &'static str {
    match harness {
        "claude" => "Claude",
        "codex" | "codex-app" => "ChatGPT",
        "opencode" | "opencode-serve" => "OpenCode",
        _ => "Account",
    }
}

/// How every surface names the account an agent runs on (AC-235): the provider and its plan,
/// the email with its local part shortened, and whose login it is (the Mac's default login, or the
/// account's own name). `short` leaves out whose login when the email already says it.
pub fn shown(p: &Profile, email: Option<&str>, plan: Option<&str>) -> Value {
    let provider = provider_words(&p.harness);
    let plan = plan.and_then(plan_words);
    let email = email.filter(|e| !e.is_empty());
    let who = if p.is_system { DEFAULT_LOGIN.to_string() } else { p.name.clone() };
    let head = match &plan { Some(plan) => format!("{provider} {plan}"), None => provider.to_string() };
    let label = [Some(head.clone()), email.map(str::to_string), Some(who.clone())].into_iter().flatten().collect::<Vec<_>>().join(" · ");
    let short = format!("{head} · {}", email.map(str::to_string).unwrap_or_else(|| who.clone()));
    json!({"provider": provider, "plan": plan, "email": email, "default": p.is_system, "name": who, "label": label, "short": short})
}

fn follows(p: &Profile) -> Option<&'static str> {
    if !p.is_system { return None; }
    Some(match p.harness.as_str() { "codex" => "the ChatGPT / Codex app login (~/.codex)", "claude" => "the Claude app login (~/.claude)", _ => "OpenCode's own configuration" })
}

impl Daemon {
    pub fn account_list(&self) -> Result<Value> {
        let store = self.store.lock().unwrap();
        let runs = store.runs()?;
        let list: Vec<Value> = store.profiles()?.into_iter().map(|p| {
            let used = runs.iter().filter(|r| r.profile_id.as_deref() == Some(&p.id)).map(|r| r.created_ms).max();
            let active = runs.iter().any(|r| r.profile_id.as_deref() == Some(&p.id) && ACTIVE.contains(&r.status.as_str()));
            let harnesses: Vec<&str> = match p.harness.as_str() { "codex" => vec!["codex", "codex-app"], h => vec![h] };
            json!({"id": p.id, "name": p.name, "provider": provider_of(&p.harness), "harness_family": p.harness, "harnesses": harnesses,
                   "kind": if p.is_system { "follows-app" } else { "fixed" }, "follows": follows(&p), "last_used_ms": used, "active_runs": active,
                   "removable": !p.is_system, "account": p.account})
        }).collect();
        Ok(json!({"accounts": list, "providers": providers()}))
    }

    pub fn account_create(&self, provider: &str, name: &str) -> Result<Value> {
        let harness = harness_for(provider).ok_or_else(|| anyhow!("{provider} has no account login Overseer can use (no API keys)"))?;
        let profile = self.create_profile(name, harness)?;
        Ok(json!({"account": profile, "provider": provider}))
    }

    /// Removes a fixed account: its record and its own credential folder only.
    pub fn account_remove(&self, id: &str) -> Result<Value> {
        let profile = self.profile(id)?;
        if profile.is_system {
            bail!("{} follows the desktop app's login; Overseer never removes or signs out desktop logins", profile.name);
        }
        let busy = self.store.lock().unwrap().runs()?.into_iter().any(|r| r.profile_id.as_deref() == Some(id) && ACTIVE.contains(&r.status.as_str()));
        if busy {
            bail!("{} has active runs; stop them before removing the account", profile.name);
        }
        let home = profile.home.clone().ok_or_else(|| anyhow!("account has no folder"))?;
        let home = std::path::PathBuf::from(home);
        let root = std::fs::canonicalize(paths::profiles_dir())?;
        let canon = std::fs::canonicalize(&home).unwrap_or(home.clone());
        if !canon.starts_with(&root) || canon == root {
            bail!("refusing to remove a folder outside Overseer's profiles directory");
        }
        std::fs::remove_dir_all(&canon)?;
        self.store.lock().unwrap().delete_profile(id)?;
        self.emit(None, None, "profile", "user", "exact", json!({"profile_id": id, "action": "removed", "at": now()}))?;
        Ok(json!({"removed": id}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(name: &str, harness: &str, system: bool) -> Profile {
        Profile { id: "p".into(), name: name.into(), harness: harness.into(), home: None, is_system: system, created_ms: 0, account: None }
    }

    #[test]
    fn an_email_keeps_three_letters_of_its_local_part_and_its_domain() {
        assert_eq!(short_email("bilal@testbox.com").as_deref(), Some("bil…@testbox.com"));
        assert_eq!(short_email(" Ana@Work.Example ").as_deref(), Some("Ana@work.example"));
        assert_eq!(short_email("bob@x.io").as_deref(), Some("bob@x.io"));
        assert_eq!(short_email("émilie@exemple.fr").as_deref(), Some("émi…@exemple.fr"));
        for bad in ["", "no-at-sign", "@testbox.com", "bilal@", "bilal@localhost", "a b@c.com"] {
            assert_eq!(short_email(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn a_plan_reads_as_words() {
        assert_eq!(plan_words("max").as_deref(), Some("Max"));
        assert_eq!(plan_words("pro_lite").as_deref(), Some("Pro Lite"));
        assert_eq!(plan_words("TEAM").as_deref(), Some("Team"));
        assert_eq!(plan_words("").as_deref(), None);
    }

    #[test]
    fn every_account_names_its_provider_plan_email_and_whose_login_it_is() {
        let mac = shown(&profile("claude (existing login)", "claude", true), Some("bil…@testbox.com"), Some("max"));
        assert_eq!(mac["label"], "Claude Max · bil…@testbox.com · Mac's default login");
        assert_eq!(mac["short"], "Claude Max · bil…@testbox.com");
        assert_eq!(mac["default"], true);
        let work = shown(&profile("Work ChatGPT", "codex", false), Some("wor…@acme.example"), Some("team"));
        assert_eq!(work["label"], "ChatGPT Team · wor…@acme.example · Work ChatGPT");
        // Not read yet: the provider and whose login, never "Your login" and never the harness id.
        let unread = shown(&profile("codex (existing login)", "codex", true), None, None);
        assert_eq!(unread["label"], "ChatGPT · Mac's default login");
        assert_eq!(unread["short"], "ChatGPT · Mac's default login");
        for v in [&mac, &work, &unread] {
            let text = v.to_string();
            assert!(!text.contains("Your login") && !text.contains("existing login") && !text.contains("claude") && !text.contains("codex"), "{text}");
        }
    }
}
