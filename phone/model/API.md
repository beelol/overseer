# `@overseer/phone-model`: the exports the screens can build on

Everything here is pure TypeScript: no React, no React Native, no DOM, no timers, no network.
Functions take data and return data. Nothing is changed in place: a function that "changes" a
state or a conversation returns a new one and leaves the old one as it was.

These names and shapes are committed. They will grow; they will not be renamed or reshaped.

```ts
import { store, agents, conversation, markdown, review, pending, text } from '@overseer/phone-model';
import type { Run, State, DaemonEvent, OutboxEntry } from '@overseer/phone-model';
```

`src/index.ts` exports seven namespaces (one per module below) and the types of `src/types.ts`.

## Types (`src/types.ts`)

The daemon's records are re-exported from `phone/protocol/protocol.generated.ts`, not declared
again: `Task`, `Run`, `RunStatus`, `Turn`, `Workspace`, `Profile`, `Event`, `State`, `Attention`,
`Change`, `GitStatus`, `Comparison`, `FileSide`, `Hunk`, `Mark`, `NotificationSettings`,
`EventPayloads`, `KnownEventKind`, `TypedEvent`.

| Export | Meaning |
| --- | --- |
| `type DaemonEvent = Event` | One event of the daemon's stream. |
| `interface OutboxEntry { requestId: string; method: string; params: unknown; state: 'queued' \| 'sending' \| 'done' \| 'failed'; createdAt: number }` | An entry of `phone/core`'s outbox, taken as given. |
| `interface WorkspaceRemovedPayload`, `interface ProfilePayload` | Payloads of event kinds the protocol description does not have yet. |
| `ACTIVE_STATUSES: ReadonlyArray<string>` | `queued`, `starting`, `running`, `waiting_for_user`. |
| `isActive(status: string \| null \| undefined): boolean` | True while a run is still going. |
| `isRun(v: unknown): v is Run`, `isTurn`, `isAttention`, `isMark` | Narrow a value of unknown shape. |
| `record(v: unknown): Readonly<Record<string, unknown>>`, `textOf(v): string \| undefined`, `numberOf(v): number \| undefined` | Read a payload without assuming its shape. (`src/types.ts` also exports them as `text` and `number`; from the package, `text` is the namespace of the words.) |

## `store`: the phone's copy of the daemon's state

| Export | Meaning |
| --- | --- |
| `interface PhoneState { cursor: number; daemon: State['daemon'] \| null; tasks: Table<Task>; runs: Table<Run>; workspaces: Table<Workspace>; profiles: Table<Profile>; turns; kids; marks; stopping }` | The state. Read it through the functions below. |
| `interface Table<T> { byId: PMap<T>; order: PVec<string> }` | Records by id, and their ids in the daemon's order. |
| `EMPTY: PhoneState` | The state before anything was loaded. |
| `load(state: State): PhoneState` | The daemon's `state`, as the phone keeps it. |
| `apply(state: PhoneState, event: DaemonEvent): PhoneState` | The state after one event. An event at or before the cursor changes nothing. |
| `applyAll(state: PhoneState, events: Iterable<DaemonEvent>): PhoneState` | Several events, in order. |
| `snapshot(state: PhoneState): { cursor; daemon; tasks: Task[]; runs: Run[]; workspaces: Workspace[]; profiles: Profile[]; turns: Record<string, Turn[]> }` | The same records in the shape and order of the daemon's `state`. |
| `loadMarks(state: PhoneState, runId: string, marks: ReadonlyArray<Mark>): PhoneState` | The reviewed marks of a run, as `review.marks` returned them. Events keep them current afterwards. |
| `markStopping(state: PhoneState, runId: string): PhoneState` | The app saw in a run's history that its stop was asked for before the state was loaded. |
| `rows<T>(table: Table<T>): ReadonlyArray<T>` | A table's records in the daemon's order. The same array until the table changes. |
| `run(state, id): Run \| undefined`, `task(state, id)`, `workspace(state, id)`, `profile(state, id)` | One record. `id` may be `null` or `undefined`. |
| `turnsOf(state: PhoneState, runId: string): ReadonlyArray<Turn>` | The turns of a top-level run. |
| `marksOf(state: PhoneState, runId: string): ReadonlyArray<Mark>` | The reviewed marks of a run (empty until loaded). |
| `childrenOf(state: PhoneState, runId: string): ReadonlyArray<Run>` | A run's native children, in the daemon's order. |
| `descendantsOf(state: PhoneState, runId: string): ReadonlyArray<Run>` | Children, their children and so on. |
| `rootOf(state: PhoneState, runId: string): Run \| undefined` | The top-level run a run belongs to. |

## `agents`: the agents list

| Export | Meaning |
| --- | --- |
| `type AgentFilter = 'all' \| 'active' \| 'needs'` | All, Active, Needs you. |
| `type LogoKey = 'claudecode' \| 'codex' \| 'opencode' \| 'claude' \| 'openai' \| 'github' \| 'anthropic'` | A logo of `extension/media/logos` by name. |
| `interface AgentsOptions { now: number; filter?: AgentFilter; query?: string; matches?: Iterable<string>; showArchived?: boolean; collapsed?: ReadonlySet<string>; seen?: Readonly<Record<string, number>>; changed?: Readonly<Record<string, number>>; pinned?: ReadonlyArray<string>; error?: string }` | What the list depends on besides the state. `matches`: task ids the daemon's `search` returned for `query`. `seen`: when the owner last opened a run. `changed`: changed files of finished runs. |
| `interface AgentRow { id: string; kind: 'section' \| 'needs' \| 'repo' \| 'agent' \| 'child' \| 'notice'; depth: number; label: string; description: string; tooltip: string; accessibilityLabel: string; logo: LogoKey \| null; icon: string \| null; status: string \| null; statusText: string \| null; badge: string \| null; badgeTone: 'blue' \| 'yellow' \| 'orange' \| 'green' \| 'red' \| 'purple' \| 'quiet' \| null; emphasized: boolean; runId: string \| null; taskId: string \| null; repo: string \| null; expandable: boolean; expanded: boolean; context: string; active: boolean; archived: boolean; pinned: boolean }` | One row. `id` and `context` are VS Code's tree item id and context value. |
| `agentRows(state: PhoneState, options: AgentsOptions): ReadonlyArray<AgentRow>` | The rows, flat, with depth: Needs you first, then agents by repository, children under their parents. |
| `interface NeedsYou { run_id: string; rank: number; label: string; detail: string }` | One agent that needs the owner, and why. |
| `needsYou(state: PhoneState, options: Pick<AgentsOptions, 'now' \| 'seen' \| 'changed'>): ReadonlyArray<NeedsYou>` | The Needs you list, most urgent first. |
| `counts(state: PhoneState, options: Pick<AgentsOptions, 'now' \| 'seen' \| 'changed'>): { active: number; needs: number }` | For a badge and a status line. |
| `searchLocally(state: PhoneState, query: string): ReadonlyArray<string>` | Task ids whose title, repository, harness, model, account or prompt holds `query`. |
| `emptyText(state: PhoneState, options: AgentsOptions): string \| null` | What to say when the list has no rows; `null` when it has rows. |
| `logoForHarness(harness: string): LogoKey \| null`, `logoForProvider(provider: string): LogoKey \| null` | The logo of a harness or provider. |
| `interface RunHeader { runId: string; title: string; status: string; statusText: string; statusIcon: string; logo: LogoKey \| null; icon: string \| null; account: string; accountTooltip: string; model: string \| null; branch: string \| null; branchIcon: string \| null; branchTooltip: string \| null; exitReason: string \| null; child: boolean; active: boolean; archived: boolean; canSend: boolean; canStop: boolean; placeholder: string }` | The head of a conversation: title, status and one quiet line (account, model, branch). |
| `runHeader(state: PhoneState, runId: string): RunHeader \| undefined` | The header of a run's conversation. |

## `conversation`: one agent's conversation

| Export | Meaning |
| --- | --- |
| `interface Conversation { rootId: string; home: string; rows: PVec<Row>; working: { shown: boolean; label: string }; banner: string \| null; status: string \| undefined; attention: unknown; landed: PMap<number>; … }` | A conversation. `working`: what to say under the rows while the agent works. `banner`: what to say above them when older history is gone. `landed`: requests sent from a phone that became a turn. The other fields are the builder's own. |
| `type Row = UserRow \| MessageRow \| ThinkingRow \| StepsRow \| ToolRow \| EditRow \| PermissionRow \| ErrorRow \| ChildRow \| NoteRow \| FooterRow` | One row of the list. Every row has `key`, `kind`, `depth`, `turn`, `turnNumber`, `run`, `parent`, `seq`. |
| `interface UserRow { kind: 'user'; label: string; text: string; sent: 'sent' \| 'queued' \| 'sending' \| 'failed'; requestId: string \| null }` | Your prompt. |
| `interface MessageRow { kind: 'message'; role: string; label: string; text: string; markdown: boolean }` | The agent's reply (Markdown) or a program's output (plain). |
| `interface ThinkingRow { kind: 'thinking'; role: 'reasoning' \| 'plan'; label: string; icon: string; text: string }` | Reasoning or a plan; closed until opened. |
| `interface StepsRow { kind: 'steps'; icon: string; count: number; label: string; verbs: ReadonlyArray<{ verb: string; count: number }>; summary: string; failed: number; failedText: string; tooltip: string }` | Tool calls folded into one line: "6 steps", "Read · Searched · Edited 2×". |
| `interface ToolRow { kind: 'tool'; id: string; name: string; icon: string; verb: string; target: string; code: boolean; full: string; result: ToolResult; group: string \| null; input: unknown; output: unknown; isError: boolean; reported: string \| null; summary: string }` | A tool call: "Ran npm test". `full` is the whole path or command. |
| `type ToolResult = { state: 'running' } \| { state: 'ok' } \| { state: 'failed'; text: string } \| { state: 'changed'; added: number; removed: number; addedText: string; removedText: string }` | How the call ended. |
| `interface EditRow { kind: 'edit'; icon: string; files: ReadonlyArray<{ name: string; path: string; tooltip: string }>; confidence: string; group: string \| null }` | Files the agent changed; each opens the review at the hunk. |
| `interface PermissionRow { kind: 'permission'; requestId: unknown; tool: unknown; input: unknown; state: 'pending' \| 'allowed' \| 'denied' \| 'asked'; icon: string; text: string; full: string; preview: string \| null; by: string \| null }` | A permission request and what became of it. |
| `interface ErrorRow { kind: 'error'; icon: string; class: string; title: string; message: string; signIn: boolean }` | An error. |
| `interface ChildRow { kind: 'child'; icon: string; childRun: string; title: string; status: string; statusText: string; usage: string; tooltip: string }` | A native child; its rows follow, one deeper. `usage`: what it reported using, beside its title ("20k reported tokens"), empty until it reports; the tooltip says it in full. |
| `interface NoteRow { kind: 'note'; text: string; status: string \| null; icon: string \| null; tooltip: string \| null; link?: { label: string; runId: string } }` | A quiet line: how a run ended outside a turn, what was done from a phone, what Continuity (Gate L) did (a lost connection is one line per turn that counts the attempts). `link`: a handoff's "Open it", which opens the other agent. |
| `interface FooterRow { kind: 'footer'; state: 'ok' \| 'stopped' \| 'fail' \| null; icon: string \| null; text: string; tooltip: string \| null; duration: string; usage: string; usageDetail: string }` | The end of a turn: status, time, tokens and cost. |
| `interface ConversationOptions { rootId: string; home?: string }` | `home`: the Mac's home folder, when known. |
| `interface Appended { conversation: Conversation; changed: ReadonlyArray<number>; movedFrom: number }` | A change. `changed`: places in the new list of rows that are new or different. `movedFrom`: the first place from which rows sit elsewhere than before, or -1. |
| `create(options: ConversationOptions): Conversation` | An empty conversation. |
| `setRun(conversation: Conversation, run: Run, children: ReadonlyArray<Run>): Appended` | What the state says about the run now. Call it when the conversation opens and when the store's copy of the run or its children changes. |
| `append(conversation: Conversation, event: DaemonEvent): Appended` | The conversation after one event. An event seen before, or of another agent, changes nothing. |
| `appendAll(conversation: Conversation, events: Iterable<DaemonEvent>): Appended` | Several events as one change (one batch per frame). |
| `build(options: ConversationOptions, events: Iterable<DaemonEvent>, run?: Run, children?: ReadonlyArray<Run>): Conversation` | A conversation from a run's history. |
| `belongs(conversation: Conversation, event: DaemonEvent): boolean` | True for an event of this agent or of one of its native children. |
| `rowsOf(conversation: Conversation): ReadonlyArray<Row>` | Every row, as one array. The same array until the conversation changes. |
| `rowAt(conversation: Conversation, index: number): Row \| undefined` | One row, without building the array. |
| `rowCount(conversation: Conversation): number` | How many rows. |
| `visibleRows(conversation: Conversation, toggled?: ReadonlySet<string>): ReadonlyArray<Row>` | The rows a person sees: folds are closed until opened, children open until closed. `toggled` holds the keys of the folds opened and the children closed. |
| `interface ToolDetail { input: { label: string; text: string } \| null; output: { label: string; text: string; error: boolean } \| null; note: string \| null }` | What a tool call's row opens to. |
| `toolDetail(row: ToolRow): ToolDetail` | The input and result of a tool call. |
| `requestText(row: PermissionRow): string` | The exact request the agent sent. |
| `permissionActions(row: PermissionRow): ReadonlyArray<{ label: string; allow: boolean }>` | "Allow once" and "Deny" while the request is pending; nothing afterwards. |
| `markdownOf(row: MessageRow \| ThinkingRow, conversation: Conversation): ReadonlyArray<markdown.Block>` | The row's text as a Markdown tree. Parsed once for each row. |
| `describe(name: unknown, input: unknown, summary?: unknown): ToolDescription` | Icon, verb and target of a tool call. |
| `interface ToolDescription { icon: string; verb: string; pending?: string; target: string; full?: unknown; code?: boolean; added?: number; removed?: number }` | What a tool call reads as. |

## `markdown`: the agent's replies as a tree

| Export | Meaning |
| --- | --- |
| `type Block = Paragraph \| Heading \| List \| Table \| CodeBlock \| Quote \| Rule` | A block. |
| `interface Paragraph { type: 'paragraph'; children: ReadonlyArray<Inline> }` | |
| `interface Heading { type: 'heading'; level: 1 \| 2 \| 3 \| 4 \| 5 \| 6; children: ReadonlyArray<Inline> }` | |
| `interface List { type: 'list'; ordered: boolean; start: number; items: ReadonlyArray<ListItem> }` | |
| `interface ListItem { checked: boolean \| null; children: ReadonlyArray<Block \| InlineRun> }` | `checked` is `null` for an item that is not a task. |
| `interface InlineRun { type: 'inline'; children: ReadonlyArray<Inline> }` | Text of a tight list item or a table cell: inline content that is not its own paragraph. |
| `interface Table { type: 'table'; align: ReadonlyArray<'left' \| 'right' \| 'center' \| null>; head: ReadonlyArray<ReadonlyArray<Inline>>; rows: ReadonlyArray<ReadonlyArray<ReadonlyArray<Inline>>> }` | |
| `interface CodeBlock { type: 'code'; language: string; label: string; text: string; lines: number; collapsed: boolean }` | `label` is the language, or "text". `collapsed`: more than 24 lines. |
| `interface Quote { type: 'quote'; children: ReadonlyArray<Block> }` | |
| `interface Rule { type: 'rule' }` | |
| `type Inline = Text \| Strong \| Emphasis \| Strike \| InlineCode \| Link \| LineBreak \| Long` | Inline content. |
| `interface Text { type: 'text'; text: string }` | |
| `interface Strong { type: 'strong'; children: ReadonlyArray<Inline> }`, `Emphasis { type: 'emphasis'; … }`, `Strike { type: 'strike'; … }` | |
| `interface InlineCode { type: 'code'; text: string; parts: ReadonlyArray<Text \| Long> }` | `parts` is the same text with its long tokens shortened. |
| `interface Link { type: 'link'; href: string \| null; title: string \| null; opens: boolean; children: ReadonlyArray<Inline> }` | `href` is `null` when the address was removed as unsafe (`javascript:` and the like). `opens` is true for `http` and `https`, the only addresses VS Code opens. |
| `interface LineBreak { type: 'break' }` | |
| `interface Long { type: 'long'; text: string; full: string; path: boolean }` | An unbroken token of 60 characters or more, shown short; `full` is the whole of it. |
| `interface MarkdownOptions { home?: string }` | The Mac's home folder, for "~" in long paths. |
| `parse(source: string, options?: MarkdownOptions): ReadonlyArray<Block>` | The tree. Raw HTML never becomes a view: tags are dropped, their text is kept; what a script or style element holds is dropped with it. |
| `plainText(blocks: ReadonlyArray<Block>): string` | The words of a tree, a line for each block. |
| `safeHref(href: string): string \| null` | The address when VS Code's sanitizer keeps it, else `null`. |
| `knownEntities(): ReadonlyMap<string, string>` | The named entities this reads (`&copy;`, `&mdash;`: the ones of HTML 4 and some more), each with its character. |
| `opensExternally(href: string \| null): boolean` | True for `http` and `https`. |

## `review`: changed files, comparisons, hunks

| Export | Meaning |
| --- | --- |
| `type StatusLetter = 'A' \| 'D' \| 'R' \| 'M' \| 'U'` | Added, deleted, renamed, modified, conflicted: the letters of VS Code's review. |
| `statusLetter(status: string): StatusLetter` | The letter for a change the daemon reports. |
| `interface FileRow { kind: 'folder' \| 'file'; key: string; depth: number; name: string; path: string; status: StatusLetter \| null; statusText: string \| null; oldPath: string \| null; conflicted: boolean; added: number \| null; removed: number \| null; hunks: number \| null; reviewed: number \| null; tooltip: string; accessibilityLabel: string; expanded: boolean; icon: string }` | A folder or a file of the changed-files list. Counts are `null` until the file's hunks are known. `icon` is a codicon's name: folder, file, warning. |
| `interface FilesOptions { query?: string; collapsed?: ReadonlySet<string>; hunks?: Readonly<Record<string, ReadonlyArray<Hunk>>>; conflicted?: ReadonlyArray<string> }` | `hunks`: by path, for the files whose hunks were fetched. |
| `changedFiles(changes: ReadonlyArray<Change>, options?: FilesOptions): ReadonlyArray<FileRow>` | The list, flat, grouped by folder as VS Code's tree groups it. |
| `interface ChangesSummary { files: number; text: string; added: string; removed: string; names: string; tooltip: string }` | "3 files", "+10", "−2", "a.ts, b.ts, c.ts, …". |
| `changesSummary(changes: { files: number; added?: number; removed?: number; names?: ReadonlyArray<string> }): ChangesSummary \| null` | The changes bar under a conversation; `null` when nothing changed. |
| `interface ComparisonChoice { key: string; mode: string; branch: string \| null; label: string; description: string; detail: string; available: boolean; selected: boolean; base: string \| null }` | One comparison to choose. |
| `comparisonChoices(options: ReadonlyArray<Comparison>, selected?: { mode: string; branch?: string \| null }): ReadonlyArray<ComparisonChoice>` | The daemon's comparisons with VS Code's labels, the selected one marked. The last choice is "Other branch…" (`mode: 'other'`), as in VS Code's picker. |
| `interface DiffRow { kind: 'removed' \| 'added'; key: string; hunk: string; baseLine: number \| null; modifiedLine: number \| null; text: string }` | One line of a hunk. |
| `interface HunkView { key: string; index: number; reviewed: boolean; label: string; where: string; baseStart: number; modifiedStart: number; removed: number; added: number; rows: ReadonlyArray<DiffRow>; accept: { label: string; reviewed: boolean }; reject: { label: string }; hunk: Hunk }` | A hunk with its removed and added lines and its reviewed state. |
| `interface FileDiff { path: string; shown: boolean; why: string; hunks: ReadonlyArray<HunkView>; added: number; removed: number; reviewed: number; note: string \| null }` | A file's changes. |
| `fileDiff(result: { path: string; shown: boolean; why?: string \| null; hunks: ReadonlyArray<Hunk>; before?: { exists: boolean; kind: string }; now?: { exists: boolean; kind: string } }, marks?: Iterable<string>): FileDiff` | The result of `workspace.hunks` as rows. `marks`: keys marked reviewed since. |
| `hunkKey(path: string, baseLines: ReadonlyArray<string>, modifiedLines: ReadonlyArray<string>): string` | The key of a hunk, the same function the daemon and VS Code use. |
| `splitLine(text: string, width: number): ReadonlyArray<string>` | A long line in pieces of at most `width` characters, broken where words end. |
| `editTarget(diff: FileDiff): { hunk: string; line: number } \| null` | Where an edit chip opens: the first hunk not yet reviewed. |
| `acceptParams(path: string, hunk: Hunk, anchor?: string): { path: string; key: string; modified_start: number; modified_lines: ReadonlyArray<string>; base_lines: ReadonlyArray<string>; anchor?: string }` | The parameters of `review.accept` for a hunk (add `run_id`). |
| `branchChoices(branch: string): { title: string; choices: ReadonlyArray<{ label: string; mode: string; branch: string }> }` | After "Other branch…": the two ways to compare with the branch chosen. |

## `pending`: messages on their way

| Export | Meaning |
| --- | --- |
| `pendingRows(conversation: Conversation, outbox: ReadonlyArray<OutboxEntry>): ReadonlyArray<UserRow>` | A bubble for each message to this agent that has not become a turn yet, oldest first. |
| `withPending(conversation: Conversation, outbox: ReadonlyArray<OutboxEntry>, toggled?: ReadonlySet<string>): ReadonlyArray<Row>` | The visible rows, then the bubbles on their way. A message is one row with one key from queued to sent. |
| `isWaiting(conversation: Conversation, entry: OutboxEntry): boolean` | True while an entry still shows as its own bubble. |
| `sentLabel(row: UserRow): string` | "Queued", "Sending", "Not sent", or nothing for a message that was sent. |
| `keyOf(requestId: string): string` | The key of a message's row, the same before and after it became a turn. |

## `text`: the words

| Export | Meaning |
| --- | --- |
| `TEXT` | Every sentence the phone shows that VS Code also shows, copied from the extension. |
| `PHONE_ONLY` | Sentences only the phone has. |
| `COPIED: ReadonlyArray<{ text: string; from: string }>` | Every copied sentence with the file it came from. |
| `statusText(status: string \| null \| undefined): string` | "Running", "Needs you", "Done". |
| `listStatusText(status: string): string` | The agents list's word: "working", "needs you". |
| `continuityState(status: string \| null \| undefined): { text: string; icon: string; active: boolean } \| undefined` | One of the states Continuity adds (Gate L): waiting for a connection or for memory, handed off. |
| `plain(text: unknown, max?: number): string`, `plainTool(name: unknown): string` | Raw text of the daemon or a harness in plain words, as VS Code shows it (AC-245, `extension/media/plain-words.js`): no ids, snake_case, `mcp__` names or raw error text; "get issue (linear)" for `mcp__linear__get_issue`. |
| `ago(ms: number \| null \| undefined, now: number): string` | "now", "5m", "2h", "3d", as the side bar says it. |
| `agoInWords(ms: number \| null \| undefined, now: number): string` | "just now", "5m ago". |
| `duration(ms: number \| null \| undefined): string` | "12s", "3m 4s", "1h 2m". |
| `compact(n: number): string` | "18k", "1.2k", "2.5M". |
| `grouped(n: number): string` | "18,423". |
| `basename(path: unknown): string`, `firstLine(value: unknown, max?: number): string`, `shortPath(path: unknown, home?: string, keep?: number): string` | A path's last part; a text's first line; "~/…/last/two". |

## What the screens should know

- **Rows have stable keys.** Use `row.key` as the list's key. A message sent from the phone keeps
  one key (`pending.keyOf(requestId)`) from queued to sent.
- **Rows that did not change are the same objects.** A row component that is memoized on its row
  renders again only for the rows `append` names in `changed`.
- **Depth, not nesting.** Rows are flat. `depth` says how far a row is indented; a fold of steps
  or a closed child hides the rows after it that are deeper (`visibleRows` does this).
- **Call `setRun` when the store's copy of the run changes.** A permission card is `pending` only
  while the run's `attention` names its request; the working line shows only while the run goes.
- **Feed every event to `append`.** It picks the agent's own and its native children's.
- **Icons are names, not pictures.** `icon` is the name of the codicon VS Code shows (`check`,
  `error`, `terminal`, `tools`); `logo` is the name of a file in `extension/media/logos`.
- **Markdown is parsed when asked for.** `markdownOf(row, conversation)` parses once for each row;
  call it for the rows on screen.
- **After a reconnect, load `state` again.** A few things change in the daemon without an event
  (README, "What events do not say").
