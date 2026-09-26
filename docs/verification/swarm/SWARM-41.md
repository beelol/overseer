# SWARM-41 — discovery reaches selected peers

Status: partial. Revision: `9cbe10c`.

Input: an Atlas-style four-job fixture registers J1 projects, J2 tasks, J3 membership, and J4 attachments. J2 reports D1 (`TaskRepository.findById` uses only `id`) before completing. The fixture director claims the eligible bounded batch, sends a D1 advisory to J1 and J4, and records each worker's delivered and applied acknowledgement.

Expected: one durable discovery reaches the director, relevant peers receive only the bounded advisory, J3 receives nothing, and workers cannot impersonate the director or reassign one another.

Actual: the test first failed because the director could not send an advisory. The broker now accepts that director-only directive. J1/J4 each receive and apply one message; J3's inbox is empty. J2's attempted worker-origin advisory is rejected. The director turn completes with one applied discovery. `cargo test --offline --test swarm_broker` passed 9 tests.

Evidence: `daemon/tests/swarm_broker.rs` (`discovery_can_be_routed_to_only_relevant_peers_with_applied_receipts`), `daemon/src/swarm/broker.rs`, `daemon/src/swarm/director.rs`.

Remaining: the fixture explicitly chooses recipients; a live director has not inferred relevance, dispatched to actual harness sessions, or reacted to an in-flight discovery. No S1 end-to-end verdict is claimed.
