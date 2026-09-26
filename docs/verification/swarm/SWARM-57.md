# SWARM-57 — destination artifact revocation

Status: partial. Implementation revisions: `51f072f`, `9148e84`.

Input: a local backend fixture with an accepted contract artifact, an admitted dependent worker on `account-a`, a second dependent job, and an unrelated job. The director revokes `account-a` access to the artifact while the first dependent worker runs.

Expected: future context delivery to the destination fails, the active dependent worker is interrupted, later dependent admission is blocked, and unrelated work can still be admitted. Repeating the revocation is idempotent; a stale director generation cannot revoke access.

Actual: before implementation, the focused test failed because `swarm.context.revoke` did not exist. At `51f072f`, revocation is stored per run, artifact and destination; context retrieval is denied, the linked local worker exits without completing, the later dependent admission reports `artifact_permission_revoked`, and the unrelated job is admitted. Duplicate and stale-generation assertions pass. The full offline Rust suite passed with 157 tests, including this fixture.

At `9148e84`, an admitted dependent on account B cannot read account A's accepted artifact until the director grants that artifact to B. The grant survives daemon restart and permits a bounded chunk and reference in B's worker brief. It rejects stale directors, disallowed destinations, and unreviewed artifacts; revocation then denies B while A retains access. A result submitted after source review also makes dependent context unavailable. The grant/revocation fixture and full 157-test Rust suite pass.

At `f9b3018`, a fixture fault withholds the external interrupt after the revocation transaction commits, then kills and restarts the daemon. The periodic reconciler finds the same active dependent worker through the durable revocation and launch records and interrupts it. A second active worker with no dependency on that artifact remains running; Stop later interrupts that sibling. The crash fixture failed before the retry path existed. The full offline Rust suite passed 158 tests.

Reproduce: `cargo test --offline -p overseerd --test swarm_context revoked_artifact_stops_dependent_delivery_and_worker_but_not_unrelated_work -q`, `cargo test --offline -p overseerd --test swarm_context revoked_artifact_interrupt_retries_after_daemon_crash -- --nocapture`, and `cargo test --workspace --offline -q`. See `daemon/tests/swarm_context.rs` and `daemon/src/swarm/context.rs`.

The S4-shaped checkpoint fixture in `daemon/tests/swarm_checkpoint.rs` now revokes account B's grant while its replacement worker runs. Context retrieval fails, the worker is interrupted, and L2 is blocked. The joined S4 replay grants B access to a sanitized incident bundle before it resumes, though it does not revoke that grant in the same joined run.

Remaining: this is a fixture-only API. It does not prove live cross-target artifact delivery or revocation, account/credential isolation, absence of raw secrets throughout broker/ledger/logs, or denial of peer attempts to bypass director/account restrictions. The combined Stop/revocation/result/acceptance race and simultaneous native races are still unverified. Do not check the RFC box yet.
