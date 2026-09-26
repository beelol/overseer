# Overseer RFC and acceptance criteria

Status: implementation in progress — usable macOS milestone with documented gaps (see the [ledger](verification/README.md)). The owner authorized the overnight implementation session on 2026-09-24 via the session goal; the eight hours applied to the implementing agent's session, not to runs inside Overseer.
Reconstructed on 2026-09-24 from the owner's supplied conversation. The earlier agent's
actual RFC was unavailable: both the local directory and GitHub repository were empty.
This document replaces the missing draft; it does not claim to reproduce its 28 criteria.

## Product and boundary

Overseer is a local Rust orchestration daemon with a VS Code control surface and an
integrated Branch Diff review experience. It launches account-authenticated agent
harnesses, tracks their runs and descendants, and lets the user follow and edit their
work. The daemon owns durable state. Closing VS Code must not end the work.

A harness is the agent runtime (for example Codex, Claude Code, or OpenCode), not the
model or account. OpenCode is one harness, not the umbrella for all harnesses. A remote
agent service such as Devin may require a different adapter and workspace model.

The first usable release includes multiple accounts, native child visibility testing,
and editable worktree diffs. These are not postponed until after the first release.
The initial implementation can use smaller milestones, but cannot call an intermediate
milestone the completed product. A usable macOS milestone may retain explicitly unchecked
Linux and unavailable native-telemetry criteria; it must disclose those gaps.

## Decisions from the owner

| Topic | Requirement |
| --- | --- |
| Runtime | Prefer Rust for the daemon; use normal VS Code extension technology for the UI. |
| Platforms | Design for macOS and Linux; verify macOS now. No Linux machine is available; leave Linux verification unchecked. |
| Primary UI | VS Code; daemon architecture must permit a future TUI. Building a TUI is not current scope. |
| Reuse | Reuse suitable open-source code with its license and notices preserved. Inspect before adopting. |
| Accounts | Account/subscription sign-in only for now. No API-key fallback. Demonstrate two distinct ChatGPT subscription accounts running simultaneously. |
| Harness coverage | Codex and Claude Code must work through account login. OpenCode must work; mock responses or a very small Qwen Coder through Ollama are acceptable for its initial verification. Attempt other integrations where feasible; skip Devin if account login is unavailable. |
| Workspace | Isolated worktree by default. Include an explicit current-checkout mode that preserves existing dirty work. |
| Editing | The review UI must edit the selected agent's actual worktree/current checkout. |
| Diff | Show branch changes plus dirty work, including main compared with main. |
| Base | On agent launch, default to changes for the latest run. Also offer task-start, original fork/base and selectable branch comparisons, with clear labels and a base icon. This supersedes the earlier target-base default. |
| Follow | A checkbox follows edits across and within files. Selecting another file or scrolling away pauses Follow until explicit Resume. |
| Review | Continues refreshing while agents work; it is not a frozen snapshot. |
| Descendants | Recursive native children/grandchildren. Investigate missing signals; keep a limited harness usable with the relevant AC unchecked and its limitation visible. |
| Routing | Auto mode is desirable later; initially let harnesses handle their own delegation. |
| Verification | The implementing agent may verify and check ACs with evidence; a later review pass follows. Use only tiny hello-world-style paid prompts and minimal tokens. Two OpenAI accounts exist, but agent login access is unproven. |
| Scope and authority | Planning updates now; do not start implementation or harness tests without explicit confirmation. Restrict project changes and publication to the confirmed project repository; other work is verification only. |

## Proposed defaults, distinguished from confirmed decisions

These resolve implementation details without presenting them as prior user decisions.
Changing them requires a recorded RFC revision, not a hidden implementation shortcut.

- Native delegation observation comes first. Overseer-created child scheduling, Auto
  routing, races, and quota-based escalation are later work.
- Use SQLite for state/events and a versioned local protocol. Prefer a Unix socket with
  owner-only access. A localhost HTTP transport would also need authentication.
- Prefer structured harness protocols/events; use a persistent process/session backend
  where necessary. Evaluate tmux for interactive fallback. Do not force a structured
  harness into terminal scraping merely to standardize on tmux.
- Confirmed Follow behavior has `off`, `following`, and `paused by navigation` states. Manual selection of
  another file or deliberate scrolling away pauses it; an explicit Resume action starts
  it again. It does not resume on a timer and interrupt reading. Turning the checkbox
  off preserves file, selection, and scroll as far as the changing text permits.
- Review and Follow use the same live data. Review disables automatic navigation.
  Switching to another agent does not inherit Follow accidentally.
- Review comments sent to agents, automatic merges, and automatic worktree deletion are
  later work. Editing and native editor undo are required now. Hunk accept/reject was
  moved into scope by the owner on 2026-09-25 (AC-42).
- Stable VS Code on macOS is the current verification target. Keep platform-dependent
  paths, processes and credential stores behind portable boundaries for Linux; Linux
  verification is deferred under AC-41. VSCodium, Windows and Remote SSH are later qualifications.
- No embedded inference model in v1. First exhaust structured events, session records,
  hooks and deterministic parsing. A speculative 512 MB model is not a dependency.

### Exact comparison semantics

For a selected run, let `H` be current HEAD, `T` a selected comparison branch,
`M = merge-base(T, H)`, `S` the immutable task-start snapshot, `R` the latest run-start
snapshot, `F` the recorded original fork commit, `I` the index and `W` the working tree.
Snapshots record relevant file contents and index state, including existing dirty and
nonignored untracked files, without committing, staging or stashing the user's checkout.
Capture the task-start snapshot once and a fresh run-start snapshot before each launch.

The confirmed default is **R → W**, labeled **Latest run**, so a new launch initially
shows no new run changes and then reveals that run's edits. Proposed boundary: each
user-submitted follow-up that starts a new work turn also gets a fresh snapshot; transport
reconnects and process recovery do not. Keep prior run baselines addressable. This defines
“since last run” as changes from the start of the latest run, not the end of its predecessor.
Unsaved buffers remain separate labeled drafts and are preserved before any launch.

The comparison picker also offers:

- **Since task start (S → W)**, preserving dirty starting contents rather than just a SHA.
- **Original fork (F → W)**, with its recorded commit and provenance. If unknown, show
  unavailable or clearly label a detected candidate; do not invent historical certainty.
- **Target/other branch**, allowing branch selection and explicitly labeled merge-base
  (**M → W**, PR-style) or direct branch-tip comparison.

The base icon exposes selected mode, snapshot/run identity or actual refs/SHAs, and detected
parent information. Target selection defaults to the configured integration branch, then
repository default; unresolved branches/bases show an explanation. A rebase cannot silently
rewrite a recorded run/task snapshot or historical fork.

The selected comparison's changes form the main file list. A separate always-visible
**Workspace dirty** section exposes all staged (`H → I`), unstaged (`I → W`) and nonignored
untracked files, including pre-existing changes; deduplicate file rows or label sections
clearly. Thus an empty latest-run diff cannot conceal dirty work. Separate staged/unstaged
inspection remains available when net changes cancel out. Clean main/main is empty; dirty
main/main shows dirty work, not every unchanged file. Branch mode includes branch commits.

Unsaved editor buffers are additional visible drafts, labeled unsaved. Saving targets the
selected worktree. External writes must not silently overwrite a user's draft. Git dirtiness
alone proves a file changed, not who changed it: pre-existing/user edits remain visible and
must not be falsely attributed to AI.

## Architecture and ownership

The extension owns presentation, user navigation, and editor buffers. The daemon owns
repositories, tasks, execution targets, runs, process lifecycles, worktree identities,
persisted events and capability state. An extension reload rebuilds its view from the daemon.

Core records:

- `Task`: user intent, repository, target ref, task-start snapshot, recorded fork commit/provenance, acceptance metadata.
- `AgentRun`: task, parent run (optional), harness/version, account profile, provider/model,
  workspace, run-start snapshot, native session ID, lifecycle, timestamps and supported metrics.
- `Workspace`: canonical path, repository identity, worktree/current-checkout kind,
  branch, write ownership and initial dirty inventory. Children may share a workspace.
- `ExecutionTarget`: harness, isolated account profile, supported models/capabilities;
  credentials themselves do not belong in SQLite event payloads.
- `Event`: stable ID, run/session identity, ordering cursor, type, timestamp, source,
  evidence confidence and a bounded, redacted payload.

Adapter capabilities include launch, prompt, interrupt, resume, approvals, output,
file activity, children, usage and quota. Unsupported or unknown is different from zero.
Use exact structured IDs when available. Inferred child edges must carry provenance and
confidence; text saying “I delegated” is not sufficient evidence of a real child run.

Prefer one writer per workspace. Native children may intentionally share their parent's
workspace; record that relationship. Independent tasks default to separate worktrees.
Reject unexpected second writers to an existing checkout. Do not copy login state merely
by copying HOME: test each harness's credential-store and refresh behavior.

## Acceptance criteria

Only the checkboxes below are authoritative. A box is checked only when its evidence record is `verified`.
Each **Verify** clause is required evidence, not a suggestion. Fixture tests complement
real integration tests; they cannot substitute for subscription, native-child, or UI tests.
The owner explicitly permits mock/local-model verification for OpenCode AC-14 only;
record that coverage honestly rather than claiming paid-account or native-child verification.
Evidence and blockers live in [the verification ledger](verification/README.md).

### Gate A — feasibility and reuse (before architectural lock-in)

- [x] **AC-01 — Harness capability survey.** Record pinned versions, supported sign-in, process/remote transport, prompt/control APIs, native child signals, resume, usage and workspace access for Codex, Claude Code, OpenCode, Gemini CLI and Devin. **Verify:** cite primary docs and record runnable probes for installed harnesses; label untested/missing access explicitly. Documentation alone cannot establish runtime support.
- [x] **AC-02 — Account and child feasibility spikes.** Before freezing adapters, demonstrate two isolated Codex subscription sessions and native child capture for Codex, Claude Code and OpenCode, or record exact blocking behavior and the next investigated alternative. **Verify:** redacted live transcripts with versions and identities; an investigated blocker can complete this research criterion, but does not pass AC-12 or AC-19.
- [x] **AC-03 — Reuse decision.** Assess Branch Diff first; compare Agetor, Parallel Code, Pane and XCB only as relevant candidates. Record selected commit, actual license, intended reused surface and obligations before copying code. **Verify:** source/license inventory and notices for all imported code; an explicit skip rationale is acceptable. No unverified license claims from the prior conversation.

### Gate B — durable daemon and VS Code control

- [x] **AC-04 — Installable macOS foundation with portable design.** Rust daemon and extension build from a clean checkout with documented versions and platform boundaries suitable for Linux. **Verify:** clean macOS build and daemon smoke run with logs; inspect portable path/process abstractions. Actual Linux build/runtime evidence belongs to unchecked AC-41.
- [x] **AC-05 — Independent durable state.** Tasks, runs, parent relationships, workspaces and event history survive extension close/reload and daemon restart. **Verify:** run work, disconnect/reconnect UI, restart daemon, and compare identities/history; reconciliation must not duplicate tasks or spawn replacement work silently.
- [x] **AC-06 — Honest process lifecycle.** Display queued/running/waiting-for-user/completed/failed/interrupted/disconnected or unknown states based on actual signals; expose exit reason. **Verify:** successful, failing, interrupted and externally killed processes, plus loss/recovery of the session connection. Silence cannot count as completion.
- [x] **AC-07 — Persistent sessions.** Closing VS Code leaves active agents running; daemon recovery either reattaches to surviving agents or accurately reports lost sessions. **Verify:** a real harness session remains available through UI closure using minimal token-generating work; use a waiting process fixture for long-duration load, and a forced daemon crash reconciles without duplicate launches or false running status.
- [x] **AC-08 — Local access boundary.** Only the intended local user can issue daemon commands; workspace trust is required before execution. **Verify:** socket/transport access rejection and an untrusted VS Code workspace cannot launch a harness; malformed requests cannot execute shell fragments.
- [x] **AC-09 — Complete task controls.** From VS Code create a task, choose repository/target/account/harness, view output, send a follow-up, interrupt and resume where supported. **Verify:** an actual harness completes this flow; unsupported controls are disabled with an explanation, and a message reaches only its selected run.
- [x] **AC-10 — Event replay and bounded output.** Reconnection uses a cursor/snapshot without duplicate children or missing retained events; raw output is inspectable with redaction and bounded retention. **Verify:** duplicate/out-of-order fixture events, reconnect after output burst, and a retention boundary that explicitly marks truncated history.

### Gate C — accounts and harnesses

- [x] **AC-11 — Account profiles.** Users can add, name, select and sign into separate profiles through supported account login flows. No model-provider API keys are required or used as a fallback. **Verify:** login and reauthentication through the UI for each claimed supported account path, including a missing/expired login.
- [x] **AC-12 — Two simultaneous ChatGPT subscriptions.** Two distinct paid ChatGPT account profiles run Codex tasks concurrently in separate worktrees. **Verify:** overlapping live timestamps, redacted distinct account identity evidence, and successful independent file edits from both. Two processes under one account or subscription-plus-API-key do not qualify.
- [x] **AC-13 — Credential isolation on macOS.** Login, logout, refresh and expiry in profile A do not switch profile B's identity or corrupt its credentials/configuration. **Verify:** live A/B logout/login in dedicated test profiles during B's minimal work, restart both, and refresh/expiry fault tests for the actual macOS backend; inspect logs/database for leakage. Do not disturb unrelated active logins. Linux coverage belongs to AC-41.
- [x] **AC-14 — Initial adapters.** Codex and Claude Code support account-authenticated launch, streaming output, prompt routing, interruption and truthful capabilities. OpenCode integrates successfully with equivalent controls. **Verify:** tiny real account-authenticated edit/follow-up and interruption probes for Codex and Claude Code; OpenCode may use deterministic mock responses or a very small local Qwen Coder through Ollama. Exercise the actual OpenCode adapter/protocol, not just a fabricated UI response. Record exact harness/model versions and live/mock coverage; do not claim OpenCode account authentication was tested through mocks.
- [x] **AC-15 — Generic harness fallback.** A configured executable can run with selected cwd/profile environment and interactive output/control without pretending to have rich telemetry. **Verify:** executable path and arguments containing spaces, exit failures, interruption, and visible “unknown” child/usage capabilities.
- [x] **AC-16 — Permissions and limits.** Native permission requests remain actionable in the UI or attached harness session; sign-in failures, rate limits and quota exhaustion remain distinguishable. **Verify:** allow/deny and interrupt a waiting run, replay actual error formats, and ensure no silent approval or switch to an API key/account.
- [x] **AC-17 — Compatibility truthfulness.** UI/docs state support per harness, account path, version and capability; retain Gemini/Devin outcomes even if blocked or skipped. **Verify:** compare the matrix with evidence. Devin is optional and skipped unless account login works without manually managed API keys/access tokens. A telemetry-limited harness remains usable with the applicable AC unchecked; mock-tested OpenCode is labeled accordingly.

### Gate D — recursive visibility

- [x] **AC-18 — Recursive run tree.** Selecting a run exposes its children and descendants with identity, status, harness/account when known, and workspace association. **Verify:** at least three levels, shared and separate workspaces, duplicate events and delayed parents in fixtures; no cycles or duplicated nodes after reconnect.
- [x] **AC-19 — Actual native children.** Capture real native child runs from Codex, Claude Code and OpenCode, attach them to the correct parent, and expose their available output/status. **Verify:** live delegation sessions for each plus a native grandchild where the harness supports recursive delegation. Explicitly document unsupported depth; a synthetic tree does not pass the live portion.
- [x] **AC-20 — Evidence-backed inference.** Prefer structured events; investigate hooks/session files/deterministic output parsing when signals are missing. Mark inferred and unknown relationships visibly, retaining source evidence. **Verify:** positive/negative transcripts (including prose mentioning a child that never launched), parser-version mismatch, and incomplete telemetry. No inference is presented as complete ground truth.

### Gate E — workspace correctness

- [x] **AC-21 — Worktrees by default.** Each independent writing task gets an identified branch/worktree without mutating the source checkout. **Verify:** simultaneous tasks change identically named files independently; source checkout stays unchanged; existing branch/path collisions are handled without deletion.
- [x] **AC-22 — Current dirty checkout.** Explicitly choosing the current checkout records pre-existing staged, unstaged, untracked and unsaved work and leaves it intact. **Verify:** launch/edit/interrupt there, confirm original work remains, and distinguish initial dirtiness from observed run changes without inventing authorship.
- [x] **AC-23 — Shared workspace ownership.** Native children can share their parent's workspace; independent writers cannot accidentally claim it. **Verify:** shared parent/child edits remain visible under the right runs, a conflicting unrelated writer is rejected, and read-only labels are used only when access is actually enforced.
- [x] **AC-24 — Safe workspace retention.** Finished, failed and interrupted runs retain their work for review. Explicit cleanup reports dirty files and live users before removal and never removes the current checkout. **Verify:** untracked work, active child, interrupted task and current-checkout cleanup attempts preserve data.

### Gate F — live editable Branch Diff

- [x] **AC-25 — Correct repository selection.** Selecting an agent opens that agent's worktree review, even outside the open VS Code workspace. **Verify:** two repositories/worktrees with identical relative filenames; edits and refreshes never leak to the other path.
- [x] **AC-26 — Run snapshots and selectable bases.** Each launch defaults to latest-run changes from a captured baseline including existing dirty contents/index, while task-start, original fork and selected other-branch comparisons remain available. The icon identifies actual snapshot or refs/SHAs and provenance. **Verify:** two runs with edits between them, a follow-up, reconnect, staged/unstaged/untracked starting state, pre-existing feature commits, stacked branches, target advances, rebase, missing target and unknown fork. No snapshot operation mutates/stashes the checkout; prior run baselines survive restart and unresolved bases do not appear as empty success.
- [x] **AC-27 — Complete change and dirty views.** Selected-comparison changes and all workspace dirtiness remain accessible, including committed changes in branch mode, staged, unstaged, renamed, deleted and nonignored untracked files, including main/main. **Verify:** real Git fixtures for each, clean main/main, and a newly launched run with an empty run diff but pre-existing dirty work still visible. Binary/oversized files remain listed with explicit limitations; ignored files do not flood the list.
- [x] **AC-28 — No cancellation blind spot.** Opposing staged and unstaged edits remain inspectable when the net base-to-worktree diff is empty. **Verify:** stage A→B, restore B→A without staging, and inspect both layers; also test staged deletion followed by untracked recreation.
- [x] **AC-29 — Follow across and within files.** The checkbox navigates to observed agent edits, including new hunks in the already-open file. **Verify:** a live run alternates files and distant lines; unrelated user edits cannot falsely claim agent attribution. When only filesystem evidence exists, show that limitation.
- [x] **AC-30 — Navigation ownership.** Follow off preserves position; manual navigation pauses Follow with visible Resume; Review and agent switching do not unexpectedly jump the user. **Verify:** select another file, scroll, uncheck, resume, and switch agents during continuous live edits; confirm caret/scroll behavior in captured UI evidence.
- [x] **AC-31 — Live Review refresh.** File lists/hunks refresh without reopening or manual refresh, while preserving selection and drafts. **Verify:** writes, atomic replacements, staging, rename/delete, branch changes and a deliberately missed watcher event. Proposed bound: normal changes within 2 seconds after writes settle; reconciliation within 5 seconds on the recorded fixture.
- [x] **AC-32 — Edit selected workspace.** Text in the working-tree side is editable and save writes only the selected run's file; base content stays immutable. **Verify:** edit/save/reopen in external worktree and current-checkout modes, confirm disk path/content, and undo/redo through the native editor. Keep unsaved buffers labeled.
- [x] **AC-33 — Preserve conflicting drafts.** An agent's external edit, branch/base change, file deletion or view reload cannot silently overwrite or discard an unsaved user draft. **Verify:** overlap edits at the same line, preserve both versions for reconciliation/recovery, and recover a pending draft after reload.
- [x] **AC-34 — Safe file boundaries.** Writes cannot escape the selected workspace through path traversal or symlinks. Unsupported/binary/oversized/conflicted files have truthful states and safe native-editor access where applicable. **Verify:** traversal, escaping symlink, rename during edit, invalid encoding, merge conflict and large-file fixtures.
- [x] **AC-35 — Responsive review.** Large output/diff bursts do not lock the extension or create unbounded watchers, queues or retained text. **Verify:** a recorded fixture of 10,000 tracked files, 100 changed text files and four simulated active runs for 10 minutes (no sustained paid-model work); navigation remains responsive (proposed p95 under 250 ms), refresh meets AC-31 for ordinary files, and measured memory/queue behavior stabilizes after draining.

### Gate G — release evidence

- [x] **AC-36 — Packaged macOS UI.** Built VSIX and daemon install and complete task→follow→edit→review in stable VS Code on macOS. **Verify:** OS/editor/tool versions, installation logs and screenshots/recording from the real UI. Linux qualification remains separate and unchecked in AC-41.
- [x] **AC-37 — Automated regression coverage.** Real Git fixtures cover comparisons/workspaces; protocol/adapter tests cover replay, parsing, controls and failures. **Verify:** passing clean-checkout macOS checks with named tests mapped to ACs; mocks are labeled and do not satisfy live-only criteria. Linux checks are deferred to AC-41; do not imply cross-platform execution from portable test code.
- [x] **AC-38 — Reproducible acceptance ledger.** Every checked criterion links to evidence with tested implementation commit, environment, steps, expected/actual results and limitations. **Verify:** audit every checked ID; failures reopen affected criteria; missing credentials/hardware remain blocked, not waived.
- [x] **AC-39 — Minimal dogfood flow.** Use Overseer for a tiny hello-world-style change in an isolated Overseer worktree, follow edits, edit from review, run checks and preserve the result. **Verify:** minimal live session and diff/test evidence; record native delegation where observed and keep unverified child coverage in AC-19. Use fixture output for sustained/high-volume tests. Product changes may only be pushed to the confirmed project repo after implementation start is authorized; no automatic merge.
- [x] **AC-40 — Repository handoff.** README shows accurate current progress and links to this canonical checklist; docs explain install, accounts, capabilities, recovery and known blockers. Implementation/evidence intended for handoff reaches GitHub with no credentials. **Verify:** remote revision/file readback plus a fresh reader following setup instructions. Publishing this draft alone does not complete this release criterion.

### Gate H — review actions and run readability (added by the owner, 2026-09-25)

- [x] **AC-42 — Hunk accept and reject.** In the live review, each change against the selected comparison can be accepted (kept and marked reviewed) or rejected (restored to the comparison base in the selected run's workspace) one hunk at a time, with native undo. Rejecting never alters other hunks, other files, unsaved drafts or a different workspace, and a hunk the agent changes while it is being accepted or rejected is detected as a conflict, not silently overwritten. Reviewed state survives refreshes and window reloads and is cleared when the hunk changes again. **Verify:** a live run with several hunks across two files; reject one and accept another; confirm disk contents and native undo/redo; staged/unstaged and untracked-file cases; a concurrent agent edit to the same region during accept/reject; reload keeps reviewed state; works in worktree and current-checkout modes.
- [x] **AC-43 — Structured run conversation view.** A run's panel reads as a conversation rather than a flat event log: prompts and agent messages as turns, tool calls collapsible with their inputs and results, file edits linked to the review (opening the file at the edited hunk), permission requests inline with their decision, native children nested under the tool call that spawned them with their own output, errors and exit reasons highlighted, and usage per turn. The raw event log and raw output stay available. **Verify:** live Codex (exec and app-server) and Claude Code runs, including a native child and a permission request; expanding/collapsing tool calls; clicking a file edit opens that hunk in the correct worktree review; a fixture burst at the retention bound stays responsive (AC-35 bound) and truncation remains visible.

### Gate I — future owner requests (2026-09-25)

- [x] **AC-44 — Merge back.** Overseer keeps never merging automatically: each task stays on its `overseer/<name>` branch and worktree until the user acts. A finished or idle run offers a **Merge back** action in the Overseer view: Overseer merges the run's branch into the task's target branch in the source checkout with plain Git; if there are conflicts it hands only the conflict resolution to the same harness, account and session that did the work, and shows the result for review before completing. The source checkout's existing dirty work is never disturbed: merge back refuses (with an explanation) when the target checkout has uncommitted changes. **Verify:** live runs in disposable repos: a clean merge back; a conflicting merge back resolved by the agent and reviewed; a dirty target checkout refused and left untouched; the action disabled with an explanation while the run is active or when the branch cannot be merged; no automatic merge ever happens.
- [x] **AC-45 — Visible background agents.** Agents and the daemon keep running after VS Code closes (unchanged), but this is never silent: when the last VS Code window disconnects while runs are active, the user gets an OS notification naming the running agents and how to stop them; reopening VS Code shows them; a **Stop agents and daemon** command asks for confirmation, interrupts active runs and stops the daemon. **Verify:** close VS Code with an active live run and see the notification; reopen and see the run; stop everything from the command and confirm no Overseer or harness processes remain; no notification when nothing is running.
- [x] **AC-46 — Simple account governance.** Accounts become first-class and ultra simple as described in the [account governance RFC](rfcs/account-governance.md): add a Claude/Anthropic, OpenAI/ChatGPT or (when it has an account login) Devin account, name it, sign in through the provider's own flow, and see status, plan and an identity fingerprint; New Task offers only the accounts compatible with the chosen harness; desktop-app logins are labeled as following the app, distinct from fixed accounts. No API keys. **Verify:** the acceptance list in that RFC: one account per available provider added through the UI; compatible-only account choice per harness; switching the desktop app's account does not change a fixed account; re-sign-in and removal affect only that account.
- [x] **AC-47 — Polished, theme-compatible UI.** Task creation and run views are designed rather than stock pickers: harness and account are chosen from rounded tiles with icons, status and capability hints; spacing, typography and states are consistent; everything uses VS Code theme tokens so light, dark and high-contrast themes all look right; keyboard navigation and screen-reader labels work. **Verify:** screenshots of task creation, run panel and review in light, dark and high-contrast themes; keyboard-only task creation; no hard-coded colors outside theme tokens; accessibility labels present.
- [x] **AC-48 — Overseer view (command center).** A full-page Overseer view that works without the native sidebar and is not tied to the folder open in the VS Code window: an agents column (tasks → runs → native descendants across any repositories, expand/collapse, live status), the selected run's live editable diff review to its right, and that run's conversation (AC-43) with its event log alongside. One window can supervise agents in many repositories; selecting a node switches the review without unexpected jumps (AC-30). **Verify:** packaged-UI screenshots with the native sidebar closed; runs from two different repositories that are not open in the window; expand/collapse at three levels; switching runs switches the review and conversation; narrow and wide windows.
- [x] **AC-49 — Restore the open session.** After a window reload or VS Code restart, Overseer reopens what was open: the selected run, review panels with their comparison mode, Follow state (never auto-resumed), file and scroll position, run panels, and sidebar expansion. Runs whose worktree was removed show an explanation instead of failing. **Verify:** open several runs and panels, reload the window and restart VS Code; the same runs, comparisons and positions return; a removed worktree is explained.

- [x] **AC-50 — Open a pull request from a run.** Beside Merge back, an **Open PR** action pushes the run's branch and creates a pull request with a generated description, using the GitHub sign-in VS Code already has (no personal access tokens pasted into Overseer). Disabled with an explanation when there is no GitHub remote or no VS Code GitHub session. **Verify:** a PR created from a live run against a repository the owner chooses; missing remote and signed-out cases explained; no automatic merge.
- [x] **AC-51 — Worktree file hierarchy.** The Overseer view can show the selected run's worktree as a file tree (changed files highlighted, open any file in the editor) without depending on the folder open in the VS Code window. **Verify:** browse and open files in worktrees of two different repositories from one window; changed files marked; large repositories stay responsive.
- [x] **AC-52 — Native Overseer notifications (macOS).** The background-agent notification (AC-45) comes from Overseer itself rather than Script Editor: the banner shows Overseer's name and icon, clicking it opens VS Code at the Overseer view, and *System Settings → Notifications* lists **Overseer** with its own switch. A bundled helper app posts the notification; if notifications are denied, Overseer falls back to the current method and records which one it used. Design: [native notifications RFC](rfcs/native-notifications.md). **Verify:** close VS Code with an agent running and see an Overseer-branded banner (owner-observed or screenshot); click it and land in the Overseer view; find Overseer in Notifications settings; deny permission and confirm the fallback banner and the recorded `delivered_via`; **Overseer: Test Notification** posts a sample banner.
- [ ] **AC-53 — Fixed Claude accounts.** A Claude account added in Overseer (its own `CLAUDE_CONFIG_DIR`, separate from the Claude desktop login) can be signed in, used, signed out and signed in again without affecting the desktop login or runs on it, including on the macOS Keychain credential backend; if Claude's own per-folder Keychain entries are not separate, Overseer manages each account's Claude credentials itself ([Claude credentials RFC](rfcs/claude-credentials.md)). Moved out of AC-11/AC-13, which keep the ChatGPT paths. **Verify:** with a second Claude account, Add Account → Anthropic → Sign In; run a tiny task with it; Sign Out and Sign In again while a Claude run on the desktop login keeps working; both identities (fingerprints) and their Keychain entries stay separate; no credential in Overseer's database or logs.

### Gate J — daily-driver orchestrator UI (added by the owner, 2026-09-26)

Overseer should be the place to run Claude Code and Codex from, comfortable enough that the owner never opens them directly. It must be ultra clear, easy to approach and look polished: little text, generous padding, meaning carried by icons, status and layout instead of labels, and details only on demand. Design direction, principles and references: [orchestrator UI RFC](rfcs/orchestrator-ui.md). Screenshots and audits run in both Overseer themes and at least one stock VS Code theme. Codicons stay the icon style; provider logos are the exception (AC-65).

- [x] **AC-54 — Clean, calm presentation with less text.** Every Overseer surface is uncluttered and reads at a glance. Meaning is carried by icons (with tooltips), status dots, color and layout instead of labels and sentences; explanations move into tooltips, empty states and the details disclosure. Long values (paths, branch names, run/session/tool IDs, prompts, JSON) are shortened (home as `~`, middle ellipsis, first line, readable summaries) with the full value in a tooltip and a Copy action, never printed as walls of text. Each surface has one clear primary action, a few icon-only secondary actions and an overflow or ⌘K action menu for the rest; a disabled action explains why on hover. Padding, spacing, type sizes, radii and states follow one token scale across views. **Verify:** an automated audit opens every Overseer view (agents, chat, review, files, new agent, accounts, grid, dashboard) at 1280 px and 900 px wide in light and dark, fails on any horizontally overflowing element or any unbroken text run over 80 characters outside code blocks, and counts the visible text per view: at least 40% less than the recorded baseline of today's UI with no function lost; every icon-only control has a tooltip and an accessible name; before/after screenshots of each view; a list of the odd-looking items found in the current UI, each fixed.
- [x] **AC-55 — A chat that feels great.** The run conversation (AC-43) looks and behaves like the best modern chat apps: a centered, readable column (about 72 characters per line) with generous gutters and consistent spacing between turns; the user's messages as soft bubbles; agent replies rendered as Markdown (headings, lists, tables, links, syntax-highlighted code blocks with Copy), streamed smoothly without layout jumps; tool calls as quiet one-line chips with an icon ("Edited hello.md +1 −0", "Ran npm test ✓ 3 s") that expand to inputs and results; consecutive tool calls grouped into one collapsible line; permission requests as cards with a readable summary (never raw JSON) and Allow / Deny; native children as nested collapsible threads; a composer pinned to the bottom that grows with its content (Enter sends, Shift+Enter new line, Stop while running) and keeps focus; auto-scroll follows new output unless the user scrolled up, with Jump to latest; subtle motion only (no spinners where a status dot will do). **Verify:** live Claude Code and Codex runs and fixture runs rendered in light and dark (screenshots at 900 and 1600 px); Markdown fixtures with code, tables and long lines render correctly; while streaming, earlier content does not shift (layout-shift check); a 2,000-event conversation scrolls with p95 frame time under 16 ms and appends a new event in under 100 ms.
- [x] **AC-56 — Overseer themes, light and dark.** The extension contributes optional **Overseer Dark** and **Overseer Light** color themes: purple accents over silver and graphite metal neutrals, quiet borders, flat tabs and fewer separators so VS Code feels lightweight and modern. They cover the workbench, editor, diff, terminal, notifications and Overseer's views; Overseer's views keep using theme tokens so they also look right in any other theme, including high contrast. **Verify:** an automated check that every text foreground/background pair in both themes meets WCAG AA contrast; screenshots of the dashboard, a diff and a terminal in both themes; switching themes updates open Overseer views live.
- [x] **AC-57 — Overseer dashboard.** One command (and a button in the status bar and Overseer view) opens Overseer as a dashboard: the Overseer view fills the window and unrelated chrome (side bar, panel, minimap, breadcrumbs and whatever else VS Code lets an extension hide) is hidden for this window only; **Exit Dashboard** restores the previous layout exactly. The dashboard can open in its own window with no folder, and optionally when VS Code starts. **Verify:** entering and exiting restores the prior layout (compared before and after); the dashboard survives a reload (AC-49); it works in a window without a folder; no user or workspace setting changes persistently unless the user opts in.
- [ ] **AC-58 — Agent grid.** A toggle tiles agents like terminal panes: 1, 2, 2×2, 3×2 or 3×3 by count, up to a configurable maximum (default 6, at most 9). Each tile shows the agent's name, account and status and its live conversation in compact form, with a one-line composer and inline Allow / Deny; Enter or a click opens the agent in the normal chat and review; tiles can be pinned; arrow keys move between tiles. **Verify:** nine concurrent fixture runs stream in the grid at once, each tile updating within 250 ms of its event, with webview event-loop lag p95 under 50 ms; a permission request is answered from its tile; a pinned finished run stays; screenshots at 4 and 9 tiles in both themes.
- [ ] **AC-59 — Start a new agent from the chat.** With no agent selected, the middle of the Overseer view is a new-agent composer instead of an empty panel: type the task; repository, harness, account, model and workspace mode (new worktree by default) appear as compact chips with remembered defaults and can be changed inline; Enter starts the agent, which becomes selected and streams in place. Problems (untrusted workspace, signed-out account, harness not installed) appear inline with their fix. **Verify:** Codex, Claude and generic runs started keyboard-only from the composer; defaults remembered across reloads; each problem case shown inline; the full New Task form stays reachable.
- [ ] **AC-60 — Native-CLI parity for everyday use.** What people do in Claude Code or Codex directly works from Overseer: choose model, reasoning effort and permission/approval mode when starting and for the next turn where the harness supports it (otherwise shown as unsupported); attach files and paste images into a prompt; @-mention files from the run's worktree; steer a running agent (interrupt with a message, or queue it, per harness support); continue a finished run's session after VS Code or the daemon restarts. **Verify:** tiny live Claude Code and Codex runs exercise each capability; support per harness recorded in docs/compatibility.md; a pasted image and an @-mentioned file demonstrably reach the agent (its reply refers to their content).
- [x] **AC-61 — Needs-you inbox and keyboard control.** One **Needs you** list gathers agents waiting on a permission or question, failed runs and finished runs not yet reviewed, with counts in the status bar and on the view; keyboard shortcuts jump to the next waiting agent, allow or deny, switch agents (searchable quick pick), start a new agent, send and stop; every action in the Overseer view works without a mouse. **Verify:** a scripted keyboard-only session over three concurrent runs answers permissions, reviews changes and sends follow-ups without a click; all controls have screen-reader labels.
- [ ] **AC-62 — Usage and limits.** Each account shows its plan and, where the harness reports them, usage and rate-limit status (remaining share and reset time) and tokens or cost per run; starting a task on an account near its limit warns and suggests another compatible account. Nothing is invented: values a harness does not report are shown as not reported. **Verify:** live Codex and Claude runs show the usage their harness reports (or "not reported"), matching the harness's own output; the near-limit warning with fixtures.
- [x] **AC-63 — History that stays tidy.** The agents list shows active and recent runs by default; finished runs can be archived (by hand or automatically after a chosen age) and restored; a search finds any run by prompt, message text, file name, repository, account or status; worktrees of archived runs can be cleaned up in bulk with the existing safety checks. **Verify:** with a 300-run fixture, search answers in under 200 ms; archive, restore and bulk cleanup never discard unmerged work without confirmation.
- [ ] **AC-64 — Default-to-Overseer session (owner-confirmed).** The owner works for at least an hour using only Overseer for Claude Code and Codex (no direct CLI or app use), across at least two repositories and both harnesses, including the grid, a review and a merge back or pull request. Every friction point is logged and either fixed or turned into a follow-up with a reason. **Verify:** the owner's dated confirmation that they did not need to leave Overseer, the friction log, and the outcome of each item.
- [x] **AC-65 — Provider logos.** Accounts, harness choices, agent rows, grid tiles and the new-agent composer show each provider's real logo (Claude/Anthropic, OpenAI/Codex, OpenCode, GitHub for pull requests) instead of generic icons, taken from a source whose license permits redistribution in the extension (for example Simple Icons, CC0, or LobeHub Icons, MIT) and used as those brands' guidelines allow. Logos adapt to light, dark and high-contrast themes (monochrome where color would clash), stay crisp at small sizes, and every other icon stays a codicon. Where no suitably licensed logo exists, a neutral icon is used and the gap recorded. **Verify:** the license and source of every bundled logo recorded in a third-party notices file shipped in the VSIX; screenshots of each place a logo appears in both Overseer themes and high contrast.
- [ ] **AC-66 — Design review against references (owner-confirmed).** Before the owner's session (AC-64), the design is studied and reviewed, not guessed: the agent studies well-regarded products (listed in the orchestrator UI RFC), records what Overseer adopts from each, and publishes a visual review page with every view in both themes beside the before state. The owner marks what is not yet right; every marked item is changed and shown again until the owner confirms the UI looks clean and polished. **Verify:** the reference notes, the review page(s), the owner's marked items with their outcomes, and the owner's dated confirmation.

### Gate K — native sidebar, chat and diff together (added by the owner, 2026-09-26)

Owner decision (2026-09-26), after using the Gate J build: the native Overseer side bar becomes the one agents list (it replaces the dashboard's agent rail); the editor area shows the chat alone when there is nothing to review and the diff beside the chat once an agent has changes; the Workspace Dirty view folds into the review. Gate J criteria keep their IDs and must still hold in this layout (AC-81). Built in its own pull request at the owner's request. Design notes: [orchestrator UI RFC](rfcs/orchestrator-ui.md#gate-k-layout).

- [ ] **AC-67 — One agents list: the native side bar.** The Overseer view container (activity bar; movable above Explorer) is the only agents list: a **Needs you** section first, then agents grouped by repository, native children nested, status icon, title and relative time per row, and archived agents behind a filter. The editor area no longer shows its own agent rail. **Verify:** packaged-UI scenario over the AC-61 fixture set shows the sections, nesting and archive filter; the editor-area dashboard has no rail; the side bar's visible text stays within the Gate J agents budget (238 characters for the same fixtures).
- [ ] **AC-68 — Provider logos in the side bar.** Agent rows, accounts and Needs-you rows show the Claude, Codex, OpenCode and program marks as tree icons with light, dark and high-contrast variants (licensed sources, AC-65); every other icon is a codicon. **Verify:** screenshots in Overseer Dark, Overseer Light and High Contrast; the installed VSIX contains each variant and the notices.
- [ ] **AC-69 — Search and filter in the side bar.** A search action on the Agents view (and type-to-filter) finds agents by title, prompt, message text, file, repository, account or status through the daemon's search, shows matches in the tree with a clear-filter action, and works from the keyboard. **Verify:** with the 300-run fixture, results appear in under 200 ms, keyboard only; clearing restores the tree and selection.
- [ ] **AC-70 — Quiet row actions.** Each agent row offers stop, archive and pin-to-grid as hover icons with tooltips and accessible names, the rest in its context menu; the Overseer activity icon carries the Needs-you count as a badge; disabled actions explain why. **Verify:** every action is reachable by mouse, context menu and keyboard; the badge matches Needs you; accessible-name audit passes.
- [ ] **AC-71 — Take an agent out.** Dragging an agent from the side bar into the editor area opens its chat there (or pins it into the grid); where VS Code does not allow a drop, **Open to the Side** and **Pin to Grid** do the same and the limitation is recorded. **Verify:** scenario drags an agent into an editor group (or uses the recorded alternative) and the chat or tile opens for that agent.
- [ ] **AC-72 — Chat in the middle when there is nothing to review.** With no agent selected, or a selected agent with no changes, the editor area is one group showing the chat, or the new-agent composer (AC-59); the side bar's **+** opens the composer there. **Verify:** layout measured with no agent, with a fresh agent and after starting one: a single editor group with the chat or composer and no review open.
- [ ] **AC-73 — Changes bring the diff forward.** When the selected agent has changes (its first file edit, or on selecting an agent that has changes), the editor area becomes the editable review on the left (about two thirds) and the chat on the right (about one third) in the same window; closing the review returns the chat to the middle; selecting another agent keeps the chosen arrangement. **Verify:** fixture agent edits a file: the layout switches within 500 ms with the review on the changed file; closing the review restores the single chat; no settings change; the arrangement survives a reload (AC-49).
- [ ] **AC-74 — Follow or manual review.** In follow mode (the default while an agent works) the review shows the file and hunk the agent is editing; in manual mode it keeps the user's file, scroll and selection. One icon in the review header switches modes, remembered per agent. **Verify:** a fixture agent edits three files: follow mode moves with each edit; manual mode keeps the file and scroll position unchanged (measured).
- [ ] **AC-75 — One place for changes.** The separate Workspace Dirty view is removed. The review has one scope picker: **All changes** (default), **Staged**, **Unstaged** and **Untracked**, next to the existing comparison bases (latest turn, since task start, fork, another branch); conflicted files and unsaved drafts show as markers in the file list. Everything AC-22 and AC-27 require stays reachable here. **Verify:** a fixture with staged, unstaged, untracked, renamed, deleted, conflicted and unsaved files shows each under the right scope; the AC-22 and AC-27 scenarios pass against the review.
- [ ] **AC-76 — Review that stays clean at any width.** In the editable review, hunk Accept/Revert never covers code, file rows use codicons, very large diffs start collapsed with a count, headers stay on one line, and editing, saving and hunk actions (AC-42) keep working. **Verify:** overlap and overflow audit at 900, 1280 and 1600 px in both Overseer themes; the AC-42 hunk scenario passes.
- [ ] **AC-77 — Chat that works beside a diff.** At one third of the window (down to 360 px) the chat stays readable and usable: composer chips wrap, tool steps stay folded, code blocks scroll sideways, nothing overflows, and the header keeps the agent's name. **Verify:** audit at chat widths 360, 480 and 640 px in both themes: no overflow, no long runs, the name visible.
- [ ] **AC-78 — Quiet turn endings.** A turn the user stopped reads "Stopped" (no empty error card and no "Failed" footer); a failed turn shows its reason once; harness lines Overseer cannot parse stay in the event log instead of the chat. **Verify:** live-shaped fixtures for a stopped Claude turn, a stopped Codex turn and an unparsed line render as described in both themes.
- [ ] **AC-79 — Grid and dashboard mode in the new layout.** The grid (AC-58) opens in the editor area and returns to the previous arrangement when closed; dashboard mode (AC-57) hides unrelated chrome and keeps the Overseer side bar. **Verify:** enter and leave the grid and dashboard mode from chat-only and chat-with-diff arrangements: each returns exactly to the arrangement before.
- [ ] **AC-80 — Remembered place.** After a reload or restart Overseer reopens the last agent with its arrangement (chat only or chat with diff), follow/manual mode, review scope and scroll positions. **Verify:** reload and quit/relaunch scenarios compare each value before and after.
- [ ] **AC-81 — Gate J still holds.** The Gate J criteria keep passing in the Gate K layout: chat (AC-55), composer (AC-59), grid (AC-58), Needs you and keyboard (AC-61; shortcuts now also work from the side bar), history (AC-63), usage (AC-62), themes and logos (AC-56, AC-65), with the text budget (AC-54) re-measured. **Verify:** every Gate J scenario reruns green against the Gate K build; a new audit baseline is recorded for Gate K.
- [ ] **AC-82 — Gate K design review (owner-confirmed).** The review page shows every view before (Gate J) and after (Gate K) in both themes; the owner marks what is not right; each marked item is changed and shown again until the owner confirms. **Verify:** the published page, the owner's marks with outcomes and the dated confirmation.

### Gate L — offline mode and local models (added by the owner, 2026-09-26)

Owner request (2026-09-26): the work must always get done. A lost connection changes how, never whether. If one provider is down and another works, use the best working one. If nothing online works and *transitioning* is on, move the work to the best local Ollama model this machine's memory allows, downloading the model (and Ollama itself) only when the settings allow. If transitioning is off, say that Overseer is offline and keep retrying until a connection returns, offering local models for new agents meanwhile. Memory stays conservative: 40% of total by default, never above 50%, and never more than what is free now minus headroom, reassessed for every new run and turn. Design: [offline mode RFC](rfcs/offline-mode.md).

- [ ] **AC-83 — Offline is not an outage.** The daemon keeps one connection state, online, degraded (named providers unreachable) or offline (no network), from credential-free probes and a new `network` error class; auth, rate-limit and quota errors never change it; every change is an event with the reason and per-provider health. **Verify:** protocol tests with fixture probe results and fixture harness errors: one provider's hosts failing gives degraded naming that provider; the baseline failing gives offline; a 429 and a usage-limit error leave the state online; the status bar and side bar show each state.
- [ ] **AC-84 — Fail over to the best working provider.** With transitioning on, a run whose provider is unreachable continues on the best other online provider (the configured order) that has an installed harness and a signed-in account, through a handoff in the same task and workspace, announced in both chats; local is used only when no online provider works; with transitioning off the run waits (AC-92). **Verify:** fixture harnesses for two providers, one failing with network errors while the other works: the successor starts on the other harness within 30 s, the predecessor reads handed off (not failed), the review keeps the same worktree, the announcement names the reason; with both providers failing no failover happens and the offline policy applies; a live check with Codex's hosts blocked and Claude working, tiny prompt.
- [ ] **AC-85 — Local inventory.** `local.inventory` reports total and available memory, memory pressure, Ollama presence, version and running state, installed models with size, family, quantization, context length and capabilities, model geometry, loaded models with measured memory, and free disk; unknown is reported as unknown. **Verify:** on this machine the report matches `sysctl`, `vm_stat`, `/api/tags`, `/api/show` and `/api/ps` taken at the same moment; with Ollama stopped, and with it hidden from PATH and `/Applications`, the report says so without guessing; a protocol test with a fixture Ollama server.
- [ ] **AC-86 — Memory budget and fit.** The budget is `min(total × ceiling%, available − headroom)` with a 40% default and a 50% hard maximum; a model fits when its measured size, or the estimate `weights + KV cache(context) + 1 GiB`, is at most the budget; context shrinks from the target (64k) through 32k to the floor (16k) before a model is skipped; ranking is tier, then the largest fitting context, with models that fit only below 32k after those that fit at 32k or more; the budget is recomputed for every new run and turn; measured sizes from `/api/ps` are recorded and replace estimates. **Verify:** unit tests over machine profiles (16, 32, 64 and 128 GiB) and fixture inventories reproduce the RFC's worked-example table; on this machine the pick is `qwen3-coder:30b` at 64k or more and its estimate is within 15% of the measured size; a fixture inventory with 100 GiB in use drops the pick to a 14B-class model at 16k; a ceiling of 60% is refused; no pick is ever above the budget.
- [ ] **AC-87 — Verified local catalogue.** Overseer ships a catalogue of coding models with tiers and a verified mark per local harness; automatic picks use only models Ollama reports with `tools` and the catalogue marks verified (any `tools` model when `allowUnverifiedModels` is on); Codex `--oss --local-provider ollama` is evaluated as a second local harness. **Verify:** every catalogue model this machine can run within the budget completes the write check through the real OpenCode + Ollama path (and through Codex `--oss` where it works), with the result and versions recorded in the catalogue file and the ledger; a model that fails (as `qwen2.5-coder:14b` did on 2026-09-25) is excluded from default picks and shown as unverified in the UI.
- [ ] **AC-88 — Settings the daemon enforces.** The offline settings (transition, provider order, downloads, Ollama install, prefetch, ceiling, headroom, context target and floor, preferred and unverified models, local harness, return online, retry cap, stall, probes, idle stop, registry) are edited in VS Code, pushed to the daemon, persisted there, range-checked and enforced with VS Code closed; `overseerd ctl offline.status` prints state, budget and pick. **Verify:** change each setting in VS Code and read it back from `ctl`; quit VS Code and show the daemon still applies the transition and download settings in a fixture offline scenario; an out-of-range value is refused with the reason.
- [ ] **AC-89 — Download models only when allowed.** With downloads off, a pick that needs a model is reported as not installed and nothing is pulled; with downloads on, the pull streams progress in the chat, can be cancelled, resumes a partial download, is refused without disk space, never runs offline, and the first pull asks once with the size; prefetch keeps the best-fitting eligible model downloaded while online and never during a paid turn. **Verify:** Ollama's request log shows no pull with the setting off; a real pull of a small catalogue model shows progress and completes, a second is cancelled midway and resumed; a fixture with too little disk is refused; prefetch pulls exactly the model `local.pick` names and nothing else.
- [ ] **AC-90 — Install and run Ollama only when allowed.** With install off, a machine without Ollama is reported so and nothing is installed; with install on, Overseer uses Homebrew when present or the official archive after its Developer ID signature verifies, starts `ollama serve` on loopback only when nothing answers on the port, stops it after the idle time, and never stops an Ollama the user runs. **Verify:** with Ollama hidden from PATH and `/Applications`, the install path completes and `/api/version` answers; a tampered archive fixture fails verification and is deleted; the started server listens on `127.0.0.1` only; the user's own running Ollama keeps its pid across a session.
- [ ] **AC-91 — Transition to local when offline.** With transitioning on, a run whose turn fails with a network error (or stalls for `stallSeconds`) while the state is offline is handed off to a successor on the local harness with the budget's model and context: same task and worktree, a bounded handoff prompt (task, last messages, files changed, pending message), the predecessor marked handed off, and the chat saying *Transitioning to <model> (local, Ollama) because you've disconnected*; when no eligible model is installed it says why and waits (AC-92). **Verify:** a fixture harness failing with network errors and fixture probes reporting offline: within 30 s a real OpenCode + Ollama run starts with the AC-86 pick, completes the write check in the same worktree, both chats show the announcements, the run tree shows predecessor → successor with the reason, and the workspace has one writer throughout; the no-model case is shown and waits; the stall case interrupts and hands off with Overseer's action recorded.
- [ ] **AC-92 — Wait and retry, never fail.** With transitioning off, or when no target exists, a network-failed turn's run enters `waiting_for_connection`; Overseer retries after a passing probe with 5 s doubling to the cap with jitter, forever until stopped, delivers the pending message exactly once through the harness's own resume, records each attempt, and shows one quiet card with *Use a local model now* and *Stop*; one global Needs-you item counts waiting agents; new agents started offline offer local models only. **Verify:** fixture: harness network failures and probes offline for three minutes, then online: the turn resumes through resume (`--resume`, `exec resume` or `--session`) with exactly one delivery and no duplicate turn; the attempts and delays in the event log match the schedule; the card and the Needs-you item render in both themes; *Use a local model now* performs the AC-91 handoff; Stop ends the wait.
- [ ] **AC-93 — Back online.** When the state returns to online, local and failed-over runs finish their current turn; new agents default to their online harness and account; a local run offers *Switch back* and *Stay local* (or switches at the next turn with `returnOnline: auto`); switching back is a handoff that resumes the original harness's session when it still exists and summarises the local work otherwise. **Verify:** fixture offline → local → online: the composer's default is the online harness again; Switch back continues in the original harness with the same worktree and its session id when available; Stay local keeps working locally; `auto` switches at the next turn with the announcement.
- [ ] **AC-94 — Local models as a first-class choice.** Online or not, the composer and New Task list installed local models under Local with a fit badge (fits at 64k, fits at 16k, too big with the numbers, not installed with the download size); a run started on one shows the Local provider mark and tokens with cost 0 and needs no network; offline, the online harnesses are labelled offline and disabled with the reason. **Verify:** packaged-UI screenshots in both Overseer themes; a local run started from the composer completes the write check with the fixture network disabled; the badges match `local.pick`'s dry run.
- [ ] **AC-95 — Honest offline UI.** The status bar, side bar, chat cards, grid tiles and Needs you show online, degraded and offline with the reason, each transition once, local runs marked, waiting runs with a cloud icon rather than a failure, handed-off predecessors folded under their successor, and the Gate J text budget kept. **Verify:** an audit scenario at each state and after each transition, in both Overseer themes and a stock theme; no view claims online while offline; the text budget re-measured.
- [ ] **AC-96 — Several local agents.** Local runs on the same tag share one loaded model and a queued run says so; a second, different model loads only when both fit the budget; when available memory falls under the headroom the next turn's pick shrinks with one note and no running turn is killed. **Verify:** three fixture-driven local runs: `/api/ps` shows one loaded model and the queue note appears; a second model is refused when the sum exceeds the budget; a fixture memory squeeze changes the next pick and leaves the running turn alone.
- [ ] **AC-97 — Offline session (owner-confirmed).** With transitioning on and a verified local model installed, the owner turns the network off during a real run: the run transitions to local and finishes its task in the same worktree; turning the network back on offers Switch back, which continues in the original harness. **Verify:** the owner's dated confirmation, screenshots of both announcements, the run tree and the review at each step.

### Deferred platform qualification

- [ ] **AC-41 — Linux verification (deferred by owner).** Build/install daemon and VSIX, run regression checks, verify account isolation and complete the real VS Code flow on Linux. **Verify:** actual Linux OS/editor/harness versions, build/test logs, credential-backend evidence and UI recording. No Linux environment is currently available; leave unchecked. Portable design, macOS passes or cross-compilation do not satisfy it.

## Verification and completion rules

Statuses: `not started`, `in progress`, `implemented / unverified`, `blocked`, `verified`.
Only `verified` gets `[x]`. Count milestone progress separately from product completion.
Never delete, soften, defer or rename a failing AC solely to declare success. Scope changes
need a recorded owner decision; keep the original ID and explain any replacement.

The owner authorizes the implementer to record reproducible evidence and mark criteria
verified; a separate review pass can follow. No separate reviewer is required now.
Claims of complete feedback require known coverage, with gaps explicitly disclosed.

AC-01–03 are research gates. AC-04–40 and AC-42–66 describe the macOS implementation and
evidence scope; AC-41 retains Linux qualification as deferred and unchecked. A usable macOS milestone may
be delivered with documented unavailable native telemetry (AC-19) and inaccessible account
verification left unchecked. Codex/Claude account integration and OpenCode integration remain
required; two-account verification is never passed by simulation. Do not call partial
coverage “all ACs complete.” Agent access to the owner's two accounts is still unproven.

### Owner-directed scope revision

This planning revision supersedes the earlier target-base default with latest-run snapshots,
permits mocks/Ollama for OpenCode AC-14, permits usable harnesses with incomplete child
telemetry, and moves Linux evidence from AC-04/13/36/37 into AC-41. These are explicit owner
changes, not implementation waivers. No acceptance box was checked by this revision.

Revision of 2026-09-25 (owner, after the first macOS milestone): hunk accept/reject (AC-42)
and a structured run conversation view (AC-43) are added as acceptance criteria. Existing
IDs are unchanged; both new criteria start unchecked.

Second revision of 2026-09-25 (owner): AC-44 (merge back or open a PR; still never
automatic), AC-45 (visible background agents; they keep running), AC-46 (simple account
governance, [side RFC](rfcs/account-governance.md)), AC-47 (polished theme-compatible UI),
AC-48 (command-center layout) and AC-49 (restore the open session) are added unchecked.

Owner decisions of 2026-09-25 (third): AC-42 Accept only marks a hunk reviewed (no git
staging). AC-44 is merge back only, done by Overseer with Git and agent help for conflicts;
opening a PR moves to AC-50 (coming soon, via VS Code's GitHub sign-in). AC-48 is a
full-page Overseer view independent of the native sidebar and of the window's folder;
a worktree file tree becomes AC-51. AC-46 keeps OpenCode on local models and mocks for now.

Owner request of 2026-09-25 (fourth): background-agent notifications currently appear as
Script Editor's (they are posted with `osascript`). AC-52 adds native Overseer notifications
through a bundled helper app ([RFC](rfcs/native-notifications.md)); it starts unchecked.
The live Claude sign-in parts of AC-11 and AC-13 move to AC-53 (fixed Claude accounts), which
waits for a second Claude account; AC-11 and AC-13 keep the ChatGPT paths.

Owner request of 2026-09-26 (fifth): offline mode and local models. Gate L (AC-83 to AC-97)
adds a connection state that tells a provider outage from being offline, failover to the best
working provider, a memory-budgeted pick of local Ollama models, settings that gate downloads and
the Ollama install, transition to local with the owner's announcement, wait-and-retry that never
fails, and the way back online ([side RFC](rfcs/offline-mode.md)). All start unchecked. Quota-
and rate-limit-driven routing stays on the roadmap below.

## Later roadmap (not hidden first-release acceptance criteria)

Auto target selection, provider/subscription quota-aware routing (Gate L covers outages and offline only), escalation/review policies,
Overseer-launched delegation, remote workspace synchronization, TUI, VSCodium qualification,
Windows/Remote SSH, review comments sent to agents and marketplace distribution. Reassess each with
its own bounded acceptance criteria. Do not silently expand an overnight run to include these.

## Draft implementation goal (not started by this RFC revision)

> Implement Overseer in this repository against AC-01–41, with Linux verification deferred. Start with the feasibility gates,
> then deliver durable daemon/UI, account isolation and native child tracking, followed by
> live editable diffs and macOS verification. Reuse Branch Diff where justified. Keep
> this checklist and the verification ledger current, mark only reproducibly verified
> criteria complete, and continue independent work when credentials or platforms block a
> test. Preserve work and report exact remaining blockers. Never weaken the acceptance
> criteria to finish. A partial milestone is progress, not completion.

### Start gate and execution authority

**Start confirmation recorded (2026-09-24):** the owner started the overnight implementation
goal, authorizing implementation, dependency installation, tiny live harness runs on existing
subscriptions, local commits, pushes and PRs to `beelol/overseer` (no purchases, no automatic
merge, no login changes on the owner's behalf). The eight hours were the implementing
agent's session budget. No recurring automation was created.

For the later implementation session, the owner accepted the proposed limits: eight hours,
project dependency installation, existing subscriptions, local commits, branches/PRs allowed,
no new purchases and no automatic merge. The owner's follow-up asked whether the eight hours
referred to runs inside Overseer: our intended meaning is the implementing agent's session,
not an eight-hour allowance for every child/test run. Confirm that distinction when obtaining
the start confirmation; do not silently transfer a session budget to product-launched runs.

For verification, use only tiny hello-world-style paid prompts and minimal token usage.
Use deterministic fixtures for parser errors, load, repeated/retry and long-duration tests;
use small local Qwen Coder via Ollama or mocks for OpenCode. Do not run expensive model races,
benchmark loops or unattended retry loops against paid accounts. Record actual usage when
exposed; unknown usage is not zero. Product-level per-run limits remain a future decision.

All project implementation and publication stay in the confirmed project repository. Other
repository access/work is limited to necessary inspection/verification; do not push to or
modify the upstream Branch Diff project or unrelated projects. Test mutations belong in
isolated disposable fixtures. The owner explicitly confirmed that “overmind” meant `beelol/overseer`;
this is the only authorized publishing destination.
Account login may require the owner later; blocked login does not waive evidence requirements.

## Source assessment

See [source notes](source-assessment.md) for the inspected Branch Diff revision, current
integration documentation, and distinctions between confirmed code and untested proposals.
