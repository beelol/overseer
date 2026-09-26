# SWARM-54 — artifact evidence integrity

Status: partial. Revision: `594ce7a`.

Input: a submitted fixture artifact with run, job, attempt, source revision and SHA-256 provenance. The test deletes its database row, restores it, changes its revision, restores that, then changes its content without changing the stored hash.

Expected: the director cannot accept a missing, stale or corrupted artifact; restoring the original content permits acceptance against the submitted attempt.

Actual: missing and stale-revision evidence were already rejected. The original test demonstrated that changed content was accepted; after adding hash recomputation at acceptance, all three corruptions are rejected and the restored artifact is accepted. Replaying an artifact ID also verifies the stored content hash. At `594ce7a`, a second test alters the accepted contract artifact after review and before final completion. Completion rejects the changed hash, leaves the run running, and succeeds after the original content is restored. The workspace suite passed with 115 tests.

Evidence: `daemon/tests/swarm_plan.rs`, `daemon/tests/swarm_dispatch.rs`, `daemon/src/swarm/artifacts.rs`, `daemon/src/swarm/completion.rs`, `docs/verification/swarm/milestone-12.md`.

Remaining: this is database-backed fixture evidence. Live workspace file provenance and permission-scoped retrieval by a replacement worker are not implemented. The fixture final report revalidates stored content, attempt and revision, but no live director synthesis is connected.
