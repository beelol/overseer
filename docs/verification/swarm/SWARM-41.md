# SWARM-41 — discovery reaches selected peers

Status: verified at fixture scope on 2026-09-28 (`claude/auto-swarm`); see the last section. Revisions: `9cbe10c`, `3889730`, `362248b7`.

Input: an Atlas-style four-job fixture registers J1 projects, J2 tasks, J3 membership, and J4 attachments. J2 reports D1 (`TaskRepository.findById` uses only `id`) before completing. The fixture director claims the eligible bounded batch, sends a D1 advisory to J1 and J4, and records each worker's delivered and applied acknowledgement.

Expected: one durable discovery reaches the director, relevant peers receive only the bounded advisory, J3 receives nothing, and workers cannot impersonate the director or reassign one another.

Actual: the test first failed because the director could not send an advisory. The broker now accepts that director-only directive. J1/J4 each receive and apply one message; J3's inbox is empty. J2's attempted worker-origin advisory is rejected. The director turn completes with one applied discovery. `cargo test --offline --test swarm_broker` passed 9 tests.

Evidence: `daemon/tests/swarm_broker.rs` (`discovery_can_be_routed_to_only_relevant_peers_with_applied_receipts`), `daemon/src/swarm/broker.rs`, `daemon/src/swarm/director.rs`.

Additional fixture at `3889730`: a supervised local `/bin/sh` worker receives its own broker identity through an owner-private launch file. The fixture director sends a targeted advisory after dispatch. The running worker polls its own inbox, records separate delivered and applied acknowledgements, and exits. The same test file has two workers submit artifacts/results through the broker; the director's accepted contract evidence unlocks dependent work. The full workspace suite passed 115 tests.

Remaining: the fixture explicitly chooses recipients; a live director has not inferred relevance, delivered into a qualified Claude/Codex/OpenCode session, or reacted to an in-flight discovery. The scripted worker applies an advisory immediately, not during a long tool call. No S1 end-to-end verdict is claimed.

At `9b853ec`, a separately supervised local director process now claims J2's early D1 discovery, sends one targeted advisory to J4, and later reviews both worker results without the test runner issuing director commands. J2's script waits for J4's applied acknowledgement before submitting its result; the test asserts D1's durable sequence precedes that result, J4's advisory phase is `applied`, and the run completes from the director process. `cargo test --offline -p overseerd --test swarm_director_loop --test swarm_director_process --test swarm_director --quiet` passed 1 new loop, 9 supervised-process and 10 director tests. This is a deterministic local generic-harness loop with a preselected recipient, not an inferred semantic recipient or a live provider turn-boundary path; the criterion remains partial.

## Verified at fixture scope (2026-09-28)

The joined S1 replay `atlas_s1_backend_evidence_flows_through_swarm_review` (`daemon/tests/swarm_atlas.rs`, real Express/PostgreSQL Atlas backend) was extended in this session. Extending it found a bug: S1's two coordination turns in a row (route D1, then resolve J4's overlap) stalled the run as `director_no_progress`, because only a plan revision, an acceptance or a resolved conflict counted as progress. Fixed in `362248b7`: an advisory, redirect or retraction the director records during the turn is progress; an empty turn still counts toward the stall. Regression: `coordination_turns_that_direct_workers_are_progress_but_empty_turns_are_not` (`daemon/tests/swarm_director.rs`), red before the fix; the migration test covers the new claim snapshot.

| Clause | What the S1 replay shows |
| --- | --- |
| D1 is delivered before J2 completes, and persisted | J2 reports D1 (symbol, source revision, callers) while its attempt is running; a duplicate D1 is one effect; J2's result comes later |
| The director is told in the next eligible bounded batch | the batch claimed after the 5-second batching delay starts with D1 |
| The advisory goes only to J1 and J4, with receipt and application | the only `D1-to-*` envelopes are addressed to J1's and J4's attempts, each `delivered` then `applied`; J2's and J3's inboxes hold none |
| An in-flight director turn queues the next batch | J4's overlap claim arrives while the D1 turn is open; a second claim is `busy` and there is one director turn; after it completes, the next batch holds the overlap, which the director resolves with J4's applied redirect |
| No periodic model calls in an unchanged run | afterwards, repeated claims over 30 seconds of fixture time return `idle`; the run still has exactly two director turns; `director_waits_for_age_or_byte_limit_without_background_inference` and `unchanged_waiting_route_does_not_create_director_turns_until_recovery` |

Rerun on 2026-09-28: all 17 joined Atlas tests against disposable PostgreSQL 16; `swarm_director` 13, `swarm_conflict` 11, `swarm_director_loop` 8, `swarm_director_process` 14, `swarm_start` 4, `swarm_dispatch` 8, `swarm_state` 21, `swarm_plan` 13; the schema unit tests.

Boundary: the director's routing choices are scripted; a live director's semantic choice of recipients is not claimed.
