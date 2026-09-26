# SWARM-54 — artifact evidence integrity

Status: partial. Revision: `7f8dd6c`.

Input: a submitted fixture artifact with run, job, attempt, source revision and SHA-256 provenance. The test deletes its database row, restores it, changes its revision, restores that, then changes its content without changing the stored hash.

Expected: the director cannot accept a missing, stale or corrupted artifact; restoring the original content permits acceptance against the submitted attempt.

Actual: missing and stale-revision evidence were already rejected. The new test demonstrated that changed content was accepted; after adding hash recomputation at acceptance, all three corruptions are rejected and the restored artifact is accepted. Replaying an artifact ID also verifies the stored content hash. The workspace suite passed with 69 tests at revision `7f8dd6c`.

Evidence: `daemon/tests/swarm_plan.rs`, `daemon/src/swarm/artifacts.rs`, `docs/verification/swarm/milestone-12.md`.

Remaining: this is database-backed fixture evidence. A live artifact file can disappear or change after acceptance; final-report revalidation and permission-scoped retrieval by a replacement worker are not implemented.
