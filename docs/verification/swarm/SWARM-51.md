# SWARM-51 — uncertain worker liveness

Status: partial. Revision: `8a72059`. Fixture: supervised local generic worker running `/bin/sleep` with a live quota reservation.

Input: admit and launch one worker, then inject Overseer's `disconnected` run state with an `ended_ms` value while its process may still be alive. Call worker reconciliation, allow the daemon's periodic terminal scan to run, and call reconciliation again. Restore the ordinary running state before the rest of the restart/stop fixture proceeds.

Expected: a lost supervisor or transport is not confirmed process death. No terminal event, attempt finalization, reservation release or replacement eligibility may follow solely from the `disconnected` status or its timestamp.

Observed: before `8a72059`, `swarm.worker.reconcile` returned `terminal` for the disconnected worker and confirmed its exit. The reconciler now returns `unknown`, and the periodic terminal scan excludes disconnected runs. The attempt remains `registered` and its quota reservation remains `active` through a daemon tick. The full Rust suite passes: `cargo test --workspace --offline` (128 tests: 9 unit, 48 protocol, 71 Swarm).

Evidence: `daemon/tests/swarm_runtime.rs` (`admitted_worker_launch_replays_to_one_supervised_run_after_daemon_restart`), `daemon/src/swarm/runtime.rs`.

Remaining: output silence and reachability are not yet sampled separately; the configured 60-second transition, recovery after a reachable transport, and a real dead-process reconciliation path need versioned fixtures and live qualification. Progress spam/deadline interaction and retries after uncertain side effects also remain unverified. This criterion stays unchecked.
