# SWARM-48 — invalid planning subgraphs

Status: partial. Revisions: `77774af`, `49617a6`, `b71a451`.

Input: a 10-job fixture containing a valid root and child, another independent valid job, a missing dependency and its dependent, a two-job cycle, a missing acceptance check, and two jobs with the same ID. The director explicitly sets `allow_partial: true`; the default remains strict validation.

Expected: reject invalid dispatch while keeping the independent valid subgraphs. Report omitted jobs so the director can repair them without assuming that a smaller plan fulfills the objective.

Actual: the initial red test failed on the missing acceptance field before retaining any work. After the change, the daemon stores three jobs and returns seven indexed rejection reasons. Two stored jobs are ready and their dependent is planned. Strict cycle and unknown-dependency rejection tests still pass. `cargo test --workspace --offline` passed 77 tests.

Evidence: `daemon/tests/swarm_state.rs` (`partial_plan_keeps_independent_valid_subgraphs`, `plan_validates_dependencies_and_revision_before_dispatch`), `daemon/tests/swarm_plan.rs` (resource conflict), and `daemon/src/swarm/plan.rs`.

Additional fixture at `49617a6`: two consecutive `no_progress` director batch completions durably set the run to `stalled`, expose `stall_reason: director_no_progress` and the turn count, and block another batch after daemon restart. A duplicate completion does not increment the count. At this revision, an explicit `progress` outcome reset the counter; `b71a451` supersedes that caller-trusting behavior. A late completion after Stop preserves `stopping`, and process recovery cannot clear a no-progress stall. The focused director suite passed 8 tests; `cargo test --workspace --offline` passed 106 tests.

Additional fixtures at `b71a451`: two invalid initial plans or two invalid repair revisions, including a daemon restart, persist `failed_planning_turns: 2` and `stall_reason: planning_failed`. A stale generation does not consume a turn; a valid plan clears a preceding failure. An unsupported caller claim of `progress` no longer resets the director counter: completion compares the claimed turn's plan revision and accepted-decision snapshot to durable run state. Evidence accepted during the turn resets the counter. Identical plans and repair revisions return `unchanged` without advancing revision or clearing the stall counter. The schema migration adds the counters and decision snapshot to an earlier Swarm database without losing its run. Focused suites passed (`cargo test --test swarm_state --test swarm_director --test swarm_plan --offline`); `cargo test --workspace --offline` passed 109 tests.

Earlier remaining gaps: the live director does not yet consume partial-plan rejections and repair them. At that revision the fixture detector recognized changed plan revision and accepted evidence, but not every material-progress event. Some repair failures outside plan validation still returned errors without incrementing the failure counter. No live director runs or complete S3/S5 trace existed. This criterion remained unchecked.

Resolved-conflict progress follow-up (2026-09-27):
`daemon/tests/swarm_conflict.rs::resolving_a_conflict_during_a_director_turn_resets_no_progress_count`
starts with one no-progress turn, accepts an independent reproduction before
claiming the next turn, then resolves the open evidence conflict during that
turn. Before the fix, `complete_batch` returned `material_progress:false`,
raised the counter to two and stalled the run despite the resolution. A turn
now stores the number of resolved conflicts at claim time and compares it
with the durable count at completion; a caller's `progress` label alone still
cannot reset the counter. The regression passes and the run remains planning
with its counter reset to zero.

The schema migration backfills both the accepted-decision and resolved-conflict
snapshots for legacy active turns so pre-upgrade decisions cannot become false
new progress. `active_legacy_turn_does_not_gain_false_progress_on_upgrade`
failed first with a zero conflict snapshot, then with a zero accepted-decision
snapshot; both passed after conservative backfills. The daemon unit, conflict,
director and plan suites passed against the final code (83 + 3 + 11 + 10 tests).
The opt-in Atlas PostgreSQL disagreement replay was listed but ignored by a
plain test invocation; it was not counted as validation for this change.

Remaining: other material-progress and repair-error classes, autonomous
director repair, qualified live behavior and complete S3/S5 traces are still
unverified. SWARM-48 remains partial.

Semantic repair-loop follow-up at `462a13b` (2026-09-27):
`daemon/tests/swarm_plan.rs::two_semantically_invalid_repair_turns_stall_after_restart`
starts with a valid two-job plan, narrows scope so one logical job is superseded,
then submits a repair that illegally reuses that ID twice with a daemon restart
between attempts. Before the fix, the first rejection left
`failed_planning_turns` at zero. The semantic rejection now records a failed
planning turn after rolling back its revision transaction. The first rejection
leaves revision 2 intact with one failed turn; the second durably sets
`status: stalled` and `stall_reason: planning_failed`. Stale-generation and
malformed-input guards still run before this counter path.

Reproduction: `cargo test -p overseerd --test swarm_plan
two_semantically_invalid_repair_turns_stall_after_restart --offline -- --nocapture`
failed at the first expected count before the fix and passed afterward.
`cargo test -p overseerd --test swarm_plan --test swarm_state --test
swarm_director --offline -- --test-threads=1` passed 11 + 19 + 11 tests;
the related integrated-patch scope-protection test passed separately.
`git diff --check` passed. Repository-wide `cargo fmt --all -- --check`
reports extensive existing formatting differences outside this edit; no
bulk formatting was applied.

Other semantic repair rejections, autonomous director repair, live behavior,
and complete S3/S5 traces remain unverified. The RFC box stays unchecked.
