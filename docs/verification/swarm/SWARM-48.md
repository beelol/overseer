# SWARM-48 — invalid planning subgraphs

Status: partial. Revisions: `77774af`, `49617a6`.

Input: a 10-job fixture containing a valid root and child, another independent valid job, a missing dependency and its dependent, a two-job cycle, a missing acceptance check, and two jobs with the same ID. The director explicitly sets `allow_partial: true`; the default remains strict validation.

Expected: reject invalid dispatch while keeping the independent valid subgraphs. Report omitted jobs so the director can repair them without assuming that a smaller plan fulfills the objective.

Actual: the initial red test failed on the missing acceptance field before retaining any work. After the change, the daemon stores three jobs and returns seven indexed rejection reasons. Two stored jobs are ready and their dependent is planned. Strict cycle and unknown-dependency rejection tests still pass. `cargo test --workspace --offline` passed 77 tests.

Evidence: `daemon/tests/swarm_state.rs` (`partial_plan_keeps_independent_valid_subgraphs`, `plan_validates_dependencies_and_revision_before_dispatch`), `daemon/tests/swarm_plan.rs` (resource conflict), and `daemon/src/swarm/plan.rs`.

Additional fixture at `49617a6`: two consecutive `no_progress` director batch completions durably set the run to `stalled`, expose `stall_reason: director_no_progress` and the turn count, and block another batch after daemon restart. A duplicate completion does not increment the count. An explicit `progress` outcome resets the counter; a late completion after Stop preserves `stopping`, and process recovery cannot clear a no-progress stall. The focused director suite passed 8 tests; `cargo test --workspace --offline` passed 106 tests.

Remaining: the live director does not yet use the partial-plan response to repair a plan. The `progress` outcome is supplied by a fixture caller, not verified against recorded plan changes, accepted evidence, or resolved blockers. Failed planning/repair turns are not counted, and no live director runs. This criterion remains unchecked.
