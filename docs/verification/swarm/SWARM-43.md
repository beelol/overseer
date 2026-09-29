# SWARM-43 — redirect receipt and application

Status: verified at fixture scope on 2026-09-28 (`claude/auto-swarm`); see the last section. Code revisions: `216f4a1` (daemon deadline), `8efd953` (joined Atlas replay), `c168d78` (revision-2 redirect). Policy: approved Swarm RFC's 30-second applied-ack deadline. Fixtures: `daemon/tests/swarm_broker.rs`, `daemon/tests/swarm_runtime.rs`, and `daemon/tests/swarm_atlas.rs`. No live account or harness path was used.

Inputs: a planned Atlas-shaped J4 attachment audit, with one registered worker attempt and a dependent review job. A director sends `redirect` with stable message ID `j4-redirect`. The worker reports `delivered` while a simulated long-running command remains active. A second fixture links the attempt to a supervised `/bin/sleep 30` process, kills the daemon, ages the delivered receipt past 30 seconds, and restarts it.

Expected: `applied` cannot precede `delivered`; duplicate delivered receipts do not extend the deadline; before 30 seconds the job remains reserved; at 30 seconds the daemon holds J4 and its dependent work, records one checkpoint request, and interrupts a linked still-active worker. An already applied redirect never times out. A daemon restart does not lose the deadline or generate duplicate checkpoint requests.

Actual: the broker fixture first failed because a queued directive could be acknowledged as applied. After the change, that direct acknowledgement is rejected, delivered replay retains its original timestamp, the 29,999 ms check leaves J4 reserved, and the 30,000 ms check marks J4 `cancel_requested` with `redirect_ack_timeout`. The dependent remains planned. One checkpoint request is queued; a repeated due check reports zero new timeouts. The linked-worker fixture confirms an interrupt marker appears after daemon restart while `/bin/sleep 30` is still active. An applied redirect is excluded from the timeout. Existing unrelated stop reasons are not overwritten.

Reproduce with `cargo test --offline -p overseerd --test swarm_broker --test swarm_runtime --test swarm_control --test swarm_director --test swarm_scenarios -- --test-threads=1`. The affected run passed 49 tests with one Node.js 24 scenario test ignored by its existing opt-in gate. After the final stop-reason guard, the focused `directive_delivery_and_application_are_distinct` and `unapplied_redirect_times_out_and_holds_dependent_work` tests both passed. Local Unix-socket fixtures required the test process to run outside the filesystem sandbox. `git diff --check` passed before commit.

Joined Atlas replay at `8efd953`, refined at `c168d78`: a scripted J4 worker starts the versioned PostgreSQL attachment probe and keeps polling the daemon broker while that command is held. A marker written after the backend request records fixture version 1, J4, and the seeded foreign attachment status 200. The director revises J4's assignment to plan revision 2, which emits a redirect during the held command. The worker acknowledges `delivered`, not `applied`; the test advances the persisted receipt timestamp beyond 30 seconds. The daemon records `redirect_ack_timeout`, leaves a dependent review job planned, queues a checkpoint request and interrupts only the linked worker. The redirect remains delivered, with zero J4 result messages and zero acceptance decisions, so a partial probe cannot become a final finding. The completion attempt is rejected. Reproduce with `./fixtures/swarm/atlas-v1/run-swarm.sh` (Node.js 24 and Docker; disposable PostgreSQL 16): all seven joined Atlas tests passed after each change. `node --check` passed for the two changed fixture scripts. The pre-implementation run passed the previous six tests and failed the new J4 test because its worker never entered the probe. The 30-second boundary is injected deterministically by aging a committed receipt; this is not a 30-second wall-clock wait.

Remaining: The complete six-job S1 revision-2 assignment and old-revision evidence flow, actual delivery/acknowledgement through a qualified live harness during a tool call, and exclusion of unsupported harness paths remain unverified. The joined J4 path uses a scripted worker with a real local backend probe, not a provider agent. SWARM-43 stays unchecked.

Finished-attempt directive guard (2026-09-27):
`finished_attempt_cannot_receive_a_new_directive_but_can_replay_one` first
failed because a director could queue a new redirect after the worker's exit
was confirmed. The broker now accepts an exact replay of an earlier advisory
with its original receipt, but refuses a new directive for that finished
attempt. The test restarts the daemon between exit and replay and checks that
the worker inbox contains only the original advisory. The focused red run
observed the unwanted `after-exit` queued receipt; the corrected broker,
control, and supervised-director suites passed 19 + 6 + 8 tests with
`cargo test -p overseerd --test swarm_broker --test swarm_director_loop
--test swarm_control --offline -- --test-threads=1`. This closes one local
stale-delivery path, not the live mid-tool-call acknowledgement or complete S1
trace. SWARM-43 remains partial.

Application gates at `9759b031` (2026-09-27): the focused
`acceptance_waits_for_directive_application_before_unlocking_dependents` test
first accepted a result while its redirect was still queued. Acceptance now
holds an attempt with any unapplied director redirect, advisory or retraction.
For each message kind, the test verifies queued and delivered phases block
acceptance, including restart after delivery; application allows acceptance,
and confirmed exit then releases the dependent job. A second regression,
`completion_does_not_hide_a_directive_sent_after_review`, first completed a run
with an advisory still queued after review. Completion now checks directives
for the accepted attempt and refuses that report; the applied variant succeeds.

Validation: `cargo test -p overseerd --test swarm_broker --test swarm_plan
--test swarm_director_loop --test swarm_integration --test swarm_context
--offline -- --test-threads=1` passed 21 + 11 + 8 + 25 + 5 tests, 70 total.
`git diff --check` passed. These tests use the local fixture broker and simulated
application receipts. They do not prove the recipient actually followed a
directive through a qualified provider transport, autonomous correction, or
the complete S1 trace; SWARM-43 remains partial.

## Verified at fixture scope (2026-09-28)

| Clause | Test |
| --- | --- |
| J4's revision-2 redirect delivered while its tool call is active; delivered and applied shown separately | `atlas_s5_redirect_during_long_probe_interrupts_and_holds_review` (J4's real attachment probe is held mid-call; the revision-2 redirect is `delivered`, never `applied`); `directive_delivery_and_application_are_distinct`; `delivered_redirect_interrupts_a_long_running_worker_after_restart` |
| Old-revision output cannot satisfy the new assignment | `atlas_s1_faults_quarantine_stale_and_missing_evidence` (J4's revision-1 evidence is refused as stale after the revision); `superseded_result_is_applied_after_delivery_without_accepting_old_evidence`; `acceptance_waits_for_directive_application_before_unlocking_dependents` |
| After 30 s without an applied acknowledgement: hold dependent work, request interrupt and checkpoint | the Atlas held-redirect replay (dependent review held, one checkpoint request, the worker interrupted, no result accepted); `unapplied_redirect_times_out_and_holds_dependent_work` |
| A harness with no qualified delivery/acknowledgement path is excluded before launch | `a_harness_without_a_swarm_delivery_path_is_excluded_before_launch` (`daemon/tests/swarm_native.rs`, new; passed on first run once the director's account was set apart): an approved Codex profile is excluded in the route decision as `swarm_worker_launch_unsupported` and the job launches on a Claude account with the daemon's Swarm tools; `uncontrolled_native_delegation_blocks_admission_before_reserving_or_launching` (the fixture path refuses a Codex target before any attempt or reservation) |

The new test runs the proposed native path (`swarm.native_director` switched on inside the test only; the default stays off). Rerun on 2026-09-28: `swarm_native` 8 passed serially (one earlier full-file run had `four_way_launch_matrix_auto_manual_by_swarm_on_off` fail once under machine load; it passed alone and in the next full-file run), the Atlas redirect and stale-evidence replays against PostgreSQL 16, and `swarm_runtime delivered_redirect`.

Boundary: whether a real Claude worker applies a redirect in the middle of a live tool call is SWARM-25 material.
