# SWARM-22 — process replay through daemon restart

Status: partial. Revisions: `ae078e6`, `6a32047`.

Input: a fixture-admitted job launches one local `/bin/sleep` worker through Overseer's existing supervisor in an isolated worktree. The daemon is killed and restarted while the worker remains active. The identical launch request is replayed, then Stop is sent.

Expected: durable intent binds the logical attempt to one Overseer run before external launch. Replay returns that run, does not create another process, and the reserved attempt remains visible. Stop interrupts the surviving worker; confirmed exit is required before the attempt may finish.

Actual: the first focused test failed because `swarm.worker.launch` did not exist. It now creates one linked run. Replay after restart returns the same run ID; changing the launch request is rejected. The job remains `reserved` after the launch acknowledgement, and an early exit confirmation fails. Stop interrupts the process. Reconciliation records one terminal event in the director inbox and confirms exit; replay records no second event. A plan revision before Stop does not turn the cancelled job into a retry. `cargo test --workspace --offline` passed 83 tests.

Evidence: `daemon/tests/swarm_runtime.rs` (`admitted_worker_launch_replays_to_one_supervised_run_after_daemon_restart`), `daemon/src/swarm/runtime.rs`, `daemon/src/store.rs`.

Additional fixture at `6a32047`: `swarm.dispatch.next` records a request intent, chooses a ready job through the fair shared admission transaction, and launches a supervised scripted worker without a caller choosing a job. A one-shot failure after admission but before launch leaves exactly one durable attempt and no process. After daemon restart, replay rotates the lost token only because no worker run was linked, then launches that same attempt. A second daemon restart after launch returns the existing worker; changed request content is rejected. A third pending admission followed by Stop cannot launch on replay. The test observes two linked workers and no duplicate attempts or runs. The focused dispatch/runtime/scheduler suites passed six tests; the workspace suite passed 112 tests at this revision.

Remaining: this path still requires a caller to retry a pending intent; the daemon does not yet autonomously drain pending dispatches. It uses an injected fixture target and scripted generic harness, not a live account-qualified director/worker. It does not crash precisely between supervisor spawn and its process record, or between result persistence and acknowledgement. VS Code close/reopen, accepted-result replay and usage reconciliation remain unverified. This criterion stays unchecked.
