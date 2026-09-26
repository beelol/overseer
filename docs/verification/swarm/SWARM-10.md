# SWARM-10 — allocation and reservation settlement

Status: partial. Revision: `14c62f6`. Fixture: one admitted supervised local `/bin/sleep` worker with a durable quota reservation.

Input: admit the worker, request Stop, simulate an unreachable first control socket, restart the daemon, and wait for the linked worker's confirmed exit. Read the reservation before and after exit.

Expected: a confirmed process exit does not imply known token consumption. The reservation must stop being classified as an active process but continue to bind capacity until the shared account-usage authority reconciles it.

Actual: before confirmed exit, the reservation is `active`; afterward it is `uncertain`. Admission already counts both `active` and `uncertain` reservations. The focused regression was red before `14c62f6` (`active` after exit), then passed after the change. Replay with `cargo test --offline -p overseerd --test swarm_runtime stop_retries_an_initially_unreachable_worker_after_daemon_restart -- --nocapture`. `cargo test --workspace --offline -q` passed 160 tests and `git diff --check` passed.

Evidence: `daemon/tests/swarm_runtime.rs` (`stop_retries_an_initially_unreachable_worker_after_daemon_restart`), `daemon/src/swarm/artifacts.rs` (`confirm_exit`), `daemon/src/swarm/admission.rs`.

Remaining: no authoritative native usage measurement or reconciliation transaction exists, so the uncertain hold cannot yet be released or charged to actual use. Finishing-work draw and live harness/account qualification are also unverified. This criterion remains unchecked.
