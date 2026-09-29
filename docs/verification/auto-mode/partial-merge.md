# Partial merge of Auto routing and Swarm (AC-204)

Branch `claude/auto-swarm`, prepared 2026-09-28 and brought up to date with main at `ecd82e93` (merge `4e074a4c`). Main has neither Auto routing nor Swarm (no `auto_*`, `account_booking`, `upper_draw` or `swarm/` under `daemon/src/`). This note says what a merge would land, what stays hidden and why, and which criteria are recorded. Nothing has been merged; the merge monitor (AC-146) does that.

## What lands switched on

These are finished and tested, and are safe with every unfinished feature off:

| Surface | Why it can be on | Evidence |
| --- | --- | --- |
| One account booking for launches (`account_booking.rs`) | An ordinary start books its account only when its draw is qualified (five isolated runs of the same harness, model and effort); otherwise it starts unbooked, as on main. Nothing is calibrated on a fresh install, so behaviour is unchanged until evidence exists. | `daemon/tests/shared_launch.rs`; ledger "Step 4" and "Qualified upper draw" |
| One app-slot count (`app_slots_in_use`) | Every admission path (manual, Gate S's Overseer starts and watchers, Auto, Swarm) reads the same count, so the agent limit holds across them. | `shared_launch.rs`, `auto_gate_s.rs` |
| Qualified upper draw (`upper_draw.rs`) | Learns only from recorded runs and structured readings; it never admits work by itself. | unit `upper_draw::tests` |
| Local learning history and **Auto Usage…** | The content-free local measurements are recorded for every run (capped, 30/90-day expiry, separate file). Their inspection, explicit export and confirmed clear therefore stay visible, so the owner can always see and clear them. | AUTO-AC-37, 38 (partial), 39 (partial), 41; `test/unit/auto-usage.js`, `scenario-auto-root.js` |
| Account privacy in run transcripts | Account replies keep no raw account id, e-mail or credit balance. | `auto_privacy.rs`; AUTO-AC-26 |
| Daemon Auto methods (`auto.*`) | Every launching method requires Auto Mode, which is persisted **off** by default (`auto.mode.set`); Gate S's Overseer cannot set it (`auto.mode.set` is `NEVER` in its control table). | AUTO-AC-01 |
| Daemon Swarm methods (`swarm.*`) | `swarm.native_director` is **on by default** since the owner's decision of 2026-09-28, so the daemon accepts a confirmed `swarm.start` and runs a Claude director on an approved Claude account. Nothing in VS Code reaches it while `overseer.experimental.swarm` is off, and Overseer's conversation can confirm a start only after the owner's yes (`swarm.start` is Confirm; `swarm.native_director.set` is Never). With the switch off, a start is blocked `no_qualified_director` and `swarm.create` is refused. | `swarm_native.rs` `native_director_is_on_by_default_and_the_owner_can_turn_it_off`, `swarm_broker.rs` `unfinished_runtime_transitions_are_disabled_without_fixture_opt_in` |

## What stays hidden, and why

| Surface | Gate (default) | Why it is unfinished | Test that it is hidden when off |
| --- | --- | --- | --- |
| Auto routing in the composer's agent menu ("Automatic selection") | `overseer.experimental.autoRouting` (false) | Auto is 23 of 40 verified. Open: AUTO-AC-04/05/06 need live reads, 10 provider cache timestamps, 14/15 depend on routing inference (34, not started), 17 the unknown-identity policy (owner decision), 21 a real handoff, 24 mid-handoff crash and UI reconnect, 25 the packaged Gate S targets, 31/32/33/35/38 live work. Claude calibration is built from the owner's decisions of 2026-09-28 and waits on live readings. | `test/unit/unfinished-features.js` (the menu is empty of Auto items and a remembered Auto choice is dropped while off) |
| Auto routing tile in the New Task form | same | same | `unfinished-features.js` |
| Starting an Auto root from VS Code (`TaskLauncher.startAuto`, which also turns Auto Mode on) | same | same | `test/unit/task-launcher-auto.js`: while off, an Auto start is refused before any daemon request (no `auto.mode.set`, no `auto.start`) |
| Swarm commands: Start Swarm…, Filter Swarm Jobs…, Pause, Resume, Stop…, Turn Swarm Off, Extend Swarm Deadline… | `overseer.experimental.swarm` (false) | Swarm is 45 of 64 verified at fixture scope; the director and native workers are decided and built, but only the synthetic Claude fixture has directed a Swarm; no live run. | `unfinished-features.js`: each command's enablement, command-palette entry and every menu entry require the setting, and each handler is guarded in code (`whenOn('swarm', …)`) |
| The Swarms section of the Agents view | same | same | `test/unit/swarm-view.js`: while off, `swarm.list` is never requested and no Swarms section is shown |
| Native Swarm director and workers (daemon) | `swarm.native_director` (**on** by default; the owner can turn it off) | Decided; no live director yet. | `swarm_native.rs` default-on and off tests |
| Swarm fixture launch bridges, director owner, storage page limits | `OVERSEER_SWARM_FIXTURE_API=1` (test-only) | Fixture-only transitions; no product authority. | `swarm_broker.rs` |
| Shared-booking fixture inputs | `OVERSEER_SHARED_BOOKING_FIXTURE_API=1` (test-only) | Fixture draws for tests only. | `shared_launch.rs` |

Test-only crash points (`OVERSEER_TEST_SHARED_LAUNCH_CRASH`) and fixture hooks do nothing unless their variable is set.

The packaged scenarios that exercise these features turn their setting on: `scenario-auto-root.js` and `scenario-composer.js` (`overseer.experimental.autoRouting`), `scenario-swarm-status.js`, `scenario-swarm-scale.js` and `scenario-swarm-allowance.js` (`overseer.experimental.swarm`). No packaged scenario yet checks the off state in a real VS Code window; the unit tests above check the manifest, the handlers, the view model and the launcher.

## Criteria recorded

- **Auto (docs/verification/auto-mode/README.md):** 23 of 40 core criteria verified: AUTO-AC-01, 02, 03, 07, 08, 09, 11, 12, 13, 16, 18, 19, 20, 22, 23, 26, 27, 28, 29, 36, 37, 40 and 41. AUTO-AC-19 and 36 were verified in this session at fixture scope. AUTO-AC-30 is conditionally deferred. The other 17 stay in progress or not started, each with its gap in the ledger and its class in `live-plan.md`.
- **Swarm (docs/verification/swarm/coverage.json):** 45 of 64 verified at fixture scope. Left: live-account runs (SWARM-01, 13, 17, 25, 26, 27, 28, 31, 39, 52, 56, 63), an owner choice for a director whose draw is unknown or exceeds the allocation (SWARM-24/40), and packaged-UI scenarios (SWARM-18, 22, 23, 37); S0 to S5 partial.
- **Contract (docs/rfcs/swarm-auto-contract.md):** CONTRACT-04 is partial (the Swarm side, fixtures): two attempts at most across routes and a restart, an uncertain effect pauses with no selection. CONTRACT-01, 02, 03 and 05 are unchanged.

Every unfinished part keeps its gap in its own record; none is marked done by this merge.

## What the full test run covers

`cargo test --workspace --no-fail-fast` on the merged branch (failures rerun alone), the extension's unit tests and source check, the link check, the VSIX build and the packaged UI scenarios, including `scenario-auto-root`, `scenario-composer`, `scenario-swarm-status`, `scenario-swarm-scale` and `scenario-swarm-allowance` with their setting on. No paid turn, live account or sign-in is needed.
