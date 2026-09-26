# overseer-tui evidence (T-01 to T-13)

Environment: macOS 26.6.2 arm64, Rust 1.89.0, ratatui 0.30.2, crossterm 0.29; a real `overseerd`
from this workspace in an isolated `OVERSEER_HOME` per test. Agents are generic fixture programs
and the SYNTHETIC Claude fixture (`fixtures/fake-harness/claude-fixture.js`), except T-13
(live Claude Code and Codex). Screens are the TUI's own `ui::draw` rendered off-screen
(`.txt`, plus `.svg` for a dark and `-light.svg` for a light terminal), except T-11's pty runs
of the real binary.

Run everything with `cargo test -p overseer-tui` (T-13 needs `OVERSEER_TUI_LIVE=1`).

| AC | Test | Evidence | Result |
| --- | --- | --- | --- |
| T-01 Same daemon | `tests/live.rs` `t01_…` | [t01-same-daemon](t01-same-daemon.txt) | A message typed in the TUI is a turn in the daemon and an event seen by a second subscribed client; an agent and a follow-up made through `ctl` appear in the TUI. |
| T-02 Pages of nine | `tests/interact.rs` `t02_…` | [page 1](t02-page-1.txt), [2](t02-page-2.txt), [3](t02-page-3.txt) | 20 agents: 12–20 / 3–11 / 1–2, "page n/3"; focus stays on agent 15 when agent 21 starts (it becomes tile 1); filter all → active → needs you. |
| T-03 Live tiles | `tests/live.rs` `t03_…` | [t03-live-tiles](t03-live-tiles.txt) | Event → tile lag p95 2 ms; connection dropped mid-stream: all 60 lines present exactly once. |
| T-04 Keys and help | `tests/interact.rs` `t04_…` | [t04-help](t04-help.txt) | Arrows/hjkl, page-edge wrapping, 1–9, Tab/Shift-Tab across pages, help, mouse click. |
| T-05 Talk to any agent | `tests/interact.rs` `t05_…` | [t05-composer](t05-composer.txt) | A's message reaches only A; B's two-line draft survives switching tiles and is delivered; a running Claude turn shows "can't send now: a turn is running" and keeps the draft. |
| T-06 Answer and control | `tests/control.rs` `t06_…` | [t06-waiting](t06-waiting.txt) | `w` finds waiting agents; `a` allowed (file written), `d` denied (nothing written), both as `permission_answered`; `x`/`n` cancels, `x`/`y` interrupts only the focused agent. |
| T-07 Zoom | `tests/control.rs` `t07_…` | [bottom](t07-zoom-bottom.txt), [top](t07-zoom-top.txt) | 2,000-line agent: bottom, `g` top, `G` back; live agent keeps following; messages from zoom. |
| T-08 New agent | `tests/control.rs` `t08_…` | [t08-form](t08-form.txt) | Generic and Claude agents from the form; task record matches (repository, prompt, account `system-claude`); tile 1, focused; prompt required for Claude. |
| T-09 Readable | `tests/look.rs` `t09_…`, `ui::tests` | [200×60](t09-200x60.txt), [120×40](t09-120x40.txt), [80×24](t09-80x24.txt); light: [svg](t09-120x40-light.svg) | Status glyphs, shortened titles and paths, compact list + focused agent below 100×30, live re-layout; 256-color and truecolor accents. |
| T-10 Nine busy agents | `tests/look.rs` `t10_…` | [t10-timings](t10-timings.txt), [t10-nine-busy](t10-nine-busy.txt) | Key + full redraw p95 ≈ 8 ms; event lag p95 ≈ 6 ms; no redraw while idle; history capped at 4,000 items per agent. |
| T-11 Safe to quit | `tests/look.rs` `t11_…` ×2, `daemon/tests/protocol.rs` `t11_…` | pty output checked in the test | Quit asks about drafts; agents keep running; the daemon counts the TUI as a watching UI (no background notice while it is attached, one after it quits); a panic still leaves the alternate screen. |
| T-12 Built and documented | `tests/look.rs` `t12_…` | [t12-help](t12-help.txt), README "In a terminal" | Workspace build and tests pass; `cargo clippy -p overseer-tui --all-targets` clean. |
| T-13 Live agents | `tests/live_harness.rs` (opt-in) | [first turns](t13-live-first-turns.txt), [follow-up](t13-live-follow-up.txt), [records](t13-live-records.json) | Claude Code (haiku) and Codex (gpt-5.6-luna) streamed in their tiles; a follow-up typed in the TUI became Claude's turn 2. |
| T-14 Changes view | `tests/control.rs` `t14_…` | [t14-changes](t14-changes.txt) | An agent's edited and added files with `+/−` counts, each file's diff, and "Since task start" as a second comparison. |
| T-15 Search | `tests/interact.rs` `t15_…` | [t15-search](t15-search.txt) | Narrowed by title ("ref") and by repository ("payments"), kept after Enter, cleared by Esc. |
| T-16 Accounts and sign-in | `tests/look.rs` `t16_…` | [t16-accounts](t16-accounts.txt) | Panel shows providers, kinds and statuses; the real binary in a pty suspends, runs the account's own login (only its `CODEX_HOME`), resumes; the daemon then reports it signed in (team) and the desktop login still Pro. |
| T-17 Tool details | `tests/control.rs` `t17_…` | [t17-expanded-tools](t17-expanded-tools.txt) | The Write call expands to `perm.txt`, `1 line` and `File created successfully at: ./perm.txt`; `e` folds it. |
| T-18 Merge back | `tests/control.rs` `t18_…` | [step 1](t18-merge-step-1.txt), [step 2](t18-merge-step-2.txt) | Refused while running; "commit 1 worktree file and merge main into …"; "1 file lands"; `main` unchanged until the second yes, then it has the change. |
