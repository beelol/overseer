# SWARM-43 — redirect receipt and application

Status: partial. Code revisions: `216f4a1` (daemon deadline), `8efd953` (joined Atlas replay), `c168d78` (revision-2 redirect). Policy: approved Swarm RFC's 30-second applied-ack deadline. Fixtures: `daemon/tests/swarm_broker.rs`, `daemon/tests/swarm_runtime.rs`, and `daemon/tests/swarm_atlas.rs`. No live account or harness path was used.

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
