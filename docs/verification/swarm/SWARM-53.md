# SWARM-53 — mutable resource isolation

Status: partial. Revisions: `fa376e9` (admission claims), `0d2074c` (durable planned claims).

Input: two fixture categories request attempts against the same database namespace. Their admission requests declare `db:shared-test` as an exclusive write claim; the second also tries a read claim while the first writer remains active.

Expected: the conflict is detected before the second attempt or quota reservation is committed. Once the first worker's rejected attempt has confirmed exit and released the claim, the unchanged second request can proceed.

Actual: the pre-admission test initially admitted both writers. With resource claims acquired in the admission transaction, both conflicting requests return `resource_conflict`, the second job stays ready, and its attempt count remains zero. After the first attempt is reviewed and exits, the second writer is admitted. A later fixture moves exact write ownership into each durable job plan and omits it from both admission requests. Before `0d2074c`, both requests were admitted; now the second is blocked, and a request cannot downgrade the planned write to a read. The full offline Rust suite passed 152 tests (10 unit, 49 protocol, 93 Swarm) at `0d2074c`.

Evidence: `daemon/tests/swarm_admission.rs`, `docs/verification/swarm/milestone-13.md`.

Remaining: planned claims are enforced even when an admission request omits them, but real tool/resource discovery remains absent. Late contamination detection, invalidation and budget-bounded rerun are not implemented. These fixtures use declared exact resource names; aliases are not canonicalized.
