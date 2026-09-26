# SWARM-35 — contract revision and dependent integration

Status: partial. Revision: `14faa93`. Fixture version: local Git integration worktree and scripted accepted `patch` artifact.

Input: a `contract` job produces an accepted patch, exits, and has a dependent `consumer` job. Inspect the dependent before and after `swarm.integrate` applies the patch. Existing plan fixtures cover rejected results and plan revisions separately.

Expected: accepting a patch alone cannot release its dependent; only an integrated current-revision result may do so. A stale source commit or conflicting patch must not silently replace the accepted integration tree.

Actual: the dependent remains `planned` after accepted review and confirmed exit; it becomes `ready` after the patch is applied in the isolated integration worktree. Source HEAD changes block new integration. Two conflicting accepted patches leave only the first integrated commit while retaining the second artifact. The focused test failed before the integration gate and then passed. The full offline Rust suite passed 166 tests.

Replay: `cargo test --offline -p overseerd --test swarm_integration -- --nocapture`; `cargo test --workspace --offline -q`.

Evidence: `daemon/tests/swarm_integration.rs` (`dependent_job_waits_for_accepted_patch_to_integrate`, `source_commit_change_blocks_stale_patch_integration`, `conflicting_accepted_patches_preserve_first_commit_and_second_artifact`), `daemon/src/swarm/artifacts.rs`, `daemon/src/swarm/integration.rs`, and the prior plan-revision fixtures in `daemon/tests/swarm_plan.rs`.

Remaining: the combined check and session-reuse clauses are not implemented. No S3 multi-module run or live worker path has been replayed. This criterion stays unchecked.
