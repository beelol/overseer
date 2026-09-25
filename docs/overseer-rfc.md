# Overseer RFC and acceptance criteria

Status: implementation-ready draft, with explicit feasibility gates and proposed defaults.
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
milestone the completed product.

## Decisions from the owner

| Topic | Requirement |
| --- | --- |
| Runtime | Prefer Rust for the daemon; use normal VS Code extension technology for the UI. |
| Platforms | macOS and Linux first. |
| Primary UI | VS Code; daemon architecture must permit a future TUI. Building a TUI is not current scope. |
| Reuse | Reuse suitable open-source code with its license and notices preserved. Inspect before adopting. |
| Accounts | Account/subscription sign-in only for now. No API-key fallback. Demonstrate two distinct ChatGPT subscription accounts running simultaneously. |
| Harness coverage | Broad, extensible harness support, explicitly including OpenCode and investigation of Devin. Do not hard-code a closed provider list. |
| Workspace | Isolated worktree by default. Include an explicit current-checkout mode that preserves existing dirty work. |
| Editing | The review UI must edit the selected agent's actual worktree/current checkout. |
| Diff | Show branch changes plus dirty work, including main compared with main. |
| Base | Default to the target base; expose an alternative base with an icon and explain the actual comparison. |
| Follow | A checkbox follows edits across files and within the same file. Manual navigation should temporarily stop following. |
| Review | Continues refreshing while agents work; it is not a frozen snapshot. |
| Descendants | Recursive children and grandchildren, including native harness delegation. Investigate missing signals rather than assuming children cannot be exposed. |
| Routing | Auto mode is desirable later; initially let harnesses handle their own delegation. |
| Verification | Reproducible testing before checking off any criterion. Keep code, spec, and evidence in this repository. |

## Proposed defaults, distinguished from confirmed decisions

These resolve implementation details without presenting them as prior user decisions.
Changing them requires a recorded RFC revision, not a hidden implementation shortcut.

- Initial rich adapters: Codex, Claude Code, and OpenCode. Evaluate Gemini CLI and Devin
  in the compatibility matrix. Other executable harnesses have a generic process adapter.
  This is a finite first release, not a claim that every future harness is integrated.
- Native delegation observation comes first. Overseer-created child scheduling, Auto
  routing, races, and quota-based escalation are later work.
- Use SQLite for state/events and a versioned local protocol. Prefer a Unix socket with
  owner-only access. A localhost HTTP transport would also need authentication.
- Prefer structured harness protocols/events; use a persistent process/session backend
  where necessary. Evaluate tmux for interactive fallback. Do not force a structured
  harness into terminal scraping merely to standardize on tmux.
- Follow has `off`, `following`, and `paused by navigation` states. Manual selection of
  another file or deliberate scrolling away pauses it; an explicit Resume action starts
  it again. It does not resume on a timer and interrupt reading. Turning the checkbox
  off preserves file, selection, and scroll as far as the changing text permits.
- Review and Follow use the same live data. Review disables automatic navigation.
  Switching to another agent does not inherit Follow accidentally.
- Hunk accept/discard, review comments sent to agents, automatic merges, and automatic
  worktree deletion are later work. Editing and native editor undo are required now.
- VS Code stable on macOS and Linux is the release target. VSCodium compatibility is a
  later qualification, using stable APIs where practical; Windows and Remote SSH are deferred.
- No embedded inference model in v1. First exhaust structured events, session records,
  hooks and deterministic parsing. A speculative 512 MB model is not a dependency.

### Exact comparison semantics

For a selected run, let `H` be its current HEAD, `T` its configured target branch,
`M = merge-base(T, H)`, `S` its immutable task-start commit, `I` the Git index, and `W`
the current working tree. Target defaults to the repository's configured integration
branch, then its default branch; if neither can be resolved, ask for a target.

Default review is **M → W** (PR-style comparison against the target). Offer **S → W**
for changes since the task began. The base icon shows target ref, resolved commit IDs,
and the selected mode. Preserve Branch Diff's detected fork/parent information as a
separately labeled informational value; an inferred historical fork is not necessarily
provable. A direct target-tip comparison must not be mislabeled as merge-base review.

The owner's phrase “real base” was ambiguous. These explicit labels are the proposed
interpretation; target, merge-base, task-start and detected parent must never be collapsed
into one ambiguous “base” field.

The file list is the union of net branch differences, staged changes (`H → I`), unstaged
changes (`I → W`), and nonignored untracked files. Separate staged/unstaged inspection
must remain available when the net diff cancels out. A clean main/main comparison is
empty; a dirty main/main comparison shows dirty work, not every unchanged repository file.

Unsaved editor buffers are additional visible drafts, labeled unsaved. Saving targets the
selected worktree. External writes must not silently overwrite a user's draft. Git dirtiness
alone proves a file changed, not who changed it: pre-existing/user edits remain visible and
must not be falsely attributed to AI.

## Architecture and ownership

The extension owns presentation, user navigation, and editor buffers. The daemon owns
repositories, tasks, execution targets, runs, process lifecycles, worktree identities,
persisted events and capability state. An extension reload rebuilds its view from the daemon.

Core records:

- `Task`: user intent, repository, target ref, task-start commit, acceptance metadata.
- `AgentRun`: task, parent run (optional), harness/version, account profile, provider/model,
  workspace, native session ID, lifecycle, timestamps and supported metrics.
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

Only the checkboxes below are authoritative. All are currently unchecked.
Each **Verify** clause is required evidence, not a suggestion. Fixture tests complement
real integration tests; they cannot substitute for subscription, native-child, or UI tests.
Evidence and blockers live in [the verification ledger](verification/README.md).

### Gate A — feasibility and reuse (before architectural lock-in)

- [ ] **AC-01 — Harness capability survey.** Record pinned versions, supported sign-in, process/remote transport, prompt/control APIs, native child signals, resume, usage and workspace access for Codex, Claude Code, OpenCode, Gemini CLI and Devin. **Verify:** cite primary docs and record runnable probes for installed harnesses; label untested/missing access explicitly. Documentation alone cannot establish runtime support.
- [ ] **AC-02 — Account and child feasibility spikes.** Before freezing adapters, demonstrate two isolated Codex subscription sessions and native child capture for Codex, Claude Code and OpenCode, or record exact blocking behavior and the next investigated alternative. **Verify:** redacted live transcripts with versions and identities; an investigated blocker can complete this research criterion, but does not pass AC-12 or AC-19.
- [ ] **AC-03 — Reuse decision.** Assess Branch Diff first; compare Agetor, Parallel Code, Pane and XCB only as relevant candidates. Record selected commit, actual license, intended reused surface and obligations before copying code. **Verify:** source/license inventory and notices for all imported code; an explicit skip rationale is acceptable. No unverified license claims from the prior conversation.

### Gate B — durable daemon and VS Code control

- [ ] **AC-04 — Installable macOS/Linux foundation.** Rust daemon and extension build from a clean checkout with documented tool versions and dependency setup. **Verify:** clean builds and daemon smoke runs on both operating systems, with logs and platform versions.
- [ ] **AC-05 — Independent durable state.** Tasks, runs, parent relationships, workspaces and event history survive extension close/reload and daemon restart. **Verify:** run work, disconnect/reconnect UI, restart daemon, and compare identities/history; reconciliation must not duplicate tasks or spawn replacement work silently.
- [ ] **AC-06 — Honest process lifecycle.** Display queued/running/waiting-for-user/completed/failed/interrupted/disconnected or unknown states based on actual signals; expose exit reason. **Verify:** successful, failing, interrupted and externally killed processes, plus loss/recovery of the session connection. Silence cannot count as completion.
- [ ] **AC-07 — Persistent sessions.** Closing VS Code leaves active agents running; daemon recovery either reattaches to surviving agents or accurately reports lost sessions. **Verify:** a real long-running harness continues through UI closure, and a forced daemon crash reconciles without duplicate launches or false running status.
- [ ] **AC-08 — Local access boundary.** Only the intended local user can issue daemon commands; workspace trust is required before execution. **Verify:** socket/transport access rejection and an untrusted VS Code workspace cannot launch a harness; malformed requests cannot execute shell fragments.
- [ ] **AC-09 — Complete task controls.** From VS Code create a task, choose repository/target/account/harness, view output, send a follow-up, interrupt and resume where supported. **Verify:** an actual harness completes this flow; unsupported controls are disabled with an explanation, and a message reaches only its selected run.
- [ ] **AC-10 — Event replay and bounded output.** Reconnection uses a cursor/snapshot without duplicate children or missing retained events; raw output is inspectable with redaction and bounded retention. **Verify:** duplicate/out-of-order fixture events, reconnect after output burst, and a retention boundary that explicitly marks truncated history.

### Gate C — accounts and harnesses

- [ ] **AC-11 — Account profiles.** Users can add, name, select and sign into separate profiles through supported account login flows. No model-provider API keys are required or used as a fallback. **Verify:** login and reauthentication through the UI for each claimed supported account path, including a missing/expired login.
- [ ] **AC-12 — Two simultaneous ChatGPT subscriptions.** Two distinct paid ChatGPT account profiles run Codex tasks concurrently in separate worktrees. **Verify:** overlapping live timestamps, redacted distinct account identity evidence, and successful independent file edits from both. Two processes under one account or subscription-plus-API-key do not qualify.
- [ ] **AC-13 — Credential isolation.** Login, logout, refresh and expiry in profile A do not switch profile B's identity or corrupt its credentials/configuration. **Verify:** live A/B logout and login during B's work, restart both, and refresh/expiry fault tests for the actual credential backend on macOS and Linux; inspect logs/database for credential leakage.
- [ ] **AC-14 — Rich initial adapters.** Codex, Claude Code and OpenCode support account-authenticated launch, streaming output, prompt routing, interruption and truthful capabilities. **Verify:** a real edit/test/follow-up run for each, recording exact versions and limitations; unsupported resume or metrics are disclosed, not fabricated.
- [ ] **AC-15 — Generic harness fallback.** A configured executable can run with selected cwd/profile environment and interactive output/control without pretending to have rich telemetry. **Verify:** executable path and arguments containing spaces, exit failures, interruption, and visible “unknown” child/usage capabilities.
- [ ] **AC-16 — Permissions and limits.** Native permission requests remain actionable in the UI or attached harness session; sign-in failures, rate limits and quota exhaustion remain distinguishable. **Verify:** allow/deny and interrupt a waiting run, replay actual error formats, and ensure no silent approval or switch to an API key/account.
- [ ] **AC-17 — Compatibility truthfulness.** The UI and docs state support per harness, account path, version and capability; retain Gemini/Devin outcomes even if blocked. **Verify:** compare the matrix with live evidence. Token-only remote integration is blocked by the current accounts-only scope until explicitly resolved; it cannot be called supported.

### Gate D — recursive visibility

- [ ] **AC-18 — Recursive run tree.** Selecting a run exposes its children and descendants with identity, status, harness/account when known, and workspace association. **Verify:** at least three levels, shared and separate workspaces, duplicate events and delayed parents in fixtures; no cycles or duplicated nodes after reconnect.
- [ ] **AC-19 — Actual native children.** Capture real native child runs from Codex, Claude Code and OpenCode, attach them to the correct parent, and expose their available output/status. **Verify:** live delegation sessions for each plus a native grandchild where the harness supports recursive delegation. Explicitly document unsupported depth; a synthetic tree does not pass the live portion.
- [ ] **AC-20 — Evidence-backed inference.** Prefer structured events; investigate hooks/session files/deterministic output parsing when signals are missing. Mark inferred and unknown relationships visibly, retaining source evidence. **Verify:** positive/negative transcripts (including prose mentioning a child that never launched), parser-version mismatch, and incomplete telemetry. No inference is presented as complete ground truth.

### Gate E — workspace correctness

- [ ] **AC-21 — Worktrees by default.** Each independent writing task gets an identified branch/worktree without mutating the source checkout. **Verify:** simultaneous tasks change identically named files independently; source checkout stays unchanged; existing branch/path collisions are handled without deletion.
- [ ] **AC-22 — Current dirty checkout.** Explicitly choosing the current checkout records pre-existing staged, unstaged, untracked and unsaved work and leaves it intact. **Verify:** launch/edit/interrupt there, confirm original work remains, and distinguish initial dirtiness from observed run changes without inventing authorship.
- [ ] **AC-23 — Shared workspace ownership.** Native children can share their parent's workspace; independent writers cannot accidentally claim it. **Verify:** shared parent/child edits remain visible under the right runs, a conflicting unrelated writer is rejected, and read-only labels are used only when access is actually enforced.
- [ ] **AC-24 — Safe workspace retention.** Finished, failed and interrupted runs retain their work for review. Explicit cleanup reports dirty files and live users before removal and never removes the current checkout. **Verify:** untracked work, active child, interrupted task and current-checkout cleanup attempts preserve data.

### Gate F — live editable Branch Diff

- [ ] **AC-25 — Correct repository selection.** Selecting an agent opens that agent's worktree review, even outside the open VS Code workspace. **Verify:** two repositories/worktrees with identical relative filenames; edits and refreshes never leak to the other path.
- [ ] **AC-26 — Explicit bases.** Default review uses merge-base with the configured target; task-start comparison is selectable. The icon/tooltip exposes actual refs and SHAs and any detected parent separately. **Verify:** pre-existing feature commits, stacked branches, target advances, rebases, missing target and no common ancestor; unresolved bases produce an explanation rather than an empty success.
- [ ] **AC-27 — Complete dirty view.** Committed, staged, unstaged, renamed, deleted and nonignored untracked changes appear, including main/main. **Verify:** real Git fixtures for each type and a clean main/main control. Binary/oversized files remain listed with an explicit preview limitation; ignored files do not flood the list.
- [ ] **AC-28 — No cancellation blind spot.** Opposing staged and unstaged edits remain inspectable when the net base-to-worktree diff is empty. **Verify:** stage A→B, restore B→A without staging, and inspect both layers; also test staged deletion followed by untracked recreation.
- [ ] **AC-29 — Follow across and within files.** The checkbox navigates to observed agent edits, including new hunks in the already-open file. **Verify:** a live run alternates files and distant lines; unrelated user edits cannot falsely claim agent attribution. When only filesystem evidence exists, show that limitation.
- [ ] **AC-30 — Navigation ownership.** Follow off preserves position; manual navigation pauses Follow with visible Resume; Review and agent switching do not unexpectedly jump the user. **Verify:** select another file, scroll, uncheck, resume, and switch agents during continuous live edits; confirm caret/scroll behavior in captured UI evidence.
- [ ] **AC-31 — Live Review refresh.** File lists/hunks refresh without reopening or manual refresh, while preserving selection and drafts. **Verify:** writes, atomic replacements, staging, rename/delete, branch changes and a deliberately missed watcher event. Proposed bound: normal changes within 2 seconds after writes settle; reconciliation within 5 seconds on the recorded fixture.
- [ ] **AC-32 — Edit selected workspace.** Text in the working-tree side is editable and save writes only the selected run's file; base content stays immutable. **Verify:** edit/save/reopen in external worktree and current-checkout modes, confirm disk path/content, and undo/redo through the native editor. Keep unsaved buffers labeled.
- [ ] **AC-33 — Preserve conflicting drafts.** An agent's external edit, branch/base change, file deletion or view reload cannot silently overwrite or discard an unsaved user draft. **Verify:** overlap edits at the same line, preserve both versions for reconciliation/recovery, and recover a pending draft after reload.
- [ ] **AC-34 — Safe file boundaries.** Writes cannot escape the selected workspace through path traversal or symlinks. Unsupported/binary/oversized/conflicted files have truthful states and safe native-editor access where applicable. **Verify:** traversal, escaping symlink, rename during edit, invalid encoding, merge conflict and large-file fixtures.
- [ ] **AC-35 — Responsive review.** Large output/diff bursts do not lock the extension or create unbounded watchers, queues or retained text. **Verify:** a recorded fixture of 10,000 tracked files, 100 changed text files and four active runs for 10 minutes; navigation remains responsive (proposed p95 under 250 ms), refresh meets AC-31 for ordinary files, and measured memory/queue behavior stabilizes after draining.

### Gate G — release evidence

- [ ] **AC-36 — Packaged UI on both platforms.** A built VSIX and daemon install and complete the task→follow→edit→review flow in stable VS Code on macOS and Linux. **Verify:** OS/editor/tool versions, installation logs and screenshots or recordings from both; a cross-compile alone is insufficient.
- [ ] **AC-37 — Automated regression coverage.** Real Git fixtures cover comparison/workspace behavior; protocol/adapter tests cover replay, parsing, controls and failures. **Verify:** passing clean-checkout CI on macOS and Linux with named tests mapped to ACs; mocks are labeled and do not satisfy live-only criteria.
- [ ] **AC-38 — Reproducible acceptance ledger.** Every checked criterion links to evidence with tested implementation commit, environment, steps, expected/actual results and limitations. **Verify:** audit every checked ID; failures reopen affected criteria; missing credentials/hardware remain blocked, not waived.
- [ ] **AC-39 — Dogfood end to end.** Use Overseer to make a small real change to Overseer in an isolated worktree, inspect native delegation, follow edits, edit from review, run checks and preserve the result for review. **Verify:** live session and resulting diff/test evidence; do not automatically merge or publish agent-generated product changes.
- [ ] **AC-40 — Repository handoff.** README shows accurate current progress and links to this canonical checklist; docs explain install, accounts, capabilities, recovery and known blockers. Implementation/evidence intended for handoff reaches GitHub with no credentials. **Verify:** remote revision/file readback plus a fresh reader following setup instructions. Publishing this draft alone does not complete this release criterion.

## Verification and completion rules

Statuses: `not started`, `in progress`, `implemented / unverified`, `blocked`, `verified`.
Only `verified` gets `[x]`. Count milestone progress separately from product completion.
Never delete, soften, defer or rename a failing AC solely to declare success. Scope changes
need a recorded owner decision; keep the original ID and explain any replacement.

The implementer may record reproducible evidence and mark an AC verified. This is a
proposed workflow; it does not invent a requirement for a separate reviewer or owner UI
approval. Claims such as “100% harness feedback” require known coverage: preserve all
exposed relevant events and disclose gaps, rather than promise access to invisible internals.

AC-01–03 are research gates. AC-04–40 define the initial product release. All 40 must be
verified for the full goal to be complete. A compatibility survey may document a blocker;
that does not excuse a required implementation criterion. Two real ChatGPT accounts and
real Linux UI access are external test prerequisites and cannot be simulated into passing.

## Later roadmap (not hidden first-release acceptance criteria)

Auto target selection, provider/subscription quota-aware routing, escalation/review policies,
Overseer-launched delegation, remote workspace synchronization, TUI, VSCodium qualification,
Windows/Remote SSH, hunk actions/comments and marketplace distribution. Reassess each with
its own bounded acceptance criteria. Do not silently expand an overnight run to include these.

## Draft implementation goal (not started by this RFC revision)

> Implement Overseer in this repository against AC-01–40. Start with the feasibility gates,
> then deliver durable daemon/UI, account isolation and native child tracking, followed by
> live editable diffs and platform verification. Reuse Branch Diff where justified. Keep
> this checklist and the verification ledger current, mark only reproducibly verified
> criteria complete, and continue independent work when credentials or platforms block a
> test. Preserve work and report exact remaining blockers. Never weaken the acceptance
> criteria to finish. A partial milestone is progress, not completion.

Before activating an unattended implementation goal, record its time/resource budget and
publication authority for implementation changes. The conversation authorizes finishing
and publishing these project documents, but does not establish an unlimited overnight
execution/spend budget or authority to publish arbitrary future product changes. Account
login may require the owner; do not make that block unrelated implementation work.

## Source assessment

See [source notes](source-assessment.md) for the inspected Branch Diff revision, current
integration documentation, and distinctions between confirmed code and untested proposals.
