# AC201 settings qualification plan — read-only diagnosis, 2026-10-03

Inspected current full-run source `2a1bd2a12a5fe35dfb7d91e1fa27053ff6d203c2` at `/private/tmp/overseer-closeout-next-20261003`; relevant controls/helpers are unchanged from `63e2e796`. Main criterion checked in docs worktree `7d3e5d15`.

## Effective controls and evidence bounds

- RFC line628 requires clean-clone `scripts/test-all` log AND existing suites' logs with briefings/channel off/on and check-ins off/on. Defaults are not explicit off coverage.
- `daemon/src/daemon.rs:475–489`: startup `OVERSEER_CHANNEL_DEFAULT=auto|on|off` writes meta `overseer.channel`; `OVERSEER_CHECK_INS=off|done|every:N` writes meta `overseer.check_ins`. They apply on every daemon restart when present, including overwriting persisted defaults.
- `overseer/channel.rs:87–117`: non-agent roles and generic programs always get neither. Per-run `channels` rows take priority. `on` forces both true; `off` both false; `auto` requires an established Overseer run plus an active companion in the repository.
- `overseer/checkin.rs:62–68`: per-run cadence beats the global meta value; default is every:3. `done` is enabled completion-only, not off. Channel reports/questions/findings remain valid reasons for turns even when cadence is off, so 'off' must assert no cadence/free-check check-in, not prohibit every non-owner turn.
- Ordinary Rust daemon fixtures inherit process environment (`common/mod.rs:84–112`). Egress-denial fixture at protocol.rs:1970 uses env_clear and copies only basic variables; its assertions do not cover these inherited defaults. UI `Session.baseEnv():108` preserves them and launch spreads explicit scenario env afterward. Home/talk/parity do not override either default. Their VS Code settings are unrelated to these controls.

Historical `evidence/ac-201/README.md` only has protocol off/off and on/every:3; Overseer on/default; home/talk/parity on/default. Old clean-clone log ends 51/54 with Rust/audit/followups failed; later individual reruns do not make that one-command run green. No mixed-profile evidence exists.

## Overrides that invalidate an 'entire test stayed off/on' claim

`daemon/tests/overseer.rs`: global off cadence at lines228,633,1546,1638,1691,1777,1901,2088,2177,2239,2281,2344,2439,2471,2529,2589,2751,2809. Channel forced on at229,1602,1640,1692,2089,2345,2647; cadence every:3 at1454/2467 and every:1 at2648. AC189 changes per-run cadence at1369/1378 and both global modes at1446/1454; AC190 changes global and per-run channel at1602/1616/1621/1624. These are legitimate explicit-setting tests but not inherited-profile proof.

Other gate files intentionally isolate behavior: overseer_continuity.rs155/262/319/365, overseer_surfaces.rs103/167, overseer_modes.rs30, permission_card_answers.rs51 turn cadence off. UI oversight.js32 and trouble.js47 also turn cadence off. Logs may prove regression compatibility starting under a profile; label those phases as explicit owner overrides, not all-on check-in coverage.

Two current inherited-profile incompatibilities are test assumptions, not established product defects:
- AC190 starts under off, then expects second agent briefing/channel true at1566–1573. It only recognizes forced_on at1556; off legitimately suppresses these.
- AC189 begins by expecting turn3/6/finished checks at1349–1363 with inherited cadence. Under off these correctly never occur.

## Smallest setting-aware fixture correction (not authored/run)

1. AC190: read/assert effective `agent.channel` for eligible fixture agents before mutation. Keep normal auto assertions; on branch assert actual lone-agent MCP launch config, off branch keep two overlapping real fixture agents but assert no appended task/queued briefing, no MCP config, no channel-origin report/ask/claim events and no report/ask rows. Preserve the exact typed prompts/files. After off assertions, explicitly set owner channel on for NEW agents to execute the existing report/ask/claim/answer/idempotence tests; identify this transition in the log. Existing explicit global-off and per-run-on tests remain intact. Never quietly force on before the inherited-off assertions.
2. AC189: assert global `agent.cadence` initially. Under inherited off, an actual fixture completes seven turns, trips the existing free-check event, passes the established grace/batch window and has no cadence/free-check check-in starts/results. Then explicitly owner-enable every:3 for a fresh steady fixture and retain all existing 3/6/finished, drifting, done, per-run cadence, global-off and batching assertions. Do not return early and drop the enabled control cases. Default/on retain their current strong assertions.
3. Emit one bounded settings receipt per matrix run: initial eligible agent `agent.channel`, `agent.cadence`, actual fixture MCP argv/config and briefing events, before any RPC override. UI home/talk/parity should retain these effective values in result/scenario evidence; existing source currently only propagates env, so old logs cannot prove that receipt. Tests with deliberate overrides must explicitly log transitions.

## Bounded execution after source correction/review and slots release

Smallest literal off/on coverage uses TWO explicit profiles:

| Profile | Channel/briefings | Check-ins |
|---|---|---|
| off | off | off |
| on | on | every:3 |

These cover each named state but do not prove mixed-setting interactions. Do not call them a full Cartesian matrix. If independent-toggle interaction qualification is required, add off/every:3 and on/off to the same targeted commands; no basis for eight full UI suites.

Use the final frozen integration checkout and its exact-head VSIX, not today's preliminary head. Set `AC201_TARGET` to a root-approved idle shared target. Ensure only the exact-head VSIX is selected by `latestVsix()` (it chooses lexicographically last .vsix), record its SHA256, clean HEAD, and test binaries. Run serially; compiler and UI slots must be released first. No --live/--perf/paid turns.

```sh
set -e
set -o pipefail
export CARGO_TARGET_DIR="$AC201_TARGET" CARGO_BUILD_JOBS=2 RUST_TEST_THREADS=2
mkdir -p /private/tmp/ac201-final-matrix
for profile in off on; do
  if [ "$profile" = off ]; then channel=off; checks=off; else channel=on; checks=every:3; fi
  export OVERSEER_CHANNEL_DEFAULT="$channel" OVERSEER_CHECK_INS="$checks"
  printf 'source=%s channel=%s check_ins=%s\n' "$(git rev-parse HEAD)" "$channel" "$checks" > "/private/tmp/ac201-final-matrix/$profile-profile.log"
  nice -n 20 cargo test -p overseerd --test protocol --test protocol_shapes --test overseer --test overseer_continuity --test overseer_surfaces -- --test-threads=2 2>&1 | tee "/private/tmp/ac201-final-matrix/$profile-rust.log"
  # Only after root releases the full-run UI lock, at most one scenario at a time.
  for scenario in home talk parity; do
    nice -n 20 node "test/ui/scenario-$scenario.js" 2>&1 | tee "/private/tmp/ac201-final-matrix/$profile-$scenario.log"
    # Immediately preserve this run's scenario.log/result/screenshots under a profile-specific path:
    # the next profile uses the same evidence directory and otherwise overwrites them.
  done
done
unset OVERSEER_CHANNEL_DEFAULT OVERSEER_CHECK_INS
```

`--only=home,talk,parity` on scripts/test-all still repeats ALL non-UI steps and packaging; direct node commands avoid that repetition. Home covers starting agents vs talking to Overseer/Fresh/keyboard; talk covers actual proposal/yes/no/permission; parity covers existing native fixture transport, queue and redirect. Do not claim other UI scenarios ran in each setting. Full clean-clone default regression (all gate tests/scenarios discovered) is covered separately by the coordinator's exact final-source one-command gate, if it passes. If literal 'existing suites' is interpreted to require every existing packaged scenario in both profiles, the six targeted runs alone leave that clause partial; keep that scope decision explicit, not silently redefine it.

No source changes, builds, tests, UI or paid calls executed during this review.
