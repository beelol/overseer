# SWARM-56 — review pressure and 32-worker scale

Status: partial. Revision: `5004e1e`. Support level: local scripted generic workers with fixture quota snapshots; no paid provider accounts.

Input: existing admission fixtures create eight submitted results awaiting review, attempt another admission, review five, and retry below four pending results. They also admit 32 logical jobs with explicit `max_workers=32` and `max_executing=33`, using fixture clock advances for each 5-second growth wave. The new runtime fixture creates 33 jobs, admits and launches 32 separate `/bin/sleep 60` workers through Overseer's normal local supervisor, and tests the 33rd admission before stopping the run.

Expected: admissions hold at eight pending reviews and resume only below four, subject to growth-wave limits. Explicit ceilings permit 32 concurrent worker processes; the default eight is not an architectural cap. A 33rd worker is held. Stop interrupts the 32 live workers without treating them as completed results.

Observed: admission tests hold with `review_backlog`, resume at three pending, and enforce four-per-wave growth. The runtime test observes 32 unique linked Overseer runs with `running` status and 32 live child PIDs recorded by their supervisors. The 33rd request returns `worker_limit`; the run remains active until Stop and all 32 workers end unsuccessfully. The focused test and full offline Rust suite pass (132 tests: 9 unit, 48 protocol, 75 Swarm). No provider account or model inference was used.

Commands: `cargo test --offline -p overseerd --test swarm_runtime explicit_ceiling_runs_thirty_two_supervised_workers -- --nocapture`; `cargo test --workspace --offline -q` (also executes the existing admission fixtures).

Evidence: `daemon/tests/swarm_runtime.rs` (`explicit_ceiling_runs_thirty_two_supervised_workers`), `daemon/tests/swarm_admission.rs` (`review_backlog_holds_admissions_until_it_drains_below_four`, `explicit_ceiling_admits_thirty_two_fixture_workers_without_hidden_eight_cap`), and `docs/verification/swarm/milestone-7.md`.

Remaining: in-flight results crossing the review threshold, live account usage and machine limits, director/reviewer behavior under 32 actual agents, and versioned 32-worker qualification are unverified. This criterion stays unchecked.
