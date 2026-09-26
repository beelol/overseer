# SWARM-19 — integration and combined verification

Status: partial. Revision: `14faa93`. Fixture version: local Git repositories with scripted Swarm review and exit APIs; no provider account or live service.

Input: submit an accepted `patch` artifact from a worker, confirm its exit, and request integration against the pinned source commit. A second fixture gives two accepted workers conflicting edits to the same file. A third requests final completion before the accepted patch is integrated.

Expected: integration occurs only after review and confirmed exit, in an isolated worktree. A dependent job remains held until the accepted patch is applied. A conflicting second patch leaves the first commit and second artifact intact; an unintegrated patch cannot count as completed work.

Actual: `swarm.integrate` creates an Overseer-owned worktree, applies the accepted patch and records its commit. Replay returns the same commit. The dirty source checkout fingerprint is unchanged. A changed source HEAD blocks new integration. A dependent stays `planned` until integration, then becomes `ready`. A conflicting second patch fails `git apply --check`, preserving the first commit and second artifact. `swarm.complete` rejects an accepted but unintegrated patch. An executable repository hook blocks unattended integration before the hook runs. The focused fixture was red for the missing API, stale-source acceptance, premature dependent readiness, premature completion and hook execution; all six integration tests pass after the changes. `cargo test --workspace --offline -q` passed 166 tests; `git diff --check` passed.

Replay: `cargo test --offline -p overseerd --test swarm_integration -- --nocapture` and `cargo test --workspace --offline -q`.

Evidence: `daemon/tests/swarm_integration.rs`, `daemon/src/swarm/integration.rs`, `daemon/src/swarm/artifacts.rs`, `daemon/src/swarm/completion.rs`, `daemon/src/swarm/schema.rs`.

Ruling: keep `swarm.integrate` fixture-only until live adapter authority and permission scope are qualified — this prevents a simulated director call from being presented as a production integration path; the cost is that normal Swarm launch cannot yet use it.

Ruling: reject repositories with active Git commit hooks during unattended integration — this avoids running an unreviewed hook from the source repository; the cost is that such repositories need a supported, reviewed integration path before this feature works there.

Remaining: no combined build/test check runs on the integrated tree, and a crash between Git commit and SQLite acknowledgement requires reconciliation. Patch conflicts have no director resolution flow. Hook detection has a race against external hook changes. Live adapter permissions, unsaved buffers and service-side writes remain unqualified. This criterion stays unchecked.
