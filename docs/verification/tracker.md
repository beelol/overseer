# Tracker: every open criterion, who owns it, where the work is

Refreshed 2026-10-03 from main `37ca0331`, current draft PR checkpoints and the published plans. The already-started #52 run at `715ee85b` does not include this documentation update. **207 of 273 ACs verified; 66 open.** Every open AC appears once below. Auto, Swarm and TUI use their own evidence indexes; do not infer their status from the AC total. A row leaves only after the full Verify clause is evidenced in the ledger. Re-read criteria every goal pass, including owner edits.

The handoff has the active branches, agents, tests and owner decisions. Old blockers such as “wait for Swarm/phone on main” are recorded gaps to recheck, not reasons to wait on already-merged PRs.

| Criteria | Remaining work | Owner | Current action |
| --- | --- | --- | --- |
| AC-229, AC-230 | Voice read-back and permission modes | goal | #52 capture race has deterministic red/initial green; edge cases/review pending. Baseline full run passed Rust 1,483 and packaging, but Continuity UI failed; reviewed test correction awaits isolated rerun. No full pass. |
| AC-266–AC-270 | Text library/bindings, Clear prose and shared surfaces | agent: mods_design | [Draft #55](https://github.com/beelol/overseer/pull/55) at `adc4bccb`: Task 1 library, 13 fixture tests including helpers and 4 classification checks pass; AC-266 partial. Task 2 binding tests red (six), implementation underway; Task 3 delivery pending/unsupported. One compiler shared with voice; native/global/child qualification, surfaces and live prose quality remain gaps. |
| AC-271–AC-273 | Less tool noise isolation, native result adapters and measured reduction | goal + mods_design | Second requested bundle retained as required later scope. Not started; runtime isolation and each native boundary must be qualified before availability or savings claims. |
| AC-265 | Stop pauses queued messages | agent: queue_pause | Draft #53 frozen at `f02665a9`: structural guards and typed queue result fixed; focused tests and independent review pass. Fresh packaged/combined full verification pending. |
| AC-224, AC-225 | Per-agent and global goals | goal | Approved choices in handoff: 20 tries or 4 hours, native harness goals where supported, Overseer monitors each stop; design in orchestrator RFC. Implement and verify. |
| AC-128, AC-246 | Phone Overseer conversation and shared Needs you count | goal | Reviewed [phone Overseer plan](../superpowers/plans/2026-10-03-phone-overseer.md) published; authenticated shared conversation, one navigable Overseer Needs item, naming and cross-surface fixture/screenshots pending. AC-263 exact comparison labels are the narrow AC-246 exception; implementation not started. |
| AC-210, AC-213 | Phone development isolation and production pairing guard | goal | Reviewed [dev phone plan](../superpowers/plans/2026-10-03-dev-phone-isolation.md): pre-mutation instance guard, native pin/launcher and real discovery. Implementation and native dual-instance proof pending. |
| AC-216, AC-218, AC-220, AC-222, AC-223 | Remaining voice-session improvements | goal | Conversation continuity, voice quality, dictation, optional noise filtering and audio-volume behavior. Check each Verify clause separately; owner evidence where required. |
| AC-183, AC-185, AC-186, AC-188, AC-189, AC-190, AC-192, AC-195, AC-196, AC-199 | Orchestrator integration with Auto, Swarm and phone | goal | Reconcile deferred cases with current merged code and run their contract/UI tests. Old branch dependencies are stale; no automatic verification. |
| AC-200, AC-201 | Independent trust-boundary review and clean regression run | goal | [Draft #54](https://github.com/beelol/overseer/pull/54) at `c22da6fc`: recursive/escaped-secret fix, 4 units + 5 new regressions (9 with helpers) + 3 existing regressions pass; independent final review accepted. Broad AC-200 obligations and clean full/package verification remain partial. |
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
| Cactus Whistle comparison | goal | Existing eval independently audited; keep Whisper. Human-voice/current-hint follow-up waits for supplied recordings or a requested guided recording session. |
| Model selection and usage telemetry | goal, queued | docs/rfcs/model-priority-brief.md and after-current-work.md. Current Luna low-only rule controls live calls despite older broader proposals. |
| Site, later mods expansion, usability, undo/rewind | goal, queued | after-current-work.md; Mods implementation is authorized and tracked in AC-266–273. Undo/rewind remains later. |

New criteria and user requests get a row during the same pass. Mods AC-266 records the Task 1 partial checkpoint; AC-267–273 remain not started/unproved. Historical PLUG/MOD labels are not additional ledger IDs. Voice capture/completion race follows the queue fixes before the coordinator resumes Mods Tasks 2/3.
