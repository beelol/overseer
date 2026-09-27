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
| AC-82 | Gate K design review | owner + goal | review page (Gate K), PR #8 | owner's round-3 marks, then AC-154 and AC-155 |
| AC-83 to AC-98 | Continuity: offline mode and local models (Gate L) | agent: Claude Continuity | PR #9 (`claude/continuity-gate-l`, draft, at step 0 of 5; conflicts with main, its agent merges main as it goes) | watch; merge when finished; AC-97 needs the owner |
| AC-99 to AC-107 | Overseer as the whole surface (Gate M) | goal | branch `claude/gate-m-theme`, stacked on PR #8: all nine built with their scenarios passing (AC-99 files in the review, AC-100 nothing shown twice, AC-101 Overseer's own reviewer, AC-102 immersive editor area, AC-103 the Overseer theme, AC-104 grid by dragging, AC-105 tracking from the grid, AC-106 Where am I, AC-107 Talk to Overseer); AC-107's live run waits for the owner's Claude sign-in | pull request after PR #8; the look waits on the owner's marks in the Gate M review (AC-108) |
| AC-108 | Gate M design review | owner + goal | review page (Gate M), to publish | owner's marks |
| AC-109 to AC-113 | Gate K follow-ups | goal | PR #8 (`claude/gate-k-followups`) | merge once the owner OKs round 3; record in the ledger |
| AC-114 | Gate K in the owner's VS Code | owner | none | owner tries the build |
| AC-115 to AC-137, AC-141 | Phone remote (Gate N) | agent: Claude phone app | PR #10 (`claude/phone-remote-vscode-control-b48a34`), a draft that says work continues | watch; merge when it is marked ready (the simulator milestone); AC-128 waits for AC-107; AC-133 and the iPhone parts need the owner |
| AC-142 | One Overseer mark everywhere | goal + owner | branch `claude/brand-mark` (stacked on Gate M): built, scenario-brand 9 of 9 | owner approves the single-colour mark and the Mac helper icon; merges after the Gate M pull request |
| AC-178 | The phone app uses the owner's mark (app icon, Android monochrome, the door's flat mark, in-app logo) | agent: Claude phone app | not started; to be built on PR #10 | watch; asked on PR #10 |
| AC-179 | The Mac surfaces use the owner's mark (helper icon, menu-bar template image) | goal | the helper's icon is on branch `claude/brand-mark` (AC-142); no menu-bar item exists yet | verify with AC-142's merge; the menu-bar part waits for a menu-bar item |
| AC-162 to AC-177 | Voice Mode (Gate R) | agent: Claude Voice Mode | design and criteria on main; its worktree branch `claude/voice-mode-orchestrated-agents-rfc-4c5d2c` is not on GitHub yet | watch; merge when finished; AC-176 needs the owner |
| (RFC draft) | Overseer itself: context, control and watches across agents | agent: Claude orchestrator RFC | branch `claude/orchestrator-agent-control-rfc-8e2009` (docs only, no pull request) | watch; it overlaps AC-107 (Talk to Overseer, built on Gate M): the two need to agree before either merges |
| Swarm criteria | Category-directed Swarm mode | agent: Codex Swarm | PR #3 (`codex/swarm-mode`); criteria in `docs/rfcs/swarm-mode.md` on that branch | watch; merge when finished; its criteria join the ledger |
| Auto criteria | Continuous task-aware agent selection | agent: Codex Auto | PR #2 (`codex/automode-rfc`); criteria in `docs/rfcs/auto-mode.md` on that branch | watch (it has 100+ commits not pushed: ask it to push); merge when finished; its criteria join the ledger |
| AC-146 to AC-153 | Follow-through (Gate P) | goal | branch `claude/gate-p-follow-through` (pull request drafted, waiting on the owner's OK to open): AC-147 `scripts/test-all` with its evidence; AC-148's workflow commit held for the `workflow` scope; AC-149 has a known flake: the keyboard scenario loses a shortcut or an Enter on PR #8's build and on Gate M's; AC-150 is the first-click problem seen again in the conversation scenario | build; merge after PR #8 so the full run on main is green |
| AC-154 to AC-161 | Cover everything, oversee the agents (Gate Q) | goal | main and PR #8 | build; AC-161 closes last |
| AC-162 to AC-177 | Voice Mode (Gate R) | owner, then a new agent | none yet; design in `docs/rfcs/voice-mode.md` | owner answers the RFC's open questions; then one goal in its own worktree and pull request, the spike (AC-162) first; AC-176 and the pick of the animation (AC-177) need the owner |

New criteria or new agents get a row the pass they appear.
