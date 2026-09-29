# AC-146: merges

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
