# SWARM-15 — bounded routing failure retries

Status: verified at fixture scope on 2026-09-28 (`claude/auto-swarm`); see the last section.

Input: a one-job backend category permits two fixture targets backed by different accounts. The first target admits an attempt whose supervised worker fails to start its missing executable. The daemon observes its terminal state. The same logical job is admitted on the second target, where the same launch failure occurs. Each terminal receipt is replayed. A plan revision then changes the job title. A separate replay records an unknown external effect before its worker fails.

Expected: a confirmed failed launch without a submitted result releases the logical job for one alternate route. A second failure exhausts its two-attempt budget. Neither duplicate receipts nor replanning reset the count. An unknown side effect blocks replacement, including after a failed worker process.

Observed: before the fix, the first failed worker's job remained `reserved`. After the fix, it becomes `ready` with one attempt and then `failed` with two. A replay of either terminal event does not increment the count, and revision leaves the job failed. In the side-effect replay, the failed worker leaves the job `blocked`; admission on the other target is refused. The focused two-test replay and full offline Rust suite pass (138 tests: 9 unit, 48 protocol, 81 Swarm).

Commands: `cargo test --offline -p overseerd --test swarm_routing -- --nocapture`; `cargo test --workspace --offline -q`.

Evidence: `daemon/tests/swarm_routing.rs`, `daemon/src/swarm/artifacts.rs`, `docs/verification/swarm/SWARM-58.md`.

Remaining: target identities are fixture snapshots, and only a generic supervised process launch was exercised. Live routing and other failure classes remain unqualified. The RFC criterion stays unchecked.

Gate L reconciliation at `78befc0`: Continuity's ordinary successor handoff and
retry lack Swarm job/attempt authority. A linked worker or director now refuses
`run.targets` and `run.handoff`, and a linked failed process is not parked for
Continuity's 36-hour retry. The director must perform any later attempt through
Swarm admission. The fixture tests in `swarm_runtime.rs` and
`swarm_director_process.rs` prove the command boundary; they do not prove a
joined live provider outage or same-job failover. See
`main-reconciliation-2026-09-27.md`.

Cross-check at `884ac97` (2026-09-27):
`daemon/tests/swarm_integration.rs::exhausted_integrated_patch_stays_incomplete_after_late_conflict`
uses two real attempts for a dependent patch: rejected evidence, then an
accepted integrated patch. A late evidence conflict and plan revision leave
the same logical job failed at count two with `attempts_exhausted` in the
coverage readout; a third attempt is refused. Independent reproduction can
resolve the route disagreement, but it does not restore attempt budget or
qualify the stale integrated branch. This extends fixture evidence for the
cross-revision cap. Shared Auto Mode routing and unchanged waiting-state
model wakeups remain unverified, so SWARM-15 stays partial.

Waiting-state replay: `unchanged_waiting_route_does_not_create_director_turns_until_recovery`
observes the same blocked route three times with advancing snapshots, calls the director's
batch claim after each observation, restarts the daemon and calls it again. Every claim
returns `blocked`; the durable director-turn and availability-message counts remain zero.
A fresh eligible observation then creates exactly one wake message and one claimed turn.
Commands: `cargo test -p overseerd --offline --test swarm_availability unchanged_waiting_route_does_not_create_director_turns_until_recovery`
and `cargo test -p overseerd --offline --test swarm_availability -q` (12 passed).
This is a daemon-level no-churn guarantee, not proof that a live director harness cannot
make its own unrecorded model call. The criterion remains partial until the supervised
director and joined Auto route are replayed through an unchanged waiting period.

## Verified at fixture scope (2026-09-28)

| Clause | Test |
| --- | --- |
| Repeated routing failures across targets give at most two execution attempts per logical job | `one_job_falls_back_once_then_stops_and_an_uncertain_effect_pauses` (`daemon/tests/swarm_native.rs`, Auto's route selection on the proposed native path, switched on in the test only): the first failure moves the job to another eligible account, the second exhausts it, a third dispatch is refused `attempt_limit` before any route selection, also after a daemon restart; `two_failed_launch_routes_exhaust_one_logical_jobs_attempts` (`swarm_routing.rs`, fixture routes) |
| Replanning cannot reset the attempt budget | the same native test, extended in this session: after the two failures the director revises the job (a narrower acceptance check) through its `swarm_revise` tool; the job stays `failed` with two attempts and dispatch is still refused `attempt_limit` with no new route decision; `exhausted_integrated_patch_stays_incomplete_after_late_conflict`; S3's contract repair uses attempt 2 of the same job (`catalog_s3_twenty_four_patches_need_a_combined_cursor_check`); a superseded job id cannot be reused (`two_semantically_invalid_repair_turns_stall_after_restart`) |
| An unchanged waiting state produces no repeated planning calls | `unchanged_waiting_route_does_not_create_director_turns_until_recovery` (a blocked route polled repeatedly and across a restart creates no director turn and no wake until recovery); the joined S1 replay (`atlas_s1_backend_evidence_flows_through_swarm_review`): an unchanged run polled over 30 s of fixture time stays at two director turns |

Rerun serially on 2026-09-28: `swarm_native` (one_job_falls_back), `swarm_routing` 2, `swarm_availability` 12, the S1 replay.

Boundary: a live director's own polling habits are not measured; the daemon starts no director turn on an unchanged state.
