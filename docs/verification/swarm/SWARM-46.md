# SWARM-46 — changed shared contract and withdrawn conclusions

Status: verified at fixture scope on 2026-09-28 (`claude/auto-swarm`); see the last section. Fixture: `catalog-v1` version 1, Node.js 24 and scripted daemon APIs. Input: the S3 contract job first submits a timestamp-only cursor; an `accounts` worker submits a patch result against revision 1. The director revises the contract to `(createdAt,id)` and retains the same contract job identity for attempt 2.

Expected: the in-flight worker receives a revision-2 redirect, its old result cannot be accepted or integrated, and dependent work waits for the repaired contract. A separate S1 retraction must later propagate a corrected discovery to all prior recipients.

Actual: `cargo test --offline -p overseerd --test swarm_scenarios -- --ignored` observed one persisted redirect, rejected acceptance of the stale `accounts` artifact, and kept the module dependent on the repaired contract. The contract repair used attempt 2 of the original job. The local Catalog backend replay independently showed that timestamp-only pagination loses a tied row while `(createdAt,id)` passes. See [S3](S3.md) for the full fixture and command.

Remaining: the scripted worker did not acknowledge applying the redirect inside a live model tool call. The scenario did not preserve an unrelated accepted job, retract S1's D1 conclusion, or prove cross-harness delivery. This criterion remains unchecked.

## Verified at fixture scope (2026-09-28)

Checking the D1 half of the clause found a bug: a retraction sent to only some of the workers told of D1 left the others free to be accepted, and a completed run could rest on D1 (silent use of a withdrawn conclusion). Fixed in `a3d20e7a`; regression `a_withdrawn_discovery_must_be_retracted_to_every_prior_recipient` (`daemon/tests/swarm_broker.rs`), red before the fix.

| Clause | Test |
| --- | --- |
| Change S3's shared pagination contract: every affected active or pending job is notified | `catalog_s3_twenty_four_patches_need_a_combined_cursor_check` (versioned Catalog fixture, Node.js 24; passed again on 2026-09-28): the contract repair revises all 25 affected jobs to revision 2 and redirects the one in flight |
| Revision acknowledgement required; stale results invalidated | the in-flight module's revision-1 patch is refused as stale; a redirected attempt's work cannot be accepted until the redirect is applied (`acceptance_waits_for_directive_application_before_unlocking_dependents`, `unapplied_redirect_times_out_and_holds_dependent_work`) |
| Unrelated accepted work is preserved | `revision_invalidates_affected_work_and_preserves_unrelated_acceptance` (the unrelated job stays accepted at revision 1 while the consumer is replanned); `unrelated_plan_revision_preserves_an_accepted_patch_for_integration` |
| Retract D1 in S1 and propagate the correction to all prior recipients; no silent use of withdrawn conclusions | the new regression: D1 went to J1, J4 and J6; a retraction to J4 alone leaves J1 unacceptable (`withdrawn discovery`) until J1 receives and applies its own retraction; J6, accepted while D1 stood, blocks completion; J3, never told, is unaffected. `atlas_s1_faults_quarantine_stale_and_missing_evidence` and the S5 contradiction replay send retractions to J2/J4 with receipts on the real backend |

Rerun on 2026-09-28: `swarm_broker` 24, `swarm_integration` 25, `swarm_conflict` 11, `swarm_dispatch` 8, `swarm_director_loop` 8, `swarm_plan` 13; the two Catalog replays; all 17 Atlas replays.

Boundary: the director decides what to retract and to whom; the daemon now refuses to count work that was told of a withdrawn discovery without its own applied retraction.
