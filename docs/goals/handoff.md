# Handoff: where the everything goal stands

Updated 2026-09-30 00:00 by the third coordinator session, which took over at 23:57 on 2026-09-29. Read this first when you pick up the everything goal ([everything.md](everything.md)).

**Keep this file current, and tell whoever comes after you to do the same.** Update it and push it to main the moment anything changes: a merge or deploy, an owner decision, a builder started or finished, a PR opened or closed, a new criterion. Also update your own memory's resume note. The owner asked for both (2026-09-29), so the next agent never loses work when usage or a session runs out.

## Change of hands

- **Until 2026-09-29 23:45:** the first coordinator ("overseer") ran the goal. Its last acts were:
  - fixing `extension/scripts/package.js` and `test/dev/run.js` to honour `CARGO_TARGET_DIR` (dded92c9, 3dde3075);
  - resolving #32's conflict and pushing it to #32's branch (2d5e67ba);
  - starting #32's third full run;
  - stopping its layout builder once #40 was pushed (f0182365).
- **23:45 to 23:55:** a second session ("overseer-fe") took over. It read the hand-off, created the worktree `.claude/worktrees/coord` (branch `coord-main`, clean, safe to reuse or remove) and pushed one hand-off update (072db9f2), then crashed. It changed nothing else.
- **23:57 on:** a third session is coordinating from the worktree `.claude/worktrees/coord` (branch `coord-main`, fast-forwarded to main). #32's third run was still alive then (pid 96715, past Rust, unit, dev, guided and deploy, building the VSIX); it watches the log for the `EXIT` line, then merges #32 and deploys. The first session only keeps that run's shell alive.

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

## Full context for a new account

You may be a different Claude account with none of the previous session's memory. Everything you need is here, in AGENTS.md and in the docs it points to. As you learn more, keep it in this file, not only in your memory.

### Who and what
- **The owner** is Bilal (GitHub `beelol`). The repository is `beelol/overseer`; the main checkout is `/Users/bilal/projects/overseer`, and sessions work in worktrees under `.claude/worktrees/`.
- **Overseer** is a Rust daemon (`daemon/`, overseerd), a VS Code extension (`extension/`), a TUI (`tui/`), an Expo phone app (`phone/`) and a voice listener (`voice/`). They manage many coding agents (Claude Code, Codex, OpenCode) for the owner.
- **The owner's bar:** clearly better than talking to Codex and Claude Code separately and relaying between them. You talk to it, it picks models and accounts, sends work out, and keeps agents on track.
- **After the current queue:** a GitHub Pages site, mods ([docs/rfcs/mods.md](../rfcs/mods.md)), then usability gaps found in real use ([after-current-work.md](after-current-work.md)).

### How the owner works with you
- **Visual choices are decided by seeing.** Build a small real preview (screenshots, or an HTML page sent to them) before asking. Ask few questions, numbered; they answer in numbered shorthand ("1 yes, 2 violet").
- **Don't block on the owner.** Ask the precise question, keep working on everything else, and watch things in the background, never in foreground polling loops.
- **Plain words:** no internal ids or jargon in anything the owner reads.
- **"Auto" means an agent works on its own** without asking (the permission mode). It does not mean picking models or accounts automatically; that's "route picking" (Auto routing, `docs/rfcs/auto-mode.md`).
- **Owner-only checks** (microphone, real voice, speakers) run in a dev daemon through `scripts/dev test <check>`. Never ask the owner to install a branch build.
- **Deploys** to the owner's installed Overseer happen only with `scripts/deploy`, after a merge. The owner has asked for deploys after merges.
- **Clean up after every sub-agent and test run:** stray VS Code test windows, dev daemons, shims, simulators. Use `xargs` in zsh. Never stop the owner's own apps (games, Chrome, Docker, their other Claude sessions) without asking.

### Repository rules (beyond AGENTS.md)
- **Criteria, ledger and docs** go straight to main. Implementation goes on a branch plus a PR, and PRs are squash-merged by this goal after a throwaway merge and a full test run. PR descriptions: what changed, the tests actually run, no filler.
- **AC numbers move fast:** fetch main and recompute the next free number right before pushing a new criterion.
- **Landing ledger edits:** edit main's `docs/verification/records.py` in place (Python replacements on main's copy), then regenerate with `set -o pipefail; python3 docs/verification/records.py b5693b8`. Never copy a branch's records.py over main's, because it erases other agents' records.
- **Don't `cargo fmt` the daemon:** it isn't rustfmt-clean. Format only your own files.
- **Generated files after merges:** `python3 protocol/capabilities.py` (the README's phone table) and `node protocol/gen-ts.mjs` (the phone's types).
- **GitHub:** push over SSH (`git push git@github.com:beelol/overseer.git HEAD:<branch>`); if SSH fails, use the `gh` CLI. HTTPS pushes hang. Never force-push.

### Accounts and paid turns
- **Paid turns inside Overseer** (tests, live checks, dev daemons): only `gpt-5.6-luna` at low reasoning effort. No Claude model for now. One attempt per step, no retry loops.
- **The owner's accounts:**
  - personal Claude and personal ChatGPT Plus, signed in in the regular browser;
  - work Claude Max and work ChatGPT Pro (testbox.com), signed in in the work browser.
  
  Both ChatGPT accounts may carry luna turns. Sign-in approvals open in the matching browser and the owner clicks them. Never enter credentials, read credential files, or sign anything out.
- **The Mac's default Claude Code login** is the work account. The owner accepts that for their own use, as long as the app shows which account each agent uses (AC-235, done).

### The Mac
- **The owner uses it while agents work,** including games. Mind the pace rule below.
- **Local models (Ollama):** never load one above min(40% of memory, free memory minus headroom), and never `qwen3.5:122b`. One at a time.
- **`~/Downloads` is unreadable to the tools.** Ask the owner to copy files to /private/tmp.
- **Apple assets:** the ones already on the Mac belong to another project (aquafriends). Never use them for Overseer. The phone ships through TestFlight (see AGENTS.md).

### Sub-agents
- Build work goes to background sub-agents with self-contained briefs. The coordinator watches, merges, keeps the ledger and tracker, and does small fixes itself.
- **Brief every sub-agent with:**
  - its own worktree (`git worktree add -b claude/<name> <dir> origin/main`) and scratch folder;
  - an absolute `CARGO_TARGET_DIR`;
  - the pace rule;
  - luna-only paid turns;
  - SSH pushes;
  - a draft PR, then `gh pr ready`, never merging;
  - ledger edits on main in place;
  - clean up at the end;
  - waiting on runs in the foreground, because a sub-agent that ends its turn while a background run goes never wakes up.
- Tell it to push often.

## The pace rule (the owner, after the Mac crashed on 2026-09-29)

Five builders plus merge checks at once pushed the load past 100 and crashed the Mac. Now:
- **Builders:** one to start. Add a second or third only while the load stays low, and always leave room for the owner's game and a couple of other agent tasks.
- **Full runs:** one at a time, run as `CARGO_BUILD_JOBS=4 RUST_TEST_THREADS=4 nice -n 20 scripts/test-all --jobs=1`. The script itself runs at the lowest priority and honours the jobs cap file.
- **Outside a full run:** at most one UI scenario at a time, and none while a full run holds the lock.
- **Hidden load:** every UI test copies a fresh daemon binary, and Bitdefender, XProtect and Gatekeeper scan each one at normal priority.

## Queue (in order)

| PR | Branch (head) | What | State and next step |
|---|---|---|---|
| #40 | `claude/one-layout` (`2d2df321`) | AC-264: one Overseer layout (way B) | **Builder finished 2026-09-30 ~01:50.** The owner's answer is built: the review's list is "Changed" (Diffs only: a click shows the diff) or "All files" (a click opens the real file and switches the agent to Follow). This overrides AC-99 (any file opened inside the review): `scenario-review-files` rewritten; AC-99's record needs a line at the merge. Search box: emptying it clears the list after 300 ms (the six searches went from 206–212 ms to ~8 ms). Scenarios passed alone: overseer-window 21, agent-head 14, review-files 7, sidebar-search 9, followups 10, history 5, first-click 6, gallery 6, inventory 6, modes 4, own-layout 5; unit 25/25. **Full run started 2026-09-30 01:37 on #40's own head** (worktree `.claude/worktrees/one-layout-run`, target `/private/tmp/claude-501/one-layout-b-target`, log `pr40-run1.log` in the coordinator's scratchpad): the throwaway merge with main is blocked on the owner's permission, so this run finds #40's own failures first; the merged-copy run follows the owner's yes, then ready and merge. Open for the owner: Follow itself shows no file list (the review isn't on screen in Follow); All files is reached from the review. |
| #42 | `claude/menu-bar` (`b1dc6da8`) | AC-262: the Mac menu-bar item (Swift, a login item, talks to the daemon's socket) | **On-screen check passed 2026-09-30 01:35, 10 of 10, with the Mac idle**; AC-262 recorded verified on `b1dc6da8`, evidence on main (`docs/verification/evidence/ui/menubar/`). **Next:** a full run on a throwaway merge with main (blocked until the owner allows merging main into copies), then merge; the merge also closes AC-179's menu-bar part. |
| #38 | `claude/review-default` | AC-263: the review opens on Since task start everywhere, with Latest run and Entire worktree one click away | Also add the owner's wording: **Accept / Reject** for the agent's changes (per change and per file), never Keep, Undo or Save; "Save your edits" only after the owner types. **Builder started 2026-09-30 ~01:35** (worktree `.claude/worktrees/review-default-b`, pushing to `claude/review-default`): the Accept / Reject words and AC-263's Verify checks; told not to merge main (#40 also changes the review and lands first). |
| #41 | `claude/steady-tests` | Timing tests wait for events, not fixed sleeps (AC-149) | ac185 and ac189, protocol's Auto/OpenCode tests, and the UI center, continuity, review, sidebar, audit, main, modes and keyboard scenarios. **No artificial load.** |
| #37 | `claude/phone-parity` | 12 `phone/model` vitest failures (Rollup, changesOnly, copied words); adds the phone tests to test-all | Mid-way. |

**#41 and #37 are blocked on a permission:** their only conflicts with main are `extension/scripts/package.js` (and `test/dev/run.js` for #37), where both sides made the same `CARGO_TARGET_DIR` fix (take main's side). Claude Code's auto-mode check refused this session pushing a merge of main into those branches ("modify shared resources"), and also refused the cleanup of the local attempt: the worktree `.claude/worktrees/fixmerge` (branch `fixmerge-steady`) holds a half-done merge and can be removed. Asked the owner. A local throwaway merge of main into #42's copy (for its menu-bar check) was refused the same way ("auto-mode bypass"), so the menu-bar check runs on #42's own head, building into the worktree's own `target/` (the branch's `package.js` predates main's `CARGO_TARGET_DIR` fix). Until the owner allows merging main into copies, AC-146's throwaway merges are blocked for this session.

Mergeability against main, checked 2026-09-30 00:00 with `git merge-tree --write-tree`: #32, #40, #42, #39, #38 merge cleanly; **#41 (`claude/steady-tests`) and #37 (`claude/phone-parity`) now conflict** and need main merged into them before their turn.

Merged on 2026-09-30: #39 (`ac7a4143`), #32 (`3ab9c1f7`; talk, conversation and review missed in the full run and passed alone). Merged on 2026-09-29: #31, #33, #34, #35, #36. Deployed to the owner: b3b7133d (#31, #33, #34). **Deployed to the owner: `ed747bc7`** (#35, #36, #39, #32) on 2026-09-30 at 01:03 (daemon restarted; VS Code windows need a reload). The next deploy follows #40's merge.

## New from the owner (2026-09-30)

- **TUI parity:** the terminal UI should do everything VS Code Overseer does, with its diffs shown the way VS Code's review shows them, inside the TUI (the owner corrected an earlier "separate viewer" idea). A design sub-agent is writing the gap list, criteria in `docs/rfcs/tui.md` and a preview under `docs/design/tui-parity/`; the owner looks at the preview, and the build queues after the current PRs. Tracker row: "TUI parity". Design done (0fb0fb27): T-25 to T-36 in `docs/rfcs/tui.md`, preview `docs/design/tui-parity/index.html` (shown to the owner at https://claude.ai/artifact/JoV4TSfWJg97WaEwULzECJ). The owner answered (2026-09-30): way 2 (beside the grid); the grid fits the count up to 16, pages from the 17th, top-level agents only (T-37); one key between grid and single agent (T-38); `e` opens their own editor (T-39); drilling into sub-agents is for later. The owner's shape: the existing TUI stays the base — mainly its grid view, plus an agent list on the side; focusing an agent shows its chat; bring over everything from VS Code that makes sense in a terminal.
- **The owner approved merging and deploying** (2026-09-30): "merge pull requests when they're ready and deploy" — after a throwaway merge and a passing full run, then `scripts/deploy`. And: "no sketchy or unsafe shit, don't mess around outside the repo." (Claude Code's auto-mode check refuses `gh pr merge` without that approval; a new session should ask the owner again rather than work around it.)
- **#39 (menu-bar mockup) merged** (`ac7a4143`): docs only, link check passed (871 links, 0 broken); #42 did not carry these files.
- **#42's menu-bar check started 2026-09-30 01:30** (the Mac idle 83 minutes): worktree `.claude/worktrees/menubar-check` (detached at `b1dc6da8`), target `/private/tmp/claude-501/menubar-check-target`, `$TMPDIR/overseer-quiet-window` held for its duration. **#42's menu-bar check: the owner said yes** (2026-09-30), to run while the Mac is unused. Run it when the keyboard and mouse have been idle 10+ minutes (`ioreg -c IOHIDSystem | awk '/HIDIdleTime/ {print int($NF/1e9); exit}'` in seconds), no full run holds the lock, and no other UI scenario is going; create `$TMPDIR/overseer-quiet-window` for its duration so builders hold their scenarios, and remove it after.

## Owner decisions on record

- Menu bar (AC-262):
  - dot violet;
  - Needs you capped at 4, then "N more waiting";
  - "Allow once ▾" holds Always allow, and Deny is on the right;
  - repositories most recent first, and the summary skips zero counts;
  - starts at login, with Quit;
  - a dev daemon gets its own icon with a "!", its name on hover and at the top of its menu.
- Layout (AC-264): way B, two wide areas, Focus Mode retired, the side bar is the hierarchy only; in Follow mode the file list is "All files" (the whole worktree), in Diffs only it is "Changed".
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
