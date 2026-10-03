//! Persistent owner-selected text bindings. Resolution uses only stored data.
use super::{
    advance, error, expect_revision,
    library::{self, Bundle},
    revision,
};
use crate::{
    daemon::{now, Daemon},
    server,
    store::{Run, Store, Workspace},
};
use anyhow::Result;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    sync::Arc,
};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ModScope {
    AllAgents,
    Repository { repo_key: String },
    Watchers,
    Agent { run_id: String },
    Overseer,
}
impl ModScope {
    fn priority(&self) -> u8 {
        match self {
            Self::AllAgents => 0,
            Self::Repository { .. } => 1,
            Self::Watchers | Self::Overseer => 2,
            Self::Agent { .. } => 3,
        }
    }
    fn matches(&self, c: &ModContext) -> bool {
        match self {
            Self::AllAgents => c.role == "agent",
            Self::Repository { repo_key } => {
                c.repo_key == *repo_key && matches!(c.role.as_str(), "agent" | "watcher")
            }
            Self::Watchers => c.role == "watcher",
            Self::Agent { run_id } => c.run_id == *run_id,
            Self::Overseer => c.role == "overseer",
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ModFilters {
    #[serde(default)]
    pub harnesses: Vec<String>,
    #[serde(default)]
    pub accounts: Vec<String>,
    #[serde(default)]
    pub models: Vec<String>,
}
impl ModFilters {
    fn normalize(&mut self) -> Result<()> {
        for values in [&mut self.harnesses, &mut self.accounts, &mut self.models] {
            if values.len() > 64
                || values
                    .iter()
                    .any(|v| v.is_empty() || v.len() > 512 || v.chars().any(char::is_control))
            {
                return Err(error(
                    "invalid_mod",
                    "filters need at most 64 nonempty exact identifiers of at most 512 bytes",
                ));
            }
            values.sort();
            values.dedup();
        }
        Ok(())
    }
    fn matches_route(&self, c: &ModContext) -> bool {
        fn matches(values: &[String], actual: Option<&str>) -> bool {
            values.is_empty() || actual.is_some_and(|a| values.iter().any(|v| v == a))
        }
        matches(&self.harnesses, Some(&c.harness))
            && matches(&self.accounts, c.account_id.as_deref())
    }
    fn matches(&self, c: &ModContext) -> bool {
        self.matches_route(c)
            && (self.models.is_empty() || c.model.as_ref().is_some_and(|m| self.models.contains(m)))
    }
    fn overlaps(&self, other: &Self) -> bool {
        fn overlaps(a: &[String], b: &[String]) -> bool {
            a.is_empty() || b.is_empty() || a.iter().any(|v| b.contains(v))
        }
        overlaps(&self.harnesses, &other.harnesses)
            && overlaps(&self.accounts, &other.accounts)
            && overlaps(&self.models, &other.models)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModBindingInput {
    pub id: Option<String>,
    pub mod_id: String,
    pub version: String,
    pub fingerprint: String,
    pub scope: ModScope,
    pub enabled: bool,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub locked: bool,
    #[serde(default)]
    pub filters: ModFilters,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ModBinding {
    pub id: String,
    pub mod_id: String,
    pub version: String,
    pub fingerprint: String,
    pub scope: ModScope,
    pub enabled: bool,
    pub required: bool,
    pub locked: bool,
    pub filters: ModFilters,
    pub changed_ms: i64,
    pub actor: String,
}
#[derive(Clone, Debug, Serialize)]
pub struct ModContext {
    pub run_id: String,
    pub role: String,
    pub repo_key: String,
    pub harness: String,
    pub harness_version: Option<String>,
    pub account_id: Option<String>,
    pub model: Option<String>,
    pub native_thread_exists: bool,
    /// Continuity can choose/rechoose this route's model after turn admission.
    pub local_model_selection: bool,
}
#[derive(Clone, Debug, Serialize)]
pub struct ModDecision {
    pub binding_id: String,
    pub mod_id: String,
    pub fingerprint: String,
    pub status: &'static str,
    pub reason: &'static str,
    pub required: bool,
    pub delivery: &'static str,
    pub activation: &'static str,
    pub children: &'static str,
}
#[derive(Clone, Debug, Serialize)]
pub struct ModPlan {
    pub revision: i64,
    pub versions: Vec<Value>,
    pub rules_text: String,
    pub style_text: String,
    pub decisions: Vec<ModDecision>,
}

pub fn stored(store: &Store) -> Result<Vec<ModBinding>> {
    let mut stmt = store
        .conn
        .prepare("SELECT content FROM mod_bindings ORDER BY id")?;
    let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
    rows.map(|r| Ok(serde_json::from_str(&r?)?)).collect()
}

/// Callers already holding Store must use this form, never Daemon::run_role.
pub fn context_from_store(store: &Store, run: &Run, workspace: &Workspace) -> Result<ModContext> {
    let role: Option<String> = store
        .conn
        .query_row(
            "SELECT role FROM run_roles WHERE run_id=?1",
            [&run.id],
            |r| r.get(0),
        )
        .optional()?;
    let mut role = role.unwrap_or_else(|| {
        if run.parent_run_id.is_some() {
            "child".into()
        } else {
            "agent".into()
        }
    });
    // An existing ordinary agent may be assigned to a watch without a run_roles
    // mutation. This is a Mods selector only; it changes no tool/auth role. It
    // returns to the agents scope after its last active watch ends.
    if role == "agent" {
        let watching: bool = store.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM watches WHERE watcher=?1 AND ended_ms IS NULL)",
            [&run.id],
            |r| r.get(0),
        )?;
        if watching {
            role = "watcher".into();
        }
    }
    let account_id = run
        .profile_id
        .as_deref()
        .map(|id| store.auto_account_pool_id(id))
        .transpose()?
        .flatten();
    Ok(ModContext {
        run_id: run.id.clone(),
        role,
        repo_key: workspace.common_dir.clone(),
        harness: run.harness.clone(),
        harness_version: run.harness_version.clone(),
        account_id,
        model: run.model.clone(),
        native_thread_exists: run.native_id.is_some(),
        local_model_selection: crate::continuity::is_local(run),
    })
}
fn context(store: &Store, id: &str) -> Result<ModContext> {
    let run = store
        .run(id)?
        .ok_or_else(|| error("invalid_mod", "unknown run target"))?;
    let ws = store
        .workspace(&run.workspace_id)?
        .ok_or_else(|| error("invalid_mod", "unknown workspace target"))?;
    context_from_store(store, &run, &ws)
}

fn version<'a>(versions: &'a [Bundle], binding: &ModBinding) -> Result<&'a Bundle> {
    versions
        .iter()
        .find(|v| {
            v.manifest.id == binding.mod_id
                && v.manifest.version == binding.version
                && v.fingerprint == binding.fingerprint
        })
        .ok_or_else(|| {
            error(
                "mod_changed",
                "binding's pinned mod version is not installed",
            )
        })
}

pub fn resolve(
    versions: &[Bundle],
    bindings: &[ModBinding],
    c: &ModContext,
    revision: i64,
) -> Result<ModPlan> {
    let mut plan = resolve_inner(versions, bindings, c, revision)?;
    if !c.local_model_selection {
        return Ok(plan);
    }
    // This is qualification only, never model discovery or a changed security role.
    // Exact filters partition possible future models into declared ids plus one
    // unknown-model class. Reuse the same owner off/locked/style precedence.
    let known = |b: &&ModBinding| b.scope.matches(c) && b.filters.matches_route(c);
    let order = |a: &ModBinding, b: &ModBinding| {
        a.scope
            .priority()
            .cmp(&b.scope.priority())
            .then(a.id.cmp(&b.id))
    };
    let possible: Vec<_> = bindings
        .iter()
        .filter(known)
        .filter(|b| b.enabled && b.required)
        .filter(|b| {
            !bindings.iter().filter(known).any(|off| {
                off.mod_id == b.mod_id
                    && !off.enabled
                    && off.filters.models.is_empty()
                    && if off.locked {
                        !b.locked || order(off, b).is_le()
                    } else {
                        !b.locked && order(off, b).is_ge()
                    }
            })
        })
        .collect();
    if possible.is_empty() {
        return Ok(plan);
    }
    const MAX_DECLARED_MODELS: usize = 64;
    let mut models = BTreeSet::new();
    'scan: for b in bindings.iter().filter(known) {
        for m in &b.filters.models {
            models.insert(m.clone());
            if models.len() > MAX_DECLARED_MODELS {
                break 'scan;
            }
        }
    }
    let mut uncertain = BTreeSet::new();
    let mut reason = "Required applicability may change after local model selection; local text delivery is unqualified";
    if models.len() > MAX_DECLARED_MODELS {
        uncertain.extend(possible.iter().map(|b| b.id.as_str()));
        reason = "Local applicability exceeds 64 declared model candidates; narrow the model filters or disable required local bindings";
    } else {
        for model in std::iter::once(None).chain(models.into_iter().map(Some)) {
            let mut candidate = c.clone();
            candidate.model = model;
            candidate.local_model_selection = false;
            match resolve_inner(versions, bindings, &candidate, revision) {
                Ok(future) => {
                    for d in future
                        .decisions
                        .iter()
                        .filter(|d| d.required && d.status == "selected")
                    {
                        if let Some(b) = possible.iter().find(|b| b.id == d.binding_id) {
                            if !plan.decisions.iter().any(|now| {
                                now.binding_id == d.binding_id && now.status == "selected"
                            }) {
                                uncertain.insert(b.id.as_str());
                            }
                        }
                    }
                }
                Err(_) => {
                    // A future style conflict is not proof of non-applicability.
                    uncertain.extend(possible.iter().map(|b| b.id.as_str()));
                }
            }
        }
    }
    for d in &mut plan.decisions {
        if uncertain.contains(d.binding_id.as_str()) {
            d.status = "unqualified";
            d.reason = reason;
            d.delivery = "unsupported";
        }
    }
    Ok(plan)
}

fn resolve_inner(
    versions: &[Bundle],
    bindings: &[ModBinding],
    c: &ModContext,
    revision: i64,
) -> Result<ModPlan> {
    let mut candidates: BTreeMap<&str, Vec<&ModBinding>> = BTreeMap::new();
    let mut decisions = Vec::new();
    for b in bindings {
        let (status, reason) = if !b.scope.matches(c) {
            ("not_in_scope", "This binding targets another scope")
        } else if !b.filters.matches(c) {
            (
                "filtered",
                "The exact harness, account or model filter does not match",
            )
        } else {
            candidates.entry(&b.mod_id).or_default().push(b);
            ("overridden", "A narrower binding takes precedence")
        };
        decisions.push(ModDecision {
            binding_id: b.id.clone(),
            mod_id: b.mod_id.clone(),
            fingerprint: b.fingerprint.clone(),
            status,
            reason,
            required: b.required,
            delivery: "unsupported",
            activation: "next_turn",
            children: "unknown",
        });
    }
    let mut selected = Vec::new();
    for bindings in candidates.values_mut() {
        bindings.sort_by(|a, b| {
            a.scope
                .priority()
                .cmp(&b.scope.priority())
                .then(a.id.cmp(&b.id))
        });
        let winner = bindings
            .iter()
            .find(|b| b.locked)
            .copied()
            .unwrap_or_else(|| *bindings.last().unwrap());
        let decision = decisions
            .iter_mut()
            .find(|d| d.binding_id == winner.id)
            .unwrap();
        if winner.enabled {
            decision.status = "selected";
            decision.reason = if winner.locked {
                "The owner's locked binding holds"
            } else {
                "The most specific matching binding is enabled"
            };
            selected.push((winner, version(versions, winner)?));
        } else {
            decision.status = "disabled";
            decision.reason = if winner.locked {
                "The owner's locked off binding holds"
            } else {
                "The most specific matching binding is off"
            };
        }
    }
    selected.sort_by(|(a, _), (b, _)| {
        a.scope
            .priority()
            .cmp(&b.scope.priority())
            .then(a.mod_id.cmp(&b.mod_id))
            .then(a.id.cmp(&b.id))
    });
    let style_priority = selected
        .iter()
        .filter(|(_, v)| v.manifest.style.is_some())
        .map(|(b, _)| b.scope.priority())
        .max();
    let locked_styles: Vec<_> = selected
        .iter()
        .filter(|(b, v)| b.locked && v.manifest.style.is_some())
        .collect();
    let locked_style_holds = !locked_styles.is_empty();
    let style_candidates: Vec<_> = if locked_style_holds {
        locked_styles
    } else {
        selected
            .iter()
            .filter(|(b, v)| {
                v.manifest.style.is_some() && Some(b.scope.priority()) == style_priority
            })
            .collect()
    };
    if style_candidates.len() > 1 {
        return Err(error(
            "style_conflict",
            format!(
                "Styles conflict: {}",
                style_candidates
                    .iter()
                    .map(|(b, _)| b.mod_id.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        ));
    }
    let mut rules = Vec::new();
    let mut style_text = String::new();
    let mut public = Vec::new();
    for (b, v) in &selected {
        let rules_used = if let Some(r) = &v.manifest.rules {
            for path in &r.files {
                rules.push(
                    v.files
                        .get(path)
                        .ok_or_else(|| error("mod_changed", "pinned rule text is missing"))?
                        .clone(),
                );
            }
            !r.files.is_empty()
        } else {
            false
        };
        let style_used = style_candidates.iter().any(|(chosen, _)| chosen.id == b.id);
        if style_used {
            style_text = v
                .files
                .get(&v.manifest.style.as_ref().unwrap().file)
                .ok_or_else(|| error("mod_changed", "pinned style text is missing"))?
                .clone();
        }
        if rules_used || style_used {
            public.push(v.public());
        }
        if !style_used && v.manifest.style.is_some() {
            let decision = decisions.iter_mut().find(|d| d.binding_id == b.id).unwrap();
            decision.reason = if locked_style_holds && rules_used {
                "Rules selected; the owner's locked style holds"
            } else if locked_style_holds {
                "The owner's locked style holds"
            } else if rules_used {
                "Rules selected; a more specific style takes precedence"
            } else {
                "A more specific style takes precedence"
            };
            if !rules_used {
                decision.status = "overridden";
            }
        }
    }
    decisions.sort_by(|a, b| a.binding_id.cmp(&b.binding_id));
    Ok(ModPlan {
        revision,
        versions: public,
        rules_text: rules.join("\n\n"),
        style_text,
        decisions,
    })
}

fn scope_overlaps(store: &Store, a: &ModScope, b: &ModScope) -> Result<bool> {
    use ModScope::*;
    Ok(match (a, b) {
        (Agent { run_id: a }, Agent { run_id: b }) => a == b,
        (Agent { run_id }, other) | (other, Agent { run_id }) => {
            other.matches(&context(store, run_id)?)
        }
        (Repository { repo_key: a }, Repository { repo_key: b }) => a == b,
        (AllAgents, Watchers | Overseer) | (Watchers | Overseer, AllAgents) => false,
        (Overseer, Repository { .. } | Watchers) | (Repository { .. } | Watchers, Overseer) => {
            false
        }
        _ => true,
    })
}
fn validate_set(
    store: &Store,
    versions: &[Bundle],
    previous: &[ModBinding],
    new: &ModBinding,
) -> Result<()> {
    let v = version(versions, new)?;
    if let ModScope::Agent { run_id } = &new.scope {
        context(store, run_id)?;
    }
    for old in previous.iter().filter(|b| b.id != new.id) {
        if !scope_overlaps(store, &old.scope, &new.scope)? || !old.filters.overlaps(&new.filters) {
            continue;
        }
        if old.mod_id == new.mod_id {
            if old.locked && old.scope.priority() <= new.scope.priority() {
                return Err(error(
                    "binding_locked",
                    format!(
                        "Locked binding {} must be unlocked by the owner first",
                        old.id
                    ),
                ));
            }
            if new.locked && new.scope.priority() <= old.scope.priority() {
                return Err(error(
                    "binding_locked",
                    "Remove narrower overlapping bindings before locking this parent",
                ));
            }
            if old.scope.priority() == new.scope.priority() {
                return Err(error("binding_conflict", "Overlapping bindings for the same mod at the same scope priority need an explicit binding-id update"));
            }
        }
        if old.enabled
            && new.enabled
            && v.manifest.style.is_some()
            && version(versions, old)?.manifest.style.is_some()
            && (old.scope.priority() == new.scope.priority() || old.locked || new.locked)
        {
            return Err(error(
                "style_conflict",
                format!("Styles conflict: {}, {}", old.mod_id, new.mod_id),
            ));
        }
    }
    Ok(())
}

fn owner_only() -> Result<()> {
    if server::actor().is_some() {
        return Err(error(
            "mac_only",
            "Mod bindings are changed by the local owner",
        ));
    }
    Ok(())
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SetRequest {
    binding: ModBindingInput,
    expected_revision: i64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UnsetRequest {
    binding_id: String,
    expected_revision: i64,
}

pub fn set(d: &Arc<Daemon>, p: &Value) -> Result<Value> {
    owner_only()?;
    if let Some(kind) = p["binding"]["scope"]["kind"].as_str() {
        if !["all_agents", "repository", "watchers", "agent", "overseer"].contains(&kind) {
            return Err(error(
                "unsupported_scope",
                "This mod scope is not supported",
            ));
        }
    }
    let mut request: SetRequest = serde_json::from_value(p.clone())
        .map_err(|e| error("invalid_mod", format!("invalid binding: {e}")))?;
    request.binding.filters.normalize()?;
    if let ModScope::Repository { repo_key } = &mut request.binding.scope {
        if !Path::new(&*repo_key).is_absolute() {
            return Err(error(
                "invalid_mod",
                "repository key must be an absolute Git common directory",
            ));
        }
        *repo_key = std::fs::canonicalize(&*repo_key)?
            .to_string_lossy()
            .into_owned();
    }
    let (result, event) = {
        let store = d.store.lock().unwrap();
        expect_revision(
            &store,
            &json!({"expected_revision":request.expected_revision}),
        )?;
        let previous = stored(&store)?;
        if let Some(id) = &request.binding.id {
            if !previous.iter().any(|b| b.id == *id) {
                return Err(error("invalid_mod", "unknown binding id"));
            }
        }
        let i = request.binding;
        let binding = ModBinding {
            id: i
                .id
                .unwrap_or_else(|| format!("mb-{}", uuid::Uuid::new_v4().simple())),
            mod_id: i.mod_id,
            version: i.version,
            fingerprint: i.fingerprint,
            scope: i.scope,
            enabled: i.enabled,
            required: i.required,
            locked: i.locked,
            filters: i.filters,
            changed_ms: now(),
            actor: "owner".into(),
        };
        validate_set(&store, &library::versions(&store)?, &previous, &binding)?;
        let tx = store.conn.unchecked_transaction()?;
        tx.execute("INSERT INTO mod_bindings(id,content) VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET content=excluded.content", params![binding.id,serde_json::to_string(&binding)?])?;
        let revision = advance(&store)?;
        let event = store.insert_event(
            now(),
            None,
            None,
            "mods_changed",
            "user",
            "exact",
            &json!({"operation":"bind","binding":binding,"revision":revision}),
        )?;
        tx.commit()?;
        (json!({"binding":binding,"revision":revision}), event)
    };
    let _ = d.events.send(event);
    Ok(result)
}
pub fn unset(d: &Arc<Daemon>, p: &Value) -> Result<Value> {
    owner_only()?;
    let request: UnsetRequest = serde_json::from_value(p.clone())
        .map_err(|e| error("invalid_mod", format!("invalid unbind: {e}")))?;
    let (result, event) = {
        let store = d.store.lock().unwrap();
        expect_revision(
            &store,
            &json!({"expected_revision":request.expected_revision}),
        )?;
        if !stored(&store)?.iter().any(|b| b.id == request.binding_id) {
            return Err(error("invalid_mod", "unknown binding id"));
        }
        let tx = store.conn.unchecked_transaction()?;
        tx.execute(
            "DELETE FROM mod_bindings WHERE id=?1",
            [&request.binding_id],
        )?;
        let revision = advance(&store)?;
        let event = store.insert_event(
            now(),
            None,
            None,
            "mods_changed",
            "user",
            "exact",
            &json!({"operation":"unbind","binding_id":request.binding_id,"revision":revision}),
        )?;
        tx.commit()?;
        (json!({"revision":revision}), event)
    };
    let _ = d.events.send(event);
    Ok(result)
}
pub fn why(d: &Daemon, p: &Value) -> Result<Value> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Request {
        run_id: String,
    }
    let request: Request = serde_json::from_value(p.clone())
        .map_err(|e| error("invalid_mod", format!("invalid run target: {e}")))?;
    super::delivery::applied(d, &request.run_id)
}
