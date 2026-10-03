# Tracker: every open criterion, who owns it, where the work is

Refreshed 2026-10-02 from main at `cd0068ca`, the generated AC records, current PRs and worktrees. **207 of 265 ACs verified; 58 open.** Every open AC appears once below. Auto, Swarm and TUI use their own evidence indexes; do not infer their status from the AC total. A row leaves only after the full Verify clause is evidenced in the ledger. Re-read criteria every goal pass, including owner edits.

The handoff has the active branches, agents, tests and owner decisions. Old blockers such as “wait for Swarm/phone on main” are recorded gaps to recheck, not reasons to wait on already-merged PRs.

| Criteria | Remaining work | Owner | Current action |
| --- | --- | --- | --- |
| AC-229, AC-230 | Voice read-back and permission modes | goal | #52 takes over #49; red/green fixes for Auto confirmation, native permission answers/queue and complete task read-back; full regression pending. |
| AC-265 | Stop pauses queued messages | agent: queue_pause | codex/queue-pause; daemon, voice, chat/grid and TUI evidence required. |
| AC-224, AC-225 | Per-agent and global goals | goal | Approved choices in handoff: 20 tries or 4 hours, native harness goals where supported, Overseer monitors each stop; design in orchestrator RFC. Implement and verify. |
| AC-128, AC-246 | Phone Overseer conversation and shared Needs you count | goal | Phone gateway/client integration and parity verification; Gate S and phone are already on main. |
| AC-210, AC-213 | Phone development isolation and production pairing guard | goal | Build and verify against merged phone/dev-daemon code; old waiting-for-#10 blocker is stale. |
| AC-216, AC-218, AC-220, AC-222, AC-223 | Remaining voice-session improvements | goal | Conversation continuity, voice quality, dictation, optional noise filtering and audio-volume behavior. Check each Verify clause separately; owner evidence where required. |
| AC-183, AC-185, AC-186, AC-188, AC-189, AC-190, AC-192, AC-195, AC-196, AC-199 | Orchestrator integration with Auto, Swarm and phone | goal | Reconcile deferred cases with current merged code and run their contract/UI tests. Old branch dependencies are stale; no automatic verification. |
| AC-200, AC-201 | Independent trust-boundary review and clean regression run | goal | Review by someone other than builder; missing packaged-flow evidence and clean build suite. |
| AC-244 | Opening agents preserves layout | goal | Recheck against merged one-layout behavior; old Focus Mode design blocker may be obsolete. Verify current single-folder and workspace flows. |
| AC-178 | Phone branding | goal | Inspect merged brand assets and collect full Verify evidence; ledger remains not started. |
| AC-234 | Automatic deployment after green merges | goal | Implement only after required CI checks are reliable; installation/deploy to owner remains an explicit owner action. |
| AC-146, AC-148, AC-149, AC-151, AC-156, AC-161, AC-204 | Merge coordination and release verification | goal | Refresh stale records; required CI/full suite; three quiet-machine clean runs for AC-149; live harness checks obey Luna low-only rule; AC-161 closes last. |
| AC-115, AC-120, AC-129, AC-135, AC-136, AC-137 | Phone gaps needing implementation and device evidence | goal + owner | Check per-AC evidence: discovery, push, speed/motion and real iPhone. Android door #24 already merged; quiet 20-run measurement still owed. Simulators serial and only when load allows. |
| AC-117, AC-133 | Real phone pairing and usage session | owner | Guided iPhone checks; do not fabricate simulator coverage for camera or real network permissions. |
| AC-162, AC-163, AC-164, AC-176, AC-177 | Voice microphone, speakers and real-room checks | owner | Use scripts/dev test voice-mode when requested; preserve exact pending gaps in each evidence record. |
| AC-179, AC-240 | Real macOS notification appearance and click | owner | Guided dev-daemon checks; banner/Finder mark and click into VS Code or TUI. Automated route coverage is already recorded. |
| AC-205 | Real Wi-Fi loss and recovery | owner | Guided check when no agents are in flight; never switch network without owner participation. |
| AC-64, AC-66, AC-114, AC-202 | Owner design and daily-use acceptance | owner | Dated owner marks and usage sessions; fixtures cannot close these. |
| AC-53 | Separate Claude accounts | owner (deferred) | No Claude paid runs under current rule; do not access or change owner credentials. |
| AC-41 | Linux verification | owner (deferred) | Requires suitable Linux environment and the full Verify clause. |

## Separate criteria and requested follow-ups

| Scope | Owner | Current action |
| --- | --- | --- |
| AUTO-AC-01 to AUTO-AC-40 | goal | Reconcile docs/verification/auto-mode/README.md with each Verify clause; previous summary says 23/40 fixture-verified. Live/account and packaged-UI gaps remain; keep experimental routing gated. |
| SWARM-01 to SWARM-64; S0 to S5 | goal | Reconcile docs/verification/swarm/coverage.json; previous summary says 46/64 fixture-verified. SWARM-24 adapter, UI, real communication paths and scenario closure remain; owner starts Swarm. |
| TUI T-01 to T-41 | goal watches regressions | T-29/T-35 merged with #51; all checked in RFC. Reopen only against evidence or changed requirements. |
| Mods and two bundled options | goal (design recorded) | First release design, Clear prose rules and evaluation rubric recorded in docs/rfcs/mods-first-release.md and docs/design/mods/. No implementation yet. Next builder slice: library/bindings and Clear prose; then qualified result transformers. |
| Cactus Whistle comparison | goal | Existing eval independently audited; keep Whisper. Human-voice/current-hint follow-up waits for supplied recordings or a requested guided recording session. |
| Model selection and usage telemetry | goal, queued | docs/rfcs/model-priority-brief.md and after-current-work.md. Current Luna low-only rule controls live calls despite older broader proposals. |
| Site, later mods expansion, usability, undo/rewind | goal, queued | after-current-work.md; user now explicitly pulled Mods planning forward. Undo/rewind remains later. |

New criteria and user requests get a row during the same pass. Do not promote draft Mods labels to verified acceptance criteria.
