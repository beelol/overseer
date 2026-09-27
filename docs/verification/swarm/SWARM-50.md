# SWARM-50 — persistence failures

Status: partial. Revision: `042b3e5`.

Input: real daemon and temporary SQLite database. A trigger fails the result transaction at its job-state update; another trigger fails reservation insertion during admission. After the reservation fault clears, the daemon restarts before the identical request is retried.

Expected: neither failed operation is acknowledged, and no partial message, attempt, allocation, reservation or admission survives. Retry commits once.

Actual: both injected failures return errors. The result remains absent with the job still reserved. The failed admission leaves zero related rows and a ready job. After clearing the faults, each identical request succeeds once and subsequent replay returns the existing identity. The workspace suite passed with 67 tests at revision `042b3e5`.

Evidence: `daemon/tests/swarm_faults.rs`, `docs/verification/swarm/milestone-9.md`.

Follow-up: `full_storage_blocks_new_admissions_until_write_capacity_recovers` uses the fixture-only SQLite page ceiling to produce a real `SQLITE_FULL` during a large result write. The report receives no acknowledgement. The daemon exposes `swarm_storage: blocked` in `state`, and new admissions and scripted launches fail closed. A write probe cannot clear the block while the limit remains; after restoring capacity, `swarm.storage.recover` clears it, the identical result commits once, its replay is recognized, and the waiting category admits once. `swarm.storage.limit_pages` is available only in fixture mode. Full and I/O errors from Swarm requests trigger this in-memory block; ordinary constraint failures do not.

Related worker-launch evidence: `SWARM-22.md` records durable reserved/linked/spawn-requested launch phases and conservative orphan reconciliation after daemon restart. The full-storage fixture does not yet cover a process already being spawned when storage fills.

Remaining: transient write failure during discovery and dispatch intent needs explicit replay, as do storage failure during external process creation and recovery after daemon restart. The blocked flag is in memory, so restart must re-probe storage before any live Swarm admission. Live provider paths are still unqualified. This criterion remains partial.
