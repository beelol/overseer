# Side RFC: Overseer terminal UI (`overseer-tui`)

Status: proposed by the owner on 2026-09-26 as a draft pull request. Acceptance criteria:
T-01 to T-13 below (their own namespace, so they do not collide with the main RFC's AC
numbers while Gate J is in flight; they join the main ledger when this merges).

## Why

VS Code is one way to watch agents. When many run at once, a dense, keyboard-driven terminal
view is faster: nine agents live on one screen, jump between them, type to any of them, and
answer what they ask without leaving the keyboard. It must stay a view onto the same daemon
(`overseerd`), not a second orchestrator: everything VS Code sees, the TUI sees, live.

## Product decisions (made, not open)

- **Its own binary.** `overseer-tui` is a Rust crate in the workspace (`tui/`), built with
  ratatui and crossterm. It does not link the daemon; it speaks the daemon's protocol.
- **Same daemon, same stream.** It finds `overseerd` (env `OVERSEERD`, next to itself, on `PATH`,
  or inside the installed VS Code extension), asks it for the socket path, starts it if needed
  (detached, like VS Code), says `hello` as client `tui`, reads `state`, and subscribes to
  `events.subscribe` from the state cursor. Reconnects resume from the last cursor.
- **Pages of nine.** Top-level agents (runs without a parent), newest first, nine per page.
  Page 1 is the newest nine. Order is by creation time, so tiles do not jump when statuses change.
  A full page is 3×3; a page with fewer agents uses the space (1, 1×2, 1×3, 2×2, 2×3), and
  arrow keys follow the same shape. A filter cycles All → Active → Needs you.
- **Focus follows the agent, not the slot.** When a new agent arrives, the focused agent keeps
  focus even if it moves to another slot or page.
- **Tiles are compact conversations.** Agent text (light Markdown), one-line tool rows
  ("⚙ Edit README.md ✓"), file edits, permission requests, errors and turn results with tokens,
  newest at the bottom. Paths show relative to the agent's worktree or repository.
- **Type to any agent.** `i` (or Enter) opens a composer on the focused tile; Enter sends a
  follow-up to that agent only; drafts are kept per agent.
- **Keyboard first.** Every action has a key; `?` shows them. Mouse clicks focus tiles.
- **The TUI counts as a watching UI.** While a TUI is attached, the daemon does not treat the
  agents as running unseen (the AC-45 background notice), exactly like a VS Code window.

## Key map

| Key | Action |
| --- | --- |
| ←↓↑→ / h j k l | Move between tiles |
| 1–9 | Focus tile n on this page |
| Tab / Shift-Tab | Next / previous agent (across pages) |
| ] / [ , PgDn / PgUp | Next / previous page |
| J / K | Pick the next / previous agent in the agent list: its conversation opens in a column beside the grid (PgUp/PgDn, Home/End scroll it; e tool details) (T-25) |
| Esc | Close the picked agent's conversation; the grid stays on that agent (T-25) |
| L | Hide or show the agent list (T-25) |
| i, Enter | Compose a message to the focused agent (Enter sends, Esc closes, Alt-Enter new line) |
| g, z | The grid ⇄ the focused agent's full view: its whole conversation with scrollback (j/k, PgUp/PgDn, Home/G) and tool details; g, z or Esc returns to the grid on that agent (T-38) |
| v | Review: the focused agent's changes through the daemon, opening on its default comparison (Since task start) (T-27) |
| 1 / 2 / 3, c (review) | Since task start / Latest run / Entire worktree; c cycles every available comparison (T-27) |
| t (review) | Changed ⇄ All files (T-28) |
| j / k, n / p, J / K (review) | Next / previous file, next / previous change, scroll (T-29) |
| a / A (review) | Accept the change / every change of the file (T-29) |
| r / R (review) | Reject the change / every change of the file, after y/n (T-29) |
| e (review) | Open the file at the change in `$EDITOR` (else `vi`); the review refreshes when it exits (T-39) |
| Ctrl-R (review) | Reload the review (T-29) |
| e (zoom) | Expand or fold every tool call's input and result |
| a / d | Allow / deny the focused agent's pending permission |
| w | Jump to the next agent waiting for you |
| x | Interrupt the focused agent (asks y/n) |
| M | Merge back: commit the worktree and merge the target in (y/n), then merge into the target (y/n) |
| P | Open a GitHub pull request (y/n): commit, push with your Git credentials, create it with `gh` |
| C | Remove a finished agent's worktree (its branch is kept; lists uncommitted files first) |
| X | Stop all agents and the daemon (y/n); the TUI does not restart it until `r` |
| n | New agent (repository, harness, account, model, prompt) |
| f | Filter: All → Active → Needs you |
| / | Search agents by title, repository, harness, model, account or prompt (Esc clears) |
| A | Accounts: sign-in status; `s` signs in (the provider's own login, in this terminal), `S` device code for ChatGPT |
| S | Audio Mode: on or off, track, system voice, a private Commander folder, preview (the daemon plays) |
| ? | Help |
| q | Quit (agents keep running) |

## Acceptance criteria

T-01 to T-13 were the first draft; T-14 onward extend it toward a full TUI. Verified criteria are checked here; the evidence for each is listed in
[evidence/tui](../verification/evidence/tui/README.md).

- [x] **T-01 — Same daemon, one source of truth.** `overseer-tui` connects to the same
  `overseerd` socket VS Code uses (starting the daemon when it is not running), identifies as
  client `tui`, and keeps no state of its own: agents, statuses and conversations come from
  `state` and the live event stream. Anything done in the TUI (a message, an answer, an
  interrupt, a new agent) appears in VS Code, and anything done in VS Code appears in the TUI.
  **Verify:** an integration test drives a real `overseerd` (isolated `OVERSEER_HOME`) from
  the TUI's app loop and checks the daemon's records; a second client subscribed to the same
  daemon (as VS Code is) sees the TUI's actions as events.
- [x] **T-02 — Pages of nine.** Agents appear newest first as a 3×3 grid; page n shows agents
  9(n−1)+1 to 9n; the header shows the page ("2/3") and counts (total, active, needs you);
  `]`/`[` and PgDn/PgUp change pages; the filter cycles All → Active → Needs you; a new agent
  appears on page 1 while the focused agent keeps focus. **Verify:** with 20 fixture agents,
  snapshots of pages 1–3, the page indicator, and focus kept on the same agent when a new one
  starts.
- [x] **T-03 — Live tiles.** Each tile shows its agent's status, title, harness, account and
  model, elapsed time and a live tail of its conversation, updated from the event stream as it
  happens; statuses and turns refresh like VS Code's (state reloaded on status and turn events).
  A dropped connection resumes from its cursor with no lost or doubled lines. **Verify:** a
  fixture agent's lines appear in its tile within 250 ms of the event; after the connection is
  cut and restored mid-stream, the tile holds every line exactly once.
- [x] **T-04 — Keyboard navigation and help.** Arrows/hjkl move focus spatially, 1–9 jump,
  Tab/Shift-Tab walk agents across pages, `?` lists every key, mouse clicks focus a tile, and
  the focused tile is unmistakable (accent border and title). **Verify:** key-driven tests for
  each binding, including wrapping at page edges; a help-overlay snapshot.
- [x] **T-05 — Talk to any agent.** `i`/Enter opens a composer on the focused agent; Enter
  sends a follow-up to that agent only; Esc closes it and keeps the draft for that agent;
  Alt-Enter adds a line. When the agent cannot take a message now (a turn is running, the
  harness has no follow-ups, it is a native child), the composer says why instead of failing.
  **Verify:** messages typed to two different agents reach only their own runs (daemon turn
  records); drafts survive switching tiles; the "busy" explanation shows for a running agent.
- [x] **T-06 — Answer and control.** A pending permission shows in its tile with a readable
  summary (tool and target, not raw JSON) and the keys to answer; `a`/`d` allow or deny it for
  that agent; `w` jumps to the next agent waiting for you; `x` interrupts after a y/n prompt.
  **Verify:** with the Claude fixture's permission mode, `a` on one agent and `d` on another
  produce the matching `permission_answered` events and outcomes; `x` interrupts only the
  focused agent.
- [x] **T-07 — Zoom with scrollback.** `z` shows the focused agent full screen with its whole
  conversation (history loaded from the daemon), follow-at-bottom while live, and scrolling
  with j/k, PgUp/PgDn, g/G; `z`/Esc returns to the grid on the same tile. **Verify:** zoom on
  a 2,000-event agent scrolls to the top and back; new events keep following at the bottom.
- [x] **T-08 — Start a new agent.** `n` opens a small form: repository (defaults to the current
  directory's Git root, else the last one used), harness (installed ones only), account
  (compatible ones, signed-in first), optional model, and the prompt; Enter launches it with
  `task.create`; it appears as tile 1 on page 1 and takes focus. **Verify:** a new fixture
  agent started from the form, with its task record matching the form.
- [x] **T-09 — Readable at a glance.** Status is a colored glyph (running, waiting for you,
  done, failed, interrupted), titles and paths are shortened with `…` (home as `~`), tool calls
  are one line, and the layout works in dark and light terminals (terminal default colors plus
  a purple accent), with 256-color and truecolor. Below 100×30 the grid becomes one focused
  tile plus a compact list, and resizing re-lays out live. **Verify:** snapshots at 200×60,
  120×40 and 80×24; no rendered line wider than the terminal.
- [x] **T-10 — Responsive with nine busy agents.** Nine concurrent chatty fixture agents on one
  page: key handling stays under 50 ms p95, each tile updates within 250 ms of its event, the
  TUI redraws only when something changed (idle CPU near zero), and memory stays bounded (a
  per-agent history cap). **Verify:** a timed test with nine streaming agents reporting input
  latency, update lag and idle redraw count.
- [x] **T-11 — Safe to quit, counted as a watcher.** `q` quits (asking first when a draft is
  unsent); agents and the daemon keep running; the terminal is restored even after a panic;
  while the TUI is attached, the daemon counts it as a watching UI, so closing the last VS Code
  window does not report the agents as running unseen. **Verify:** daemon test for the `tui`
  client count; quitting leaves the runs and the daemon untouched; a forced panic leaves the
  terminal usable.
- [x] **T-12 — Built, documented and tested with the workspace.** `cargo build` builds
  `overseer-tui` with the daemon; `overseer-tui --help` explains options and keys; the README
  documents it; `cargo test` covers the model, rendering, paging, key map and the integration
  tests. **Verify:** clean `cargo build`, `cargo test` and `cargo clippy` for the crate; help
  output and README section.
- [x] **T-13 — Live agents.** Tiny live runs with the real harnesses (Claude Code on its existing
  login with haiku, and Codex with gpt-5.6-luna) show up and stream in the TUI, and a follow-up
  typed in the TUI reaches the live Claude agent. **Verify:** snapshots from the live session
  and the daemon's records of the typed follow-up.
- [x] **T-14 — Changes view.** `v` shows what the focused agent changed, like the VS Code review:
  its changed files with status and line counts, the selected file's diff (added and removed lines
  colored), and the same comparisons as the review (latest run, earlier turns, task start, fork,
  target branch) cycled with `c`. Nothing is written: files and trees come from the daemon
  (`comparison.options`, `workspace.diff`) and the diff from read-only Git. **Verify:** an agent
  that edits one file and adds another shows both with `+/−` counts, each file's diff, and a
  second comparison.
- [x] **T-15 — Search.** `/` filters agents as you type by title, repository, harness, model,
  account, prompt or status; Enter keeps the search (shown in the header), Esc clears it, and
  focus lands on a match. **Verify:** four agents in two repositories narrowed by title and by
  repository name; kept after Enter; cleared by Esc.
- [x] **T-16 — Accounts and sign-in.** `A` lists accounts by provider with their kind (follows the
  desktop app, or fixed) and sign-in status (plan and fingerprint, never tokens); `s` signs the
  selected account in with its provider's own login, run in this terminal while the TUI is
  suspended, then resumes and refreshes; `S` uses ChatGPT's device code. Only that account's
  folder is touched; no API keys. **Verify:** the panel's statuses; the real binary in a
  terminal signs a fixed account in (fixture account CLI) and comes back, the daemon reports
  it signed in, and the desktop login is unchanged.
- [x] **T-17 — Tool details in zoom.** In zoom, `e` expands every tool call to show its input
  (`$ command`, the path with the replaced line, or the call's JSON) and the first lines of its
  result under the call; `e` folds them again. Paths are shortened as elsewhere. **Verify:**
  a Claude fixture Write call shows its path, size and result when expanded, and nothing extra
  when folded.
- [x] **T-18 — Merge back from the terminal.** `M` on a finished agent runs VS Code's merge back,
  never automatically: it explains when it is unavailable (still running, nothing to merge,
  blocked source checkout); step 1 (y/n) commits the worktree and merges the target into the
  agent's branch, with conflicts sent back to the agent; step 2 (y/n) says how many files land
  and merges into the target branch in the source checkout, keeping the worktree and branch.
  **Verify:** `M` on a running agent explains; on a finished one, nothing reaches the target
  before the second yes, and after it the change is on the target branch.
- [x] **T-19 — Attention from another window.** When an agent starts waiting for you, the TUI rings
  the terminal bell (unless `--no-bell`, or the daemon's Audio Mode plays the cue instead: T-24) and says who, with `w` to jump there; the terminal's window
  title always carries the counts ("Overseer · 1 needs you · 2 active"). **Verify:** a new waiting
  agent sets the bell, the notice and the title through the app loop; the real binary in a
  terminal writes the title escape and a bell.
- [x] **T-20 — Clean up a finished worktree.** `C` on a finished agent removes its worktree after
  y/n, keeps its branch, and names any uncommitted files that would be lost; running agents and
  current-checkout tasks are refused with the reason (the daemon's own safety checks). **Verify:**
  a finished agent with an untracked file: the prompt names it, `n` keeps the worktree, `y`
  removes it, and the branch remains.
- [x] **T-21 — Stop everything, start again.** `X` lists the running agents and, after y/n, stops
  them and the daemon (VS Code's *Stop Agents and Daemon*). The TUI then does not start the daemon
  again on its own (from this TUI or when another UI stopped it); it shows "stopped" until `r`,
  which starts the daemon in the same data directory. Worktrees and history are kept.
  **Verify:** two running agents: the prompt names them; after `y` the daemon exits and is not
  respawned; `r` brings it back and both agents show as interrupted.
- [x] **T-22 — Open a pull request.** `P` on a finished agent uses the daemon's Open PR plan
  (the same checks as VS Code: GitHub remote, not running, something to propose), says what will
  happen, and after y/n commits the worktree (daemon), pushes the branch with the user's own Git
  credentials and creates the pull request with the user's GitHub CLI (`gh`), with VS Code's
  generated description (run, task, commits, files, never merges). No token passes through
  Overseer; an existing pull request is reused; the URL and number are recorded on the run; the
  push and `gh` run off the event loop. **Verify:** against a local stand-in for github.com and a
  recording `gh`: the branch on the remote equals the worktree HEAD, `gh` got the repository,
  head, base, title and body, the run has a `pull_request` event, and the target is unchanged.
- [x] **T-23 — Audio Mode from the terminal.** `S` opens Audio Mode (Gate O in the main RFC). It
  shows what the daemon reports: on or off, whether playback is available, the track, the system
  voice and whether a private Commander folder is set. From there the user turns Audio Mode on or
  off, chooses Reactor, System voice or Commander, chooses an installed voice, enters a private
  Commander folder, and previews the three core cues. Every change goes through the daemon and
  is what VS Code then shows; a change made in VS Code shows in the terminal within 2 s. The TUI
  never plays a sound and keeps no audio setting of its own. The folder's files stay where they
  are; a folder that is not a Commander pack is refused with the daemon's reason. With a daemon
  that has no audio methods `S` says that Audio Mode is unavailable and changes nothing. `?` and
  `--help` list `S`. **Verify:** against a real daemon with its cue log: turning on from the TUI
  makes `audio.get` report on; a preview adds exactly that cue to the log; each track and the
  voice reach the daemon; a folder that is not a pack is refused with the reason on screen and a
  synthetic pack is accepted with no file copied under the daemon's folder; a setting changed
  through `ctl` shows in the open panel within 2 s; with a client that answers `audio.get` with an
  error the panel says unavailable; snapshots of the panel at 80×24 and 140×40, dark and light.
- [x] **T-24 — One signal when an agent needs you.** When a top-level agent starts waiting, one
  signal reaches the user: the daemon's cue, or else the terminal bell of T-19, never both and
  never neither. The bell is withheld only while the daemon's latest answer says that Audio Mode
  is on and playback is available. In every other case it rings in the same pass as the state:
  Audio Mode off, playback unavailable, no answer yet, an answer with an error, or a daemon
  without `audio.get`. The window title, the notice and Needs you change in that same pass and
  never wait for audio. A change made in another client counts for every need that arrives 2 s or
  more after it. `--no-bell` silences the bell in every case. **Verify:** T-19's test passes as it
  is on main. Against a real daemon with its cue log: off gives one bell and no cue; on gives one
  cue and no bell, with the title and the notice already changed; turned on through `ctl` while
  the TUI runs, a need 2 s later gives a cue and no bell; on with the Commander folder removed
  gives the bell. With a client that answers `audio.get` with an error, and with one that never
  answers, the bell rings in the same pass. The real binary in a terminal writes a bell with Audio
  Mode off and none beyond the title's terminators with it on. The daemon and TUI suites pass on
  main with pull requests #5 and #6 together.

T-23 and T-24 came with pull request #6 (code at 47312f3, merged into main as ea6a6c2 on 2026-09-27). Their evidence is in
[the evidence index](../verification/evidence/tui/README.md):
[both suites on main with pull requests #5 and #6](../verification/evidence/tui/t23-t24-suites.txt) and
[the run against a daemon without Audio Mode](../verification/evidence/tui/t24-old-daemon.txt).

## Parity with VS Code and the review in the terminal

Status: proposed on 2026-09-30 from the owner's request: "a tui that does pretty much everything
the same way" as Overseer in VS Code, "it's just convenient to use the terminal for everything
else." Criteria T-25 to T-41 below. The owner decided the shape the same day:

- **The TUI we have stays the base.** Nothing is rebuilt. The main screen is still the pages of
  nine, with an agent list added on the side. Picking an agent in the list shows its
  conversation.
- **Everything that makes sense in a terminal comes over.** Things that only exist because
  VS Code is a window manager (popping the review out, the Overseer window layout, dragging the
  grid, themes) stay in VS Code. The table below says which is which.
- **The review happens inside the TUI.** The owner first thought of a separate diff viewer and
  text editor, "maybe external like the system Overseer was built on" (that is Branch Diff, the
  owner's VS Code extension whose review Overseer vendored and grew into its own). Then decided:
  no separate viewer and no outside tool. The TUI's changes view (`v`) becomes Overseer's review:
  the same comparisons, the same file lists and the same Accept and Reject as VS Code, all
  through the daemon, so a change accepted in the terminal shows as accepted in VS Code and on
  the phone.
- **Editing files goes to the owner's own editor** (the owner, 2026-09-30: "E in native editor is
  probably good for now"). `e` opens the file at that change in `$EDITOR`; that editor shows no
  diffs and doesn't update live while the agent works, so the review stays the place for both and
  refreshes when the editor closes (T-39).
- **The owner's answers (2026-09-30):** way 2, the picked agent's conversation beside the grid; the
  grid sizes itself to fit how many agents there are, up to 16 on one screen, and pages from the
  17th (T-37); one key switches between the grid and a single agent's view (T-38); the grid shows
  top-level agents only for now, and whether to drill down into an agent's sub-agents is left for
  later.

The preview the owner picks from is
[docs/design/tui-parity/index.html](../design/tui-parity/index.html): the main screen with the
agent list, the review screen, and the two numbered ways of showing a picked agent.

### What VS Code does and what the terminal does today

"Has" means the TUI does it now; "partial" and "missing" name the gap and the criterion that closes it.

| In VS Code | In the TUI today | How the TUI does it |
| --- | --- | --- |
| Agents list in the side bar, grouped by repository, newest first, with status and account | Partial: a list exists only below 100×30, one page, not grouped | An agent list beside the grid at every size, grouped by repository (T-25) |
| Clicking an agent shows its conversation (its chat) | Partial: `z` zooms the focused tile full screen | Picking an agent in the list shows its conversation (T-25) |
| State rollup: working, needs you, to review, reviewed, failed | Partial: the header counts total, active and needs you | The same five counts, from the same numbers VS Code uses (T-26) |
| Unreviewed mark on finished agents, per repository and overall | Missing (VS Code keeps the marks to itself) | Marks kept by the daemon so every surface shows the same; shown in the list (T-26) |
| Account shown on every agent | Has: on each tile | Also on each list row (T-25) |
| Talk to Overseer (home), proposals answered yes or no, the "while you were away" summary | Has: `o` | Unchanged; the list and grid stay beside it |
| Voice Mode: state, heard words, mute, cancel, yes and no | Missing (Audio Mode cues only) | A voice line in Overseer's conversation and keys for each (T-35) |
| Needs you: count, list, jump to the next | Has: header count, `w`, the Needs you filter, the bell | Unchanged |
| Answer a permission: Allow once, Allow for this session, Deny with a note | Partial: `a` allow and `d` deny only | All three, with the note (T-31) |
| Follow-up to an agent | Has: `i` | Unchanged |
| Interrupt, stop everything | Has: `x`, `X` | Unchanged |
| New agent: repository, harness, account, model, effort, permission mode, prompt | Partial: no effort and no permission mode | The two missing fields (T-32) |
| Search agents | Has: `/` | Also filters the list (T-25) |
| Review opens on "Since task start"; "Latest run" and "Entire worktree" one click away; the header names what is shown | Partial: `c` cycles comparisons, but the first one opens, not the daemon's default | Opens on the daemon's default, each comparison one key away (T-27) |
| Changed or All files in the review's file list | Missing: changed files only | The same switch; All files lists the whole worktree (T-28) |
| Accept or Reject each change and each file | Missing: the diff is read-only | Accept and Reject per change and per file, through the daemon (T-29) |
| Follow: the review follows the file the agent is editing | Missing | The same Follow, on and off (T-30) |
| Type into the agent's files and "Save your edits" | Missing | Open question: the owner's own editor, or VS Code only |
| Merge back, "Merged into main (commit)", Clean up afterwards | Has: `M`, the merged line, `C` | Unchanged |
| Cancel a conflicted merge | Missing | Cancel from the same place (T-33) |
| Open a pull request | Has: `P` | Unchanged |
| Archive a finished agent, show and restore archived ones | Missing | Archive, show archived, restore (T-34) |
| Accounts and sign-in | Has: `A` | Unchanged |
| Phone access, pairing, devices | Has: `O`, `D` | Unchanged |
| Audio Mode | Has: `S` | Unchanged |
| Pop the review out to another screen, the Overseer window layout, dragging the grid, themes | Not in a terminal | Stay in VS Code: the terminal is already its own window, and colours come from the terminal |
| A command that VS Code gains later | — | The parity table fails its test until the command has a key or a reason (T-36) |

### Criteria

- [ ] **T-25 — An agent list beside the grid.** The main screen keeps the pages of nine and adds an
  agent list on the left: agents grouped by repository, the most recently active repository and
  agent first. Each row shows the status mark, the title (shortened with `…`), the account, and a
  mark for needs you, to review or merged. Each repository's heading shows its counts. `J`/`K` (or
  a click) picks the next or previous agent in the list, and the picked agent's conversation shows
  in a column beside the grid (way 2, the owner's pick), with the same scrollback and tool details as zoom. Esc returns to the grid on that agent.
  `L` hides or shows the list. Search and the filter narrow the list and the grid together. The
  list is shown from 100 columns; narrower terminals keep today's compact layout.
  **Verify:** with 20 fixture agents in 3 repositories, snapshots at 200×60, 140×40 and 100×30
  show the grouped list, each row's account and marks, and the repository counts. `J` three times
  shows the third agent's conversation and Esc returns to the grid with that agent focused. A
  search for one repository leaves only its group. No rendered line is wider than the terminal.
- [ ] **T-26 — The same counts and the same unreviewed marks as VS Code.** The header shows
  working, needs you, to review, reviewed and failed, the same five counts as VS Code's rollup,
  with zero counts left out. Whether a finished agent has been reviewed is kept by the daemon, not
  by one VS Code window, so opening an agent's review in the TUI clears its mark in VS Code and the
  other way round; merging clears it too. **Verify:** a fixture finishes 6 agents in 2
  repositories. The TUI header and VS Code's side bar give the same counts. Opening one agent's
  review in the TUI clears only that mark, in both. A mark cleared in VS Code clears in the TUI
  within 2 s.
- [ ] **T-27 — The review opens on "Since task start".** `v` opens the review on the comparison
  the daemon marks as the default: "Since task start". `1`, `2` and `3` switch to Since task start,
  Latest run and Entire worktree, and `c` still cycles through every comparison. The review's
  header always names what is shown and how many files and lines changed. A comparison that is not
  available says why instead of disappearing. **Verify:** a finished fixture agent in its own
  worktree and one in the current checkout both open on Since task start. Each key shows the right
  files and counts. The header names each one. Snapshots of the three.
- [ ] **T-28 — Changed or All files.** `t` switches the file list between Changed (what the
  comparison shows) and All files (the agent's whole worktree, from the daemon), as in VS Code.
  In All files, changed files carry their `+/−` counts, and picking an unchanged file shows its
  contents read-only. The switch says which list is shown. **Verify:** an agent that edited 2 of 5
  files lists 2 in Changed and 5 in All files; an unchanged file's contents show; the switch
  snapshot.
- [ ] **T-29 — Accept and Reject in the terminal.** In the review, `n`/`p` move between changes.
  `a` Accepts the change under the cursor and `A` the whole file; `r` Rejects the change and `R`
  the whole file, after a y/n that says how many lines go back to what was there before (reloading the review moves from `r` to Ctrl-R). Accept
  marks a change reviewed; Reject puts back the comparison's text in the agent's worktree. Both go
  through the daemon, with its conflict check: a change the agent altered meanwhile is refused
  with the reason. The words are Accept and Reject, never Keep, Undo or Save. What is accepted in
  the TUI shows as accepted in VS Code and on the phone, and the other way round. **Verify:**
  against a real daemon: a change accepted in the TUI is accepted in the daemon's marks and in
  VS Code's review; a change rejected in the TUI restores the base lines in the worktree; a whole
  file accepted and rejected; a change the fixture agent edits again before Reject is refused with
  the reason; a text audit finds no Keep, Undo or Save in the review.
- [ ] **T-30 — Follow in the review.** `F` turns Follow on: the review moves to the file the agent
  is editing and to the change being made, as edits arrive. Moving by hand pauses Follow and the
  header says "Paused"; `F` resumes it. **Verify:** a fixture agent editing three files in turn:
  the review moves to each within 250 ms of its edit; `j` pauses it; `F` resumes it.
- [ ] **T-31 — A waiting agent can always be answered from the terminal.** A pending permission
  offers Allow once (`a`), Allow for this session (`s`, the harness's own rule) and Deny with a
  note (`d` opens a one-line note; Enter sends, empty is fine). The note reaches the agent as the
  reason. This is the terminal side of the main RFC's "a waiting agent can always be answered".
  **Verify:** on the Claude fixture: Allow for this session is not asked again for the same tool;
  a denial's note is in the fixture's input log; the prompt's snapshot.
- [ ] **T-32 — New agent with every choice VS Code has.** The `n` form adds effort and permission
  mode (Ask first, Accept edits, Auto) to repository, harness, account, model and prompt, offering
  only what the chosen harness and model support, remembered like VS Code's composer. **Verify:** a
  fixture agent started with a chosen effort and mode has both in its task record; a harness
  without efforts hides the field.
- [ ] **T-33 — Cancel a conflicted merge.** When merge back stops on conflicts, `M` on that agent
  offers to cancel the merge (y/n), which puts the worktree back as it was before the merge, as
  VS Code's Cancel merge does. The merge confirmation lists every file that will be committed,
  untracked ones included. **Verify:** a fixture merge that conflicts; cancelling restores the
  worktree's pre-merge HEAD and files; the confirmation of a worktree with an untracked `.env`
  names it.
- [ ] **T-34 — Archive and restore.** `E` archives a finished agent (y/n): it leaves the list and
  the grid, as in VS Code. The filter gains Archived, where `E` restores one. **Verify:** archiving
  a fixture agent in the TUI hides it in both the TUI and VS Code; restoring brings it back in both.
- [ ] **T-35 — Voice Mode in the terminal.** Overseer's conversation (`o`) shows a Voice Mode line:
  off, listening, hearing you, thinking, speaking, muted or paused for a call, and the words as they
  are heard. Spoken requests are the same cards as typed ones. Keys turn Voice Mode on and off,
  mute, cancel the open request, and answer a read-back yes or no. The daemon listens and speaks;
  the TUI only shows and sends. **Verify:** with a fixture voice session each state shows in turn,
  a card fills in as it advances, and mute, cancel and yes work by key; the same state shows in
  VS Code at the same time.
- [ ] **T-36 — Nothing left out without a reason.** This RFC keeps a table of every command VS Code
  Overseer offers, each with its TUI key or the reason it stays in VS Code. **Verify:** a unit test
  reads the commands in `extension/package.json` and fails when one has no row; `?` lists every key
  the table names.

- [ ] **T-37 — The grid fits the count.** The grid shows top-level agents only (runs without a
  parent) and sizes itself to how many there are, up to 16 on one screen (1, 1×2, 2×2, 2×3, 3×3,
  3×4, 4×4 and the shapes between), narrowing to fit beside the list and a picked agent's
  conversation; from the 17th agent it pages, 16 per page. Arrow keys follow the shape.
  **Verify:** snapshots at 200×60 with 1, 4, 7, 12, 16 and 17 fixture agents show the expected
  shape (the 17th on page 2), with and without the conversation column; sub-agents never get a
  tile.
- [ ] **T-38 — One key between the grid and one agent.** A key (`g`, shown in `?`) switches between
  the grid and the focused agent's full view (its conversation with scrollback and tool details),
  and back to the grid on the same agent. **Verify:** with 9 fixture agents, focusing the fourth
  and pressing the key shows its full conversation; pressing it again shows the grid with the
  fourth focused; the key works from the list and from a tile.
- [ ] **T-39 — Edit in your own editor.** In the review, `e` suspends the TUI and opens the file at
  the current change in `$EDITOR` (falling back to `vi`); when the editor exits, the TUI comes back
  and the review refreshes from the daemon, showing the owner's edits as theirs (not the agent's).
  **Verify:** with `EDITOR` set to a script that appends a line and exits, `e` on a change returns
  to the review with that line shown as the owner's edit and the terminal restored (no leftover
  raw mode or alternate screen).

- [ ] **T-40 — Dashboard mode for big screens.** An option beside the grid (the owner, 2026-09-30:
  "screens are pretty big now … agents on the left for now are good, and then review in the middle,
  and then the chat on the right as a dashboard mode, as just an option"): the agent list on the
  left, the picked agent's review in the middle (the T-27 to T-29 review) and its conversation on
  the right, like the Overseer layout in VS Code (AC-264). A key (`D`, shown in `?`) switches between
  the grid and dashboard mode and keeps the picked agent; `--dashboard` starts in it. Picking another
  agent in the list changes the review and the conversation together; Tab moves focus between the
  three columns. **Verify:** snapshots at 240×70 and 200×60 with 9 fixture agents show the three
  columns; `J` changes both the review and the conversation to the next agent; `D` returns to the
  grid on that agent; below 160 columns dashboard mode says it needs a wider terminal and stays on
  the grid.
- [ ] **T-41 — A grid-only terminal beside it.** `overseer-tui --grid` shows only the grid of agents
  (no list, no conversation column), so the owner can keep the grid in a second terminal and
  dashboard mode in the first (the owner: "maybe you can open a second TUI, and then I can view the
  agent grid only on the second TUI, like two terminals. Maybe that's the best use case"). Both
  terminals are clients of the same daemon and stay live; picking an agent in the grid-only
  terminal does not move the other. **Verify:** two TUIs against one fixture daemon, one with
  `--grid` and one with `--dashboard`; a new fixture agent appears in both; answering a permission
  in one clears it in the other; snapshots of both.

### Open for later

- Swapping one of dashboard mode's columns for the grid of agents (the owner, 2026-09-30, "just
  theorizing"): with the grid on screen the agent list isn't needed, so the grid could take the
  left or the middle. T-41's second terminal covers the need for now.

- How sub-agents show: drilled into from their parent, or seen as tiles next to the others (the
  owner, 2026-09-30: "maybe assume all top level for now"; "not a priority, needs more thought").
