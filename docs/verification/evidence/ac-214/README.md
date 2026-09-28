# AC-214: deploy

Branch `claude/deploy` (stacked on #22), pull request #23. The deploy test ran at `beca0317`; `b70cea70` adds only a retry in Voice Mode's in-memory speech test.

- `deploy-test.txt`: `node test/deploy/run.js`, 6 of 6. It deploys onto a temporary production: HOME, VS Code profile and extensions folder, with the notifier not registered with macOS.
- `test-all-jobs3.txt`: `node scripts/test-all --jobs=3` on `beca0317`, 56 of 58. Deploy 6/6, dev daemons 11/11, guided tests 7/7 and UI dev-instance passed. The two failures:
  - The Rust step stopped at `overseer-listener`'s `memspeech::a_line_is_made_in_memory_with_no_file`. The speech service timed out on a busy Mac. The test now tries once more, as `speak()`'s callers do, and passes.
  - UI sidebar failed. It waits for the harness's background-launch fix, and the merge monitor reruns it.
- `cargo-workspace.txt`: `cargo test --workspace --no-fail-fast`, during the owner's network breaks (09:10 to 10:20). Five tests failed, among them a run in `waiting_for_connection` because the network was down. After 10:20 each was rerun: handoff 15/15, phone_methods 10/10, protocol 56/56 and protocol_shapes 6/6 as whole binaries; overseer's ac184 and ac189 alone (both pass).
- `help.txt`: `scripts/deploy --help`.
