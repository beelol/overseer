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

Transitive dependency hold at `48e8697` (2026-09-27):
`late_conflict_holds_a_ready_transitive_dependent_before_admission` accepts
the right-hand route finding and a middle job based on it, making a leaf job
ready. When a later left-hand result disputes the route finding, the fixture
previously observed the leaf still `ready`. The broker now records the conflict
and demotes affected ready jobs in one transaction. Dependency checks walk
the full upstream chain, and admission rechecks dependencies even if a job
was already marked ready, so the leaf waits on its middle prerequisite rather
than launching from disputed evidence. After an independent accepted
reproduction supports the original right-hand finding, resolution rematerializes
the leaf as ready. The focused test failed before the fix and passed afterward.

The affected admission, conflict, context, plan, scheduler, and state suites
passed 35 + 5 + 5 + 11 + 4 + 19 tests; the strengthened resolution/refill
assertion passed in a later focused rerun. `git diff --check` passed. Already
running or accepted dependent work is not yet interrupted/re-reviewed solely
because a conflict opens; that remains a separate safety gap, along with live
director behavior and semantic choice. The criterion remains partial.

Active-dependent follow-up at `a7c6c9b` (2026-09-27), after merging main at
`639dfcb`: the first fixture registered a dependent attempt after an accepted
route finding, then opened a later conflict against that finding. Before the
change the dependent stayed `reserved`. The conflict transaction now records a
checkpoint request and `cancel_requested` with `evidence_conflict` for registered
downstream attempts, including transitive dependents. Restart and duplicate
conflict registration leave one checkpoint. Confirmed exit leaves the job blocked
for director replan, rather than treating the old attempt as accepted or silently
retrying it. The fixture passed after the change.

A second fixture admitted a real linked `/bin/sleep` worker for the dependent
and another for unrelated work. Opening the conflict interrupted only the
dependent. Its attempt reconciled to blocked; the unrelated worker remained
running. A failed first fixture setup exposed the existing `benefit_unproven`
admission requirement, which was satisfied by recording a beneficial batch.
The next run observed process interruption before asynchronous attempt
reconciliation, so the fixture now waits for the durable blocked state. The
focused linked-worker test and the seven-test conflict suite passed. Before the
linked fixture was added, the affected admission, conflict, context, plan,
runtime, and state suites passed 35 + 6 + 5 + 11 + 24 + 19 tests. The final
seven-test conflict suite, `git diff --check`, JSON validation, and the
repository link check also passed.

Main's Gate S update clarified AC-195's pending joined contract tests and its
settled briefing, watch, hold, and permission-denial behavior. The RFC now
applies those rules to the single director without giving workers a second
instruction channel. No worker-count, account-allocation, or routing policy
changed. The main update brought five broken README links to its live-probe
evidence; this branch corrected them, and the link check found zero broken
links. Accepted dependent work still needs automatic invalidation/re-review;
autonomous semantic judgment, unresolved partial reporting, and a qualified
live director remain unverified. SWARM-47 stays partial.

Accepted-dependent follow-up at `07817d7` (2026-09-27): a focused fixture
accepts a route finding, a consumer of that finding, its dependent summary,
and an unrelated check before a late contradictory result arrives. Before the
change the consumer and summary still read `accepted`. Conflict registration
now walks the downstream graph and marks already finished submitted/accepted
dependents `blocked` with `evidence_conflict`, preserving their decisions,
attempt counts and artifacts as history. A dependent whose attempt is still
registered instead gets the durable checkpoint/interrupt path. Coverage reads
`dependency_conflict` rather than a current checked conclusion. Restart and
duplicate conflict registration leave one effect, and the unrelated accepted
job remains accepted.

An independent reproduction supporting the original route result resolves the
ancestor conflict but does not silently restore the consumer or summary. The
director explicitly revises the consumer acceptance check; the revision
propagates to the summary. An old artifact is refused for the new revision.
The transitive fixture then executes and accepts a second middle attempt with
fresh evidence before its leaf becomes ready. The new focused fixture failed
first with the consumer still accepted and passed after the change. An older
fixture initially failed because it expected resolution alone to release the
leaf; it now checks the safer re-review sequence. The eight-test conflict suite,
broker (18), context (5), plan (11), runtime (24), and state (19) suites passed.
The disposable PostgreSQL Atlas replay passed all 17 tests, including the J7/J8
contradiction. `git diff --check`, JSON validation, and the link check passed.

This evidence is fixture-only. A real director's semantic choice, a complete
unresolved partial report, an integrated-patch dependent under late
contradiction, and qualified live director behavior remain unverified. The
criterion and RFC box remain partial/unchecked.

Integration race follow-up at `f6ece94` (2026-09-27): an accepted dependent
patch begins integration into Overseer's isolated worktree and records its
durable intent. While the fixture holds it before Git commit, a conflicting
route result opens in under one second and invalidates the dependent review.
Before the change, integration still committed the patch and acknowledged it.
The integration path now checks that the job remains accepted and has no stop
reason immediately before Git commit and again within the acknowledgement
transaction. The same fixture now refuses the stale patch: no integrated
artifact is acknowledged, the isolated branch stays at its base commit, and
the user's source checkout is unchanged. The complete integration (20) and
conflict (8) suites passed; the focused fixture was red before and green after.

An interrupted integration intent and staged patch remain in the isolated
worktree for explicit reconciliation. A conflict after the Git commit but
before durable acknowledgement also needs a recovery/cleanup fixture and
safe path. This iteration therefore does not qualify automatic integration
recovery under every conflict timing; SWARM-47 remains partial.

Invalidated-integration recovery at `884f81a` (2026-09-27): the daemon now
reconciles pending integration intents for jobs invalidated by a late evidence
conflict. One fixture pauses after staging an accepted dependent patch; conflict
registration prevents its commit, and the reconciler removes the exact staged
effect and intent. A second fixture interrupts after Git commit but before
durable acknowledgement, opens the conflict, restarts the daemon, and observes
the private branch return to its recorded parent without acknowledging the
artifact. Both fixtures assert that the user's source checkout is unchanged.
The reconciler checks the private workspace path, recorded parent, expected
tree, clean state and exact integration commit message before resetting it.
A tamper fixture changes the private worktree before restart; recovery leaves
that edit and intent untouched, while plan revision refuses to clear the
conflict until reconciliation. The post-commit fixture failed first because
the intent remained after restart; it passed after the reconciler was added.
`cargo test -p overseerd --test swarm_integration --test swarm_conflict
--test swarm_plan --test swarm_state --offline -- --test-threads=1` passed
22 + 8 + 11 + 19 tests; `git diff --check` passed.

Already acknowledged integrated patches still need a joined contradiction
policy and fixture. Autonomous semantic judgment, unresolved partial reporting,
and a qualified live director path also remain unverified. SWARM-47 stays
partial and its RFC box remains unchecked.

Acknowledged-patch branch hold at `6790bde` (2026-09-27): a fixture first
integrates a dependent patch into the private branch, then accepts an unrelated
patch while a late result disputes the dependency. Before the change, the
unrelated patch could commit on top of the disputed branch. The fixture now
starts that second integration, waits for its durable intent, and opens the
conflict during its pre-commit delay. The integration refuses to commit or
acknowledge; background reconciliation removes its exact staged effect and
intent while preserving the already acknowledged first commit. A fresh
integration request and combined verification are held while the integrated
dependent job remains blocked. The user's source checkout is unchanged.

The same fixture found that a plan revision could clear the blocked job before
the unrelated intent was reconciled; revision now waits for that intent. The
plan-revision assertion failed first with an accepted revision and passed after
the guard. The verification assertion failed first because it reached the
unconfigured fixture verifier and passed after the branch guard. The affected
integration (23), conflict (8), plan (11), and state (19) suites passed with
`--offline -- --test-threads=1`; `git diff --check` passed.

The acknowledged commit is deliberately preserved as historical work; no
automatic rollback of an acknowledged patch is claimed. A durable repair or
explicit disposition path for an integrated patch after conflict resolution,
including a branch with later acknowledged commits, still needs design and
fixtures. The real director's semantic choice, unresolved partial reporting,
and qualified live review remain unverified. SWARM-47 remains partial.

Single-patch repair path at `223db2d` (2026-09-27): the same fixture revises
the invalidated dependent job after the in-flight intent is reconciled. Before
this change, revision cleared its `evidence_conflict` stop reason and an
unrelated accepted patch could immediately integrate on top of the still-open
contradiction. The branch hold now follows unresolved conflict ancestry and
the integrated job's current plan revision, rather than relying only on the
temporary stop reason. After an independent accepted reproduction resolves the
conflict, unrelated integration remains held until the dependent's second
attempt submits a fresh accepted repair patch. That patch integrates as a new
commit whose parent is the old acknowledged commit; the historical artifact is
not silently erased. The unrelated patch can then integrate, and the user's
source checkout stays unchanged. A three-run focused replay passed after the
test stopped generating its patch by briefly editing the private worktree;
that fixture edit had caused an occasional Git index timing mismatch.

The affected integration (23), conflict (8), plan (11), and state (19) suites
passed with `--offline -- --test-threads=1`, followed by the focused parent-
commit assertion and `git diff --check`. This proves one resolved conflict,
one invalidated integrated job, and one fresh patch repair in the local fixture.
Multiple invalidated integrated jobs, exhausted attempt budget, an explicit
no-code-change disposition, director semantic judgment, unresolved partial
reporting, and qualified live review remain unverified; SWARM-47 is partial.

Two-patch dependent repair at `535dc01` (2026-09-27): a second fixture
integrates A's patch, then B's patch after A's integration, before a late
contradiction invalidates both accepted jobs. The director revises both
acceptance checks, obtains an accepted independent reproduction supporting
the original route result, and resolves the conflict. Before the change,
integration of A's fresh repair was refused because B's old integrated patch
was also stale. The branch guard now allows a fresh accepted repair only when
that job has no still-stale integrated ancestor. A repairs first; B remains
planned until A's new patch is integrated, then B repairs. The fixture checks
that each repair is a new child commit of the preceding private commit and
that the user's source checkout is unchanged. It failed first at A's repair
and passed after the ordering rule.

The affected integration (24), conflict (8), plan (11), and state (19) suites
passed with `--offline -- --test-threads=1`; `git diff --check` passed. This
qualifies one two-job dependency chain in a scripted local backend. Independent
multi-patch repair with overlapping edits, an exhausted attempt budget,
explicit no-code-change disposition, autonomous semantic judgment, unresolved
partial reporting, and qualified live director review remain unverified.
SWARM-47 remains partial.
