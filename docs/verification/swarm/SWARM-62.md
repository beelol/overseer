# SWARM-62 — run deadline

Status: partial. Revision: `042b3e5`.

Input: two runs with a 1.5-second fixture deadline. One has a registered attempt and a queued job; the other has no approved target. Neither receives another admission request. A separate case presents an expired admission timestamp.

Expected: the daemon expires both runs without a model wakeup, identifies deadline as the reason, cancels queued work and requests a stop for active work.

Actual: both runs enter `stopping` within the bounded test wait, expose `stop_reason: deadline`, and distinguish `cancel_requested` active work from `cancelled` queued work. The active attempt receives a queued Stop directive. The admission-triggered path records the same reason. The independent-expiry test first failed with the run still planning and then passed after the daemon timer was added. The workspace suite passed with 67 tests at revision `042b3e5`.

Evidence: `daemon/tests/swarm_control.rs`, `daemon/tests/swarm_state.rs`, `docs/verification/swarm/milestone-10.md`.

Remaining: a queued directive does not prove harness interruption or descendant termination. Confirmed exit, visible unconfirmed exits, UI display, and an explicit extension that preserves the original account allocation remain unverified.
