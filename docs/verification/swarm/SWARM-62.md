# SWARM-62 — run deadline

Status: partial. Revision: `c37ca47`.

Input: two runs with a 1.5-second fixture deadline. One has a registered attempt and a queued job; the other has no approved target. Neither receives another admission request. A separate case presents an expired admission timestamp.

Expected: the daemon expires both runs without a model wakeup, identifies deadline as the reason, cancels queued work and requests a stop for active work.

Actual: both runs enter `stopping` within the bounded test wait, expose `stop_reason: deadline`, and distinguish `cancel_requested` active work from `cancelled` queued work. The active attempt receives a queued Stop directive. A new fixture launches a supervised local process and then sends no more admissions; the daemon deadline timer interrupts the process, observes its terminal state, delivers a neutral director event, and marks the job cancelled. The admission-triggered path records the same reason. The combined-main workspace suite passed 102 tests.

Follow-up revision `f91fa58`: the supervised-worker fixture sends ten progress messages during a 2.5-second run. The persisted creation time and effective deadline do not move, the daemon still interrupts the worker for `deadline`, and the job is cancelled with one attempt. The full Rust suite passed 128 tests.

At `f6886d0`, `swarm.get` now exposes linked workers whose exits remain unconfirmed while a run is stopping, including the last Stop-signal outcome and bounded detail count. A Stop fixture proves the readout across a simulated failed signal, restart, and confirmed exit. The deadline-specific fixture was not extended to observe this readout; that path remains partial evidence.

Evidence: `daemon/tests/swarm_control.rs`, `daemon/tests/swarm_state.rs`, `daemon/tests/swarm_runtime.rs`, `docs/verification/swarm/milestone-10.md`.

Remaining: the generic local process is not a qualified live harness, and native descendants are not covered. Deadline-specific unconfirmed-exit readout, UI display, and an explicit extension that preserves the original account allocation remain unverified.
