# SWARM-60 — scoped peer communication

Status: partial. Revision: `9cbe10c`.

Input: the same Atlas-style D1 fixture reports one worker discovery and directs a short advisory to two of three other registered workers. The unrelated membership worker is excluded.

Expected: the director mediates peer sharing; recipient-specific delivery and application are durable, while worker text cannot issue director directives.

Actual: the broker stores J2's discovery in the director inbox, accepts only director-origin advisories for J1/J4, preserves delivered/applied phases, and rejects a worker-origin advisory. J3 receives no advisory. The focused broker suite passed 9 tests.

Evidence: `daemon/tests/swarm_broker.rs`, `daemon/src/swarm/broker.rs`.

Remaining: no live harness delivery, automatic relevance choice, destination artifact permissions, or cross-category access policy is qualified. This is a scripted daemon contract, so the criterion remains unchecked.
