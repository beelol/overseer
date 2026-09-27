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
| i, Enter | Compose a message to the focused agent (Enter sends, Esc closes, Alt-Enter new line) |
| z | Zoom: focused agent full screen with scrollback (j/k, PgUp/PgDn, g/G; z or Esc returns) |
| v | Changes: the focused agent's changed files and their diffs (j/k file, J/K scroll, c comparison) |
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
  the terminal bell (unless `--no-bell`) and says who, with `w` to jump there; the terminal's window
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
