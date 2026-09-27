# SWARM-53 — mutable resource isolation

Status: partial. Revisions: `fa376e9` (admission claims), `0d2074c` (durable planned claims), current draft branch (late observation fixture).

Input: two fixture categories request attempts against the same database namespace. Their admission requests declare `db:shared-test` as an exclusive write claim; the second also tries a read claim while the first writer remains active.

Expected: the conflict is detected before the second attempt or quota reservation is committed. Once the first worker's rejected attempt has confirmed exit and released the claim, the unchanged second request can proceed.

Actual: the pre-admission test initially admitted both writers. With resource claims acquired in the admission transaction, both conflicting requests return `resource_conflict`, the second job stays ready, and its attempt count remains zero. After the first attempt is reviewed and exits, the second writer is admitted. A later fixture moves exact write ownership into each durable job plan and omits it from both admission requests. Before `0d2074c`, both requests were admitted; now the second is blocked, and a request cannot downgrade the planned write to a read. The full offline Rust suite passed 152 tests (10 unit, 49 protocol, 93 Swarm) at `0d2074c`.

Earlier evidence: `docs/verification/swarm/milestone-13.md`.

Late observation fixture: Two admitted workers report use of `db:shared-late` after submitting finding artifacts. The second exclusive observation records contamination against both active attempts, moves both jobs to `cancel_requested`, queues their stop messages, keeps artifacts for audit, marks both coverage rows `contaminated`, and rejects acceptance. A replay of the observation is idempotent. After confirmed exits, both jobs become ready for another budget-checked admission; repeating the collision on attempt 2 fails both jobs and prevents attempt 3. A separate test upgrades a declared read claim to observed write and quarantines both readers. Two observed readers of the same snapshot remain parallel. A supervised three-worker fixture confirms that a late collision requests interruption of the two affected processes while the unrelated process stays active. The daemon retries missed targeted stop signals after restart.

Current evidence: `daemon/tests/swarm_admission.rs`, `daemon/tests/swarm_runtime.rs`.

Remaining: The late observation API is fixture-only and relies on an explicit resource name supplied by the caller. Live tool/resource discovery, canonical aliases, external service mutation detection, and a full Atlas replay are not implemented. When a conflicting owner has no active registered attempt, overlap cannot be proved and the observation is rejected without quarantine; this needs a conservative production rule before enabling it live.
