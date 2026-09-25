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

- [ ] **AC-11 — Account profiles.** Users can add, name, select and sign into separate profiles through supported account login flows. No model-provider API keys are required or used as a fallback. **Verify:** login and reauthentication through the UI for each claimed supported account path, including a missing/expired login.
- [x] **AC-12 — Two simultaneous ChatGPT subscriptions.** Two distinct paid ChatGPT account profiles run Codex tasks concurrently in separate worktrees. **Verify:** overlapping live timestamps, redacted distinct account identity evidence, and successful independent file edits from both. Two processes under one account or subscription-plus-API-key do not qualify.
- [ ] **AC-13 — Credential isolation on macOS.** Login, logout, refresh and expiry in profile A do not switch profile B's identity or corrupt its credentials/configuration. **Verify:** live A/B logout/login in dedicated test profiles during B's minimal work, restart both, and refresh/expiry fault tests for the actual macOS backend; inspect logs/database for leakage. Do not disturb unrelated active logins. Linux coverage belongs to AC-41.
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

- [ ] **AC-50 — Open a pull request from a run (coming soon).** Beside Merge back, an **Open PR** action pushes the run's branch and creates a pull request with a generated description, using the GitHub sign-in VS Code already has (no personal access tokens pasted into Overseer). Disabled with an explanation when there is no GitHub remote or no VS Code GitHub session. **Verify:** a PR created from a live run against a repository the owner chooses; missing remote and signed-out cases explained; no automatic merge.
- [ ] **AC-51 — Worktree file hierarchy.** The Overseer view can show the selected run's worktree as a file tree (changed files highlighted, open any file in the editor) without depending on the folder open in the VS Code window. **Verify:** browse and open files in worktrees of two different repositories from one window; changed files marked; large repositories stay responsive.
- [ ] **AC-52 — Native Overseer notifications (macOS).** The background-agent notification (AC-45) comes from Overseer itself rather than Script Editor: the banner shows Overseer's name and icon, clicking it opens VS Code at the Overseer view, and *System Settings → Notifications* lists **Overseer** with its own switch. A bundled helper app posts the notification; if notifications are denied, Overseer falls back to the current method and records which one it used. Design: [native notifications RFC](rfcs/native-notifications.md). **Verify:** close VS Code with an agent running and see an Overseer-branded banner (owner-observed or screenshot); click it and land in the Overseer view; find Overseer in Notifications settings; deny permission and confirm the fallback banner and the recorded `delivered_via`; **Overseer: Test Notification** posts a sample banner.

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

AC-01–03 are research gates. AC-04–40 and AC-42–52 describe the macOS implementation and
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

## Later roadmap (not hidden first-release acceptance criteria)

Auto target selection, provider/subscription quota-aware routing, escalation/review policies,
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
