# SWARM-04 — deterministic scheduler replay

Status: partial. Code revision: `91d38c2c`. Verification date: 2026-09-27.

Input: a three-job backend plan, the same saved policy and benefit estimate,
one selected target, and a synthetic exact `points` window. The test shuts
down the daemon, checkpoints its SQLite WAL, and copies the database before
either scheduler acts. Both copies restart with identical plan, allocation,
reservation and scheduler cursor state. Both receive the same observation,
request ID, clock and cost estimate.

Expected: both schedulers choose the same category, job, target and admission
status. Renaming only the provider label must not change eligibility. A later
health failure must yield the same blocked reason and candidate explanation.

Actual: both copies admitted `j000`. With only the provider label changed in
one observation, both admitted `j001` to the same target. With that target
marked down, both returned `all_categories_blocked` and candidate reason
`target_unhealthy`. The five-test scheduler suite passed:
`cargo test -p overseerd --offline --test swarm_scheduler -q`.
Evidence: `daemon/tests/swarm_scheduler.rs`, especially
`identical_persisted_state_replays_scheduler_decisions_and_provider_labels_do_not_route`.
No paid harness or network account was used.

Remaining: this is deterministic replay of Swarm's job selection with a
fixture-supplied target. Auto Mode still owns live target ranking and its
quota observations, and ordinary/Auto/Swarm allowance commitments are not
yet one transaction. Replay with the integrated Auto route and shared
reservation state is needed before the entire criterion is verified.
