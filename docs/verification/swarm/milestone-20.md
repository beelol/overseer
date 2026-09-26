# Milestone 20 — current-commit combined verification

Revisions: `089df35` (combined gate), `12ef1b6` (fixture ordering). This is partial SWARM-18/19/35 evidence; no RFC acceptance box is checked.

Two local patch fixtures each pass a checker in isolation. Both are accepted, their worker exits are confirmed, and they integrate into one Overseer-owned worktree. The checker passes after the first integration commit, then fails after the second commit. The earlier pass is tied to its commit and cannot satisfy final completion after the tree changes. A separate passed-check fixture proves that completion is possible for the current commit and that changing the checker invalidates its prior verdict.

`swarm.verify` is behind the existing fixture-only API flag. The executable comes from the daemon's test environment, not a worker message. Verification runs outside the SQLite lock, has a ten-second process-group timeout and bounded output, and records the verdict with the run revision, integration commit and checker digest. A slow checker leaves Stop responsive; Stop makes its result `interrupted`. Concurrent checks for one run are held. A verifier attempt left `running` by daemon death remains an explicit reconciliation gap and blocks new attempts.

The focused missing-method test was red before implementation. Eleven integration fixtures passed after implementation. An exact-revision full-suite run exposed a worker-restart fixture that assumed `active` before the first successful reachability probe; the assertion now follows that probe without changing product liveness behavior. The focused worker fixture and subsequent `cargo test --workspace --offline -q` passed 171 Rust tests; `git diff --check` passed. No live provider, paid account, user checkout or normal launch path was used.

Remaining: versioned S3 migration and its full behavior tests; normal verifier authority/sandboxing; a recovered orphaned verifier; director-led patch conflict repair and complete scenario trace. No fixture result is presented as live support.
