# SWARM-20 — control actions and descendant exit

Status: partial. Revision: `59d59ff`.

Input: a fixture run has an admitted, running `/bin/sleep` worker. A test-only fault simulates an unreachable control socket on Stop's first interrupt attempt, then kills and restarts the daemon. The fixture is `stop_retries_an_initially_unreachable_worker_after_daemon_restart` in `daemon/tests/swarm_runtime.rs`.

Expected: Stop prevents further work, keeps the worker's exit unconfirmed, retains its execution attempt, and retries the signal after recovery until the linked worker reaches a terminal state.

Actual: Stop returns the worker in `unconfirmed` and leaves the job `cancel_requested` while the process is still running. The durable `swarm_stop_signals` row records one unconfirmed attempt. After restart, the daemon retries after a five-second backoff and the worker reaches `interrupted`; the row records at least two attempts, while the job still has one execution attempt. `cargo test --workspace --offline -q` passed 160 tests at this revision; `git diff --check` passed.

Evidence: `daemon/tests/swarm_runtime.rs`, `daemon/src/swarm/runtime.rs`, `daemon/src/swarm/schema.rs`, `daemon/src/server.rs`.

Remaining: this fixture uses a scripted local worker and simulated first signal failure. Live Pause checkpoints, native descendants, visible reservation/exit state in the UI, and full Swarm-off drain remain unverified. This criterion remains unchecked.
