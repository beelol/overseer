# SWARM-22 — process replay through daemon restart

Status: partial. Revisions: `ae078e6`, `6a32047`, `05be14b`.

Input: a fixture-admitted job launches one local `/bin/sleep` worker through Overseer's existing supervisor in an isolated worktree. The daemon is killed and restarted while the worker remains active. The identical launch request is replayed, then Stop is sent.

Expected: durable intent binds the logical attempt to one Overseer run before external launch. Replay returns that run, does not create another process, and the reserved attempt remains visible. Stop interrupts the surviving worker; confirmed exit is required before the attempt may finish.

Actual: the first focused test failed because `swarm.worker.launch` did not exist. It now creates one linked run. Replay after restart returns the same run ID; changing the launch request is rejected. The job remains `reserved` after the launch acknowledgement, and an early exit confirmation fails. Stop interrupts the process. Reconciliation records one terminal event in the director inbox and confirms exit; replay records no second event. A plan revision before Stop does not turn the cancelled job into a retry. `cargo test --workspace --offline` passed 83 tests.

Evidence: `daemon/tests/swarm_runtime.rs` (`admitted_worker_launch_replays_to_one_supervised_run_after_daemon_restart`), `daemon/src/swarm/runtime.rs`, `daemon/src/store.rs`.

Additional fixture at `6a32047`: `swarm.dispatch.next` records a request intent, chooses a ready job through the fair shared admission transaction, and launches a supervised scripted worker without a caller choosing a job. A one-shot failure after admission but before launch leaves exactly one durable attempt and no process. After daemon restart, replay rotates the lost token only because no worker run was linked, then launches that same attempt. A second daemon restart after launch returns the existing worker; changed request content is rejected. A third pending admission followed by Stop cannot launch on replay. The test observes two linked workers and no duplicate attempts or runs. The focused dispatch/runtime/scheduler suites passed six tests; the workspace suite passed 112 tests at this revision.

Additional fixture at `05be14b`: after ordinary daemon reconciliation, startup scans only persisted, already-admitted, unlinked dispatch intents when fixture APIs are explicitly enabled. It launches the pending attempt before the socket begins serving requests; a subsequent caller replay returns its existing run. An expired target or quota snapshot leaves the admitted attempt reserved and does not start a process, including after restart. The same test simulates target-permission revocation and confirms that replay is blocked. A failed recovery is logged once on startup instead of spinning on a background timer. `cargo test --workspace --offline` passed 113 tests (5 unit, 43 existing protocol, 65 Swarm).

Evidence: `daemon/tests/swarm_dispatch.rs` (`dispatch_recovers_admitted_but_unlaunched_worker_without_duplicate_attempt`, `startup_does_not_launch_an_admitted_worker_from_an_expired_snapshot`), `daemon/src/swarm/dispatch.rs`, `daemon/src/main.rs`.

Remaining: recovery is fixture-only and requires the original snapshot to remain fresh; renewing a stale target and resolving a pending reservation require a live Auto Mode authority. It does not crash precisely between supervisor spawn and its process record, or between result persistence and acknowledgement. VS Code close/reopen, accepted-result replay and usage reconciliation remain unverified. This criterion stays unchecked.
