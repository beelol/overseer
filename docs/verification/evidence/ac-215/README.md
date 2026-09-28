# AC-215: guided owner tests

Branch `claude/guided-tests` at `83e49b01` (main merged in after #21), pull request #22.

- `guided-test.txt`: `node test/dev/guided.js`, 7 of 7.
- `test-all-no-ui.txt`: `node scripts/test-all --no-ui`: every step passes except Rust, where `protocol::ac45` failed under load (average about 40). Rerun alone, the whole protocol suite passes (56 of 56), as do `protocol_shapes`, `voice` (second run; its first failed under the same load) and `voice_live`. This stage changes no extension or UI code (`scripts/dev`, the owner-check data, tests, AGENTS.md).
- `voice-mode-dry-run/` and `voice-mode-step1.png`: the Voice Mode check's real preparation, run once by the agent with no owner (2026-09-28 08:02). It used the check file with the owner's logins turned off, so that no login was used. It built `main` (`f6b6a0534a85`) into dev daemon `dev-check-voice-mode` and opened a dev VS Code titled `[dev-check-voice-mode] scratch` on the Overseer view, with the two stand-in agents. Step 1 is recorded; steps 2 to 8 are skipped (they are the owner's). `--finish` removed everything. That main did not yet have stage 2, so the status bar read "Overseer 1 active" and not the dev label.
