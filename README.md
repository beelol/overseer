# Overseer

A local agent orchestration daemon in Rust (`overseerd`) with a VS Code extension for
account-based agent runs, recursive native-child visibility, and live editable worktree
review built on [Branch Diff](https://github.com/beelol/branch-diff).

**Status: usable macOS milestone — not the complete product.** Verified acceptance
criteria: **51 / 64** · **1** partial (see [ledger](docs/verification/README.md)). Unverified:
AC-41, AC-53, AC-54, AC-55, AC-56, AC-57, AC-58, AC-59, AC-60, AC-61, AC-62, AC-63, AC-64. The biggest gaps are the daily-driver UI (Gate J, AC-54 to AC-64, added by the
owner on 2026-09-26; [design](docs/rfcs/orchestrator-ui.md)), fixed Claude accounts (AC-53, partial;
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
- [ ] **AC-54** Clean, compact presentation — not started (added by the owner on 2026-09-26) — [evidence](docs/verification/AC-54.md)
- [ ] **AC-55** Chat-style conversation — not started (added by the owner on 2026-09-26) — [evidence](docs/verification/AC-55.md)
- [ ] **AC-56** Overseer themes, light and dark — not started (added by the owner on 2026-09-26) — [evidence](docs/verification/AC-56.md)
- [ ] **AC-57** Overseer dashboard — not started (added by the owner on 2026-09-26) — [evidence](docs/verification/AC-57.md)
- [ ] **AC-58** Agent grid — not started (added by the owner on 2026-09-26) — [evidence](docs/verification/AC-58.md)
- [ ] **AC-59** Start a new agent from the chat — not started (added by the owner on 2026-09-26) — [evidence](docs/verification/AC-59.md)
- [ ] **AC-60** Native-CLI parity for everyday use — not started (added by the owner on 2026-09-26) — [evidence](docs/verification/AC-60.md)
- [ ] **AC-61** Needs-you inbox and keyboard control — not started (added by the owner on 2026-09-26) — [evidence](docs/verification/AC-61.md)
- [ ] **AC-62** Usage and limits — not started (added by the owner on 2026-09-26) — [evidence](docs/verification/AC-62.md)
- [ ] **AC-63** History that stays tidy — not started (added by the owner on 2026-09-26) — [evidence](docs/verification/AC-63.md)
- [ ] **AC-64** Default-to-Overseer session (owner-confirmed) — owner session after the rest of Gate J — [evidence](docs/verification/AC-64.md)
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
- [ ] [AC-54](docs/verification/AC-54.md) (Clean, compact presentation): Not started (added by the owner on 2026-09-26; see docs/rfcs/orchestrator-ui.md).
- [ ] [AC-55](docs/verification/AC-55.md) (Chat-style conversation): Not started (added by the owner on 2026-09-26; see docs/rfcs/orchestrator-ui.md).
- [ ] [AC-56](docs/verification/AC-56.md) (Overseer themes, light and dark): Not started (added by the owner on 2026-09-26; see docs/rfcs/orchestrator-ui.md).
- [ ] [AC-57](docs/verification/AC-57.md) (Overseer dashboard): Not started (added by the owner on 2026-09-26; see docs/rfcs/orchestrator-ui.md).
- [ ] [AC-58](docs/verification/AC-58.md) (Agent grid): Not started (added by the owner on 2026-09-26; see docs/rfcs/orchestrator-ui.md).
- [ ] [AC-59](docs/verification/AC-59.md) (Start a new agent from the chat): Not started (added by the owner on 2026-09-26; see docs/rfcs/orchestrator-ui.md).
- [ ] [AC-60](docs/verification/AC-60.md) (Native-CLI parity for everyday use): Not started (added by the owner on 2026-09-26; see docs/rfcs/orchestrator-ui.md).
- [ ] [AC-61](docs/verification/AC-61.md) (Needs-you inbox and keyboard control): Not started (added by the owner on 2026-09-26; see docs/rfcs/orchestrator-ui.md).
- [ ] [AC-62](docs/verification/AC-62.md) (Usage and limits): Not started (added by the owner on 2026-09-26; see docs/rfcs/orchestrator-ui.md).
- [ ] [AC-63](docs/verification/AC-63.md) (History that stays tidy): Not started (added by the owner on 2026-09-26; see docs/rfcs/orchestrator-ui.md).
- [ ] [AC-64](docs/verification/AC-64.md) (Default-to-Overseer session (owner-confirmed)): Owner action after the rest of Gate J: work for an hour using only Overseer for Claude Code and Codex; log friction.
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
