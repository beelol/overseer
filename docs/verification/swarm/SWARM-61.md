# SWARM-61 — Stop ordering around local launch

Status: partial. Latest evidence revision: `dc1c880`.

Input: a fixture-admitted attempt has a durable but unlinked launch intent. Stop commits before the request is replayed. Separately, Stop reaches a linked `/bin/sleep` worker after a daemon restart.

Expected: an unlinked request cannot spawn after Stop; a linked worker is interrupted without treating the interrupt request as confirmed exit.

Actual: the red pending-intent fixture launched after Stop. The fixed path rechecks run and job state before continuing and starts no process. Stop and launch now share a serialization lock so an in-flight launch cannot cross cancellation. The linked worker receives an interrupt through Overseer's existing supervisor and reaches a terminal run state; the attempt is finished only after that state is observed. A rejected attempt exiting after Stop no longer requeues its job, and a superseded attempt cannot requeue work after Stop. The workspace suite passed 83 tests.

At `f9b3018`, a separate fixture commits artifact revocation while withholding the first external interrupt, restarts the daemon, and confirms the same dependent worker reaches `interrupted` through the periodic retry. An unrelated concurrently running worker remains active. The full offline Rust suite passed 158 tests.

At `a91579c`, the `stop_revocation_result_and_acceptance_keep_one_durable_order` fixture executes four orderings of final-result submission, director acceptance, artifact revocation, and Stop. A shared SQLite operation sequence is written in the same transaction as each successful operation. The fixture verifies the recorded order, rejection of acceptance after Stop or revocation, preservation of late results in the director inbox, no launch or completion after Stop, and no duplicate sequence entries on replay. Revocation after acceptance blocks the dependent job. The full offline Rust suite passed 159 tests (`cargo test --workspace --offline -q`); `git diff --check` passed. Global `cargo fmt --all -- --check` fails on extensive pre-existing formatting differences outside this change.

At `59d59ff`, `stop_retries_an_initially_unreachable_worker_after_daemon_restart` simulates a failed first interrupt against a running local worker. Stop persists `cancel_requested` and an unconfirmed signal attempt without claiming exit. After the daemon is killed and restarted, its timer retries the still-active linked worker after a five-second backoff. The worker reaches `interrupted`, the durable signal record changes to requested with at least two attempts, and the logical job remains at one execution attempt. The retry scan is scoped to stopping runs and linked active workers. The full offline Rust suite passed 160 tests (`cargo test --workspace --offline -q`); `git diff --check` passed.

Evidence: `daemon/tests/swarm_runtime.rs`, `daemon/tests/swarm_plan.rs`, `daemon/tests/swarm_context.rs`, `daemon/src/swarm/runtime.rs`, `daemon/src/server.rs`, `daemon/src/swarm/artifacts.rs`, `daemon/src/swarm/schema.rs`.

Remaining: these are scripted local orderings and a simulated failed signal, not simultaneous native-process races or a qualified live harness. Native descendants and explicit user resume/extension are not covered. This criterion remains unchecked.

At `dc1c880`, a user Stop no longer depends on a current director generation or plan revision: an ID-only Stop commits after a plan revision, and a repeated Stop from a stale view is idempotent. The launch lock still serializes the durable Stop transition against admission and launch checks, then is released before the external worker interrupt. A fixture pauses a responsive local worker control shim to show another admission request completes while that interrupt is waiting. The same lock boundary is applied to the periodic deadline timer before its interrupt and liveness I/O. The full serialized offline workspace suite passed 204 tests with 11 ignored. Simultaneous native races, descendants, and explicit user resume remain unqualified; SWARM-61 remains partial.

At `b70433b`, the opt-in Atlas J2 PostgreSQL probe supplies real backend evidence before two scripted last-result/Stop orderings. The durable operation ledger records Stop→result and result→Stop respectively; both retain one artifact and one result, reject acceptance and completion, deduplicate a repeated result ID, and end stopped after confirmed attempt exit. All 12 joined Atlas tests pass with disposable PostgreSQL 16 and Node.js 24. This does not qualify simultaneous native timing, descendants or explicit resume; SWARM-61 remains partial.

Selected-account revocation follow-up (this revision): `identity_revocation_stop_result_and_review_have_one_durable_order` exercises result→accept→revoke→Stop, result→revoke→rejected accept→Stop, and revoke→Stop→late result→rejected accept against one fixture-admitted attempt. The injected identity revocation records `revoke` in the same transaction as targeted cancellation. The test verifies each successful operation appears once and in order in `swarm_operation_order`; a duplicate result and Stop add no entries, the late result remains in the director inbox, and no worker launch or completion is allowed after Stop. A director-wide availability message has nullable job and attempt IDs; the inbox reader now returns those as null instead of failing to read the whole inbox. The full serialized offline workspace suite passed (`cargo test --workspace --offline -q -- --test-threads=1`), and the final explicit null-field assertion passed its focused rerun. This is a scripted local ordering, not a simultaneous native race or live account feed. SWARM-61 remains partial.

Main-branch Gate N alignment: device-scoped request-ID replay after reconnect and a stale queued phone control racing a newer VS Code/CLI control have no joined Swarm gateway fixture yet. The local ordering tests do not establish either behavior.

Start replay follow-up (this revision): `swarm.create` now accepts a bounded
`request_id` with an optional `request_scope` (default `local`). The daemon commits
the category run and its request fingerprint in one SQLite transaction. A retry
returns the same run, even after the daemon restarts; changing the objective or
another request field under the same scoped ID is rejected. Equal request IDs in
different scopes can create different categories. The focused black-box tests in
`daemon/tests/swarm_create.rs` failed before the change because a replay hit the
active-category guard, then passed after it. The full
`cargo test --workspace --offline -- --test-threads=1` suite exited successfully;
declared opt-in backend tests remained ignored. This makes the daemon Start effect
replay-safe; it does not launch the director. A future phone gateway must supply
the authenticated device as `request_scope` rather than trusting a device-provided
scope. Phone authorization, confirmation, other control IDs, and stale queued
phone commands still need joined tests. SWARM-61 remains partial.

Versioned Stop follow-up (`eca5a05`): a remote caller may now send a bounded
`request_id` and `request_scope` with the plan and control revisions it saw.
Pause, Resume, Off, a limit change, deadline extension and Stop advance the
durable control revision; plan changes retain their separate revision. A stale
queued Stop is rejected before cancellation if either expected revision no
longer matches. The local UI/CLI's existing ID-only Stop remains immediate
and does not require a fresh view. A successful remote Stop saves its request
fingerprint and response in the same SQLite transaction as cancellation and
the ordered Stop operation; replay after daemon restart returns one recorded
effect. Reusing its scoped ID with changed input is rejected.

The new `daemon/tests/swarm_stop_replay.rs` fixture first failed because a
run had no control revision. It then passed with Pause→Resume, a newer local
limit, and a plan revision all fencing old phone-style requests. A fresh
request stopped the run, replayed after restart, and left one Stop operation.
The schema migration test passed; the affected control, plan-replay, runtime,
state and Stop-replay suites passed 6 + 3 + 21 + 19 + 1 local tests, with no
paid harness. `git diff --check` passed.

This is a daemon protocol boundary, not a joined phone result. Gate N must
derive `request_scope` from its authenticated device identity and enforce its
Full control/Watch only permissions and confirmations; the current local
protocol accepts a caller-supplied scope. Race replays across an actual phone
reconnect and a simultaneous VS Code/CLI action remain open. SWARM-61 stays
partial.

At `dedd7721`, a director's versioned partial-close request now shares the
durable Stop transaction and request ledger. A fixture proves that a reason
without recorded exhaustion changes nothing, and a later user Stop wins:
the stale partial request cannot attach its report afterward. Replaying a
successful partial close after daemon restart returns the one saved result;
changing its input is rejected. These are local ordered fixture cases, not
a simultaneous phone/VS Code race. SWARM-61 remains partial.
