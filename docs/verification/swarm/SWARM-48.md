# SWARM-48 — invalid planning subgraphs

Status: partial. Revision: the partial-plan implementation commit in this branch.

Input: a 10-job fixture containing a valid root and child, another independent valid job, a missing dependency and its dependent, a two-job cycle, a missing acceptance check, and two jobs with the same ID. The director explicitly sets `allow_partial: true`; the default remains strict validation.

Expected: reject invalid dispatch while keeping the independent valid subgraphs. Report omitted jobs so the director can repair them without assuming that a smaller plan fulfills the objective.

Actual: the initial red test failed on the missing acceptance field before retaining any work. After the change, the daemon stores three jobs and returns seven indexed rejection reasons. Two stored jobs are ready and their dependent is planned. Strict cycle and unknown-dependency rejection tests still pass. `cargo test --workspace --offline` passed 77 tests.

Evidence: `daemon/tests/swarm_state.rs` (`partial_plan_keeps_independent_valid_subgraphs`, `plan_validates_dependencies_and_revision_before_dispatch`), `daemon/tests/swarm_plan.rs` (resource conflict), and `daemon/src/swarm/plan.rs`.

Remaining: the live director does not yet use this response to repair a plan. Two failed planning/repair turns and two no-progress director turns do not yet produce a visible stalled state. This criterion remains unchecked.
