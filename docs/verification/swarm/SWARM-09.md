# SWARM-09 — shared account identity

Status: partial. Revision: `47643df`.

Input: a policy snapshot gives two targets the same account ID but disjoint pool IDs. A second version gives them one shared pool plus a model-specific extra pool. A separate admission fixture has two harness targets sharing one quota pool across categories.

Expected: the first snapshot cannot double the account allowance; the second remains usable while both binding pools are checked. Different harness labels do not split a known shared pool.

Actual: the disjoint snapshot initially made both targets eligible. The policy now returns `account_pool_conflict` for the qualified target. The overlapping snapshot stays eligible with both windows. Existing admission tests show same-pool reservations constrain a second category. The workspace suite passed with 72 tests at revision `47643df`.

Evidence: `daemon/tests/swarm_policy.rs`, `daemon/tests/swarm_admission.rs`, `docs/verification/swarm/milestone-14.md`.

Remaining: the snapshots are injected fixtures. Auto Mode has not supplied verified account identities or pool relationships, and independent real accounts have not been qualified. This conservative rule may exclude legitimate endpoint-specific quotas until their common or independently verified identity is supplied.
