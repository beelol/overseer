# SWARM-01 — Auto/Manual × Swarm on/off

Status: partial (one of the four combinations has a normal launch path, on fixtures). Revisions: `ccbedbf0` (daemon `swarm.start`), `a822cb1d` (VS Code **Start Swarm…**), on `claude/auto-swarm`.

Verify clause: exercise all four Auto/Manual × Swarm on/off combinations. Swarm off creates no Overseer worker; a manual swarm stays within its selected pool; an enabled swarm may choose one agent; existing manual and native behaviour is intact.

## Manual × Swarm on

The normal start ([S0](S0.md)) inherits the approved pool (application or category settings, or a one-time selection that is then remembered for the category) and commits the run only after the owner confirms one read-back. In `s0_normal_start_reads_back_once_and_runs_end_to_end` the daemon-launched director offers a job to a target outside that pool and admission refuses it `not_allowed`; every admitted worker runs on an approved target. When no approved account has a usable reading the read-back says `serial` ("one agent at a time"), and `one_slot_director_executes_and_accepts_a_job_without_spawning_a_worker` (`swarm_director_loop.rs`) shows a director doing a job itself with no worker. The director and workers are scripted fixtures (no model harness is a qualified director), so this is fixture coverage of the path, not of a live Swarm.

## Not yet exercised

- **Swarm off:** ordinary starts are unchanged and create no Swarm worker, but no test drives the four-way matrix; `swarm.off` (drain and stop delegating) is covered separately in `swarm_control.rs`.
- **Auto × Swarm on:** Swarm admission books account workers through the shared booking with the qualified draw (CONTRACT-01), but no normal start chooses routes through Auto's selector, and no Auto-routed Swarm run exists.
- **Auto × Swarm off:** Auto's own tests cover it; it is not run as part of this matrix.
- **Native behaviour intact:** not re-run as part of this matrix (the full suite was not run this session).

Evidence: `daemon/tests/swarm_start.rs`, `fixtures/swarm/s0-start-v1/`, `test/unit/swarm-start.js`, [S0](S0.md). Remaining: a qualified director, Auto routing of Swarm jobs, and the four-combination test.
