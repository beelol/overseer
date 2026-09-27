# AC-146: merges

## Pull requests #8, #11, #12 and #13 (the everything goal's own work), 2026-09-27

- **Agent quiet:** these are this goal's own branches; the owner asked to merge without waiting for the design-review marks.
- **Throwaway copy:** main (with Audio Mode) merged into `claude/gate-k-followups` (clean), then into `claude/gate-m-theme` (clean), then into `claude/brand-mark` (one conflict in `docs/design/brand.md`: both sides kept, the logo branch's two rules and main's Voice Mode exception), then `claude/gate-p-follow-through` on top (clean).
- **Tests:** `scripts/test-all` on that copy: Rust 121 passed, unit 2 of 2, the source check, the VSIX build, 40 of 48 UI scenarios. The eight failures were fixed on main right after the merge or passed on rerun: audit (the grid header's visible text), arrangement (a first-edit shortcut reverted), brand (the search matched itself), modes (hidden tab strips in the immersive dashboard), review-width (the header at 322 px), review (staging now refreshes in 225 ms), notify (passed on rerun), keyboard (still lags under load: AC-149).
- **Merged:** #8 `8d239cb`, #11 `10b8f73`, #12 `01ce6ed`, #13 `fb43c9b`, each with a merge commit (the owner then asked for squash merges from here on).

## Pull requests #5 and #6 (Audio Mode), 2026-09-27

Merged by the Audio Mode agent with the owner, #5 first (`e0db692`) and #6 after it (`ea6a6c2`); the agent recorded AC-143 to AC-145, T-23 and T-24.
