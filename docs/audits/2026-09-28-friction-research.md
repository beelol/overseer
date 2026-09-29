# Friction research: managing many agents by talking to Overseer, 2026-09-28

The owner's bar (`docs/goals/zero-friction.md`): zero friction to open Overseer, tell it to do
something, change course, and follow an agent then switch to Manual edit whenever, with simple
toggles and automatic behaviour — much better than Claude Code or Codex alone for managing lots of
agents, ideally just by talking. This report is what AC-216–AC-252 and the 2026-09-28 usability
audit (64 findings, AC-236–AC-249) miss, found by using Overseer the way a busy developer would,
compared against Claude Code, Codex and the best multi-agent tools, and checked against what VS
Code can actually do.

**How it was done:** a dev daemon `dev-research` (`scripts/dev up --name research`, `scripts/dev
code --name research`), fixture Claude harness only (Codex/OpenCode off, no logins, no paid
turns), two fixture repositories (`site`, `notes`). Driven over CDP with `test/ui/cdp.js` against
the real pinned VS Code window (not an ephemeral harness session), the way `scripts/dev code`
leaves it. The day in miniature: cold open, three tasks across the two repos, a permission-gated
agent followed to completion, its review opened, then 8 more agents fired at both repos to reach
11 concurrent/finished agents for the "many agents" pass. Screenshots are in
[`2026-09-28-friction-research/`](2026-09-28-friction-research/). The instance was removed
afterwards (`scripts/dev down`, `scripts/dev clean`); `ps` confirmed no `overseerd`,
`claude-fixture` or VS Code process of this session remained (other agents' own `ovs-ui-*` and
`.opencode` processes on the same machine were left untouched, per AGENTS.md).

**What this could not judge:** the fixture harness finishes in under a second, so wall-clock times
below measure *my* scripted delays, not a real model's pace — the numbers that matter are action
counts (clicks, keystrokes, commands), not seconds. Codex and OpenCode were off in this sandbox
(scripts/dev's own default), so AC-237's harness/model/account routing could not be exercised
hands-on. Redirecting a running agent mid-turn and a full Merge Back were only partly exercised;
both are called out below as unverified rather than reported as bugs.

## Top 10, ranked by how much it hurts the "manage many agents by talking" promise

1. **Following an agent buries Overseer, instead of sitting beside it.** Home, an agent's
   transcript, and its review all take turns in the same single "Overseer" editor tab, so once
   you click into an agent you have left the conversation that was supposed to be the whole point
   of talking to Overseer, and getting back means navigating away from the agent you just opened.
   Fix: an agent's Follow/chat opens in its own tab beside Overseer's conversation (or a one-key
   "back to Overseer" that never loses your place), not on top of it.
2. **Nothing distinguishes reviewed from unreviewed once several agents finish at once.** At 11
   finished agents across two repos, every row in the side bar carries the same identical
   checkmark; there is no way to tell "already looked at this" from "haven't yet" without opening
   each one, and no bulk action once several are ready. Fix: a distinct unread/unreviewed mark
   that only clears when its review is opened or merged, plus a way to act on several at once.
3. **Overseer's own conversation stays silent about a pile of finished work.** Reopening Talk to
   Overseer after 11 agents finished across both repos shows the exact same empty "What's next?"
   hero as a brand-new profile — nothing says "11 finished, 2 in notes need a look." Fix: Overseer
   leads with a one-line summary of what happened since you last looked, grouped by repository and
   outcome, before the hero.
4. **There is nothing between the flat list and the full tile grid.** The grid is meant to be the
   multi-agent overview, but at 11 finished agents it showed zero tiles ("No agent is working or
   pinned yet"); the only other view, Dashboard, showed one agent's review, not an overview either.
   Even fixing the grid to include finished agents (already flagged in the usability audit) still
   leaves nothing between an empty grid and a wall of a dozen tiles. Fix: a state rollup (working /
   needs you / done unreviewed / done reviewed / failed) that reads the same from the side bar and
   the grid, and scales past a screenful.
5. **A repository's badge shows only "active", and disappears at zero.** With one agent running
   and one finished, the side bar reads "site 1"; once every agent in a repository finishes, the
   badge vanishes entirely, so a repository that just produced four finished agents looks
   identical, at a glance, to one that was never touched. Fix: the badge counts unreviewed
   finished work too, not just active runs, so "nothing happened here" and "four things are
   waiting" never look the same.
6. **VS Code's own unused chat panel taxes every screen, all session, not just the first one.**
   The built-in "Build with Agent" panel (screenshot 01) is already a known first-look problem;
   what's new is that it never goes away — it was still open, still empty, still labelled "AI
   responses may be inaccurate", in every screenshot taken through cold open, three task starts, a
   followed agent, and an open review, permanently costing roughly a fifth of the window's width
   and squeezing Overseer's own review and chat into narrower columns than they need. Fix: close
   it the first time Overseer's own view opens, the same way `dashboard-mode.js` already closes
   the terminal panel and empty groups, and keep it closed for the session.
7. **The composer doesn't clear after sending a task.** Right after Enter starts an agent — the
   run is already visible in the side bar — the box still shows the just-submitted text
   (screenshots 04/05), with no visual confirmation that it was sent rather than sitting there
   unsent. Fix: clear the box and show a brief "sent" state the moment a task starts.
8. **Starting an agent in a second repository needs a native macOS folder picker.** Everything
   else about starting a task stays inside VS Code's own keyboard-reachable UI; the repository
   chip's "Choose…" (`composer.js`) is the one exception, posting to a native `Open` dialog with
   no keyboard-only path and no spoken equivalent — the opposite of AC-216's promise that typing
   and speaking behave the same. Fix: an in-app repository picker (recent repos, fuzzy search, a
   typed path) that never leaves the webview.
9. **Popping Follow onto another screen has no VS Code API to build on.** AC-251 already
   anticipates VS Code being unable to float a webview and asks the command to say so once; this
   research confirms that outcome is the likely one: dragging a tab into its own OS window is a
   user gesture VS Code shipped around 1.83, with no documented, stable extension API to trigger
   it programmatically — only an internal, undocumented command
   (`workbench.action.moveEditorToNewWindow`) that has moved across releases. Nothing in
   `extension/src` attempts this today. Build this expecting the fallback path to be the common
   case, not the exception, and record the VS Code version it was tried against.
10. **Two unrelated "Sign In" surfaces sit side by side.** VS Code's own top-right "Sign In" pill
    (for its built-in accounts) appears in every screenshot beside Overseer's own, completely
    separate Accounts list in the side bar ("Claude · Your login"); nothing distinguishes them for
    a new owner opening Overseer cold. Fix: hide or relabel VS Code's own sign-in affordance
    whenever Overseer's view is active, or badge it clearly as unrelated to Overseer.

## The day in miniature: steps and seconds

Numbers are action counts first (the real cost); seconds are the fixture's, not a model's.

| Step | Actions | Notes |
|---|---|---|
| Cold open → Overseer view visible | 1 click (activity bar icon) | Window opens on Explorer + VS Code's own chat, not Overseer (known, AC-236 territory) |
| Reach the composer (home) | 1 command (`Overseer: New Agent`) | ~1–2.5s to render |
| Start task 1 (site, default chips) | type + Enter = 2 actions | Run visible in the side bar in <1s (fixture speed); box does not clear (#7) |
| Start task 2 (site, second agent) | 1 command back to home + type + Enter = 3 actions | No confirmation that task 1 is unaffected |
| Explore cross-repo start | 1 click on the repo chip | No in-session keyboard path found to a second, not-yet-open repository (#8) |
| Start task 3 (permission-gated) | type + Enter = 2 actions | Reached `waiting_for_user` in <1s (fixture) |
| Follow the waiting agent | 1 click (side bar row) | Transcript + permission card visible in ~1.5s; buries Overseer's conversation (#1) |
| Answer the permission | 1 click ("Allow once") | Run completes in ~1s (fixture) |
| Open the review | 1 command (`Overseer: Open Review`) | Diff renders; window now 3–4 columns wide, one of them VS Code's unused chat (#6) |
| Merge | 1 command attempted (`Merge Back…`) | No visible dialog appeared in this session — **not confirmed either way**, flagged for a follow-up check rather than reported as broken |
| Scale to 11 agents (2 repos) | 8 tasks fired in a loop | Side bar stayed legible but gave no triage (#2); grid showed 0/11 tiles; dashboard showed 1 agent, not an overview (#4); repo badges disappeared once their agents finished (#5) |

Every moment of doubt was at a state transition, not a missing feature: did that task actually
send (#7)? Am I still talking to Overseer or did I just leave it (#1)? Did anything happen in
`site` while I was looking at `notes` (#5)? Is that stack of "done" agents in the same state or
not (#2, #3, #4)?

## Comparison: Claude Code, Codex and the best multi-agent tools

Researched from public docs/trained knowledge plus verified `--help` output for `claude`, `codex`
and `opencode` (all installed locally; no network calls, no live sessions). Claims from `--help`
are marked verified; the rest is expert judgement, flagged as inference.

- **Opening.** `claude` and `codex` cold-start into a REPL/TUI in the cwd (`-c`/`-r`/`resume`,
  `codex resume`/`fork`, verified); neither restores an arranged multi-pane *editor* layout, because
  neither owns an editor. **No competitor has an AC-250 analogue** — one command that snapshots and
  restores a real, multi-pane VS Code layout.
- **Telling it to do something.** Codex's `agents` browser, `queue --thread <uuid> --message`, and
  `cloud exec/list/status/apply/diff` (verified) are the most session-fleet-aware CLI surfaces
  seen; none pick harness, model or account *for* you — you pass flags. **Nothing routes a request
  to whichever of several installed harnesses/accounts fits and asks only when it can't**, which is
  what AC-237/AC-216 ask for and what only a daemon owning multiple harness integrations can do.
- **Changing course.** Codex's `queue` is a real building block for "redirect without switching
  context" — it posts into any thread by id from anywhere. Claude Code and opencode expect you to
  be attached (or to resume) to steer a session. None has a natural-language dispatcher sitting
  above a whole fleet the way Overseer's conversation model intends.
- **Follow → manual edit → hand back.** This is Overseer's most distinctive bet and, per the VS
  Code capability check below, its least-built one: Claude Code and Codex both surface a diff/PR
  view you read or `git apply`/checkout by hand in a separate window; none has a live, same-window
  "follow the agent's cursor with inline diffs, flip a toggle to take the pen, flip back" pattern.
  Cursor's background agents (inference) come closest — open the branch in an ordinary tab — but
  that's a separate tab, not "put you in its head" in the current window.
- **Many concurrent agents.** Codex's `agents`/`cloud list` and opencode's session list are the
  most fleet-aware surfaces verified; all degenerate into a flat list at scale, and none solve
  cross-agent conflict prediction or centralized account/rate-limit booking (Overseer's
  `account_booking.rs` has no counterpart known elsewhere) — this research's #2–#5 above show
  Overseer doesn't yet cash in that structural advantage either.
- **Notifications outside the app.** None of the three are anything but foreground, terminal- or
  IDE-attached tools; without a persistent process, none can notify once you've closed the window.
  Overseer's daemon is a structural advantage here that AC-240 already claims.
- **One conversational supervisor.** No competitor has a persona that reasons across a whole fleet
  conversationally rather than exposing a session list plus per-session chat. This is Overseer's
  sharpest edge and the reason #1–#3 above (Overseer's own conversation losing the plot at scale)
  matter more for Overseer than for anyone else — it's the one thing competitors can't copy without
  becoming a daemon-plus-extension themselves.

## What VS Code can actually do

Checked against VS Code 1.139.1 (installed here; extension manifest targets `^1.136.0`) by reading
`extension/src/**` for what's already used and reasoning from the documented extension API.

- **Floating a webview onto another window/screen (AC-251).** VS Code shipped "auxiliary windows"
  around 1.83–1.85: a user can drag any editor tab, including a webview's, into its own OS window
  and move it to another monitor. This is a **user gesture, not an extension API** — there is no
  `vscode.moveWebview` and no documented, stable command to trigger it programmatically. The
  closest is the internal, undocumented `workbench.action.moveEditorToNewWindow`, whose id and
  behaviour have moved across releases. Webview state generally survives the move (same webview
  instance, reparented) but this isn't guaranteed by any spec. **Nothing in `extension/src` attempts
  this today.** AC-251's own fallback clause ("if VS Code in use cannot float a webview, the
  command says so once") should be treated as the expected outcome to design for, not an edge case.
- **Layout control (AC-250, AC-244, AC-233).** Fully available and already partly exercised:
  `vscode.setEditorLayout`/`getEditorLayout` (stable ~1.55), the `tabGroups` API (stable 1.68,
  `tabGroups.close`), and ordinary built-in commands (`workbench.action.closePanel`,
  `closeAuxiliaryBar`, `moveEditorToRightGroup`, `focusFirstEditorGroup`, …). `extension/src/
  dashboard-mode.js` already saves and restores the full layout and panel/sidebar/auxiliary-bar
  visibility around Dashboard mode, and `extension/src/arrangement.js` already drives
  `setEditorLayout` for chat/split/grid arrangements — this is a working prototype of exactly what
  AC-250 needs, and of what finding #6 above asks for (closing VS Code's own chat panel
  automatically). AC-233 (open an agent's whole worktree, Follow with inline diffs, a Diffs-only
  toggle) needs no auxiliary-window API at all — ordinary `createWebviewPanel`/file-open APIs
  inside the current window — but **no code anywhere in `extension/src` implements Follow-with-
  inline-diffs or a Manual-edit toggle yet**; it is the single largest piece of unbuilt scaffolding
  behind this whole research effort, and the direct prerequisite for finding #1 and for AC-251/AC-257
  below.

## Proposed criteria

Numbered from the next free number on `main` (AC-252 is the highest there as of this research;
re-check immediately before merging, since parallel agents take numbers quickly).

- [ ] **AC-253 — Overseer leads with what happened while you were away.** Opening Talk to Overseer,
  or asking "what happened", after one or more agents reached a terminal state since the owner's
  last visit, gives a one-line summary grouped by repository and outcome ("3 finished clean in
  site, 1 needs you in notes") as the first thing shown, before the empty-state hero. **Verify:** a
  fixture scenario finishes 8 agents across 2 repositories while home is closed; reopening it shows
  the summary as the conversation's first message, not "What's next?"; asking "what happened while
  I was away" gives the same summary.
- [ ] **AC-254 — Reviewed and unreviewed are never the same mark.** Once an agent reaches a
  terminal state, it carries a distinct unreviewed mark, separate from "Needs you" (which is about
  action, not review), that clears only when its review is opened or it is merged; the mark and a
  count of it appear on the agent's row, its repository group, and the Agents view's own header.
  **Verify:** a fixture scenario finishes 6 agents across 2 repositories with none reviewed; the
  side bar shows an unreviewed count per repository and overall; opening one agent's review clears
  only its own mark, leaving the rest; a packaged-UI screenshot at 6 unreviewed and at 3.
- [ ] **AC-255 — A state rollup between the list and the grid.** The Agents view and the grid both
  read from one set of counts by state (working, needs you, done unreviewed, done reviewed,
  failed), shown as a small summary row that never depends on any agent being "active" to appear.
  **Verify:** a fixture scenario with 11 finished agents across 2 repositories shows nonzero counts
  in the rollup even though the grid's own tile view (which only shows working/pinned agents) shows
  none; the side bar and the grid header report the same numbers.
- [ ] **AC-256 — A repository's badge never goes quiet on finished work.** A repository group's
  count includes unreviewed finished agents, not only active ones, so a repository that just
  produced several finished agents is never indistinguishable, at a glance, from one that was never
  touched. **Verify:** a fixture scenario where a repository's last active agent finishes unreviewed
  shows a nonzero badge before and after that transition; the badge reaches zero only once every
  agent in it is reviewed or archived.
- [ ] **AC-257 — Following an agent sits beside Overseer's conversation, not on top of it.**
  Opening an agent (AC-233) does not replace the tab holding Overseer's own conversation; a single
  action returns to that conversation without losing where you were in the agent's worktree, and a
  single action from Overseer's conversation returns to the agent. **Verify:** in the packaged UI,
  opening a fixture agent from Overseer's conversation, then returning to the conversation, shows
  both the agent's worktree state (as left) and the conversation's own history intact when going
  back to the agent; screenshots of both states in sequence.
- [ ] **AC-258 — VS Code's own chat panel stays out of Overseer's way all session, not only at
  first launch.** The first time any Overseer view opens in a window, VS Code's own built-in chat
  view is closed the same way `dashboard-mode.js` already closes the terminal panel and empty
  groups, and it is not reopened by Overseer's own actions for the rest of the session (the owner
  opening it themselves is unaffected). **Verify:** a packaged-UI scenario that starts an agent,
  follows it, and opens its review never shows VS Code's own chat view in any of the three
  screenshots, at 1440×900 and 1920×1080; the owner manually reopening it is left alone.
- [ ] **AC-259 — Sending a task clears the box and says so.** The instant a typed task starts an
  agent, the composer's text field clears and shows a brief confirmation (not just the new run
  appearing in the side bar) before returning to its placeholder. **Verify:** a packaged-UI
  scenario types a task, presses Enter, and checks the field's value is empty and a sent
  confirmation appeared within one second, before the field returns to "Send off a task".
- [ ] **AC-260 — Starting an agent in another repository never needs a native dialog.** The
  repository chip's picker offers recent repositories, a fuzzy search over known repositories, and
  a typed path, entirely inside the webview; the native `Open Folder` dialog is never the only way
  to add one. **Verify:** a packaged-UI scenario adds a not-yet-open repository by typing its path
  into the repository chip's own picker and starts a task there, with no native dialog opened;
  keyboard-only from the composer.
- [ ] **AC-261 — One Sign In, clearly Overseer's or clearly not.** Whenever Overseer's own view is
  active, VS Code's own account/sign-in entry point (used by its built-in chat) is either hidden or
  visibly labelled as unrelated to Overseer, so a new owner never has to guess which one starts
  Overseer. **Verify:** packaged-UI screenshots of a fresh profile's first Overseer view in the
  three themes show either no competing sign-in control or one clearly labelled as VS Code's own.

## Recommended build order, in waves

AC-233 (open an agent's whole worktree, Follow with inline diffs, a Diffs-only/Manual-edit toggle)
already exists on the ledger, is unimplemented today, and is the direct prerequisite for finding #1
and for AC-257 below — whoever owns it should land before those. AC-250/AC-251 (layout, pop-out)
and AC-238 (Overseer checks finished work) are also already on the ledger and share files with
several criteria here; the notes below say where to coordinate rather than duplicate.

- **Wave 1 — independent, one PR each, safe in parallel (composer-only or isolated files).**
  - AC-259 and AC-260 together, one PR (`extension/media/composer.js` plus one new picker
    command): both touch the same chip; splitting them across agents would just collide.
  - AC-261 alone (wherever the extension toggles its own activation/account UI, likely
    `extension/src/extension.js`): unrelated files, no reason to wait.
- **Wave 2 — the sidebar/state-rollup cluster, one PR (they share the same tree and counts).**
  - AC-254, AC-255, AC-256 together: all three read and render the same per-run and per-repository
    state in the Agents view's tree provider, plus whatever the daemon exposes for counts
    (likely `daemon/src/store.rs`/`session.rs` alongside the extension's sidebar files). Building
    them separately would mean three PRs fighting over the same tree-rendering code.
- **Wave 3 — ambient layout, coordinate with AC-250's owner.**
  - AC-258 extends exactly the file `dashboard-mode.js`/`arrangement.js` that AC-250 is presumably
    already touching; land it as a follow-on to AC-250 in the same PR or immediately after, not in
    parallel, to avoid two agents reshaping the same layout-restore code at once.
- **Wave 4 — Overseer's own conversation, coordinate with AC-238's owner.**
  - AC-253 extends the same digest/summary logic AC-238 ("Overseer checks finished work") is
    presumably building in `daemon/src/overseer/session.rs`; sequence after AC-238 lands, or fold
    into the same PR, rather than two agents shaping Overseer's turn logic independently.
- **Wave 5 — last, depends on AC-233 and AC-250/AC-251 existing.**
  - AC-257 needs AC-233's worktree-Follow view and AC-250/AC-251's window/tab handling to exist
    first; building it earlier would mean redoing it once those land.
