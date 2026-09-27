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
| AC-114 | Gate K in the owner's VS Code | owner | none | owner tries the build |
| AC-115 to AC-137, AC-141 | Phone remote (Gate N) | agent: Claude phone app | PR #10 (`claude/phone-remote-vscode-control-b48a34`), a draft that says work continues | watch; merge when it is marked ready (the simulator milestone); AC-128 waits for AC-107; AC-133 and the iPhone parts need the owner |
| AC-178 | The phone app uses the owner's mark (app icon, Android monochrome, the door's flat mark, in-app logo) | agent: Claude phone app | not started; to be built on PR #10 | watch; asked on PR #10 |
| AC-179 | The Mac surfaces use the owner's mark (helper icon, menu-bar template image) | owner | partial on main: the helper's icon is built from the mark and checked as installed | the owner's banner and Finder screenshots; the menu-bar part waits for a menu-bar item |
| AC-162 to AC-177 | Voice Mode (Gate R) | goal (no agent is building it: the design agent's branch has nothing beyond main and has been idle since 2026-09-26) | design and criteria on main | the goal builds it from zero after Gate S lands the shared Overseer session; AC-176 and AC-177 need the owner |
| AC-180 to AC-202 | Overseer itself: context, control and watches across agents (Gate S) | agent: Claude Overseer itself | PR #14; AC-180, AC-181 and AC-184 verified; AC-182, AC-183, AC-185 to AC-194 and AC-196 to AC-200 partial (the daemon, VS Code and the terminal; the live probes, the neighbours' contract tests and the phone to come) | watch; merge when finished (squash); it builds on Talk to Overseer (AC-107) |
| Swarm criteria | Category-directed Swarm mode | agent: Codex Swarm | PR #3 (`codex/swarm-mode`); criteria in `docs/rfcs/swarm-mode.md` on that branch | watch; merge when finished; its criteria join the ledger |
| Auto criteria | Continuous task-aware agent selection | agent: Codex Auto | PR #2 (`codex/automode-rfc`); criteria in `docs/rfcs/auto-mode.md` on that branch | watch (409 commits not pushed; asked on 2026-09-27 to push); merge when finished; its criteria join the ledger |
| AC-146, AC-148, AC-149, AC-151 | Follow-through (Gate P) | goal (AC-148: owner) | main | AC-146: the hourly schedule needs the owner's permission; AC-148: the owner's `workflow` scope; AC-149: three clean runs on a quiet machine; AC-151: the four app-server Codex live runs (no low-effort setting in that transport); |
| AC-156, AC-161 | Cover everything (Gate Q) | goal | main | AC-156: Auto's next merge of main; AC-161 closes last |

New criteria or new agents get a row the pass they appear.
