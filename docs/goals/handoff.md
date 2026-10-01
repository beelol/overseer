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

**Cleaned up 2026-09-30 09:30 (owner: "clean anything not in use"):** the worktrees `fixmerge`, `menubar-check`, `one-layout-run` and the first coordinator's `ol-main`, `ol-wt`, `pm-wt` (with their build folders, about 15 GB); no dev daemons, simulators or test windows were left. In use: `coord` (this coordinator) and `tui-parity-build` (the TUI builder). Other sessions' worktrees (Codex, Kilo, older Claude sessions) are theirs and were left alone.

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
| #38 | `claude/review-default` (`79b3be10`) | AC-263: the review opens on Since task start everywhere, with Latest run and Entire worktree one click away; Accept / Reject | **Builder finished 2026-09-30 ~03:45** (draft, a comment says done): Accept / Reject per change and per file (Accept file, Reject file), "Save your edits" only after typing; narrow cards show the file buttons as icons (tooltip and label keep the words); review-compare covers the rest of AC-263's Verify. Alone on the final build: review-compare, review, review-merge, hunks, review-files, review-marks, review-width, scopes passed; cargo review_merge 193, protocol 10; unit 25/25. **Next:** after #40 merges, merge main in (expect conflicts with #40 in browser.js, browser.css, panel.js, scenario-review-files.js), full run, merge; update AC-232's quoted label and AC-263's record on main. The owner (2026-09-30): icons on narrow cards are fine if they're a clear check and X ("similar to how Antigravity used to do it"); "clear" means coloured and well shaped, not transparent (a check in the accept colour and an X in the reject colour, from the design tokens); make sure they are, and show a screenshot. **17:07: a builder is merging main (with #40 and #42) into #38** (worktree `.claude/worktrees/review-default-m`), resolving the review conflicts with #40, making the narrow-card icons a coloured check and X, running the review scenarios, then marking it ready. |
| #41 | `claude/steady-tests` | Timing tests wait for events, not fixed sleeps (AC-149) | ac185 and ac189, protocol's Auto/OpenCode tests, and the UI center, continuity, review, sidebar, audit, main, modes and keyboard scenarios. **No artificial load.** |
| #37 | `claude/phone-parity` | 12 `phone/model` vitest failures (Rollup, changesOnly, copied words); adds the phone tests to test-all | Mid-way. |

**The owner allowed merging main into branches and copies (2026-09-30): "you don't need to ask you just need to do it yourself."** #41 (`97342c05`) and #37 (`70d51ef6`) have main merged in (the same `CARGO_TARGET_DIR` fix on both sides; main's side kept); both now merge cleanly.

Mergeability against main, checked 2026-09-30 00:00 with `git merge-tree --write-tree`: #32, #40, #42, #39, #38 merge cleanly; **#41 (`claude/steady-tests`) and #37 (`claude/phone-parity`) now conflict** and need main merged into them before their turn.

Merged on 2026-09-30: #40 (`4da1640e`, one layout with Follow inside the review), #42 (`86e993fd`, the menu bar; two class-table gaps fixed in the merge check), #39 (`ac7a4143`), #32 (`3ab9c1f7`; talk, conversation and review missed in the full run and passed alone). Merged on 2026-09-29: #31, #33, #34, #35, #36. Deployed to the owner: b3b7133d (#31, #33, #34). **Deployed to the owner: `9a4a2c0b`** (#42 the menu bar, plus everything before) on 2026-09-30 at 16:45: daemon restarted, the menu-bar item started and registered as a login item; VS Code windows need a reload. Before it, `ed747bc7` (#32 and earlier) at 01:03. A deploy of `c42ad18c` (#40) started 17:05.

## New from the owner (2026-09-30)

- **TUI parity:** the terminal UI should do everything VS Code Overseer does, with its diffs shown the way VS Code's review shows them, inside the TUI (the owner corrected an earlier "separate viewer" idea). A design sub-agent is writing the gap list, criteria in `docs/rfcs/tui.md` and a preview under `docs/design/tui-parity/`; the owner looks at the preview, and the build queues after the current PRs. Tracker row: "TUI parity". **A builder started 2026-09-30 03:20** on `claude/tui-parity` (worktree `.claude/worktrees/tui-parity-build`, target `/private/tmp/claude-501/tui-parity-target`): T-25, T-37, T-38, T-27 to T-29, T-39 in that order; T-26 waits for #42's `review.seen`; T-30 to T-36 later. Pushed T-25 (`1c7e6922`), T-37 (`8870ef33`), T-38 (`a47304a0`) as draft PR #43; stopped by a usage limit mid-way through the review (T-27 to T-29, edits uncommitted in its worktree) and resumed 09:25. **Finished 09:30, PR #43 (draft, `14f8aecd`):** T-37, T-38, T-28, T-29, T-39 meet their Verify clauses (`tui/tests/parity.rs`, evidence `docs/verification/evidence/tui/parity-*`); T-25 partial (the to-review mark needs #42's `review.seen`, T-26); T-27 partial (Entire worktree and the checkout's default come with #38). Changes to verified TUI criteria: sixteen per page (T-02, T-04 tests rewritten), zoom's top moved from `g` to Home (T-07), the review heading (T-14). Owner questions from it: the account on list rows shows the named account or plan; PgUp/PgDn scroll an open conversation, `]`/`[` change pages. **Next:** after #38 and #42 merge, main merged in, T-25/T-27 tightened, a full run, merge; then T-40 (dashboard mode: agents left, review middle, chat right, `D` to switch) and T-41 (`overseer-tui --grid` for a second terminal), both added by the owner 2026-09-30 after seeing the preview ("The TUI looks amazing"), then T-26, T-30 to T-36. Design done (0fb0fb27): T-25 to T-36 in `docs/rfcs/tui.md`, preview `docs/design/tui-parity/index.html` (shown to the owner at https://claude.ai/artifact/JoV4TSfWJg97WaEwULzECJ). The owner answered (2026-09-30): way 2 (beside the grid); the grid fits the count up to 16, pages from the 17th, top-level agents only (T-37); one key between grid and single agent (T-38); `e` opens their own editor (T-39); drilling into sub-agents is for later. The owner's shape: the existing TUI stays the base — mainly its grid view, plus an agent list on the side; focusing an agent shows its chat; bring over everything from VS Code that makes sense in a terminal.
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
- **Never `git commit -a` in a worktree after a full run there.** The run rewrites hundreds of evidence files; commit only the files you changed.
- **`/private/tmp` is wiped on reboot.** Push work, and keep notes on main (this file) or in memory, not only in the scratchpad.
