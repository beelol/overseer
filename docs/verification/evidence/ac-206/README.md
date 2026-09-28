# Gate T stage 2: the dev daemons feature (AC-206 to AC-209, AC-211)

Branch `claude/dev-instance` at `0fef4d33` (main merged in after #19), pull request #21.

- `dev-tests.txt`: `node test/dev/run.js` (11 of 11) and `node test/unit/dev-pin.js`.
- `test-all-jobs2.txt`: `node scripts/test-all --jobs=2` at 07:00 on 2026-09-28: 52 of 55. `UI dev-instance` passed in the suite.
- `cargo-workspace.txt`: `cargo test --workspace --no-fail-fast`: every suite passes except `protocol::ac45`.
- `help.txt`: `scripts/dev --help`.
- The packaged-UI scenario's screenshots and log: [../ui/dev-instance/](../ui/dev-instance/) (committed on the branch; copied here on merge).

Failures, each rerun alone:

| Check | Full run | Alone |
| --- | --- | --- |
| voice `ac169_two_agents_in_flight…` (test-all's Rust step) | failed | passed |
| protocol `ac45_one_notice_per_quit…` (workspace run) | failed | passed (it also failed under load on #19 and passed alone) |
| UI inventory | failed | passed |
| UI review-width | failed | passed (AC-149's load list) |

None of these touch stage 2's files: `scripts/dev`, the extension's pin, and the tests.
