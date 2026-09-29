# SWARM-26 — serial versus Swarm evaluation

Status: partial. Implementation revision: `9b86545`. Fixture: `evaluation-v1`, manifest version 1, scripted local workers, disposable Git worktrees and a fixture-only quota pool. No provider or customer service is contacted.

Input: four fixed two-job cases—`serial_dependency` (B depends on accepted A), `independent`, `exclusive_conflict` (both write `db:shared`), and `constrained_budget` (225 fixture milli-work-units available against serial cost 220 and parallel cost 230). Each case runs a one-worker baseline and a two-worker candidate. Both use the same two 1,500 ms scripted workers and the same evidence checks. Each worker stores one reproduction artifact and reports 100 scripted fixture milli-work-units. A dependent B is evaluated in a second benefit wave only after A is accepted.

Expected: independent work chooses and executes in parallel, finishing sooner at equal 2/2 acceptance; the other three choose serial and hold B until A is accepted. A dependency must block B as `dependency_pending` with A in `waiting_on`, simultaneous exclusive writes as `resource_conflict`, and a rejected candidate parallel plan as `benefit_serial`. All eight runs must complete with 2/2 accepted evidence and 200 scripted work units. No speedup is required for cases where parallelism is unsafe or unaffordable.

Actual: the independent candidate chose `beneficial` and parallel; the dependency, exclusive-write and constrained-budget candidates chose serial for `dependent_jobs`, `resource_conflict`, and `allocation_exceeded`. All eight local runs accepted 2/2 checks, reported 200 scripted work units and reached evidence-gated completion. The second worker was held for the corresponding dependency, resource or benefit reason. One run on this machine measured:

| Case | One-worker baseline | Two-worker candidate | Candidate execution |
| --- | ---: | ---: | --- |
| Independent | 6,939 ms | 5,161 ms | Parallel |
| Dependency | 8,773 ms | 9,978 ms | Serial, second wave |
| Exclusive write | 7,437 ms | 7,682 ms | Serial |
| Constrained budget | 6,070 ms | 5,390 ms | Serial |

The independent case's elapsed time includes admission, supervisor launch, worker execution, review and completion. Relative to the ideal scripted worker delay, observed orchestration overhead was 3,939 ms baseline and 3,661 ms candidate on that run. Host load affects these numbers; the serial cases make no speed claim. The daemon's provider `actual_usage_milli` correctly remains unknown.

Replay: `cargo test --offline -p overseerd --test swarm_evaluation -- --nocapture --test-threads=1`. After correcting two test-only expected block reasons, the three evaluation tests and fourteen affected runtime tests passed. Nine benefit and eighteen broker regressions passed before that assertion correction; no implementation code changed between those runs. The first sandboxed attempt could not start the local Unix-socket daemon, so the recorded replay used local process permissions outside the sandbox.

Evidence: `fixtures/swarm/evaluation-v1/manifest.json`, `fixtures/swarm/evaluation-v1/worker.py`, `fixtures/swarm/evaluation-v1/README.md`, `daemon/tests/swarm_evaluation.rs`, plus [SWARM-05](SWARM-05.md) for retained estimates and outcomes.

Remaining: `work_units` are scripted receipts, not provider tokens. No native usage, supported-harness comparison or repeatable live quality/time result has been established. The independent wall-time improvement is machine-specific and does not establish universal speed or usage savings. SWARM-26 stays unchecked.

At `53df06a`, the daemon made the serial-dependency blocker explicit as
`dependency_pending` with the predecessor IDs in `waiting_on`; the evaluation
fixture expectation changed accordingly. Its three tests passed alongside the
affected admission and integration suites. The measured results above are
unchanged, and provider usage remains unverified.
