//! The normal Swarm start (scenario S0): a category, an objective and Start.
//! Without a confirmation the call returns one compact read-back of what the
//! run would inherit: the approved account pool, the allocation rule and each
//! account's current reading, the worker ceiling, the deadline and the
//! director. The owner's yes is that read-back's digest; with it the daemon
//! commits the run and launches its director through the one launch path
//! (the director's durable app-slot hold, its owner identity and its bound
//! run). No per-worker question or settings form is part of it.
//!
//! Director choice: a director needs daemon-issued Swarm tools and enforced
//! descendant control. No model harness has either yet, so outside the
//! fixture API the start is visibly blocked (`no_qualified_director`) and no
//! weaker substitute runs. Under `OVERSEER_SWARM_FIXTURE_API=1` a scripted
//! director named by `OVERSEER_SWARM_FIXTURE_DIRECTOR` is the qualified one.

use super::{required, runtime, settings};
use crate::daemon::Daemon;
use crate::store::Store;
use anyhow::{anyhow, bail, Result};
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::sync::{Arc, Mutex};

/// Serializes confirmed starts, so one request id creates one run and one
/// director even when a client retries concurrently.
static START: Mutex<()> = Mutex::new(());

struct Director {
    program: String,
    args: Vec<String>,
}

fn director_choice() -> std::result::Result<Director, &'static str> {
    if std::env::var("OVERSEER_SWARM_FIXTURE_API").as_deref() != Ok("1") {
        return Err("no_qualified_director");
    }
    let Ok(raw) = std::env::var("OVERSEER_SWARM_FIXTURE_DIRECTOR") else {
        return Err("no_qualified_director");
    };
    let Ok(config) = serde_json::from_str::<Value>(&raw) else {
        return Err("no_qualified_director");
    };
    let program = config["program"].as_str().unwrap_or_default().to_string();
    let args: Option<Vec<String>> = config["args"].as_array()
        .map(|args| args.iter().filter_map(|a| a.as_str().map(str::to_string)).collect());
    match args {
        Some(args) if program.starts_with('/') && std::path::Path::new(&program).is_file()
            && args.len() <= 30 && args.iter().all(|a| a.len() <= 4096) => Ok(Director { program, args }),
        _ => Err("no_qualified_director"),
    }
}

/// What the owner is shown for one approved target. A target that names an
/// account profile reports that account's latest structured reading; any
/// other target's allowance is unknown to the daemon (under the fixture API,
/// a fixture target's allowance is supplied by the director's snapshot).
fn account(store: &Store, target: &str, effective: &Value, now: i64) -> Result<Value> {
    let Some(profile) = store.profile(target)? else {
        let fixture = std::env::var("OVERSEER_SWARM_FIXTURE_API").as_deref() == Ok("1");
        return Ok(json!({"target":target,"kind":if fixture { "fixture" } else { "unregistered" },
            "quota":if fixture { "fixture" } else { "unknown" }}));
    };
    let percent = effective["run_allocation_percent"].as_i64().unwrap_or(10);
    let reserve_percent = effective["finishing_reserve_percent"].as_i64().unwrap_or(20);
    let latest = store.latest_auto_quota(target)?;
    let (quota, observed_ms, windows) = match &latest {
        None => ("unknown", Value::Null, json!([])),
        Some(reading) => {
            let snapshot = &reading.snapshot;
            let state = if snapshot.needs_refresh(now) { "stale" }
                else if snapshot.ordinary_usage_allowed == Some(true) { "measured" }
                else if snapshot.ordinary_usage_allowed == Some(false) { "exhausted" }
                else { "allowance_unknown" };
            let windows: Vec<Value> = snapshot.windows.iter().map(|w| {
                let remaining = ((100.0 - w.used_percent.clamp(0.0, 100.0)) * 1000.0).floor() as i64;
                let allocation = remaining.saturating_mul(percent) / 100;
                json!({"window":w.window,"remaining_milli":remaining,
                    "allocation_milli":allocation,
                    "reserve_milli":allocation.saturating_mul(reserve_percent) / 100,
                    "reset_ms":w.reset_ms})
            }).collect();
            (state, json!(snapshot.observed_ms), json!(windows))
        }
    };
    Ok(json!({"target":target,"kind":"account","harness":profile.harness,"quota":quota,
        "observed_ms":observed_ms,"windows":windows}))
}

struct Prepared {
    readback: Value,
    digest: String,
    director: std::result::Result<Director, &'static str>,
    targets: Vec<String>,
    save_selection: bool,
    needs_selection: bool,
    repositories: Vec<String>,
}

fn prepare(d: &Arc<Daemon>, p: &Value) -> Result<Prepared> {
    let category = required(p, "category")?.trim().to_string();
    let objective = required(p, "objective")?.trim().to_string();
    if category.is_empty() || category.len() > 160 || objective.is_empty() || objective.len() > 8000 {
        bail!("category or objective is empty or too long");
    }
    if crate::redact::redact(&category) != category || crate::redact::redact(&objective) != objective {
        bail!("category or objective contains sensitive text");
    }
    let source_change_permission = p.get("source_change_permission").map(Value::as_str)
        .unwrap_or(Some("none")).filter(|v| ["none", "isolated"].contains(v))
        .ok_or_else(|| anyhow!("invalid source change permission"))?;
    let paths = p["repositories"].as_array()
        .filter(|paths| !paths.is_empty() && paths.len() <= 16)
        .ok_or_else(|| anyhow!("a Swarm start needs 1-16 repositories"))?;
    let mut repositories = Vec::new();
    let mut scope = Vec::new();
    for path in paths {
        let path = path.as_str().filter(|path| std::path::Path::new(path).is_absolute())
            .ok_or_else(|| anyhow!("repository path must be absolute"))?;
        let top = crate::git::toplevel(std::path::Path::new(path))?;
        let commit = crate::git::rev_parse(&top, "HEAD")
            .ok_or_else(|| anyhow!("repository has no source revision"))?;
        scope.push(json!({"repo_root":top,"source_commit":commit}));
        repositories.push(path.to_string());
    }
    let store = d.store.lock().unwrap();
    // The inherited pool first; a selection is either the one-time choice
    // (nothing is approved yet) or an explicit run override.
    let (_, inherited) = settings::resolve(&store, &category, p.get("policy"), None)?;
    let inherited_empty = inherited.as_array().is_none_or(Vec::is_empty);
    let selection = p.get("allowed_targets").filter(|v| !v.is_null());
    let (policy, targets) = settings::resolve(&store, &category, p.get("policy"), selection)?;
    let targets: Vec<String> = serde_json::from_value(targets)?;
    let targets_source = match (selection.is_some(), inherited_empty) {
        (true, true) => "selection",
        (true, false) => "run",
        (false, _) => policy["allowed_targets_source"].as_str().unwrap_or("built_in"),
    };
    let effective = &policy["effective"];
    let now = crate::daemon::now();
    let accounts = targets.iter().map(|target| account(&store, target, effective, now))
        .collect::<Result<Vec<_>>>()?;
    let agent_limit = store.agent_limit()?;
    drop(store);
    let max_workers = effective["max_workers"].as_i64().unwrap_or(8);
    // The running director takes one of the app's slots.
    let workers = max_workers.min(agent_limit.saturating_sub(1)).max(0);
    let deadline_ms = effective["deadline_ms"].as_i64().unwrap_or(3_600_000);
    let fanout = if targets.is_empty() { "none" }
        else if accounts.iter().any(|a| a["quota"] == "measured" || a["quota"] == "fixture") { "bounded" }
        else { "serial" };
    let director = director_choice();
    let minutes = deadline_ms / 60_000;
    let summary = match (fanout, &director) {
        (_, Err(_)) => format!("Blocked · no qualified director · {minutes} min"),
        ("none", _) => "Choose the accounts this category may use".to_string(),
        ("serial", _) => format!("Auto · one agent at a time (usage unknown) · {minutes} min"),
        _ => format!("Auto · up to {workers} workers · {minutes} min"),
    };
    let readback = json!({
        "category":category,"objective":objective,"repositories":scope,
        "source_change_permission":source_change_permission,
        "account_pool":{"targets":targets,"source":targets_source,"accounts":accounts,
            "needs_account_selection":targets.is_empty()},
        "allocation":{"run_allocation_percent":effective["run_allocation_percent"],
            "finishing_reserve_percent":effective["finishing_reserve_percent"],
            "rule":"per window, a share of the reported remaining allowance, frozen at the run's first booking there"},
        "ceiling":{"max_workers":max_workers,"agents_max_active":agent_limit,"workers":workers,
            "growth_per_wave":effective["growth_per_wave"]},
        "deadline_ms":deadline_ms,
        "fanout":fanout,
        "director":match &director {
            Ok(_) => json!({"state":"qualified","kind":"fixture_scripted","harness":"generic"}),
            Err(reason) => json!({"state":"blocked","reason":reason}),
        },
        "policy_sources":policy["sources"],
        "summary":summary,
    });
    let digest = format!("{:x}", Sha256::digest(readback.to_string().as_bytes()));
    Ok(Prepared { readback, digest, director, needs_selection: targets.is_empty(), targets,
        save_selection: selection.is_some() && inherited_empty, repositories })
}

/// `swarm.start`: without `confirm_readback_sha256`, the read-back only. With
/// it (and a `request_id`), the owner's yes to exactly that read-back.
pub fn start(d: &Arc<Daemon>, p: &Value) -> Result<Value> {
    let prepared = prepare(d, p)?;
    let Some(confirm) = p.get("confirm_readback_sha256") else {
        return Ok(json!({"status":"readback","readback":prepared.readback,
            "readback_sha256":prepared.digest}));
    };
    let confirm = confirm.as_str().filter(|c| c.len() == 64)
        .ok_or_else(|| anyhow!("invalid read-back confirmation"))?;
    let request_id = required(p, "request_id")?;
    let request_scope = p["request_scope"].as_str().unwrap_or("local");
    let _serial = START.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let known: bool = d.store.lock().unwrap().conn.prepare(
        "SELECT 1 FROM swarm_create_requests WHERE request_scope=?1 AND request_id=?2")?
        .exists(params![request_scope, request_id])?;
    if !known {
        // Nothing is committed unless the owner confirmed this exact read-back.
        let refused = |status: &str, reason: Option<&str>| json!({"status":status,"reason":reason,
            "readback":prepared.readback,"readback_sha256":prepared.digest});
        if let Err(reason) = &prepared.director {
            return Ok(refused("blocked", Some(reason)));
        }
        if prepared.needs_selection {
            return Ok(refused("needs_account_selection", None));
        }
        if confirm != prepared.digest {
            return Ok(refused("readback_changed", None));
        }
        // The director needs a slot now; the launch path re-checks it under
        // its own hold, so this only avoids committing a run it cannot start.
        let store = d.store.lock().unwrap();
        if store.active_agent_count()? >= store.agent_limit()? {
            return Ok(refused("blocked", Some("global_agent_limit")));
        }
    }
    let create = json!({
        "request_id":request_id,"request_scope":request_scope,
        "category":prepared.readback["category"],"objective":prepared.readback["objective"],
        "repositories":prepared.repositories,
        "source_change_permission":prepared.readback["source_change_permission"],
        "allowed_targets":prepared.targets,
        "policy":p.get("policy").cloned().unwrap_or(json!({})),
        "start_readback_sha256":confirm,
    });
    let run = super::create(&mut d.store.lock().unwrap(), &create)?;
    let id = run["id"].as_str().ok_or_else(|| anyhow!("run was not recorded"))?.to_string();
    let duplicate = run["duplicate"] == true;
    if !duplicate {
        let store = d.store.lock().unwrap();
        store.conn.execute("INSERT OR IGNORE INTO swarm_start_confirmations(run_id,readback_sha256,readback,confirmed_ms)
            VALUES(?1,?2,?3,?4)", params![id, confirm, prepared.readback.to_string(), crate::daemon::now()])?;
    }
    if prepared.save_selection && !duplicate {
        // The one-time selection becomes the category's approved pool, so
        // the next start inherits it without asking again.
        let category = prepared.readback["category"].as_str().unwrap_or_default();
        settings::set_policy(&mut d.store.lock().unwrap(), &json!({"scope":"category",
            "category":category,"allowed_targets":prepared.targets}))?;
    }
    let owner: Option<Option<String>> = d.store.lock().unwrap().conn.query_row(
        "SELECT overseer_run_id FROM swarm_director_owners WHERE run_id=?1", [&id], |r| r.get(0))
        .optional()?;
    let director = match (owner, &prepared.director) {
        (Some(Some(process)), _) => json!({"status":"launched","overseer_run_id":process,"duplicate":true}),
        // An owner without a linked process is being reconciled; never launch a second one.
        (Some(None), _) => json!({"status":"launch_uncertain","duplicate":true}),
        (None, Err(reason)) => json!({"status":"blocked","reason":reason}),
        (None, Ok(director)) => {
            let repo = &prepared.repositories[0];
            let mut args: Vec<Value> = director.args.iter().map(|a| json!(a)).collect();
            args.push(json!(repo));
            runtime::launch_director(d, &json!({"run_id":id,"generation":1,"repo":repo,
                "program":director.program,"args":args,
                "prompt":prepared.readback["objective"],
                "title":format!("{} director", prepared.readback["category"].as_str().unwrap_or("Swarm"))}))?
        }
    };
    let run = super::get(&d.store.lock().unwrap(), &id)?;
    Ok(json!({"status":"started","duplicate":duplicate,"run":run,"director":director}))
}

/// The recorded confirmation of a normally started run, if any.
pub(super) fn confirmation(store: &Store, run: &str) -> Result<Value> {
    Ok(store.conn.query_row(
        "SELECT readback_sha256,readback,confirmed_ms FROM swarm_start_confirmations WHERE run_id=?1",
        [run], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?)))
        .optional()?
        .map(|(digest, readback, at)| {
            let readback: Value = serde_json::from_str(&readback).unwrap_or(Value::Null);
            json!({"readback_sha256":digest,"confirmed_ms":at,"summary":readback["summary"],
                "readback":readback})
        })
        .unwrap_or(Value::Null))
}
