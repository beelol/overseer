# AC-146: merges

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
