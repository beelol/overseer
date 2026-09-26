# SWARM-08 — shared admission with ordinary runs

Status: partial. Revision: `d9fee5f`.

Input: a real local `/bin/sleep` task occupies an ordinary Overseer run while a fixture Swarm has a two-agent global limit (director plus one possible worker). Both use the same daemon and SQLite store. The Swarm target's quota snapshot is otherwise eligible.

Expected: the ordinary process occupies a global execution slot. Swarm admission holds its first worker until the ordinary run has a confirmed terminal state, then allows the worker from the unchanged quota fixture.

Actual: the red test admitted the Swarm worker while the ordinary process was active. After the fix, admission reports `global_agent_limit`; after the ordinary run exits, it reports `admitted`. `cargo test --workspace --offline` passed 78 tests. The test runs no paid model.

Evidence: `daemon/tests/swarm_admission.rs` (`ordinary_run_occupies_global_slot_until_confirmed_exit`), `daemon/src/swarm/admission.rs`.

Remaining: this is one-directional. An ordinary `task.create` after Swarm admission is not checked against the same slot ledger, and no ordinary run yet reserves an account quota window. Shared account identity, reverse launch races and descendant enforcement therefore remain unverified; neither SWARM-07 nor SWARM-08 is checked.
