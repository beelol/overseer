# CONTRACT-03 — scoped availability and failures

Status: not started
Tested implementation commit: none; branches are separate.
Verification date and verifier: 2026-09-26, Swarm implementing agent (contract audit only).
OS / architecture / VS Code / harness versions: pin when integrated test runs.
Harness, provider and redacted account identities: synthetic harness, endpoint, account and model scopes; no live identities.
Prerequisites and fixture: versioned Auto route/health/quota snapshot consumed by Swarm admission.
Steps or exact reproducible commands: inject local harness, endpoint outage, account-auth, account-quota, model-window and ordinary job failures one at a time, with an unrelated healthy target and running job. Age a known exhausted observation and compare it to never-known data and a public incident advisory.
Expected result: only matching routes are excluded; unrelated work continues; known exhaustion cannot become fresh capacity merely through expiry or reset; unknown and stale states remain visible.
Actual result: not run across modes. Swarm policy fixtures demonstrate scoped target exclusion; Auto's separate selector has scoped observations, but the bridge is absent.
Evidence paths: `daemon/tests/swarm_policy.rs`, `docs/verification/swarm/milestone-4.md` (partial precursor only).
Live vs fixture coverage: fixture precursor; no integrated or live coverage.
Known limitations and remaining platform/account combinations: provider-specific live observations and running-job behavior unqualified.
Blocker, attempted alternatives and next action: keep unchecked; feed Auto's versioned observations into shared admission and exercise every failure scope.
