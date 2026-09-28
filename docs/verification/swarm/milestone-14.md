# Swarm implementation milestone 14 — conservative account pools

The read-only policy preview now checks that every target labeled as the same account shares at least one quota pool. Without that common identity, the target is ineligible with `account_pool_conflict`; with a common account pool and additional model-specific pool, both windows remain binding. Fixture admission already shares reservations for identical pool IDs across categories.

Evidence: `daemon/tests/swarm_policy.rs`, `daemon/tests/swarm_admission.rs`. SWARM-09 remains partial until Auto Mode provides real account/pool discovery and independent-account evidence.
