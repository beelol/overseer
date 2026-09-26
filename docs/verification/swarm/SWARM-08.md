# SWARM-08 — shared admission with ordinary runs

Status: partial. Revision: `d9fee5f`.

Input: a real local `/bin/sleep` task occupies an ordinary Overseer run while a fixture Swarm has a two-agent global limit (director plus one possible worker). Both use the same daemon and SQLite store. The Swarm target's quota snapshot is otherwise eligible.

Expected: the ordinary process occupies a global execution slot. Swarm admission holds its first worker until the ordinary run has a confirmed terminal state, then allows the worker from the unchanged quota fixture.

Actual: the red test admitted the Swarm worker while the ordinary process was active. After the fix, admission reports `global_agent_limit`; after the ordinary run exits, it reports `admitted`. `cargo test --workspace --offline` passed 78 tests. The test runs no paid model.

Follow-up at `6152228`: a fixture-launched Swarm worker now appears in Overseer's ordinary `runs` table as well as `swarm_attempts`. Admission excludes that linked ordinary row while the attempt is registered, avoiding a double count; with a three-agent total limit, a second worker can enter alongside the director and first worker. Its test first failed with `global_agent_limit`, then passed. The workspace suite passed 81 tests.

Evidence: `daemon/tests/swarm_admission.rs` (`ordinary_run_occupies_global_slot_until_confirmed_exit`), `daemon/src/swarm/admission.rs`.

Follow-up: ordinary `task.create`, direct Swarm admission and scheduler admission now serialize through the daemon launch lock. An ordinary task checks the strictest active Swarm global ceiling before creating a workspace. In a two-agent fixture, a director plus reserved worker blocks an ordinary task with no run/workspace left behind. After the worker reservation is released, two concurrent ordinary launch requests race for the one available slot: exactly one launches and one is rejected. The new regression failed before the change and passed afterward. A full-suite run had one intermittent worker-reachability failure in an unrelated runtime fixture; that focused runtime test passed on immediate rerun, and the second full offline Rust suite passed 172 tests. This remains fixture evidence, not live shared account allowance evidence.

Remaining: ordinary tasks still have no shared account-quota reservation, linked profiles are not reconciled to one subscription pool, and reviewer/native-descendant slots are not included. Concurrent Swarm-versus-ordinary quota admission and live Auto Mode integration remain unverified; neither SWARM-07 nor SWARM-08 is checked.
