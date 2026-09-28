# SWARM-29 — 100-job bounded scale

Status: verified at fixture scope on 2026-09-28 (`claude/auto-swarm`); see the last section. Revisions: `060a0d9` (100-job admission and acceptance), `5f455f8` (director turn during 32 supervised workers).

Input: a version-1 plan with 100 independent jobs, synthetic exact quota of 1,000,000 milli-points, and explicit ceilings of 32 workers and 33 total executing agents. Each group of at most 32 jobs receives a fixture-only beneficial parallel decision. The fixture advances admission time by five seconds after each four-job growth wave, submits one intact local evidence artifact per job, records a result, accepts it, and confirms its unlinked attempt's exit. A separate three-job plan uses the same ceilings but only 3,000 milli-points of remaining quota and a 100-milli-point estimate per worker.

Expected: at most 32 attempts are active alongside director capacity; the 33rd cannot enter. Slots are reused without duplicate logical jobs or acceptances until all 100 have accepted checks. A tighter binding allowance should yield a smaller effective worker pool and a reason.

Observed: the first batch reaches 32 registered attempts while the run is `running`; the 33rd receives `worker_limit`. Four batches admit and accept all 100 jobs, each with one attempt and one accept decision. Replaying the first admission ID after completion returns `already_admitted`. Under the smaller quota, the first two jobs are admitted and the third returns `finishing_reserve`. The focused `swarm_admission` suite passes 14 tests. A separate fixture in `swarm_runtime.rs` starts 32 distinct supervised local processes, delivers a worker discovery to an active director turn while all 32 remain alive, rejects the 33rd worker, and stops the 32 processes. It does not execute the 100-job acceptance chain. The focused runtime test passed at `5f455f8` with local process and Unix-socket access.

Commands: `cargo test --offline -p overseerd --test swarm_admission hundred_jobs_cycle_through_thirty_two_slots_and_accept_once -- --nocapture`; `cargo test --offline -p overseerd --test swarm_admission quota_headroom_explains_smaller_pool_than_worker_ceiling -q`; `cargo test --offline -p overseerd --test swarm_admission -q`; `cargo test --offline -p overseerd --test swarm_runtime explicit_ceiling_runs_thirty_two_supervised_workers -- --nocapture` (with local process and Unix-socket access).

Evidence: `daemon/tests/swarm_admission.rs` (`hundred_jobs_cycle_through_thirty_two_slots_and_accept_once`, `quota_headroom_explains_smaller_pool_than_worker_ceiling`), `daemon/tests/swarm_runtime.rs` (`explicit_ceiling_runs_thirty_two_supervised_workers`), `docs/verification/swarm/SWARM-56.md`.

Remaining: the 100-job chain uses unlinked fixture attempts and synthetic evidence strings. The director turn in the 32-process fixture consumes a scripted discovery; it is not a model review. These fixtures do not demonstrate a working model director, executed acceptance checks, actual account usage or 100 supervised workers, nor combine the separate 32-process fixture with acceptance review under load. Keep the RFC box unchecked.

Ready-window follow-up at `b8faa7d`: a 130-job fixture first failed because all 130 independent jobs were marked ready. The daemon now materializes at most the run's saved ready-window limit (100 by default), retains 30 eligible jobs durably as planned, and promotes the next job in the same transaction when one is admitted. The window survives daemon restart and an additive plan revision. The full offline workspace suite passed 211 non-ignored tests, with 11 intentionally ignored; the strengthened focused fixture passed afterward. This does not qualify live worker scheduling or acceptance review.

## Verified at fixture scope (2026-09-28)

The gap was a combined supervised run with executed checks. `hundred_supervised_jobs_run_their_checks_through_thirty_two_workers` (`daemon/tests/swarm_runtime.rs`, new; passed after its setup was corrected twice: benefit batches must be committed as admission reaches them, and the first 32 workers wait on a gate so that they are observed together) runs it:

| Clause | What the test shows |
| --- | --- |
| 100 fixture jobs, sufficient allowance, independent work, total limit 33, worker limit 32 | one category, 100 independent jobs on a repository commit holding 100 module files; `agents.max_active=33`, `max_workers=32` |
| 32 active workers plus director capacity; no oversubscription | 32 real worker processes are running at once (checked by process status); the registered-attempt count never exceeds 32; with the category's director slot held, an ordinary start at that moment is refused `agent limit` |
| No duplicate jobs; all 100 accepted exactly once after checks | each worker executes its check in its own worktree (`grep -c 'status ok' modules/mNNN.txt`) and reports the exit status and output as evidence; the director (the test) accepts only `exit=0 count=1`; 100 attempts, 100 distinct worker runs, 100 acceptance decisions, every job `accepted` with one attempt |
| Constrained capacity gives a smaller effective pool with an explanation | `quota_headroom_explains_smaller_pool_than_worker_ceiling` (with little allowance only two of three jobs are admitted under a 32-worker ceiling; the third is held `finishing_reserve`, recorded as the last admission reason); the scaled readout in [SWARM-23](SWARM-23.md)'s packaged fixture |

Run: `cargo test --offline -p overseerd --test swarm_runtime hundred_supervised -- --test-threads=1` (about 85 s on this machine; no paid provider, no model).

Boundary: the director's choices are the test's; a model director at this scale and live account limits are SWARM-25 material.
