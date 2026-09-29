# SWARM-10 — allocation and reservation settlement

Status: verified for the criterion's deterministic numeric behavior. Initial evidence revision: `14c62f6`. Latest implementation revision: `635728d6`. Support level: fixture quota points and daemon admission, not a live account feed.

Input: admit the worker, request Stop, simulate an unreachable first control socket, restart the daemon, and wait for the linked worker's confirmed exit. Read the reservation before and after exit.

Expected: a confirmed process exit does not imply known token consumption. The reservation must stop being classified as an active process but continue to bind capacity until the shared account-usage authority reconciles it.

Actual: before confirmed exit, the reservation is `active`; afterward it is `uncertain`. Admission already counts both `active` and `uncertain` reservations. The focused regression was red before `14c62f6` (`active` after exit), then passed after the change. Replay with `cargo test --offline -p overseerd --test swarm_runtime stop_retries_an_initially_unreachable_worker_after_daemon_restart -- --nocapture`. `cargo test --workspace --offline -q` passed 160 tests and `git diff --check` passed.

Evidence: `daemon/tests/swarm_runtime.rs` (`stop_retries_an_initially_unreachable_worker_after_daemon_restart`), `daemon/src/swarm/artifacts.rs` (`confirm_exit`), `daemon/src/swarm/admission.rs`.

Follow-up at `28bbe73`: `daemon/tests/swarm_admission.rs` (`shared_pool_reservation_blocks_stale_capacity_across_categories`) admits an attempt in one category, uses the fixture exit-confirmation API, then retries admission from another category on the same quota pool. The second category remains blocked against a stale snapshot while the first reservation is `uncertain`. The separate runtime fixture above covers an actual supervised worker exit. Replay: `cargo test --offline -p overseerd --test swarm_admission shared_pool_reservation_blocks_stale_capacity_across_categories -- --nocapture` passed.

Historical gap: no authoritative native usage measurement or reconciliation transaction exists, so the uncertain hold cannot yet be released or charged to actual use. Those live/account requirements remain open under SWARM-13 and SWARM-24; they do not establish a live support claim here.

Pool-freeze follow-up: the first successful admission now records the run's
allocation ceiling for every then-approved pool with comparable allowance.
Changing the selected target later cannot calculate a larger allocation from
an account's increased balance or from a newly introduced pool. The
owner-selection fixture confirms a tenfold balance increase does not enlarge
the original cap; a separate fixture blocks a newly selected account with no
frozen pool allocation. A run with no admitted work may still recover as fresh
allowance arrives. The original finishing-reserve and uncertain-settlement
limitations above remain outside this criterion's deterministic numeric clauses.

Exact numeric qualification: the focused `worker_cannot_claim_finishing_purpose_to_spend_the_completion_reserve` replay uses 1,000,000 fresh milli-points. The default 10% allocation is 100,000 milli-points (100 units), with 20,000 reserved for finishing. The daemon admits a 10-unit inspection, then a 60-unit worker, and returns `finishing_reserve` for a further 11-unit ordinary worker. The same ordinary job's forged `purpose=finishing` request was **admitted before the fix**; it now returns `finishing_purpose_not_authorized`. The director's saved plan labels a separate job `budget_role=finishing`; after daemon restart, that job admits 20 units from the protected reserve at the original 100-unit cap. In an independent run with a 35-unit finishing estimate, a 10-unit reservation succeeds and the subsequent 60-unit worker is held by `finishing_reserve`.

Implementation: `budget_role` is stored with each job, defaults to `worker` for older plans, appears in the job readout, and changing it is a material plan revision. The admission request cannot promote an ordinary job to finishing work. Evidence is in `daemon/tests/swarm_admission.rs`, `daemon/src/swarm/{admission,plan,revision,schema,mod}.rs`. The named replay failed on the unauthorized admission before implementation and passed afterward; affected admission, state, plan and revision suites passed 37 + 21 + 11 + 5 tests. `cargo test -p overseerd --offline -q -- --test-threads=1` passed. The full concurrent workspace run had two load-sensitive failures outside this criterion: a TUI event-lag threshold and a director-loop fixture that read its readiness file before the attempt ID was written. The TUI check and director-loop file passed alone; the fixture now publishes its ready marker with an atomic rename. This verifies every numeric clause in SWARM-10 using deterministic admission data; it does not claim live harness/account support.

Ruling: SWARM-10's explicit arithmetic and finishing-access clauses can be verified by deterministic admission fixtures. Native usage reconciliation and the ordinary/Auto/Swarm shared account transaction remain open under SWARM-13 and SWARM-24. If SWARM-10 is interpreted as a live-account qualification, reopen it; no live support is inferred from this fixture.
