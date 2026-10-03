# AC230 typed modes: stopped queues need owner resume — 2026-10-03

Baseline combined integration: `63e2e7960cd3a2bd6afbbd21ea1a1fcc227052bb`. The full run failed `ac230_typed_conversations_set_each_mode_and_start_one_in_auto`; the coordinator reran the existing single test binary alone (33.53 s), and an independent `RUST_BACKTRACE=1 nice -n 20` rerun failed at the same first echo (32.92 s). `backtrace-red.log` identifies the caller at original `overseer_modes.rs:134`, after the public Stop/follow-up sequence rather than the later Auto-start echo.

This is a fixture setup mismatch with intended AC265 behavior. `run.interrupt` pauses the queue; `server.rs:2748–2752` enqueues later follow-ups without resuming. The modes test expected a follow-up echo while leaving that queue paused. Existing AC185 coverage already asserts held message cards and explicit owner resume.

The correction changes only the AC230 test: assert Stop paused the queue, follow-up was queued, its exact text remained pending and no new turn started, then explicitly call the owner's `run.resume_queue` before expecting the echo. Existing mode control/card checks and both Auto argv assertions remain intact. No product bypass or increased timeout is included.

Original single-binary command: `RUST_BACKTRACE=1 nice -n 20 /private/tmp/overseer-closeout-integration-target/debug/deps/overseer_modes-0f9bd2c5cbfd0b5b --exact ac230_typed_conversations_set_each_mode_and_start_one_in_auto --nocapture --test-threads=1`.

After the coordinator released a bounded compiler slot, both focused commands used `CARGO_TARGET_DIR=/private/tmp/overseer-closeout-verify-target`, `CARGO_BUILD_JOBS=2`, `RUST_TEST_THREADS=2` and `nice -n 20`:

- `cargo test -p overseerd --test overseer_modes ac230_typed_conversations_set_each_mode_and_start_one_in_auto --jobs 2 -- --exact --nocapture --test-threads=2`: 1/1 passed in 5.22 s (`exact-green.log`).
- `cargo test -p overseerd --test overseer_modes --jobs 2 -- --test-threads=2`: 7/7 passed in 6.39 s (3 scenarios and 4 shared helpers, `file-green.log`).
- `git diff --check`: passed.

Independent parent source review accepted the +12/-1 test boundary before these checks. No Cargo or UI ran during initial diagnosis/source correction; only the explicitly authorized existing isolated synthetic test binary ran. Later Cargo ran only these two bounded checks. The frozen integration copy and published implementation heads were not modified. No broader tests, native model, paid harness or UI ran for this correction. Test helpers cleaned their own fixture daemons/runs, and the compiler slot was released promptly.

The original full run on source `63e2e796` remains frozen and FAILED its Rust stage. These focused results do not repair that completed run or claim a full pass; the coordinator must integrate this finished test correction and rerun the final combined gate. Criteria/ledger remain its scope.
