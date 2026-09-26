# Overseer

A local agent orchestration daemon in Rust (`overseerd`) with a VS Code extension for
account-based agent runs, recursive native-child visibility, and live editable worktree
review built on [Branch Diff](https://github.com/beelol/branch-diff).

**Status: usable macOS milestone — not the complete product.** Verified acceptance
criteria: **58 / 82** · **6** partial (see [ledger](docs/verification/README.md)). Unverified:
AC-41, AC-53, AC-58, AC-59, AC-60, AC-62, AC-64, AC-66, AC-67, AC-68, AC-69, AC-70, AC-71, AC-72, AC-73, AC-74, AC-75, AC-76, AC-77, AC-78, AC-79, AC-80, AC-81, AC-82. The biggest gaps are the daily-driver UI (Gate J partials, and Gate K, AC-67 to AC-82: the native side bar
with chat and diff side by side, added by the owner on 2026-09-26; [design](docs/rfcs/orchestrator-ui.md#gate-k-layout)), fixed Claude accounts (AC-53, partial;
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
- [ ] **AC-58** Agent grid — ◐ partial: nine concurrent fixture streams tile 3×3 and a maximum of 4 tiles 2×2; a permission request is answered from its tile; a pinned finished run stays; arrow keys move between tiles and Enter opens the agent; webview event-loop lag p95 2 ms; screenshots at 4 and 9 tiles in both themes / deferred: the per-tile update time: a streamed line reaches its tile in 853 ms at p95 (target 250 ms); the daemon records the same lines within 62 ms p95, so the delay is between the daemon and the webview — [evidence](docs/verification/AC-58.md)
- [ ] **AC-59** Start a new agent from the chat — ◐ partial: with no agent selected the middle is the composer; Claude and Codex agents start keyboard-only and stream in place as the selected agent; a signed-out account is shown inline with Sign in and Start disabled; a harness that is not installed is labelled so; the Full form link stays / deferred: a generic program started keyboard-only (choosing Run a program from the agent menu left the chip on Codex) and defaults remembered across a reload (the composer did not finish loading after the reload in the scenario) — [evidence](docs/verification/AC-59.md)
- [ ] **AC-60** Native-CLI parity for everyday use — ◐ partial: live Claude Code and Codex runs take model, reasoning effort and permission mode per turn (argv from each run's launch record), an attached image and a mentioned worktree file reach the agent (replies name the red color and README.md's first line), a running turn is stopped and the next message answered, and finished runs continue their session after the daemon restarts; support per harness is in docs/compatibility.md / deferred: the same capabilities driven from the chat composer in the packaged UI: after the options menu closes, Enter does not send, so the paste, @-mention, options, queue and ⌥Enter checks in scenario-parity.js fail; the live turns used the daemon API the composer calls — [evidence](docs/verification/AC-60.md)
- [x] **AC-61** Needs-you inbox and keyboard control — [evidence](docs/verification/AC-61.md)
- [ ] **AC-62** Usage and limits — ◐ partial: Claude's live usage (5 hours 17%, week 48%, reset times) is exactly its own rate_limit_event; both ChatGPT accounts report plan and usage from Codex's session log (ChatGPT A team 0%/0%, ChatGPT B plus 0%/16%); OpenCode says not reported; tokens and cost per turn are shown; near-limit warning with fixtures / deferred: an independent check of the Codex numbers against the raw token_count line in each account's session log (those logs sit in the account folders next to the credentials, which this session does not read) — [evidence](docs/verification/AC-62.md)
- [x] **AC-63** History that stays tidy — [evidence](docs/verification/AC-63.md)
- [ ] **AC-64** Default-to-Overseer session (owner-confirmed) — owner session after the rest of Gate J — [evidence](docs/verification/AC-64.md)
- [x] **AC-65** Provider logos — [evidence](docs/verification/AC-65.md)
- [ ] **AC-66** Design review against references (owner-confirmed) — ◐ partial: the references are studied and what Overseer adopts is written down (docs/design/references.md, the RFC's "What Overseer adopts"); the review page shows every view before and after in both Overseer themes and a stock theme, plus the new views (chat, live chats, grid, dashboard mode, composer, Needs you, history, usage, themes and logos), with a Looks right / Needs work mark and a note per view saved for the owner / deferred: the owner's marks, the changes they ask for, and the owner's dated confirmation that the UI looks clean and polished — [evidence](docs/verification/AC-66.md)
- [ ] **AC-67** One agents list: the native side bar — not started (Gate K, added by the owner on 2026-09-26) — [evidence](docs/verification/AC-67.md)
- [ ] **AC-68** Provider logos in the side bar — not started (Gate K, added by the owner on 2026-09-26) — [evidence](docs/verification/AC-68.md)
- [ ] **AC-69** Search and filter in the side bar — not started (Gate K, added by the owner on 2026-09-26) — [evidence](docs/verification/AC-69.md)
- [ ] **AC-70** Quiet row actions — not started (Gate K, added by the owner on 2026-09-26) — [evidence](docs/verification/AC-70.md)
- [ ] **AC-71** Take an agent out — not started (Gate K, added by the owner on 2026-09-26) — [evidence](docs/verification/AC-71.md)
- [ ] **AC-72** Chat in the middle when there is nothing to review — not started (Gate K, added by the owner on 2026-09-26) — [evidence](docs/verification/AC-72.md)
- [ ] **AC-73** Changes bring the diff forward — not started (Gate K, added by the owner on 2026-09-26) — [evidence](docs/verification/AC-73.md)
- [ ] **AC-74** Follow or manual review — not started (Gate K, added by the owner on 2026-09-26) — [evidence](docs/verification/AC-74.md)
- [ ] **AC-75** One place for changes — not started (Gate K, added by the owner on 2026-09-26) — [evidence](docs/verification/AC-75.md)
- [ ] **AC-76** Review that stays clean at any width — not started (Gate K, added by the owner on 2026-09-26) — [evidence](docs/verification/AC-76.md)
- [ ] **AC-77** Chat that works beside a diff — not started (Gate K, added by the owner on 2026-09-26) — [evidence](docs/verification/AC-77.md)
- [ ] **AC-78** Quiet turn endings — not started (Gate K, added by the owner on 2026-09-26) — [evidence](docs/verification/AC-78.md)
- [ ] **AC-79** Grid and dashboard mode in the new layout — not started (Gate K, added by the owner on 2026-09-26) — [evidence](docs/verification/AC-79.md)
- [ ] **AC-80** Remembered place — not started (Gate K, added by the owner on 2026-09-26) — [evidence](docs/verification/AC-80.md)
- [ ] **AC-81** Gate J still holds — not started (Gate K, added by the owner on 2026-09-26) — [evidence](docs/verification/AC-81.md)
- [ ] **AC-82** Gate K design review (owner-confirmed) — not started (Gate K, added by the owner on 2026-09-26) — [evidence](docs/verification/AC-82.md)
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
- [ ] [AC-58](docs/verification/AC-58.md) (Agent grid): Per-tile latency 853 ms p95 vs 250 ms. Next: time each hop in the extension host (daemon socket → RunFeed batch → postMessage) and the tile renderer, and remove the slow hop.
- [ ] [AC-59](docs/verification/AC-59.md) (Start a new agent from the chat): Keyboard selection of Run a program and remembered defaults after reload. Next: fix the agent-menu keyboard pick for the generic entry and the composer's reload state, then rerun scenario-composer.js.
- [ ] [AC-60](docs/verification/AC-60.md) (Native-CLI parity for everyday use): Composer keyboard focus after the options menu (and the queue display). Next: keep focus in the prompt when the options menu closes, then rerun scenario-parity.js.
- [ ] [AC-62](docs/verification/AC-62.md) (Usage and limits): Independent Codex comparison. Next: have the daemon include the raw token_count line it read in account.usage (no credentials), then compare it in the live scenario.
- [ ] [AC-64](docs/verification/AC-64.md) (Default-to-Overseer session (owner-confirmed)): Owner action after the design review (AC-66): work for an hour using only Overseer for Claude Code and Codex; log friction.
- [ ] [AC-66](docs/verification/AC-66.md) (Design review against references (owner-confirmed)): Waiting for the owner's marks. Next: read the marks from the page, change each marked item, republish the page and ask for confirmation.
- [ ] [AC-67](docs/verification/AC-67.md) (One agents list: the native side bar): Not started (added by the owner on 2026-09-26; built in its own pull request).
- [ ] [AC-68](docs/verification/AC-68.md) (Provider logos in the side bar): Not started (added by the owner on 2026-09-26; built in its own pull request).
- [ ] [AC-69](docs/verification/AC-69.md) (Search and filter in the side bar): Not started (added by the owner on 2026-09-26; built in its own pull request).
- [ ] [AC-70](docs/verification/AC-70.md) (Quiet row actions): Not started (added by the owner on 2026-09-26; built in its own pull request).
- [ ] [AC-71](docs/verification/AC-71.md) (Take an agent out): Not started (added by the owner on 2026-09-26; built in its own pull request).
- [ ] [AC-72](docs/verification/AC-72.md) (Chat in the middle when there is nothing to review): Not started (added by the owner on 2026-09-26; built in its own pull request).
- [ ] [AC-73](docs/verification/AC-73.md) (Changes bring the diff forward): Not started (added by the owner on 2026-09-26; built in its own pull request).
- [ ] [AC-74](docs/verification/AC-74.md) (Follow or manual review): Not started (added by the owner on 2026-09-26; built in its own pull request).
- [ ] [AC-75](docs/verification/AC-75.md) (One place for changes): Not started (added by the owner on 2026-09-26; built in its own pull request).
- [ ] [AC-76](docs/verification/AC-76.md) (Review that stays clean at any width): Not started (added by the owner on 2026-09-26; built in its own pull request).
- [ ] [AC-77](docs/verification/AC-77.md) (Chat that works beside a diff): Not started (added by the owner on 2026-09-26; built in its own pull request).
- [ ] [AC-78](docs/verification/AC-78.md) (Quiet turn endings): Not started (added by the owner on 2026-09-26; built in its own pull request).
- [ ] [AC-79](docs/verification/AC-79.md) (Grid and dashboard mode in the new layout): Not started (added by the owner on 2026-09-26; built in its own pull request).
- [ ] [AC-80](docs/verification/AC-80.md) (Remembered place): Not started (added by the owner on 2026-09-26; built in its own pull request).
- [ ] [AC-81](docs/verification/AC-81.md) (Gate J still holds): Not started (added by the owner on 2026-09-26; built in its own pull request).
- [ ] [AC-82](docs/verification/AC-82.md) (Gate K design review (owner-confirmed)): Not started (added by the owner on 2026-09-26; built in its own pull request).
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
- [Inspected sources and reuse assessment](docs/source-assessment.md)

Design targets macOS and Linux; only macOS is verified. Auto routing, a TUI, VSCodium,
Windows/Remote SSH and review comments sent to agents are later milestones.
