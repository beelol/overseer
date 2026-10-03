# PR53 CI corrections (2026-10-03)

Baseline: `a8edd4d8b99a5dac3c030492f973f3f5ab71bcec`. Fixture-only isolated daemon, no paid turns or owner credentials. All Rust checks used `/private/tmp/overseer-queue-pause-target`, nice 20, two build/test workers. These focused checks are not a full-suite result.

- `swarm-red2.log`: exact CI failure reproduced: stopped Swarm worker follow-up returned `{delivery: queued}` instead of the existing protected-target error.
- `protocol-red.log`: exact CI failure reproduced: real state run queue fields were absent from the protocol description.
- `client-red2.log`: new client type checks against baseline generated protocol fail for the missing queue state/event and false Turn-only follow-up result. Types were regenerated before green checks.
- `targeted-green2.log`: protocol shapes 6/6, queue pause 12/12, Swarm control 10/10. Includes rejection of native children, managed work units, linked Swarm directors, removed/missing workspaces and actual control-lost Swarm worker through follow-up/queue/redirect, with unchanged queue, turns and snapshot count.
- `protocol-green3.log`: final protocol checks 7/7, including real paused follow-up acknowledgement, actual queued/queue_changed events, generated types and malformed acknowledgement rejection.
- `client-green2.log`: phone-core strict typecheck and 219 runtime tests in 13 files pass. The isolated checkout uses its own locked core dependencies and an ignored read-only link to existing Expo dependencies for the base configuration.
- `phone-typecheck2.log` / `model-typecheck2.log`: phone app and model consumer typechecks pass.
- `ac185-green.log`: approval after Stop stays held/paused with no extra turn; one explicit resume delivers the exact UTF-8 bytes, empties the queue and advances the same card.
- `t04-green.log`: current Stop-and-pause help passes; updated snapshots live in `../../tui/t04-help*`.

The shared validator reads structural identity and workspace state before enqueue/snapshot, and is reused by turn launch. Swarm-owned launches keep their existing dedicated authorization; profile/account admission remains at actual launch. Phone method classifications and permission/voice policy are unchanged.

AC198's captured-reply assertion and the production voice completion/capture race are assigned separately to the coordinator; this correction does not alter those paths. UI/full-suite verification remains pending the coordinator lock/slot. This PR stays draft.

Independent read-only source review by `audit_voice_eval` found no evidence-backed blocker in the shared structural guards, queue/result schema union or explicit-resume card assertions. The reviewer ran no additional builds/tests/UI.
