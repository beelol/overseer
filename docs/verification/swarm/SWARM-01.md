# SWARM-01 — Auto/Manual × Swarm on/off

Status: partial (all four combinations run on one daemon on fixtures; the native director that makes Swarm on possible outside fixtures is on by default since the owner's decision of 2026-09-28, `e7e9e18c`, but has run only as the synthetic Claude fixture). Revisions: `ccbedbf0` (daemon `swarm.start`), `a822cb1d` (VS Code **Start Swarm…**), `cfa0eba8` (native director behind `swarm.native_director`), `7fc8dc96` (the matrix test), on `claude/auto-swarm`.

Verify clause: exercise all four Auto/Manual × Swarm on/off combinations. Swarm off creates no Overseer worker; a manual swarm stays within its selected pool; an enabled swarm may choose one agent; existing manual and native behaviour is intact.

## The four-way matrix

`four_way_launch_matrix_auto_manual_by_swarm_on_off` (`daemon/tests/swarm_native.rs`, passed with `--test-threads=1`): one daemon with the Swarm fixture API off (the product path), the Codex app-server and Claude fixtures (synthetic), `swarm.native_director` on and the shared booking's fixture draw for one Claude worker account. Each cell is measured while its work runs, then released (every slot back to 0). The launch path is read from the daemon's own records.

| Cell | Launch path | Booking | App slots | Swarm runs / workers after |
| --- | --- | --- | --- | --- |
| Manual × Swarm off (`task.create`, Auto Mode off) | ordinary | none: the automatic booking finds no qualified draw, so it runs unbooked | 1 | 0 / 0 |
| Auto × Swarm off (`auto.start`, Auto Mode on) | Auto root (`system-codex/gpt-6-sol/medium`, Auto's choice) | Auto's unknown-draw claim on the account (`auto_pool_claims` active), no known-window booking | 1 | 0 / 0 |
| Manual × Swarm on (`swarm.start`, Auto Mode off) | Swarm director (Claude on the approved `system-claude`) + two Swarm workers | director none (its `swarm/director` draw is unknown); each worker a Swarm booking holding its slot, bound to its run | 3 | 1 / 2 |
| Auto × Swarm on (`swarm.start`, Auto Mode on) | the same | the same | 3 | 2 / 4 |

Swarm off creates no Swarm run or worker. Every worker ran on the approved target the director named (a target outside the pool is refused `not_allowed` in S0). No Swarm worker is an Auto root: Auto Mode does not change how a Swarm launches, and no Swarm route is chosen by Auto's selector.

## Manual × Swarm on

The normal start ([S0](S0.md)) inherits the approved pool (application or category settings, or a one-time selection that is then remembered for the category) and commits the run only after the owner confirms one read-back. In `s0_normal_start_reads_back_once_and_runs_end_to_end` the daemon-launched director offers a job to a target outside that pool and admission refuses it `not_allowed`; every admitted worker runs on an approved target. When no approved account has a usable reading the read-back says `serial` ("one agent at a time"), and `one_slot_director_executes_and_accepts_a_job_without_spawning_a_worker` (`swarm_director_loop.rs`) shows a director doing a job itself with no worker. The director and workers are scripted fixtures (no model harness is a qualified director), so this is fixture coverage of the path, not of a live Swarm.

## Why it stays partial

- **Swarm on is decided but not live.** `swarm.native_director` is on by default since `e7e9e18c` (off, a normal start outside the fixture API is blocked `no_qualified_director`); only the synthetic Claude fixture has directed a Swarm.
- **Auto × Swarm on is Swarm with Auto Mode on, not Auto-routed Swarm.** No normal start chooses Swarm routes through Auto's selector.
- **"May choose one agent"** rests on the serial read-back and `swarm_director_loop.rs` (director executes a job itself); the matrix does not re-run it.
- **Native behaviour intact:** targeted regressions only (`swarm_start` 4/4 with the switch off, Gate S `ac180`/`ac185`/`ac190`, the booked Claude worker in `shared_launch.rs`); the full suite was not run this session.
- **Nothing live:** fixture harnesses only; no account was used.

Evidence: `daemon/tests/swarm_native.rs`, `daemon/tests/swarm_start.rs`, `fixtures/swarm/s0-start-v1/`, `fixtures/fake-harness/claude-fixture.js`, `test/unit/swarm-start.js`, [S0](S0.md). Remaining: the owner's decision on the director (the switch), Auto routing of Swarm jobs, and a full-suite run.
