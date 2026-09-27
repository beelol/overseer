# SWARM-36 — fair admissions across categories

Status: partial. Fixture-only implementation at `09d1a89`; no live director or Auto Mode integration.

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
