# Swarm implementation milestone 9 — failed writes and authority boundary

Two black-box daemon tests inject SQLite write failures. A failed result transaction neither acknowledges the worker message nor moves its job to `submitted`; after the fault clears, the same message succeeds once. A failed reservation transaction leaves no attempt, admission, allocation, or reservation row. The daemon restarts before the request is retried, and replay returns the one committed attempt. These tests establish atomic local persistence at two current write boundaries; they do not simulate an OS disk-full condition or a live harness launch.

The daemon now rejects coordinator-only plan, directive, inbox, claim, decision, revision, and director-ack calls unless the fixture API is enabled. This keeps those incomplete authority paths unavailable in the default daemon. Worker reports and artifact writes still use attempt tokens. A live director capability and role-scoped message delivery are required before enabling them for production.

Evidence: `daemon/tests/swarm_faults.rs` (2 passing tests) and `daemon/tests/swarm_broker.rs` (7 passing tests). SWARM-50 remains partial because actual launch intent, blocked-state display, and disk-full/recovery behavior are outstanding.

An additional admission test fills the director inbox with 1,000 queued progress events. The next worker admission returns `director_inbox_full`, while an already running worker can still submit its terminal result. This completes the fixture portion of inbox backpressure in SWARM-49. Live scheduling and UI explanation remain outstanding.
