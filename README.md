# Overseer

A local agent orchestration daemon in Rust (`overseerd`) with a VS Code extension for
account-based agent runs, recursive native-child visibility, and live editable worktree
review built on [Branch Diff](https://github.com/beelol/branch-diff).

**Status: usable macOS milestone — not the complete product.** Verified acceptance
criteria: **116 / 202** · **7** partial (see [ledger](docs/verification/README.md)). Unverified:
AC-41, AC-53, AC-64, AC-66, AC-83, AC-84, AC-87, AC-89, AC-97, AC-114, AC-115, AC-116, AC-117, AC-118, AC-119, AC-120, AC-121, AC-122, AC-123, AC-124, AC-125, AC-126, AC-127, AC-128, AC-129, AC-130, AC-131, AC-132, AC-133, AC-134, AC-135, AC-136, AC-137, AC-141, AC-146, AC-148, AC-149, AC-151, AC-152, AC-156, AC-157, AC-158, AC-159, AC-160, AC-161, AC-162, AC-163, AC-164, AC-165, AC-166, AC-167, AC-168, AC-169, AC-170, AC-171, AC-172, AC-173, AC-174, AC-175, AC-176, AC-177, AC-178, AC-179, AC-180, AC-181, AC-182, AC-183, AC-184, AC-185, AC-186, AC-187, AC-188, AC-189, AC-190, AC-191, AC-192, AC-193, AC-194, AC-195, AC-196, AC-197, AC-198, AC-199, AC-200, AC-201, AC-202. The biggest gaps are the daily-driver UI (Gate J partials, and Gate K, AC-67 to AC-82: the native side bar
with chat and diff side by side, added by the owner on 2026-09-26; [design](docs/rfcs/orchestrator-ui.md#gate-k-layout)), Continuity, the offline mode with local models (Gate L, AC-83 to AC-98 and AC-138 to AC-140, added by the owner on 2026-09-26; [design](docs/rfcs/offline-mode.md)), Overseer as the whole surface (Gate M, AC-99 to AC-108, added by the owner on 2026-09-26: the review as the home for files, nothing shown twice, a less VS Code-like editor area with a bold Overseer theme, a grid built by dragging, and a chat with Overseer itself; [design](docs/rfcs/orchestrator-ui.md#gate-m-overseer-as-the-whole-surface)), the phone remote on the same network (Gate N, AC-115 to AC-137 and AC-141, added by the owner on 2026-09-26: a hyper fast iOS and Android app that sees and controls every agent through a gateway in the daemon, paired once and built on the simulators first; [design](docs/rfcs/phone-remote.md)), Voice Mode (Gate R, AC-162 to AC-177, added by the owner on 2026-09-27: a voice to talk to constantly that redirects every agent from context, answers quickly and shows every word it sent, with audio collected on the Rust side and the animated mark in the middle moving with the voice; [design](docs/rfcs/voice-mode.md)), fixed Claude accounts (AC-53, partial;
[design](docs/rfcs/claude-credentials.md)), which wait for a second Claude account, and Linux (AC-41),
which is out of scope for now. The full list is under [Acceptance criteria](#acceptance-criteria); next actions are in [Follow-ups](#follow-ups).

## Owner actions

Only the owner can do these (AC-160). Each is one step; the criterion it unblocks is in brackets. The everything goal (`docs/goals/everything.md`) keeps this list current.

- Decide where the one-line search should live: VS Code keeps every extension side-bar pane at least 120 px tall, so a separate search pane is never one line [AC-155].
- Sign in to Claude Code again (`claude`, then `/login`) [the Claude half of AC-81, AC-45].
- Try the Gate K build in your own VS Code [AC-114].
- Let the GitHub CLI push workflow files: `gh auth refresh -s workflow` (the checks for every pull request are written and waiting) [AC-148].
- Say whether the Gate J design review still needs marks or the Gate K review replaces it [AC-66].
- Work an hour using only Overseer [AC-64].
- Pick the animation for the mark in Voice Mode on the [preview page](https://claude.ai/artifact/7YePXA48Ht7CoBAtYJuyWr): one, or a mix [AC-177].
- Answer the nine questions at the end of the [Voice Mode RFC](docs/rfcs/voice-mode.md#open-questions-for-the-owner); its defaults stand until then [Gate R, AC-162 to AC-177].
- Later: a second Claude account [AC-53]; a Linux machine [AC-41]; the owner-confirmed sessions of Continuity and the phone app when their agents finish [AC-97, AC-133].

## Acceptance criteria

Checked means verified with evidence; each item links to its evidence record. The same
checkboxes appear in the [RFC](docs/overseer-rfc.md), which holds the full criterion text
and Verify clauses. Both lists are generated from the records by
`python3 docs/verification/records.py <commit>`, so they cannot disagree.

<!-- ac-list:start -->
- [x] **AC-01** Harness capability survey — [evidence](docs/verification/AC-01.md)
- [x] **AC-02** Account and child feasibility spikes — [evidence](docs/verification/AC-02.md)
- [x] **AC-03** Reuse decision — [evidence](docs/verification/AC-03.md)
- [x] **AC-04** Installable macOS foundation with portable design — [evidence](docs/verification/AC-04.md)
- [x] **AC-05** Independent durable state — [evidence](docs/verification/AC-05.md)
- [x] **AC-06** Honest process lifecycle — [evidence](docs/verification/AC-06.md)
- [x] **AC-07** Persistent sessions — [evidence](docs/verification/AC-07.md)
- [x] **AC-08** Local access boundary — [evidence](docs/verification/AC-08.md)
- [x] **AC-09** Complete task controls — [evidence](docs/verification/AC-09.md)
- [x] **AC-10** Event replay and bounded output — [evidence](docs/verification/AC-10.md)
- [x] **AC-11** Account profiles — [evidence](docs/verification/AC-11.md)
- [x] **AC-12** Two simultaneous ChatGPT subscriptions — [evidence](docs/verification/AC-12.md)
- [x] **AC-13** Credential isolation on macOS — [evidence](docs/verification/AC-13.md)
- [x] **AC-14** Initial adapters — [evidence](docs/verification/AC-14.md)
- [x] **AC-15** Generic harness fallback — [evidence](docs/verification/AC-15.md)
- [x] **AC-16** Permissions and limits — [evidence](docs/verification/AC-16.md)
- [x] **AC-17** Compatibility truthfulness — [evidence](docs/verification/AC-17.md)
- [x] **AC-18** Recursive run tree — [evidence](docs/verification/AC-18.md)
- [x] **AC-19** Actual native children — [evidence](docs/verification/AC-19.md)
- [x] **AC-20** Evidence-backed inference — [evidence](docs/verification/AC-20.md)
- [x] **AC-21** Worktrees by default — [evidence](docs/verification/AC-21.md)
- [x] **AC-22** Current dirty checkout — [evidence](docs/verification/AC-22.md)
- [x] **AC-23** Shared workspace ownership — [evidence](docs/verification/AC-23.md)
- [x] **AC-24** Safe workspace retention — [evidence](docs/verification/AC-24.md)
- [x] **AC-25** Correct repository selection — [evidence](docs/verification/AC-25.md)
- [x] **AC-26** Run snapshots and selectable bases — [evidence](docs/verification/AC-26.md)
- [x] **AC-27** Complete change and dirty views — [evidence](docs/verification/AC-27.md)
- [x] **AC-28** No cancellation blind spot — [evidence](docs/verification/AC-28.md)
- [x] **AC-29** Follow across and within files — [evidence](docs/verification/AC-29.md)
- [x] **AC-30** Navigation ownership — [evidence](docs/verification/AC-30.md)
- [x] **AC-31** Live Review refresh — [evidence](docs/verification/AC-31.md)
- [x] **AC-32** Edit selected workspace — [evidence](docs/verification/AC-32.md)
- [x] **AC-33** Preserve conflicting drafts — [evidence](docs/verification/AC-33.md)
- [x] **AC-34** Safe file boundaries — [evidence](docs/verification/AC-34.md)
- [x] **AC-35** Responsive review — [evidence](docs/verification/AC-35.md)
- [x] **AC-36** Packaged macOS UI — [evidence](docs/verification/AC-36.md)
- [x] **AC-37** Automated regression coverage — [evidence](docs/verification/AC-37.md)
- [x] **AC-38** Reproducible acceptance ledger — [evidence](docs/verification/AC-38.md)
- [x] **AC-39** Minimal dogfood flow — [evidence](docs/verification/AC-39.md)
- [x] **AC-40** Repository handoff — [evidence](docs/verification/AC-40.md)
- [ ] **AC-41** Linux verification (deferred by owner) — deferred: no Linux environment — [evidence](docs/verification/AC-41.md)
- [x] **AC-42** Hunk accept and reject — [evidence](docs/verification/AC-42.md)
- [x] **AC-43** Structured run conversation view — [evidence](docs/verification/AC-43.md)
- [x] **AC-44** Merge back — [evidence](docs/verification/AC-44.md)
- [x] **AC-45** Visible background agents — [evidence](docs/verification/AC-45.md)
- [x] **AC-46** Simple account governance — [evidence](docs/verification/AC-46.md)
- [x] **AC-47** Polished, theme-compatible UI — [evidence](docs/verification/AC-47.md)
- [x] **AC-48** Overseer view (command center) — [evidence](docs/verification/AC-48.md)
- [x] **AC-49** Restore the open session — [evidence](docs/verification/AC-49.md)
- [x] **AC-50** Open a pull request from a run — [evidence](docs/verification/AC-50.md)
- [x] **AC-51** Worktree file hierarchy — [evidence](docs/verification/AC-51.md)
- [x] **AC-52** Native Overseer notifications (macOS) — [evidence](docs/verification/AC-52.md)
- [ ] **AC-53** Fixed Claude accounts — ◐ partial: the design for keeping each Claude account's credentials separate is written (docs/rfcs/claude-credentials.md: check per-folder Keychain entries first, otherwise Overseer-managed credentials); the account flows it builds on (Add Account → Anthropic → Sign In with its own CLAUDE_CONFIG_DIR, sign-out, expiry and Sign in again) pass with the synthetic account CLI / deferred: a live test with a second Claude account (the owner asked not to test Claude yet, and has one Claude account) — [evidence](docs/verification/AC-53.md)
- [x] **AC-54** Clean, calm presentation with less text — [evidence](docs/verification/AC-54.md)
- [x] **AC-55** A chat that feels great — [evidence](docs/verification/AC-55.md)
- [x] **AC-56** Overseer themes, light and dark — [evidence](docs/verification/AC-56.md)
- [x] **AC-57** Overseer dashboard — [evidence](docs/verification/AC-57.md)
- [x] **AC-58** Agent grid — [evidence](docs/verification/AC-58.md)
- [x] **AC-59** Start a new agent from the chat — [evidence](docs/verification/AC-59.md)
- [x] **AC-60** Native-CLI parity for everyday use — [evidence](docs/verification/AC-60.md)
- [x] **AC-61** Needs-you inbox and keyboard control — [evidence](docs/verification/AC-61.md)
- [x] **AC-62** Usage and limits — [evidence](docs/verification/AC-62.md)
- [x] **AC-63** History that stays tidy — [evidence](docs/verification/AC-63.md)
- [ ] **AC-64** Default-to-Overseer session (owner-confirmed) — owner session after the rest of Gate J — [evidence](docs/verification/AC-64.md)
- [x] **AC-65** Provider logos — [evidence](docs/verification/AC-65.md)
- [ ] **AC-66** Design review against references (owner-confirmed) — ◐ partial: the references are studied and what Overseer adopts is written down (docs/design/references.md, the RFC's "What Overseer adopts"); the review page shows every view before and after in both Overseer themes and a stock theme, plus the new views (chat, live chats, grid, dashboard mode, composer, Needs you, history, usage, themes and logos), with a Looks right / Needs work mark and a note per view saved for the owner / deferred: the owner's marks, the changes they ask for, and the owner's dated confirmation that the UI looks clean and polished — [evidence](docs/verification/AC-66.md)
- [x] **AC-67** One agents list: the native side bar — [evidence](docs/verification/AC-67.md)
- [x] **AC-68** Provider logos in the side bar — [evidence](docs/verification/AC-68.md)
- [x] **AC-69** Search and filter in the side bar — [evidence](docs/verification/AC-69.md)
- [x] **AC-70** Quiet row actions — [evidence](docs/verification/AC-70.md)
- [x] **AC-71** Take an agent out — [evidence](docs/verification/AC-71.md)
- [x] **AC-72** Chat in the middle when there is nothing to review — [evidence](docs/verification/AC-72.md)
- [x] **AC-73** Changes bring the diff forward — [evidence](docs/verification/AC-73.md)
- [x] **AC-74** Follow or manual review — [evidence](docs/verification/AC-74.md)
- [x] **AC-75** One place for changes — [evidence](docs/verification/AC-75.md)
- [x] **AC-76** Review that stays clean at any width — [evidence](docs/verification/AC-76.md)
- [x] **AC-77** Chat that works beside a diff — [evidence](docs/verification/AC-77.md)
- [x] **AC-78** Quiet turn endings — [evidence](docs/verification/AC-78.md)
- [x] **AC-79** Grid and dashboard mode in the new layout — [evidence](docs/verification/AC-79.md)
- [x] **AC-80** Remembered place — [evidence](docs/verification/AC-80.md)
- [x] **AC-81** Gate J still holds — [evidence](docs/verification/AC-81.md)
- [x] **AC-82** Gate K design review (owner-confirmed) — [evidence](docs/verification/AC-82.md)
- [ ] **AC-83** Offline is not an outage — ◐ partial: the daemon keeps one connection state decided from the system's own answer, the probes and the agents' errors, with every change an event; a provider outage gives degraded naming the provider, a failing baseline or the system's no-network gives offline, and a 429 or a usage limit leaves it online; on this machine the system's answer, both baseline probes and both providers are read live; the status bar and the side bar show each state in the packaged extension / deferred: the owner turning Wi-Fi off and on while the daemon logs the change (offline within 10 s of the system signal, online after the checks agree): this session needs the network itself — [evidence](docs/verification/AC-83.md)
- [ ] **AC-84** Fail over to the best working provider — ◐ partial: with one provider unreachable and the other working, a run whose turn fails on the connection continues on the other harness in the same task and worktree, the predecessor reads handed off and never failed, both chats say why, and the review shows the successor's work; the owner's provider order is kept; among several accounts the one with the most quota left is taken, never one at its limit, and with equal quota the one used last; with every provider failing nothing fails over and the work goes local; with Continuity off the run waits and is offered the other provider; a move that would loosen the permission mode is offered with the difference and made only when the user accepts the mode / deferred: the live check with Codex's hosts blocked and Claude Code working. It is written (`node test/local/handoff-live.js failover`) and blocked: Claude Code's existing login on this machine is signed out (`claude auth status`: `loggedIn: false`) — [evidence](docs/verification/AC-84.md)
- [x] **AC-85** Local inventory read from the machine — [evidence](docs/verification/AC-85.md)
- [x] **AC-86** Memory budget and fit — [evidence](docs/verification/AC-86.md)
- [ ] **AC-87** Verified local catalogue, Qwen coders first — ◐ partial: every Qwen coder of the catalogue went through the real OpenCode and Ollama path, and through Codex's own local mode, on this machine, inside the memory budget, one model at a time and smallest first; the results and the versions are in the catalogue file; `qwen3-coder:30b` passed three of three with both; every `qwen2.5-coder` size failed with both and is excluded from automatic picks, with its reason given wherever a pick is explained; automatic picks use only models that Ollama reports with `tools` and the catalogue marks passed, or any `tools` model when `allowUnverifiedModels` is on / deferred: a failed model shown as unverified in the packaged UI (the daemon gives the mark and the reason in `local.pick` and `local.catalogue`); it arrives with the local models in the composer (AC-94) — [evidence](docs/verification/AC-87.md)
- [x] **AC-88** Settings the daemon enforces — [evidence](docs/verification/AC-88.md)
- [ ] **AC-89** Download models only when allowed — ◐ partial: with downloads off a pick that needs a model is reported as not installed and the registry is never asked; with downloads on, the first pull is confirmed once with the model and its size, progress is streamed as events, a pull is cancelled midway and the next one continues from what was downloaded, a disk with too little room refuses it, nothing is pulled offline, and one pull runs at a time; prefetch is off until asked, fetches exactly the model `local.pick` names and nothing else, never during a paid turn, and stops when downloads are switched off; real pulls of four Qwen coders on this machine / deferred: the progress inside a run's chat, and the one-time prefetch offer when downloads are first allowed (the extension shows a download's progress with Cancel above the composer and in the Local models pick, and the first pull asks once with its size; the offer to keep the best-fitting model ready is not built) — [evidence](docs/verification/AC-89.md)
- [x] **AC-90** Install and run Ollama only when allowed — [evidence](docs/verification/AC-90.md)
- [x] **AC-91** Transition to local when offline — [evidence](docs/verification/AC-91.md)
- [x] **AC-92** Wait and retry, never fail (for 36 hours) — [evidence](docs/verification/AC-92.md)
- [x] **AC-93** Back online — [evidence](docs/verification/AC-93.md)
- [x] **AC-94** Local models as a first-class choice — [evidence](docs/verification/AC-94.md)
- [x] **AC-95** Honest offline UI — [evidence](docs/verification/AC-95.md)
- [x] **AC-96** Several local agents — [evidence](docs/verification/AC-96.md)
- [ ] **AC-97** Offline session (owner-confirmed) — not started (Gate L, added by the owner on 2026-09-26) — [evidence](docs/verification/AC-97.md)
- [x] **AC-98** On by default, explained once — [evidence](docs/verification/AC-98.md)
- [x] **AC-99** The review is where files live — [evidence](docs/verification/AC-99.md)
- [x] **AC-100** Nothing shown twice — [evidence](docs/verification/AC-100.md)
- [x] **AC-101** Overseer's own reviewer — [evidence](docs/verification/AC-101.md)
- [x] **AC-102** An immersive editor area — [evidence](docs/verification/AC-102.md)
- [x] **AC-103** The Overseer theme — [evidence](docs/verification/AC-103.md)
- [x] **AC-104** Build the grid by dragging — [evidence](docs/verification/AC-104.md)
- [x] **AC-105** Track an agent from the grid — [evidence](docs/verification/AC-105.md)
- [x] **AC-106** Never lose track of windows — [evidence](docs/verification/AC-106.md)
- [x] **AC-107** Talk to Overseer — [evidence](docs/verification/AC-107.md)
- [x] **AC-108** Gate M design review (owner-confirmed) — [evidence](docs/verification/AC-108.md)
- [x] **AC-109** A composer that does not wrap — [evidence](docs/verification/AC-109.md)
- [x] **AC-110** Account names read once — [evidence](docs/verification/AC-110.md)
- [x] **AC-111** The composer says what's next — [evidence](docs/verification/AC-111.md)
- [x] **AC-112** Search you can see — [evidence](docs/verification/AC-112.md)
- [x] **AC-113** No empty grid — [evidence](docs/verification/AC-113.md)
- [ ] **AC-114** Gate K in the owner's VS Code (owner-confirmed) — not started (Gate K follow-up from the owner's marks) — [evidence](docs/verification/AC-114.md)
- [ ] **AC-115** Feasibility and reuse before lock-in — not started (Gate N, added by the owner on 2026-09-26) — [evidence](docs/verification/AC-115.md)
- [ ] **AC-116** A gateway switched on and off on the desktop — not started (Gate N, added by the owner on 2026-09-26) — [evidence](docs/verification/AC-116.md)
- [ ] **AC-117** Pairing needs the Mac — not started (Gate N, added by the owner on 2026-09-26) — [evidence](docs/verification/AC-117.md)
- [ ] **AC-118** Encrypted and mutually authenticated — not started (Gate N, added by the owner on 2026-09-26) — [evidence](docs/verification/AC-118.md)
- [ ] **AC-119** Devices, scopes and revoking — not started (Gate N, added by the owner on 2026-09-26) — [evidence](docs/verification/AC-119.md)
- [ ] **AC-120** Found on the network — not started (Gate N, added by the owner on 2026-09-26) — [evidence](docs/verification/AC-120.md)
- [ ] **AC-121** Never lose the session — not started (Gate N, added by the owner on 2026-09-26) — [evidence](docs/verification/AC-121.md)
- [ ] **AC-122** Sent exactly once — not started (Gate N, added by the owner on 2026-09-26) — [evidence](docs/verification/AC-122.md)
- [ ] **AC-123** The Mac stays awake while it matters — not started (Gate N, added by the owner on 2026-09-26) — [evidence](docs/verification/AC-123.md)
- [ ] **AC-124** See every agent — not started (Gate N, added by the owner on 2026-09-26) — [evidence](docs/verification/AC-124.md)
- [ ] **AC-125** Control every agent — not started (Gate N, added by the owner on 2026-09-26) — [evidence](docs/verification/AC-125.md)
- [ ] **AC-126** Review on the phone — not started (Gate N, added by the owner on 2026-09-26) — [evidence](docs/verification/AC-126.md)
- [ ] **AC-127** Everything else Overseer has — not started (Gate N, added by the owner on 2026-09-26) — [evidence](docs/verification/AC-127.md)
- [ ] **AC-128** Talk to Overseer from the phone — not started (Gate N, added by the owner on 2026-09-26) — [evidence](docs/verification/AC-128.md)
- [ ] **AC-129** Needs-you notifications you can switch — not started (Gate N, added by the owner on 2026-09-26) — [evidence](docs/verification/AC-129.md)
- [ ] **AC-130** Safe without friction — not started (Gate N, added by the owner on 2026-09-26) — [evidence](docs/verification/AC-130.md)
- [ ] **AC-131** One app, iOS and Android, that looks like Overseer — not started (Gate N, added by the owner on 2026-09-26) — [evidence](docs/verification/AC-131.md)
- [ ] **AC-132** Regression coverage for the phone — not started (Gate N, added by the owner on 2026-09-26) — [evidence](docs/verification/AC-132.md)
- [ ] **AC-133** Phone session (owner-confirmed) — not started (Gate N, added by the owner on 2026-09-26) — [evidence](docs/verification/AC-133.md)
- [ ] **AC-134** Platform behaviour behind generic interfaces — not started (Gate N, added by the owner on 2026-09-26) — [evidence](docs/verification/AC-134.md)
- [ ] **AC-135** Hyper fast — not started (Gate N, added by the owner on 2026-09-26) — [evidence](docs/verification/AC-135.md)
- [ ] **AC-136** The door — not started (Gate N, added by the owner on 2026-09-26) — [evidence](docs/verification/AC-136.md)
- [ ] **AC-137** Motion throughout — not started (Gate N, added by the owner on 2026-09-26) — [evidence](docs/verification/AC-137.md)
- [x] **AC-138** Permission modes carry over — [evidence](docs/verification/AC-138.md)
- [x] **AC-139** OpenCode session transport spike — [evidence](docs/verification/AC-139.md)
- [x] **AC-140** Memory safety guard — [evidence](docs/verification/AC-140.md)
- [ ] **AC-141** Pair once — not started (Gate N, added by the owner on 2026-09-26) — [evidence](docs/verification/AC-141.md)
- [x] **AC-142** One Overseer mark everywhere — [evidence](docs/verification/AC-142.md)
- [x] **AC-143** Opt-in audio cues owned by the daemon — [evidence](docs/verification/AC-143.md)
- [x] **AC-144** A lost session asks for attention — [evidence](docs/verification/AC-144.md)
- [x] **AC-145** Audio Mode by ear (owner-confirmed) — [evidence](docs/verification/AC-145.md)
- [ ] **AC-146** Reconcile and merge the work in flight — not started (Gate P, added by the owner on 2026-09-27) — [evidence](docs/verification/AC-146.md)
- [x] **AC-147** One command runs every test — [evidence](docs/verification/AC-147.md)
- [ ] **AC-148** Checks on every pull request — not started (Gate P, added by the owner on 2026-09-27) — [evidence](docs/verification/AC-148.md)
- [ ] **AC-149** A steady UI suite — ◐ partial: causes fixed on main: the UI harness aims a click only once its target has stopped moving; the review no longer takes keyboard focus while following an agent; ⌥⌘J presses queue; the keyboard scenario waits for each selection; staging refreshes the review in 225 ms (was about 2 s); the hunk scenario's redo passed in every run this session / deferred: three consecutive clean full runs on one build, and the first-edit p95 under 400 ms over ten runs (last single runs: 361 and 466 ms): both need a machine where no other agent is running VS Code scenarios at the same time (the phone and Continuity agents were running theirs throughout) — [evidence](docs/verification/AC-149.md)
- [x] **AC-150** The first click always lands — [evidence](docs/verification/AC-150.md)
- [ ] **AC-151** Every live scenario rerun on the current build — not started (Gate P, added by the owner on 2026-09-27) — [evidence](docs/verification/AC-151.md)
- [ ] **AC-152** Performance re-measured — not started (Gate P, added by the owner on 2026-09-27) — [evidence](docs/verification/AC-152.md)
- [x] **AC-153** A ledger that stays true — [evidence](docs/verification/AC-153.md)
- [x] **AC-154** Composer choices fill the row — [evidence](docs/verification/AC-154.md)
- [x] **AC-155** One-line search with a filter menu — [evidence](docs/verification/AC-155.md)
- [ ] **AC-156** Every agent works from the same rules — not started (Gate Q, added by the owner on 2026-09-27) — [evidence](docs/verification/AC-156.md)
- [ ] **AC-157** Oversee the other agents — not started (Gate Q, added by the owner on 2026-09-27) — [evidence](docs/verification/AC-157.md)
- [ ] **AC-158** Gate M's theme and immersive look are back in scope — not started (Gate Q, added by the owner on 2026-09-27) — [evidence](docs/verification/AC-158.md)
- [ ] **AC-159** The toolchain works without Xcode's license — not started (Gate Q, added by the owner on 2026-09-27) — [evidence](docs/verification/AC-159.md)
- [ ] **AC-160** Owner actions in one place — not started (Gate Q, added by the owner on 2026-09-27) — [evidence](docs/verification/AC-160.md)
- [ ] **AC-161** Everything merged into one main — not started (Gate Q, added by the owner on 2026-09-27) — [evidence](docs/verification/AC-161.md)
- [ ] **AC-162** Voice spike before lock-in — not started (Gate R, added by the owner on 2026-09-27; the goal's first step) — [evidence](docs/verification/AC-162.md)
- [ ] **AC-163** Owned by the daemon, heard in Rust, off until asked — not started (Gate R, added by the owner on 2026-09-27) — [evidence](docs/verification/AC-163.md)
- [ ] **AC-164** Holds the floor — not started (Gate R, added by the owner on 2026-09-27) — [evidence](docs/verification/AC-164.md)
- [ ] **AC-165** A quick answer that it is working on it — not started (Gate R, added by the owner on 2026-09-27) — [evidence](docs/verification/AC-165.md)
- [ ] **AC-166** The right agents, from context — not started (Gate R, added by the owner on 2026-09-27) — [evidence](docs/verification/AC-166.md)
- [ ] **AC-167** Redirect without trampling — not started (Gate R, added by the owner on 2026-09-27) — [evidence](docs/verification/AC-167.md)
- [ ] **AC-168** New agents from a request — not started (Gate R, added by the owner on 2026-09-27) — [evidence](docs/verification/AC-168.md)
- [ ] **AC-169** Evidence for every word sent — not started (Gate R, added by the owner on 2026-09-27) — [evidence](docs/verification/AC-169.md)
- [ ] **AC-170** Correct and cancel — not started (Gate R, added by the owner on 2026-09-27) — [evidence](docs/verification/AC-170.md)
- [ ] **AC-171** What voice may do — not started (Gate R, added by the owner on 2026-09-27) — [evidence](docs/verification/AC-171.md)
- [ ] **AC-172** One speaker at a time — not started (Gate R, added by the owner on 2026-09-27) — [evidence](docs/verification/AC-172.md)
- [ ] **AC-173** Private and bounded — not started (Gate R, added by the owner on 2026-09-27) — [evidence](docs/verification/AC-173.md)
- [ ] **AC-174** Voice in the UI — not started (Gate R, added by the owner on 2026-09-27) — [evidence](docs/verification/AC-174.md)
- [ ] **AC-175** Keeps working when things fail — not started (Gate R, added by the owner on 2026-09-27) — [evidence](docs/verification/AC-175.md)
- [ ] **AC-176** Voice Mode by voice (owner-confirmed) — not started (Gate R, added by the owner on 2026-09-27) — [evidence](docs/verification/AC-176.md)
- [ ] **AC-177** The mark shows it is hearing you — not started (Gate R, added by the owner on 2026-09-27) — [evidence](docs/verification/AC-177.md)
- [ ] **AC-178** The phone app uses the owner's mark — not started (Brand, added by the owner on 2026-09-27): the phone app's agent uses the owner's files — [evidence](docs/verification/AC-178.md)
- [ ] **AC-179** The Mac surfaces use the owner's mark — not started (Brand, added by the owner on 2026-09-27): the Mac helper's icon is built with AC-142; a menu-bar item does not exist yet — [evidence](docs/verification/AC-179.md)
- [ ] **AC-180** Spikes before lock-in — not started (Gate S, added by the owner on 2026-09-27; the goal's first step) — [evidence](docs/verification/AC-180.md)
- [ ] **AC-181** Overseer lives in the daemon — not started (Gate S, added by the owner on 2026-09-27) — [evidence](docs/verification/AC-181.md)
- [ ] **AC-182** One conversation, from home — not started (Gate S, added by the owner on 2026-09-27) — [evidence](docs/verification/AC-182.md)
- [ ] **AC-183** A digest of every agent — not started (Gate S, added by the owner on 2026-09-27) — [evidence](docs/verification/AC-183.md)
- [ ] **AC-184** Overseer reads on demand, and only reads — not started (Gate S, added by the owner on 2026-09-27) — [evidence](docs/verification/AC-184.md)
- [ ] **AC-185** A fixed set of actions, on one agent or all, each with its card — not started (Gate S, added by the owner on 2026-09-27) — [evidence](docs/verification/AC-185.md)
- [ ] **AC-186** Ask first, Steer, Auto — not started (Gate S, added by the owner on 2026-09-27) — [evidence](docs/verification/AC-186.md)
- [ ] **AC-187** Rein in: hold, release and guardrails — not started (Gate S, added by the owner on 2026-09-27) — [evidence](docs/verification/AC-187.md)
- [ ] **AC-188** Change direction — not started (Gate S, added by the owner on 2026-09-27) — [evidence](docs/verification/AC-188.md)
- [ ] **AC-189** Overseer keeps agents on task — not started (Gate S, added by the owner on 2026-09-27) — [evidence](docs/verification/AC-189.md)
- [ ] **AC-190** Agents that know about each other — not started (Gate S, added by the owner on 2026-09-27) — [evidence](docs/verification/AC-190.md)
- [ ] **AC-191** Context passed between agents — not started (Gate S, added by the owner on 2026-09-27) — [evidence](docs/verification/AC-191.md)
- [ ] **AC-192** Conflicts between agents in flight — not started (Gate S, added by the owner on 2026-09-27) — [evidence](docs/verification/AC-192.md)
- [ ] **AC-193** One agent watches another — not started (Gate S, added by the owner on 2026-09-27) — [evidence](docs/verification/AC-193.md)
- [ ] **AC-194** A watch that checks — not started (Gate S, added by the owner on 2026-09-27) — [evidence](docs/verification/AC-194.md)
- [ ] **AC-195** With Swarm: one decision-maker per swarm — not started (Gate S, added by the owner on 2026-09-27) — [evidence](docs/verification/AC-195.md)
- [ ] **AC-196** With route picking: routes, admission and metering — not started (Gate S, added by the owner on 2026-09-27) — [evidence](docs/verification/AC-196.md)
- [ ] **AC-197** Handoffs and offline — not started (Gate S, added by the owner on 2026-09-27) — [evidence](docs/verification/AC-197.md)
- [ ] **AC-198** Quiet and bounded — not started (Gate S, added by the owner on 2026-09-27) — [evidence](docs/verification/AC-198.md)
- [ ] **AC-199** Every surface — not started (Gate S, added by the owner on 2026-09-27) — [evidence](docs/verification/AC-199.md)
- [ ] **AC-200** What agents say is data — not started (Gate S, added by the owner on 2026-09-27) — [evidence](docs/verification/AC-200.md)
- [ ] **AC-201** Regression coverage — not started (Gate S, added by the owner on 2026-09-27) — [evidence](docs/verification/AC-201.md)
- [ ] **AC-202** Orchestration session (owner-confirmed) — not started (Gate S, added by the owner on 2026-09-27) — [evidence](docs/verification/AC-202.md)
<!-- ac-list:end -->

## What works today (macOS, VS Code 1.139)

- **Daemon** — SQLite state, owner-only Unix socket (versioned JSON-lines protocol), one
  supervisor process per harness run so work survives VS Code closing and daemon crashes;
  on restart the daemon reattaches or reports the session as lost, never relaunching work.
- **Harnesses** — Codex via `exec` and via the app-server transport (`codex-app`, with
  permission requests you Allow/Deny in the run panel) on a ChatGPT login; Claude Code on a
  claude.ai login (permission requests, nested subagents, follow-ups, interrupt); OpenCode
  through its real runtime (verified with a mock provider and local Ollama models); and any
  executable (generic). All live-verified on macOS except OpenCode account login. API keys
  are never forwarded to harnesses. See the [compatibility matrix](docs/compatibility.md).
- **Native children** — live child and grandchild capture for Claude Code, Codex
  (`codex-app` with `-c agents.max_depth=2`; default depth 1) and OpenCode, shown as a
  recursive tree with evidence and confidence; missing telemetry is shown as unknown.
- **Workspaces** — a new worktree per task by default, or the current checkout with its
  staged/unstaged/untracked/unsaved work recorded and preserved. Single writer per checkout;
  cleanup reports dirty files and active runs and never removes the current checkout.
- **Review** — Branch Diff's editable Monaco review opened on the selected run's worktree.
  Default comparison is **Latest run** (a snapshot taken at the start of every turn,
  including dirty and untracked files, without touching your index/stash); also since
  earlier turns, **Since task start**, **Original fork**, and any branch (merge-base or tip).
  A separate **Workspace Dirty** view always shows staged, unstaged, untracked, conflicted
  and unsaved work. **Follow** jumps to agent-reported edits across and within files and
  pauses when you scroll or select a file until you press **Resume**. Each hunk has
  **Accept** (marks it reviewed, no Git staging) and **Reject** (restores the comparison
  base through a native edit you can undo in the editor); concurrent agent edits are
  conflicts, never overwritten.
- **Conversation** — each run panel reads as a conversation: turns, collapsible tool calls
  with inputs and results, file edits that open the review at the hunk, inline permission
  requests, native children nested under the tool call that spawned them, errors (with
  **Sign in again** for expired logins) and per-turn usage; the raw event log is one tab away.
- **Overseer view** — **Open Overseer View** lays out an agents column (every repository,
  not just the open folder) and the selected run's worktree files beside its review and
  conversation; it works with the native sidebar closed.
- **Merge back** — never automatic: Overseer commits the worktree, merges the target into
  the run's branch in the worktree (conflicts go back to the same agent session), shows you
  exactly what lands, and merges into the target only after you confirm; a dirty target
  checkout is refused and left untouched. **Open PR…** instead pushes the branch and opens a
  GitHub pull request with the GitHub account VS Code is signed in to (no tokens to paste).
- **Accounts** — accounts by provider (OpenAI/ChatGPT, Anthropic/Claude, OpenCode local);
  desktop-app logins are labeled as following the app; New Task offers only compatible
  accounts. Account login only, never API keys.
- **Session restore and background agents** — reviews, run panels, comparisons, Follow
  (paused), scroll positions and the Agents tree return after reloads and restarts. Closing
  VS Code with agents running posts a macOS notification naming them (from the bundled
  Overseer notifier app; clicking it opens the Overseer view; **Test Notification** checks it);
  **Stop Agents and Daemon** stops everything on request.
- **Audio Mode** — off until you turn it on. The daemon plays one short cue when a top-level
  agent starts, completes or needs you, also with VS Code closed and never twice because
  several windows are open; children, tool calls and progress stay silent, and needs that
  arrive together play one cue. Tracks: twelve bundled Reactor synth cues (31,488 bytes), a
  macOS system voice, or your own private Commander folder, played where it is. Playback is
  macOS only for now. See the [design](docs/rfcs/audio-mode.md).

## Build and install (macOS)

Requirements: Rust 1.89+ (`cargo`), Node 24, Git, the VS Code `code` CLI, and on macOS the Xcode command-line tools (`swiftc`, for the bundled Overseer notifier app).

```bash
git clone https://github.com/beelol/overseer.git && cd overseer
npm ci --prefix extension/branch-diff/tooling/review --ignore-scripts
npm ci --prefix extension/tooling/vsce --ignore-scripts
node extension/scripts/package.js
code --install-extension extension/overseer-0.1.0.vsix
```

`package.js` builds the review bundle, builds `overseerd` in release mode
(`target/release/overseerd`; a few dead-code warnings are expected), copies it into the
extension as `bin/overseerd-darwin-arm64` (or your platform/arch), and writes the VSIX.
Reload VS Code; the **Overseer** mark appears in the activity bar. The extension starts
the daemon on demand (detached), so agents keep running after you close VS Code.

Run the checks:

```bash
cargo test
node test/unit/webview-scripts.js
```

Packaged-UI scenarios (open a real, isolated VS Code window; see [test/ui](test/ui)). The
free ones use fixtures and mocks: `trust`, `main`, `review`, `restore`, `conversation`,
`hunks`, `accounts`, `signin`, `center`, `theme` and `perf` (10 minutes). Scenarios whose
header says LIVE spend a few tiny paid prompts.

```bash
node test/ui/scenario-main.js
```

## Using it

1. **Accounts**: **Add Account…** picks a provider and a name, then **Sign In** runs that
   provider's own login in a terminal (ChatGPT in the browser or with a device code). Existing
   desktop logins appear as accounts that follow the app. Overseer never asks for API keys
   and never signs out a desktop login; **Remove Account…** deletes only that account's folder.
2. **New Task…**: a form of tiles: repository (any repository, not only the open folder),
   harness with capability hints, a compatible signed-in account, *New worktree* or *Current
   checkout*, start branch, optional model and approval policy, and the prompt (⌘Enter
   starts). **Start Task with Quick Picks…** does the same with pickers.
3. **Open Overseer View** for the full-page layout, or use the Agents tree. Selecting a run
   opens its **Review** and its **conversation** with **Send follow-up**, **Interrupt**,
   permission **Allow/Deny**, **Raw output** and **Merge back…**. Controls a harness cannot
   support are disabled with the reason.
4. In the review, click the comparison label (base icon) to switch comparisons; use each
   hunk's **✓ Accept** / **↶ Reject**, or edit the working-tree side and **Save**. **Open in
   Native Diff** gives full editor features including undo/redo.
5. **Merge back…** when a run is done: prepare, review exactly what lands, confirm. Or
   **Open PR…** to push the branch and open a GitHub pull request instead.
6. Agents keep running when VS Code closes (you get a notification). **Stop Agents and
   Daemon** (Agents view menu) stops them all after confirmation.
7. **Audio Mode and Reactor Cues…** (Agents view menu or the command palette) turns the cues
   on or off, picks the track and the system voice, imports a private Commander folder and
   previews a cue.

### In a terminal: `overseer-tui`

A keyboard-first view of the same agents, live from the same daemon ([design](docs/rfcs/tui.md)):
nine per page (page 1 is the newest nine), each tile streaming its agent's conversation.

```bash
cargo build --release -p overseer-tui
```

```bash
target/release/overseer-tui
```

It finds the daemon VS Code uses (or pass `--daemon PATH`), starts it if needed, and quitting
leaves every agent running. Keys: arrows or `hjkl` move, `1`–`9` jump, `]`/`[` page, `i` or
Enter messages the focused agent, `z` zooms with scrollback, `a`/`d` answer a permission, `w`
jumps to the next agent waiting for you, `x` interrupts, `n` starts a new agent, `f` filters,
`/` searches, `v` shows an agent's changes and diffs, `M` merges it back (one confirmation per
step), `P` opens a GitHub pull request with your `gh`, `C` removes a finished worktree, `A` lists accounts and signs them in, `S` opens Audio Mode (on or off, track, voice, a private
Commander folder, preview; the daemon plays, and the terminal bell rings only when it does not), `X` stops every
agent and the daemon, `?` lists every key, `q` quits.

## Recovery

- State lives in `~/Library/Application Support/Overseer` (`overseer.sqlite`, per-run
  output under `runs/`, worktrees under `worktrees/`, profiles under `profiles/`). Linux
  uses `$XDG_DATA_HOME/overseer`. `OVERSEER_HOME` overrides it.
- The daemon binary is `target/release/overseerd` in a build, or
  `~/.vscode/extensions/beelol.overseer-0.1.0/bin/overseerd-darwin-arm64` once installed.
  `overseerd serve` runs it in the foreground without VS Code (the extension normally starts
  it detached). `overseerd ctl state` prints the daemon state; `overseerd ctl daemon.shutdown`
  stops the daemon (runs continue under their supervisors and are reattached next start).
- If a supervisor is killed, the run is marked `disconnected`/lost with the reason; nothing
  is relaunched automatically. Snapshot refs live under `refs/overseer/snapshots/*` in your
  repository and can be deleted with `git for-each-ref --format='%(refname)' refs/overseer | xargs -n1 git update-ref -d`.
- Worktrees are only removed by the explicit **Clean Up Worktree…** action (branch kept).

## Follow-ups

Unchecked criteria keep their AC in the [RFC](docs/overseer-rfc.md); this list only tracks
the owner action or decision each one needs.

- [ ] [AC-41](docs/verification/AC-41.md) (Linux verification (deferred by owner)): Needs a Linux machine with VS Code and the harnesses. Next: run the README build, `cargo test`, and the UI scenarios there.
- [ ] [AC-53](docs/verification/AC-53.md) (Fixed Claude accounts): Needs a second Claude account (the owner has one today); not to be tested yet (owner, 2026-09-25). Next: check whether Claude keeps a separate Keychain entry per CLAUDE_CONFIG_DIR, otherwise add Overseer-managed Claude credentials (docs/rfcs/claude-credentials.md); then Add Account → Anthropic → Sign In with it, Sign Out and Sign In again while a Claude run on the desktop login keeps working; confirm both identities and the macOS Keychain entries stay separate.
- [ ] [AC-64](docs/verification/AC-64.md) (Default-to-Overseer session (owner-confirmed)): Owner action after the design review (AC-66): work for an hour using only Overseer for Claude Code and Codex; log friction.
- [ ] [AC-66](docs/verification/AC-66.md) (Design review against references (owner-confirmed)): Waiting for the owner's marks. Next: read the marks from the page, change each marked item, republish the page and ask for confirmation.
- [ ] [AC-83](docs/verification/AC-83.md) (Offline is not an outage): Owner: with the daemon running, turn Wi-Fi off for a minute and on again; the daemon's event log (`overseerd ctl events.list`, kind `connection`) then shows offline within 10 s of the system signal and online after the checks agree.
- [ ] [AC-84](docs/verification/AC-84.md) (Fail over to the best working provider): Owner: sign Claude Code in (`claude auth login` in a terminal), or name an Overseer Claude profile the check may use. Then: `node test/local/handoff-live.js failover` (two tiny turns on Claude's smallest model; Codex never reaches its provider).
- [ ] [AC-87](docs/verification/AC-87.md) (Verified local catalogue, Qwen coders first): Next: the unverified mark in the packaged UI (AC-94).
- [ ] [AC-89](docs/verification/AC-89.md) (Download models only when allowed): Next: a progress card in the chat of the agent whose pick needs the download, and the one-time prefetch offer.
- [ ] [AC-97](docs/verification/AC-97.md) (Offline session (owner-confirmed)): Not started (Gate L, added by the owner on 2026-09-26; design in docs/rfcs/offline-mode.md; built in its own worktree and pull request).
- [ ] [AC-114](docs/verification/AC-114.md) (Gate K in the owner's VS Code (owner-confirmed)): Not started (Gate K follow-up from the owner's marks on 2026-09-26).
- [ ] [AC-115](docs/verification/AC-115.md) (Feasibility and reuse before lock-in): Not started (Gate N, added by the owner on 2026-09-26; design in docs/rfcs/phone-remote.md; built in its own worktree and pull request).
- [ ] [AC-116](docs/verification/AC-116.md) (A gateway switched on and off on the desktop): Not started (Gate N, added by the owner on 2026-09-26; design in docs/rfcs/phone-remote.md; built in its own worktree and pull request).
- [ ] [AC-117](docs/verification/AC-117.md) (Pairing needs the Mac): Not started (Gate N, added by the owner on 2026-09-26; design in docs/rfcs/phone-remote.md; built in its own worktree and pull request).
- [ ] [AC-118](docs/verification/AC-118.md) (Encrypted and mutually authenticated): Not started (Gate N, added by the owner on 2026-09-26; design in docs/rfcs/phone-remote.md; built in its own worktree and pull request).
- [ ] [AC-119](docs/verification/AC-119.md) (Devices, scopes and revoking): Not started (Gate N, added by the owner on 2026-09-26; design in docs/rfcs/phone-remote.md; built in its own worktree and pull request).
- [ ] [AC-120](docs/verification/AC-120.md) (Found on the network): Not started (Gate N, added by the owner on 2026-09-26; design in docs/rfcs/phone-remote.md; built in its own worktree and pull request).
- [ ] [AC-121](docs/verification/AC-121.md) (Never lose the session): Not started (Gate N, added by the owner on 2026-09-26; design in docs/rfcs/phone-remote.md; built in its own worktree and pull request).
- [ ] [AC-122](docs/verification/AC-122.md) (Sent exactly once): Not started (Gate N, added by the owner on 2026-09-26; design in docs/rfcs/phone-remote.md; built in its own worktree and pull request).
- [ ] [AC-123](docs/verification/AC-123.md) (The Mac stays awake while it matters): Not started (Gate N, added by the owner on 2026-09-26; design in docs/rfcs/phone-remote.md; built in its own worktree and pull request).
- [ ] [AC-124](docs/verification/AC-124.md) (See every agent): Not started (Gate N, added by the owner on 2026-09-26; design in docs/rfcs/phone-remote.md; built in its own worktree and pull request).
- [ ] [AC-125](docs/verification/AC-125.md) (Control every agent): Not started (Gate N, added by the owner on 2026-09-26; design in docs/rfcs/phone-remote.md; built in its own worktree and pull request).
- [ ] [AC-126](docs/verification/AC-126.md) (Review on the phone): Not started (Gate N, added by the owner on 2026-09-26; design in docs/rfcs/phone-remote.md; built in its own worktree and pull request).
- [ ] [AC-127](docs/verification/AC-127.md) (Everything else Overseer has): Not started (Gate N, added by the owner on 2026-09-26; design in docs/rfcs/phone-remote.md; built in its own worktree and pull request).
- [ ] [AC-128](docs/verification/AC-128.md) (Talk to Overseer from the phone): Not started (Gate N, added by the owner on 2026-09-26; design in docs/rfcs/phone-remote.md; built in its own worktree and pull request).
- [ ] [AC-129](docs/verification/AC-129.md) (Needs-you notifications you can switch): Not started (Gate N, added by the owner on 2026-09-26; design in docs/rfcs/phone-remote.md; built in its own worktree and pull request).
- [ ] [AC-130](docs/verification/AC-130.md) (Safe without friction): Not started (Gate N, added by the owner on 2026-09-26; design in docs/rfcs/phone-remote.md; built in its own worktree and pull request).
- [ ] [AC-131](docs/verification/AC-131.md) (One app, iOS and Android, that looks like Overseer): Not started (Gate N, added by the owner on 2026-09-26; design in docs/rfcs/phone-remote.md; built in its own worktree and pull request).
- [ ] [AC-132](docs/verification/AC-132.md) (Regression coverage for the phone): Not started (Gate N, added by the owner on 2026-09-26; design in docs/rfcs/phone-remote.md; built in its own worktree and pull request).
- [ ] [AC-133](docs/verification/AC-133.md) (Phone session (owner-confirmed)): Not started (Gate N, added by the owner on 2026-09-26; design in docs/rfcs/phone-remote.md; built in its own worktree and pull request).
- [ ] [AC-134](docs/verification/AC-134.md) (Platform behaviour behind generic interfaces): Not started (Gate N, added by the owner on 2026-09-26; design in docs/rfcs/phone-remote.md; built in its own worktree and pull request).
- [ ] [AC-135](docs/verification/AC-135.md) (Hyper fast): Not started (Gate N, added by the owner on 2026-09-26; design in docs/rfcs/phone-remote.md; built in its own worktree and pull request).
- [ ] [AC-136](docs/verification/AC-136.md) (The door): Not started (Gate N, added by the owner on 2026-09-26; design in docs/rfcs/phone-remote.md; built in its own worktree and pull request).
- [ ] [AC-137](docs/verification/AC-137.md) (Motion throughout): Not started (Gate N, added by the owner on 2026-09-26; design in docs/rfcs/phone-remote.md; built in its own worktree and pull request).
- [ ] [AC-141](docs/verification/AC-141.md) (Pair once): Not started (Gate N, added by the owner on 2026-09-26; design in docs/rfcs/phone-remote.md; built in its own worktree and pull request).
- [ ] [AC-146](docs/verification/AC-146.md) (Reconcile and merge the work in flight): Not started (Gate P, added by the owner on 2026-09-27).
- [ ] [AC-148](docs/verification/AC-148.md) (Checks on every pull request): Not started (Gate P, added by the owner on 2026-09-27).
- [ ] [AC-149](docs/verification/AC-149.md) (A steady UI suite): Needs a quiet machine (no other agent running UI scenarios) for the three-in-a-row runs and the p95.
- [ ] [AC-151](docs/verification/AC-151.md) (Every live scenario rerun on the current build): Not started (Gate P, added by the owner on 2026-09-27).
- [ ] [AC-152](docs/verification/AC-152.md) (Performance re-measured): Not started (Gate P, added by the owner on 2026-09-27).
- [ ] [AC-156](docs/verification/AC-156.md) (Every agent works from the same rules): Not started (Gate Q, added by the owner on 2026-09-27).
- [ ] [AC-157](docs/verification/AC-157.md) (Oversee the other agents): Not started (Gate Q, added by the owner on 2026-09-27).
- [ ] [AC-158](docs/verification/AC-158.md) (Gate M's theme and immersive look are back in scope): Not started (Gate Q, added by the owner on 2026-09-27).
- [ ] [AC-159](docs/verification/AC-159.md) (The toolchain works without Xcode's license): Not started (Gate Q, added by the owner on 2026-09-27).
- [ ] [AC-160](docs/verification/AC-160.md) (Owner actions in one place): Not started (Gate Q, added by the owner on 2026-09-27).
- [ ] [AC-161](docs/verification/AC-161.md) (Everything merged into one main): Not started (Gate Q, added by the owner on 2026-09-27).
- [ ] [AC-162](docs/verification/AC-162.md) (Voice spike before lock-in): Not started (Gate R, added by the owner on 2026-09-27; the goal's first step).
- [ ] [AC-163](docs/verification/AC-163.md) (Owned by the daemon, heard in Rust, off until asked): Not started (Gate R, added by the owner on 2026-09-27).
- [ ] [AC-164](docs/verification/AC-164.md) (Holds the floor): Not started (Gate R, added by the owner on 2026-09-27).
- [ ] [AC-165](docs/verification/AC-165.md) (A quick answer that it is working on it): Not started (Gate R, added by the owner on 2026-09-27).
- [ ] [AC-166](docs/verification/AC-166.md) (The right agents, from context): Not started (Gate R, added by the owner on 2026-09-27).
- [ ] [AC-167](docs/verification/AC-167.md) (Redirect without trampling): Not started (Gate R, added by the owner on 2026-09-27).
- [ ] [AC-168](docs/verification/AC-168.md) (New agents from a request): Not started (Gate R, added by the owner on 2026-09-27).
- [ ] [AC-169](docs/verification/AC-169.md) (Evidence for every word sent): Not started (Gate R, added by the owner on 2026-09-27).
- [ ] [AC-170](docs/verification/AC-170.md) (Correct and cancel): Not started (Gate R, added by the owner on 2026-09-27).
- [ ] [AC-171](docs/verification/AC-171.md) (What voice may do): Not started (Gate R, added by the owner on 2026-09-27).
- [ ] [AC-172](docs/verification/AC-172.md) (One speaker at a time): Not started (Gate R, added by the owner on 2026-09-27).
- [ ] [AC-173](docs/verification/AC-173.md) (Private and bounded): Not started (Gate R, added by the owner on 2026-09-27).
- [ ] [AC-174](docs/verification/AC-174.md) (Voice in the UI): Not started (Gate R, added by the owner on 2026-09-27).
- [ ] [AC-175](docs/verification/AC-175.md) (Keeps working when things fail): Not started (Gate R, added by the owner on 2026-09-27).
- [ ] [AC-176](docs/verification/AC-176.md) (Voice Mode by voice (owner-confirmed)): Not started (Gate R, added by the owner on 2026-09-27).
- [ ] [AC-177](docs/verification/AC-177.md) (The mark shows it is hearing you): Not started (Gate R, added by the owner on 2026-09-27).
- [ ] [AC-178](docs/verification/AC-178.md) (The phone app uses the owner's mark): Not started: the phone app's agent (Gate N) replaces its placeholder marks with the owner's files in docs/design/brand/.
- [ ] [AC-179](docs/verification/AC-179.md) (The Mac surfaces use the owner's mark): Not started: verified with AC-142's merge for the helper; the menu-bar part waits for a menu-bar item.
- [ ] [AC-180](docs/verification/AC-180.md) (Spikes before lock-in): Not started (Gate S, added by the owner on 2026-09-27; the goal's first step).
- [ ] [AC-181](docs/verification/AC-181.md) (Overseer lives in the daemon): Not started (Gate S, added by the owner on 2026-09-27).
- [ ] [AC-182](docs/verification/AC-182.md) (One conversation, from home): Not started (Gate S, added by the owner on 2026-09-27).
- [ ] [AC-183](docs/verification/AC-183.md) (A digest of every agent): Not started (Gate S, added by the owner on 2026-09-27).
- [ ] [AC-184](docs/verification/AC-184.md) (Overseer reads on demand, and only reads): Not started (Gate S, added by the owner on 2026-09-27).
- [ ] [AC-185](docs/verification/AC-185.md) (A fixed set of actions, on one agent or all, each with its card): Not started (Gate S, added by the owner on 2026-09-27).
- [ ] [AC-186](docs/verification/AC-186.md) (Ask first, Steer, Auto): Not started (Gate S, added by the owner on 2026-09-27).
- [ ] [AC-187](docs/verification/AC-187.md) (Rein in: hold, release and guardrails): Not started (Gate S, added by the owner on 2026-09-27).
- [ ] [AC-188](docs/verification/AC-188.md) (Change direction): Not started (Gate S, added by the owner on 2026-09-27).
- [ ] [AC-189](docs/verification/AC-189.md) (Overseer keeps agents on task): Not started (Gate S, added by the owner on 2026-09-27).
- [ ] [AC-190](docs/verification/AC-190.md) (Agents that know about each other): Not started (Gate S, added by the owner on 2026-09-27).
- [ ] [AC-191](docs/verification/AC-191.md) (Context passed between agents): Not started (Gate S, added by the owner on 2026-09-27).
- [ ] [AC-192](docs/verification/AC-192.md) (Conflicts between agents in flight): Not started (Gate S, added by the owner on 2026-09-27).
- [ ] [AC-193](docs/verification/AC-193.md) (One agent watches another): Not started (Gate S, added by the owner on 2026-09-27).
- [ ] [AC-194](docs/verification/AC-194.md) (A watch that checks): Not started (Gate S, added by the owner on 2026-09-27).
- [ ] [AC-195](docs/verification/AC-195.md) (With Swarm: one decision-maker per swarm): Not started (Gate S, added by the owner on 2026-09-27).
- [ ] [AC-196](docs/verification/AC-196.md) (With route picking: routes, admission and metering): Not started (Gate S, added by the owner on 2026-09-27).
- [ ] [AC-197](docs/verification/AC-197.md) (Handoffs and offline): Not started (Gate S, added by the owner on 2026-09-27).
- [ ] [AC-198](docs/verification/AC-198.md) (Quiet and bounded): Not started (Gate S, added by the owner on 2026-09-27).
- [ ] [AC-199](docs/verification/AC-199.md) (Every surface): Not started (Gate S, added by the owner on 2026-09-27).
- [ ] [AC-200](docs/verification/AC-200.md) (What agents say is data): Not started (Gate S, added by the owner on 2026-09-27).
- [ ] [AC-201](docs/verification/AC-201.md) (Regression coverage): Not started (Gate S, added by the owner on 2026-09-27).
- [ ] [AC-202](docs/verification/AC-202.md) (Orchestration session (owner-confirmed)): Not started (Gate S, added by the owner on 2026-09-27).
- [ ] Decide a retention policy for snapshot refs under `refs/overseer/snapshots/*` (they accumulate per turn; harmless but unbounded). Clearly labeled follow-up; no AC covers it.
- [ ] Decide whether the *existing login* Codex profile should be discouraged: on this machine `~/.codex` is shared with the ChatGPT desktop app and switched accounts during the session (see [AC-02](docs/verification/AC-02.md)). Clearly labeled follow-up.
- [ ] Remove or update the stale `~/Library/pnpm/codex` (0.1.x) on PATH; Overseer ignores it in favour of the ChatGPT.app bundle. Owner environment note.
- [ ] VS Code on this machine trusts `/` in its workspace-trust list, so folders never open in Restricted Mode; the trust test uses an empty window ([AC-08](docs/verification/AC-08.md)). Owner environment note.

## Project documents

- [RFC and authoritative acceptance checklist](docs/overseer-rfc.md)
- [Verification ledger and evidence](docs/verification/README.md)
- [Harness compatibility](docs/compatibility.md)
- [Side RFC: simple account governance](docs/rfcs/account-governance.md)
- [Side RFC: native Overseer notifications on macOS](docs/rfcs/native-notifications.md)
- [Side RFC: Overseer-managed Claude credentials](docs/rfcs/claude-credentials.md)
- [Side RFC: daily-driver orchestrator UI](docs/rfcs/orchestrator-ui.md)
- [Side RFC: Continuity — offline mode and local models](docs/rfcs/offline-mode.md)
- [Side RFC: terminal UI (`overseer-tui`)](docs/rfcs/tui.md)
- [Side RFC: Audio Mode — opt-in cues from the daemon](docs/rfcs/audio-mode.md) and its [prepared goal](docs/rfcs/audio-mode-goal.md)
- [Side RFC: phone remote on the same network](docs/rfcs/phone-remote.md) and its [prepared goal](docs/rfcs/phone-remote-goal.md)
- [Side RFC: Voice Mode — talk to Overseer, redirect every agent](docs/rfcs/voice-mode.md)
- [Inspected sources and reuse assessment](docs/source-assessment.md)

Design targets macOS and Linux; only macOS is verified. Auto routing beyond the offline fallback ([Gate L](docs/rfcs/offline-mode.md)), a relay for phone access away from the local network (after [Gate N](docs/rfcs/phone-remote.md)), VSCodium,
Windows/Remote SSH and review comments sent to agents are later milestones.
