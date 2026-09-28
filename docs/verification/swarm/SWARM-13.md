# SWARM-13 — enforcement and measured settlement

Status: partial. Support level: local supervised-process and injected-quota fixtures only. The RFC box remains unchecked.

Input examined: one linked scripted worker is stopped after an unreachable control socket and daemon restart. Its process exit is confirmed. A read-only comparison to the separate local Auto Mode branch at `7ca6b89d` inspected `daemon/src/auto_quota.rs` and `daemon/src/auto_consumption.rs` without merging or changing that agent's code.

Expected: a process exit alone cannot release account capacity. Delayed or ambiguous provider usage needs estimate-based labeling and bounded overrun behavior. Settling a reservation requires a comparable, attributed native-unit total; an inclusive parent total must not be added to child totals. A limit may be called strict only after the chosen adapter enforces it through descendants.

Observed: `stop_retries_an_initially_unreachable_worker_after_daemon_restart` passes and leaves the worker's reservation `uncertain` after exit. Swarm capacity still reports provider usage `unknown` with source `fixture_admission`; it does not advertise a strict provider spend cap. Auto's current branch provides percentage-used quota windows and estimated thread credits, which are distinct measurements. Its window-delta assessor returns a bounded interval only under qualified attribution and otherwise returns `unverified`. The revised [shared contract](../../rfcs/swarm-auto-contract.md) states the required unit, provenance, attribution, precision and inclusive-parent rules.

Command: `cargo test -p overseerd --offline --test swarm_runtime stop_retries_an_initially_unreachable_worker_after_daemon_restart -- --nocapture` (passed). Evidence: `daemon/tests/swarm_runtime.rs`, `daemon/src/swarm/{artifacts,admission,mod}.rs`, and the contract's read-only Auto comparison.

Remaining: no selected live adapter has demonstrated a hard cap; no authoritative shared usage settlement exists; no overshoot fixture reconciles actual versus estimated draw; no parent/child aggregate is charged once in the common account ledger. Keep reservations uncertain and SWARM-13 partial until those transactions and tests are joined with Auto Mode. Do not subtract Auto's estimated credits or token counts from a subscription percentage.
