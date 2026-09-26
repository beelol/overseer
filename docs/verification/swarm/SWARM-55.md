# SWARM-55 — typed audit coverage

Status: partial. Revision: `335d4a9`. Fixture version: inline scripted broker fixture 1. Policy: fixture-only director transitions; one registered attempt per logical job.

Input: a LedgerPay-style audit run with three independent jobs. A worker reports a tested negative result for invalid signatures, an unavailable queue for retry testing, and a claimed duplicate-grant defect with a reproduction artifact. A separate fixture sends a defect claim with only a finding artifact, then supplies a reproduction artifact. A third fixture sends a queue failure after a negative result was accepted, followed by a later “all clear” from the same attempt.

Expected: the coverage readout keeps checked-negative, environment-blocked, and confirmed-application-defect outcomes distinct. An unavailable queue cannot be accepted as a passed check or called an application defect. A claimed defect cannot be confirmed without reproduction evidence. A later message from the same attempt cannot hide an unresolved environment failure at final completion.

Observed: before `335d4a9`, the environment-failure job was accepted, the unsupported defect claim was accepted, and a later negative message hid the queue failure. After the change, those three checks fail closed. `swarm.coverage` reports the distinct states and survives daemon restart. `cargo test --offline --test swarm_broker` passed 12 tests; `cargo test --workspace --offline` passed 118 tests (5 unit, 43 existing protocol, 70 Swarm).

Follow-up revision `533e746`: an accepted negative followed by a new same-attempt defect claim now appears as `review_stale`, not `confirmed_application_defect`; final completion requires a fresh review. A stable-ID replay of the original negative remains idempotent after daemon restart. The focused regression and full Rust suite pass (128 tests: 9 unit, 48 protocol, 71 Swarm). This protects the distinction between a checked negative and an unsupported late defect claim, but does not qualify the full scenario.

Evidence: `daemon/tests/swarm_broker.rs` (`audit_coverage_distinguishes_negative_environment_and_defect_results`, `reported_defect_needs_reproducer_evidence_before_confirmation`, `late_environment_failure_cannot_be_hidden_by_an_earlier_acceptance`, `late_result_invalidates_the_accepted_review_before_completion`), `daemon/src/swarm/broker.rs`, `daemon/src/swarm/coverage.rs`, `daemon/src/swarm/artifacts.rs`, `daemon/src/swarm/completion.rs`.

Remaining: these are broker fixtures, not versioned S1/S2 backend replays or live harness paths. The director still supplies review decisions deterministically; an artifact labeled `reproduction` has not been independently executed or linked to application state by an integration runner. The coverage view does not yet express every required endpoint/permission/ownership matrix or resolve conflicting attempts. This criterion stays unchecked.
