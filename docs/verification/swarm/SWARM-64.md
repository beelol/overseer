# SWARM-64 — versioned scenario and fault replays

Status: partial. Current implementation revision: `d48c33a`. Fixture support only; no live harness communication qualification.

Input and expected behavior: replay S0–S5 against versioned backends and explicit ordered dispatch, message, artifact and failure traces, including adaptive limits and each injected fault. A completed scenario needs its stated final artifact and expected terminal state, not merely a passing policy unit test.

Actual: the first versioned backend is Catalog v1 for S3. Its 24 TypeScript routes provide tied sort keys and insert-between-page behavior. `node fixtures/swarm/catalog-v1/replay-backend.mjs` proves the offset baseline and timestamp-only cursor fail, while the tuple cursor passes. `cargo test --offline -p overseerd --test swarm_scenarios -- --ignored` now joins contract revision, one stale worker-result redirect, four-to-eight admission, eight-result review pressure, 26 isolated patch integrations, a combined checker that fails after four modules and passes after all 24, and evidence-gated `completed` state in one scripted run. A non-ignored test separately exercises the admission gates. [S3](S3.md) records the exact fixture, commands and limitations.

Remaining: S3's director-driven adaptive choice, scope narrowing, conflict/fault variants, final user-facing artifact view and live communication path have not been replayed. S0–S2 and S4–S5 lack complete versioned backends and scenario traces. No RFC checkbox is checked.
