# Swarm implementation milestone 1 — durable plan and broker

This branch now stores category runs, logical jobs, attempt identities, and broker messages in the daemon SQLite database. The tests exercise one active run per category, restart persistence, a validated dependency graph, generation/revision rejection, and 100-job pagination. Broker tests exercise durable worker discovery, replay after daemon restart, stable-ID dedupe across 2,000 progress replays, bounded envelopes, worker-role rejection, and separate delivered/applied directive acknowledgements.

Evidence: `cargo test --workspace --offline` on 2026-09-26 passed 4 unit tests, 25 existing protocol tests, 3 `swarm_state` tests, and 3 `swarm_broker` tests. The tests start a local daemon with isolated data directories. The extension's pre-existing `npm test` entrypoint is broken because `extension/test/run.js` is absent on the base branch; it was not used as swarm evidence.

These are partial requirements, not a working Swarm mode. No director model turn, worker launch, quota admission, runtime directive transport, artifact acceptance, integration, or VS Code control has been implemented. `swarm.attempt.register` currently creates a durable attempt identity for broker testing without proving a launched process; it must be restricted behind scheduler-owned admission before release. Auto Mode's implementation contract and live harness capabilities remain unavailable on the base branch.

The original checkout and the separate Auto Mode branch were not modified.
