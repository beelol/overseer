# SWARM-60 — scoped peer communication

Status: partial. Revision: `9cbe10c`.

Input: the same Atlas-style D1 fixture reports one worker discovery and directs a short advisory to two of three other registered workers. The unrelated membership worker is excluded.

Expected: the director mediates peer sharing; recipient-specific delivery and application are durable, while worker text cannot issue director directives.

Actual: the broker stores J2's discovery in the director inbox, accepts only director-origin advisories for J1/J4, preserves delivered/applied phases, and rejects a worker-origin advisory. J3 receives no advisory. The focused broker suite passed 9 tests.

Evidence: `daemon/tests/swarm_broker.rs`, `daemon/src/swarm/broker.rs`.

Remaining: no live harness delivery, automatic relevance choice, destination artifact permissions, or cross-category access policy is qualified. This is a scripted daemon contract, so the criterion remains unchecked.

Worker-inbox isolation at `7ee7d0c`: the fixture test first showed that an attempt ID alone could read a targeted redirect, even though applying it required the worker token. `swarm.messages` now verifies the recipient's attempt token before returning worker directives. Missing and wrong tokens are rejected; the recipient still reads, delivers and applies its own directive. Scripted Atlas J4 now includes its private token when polling, and the dispatch fixture checks the applied record without exposing that token in a launch response. The full daemon test suite passed serially, including 18 broker tests; all 16 opt-in Atlas PostgreSQL tests passed. This is a daemon/scripted-worker boundary test, not proof of provider-specific credential isolation or live destination permissions.
