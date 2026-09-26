# SWARM-08 — shared admission with ordinary runs

Status: partial. Revision: `d9fee5f`.

Input: a real local `/bin/sleep` task occupies an ordinary Overseer run while a fixture Swarm has a two-agent global limit (director plus one possible worker). Both use the same daemon and SQLite store. The Swarm target's quota snapshot is otherwise eligible.

Expected: the ordinary process occupies a global execution slot. Swarm admission holds its first worker until the ordinary run has a confirmed terminal state, then allows the worker from the unchanged quota fixture.

Actual: the red test admitted the Swarm worker while the ordinary process was active. After the fix, admission reports `global_agent_limit`; after the ordinary run exits, it reports `admitted`. `cargo test --workspace --offline` passed 78 tests. The test runs no paid model.

Follow-up at `6152228`: a fixture-launched Swarm worker now appears in Overseer's ordinary `runs` table as well as `swarm_attempts`. Admission excludes that linked ordinary row while the attempt is registered, avoiding a double count; with a three-agent total limit, a second worker can enter alongside the director and first worker. Its test first failed with `global_agent_limit`, then passed. The workspace suite passed 81 tests.

Evidence: `daemon/tests/swarm_admission.rs` (`ordinary_run_occupies_global_slot_until_confirmed_exit`), `daemon/src/swarm/admission.rs`.

Earlier fixture at `d9fee5f`: ordinary `task.create`, direct Swarm admission and scheduler admission serialized through the daemon launch lock. An ordinary task checked the strictest active Swarm global ceiling, rejecting a manual start if the director and worker occupied it. The old regression proved one of two concurrent manual launches was refused. The full offline Rust suite passed 172 tests at that point, but PR review identified the new manual-start failure as a regression in existing behavior.

Intermediate follow-up: manual `task.create` no longer uses a Swarm run's limit to refuse launch. A focused red test reproduced the refusal. With the gate removed, a manual task started while a director and reserved worker occupied a two-agent Swarm ceiling; subsequent Swarm admission remained blocked by `global_agent_limit`. Two concurrent manual starts also succeeded and kept new Swarm admission held. Ordinary `task.create` no longer held `swarm_launch_lock` across repository inspection, worktree creation and process launch. At that revision, this was fixture evidence only; the Swarm-specific ceiling still allowed manual starts to put total activity above it. Swarm's own timer still performs process work under its launch lock and needs separate review.

Before the app-limit follow-up, ordinary tasks still had no shared account-quota reservation, linked profiles were not reconciled to one subscription pool, and there was no application-wide agent cap. Concurrent Swarm-versus-ordinary quota admission and live Auto Mode integration remained unverified.

App-limit follow-up: `agents.max_active` is now a persisted application setting (default 9), used by both ordinary starts and Swarm admission. A manual start reserves a slot before repository/worktree preparation and converts it into its queued run under the same lock used by Swarm admission. A terminal-run follow-up reserves a slot until its new process starts. Active top-level runs, registered worker attempts and running directors count once; native children inherit the parent's slot. At the configured cap, an ordinary start receives typed `agent_limit` with the active count, limit and current agents; Swarm admission waits with `global_agent_limit`. Two simultaneous manual starts at a one-agent cap admit exactly one. The setting survives daemon restart; invalid zero is rejected. Thirty-two-worker stress fixtures explicitly set 33 app slots. No live paid model was used.

The RFC's SWARM-07 wording was revised from counting every native descendant to counting top-level agents, following the app-level cap product decision in the draft PR review. Its verification hash changed from `7e5d0d63d2a94e41e5764ac5dce374544f3e349b6afa58226330adfa55645dce` to the hash recorded in `coverage.json`.

This closes the app-slot race tested here. The independent account-quota race remains: ordinary and Auto Mode launches do not yet reserve from the same durable subscription pool as Swarm. The app's cap also does not yet prove live reviewer behavior, serial director work at max=1, or activation after lowering the limit, so SWARM-07 and SWARM-08 remain partial.

Verification: `cargo test --workspace --offline -q -- --test-threads=1` passed 196 tests with 11 ignored. The parallel suite once failed the unrelated `workspace.tree` 2-second timing assertion at 2.38 seconds; that test passed alone and in the serialized full suite. A final focused app-limit rerun passed after the status-response adjustment. `cargo fmt --all --check` reports extensive pre-existing formatting differences across unrelated files, so this change did not reformat the repository.
