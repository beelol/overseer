# SWARM-21 — revisions preserve only valid work

Status: partial. Implementation revision: `3c802c8`. Fixture: local Git repository, scripted accepted worker patch and plan revision; no provider call.

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
