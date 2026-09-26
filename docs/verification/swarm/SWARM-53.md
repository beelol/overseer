# SWARM-53 — mutable resource isolation

Status: partial. Revision: `fa376e9`.

Input: two fixture categories request attempts against the same database namespace. Their admission requests declare `db:shared-test` as an exclusive write claim; the second also tries a read claim while the first writer remains active.

Expected: the conflict is detected before the second attempt or quota reservation is committed. Once the first worker's rejected attempt has confirmed exit and released the claim, the unchanged second request can proceed.

Actual: the pre-admission test initially admitted both writers. With resource claims acquired in the admission transaction, both conflicting requests return `resource_conflict`, the second job stays ready, and its attempt count remains zero. After the first attempt is reviewed and exits, the second writer is admitted. The workspace suite passed with 71 tests at revision `fa376e9`.

Evidence: `daemon/tests/swarm_admission.rs`, `docs/verification/swarm/milestone-13.md`.

Remaining: resource claims must be derived and enforced for real tools, including mutable test databases and services. Late contamination detection, invalidation and budget-bounded rerun are not implemented. This fixture uses a declared exact resource name; aliases are not canonicalized.
