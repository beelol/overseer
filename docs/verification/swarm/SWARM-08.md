# SWARM-08 — shared admission with ordinary runs

Status: partial. Revision: `d9fee5f`.

Input: a real local `/bin/sleep` task occupies an ordinary Overseer run while a fixture Swarm has a two-agent global limit (director plus one possible worker). Both use the same daemon and SQLite store. The Swarm target's quota snapshot is otherwise eligible.

Expected: the ordinary process occupies a global execution slot. Swarm admission holds its first worker until the ordinary run has a confirmed terminal state, then allows the worker from the unchanged quota fixture.

Actual: the red test admitted the Swarm worker while the ordinary process was active. After the fix, admission reports `global_agent_limit`; after the ordinary run exits, it reports `admitted`. `cargo test --workspace --offline` passed 78 tests. The test runs no paid model.

Follow-up at `6152228`: a fixture-launched Swarm worker now appears in Overseer's ordinary `runs` table as well as `swarm_attempts`. Admission excludes that linked ordinary row while the attempt is registered, avoiding a double count; with a three-agent total limit, a second worker can enter alongside the director and first worker. Its test first failed with `global_agent_limit`, then passed. The workspace suite passed 81 tests.

Evidence: `daemon/tests/swarm_admission.rs` (`ordinary_run_occupies_global_slot_until_confirmed_exit`), `daemon/src/swarm/admission.rs`.

Earlier fixture at `d9fee5f`: ordinary `task.create`, direct Swarm admission and scheduler admission serialized through the daemon launch lock. An ordinary task checked the strictest active Swarm global ceiling, rejecting a manual start if the director and worker occupied it. The old regression proved one of two concurrent manual launches was refused. The full offline Rust suite passed 172 tests at that point, but PR review identified the new manual-start failure as a regression in existing behavior.

Current follow-up: manual `task.create` no longer uses a Swarm run's limit to refuse launch. A focused red test reproduced the refusal. With the gate removed, a manual task starts while a director and reserved worker occupy a two-agent ceiling; subsequent Swarm admission remains blocked by `global_agent_limit`. Two concurrent manual starts also succeed and keep new Swarm admission held. Ordinary `task.create` no longer holds `swarm_launch_lock` across repository inspection, worktree creation and process launch, restoring its independent launch path. All 16 admission tests and the offline workspace suite pass. This is fixture evidence, not live shared account allowance evidence. Until admission is app-level, the Swarm ceiling limits Swarm dispatch, while explicit manual starts can temporarily put total activity above it. Swarm's own timer still performs process work under its launch lock and needs separate review.

Remaining: ordinary tasks still have no shared account-quota reservation, linked profiles are not reconciled to one subscription pool, and reviewer/native-descendant slots are not included. Concurrent Swarm-versus-ordinary quota admission and live Auto Mode integration remain unverified; neither SWARM-07 nor SWARM-08 is checked.
