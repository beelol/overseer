# AC-146: merges

## Pull request #44 (VS Code reads the daemon's reviewed marks; T-26), 2026-10-01

- **Finished:** built by a builder on the coordinator's brief; 11 lines in `extension/src/extension.js` and two checks in `scenario-review-marks`.
- **Throwaway copy:** #44 with main (clean). It changes no Rust. Unit 25/25, the extension's check, links (875, 0 broken); the UI scenarios one at a time (LIVE, ON SCREEN and perf skipped as test-all does): 70 of 72; continuity and review passed alone.
- **Merged:** squash. T-26 verified (ticked in `docs/rfcs/tui.md`).

## Pull request #43 (the terminal UI's parity slice), 2026-10-01

- **Finished:** built by its agent (T-25, T-27 to T-29, T-37 to T-39), main merged in after #38, #40 and #42 (clean), the to-review marks and counts added (T-26 partial), marked ready.
- **Throwaway copy:** #43 with main. `test-all --no-ui`: dev, guided, deploy, links, unit ok; Rust's one failure (the README's phone table not regenerated after the merge) fixed on the branch (`6c9447a5`); `cargo test --workspace --no-fail-fast` 1,424 passed, 0 failed. The UI scenarios one at a time, LIVE, ON SCREEN and perf skipped as test-all does: 71 of 72; inventory passed alone (#43 changes nothing in the extension).
- **Merged:** squash, `e5836534`. T-25, T-27, T-28, T-37, T-38, T-39 verified (ticked in `docs/rfcs/tui.md`); T-26 and T-29 partial.

## Pull request #37 (the phone model brought up to the extension; test-all runs the phone's tests), 2026-10-01

- **Finished:** its first builder stopped mid-way on 2026-09-29; a builder merged main in (clean) and brought the phone model up to #32, #38, #40 and #42 (Accept / Accepted on hunks, "N of M accepted", the review tree drawn in Diffs, the search hint copied from `agent-search.js`), then marked it ready.
- **Throwaway copy:** #37 with main (clean). It changes only `phone/` and two steps in `scripts/test-all`, so the check is the phone's suites: `npm test --prefix phone/model` 152 of 152 (15 files, tsc clean), `npm run check --prefix phone` 32 suites, 477 of 477 (tokens, icons, assets, lint, types). The e2e flows were updated, not run (no simulators).
- **Merged:** squash, `e2806565`. scripts/test-all now runs the phone model's tests and the phone app's check (AC-147).

## Pull request #38 (the review opens on Since task start; Accept and Reject), 2026-09-30

- **Finished:** built by its agent; main (with #40 and #42) merged in by a builder (`acf519dc`: #40's Follow | Diffs only toolbar kept, #38's comparison row under it), the owner's coloured check and X for narrow cards (`78371acd`).
- **Throwaway copy:** #38 with main. `scripts/test-all --jobs=1`: Rust 1,415 passed, dev 12/12, guided 7/7, deploy 6/6, links; the UI was stopped at scenario 28 by the coordinator's 2-hour limit, and the rest ran one at a time. Real failures from #38, each passing on main: audit (the review's text 198 against 175), inventory (the unsaved mark hidden on narrow cards), codex-follow (Resume's jump sent before the review knew Follow was back), notify-agents (timing). Fixed in `15c05bf8` (the comparison row's other choices as icons: 173; the unsaved mark kept; the jump after the state); audit, inventory, notify-agents, follow, main and the review scenarios passed alone. The hand-written loop also ran five LIVE scenarios test-all skips (see the hand-off's pitfalls).
- **Merged:** squash, `31e1a39c`. AC-263 verified; AC-232's text now quotes "Save your edits".

## Pull request #40 (one Overseer layout, Follow inside the review), 2026-09-30

- **Finished:** phase 2 by its agent on 2026-09-29; the owner then rejected Follow opening files in a plain editor, and the rework put Follow inside the review (`26fabb77`..`d67dab5c`, "follow looks fantastic"); "Ask first" and "Start fresh" moved to the composer's foot (`861b905e`).
- **Throwaway copy:** a first full run on the branch's own head (Rust 1,385, UI 76 of 79: theme and conversation fixed on the branch, chat passed alone); then main with #42 merged in and pushed (`df4e814d`, clean).
- **Tests:** `scripts/test-all --jobs=1` at `nice -n 20` on `df4e814d`: Rust 1,414 passed, 0 failed; UI 78 of 80. review-width (the header past the edge at 900 px, from the new switch) fixed in the product (`b6bd0fb9`); chat (a classic scroll bar on this Mac takes 15 px; main fails it the same way) fixed in the scenario (`91e107ee`). review-width, chat, review, review-files, agent-head, overseer-window, home and gallery passed alone on the final head. Main gained no code between the copy and the merge.
- **Merged:** squash, `4da1640e`. AC-264 verified at the merge; AC-99's record notes that an unchanged file now shows in Follow's view.

## Pull request #42 (Overseer in the Mac's menu bar), 2026-09-30

- **Finished:** built by its agent on 2026-09-29; the on-screen check passed 10 of 10 on 2026-09-30 with nobody at the Mac (the owner's yes to run it then).
- **Throwaway copy:** main merged into `claude/menu-bar` (the fixture's `menubar` mode and #32's `tested` mode both kept) and pushed (`8ec869a4`).
- **Tests:** `scripts/test-all --jobs=1` at `nice -n 20`: 78 of 81. Rust stopped at two real gaps, both the new methods missing from a class table: `protocol/protocol.json` (Gate N's gateway test; `21c1179e`, Mac only, the README's phone table and the phone's types regenerated) and Overseer's action classes in `daemon/src/overseer/control.rs` (AC-185; `3ed2eeff`: `menubar.snapshot` read, `review.seen` never). Then `cargo test --workspace --no-fail-fast`: 1,414 passed, 0 failed. review passed alone on the rebuilt copy (it missed twice before, in the full run and alone, as on #32's copy); conversation's miss is fixed on #40.
- **Merged:** squash, `86e993fd`, with the owner's go-ahead to merge and deploy. AC-262 verified at the merge; AC-179's menu-bar part closed (AC-179 stays partial on the owner's notification screenshots).

## Pull requests #32 (Overseer's brain) and #39 (the menu-bar mockup), 2026-09-30

- **Finished:** #32 marked ready by its agent (head `2d5e67ba`, main merged in with the `extension/src/views.js` conflict resolved: main's tooltip with landing and account, `run.plain_reason` first). #39 is documentation only (the mockup and the owner's answers); #42 does not carry its files.
- **Throwaway copy:** #32 plus main plus the two `CARGO_TARGET_DIR` fixes (`dded92c9`, `3dde3075`); #39 merged into main on its own.
- **Tests:** #32: `scripts/test-all --jobs=1` at `nice -n 20`: Rust 1,401 passed, 0 failed; unit 25 of 25; source check; links; dev daemons 12 of 12; guided tests 7 of 7; deploy 6 of 6; 78 of 81 UI scenarios. talk and conversation passed alone on the first rerun, review on the second (AC-149's list of scenarios that miss under load; #32 does not touch the review). #39: the link check on the merged copy, 871 links, 0 broken.
- **Merged:** squash, #39 `ac7a4143`, #32 `3ab9c1f7`, with the owner's go-ahead to merge and deploy (2026-09-30). AC-237, 238, 248 and 253 verified, AC-239 partial, records pointed at the merge.

## Pull requests #36 and #35 (the account every agent runs on; merge from the agent, and the review says what it shows), 2026-09-29

- **Finished:** both marked ready by their agents. #36 (head `27a4564f`): AC-235 verified; the owner accepted (2026-09-29) that a long agent title can cut off the side bar row's account text (the hover and the accessible name keep it). #35 (head `994f78bf`): AC-232 and AC-243 verified; its question about the review's default in the owner's checkout became AC-263 (the owner chose Since task start everywhere).
- **Throwaway copy:** current main, #36 (merged cleanly) and #35 in one copy. #35 conflicted with #36 in `extension/src/command-center.js` (the view's state now carries both Overseer's account and the landings) and `extension/src/views.js` (an agent's row reads what its work became, then the time, then its account). The generated protocol files were already current.
- **Tests:** `scripts/test-all --jobs=3` under the machine lock: 76 of 80. UI audit, review and continuity (twice) passed alone. The Rust step stopped at `overseer`'s ac189, so the whole workspace was then run with `--no-fail-fast`: 1,382 passed, 3 failed (ac189, and protocol's two OpenCode Auto tests), and all three passed alone at load 8 (ac189 waits on fixed sleeps; its fix to wait for events is queued).
- **Merged:** squash, #36 `7be5f462`, then #35 `0fad2bee` after main was merged into its branch with the same resolutions (the code identical to the tested copy).

## Pull requests #31, #33 and #34 (views polish; the layout command and pop-out; the agent's head), 2026-09-29

- **Finished:** all three marked ready by their agents. #31 (head `eb5d1c6a`): AC-217, 236, 245, 247, 254, 255 and 256 verified, AC-246 partial; it changes Needs you to mean "waiting for your answer" and adds a separate "to review" mark (AC-254), which changes AC-61's meaning (told to the owner). #33 (head `7b6af3bf`): AC-250, 251 and 258 verified, AC-244 partial (a dashboard in a window without a workspace file still writes user settings: the owner's call). #34 (head `7b4a6829`): AC-233 and AC-257 verified.
- **Throwaway copy:** one copy with current main, #31, #33 and #34, so one run covered all three. #33 conflicted with #31 in `extension/package.json` (kept #31's Focus Mode names and #33's four new commands; #31's settings text plus #33's sentence on where the settings go) and `extension/src/extension.js` (#33's workspace and pop-out commands plus #31's New Agent that targets an agent). #34 conflicted with both in `package.json` (settings and the source check list), `command-center.js` (the ready message sends both the composer target and the head agent) and `extension.js` (the head, `keepConversation` and #31/#33's `showInCenter` combined: opened from the conversation, the conversation keeps its tab; otherwise the workspace rule applies). Each branch then got main merged in with those same resolutions (identical code to the tested copy) and was squashed in order.
- **Tests:** `scripts/test-all --jobs=3` under the machine lock on that copy: 76 of 78, every Rust test passing. center and sidebar failed under load and passed alone. Before the full run, agent-head, one-view, workspace and own-layout passed alone; review failed once and passed on a rerun (a different check each time, load about 6.6).
- **Merged:** squash, #31 `b2c186fb`, #33 `50a6d041`, #34 `38919c1b`.

## Pull request #28 (the audit's high-severity bugs; test windows off the owner's screen; no leaked test processes), 2026-09-29

- **Finished:** by its agent (head `c6eef220`), each bug reproduced by a test that failed first: Open PR refusing a worktree left mid-merge with conflict markers (finding 40); Overseer's session loop catching up from the store after falling behind (finding 59: 798,807 events behind in the test); agents' questions kept until Overseer's first turn and across Start fresh and the daily cap (finding 60). AC-249 verified (in-window dialogs for test and dev windows). Test processes are stopped on any exit, and `scripts/test-all` fails a run that leaves any behind. audit's transient "Updating comparison…" is quiet and arrangement's review lag is cut from up to 1,076 ms to about 250 ms (a product fix).
- **Throwaway copy:** main (with #27 and #30) merged in (`ccab8dab`). `daemon/src/overseer/session.rs` conflicted: #28 moved the event loop into `handle_event` for the catch-up, and #27 had added `expire_stale_proposals` in the old loop's status arm; kept #28's structure and added that line to `handle_event`. Two generated files were regenerated after the merge: the README's phone table and the phone app's protocol types.
- **Tests:** `scripts/test-all --jobs=3` under the machine lock: 68 of 69 (every UI scenario passed); the Rust step stopped on the README table (fixed), then `cargo test --workspace --no-fail-fast`: 1,359 passed, and the one failure (the app's types, ac134) passed after regenerating them.
- **Merged:** squash, `5492e130`.

## Pull request #27 (one view for talking to Overseer; it moves you around VS Code; you can tell it is working), 2026-09-29

- **Finished:** marked ready by its agent (head `8d89b0e7`, main merged in). AC-226, AC-227 and AC-228 verified; AC-217 partial (the turn-on frames exist in the Overseer theme only, as stills). It removed the docked Talk to Overseer panel (home is the one view), made Needs you a badge, and fixed a daemon bug that kept stale proposals open (the stuck Needs you).
- **Tests:** `scripts/test-all --jobs=3` under the machine lock: 64 of 65; the miss, protocol ac45, passes alone three times in a row at load 2.4 (a fixed 1.5 s wait under ~190 parallel tests). #30 then merged, so main was merged in again: `extension/package.json` conflicted (both added settings; all four kept). On that combination: one_view 7, overseer 34, voice 44, notices 8; unit 19 of 19; links; VSIX; UI one-view, home, voice, keys-on-screen, notify-agents, one-signin, keyboard and gallery passed, and talk passed on its rerun (a webview not ready in time on the first).
- **Merged:** squash, `24c3c245`.

## Pull request #30 (one Sign In, keys on what you see, notifications outside VS Code), 2026-09-29

- **Finished:** AC-261 and AC-242 verified, AC-240 partial (a click cannot reach the TUI; the owner's real banner). Its agent finished with the final run queued; the coordinator saw it through.
- **Tests:** `scripts/test-all --jobs=3` under the machine lock on the branch: 63 of 66. The misses are known and not this branch's: protocol ac45 (a load-timing test that passes alone), review-width (the flaky width check #28 fixes) and one link mid-rewrite during the run (0 broken after).
- **Throwaway copy:** current main merged in (`ea1e2bdf`). Code merged cleanly; ledger files conflicted and were resolved by keeping each criterion's furthest record (#29's AC-259 and AC-260, this branch's AC-240, 242 and 261); the daemon builds, unit 18 of 18, 0 broken links, 178 verified.
- **Merged:** squash, `5e7d4e54`.

## Pull request #29 (the composer: sending clears the box, another repository without a native dialog), 2026-09-29

- **Finished:** marked ready by its agent (head `acdd85f0`, main merged in): AC-259 and AC-260 each pass their whole Verify clause (a keyboard-only packaged scenario, 16 of 16, and a unit test, both failing on main first).
- **Tests:** its `scripts/test-all --jobs=3` under the machine lock with main merged in: 62 of 64; the misses (protocol ac45, the learning sqlite-timeout test, audit's review budget) pass alone, and none is in its area. Main gained only ledger notes after that run.
- **Merged:** squash, `3b7b397b`.

## Pull request #26 (Auto routing and Swarm, partial merge AC-204), 2026-09-28

- **Finished:** built by the everything goal with a sub-agent on `claude/auto-swarm`, marked ready (head `6f78182e`). Unfinished VS Code surfaces stay behind `overseer.experimental.autoRouting` and `overseer.experimental.swarm` (both off); `swarm.native_director` is on by default (owner decision, 2026-09-28) but only acts on a confirmed `swarm.start`. Auto 23 of 40 and Swarm 45 of 64 verified at fixture scope; the rest need live runs on the owner's personal accounts, one owner choice (an unmetered director, SWARM-24/40) or packaged-UI scenarios.
- **Throwaway copy / merges:** main merged into the branch twice (`4e074a4c` with fixes in `62bf1da0`, 15 conflicts resolved keeping both sides; then `6f78182e`, clean). One schema label, 25; a database written by main's daemon opens, keeps its runs and gains the new tables.
- **Tests:** `cargo test --workspace --no-fail-fast` 1,333 passed (overseer ac189 passed alone); unit 15 of 15, check, links, VSIX, `test/dev/run.js` 11 of 11; every non-live UI scenario at `e91f157d` (52 of 54, continuity and sidebar passing alone), the ones the second merge touches (audit, dev-instance, voice), and audio alone at `6f78182e` (passed). The UI run found four real failures (a header overflow menu taking the side bar's first click; palette titles that only matched by accident on main; sidebar-search's palette selection), fixed in `e91f157d`.
- **Merged:** squash, `17762f81`.

## Pull request #24 (the Android door's dropped frames), 2026-09-28

- **Finished:** marked ready by its agent (head `3d4d91ef`, main merged in). The cause, from atrace cold starts: the Mac's state drawn mid-opening (about 190 prop updates, a 44 ms frame), and the opening starting over the first screen's mount. The fix holds the Mac's state until the door has opened, and starts the opening on the UI thread after three on-time frames or 200 ms at most. The door looks the same and opens in 1 s, but may start up to 0.2 s later on a cold start. Touches Gate N's session layer (`hold.ts`), as the PR says.
- **Numbers (20 runs each):** at load 8 to 10, before 18 of 1,214 frames dropped (longest opening 1,165.8 ms), after 2 of 1,200 (1,015 to 1,025 ms). Above load 14 both builds drop frames while the app is idle (the emulator's compositor waits on the host). AC-135 and AC-136 stay partial: a quiet-machine 20-run check is owed.
- **Tests:** the agent's `scripts/test-all --no-ui` (every group passed) and pairing on the emulator. On the merge with current main: the phone's `npm run check` 471 of 471, unit 9 of 9, links.
- **Merged:** squash, `88cd2779`.

## Pull request #20 (Gate S gaps), 2026-09-28

- **Finished:** marked ready by its agent (head `90c7ea44`). AC-182, 187, 191, 193, 194, 197 and 198 verified; the rest of Gate S stays partial with gaps that need Swarm on main or PR #10's phone parts (now merged). Touched Continuity (`handoff.rs`), `redact.rs`, `daemon.rs`, `store.rs` and the extension's views, chat and home, as its PR says.
- **Throwaway copy:** main merged in twice. The second time (`9d2857f9`), after #21 to #25, conflicts were in generated files: the README's and ledger README's counts (main's side kept, then regenerated: 171 verified) and home's evidence (this branch's side: it reworked the home scenario).
- **Tests:** test-all `--jobs=3` on the first merge: 52 of 57. Rust ac45 (load) passed alone; a full `--no-fail-fast` Rust run had 463 passed and 5 misses during the owner's network breaks, each passing alone (TUI t08 and t10, overseer ac197, phone_methods ac129, protocol ac16). After #25, on the second merge: build, unit 9 of 9, VSIX, and UI home, sidebar-search, oversight, review-width and audit each passed alone.
- **Merged:** squash, `88e6ab2f`.

## Pull request #25 (UI scenarios never take focus), 2026-09-28

- **Why:** the owner could not work while scenario windows kept coming to the front; UI runs were paused on every branch until this merged.
- **Finished:** marked ready by its agent (head `96944787`). On macOS the harness starts VS Code paused under its inspector, shows windows without activating the app, keeps them transparent and click-through, and quits through `app.quit()`. `OVERSEER_UI_FOREGROUND=1` gives the old launch. Also touches `scripts/dev code --inspect` (Gate T).
- **Tests:** the agent's two full `scripts/test-all --jobs=3` runs: 54 of 56 (both misses pass alone), then 48 of 56 at load 39 to 49 (each timing miss passes alone or misses the same way on main's foreground launch). Across 15,218 frontmost-app samples at 50 ms in the second run, no scenario window was in front. On the merge with main (`pm-wt`): unit 9 of 9, check, links, VSIX, and UI keyboard passed.
- **Merged:** squash, `72e79ac0`.

## Pull requests #22 and #23 (guided owner tests and deploy, Gate T stages 3 and 4), 2026-09-28

- **Finished:** both marked ready by the dev daemons agent; AC-215 and AC-214 verified on main (`dea2bff9`). #23 was stacked on #22.
- **Throwaway copy:** #23 (which contains #22) with current main merged in, without conflicts. After #22's squash, #23 conflicted only in `scripts/test-all` (its deploy step; kept).
- **Tests:** the agent's `scripts/test-all --jobs=3` on stage 4 gave 56 of 58: memspeech's `speak()` timeout got the retry its callers use (b70cea70), and UI sidebar waits for its rerun after the focus fix (the owner asked for no VS Code test windows until then). On the merge: unit 9 of 9, links, `test/dev/run.js` 11 of 11, `test/dev/guided.js` 7 of 7, `dev_instance` 6 of 6, `test/deploy/run.js` 6 of 6. Found while checking: `scripts/deploy` failed when `CARGO_TARGET_DIR` was set (the binary was built outside its clone); fixed in the merge (`3e0463d2`), and deploy then passed 6 of 6 with it set.
- **Merged:** #22 squash `e66df29a`, #23 squash `48200ea6`.

## Pull request #21 (the dev daemons feature, Gate T stage 2), 2026-09-28

- **Finished:** marked ready by the dev daemons agent (head `0fef4d33`, main merged in with Voice Mode's `voice.subscribe` kept); AC-206 to AC-209 and AC-211 verified on main (`f6b6a053`).
- **Throwaway copy:** main (with #10) merged in without conflicts (`84d420c8`).
- **Tests:** `scripts/test-all --jobs=3`: 56 of 56 passed (Rust, unit, check, links, VSIX and every UI fixture scenario, audit included).
- **Merged:** squash, `76d4ae85`. Stages 3 (guided owner tests, AC-215) and 4 (deploy, AC-214) follow as their own pull requests.

## Pull request #10 (phone remote, Gate N), 2026-09-28

- **Finished:** marked ready by the phone agent (head `1488b4be`), with main merged in at `295f97f2` (Voice Mode and the production guard: conflicts in `server.rs`, `daemon-client.js`, `extension.js` and `Cargo.lock` kept both sides; Voice Mode's methods are Mac-only for phones). Its own records on main (`420853a4`): 15 verified, 7 partial, AC-128 and AC-133 not started.
- **Throwaway copy:** current main merged in without conflicts (`efbddb96`; main had gained only docs).
- **Tests:** `scripts/test-all --jobs=3` at load 8 to 10. The first run: Rust 392 passed and `protocol`'s ac45 failed (a 600 ms notice; it passes alone, 4 of 4), and the VSIX step failed from the copy's setup (no `target/`). The second: Rust 444 passed, 0 failed; unit 8 of 8; check; links; VSIX; 49 of 50 UI scenarios. `audit` fails on review's text budget (175 → 178) exactly as on main; it is not this branch's (recorded as its own row).
- **Merged:** squash, `b95aedfa`. The TestFlight workflow started on the merge (run 36442588449).

## Pull request #19 (production guard, Gate T stage 1), 2026-09-28

- **Finished:** marked ready by the dev daemons agent; AC-212 verified on main (`5b994d28`).
- **Throwaway copy:** one conflict, `extension/src/daemon-client.js`. The guard waits for the daemon's `hello` answer before connecting, and main's Voice Mode subscribes to `voice.subscribe` on connect. The resolution keeps the guard's flow and subscribes to Voice's channel once connected (`a6d7086f`).
- **Tests on the merge:** unit 5 of 5, source check, links; `dev_instance` 2 of 2; the TUI suite; the daemon's `voice` 36 of 36; UI scenarios voice and home passed. The branch's own `scripts/test-all` was run by its agent (load-only misses passed alone). The whole suite runs again on main after the phone agent's quiet window (05:15 to 06:55).
- **Merged:** squash, `66731002`.

## Pull request #16 (Voice Mode, Gate R), 2026-09-28

- **Finished:** marked ready by the Voice Mode agent, with main merged in.
- **Throwaway copy:** merged into current main without conflicts.
- **Tests:** `scripts/test-all --jobs=2` at load 44 to 59: 46 of 53. At load 6 to 12, accounts, continuity and center pass here and on main. Review's conflict check is flaky on both (one fail, one pass alone). Arrangement, audit's review count and the TUI's t10 miss only under load, as on main.
- **Merged:** squash, `b8be3812`. Records on main: AC-165 to AC-175 verified. AC-162, 163, 164 and 177 are partial and AC-176 is not started: all wait on the owner's checks, run in a dev daemon through Gate T's guided test.

## Pull request #15 (Continuity follow-up), 2026-09-28

- **Finished:** marked ready by the Continuity agent.
- **Throwaway copy:** main merged in without conflicts.
- **Tests:** `scripts/test-all --jobs=2`: 50 of 52. The review scenario and Gate S's ac192, ac193 and ac200 passed alone (and pass on main).
- **Merged:** squash, `f2161079`. Its records were on the branch: AC-97 verified with the owner's screenshots, AC-83 partial.

## Pull request #14 (Overseer itself, Gate S), 2026-09-27

- **Finished:** marked ready. The monitor's first run found nine UI scenarios failing that pass on main; the agent fixed them (`3650000e`) and then handed the pull request to the monitor.
- **Throwaway copy:** the branch already contained main; it merged into current main without conflicts.
- **Tests:** `scripts/test-all --jobs=2` at load 25 to 94: 45 of 52. Each remaining failure was checked one scenario at a time: review and scopes passed alone; center, review-width at 900 px, arrangement's first edit and audit's review count also fail on main under the same load (tracker rows added for center and the audit count). Gate S's Rust suite: 22 of 24 under load; ac189 and ac200 pass alone.
- **Merged:** squash, `4c371ecd`. Its criteria were already recorded on main (AC-180, AC-181, AC-184 verified; the rest partial with their gaps).

## Pull request #9 (Continuity, Gate L), 2026-09-27

- **Agent finished:** marked ready; the monitor asked for a compact first-use notice (the audit's composer budget), the agent fixed it and merged main.
- **Throwaway copy:** current main merged into `claude/continuity-gate-l`; the only conflicts were the generated ledger files, regenerated from the merged `records.py` (127 verified).
- **Tests:** `scripts/test-all --jobs=3` on that copy: Rust, unit, the source check, the link check and the VSIX build passed; 48 of 51 UI scenarios. arrangement and review passed when rerun alone; keyboard's first ⌥⌘J fails the same way on main (AC-149), so it is not this branch. On the agent's final head (its own merge of main, with the daemon fix `06d6d75`), `cargo test --workspace`: 207 passed.
- **Merged:** squash, `658a9f0`. Gate L's criteria came in with it (the branch recorded them): all verified but AC-83 (the owner's Wi-Fi toggle) and AC-97 (the owner's offline session), now on the owner-actions list.

## Pull requests #8, #11, #12 and #13 (the everything goal's own work), 2026-09-27

- **Agent quiet:** these are this goal's own branches; the owner asked to merge without waiting for the design-review marks.
- **Throwaway copy:** main (with Audio Mode) merged into `claude/gate-k-followups` (clean), then into `claude/gate-m-theme` (clean), then into `claude/brand-mark` (one conflict in `docs/design/brand.md`: both sides kept, the logo branch's two rules and main's Voice Mode exception), then `claude/gate-p-follow-through` on top (clean).
- **Tests:** `scripts/test-all` on that copy: Rust 121 passed, unit 2 of 2, the source check, the VSIX build, 40 of 48 UI scenarios. The eight failures were fixed on main right after the merge or passed on rerun: audit (the grid header's visible text), arrangement (a first-edit shortcut reverted), brand (the search matched itself), modes (hidden tab strips in the immersive dashboard), review-width (the header at 322 px), review (staging now refreshes in 225 ms), notify (passed on rerun), keyboard (still lags under load: AC-149).
- **Merged:** #8 `8d239cb`, #11 `10b8f73`, #12 `01ce6ed`, #13 `fb43c9b`, each with a merge commit (the owner then asked for squash merges from here on).

## Pull requests #5 and #6 (Audio Mode), 2026-09-27

Merged by the Audio Mode agent with the owner, #5 first (`e0db692`) and #6 after it (`ea6a6c2`); the agent recorded AC-143 to AC-145, T-23 and T-24.
