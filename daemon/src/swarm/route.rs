//! With Swarm on, Auto's selector chooses each job's route within the
//! category's approved pool (SWARM-24, CONTRACT-04). The director plans the
//! job and may state requirements (capability tier, tools, a preferred
//! harness, a broad task class); it never names the worker's account.
//!
//! The candidates are the capability priors of every approved account
//! profile a Swarm worker can launch on (Claude today), with:
//! - Auto's eligibility (`auto_select`): account identity, exhaustion across
//!   profiles of one account, scoped health, the endpoint recovery check,
//!   capability tier, tools and sandbox;
//! - fit: the upper draw the booking would book (the worker's qualified
//!   draw, or a fixture draw behind the booking's fixture API) against the
//!   account's headroom less its live bookings *and* against the category's
//!   remaining allocation in the same windows;
//! - Auto's task-aware ranking: the lowest adequate tier, then health,
//!   allowance, fit, the recommended default and the preferred harness.
//!
//! A route the booking could not admit (unknown draw, a busy account) is
//! excluded with its reason rather than tried. Admission stays the one
//! authority: it rechecks everything in its own transaction.

use crate::auto_select::{self, CapabilityTier, Decision, Exclusion, Fit, Route, Sandbox, WorkUnit};
use crate::daemon::Daemon;
use anyhow::{anyhow, bail, Result};
use rusqlite::params;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

pub const SELECTOR_VERSION: &str = "swarm-auto-route-v1";

/// What the director may state about a job. Nothing here names an account.
#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct Requirements {
    pub min_tier: Option<CapabilityTier>,
    pub required_tools: BTreeSet<String>,
    pub preferred_harness: Option<String>,
    pub task_class: Option<String>,
}

pub fn requirements(value: &Value) -> Result<Requirements> {
    let mut out = Requirements::default();
    if value.is_null() {
        return Ok(out);
    }
    let fields = value.as_object().ok_or_else(|| anyhow!("requirements must be an object"))?;
    for (key, field) in fields {
        match key.as_str() {
            "min_tier" => out.min_tier = Some(serde_json::from_value(field.clone())
                .map_err(|_| anyhow!("min_tier must be general or frontier"))?),
            "required_tools" => {
                let tools = field.as_array().filter(|t| t.len() <= 16)
                    .ok_or_else(|| anyhow!("required_tools must be a bounded list"))?;
                for tool in tools {
                    let name = tool.as_str().filter(|n| crate::daemon::valid_required_tool(n))
                        .ok_or_else(|| anyhow!("invalid required tool"))?;
                    out.required_tools.insert(name.to_string());
                }
            }
            "preferred_harness" => out.preferred_harness = Some(field.as_str()
                .filter(|h| !h.is_empty() && h.len() <= 40)
                .ok_or_else(|| anyhow!("invalid preferred_harness"))?.to_string()),
            "task_class" => out.task_class = Some(field.as_str()
                .filter(|c| matches!(*c, "browser_check" | "routine_edit" | "difficult_diagnosis" | "general"))
                .ok_or_else(|| anyhow!("task_class must be a supported broad work category"))?.to_string()),
            other => bail!("unknown requirement {other}"),
        }
    }
    Ok(out)
}

/// A fixture caller's draw for one profile (behind the booking's fixture API).
pub struct FixtureDraw {
    pub quota_event_seq: i64,
    pub upper_draw_milli: Vec<i64>,
}

pub struct Choice {
    pub decision: Decision,
    pub selected: Option<Route>,
    pub trace: Value,
}

/// Routes of this job's attempts whose worker failed with no recorded
/// effect: a second attempt prefers another route when one is eligible.
fn failed_attempt_routes(conn: &rusqlite::Connection, run: &str, job: &str) -> Result<BTreeSet<String>> {
    let mut stmt = conn.prepare(
        "SELECT s.target_profile_id||'/'||COALESCE(s.target_model,'')||'/'||COALESCE(s.target_effort,'')
         FROM swarm_admissions s
         JOIN swarm_attempts a ON a.id=s.attempt_id AND a.status='finished'
         JOIN swarm_worker_launches l ON l.attempt_id=a.id
         JOIN runs r ON r.id=l.overseer_run_id AND r.status='failed'
         WHERE s.run_id=?1 AND s.job_id=?2 AND s.target_profile_id IS NOT NULL")?;
    let rows = stmt.query_map(params![run, job], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<BTreeSet<_>>>()?;
    Ok(rows)
}

/// Choose one job's route within the run's approved pool. `refused` holds
/// routes admission has already refused before any effect in this dispatch.
pub fn select(
    d: &Arc<Daemon>,
    current: &Value,
    job: &str,
    needs: &Requirements,
    refused: &BTreeMap<String, String>,
    fixture_draws: &BTreeMap<String, FixtureDraw>,
) -> Result<Choice> {
    let run = current["id"].as_str().ok_or_else(|| anyhow!("run has no id"))?;
    let effective = &current["policy"]["effective"];
    let now = crate::daemon::now();
    let approved: Vec<String> = current["allowed_targets"].as_array().into_iter().flatten()
        .filter_map(Value::as_str).map(str::to_string).collect();
    let mut routes: Vec<Route> = Vec::new();
    let mut pre_exclusions: Vec<Exclusion> = Vec::new();
    let mut allowed_profiles = BTreeSet::new();
    let (budget_role, failed_routes) = {
        let store = d.store.lock().unwrap();
        let role: String = store.conn.query_row(
            "SELECT budget_role FROM swarm_jobs WHERE run_id=?1 AND id=?2",
            params![run, job], |r| r.get(0))?;
        (role, failed_attempt_routes(&store.conn, run, job)?)
    };
    {
        let store = d.store.lock().unwrap();
        for id in &approved {
            let Some(profile) = store.profile(id)? else { continue };
            if !crate::adapters::swarm_worker_launch_supported(&profile.harness) {
                if matches!(profile.harness.as_str(), "codex" | "opencode") {
                    pre_exclusions.push(Exclusion { route_id: id.clone(),
                        reason: "swarm_worker_launch_unsupported".into() });
                }
                continue;
            }
            allowed_profiles.insert(id.clone());
            let latest = store.latest_auto_quota(id)?;
            let mut found = crate::auto_route::claude_prior_routes(
                latest.as_ref().map(|reading| &reading.snapshot), id, now);
            match store.auto_account_pool_id(id)? {
                Some(pool) => {
                    let observations = store.auto_account_quota_observations(id)?;
                    crate::server::apply_account_pool(&mut found, &pool, &observations, now);
                }
                None => for route in &mut found { route.unresolved_quota_pool_identity = true; },
            }
            routes.extend(found);
        }
    }
    if routes.len() > 128 {
        bail!("swarm route candidates exceeded their bound");
    }
    {
        let store = d.store.lock().unwrap();
        let health = crate::auto_health::recent_local_observations(&store, now).unwrap_or_default();
        for route in &mut routes {
            route.health = crate::auto_health::evaluate(route, &health, now);
        }
    }
    let recovering = crate::server::mark_endpoint_recovery(d, &mut routes, None)?;
    // Fit, as the booking would compute it, plus this category's allocation.
    let mut fit_evidence = Vec::new();
    let mut swarm_reasons: BTreeMap<String, &'static str> = BTreeMap::new();
    {
        let store = d.store.lock().unwrap();
        let conn = &store.conn;
        for route in &mut routes {
            if route.unresolved_quota_pool_identity {
                fit_evidence.push(json!({"route_id":route.id,"fit":"unknown","reason":"account_identity_unknown"}));
                continue;
            }
            let fingerprint = route.pool_id.trim_start_matches("account/").to_string();
            if crate::account_booking::unbooked_run_on_account(conn, &fingerprint)? {
                swarm_reasons.insert(route.id.clone(), "account_pool_busy");
                fit_evidence.push(json!({"route_id":route.id,"fit":"unknown","reason":"account_pool_busy"}));
                continue;
            }
            let bucket = crate::upper_draw::DrawBucket::agent(&route.harness, Some(&route.model), Some(&route.effort));
            let draw = match fixture_draws.get(&route.profile_id) {
                Some(f) => crate::account_booking::PreviewDraw::Fixture {
                    quota_event_seq: f.quota_event_seq, upper_draw_milli: &f.upper_draw_milli },
                None => crate::account_booking::PreviewDraw::Qualified(&bucket),
            };
            let preview = crate::account_booking::preview_account_fit_with(conn, &route.profile_id, draw, now)?;
            let crate::account_booking::AccountFitPreview::Windows { quota_event_seq, windows, sample_count, .. } = preview else {
                let crate::account_booking::AccountFitPreview::Unknown(reason) = preview else { unreachable!() };
                route.fit = Fit::Unknown;
                swarm_reasons.insert(route.id.clone(), reason);
                fit_evidence.push(json!({"route_id":route.id,"fit":"unknown","reason":reason}));
                continue;
            };
            let account_room = windows.iter().all(|w|
                w.upper_draw_milli <= w.headroom_milli.saturating_sub(w.committed_milli));
            let cited = crate::account_booking::cited_windows_in_tx(conn, &route.profile_id, quota_event_seq)?;
            let allocation = match &cited {
                Some(cited) if cited.len() == windows.len() => Some(super::admission::category_allocation(
                    conn, run, cited, effective, budget_role == "finishing")?),
                _ => None,
            };
            let category_room = allocation.as_ref().map(|alloc| windows.iter().zip(alloc)
                .all(|(w, a)| w.upper_draw_milli <= a.remaining));
            let (fit, reason) = match (account_room, category_room) {
                (false, _) => (Fit::Unaffordable, "estimated_draw_exceeds_allowance"),
                (true, Some(false)) => {
                    swarm_reasons.insert(route.id.clone(), "category_allocation_exceeded");
                    (Fit::Unaffordable, "category_allocation_exceeded")
                }
                (true, Some(true)) => (Fit::Fits, "draw_fits_account_and_category"),
                (true, None) => {
                    swarm_reasons.insert(route.id.clone(), "upper_draw_unknown");
                    (Fit::Unknown, "upper_draw_unknown")
                }
            };
            route.fit = fit;
            fit_evidence.push(json!({"route_id":route.id,"fit":fit,"reason":reason,
                "quota_event_seq":quota_event_seq,"sample_count":sample_count,
                "draw":if fixture_draws.contains_key(&route.profile_id) { "fixture" } else { "qualified" },
                "windows":windows.iter().zip(allocation.iter().flatten().map(Some).chain(std::iter::repeat(None)))
                    .map(|(w, a)| json!({"window":w.window,"upper_draw_milli":w.upper_draw_milli,
                        "headroom_milli":w.headroom_milli,"committed_milli":w.committed_milli,
                        "category_remaining_milli":a.map(|a| a.remaining)})).collect::<Vec<_>>()}));
        }
    }
    let job_deadline = effective["deadline_ms"].as_i64().unwrap_or(3_600_000).clamp(1_000, 86_400_000) as u64;
    let work = WorkUnit {
        id: format!("swarm/{run}/{job}"),
        min_tier: needs.min_tier.unwrap_or(CapabilityTier::General),
        required_tools: needs.required_tools.clone(),
        context_needed: 0, requires_approvals: false,
        min_sandbox: Sandbox::WorkspaceWrite, max_sandbox: Sandbox::WorkspaceWrite,
        allowed_profiles, pinned_route: None,
        preferred_harness: needs.preferred_harness.clone(),
        task_class: needs.task_class.clone(), execution_budget_ms: Some(job_deadline),
    };
    // Auto's eligibility over every candidate first, for its own reasons.
    let first = auto_select::select(&work, &routes);
    let auto_excluded: BTreeMap<String, String> = first.exclusions.iter()
        .map(|e| (e.route_id.clone(), e.reason.clone())).collect();
    // Then Swarm's: a route the booking cannot admit, a route admission
    // refused in this dispatch, and (while another is eligible) a route whose
    // earlier attempt of this job failed.
    let mut exclusions = pre_exclusions;
    let mut candidates = Vec::new();
    let mut after_failure = Vec::new();
    // No native worker has proved an audit-only source boundary (admission
    // refuses it too); in an audit every account route is out.
    let audit = current["source_change_permission"] == "none";
    for route in &routes {
        if audit {
            exclusions.push(Exclusion { route_id: route.id.clone(), reason: "audit_source_boundary_unqualified".into() });
            continue;
        }
        let reason = match auto_excluded.get(&route.id) {
            Some(reason) => Some(swarm_reasons.get(&route.id)
                .filter(|_| reason == "estimated_draw_exceeds_allowance")
                .map(|r| r.to_string()).unwrap_or_else(|| reason.clone())),
            None => refused.get(&route.id).map(|r| format!("admission_refused:{r}"))
                .or_else(|| (route.fit != Fit::Fits).then(|| swarm_reasons.get(&route.id)
                    .copied().unwrap_or("upper_draw_unknown").to_string())),
        };
        match reason {
            Some(reason) => exclusions.push(Exclusion { route_id: route.id.clone(), reason }),
            None if failed_routes.contains(&route.id) => after_failure.push(route.clone()),
            None => candidates.push(route.clone()),
        }
    }
    let prefer_other = !candidates.is_empty();
    if prefer_other {
        for route in &after_failure {
            exclusions.push(Exclusion { route_id: route.id.clone(), reason: "earlier_attempt_failed_on_route".into() });
        }
    } else {
        candidates = after_failure;
    }
    let chosen = auto_select::select(&work, &candidates);
    let selected = chosen.selected.as_deref().and_then(|id| routes.iter().find(|r| r.id == id)).cloned();
    let decision = Decision { work_unit_id: work.id.clone(), selected: chosen.selected.clone(),
        exclusions, reason: chosen.reason.clone() };
    let mut trace = json!({"selector_version":SELECTOR_VERSION,"auto_selector":"auto_select::select",
        "requirements":needs,"decision":decision,
        "selected_route":selected.as_ref().map(|r| json!({"harness":r.harness,"profile_id":r.profile_id,
            "model":r.model,"effort":r.effort,"tier":r.tier,"quota":r.quota,"fit":r.fit,"health":r.health})),
        "fit":fit_evidence,"selection_input":{"work":work,"routes":routes},
        "inference":{"state":"not_used","output":null}});
    if let Some(check) = selected.as_ref().and_then(|r| crate::server::recovery_check_for(&recovering, r)) {
        trace["recovery_check"] = check;
    }
    Ok(Choice { decision, selected, trace })
}

/// Admission refusals that are about the chosen route's account alone and
/// happen before any effect: another eligible route may be tried in the same
/// dispatch. Anything else (the job, the run, a slot, the audit boundary)
/// would refuse every route and is returned as is.
pub fn route_scoped_refusal(reason: &str) -> bool {
    matches!(reason, "account_identity_unknown" | "account_identity_changed" | "quota_unknown"
        | "snapshot_superseded" | "snapshot_expired" | "account_exhausted" | "account_allowance_unknown"
        | "upper_draw_unknown" | "account_pool_busy" | "allocation_exhausted" | "shared_pool_headroom"
        | "duplicate_window")
}
