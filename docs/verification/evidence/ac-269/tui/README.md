# AC269 TUI draft checkpoint

Base: eeb1f3f1f7595d4a63a6b4edbc501da62c6f00d1, coordinated follow-up. These source hashes identify the exact working-tree application/tests used by the final focused checks; only evidence/plan files were changed after checks. The source and evidence are saved together in this checkpoint commit. No criterion is marked verified here.

## Evidence

- Initial baseline: 10 cases enumerated; 9 actual missing-Mods-entry assertion failures, 1 existing Compose guard pass. Fresh TUI compilation/test executable is recorded. `nice` was attempted but denied by the sandbox in this initial log; later checks ran through the approved escalation with actual nice20/jobs2/threads2.
- Controls: 14/16 then16/16. Actual own-session event burst made3 reads instead of1; the selected binding field was hidden in a short terminal. Both corrected.
- Selection: exact1-case RED then17/17. Selecting away from an outstanding Overseer summary left its pending flag set and blocked a later explicit selection.
- Footer: exact1-case RED then18/18. Mods incorrectly inherited agent-action footer instructions.
- Required scope/readable labels:18/20 RED. No-active-run scope navigation trapped the owner before Overseer scope, and Applied used the run id as its primary name. Final tests include both corrections and readable public outcome/transport labels.
- Busy/session: exact1-case RED (25 filtered) reproduced a sent operation's acknowledgement being discarded after the current session changed, stranding busy. The final case checks both preview and install: old reads become stale, acknowledgement releases the original pending operation, current facts refresh, stale preview cannot be reused, and no write is replayed.
- Form revision: exact1-case RED (26 filtered) reproduced old binding fields sent with newly adopted revision8, overwriting a concurrent owner edit. Refresh now cancels binding/filter drafts and review prompts; it never rebases their fields.
- Final focused checks: Mods27/27, TUI library27/27, existing fake-client Audio guard1/1, public CLI help1/1. All nonzero suites passed. The CLI help fixture generated its existing artifact; that unrelated generated artifact was restored to its base contents.

Commands used `CARGO_TARGET_DIR=/private/tmp/overseer-closeout-verify-target`, `CARGO_BUILD_JOBS=2`, `RUST_TEST_THREADS=2`, `nice -n20 cargo test -p overseer-tui`. Suite arguments: `--test mods`, `--lib`, `--test audio t24_the_bell_is_withheld_only_while_the_daemon_says_it_plays -- --exact`, `--test look t12_help_explains_options_and_keys -- --exact`. RED exact filters are recorded in each test log. No UI/package/full run, actual terminal session, provider turn, owner profile, private audio or production daemon was used.

## Review and boundaries

Coordinator independent source review accepted the complete bounded module and required corrections, including busy/session acknowledgement and refreshed-form invalidation. The source is frozen for publication; actual terminal interaction and combined full-suite qualification remain pending. TestBackend proves terminal buffers and fake Requests prove client behavior; they do not prove prose quality or installed harness delivery. Native/global/dynamic-local/child support remains the daemon's reported unqualified boundary. Less tool noise remains planned/inactive, with no savings claim. Gate S and transformer execution are separate slices.

## Exact source SHA256

- `tui/src/app.rs`: `63ad7f50fa2fed227305a07affe8d0261f908ce174c7a10e344c8703793e862f`
- `tui/src/app/mods.rs`: `7e2146c5c31904c33ccce86c95076b06e119c3c8891e7524a60a204845937d79`
- `tui/src/ui.rs`: `0c38232ab6dd10e77bcfe9f422901a2a20c77b0e0270a395afce3f6e094125b4`
- `tui/tests/mods.rs`: `f7cdd1017e127acfe95ed25b4064d89aa3cf7ad2a0c338547f490ae450653ad4`
