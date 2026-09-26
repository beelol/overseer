# Swarm implementation milestone 2 — claims and evidence review

The daemon now records category-spanning resource claims. Read claims can coexist; a write claim conflicts with any other active claim on the same resource. This covers separate QA/backend jobs that read one file, and prevents two workers from claiming the same mutable test database or hypothesis. An independent reproducer uses its own explicit claim.

Worker artifacts are immutable by run-scoped ID, carry job/attempt/source-revision provenance and a SHA-256 digest, and are redacted before storage. A director decision requires artifact IDs actually submitted by the same attempt. A rejected result leaves dependent work blocked; a retry can be accepted within the two-attempt limit. Accepted output does not release claims or make dependent work ready until that attempt is confirmed exited. These are fixture-level state transitions, not live process verification.

Evidence: `daemon/tests/swarm_plan.rs` for cross-category claims and evidence-gated review. Live adapter enforcement, director lease authority, external side-effect reconciliation, artifact permission checks, plan revision transitions, and combined integration remain incomplete. The `swarm.attempt.confirm_exit` RPC is temporary fixture-facing scaffolding; runtime reconciliation must own proof of exit before this can be released.

Validation: `cargo test --workspace --offline` passed: 4 daemon unit, 25 existing protocol, 5 swarm broker, 3 swarm plan, and 3 swarm state tests. These tests prove only the scripted state transitions above, not a live harness or completed RFC criterion.
