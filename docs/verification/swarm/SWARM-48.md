# SWARM-48 — invalid planning subgraphs

Status: partial. Revisions: `77774af`, `49617a6`, `b71a451`.

Input: a 10-job fixture containing a valid root and child, another independent valid job, a missing dependency and its dependent, a two-job cycle, a missing acceptance check, and two jobs with the same ID. The director explicitly sets `allow_partial: true`; the default remains strict validation.

Expected: reject invalid dispatch while keeping the independent valid subgraphs. Report omitted jobs so the director can repair them without assuming that a smaller plan fulfills the objective.

Actual: the initial red test failed on the missing acceptance field before retaining any work. After the change, the daemon stores three jobs and returns seven indexed rejection reasons. Two stored jobs are ready and their dependent is planned. Strict cycle and unknown-dependency rejection tests still pass. `cargo test --workspace --offline` passed 77 tests.

Evidence: `daemon/tests/swarm_state.rs` (`partial_plan_keeps_independent_valid_subgraphs`, `plan_validates_dependencies_and_revision_before_dispatch`), `daemon/tests/swarm_plan.rs` (resource conflict), and `daemon/src/swarm/plan.rs`.

Additional fixture at `49617a6`: two consecutive `no_progress` director batch completions durably set the run to `stalled`, expose `stall_reason: director_no_progress` and the turn count, and block another batch after daemon restart. A duplicate completion does not increment the count. An explicit `progress` outcome resets the counter; a late completion after Stop preserves `stopping`, and process recovery cannot clear a no-progress stall. The focused director suite passed 8 tests; `cargo test --workspace --offline` passed 106 tests.

Additional fixtures at `b71a451`: two invalid initial plans or two invalid repair revisions, including a daemon restart, persist `failed_planning_turns: 2` and `stall_reason: planning_failed`. A stale generation does not consume a turn; a valid plan clears a preceding failure. An unsupported caller claim of `progress` no longer resets the director counter: completion compares the claimed turn's plan revision and accepted-decision snapshot to durable run state. Evidence accepted during the turn resets the counter. Identical plans and repair revisions return `unchanged` without advancing revision or clearing the stall counter. The schema migration adds the counters and decision snapshot to an earlier Swarm database without losing its run. Focused suites passed (`cargo test --test swarm_state --test swarm_director --test swarm_plan --offline`); `cargo test --workspace --offline` passed 109 tests.

Remaining: the live director does not yet consume partial-plan rejections and repair them. The fixture detector recognizes changed plan revision and accepted evidence, but does not yet classify every possible material-progress event, such as a resolved blocker. Some repair failures outside plan validation still return errors without incrementing the failure counter. No live director runs or complete S3/S5 trace exists. This criterion remains unchecked.
