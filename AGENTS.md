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
- One Overseer mark everywhere (AC-142): see `docs/design/brand.md`. The files live in `docs/design/brand/`: the full-colour icon, the transparent mark and the single-colour glyph. Use nothing else, on VS Code, the Mac, the phone or anywhere.
- Themes: Overseer Dark and Overseer Light, plus the bold "Overseer" theme of Gate M (AC-103). Colours come from the design tokens (`extension/design/tokens.js`), never hard-coded.

## Paid turns
- ChatGPT accounts: only `gpt-5.6-luna` at low reasoning effort. Claude: light use (haiku). One attempt per step, no retry loops.
- Never touch the owner's checkouts, logins or credentials; never sign anything out. Restart or reinstall on the owner's daemon only when no runs are active.

## Tests
- `cargo test --workspace` (daemon and TUI); `node test/unit/*.js`; `node extension/scripts/package.js`, then `node test/ui/scenario-<name>.js` (isolated VS Code profiles). `scripts/test-all` runs everything (AC-147).
- Leave no test windows, daemons, shims or runs going.

## Where the designs are
- Orchestrator UI (Gates J, K, M): `docs/rfcs/orchestrator-ui.md`
- Continuity, offline mode (Gate L): `docs/rfcs/offline-mode.md`
- Phone remote (Gate N): `docs/rfcs/phone-remote.md`, its wire format `docs/rfcs/phone-remote-protocol.md` and its goal `docs/rfcs/phone-remote-goal.md`; the work is in pull request #10
- Audio Mode (Gate O), TUI (`docs/rfcs/tui.md`), Auto and Swarm: their RFCs under `docs/rfcs/`
- Voice Mode (Gate R): `docs/rfcs/voice-mode.md`; audio is collected on the Rust side
- Overseer itself (Gate S): `docs/rfcs/orchestrator.md`; one Overseer session in the daemon, shared with Voice Mode and Talk to Overseer (AC-107)
- Follow-through and agent oversight (Gates P and Q): `docs/overseer-rfc.md`; the goal is `docs/goals/everything.md` and what it tracks is `docs/verification/tracker.md`
