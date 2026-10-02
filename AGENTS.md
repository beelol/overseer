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
- Only `gpt-5.6-luna` at low reasoning effort, for every paid turn Overseer runs (tests, live checks, dev daemons, the orchestrator's own run in a dev daemon). No Claude model for now, not even haiku (the owner, 2026-09-28). One attempt per step, no retry loops.
- Never touch the owner's checkouts, logins or credentials; never sign anything out. Restart or reinstall on the owner's daemon only when no runs are active.

## Tests
- `scripts/test-all` runs everything and prints one summary (AC-147): Rust (daemon and TUI), the extension's unit tests and source check, the ledger's link check, the VSIX build and every packaged-UI fixture scenario. `--jobs=3` runs UI scenarios three at a time; `--only=a,b` picks scenarios; `--no-ui` skips them; `--live` and `--perf` add the paid and load scenarios. Run it before asking for a merge, and run at least one UI scenario before pushing extension changes to `main` (a change that stops the extension activating breaks every agent's build).
- Several agents run VS Code scenarios on the same machine: a timing check that fails under that load is rerun alone before it is called a regression.
- `scripts/test-all` takes a machine-wide lock (`$TMPDIR/overseer-test-all.lock`): one full run at a time on this Mac, and a run waits its turn. Don't work around it; single scenarios and single `cargo test` files need no lock. `OVERSEER_TEST_LOCK=off` is for CI runners only. The whole run is at the lowest CPU priority, and `$TMPDIR/overseer-test-jobs-max` (a number) caps how many UI scenarios any run on this Mac opens at once: the coordinator sets it to 1 while the owner is using the Mac. Outside a full run, run at most one UI scenario at a time, and none while a full run holds the lock; prefix builds and tests with `nice -n 20`.
- Leave no test windows, daemons, shims or runs going.

## Running a dev Overseer
- The dev daemons feature runs developer versions of Overseer beside the owner's installed one without colliding with it or with each other. To build and try Overseer itself (a daemon, VS Code or the TUI from your checkout), use a dev daemon, never the installed Overseer: `scripts/dev up --name <short-name> --repo <your worktree>`. It builds from that checkout, starts a daemon with its own data folder, socket, gateway port and VS Code profile, and prints where everything is. You need no one's permission to start one.
- Then `scripts/dev ctl --name <name> state` (any daemon method), `scripts/dev code --name <name> [folder]` (an isolated VS Code pinned to it), `scripts/dev tui --name <name>`, `scripts/dev logs --name <name>`, `scripts/dev up --name <name> --restart` after a rebuild. `scripts/dev --help` has everything.
- Dev daemons have no logins: the Claude harness is the fixture, Codex and OpenCode are off. `--owner-logins` uses the logins in `CLAUDE_CONFIG_DIR` and/or `CODEX_HOME` (and refuses without them; it never falls back to the Mac's default logins, AC-221), and then the paid-turn rules below apply.
- When the owner asks for an owner check a gate lists ("let's start the voice mode test"), run it with `scripts/dev test <check>` (`scripts/dev test --list` names them; the checks are `docs/owner-checks/*.json`): `--start` builds the check's branch into a dev daemon and a dev VS Code and prints step 1; tell the owner each step and `--record` what they say or see (or `--skip` with the reason); `--finish` writes the evidence and cleans up. Commit the evidence with the criteria's records on main.
- Always clean up: `scripts/dev clean --name <name>` (or `--all`) stops its agents, daemon, VS Code and TUI and removes its folder.
- Never touch the production daemon (the owner's installed Overseer): never stop, restart, reinstall or point anything at it, and never deploy to it unless the owner asked you to in this conversation.
- Deploying (`scripts/deploy`, see `scripts/deploy --help`) is the only way a change reaches the owner's installed Overseer: after the pull request is merged to main, and only when the owner asked for it.

## Where the designs are
- Orchestrator UI (Gates J, K, M): `docs/rfcs/orchestrator-ui.md`
- Continuity, offline mode (Gate L): `docs/rfcs/offline-mode.md`
- Phone remote (Gate N): `docs/rfcs/phone-remote.md`, its wire format `docs/rfcs/phone-remote-protocol.md` and its goal `docs/rfcs/phone-remote-goal.md`; the work is in pull request #10
- Audio Mode (Gate O) and the TUI (`docs/rfcs/tui.md`): their RFCs under `docs/rfcs/`
- Auto and Swarm: `docs/rfcs/auto-mode.md` and `docs/rfcs/swarm-mode.md`, built together by the everything goal on `claude/auto-swarm` since 2026-09-27. Auto owns the one shared account booking (`daemon/src/account_booking.rs`); nothing else keeps an account ledger
- Voice Mode (Gate R): `docs/rfcs/voice-mode.md`, its goal `docs/rfcs/voice-mode-goal.md` and the mark's animation `docs/design/voice-mark/index.html`; audio is collected on the Rust side
- Overseer develops Overseer (Gate T: the dev daemons feature, the production guard, guided owner tests, deploy): `docs/rfcs/dev-instance.md` and its goal `docs/rfcs/dev-instance-goal.md`
- Overseer itself (Gate S): `docs/rfcs/orchestrator.md`, its goal `docs/rfcs/orchestrator-goal.md`; one Overseer session in the daemon, shared with Voice Mode and Talk to Overseer (AC-107)
- Follow-through and agent oversight (Gates P and Q): `docs/overseer-rfc.md`; the goal is `docs/goals/everything.md` and what it tracks is `docs/verification/tracker.md`

## Phone app releases (TestFlight / iOS)
- The phone app ships to iOS through TestFlight: App Store Connect app **Overseer Remote**, bundle `com.beelol.overseer.phone`, team `FQ6YGD7554`. The first build (0.1.0 (1)) is installed on the owner's iPhone. The full flow, IDs and signing setup are in `docs/goals/testflight-goal.md`.
- **⚠️ Build-number rule — the thing that bites:** App Store Connect **rejects** any upload whose build number (`CFBundleVersion`) reuses one already uploaded for the same marketing version ("The bundle version must be higher than the previously uploaded version"). **Every upload needs a strictly higher build number than the last.** Use `scripts/testflight-release.sh`, which stamps a timestamp build number (`YYYYMMDDHHMM`) so uploads are always unique and increasing — never hand-reuse a number, and don't rely on the default `1`.
- Signing is **manual**: the STATION 42 "Apple Distribution" cert + the "Overseer App Store" provisioning profile. An **App Manager** API key cannot create/download profiles (Apple requires **Admin** for cloud signing), so the profile is created once in the portal and installed locally. Secrets (the API key `.p8`, any keystore) live outside the repo and are never committed.
- If a run fails at **"Import signing certificate and profile"** with `SecKeychainItemImport: ... passphrase ... not correct`, then `IOS_DIST_CERT_PASSWORD` doesn't match `IOS_DIST_CERT_P12`. Re-export the cert with a known (simple) password and re-set both secrets. To verify a `.p12` + password pair, use `security import` into a throwaway keychain (what CI actually does) — **not** `openssl pkcs12 -noout`, which needs `-legacy` for Keychain Access exports and gives false negatives.
- Build locally from the phone source tip (never a stale copy): `scripts/testflight-release.sh`. CI: `.github/workflows/ios-testflight.yml` builds from `main` on changes to `phone/**` (and manual dispatch), signs, and uploads. It needs the owner's repository secrets (names in the file header) to run.
