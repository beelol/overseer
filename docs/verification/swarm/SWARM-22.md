# SWARM-22 — process replay through daemon restart

Status: partial. Revision: `6152228`.

Input: a fixture-admitted job launches one local `/bin/sleep` worker through Overseer's existing supervisor in an isolated worktree. The daemon is killed and restarted while the worker remains active. The identical launch request is replayed, then Stop is sent.

Expected: durable intent binds the logical attempt to one Overseer run before external launch. Replay returns that run, does not create another process, and the reserved attempt remains visible. Stop interrupts the surviving worker; confirmed exit is required before the attempt may finish.

Actual: the first focused test failed because `swarm.worker.launch` did not exist. It now creates one linked run. Replay after restart returns the same run ID; changing the launch request is rejected. The job remains `reserved` after the launch acknowledgement, and an early exit confirmation fails. Stop interrupts the process and confirmation succeeds after its terminal status. `cargo test --workspace --offline` passed 81 tests.

Evidence: `daemon/tests/swarm_runtime.rs` (`admitted_worker_launch_replays_to_one_supervised_run_after_daemon_restart`), `daemon/src/swarm/runtime.rs`, `daemon/src/store.rs`.

Remaining: this is a scripted generic harness, not a live account-qualified worker. The test does not crash precisely between supervisor spawn and its process record, or between result persistence and acknowledgement. VS Code close/reopen, accepted-result replay and usage reconciliation remain unverified.
