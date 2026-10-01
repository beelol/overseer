# `@overseer/phone-model`

The view models of Overseer's phone app: the code that turns the daemon's state and events into
exactly what the phone's screens show. Pure TypeScript. No React, no React Native, no DOM, no
timers, no network, no runtime dependency. Every function takes data and returns data.

The phone must show the same content as VS Code. That is not assumed here, it is measured: the
tests load the extension's real files and compare, row by row.

`API.md` lists every export with its signature. Those names and shapes are committed.

## Run it

```sh
cd phone/model
npm install
npm test          # tsc --noEmit (sources, then tests), then vitest
npm run record    # records the fixtures again from a real overseerd (see "The recordings")
```

`npm test` needs no simulator, no daemon and no network. It reads `extension/` and
`phone/protocol/protocol.generated.ts` from this repository.

Last run (Apple M5 Max, Node 24.20, the machine busy with other work, load average 12):

```
Test Files  14 passed (14)
     Tests  149 passed (149)
  Duration  27.70s
```

## What is in it

| File | What it does |
| --- | --- |
| `src/types.ts` | The daemon's records, re-exported from `phone/protocol/protocol.generated.ts`. Nothing is declared twice. |
| `src/store.ts` | The phone's copy of the daemon's state, kept current from events: `load`, `apply`. |
| `src/agents.ts` | The agents list: VS Code's side bar as flat rows with a depth. The head of a conversation. |
| `src/conversation.ts` | One agent's conversation as rows, built one event at a time. |
| `src/describe.ts` | What a tool call reads as: icon, verb, target ("Ran npm test"). |
| `src/markdown.ts` | The agent's replies as a small tree, cleaned the way VS Code cleans them. |
| `src/marked-lexer.ts` | How VS Code reads Markdown: the lexer of marked 14.0.0, the version the extension ships. |
| `src/review.ts` | Changed files, the comparison to choose, a file's hunks as rows. |
| `src/pending.ts` | Messages on their way: queued, sending, sent, as one bubble. |
| `src/text.ts` | Every sentence the phone shows that VS Code also shows, copied from the extension. |
| `src/persistent.ts` | A map and a list that share what they did not change. |
| `scripts/record.mjs` | Records the fixtures from a real `overseerd` with fixture harnesses. |
| `test/fixtures/` | 18 recordings. |
| `test/helpers/vscode.ts` | VS Code's chat and Markdown: the extension's real files in a page, and how the page is read. |
| `test/helpers/sidebar.ts` | VS Code's side bar: the extension's real `views.js`, and how its tree is read. |
| `test/helpers/source.ts` | Takes a function out of a file of the extension as it stands, for the parts that cannot be loaded whole. |
| `test/helpers/describe.ts` | The phone's rows as the same plain description the page is read into. |
| `test/helpers/random.ts` | Streams of events made from a seed. |
| `licenses/marked-LICENSE.md` | The licence of marked (MIT). |

## The recordings

`scripts/record.mjs` starts a real `overseerd` (`target/debug/overseerd serve`; it builds it with
`cargo build -p overseerd` when it is missing) for each scenario, talks to it over its Unix socket
(a line of JSON for each message), and saves what it says.

Each daemon gets its own `OVERSEER_HOME` in a temporary directory, `OVERSEER_GATEWAY_MDNS=off`,
its own Git repositories, a notification command that does nothing, and fixture harnesses only
(`fixtures/fake-harness`). Every `OVERSEER_*` variable of the caller is removed first. The script
refuses to record when the daemon that answers is not the one it started. At the end it stops the
runs, stops the daemon and removes the directory. It cannot reach the owner's daemon, a real
harness or a paid account.

A recording holds:

| Part | What it is |
| --- | --- |
| `initial` | `state` before the first agent |
| `events` | every event of the stream, in order |
| `checkpoints` | `state` after every 10 events (so also after every 50), and at the moments a scenario marks ("permission pending") |
| `final` | `state` at the end |
| `calls` | what the daemon answered to requests a scenario marks (`search`, `comparison.options`, `workspace.diff`, `workspace.hunks`, `review.*`) |
| `marks` | names for the runs of the scenario |

Paths are replaced by fixed ones (`/fixture` for the temporary directory, `/overseer` for this
repository, `/Users/fixture` for the home folder), so a recording does not say where it was made.
Nothing else is changed.

| Recording | What happens in it | Events |
| --- | --- | --- |
| `showcase` | reads, a search, an edit, a new file, a test run, a Markdown summary | 32 |
| `showcase-permission` | the same, then a permission request, allowed | 40 |
| `nested` | a child and its child; the grandchild is heard of before its parent | 23 |
| `permission-allow`, `permission-deny` | a request, allowed; a request, denied with a message | 19, 19 |
| `echo-follow-up` | a turn, then a follow-up turn | 19 |
| `slow-interrupted` | a turn stopped while it works | 13 |
| `stop-live` | a turn stopped, ending the way Claude Code 2.1 ends it | 13 |
| `ratelimit`, `auth`, `quota`, `failed-reason` | the four ways a turn fails | 12, 12, 11, 12 |
| `unparsed` | a line the parser does not understand | 12 |
| `generic-300` | a program that prints 300 lines, and one to stderr | 307 |
| `codex-subagent` | a Codex transcript with a sub-agent (the replay harness) | 21 |
| `codex-app-tree` | Codex app-server: a child with a child, a command approval | 27 |
| `review` | an agent that changed files; comparisons, the diff, hunks, marks made and taken away, hunks rejected | 37 |
| `nine-agents` | nine agents in two repositories: a nested child and grandchild, one archived, one waiting for an answer, one working, three failed | 126 |

## How parity is measured

### The store, against the daemon's `state`

For every recording: `load` a state, `apply` the events one by one, and at every moment the
recording has the daemon's state for, compare the phone's tasks, runs, workspaces, profiles and
turns with it, field by field. This is done from the first state and again from every later one.

Result: 18 recordings, 111 states compared from the first state, 0 differences.

Five times are read from the daemon's clock a moment before or after it writes the event, and are
not in the event: a task's `archived_ms`, a workspace's `removed_ms`, a turn's `ended_ms`, a
child's `ended_ms`, a mark's `at_ms`. The phone uses the event's time. The test lets these differ
by up to 50 ms, counts them, and lets nothing else differ. In the recordings as they stand they
differ 0 times; in an earlier recording of `nine-agents` the archive time differed by 1 ms.

### The conversation, against VS Code's chat

`test/helpers/vscode.ts` makes a page with jsdom and runs the extension's real files in it, in the
order the webview loads them. The list is read from `extension/src/webview-html.js` itself
(`SHARED_JS`, then `CHAT_JS` up to `conversation.js`: today `ui.js`, `logos.js`, marked,
DOMPurify, highlight.js, `markdown.js`, `plain-words.js`, `continuity-text.js`, `continuity.js`,
`conversation.js`), so a script the chat's page gains is loaded here too; a list kept by hand here
once missed `plain-words.js` and Continuity's scripts while the phone drifted. What follows
`conversation.js` is the chat page's own shell (the composer, `chat.js`). Nothing of the extension
is copied or rewritten. The same events go to
`window.OverseerConversation` and to the phone's model. Then both are turned into the same plain
description, a line for each row, and compared with deep equality.

How the page is read:

| What | How |
| --- | --- |
| A row's text | the text of its elements (`textContent`); for short labels, white space squashed |
| An icon | the codicon's name, from its class (`codicon-check` is `check`) |
| Hidden things | an element with the `hidden` attribute is not read (a footer before its turn ends, the working line of a finished run) |
| Tooltips and labels | the `title` and `aria-label` attributes: the full path, the full command, the name of a status, "You", "Agent", "Sub-agent" |
| A turn | every row carries the number VS Code keeps on its turn (`data-turn`) |
| A fold of steps | its line ("6 steps", the verbs, "1 failed", the tooltip), then its tool calls one deeper, then its edit chips at the fold's own depth |
| A tool call | icon, verb, target, whether the target is code, the tooltip, the result (running, ok, "failed", "exit 2", "+8 −0"); then the row is opened as a click opens it and its two sections are read (label and text of the input, label and text of the result, or the note) |
| A permission card | its state (pending, allowed, denied, asked), icon, sentence, tooltip, the preview, the buttons with what they answer, the request as text |
| An error | icon, title, message, class, the "Sign in again" button and its label |
| A quiet line | its words, a link after them (Continuity's "Open it"), status, icon, tooltip |
| A child | icon, title, the usage it reported, status, tooltip, the run; its rows one deeper |
| A footer | state, icon, sentence, tooltip, duration, tokens and cost, the detail in the tooltip |
| A reply | its Markdown as rendered (see "Markdown") |
| Above and below | the banner when shown, the working line when shown |
| Depth | 0 for what a turn holds; one more inside a fold, under a tool call, inside a child |

A page that holds something the reader does not know fails the test, so nothing is skipped.

Three ways of feeding, for every agent of every recording:

1. **A panel opened at a moment**, for every moment a recording has the daemon's state for: the
   run as that state has it, then the history up to that moment.
2. **A panel that stays open**, the state read again after every event, compared after every event.
3. **A panel that stays open**, the state read again only at the start and at the end.

VS Code is given what its feed passes on (`extension/src/run-feed.js`: the events of the run and
of its descendants). The phone is given every event and picks its own.

| Recording | Agents | Conversations compared | Rows compared | Differences |
| --- | --- | --- | --- | --- |
| auth | 1 | 15 | 38 | 0 |
| codex-app-tree | 1 | 32 | 235 | 0 |
| codex-subagent | 1 | 25 | 167 | 0 |
| echo-follow-up | 1 | 23 | 79 | 0 |
| failed-reason | 1 | 15 | 38 | 0 |
| generic-300 | 1 | 339 | 55,685 | 0 |
| nested | 1 | 27 | 132 | 0 |
| nine-agents | 9 | 574 | 3,460 | 0 |
| permission-allow | 1 | 23 | 82 | 0 |
| permission-deny | 1 | 23 | 82 | 0 |
| quota | 1 | 14 | 28 | 0 |
| ratelimit | 1 | 15 | 38 | 0 |
| review | 1 | 42 | 363 | 0 |
| showcase | 1 | 37 | 268 | 0 |
| showcase-permission | 1 | 47 | 435 | 0 |
| slow-interrupted | 1 | 17 | 35 | 0 |
| stop-live | 1 | 16 | 33 | 0 |
| unparsed | 1 | 15 | 29 | 0 |

**Where the recordings do not reach.** A fixture harness never sends a result before its call, a
child before the call that started it, two children at once, a request asked twice. So
`test/conversation-random.test.ts` makes streams from a seed (`test/helpers/random.ts`): every
kind of event the chat reads, in any order, with the state read again at random moments, events
of other agents, and events repeated. They are shaped like the daemon's events; they are not
recordings. Both sides are compared after every step.

The streams carry Continuity's events too (a lost connection again and again, handoffs, stalls,
the memory valve, local models, its system lines) and Overseer's oversight (holds, guardrails,
check-ins, watches, briefings, queued messages, the kinds it keeps quiet), but not Continuity's
waiting states or `back_online`, Overseer's proposals and message cards, or Auto's decisions:
VS Code draws a card for those that the phone does not have yet, and a test checks the phone draws
nothing for them (see "Known differences").

Result: 24 streams, 2,916 conversations compared, 97,021 rows compared, 0 differences. What the
streams end with: folds, tool calls in a fold or a child, children under a tool call or a child,
edit chips under a fold, permission cards in all four states, footers in all four.

This test found one real change while it was written: `conversation.js` had just been given a
line for `remote_command` ("Message from Bilal's iPhone") and `push` had been made quiet. The
phone's model follows both.

### The agents list, against VS Code's side bar

`views.js` asks for the `vscode` module, which exists only inside VS Code. It is given a stand-in
with the few things the file uses (tree items, icons, colours, addresses, an event emitter, a
settings reader with every setting at its default): plain objects that keep what they are given,
so none of the list's logic is in the stand-in. The state reaches the list through the model's own
`refresh`, as in VS Code. What `extension.js` hands the list is taken from `extension.js`: the
list of agents that need the owner (`attention`, within `activate`) is its source as it stands,
run with the same state and the same `media/rollup.js`, and the reviewed marks are given as
`extension.js` gives them. A name `attention` uses that is not given fails the test, so the next
thing `extension.js` passes the list is noticed. The tree is walked the way VS Code walks it
(`getChildren`, then the children of every open row) and each row is read from its tree item: id,
label, description, tooltip, the label for screen readers, context value, logo or icon, and from
its decoration the badge, the word for the status and whether the row takes the badge's colour.
The clock is set, so "7m" and "to review" are the same on both sides.

| What is listed | Rows | Differences |
| --- | --- | --- |
| Nine agents as the list opens (the rollup, Needs you, two repositories with what is to review, a child with a child) | 15 | 0 |
| Some agents reviewed, two pinned, two rows closed | 10 | 0 |
| Needs you closed | 14 | 0 |
| Archived agents | 2 | 0 |
| 11 searches, each in the list and in the archive, with what the daemon's `search` found | 43 | 0 |
| The daemon cannot be reached | 1 | 0 |
| Every moment of every recording, the list and the archive (186 lists) | 402 | 0 |
| Every status (VS Code's, Continuity's, one never heard of), just ended, reviewed, and ended over a week ago | 39 | 0 |

The agents that need the owner, their order and their reasons are compared too (3 cases, 0
differences). For every search the phone's own search finds nothing the daemon's does not.

### Markdown, against VS Code's renderer

The same page as the chat. `window.OverseerMarkdown.render` renders; the page is read into lines,
a block on each and what it holds indented under it: `p`, `h2`, `ul`, `ol start=3`, `li`, `li [x]`,
`text` (a list item's text that is not a paragraph), `quote`, `rule`, `table align=[…]` with its
`head` and `row`s, and `code` with its label, the lines VS Code counts, whether it is cut, the
words of its button and its text. Inside a line: text, `<b>`, `<i>`, `<s>`, `<code>`, `<br>`,
`<a href tooltip>`, and `<long full>` for a shortened token. White space is squashed, as a
browser shows it. Text that stands at the top of a reply in no paragraph (what raw HTML leaves)
reads as a paragraph.

| Inputs | How many | Compared | Different |
| --- | --- | --- | --- |
| Every reply of the recordings (the showcase reply is what `test/ui/scenario-chat.js` checks) | 23 | structure, 79 lines | 0 |
| Written inputs, a construct each (`test/helpers/markdown-corpus.ts`) | 105 | structure, 352 lines | 0 |
| Inputs made from a seed, pieces of Markdown in any order | 2,500 | structure, 7,991 lines | 0 |
| Unsafe inputs: raw HTML, scripts, `javascript:` and `data:` addresses, event handlers | 45 | the words shown | 0 |

For the unsafe inputs the test also checks that no link keeps an address that runs code, and that
a link opens only when VS Code would open it. `safeHref` is compared with DOMPurify itself on 26
addresses, and the 307 entities the phone knows with what the page's parser reads them as.

### Review

VS Code's review cannot be loaded whole outside VS Code (it needs Monaco and the Git extension).
The functions compared are taken from its files as they stand and run as they are: the
navigator's tree (`renderTree` and what it calls in Changes only) and `hunkHash` of
`extension/branch-diff/review/browser.js` in a page; `STATUS`, `statusLetter` and the items of the
comparison picker of `extension/src/review.js`. VS Code's navigator can also browse every file of
the worktree (All files, AC-99); the phone's review is the changed files (AC-126), so the tree is
compared in Changes only.

| What | Compared | Differences |
| --- | --- | --- |
| The changed files of the recording, as VS Code's tree lists them | 2 lists, 17 rows | 0 |
| Changed files made for the test: folders in folders, a search, folders closed, conflicts | 7 lists, 63 rows | 0 |
| The letter of every status | 9 | 0 |
| The comparison choices, with the items of the real picker | 6 | 0 |
| The keys of hunks, with the daemon's and with `hunkHash` | 10 | 0 |
| Reviewed marks kept current from events, with what `review.marks` answered | 10 | 0 |
| The counts of added and removed lines, with the daemon's `workspace.changes` | 6 files, +16 −2 | 0 |

### Words and tool calls

| What | Compared | Differences |
| --- | --- | --- |
| Sentences copied into `src/text.ts`, each looked for in the file it was copied from | 188 sentences, 10 files | 0 |
| Tool calls: `describe` with the real one, 34 names × 36 inputs × 9 summaries | 11,016 | 0 |
| The host of an address, with what `URL` gives | 34 | 0 |
| `ago`, `duration`, `compact`, `basename`, `firstLine`, `shortPath`, `statusText` with the extension's | every case in `test/text.test.ts` | 0 |

### Incremental equals batch

For every recording and every agent in it: one event at a time, 12 cuts into pieces at random
places, and all at once give the same rows. The same for 40 streams made from a seed with the
state read again in between. After every change the test checks what `append` said: every row
not named in `changed` is the same object as before, and before `movedFrom` every row sits where
it sat. A conversation that was given to `append` is as it was afterwards.

### Messages on their way

Queued, sending, the daemon's event, the turn, the answer before or after it: one bubble with one
key at each of 7 steps. A turn known only by its words is not shown twice.

## Speed

Measured by `test/speed.test.ts`, printed on every run. The limits are the task's. The numbers
are from a machine that was busy with other work.

| What | Measured | Limit |
| --- | --- | --- |
| One more event on a conversation of 5,020 rows in many turns | 6.5 µs (median of 5 rounds of 1,000 events; slowest round 6.7 µs) | 1 ms |
| One more event on a conversation of 5,489 rows in one turn | 4.8 µs (slowest round 4.9 µs) | 1 ms |
| Rows an event touches | 1.19 on average, at most 3 | bounded |
| A minute of 20 events a second on 5,020 rows, the rows read as an array after every event | 30.1 ms of work in all | |
| The rows as an array and the visible rows, once a frame | 62 µs (median) | |
| 1,000 events on a store of 1,000 agents | 0.51 ms (median of 7 rounds; slowest 1.70 ms) | 16 ms |
| What one status event copies in a store of 1,000 agents | one bucket of 6 runs and a list of 256 slots; 255 of 256 buckets shared | no large array |
| What one event copies in a conversation of 5,020 rows | one piece of 64 rows and a list of 79 pieces; 78 of 79 pieces shared | no large array |
| The agents list of 1,000 agents after a change | 2.6 ms | |

## Decisions

1. **The daemon's records are not declared here.** `src/types.ts` re-exports them from
   `phone/protocol/protocol.generated.ts`. What that description lacks is narrowed from `unknown`
   by a function, never assumed.
2. **State and conversations are never changed in place.** An update returns a new value that
   shares what it did not change. Two small structures do the sharing: a map in 256 buckets and a
   list in pieces of 64 (`src/persistent.ts`). Nothing large is copied for an event.
3. **The store's tables keep the daemon's order.** Tasks and workspaces by creation time; runs and
   profiles by creation time, then id. A new record is put where the daemon's `ORDER BY` puts it.
4. **An event at or before the cursor changes nothing.** A replay that overlaps is harmless.
5. **A conversation is a flat list of rows with a depth**, ready for a list that builds only what
   is visible. A fold of steps and a child are rows; what they hold follows, one deeper.
6. **Every row has a key that never changes.** A row that did not change is the same object.
7. **`append` says what changed**: the places of the rows that are new or different, and the first
   place from which rows moved. Rows move when a child is put under the tool call that started
   it, when a second tool call folds the first, when a child that works beside another speaks.
8. **`setRun` is VS Code's `setRun`.** The chat in VS Code learns the run's status and the request
   it waits on from the state, not from events. The phone does the same, from its store.
9. **The conversation picks its events.** Events of other agents and events of no agent change
   nothing, as VS Code's feed does not pass them on.
10. **A tool call's result is decided when VS Code decides it**: when the call or its result
    arrives, and for the agent's own calls at the end of the turn. So a call without a result
    shows as running until its turn ends, on both.
11. **Markdown is read by marked's own rules.** A parser written by hand agreed with VS Code on
    every reply and every written input, and disagreed on 131 of 3,000 inputs made from a seed
    (a fence on the last line, an address in capital letters, e-mail addresses after a mark,
    loose and tight lists, lists inside quotes). What a line of Markdown means is decided by
    marked's rules and the order it tries them in, so `src/marked-lexer.ts` is marked's lexer
    (14.0.0, MIT, the version in `extension/media/vendor`), in the extension's configuration.
    Only the lexer: the phone has no page, so `src/markdown.ts` builds a tree instead of HTML.
12. **Raw HTML never becomes a view.** A tag is dropped and its text kept; what a script, a style
    or a similar element holds is dropped with it. `<br>` is a line break.
13. **A link keeps its address only when DOMPurify would keep it, and opens only for `http` and
    `https`**, which is what the extension's host opens.
14. **Markdown is parsed when asked for**, once for each row (`markdownOf`), not when the event
    arrives.
15. **Words are copied, with their file.** `src/text.ts` names the file each sentence came from;
    a test looks for it there.
16. **Numbers are grouped with commas** ("18,423"). VS Code uses the editor's language; its chat
    is in English.
17. **"2m", not "2 min ago".** The side bar says "2m" (`views.js`), so the phone says "2m".
    `agoInWords` gives "2m ago", the form `extension/src/phone-text.js` uses.
18. **The agents list keeps VS Code's ids and context values** (`agent:<task>`, `run:<run>`,
    `agent-done-archived`), so what is open or closed and what a row can do mean the same.
19. **Needs you is what waits for an answer, counted as `media/rollup.js` counts it** (AC-246).
    An agent at its end is "to review" (AC-254) until it is opened after it ended: `seen`, when
    the owner last opened each agent on the phone, is the phone's reviewed marks.
20. **The filters All, Active and Needs you are the terminal UI's** (`tui/src/app.rs`). VS Code's
    side bar has none.
21. **A hunk is its removed lines, then its added lines.** The daemon sends hunks without context.
    Each line has its number on its own side.
22. **A message on its way and the turn it becomes share one key**, `sent:<request id>`. The
    daemon names the request in the `remote_command` event just before the turn starts.
23. **Recordings are scrubbed of paths and of nothing else.**
24. **A checkpoint every 10 events**, which includes every 50, so short recordings are checked
    more than once.

## Known differences from VS Code

Every place where the phone's model does not do what VS Code does, or does more.

| Where | VS Code | The phone's model | Why |
| --- | --- | --- | --- |
| Permission card | does not say who answered | `by` holds who answered ("the Mac", "phone:…") | The task asks for it; the event carries it. It is not compared, because VS Code shows nothing. |
| Raw HTML in a reply | keeps the tags its sanitizer allows: a raw `<strong>` is bold, a raw `<a href>` is a link, a raw `<h1>` a heading, a raw list a list | shows their text | Never raw HTML on the phone. The words are the same (45 inputs compared). |
| A link's title | overwrites it with the address, as the tooltip | `href` is the address; `title` keeps the title written in the Markdown | Nothing is lost; the app can ignore `title`. The tooltip compared is the address. |
| Code blocks | coloured by highlight.js | language and text; no colours | Colouring belongs to the app's views. |
| A code block's text | ends with a line end marked adds | without it; `lines` is the number VS Code counts ("Show all 31 lines") | A native text view would show an empty last line. |
| A task item | a disabled checkbox in the item | `checked` on the item | The app draws its own. |
| Named entities | a browser knows about 2,200 | 307: the ones of HTML 4 and some more | Size. An entity the phone does not know is shown as written (`&foo;`). |
| Raw HTML that is not closed, or opens in one block and closes in another | the browser's parser decides | what follows an open `<script>` is dropped until its end; blocks that begin inside it are text | The words agree in the inputs compared; the structure is not promised. |
| Malformed tool input (`edits: [null]`) | `describe` throws; the chat drops the event | describes what it can | An event must not stop the phone's chat. |
| A tool call's target when there is none | `undefined` | `''` | Shown the same. |
| Grid tiles (`compact`) | fewer details | not there | The phone has no grid. |
| Numbers | the editor's language | commas | See decision 16. |
| How long ago, in a sentence | a date after a week ("on Sep 3") | keeps counting days ("30d ago") | A date needs the phone's language; the app can format one. |
| Agents list: filters | none | All, Active, Needs you | The terminal UI's; the task asks for them. With Needs you, only that section is listed. |
| Agents list: search by itself | titles; the rest through the daemon | title, prompt, repository, harness, model, account; joined with what the daemon found | Works without a connection. It never finds what the daemon does not. |
| Agents list: tooltip | Markdown, the title bold | the same lines, plain | No Markdown in a native label. |
| Agents list: colour | only failed, disconnected and waiting rows take the badge's colour | `badgeTone` is there for every status; `emphasized` says when VS Code colours the row | The app decides. |
| Agents list: the daemon cannot be reached | a row without an id | the same row, id `notice:daemon` | A list needs a key. |
| Agents list: the rollup row | its tooltip adds "Click to show only one of them."; a click opens a filter picker | the counts only | The phone's filters are the chips above the list. The rest of the row is compared. |
| Agents list: reviewed marks | kept in VS Code's storage, set when an agent at its end or its review is opened, or it is merged | kept on the phone, set when an agent is opened on the phone | The phone cannot read VS Code's storage. Sharing them needs a daemon method. The rule (reviewed once opened after it ended, within a week) is the same and is compared. |
| Agents list: Needs you | also counts Overseer's own proposals and conflicts, and Continuity's agents waiting for a connection | the agents waiting for an answer | The phone does not keep the state's `overseer` summary or ask for Continuity's status yet. |
| Agents list: what the work became | "Merged into main (1a2b3c4)" first on a finished agent's row and in its status (AC-243, the state's `landings`) | not there | The phone's store does not keep the state's `landings` yet; the recordings have none. The chat's merge and pull request lines are there. |
| Agents list: oversight marks | "held", "watched", conflicts beside an agent (AC-199) | not there | The phone's store does not keep the state's `oversight` yet. |
| Conversation: Continuity's cards | a card while an agent waits for a connection or memory (Use a local model now, Retry now, Stop) and one when it is back online | not there; its quiet lines are | They need the daemon's Continuity status and actions, which the phone does not have yet (AC-127: "the connection state from Gate L when it exists"). |
| Conversation: Auto's decisions | a card for `auto_decision` (Auto routing, off by default) | not there | Unfinished in the extension too (AC-204). |
| Conversation: Overseer's own conversation | proposal cards with Yes and No, message cards (asks, reports, findings) | not there | Talk to Overseer on the phone is AC-128, not built yet; Overseer's run is not in the phone's list. |
| Review: context lines | Monaco shows the whole file | removed and added lines only | The daemon sends no context. |
| Review: unsaved, read-only, staged and unstaged | markers and scopes | not there | Editing on the phone is not in this gate. |
| Review: words for a status | the letter | the letter, and "New file", "Deleted file", "Renamed from …" | A letter is little on a phone. The letter is what is compared. |

## What events do not say

The store is kept current from events alone. After any sequence of recorded events it equals the
daemon's state. These changes the daemon makes without an event, or with less than the state
holds. They are from reading `daemon/src`; the recordings do not hold them. The phone should load
`state` again when it reconnects, which makes all of them right.

| What | What happens | 
| --- | --- |
| `profile.rename` | No event. The old name stays until `state` is loaded. |
| A follow-up with another model | `runs.model` changes; `turn_started` does not say so. |
| `review.import` | Marks are added without events. |
| A permission request with a secret in its input | The event is redacted; the state's `attention` is not. The phone shows the redacted one. |
| A stop asked for before the state was loaded | The daemon ends the turn as `interrupted` because of a file it keeps; the phone would say `failed`. `markStopping` tells the store when the app saw the request in the history. |
| Five times (see "The store") | The event's time is used; up to a few milliseconds apart. |

## What was found on the way

For the owners of the other areas. Nothing here was changed by this package.

| Where | What |
| --- | --- |
| `protocol/protocol.json` | The records agree with every recording: no field missing, none unseen. |
| `protocol/protocol.json` | Event kinds the daemon sends that are not described: `interrupt_requested` (payload `{}`), `daemon_started` (`{ pid, reconcile }`), and from `daemon/src`: `workspace_removed` (`{ workspace_id, path, branch_kept, discarded_dirty }`), `reattached` (`{ note }`), `merge_back`, `daemon_error`, `background_notice`, `daemon_stopping`. The store reads `interrupt_requested`, `workspace_removed` and `reattached`. |
| `protocol/protocol.json` | `task.create` can answer with `launch_error` (a string) beside `task`, `run`, `workspace`; the description does not have it. |
| `daemon` | `task_archived`, `workspace_removed`, `turn_done` and a child's `status` do not carry the time the state keeps; `profile.rename` writes no event. |
| `extension/media/conversation.js` | `describe` throws on an edit that is `null`. |
| `extension/media/conversation.js` | A tool call's result is drawn again only when its own events arrive or its turn ends. After a run fails without a `turn_done`, a call without a result keeps its running dot. The phone does the same, to agree. |
| `extension/media/conversation.js` | Kinds it has no picture for show as a line of their name. With the kinds of Gate N this shows "review mark", "review reject", "merge back", "pull request" in the chat. The phone does the same, to agree. |

## The licence of marked

`src/marked-lexer.ts` is derived from marked 14.0.0, Copyright (c) 2011-2024 Christopher Jeffrey,
MIT. The licence is in `licenses/marked-LICENSE.md`. The extension ships the same version in
`extension/media/vendor/marked.umd.js`.

## Not checked

- The app's JavaScript engine. The Markdown rules use regular expressions with Unicode classes
  (`\p{P}`, `\p{S}`, `\p{L}`, `\p{N}`). All 175 regular expressions the model writes or builds
  were given to the Hermes compiler in `phone/node_modules` (`hermes-compiler`), which compiled
  them without an error. They were not run on a device or a simulator.
- A message sent from a real phone. The `remote_command` event in `test/pending.test.ts` is shaped
  as `daemon/src/gateway/remote.rs` writes it; it was not recorded through the gateway.
- OpenCode. No recording uses it.
