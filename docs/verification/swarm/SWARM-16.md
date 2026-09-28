# SWARM-16 — checkpoint recovery after worker failure

Status: verified at fixture scope on 2026-09-28 (`claude/auto-swarm`); see the last section. Reproduce with `cargo test --offline -p overseerd --test swarm_checkpoint -q` and `./fixtures/swarm/dispatch-v1/run-swarm.sh`. See `daemon/tests/swarm_checkpoint.rs` and [S4](S4.md).

A failed SQL worker stores a checkpoint artifact containing a sanitized trace, source reference and open question. Reconciliation confirms the worker failed and makes the same logical job ready. The artifact survives daemon restart. A replacement on a different selected account is blocked until the director grants that destination access; an unselected account and stale director generation cannot receive a grant. The replacement's worker brief lists the checkpoint and its context API returns the content. A second grant request is idempotent. Revocation during replacement stops delivery, interrupts the worker and blocks the job. The joined S4 Go/PostgreSQL replay uses this handoff on the second attempt while other scoped jobs continue.

This transfers an artifact, not the original native session. The checkpoint is authored by the fixture worker, and the replay does not recover already accepted artifacts from a failed worker automatically. Side-effect reconciliation is exercised separately in `daemon/tests/swarm_effects.rs` and the S2 joined replay, not in this checkpoint handoff. Live provider/account outage routing and harness session recovery remain unverified. Keep the RFC box open.

Gate L's generic successor-run recovery is excluded from linked Swarm runs
because it cannot transfer the job's attempt reservation and checkpoint grant.
A failed Swarm process remains a failed attempt for director reconciliation;
the checkpoint path above is the permitted recovery path until same-job
Continuity routing is built. The linked-run refusal is exercised by
`daemon/tests/swarm_runtime.rs` and `daemon/tests/swarm_director_process.rs`.

At `82165fcb`, the failed-attempt handoff also carries a separate durable
trace artifact when the director explicitly grants that artifact to the same
logical job on selected account B. A grant to unselected C is denied. B's
replacement brief lists the checkpoint and trace with their hashes, and its
context read returns the original trace. Revoking the trace marks the job
blocked; an injected crash after the durable revocation but before interrupt
still interrupts B after daemon restart. The focused test first failed because
`swarm.context.grant` allowed only a finished checkpoint, then passed after the
same-job artifact rule and revocation replay were changed. This does **not**
accept the failed attempt's trace as a finding for dependent jobs.

Commands: `cargo test -p overseerd --offline --test swarm_checkpoint failed_worker_checkpoint_and_evidence_require_destination_grants_before_replacement -- --nocapture`
(red before, green after); `cargo test -p overseerd --offline --test swarm_checkpoint --test swarm_context -q`
(1 + 5 passed); `cargo test -p overseerd --offline --test swarm_admission --test swarm_runtime -q`
(39 + 24 passed); `git diff --check`; `scripts/check-links` (697 links, zero
broken). `cargo fmt --check` still reports repository-wide pre-existing style
differences, including files outside this change, and was not applied wholesale.

Remaining: the fixture worker authored its checkpoint before failure. There is
no automatic checkpoint if it dies first, no native-session portability, no
live account-outage handoff, and no joined proof that an accepted artifact and
an uncertain external effect are both carried and reconciled on the new route.
SWARM-16 remains partial.

## Verified at fixture scope (2026-09-28)

| Clause | Test |
| --- | --- |
| After a worker fails, its accepted artifacts are recovered through a checkpoint on another eligible target | `failed_worker_checkpoint_and_evidence_require_destination_grants_before_replacement` (`swarm_checkpoint.rs`): the first attempt on target A writes a checkpoint and evidence, then its process fails; across a daemon restart, attempt 2 on selected target B is held until the director grants those artifacts to B, then reads them; an unselected destination cannot; joined: `shipment_incident_survives_sql_account_loss_with_two_attempts` (Dispatch Go/PostgreSQL S4: L2 fails on account A, its checkpoint reaches the attempt-2 replacement on account B while L1/L3 continue; passed again against PostgreSQL 16) |
| Without claiming native session portability | the replacement is a new launch carrying the checkpoint and granted artifacts in its brief; no session or transcript of the failed attempt is transferred or resumed (the brief holds references, see [SWARM-34](SWARM-34.md)) |
| An uncertain external side effect blocks retry until reconciled | `failed_worker_with_unknown_effect_cannot_route_to_replacement` (`swarm_routing.rs`); `lost_side_effect_ack_blocks_retry_until_outcome_is_reconciled` (`swarm_effects.rs`); on the route-selection path `one_job_falls_back_once_then_stops_and_an_uncertain_effect_pauses` (`side_effect_unreconciled`, no route decision) |

Rerun serially on 2026-09-28: `swarm_checkpoint` 1, `swarm_routing` 2, `swarm_effects` 4, `swarm_native` (fallback test), and the S4 replacement replay.

Boundary: the checkpoint is written by the worker (or granted by the director); assembling one automatically from a worker that died without writing it is not claimed.
