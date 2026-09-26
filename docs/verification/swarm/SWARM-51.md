# SWARM-51 — uncertain worker liveness

Status: partial. Revision: `8a72059`. Fixture: supervised local generic worker running `/bin/sleep` with a live quota reservation.

Input: admit and launch one worker, then inject Overseer's `disconnected` run state with an `ended_ms` value while its process may still be alive. Call worker reconciliation, allow the daemon's periodic terminal scan to run, and call reconciliation again. Restore the ordinary running state before the rest of the restart/stop fixture proceeds.

Expected: a lost supervisor or transport is not confirmed process death. No terminal event, attempt finalization, reservation release or replacement eligibility may follow solely from the `disconnected` status or its timestamp.

Observed: before `8a72059`, `swarm.worker.reconcile` returned `terminal` for the disconnected worker and confirmed its exit. The reconciler now returns `unknown`, and the periodic terminal scan excludes disconnected runs. The attempt remains `registered` and its quota reservation remains `active` through a daemon tick.

Follow-up revision `1d5792d`: the daemon samples up to four due local worker control sockets per tick, with a 250 ms probe timeout and 15-second per-worker cadence. The shared durable reducer stores `reachable`, `suspect`, and `unknown`: the first failed probe starts an unreachable clock; 60 seconds of continuous failures marks unknown; a successful probe clears it. The scripted worker emits no output yet is sampled as reachable. A fixture injects repeated failed probes around the exact threshold and a progress message in between; progress does not reset the clock or create another attempt. Out-of-order samples are rejected. Reconciliation reports the sampled unknown state, and stored state survives daemon restart. The focused regression and full Rust suite pass (128 tests: 9 unit, 48 protocol, 71 Swarm).

Follow-up revision `442443a`: the supervised worker's actual Unix control socket is moved while its process stays alive. The fixture advances the existing daemon probe through five 15-second intervals using a test-only clock hook. Each probe fails through the real local transport path; the state becomes `suspect`, then `unknown` at 60 seconds, and reconciliation preserves uncertainty. Restoring the socket makes the next probe `reachable` without a replacement attempt. The focused regression and full Rust suite pass (128 tests). This is a deterministic clock replay, not a 60-second wall-clock outage.

Evidence: `daemon/tests/swarm_runtime.rs` (`admitted_worker_launch_replays_to_one_supervised_run_after_daemon_restart`), `daemon/src/swarm/runtime.rs`, `daemon/src/swarm/schema.rs`, `daemon/src/server.rs`, and `daemon/src/shim.rs`.

Remaining: a real 60-second wall-clock transport outage has not yet been replayed. Confirmed dead-process reconciliation, progress-spam interaction with every job/run deadline, and retry after uncertain side effects need versioned fixtures and live qualification. This criterion stays unchecked.
