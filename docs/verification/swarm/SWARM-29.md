# SWARM-29 — 100-job bounded scale

Status: partial. Revision: `060a0d9`. Support level: deterministic daemon fixtures; no paid provider work.

Input: a version-1 plan with 100 independent jobs, synthetic exact quota of 1,000,000 milli-points, and explicit ceilings of 32 workers and 33 total executing agents. Each group of at most 32 jobs receives a fixture-only beneficial parallel decision. The fixture advances admission time by five seconds after each four-job growth wave, submits one intact local evidence artifact per job, records a result, accepts it, and confirms its unlinked attempt's exit. A separate three-job plan uses the same ceilings but only 3,000 milli-points of remaining quota and a 100-milli-point estimate per worker.

Expected: at most 32 attempts are active alongside director capacity; the 33rd cannot enter. Slots are reused without duplicate logical jobs or acceptances until all 100 have accepted checks. A tighter binding allowance should yield a smaller effective worker pool and a reason.

Observed: the first batch reaches 32 registered attempts while the run is `running`; the 33rd receives `worker_limit`. Four batches admit and accept all 100 jobs, each with one attempt and one accept decision. Replaying the first admission ID after completion returns `already_admitted`. Under the smaller quota, the first two jobs are admitted and the third returns `finishing_reserve`. The focused `swarm_admission` suite passes 14 tests. A separate earlier fixture in `swarm_runtime.rs` starts 32 distinct supervised local processes and holds a 33rd, but it does not execute this 100-job acceptance chain.

Commands: `cargo test --offline -p overseerd --test swarm_admission hundred_jobs_cycle_through_thirty_two_slots_and_accept_once -- --nocapture`; `cargo test --offline -p overseerd --test swarm_admission quota_headroom_explains_smaller_pool_than_worker_ceiling -q`; `cargo test --offline -p overseerd --test swarm_admission -q`.

Evidence: `daemon/tests/swarm_admission.rs` (`hundred_jobs_cycle_through_thirty_two_slots_and_accept_once`, `quota_headroom_explains_smaller_pool_than_worker_ceiling`), `daemon/tests/swarm_runtime.rs` (`explicit_ceiling_runs_thirty_two_supervised_workers`), `docs/verification/swarm/SWARM-56.md`.

Remaining: the 100-job chain uses unlinked fixture attempts and synthetic evidence strings. It does not demonstrate a working model director, executed acceptance checks, actual account usage or 100 supervised workers, nor combine the separate 32-process fixture with review under load. Keep the RFC box unchecked.
