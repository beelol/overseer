# Usability and correctness audit, 2026-09-28

The owner's words were: "something's just not quite right about how you use it." The bar is that Overseer is clearly better than talking to Codex and Claude Code separately and relaying between them.

This audit reports what gets in the way of that bar. It builds nothing, and it leaves out what AC-216 to AC-235 already cover.

**How it was done**
- The core loops were walked by hand in an isolated dev Overseer: `scripts/dev up --name audit`, then `scripts/dev code --name audit` on main at `beb3cb81`, driven over CDP.
- It used fixture harnesses only: no paid turns, no live accounts. The dev instance was removed afterwards with `scripts/dev down` and `scripts/dev clean`.
- Two fixture repositories were used, `site` and `notes`, with five agents. The loops walked:
  - start an agent from home, watch it, answer its permission;
  - review it and merge it back;
  - run several agents at once, in the grid and the dashboard;
  - talk to Overseer, typed, and steer an agent through it;
  - Talk to Overseer, notifications and the TUI.
- The code was read in `extension/src`, `extension/media`, `daemon/src/overseer`, `daemon/src/voice`, `daemon/src/merge.rs` and `daemon/src/pr.rs`, and in `tui/`.
- Screenshots are in [`2026-09-28-usability/`](2026-09-28-usability/). They were taken at a 1440×900 window, which is a MacBook's default size.

**What this audit could not judge**
- The fixture's Overseer is scripted, so the quality of its answers is not judged here. The wiring it depends on is.
- Voice (no microphone) and the phone (pull request #10) were not exercised.

## Top 10, ranked by how much they hurt

1. **The front door starts agents, not Overseer.**
   - Why it hurts: home's box starts a bare agent in a new worktree, and the owner must pick the repo, harness, model and worktree. Talking to the orchestrator needs `@overseer` or an unlabelled rocket chip, so it is Claude Code with extra steps.
   - Fix: home's box talks to Overseer by default. Overseer decides what to start, and "start an agent directly" becomes the secondary choice.
2. **Overseer cannot choose the model, account or harness, and Auto routing is switched off.**
   - Why it hurts: every agent Overseer starts runs on Overseer's own harness and the default login. "Use Codex for this" or "use the cheap model" cannot be honoured, which is the core of what an orchestrator is for.
   - Fix: Overseer's start takes a harness, model, account, effort and mode. It goes through Auto routing, and Overseer says in one line why it chose them.
3. **Overseer never checks finished work, and cannot offer to merge it.**
   - Why it hurts: when an agent finishes, Overseer reports "done" from 400-character snippets. It cannot see tests or the whole answer, and a check-in turn is forbidden even to *propose* a merge. The owner still has to read the result and relay the next step themselves.
   - Fix: at an agent's end, Overseer reads its final message, diff and test output, says whether it did what was asked, and proposes the merge (or the fix).
4. **Failed and rate-limited agents are dead ends.**
   - Why it hurts: a usage limit shows as "Failed: turn reported failure; last error [rate_limit]: API Error: Request rejected (429)". Overseer is not told, has no retry on another account, and the grid hides failed agents.
   - Fix: a failure or limit reaches Overseer, which explains it in plain words and offers (at Auto, does) "continue on the other account or model".
5. **You are not told anything unless you are looking at VS Code.**
   - Why it hurts: Mac notifications only fire after VS Code closes. Inside VS Code, a permission toast is skipped whenever the Overseer view is open, and nothing announces "done" or "failed". Two permissions, two failures and one finish produced zero notifications.
   - Fix: a Mac notification for "needs you", "done" and "failed" when VS Code is not focused. Clicking it opens that agent.
6. **Home's input box disappears once you have talked to Overseer.**
   - Why it hurts: at 1440×900 the text box starts below the bottom of the view (818 px down a 798 px view, with overflow hidden). It stays mostly hidden after "Got it" is clicked.
   - Fix: the input is pinned to the bottom, the conversation scrolls above it, and the "What's next?" hero and banners go away once there is a conversation.
7. **You cannot reply to an agent at the moment it waits for you.**
   - Why it hurts: the grid tile's input is disabled ("Working…") while an agent waits on a permission, and the chat offers only "Message for when it finishes".
   - Overseer's steer to a blocked agent is queued "until its turn ends", which never comes, and the card says "Done".
   - Fix: a waiting agent always takes a reply (including "deny, and do this instead"), and Overseer says when an agent is blocked on the owner.
8. **⌥⌘Y approves a request you cannot see.**
   - Why it hurts: with the selected agent not waiting, the shortcut allows the *first* waiting agent's tool anywhere, sight unseen, and says nothing about what it allowed.
   - Fix: the shortcut answers only the agent on screen; otherwise it shows which request, from which agent, first.
9. **Merging is a hidden four-step modal chore, and nothing changes after it.**
   - Why it hurts: Merge Back lives only in the palette and menus, needs two modal dialogs full of branch names, and refuses when your own checkout is dirty or on another branch.
   - Afterwards the agent still reads "done", its review still lists every file, the chat shows "merge back" twice, and the worktree stays.
   - Fix: a Merge button on the finished agent, in its chat and its review, with one confirmation. Afterwards the agent reads "Merged into main" and offers Clean up.
10. **Opening an agent rearranges your VS Code.**
    - Why it hurts: selecting an agent collapses your split editors into one group (`setEditorLayout`). The dashboard hides tabs in every VS Code window through user settings and closes your panel and secondary side bar.
    - Fix: Overseer opens beside what you have. Its dashboard settings are scoped to its own window, and your layout is restored exactly.

## All findings, by area

Findings: 64 in total. Severity is H (high), M (medium) or L (low). "Seen" means reproduced in the dev Overseer; "code" means found by reading the code, with file:line.

### A. First run and the front door

1. **H Home defaults to starting an agent, not to Overseer.** Seen and code.
   - `extension/media/composer.js:10` sets `target = 'agent'`. The Send-to chip is an icon only (screenshot 02).
   - The conversation stays hidden until the owner has spoken to Overseer once (`extension/media/home.js:157-158`).
2. **M Nothing opens on first run.**
   - VS Code lands on Explorer, with VS Code's own "Build with Agent" chat beside it (screenshot 01). A toast asks whether to replace Explorer.
   - The Agents empty state says "No agent tasks yet. [New Task]", and its link opens the full form, not home.
3. **M Three ways to start, with different options.** New Agent (composer), New Task… (form) and Start Task with Quick Picks… (no effort, mode or images). The palette lists 68 Overseer commands and there is no walkthrough.
4. **M The "Continuity is on" banner sits on home until dismissed.** It is jargon for offline mode, and it pushes the input down (screenshot 12).
5. **L Chips truncate at a normal width.** "Claude Cod…", "Default mo…" and "New worktr…" (screenshot 02).
6. **M The Agent menu shows a raw error.** "Ollama is installed but not running (Ollama is not answering: http://127.0.0.1:9/api/version: Connection Failed: Connect error: Connection refused (os error 61))". Codex reads "not installed" in the header and "offline" on the row (screenshot 03).
7. **L The composer never reloads its data.** It is fetched once per webview (`composer.js:250`), so trust, installed CLIs and settings changes stay stale until the view is recreated.

### B. Overseer as the orchestrator (the bar)

8. **H No harness, model, account, effort or mode on Overseer's `start`.**
   - The `propose` schema lacks them (`daemon/src/overseer/mod.rs:106`).
   - The start falls back to Overseer's own harness: `a["harness"]…or_else(|| self.overseer_session()…)…unwrap_or("claude")` (`daemon/src/overseer/session.rs:1079`). It uses the default login.
   - Nothing in `overseer/` calls `auto_select`, `auto_route` or `account_booking`.
9. **H Auto routing, the part that "picks the account, agent, model and effort", is off by default and marked unfinished.** See `extension/package.json:260` (`overseer.experimental.autoRouting`: "**Unfinished.**"). No Auto option appears in the composer (screenshot 03).
10. **H Overseer cannot see what an agent produced.**
    - Conversation text is cut at 600 characters, and tool steps are 200-character summaries with no output (`daemon/src/overseer/control.rs:283-284`).
    - The digest keeps 3 messages × 400 characters (`digest.rs:14-15`). There is no whole-diff read and no test output.
11. **H A check-in turn cannot propose a merge, PR, archive or permission change.** `if class == CONFIRM && !owner_asked { bail!("{kind} happens only when the owner asks for it; this turn was started by {cause}") }` (`session.rs:590-591`). "It's done and looks right; merge it?" is impossible, even as a question.
12. **H Nothing brings a failed, limited or stuck agent to Overseer.**
    - Agent `error` events have no handler (`session.rs:1264-1316`), and the digest has no errors (`digest.rs:186-211`).
    - The check-in results are only `on_task|drifting|done` (`checkin.rs:378`).
    - Nothing triggers a turn on long silence or no progress.
13. **H The only retry is by hand.** There is no action to continue an agent on another account or model after a limit. Continuity moves only network and service failures (`continuity.rs:353`).
14. **M Missing orchestrator actions.** There is no hand-off from one agent to another, no split or fan-out, no compare, and no "tell me / do Y when X finishes".
    - `hold` accepts `release_on: agent_done`, but it is not in the schema.
    - `watch` and `swarm` are accepted by the daemon (`session.rs:17`) but are not in the tool's enum.
    - There is no tool listing repositories, accounts or quotas.
15. **M An explicit order still needs a second yes at Ask first.**
    - Typing "tell Write the pricing page copy to use a friendlier tone" produced a proposal card that needed Yes (screenshot 14).
    - Typed requests wait for yes on everything, even `pin`, a Look action (`session.rs:705`). Spoken ones go out after the settle window (`:701-704`).
16. **M Overseer's instructions are thin and are sent once.**
    - The first-turn text (`session.rs:29-32`) describes 8 of the 20 actions. It never asks Overseer to verify before reporting done, to keep agents on track, or to pick models, and it never says the current level.
    - It is not re-sent after a Continuity hand-off (`session.rs:321, 375`).
17. **M The per-turn state degrades as history grows.**
    - The roster keeps every run ever (`digest.rs:342`).
    - Past 32 KB the turn switches to a slim form without `repo`, `worktree` or changed files (`session.rs:338-341`). Around 40 agents in, Overseer no longer knows where to start agents.
18. **M The digest cuts the useful part.**
    - Prompts come first, then changed files, messages and asks, then a 4 KB cut from the end (`digest.rs:279-331`). The note "ask for a smaller range" refers to a parameter `agent` does not have (`mod.rs:263`).
    - The digest reads the oldest 5,000 events, not the newest (`digest.rs:130`).
19. **M Changed-file counts reset on restart.** The cache is in memory (`conflicts.rs:140`) and refreshed only for active agents (`:512`), so a finished agent "changed nothing" after a restart.
20. **M Proposals go stale on any status change and never remind.**
    - "Not done: X is now waiting_for_user, not running… Ask again." (`session.rs:790-797`).
    - There is no expiry, and no phone push for proposals (`gateway/push.rs:23-33`).
21. **M Starts are uncapped except by voice.** "More than three new agents are Confirm" (`control.rs:16-17`) is checked only in voice's `decorate` (`voice/request.rs:1802-1806`).
22. **M Overseer's answers are shown as raw Markdown on home and in the TUI.** "- \*\*Add sessions docs to notes\*\*: waiting_for_user (permission)" (screenshots 12 and 13, and `17-tui-overseer.txt`). The agent chat renders Markdown; home does not (`home.js:44`).

### C. Keeping you informed (Needs you, notifications)

23. **H There are no Mac notifications while VS Code is open, even when it is in the background.**
    - `background::notify` is called only when VS Code closes (`daemon/src/background.rs:141`), plus the Test Notification.
    - The in-VS Code permission toast is skipped whenever the Overseer view is visible (`extension/src/extension.js:290`). There is none for done or failed.
    - The dev instance logged no notification for two permissions, two failures and one finish.
24. **H With VS Code closed, a permission waits forever.** No timeout and no Mac notification (`daemon.rs:2871-2875`). The close notice says "Reopen VS Code to watch them" and never mentions the TUI (`background.rs:134-141`).
25. **M The permission toast does not name the agent.** "An agent is waiting for permission to use Write." (`extension.js:291`).
26. **M Needs you counts differ between surfaces.**
    - With the same five agents, the extension said "Needs you 4" (two approvals and two failures), and the TUI said "◆ 2 need you" (screenshot 09, `16-tui-grid-and-help.txt`).
    - The extension also counts "Review" rows for up to 7 days (`extension.js:108-136`). The badge inflates and real waits get buried.
27. **M ⌥⌘J (next that needs you) does nothing while a proposal is open.** The "Decide" item uses `run_id: 'overseer'` (`extension.js:123`), which `selectRun` cannot find and returns from silently (`:327-328`).
28. **M Needs you shows an agent twice.** It appears once under Needs you and once under its repository (screenshot 09). Nothing ranks or explains the four rows beyond "Approve" or "Failed".

(Needs you not clearing when an agent finishes is AC-228. The root cause was seen here: a "Review" row clears only on a *new* selection of the agent (`markReviewed` in `selectRun`), so an agent you watched finish stays in Needs you. See screenshot 05.)

### D. Watching and answering agents

29. **H ⌥⌘Y allows the first waiting request anywhere.** `waiting.find(... selectedRun ...) || waiting[0]` (`extension.js:659`), then nothing says what was allowed.
30. **H You cannot reply while an agent waits.**
    - The grid tile input is disabled with "Working…" (`extension/media/grid.js:195-197`, `ACTIVE` includes `waiting_for_user`; screenshot 10).
    - The chat says "Message for when it finishes" (screenshot 04).
    - The TUI says "a turn is running; wait for it or press x to interrupt" (`tui/src/app.rs:1179-1181`).
31. **M Permissions are Allow once or Deny only.** There is no "allow for this session", no "always allow this rule" and no deny with a reason (`adapters.rs:493-506`; `run-actions.js:69`). The daemon accepts a deny message (`server.rs:2743`) but no surface sends one. Plain Claude Code offers all three.
32. **M The permission bar truncates what it asks.** "Allow Create CHAN…", and it appears twice, in the transcript and in the bar (screenshot 04).
33. **M The grid hides failed and finished agents.** Only two of the five agents appeared (screenshot 10), and the tiles do not say which repository an agent is in.
34. **M Starting an agent squeezes four columns into the window.** The side bar, a Review that opens by itself, the chat, and VS Code's own chat (screenshot 04). File names read "R…" and "src/aut…".
35. **M The chat editor tab is always titled "Overseer",** whichever agent is open. Home is replaced by the agent's chat in the same tab, and getting back needs "New Agent" (⌥⌘N).
36. **L The agent's cost shows in dollars ($0.04) on a subscription login,** which the owner does not pay per turn.
37. **L The TUI hides its messages after 6 s** (`tui/src/app.rs:734-737`), including multi-step merge instructions and git errors.

### E. Review, merge and cleanup

38. **H Merge Back is palette or menu only, and takes two modal dialogs** (screenshots 06 and 07). They are full of branch names, absolute paths and "Conflicts go back to claude". The branch name is cut with a trailing hyphen: `overseer/add-session-refresh-so-expired-sessions-`.
39. **H After merging, nothing says it merged.**
    - The agent still reads "done". The review switches to "0 files … No changes for this comparison. Pre-existing dirty work stays listed in the Workspace Dirty view." (screenshot 11).
    - The chat shows "merge back" twice with no commit (screenshot 08). Unknown event kinds render raw (`conversation.js:503`).
    - The success message is a toast that collapses into the status bar.
40. **H Open PR can push unresolved conflict markers** (code, not reproduced). After merge back leaves the worktree mid-merge (`merge.rs:123-135`), `pr_plan` does not check for it (`pr.rs:78-84`). `commit_worktree` then runs `git add -A` and `commit --no-verify` (`merge.rs:228-236`), and the result is pushed.
41. **H Merge and PR commit every untracked file, skip hooks and show only a count.** `git add -A`, `commit --no-verify`, `push --no-verify` (`merge.rs:233-234`; `pull-request.js:87`). A stray `.env` gets published, and the owner's secret-scanning hooks are skipped.
42. **M A conflicted merge cannot be cancelled.** `workspace.merge_abort` exists (`server.rs:3134`), but no surface calls it.
43. **M Merging needs your own checkout clean and on the target branch** (`merge.rs:85-92`). Working in your checkout while agents run blocks every merge.
44. **M Overseer's merge skips the "what will land" step.** On conflicts it says "finish it from the review", and the review has no merge controls (`session.rs:1011-1024`).
45. **M After a Continuity hand-off, conflicts are sent to the dead run.** The result is "run handoff changed while preparing the follow-up; retry it", which can never succeed (`merge.rs:64`; `daemon.rs:1846-1847`).
46. **M The review's status text sticks and leaks internals.**
    - "Updating comparison…" stayed for minutes on a finished agent (screenshots 04-11).
    - "Following: README.md:3 (agent tool input (tool-input))" comes from `extension/src/review.js:393`.
    - A new file shows a red "1 −" empty line at the top.
47. **M Cleanup can lose unsaved edits or get stuck.** The dirty check reads only disk (`daemon.rs:3551-3589`). A worktree folder that is already gone can never be cleaned up.
48. **L Merged `overseer/*` branches and `refs/overseer/snapshots/*` are never deleted** (`daemon.rs:714-715`). There is no per-turn undo, although a snapshot exists for every turn.

### F. Words: internal strings shown to the owner

AC-219 and AC-228 cover voice cards. These are everywhere else.

49. **M Owner messages show run ids.** "tell @Write the pricing page copy (r-198adab9b2b0) to use a friendlier tone" (screenshot 13; `composer.js:100-104`).
50. **M Proposal cards show internals.**
    - "owner · vscode", "named · add", "held", and "Done" for something only queued (screenshot 14; `home.js:84-116`).
    - Overseer's text ended "(proposal p-c1fd9df9ff0f)". That is the tool result echoed by the fixture, but the id reaches the model's text.
51. **M Talk to Overseer shows "Used mcp__overseer__propose", "Shared the state of 5 agents", and "Done 0s 2 tokens" after every turn** (screenshot 15). The TUI shows "▶ started: Started …".
52. **M Failure text is raw.** "Failed: turn reported failure; last error [rate_limit]: API Error: Request rejected (429) · rate limited" appears in the side bar, the tooltips and the TUI.
53. **M Daemon errors are shown verbatim.**
    - `` `Overseer: ${error.message}` `` (`extension.js:299`).
    - "Prepare the merge first (state "idle")." (`merge.rs:194`).
    - "The run is still waiting for user" (`merge.rs:66`).
    - "(The proposal could not be made: no cadence "auto": choose off, done, every_turn or every:N)" (`session.rs:1208`, `checkin.rs:42`).
    - "Done: start failed: the codex harness is not installed on this Mac." (`session.rs:814`).
54. **L Lowercase harness ids and raw enums appear in pickers and dialogs.** "claude · waiting_for_user" (`pull-request.js:60`), "on-request / untrusted / never", "children: native", and an 8-character account fingerprint "fixture · bd97832e · desktop".
55. **L Account rows and tooltips show a hash and raw status output** (`views.js:597, 614`).

### G. Names

56. **M One thing has several names.**
    - Agent vs task vs run: "New Agent", "New Task…", "No agent tasks yet", "Copy run ID", "No run to open a pull request from".
    - Answering: "Allow once", "Allow", "Approve", "Yes" and "Allow Pending Request".
    - Landing: "Merge Back…", "Prepare Merge Back", "Complete Merge Back", and "Open PR" vs "Open Pull Request…".
    - The main view: "Open Overseer View", "Open Dashboard", "Toggle Dashboard"; the status-bar tooltip says "open the dashboard" but opens the Overseer view.
    - Changes: the TUI says "changes", the extension says "review".
57. **M "Dashboard" is one agent's review and chat, not an overview** (screenshot 11). The only overview is the grid, which hides failed and finished agents.
58. **L Stop shortcuts disagree.** The chat says ⌘., and the command is ⌥⌘. (`chat.js:28`; `package.json`). Talk to Overseer's "@" mentions files, while home's "@" mentions agents.

### H. Reliability of Overseer's session

59. **H Events are lost under load.** `Err(RecvError::Lagged(_)) => continue` (`session.rs:1254`) on a 4096-event bus, with no reconcile.
    - A lost `output` drops Overseer's reply.
    - A lost `turn_done` strands queued owner messages and agents' queued messages.
    - `conflicts.rs:494` does reconcile on lag.
60. **H Agents' questions and watcher findings are deleted before Overseer's first turn and after Start fresh.**
    - `DELETE FROM check_in_queue` when `run_id` is null (`checkin.rs:254-256`). The same happens at the daily cap (`:271`).
    - Asks are deduplicated forever (`channel.rs:307-309`), so asking again does nothing.
    - The agent was told an answer is coming.
61. **M A spoken request can say "Not sent … Nothing will be sent later." and then send it anyway.** The request closes at 45 s (`voice/request.rs:1351-1367`), but Overseer's turn is not cancelled, and its later proposal settles and goes out (`session.rs:699-704`).
62. **M The cause flag is session-wide.** A voice request that sets it (`voice/request.rs:851, 1124`) during a check-in turn lets that turn's proposals go out without a yes at Ask first (`session.rs:572, 701-704`).
63. **M The docked Talk to Overseer panel goes stale after Start fresh** (`overseer-chat.js:17, 36, 90-93`). The same conversation is shown twice, rendered differently (screenshot 15).

### I. Dev tooling (found while testing)

64. **H Test dialogs reached the owner's screen.**
    - During this audit, Merge Back in a `scripts/dev code --inspect` window opened a native macOS alert on the owner's screen ("Merge back overseer/add-session-refresh-so-expired-sessions- into main?"). The owner had to cancel it.
    - The UI harness sets `window.dialogStyle: custom` (`test/ui/harness.js:106`), but `scripts/dev code` does not. The background launch hides the window, but not its modal dialogs.
    - Also: `scripts/dev code` fails in a fresh worktree until two `npm ci` commands are run. The message says which.

## Compared with plain Claude Code or Codex

**What a plain CLI user has that Overseer makes harder:**
- one box, and it just goes;
- "don't ask again" and "no, do this instead" on a permission;
- a notification (or a terminal bell) when it is done or waiting;
- replying while the agent waits;
- the whole transcript. Overseer keeps 5,000 events per run, and the TUI drops many event kinds;
- `/undo` or a checkpoint rewind;
- `git diff` in the checkout they already have open, with their editor layout left alone;
- the harness's own error text, where Overseer wraps it in "turn reported failure; last error [rate_limit]".

**What Overseer should do for them that it does not yet:**
- choose the model and account and say why;
- notice a stuck, failed or limited agent and move it;
- read the finished work and say whether it did the job;
- offer the merge;
- carry context from one agent to the next (AC-231 covers the start);
- tell them outside VS Code.

AC-216 to AC-235 cover the conversation itself, focus, voice and the review's labels. The gap left is Overseer acting as the manager between those conversations.

## Proposed criteria

They are numbered from AC-236, the next free number on main at `d7020126`. None overlaps AC-216 to AC-235.

- [ ] **AC-236 — Home talks to Overseer first.** Home's box sends to Overseer by default, and Overseer starts, steers or answers. "Start an agent directly" (today's composer, with its chips) is one labelled choice away and remembered per owner. Home shows the conversation from the first visit, with a one-line hint of what to ask. **Verify:** in the packaged UI, a fresh profile's first Enter on home reaches Overseer and starts no agent unless Overseer does. The labelled "Start directly" choice starts one as today. Screenshots in the three themes.
- [ ] **AC-237 — Overseer starts agents on the right harness, model and account.**
  - Overseer's `start` takes a harness, model, account, effort and permission mode, chosen through Auto routing (on by default once its checks pass), within the owner's allowed accounts.
  - The owner can say "use Codex", "the cheap model" or "my other account". Overseer's reply says what it picked and why, in one line.
  - **Verify:** fixture conversations start agents on a named harness, a named model and a named profile, each confirmed in the run's arguments. An unnamed start follows the route pick and its reason appears on the card. A tool-schema test lists the fields.
- [ ] **AC-238 — Overseer checks finished work and offers the next step.**
  - When an agent finishes, Overseer reads its final message in full, its whole diff and its last test output, and says in one or two sentences whether it did what was asked.
  - It then proposes the next step (merge, PR, a fix message), even from a turn the owner did not start. The proposal still waits for the owner's yes.
  - **Verify:** a fixture agent that finishes with passing tests gets a "did it" verdict and a merge proposal. One that finishes with a failing test gets a fix proposal. Neither acts without a yes at Ask first. The verdict cites the test output.
- [ ] **AC-239 — Stuck, failed and limited agents come back to Overseer.**
  - An agent's error, usage limit, repeated failure or long silence (10 minutes with no event while running, by default) triggers an Overseer turn with the reason.
  - Overseer explains it in plain words and offers (at Auto, does) the fix: continue on another account or model, retry, or stop.
  - **Verify:** fixture agents in `ratelimit`, `failed-reason` and a silent mode each produce one Overseer turn and a card with a plain reason. "Continue on the other account" resumes the session on a second fixture profile. No raw `[rate_limit]` or HTTP code shows on any surface.
- [ ] **AC-240 — You hear about it outside VS Code.**
  - When VS Code is not focused (or closed), an agent needing the owner, finishing or failing posts one Mac notification, grouped per agent and naming it and what it needs.
  - Clicking it opens that agent in VS Code (or the TUI when VS Code is closed). A setting chooses which kinds.
  - The in-VS Code permission toast names the agent and shows even when the Overseer view is open on another agent.
  - **Verify:** with a dev daemon and an unfocused window, fixture permission, finish and failure each write one entry to the instance's `notifications.log` with the agent's title. A focused window writes none. Clicking the notification's open URL focuses that agent.
- [ ] **AC-241 — A waiting agent can always be answered.**
  - While an agent waits on a permission or question, its chat, its grid tile and the TUI accept a reply.
  - Permissions offer Allow once, Allow for this session (the harness's rule), and Deny with a note. The note reaches the agent as the reason.
  - Overseer's message to a blocked agent says it is blocked and asks whether to answer the permission, instead of reporting "Done".
  - **Verify:** packaged-UI checks that the tile and chat inputs are enabled on a fixture `permission` agent. Deny with a note sends the note (fixture stdin log). Allow for this session is not asked again for the same tool. A proposal to a blocked agent reads "blocked on your permission".
- [ ] **AC-242 — Keys act only on what you can see.**
  - ⌥⌘Y and ⌥⌘⌫ answer only the agent on screen. With none on screen, they show which agent and tool first.
  - ⌥⌘J moves to Overseer's open proposal when that is the next item.
  - Palette commands that need an agent (Merge Back, Stop, Clean Up, Send Follow-up) ask which one instead of doing nothing or sending an empty id.
  - **Verify:** a packaged-UI scenario with two waiting agents: ⌥⌘Y on agent A never answers B. ⌥⌘J reaches an open proposal. Each command from the palette with nothing selected shows a picker.
- [ ] **AC-243 — Merge from the agent, and it reads merged afterwards.**
  - A finished agent's chat and review have a Merge button (and Open PR where there is a remote), with one confirmation that lists the files.
  - Afterwards the agent reads "Merged into main (commit)" in the side bar, the chat, the grid and the TUI, and offers Clean up.
  - A conflicted merge can be cancelled from the same place. Open PR refuses a worktree in the middle of a merge. Untracked files are listed before they are committed, and hooks run.
  - **Verify:** the merge scenario merges from the chat's button. Every surface shows "Merged". Cancel restores the pre-merge worktree. A fixture mid-merge worktree refuses Open PR. A fixture `.env` is listed in the confirmation. A fixture pre-commit hook runs.
- [ ] **AC-244 — Opening an agent leaves your layout alone.**
  - Selecting an agent, opening home or the review never merges the owner's editor groups.
  - The dashboard's settings apply to its window only (workspace scope) and are restored on exit and after a crash.
  - Entering it closes no panel or side bar the owner opened.
  - **Verify:** a packaged-UI scenario with two editor groups and the secondary side bar open: selecting an agent keeps both. Entering and leaving the dashboard leaves a second window's `workbench.editor.showTabs` unchanged.
- [ ] **AC-245 — No internal words on any surface.**
  - AC-219 and AC-228's rule, extended to every surface: the extension's views, toasts, dialogs and tooltips, the TUI and home's cards.
  - None of these may show:
    - run, proposal or share ids (`r-`, `p-`, `sh-`, `w-`);
    - snake_case states (`waiting_for_user`);
    - tool names (`mcp__…`);
    - lowercase harness ids;
    - confidence tags (`tool-input`);
    - `answered_by · surface`;
    - raw git, HTTP or OS error text.
  - Markdown from Overseer renders as Markdown on home and in the TUI.
  - **Verify:** a text check over every packaged-UI scenario's DOM text and the TUI's rendered screens fails on any listed pattern. Home and TUI screenshots show rendered lists.
- [ ] **AC-246 — One name for each thing.**
  - "Agent" everywhere, never "task" or "run" for the owner. One start command (New Agent), with the full form reachable from it.
  - "Needs you" counts the same items with the same number in the extension, the TUI and the phone.
  - "Dashboard" is either an overview of all agents or is renamed.
  - **Verify:** a check over `package.json` titles and user-facing strings for the banned words. The same fixture state gives the same Needs-you count in the extension badge, the TUI header and the phone's list.
- [ ] **AC-247 — Home's input is always on screen.**
  - At 1280×800 and larger, with any length of conversation, home's text box and its chips are fully visible, and the conversation scrolls above them.
  - One-time banners (Continuity) collapse to one line and never push the input off screen.
  - **Verify:** packaged-UI screenshots at 1280×800 and 1440×900 with a 20-message conversation. A DOM check that `#task`'s bottom is within the viewport.
- [ ] **AC-248 — Overseer's session never drops what it was told.**
  - A lagged event stream reconciles from the store, as conflicts do.
  - Agents' asks, reports and findings are kept until Overseer's first turn (and across Start fresh and the daily cap).
  - Queued owner messages are retried when a turn fails to start. A voice request closed as "not sent" cancels anything its turn later proposes.
  - **Verify:** daemon tests force a `Lagged` error and still deliver the reply and the queued message. An ask made before Overseer's first turn is answered in it. A failed turn start is retried. A proposal arriving after its voice request closed is withdrawn.
- [ ] **AC-249 — Test windows never reach the owner's screen.**
  - `scripts/dev code --inspect` and every harness launch set in-window dialogs (`window.dialogStyle: custom`), so a modal from a background test window never shows as a macOS alert. Owner-check windows (AC-221) keep native dialogs.
  - `scripts/dev code` installs its packaging tools on first use.
  - **Verify:** a dev-instance scenario triggers Merge Back in a `--inspect` window and finds the dialog in the page's DOM. `scripts/dev code` in a fresh worktree packages without a manual `npm ci`.
