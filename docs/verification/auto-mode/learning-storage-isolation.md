# Local learning storage boundary

The Auto learning tables currently share the execution SQLite file. Exact row caps and retention limit record counts, but a database-wide page cap would also stop event, run, and workspace writes. AUTO-AC-39 needs a learning-specific byte limit, and AUTO-AC-41 requires telemetry pressure to leave durable execution intact.

## Feasibility finding (2026-09-26)

A local SQLite probe used a WAL execution database, an attached learning database with `PRAGMA learning.max_page_count` set two pages above its initial size, an execution insert, and a learning savepoint. The second 3 KB learning-row insertion returned `SQLITE_FULL`; `ROLLBACK TO learning_write` then failed with `no such savepoint`. SQLite had aborted the shared transaction. An attached database on the execution connection cannot provide the required failure isolation merely by wrapping telemetry writes in a savepoint.

The same probe with **two connections** filled a page-capped learning database and rolled back its transaction while the execution connection committed both event rows. Its learning table retained zero partial rows. This is a system-SQLite feasibility probe, not bundled-rusqlite or daemon acceptance evidence.

A second local system-SQLite probe set a 16-page (65,536-byte) limit and committed 100 small updates to one learning row. With WAL, the open database plus sidecars reached 457,136 bytes (including a 420,272-byte WAL); with DELETE journaling, the observed peak was 8,192 bytes. The page limit alone therefore does not bound total WAL storage. This small probe does not prove a rollback journal has a hard total-size bound for arbitrary large transactions.

## Implementation boundary

Use one execution connection and a separate local learning connection/file owned by the same daemon. The learning connection alone gets a fixed page-count cap, set from a documented byte limit and its page size. Prefer a separate rollback-journal connection over WAL until worst-case journal and temporary-file bounds are measured; cap serialized rows and transaction size independently. A full learning file must cause one learning transaction to roll back, set `learning_paused`, and leave the execution event and selected work unit untouched. Storage recovery can resume recording new work; it must not replay an old model turn or regenerate cleared samples.

Move detailed measurements, daily aggregates, and thread-credit observations to the learning connection. Keep active run/account-generation evidence and quota observations in the execution connection until their safety and retention lifetimes are specified separately. Bound every serialized learning record and retained row count in addition to the page cap. Use a local file adjacent to the execution database, with owner-only permissions. No telemetry service, network call, inference call, or provider probe is introduced.

Migration must retain existing learning rows. Copy legacy rows into the learning database with idempotent keys and commit **there first**; only then mark migration complete in the execution database and clear legacy rows. If interrupted before the marker, retrying the copy must not double aggregates. Clearing learning history must leave the migration marker in place so old execution events cannot recreate deleted learning. Existing `:memory:` tests need a separate in-memory learning connection.

Before promoting AC-39 or AC-41, exercise the bundled SQLite build: page-cap pressure during a selected child, aggregate failure and recovery, migration interruption, 30/90-day expiry, exact row caps, explicit export and clear, active-child ownership, restart, and a full execution-database failure that still stops safely. Measure write/maintenance latency at the row and page cap. A SQLite page cap bounds the learning database file; journal and temporary-file behavior must be inspected and documented separately.
