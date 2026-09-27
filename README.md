# Overseer

A local agent orchestration daemon in Rust (`overseerd`) with a VS Code extension for
account-based agent runs, recursive native-child visibility, and live editable worktree
review built on [Branch Diff](https://github.com/beelol/branch-diff).

**Status: usable macOS milestone — not the complete product.** Verified acceptance
criteria: **77 / 142** · **4** partial (see [ledger](docs/verification/README.md)). Unverified:
AC-41, AC-53, AC-64, AC-66, AC-81, AC-82, AC-83, AC-84, AC-85, AC-86, AC-87, AC-88, AC-89, AC-90, AC-91, AC-92, AC-93, AC-94, AC-95, AC-96, AC-97, AC-98, AC-99, AC-100, AC-101, AC-102, AC-103, AC-104, AC-105, AC-106, AC-107, AC-108, AC-109, AC-110, AC-111, AC-112, AC-113, AC-114, AC-115, AC-116, AC-117, AC-118, AC-119, AC-120, AC-121, AC-122, AC-123, AC-124, AC-125, AC-126, AC-127, AC-128, AC-129, AC-130, AC-131, AC-132, AC-133, AC-134, AC-135, AC-136, AC-137, AC-138, AC-139, AC-140, AC-141. The biggest gaps are the daily-driver UI (Gate J partials, and Gate K, AC-67 to AC-82: the native side bar
with chat and diff side by side, added by the owner on 2026-09-26; [design](docs/rfcs/orchestrator-ui.md#gate-k-layout)), Continuity, the offline mode with local models (Gate L, AC-83 to AC-98 and AC-138 to AC-140, added by the owner on 2026-09-26; [design](docs/rfcs/offline-mode.md)), Overseer as the whole surface (Gate M, AC-99 to AC-108, added by the owner on 2026-09-26: the review as the home for files, nothing shown twice, a less VS Code-like editor area with a bold Overseer theme, a grid built by dragging, and a chat with Overseer itself; [design](docs/rfcs/orchestrator-ui.md#gate-m-overseer-as-the-whole-surface)), the phone remote on the same network (Gate N, AC-115 to AC-137 and AC-141, added by the owner on 2026-09-26: a hyper fast iOS and Android app that sees and controls every agent through a gateway in the daemon, paired once and built on the simulators first; [design](docs/rfcs/phone-remote.md)), fixed Claude accounts (AC-53, partial;
[design](docs/rfcs/claude-credentials.md)), which wait for a second Claude account, and Linux (AC-41),
which is out of scope for now. The full list is under [Acceptance criteria](#acceptance-criteria); next actions are in [Follow-ups](#follow-ups).

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
- [ ] **AC-81** Gate J still holds — ◐ partial: every fixture scenario passes against the Gate K build: the Gate J scenarios (Gate K audit with a new baseline, chat, parity, composer, grid, dashboard, keyboard with the shortcuts also from the side bar, history, usage, look, theme) and the earlier ones (review, main, center, conversation, files, hunks, pr, notify, signin, accounts, trust); the text budget re-measured per view is no higher than Gate J (agents 225, chat 1,063, files 59, review 150, grid 993, new agent 133, accounts 189) / deferred: the live Gate J scenario (scenario-live-gatej.js: Claude and both ChatGPT accounts, several paid turns) was not rerun on the Gate K build; its fixture counterparts pass and the Codex usage part was rerun live (AC-62) — [evidence](docs/verification/AC-81.md)
- [ ] **AC-82** Gate K design review (owner-confirmed) — ◐ partial: the review page shows every view in Gate J and Gate K in both Overseer themes; the owner marked all 19 views on 2026-09-26: 16 Looks right (editor area and agents list "gate k looking great", chat, chat beside a diff, arrangement, review, scopes, follow, endings, grid, new agent, Needs you, take out, remembered place, themes, and Overall) and 3 Needs work / deferred: the three Needs work items changed and shown again, and the owner's confirmation after them — [evidence](docs/verification/AC-82.md)
- [ ] **AC-83** Offline is not an outage — not started (Gate L, added by the owner on 2026-09-26) — [evidence](docs/verification/AC-83.md)
- [ ] **AC-84** Fail over to the best working provider — not started (Gate L, added by the owner on 2026-09-26) — [evidence](docs/verification/AC-84.md)
- [ ] **AC-85** Local inventory read from the machine — not started (Gate L, added by the owner on 2026-09-26) — [evidence](docs/verification/AC-85.md)
- [ ] **AC-86** Memory budget and fit — not started (Gate L, added by the owner on 2026-09-26) — [evidence](docs/verification/AC-86.md)
- [ ] **AC-87** Verified local catalogue, Qwen coders first — not started (Gate L, added by the owner on 2026-09-26) — [evidence](docs/verification/AC-87.md)
- [ ] **AC-88** Settings the daemon enforces — not started (Gate L, added by the owner on 2026-09-26) — [evidence](docs/verification/AC-88.md)
- [ ] **AC-89** Download models only when allowed — not started (Gate L, added by the owner on 2026-09-26) — [evidence](docs/verification/AC-89.md)
- [ ] **AC-90** Install and run Ollama only when allowed — not started (Gate L, added by the owner on 2026-09-26) — [evidence](docs/verification/AC-90.md)
- [ ] **AC-91** Transition to local when offline — not started (Gate L, added by the owner on 2026-09-26) — [evidence](docs/verification/AC-91.md)
- [ ] **AC-92** Wait and retry, never fail (for 36 hours) — not started (Gate L, added by the owner on 2026-09-26) — [evidence](docs/verification/AC-92.md)
- [ ] **AC-93** Back online — not started (Gate L, added by the owner on 2026-09-26) — [evidence](docs/verification/AC-93.md)
- [ ] **AC-94** Local models as a first-class choice — not started (Gate L, added by the owner on 2026-09-26) — [evidence](docs/verification/AC-94.md)
- [ ] **AC-95** Honest offline UI — not started (Gate L, added by the owner on 2026-09-26) — [evidence](docs/verification/AC-95.md)
- [ ] **AC-96** Several local agents — not started (Gate L, added by the owner on 2026-09-26) — [evidence](docs/verification/AC-96.md)
- [ ] **AC-97** Offline session (owner-confirmed) — not started (Gate L, added by the owner on 2026-09-26) — [evidence](docs/verification/AC-97.md)
- [ ] **AC-98** On by default, explained once — not started (Gate L, added by the owner on 2026-09-26) — [evidence](docs/verification/AC-98.md)
- [ ] **AC-99** The review is where files live — not started (Gate M, added by the owner on 2026-09-26) — [evidence](docs/verification/AC-99.md)
- [ ] **AC-100** Nothing shown twice — not started (Gate M, added by the owner on 2026-09-26) — [evidence](docs/verification/AC-100.md)
- [ ] **AC-101** Overseer's own reviewer — not started (Gate M, added by the owner on 2026-09-26) — [evidence](docs/verification/AC-101.md)
- [ ] **AC-102** An immersive editor area — not started (Gate M, added by the owner on 2026-09-26) — [evidence](docs/verification/AC-102.md)
- [ ] **AC-103** The Overseer theme — not started (Gate M, added by the owner on 2026-09-26) — [evidence](docs/verification/AC-103.md)
- [ ] **AC-104** Build the grid by dragging — not started (Gate M, added by the owner on 2026-09-26) — [evidence](docs/verification/AC-104.md)
- [ ] **AC-105** Track an agent from the grid — not started (Gate M, added by the owner on 2026-09-26) — [evidence](docs/verification/AC-105.md)
- [ ] **AC-106** Never lose track of windows — not started (Gate M, added by the owner on 2026-09-26) — [evidence](docs/verification/AC-106.md)
- [ ] **AC-107** Talk to Overseer — not started (Gate M, added by the owner on 2026-09-26) — [evidence](docs/verification/AC-107.md)
- [ ] **AC-108** Gate M design review (owner-confirmed) — not started (Gate M, added by the owner on 2026-09-26) — [evidence](docs/verification/AC-108.md)
- [ ] **AC-109** A composer that does not wrap — not started (Gate K follow-up from the owner's marks) — [evidence](docs/verification/AC-109.md)
- [ ] **AC-110** Account names read once — not started (Gate K follow-up from the owner's marks) — [evidence](docs/verification/AC-110.md)
- [ ] **AC-111** The composer says what's next — not started (Gate K follow-up from the owner's marks) — [evidence](docs/verification/AC-111.md)
- [ ] **AC-112** Search you can see — not started (Gate K follow-up from the owner's marks) — [evidence](docs/verification/AC-112.md)
- [ ] **AC-113** No empty grid — not started (Gate K follow-up from the owner's marks) — [evidence](docs/verification/AC-113.md)
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
- [ ] **AC-138** Permission modes carry over — not started (Gate L, added by the owner on 2026-09-26) — [evidence](docs/verification/AC-138.md)
- [ ] **AC-139** OpenCode session transport spike — not started (Gate L, added by the owner on 2026-09-26; the goal's first step) — [evidence](docs/verification/AC-139.md)
- [ ] **AC-140** Memory safety guard — not started (Gate L, added by the owner on 2026-09-26) — [evidence](docs/verification/AC-140.md)
- [ ] **AC-141** Pair once — not started (Gate N, added by the owner on 2026-09-26) — [evidence](docs/verification/AC-141.md)
- [x] **AC-142** Opt-in audio cues owned by the daemon — [evidence](docs/verification/AC-142.md)
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
Reload VS Code; an **Overseer** (eye) icon appears in the activity bar. The extension starts
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
step), `P` opens a GitHub pull request with your `gh`, `C` removes a finished worktree, `A` lists accounts and signs them in, `X` stops every
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
- [ ] [AC-81](docs/verification/AC-81.md) (Gate J still holds): Next: rerun scenario-live-gatej.js on the Gate K build when paid turns on both ChatGPT accounts are wanted.
- [ ] [AC-82](docs/verification/AC-82.md) (Gate K design review (owner-confirmed)): Next: the three Needs work items are AC-109 to AC-113 (after the Gate K merge, #7); show them again on the page and get the owner's confirmation.
- [ ] [AC-83](docs/verification/AC-83.md) (Offline is not an outage): Not started (Gate L, added by the owner on 2026-09-26; design in docs/rfcs/offline-mode.md; built in its own worktree and pull request).
- [ ] [AC-84](docs/verification/AC-84.md) (Fail over to the best working provider): Not started (Gate L, added by the owner on 2026-09-26; design in docs/rfcs/offline-mode.md; built in its own worktree and pull request).
- [ ] [AC-85](docs/verification/AC-85.md) (Local inventory read from the machine): Not started (Gate L, added by the owner on 2026-09-26; design in docs/rfcs/offline-mode.md; built in its own worktree and pull request).
- [ ] [AC-86](docs/verification/AC-86.md) (Memory budget and fit): Not started (Gate L, added by the owner on 2026-09-26; design in docs/rfcs/offline-mode.md; built in its own worktree and pull request).
- [ ] [AC-87](docs/verification/AC-87.md) (Verified local catalogue, Qwen coders first): Not started (Gate L, added by the owner on 2026-09-26; design in docs/rfcs/offline-mode.md; built in its own worktree and pull request).
- [ ] [AC-88](docs/verification/AC-88.md) (Settings the daemon enforces): Not started (Gate L, added by the owner on 2026-09-26; design in docs/rfcs/offline-mode.md; built in its own worktree and pull request).
- [ ] [AC-89](docs/verification/AC-89.md) (Download models only when allowed): Not started (Gate L, added by the owner on 2026-09-26; design in docs/rfcs/offline-mode.md; built in its own worktree and pull request).
- [ ] [AC-90](docs/verification/AC-90.md) (Install and run Ollama only when allowed): Not started (Gate L, added by the owner on 2026-09-26; design in docs/rfcs/offline-mode.md; built in its own worktree and pull request).
- [ ] [AC-91](docs/verification/AC-91.md) (Transition to local when offline): Not started (Gate L, added by the owner on 2026-09-26; design in docs/rfcs/offline-mode.md; built in its own worktree and pull request).
- [ ] [AC-92](docs/verification/AC-92.md) (Wait and retry, never fail (for 36 hours)): Not started (Gate L, added by the owner on 2026-09-26; design in docs/rfcs/offline-mode.md; built in its own worktree and pull request).
- [ ] [AC-93](docs/verification/AC-93.md) (Back online): Not started (Gate L, added by the owner on 2026-09-26; design in docs/rfcs/offline-mode.md; built in its own worktree and pull request).
- [ ] [AC-94](docs/verification/AC-94.md) (Local models as a first-class choice): Not started (Gate L, added by the owner on 2026-09-26; design in docs/rfcs/offline-mode.md; built in its own worktree and pull request).
- [ ] [AC-95](docs/verification/AC-95.md) (Honest offline UI): Not started (Gate L, added by the owner on 2026-09-26; design in docs/rfcs/offline-mode.md; built in its own worktree and pull request).
- [ ] [AC-96](docs/verification/AC-96.md) (Several local agents): Not started (Gate L, added by the owner on 2026-09-26; design in docs/rfcs/offline-mode.md; built in its own worktree and pull request).
- [ ] [AC-97](docs/verification/AC-97.md) (Offline session (owner-confirmed)): Not started (Gate L, added by the owner on 2026-09-26; design in docs/rfcs/offline-mode.md; built in its own worktree and pull request).
- [ ] [AC-98](docs/verification/AC-98.md) (On by default, explained once): Not started (Gate L, added by the owner on 2026-09-26; design in docs/rfcs/offline-mode.md; built in its own worktree and pull request).
- [ ] [AC-99](docs/verification/AC-99.md) (The review is where files live): Not started (Gate M, added by the owner on 2026-09-26; built in its own pull request).
- [ ] [AC-100](docs/verification/AC-100.md) (Nothing shown twice): Not started (Gate M, added by the owner on 2026-09-26; built in its own pull request).
- [ ] [AC-101](docs/verification/AC-101.md) (Overseer's own reviewer): Not started (Gate M, added by the owner on 2026-09-26; built in its own pull request).
- [ ] [AC-102](docs/verification/AC-102.md) (An immersive editor area): Not started (Gate M, added by the owner on 2026-09-26; built in its own pull request).
- [ ] [AC-103](docs/verification/AC-103.md) (The Overseer theme): Not started (Gate M, added by the owner on 2026-09-26; built in its own pull request).
- [ ] [AC-104](docs/verification/AC-104.md) (Build the grid by dragging): Not started (Gate M, added by the owner on 2026-09-26; built in its own pull request).
- [ ] [AC-105](docs/verification/AC-105.md) (Track an agent from the grid): Not started (Gate M, added by the owner on 2026-09-26; built in its own pull request).
- [ ] [AC-106](docs/verification/AC-106.md) (Never lose track of windows): Not started (Gate M, added by the owner on 2026-09-26; built in its own pull request).
- [ ] [AC-107](docs/verification/AC-107.md) (Talk to Overseer): Not started (Gate M, added by the owner on 2026-09-26; built in its own pull request).
- [ ] [AC-108](docs/verification/AC-108.md) (Gate M design review (owner-confirmed)): Not started (Gate M, added by the owner on 2026-09-26; built in its own pull request).
- [ ] [AC-109](docs/verification/AC-109.md) (A composer that does not wrap): Not started (Gate K follow-up from the owner's marks on 2026-09-26).
- [ ] [AC-110](docs/verification/AC-110.md) (Account names read once): Not started (Gate K follow-up from the owner's marks on 2026-09-26).
- [ ] [AC-111](docs/verification/AC-111.md) (The composer says what's next): Not started (Gate K follow-up from the owner's marks on 2026-09-26).
- [ ] [AC-112](docs/verification/AC-112.md) (Search you can see): Not started (Gate K follow-up from the owner's marks on 2026-09-26).
- [ ] [AC-113](docs/verification/AC-113.md) (No empty grid): Not started (Gate K follow-up from the owner's marks on 2026-09-26).
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
- [ ] [AC-138](docs/verification/AC-138.md) (Permission modes carry over): Not started (Gate L, added by the owner on 2026-09-26; design in docs/rfcs/offline-mode.md; built in its own worktree and pull request).
- [ ] [AC-139](docs/verification/AC-139.md) (OpenCode session transport spike): Not started (Gate L, added by the owner on 2026-09-26; design in docs/rfcs/offline-mode.md; built in its own worktree and pull request).
- [ ] [AC-140](docs/verification/AC-140.md) (Memory safety guard): Not started (Gate L, added by the owner on 2026-09-26; design in docs/rfcs/offline-mode.md; built in its own worktree and pull request).
- [ ] [AC-141](docs/verification/AC-141.md) (Pair once): Not started (Gate N, added by the owner on 2026-09-26; design in docs/rfcs/phone-remote.md; built in its own worktree and pull request).
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
- [Side RFC: Audio Mode — opt-in cues from the daemon](docs/rfcs/audio-mode.md)
- [Side RFC: phone remote on the same network](docs/rfcs/phone-remote.md) and its [prepared goal](docs/rfcs/phone-remote-goal.md)
- [Inspected sources and reuse assessment](docs/source-assessment.md)

Design targets macOS and Linux; only macOS is verified. Auto routing beyond the offline fallback ([Gate L](docs/rfcs/offline-mode.md)), a relay for phone access away from the local network (after [Gate N](docs/rfcs/phone-remote.md)), VSCodium,
Windows/Remote SSH and review comments sent to agents are later milestones.
