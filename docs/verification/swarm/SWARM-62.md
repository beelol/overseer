# SWARM-62 — run deadline

Status: partial. Revision: `c37ca47`.

Input: two runs with a 1.5-second fixture deadline. One has a registered attempt and a queued job; the other has no approved target. Neither receives another admission request. A separate case presents an expired admission timestamp.

Expected: the daemon expires both runs without a model wakeup, identifies deadline as the reason, cancels queued work and requests a stop for active work.

Actual: both runs enter `stopping` within the bounded test wait, expose `stop_reason: deadline`, and distinguish `cancel_requested` active work from `cancelled` queued work. The active attempt receives a queued Stop directive. A new fixture launches a supervised local process and then sends no more admissions; the daemon deadline timer interrupts the process, observes its terminal state, delivers a neutral director event, and marks the job cancelled. The admission-triggered path records the same reason. The combined-main workspace suite passed 102 tests.

Evidence: `daemon/tests/swarm_control.rs`, `daemon/tests/swarm_state.rs`, `daemon/tests/swarm_runtime.rs`, `docs/verification/swarm/milestone-10.md`.

Remaining: the generic local process is not a qualified live harness, and native descendants are not covered. Visible unconfirmed exits, UI display, and an explicit extension that preserves the original account allocation remain unverified.
