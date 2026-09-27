# Tracker: every open criterion, who owns it, where the work is

"Everything" means every row here. Each pass of the goal (`docs/goals/everything.md`) refreshes this table from the ledger (`docs/verification/records.py`, `docs/verification/README.md`) and from the pull requests, does the rows the goal owns, and watches the rows other agents own until their pull requests are finished and merged (AC-146, AC-157). A row leaves the table when its criteria are verified.

Owners: **goal** = the agent running the everything goal. **owner** = only the owner can close it. **agent: X** = another agent is building it; the goal watches it and merges it when that agent is done.

| Criteria | What | Owner | Where the work is | How it closes |
| --- | --- | --- | --- | --- |
| AC-41 | Linux verification | owner (deferred) | none | when a Linux machine is available |
| AC-53 | Fixed Claude accounts | owner | none | needs a second Claude account |
| AC-64 | Default-to-Overseer session | owner | none | owner works an hour in Overseer |
| AC-66 | Gate J design review | owner | review page (Gate J) | owner marks, or decides Gate K's review replaces it |
| AC-81 | Gate J still holds | goal | main | Claude half of the live Gate J scenario once the owner signs in to Claude Code |
| AC-83 to AC-98 | Continuity: offline mode and local models (Gate L) | agent: Claude Continuity | PR #9 (`claude/continuity-gate-l`, draft, at step 0 of 5; conflicts with main, its agent merges main as it goes) | watch; merge when finished; AC-97 needs the owner |
| AC-107 | Talk to Overseer | goal | merged on main (#11); the live Claude run in progress (the owner signed in on 2026-09-27) | the one live Claude turn |
| AC-114 | Gate K in the owner's VS Code | owner | none | owner tries the build |
| AC-115 to AC-137, AC-141 | Phone remote (Gate N) | agent: Claude phone app | PR #10 (`claude/phone-remote-vscode-control-b48a34`), a draft that says work continues | watch; merge when it is marked ready (the simulator milestone); AC-128 waits for AC-107; AC-133 and the iPhone parts need the owner |
| AC-178 | The phone app uses the owner's mark (app icon, Android monochrome, the door's flat mark, in-app logo) | agent: Claude phone app | not started; to be built on PR #10 | watch; asked on PR #10 |
| AC-179 | The Mac surfaces use the owner's mark (helper icon, menu-bar template image) | goal | the helper's icon is on main (#12) | verify the helper on main; the menu-bar part waits for a menu-bar item |
| AC-162 to AC-177 | Voice Mode (Gate R) | agent: Claude Voice Mode | design and criteria on main; its worktree branch `claude/voice-mode-orchestrated-agents-rfc-4c5d2c` is not on GitHub yet | watch; merge when finished; AC-176 needs the owner |
| AC-180 to AC-202 | Overseer itself: context, control and watches across agents (Gate S) | owner, then a new agent | none yet; design in `docs/rfcs/orchestrator.md`, prepared goal in `docs/rfcs/orchestrator-goal.md`. It can be built in parallel with Voice Mode (Gate R): they share the one Overseer session, whose daemon methods are the contract, and AC-107 moves into the daemon | not started, by the owner's decision (2026-09-27); when the owner starts it, one goal in its own worktree and pull request, the spike (AC-180) first; AC-202 needs the owner |
| Swarm criteria | Category-directed Swarm mode | agent: Codex Swarm | PR #3 (`codex/swarm-mode`); criteria in `docs/rfcs/swarm-mode.md` on that branch | watch; merge when finished; its criteria join the ledger |
| Auto criteria | Continuous task-aware agent selection | agent: Codex Auto | PR #2 (`codex/automode-rfc`); criteria in `docs/rfcs/auto-mode.md` on that branch | watch (it has 100+ commits not pushed: ask it to push); merge when finished; its criteria join the ledger |
| AC-146, AC-148, AC-149, AC-151, AC-152 | Follow-through (Gate P) | goal (AC-148: owner) | main | AC-146: the hourly schedule needs the owner's permission; AC-148: the owner's `workflow` scope; AC-149: three clean runs on a quiet machine; AC-151: the Codex live reruns; AC-152: the load test (running) |
| AC-156, AC-161 | Cover everything (Gate Q) | goal | main | AC-156: Auto's next merge of main; AC-161 closes last |

New criteria or new agents get a row the pass they appear.
