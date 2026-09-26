# SWARM-57 — destination artifact revocation

Status: partial. Implementation revision: `51f072f`.

Input: a local backend fixture with an accepted contract artifact, an admitted dependent worker on `account-a`, a second dependent job, and an unrelated job. The director revokes `account-a` access to the artifact while the first dependent worker runs.

Expected: future context delivery to the destination fails, the active dependent worker is interrupted, later dependent admission is blocked, and unrelated work can still be admitted. Repeating the revocation is idempotent; a stale director generation cannot revoke access.

Actual: before implementation, the focused test failed because `swarm.context.revoke` did not exist. At `51f072f`, revocation is stored per run, artifact and destination; context retrieval is denied, the linked local worker exits without completing, the later dependent admission reports `artifact_permission_revoked`, and the unrelated job is admitted. Duplicate and stale-generation assertions pass. The full offline Rust suite passed with 157 tests, including this fixture.

Reproduce: `cargo test --offline -p overseerd --test swarm_context revoked_artifact_stops_dependent_delivery_and_worker_but_not_unrelated_work -q` and `cargo test --workspace --offline -q`. See `daemon/tests/swarm_context.rs` and `daemon/src/swarm/context.rs`.

Remaining: this is a fixture-only API. It does not prove live cross-target artifact delivery or revocation, account/credential isolation, absence of raw secrets throughout broker/ledger/logs, or denial of peer attempts to bypass director/account restrictions. The S4 incident replay and the Stop/revocation/result/acceptance race are still unverified. Do not check the RFC box yet.
