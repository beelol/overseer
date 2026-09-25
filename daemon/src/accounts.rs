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
        "opencode" => "local",
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
        {"id": "local", "label": "OpenCode (local models)", "harnesses": ["opencode"], "available": installed("opencode"),
         "sign_in": "none: local providers and mocks (owner decision 2026-09-25)", "why": if installed("opencode") { Value::Null } else { json!("OpenCode is not installed") }},
        {"id": "devin", "label": "Devin", "harnesses": [], "available": false, "why": "Devin has no account-login CLI yet (only API keys, which Overseer does not use)"},
    ])
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
                   "removable": !p.is_system})
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
