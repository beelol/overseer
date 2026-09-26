# SWARM-20 — control actions and descendant exit

Status: partial. Revision: `59d59ff`.

Input: a fixture run has an admitted, running `/bin/sleep` worker. A test-only fault simulates an unreachable control socket on Stop's first interrupt attempt, then kills and restarts the daemon. The fixture is `stop_retries_an_initially_unreachable_worker_after_daemon_restart` in `daemon/tests/swarm_runtime.rs`.

Expected: Stop prevents further work, keeps the worker's exit unconfirmed, retains its execution attempt, and retries the signal after recovery until the linked worker reaches a terminal state.

Actual: Stop returns the worker in `unconfirmed` and leaves the job `cancel_requested` while the process is still running. The durable `swarm_stop_signals` row records one unconfirmed attempt. After restart, the daemon retries after a five-second backoff and the worker reaches `interrupted`; the row records at least two attempts, while the job still has one execution attempt. `cargo test --workspace --offline -q` passed 160 tests at this revision; `git diff --check` passed.

At `f6886d0`, the same fixture verifies that a fresh `swarm.get` call exposes the unconfirmed worker, its job and attempt IDs, process state and last signal outcome after the first failed signal. Once the worker's exit is confirmed after restart, the unconfirmed count falls to zero. The response caps details at 100 workers and reports whether more exist. The full offline Rust suite passed 160 tests (`cargo test --workspace --offline -q`); `git diff --check` passed.

At `14c62f6`, the fixture verifies the quota reservation remains `active` while exit is unconfirmed and becomes `uncertain` after confirmed exit. This keeps capacity held until account usage can be reconciled. The full offline Rust suite passed 160 tests.

Evidence: `daemon/tests/swarm_runtime.rs`, `daemon/src/swarm/runtime.rs`, `daemon/src/swarm/schema.rs`, `daemon/src/server.rs`.

Remaining: this fixture uses a scripted local worker and simulated first signal failure. Live Pause checkpoints, native descendants, reservation/exit display in VS Code, and full Swarm-off drain remain unverified. This criterion remains unchecked.

At `be4dd73`, local control fixtures establish terminalization and claim cleanup:

- Inputs: an empty run; a queued job with a write claim; an active supervised worker whose first stop signal fails; a submitted result awaiting review during Swarm off; and a combined checker still running when Stop arrives. Tests: `stopped_empty_swarm_is_terminal_and_releases_category`, `cancelled_jobs_release_claims_for_later_swarms`, `stop_retries_an_initially_unreachable_worker_after_daemon_restart`, `pause_resume_and_off_keep_active_evidence_but_stop_new_delegation`, and `stop_remains_responsive_while_combined_checker_is_running`.
- Expected: an idle run reaches `stopped` and frees its category and queued-job claims; an active run stays `stopping` or `draining` until worker exit, review, and verification finish. Unresolved attempts and effects keep their claims.
- Actual: the new claim test failed before the fix with `active` rather than `released`, then passed. The other focused tests passed after the change. `cargo test --workspace --offline -q` passed all offline suites (184 passed, 11 ignored); `git diff --check` passed. Repository-wide `cargo fmt --check` still reports extensive pre-existing formatting differences outside this patch, so it was not applied.
- Evidence: `daemon/src/swarm/mod.rs`, `daemon/src/swarm/control.rs`, `daemon/src/swarm/artifacts.rs`, `daemon/src/swarm/verification.rs`, `daemon/tests/swarm_state.rs`, `daemon/tests/swarm_control.rs`, `daemon/tests/swarm_runtime.rs`, and `daemon/tests/swarm_integration.rs`.

At `a45beac`, `stopped_run_recovers_after_orphaned_checker_exits` starts a combined checker, sends Stop, kills the daemon, restarts it while the orphaned checker is alive, then lets the checker exit. Before the patch, the run remained `stopping` beyond five seconds. After periodic lease reconciliation, the checker is recorded `interrupted` and the run reaches `stopped` without another verification request. The affected state, control and integration suites passed (3 + 14 + 9 tests); `cargo test --workspace --offline -q` passed all offline suites (185 passed, 11 ignored).

Remaining: live Pause checkpoints, native descendants and VS Code display are not qualified. SWARM-20 remains partial.
