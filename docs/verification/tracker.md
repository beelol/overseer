# Tracker: every open criterion, who owns it, where the work is

"Everything" means every row here. Each pass of the goal (`docs/goals/everything.md`) refreshes this table from the ledger (`docs/verification/records.py`, `docs/verification/README.md`) and from the pull requests, does the rows the goal owns, and watches the rows other agents own until their pull requests are finished and merged (AC-146, AC-157). A row leaves the table when its criteria are verified.

Owners: **goal** = the agent running the everything goal. **owner** = only the owner can close it. **agent: X** = another agent is building it; the goal watches it and merges it when that agent is done.

| Criteria | What | Owner | Where the work is | How it closes |
| --- | --- | --- | --- | --- |
| AC-41 | Linux verification | owner (deferred) | none | when a Linux machine is available |
| AC-53 | Fixed Claude accounts | owner | none | needs a second Claude account |
| AC-64 | Default-to-Overseer session | owner | none | owner works an hour in Overseer |
| AC-66 | Gate J design review | owner | review page (Gate J) | owner marks, or decides Gate K's review replaces it |
| AC-83, AC-97 | Continuity (Gate L): Wi-Fi off and on with the daemon logging it; the owner's offline session | owner | merged on main (#9); the rest of Gate L is verified | the owner runs `node test/local/wifi-live.js` [AC-83] and `node test/local/owner-session.js start` [AC-97] |
| AC-83, AC-97 (follow-up) | Continuity: the Wi-Fi loss simulated (the system's answer only; probes, timeouts and a real agent's lost connection run for real), a probe race fixed | agent: Claude Continuity | PR #15 (`claude/continuity-owner-evidence`), marked ready | merge (AC-146); the owner's own Wi-Fi and session steps stay on the owner list |
| AC-114 | Gate K in the owner's VS Code | owner | none | owner tries the build |
| AC-115 to AC-137, AC-141 | Phone remote (Gate N) | agent: Claude phone app (resumed on 2026-09-27; the goal handed Gate N back after a short takeover) | PR #10 (`claude/phone-remote-vscode-control-b48a34`); it merges `claude/phone-takeover` (main merged in, new methods classed, AC-178's icons, e2e fixes); records on main: 6 verified, 16 partial | watch; merge when it is marked ready; AC-128 waits for Talk to Overseer in the daemon (Gate S); AC-133 and the iPhone parts need the owner |
| AC-178 | The phone app uses the owner's mark (app icon, Android monochrome, the door's flat mark, in-app logo) | agent: Claude phone app | built on `claude/phone-takeover` (icons from `docs/design/brand/`), going into PR #10 | watch; screenshots with the phone's runs |
| AC-179 | The Mac surfaces use the owner's mark (helper icon, menu-bar template image) | owner | partial on main: the helper's icon is built from the mark and checked as installed | the owner's banner and Finder screenshots; the menu-bar part waits for a menu-bar item |
| AC-162 to AC-177 | Voice Mode (Gate R) | agent: Claude Voice Mode (started 2026-09-27 from `docs/rfcs/voice-mode-goal.md`) | PR #16 (`claude/voice-mode`), a draft; it was stacked on #14, which is now merged | watch; merge when ready; AC-176 and AC-177 need the owner |
| AC-182, AC-183, AC-185 to AC-201 | Overseer itself (Gate S): the partial criteria | goal (the Gate S agent handed it over; pull request #14 merged as `4c371ecd`) | main | each record states its gap: Swarm (AC-195) and route picking (AC-196) arrive with `claude/auto-swarm`, the phone client (AC-199) with pull request #10, VS Code screenshots of holds, watches and findings, and clauses whose literal form is an hour of waiting; AC-202 is the owner's |
| Swarm criteria (SWARM-01 to SWARM-64, S0 to S5) | Category-directed Swarm mode | goal (the owner stopped the Swarm agent on 2026-09-27; built together with Auto) | `claude/auto-swarm` (continues pull request #3's `codex/swarm-mode`); RFC `docs/rfcs/swarm-mode.md` on main | 4 verified, 57 partial, 3 unverified at the handover; next: merge onto Auto's shared booking, then a normal start-to-finish run (S0) |
| Auto criteria (AUTO-AC-01 to AUTO-AC-40) | Continuous task-aware agent selection | goal (the owner stopped the Auto agent on 2026-09-27; built together with Swarm) | `claude/auto-swarm` (continues pull request #2's `codex/automode-rfc`); RFC `docs/rfcs/auto-mode.md` on main | 14 of 40 verified at the handover; next: run and supervisor binding and recovery for the shared launch booking (`6abf8d71`), then one launch transaction for ordinary, Auto and Swarm starts |
| AC-146, AC-148, AC-149, AC-151 | Follow-through (Gate P) | goal (AC-148: owner) | main | AC-146: the hourly schedule needs the owner's permission; AC-148: the owner's `workflow` scope; AC-149: three clean runs on a quiet machine; AC-151: the four app-server Codex live runs (no low-effort setting in that transport); |
| AC-149 (UI suite) | Scenarios that miss only under load: center, audit's review count, review-width at 900 px, arrangement's first edit | goal | main | each passes on main one at a time at low load (2026-09-27, load about 12); AC-149's three clean full runs still need a quiet machine |
| T-NN (TUI) | Tiles show full temporary paths (reported by the Audio Mode agent) | goal | main | not reproduced from the recorded screens (only the new-agent form's Repository field shows a full path, as intended); needs the Audio Mode agent's exact case |
| AC-203, AC-204 | Takeovers and partial merges (Gate Q) | goal | main | AC-203 verified; AC-204 with the first finished slice (the shared account booking) |
| AC-156, AC-161 | Cover everything (Gate Q) | goal | main | AC-156: Auto's next merge of main; AC-161 closes last |

New criteria or new agents get a row the pass they appear.
