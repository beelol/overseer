# SWARM-08 — shared admission with ordinary runs

Status: verified at fixture scope on 2026-09-28 (`claude/auto-swarm`); see the last section. First revision: `d9fee5f`.

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

This closes the app-slot race tested here. The independent account-quota race remains: ordinary and Auto Mode launches do not yet reserve from the same durable subscription pool as Swarm. The app's cap also does not yet prove live reviewer behavior, serial director work at max=1, or live activation after lowering the limit, so SWARM-07 and SWARM-08 remain partial.

Further fixture checks: a stopped Swarm releases its director slot only after its registered attempt has a confirmed exit, allowing another waiting director to admit work. Lowering the app cap from three to two leaves two existing workers running; no new worker enters until enough attempts finish. Both focused tests pass. These verify slot accounting and drain behavior in the fixture API, not live director execution.

Verification: `cargo test --workspace --offline -q -- --test-threads=1` passed 196 tests with 11 ignored. The parallel suite once failed the unrelated `workspace.tree` 2-second timing assertion at 2.38 seconds; that test passed alone and in the serialized full suite. A final focused app-limit rerun passed after the status-response adjustment. `cargo fmt --all --check` reports extensive pre-existing formatting differences across unrelated files, so this change did not reformat the repository.

Step 3 of the handover on `claude/auto-swarm` (`dc8b37aa`, 2026-09-27): the app-wide
cap is now one count shared with Auto and booked starts (`account_booking::app_slots_in_use`),
read inside each admission transaction; Swarm's in-memory pending counter is replaced by
durable slot holds in that count. A daemon race of an ordinary start, an Auto root, a Swarm
worker admission and a booked start for the last slot admits exactly one and holds it across a
restart (`daemon/tests/shared_launch.rs`,
`ordinary_auto_swarm_and_booked_starts_race_for_the_last_slot_and_one_wins`). A Swarm account
worker books the shared account windows (`swarm_worker_admission_books_the_shared_account_and_binds_its_run`).
Still partial: bookings use fixture upper draws only (no qualified producer), Auto roots and
children hold unknown-draw claims rather than per-window bookings, and live providers are untested.

Qualified upper draw (`b08646f8`, 2026-09-27): a Swarm account worker without fixture inputs now
books on the qualified upper draw that Auto learns from isolated runs of the same harness, model
and effort on the same account (`daemon/src/upper_draw.rs`); with fewer than five attributable
runs it is refused `upper_draw_unknown` with its sample count
(`qualified_draw_admits_a_swarm_worker_after_five_isolated_runs`). The slot count is unchanged.
Still partial: the samples in that test are Claude fixture runs with readings written as the
fixture helper does (Claude has no between-run reading yet), Auto roots and children still hold
unknown-draw claims, and live providers are untested.

## Verified at fixture scope (2026-09-28)

| Clause | Test |
| --- | --- |
| Concurrent Swarm and non-Swarm launches against one nearly depleted pool cannot both reserve the last capacity | `calibrated_auto_units_and_a_swarm_worker_race_for_the_last_window` (`daemon/src/upper_draw.rs`): two Auto children and a Swarm worker, each on its own SQLite connection, race for account room that fits one draw; exactly one books, the Auto losers are refused `unaffordable`, the Swarm loser `shared_pool_headroom`, and nothing the losers tried is left behind. The last app slot, the other scarce capacity: `ordinary_auto_swarm_and_booked_starts_race_for_the_last_slot_and_one_wins` (`shared_launch.rs`, four callers, one winner, held across a restart) |
| Repeat across two tasks and two profiles known to share a subscription | `a_second_profile_of_the_same_subscription_races_for_the_same_last_window` (new; passed on first run and in two reruns): the Auto units (one task) book through profile A while the Swarm worker (a second task, the swarm) books through profile B with the same recorded account fingerprint; still exactly one books. `swarm_worker_admission_books_the_shared_account_and_binds_its_run` shows an ordinary booked start on the same account seeing the Swarm worker's draws (`shared_pool_headroom`) |
| Atomic admission | the booking commits the account claim, window draw, app slot and workspace writer in one transaction (`booked_start_counts_its_slot_once_and_releases_holds_when_the_run_ends`, `crash_before_the_effects_claim_releases_and_after_it_keeps_the_writer_without_retry`); Swarm admission books inside its own immediate transaction |

Rerun serially on 2026-09-28: `upper_draw` unit tests 11, `shared_launch` 7.

Boundary: draws are fixture or calibrated fixture values; live provider readings and Auto roots booking per-window draws (they still hold unknown-draw claims) are outside this fixture proof.
