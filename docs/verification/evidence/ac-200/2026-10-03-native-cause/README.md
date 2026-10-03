# AC-200: archived native caller and Fresh

Source baseline: `63e2e7960cd3a2bd6afbbd21ea1a1fcc227052bb` (coordinator's combined implementation tree). Reviewed implementation and final tests: `c28fff7121fc96db8d08a34dcc5be8ceb2ad819c`. Verifier: Codex; independent source review by the coordinator and Mods agent. These are synthetic isolated daemon/native MCP fixtures; no paid models, owner profiles, production daemon, microphone, or UI checks.

## Confirmed defect and scope

`true-red.log` reached the actual authority assertion on unchanged product source. A daemon-started finding turn retained its real native MCP launch token and transport across `overseer.fresh`. The fresh conversation had `run_id=null`, zero turns and `last_cause=NULL`. One released native `propose` call for `archive` returned `isError:false` and created an open Confirm proposal in that new conversation with `cause:"owner"`. Archive execution still required yes; this evidence proves forbidden proposal admission/attribution, not archive execution.

A native action now snapshots the active session, authenticates its actual calling run, freezes session/level/cause through handling, and revalidates that session under the final insertion lock. `propose` and native `answer` share this boundary. Final proposal state, settle timestamp and cause are inserted together. Withdrawn proposal events use their original session's run. Owner/voice and PR52 captured-output wrappers keep their existing paths. Pending agent questions remain global and can be answered by a legitimate new native owner turn across Fresh.

This closes archived/replaced caller-to-current-conversation authority borrowing. Same-run cross-turn MCP provenance remains a separate, unqualified gap. No broad AC-200 verification, historical credential migration, TOCTOU audit, paid/native-installed qualification, or final combined full-suite pass is claimed. Combination with the finished Mods delivery changes and coordinator full verification remains pending.

## Final focused checks

Commands used `nice -n 20`, `CARGO_BUILD_JOBS=2`, `RUST_TEST_THREADS=2`, and retired target `/private/tmp/overseer-closeout-verify-target`. Disposable socket/process controls required an escalated local test execution. Each test attempt was bounded; no paid retries or owner instances were used.

- `cargo test -p overseerd --test overseer_native_cause -- --nocapture`: **7/7**, 16.33 s (`native-green.log`): 3 integration scenarios plus 4 common helpers. Covers stale finding/native propose, stale native answer, valid current finding Confirm refusal and quiet hold, valid new owner proposal, and legitimate answer to an older pending question after Fresh.
- `cargo test -p overseerd --test overseer_redaction --test tool_refusal_redaction -- --nocapture`: the **redaction file passed 9/9**, 2.61 s; the overall first command failed at the separate refusal fixture's old token authority assumption (`security-compat-failure.log`), so it is not recorded as an overall green command.
- After adapting only that refusal fixture to an actual isolated native launch/token, `cargo test -p overseerd --test tool_refusal_redaction -- --nocapture`: **6/6**, 2.05 s (`refusal-green.log`). Original direct and actual MCP wire secret payload, redaction, refusal-context, and result/error assertions remain intact.
- `cargo test -p overseerd --test voice ac_capture_ -- --nocapture`: **6/6**, 9.57 s (`voice-capture-green.log`), including immutable predecessor fenced actions and withdrawal.
- `cargo test -p overseerd --test overseer ac185_actions_have_classes_and_cards -- --exact --nocapture`: **1/1**, 4.23 s (`proposal-green.log`).
- Fixture JavaScript syntax and Git whitespace checks passed.

## Earlier attempts retained honestly

`setup-sandbox-failure.log`: daemon never started; process-priority and process-list access were denied. This was setup failure, not a security RED. `hidden-run-helper-failure.log`: daemon reached the finding gate, then the test helper tried to find an intentionally hidden Overseer run in `state.runs`; corrected to bounded read-only durable status inspection. This was also not a security RED.

`writer-guard-failure.log`: after Fresh, starting another native run was correctly refused because the old scratch checkout still had an active writer. The final test uses Fresh alone and never bypasses that guard or forces transport survival. `true-red.log`: the narrower Fresh-only case failed on actual native proposal admission in 8.47 s.

`positive-open-list-failure.log`: the stale-call cases passed, and the valid finding's hold took effect; its positive test then looked for the already-answered proposal in the open-only list. Only that inspection changed to its durable cause/state. `security-compat-failure.log`: an Overseer-role token for an unrelated generic run was correctly rejected before invalid-action parsing. Its copied refusal fixture was changed to a real active native owner launch, preserving the original security assertions. Frozen prior PRs were not modified.
