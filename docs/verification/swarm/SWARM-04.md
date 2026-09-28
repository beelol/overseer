# SWARM-04 — deterministic scheduler replay

Status: verified at fixture scope on 2026-09-28 (`claude/auto-swarm`); see the last section. Earlier code revision: `91d38c2c` (2026-09-27).

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

## Verified at fixture scope (2026-09-28)

The gap was replay of the whole route choice now that Auto's selector chooses each job's route. `e224dc2f` adds `swarm.route.replay` (a read method): Auto's selector reruns on a recorded decision's input.

| Clause | Test |
| --- | --- |
| Identical plan, policy, telemetry and reservation state give the same scheduler decisions and reason codes on replay | Swarm's own scheduler: `identical_persisted_state_replays_scheduler_decisions_and_provider_labels_do_not_route` (`swarm_scheduler.rs`, a copied durable database replays the same admissions and blocked reasons). Auto's route choice: `each_route_decision_replays_to_the_same_route_and_reason` (`swarm_native.rs`, new; proposed native path, switched on in the test only): three decisions (default, frontier tier, category allocation exhausted on the first account) each replay from their recorded input to the recorded route and reason, with every Auto exclusion among the recorded ones, identically after a daemon restart; a decision edited to name a route its input does not justify replays as `matches: false` |
| Changing a provider name without changing capabilities or health does not change eligibility | `each_window_binds_and_provider_label_does_not_change_policy` (`swarm_policy.rs`) and the scheduler test above |

Rerun serially on 2026-09-28: `swarm_native` 11, `swarm_scheduler` 5, `swarm_policy` 12, `overseer ac185`.

Boundary: a replay proves the choice follows from its recorded input; whether that input matched live provider state at the time is Auto's observation feed (live).
