# SWARM-47 — contradictory evidence after review

Status: partial. Implementation revision: `70cffa9`. Support: fixture-only director actions, scripted Atlas workers, disposable PostgreSQL 16 backend. The RFC box remains unchecked.

Input: J2 reports a foreign task mutation (HTTP 200 and Bob's row changed), while J7 reports HTTP 403 and an unchanged row under the guarded fixture option. The director registers both submitted artifact chains as one conflict. A third job, J8, independently probes the unguarded and guarded variants in fresh, separate PostgreSQL namespaces. An alternate local fixture declares the disagreement explicitly unresolved.

Expected: keep both chains and their provenance, prevent acceptance and completion while the disagreement is open, require a bounded independent reproduction before a resolved conclusion, and leave an unresolved conclusion visible without calling it a pass. Replay of the same conflict ID must have one effect across daemon restart.

Observed: `swarm.conflict.open` validates two distinct submitted attempts and their artifact hashes, source revisions and result references. It durably records the disagreement and is idempotent after restart. Coverage for the affected jobs becomes `conflict_unresolved`; accepting either job and completing the run are blocked. `swarm.conflict.resolve` rejects evidence from one of the disputed jobs and rejects a third job before its reproduction artifact is accepted and its attempt exit confirmed. After J8's accepted paired probe, the director resolves the conflict as supporting J2; J2, J7 and the independent J4 path can then be reviewed, and the Atlas replay completes with the fixture-configuration difference stated in its summary. The separate unresolved fixture keeps the conflict visible and continues to block acceptance. The conflict table retains both original artifact references and the reproduction reference.

Evidence: `daemon/tests/swarm_conflict.rs` (two deterministic restart, provenance and unresolved tests), `daemon/tests/swarm_atlas.rs::atlas_s5_contradictory_j7_requires_bounded_j8_reproduction`, `fixtures/swarm/atlas-v1/src/probe.ts` and `probe.test.mjs`, and the conflict store/API in `daemon/src/swarm/conflicts.rs`. Red-first checks failed on the missing conflict method and missing J8 probe; an additional coverage assertion initially returned `defect_awaiting_review` rather than `conflict_unresolved`. After the implementation, `cargo test --offline -p overseerd --test swarm_conflict --quiet` passed two tests, `./fixtures/swarm/atlas-v1/run-local.sh` passed twelve backend tests, `./fixtures/swarm/atlas-v1/run-swarm.sh` passed sixteen joined tests, and `cargo test --offline -p overseerd -- --test-threads=1` passed all non-ignored daemon tests.

Earlier revision `533e746` proved that a later contradictory result invalidates an earlier acceptance through a stored review sequence. That protection remains covered by `late_result_invalidates_the_accepted_review_before_completion` in `daemon/tests/swarm_broker.rs`.

At `70cffa9`, post-acceptance contradiction registration was still missing.
The director's semantic choice of which conclusion J8 supports was scripted,
the attempt budget was fixture-injected, and an explicit unresolved conflict
had no completed-run partial-report path. No live director or qualified
harness path was exercised.

Post-acceptance contradiction follow-up at `ccdfd97` (2026-09-27):
`daemon/tests/swarm_conflict.rs::late_conflict_holds_accepted_jobs_until_contradicted_review_is_revised`
accepts and confirms exit for two incompatible fixture findings and an
independent reproduction. Previously `swarm.conflict.open` rejected the
already-accepted findings as not undecided. It now records their two submitted
artifact chains as an open conflict. The fixture restarts the daemon, replays
the conflict ID, and observes one durable record; both coverage rows become
`conflict_unresolved` and final completion is refused.

The independent reproduction supports the left finding. Because the right
finding was already accepted, resolution is refused until the director revises
that job's acceptance check. The revision leaves its old decision and evidence
in the ledger but no longer treats the job as accepted; resolution can then
record the reproduced outcome, while completion still refuses the unreviewed
revised job. The focused test failed before the fix at conflict registration
and passed afterward. `cargo test -p overseerd --test swarm_conflict --test
swarm_broker --test swarm_plan --test swarm_state --offline -- --test-threads=1`
passed 4 + 18 + 11 + 19 tests; `git diff --check` passed.

This is fixture-only. Autonomous semantic judgment, a complete dependent-job
re-review after contradiction, an unresolved partial final report, and a
qualified live director path remain unverified. SWARM-47 stays partial.
