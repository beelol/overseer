# SWARM-35 — contract revision and dependent integration

Status: verified at fixture scope on 2026-09-28 (`claude/auto-swarm`); see the last section. Earlier status: partial at `14faa93`. Fixture version: local Git integration worktree and scripted accepted `patch` artifact.

Input: a `contract` job produces an accepted patch, exits, and has a dependent `consumer` job. Inspect the dependent before and after `swarm.integrate` applies the patch. Existing plan fixtures cover rejected results and plan revisions separately.

Expected: accepting a patch alone cannot release its dependent; only an integrated current-revision result may do so. A stale source commit or conflicting patch must not silently replace the accepted integration tree.

Actual: the dependent remains `planned` after accepted review and confirmed exit; it becomes `ready` after the patch is applied in the isolated integration worktree. Source HEAD changes block new integration. Two conflicting accepted patches leave only the first integrated commit while retaining the second artifact. The focused test failed before the integration gate and then passed. The full offline Rust suite passed 166 tests.

Replay: `cargo test --offline -p overseerd --test swarm_integration -- --nocapture`; `cargo test --workspace --offline -q`.

Evidence: `daemon/tests/swarm_integration.rs` (`dependent_job_waits_for_accepted_patch_to_integrate`, `source_commit_change_blocks_stale_patch_integration`, `conflicting_accepted_patches_preserve_first_commit_and_second_artifact`), `daemon/src/swarm/artifacts.rs`, `daemon/src/swarm/integration.rs`, and the prior plan-revision fixtures in `daemon/tests/swarm_plan.rs`.

Follow-up at `fca22c0`: interrupted patch integration now replays from a durable intent, preserving the accepted dependency artifact and one integration commit. This does not change the contract revision and session-reuse gaps.

Follow-up at `089df35`: a combined checker attached to the current integration commit can now block final completion after individually valid patches produce an invalid combined tree.

Follow-up with `catalog-v1`: the versioned S3 fixture revises a timestamp-only contract to a tuple cursor on attempt 2 of the same logical job, redirects one stale module result, rejects its acceptance, and integrates 24 module patches after the contract repair. A combined check fails after four modules and passes after all 24. Replay with `cargo test --offline -p overseerd --test swarm_scenarios -- --ignored`; see [S3](S3.md).

Remaining: applied redirect acknowledgement, qualified session reuse, director-led conflict repair and the complete adaptive S3 run are not implemented. No live worker path has been replayed. This criterion stays unchecked.

Follow-up at `498932c`: dependency eligibility now has one check for accepted status, no active attempt, and no pending accepted patch integration. Two new fixtures reproduced premature readiness before the fix: accepting an unrelated prerequisite and revising a plan to add or change dependents. Both stay `planned` until integration and become `ready` afterward. The complete offline workspace suite passed 207 tests, with 11 intentionally ignored. The remaining gaps above keep this criterion partial.

Applied-directive gate at `9759b031`: a submitted result cannot be accepted
while its attempt has a queued or delivered director redirect, advisory or
retraction. A local broker regression keeps the dependent planned across a
daemon restart, then makes it ready only after the directive's applied receipt,
acceptance and confirmed exit. A final report also refuses an unapplied
directive sent after review. The affected suites passed 70 tests; exact inputs,
red failures and replay command are in [SWARM-43](SWARM-43.md). This is local
receipt-state evidence, not qualified live session application or complete S3
contract repair. SWARM-35 remains partial.

## Verified at fixture scope (2026-09-28, second session)

Built: reuse of a related worker session. `swarm.worker.launch` takes `reuse_session_of` (an earlier attempt). The daemon allows it only when that attempt is of the same swarm run (another run's or category's session is unrelated context), has exited, is related (an earlier attempt of the same logical job, or of a job this one depends on), for a dependent was accepted and not rejected and is not from before its job changed (stale), is not contaminated or in an open evidence conflict, ran on the same admitted route (a session stays on its account and harness), reported a native session, and has not already been continued by another attempt. The new worker run then starts by resuming that native session (`--resume <id>` for Claude; the run's first turn resumes instead of starting fresh), and the launch is recorded in `swarm_session_reuse`. Without reuse, a job starts a fresh session with only its own brief.

| Clause | Test |
| --- | --- |
| Two workers depend on one contract; changing it invalidates the affected pending result/job and blocks stale integration | `revision_invalidates_affected_work_and_preserves_unrelated_acceptance` (`swarm_plan.rs`); `source_commit_change_blocks_stale_patch_integration` (`swarm_integration.rs`); `catalog_s3_twenty_four_patches_need_a_combined_cursor_check` (`swarm_scenarios.rs`, Node 24, rerun 2026-09-28: the stale revision-1 module result is redirected and cannot be accepted; the contract repair is attempt 2) |
| A rejected result cannot unlock dependent work | `evidence_review_and_confirmed_exit_gate_dependent_work` (`swarm_plan.rs`); `acceptance_waits_for_directive_application_before_unlocking_dependents` (`swarm_broker.rs`) |
| Reuse a valid related worker session | `a_related_session_is_reused_and_unrelated_context_never_is` (`swarm_session_reuse.rs`, new): a dependent job's worker resumes the contract worker's session (`--resume fixture-session-1`); `a_session_from_before_a_contract_change_is_stale_for_dependents`: the contract repair (attempt 2) continues attempt 1's session, a dependent may continue attempt 2's but not attempt 1's (stale); `a_rejected_or_contaminated_session_is_not_reused_by_dependents`: a retry continues the rejected attempt's session, a dependent may not |
| …but do not carry unrelated category context into a new job | the first new test: a session is refused to a job without the dependency, to another category's swarm, while its attempt has not exited, and a second time once continued; a fresh launch has no `--resume` and none of the contract worker's prompt |

Red first: before the build, `reuse_session_of` was ignored and every launch started fresh (the related-reuse assertions failed; the stale and rejected launches succeeded).

Runs on 2026-09-28, one file at a time with `--test-threads=1`: `swarm_session_reuse` 3 passed; `swarm_runtime` 27; `swarm_dispatch` 8; `swarm_native` 12; `shared_launch` 7; `swarm_scenarios --ignored` 2 (Node 24).

Boundary: native Claude workers on the synthetic Claude fixture (a fixed session id; echo mode reports the arguments it was started with) and a scripted director. Whether a real harness resumes a session in the new worker's own worktree is live qualification (class (b)), not shown here.
