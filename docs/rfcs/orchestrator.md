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
| What Auto means | The Auto an agent has when it is left to work on its own (the permission mode). Overseer has that kind of Auto too. It is Overseer's own switch. |
| Choosing who does the work | A different layer from Overseer's Auto, and it should perhaps have another name. It is the feature of pull request #2, called Auto mode there. This document calls it *route picking*. The owner's remark is passed on in a comment on that pull request; the name is theirs to settle. |
| Who acts on what a watcher finds | Overseer. It acts for the watcher and tells the agent what to do. |
| Agents outside Overseer | Ignored for now. Only agents Overseer runs are in this gate. |
| Voice | Voice Mode is separate work (Gate R). This gate does not build it and stays compatible with it. |

## Proposed defaults, distinguished from the decisions above

These are this RFC's choices, not the owner's. They stand until the owner changes them; a change
is a recorded revision.

- **Briefings and the channel.** An agent is told about the others, and can message Overseer, only
  when more than one agent works in a repository
  ([Agents that know about each other](#agents-that-know-about-each-other)). The owner is not
  sure yet (2026-09-27). The other choice is every agent, always: simpler to explain, and a lone
  agent then carries a paragraph and three commands it has no use for.
- **Permission requests.** Overseer never answers one by itself, at any level. The owner can answer
  one from the conversation. The owner is not sure yet (2026-09-27) and wondered whether Auto
  should answer them. The reason for never: an agent that asks is an agent the owner told to ask.
  To let an agent work without asking, the owner runs that agent on Auto, and then it sends no
  request for Overseer to answer. If Overseer answered for it, it would loosen the agent's
  permission mode behind the owner's back, which AC-16 and AC-138 rule out. Changing this is a
  recorded revision of AC-185 and of AC-16.
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

**When.** Proposed: only when more than one agent works in a repository. A lone agent gets no
briefing and no channel and works exactly as today. It can be set per agent and as a default. An
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
| **Not from the conversation** | Accounts and sign-in; phone access and devices; settings; cleanup; review actions; loosening a permission mode; stopping the daemon; Overseer's own level and caps | Overseer opens the place in the UI and says so |

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

## Order of work

Built in its own worktree and pull request, like the other gates.

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
