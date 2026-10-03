# Internal runner baseline authoring — not executed

Base7d30ec6, separate codex/mods-runner branch. Four initial macOS tests and a small native C helper are authored; no compiler, helper or external transformer has run. The runner source is an unavailable stub imported only by the integration test, not registered in the daemon module or exposed through RPC/hooks. It returns runner_unavailable for every request. The tests are expected to reach four distinct missing-runner assertions only after helper setup compiles successfully; no RED is claimed yet.

Initial cases cover exact Unicode/multiline output, silent deadline, simultaneous stdout/stderr limits and changed-program refusal. These do not yet prove isolation, cleanup, cancellation, races, installation, raw retrieval or native delivery. Remaining hostile fixtures and all full AC271–273 clauses are mandatory under main's 2026-10-03-mods-external-runner plan. The test helper is synthetic, built by /usr/bin/cc in a private temporary directory during a future allocated test slot; no RTK download, owner path, network or paid model is involved in these four cases.

Production implementation must wait for actual baseline results and review. A future helper process and descendants must be owned, terminated and reaped on every return path; the current stub starts none. Main owns criteria and ledger changes. No AC completion, performance or savings claim.

## First executed baseline

Source c13460e896c2035414c9022b39e68239a4460e18. Coordinator allocated the serialized verify target, package-cleaned overseerd only, and ran nice20/jobs1/threads1 cargo test -p overseerd --test mods_runner --no-run. Fresh=false test artifact, absolute Cargo source path, .d references to runner.rs/helper.c, clean source, executable/source SHA256 and exactly4 names were verified before invocation. Build exited0 in20.33s. The exact executable ran with --nocapture --test-threads=1 at nice20.

Actual result:0 passed,4 failed in0.28s, exit101. Every native C helper compiled successfully; all four tests reached their intended assertion and received runner_unavailable. This proves the missing runner boundary, not enforcement, timeout, output bounding, program-change rejection or cleanup. No helper was executed by the unavailable runner. Preserve these results before implementing; subsequent hostile cases and the full plan remain mandatory. No AC is closed. Compiler/runtime released to the historical migration agent after this run.
