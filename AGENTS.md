# Working on Overseer — rules for every agent

Codex, Kilo and Claude all read this file (Claude through `CLAUDE.md`). The owner set these rules; they apply to every branch.

## Criteria and the ledger
- Criteria live in `docs/overseer-rfc.md` (AC-NN, each with a **Verify** clause). A criterion is verified only when its Verify clause is fully covered; otherwise it is partial with the gap stated.
- The ledger is `docs/verification/records.py`: add or update your criterion's record with an explicit `commit=`, then regenerate with `set -o pipefail; python3 docs/verification/records.py b5693b8`.
- Criteria and ledger changes go to `main`; implementation goes in its own branch and pull request.

## Pushing and merging
- Push after each criterion. Never keep more than about 20 unpushed commits or three hours of unpushed work.
- Never force-push. Never rewrite `main`.
- When your work is finished, mark your pull request ready for review and say so in it. It is merged by the merge monitor (AC-146): main merged into a throwaway copy, conflicts resolved so both sides keep working, the full test suite run, then a merge commit. Nothing is merged while its agent is still working.
- Do not change another agent's area without saying so in your pull request.

## Brand
- One Overseer mark everywhere (AC-142): see `docs/design/brand.md`. The files live in `docs/design/brand/`: `overseer-app-icon.png` (the full app icon: Marketplace, any Mac app icon, the phone's home screen), `overseer-logo.png` (the colour mark: Overseer's own views, marks inside the apps, Android's adaptive foreground) and `overseer-icon-flat.png` (the single-colour silhouette: surfaces that tint one colour, the phone's door and launch screen, Android's themed icon, a Mac menu-bar template); `overseer-mark.svg` is the silhouette as SVG. Use nothing else, on VS Code, the Mac, the phone or anywhere (AC-178, AC-179).
- Themes: Overseer Dark and Overseer Light, plus the bold "Overseer" theme of Gate M (AC-103). Colours come from the design tokens (`extension/design/tokens.js`), never hard-coded.

## Paid turns
- ChatGPT accounts: only `gpt-5.6-luna` at low reasoning effort. Claude: light use (haiku). One attempt per step, no retry loops.
- Never touch the owner's checkouts, logins or credentials; never sign anything out. Restart or reinstall on the owner's daemon only when no runs are active.

## Tests
- `scripts/test-all` runs everything and prints one summary (AC-147): Rust (daemon and TUI), the extension's unit tests and source check, the ledger's link check, the VSIX build and every packaged-UI fixture scenario. `--jobs=3` runs UI scenarios three at a time; `--only=a,b` picks scenarios; `--no-ui` skips them; `--live` and `--perf` add the paid and load scenarios. Run it before asking for a merge, and run at least one UI scenario before pushing extension changes to `main` (a change that stops the extension activating breaks every agent's build).
- Several agents run VS Code scenarios on the same machine: a timing check that fails under that load is rerun alone before it is called a regression.
- Leave no test windows, daemons, shims or runs going.

## Where the designs are
- Orchestrator UI (Gates J, K, M): `docs/rfcs/orchestrator-ui.md`
- Continuity, offline mode (Gate L): `docs/rfcs/offline-mode.md`
- Phone remote (Gate N): `docs/rfcs/phone-remote.md`, its wire format `docs/rfcs/phone-remote-protocol.md` and its goal `docs/rfcs/phone-remote-goal.md`; the work is in pull request #10
- Audio Mode (Gate O) and the TUI (`docs/rfcs/tui.md`): their RFCs under `docs/rfcs/`
- Auto and Swarm: `docs/rfcs/auto-mode.md` and `docs/rfcs/swarm-mode.md`, built together by the everything goal on `claude/auto-swarm` since 2026-09-27. Auto owns the one shared account booking (`daemon/src/account_booking.rs`); nothing else keeps an account ledger
- Voice Mode (Gate R): `docs/rfcs/voice-mode.md`, its goal `docs/rfcs/voice-mode-goal.md` and the mark's animation `docs/design/voice-mark/index.html`; audio is collected on the Rust side
- Overseer develops Overseer (Gate T: dev instances, the production guard, deploy): `docs/rfcs/dev-instance.md` and its goal `docs/rfcs/dev-instance-goal.md`
- Overseer itself (Gate S): `docs/rfcs/orchestrator.md`, its goal `docs/rfcs/orchestrator-goal.md`; one Overseer session in the daemon, shared with Voice Mode and Talk to Overseer (AC-107)
- Follow-through and agent oversight (Gates P and Q): `docs/overseer-rfc.md`; the goal is `docs/goals/everything.md` and what it tracks is `docs/verification/tracker.md`
