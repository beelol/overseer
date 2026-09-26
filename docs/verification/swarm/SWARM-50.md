# SWARM-50 — persistence failures

Status: partial. Revision: `042b3e5`.

Input: real daemon and temporary SQLite database. A trigger fails the result transaction at its job-state update; another trigger fails reservation insertion during admission. After the reservation fault clears, the daemon restarts before the identical request is retried.

Expected: neither failed operation is acknowledged, and no partial message, attempt, allocation, reservation or admission survives. Retry commits once.

Actual: both injected failures return errors. The result remains absent with the job still reserved. The failed admission leaves zero related rows and a ready job. After clearing the faults, each identical request succeeds once and subsequent replay returns the existing identity. The workspace suite passed with 67 tests at revision `042b3e5`.

Evidence: `daemon/tests/swarm_faults.rs`, `docs/verification/swarm/milestone-9.md`.

Remaining: a full-storage fault, durable launch intent and external process reconciliation, plus a visible degraded-storage state and recovery path, are unverified. These tests do not prove that a live process cannot be duplicated after a lost launch acknowledgement.
