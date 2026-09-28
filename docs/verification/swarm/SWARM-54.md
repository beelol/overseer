# SWARM-54 — artifact evidence integrity

Status: verified at fixture scope on 2026-09-28 (`claude/auto-swarm`); see the last section. First revision: `594ce7a`.

Input: a submitted fixture artifact with run, job, attempt, source revision and SHA-256 provenance. The test deletes its database row, restores it, changes its revision, restores that, then changes its content without changing the stored hash.

Expected: the director cannot accept a missing, stale or corrupted artifact; restoring the original content permits acceptance against the submitted attempt.

Actual: missing and stale-revision evidence were already rejected. The original test demonstrated that changed content was accepted; after adding hash recomputation at acceptance, all three corruptions are rejected and the restored artifact is accepted. Replaying an artifact ID also verifies the stored content hash. At `594ce7a`, a second test alters the accepted contract artifact after review and before final completion. Completion rejects the changed hash, leaves the run running, and succeeds after the original content is restored. The workspace suite passed with 115 tests.

At `9148e84`, the context fixture adds an artifact created after the source was accepted. Dependent workers cannot receive it, and the director cannot grant it to another destination. A late source result makes previously accepted dependency context unavailable pending a fresh review. The full offline Rust suite passed 157 tests.

Evidence: `daemon/tests/swarm_plan.rs`, `daemon/tests/swarm_dispatch.rs`, `daemon/tests/swarm_context.rs`, `daemon/src/swarm/artifacts.rs`, `daemon/src/swarm/context.rs`, `daemon/src/swarm/completion.rs`, `docs/verification/swarm/milestone-12.md`.

Remaining: this is database-backed fixture evidence. Live workspace file provenance and permission-scoped retrieval by a replacement worker are not implemented. The fixture final report revalidates stored content, attempt and revision, but no live director synthesis is connected.

## Verified at fixture scope (2026-09-28)

| Clause | Test |
| --- | --- |
| Remove an evidence artifact, or change its source revision, after submission: acceptance blocked until restored or revalidated | `acceptance_revalidates_present_revisioned_untampered_evidence` (missing, stale revision and changed content are each refused; the restored artifact is accepted); `completion_rejects_an_accepted_but_unintegrated_patch` and the completion-time hash check (evidence altered after review blocks completion until restored); `atlas_s1_faults_quarantine_stale_and_missing_evidence` (joined, real J2 evidence removed) |
| An explicit provenance chain from the final finding to job/attempt, assignment, source and reproduction | `a_final_finding_keeps_its_provenance_chain_and_another_worker_can_read_it` (`daemon/tests/swarm_plan.rs`, new; passed once its workers were admitted through admission, which records each artifact's destination): J2's reproduction is removed after submission and refused until restored; after J7's independent reproduction resolves J2's conflict with J5, the readouts give J2's coverage row (job, attempt, assignment revision, artifact, `confirmed_application_defect`), the conflict's reproduction (J7 and its artifact) and the run's pinned source commit; the artifact carries its job, source revision and SHA-256 |
| A replacement worker retrieves permitted artifacts without the old transcript | `failed_worker_checkpoint_and_evidence_require_destination_grants_before_replacement` (the same job's attempt 2 on another selected target reads the failed attempt's checkpoint and evidence only after a destination grant, never a transcript; revocation closes it); the new test (a later worker on another destination is refused before the grant and afterwards reads J2's reproduction with its job, source revision and hash) |

Rerun serially on 2026-09-28: `swarm_plan` 13, `swarm_checkpoint` 1, `swarm_integration completion` 2, and the Atlas stale/missing replay.

Boundary: artifacts are daemon-stored records; provenance of files in a live worker's workspace is not claimed.
