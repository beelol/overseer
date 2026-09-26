# SWARM-61 — Stop ordering around local launch

Status: partial. Revision: `ae078e6`.

Input: a fixture-admitted attempt has a durable but unlinked launch intent. Stop commits before the request is replayed. Separately, Stop reaches a linked `/bin/sleep` worker after a daemon restart.

Expected: an unlinked request cannot spawn after Stop; a linked worker is interrupted without treating the interrupt request as confirmed exit.

Actual: the red pending-intent fixture launched after Stop. The fixed path rechecks run and job state before continuing and starts no process. Stop and launch now share a serialization lock so an in-flight launch cannot cross cancellation. The linked worker receives an interrupt through Overseer's existing supervisor and reaches a terminal run state; the attempt is finished only after that state is observed. A rejected attempt exiting after Stop no longer requeues its job, and a superseded attempt cannot requeue work after Stop. The workspace suite passed 83 tests.

At `f9b3018`, a separate fixture commits artifact revocation while withholding the first external interrupt, restarts the daemon, and confirms the same dependent worker reaches `interrupted` through the periodic retry. An unrelated concurrently running worker remains active. The full offline Rust suite passed 158 tests.

At `a91579c`, the `stop_revocation_result_and_acceptance_keep_one_durable_order` fixture executes four orderings of final-result submission, director acceptance, artifact revocation, and Stop. A shared SQLite operation sequence is written in the same transaction as each successful operation. The fixture verifies the recorded order, rejection of acceptance after Stop or revocation, preservation of late results in the director inbox, no launch or completion after Stop, and no duplicate sequence entries on replay. Revocation after acceptance blocks the dependent job. The full offline Rust suite passed 159 tests (`cargo test --workspace --offline -q`); `git diff --check` passed. Global `cargo fmt --all -- --check` fails on extensive pre-existing formatting differences outside this change.

At `59d59ff`, `stop_retries_an_initially_unreachable_worker_after_daemon_restart` simulates a failed first interrupt against a running local worker. Stop persists `cancel_requested` and an unconfirmed signal attempt without claiming exit. After the daemon is killed and restarted, its timer retries the still-active linked worker after a five-second backoff. The worker reaches `interrupted`, the durable signal record changes to requested with at least two attempts, and the logical job remains at one execution attempt. The retry scan is scoped to stopping runs and linked active workers. The full offline Rust suite passed 160 tests (`cargo test --workspace --offline -q`); `git diff --check` passed.

Evidence: `daemon/tests/swarm_runtime.rs`, `daemon/tests/swarm_plan.rs`, `daemon/tests/swarm_context.rs`, `daemon/src/swarm/runtime.rs`, `daemon/src/server.rs`, `daemon/src/swarm/artifacts.rs`, `daemon/src/swarm/schema.rs`.

Remaining: these are scripted local orderings and a simulated failed signal, not simultaneous native-process races or a qualified live harness. Native descendants and explicit user resume/extension are not covered. This criterion remains unchecked.
