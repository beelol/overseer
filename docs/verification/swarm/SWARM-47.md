# SWARM-47 — contradictory evidence after review

Status: partial. Revision: `533e746`. Fixture: inline scripted broker fixture 2. Policy: fixture-only director transitions; one registered attempt for one route-audit job.

Input: the worker submits a negative route-audit result and its evidence. The fixture director accepts it. After confirmed worker exit and a daemon restart, the same result is replayed with its stable ID, then the worker reports a new confirmed-defect claim that contradicts the accepted negative. Both result envelopes remain in the durable director inbox; the batch is applied before final completion is attempted.

Expected: duplicate replay has one effect. The newer result cannot inherit the old acceptance. Coverage must show that review is stale, and the final completion gate must require a fresh decision rather than call the defect confirmed or the check passed.

Observed: before `533e746`, coverage reported `confirmed_application_defect` even though the defect had no reproduction evidence, and the old accepted check could pass final completion. The accepted decision now stores the highest result/submit message sequence actually inspected during review. A later result gets a greater sequence, leaves the original envelope intact, changes coverage to `review_stale`, and blocks final completion. The original message replay remains idempotent after restart. The focused test and full Rust suite pass: `cargo test --offline --test swarm_broker late_result_invalidates_the_accepted_review_before_completion`; `cargo test --workspace --offline` (128 tests: 9 unit, 48 protocol, 71 Swarm).

Evidence: `daemon/tests/swarm_broker.rs` (`late_result_invalidates_the_accepted_review_before_completion`), `daemon/src/swarm/artifacts.rs`, `daemon/src/swarm/coverage.rs`, `daemon/src/swarm/completion.rs`, and the additive schema migration in `daemon/src/swarm/schema.rs`.

Remaining: the fixture has one job and one attempt. S1's J2/J7 cross-worker contradiction, bounded independent reproduction, explicit unresolved synthesis, live director review, and versioned backend replay are unverified. Existing accepted decisions migrated without a recorded review sequence conservatively appear stale until reviewed in a new attempt/revision. The criterion stays unchecked.
