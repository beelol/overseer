# SWARM-21 — revisions preserve only valid work

Status: verified at fixture scope on 2026-09-28 (`claude/auto-swarm`); see the last section. Implementation revisions: `3c802c8`, `690bcff2`. Fixture: local Git repository, scripted accepted worker patch and plan revision; no provider call.

Input: accept a revision-1 patch for `writer`, then revise only a separate `other` job to revision 2. Request patch integration using the current run revision. In a paired run, revise `writer` itself after accepting its old patch.

Expected: the unchanged accepted writer retains its revision-1 evidence and integrates once under run revision 2; a caller using stale run revision 1 is rejected. A writer whose acceptance changed cannot integrate its old patch.

Actual: the focused test first failed because integration required the job revision to equal the run revision. The fix binds the patch, attempt and accept decision to the retained **job** revision while requiring the caller's **run** revision to be current. The unchanged patch integrates and replay deduplicates it; the changed writer is rejected. `cargo test --workspace --offline -q` passed 193 tests with 11 ignored; `git diff --check` passed.

Replay: `cargo test --offline -p overseerd --test swarm_integration unrelated_plan_revision_preserves_an_accepted_patch_for_integration -q`.

Follow-up at `a44d772`: a revision-1 terminal result that was already queued when its
job changed to revision 2 stayed in the director inbox after delivery, blocking truthful
completion. `superseded_result_is_applied_after_delivery_without_accepting_old_evidence`
first failed with `pending_review=1`; after the fix it passes with `pending_review=0`,
the original result marked `applied`, and zero acceptance decisions for that job.
`unreviewed_result_returns_to_director_after_batch_completion_and_restart` still proves
an unchanged job's unreviewed result remains queued. The Catalog S3 replay now reaches
completion after its old result is delivered and acknowledged. Exact commands and the
broader regression results are in [S3](S3.md). Status remains partial.

At `ce34a52`, a scripted scope change omits jobs from a plan while retaining their rows,
attempts, messages and artifacts. The first focused test failed with `revision must retain
existing job ids`. The revised transition marks queued jobs `superseded`, sends a durable
Stop to active excluded attempts, waits for confirmed exit before making them superseded,
and prevents their revision-1 evidence from being accepted as revision 2. An unchanged
retained job keeps its original assignment revision. The coverage readout labels excluded
jobs `excluded_by_scope`; completion requires checks only for retained jobs and refuses
extra checks for excluded jobs. A real isolated-worktree integration test refuses to hide
an already applied patch, and an effect test refuses to erase an uncertain external action.
The 24-module Catalog manifest replay narrows to 12 and leaves precisely 12 excluded jobs.

Replay: `cargo test -p overseerd --test swarm_state`,
`cargo test -p overseerd --test swarm_integration narrowing_scope_refuses_to_hide_an_already_integrated_patch`,
`cargo test -p overseerd --test swarm_effects narrowing_scope_cannot_erase_an_uncertain_external_effect`,
and `cargo test -p overseerd --test swarm_scenarios catalog_s3_narrowing_to_twelve_modules_supersedes_the_other_twelve`.
The complete serial offline Rust suite, the opt-in Catalog backend replay and 17 Atlas
backend tests passed; `git diff --check` passed. No provider account or production service
was used.

Remaining: user-message intake and qualified live director/worker delivery are not
connected. Reversing an already integrated excluded patch needs an explicit new
integration plan; the daemon refuses silent exclusion. This local plan/revision boundary
is partial evidence for SWARM-21, whose RFC box remains unchecked.

Follow-up on 2026-09-27: the opt-in Catalog scoped replay integrates the contract
and twelve retained patches into a thirteen-commit isolated branch. Retained module
attempts keep source revision 1 while the run is at revision 2. The first joined
replay exposed that `swarm.integrate` looked up accept decisions at the job revision
instead of the run revision; it now checks the accepted attempt and artifact. The
completion guard likewise finds unintegrated patches through the attempt's source
revision. Premature completion is rejected, the eleven-route combined check fails,
and the twelve-route check passes only after the last patch integrates. The shared
contract keeps the excluded twelve routes working with their old page parameter.
User-message intake, live director/worker delivery and reversal of already integrated
excluded patches remain open; SWARM-21 is partial.

Director self-work follow-up at `f831eac`: a one-slot supervised director admits an
`inspect` job, then revises the plan to omit it while the job is still running.
`director_death_after_scope_narrowing_supersedes_self_job` first found the job stuck at
`cancel_requested` after confirmed director exit. Recovery now finishes that exact
attempt, retains its quota reservation as uncertain, releases its safe claims, and
settles the omitted job as `superseded` without scheduling a second attempt. The
focused `swarm_director_loop` suite and full serialized offline workspace suite pass.
This is scripted local evidence; user-message intake and qualified live delivery remain
open, so SWARM-21 stays partial.
Replay: `cargo test --offline -q -p overseerd --test swarm_director_loop -- --test-threads=1`.

## Verified at fixture scope (2026-09-28)

The missing piece was the user's side of "change user requirements": there was no intake, only a director revision. `690bcff2` adds `swarm.requirements.change` (the owner's words into the director's inbox; admission held `requirements_pending` until the director's revision names the request; the revision recorded against it; generation unchanged; Overseer class Confirm).

| Clause | Test |
| --- | --- |
| Change user requirements during execution | `catalog_s3_owner_requirement_change_becomes_a_recorded_plan_revision` (`daemon/tests/swarm_scenarios.rs`, new): mid-run, the owner asks to migrate only twelve of 24 modules; the change is idempotent by request id, survives a daemon restart, reaches the director as a `requirement` event, and holds new admission until the director's revision names it |
| Invalidate incompatible queued jobs | the director's revision supersedes the twelve excluded modules; the excluded module in flight receives Stop; `revision_invalidates_affected_work_and_preserves_unrelated_acceptance`, `narrowing_scope_supersedes_queued_work_and_stops_an_active_excluded_attempt` |
| Retain revision provenance | `swarm.get` lists the owner's change with `applied_revision: 2`; the generation stays 1; `committed_revision_replays_after_lost_reply_and_restart` (revision replay) |
| Reject stale worker results from automatic acceptance into the revised plan | the excluded module's revision-1 result cannot be accepted; `superseded_result_is_applied_after_delivery_without_accepting_old_evidence`, `source_commit_change_blocks_stale_patch_integration` |
| S3 24→12: accept and integrate only retained jobs; refuse completion with an unintegrated retained patch; excluded routes still work under the shared contract | `catalog_s3_narrowed_branch_integrates_and_checks_twelve_modules` (versioned Catalog fixture, Node.js 24; passed again on 2026-09-28): twelve retained patches integrate, completion is refused while one retained patch is unintegrated, the combined check fails at 11 and passes at 12, and the excluded routes keep their original page/offset behaviour; `narrowing_scope_refuses_to_hide_an_already_integrated_patch` |

Rerun serially on 2026-09-28: `swarm_scenarios` 5 (including the two opt-in Catalog replays), `swarm_admission` 40, `swarm_state` 21, `swarm_revision_replay` 5, `swarm_plan` 13, daemon unit tests 250, and `overseer ac185` (every daemon method classified).

Boundary: the owner's change arrives through the daemon method; its VS Code and phone entry points are not built here, and the native director's tools do not yet carry the `requirements` argument (the native path stays behind `swarm.native_director`, off).
