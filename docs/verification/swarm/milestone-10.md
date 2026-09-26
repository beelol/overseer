# Swarm implementation milestone 10 — independent deadline expiry

The daemon now checks active category deadlines once per second, including runs that are waiting for an approved account or are paused. A due run enters `stopping`, records `stop_reason: deadline`, cancels queued jobs and queues Stop directives for registered attempts. The same reason is recorded when an admission request itself discovers expiry. A test waits for two runs to expire without sending further admission requests and verifies that active and queued jobs receive distinct states.

The new stop-reason column is additive. A restart test removes that column from an existing swarm database, starts the daemon again, and confirms the run and schema version migrate without loss. Schema version is now 2; a database marked newer than the daemon is rejected instead of silently being downgraded.

Evidence: `daemon/tests/swarm_control.rs` and `daemon/tests/swarm_state.rs`. SWARM-62 remains partial: queued Stop messages are not evidence that a live harness or its descendants actually stopped. Explicit deadline extension, exit confirmation and UI display are still required. The restart case is partial SWARM-22 evidence, not a full launch-reconciliation test.
