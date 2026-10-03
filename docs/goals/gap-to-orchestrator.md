# What's missing before Overseer beats Codex and Claude Code directly

Written 2026-10-01 by the coordinator, at the owner's request ("what else is missing before it's better than codex and claude code and is a full orchestrator that can swarm and automatically pick the right models and switch when usage runs out?"). Local only: no cloud or relay. Keep it current as gaps close.

## Where it stands

- **The everyday surfaces work and are tested:** agents list, chat, the review (Since task start, Accept / Reject, Follow inside the review), one layout, the menu bar, the phone app, the terminal UI with grid, dashboard mode and the review. All of it is checked against fake harnesses (fixtures), merged and deployed.
- **Overseer's brain exists:** it picks harness, model and account for a new agent (AC-237), reads finished work (AC-238), brings stuck and rate-limited agents back with a plain reason and offers to continue on another account (AC-239, #48), and gives new agents what it knows (AC-231, #48).
- **Auto (picking models and accounts by itself) and Swarm (many agents on one job) are built but switched off** behind `overseer.experimental.autoRouting` and `overseer.experimental.swarm`. Auto: 23 of 40 criteria verified, Swarm: 46 of 64, all against fixtures.

## The gaps, biggest first

### 1. Nothing is proven on the real harnesses (the biggest gap)

Almost every check runs against fake Claude Code and fake Codex. The real ones change their output, limits and errors, and that is exactly where an orchestrator breaks.
- Auto's remaining 17 and Swarm's remaining 18 criteria need live runs on real accounts (Swarm: SWARM-01, 13, 17, 25-28, 31, 39, 52, 56, 63; S0, a normal swarm run, is still partial).
- The paid-turn rule (only gpt-5.6-luna at low effort, no Claude model) blocks every live Claude check, so "works with real Claude Code" is unproven.
- **Needs the owner:** a small, budgeted live test plan (which accounts, which models, how many turns). Without it, Auto and Swarm stay experimental.

### 2. Switching when usage runs out is reactive, not ahead of time

- Today an agent that hits its limit comes back to Overseer and is continued on another account (AC-239). It works after the failure.
- Missing: switching **before** the limit. Codex reports its rate limits (`account/rateLimits/read`); Claude Code reports almost nothing, so Overseer can't see Claude's remaining usage. Needs usage telemetry (queued: per-account usage from local files) and a rule that books the next turn on an account with room (Auto owns the one account booking).
- Missing: continuing **across harnesses** (a Claude Code session moved to Codex keeps no session; only Overseer's context block from AC-231 carries over). Needs a hand-off criterion: the new agent gets the old one's diff, plan and last messages.

### 3. Picking the right model is calibrated by hand

- Auto's choices come from the owner's decisions of 2026-09-28, not from measured results. The queued model-priority research (which model for which task, by usage and results) and the live consistency test the owner approved (personal ChatGPT Sol medium vs personal Claude Opus medium, same tasks) are not started.
- Missing: learning from outcomes (did the agent's work pass review, how many turns, how much usage) to adjust choices. Auto has a learning store; it has no real data yet.

### 4. Swarm isn't usable yet

- Behind a setting; four packaged-UI criteria (SWARM-18, 22, 23, 37) and SWARM-24's adapter are open; no live swarm has run.
- Missing for real use: a clear view of a swarm's progress and cost, and one-click stop, review and merge of all its pieces.

### 5. Where plain Claude Code or Codex still win

- **Undo / rewind:** Claude Code rewinds to a checkpoint; Overseer only has Reject per change or per file. Proposed new criterion.
- **History cap:** Overseer keeps 5,000 events per agent; long runs lose their beginning. The CLI keeps everything.
- **Exact error text:** plain reasons are friendlier, but the harness's own message should be one click away.
- **Harness features in Overseer's chat:** slash commands, skills, MCP status and hooks still work because the real harness runs, but Overseer's chat doesn't show or offer them. To check and list.
- **"One box and it just goes":** home talks to Overseer first; the fastest path to "start an agent with this" must stay one step (AC-252, measured, not started).

### 6. The manager loop

- In progress: AC-229 (Overseer says what it heard before acting), AC-230 (permission modes per conversation).
- Not started: goals for an agent and for Overseer (AC-224, AC-225; design first), deploys that follow merges by themselves (AC-234), AC-252 (the five commonest actions in one step, measured).
- Partial: you hear about things outside VS Code (AC-240; the menu bar covers needs-you).

### 7. Trust in the build itself

- AC-149: three clean full test runs on an idle Mac. Two focus checks fail whenever the owner is at the Mac; they need overnight runs.
- AC-64: the owner works an hour in Overseer for real; that finds what tests can't.

### 8. Phone and voice

- Talking to Overseer from the phone (AC-128) isn't started.
- Voice Mode's follow-ups (AC-216, 218, 220, 222, 223) wait for the owner's voice session in a dev daemon.

## Order the coordinator proposes

1. Finish what's running: #47, #48 (merge check now), AC-229 and AC-230.
2. A live test plan for the owner's yes (gap 1), then the live Auto and Swarm runs inside it.
3. Switching ahead of the limit and across harnesses (gap 2), with usage telemetry.
4. Undo / rewind and the history cap (gap 5).
5. Swarm's packaged UI and a first real swarm (gap 4); then turn Auto and Swarm on by default.
6. Goals (design shown first), AC-252, AC-234, phone AC-128.

## The owner's answers (2026-10-02)

- Live tests: up to $10, on the account logins (subscription usage); whether the Claude login is included was asked.
- Undo / rewind: later. Undo reverses an agent's file changes; rewind pauses the current task and undoes its last step.
- Swarm stays the owner's to start; Overseer may suggest it for a giant or mixed task and ask.
- Auto (the permission mode) set by Overseer itself: none by default, and it always tells and asks first.

## Decisions only the owner can make

1. A live-run budget: which accounts (personal Claude, personal and work ChatGPT), which models, roughly how many turns, and whether Claude may be used for these checks.
2. Undo / rewind as a new criterion now.
3. When Auto and Swarm should be on by default (after their live runs pass?).
