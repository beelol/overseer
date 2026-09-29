# SWARM-36 — fair admissions across categories

Status: verified at fixture scope on 2026-09-28 (`claude/auto-swarm`); see the last section. Fixture implementation at `09d1a89`.

Input: two active category runs, one with 100 ready jobs and one with two, share a fresh synthetic account/pool snapshot. A fixture-only central scheduler is asked for four worker admissions with stable request IDs. The daemon restarts between the third and fourth requests; one earlier request is replayed, and a changed replay is attempted.

Expected: when both categories are eligible, the first two available worker admissions include one from each. The category turn order is durable and cannot duplicate or overwrite admission on replay. Each selected job uses the same quota, concurrency, wave and resource transaction as ordinary fixture Swarm admission.

Actual: admissions alternate A → B → A → B with jobs `j000`, `j000`, `j001`, `j001`. A replay returns the existing attempt, a changed replay is rejected, and the database contains four attempts after daemon restart. The scheduler cursor and dispatch record commit in the same SQLite transaction as the reservation and attempt. No model call is made to decide which category gets the next slot.

Verification: the focused scheduler/admission/runtime suites passed 15 tests. `cargo test --workspace --offline` passed 111 tests (5 unit, 43 protocol, 63 Swarm). `git diff --check` passed. Evidence: `daemon/tests/swarm_scheduler.rs`, `daemon/src/swarm/scheduler.rs`, `daemon/src/swarm/admission.rs` and `daemon/src/swarm/schema.rs`.

Remaining: the path is fixture-only and takes an injected target; it does not consume Auto Mode's live route/snapshot, launch the chosen worker, or run a director. The fixture has a logical director slot in admission accounting but no supervised director process. It does not demonstrate cross-category handoffs, per-job requirements, different account eligibility, or independent scope/budget ledgers under a live dispatcher. SWARM-36 stays unchecked.

At implementation revision `3948c47`, a second replay covers a blocked first
job. Two categories each have a ready job requiring an exclusive `db:shared`
claim and a separate independent job. The first category acquires the shared
claim; after daemon restart, the next scheduler request encounters the second
category's conflicting `j000` and admits its `j001` instead of skipping that
category's fair turn. Replaying the request returns the same attempt. The
conflicting job remains ready and consumes no attempt. The new test failed
before the fix because the scheduler admitted the first category's next job.

Verification: `cargo test --offline -p overseerd --test swarm_scheduler`
passed 2 tests; `cargo test --offline -p overseerd --test swarm_admission`
passed 32 tests; `git diff --check` passed. This verifies the fixture scheduler's
same-category scan and durable fair cursor, not a live Auto route, worker launch
or autonomous director. Status remains partial.

At `6910688`, the same scheduler consumes durable per-job capability requirements.
The two-account fixture admits a source job on a code-only target and a browser
job on a browser-capable target without the caller restating either job's
requirement. This closes the fixture's per-job-capability gap; the target
snapshot is still injected and the scheduler does not autonomously ask Auto
for the next route. The four-test scheduler suite and full offline Rust
workspace suite passed. SWARM-36 remains partial.

Two-repository supervised dispatch follow-up (2026-09-27):
`daemon/tests/swarm_dispatch.rs::dispatch_skips_category_outside_requested_repository_before_reserving_attempt`
creates a 100-job category in repository A and a two-job category in repository B,
then starts one supervised director for each under a four-agent ceiling. The
first dispatch asks for B's approved checkout even though A sorts first. Before
the fix, the scheduler reserved A's job and worker launch failed with
`repository is outside the approved Swarm scope`. The dispatcher now resolves
the requested source revision once and filters category scopes before
admission. An unapproved third repository consumes no attempt; B launches
first, A launches second, a replay returns B's existing worker, and a fifth
ordinary process is refused. Both directors and workers are stopped and their
process exits confirmed by the fixture. Worker launch still rechecks source
authority after selection.

Verification: the focused regression failed before the fix and passed after
it. The affected dispatch, scheduler, admission and runtime suites passed
(8 + 4 + 35 + 24 tests); after tightening the filter's error handling, the
final dispatch and scheduler suites passed again (8 + 4 tests). This adds
real supervised director/worker evidence with separate repository scopes.
The dispatcher still takes an injected route, quota snapshot and requested
checkout rather than obtaining those from Auto; cross-category handoffs and
the shared live account authority remain unverified. SWARM-36 stays partial.

## Verified at fixture scope (2026-09-28)

| Clause | Test |
| --- | --- |
| Two eligible categories with backlogs of 100 and 2 on shared accounts: under default round-robin, each gets one of the next two worker admissions | `round_robin_admission_is_durable_and_replay_safe_across_unequal_categories` (`swarm_scheduler.rs`: A, B, A, then B after a restart; replay-safe per request) |
| …when its director slot is reserved | `dispatch_skips_category_outside_requested_repository_before_reserving_attempt` (`swarm_dispatch.rs`: both categories' supervised directors hold their slots; B launches first, A second, a fifth process is refused at a four-agent ceiling); `active_category_director_uses_its_reserved_app_slot` |
| Separate scopes, ledgers and budgets | the dispatch replay (separate approved repositories); `categories_share_accounts_but_not_scopes_ledgers_budgets_or_evidence` (`swarm_scheduler.rs`, new; passed once its setup reported before deciding): each category freezes its own allocation from the same pool, and no message of one category's attempts appears in the other's ledger; `shared_pool_reservation_blocks_stale_capacity_across_categories` (shared account, separate allocations) |
| Cross-category resource conflicts and handoffs stay explicit | the new test: A's exclusive write claim holds B's writer as `resource_conflict`; B's plan cannot depend on A's job (`unknown dependency`); B's worker cannot read A's accepted evidence, under either run; `shared_reads_and_exclusive_claims_span_categories` |

Rerun serially on 2026-09-28: `swarm_scheduler` 6, `swarm_dispatch` 8.

Boundary: an explicit cross-category handoff operation does not exist yet; the clause requires that none happens implicitly, which is what is shown.
