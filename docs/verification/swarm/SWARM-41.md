# SWARM-41 — discovery reaches selected peers

Status: partial. Revisions: `9cbe10c`, `3889730`.

Input: an Atlas-style four-job fixture registers J1 projects, J2 tasks, J3 membership, and J4 attachments. J2 reports D1 (`TaskRepository.findById` uses only `id`) before completing. The fixture director claims the eligible bounded batch, sends a D1 advisory to J1 and J4, and records each worker's delivered and applied acknowledgement.

Expected: one durable discovery reaches the director, relevant peers receive only the bounded advisory, J3 receives nothing, and workers cannot impersonate the director or reassign one another.

Actual: the test first failed because the director could not send an advisory. The broker now accepts that director-only directive. J1/J4 each receive and apply one message; J3's inbox is empty. J2's attempted worker-origin advisory is rejected. The director turn completes with one applied discovery. `cargo test --offline --test swarm_broker` passed 9 tests.

Evidence: `daemon/tests/swarm_broker.rs` (`discovery_can_be_routed_to_only_relevant_peers_with_applied_receipts`), `daemon/src/swarm/broker.rs`, `daemon/src/swarm/director.rs`.

Additional fixture at `3889730`: a supervised local `/bin/sh` worker receives its own broker identity through an owner-private launch file. The fixture director sends a targeted advisory after dispatch. The running worker polls its own inbox, records separate delivered and applied acknowledgements, and exits. The same test file has two workers submit artifacts/results through the broker; the director's accepted contract evidence unlocks dependent work. The full workspace suite passed 115 tests.

Remaining: the fixture explicitly chooses recipients; a live director has not inferred relevance, delivered into a qualified Claude/Codex/OpenCode session, or reacted to an in-flight discovery. The scripted worker applies an advisory immediately, not during a long tool call. No S1 end-to-end verdict is claimed.

At `9b853ec`, a separately supervised local director process now claims J2's early D1 discovery, sends one targeted advisory to J4, and later reviews both worker results without the test runner issuing director commands. J2's script waits for J4's applied acknowledgement before submitting its result; the test asserts D1's durable sequence precedes that result, J4's advisory phase is `applied`, and the run completes from the director process. `cargo test --offline -p overseerd --test swarm_director_loop --test swarm_director_process --test swarm_director --quiet` passed 1 new loop, 9 supervised-process and 10 director tests. This is a deterministic local generic-harness loop with a preselected recipient, not an inferred semantic recipient or a live provider turn-boundary path; the criterion remains partial.
