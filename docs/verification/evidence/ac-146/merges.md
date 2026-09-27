# AC-146: merges

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
