# SWARM-32 — review pressure, bounded waves and outage recovery

Status: verified at fixture scope on 2026-09-28 (`claude/auto-swarm`). Support level: daemon fixture admission with synthetic exact quota snapshots; no provider account, model director or VS Code.

Criterion: with fixture review capacity 8, a wave size of 4 and 40 ready jobs, stop new admissions once the review backlog reaches 8; accept already-in-flight results durably and drain them before resuming. Lower capacity during an outage, recover it, and verify bounded waves and no repeated replanning on unchanged events.

Earlier evidence (before this record existed, see `coverage.json` and [milestone 7](milestone-7.md)): `review_backlog_holds_admissions_until_it_drains_below_four` (10 jobs) and `already_admitted_results_can_overflow_review_threshold_without_loss` held at 8 and resumed below 4; `default_worker_ceiling_and_four_per_wave_are_admission_bounds` bounded waves. No test joined them on 40 jobs with an outage.

Test: `forty_jobs_hold_at_eight_reviews_then_recover_from_an_outage_in_bounded_waves` (`daemon/tests/swarm_admission.rs`, new; passed once its post-recovery admissions carried the latest observed snapshot, as admission requires):

| Clause | What it shows |
| --- | --- |
| 40 ready jobs, waves of 4 | three waves of four five seconds apart; a fifth admission in a wave is `growth_wave_full` |
| Stop new admissions at 8 pending reviews | after eight results, the next admission is `review_backlog` |
| Already-in-flight results are accepted durably | the four workers already running still submit: twelve jobs are `submitted` at once; admission stays held |
| Drain before resuming | once review brings the backlog below four, admission resumes in a wave of four and no more |
| Lower capacity during an outage | an observation with no remaining allowance blocks the run; a new admission is refused |
| No repeated replanning on unchanged events | three more identical observations are `blocked` with `woken: false`, and the director inbox gains no event |
| Recover, bounded waves again | a recovered observation wakes the director once (a repeat does not); admission resumes four at a time |

Run: `cargo test --offline -p overseerd --test swarm_admission -- --test-threads=1` (40 passed, 2026-09-28).

Boundary: review "capacity" is the daemon's count of submitted results; how fast a live director reviews them, and a live account outage feed, are not part of this fixture proof.
