// Every sentence the phone shows that VS Code also shows. Each one is copied from the extension and
// says which file it was copied from; test/text.test.ts reads those files and fails when a
// sentence here is no longer in its file, so the two cannot drift silently.
//
// Sentences the phone has and VS Code does not are in PHONE_ONLY, at the end.

const UI = 'extension/media/ui.js';
const CONVERSATION = 'extension/media/conversation.js';
const CHAT = 'extension/media/chat.js';
const VIEWS = 'extension/src/views.js';
const EXTENSION = 'extension/src/extension.js';
const PACKAGE = 'extension/package.json';
const REVIEW = 'extension/src/review.js';
const BROWSER = 'extension/branch-diff/review/browser.js';
const PHONE_TEXT = 'extension/src/phone-text.js';
const DAEMON = 'daemon/src/daemon.rs';
const ROLLUP = 'extension/media/rollup.js';

export interface Copied {
  /** The words, as they stand in the file. */
  readonly text: string;
  /** The file they were copied from, from the repository's root. */
  readonly from: string;
}

const copies: Copied[] = [];

/** Words copied from a file of the extension. */
function from<T extends string>(file: string, words: T): T {
  copies.push({ text: words, from: file });
  return words;
}

/** A sentence with a changing part: `source` is how the file writes it, `say` writes it here. */
function shaped<F>(file: string, source: string, say: F): F {
  copies.push({ text: source, from: file });
  return say;
}

/** Every copied sentence with its file, for the test that compares them. */
export const COPIED: ReadonlyArray<Copied> = copies;

const plural = (n: number): string => (n === 1 ? '' : 's');

/** 18423 becomes "18,423" whatever the phone's language is: VS Code's chat is in English. */
export function grouped(n: number): string {
  const sign = n < 0 ? '-' : '';
  const [whole = '0', part] = String(Math.abs(n)).split('.');
  return sign + whole.replace(/\B(?=(\d{3})+(?!\d))/g, ',') + (part !== undefined ? '.' + part : '');
}

export const TEXT = {
  /** A run's status in a conversation and its header (ui.js STATUS_TEXT). */
  status: {
    queued: from(UI, 'Queued'), starting: from(UI, 'Starting'), running: from(UI, 'Running'), waiting_for_user: from(UI, 'Needs you'), completed: from(UI, 'Done'),
    failed: from(UI, 'Failed'), interrupted: from(UI, 'Stopped'), disconnected: from(UI, 'Disconnected'), unknown: from(UI, 'Unknown'),
  } as Readonly<Record<string, string>>,

  /** A run's status in the agents list (views.js STATUS_TEXT). */
  listStatus: {
    queued: from(VIEWS, 'queued'), starting: from(VIEWS, 'starting'), running: from(VIEWS, 'working'), waiting_for_user: from(VIEWS, 'needs you'), completed: from(VIEWS, 'done'),
    failed: from(VIEWS, 'failed'), interrupted: from(VIEWS, 'stopped'), disconnected: from(VIEWS, 'disconnected'), unknown: from(VIEWS, 'unknown'),
  } as Readonly<Record<string, string>>,

  /** The mark beside an agent's row (views.js STATUS_BADGE). */
  badge: {
    queued: from(VIEWS, '○'), starting: from(VIEWS, '○'), running: from(VIEWS, '●'), waiting_for_user: from(VIEWS, '!'), completed: from(VIEWS, '✓'),
    failed: from(VIEWS, '✕'), interrupted: from(VIEWS, '■'), disconnected: from(VIEWS, '✕'), unknown: from(VIEWS, '?'),
  } as Readonly<Record<string, string>>,

  /** A harness by name (ui.js HARNESS). */
  harness: {
    claude: from(UI, 'Claude Code'), codex: from(UI, 'Codex'), 'codex-app': from(UI, 'Codex app-server'), opencode: from(UI, 'OpenCode'), generic: from(UI, 'Program'),
  } as Readonly<Record<string, string>>,

  agents: {
    title: from(PACKAGE, 'Agents'),
    needsYou: from(VIEWS, 'Needs you'),
    needsYouCount: shaped(VIEWS, '`Needs you, ${list.length}`', (n: number) => `Needs you, ${n}`),
    empty: from(PACKAGE, 'No agents yet.'),
    emptyMore: from(PACKAGE, 'Overseer runs agents through a local daemon; closing VS Code does not stop them.'),
    newTask: from(PACKAGE, 'New Agent'),
    unavailable: shaped(VIEWS, '`Daemon unavailable: ${m.error}`', (error: string) => `Daemon unavailable: ${error}`),
    nativeChild: from(VIEWS, 'native child'),
    inferred: from(VIEWS, ' (inferred)'),
    currentCheckout: from(VIEWS, 'current checkout'),
    repoLabel: shaped(VIEWS, "agent${tasks.length === 1 ? '' : 's'}${active ? `, ${active} active` : ''}", (name: string, agents: number, active: number) => `${name}, ${agents} agent${plural(agents)}${active ? `, ${active} active` : ''}`),
    approve: from(ROLLUP, 'Approve'),
    reply: from(ROLLUP, 'Reply'),
    wantsToUse: shaped(ROLLUP, "`Wants to use ${r.attention.tool || 'a tool'}`", (tool: string) => `Wants to use ${tool}`),
    waitingForReply: from(ROLLUP, 'Waiting for your reply'),
    needYou: shaped(EXTENSION, "`${n} need${n === 1 ? 's' : ''} you`", (n: number) => `${n} need${n === 1 ? 's' : ''} you`),
    search: from(PACKAGE, 'Search Agents'),
    searchHint: from(EXTENSION, 'Title, message, file, repository, account or status'),
    clearSearch: from(PACKAGE, 'Clear Search'),
    matches: shaped(EXTENSION, "`${shown} match${shown === 1 ? '' : 'es'} for “${filter.query}”`", (shown: number, query: string) => `${shown} match${shown === 1 ? '' : 'es'} for “${query}”`),
    showArchived: from(PACKAGE, 'Show Archived Agents'),
    showActive: from(PACKAGE, 'Show Active Agents'),
    archive: from(PACKAGE, 'Archive'),
    stop: from(PACKAGE, 'Stop'),
  },

  conversation: {
    you: from(CONVERSATION, 'You'),
    agent: from(CONVERSATION, 'Agent'),
    subAgent: from(CONVERSATION, 'Sub-agent'),
    working: from(CONVERSATION, 'Working…'),
    thinking: from(CONVERSATION, 'Thinking'),
    plan: from(CONVERSATION, 'Plan'),
    trimmed: from(CONVERSATION, 'Older history was trimmed. Raw output keeps everything.'),
    beforeFirstTurn: from(CONVERSATION, 'Before the first turn'),
    turn: shaped(CONVERSATION, '`Turn ${t.n}`', (n: number | string) => `Turn ${n}`),
    steps: shaped(CONVERSATION, '`${g.count} steps`', (n: number) => `${n} steps`),
    times: shaped(CONVERSATION, '`${v} ${n}×`', (verb: string, n: number) => `${verb} ${n}×`),
    stepsFailed: shaped(CONVERSATION, '`${g.failed} failed`', (n: number) => `${n} failed`),
    failedResult: from(CONVERSATION, 'failed'),
    exit: shaped(CONVERSATION, '`exit ${exit[1]}`', (code: string) => `exit ${code}`),
    added: shaped(CONVERSATION, '`+${card.desc.added || 0}`', (n: number) => `+${n}`),
    removed: shaped(CONVERSATION, '`−${card.desc.removed || 0}`', (n: number) => `−${n}`),
    command: from(CONVERSATION, 'Command'),
    content: from(CONVERSATION, 'Content'),
    newText: from(CONVERSATION, 'New text'),
    input: from(CONVERSATION, 'Input'),
    result: from(CONVERSATION, 'Result'),
    error: from(CONVERSATION, 'Error'),
    noOutput: from(CONVERSATION, 'No output reported.'),
    waitingForResult: from(CONVERSATION, 'Waiting for the result…'),
    openAtHunk: from(CONVERSATION, 'Open at the edited hunk in the review'),
    allow: shaped(CONVERSATION, '`Allow ${what}?`', (what: string) => `Allow ${what}?`),
    allowed: shaped(CONVERSATION, '`Allowed · ${what}`', (what: string) => `Allowed · ${what}`),
    denied: shaped(CONVERSATION, '`Denied · ${what}`', (what: string) => `Denied · ${what}`),
    asked: shaped(CONVERSATION, '`Asked · ${what}`', (what: string) => `Asked · ${what}`),
    allowOnce: from(CONVERSATION, 'Allow once'),
    deny: from(CONVERSATION, 'Deny'),
    request: from(CONVERSATION, 'Request'),
    requestHint: from(CONVERSATION, 'The exact request the agent sent'),
    signInAgain: from(CONVERSATION, 'Sign in again'),
    signInAgainLabel: shaped(CONVERSATION, "Sign in again with this agent\\'s account", "Sign in again with this agent's account"),
    done: from(CONVERSATION, 'Done'),
    stopped: from(CONVERSATION, 'Stopped'),
    failed: from(CONVERSATION, 'Failed'),
    tokens: shaped(CONVERSATION, '} tokens`', (count: string) => `${count} tokens`),
    tokensIn: shaped(CONVERSATION, '} in`', (n: number) => `${grouped(n)} in`),
    tokensOut: shaped(CONVERSATION, '} out`', (n: number) => `${grouped(n)} out`),
    tokensCached: shaped(CONVERSATION, '} cached`', (n: number) => `${grouped(n)} cached`),
    /** What was done from a phone, in the owner's words. */
    fromPhone: {
      'run.follow_up': from(CONVERSATION, 'Message'), 'run.permission': from(CONVERSATION, 'Answered'), 'run.interrupt': from(CONVERSATION, 'Stopped'), 'task.create': from(CONVERSATION, 'Started'),
    } as Readonly<Record<string, string>>,
    aPhone: from(CONVERSATION, 'a phone'),
    doneFrom: shaped(CONVERSATION, '`${what} from ${who}`', (what: string, who: string) => `${what} from ${who}`),
    from: shaped(CONVERSATION, '`From ${who}`', (who: string) => `From ${who}`),
    errorTitle: {
      auth: from(CONVERSATION, 'Signed out'), rate_limit: from(CONVERSATION, 'Rate limited'), quota: from(CONVERSATION, 'Usage limit reached'), network: from(CONVERSATION, 'Connection problem'),
    } as Readonly<Record<string, string>>,
    errorTitleOther: from(CONVERSATION, 'Error'),
  },

  /** What a tool call reads as: done, and while it runs or is asked for (conversation.js describe). */
  tool: {
    read: from(CONVERSATION, 'Read'),
    created: from(CONVERSATION, 'Created'), create: from(CONVERSATION, 'Create'),
    edited: from(CONVERSATION, 'Edited'), edit: from(CONVERSATION, 'Edit'),
    searched: from(CONVERSATION, 'Searched'),
    foundFiles: from(CONVERSATION, 'Found files'),
    listed: from(CONVERSATION, 'Listed'),
    ran: from(CONVERSATION, 'Ran'), run: from(CONVERSATION, 'Run'),
    fetched: from(CONVERSATION, 'Fetched'),
    searchedWeb: from(CONVERSATION, 'Searched the web'),
    updatedPlan: from(CONVERSATION, 'Updated the plan'),
    items: shaped(CONVERSATION, '`${i.todos.length} items`', (n: number) => `${n} items`),
    delegated: from(CONVERSATION, 'Delegated'),
    used: from(CONVERSATION, 'Used'),
    quoted: shaped(CONVERSATION, '`“${i.pattern}”`', (words: string) => `“${words}”`),
  },

  chat: {
    reply: from(CHAT, 'Reply…  (@ to mention a file)'),
    throughParent: from(CHAT, 'Sub-agents are steered through their parent'),
    noFollowUps: shaped(CHAT, '} does not take follow-ups`', (harness: string) => `${harness} does not take follow-ups`),
    queued: from(CHAT, 'Queued'),
    stoppingThenSending: from(CHAT, 'Stopping, then sending'),
    cancel: from(CHAT, 'Cancel'),
    latest: from(CHAT, 'Latest'),
    jumpToLatest: from(CHAT, 'Jump to the latest message'),
    send: from(CHAT, 'Send'),
    queueMessage: from(CHAT, 'Queue message'),
    stop: from(CHAT, 'Stop'),
    reviewChanges: from(CHAT, 'Review changes'),
    files: shaped(CHAT, "`${n} file${n === 1 ? '' : 's'}`", (n: number) => `${n} file${plural(n)}`),
    currentCheckout: from(CHAT, 'current checkout'),
    currentCheckoutTitle: from(CHAT, 'Current checkout'),
    whenItFinishes: from(CHAT, 'Message for when it finishes'),
    // AC-243: the chat's menu names the branch the work lands on.
    mergeBack: shaped(CHAT, "`Merge into ${land.target || 'main'}…`", (target = 'main') => `Merge into ${target}…`),
    openPullRequest: from(CHAT, 'Open pull request…'),
    rawOutput: from(CHAT, 'Raw output'),
    details: from(CHAT, 'Details'),
    copyRunId: from(CHAT, 'Copy agent ID'),
    archive: from(CHAT, 'Archive'),
    restore: from(CHAT, 'Restore from archive'),
    removeWorktree: from(CHAT, 'Remove worktree…'),
    agent: from(CHAT, 'Agent'),
  },

  review: {
    other: from(REVIEW, 'Other branch…'),
    otherDetail: from(REVIEW, 'Choose a branch for merge-base (PR-style) or direct tip comparison'),
    compareWith: shaped(REVIEW, '`Compare with ${branch}`', (branch: string) => `Compare with ${branch}`),
    unavailable: from(REVIEW, 'unavailable'),
    mergeBase: from(REVIEW, 'Merge-base (PR-style)'),
    branchTip: from(REVIEW, 'Branch tip (direct)'),
    isUnavailable: shaped(REVIEW, '`${option.label} is unavailable: ${option.detail}`', (label: string, detail: string) => `${label} is unavailable: ${detail}`),
    comparison: from(BROWSER, 'Comparison'),
    noChanges: from(BROWSER, 'No changes for this comparison.'),
    comparisonUnavailable: shaped(BROWSER, "'Comparison unavailable: ' + next.error", (why: string) => `Comparison unavailable: ${why}`),
    checking: from(BROWSER, 'Checking files…'),
    loading: from(BROWSER, 'Loading diff…'),
    reviewed: from(BROWSER, 'Reviewed'),
    accept: shaped(BROWSER, '`Accept hunk ${index + 1}`', (n: number) => `Accept hunk ${n}`),
    unmark: shaped(BROWSER, '`Unmark reviewed hunk ${index + 1}`', (n: number) => `Unmark reviewed hunk ${n}`),
    reject: shaped(BROWSER, '`Reject hunk ${index + 1}`', (n: number) => `Reject hunk ${n}`),
    hunkOf: shaped(BROWSER, '`Hunk ${index + 1} of ${row.entry.path}, ${where}`', (n: number, path: string, where: string) => `Hunk ${n} of ${path}, ${where}`),
    lines: shaped(BROWSER, '`lines ${change.modifiedStartLineNumber}–${change.modifiedEndLineNumber}`', (first: number, last: number) => `lines ${first}–${last}`),
    deletionAfter: shaped(BROWSER, '`deletion after line ${change.modifiedStartLineNumber}`', (line: number) => `deletion after line ${line}`),
    conflicted: from(BROWSER, 'conflicted'),
  },

  /** The comparisons the daemon offers. The daemon writes these labels; they are listed so a test can tell when it changes them. */
  comparison: {
    latestRun: from(DAEMON, 'Latest run'),
    sinceTaskStart: from(DAEMON, 'Since task start'),
    originalFork: from(DAEMON, 'Original fork'),
    sinceTurn: shaped(DAEMON, 'format!("Since turn {}", turn.n)', (n: number) => `Since turn ${n}`),
  },

  time: {
    /** How long ago, in the agents list (views.js `ago`). */
    now: from(VIEWS, 'now'),
    minutes: shaped(VIEWS, '`${Math.floor(s / 60)}m`', (n: number) => `${n}m`),
    hours: shaped(VIEWS, '`${Math.floor(s / 3600)}h`', (n: number) => `${n}h`),
    days: shaped(VIEWS, '`${Math.floor(s / 86400)}d`', (n: number) => `${n}d`),
    /** How long ago, in a sentence (phone-text.js `ago`). */
    justNow: from(PHONE_TEXT, 'just now'),
    minutesAgo: shaped(PHONE_TEXT, '`${Math.floor(s / 60)}m ago`', (n: number) => `${n}m ago`),
    hoursAgo: shaped(PHONE_TEXT, '`${Math.floor(s / 3600)}h ago`', (n: number) => `${n}h ago`),
    daysAgo: shaped(PHONE_TEXT, '`${Math.floor(s / 86400)}d ago`', (n: number) => `${n}d ago`),
  },
} as const;

/** Sentences only the phone has. Plain words, short. */
export const PHONE_ONLY = {
  filter: { all: 'All', active: 'Active', needs: TEXT.agents.needsYou },
  sending: 'Sending',
  notSent: 'Not sent',
  answeredBy: (who: string): string => `by ${who}`,
  noMatches: 'No agents match.',
  noActive: 'No agents are working.',
  nothingNeedsYou: 'Nothing needs you.',
  noArchived: 'No archived agents.',
  notShown: (why: string): string => (why ? `Not shown: ${why}.` : 'Not shown.'),
  newFile: 'New file',
  deletedFile: 'Deleted file',
  renamedFrom: (path: string): string => `Renamed from ${path}`,
  conflicted: 'Conflicted',
} as const;

/** "Needs you" for waiting_for_user; an unknown status with its underscores as spaces (ui.js statusText). */
export function statusText(status: string | null | undefined): string {
  const known = status ? TEXT.status[status] : undefined;
  return known ?? String(status || 'unknown').replace(/_/g, ' ');
}

/** The agents list's word for a status: "working", "needs you" (views.js). */
export function listStatusText(status: string): string {
  return TEXT.listStatus[status] ?? status;
}

/** "now", "5m", "2h", "3d": how long ago in the agents list (views.js `ago`). */
export function ago(ms: number | null | undefined, now: number): string {
  if (!ms) return '';
  const s = Math.max(0, (now - ms) / 1000);
  if (s < 60) return TEXT.time.now;
  if (s < 3600) return TEXT.time.minutes(Math.floor(s / 60));
  if (s < 86400) return TEXT.time.hours(Math.floor(s / 3600));
  return TEXT.time.days(Math.floor(s / 86400));
}

/** "just now", "5m ago", "2h ago", "3d ago": how long ago in a sentence (phone-text.js `ago`, without its date). */
export function agoInWords(ms: number | null | undefined, now: number): string {
  if (!ms) return '';
  const s = Math.max(0, (now - ms) / 1000);
  if (s < 60) return TEXT.time.justNow;
  if (s < 3600) return TEXT.time.minutesAgo(Math.floor(s / 60));
  if (s < 86400) return TEXT.time.hoursAgo(Math.floor(s / 3600));
  return TEXT.time.daysAgo(Math.floor(s / 86400));
}

/** "12s", "3m 4s", "1h 2m" (ui.js `duration`). */
export function duration(ms: number | null | undefined): string {
  if (ms === null || ms === undefined || (!ms && ms !== 0)) return '';
  const s = Math.round(ms / 1000);
  return s < 60 ? `${s}s` : s < 3600 ? `${Math.floor(s / 60)}m ${s % 60}s` : `${Math.floor(s / 3600)}h ${Math.floor((s % 3600) / 60)}m`;
}

/** 18423 becomes "18k", 1204 "1.2k", 2500000 "2.5M" (ui.js `compact`). */
export function compact(n: number): string {
  return n >= 1e6 ? (n / 1e6).toFixed(1).replace(/\.0$/, '') + 'M' : n >= 1e4 ? Math.round(n / 1e3) + 'k' : n >= 1e3 ? (n / 1e3).toFixed(1).replace(/\.0$/, '') + 'k' : String(n);
}

/** The last part of a path (ui.js `basename`). */
export function basename(path: unknown): string {
  const s = String(path || '');
  return s.split('/').filter(Boolean).pop() || s;
}

/** The first line that says something, cut to `max` (ui.js `firstLine`). */
export function firstLine(value: unknown, max = 120): string {
  const line = String(value || '').split('\n').find(x => x.trim()) || '';
  return line.length > max ? line.slice(0, max - 1).trimEnd() + '…' : line;
}

/** "~/…/last/two" for a long path (ui.js `shortPath`). `home` is the Mac's home folder, when known. */
export function shortPath(path: unknown, home = '', keep = 2): string {
  if (!path) return '';
  let s = String(path);
  if (home && s.startsWith(home + '/')) s = '~' + s.slice(home.length);
  const parts = s.split('/').filter(Boolean);
  if (parts.length <= keep + 1) return s;
  return (s.startsWith('~') ? '~/…/' : '…/') + parts.slice(-keep).join('/');
}
