# SWARM-09 — shared account identity

Status: verified at fixture scope on 2026-09-28 (`claude/auto-swarm`); see the last section. First revision: `47643df`.

Input: a policy snapshot gives two targets the same account ID but disjoint pool IDs. A second version gives them one shared pool plus a model-specific extra pool. A separate admission fixture has two harness targets sharing one quota pool across categories.

Expected: the first snapshot cannot double the account allowance; the second remains usable while both binding pools are checked. Different harness labels do not split a known shared pool.

Actual: the disjoint snapshot initially made both targets eligible. The policy now returns `account_pool_conflict` for the qualified target. The overlapping snapshot stays eligible with both windows. Existing admission tests show same-pool reservations constrain a second category. The workspace suite passed with 72 tests at revision `47643df`.

Evidence: `daemon/tests/swarm_policy.rs`, `daemon/tests/swarm_admission.rs`, `docs/verification/swarm/milestone-14.md`.

Remaining: the snapshots are injected fixtures. Auto Mode has not supplied verified account identities or pool relationships, and independent real accounts have not been qualified. This conservative rule may exclude legitimate endpoint-specific quotas until their common or independently verified identity is supplied.

Explicit-revocation follow-up (this revision): a local policy snapshot marks one of two target aliases for account `shared` as `auth=revoked`, while the other alias still reports `ok`. Both aliases now return `auth_unavailable`, including when the revoked alias is not selected; the independent account remains eligible. A three-job availability fixture admits two jobs through the shared account's different routes, then revokes one route. Both jobs enter `cancel_requested` with `account_identity_revoked`, a repeated observation adds no cancellation, and the independent account still admits work. The affected policy, admission, scheduling, availability and dispatch suites pass. This uses supplied account IDs; live identity discovery and cross-mode shared quota authority remain unverified.

## Verified at fixture scope (2026-09-28)

| Clause | Test |
| --- | --- |
| Two harnesses using the same account share capacity | `shared_pool_reservation_blocks_stale_capacity_across_categories` (`swarm_admission.rs`: a Codex and an OpenCode target on one account's pool; a reservation through one constrains the other, across categories); on the shared booking, two profiles with the same account fingerprint share one pool (AUTO-AC-04's fixture part, which Swarm's route selection applies to workers through `apply_account_pool`) |
| Verified independent accounts keep separate capacity | the same fixtures keep an independent account eligible and admitting; `auto_selects_each_jobs_route_within_the_approved_pool` (job c moves to the second, independent account when the first account's category allowance is spent) |
| Uncertain identity never doubles the allowance | `one_account_without_a_common_verified_pool_cannot_double_its_allowance` (`account_pool_conflict`); on route selection an unidentified account is excluded as `unresolved_quota_pool_identity` rather than counted separately; `revoked_account_blocks_all_of_its_target_aliases` |

Rerun serially on 2026-09-28: `swarm_admission` 41, `swarm_policy` 12, `swarm_native` 11.

Boundary: account identity comes from recorded fingerprints and fixture ids; discovering that two real logins are one subscription is Auto's live identity feed.
