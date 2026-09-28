# SWARM-02 — selected accounts and permitted destinations

Status: verified at fixture scope on 2026-09-28 (`claude/auto-swarm`); see the last section. Reproduce with `cargo test --offline -p overseerd --test swarm_checkpoint -q` and `./fixtures/swarm/dispatch-v1/run-swarm.sh`.

The fixture run saves an allowlist of accounts A and B. After A's worker fails, a newly observed account C remains excluded; the director cannot grant C the checkpoint. B is selected but cannot start the replacement until it has a destination-specific artifact grant. The joined S4 replay separately rejects an unqualified cheap target. This verifies selected-target and checkpoint-destination checks in the daemon fixture, including a denied candidate during recovery.

Live account discovery, actual provider/context permissions, a denied-only routing replay and ordinary fallback are not yet exercised. Keep the RFC box open.

## Verified at fixture scope (2026-09-28)

| Clause | Test |
| --- | --- |
| A newly discovered account stays excluded until selected | `unselected_accounts_are_never_candidates_for_work_or_fallback` (`swarm_native.rs`, new; passed on first run; Auto's route selection on the proposed native path, switched on in the test only): a healthy account discovered on the machine but not in the approved pool is never a route candidate; `auto_selects_each_jobs_route_within_the_approved_pool` (the outside account is never a candidate); `newly_selected_account_cannot_create_allocation_after_first_admission`, `changing_selected_targets_preserves_admitted_evidence_and_restricts_future_work` (fixture admission) |
| A denied destination is never used for workers or fallback | the new test: a worker that fails before any effect falls back to the other approved account, never to the discovered one; every route decision's candidates exclude it |
| …or for checkpoint transfer | `failed_worker_checkpoint_and_evidence_require_destination_grants_before_replacement` (a newly observed account C cannot be granted the checkpoint); `hundred_job_summary_and_scoped_large_artifact_context` (grants to disallowed destinations refused; a revoked destination cannot read) |
| Routing with only denied candidates | the new test: with only the director's own account approved and two healthy accounts discovered, a dispatch is `blocked` / `no_eligible_route`, the discovered accounts are not candidates and nothing is admitted; `missing_target_blocks_durably_and_only_eligibility_change_wakes` (fixture) |

Rerun serially on 2026-09-28: `swarm_native` (new test), `swarm_checkpoint` 1, `swarm_context` 7, `swarm_availability` 12.

Boundary: "discovered" accounts are fixture profiles with readings; the owner's approval is the saved pool.
