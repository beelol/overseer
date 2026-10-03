# AC-267 Task2 checkpoint

This commit adds local CLI binding controls and deterministic desired-text resolution on the pinned Task1 library. It does not implement turn delivery. AC-267 remains partial: first/subsequent Overseer delivery belongs to Task3; native/global/child qualification and live prose quality are still gaps.

Tests use isolated daemon homes, disabled native harnesses, Git fixture repositories, synthetic stored account/model/harness observations, and generic `/usr/bin/true` processes. No owner credentials/configuration, paid call, UI, production daemon, or full-suite run is involved. Commands use `CARGO_TARGET_DIR=/private/tmp/overseer-mods-target CARGO_BUILD_JOBS=2 RUST_TEST_THREADS=2 nice -n 20`.

- [Missing-method RED](missing-method-red.log): six initial binding fixtures fail because mods.bind does not exist; four shared helper tests pass.
- [Watch-membership RED](watch-membership-red.log): actual watch.start assigns an existing ordinary agent, but the initial selector still reports agent; twelve tests pass and this case fails.
- [Locked-style RED](locked-style-transition-red.log): actual watch.end makes a locked all-agents style applicable again, yet a different one-run style wins. The regression fails with the two exact texts.
- [Final focused GREEN](focused-green.log): `cargo test -p overseerd --test mods_bindings --test mods --test protocol_shapes` passes library13, bindings14 and protocol7. Each suite includes four shared helper tests. Ten binding cases cover role-separated scopes, linked-worktree identity, exact shared-account/model/harness filters, on/off/group/style precedence, locked on/off bindings, pinned bytes after update/restart, stale/concurrent revisions, invalid targets/metadata, active ordinary-watcher membership, and the locked-style transition with explicit owner unlock. A real result/event fixture checks generated protocol shapes.
- [Class GREEN](classes-green.log): `cargo test -p overseerd gateway::classes::tests` passes four classification/parameter/README tests. This precedes the final locked-style resolver correction; classification source is unchanged by that correction.

`node protocol/gen-ts.mjs --check`, `python3 protocol/capabilities.py --check` and `git diff --check` also pass. Independent read-only review accepted the final lock correction and this bounded Task2 checkpoint before commit.

AllAgents excludes Overseer and current watchers. An ordinary agent assigned to an active watch uses the watcher selector until its last watch ends; its stored authentication/tool role does not change. Styles are selected deterministically, an applicable locked style holds, and conflicting applicable locked styles are refused. Bindings remain pinned to an installed id/version/fingerprint tuple; installation does not enable them. Mutation is local-owner-only and Gate S mutations remain Never until the later governance slice.

mods.why exposes desired text, `last_turn: null`, and delivery unsupported. Native configuration is unverified, global text suppression unsupported, and child coverage unknown. These fixtures establish binding/management behavior, not model consumption, instruction enforcement, better prose, or token savings. Duplicate-binding rejection and phone gateway mutation refusal lack dedicated end-to-end fixtures at this checkpoint.
