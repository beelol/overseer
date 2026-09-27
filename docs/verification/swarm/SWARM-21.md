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

Remaining: user requirement intake and live director delivery are not connected; job removal still needs a safe cancellation transition. This evidence verifies the local unchanged/affected patch boundary, not the whole criterion. The RFC box remains unchecked.
