# Overseer

A local agent orchestration daemon in Rust (`overseerd`) with a VS Code extension for
account-based agent runs, recursive native-child visibility, and live editable worktree
review built on [Branch Diff](https://github.com/beelol/branch-diff).

**Status: usable macOS milestone — not the complete product.** Verified acceptance
criteria: **44 / 51** · **2** partial (see [ledger](docs/verification/README.md)). Unverified:
AC-08, AC-11, AC-13, AC-41, AC-45, AC-50, AC-51. The biggest gaps are the newer owner requests still in progress
(hunk accept/reject, a structured run conversation view, merge back, simple account governance
([side RFC](docs/rfcs/account-governance.md)), a polished theme-compatible UI and a full-page
Overseer view), live two-account runs (AC-12/13; both ChatGPT accounts are now signed in) and a
foreign-user socket test (AC-08). Linux (AC-41) is out of scope for now; opening PRs (AC-50) and a
worktree file tree (AC-51) are future work. Partial criteria list what is proven and what is deferred. The full list is under [Acceptance criteria](#acceptance-criteria); next actions are in [Follow-ups](#follow-ups).

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
- [ ] **AC-08** Local access boundary — blocked: rejecting a different local user was never exercised (needs a second macOS account) — [evidence](docs/verification/AC-08.md)
- [x] **AC-09** Complete task controls — [evidence](docs/verification/AC-09.md)
- [x] **AC-10** Event replay and bounded output — [evidence](docs/verification/AC-10.md)
- [ ] **AC-11** Account profiles — ◐ partial: add/name/select accounts and sign in through each account's own flow in the UI (ChatGPT browser and device code, live for A/B; Claude via the fixture CLI); folders created at creation (0700); a missing login is shown and blocks the account tile; an expired login fails with a classified auth error and "Sign in again" reauthenticates that account, after which the follow-up works; no API keys / deferred: a live sign-in and re-sign-in of a fixed Claude account, and a live re-sign-in of a disposable ChatGPT account (both need the owner's browser login; A and B are never signed out) — [evidence](docs/verification/AC-11.md)
- [x] **AC-12** Two simultaneous ChatGPT subscriptions — [evidence](docs/verification/AC-12.md)
- [ ] **AC-13** Credential isolation on macOS — not yet run: isolation check with a disposable extra profile pending — [evidence](docs/verification/AC-13.md)
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
- [ ] **AC-45** Visible background agents — ◐ partial: live Claude run kept running after Cmd+Q; daemon posted the notice through macOS `osascript` (exit 0) naming the agent and the stop command; reopen showed it; Stop Agents and Daemon confirmed, interrupted it and left no daemon, shim or harness process; no notice with nothing running / deferred: a screenshot of the macOS banner itself (screen recording and the notification database are not accessible to the agent) — [evidence](docs/verification/AC-45.md)
- [x] **AC-46** Simple account governance — [evidence](docs/verification/AC-46.md)
- [x] **AC-47** Polished, theme-compatible UI — [evidence](docs/verification/AC-47.md)
- [x] **AC-48** Overseer view (command center) — [evidence](docs/verification/AC-48.md)
- [x] **AC-49** Restore the open session — [evidence](docs/verification/AC-49.md)
- [ ] **AC-50** Open a pull request from a run (coming soon) — not started (coming soon; added by the owner on 2026-09-25) — [evidence](docs/verification/AC-50.md)
- [ ] **AC-51** Worktree file hierarchy — not started (added by the owner on 2026-09-25) — [evidence](docs/verification/AC-51.md)
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
  pauses when you scroll or select a file until you press **Resume**.

## Build and install (macOS)

Requirements: Rust 1.89+ (`cargo`), Node 24, Git, and the VS Code `code` CLI.

```bash
git clone https://github.com/beelol/overseer.git && cd overseer   # until PR #1 merges: add --branch claude/overseer-macos-app-2d3d20
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
```

Packaged-UI scenarios (open a real, isolated VS Code window; see [test/ui](test/ui)):

```bash
node test/ui/scenario-main.js
```

## Using it

1. **Accounts**: the Accounts view lists *existing login* profiles per harness and any
   isolated profiles you add (**Add Account Profile…**, then **Sign In…**, which runs the
   harness's own login in a terminal with that profile's credential home). Overseer never
   asks for API keys and never logs out an existing login.
2. **New Task** (+ in the Agents view): pick the repository, harness, account profile,
   *New worktree* or *Current checkout*, the start ref, an optional model, and the prompt.
3. The run opens its **Review** (Follow on for runs you launch) and an **output panel** with
   the event stream, capabilities, **Send follow-up**, **Interrupt**, and permission
   **Allow/Deny** when a harness asks. Controls a harness cannot support are disabled with
   the reason.
4. Click the comparison label (base icon) in the review to switch comparisons; hover it to
   see the snapshot id or SHAs and their provenance.
5. Edit in the review's working-tree side and press **Save**, or **Open in Native Diff** for
   full editor features including undo/redo. Unsaved drafts are labeled and survive reloads
   and external writes.

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

- [ ] [AC-08](docs/verification/AC-08.md) (Local access boundary): Needs a second local macOS user (owner creates a standard test account). Next: as that user, `nc -U <socket>` / `overseerd ctl hello` with the owner's `OVERSEER_HOME` must fail with a permission error, and a relaxed-permission socket must still be refused by the peer-uid check (log line `rejected connection from uid …`).
- [ ] [AC-11](docs/verification/AC-11.md) (Account profiles): Owner action: in Overseer, Accounts → Add Account → Anthropic → Sign In, then Sign Out and Sign In again on that account (and optionally the same with a throwaway ChatGPT account).
- [ ] [AC-13](docs/verification/AC-13.md) (Credential isolation on macOS): Owner-provided dedicated test logins (A and B). Next: A logout/login while B runs a `sleep` turn; restart; compare fingerprints; inspect SQLite/events for token leakage (`grep` for token patterns).
- [ ] [AC-41](docs/verification/AC-41.md) (Linux verification (deferred by owner)): Needs a Linux machine with VS Code and the harnesses. Next: run the README build, `cargo test`, and the UI scenarios there.
- [ ] [AC-45](docs/verification/AC-45.md) (Visible background agents): Owner action: close VS Code while an agent runs and confirm the "Overseer: N agents still running" banner appears (allow Script Editor notifications if it does not).
- [ ] [AC-50](docs/verification/AC-50.md) (Open a pull request from a run (coming soon)): Not blocked; deferred by the owner (coming soon). Next: use VS Code's `github` authentication session to push and create the PR.
- [ ] [AC-51](docs/verification/AC-51.md) (Worktree file hierarchy): Not blocked; not started. Next: a file tree for the selected run's worktree inside the Overseer view (AC-48).
- [ ] Decide a retention policy for snapshot refs under `refs/overseer/snapshots/*` (they accumulate per turn; harmless but unbounded). Clearly labeled follow-up; no AC covers it.
- [ ] Decide whether the *existing login* Codex profile should be discouraged: on this machine `~/.codex` is shared with the ChatGPT desktop app and switched accounts during the session (see [AC-02](docs/verification/AC-02.md)). Clearly labeled follow-up.
- [ ] Remove or update the stale `~/Library/pnpm/codex` (0.1.x) on PATH; Overseer ignores it in favour of the ChatGPT.app bundle. Owner environment note.
- [ ] VS Code on this machine trusts `/` in its workspace-trust list, so folders never open in Restricted Mode; the trust test uses an empty window ([AC-08](docs/verification/AC-08.md)). Owner environment note.

## Project documents

- [RFC and authoritative acceptance checklist](docs/overseer-rfc.md)
- [Verification ledger and evidence](docs/verification/README.md)
- [Harness compatibility](docs/compatibility.md)
- [Side RFC: simple account governance](docs/rfcs/account-governance.md)
- [Inspected sources and reuse assessment](docs/source-assessment.md)

Design targets macOS and Linux; only macOS is verified. Auto routing, a TUI, VSCodium,
Windows/Remote SSH and review comments sent to agents are later milestones.
