# AC-201 — regression coverage: the suites with the new behaviour off and on

Run on 2026-09-27 from the branch of pull request #14 (`d09978a` and after), fixture harnesses
only. The daemon reads two test settings at its start: `OVERSEER_CHANNEL_DEFAULT` (auto, on, off:
briefings and the channel) and `OVERSEER_CHECK_INS` (off, done, every:N).

| Run | Setting | Result | Log |
| --- | --- | --- | --- |
| `cargo test -p overseerd --test protocol` | briefings/channel on, check-ins every:3 | 52 passed | `protocol-on.log` |
| `cargo test -p overseerd --test protocol` | briefings/channel off, check-ins off | 52 passed | `protocol-off.log` |
| `cargo test -p overseerd --test overseer` | briefings/channel on, check-ins every:3 | 24 passed (462 s) | `overseer-on.log` |
| `cargo test -p overseerd --test overseer` | the defaults (auto; check-ins every third turn) | 24 passed (451 s) | the branch's runs (see the pull request) |
| `cargo test --workspace` | the defaults | passes after the merge of main (Continuity's `handoff.rs` included) | the pull request |
| `node test/ui/scenario-talk.js` | on / every:3 | SCENARIO PASSED | `scenario-talk-on.log` |
| `node test/ui/scenario-parity.js` | on / every:3 | SCENARIO PASSED | `scenario-parity-on.log` |
| `node test/ui/scenario-home.js` | on / every:3 | SCENARIO PASSED | `scenario-home-on.log` |
| the same three scenarios | the defaults | SCENARIO PASSED | `evidence/ui/{talk,parity,home}/` |

The first run with the setting on found two things, both fixed before the run above: a turn Overseer
starts by itself must never create Overseer's run (until the owner has spoken, there is no model
to spend), and a requested conflict scan must wait out the sweep's scan of the same agent instead
of answering "already scanning".

`scripts/test-all` discovers every `test/ui/scenario-*.js`, so the home scenario is in the
one-command run; its log from a clean clone is for the merge.
