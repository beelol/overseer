# SWARM-28 — qualified target and launch binding

Status: partial. Support level: injected target snapshots and fixture-only worker launch; no live Auto Mode producer or model director route.

Revision: `cebde10`, with `origin/main` confirmed at `7bccc3a` on 2026-09-27. The branch already contains that main commit. The focused test was run red before this fix, then green after it; the full suite ran on the same implementation before commit.

Input: one planned backend job allows `fixture-claude`. Its fresh, healthy, quota-qualified target snapshot names harness `claude`. Admission reserves the target and returns an attempt. The caller then tries to launch that attempt as generic `/bin/sleep`. Policy fixtures also submit an unknown harness; a migration fixture starts with an old admission row that has no harness binding.

Expected: the admitted route's harness is durable and cannot be swapped at launch. A mismatched launch creates no worker intent or process and keeps the job reserved. Unknown harnesses are rejected during preview; a historical admission without a trustworthy binding cannot authorize a new launch. Existing generic fixtures continue to work.

Actual: the new runtime test first failed because the wrong generic worker launched. After the fix it passes: the mismatch is rejected before a launch intent is recorded; the reserved job remains. The target harness is carried from preview into admission, stored in SQLite, and checked at launch. Historical rows migrate with a null binding that fails closed. The synthetic Claude stream fixture now explicitly declares its Claude route and still passes.

Commands and results:

- `cargo test -p overseerd --test swarm_runtime admitted_target_harness_cannot_be_changed_at_worker_launch -- --nocapture`: failed before the fix because a worker launched; passed after.
- `cargo test -p overseerd --test swarm_runtime -- --test-threads=1`: 17 passed.
- `cargo test -p overseerd --test swarm_admission -- --test-threads=1`: 29 passed.
- `cargo test -p overseerd --test swarm_policy -- --test-threads=1`: 10 passed.
- `cargo test -p overseerd --bin overseerd swarm::schema::tests -- --test-threads=1`: 7 passed.
- `cargo test --workspace -- --test-threads=1`: passed; the opt-in PostgreSQL/Redis/Node fixture tests remain ignored as declared. Generated TUI evidence was restored after the run.
- `git diff --check`: passed. `cargo fmt --all -- --check` reports widespread formatting differences in pre-existing repository files outside this change; no repository-wide reformat was made.

Evidence: `daemon/tests/swarm_runtime.rs` (`admitted_target_harness_cannot_be_changed_at_worker_launch`, synthetic Claude child replay), `daemon/tests/swarm_policy.rs` (`preview_preserves_admitted_harness_identity_and_rejects_unknown_harness`), and `daemon/src/swarm/schema.rs` (`old_admission_has_no_launchable_harness_binding_after_migration`).

Remaining at `cebde10`: connect Auto's actual versioned target producer and route choice to admission and launch; bind provider endpoint, account, model and effort as well as harness; show two independent jobs taking different qualified allowed routes; test manual single-target restriction and out-of-scope worker proposals through the normal path. SWARM-28 remains unchecked.

## Route-option follow-up — `fd48b3a`

Input: a fresh fixture target names the Claude harness, `system-claude` profile, `sonnet` model and `medium` effort. After admission, the caller attempts to substitute `system-codex`, `opus` and `high`; a second variant removes the saved model before launch. The earlier synthetic Claude child replay exercises a matching route.

Expected: profile, model and effort selected at admission survive to worker creation. Caller changes or incomplete saved non-generic route data fail before a launch intent, while a matching route reaches the harness. Effort may be absent when a harness has no supported effort control.

Actual: the new test first failed because the changed options were ignored and a worker launched. It now rejects the changed and incomplete routes with no launch intent. Preview validates the route fields; admission stores them; launch uses the stored values. The synthetic Claude run records `system-claude` and `sonnet`, and its launch command contains `--model sonnet --effort medium`. A fixture without an effort remains eligible if its profile and model are present. Old rows migrate with null options and cannot start a new non-generic worker.

Commands and results:

- `cargo test -p overseerd --test swarm_runtime admitted_profile_model_and_effort_cannot_be_changed_at_worker_launch -- --nocapture`: failed before the fix because a worker launched; passed after.
- `cargo test -p overseerd --test swarm_runtime -- --test-threads=1`: 18 passed.
- `cargo test -p overseerd --test swarm_admission -- --test-threads=1`: 29 passed.
- `cargo test -p overseerd --test swarm_policy -- --test-threads=1`: 10 passed.
- `cargo test -p overseerd --bin overseerd swarm::schema::tests -- --test-threads=1`: 7 passed.
- `cargo test -p overseerd --test swarm_dispatch --test swarm_director_loop --test swarm_routing -- --test-threads=1`: 9 passed.
- `git diff --check`: passed. The last full Rust workspace pass was at `bab519b`; it has not been rerun for this follow-up.

Remaining: the injected `account_id` is not yet a verified account-generation binding, and provider endpoint/model-version identity is not revalidated against an Auto producer immediately before launch. The shared numeric allowance transaction across ordinary, Auto and Swarm work remains absent. No mixed-account live route or normal UI workflow was exercised; SWARM-28 stays partial and CONTRACT-01 remains unverified.
