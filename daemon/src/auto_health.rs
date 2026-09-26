//! Scoped, expiring service evidence for automatic route selection.

use crate::auto_select::{Health, Route};
use anyhow::{anyhow, Result};
use rusqlite::params;
use serde_json::Value;

#[derive(Clone, Debug)]
pub enum Scope {
    Provider(String),
    Endpoint {
        provider: String,
        endpoint: String,
    },
    AccountEndpoint {
        profile_id: String,
        provider: String,
        endpoint: String,
    },
    Route(String),
    Host,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Signal {
    PublicHealthy,
    PublicIncident,
    RouteSuccess,
    EndpointFailure,
    AuthenticationFailure,
    Throttled,
    HostOffline,
}

#[derive(Clone, Debug)]
pub struct Observation {
    pub scope: Scope,
    pub signal: Signal,
    pub observed_ms: i64,
    pub expires_ms: i64,
}

/// Normalize only the public component that corresponds to a supported
/// harness route. Status pages are advisory and never authorize an account.
pub fn parse_public_status(value: &Value, provider: &str, observed_ms: i64) -> Result<Observation> {
    let (required, endpoint): (&[&str], &str) = match provider {
        "openai" => (&["CLI", "Codex API"], "codex"),
        "anthropic" => (&["Claude Code"], "claude-code"),
        _ => return Err(anyhow!("unsupported public status provider")),
    };
    let components = value.get("components").and_then(Value::as_array)
        .filter(|items| items.len() <= 128)
        .ok_or_else(|| anyhow!("public status components unavailable"))?;
    let mut found = std::collections::BTreeSet::new();
    let mut incident = false;
    for component in components {
        let Some(name) = component.get("name").and_then(Value::as_str) else { continue };
        if !required.contains(&name) { continue; }
        if !found.insert(name) { return Err(anyhow!("duplicate public status component")); }
        match component.get("status").and_then(Value::as_str) {
            Some("operational") => {}
            Some("degraded_performance" | "partial_outage" | "major_outage" | "under_maintenance") => incident = true,
            _ => return Err(anyhow!("unsupported public status component state")),
        }
    }
    if found.len() != required.len() {
        return Err(anyhow!("required public status component unavailable"));
    }
    Ok(Observation { scope: Scope::Endpoint {provider:provider.into(), endpoint:endpoint.into()},
        signal:if incident {Signal::PublicIncident} else {Signal::PublicHealthy},
        observed_ms, expires_ms:observed_ms.saturating_add(60_000) })
}

/// Direct, scope-matched observations may exclude a route. Public status is
/// advisory only: an incident degrades ranking, and green never clears a
/// separate account quota or authentication block.
pub fn evaluate(route: &Route, observations: &[Observation], now_ms: i64) -> Health {
    let mut direct: Option<(i64, u8, Health)> = None;
    let mut provider_status: Option<(i64, Signal)> = None;
    for observation in observations {
        if observation.expires_ms <= observation.observed_ms
            || observation.observed_ms > now_ms
            || observation.expires_ms <= now_ms
        {
            continue;
        }
        let matching = match (&observation.scope, observation.signal) {
            (Scope::Provider(provider), Signal::PublicHealthy | Signal::PublicIncident)
                if provider == &route.provider =>
            {
                if provider_status
                    .as_ref()
                    .is_none_or(|(at, _)| *at <= observation.observed_ms)
                {
                    provider_status = Some((observation.observed_ms, observation.signal));
                }
                None
            }
            (
                Scope::Endpoint { provider, endpoint },
                Signal::PublicHealthy | Signal::PublicIncident,
            ) if provider == &route.provider && endpoint == &route.endpoint => {
                if provider_status
                    .as_ref()
                    .is_none_or(|(at, _)| *at <= observation.observed_ms)
                {
                    provider_status = Some((observation.observed_ms, observation.signal));
                }
                None
            }
            (Scope::Route(id), Signal::RouteSuccess) if id == &route.id => {
                Some((0, Health::Healthy))
            }
            (Scope::Endpoint { provider, endpoint }, Signal::EndpointFailure)
                if provider == &route.provider && endpoint == &route.endpoint =>
            {
                Some((1, Health::Unavailable))
            }
            (
                Scope::AccountEndpoint {
                    profile_id,
                    provider,
                    endpoint,
                },
                Signal::AuthenticationFailure | Signal::Throttled,
            ) if profile_id == &route.profile_id
                && provider == &route.provider
                && endpoint == &route.endpoint =>
            {
                Some((1, Health::Unavailable))
            }
            (Scope::Host, Signal::HostOffline)
                if !crate::auto_opencode::is_verified_loopback_endpoint(&route.endpoint) =>
            {
                Some((1, Health::Unavailable))
            }
            _ => None,
        };
        if let Some((priority, health)) = matching {
            let candidate = (observation.observed_ms, priority, health);
            if direct
                .as_ref()
                .is_none_or(|previous| (previous.0, previous.1) <= (candidate.0, candidate.1))
            {
                direct = Some(candidate);
            }
        }
    }
    let public_incident = provider_status.is_some_and(|(_, signal)| signal == Signal::PublicIncident);
    match direct {
        Some((_, _, health)) => health,
        None if public_incident => Health::Degraded,
        None => Health::Unknown,
    }
}

/// Read only bounded, normalized local run outcomes. Message text is not used
/// to infer allowance or persisted in this health view. Public status and
/// connectivity need separate collectors before they can enter this path.
pub fn recent_local_observations(
    store: &crate::store::Store,
    now_ms: i64,
) -> Result<Vec<Observation>> {
    const RECENT_MS: i64 = 60_000;
    let since = now_ms.saturating_sub(RECENT_MS);
    let mut observations = Vec::new();
    let mut errors = store.conn.prepare(
        "SELECT e.ts,e.payload,r.profile_id,r.harness FROM events e JOIN runs r ON r.id=e.run_id \
         WHERE e.kind='error' AND e.source='harness' AND e.ts>=?1 AND e.ts<=?2 \
         ORDER BY e.seq DESC LIMIT 100",
    )?;
    let rows = errors.query_map(params![since, now_ms], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, String>(3)?,
        ))
    })?;
    for item in rows {
        let (at, payload, profile, harness) = item?;
        let Some(profile) = profile else { continue };
        let Some((provider, endpoint)) = fixed_harness_endpoint(&harness) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<Value>(&payload) else {
            continue;
        };
        let signal = match value.get("class").and_then(Value::as_str) {
            Some("rate_limit") => Signal::Throttled,
            Some("auth") => Signal::AuthenticationFailure,
            Some("service_unavailable") => Signal::EndpointFailure,
            _ => continue,
        };
        let scope = if matches!(signal, Signal::EndpointFailure) {
            Scope::Endpoint {
                provider: provider.into(),
                endpoint: endpoint.into(),
            }
        } else {
            Scope::AccountEndpoint {
                profile_id: profile,
                provider: provider.into(),
                endpoint: endpoint.into(),
            }
        };
        observations.push(Observation {
            scope,
            signal,
            observed_ms: at,
            expires_ms: at.saturating_add(RECENT_MS),
        });
    }
    let mut successes = store.conn.prepare(
        "SELECT ended_ms,profile_id,model,effort FROM runs WHERE status='completed' \
         AND ended_ms>=?1 AND ended_ms<=?2 ORDER BY ended_ms DESC LIMIT 100",
    )?;
    let rows = successes.query_map(params![since, now_ms], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, Option<String>>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, Option<String>>(3)?,
        ))
    })?;
    for item in rows {
        let (at, Some(profile), Some(model), Some(effort)) = item? else {
            continue;
        };
        observations.push(Observation {
            scope: Scope::Route(format!("{profile}/{model}/{effort}")),
            signal: Signal::RouteSuccess,
            observed_ms: at,
            expires_ms: at.saturating_add(RECENT_MS),
        });
    }
    Ok(observations)
}

fn fixed_harness_endpoint(harness: &str) -> Option<(&'static str, &'static str)> {
    match harness {
        "codex" | "codex-app" => Some(("openai", "codex")),
        "claude" => Some(("anthropic", "claude-code")),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auto_select::{select, Allowance, CapabilityTier, Fit, Sandbox, WorkUnit};
    use std::collections::BTreeSet;

    #[test]
    fn public_feed_maps_only_codex_cli_and_api_to_advisory_health() {
        let at = 1_800_000_000_000;
        let feed = serde_json::json!({"components":[
            {"name":"Images","status":"major_outage"},
            {"name":"CLI","status":"operational"},
            {"name":"Codex API","status":"degraded_performance"}
        ]});
        let signal = parse_public_status(&feed, "openai", at).unwrap();
        let codex = route("codex", "codex-app", "openai", "codex", "acct-a");
        assert_eq!(evaluate(&codex, &[signal], at), Health::Degraded);
        let mut recovered = feed;
        recovered["components"][2]["status"] = serde_json::json!("operational");
        let green = parse_public_status(&recovered, "openai", at).unwrap();
        let mut exhausted = codex;
        exhausted.quota = Allowance::Exhausted;
        exhausted.health = evaluate(&exhausted, &[green], at);
        assert!(select(&work(), &[exhausted]).selected.is_none(),
            "public green cannot clear an exhausted account");
    }

    #[test]
    fn public_feed_requires_the_relevant_component_and_rejects_drift() {
        let at = 1_800_000_000_000;
        let unrelated = serde_json::json!({"components":[
            {"name":"Claude API (api.anthropic.com)","status":"operational"}
        ]});
        assert!(parse_public_status(&unrelated, "anthropic", at).is_err());
        let incident = serde_json::json!({"components":[
            {"name":"Claude API (api.anthropic.com)","status":"operational"},
            {"name":"Claude Code","status":"major_outage"}
        ]});
        let observation = parse_public_status(&incident, "anthropic", at).unwrap();
        let claude = route("claude", "claude", "anthropic", "claude-code", "acct-a");
        assert_eq!(evaluate(&claude, &[observation], at), Health::Degraded);
        let mut duplicate = incident.clone();
        duplicate["components"].as_array_mut().unwrap().push(
            serde_json::json!({"name":"Claude Code","status":"operational"}));
        assert!(parse_public_status(&duplicate, "anthropic", at).is_err());
        let mut unknown = incident;
        unknown["components"][1]["status"] = serde_json::json!("new_status");
        assert!(parse_public_status(&unknown, "anthropic", at).is_err());
        unknown["components"][1]["status"] = serde_json::json!("under_maintenance");
        let maintenance = parse_public_status(&unknown, "anthropic", at).unwrap();
        assert_eq!(evaluate(&claude, &[maintenance], at), Health::Degraded);
    }

    #[test]
    fn public_incident_softly_prefers_an_independent_unknown_route() {
        let at = 1_800_000_000_000;
        let mut affected = route("a", "codex-app", "openai", "codex", "acct-a");
        let independent = route("b", "claude", "anthropic", "claude-code", "acct-b");
        affected.health = evaluate(&affected,
            &[obs(Scope::Endpoint {provider:"openai".into(), endpoint:"codex".into()},
                Signal::PublicIncident, at, at + 60_000)], at);
        let decision = select(&work(), &[affected, independent]);
        assert_eq!(decision.selected.as_deref(), Some("b"));
        assert!(decision.exclusions.is_empty(), "public incidents must not hard-exclude a route");
    }

    #[test]
    fn a_recent_direct_success_outweighs_a_later_public_page_refresh() {
        let route = route("codex", "codex-app", "openai", "codex", "acct-a");
        let observations = [
            obs(Scope::Route("codex".into()), Signal::RouteSuccess, 150, 250),
            obs(Scope::Endpoint {provider:"openai".into(), endpoint:"codex".into()},
                Signal::PublicIncident, 200, 260),
        ];
        assert_eq!(evaluate(&route, &observations, 210), Health::Healthy,
            "a status fetch time is not proof the incident began after the successful turn");
    }

    fn route(id: &str, harness: &str, provider: &str, endpoint: &str, profile: &str) -> Route {
        Route {
            id: id.into(),
            harness: harness.into(),
            provider: provider.into(),
            endpoint: endpoint.into(),
            profile_id: profile.into(),
            pool_id: profile.into(),
            model: "general".into(),
            effort: "medium".into(),
            tier: CapabilityTier::General,
            tools: BTreeSet::new(),
            context_limit: Some(100_000),
            supports_approvals: true,
            sandbox: Sandbox::WorkspaceWrite,
            recommended_default: true,
            quota: Allowance::ObservedNonExhausted,
            fit: Fit::Unknown,
            health: Health::Unknown,
        }
    }

    fn work() -> WorkUnit {
        WorkUnit {
            id: "health-1".into(),
            min_tier: CapabilityTier::General,
            required_tools: BTreeSet::new(),
            context_needed: 0,
            requires_approvals: false,
            min_sandbox: Sandbox::WorkspaceWrite,
            max_sandbox: Sandbox::WorkspaceWrite,
            allowed_profiles: ["acct-a", "acct-b", "local"]
                .into_iter()
                .map(str::to_string)
                .collect(),
            pinned_route: None,
            preferred_harness: None,
        }
    }

    fn obs(scope: Scope, signal: Signal, at: i64, expires: i64) -> Observation {
        Observation {
            scope,
            signal,
            observed_ms: at,
            expires_ms: expires,
        }
    }

    #[test]
    fn green_public_feed_never_clears_exhausted_account() {
        let mut claude = route("claude", "claude", "anthropic", "claude-code", "acct-a");
        claude.quota = Allowance::Exhausted;
        claude.health = evaluate(
            &claude,
            &[obs(
                Scope::Provider("anthropic".into()),
                Signal::PublicHealthy,
                100,
                300,
            )],
            200,
        );
        let decision = select(&work(), &[claude]);
        assert!(decision.selected.is_none());
        assert_eq!(decision.exclusions[0].reason, "quota_exhausted");
    }

    #[test]
    fn unrelated_incident_does_not_affect_route_and_newer_success_outweighs_broad_incident() {
        let claude = route("claude", "claude", "anthropic", "claude-code", "acct-a");
        let unrelated = obs(
            Scope::Provider("openai".into()),
            Signal::PublicIncident,
            100,
            300,
        );
        assert_eq!(evaluate(&claude, &[unrelated], 200), Health::Unknown);
        let other_anthropic_service = obs(
            Scope::Endpoint {
                provider: "anthropic".into(),
                endpoint: "claude-api".into(),
            },
            Signal::PublicIncident,
            100,
            300,
        );
        assert_eq!(
            evaluate(&claude, &[other_anthropic_service], 200),
            Health::Unknown
        );
        let code_incident = obs(
            Scope::Endpoint {
                provider: "anthropic".into(),
                endpoint: "claude-code".into(),
            },
            Signal::PublicIncident,
            100,
            300,
        );
        assert_eq!(evaluate(&claude, &[code_incident], 200), Health::Degraded);
        let incident = obs(
            Scope::Provider("anthropic".into()),
            Signal::PublicIncident,
            100,
            300,
        );
        assert_eq!(
            evaluate(&claude, &[incident.clone()], 200),
            Health::Degraded
        );
        let success = obs(
            Scope::Route("claude".into()),
            Signal::RouteSuccess,
            150,
            300,
        );
        assert_eq!(
            evaluate(&claude, &[incident, success], 200),
            Health::Healthy
        );
    }

    #[test]
    fn endpoint_failure_blocks_both_harnesses_on_one_upstream_but_not_another() {
        let mut a = route("a", "claude", "anthropic", "api.anthropic", "acct-a");
        let mut b = route("b", "opencode", "anthropic", "api.anthropic", "acct-a");
        let mut independent = route(
            "local",
            "opencode",
            "local",
            "http://127.0.0.1:8080/v1",
            "local",
        );
        let failure = obs(
            Scope::Endpoint {
                provider: "anthropic".into(),
                endpoint: "api.anthropic".into(),
            },
            Signal::EndpointFailure,
            100,
            300,
        );
        for item in [&mut a, &mut b, &mut independent] {
            item.health = evaluate(item, &[failure.clone()], 200);
        }
        let decision = select(&work(), &[a, b, independent]);
        assert_eq!(decision.selected.as_deref(), Some("local"));
        assert_eq!(
            decision
                .exclusions
                .iter()
                .filter(|entry| entry.reason == "route_unavailable")
                .count(),
            2
        );
    }

    #[test]
    fn throttling_without_quota_proof_cools_only_its_account_endpoint() {
        let mut throttled = route("throttled", "codex-app", "openai", "codex", "acct-a");
        let independent = route("independent", "codex-app", "openai", "codex", "acct-b");
        let signal = obs(
            Scope::AccountEndpoint {
                profile_id: "acct-a".into(),
                provider: "openai".into(),
                endpoint: "codex".into(),
            },
            Signal::Throttled,
            100,
            160,
        );
        throttled.health = evaluate(&throttled, &[signal.clone()], 120);
        assert_eq!(throttled.quota, Allowance::ObservedNonExhausted);
        assert_eq!(throttled.health, Health::Unavailable);
        assert_eq!(
            evaluate(&independent, &[signal.clone()], 120),
            Health::Unknown
        );
        assert_eq!(evaluate(&throttled, &[signal], 160), Health::Unknown);
    }

    #[test]
    fn offline_host_leaves_verified_loopback_route_eligible() {
        let mut remote = route("remote", "claude", "anthropic", "claude-code", "acct-a");
        let mut local = route(
            "local",
            "opencode",
            "local",
            "http://127.0.0.1:8080/v1",
            "local",
        );
        local.quota = Allowance::NotApplicable;
        let offline = obs(Scope::Host, Signal::HostOffline, 100, 300);
        remote.health = evaluate(&remote, &[offline.clone()], 200);
        local.health = evaluate(&local, &[offline], 200);
        let decision = select(&work(), &[remote, local]);
        assert_eq!(decision.selected.as_deref(), Some("local"));
        assert_eq!(decision.exclusions[0].reason, "route_unavailable");
    }
}
