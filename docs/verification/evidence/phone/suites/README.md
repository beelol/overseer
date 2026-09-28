# The existing suites, with phone access off and on (AC-132)

`scripts/test-all --jobs=3` (AC-147) runs the Rust workspace (daemon and terminal UI), the extension's unit tests and source check, the link check, the VSIX build and every packaged-UI fixture scenario. `OVERSEER_TEST_PHONE_ACCESS=on` turns phone access on in every scenario's own daemon the way the owner does (`test/ui/harness.js`); the daemon's tests read the same variable. Nothing touches the owner's daemon.

- `test-all-phone-access-off.log`: the whole suite with phone access off, at 376146ef (main merged in), 44 of 53 passed. The load was 13 to 66 during this run: other agents' suites, a local model and virtual machines shared the Mac.
- `test-all-phone-access-on.log`: the whole suite with phone access on, at b8587cc7, 50 of 53 passed; the Rust workspace 337 of 337.
- `reruns-alone.log`: every check that failed in either run, run again alone, one at a time (AGENTS.md: a timing check that fails under load is rerun alone before it is called a regression). All pass.
- `cargo-test-final-phone-access-{off,on}.log`: the Rust workspace at the final commit, in both settings, after the fixes below.

What the failures were:

- **Real, fixed:** `ac185_actions_have_classes_and_cards`. Overseer's own table of method classes (from Gate S, merged in) did not know the methods Gate N added (44e3da9). The scenario `phone-access` checks that phone access starts off, and the suite-wide setting on had turned it on first; it now leaves that setting aside (d98e367a). `continuity` with phone access on selected the handed-off agent before the side bar had folded it under its successor (Gate L's scenario, first run with phone access on; b6148c21); it passes alone in both settings after the fix.
- **Load only:** `ac189` and `ac190` (Overseer's timing), `arrangement`, `audit`, `center`, `review-width` (main's tracker lists these as missing only under load), `accounts`, `composer`, `grid`, `theme` and `main`: each passed alone.

Earlier logs (2026-09-26 and 27, before main was merged in): `cargo-test-workspace-phone-access-{off,on}.log`, `ui-scenarios-phone-access-{off,on}.log`, `ui-scenarios-rerun.log` and `package.log`.
