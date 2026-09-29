# SWARM-50 — persistence failures

Status: verified at fixture scope on 2026-09-28 (`claude/auto-swarm`); see the last section. First revision: `042b3e5`.

Input: real daemon and temporary SQLite database. A trigger fails the result transaction at its job-state update; another trigger fails reservation insertion during admission. After the reservation fault clears, the daemon restarts before the identical request is retried.

Expected: neither failed operation is acknowledged, and no partial message, attempt, allocation, reservation or admission survives. Retry commits once.

Actual: both injected failures return errors. The result remains absent with the job still reserved. The failed admission leaves zero related rows and a ready job. After clearing the faults, each identical request succeeds once and subsequent replay returns the existing identity. The workspace suite passed with 67 tests at revision `042b3e5`.

Evidence: `daemon/tests/swarm_faults.rs`, `docs/verification/swarm/milestone-9.md`.

Follow-up: `full_storage_blocks_new_admissions_until_write_capacity_recovers` uses the fixture-only SQLite page ceiling to produce a real `SQLITE_FULL` during a large result write. The report receives no acknowledgement. The daemon exposes `swarm_storage: blocked` in `state`, and new admissions and scripted launches fail closed. A write probe cannot clear the block while the limit remains; after restoring capacity, `swarm.storage.recover` clears it, the identical result commits once, its replay is recognized, and the waiting category admits once. `swarm.storage.limit_pages` is available only in fixture mode. Full and I/O errors from Swarm requests trigger this in-memory block; ordinary constraint failures do not.

Follow-up revision `f81dcb2`: the same test reapplies the page ceiling on daemon restart, as a fixture for storage that remains full across processes. Startup's write-capacity probe marks Swarm blocked before new admissions; clearing the limit and rerunning the probe resumes work. `failed_discovery_write_replays_once_after_restart` injects an INSERT fault on a discovery envelope: no message or acknowledgement survives, and the identical message commits once after restart. `failed_dispatch_intent_cannot_admit_or_launch_and_replays_once` injects a dispatch-intent INSERT fault: intent, admission, attempt and worker launch counts all remain zero, and an identical retry after restart launches one worker and retains one attempt. The focused five-test fault suite and the six-worker-dispatch and eight-director-process regression suites passed at this revision.

Replay: `cargo test --offline -p overseerd --test swarm_faults -- --test-threads=1`; `cargo test --offline -p overseerd --test swarm_dispatch -- --test-threads=1`; `cargo test --offline -p overseerd --test swarm_director_process -- --test-threads=1`.

Related worker-launch evidence: `SWARM-22.md` records durable reserved/linked/spawn-requested launch phases and conservative orphan reconciliation after daemon restart. The full-storage fixture does not yet cover a process already being spawned when storage fills.

Remaining: storage failure during external process creation and a real filesystem-full restart remain unverified. The blocked flag is in memory but startup probes capacity anew; if database migration itself cannot complete, the daemon cannot start. This fixture does not prove sender-held replay state inside qualified live harnesses, nor a complete S5 Atlas ordered trace. Live provider paths are still unqualified. This criterion remains partial.

Atlas follow-up at `deb53a6`: `atlas_s5_full_storage_replays_unacknowledged_evidence_after_recovery` probes the real Atlas PostgreSQL J2 mutation before capping SQLite at its current page count. The 28 KB reproduction artifact gets a failed write, no receipt and zero stored artifact rows; J4 admission returns a storage block. The fixture reapplies the page limit across daemon restart, confirms admission and recovery remain blocked, then restores capacity. Retrying the exact artifact and result IDs commits each once and returns duplicate receipts on a second replay. J2's recovered result can be accepted; J4 admits after J2 exit. The disposable PostgreSQL 16/Node.js 24 runner passes all 13 Atlas tests. The earlier remaining limits still apply: scripted sender-held state is not a qualified live harness, and this does not simulate an actual filesystem-full daemon migration or external process creation under full storage.

## Verified at fixture scope (2026-09-28)

The clause names two fault kinds (storage-full and a transient write failure) at three points (dispatch intent, discovery, result). Before this session storage-full was covered only for the result; two tests now cover the other two points, so each of the six cells has its own test in `daemon/tests/swarm_faults.rs`:

| Point | Transient write failure | Storage-full (real `SQLITE_FULL`) |
| --- | --- | --- |
| Dispatch intent | `failed_dispatch_intent_cannot_admit_or_launch_and_replays_once` | `full_storage_during_dispatch_intent_launches_nothing_until_recovery` (new) |
| Discovery | `failed_discovery_write_replays_once_after_restart` | `full_storage_during_discovery_withholds_its_receipt_and_replays_once` (new) |
| Result | `failed_result_write_is_not_acknowledged_or_partially_submitted` | `full_storage_blocks_new_admissions_until_write_capacity_recovers` |

The rest of the clause:

- No durable acknowledgement precedes commit: every faulted call returns an error with no receipt, and no partial row survives (message, intent, admission, attempt, launch, reservation, allocation).
- No new work launches from an uncommitted reservation: `failed_reservation_rolls_back_attempt_and_retry_succeeds_once`, and the new dispatch test shows no intent, attempt, launch or worker process after a full-storage intent, and a second dispatch refused `storage` before it writes.
- Blocked state is surfaced: `swarm.storage.status` and `state.daemon.swarm_storage` read `blocked`; admission and dispatch fail closed until `swarm.storage.recover` succeeds after capacity returns (also across a restart in the result test).
- Recovery loses no acknowledged event and duplicates no work: an event acknowledged before the disk filled is still there afterwards; each faulted sender retry commits once and its replay returns a duplicate receipt; one attempt and one worker per dispatch request.

The two new tests passed on first run: they add coverage for existing behaviour, not a fix. Run: `cargo test --offline -p overseerd --test swarm_faults -- --test-threads=1` (7 passed). The joined Atlas replay `atlas_s5_full_storage_replays_unacknowledged_evidence_after_recovery` also passed again against disposable PostgreSQL 16.

Boundary: SQLite's page ceiling is the fixture for a full disk; an actual filesystem-full daemon migration and storage filling while an external process is being spawned are not exercised, and sender-held replay inside a real harness belongs to SWARM-25.
