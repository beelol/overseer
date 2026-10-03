# Side RFC: Overseer itself — context, control and watches across agents

Status: owner request (2026-09-27). Proposed; nothing is built. Tracked by AC-180 to AC-202 under
[Gate S](../overseer-rfc.md#gate-s--overseer-itself-added-by-the-owner-2026-09-27) in the main
RFC. The owner's [decisions](#decisions-from-the-owner-2026-09-27) are recorded below, followed by
the [proposed defaults](#proposed-defaults-distinguished-from-the-decisions-above) that stand
until the owner changes them.
Builds on the chat with Overseer (AC-107, Gate M), the run tree (AC-18, AC-19), workspace ownership
(AC-23), steering (AC-60), Needs you (AC-61) and the daemon's event replay (AC-10). Voice Mode
(Gate R) is the spoken side of the same Overseer; see [With Voice Mode](#with-voice-mode).

## Why

Overseer runs many agents and shows each one well. It does not yet *oversee* them. Each agent knows
its own task and nothing about the others; the only thing that knows about all of them is the
owner, who reads every chat and carries context between them by hand. When two agents drift into
the same files, or one goes the wrong way, the owner finds out late and fixes it one chat at a time.

The owner's request (2026-09-27), in the owner's words where it matters:

- "Orchestrator itself should have the context for all the other agents and be able to rein them
  in or change direction or send one agent to monitor another one."
- "The four in-flight agents, I see they're going into different roles. Can you make them provide
  context from each of them, have them report to you, and send the context around so they don't
  collide with each other? For example, something that can handle conflict for in-flight agents."
- "I'd like to just generally be able to talk to the overseer rather than only the agents that I'm
  firing off… not only the new task button, but also just something that, overall, you're talking
  to."
- On what Overseer is for: "Agents do produce actual work and do things. Overseer just can
  orchestrate them. Redirect them to not conflict or send them to a slightly different
  initiative. Do anything really. Kill them all at once too. Should just do what the user asks in
  reference to all the agents and automatically ensure the orchestrated agents are doing what's
  asked and redirect them. But they should still be able to do their own thing."

## Decisions from the owner (2026-09-27)

| Topic | Decision |
| --- | --- |
| Where Overseer lives | In the daemon. Talking to Overseer must not live only in the extension: "Rust should handle it." The extension, the terminal UI and the phone show the conversation; none of them owns it. |
| One chat or two | One chat, for now. Home is the conversation with Overseer, and the composer starts a new agent by default. The docked chat of Gate M shows the same conversation, not a second one. |
| What Overseer is for | Agents do the work; Overseer orchestrates them. It does what the owner asks about the agents, one of them or all at once, and by itself it makes sure each agent is doing what was asked and redirects one that is not. An agent that is on task is left to do its own thing. |
| Knowing each agent's progress | Overseer should always know how each agent is getting on. It checks in every now and then, counted in turns (every third turn), and when the agent is done. How closely it follows an agent is the owner's to direct. When the owner asks it for something, it gets up to speed first. |
| How much Overseer may do alone | Three levels. **Ask first** is the default. **Steer** is wanted. **Auto** is an option: on Auto, Overseer steers on its own. |
| What the owner types at Steer and Auto | It goes out without a yes, after a short window in which it can be corrected or cancelled. |
| A check-in when something looks wrong | Yes. Besides the cadence, a check-in is due when one of the daemon's free checks trips. |
| Caps | Overseer starts at most 100 turns a day by itself, check-ins included, and a watch wakes at most 12 times an hour. A turn that answers the owner is never counted and never refused. |
| Which model runs Overseer | The default account's harness, as AC-107 does. It can be changed, and route picking chooses when it is on. |
| Building | Nothing is started now; the criteria are on main. This gate and Voice Mode can be built in parallel. |
| Permission requests | Overseer never answers one by itself, at any level ("keep never"). The owner can answer one from the conversation. An agent that asks is an agent the owner told to ask; to let an agent work without asking, the owner runs that agent on Auto. |
| Briefings and the channel | Only when more than one agent works in a repository. A lone agent's task stays exactly as typed. |
| What Auto means | The Auto an agent has when it is left to work on its own (the permission mode). Overseer has that kind of Auto too. It is Overseer's own switch. |
| Choosing who does the work | A different layer from Overseer's Auto, and it should perhaps have another name. It is the feature of pull request #2, called Auto mode there. This document calls it *route picking*. The owner's remark is passed on in a comment on that pull request; the name is theirs to settle. |
| Who acts on what a watcher finds | Overseer. It acts for the watcher and tells the agent what to do. |
| Agents outside Overseer | Ignored for now. Only agents Overseer runs are in this gate. |
| Voice | Voice Mode is separate work (Gate R). This gate does not build it and stays compatible with it. |

## Proposed defaults, distinguished from the decisions above

These are this RFC's choices, not the owner's. They stand until the owner changes them; a change
is a recorded revision.

- **Hold on stop.** A watch can be set to hold its subject the instant the watcher raises a *stop*.
- **The name.** *Route picking* is this document's word for choosing who does the work. Candidates
  for the product's name: *Routing*, *Match*. Nothing in this gate depends on it.

## What exists today

| Piece | What it does | Where it stops |
| --- | --- | --- |
| Talk to Overseer (AC-107), on main in `extension/src/overseer-chat.js` | A docked chat that runs as a hidden task on the default harness. Every message carries a snapshot of the top-level agents (title, status, harness, repository, worktree, the first 400 characters of the last message). Overseer proposes `follow_up`, `stop`, `pin` or `start` in a fenced block; nothing happens without a yes. | It lives in the extension (its state is VS Code's `globalState`), so it stops with VS Code and the phone cannot reach it. The snapshot is shallow: no conversation, no changes. A follow-up to an agent that is still working is refused by the daemon. No holds, no reports, no conflicts, no watches. |
| Voice Mode (Gate R, AC-162 to AC-177), criteria on main, not built | The spoken side of Talk to Overseer: a request by voice reaches the right agents and shows what was sent. | It needs the Overseer session in the daemon, which does not exist yet, and a view of every agent, which this gate builds. |
| Home composer (AC-59, AC-72, AC-111) | "What's next?" starts an agent. | It only starts agents. |
| Steering (AC-60) | Queue a message, or stop and send. | The queue is kept by the extension; the daemon refuses a follow-up while a run is active. |
| Workspace ownership (AC-23) | One writer per workspace; a second is refused. | Agents in separate worktrees can change the same lines and nobody knows until merge back. |
| Needs you (AC-61) | Agents waiting on the owner. | Nothing about agents colliding or drifting. |
| Oversight passes (AC-146, AC-157) | A session outside Overseer checks the agents building Overseer and merges their work. | By hand, from Git and pull requests, not a feature of the product. |

This gate keeps AC-107's ID and behaviour and moves what it does into the daemon.

## Vocabulary

| Term | Meaning |
| --- | --- |
| Overseer | The orchestrator: one conversation, plus what the daemon coordinates. One per daemon. |
| Agent | A top-level run with its native children, started from any surface. |
| Digest | The daemon's bounded summary of one agent, built from its events with no model call. |
| Roster | One line per agent. |
| Area | The paths an agent works in: what it claimed or was given. |
| Briefing | A short paragraph Overseer adds to an agent's task about the agents working beside it. |
| Channel | How an agent reaches Overseer while it works: *report*, *ask*, *claim*. |
| Check-in | Overseer looking at what an agent did since the last time, to see that it is on task. |
| Level | How much Overseer may do without asking: *Ask first*, *Steer* or *Auto*. |
| Proposal | An action Overseer states and waits on. |
| Card | The record of one request or action in the conversation: one row per agent. |
| Hold | The agent starts no new turn until released. |
| Guardrail | A standing limit on an agent: words repeated each turn, paths the daemon watches. |
| Redirect | Stop the turn and give a new direction. |
| Share | A piece of one agent's context sent to another, from Overseer. |
| Conflict | Two agents in flight whose work collides. |
| Watch | A watcher observing a subject and filing findings. |
| Finding | A watcher's result: *fine*, *concern* or *stop*. |
| Rally | Gather the agents: who owns what, where they overlap, what each needs. |
| Route picking | Choosing harness, account, model and effort for a piece of work (Auto mode in pull request #2). |

## Shape

```
  VS Code  ·  terminal UI  ·  phone  ·  voice       one conversation, attached from anywhere
        │
        ▼
  overseerd
   ├─ Overseer's conversation ── model turns on the default account's harness
   │     reads   roster, digests, conversations, changes, conflicts      (read tools)
   │     acts    proposals ─► the daemon checks ─► actions               (a fixed set)
   │
   ├─ what it coordinates      digests · reports · areas · conflicts · holds · guardrails · watches
   │                           kept and enforced by the daemon, with no model
   │
   ├─ agents        ◄── messages, redirects, shares        ──► reports, asks, claims
   ├─ watchers      read-only                              ──► findings
   └─ swarm runs    director and workers   ◄── the swarm's own controls, advisories to the director
```

Two halves, kept apart on purpose. The **daemon half** needs no model: digests, conflict detection,
holds, guardrails and the wake rules are code, work with VS Code closed and offline, and cost
nothing. The **model half** is the conversation: it reads through the daemon, reasons, and asks the
daemon to act. The model never touches an agent, a worktree or a shell directly.

Agents do the work. Overseer produces no code and edits no file.

## The conversation

Home (no agent selected) shows the conversation with Overseer above the composer.

- **The target.** The composer sends to *New agent* by default. Enter starts an agent exactly as
  today (AC-59): no model turn in between, nothing to misread. The start appears in the
  conversation as a card, so Overseer knows about every agent without being asked.
- **Talking to Overseer.** One key, the target chip, or `@overseer` as the first word. `@` offers
  the agents by name above the files it offers today; a named agent reaches Overseer as that
  agent's id, so it never guesses from a title.
- **Wrong target.** One click corrects it: *Ask Overseer instead* stops the agent that was just
  started and removes its untouched worktree; *Start as an agent* starts one from the message.
- **What shows in it.** The owner's messages and Overseer's answers, and cards: agent started,
  request, proposal, check-in, conflict, finding, hold, report, rally. Cards carry their own
  buttons, so the common answers need no typing and no model turn.
- **The docked chat** (AC-107) is the same conversation, for when an agent's chat or review holds
  the editor area.
- **Start fresh** archives the conversation and begins a new one. Holds, guardrails, areas,
  conflicts and watches are untouched.

*Let Overseer decide* (every message goes to Overseer, which starts agents itself) is left as a
later setting. It costs a model turn and a few seconds on every message, and a task could be
misread as a question.

## Context: what Overseer knows

### The digest

For every agent the daemon keeps a digest from its events. No model writes it.

| Field | From |
| --- | --- |
| Title, role (agent, watcher, director, worker) | the run |
| What was asked: the task, the owner's later messages, Overseer's directions | turns |
| Status and since when; what it waits for | events, attention |
| Harness, account, model, effort, permission mode | the launch record |
| Repository, branch, worktree, base | the workspace |
| Changed files with counts (50, then a count) | `workspace.changes` |
| Its last three messages, 400 characters each | events |
| Children and their states | the run tree |
| Usage, or *not reported* | usage events |
| Last report, area, holds, guardrails, watches, open conflicts, last check-in | this gate |

At most 4 KiB per agent, current within 2 s of the event that changed it. The roster is one line
per agent, at most 16 KiB; past that, the oldest finished agents fold into counts.

### Reading on demand

A snapshot pushed into every message (AC-107 today) is shallow and grows with the number of agents.
Instead each turn starts with the roster and the digests of the agents that were named or that
changed since the last turn (32 KiB in all), and Overseer reads the rest when it needs it, through
tools the daemon gives its run:

| Tool | Returns |
| --- | --- |
| roster, agent | the roster; one digest |
| conversation | a range of an agent's conversation |
| changes, diff, file | changed files; one file's diff or contents, under AC-34's rules |
| search | matches across all agents (the daemon's search, AC-63) |
| conflicts, reports, usage | what the daemon holds |
| propose | asks the daemon for actions (see [Control](#control-what-overseer-can-do)) |

Every answer is bounded (32 KiB) and redacted. The proposed way to give a harness these tools is an
MCP server the daemon provides over a small stdio shim, set up for that run only and never written
into the user's own configuration (the rule Continuity follows for OpenCode). The spike (AC-180)
settles it per harness. Where a harness cannot take tools, the turn carries the digests and nothing
else, and the chat says so.

Overseer's run has **no shell, no file writing and no network tools of its own**, and its folder is
empty. Everything it sees comes through the daemon.

### Agents that know about each other

Today: the owner starts *Login API* in a repository, and later *Login page* in the same one.
*Login page* has no idea that *Login API* exists. When it needs to know what the login endpoint
returns, it guesses, or it opens `api/` and changes it. And neither agent can tell Overseer
anything: each can only answer in its own chat.

With this gate, two things happen when *Login page* starts.

**1. A briefing.** Overseer adds a short paragraph to the task the owner typed:

> Another agent, *Login API*, is working in this repository, in `api/`. Leave `api/` to it. If you
> need something from it, or you find something it should know, tell Overseer.

The owner sees it in the agent's chat as one line, *Overseer added a briefing*, which opens to the
full text. Nothing is added to a task without being shown. A briefing is at most 1 KiB.

**2. A channel back to Overseer.** The agent gets three commands it can run while it works, the
same way it runs `git` or the tests:

| The agent runs | It is saying | What the owner sees |
| --- | --- | --- |
| report | "This is what I am doing and what I have changed." | A report card in the conversation with Overseer |
| ask | "What does the login endpoint return?" | Overseer answers from what it knows about *Login API*, or asks that agent, and sends the answer back |
| claim | "I am taking `web/login/`." | The agent's area; another agent writing there is a conflict |

Without the channel, Overseer still sees every agent from outside (the digest) and can still
message any of them. The channel is what lets an agent *start* the conversation, in the middle of
its work, instead of waiting to be asked.

**When.** The owner's decision: only when more than one agent works in a repository. A lone agent
gets no briefing and no channel and works exactly as today. It can be set per agent and as a default. An
agent that is already running gets its briefing as a queued message when a second one starts or
when the owner rallies.

**How.** The commands are tools the daemon gives the agent's harness; the spike (AC-180) settles
how per harness. They are the operations Swarm's workers have, with Overseer as the one they reach
instead of a director, under the same rules: a stable id per message, stored before it is
acknowledged, one effect however often it is repeated. Who is speaking comes from the run's token,
never from the text.

**Rally.** "Rally my agents" is one operation. It answers from the digests first, which is free.
It asks an agent for a report only where the digest cannot answer (no area, an unclear task), says
how many agent turns that costs before it spends them, and returns one map: who owns what, where
they overlap, what each needs, and the areas and shares it proposes. One yes applies it.

## Control: what Overseer can do

### The actions

The daemon sorts every action into one of four classes, whatever the model claims. They are the
tiers Voice Mode uses (AC-171), so typing and speaking follow one set of rules.

| Class | Actions | How |
| --- | --- | --- |
| **Look** | Questions about the agents; select or track an agent; the grid; pin | At once |
| **Steer** | Message, share within one repository, ask for a report, set an area, hold, release, guardrail, redirect, stop, watch, up to three new agents | As the level says (next section) |
| **Confirm** | Answering a permission request; merge back; a pull request; archive; more than three new agents; starting a swarm or raising its limit; a share across repositories | Only when the owner asked. Read back in one sentence, then a yes. At every level |
| **Not from the conversation** | Accounts and sign-in; phone access and devices; settings; cleanup; review actions; stopping the daemon; Overseer's own level and caps | Overseer opens the place in the UI and says so |

An agent's permission mode (Ask first, Accept edits, Auto) is a Steer action since AC-230: the owner
sets it by conversation, typed or spoken, and starts agents in a stated mode. The owner decided
(2026-09-28) that Overseer may set Auto on its own, within its level and only in the repositories
the owner allows (`overseer.auto_repos`, set on the Mac); each time, the reason is recorded on the
agent and its card. A waiting permission request comes up by itself in the conversation as a
yes/no, and Voice Mode reads it out.

- **One, several or all.** Every Steer action can name one agent, several, or all of them:
  "stop everyone", "hold everything in `overseer` until I'm back".
- **Overseer never starts a Confirm action by itself.** It can show that an agent waits for a
  permission; the answer is the owner's.
- **Native children** are steered through their parent, as today.

### Cards: what was sent

Every request the owner makes and every action Overseer starts by itself has a card in the
conversation. It is Voice Mode's card (AC-169), so a typed request and a spoken one leave the same
record.

```
14:32 · typed
"Phone and Continuity should both use the new wire format."

 Phone app     named   redirect   picked up 14:32:13   Switch the gateway client to…
 Continuity    named   add        queued               When this turn ends: use the…
```

| Column | Content |
| --- | --- |
| Agent | Logo and name; a click opens its chat at the message |
| Why | The reason it was chosen: named, owns the subject, a check-in, a conflict, a finding |
| Delivery | *Add* (waits for the end of the agent's turn), *redirect*, *stop* or *start*, as AC-167 |
| State | Held, sent, delivered, picked up, answered; or failed, cancelled, not sent. Each with its time |
| Message | The first line; it opens to the whole text |

The text in the card is the text that was sent, byte for byte. In the agent's chat the message
reads as a turn from Overseer. AC-107 marks these messages with a text prefix today; here the turn
carries its source as data. Each action is also an event with `overseer` as its source, what led to
it, and who approved it and where.

### Asking first

The level decides how Steer actions happen.

| Level | What the owner asks for | What Overseer starts by itself |
| --- | --- | --- |
| **Ask first** (default) | A proposal, then a yes | A proposal, then a yes |
| **Steer** | Goes out after a settle window | Message, share, ask for a report, area, hold, release, guardrail: done and announced. Redirect, stop, new agent, watch: a proposal |
| **Auto** | Goes out after a settle window | Done and announced |

- **The settle window** is Voice Mode's (AC-170): 2 s in which the card is shown and the request
  can be corrected or cancelled. A stop has none.
- **Spoken requests** follow Gate R at every level: they go out after the settle window. Gate R
  recorded that decision.
- **Announced** means a card in the conversation and the turn in the agent's chat, with the cause.
- **The level is Overseer's own switch**, set by the owner on the Mac and enforced by the daemon,
  not by the prompt. It is named after the permission mode agents have. Route picking is another
  switch; neither changes the other.
- **The classes and the caps hold at every level.**

A proposal says exactly what will happen, to which agents, with what text. It is answered once: a
second answer from any surface gets the first one's outcome (the rule of AC-125). A proposal whose
agent changed state since it was made is made again, not carried out. Proposals waiting for the
owner are in Needs you.

### Keeping agents on task

"Automatically ensure the orchestrated agents are doing what's asked and redirect them. But they
should still be able to do their own thing."

**What was asked** of an agent is its task, the owner's later messages, Overseer's directions, its
guardrails and its area.

**Free checks.** The daemon runs these all the time, with no model:

| Check | Trips when |
| --- | --- |
| Outside its area | The agent writes outside its area or across a guardrail |
| Conflict | Its work collides with another agent's ([Conflicts](#conflicts-between-agents-in-flight)) |
| Going in circles | The same command fails three times in a row |

**Check-ins.** Overseer's model looks at what changed since the last check-in (the digest, the new
messages, the changed files) and gives one result, with its reason:

| Result | What happens |
| --- | --- |
| *On task* | Recorded. Nothing is sent to the agent. |
| *Drifting* | Overseer acts at its level: a proposal at Ask first, a message or a hold at Steer, a redirect at Auto. |
| *Done* | When the agent finishes: a card with what was asked, what was done and what was left out. |

**When.** The owner's decision: every now and then, counted in turns, and when the agent is done.

| A check-in is due | |
| --- | --- |
| Every third turn of the agent | The default. |
| When the agent finishes | Its result is *done*, or *drifting* if it stopped short. |
| When a free check trips | Something looks wrong. Confirmed by the owner. |
| As the owner directs | "Check on Phone every turn." "Leave Docs alone until it is done." Said in the conversation or set on the agent: every turn up to every twentieth, only when done, or off. |

Check-ins for several agents within 5 s are one turn.

**Getting up to speed.** When the owner asks Overseer for anything, it first takes in every agent
that changed since it last looked: their digests, within the turn's 32 KiB, and the rest through
its tools. So an answer is never older than the agents' latest events, however long Overseer was
idle.

**What a check-in does not do.** It judges *what* the agent is doing, not *how*. An agent that is
on task hears nothing from Overseer and works as it would alone.

A check-in reads the digest. A [watch](#watches) is the deeper tool: an agent of its own that
reads the diffs and can run the tests.

### Rein in: holds and guardrails

A **hold** keeps an agent from starting a new turn. Its current turn finishes, or stops with
*hold now*. Messages queue behind it. It names who set it, why, and what releases it: a release, a
conflict resolved, another agent finishing, or a time. A hold is a flag on the agent, not a new
run status, so no surface meets a status it does not know. The owner's own message to a held agent
offers *Release and send*.

A **guardrail** is a standing limit: words repeated to the agent at the start of each later turn
(1 KiB per agent), and paths it must stay in or out of. The daemon watches the paths. A write
across a guardrail is reported within 2 s and, when the guardrail says so, holds the agent at once
with no model turn. A guardrail reads *enforced* only where the harness itself refuses the write;
otherwise it reads *watched*. The same honesty AC-23 asks of read-only labels.

### Change direction: redirect

1. The daemon takes a snapshot, so nothing uncommitted can be lost.
2. It stops the turn. Where the harness cannot be stopped mid-turn it waits for the end and says so.
3. It sends the direction as the next turn, from Overseer.
4. The card shows *delivered* when that turn starts and *picked up* when the agent has taken it
   in: through its channel where it has one, otherwise when it answers.
5. What the agent wrote before reads *before the change of direction*; the review gains *since the
   change of direction* as a comparison.
6. If it is not picked up, the card says so. It is not sent again by itself.

Queue and stop-then-send become daemon methods. Today the extension holds the queue, so a queued
message dies with the window and no other surface can queue one. The phone (AC-125) and Voice Mode
(AC-167) need the same; whichever is built first defines the methods.

## Sharing context

*Share* sends a report, a finding, a range of messages, a diff, or a note Overseer wrote, from one
agent to another, as a message from Overseer that names where it came from.

- At most 8 KiB inline. Larger pieces go as a patch file in the receiving run's folder, or as a
  branch and commit the agent can read (worktrees of one repository share their objects).
- Within one repository a share is a Steer action. Across repositories it is a Confirm action.
  A destination the owner has denied, Swarm's context permissions included, is never used.
- Redaction applies.
- A share that turns out wrong is withdrawn, and everyone who received it is told.

## Conflicts between agents in flight

The daemon finds them. No model is involved in finding one.

| Kind | How it is found | Weight |
| --- | --- | --- |
| Same lines | A trial merge of the two agents' snapshots conflicts | needs a decision |
| Area crossed | A write inside another agent's area | needs a decision |
| Same file | Both changed it; the trial merge is clean | advisory |
| Target moved | The agent's work no longer merges into its target branch | advisory |
| Same workspace | A second writer in one workspace | refused already (AC-23) |
| Same purpose | Overseer's or a watcher's judgment | advisory, labelled as judgment |

- **Trial merges** run on snapshot commits with `git merge-tree`, so uncommitted work counts and
  no worktree, index or branch is touched. The daemon already takes such snapshots for the review.
- **Who is compared.** Agents in the same repository that are active, or finished and not yet
  merged. Only pairs where one side changed are compared again.
- **When.** Within 10 s of the edits settling. A conflict goes away by itself when the overlap does.
- **The card** offers, with no model turn: *assign* (one agent keeps the path, the other gets a
  guardrail), *sequence* (hold one until the other finishes, then tell it to bring in that
  branch), *share* and *dismiss*. Overseer may propose one of them with its reason, and at Auto
  it settles the conflict itself and says how.
- **Where it shows.** A card in the conversation and a badge on both agents. Only the kinds that
  need a decision wait in Needs you.

## Watches

"Send one agent to monitor another one."

Overseer itself looks at every agent lightly, through check-ins. A **watcher** is something more:
an ordinary agent given one job, to look closely at another agent's work.

- **Read-only.** The watcher reads its subject's digest, conversation and changes through the
  daemon's tools. It has no write access to the subject's workspace.
- **Woken by events, not by a clock.** The daemon wakes it when the subject's turn ends, and once
  when the subject finishes, with what changed since the last wake (32 KiB, the rest on demand).
  Nothing changed means no wake.
- **Findings.** *Fine* is recorded and silent. *Concern* goes to Overseer. *Stop* goes to Overseer
  at once.
- **Who watches.** A new agent, or an idle agent the owner names. With route picking on, the route
  prefers a different model or provider from the subject's, so the check is independent.
- **A watch that checks.** Set to *check*, the watcher gets a worktree of its own at the subject's
  latest snapshot, refreshed at each wake, where it can run the tests. It never writes to the
  subject's worktree. This is what catches "the tests pass" when they do not.
- **Limits.** No watcher of a watcher, no circle, two watchers per subject, 12 wakes an hour per
  watch. A watcher counts toward the agent limit, as Swarm already counts a separately launched
  reviewer.
- **The end.** The subject finishes, the watch is ended, or its budget is reached; it says which.

### Who acts on a finding

Agent A watches agent B. A sees B deleting tests so that the suite passes. Someone has to stop B.

It is not A. A files the finding (*stop*, with the evidence) and does nothing else: it never
messages B and never stops it. Overseer acts for it and tells B what to do, at its level:

| Level | What happens to B |
| --- | --- |
| Ask first | Overseer proposes to hold B and says why. It waits for the yes. |
| Steer | Overseer holds B at once and tells the owner. Redirecting B is a proposal. |
| Auto | Overseer holds B and redirects it, and tells the owner. |

Why not let A deal with B directly: two agents instructing each other have nobody in charge, and
what A told B would be something Overseer and the owner never saw. With one chain of command,
everything that reaches B comes from Overseer, is attributed, and is in the record.

One switch per watch is for the owner who does not want to wait for a model turn: **hold on
stop**. With it on, the daemon holds B the instant A files a *stop*, at any level. What happens
next is still Overseer's, at its level.

## Goals

The owner's request (2026-09-28): give an agent a goal it keeps working toward until it holds,
and give Overseer itself one by talking to it. Tracked by AC-224 (an agent's goal) and AC-225
(Overseer's goal) in the main RFC. Design only; nothing is built. The preview is
[docs/design/goals](../design/goals/index.html).

### What a goal is

A goal is a **condition in plain words** plus **how it is checked**. Nothing else: no plan, no
steps. The agent, or Overseer, decides how to get there.

| Check | How it is decided | Costs |
| --- | --- | --- |
| **A command passes** | The daemon runs the command in the agent's worktree (for Overseer's goal: in the named repository's checkout or a fresh worktree of the named branch) when a turn ends. Exit code 0 holds; anything else does not, and the last 40 lines of output are the reason. | No model turn. Time to run the command (10 minutes at most, then it counts as not holding). |
| **Overseer reads the work** | A check-in (the one in [Keeping agents on task](#keeping-agents-on-task)) with the goal as its question: the digest, the new messages, the diff and the last test output. Its answer is *holds* or *not yet*, with one sentence why. | One Overseer turn per check. |
| **The owner says so** | The goal never holds by itself. When the agent says it is done, a card asks the owner *Holds* or *Not yet*; *Not yet* takes a sentence, which becomes the next turn's direction. | Nothing until the owner answers. |

- **Default check.** A goal set with a command ("until `cargo test --workspace` passes") is
  checked by the command. A goal with none is checked by Overseer reading the work. The owner's
  check is chosen explicitly ("until I say it's done").
- **Who may set a command.** The owner, typing it or approving it. Overseer may propose a command
  for a goal it was given in words ("until the tests pass" in a Rust repository becomes `cargo
  test --workspace`), and the proposal shows the exact command; it never takes one from an
  agent's text. The command runs with the agent's own permissions in its own worktree, nothing
  wider.
- **One goal per agent, one for Overseer.** A new goal replaces the old one, and the
  conversation says so.
- **States.** *Working toward it*, *holds* (done), *stuck* (with a reason), *waiting on you*,
  *paused* (a limit, a hold or a recovery), *cleared*. Every change of state is an event with its
  reason, so every surface reads the same thing.

### An agent's goal

Set from the agent's chat (a goal chip in the composer, or `/goal` typed there), from the row's
menu, from the phone or the terminal UI, or by telling Overseer ("keep Docs going until the
links check passes"). Setting it from the conversation is a Steer action, so the level applies.

**Where the harness has goals of its own, they are used.** Claude Code's `/goal` keeps the agent
going with a Stop hook until its condition holds. When the agent runs on a Claude Code version
that has it, Overseer sends `/goal <condition>` as the agent's next input, exactly as the owner
would type it, and the harness does the continuing inside one turn of Overseer's. The daemon
still checks when the harness stops: Claude Code's own check is the model's judgment, and a
command check is the daemon's to run. If the daemon's check does not hold, the daemon continues
the agent as below. Which versions have `/goal` is read from the harness's own help at the
first launch and kept with the profile; a version without it falls back to the daemon. The repo
records nothing about Codex having goals of its own; if Codex or OpenCode gains one, it is used
the same way, by version.

**Where it has none (Codex, OpenCode, a generic program, or Claude Code without `/goal`), the
daemon holds the goal.** At the end of each turn:

1. The daemon runs the check.
2. If it holds: the goal is *done*, the card says so with the evidence (the command's last lines,
   or Overseer's sentence), and nothing more is sent.
3. If not, and no limit is reached: the daemon continues the agent with one message, from
   Overseer, that restates the goal and gives the reason it does not hold yet:
   *"Goal: `cargo test --workspace` passes. Not yet: 2 tests fail in `store::tests` (output
   below). Keep going."* It is delivered like any queued message, at the end of the turn.
4. If a limit is reached, or the agent is going in circles: the goal is *stuck* and the card says
   why in one line. The agent stays as it is; nothing is reverted.

**Going in circles.** Three continuations in a row with the same failing reason and no change to
the agent's files is *stuck: no progress after three tries*. The same free check Overseer already
runs for a command failing three times.

**Clearing.** The owner clears a goal from the goal line, the chat or the conversation. A
continuation already queued is withdrawn; a turn in flight finishes as a normal turn. On Claude
Code, clearing also sends the harness's own clear (`/goal` with no condition, or what its help
names).

### Overseer's goal

Set by talking to Overseer: "keep going until the tests pass on main", "until every criterion on
the tracker is met or waits on me". Overseer reads it back in one sentence with its check ("I'll
keep working until `scripts/test-all` passes on main; I'll check after each agent finishes. Go?")
and the owner says yes. At Ask first and Steer the read-back waits for a yes; at Auto it goes out
after the same 2-second window as anything else the owner types, and can be stopped in that window
(the owner, 2026-10-02: "auto is fine"). The default limits below are the owner's choice of the same
day.

- **Where it lives.** In the daemon, with who set it, from which surface and when. It survives a
  restart; after one, Overseer checks the goal once before doing anything else for it.
- **When Overseer works on it.** At the end of each of its own turns and whenever an agent
  finishes, fails or comes back with trouble, it checks the goal. If it does not hold, it takes
  the next step within its level: start an agent, message one, propose a merge, give an agent a
  goal of its own. The level gates every action exactly as in [Asking first](#asking-first); a
  goal never raises it. At Ask first, a goal mostly produces proposals, and Overseer waits for
  them.
- **Progress.** A goal line at the top of the conversation: the condition, the last check and
  its result, what Overseer is doing about it ("2 agents working: Store fix, Flaky test"). Asked
  "how's it going?", Overseer answers from the same record. It reports by itself only when the
  goal holds, is stuck, or waits on the owner.
- **Waiting on the owner.** When the only way forward is the owner's (a Confirm action, a
  permission, a proposal at Ask first, a question), the goal is *waiting on you*: one card, asked
  once, in Needs you. Overseer takes no more turns for the goal until the owner answers.
- **Stops.** When the goal holds (a report: what was done, by which agents, the check's
  evidence), when it is stuck (the reason, and what Overseer would try next if allowed), when it
  is cleared, or when a limit is reached.

An agent's goal and Overseer's goal compose: Overseer working on "tests pass on main" may give
the agent fixing a test its own goal, "`cargo test -p store` passes". The agent's goal is
continued by the daemon with no Overseer turn; Overseer looks again when that agent's goal is
done or stuck.

### What the owner sees

- **On the agent's row**, under the title: the goal mark, the condition (one line, cut to fit) and
  the state: *2 of 20 · not yet: 2 tests fail*, *done*, *stuck: no progress after three tries*,
  *waiting on you*. The terminal UI shows the same line in the tile; the phone shows it under the
  agent's name.
- **In the agent's chat**, a goal line pinned above the composer with the same state and *Clear*
  and *Edit*; each continuation reads as a turn from Overseer with the reason, and each check is a
  small line between turns ("Checked: not yet, 2 failing").
- **In Overseer's conversation**, the global goal pinned at the top; *Goal set*, *Goal done* and
  *Goal stuck* cards for both kinds; agent goals Overseer set appear on its action cards.
- **Done and stuck** are the two states that reach the owner when away: a notification (where
  AC-240's notifications are on) and one attention cue in Audio Mode.

Owner-facing text never shows an internal id, an error class or an HTTP code. A stuck reason is a
sentence: "stopped after 20 tries; the last one still had 1 failing test".

### Limits

Every goal has limits; whichever is reached first stops it, and the card names which.

| Limit | Agent goal default | Overseer goal default |
| --- | --- | --- |
| Continuations (turns started for the goal) | 20 | Counted in Overseer's 100 self-started turns a day; no separate cap |
| Time since the goal was set | 4 hours | 24 hours |
| Usage | The account's own limit (then recovery, below) | The same, for each agent it starts |
| No progress | Three continuations with the same reason and no file change | Three of its own checks in a row with the same reason and nothing done in between |

- The owner can change the limits when setting the goal ("for at most 5 tries", "until tonight")
  or on the goal line.
- **Paid turns.** A continuation is a paid turn like any other. Building and verifying goals
  follows the project's paid-turn rules: fixtures throughout, and any live check runs only on the
  allowed model at low effort, one attempt per step. The defaults above are for the owner's own
  use; the verifying fixtures use limits of 3 to 5.
- A turn that answers the owner is never counted, as before.

### With Auto, the account booking and recovery

- **Overseer's Auto level.** Auto lets Overseer act on a goal without asking; it does not lift
  any Confirm action, any limit or any cap. Setting Overseer's goal still takes the owner's yes.
- **An agent's permission mode.** A goal does not change it. An agent on Ask first that waits for
  a permission is *waiting on you*; nothing answers the permission for it.
- **Route picking and the account booking.** Each continuation is a new turn, admitted through
  the one account booking like any other: with route picking on, it may be booked on an account
  with room; with it off, it uses the agent's own route. A goal never books around a refusal.
- **Recovery (AC-239).** When an agent working on a goal hits a usage limit, fails or goes
  silent, recovery runs first, unchanged: Overseer explains it and offers (at Auto, does) the
  fix. The goal is *paused* meanwhile. An agent continued on another account or model keeps its
  goal, its count and its limits; continuation resumes after the successor's first turn ends. A
  recovery that is declined or fails makes the goal *stuck* with the recovery's reason.
- **Holds and redirects.** A held agent's goal is *paused*; a release resumes it. A redirect
  keeps the goal unless the owner or the redirect says otherwise; the card asks when unclear.
- **Swarm.** A swarm's goal is its objective, kept by its director. Overseer does not continue
  swarm workers; a goal set on a swarm becomes a revision of its objective, through the swarm's
  own controls.

### Protocol and data (indicative)

- **Methods.** `goal.set` (target: an agent or Overseer; condition; check: command, read or
  owner; limits), `goal.clear`, `goal.get`, `goal.answer` (the owner's *holds* / *not yet*).
  Reading is `read`, setting and clearing are `control` with a request id, for the phone.
- **Store (additive).** `goals` (target, condition, check, limits, set by, surface, time, state,
  reason, count) and `goal_checks` (time, result, evidence).
- **Events.** `goal_set`, `goal_check`, `goal_continued`, `goal_state` (with its reason),
  `goal_cleared`.
- **Code.** A new `daemon/src/overseer/goals.rs`, called from where a turn ends (as check-ins
  and the finished-work check are) and from trouble recovery.

## With Voice Mode

Voice Mode (Gate R, [its RFC](voice-mode.md)) is separate work. This gate does not build it. The
two describe one Overseer, so they share what follows.

| Topic | Rule |
| --- | --- |
| One Overseer | Typed and spoken messages are one conversation with one memory, kept by the daemon. Gate R says the session moves from the extension into the daemon; this gate is where it is built (AC-181). The two can be built in parallel: the session's daemon methods are the contract, written down by whoever reaches that step first. |
| What the orchestrator sees | Voice Mode's view for a request (roster, focus, recent events) is drawn from this gate's roster and digests. One source, so both know the same things about an agent. |
| Classes | The four tiers of AC-171 are the four classes here. |
| Delivery, states, cards | AC-167's add, redirect and stop; AC-169's states and card. A typed request and a spoken one leave the same record. |
| Levels | A spoken Steer request goes out after the settle window at every level. The level decides typed requests and what Overseer starts by itself. |
| Holds, guardrails, areas, watches | They can be asked for by voice like any Steer action and show in the card. |
| Speaking up | Gate R's default is that Overseer speaks only in answer. So check-ins, conflicts and findings arrive as cards and as Audio Mode's attention cue, not as speech. |
| Without a model | Voice Mode's built-in phrases ("stop everyone", "what is running") are served by the daemon half. |

## With Swarm

Swarm (pull request #3, `docs/rfcs/swarm-mode.md` on its branch) gives a category one director and
many workers. Its rule: *never create a second independent decision-maker for the same run.*
Overseer sits above directors and keeps that rule.

| Topic | Rule |
| --- | --- |
| Who decides inside a swarm | The director. Overseer never assigns, accepts or rejects a job and never messages a worker. |
| How Overseer sees a swarm | As one agent: the director's summary, jobs by state, blockers, allocation used. Workers sit under it in the roster. |
| How Overseer acts on a swarm | Through the swarm's own controls (pause, resume, stop, Swarm off, the active limit, a changed objective as a plan revision), and advisories to the director's inbox. |
| Keeping a swarm on task | Overseer checks in on the director, not on each worker. Keeping workers on task is the director's job. |
| Areas and conflicts | One ledger for swarm workers and every other agent. Swarm already has claims and conflicts (`swarm.claim`, `swarm.conflict.*`); two ledgers would let a worker and an agent both own one path. |
| Messages from agents | One broker. Reports, asks and claims use the rules Swarm's broker has. |
| Watching a worker | Allowed, read-only. The finding goes to Overseer, then to the director as an advisory. |
| Agent slots | Agents and watchers Overseer starts count toward `agents.max_active`. Overseer's own run does not, so a full house never locks the owner out of it. |
| Starting a swarm, raising its limit | A Confirm action at every level, Auto included: it commits an allocation. |
| Rank | The owner, then Overseer, then a director, then its workers, all within the daemon's limits. |
| Swarm off or absent | Everything else in this gate works with plain agents. |

Whichever of Swarm and this gate lands second adopts the first one's tables for areas, conflicts
and the broker. The criterion (AC-195) stays partial until both are on main, as Swarm's own
contract criteria do.

## With route picking

Route picking (pull request #2, `docs/rfcs/auto-mode.md` on its branch, where it is called Auto
mode) chooses the route for each piece of work: harness, account, model and effort. It is a
different layer from Overseer's Auto level, which is about acting without asking. The owner has
said it should perhaps have another name; that is for pull request #2 and the owner.

| Topic | Rule |
| --- | --- |
| Two switches | Route picking on does not put Overseer on Auto, and the reverse. |
| Who decides what | Route picking chooses who does the work. Overseer decides direction. Neither overrides a route the owner pinned. |
| Agents and watchers Overseer starts | With route picking on they are work units it routes; with it off they use the owner's defaults. |
| Admission | One admission for every launch (allowance, agent slot, workspace, launch intent), the one pull requests #2 and #3 agree on (CONTRACT-01). Overseer cannot over-admit. |
| Overseer's own turns | Metered like any work. With route picking on, their model and effort may be chosen for them: a check-in needs less than untangling a conflict. |
| A redirect | Keeps the agent's route unless the owner asks. On a routed task it is a new work unit, so the route may be reassessed. |
| Continuation and replacement | The successor is the same agent to Overseer. |
| Permissions | A denial is never worked around through another agent. |
| Usage learning | Overseer's turns and watchers are recorded like other work. |
| Its delegation interface | Stays inside a task (a parent and its work units). Overseer sits above tasks. |

## With Continuity

- An agent that is handed off (pull request #9) stays one agent: digest, holds, guardrails, area,
  watches and conflicts move to the successor.
- Messages and redirects to an agent in `waiting_for_connection` or `waiting_for_memory` queue and
  arrive once.
- A watcher stays read-only through a handoff; permission modes are never loosened (AC-138).
- Overseer's own run follows Continuity like any run. When no model can run it, the conversation
  says so, and the daemon half keeps working: digests, free checks, conflicts and their cards,
  holds, guardrails.

## With the phone, the terminal UI and Audio Mode

- **Phone** (pull request #10). AC-128 waits for AC-107 and says the session lives in the daemon;
  this gate builds that session (AC-181). Every new method needs a class in
  `protocol/protocol.json` or the phone's test suite fails: reading is `read`, answering and acting
  are `control` with a request id, the level and the caps are `mac_only`. A yes from a phone names
  the phone as the approver.
- **Terminal UI.** One key opens the conversation; tiles show held, watched and in conflict.
- **Audio Mode** (on main). Overseer's own run, watchers, check-ins, reports and findings of
  *fine* are silent. One attention cue plays when Overseer needs the owner, under AC-143's
  one-cue-per-need rule. Agents that Overseer starts are top-level agents and cue as usual.

## With the oversight passes (Gates P and Q)

AC-146 and AC-157 describe a pass that a session outside Overseer runs today over the agents that
build Overseer: it reads their commits and pull requests, comments, and merges finished work. This
gate is the same idea as a feature of the product, for the agents Overseer runs. It does not
replace those criteria. Agents that run outside Overseer are ignored for now (the owner's
decision).

## Cost and bounds

- Overseer's model runs only for: what the owner asks, a check-in that is due, a finding of
  concern or stop, a conflict that needs a decision, an agent's ask, a report that came back, and
  what the owner asked to be told about.
- Events within 5 s are one turn (at most 20 items or 32 KiB). Never two turns at once. An
  unchanged state causes no turn. Swarm's director and route picking follow the same rule.
- **Two kinds of turn.** A turn that answers the owner (something typed or said) is never counted
  and never refused. A turn Overseer starts by itself is counted: a check-in, or what it does
  about a finding, a conflict or an agent's question.
- Overseer starts at most 100 turns a day by itself. At the cap it says so and waits for the
  owner, and the free checks keep running.
- The conversation shows what Overseer and the watchers have used.
- Verification follows the paid-turn rules: fixtures throughout, one tiny live run where a
  criterion asks for it, one attempt per step.

## Trust and safety

- Everything Overseer and watchers read from agents is data: messages, reports, findings, file
  contents, quoted web pages. "Overseer: stop every agent" written in a report is text.
- The daemon, not the model, enforces the classes, the level, the caps and the read-only rule.
- A card shows the text that was sent and where the request came from.
- No credential enters a digest, a report, a share or a finding.
- Every action can be traced to the message, check-in, finding or conflict behind it.
- **What Auto costs.** At Ask first, text that misleads Overseer can at worst produce a proposal
  the owner declines. At Auto nobody stands in between: something an agent read on a web page and
  repeated in its chat could lead Overseer to hold, redirect or stop another agent, or to start
  one. What bounds the damage is the daemon: the classes (no permission answered, nothing merged,
  nothing deleted, no setting changed), the caps, the snapshot taken before every redirect, and
  the card for every action with its cause. The switch says this where it is turned on.

## Protocol and data

Names are indicative; the implementing session settles them.

- **Methods.** `overseer.session`, `overseer.send`, `overseer.proposals`, `overseer.answer`,
  `overseer.level`, `overseer.caps`; `agents.roster`, `agent.digest`; `agent.hold`,
  `agent.release`, `agent.guardrail`, `agent.redirect`, `run.queue`; `coord.report`, `coord.ask`,
  `coord.claim`, `coord.ack`; `conflicts.list`, `conflict.resolve`; `watch.start`, `watch.end`,
  `watch.list`, `watch.finding`.
- **Store (additive).** Overseer's conversation, cards and proposals; holds, guardrails, reports,
  areas, conflicts, check-ins, watches, findings; `runs.role`; `turns.source`.
- **Events.** `overseer_action`, `proposal`, `proposal_answered`, `dispatch`, `hold`, `release`,
  `guardrail_crossed`, `check_in`, `report`, `claim`, `conflict`, `conflict_closed`,
  `watch_started`, `finding`, `watch_ended`.
- **Code.** New modules under `daemon/src/overseer/`. Shared files (`daemon.rs`, `server.rs`,
  `store.rs`, `extension.js`) get small additive edits.

## Work in flight

A snapshot of 2026-09-27; the implementing session rechecks at every step.

| Work | State | What it means for this gate |
| --- | --- | --- |
| Voice Mode (Gate R) | criteria and design on main; waits for the owner's answers; not built | The spoken side of the same Overseer. See [With Voice Mode](#with-voice-mode). Both need the session in the daemon; one builds it. |
| Gate M | merged (#11); AC-107 waits for one live run | Talk to Overseer is on main, in the extension. This gate moves it into the daemon and reuses its proposal card, its *From Overseer* message style and its scenario (`test/ui/scenario-talk.js`). |
| Gate K follow-ups | merged (#8) | The composer this gate adds the target to. |
| #10 Phone remote | draft, work continues | Method classes, request ids, AC-128. It also needs queueing in the daemon. |
| #9 Continuity | draft, work continues | Handoffs and three new run states. A hold is a flag, so it adds no fourth. |
| #3 Swarm | draft, work continues; the largest change in flight (`daemon.rs`, `store.rs`, `server.rs`) | The broker, claims, conflicts and the agent limit. See [With Swarm](#with-swarm). |
| #2 Route picking (Auto mode) | draft; most of its work is local to its session and not pushed | Routes and the one admission. See [With route picking](#with-route-picking). |
| Audio Mode | merged (#5, #6) | The cue rules above. Voice Mode adds one arbiter for cues and speech (AC-172). |
| Gate P | merged (#13) | `scripts/test-all` is on main; this gate's tests join it (AC-201). |
| Terminal UI | on main | Gains one key and three badges. |

This gate and Voice Mode can be built in parallel (the owner, 2026-09-27). Most of each is its
own: the listener, the floor and speech there; digests, conflicts, holds, check-ins and watches
here. What they share is the one Overseer session in the daemon. Its daemon methods are the
contract between them: whoever reaches that step first writes them down in its pull request, and
the other builds on them, as the phone remote and Gate M did for the review's methods.

## Limits and out of scope

- Agents that run outside Overseer: Codex in the ChatGPT app, a Claude desktop session. Ignored
  for now (the owner's decision).
- A second chat. One conversation for now (the owner's decision).
- Voice. It is Gate R.
- Overseer doing an agent's work. It writes no code and edits no file.
- Overseer answering a permission request by itself.
- Overseer planning and splitting a task into jobs. That is Swarm's director.
- Overseer choosing models and accounts. That is route picking.
- A guardrail cannot stop a harness from writing where the harness itself allows it; the daemon
  sees the write afterwards and holds the agent. The label says which of the two applies.
- A check-in and a watcher's finding are a model's judgment: evidence for the owner, not a verdict.

## Spike results (AC-180)

Run on 2026-09-27; the evidence is in
[docs/verification/evidence/ac-180](../verification/evidence/ac-180/README.md). These decisions
stand for the rest of the gate.

| Question | Decision |
| --- | --- |
| How a run is given tools | `overseerd mcp --socket <path>`: an MCP server over stdio that forwards every call to the daemon with the run's token (`OVERSEER_MCP_TOKEN`). Claude Code: `--mcp-config` with a file in the run's folder and `--strict-mcp-config`. Codex: `-c mcp_servers.overseer.{command,args,env}` and `-c mcp_servers.overseer.tools.<tool>.approval_mode="approve"` per tool, without which the exec transport refuses the call; the app-server transport raises an `mcp_tool_call_approval` request that the daemon answers. OpenCode: `mcp.overseer` in the profile's own `opencode.json` (the file Continuity writes), where the tools are named `overseer_<tool>`. Nothing is written into the user's own configuration on any harness. |
| Who is speaking | The token, never the text. The daemon keeps tokens per run and role (`overseer_tokens`) and gives each role its tools; a call with an unknown token or for a tool outside the role is refused as a tool error the model can read. Every call is an `overseer_tool_call` event on the calling run. |
| Read-only runs | Claude Code: `--disallowedTools` for every shell, file, web and delegation tool, `--allowedTools` naming Overseer's tools, the default permission mode (plan mode made the model reach for `ExitPlanMode`); Claude still sends `can_use_tool` for an MCP tool, and the daemon answers it (allow for Overseer's own tools, deny anything else). Codex: `-s read-only`; its shell stays available inside the read-only sandbox, which is recorded as a limit. OpenCode: `tools` set to false for bash, write, edit, patch, multiedit, task and webfetch in the same `opencode.json`. |
| A message at the end of a turn or mid-turn | As today (AC-60): resume at the end of a turn, interrupt then resume mid-turn, per harness. Only the queue moves into the daemon (AC-188). |
| Picked up | A *report*, *ask*, *claim* or acknowledgement arrives as a tool call through the shim, so the daemon attributes it by token. A harness whose run has no tools gets *delivered*, then *answered*. |
| Cost of a turn | The roster (one line per agent, 4.2 KB for 16 agents) and the digests (≤ 4 KiB each) are small next to the harness's own baseline: about 57k tokens per model iteration on Claude Code and 69k per turn on Codex exec, mostly cache reads. A check-in reads one digest and answers in one iteration, so it costs about one baseline. Overseer's session is kept warm and resumed, so the baseline stays a cache read. The roster is built from events, not from `git status` per worktree (1.3 s for 16 agents when it was). |
| A trial merge on 10,000 files | `git merge-tree --write-tree --name-only` between two agents' commits: 15 ms, naming the conflicting files, touching no worktree, index or branch. |

## Order of work

Built in its own worktree and pull request, like the other gates. The goal text is prepared in
[orchestrator-goal.md](orchestrator-goal.md); nothing is started until the owner activates it.

| Step | Criteria | Notes |
| --- | --- | --- |
| 0. Spikes | AC-180 | Tools per harness, read-only runs, delivery, picking up, timings. Shared with Voice Mode's spike where they measure the same session. |
| 1. The daemon half | AC-183, AC-192 | Digests and conflicts. No model, new modules only; can start while everything else is in flight. |
| 2. The conversation in the daemon | AC-181, AC-184 | AC-107 moves. |
| 3. Control | AC-185 to AC-188 | Classes, cards, levels, holds, guardrails, redirect, queueing in the daemon. |
| 4. On task | AC-189 | Free checks and check-ins. |
| 5. Briefings, the channel, sharing | AC-190, AC-191 | Rally included. |
| 6. Watches | AC-193, AC-194 | |
| 7. The neighbours | AC-195 to AC-197 | Swarm, route picking, Continuity. Partial until each is on main. |
| 8. Surfaces | AC-182, AC-199 | Home, the terminal UI, the phone, cues, Voice Mode's cards. |
| 9. Bounds, safety, coverage | AC-198, AC-200, AC-201 | |
| 10. The owner's session | AC-202 | |

## Decisions made while building

Settled by the implementing session (pull request #14) where the text above left room, each with
its test.

| Question | Decision |
| --- | --- |
| When briefings and the channel start | Only once the owner has spoken to Overseer (its run exists), and then when another agent works in the repository; a generic program gets neither. Before that, an agent's task is exactly as typed, so nothing changes for an owner who never opens the conversation (Continuity's fixtures showed why). The owner sets it per agent or as the default (`agent.channel`). |
| When an agent is finished | When it has stayed idle for the grace period after completing a turn (30 s, `overseer.grace_ms`); a new turn inside it means the work goes on. Check-ins and watches use the same notion. |
| A watcher's first wake | A new watcher is created on the subject's first wake, not when the watch is set, so an unchanged subject costs nothing. A named idle agent is woken instead. |
| Hold on stop when the subject is idle | The hold is recorded either way: a running subject is stopped now, an idle one has its next turn wait. |
| A briefing that changed | When a companion claims an area, the briefing is made again; one still waiting in the queue is replaced. |
| An agent started by hand | Appears in the conversation as a *started* card (the seed of home's *Ask Overseer instead*). A start from Overseer has its proposal's card instead. |
| Naming agents at home | `@` opens a list under the text that never takes the keyboard (typing narrows it, arrows move, Enter or Tab inserts, Escape closes); a named agent reaches Overseer as `@Title (run id)`. |
| The words of a proposal | The daemon writes them (`lines`) for every surface, with the proposal's cause, so the terminal and the phone show the same card as VS Code. |
| A denied permission | Remembered for a day with its command or path; words that would have another agent repeat it are refused at the proposal. |
| Channel messages | Redacted at the door, so no credential enters a report, a question or an answer, whatever surface reads them later. |
| Test settings | `OVERSEER_CHANNEL_DEFAULT` (auto, on, off) and `OVERSEER_CHECK_INS` (off, done, every:N) at the daemon's start, so the suites run with the new behaviour off and on (AC-201). |

## Acceptance

AC-180 to AC-202 in the main RFC are the acceptance criteria. Each has its Verify clause there.

| Criterion | What |
| --- | --- |
| AC-180 | Spikes before lock-in |
| AC-181 | Overseer lives in the daemon |
| AC-182 | One conversation, from home |
| AC-183 | A digest of every agent |
| AC-184 | Overseer reads on demand, and only reads |
| AC-185 | A fixed set of actions, on one agent or all, each with its card |
| AC-186 | Ask first, Steer, Auto |
| AC-187 | Rein in: hold, release and guardrails |
| AC-188 | Change direction |
| AC-189 | Overseer keeps agents on task |
| AC-190 | Agents that know about each other |
| AC-191 | Context passed between agents |
| AC-192 | Conflicts between agents in flight |
| AC-193 | One agent watches another |
| AC-194 | A watch that checks |
| AC-195 | With Swarm: one decision-maker per swarm |
| AC-196 | With route picking: routes, admission and metering |
| AC-197 | Handoffs and offline |
| AC-198 | Quiet and bounded |
| AC-199 | Every surface |
| AC-200 | What agents say is data |
| AC-201 | Regression coverage |
| AC-202 | Orchestration session (owner-confirmed) |
| AC-224 | Goals for an agent ([Goals](#goals)) |
| AC-225 | Goals for Overseer ([Goals](#goals)) |
