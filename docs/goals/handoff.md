# Handoff: where the everything goal stands

Updated 2026-09-29, late evening, by the coordinator session. Read this first when you pick up the everything goal ([everything.md](everything.md)); keep it current as work lands.

## How to pick up

1. Set the goal: paste the block in [everything.md](everything.md) into `/goal`.
2. Read [AGENTS.md](../../AGENTS.md), then this file. The tracker is [tracker.md](../verification/tracker.md); the merge log is [merges.md](../verification/evidence/ac-146/merges.md).
3. Check the machine before anything heavy:
   - `uptime`: wait while the load is high; the owner plays games on this Mac.
   - `cat $TMPDIR/overseer-test-jobs-max`: recreate it with `echo 1 > $TMPDIR/overseer-test-jobs-max` after a reboot.
   - `ls $TMPDIR/overseer-test-all.lock`: a full run is going if it's there.
4. Refresh the open pull requests: `gh pr list --repo beelol/overseer`. For each branch, `git merge-tree --write-tree gh/main gh/claude/<branch>` tells you whether it still merges cleanly.
5. Work down the queue below: **one builder and one full test run at a time**.

Nothing needed from the old session survives in /private/tmp: the builders' worktrees and scratch notes were wiped by the reboot. Every builder had pushed, so each branch on GitHub is the whole state. Recreate a worktree from the branch (`git worktree add <dir> gh/claude/<branch>`).

## The pace rule (the owner, after the Mac crashed on 2026-09-29)

Five builders plus merge checks at once pushed the load past 100 and crashed the Mac. Now:
- **Builders:** one to start. Add a second or third only while the load stays low, and always leave room for the owner's game and a couple of other agent tasks.
- **Full runs:** one at a time, run as `CARGO_BUILD_JOBS=4 RUST_TEST_THREADS=4 nice -n 20 scripts/test-all --jobs=1`. The script itself runs at the lowest priority and honours the jobs cap file.
- **Outside a full run:** at most one UI scenario at a time, and none while a full run holds the lock.
- **Hidden load:** every UI test copies a fresh daemon binary, and Bitdefender, XProtect and Gatekeeper scan each one at normal priority.

## Queue (in order)

| PR | Branch (head) | What | State and next step |
|---|---|---|---|
| #32 | `claude/overseer-brain` (`2d5e67ba`) | Overseer's brain: AC-237, 238, 248, 253 verified; AC-239 partial | **Ready.** Main is merged into the branch (the `views.js` conflict resolved: main's tooltip with landing and account, plus `run.plain_reason \|\|` before the exit reason). Its full run was going at hand-off (`scratchpad/pm-wt`, log `scratchpad/pr32-run2.log`). If that's gone, rerun it on the branch head. Earlier misses (TUI t03, UI center, continuity, history) passed alone. **own-layout failed twice** (Focus Mode's own-window settings check) and must pass before merging, or be shown load-only. Then `gh pr merge 32 --squash`, record it, and deploy. |
| #40 | `claude/one-layout` | AC-264: one Overseer layout | Way **B** chosen by the owner. The window reopens on an Overseer workspace file: the picture-3 look, the review in the middle, one right panel (home, voice or the picked agent's chat, with back and ⌥⌘U), a first-launch offer, Focus Mode retired. Also the owner's latest: remove #34's separate Worktree view (the review's file list switches "Changed \| All files" instead); remove the side bar's Search section (search moves into the Agents view's title bar). Its builder was mid-phase-2, the only builder running. |
| #42 | `claude/menu-bar` (`b1dc6da8`) | AC-262: the Mac menu-bar item (Swift, a login item, talks to the daemon's socket) | Built; Rust and unit tests pass; the ledger says partial. Needs **one more 3-minute hands-off window** from the owner for `nice -n 20 node test/ui/scenario-menubar.js`, run directly (not through test-all), to prove three fixes: replies while a menu is open, VS Code's URI prompt, and the 30-agents submenu walk. Ask the owner first; pause any full run during it. |
| #39 | `claude/menu-bar-mockup` | The mockup and the owner's answers | Merge with #42 or close as superseded. |
| #38 | `claude/review-default` | AC-263: the review opens on Since task start everywhere, with Latest run and Entire worktree one click away | Also add the owner's wording: **Accept / Reject** for the agent's changes (per change and per file), never Keep, Undo or Save; "Save your edits" only after the owner types. |
| #41 | `claude/steady-tests` | Timing tests wait for events, not fixed sleeps (AC-149) | ac185 and ac189, protocol's Auto/OpenCode tests, and the UI center, continuity, review, sidebar, audit, main, modes and keyboard scenarios. **No artificial load.** |
| #37 | `claude/phone-parity` | 12 `phone/model` vitest failures (Rollup, changesOnly, copied words); adds the phone tests to test-all | Mid-way. |

Merged on 2026-09-29: #31, #33, #34, #35, #36. Deployed to the owner: b3b7133d (#31, #33, #34). #35, #36 and later are **not deployed**; deploy after the next merges (`scripts/deploy --yes --no-fetch --ref <main>`; it waits for quiet).

## Owner decisions on record

- Menu bar (AC-262):
  - dot violet;
  - Needs you capped at 4, then "N more waiting";
  - "Allow once ▾" holds Always allow, and Deny is on the right;
  - repositories most recent first, and the summary skips zero counts;
  - starts at login, with Quit;
  - a dev daemon gets its own icon with a "!", its name on hover and at the top of its menu.
- Layout (AC-264): way B, two wide areas, Focus Mode retired, the side bar is the hierarchy only.
- The review (AC-263): Since task start everywhere; Accept and Reject.
- The account text on side bar rows may be cut off by long titles (accepted).
- Paid turns inside Overseer: gpt-5.6-luna at low effort only.

Still waiting on the owner:
- the CI timing-test choice (t10; #41 may make it moot);
- the 16.5 GB `overmind-nightly_overmind-data` Docker volume;
- the AC-61 Needs-you meaning change (merged with #31 as "waiting for your answer" plus a separate "to review" mark);
- the second menu-bar window.

## Pitfalls learned (so they don't repeat)

- **`CARGO_TARGET_DIR` must be absolute.** A relative one breaks the TUI tests, which run from another folder.
- **Never `git add -A` in a throwaway merge.** Stray `node_modules` symlinks get committed. `.gitignore` now says `node_modules`, without the slash, to catch links too.
- **Two runs must never share a log file.** A second run truncates the first run's log.
- **In zsh, `kill $pids` doesn't split the list.** Use `xargs`.
- **A test-all blocked in a child ignores SIGTERM until that child ends.** Stop its children first.
- **Merge markers can name a branch, not a SHA** (`>>>>>>> gh/main`). Match `>>>>>>> [^\n]+`.
- **Never renice by a broad pattern.** It caught the owner's installed daemon once. Exclude `~/.vscode/extensions/`.
- **Public status pages are off in tests** (`OVERSEER_PUBLIC_STATUS=off`). A real Claude outage once failed three Auto tests on main.
- **`/private/tmp` is wiped on reboot.** Push work, and keep notes on main (this file) or in memory, not only in the scratchpad.
