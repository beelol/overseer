// The agents list: the rows of VS Code's side bar (extension/src/views.js), flat, with a depth.
//
// The rollup by state comes first (AC-255), then "Needs you", then the agents by repository,
// newest activity first, each with its native children and their children under it. Archived
// agents are hidden until asked for. An agent at its end that was not looked at since carries its
// own mark, "to review" (AC-254), counted on its repository too (AC-256). The labels, the words
// for a status, the marks and the logos are the side bar's; the counts are extension/media/rollup.js's.
// test/agents-parity.test.ts loads the real views.js and rollup.js and compares row by row.
//
// The reviewed marks are the phone's own: an agent is reviewed once it was opened on the phone at
// its end (VS Code marks it the same way when it is opened there, or its review is). The marks VS
// Code keeps are in VS Code's storage, which the phone cannot read.

import { harnessName, plain } from './plain.ts';
import { childrenOf, profile as profileOf, rows as rowsOfTable, run as runOf, task as taskOf, workspace as workspaceOf } from './store.ts';
import type { PhoneState } from './store.ts';
import { ago, basename, continuityState, firstLine, listStatusText, PHONE_ONLY, statusText, TEXT } from './text.ts';
import { isActive, record } from './types.ts';
import type { Profile, Run, Task } from './types.ts';

/** What the Mac's own login is called, as VS Code calls it (AC-235, views.js `DEFAULT_LOGIN`). */
export const DEFAULT_LOGIN = "Mac's default login";

/** An account's own name: the Mac's login is "Mac's default login", never "claude (existing login)". */
export function accountName(p: Profile | undefined): string {
  if (p === undefined) return '';
  return p.is_system || / \(existing login\)$/.test(p.name || '') ? DEFAULT_LOGIN : p.name;
}

/** The account an agent runs on, in full (AC-235): "Claude Max · bil…@testbox.com · Mac's default login". */
export function accountLabel(p: Profile | undefined): string {
  return p === undefined ? '' : p.account?.label || accountName(p);
}

/** Beside the provider's logo (a list row): the plan and the shortened email, "Max · bil…@testbox.com". */
export function accountBrief(p: Profile | undefined): string {
  return p === undefined ? '' : [p.account?.plan, p.account?.email || accountName(p)].filter(Boolean).join(' · ');
}

/** The same where room is tight: "Claude Max · bil…@testbox.com". */
export function accountShort(p: Profile | undefined): string {
  return p === undefined ? '' : p.account?.short || accountName(p);
}

export type AgentFilter = 'all' | 'active' | 'needs';
export type LogoKey = 'claudecode' | 'codex' | 'opencode' | 'claude' | 'openai' | 'github' | 'anthropic';
export type BadgeTone = 'blue' | 'yellow' | 'orange' | 'green' | 'red' | 'purple' | 'quiet';

export interface AgentsOptions {
  /** The time, for "5m" and for what still needs the owner. */
  readonly now: number;
  readonly filter?: AgentFilter;
  /** What the owner typed in the search. */
  readonly query?: string;
  /** Task ids the daemon's `search` returned for `query` (message text, files, status). */
  readonly matches?: Iterable<string>;
  readonly showArchived?: boolean;
  /** Rows closed by the owner, by id. */
  readonly collapsed?: ReadonlySet<string>;
  /**
   * When the owner last opened each agent, by run id: the reviewed marks (AC-254). An agent at its
   * end is "to review" until it was opened after it ended.
   */
  readonly seen?: Readonly<Record<string, number>>;
  readonly pinned?: ReadonlyArray<string>;
  /** Said in place of the list when the daemon cannot be reached. */
  readonly error?: string;
}

export interface AgentRow {
  /** VS Code's tree item id: section:rollup, section:needs, needs:<run>, repo:<path>, agent:<task>, run:<run>. */
  readonly id: string;
  readonly kind: 'rollup' | 'section' | 'needs' | 'repo' | 'agent' | 'child' | 'notice';
  readonly depth: number;
  readonly label: string;
  /** Beside the label, quieter: a count, how long ago, why it needs you. */
  readonly description: string;
  readonly tooltip: string;
  readonly accessibilityLabel: string;
  readonly logo: LogoKey | null;
  /** A codicon's name, where there is no logo. */
  readonly icon: string | null;
  readonly status: string | null;
  /** "working", "needs you", "done, to review", "failed, reviewed". */
  readonly statusText: string | null;
  /** The mark of the status: ●, !, ✓; ✦ for done work not reviewed yet. */
  readonly badge: string | null;
  readonly badgeTone: BadgeTone | null;
  /** The row takes the badge's colour: needs you, and work at its end not reviewed yet (✦, or a failure's ✕). */
  readonly emphasized: boolean;
  /** An agent at its end that was not opened since (AC-254). */
  readonly toReview: boolean;
  readonly runId: string | null;
  readonly taskId: string | null;
  readonly repo: string | null;
  readonly expandable: boolean;
  readonly expanded: boolean;
  /** VS Code's context value: agent-active, agent-done-archived, agent-child, needs, repo, section-needs. */
  readonly context: string;
  readonly active: boolean;
  readonly archived: boolean;
  readonly pinned: boolean;
}

export interface NeedsYou {
  readonly run_id: string;
  /** 0: it waits for an answer (rollup.js ranks Overseer's own decisions 0 too, and Continuity's waiting agents 3). */
  readonly rank: number;
  readonly label: string;
  readonly detail: string;
}

const HARNESS_LOGO: Readonly<Record<string, LogoKey>> = { claude: 'claudecode', codex: 'codex', 'codex-app': 'codex', opencode: 'opencode', 'opencode-serve': 'opencode' };
const PROVIDER_LOGO: Readonly<Record<string, LogoKey>> = { anthropic: 'claude', openai: 'openai', local: 'opencode', github: 'github' };
const BADGE_TONE: Readonly<Record<string, BadgeTone>> = { queued: 'yellow', starting: 'blue', running: 'blue', waiting_for_user: 'orange', completed: 'green', failed: 'red', interrupted: 'quiet', disconnected: 'red', unknown: 'purple' };

export const logoForHarness = (harness: string): LogoKey | null => HARNESS_LOGO[harness] ?? null;
export const logoForProvider = (provider: string): LogoKey | null => PROVIDER_LOGO[provider] ?? null;

// The states of rollup.js: at work, at their end.
const WORKING = new Set(['queued', 'starting', 'running', 'waiting_for_connection', 'waiting_for_memory']);
const DONE = new Set(['completed', 'interrupted']);
const FAILED = new Set(['failed', 'disconnected']);
const WEEK = 7 * 86400000;
const endOf = (r: Run): number => r.ended_ms || r.created_ms || 0;

/** Going, as the side bar counts it: the daemon's active states and Continuity's waiting ones (views.js ACTIVE). */
const listActive = (status: string | null | undefined): boolean => isActive(status) || !!continuityState(status)?.active;

/**
 * Whether an agent at its end still waits to be reviewed (AC-254, rollup.js `unreviewed`): a
 * top-level run that ended (done, stopped or failed) in the last 7 days and was not opened since.
 */
function unreviewed(run: Run, seen: Readonly<Record<string, number>> | undefined, now: number): boolean {
  if (run.parent_run_id || !(DONE.has(run.status) || FAILED.has(run.status))) return false;
  if (now - endOf(run) > WEEK) return false;
  return !((seen?.[run.id] || 0) >= endOf(run));
}

/**
 * The agents that need the owner (AC-246): what waits for their answer, counted as VS Code and the
 * TUI count it (extension/media/rollup.js `needsYou`): each listed agent that waits on a permission
 * or a question. Failed and finished agents are "to review" instead (AC-254), not Needs you.
 * (VS Code also counts Overseer's own proposals and Continuity's waiting agents: the phone does not
 * keep Overseer's summary of the state or ask for Continuity's status yet.)
 */
export function needsYou(state: PhoneState, _options: Pick<AgentsOptions, 'now' | 'seen'>): ReadonlyArray<NeedsYou> {
  const out: NeedsYou[] = [];
  const t = TEXT.agents;
  for (const r of listedRoots(state)) {
    if (r.status !== 'waiting_for_user') continue;
    const asks = r.attention?.kind === 'permission';
    out.push({ run_id: r.id, rank: 0, label: asks ? t.approve : t.reply, detail: asks ? t.wantsToUse(String(r.attention?.tool || 'a tool')) : t.waitingForReply });
  }
  return out;
}

/** For a badge and a status line: runs still going, and agents that need the owner. */
export function counts(state: PhoneState, options: Pick<AgentsOptions, 'now' | 'seen'>): { readonly active: number; readonly needs: number } {
  return { active: rowsOfTable(state.runs).filter(r => isActive(r.status)).length, needs: needsYou(state, options).length };
}

/** The agents by state (AC-255, rollup.js `counts`): the side bar's summary row and the grid's header. */
export interface Rollup {
  readonly working: number;
  readonly needs: number;
  /** Done, not reviewed yet. */
  readonly unreviewed: number;
  readonly reviewed: number;
  /** Failed, not reviewed yet: a failure once reviewed counts as reviewed. */
  readonly failed: number;
}

/** The agents by state, counted as VS Code, the TUI and the grid count them. */
export function rollup(state: PhoneState, options: Pick<AgentsOptions, 'now' | 'seen'>): Rollup {
  const c = { working: 0, needs: needsYou(state, options).length, unreviewed: 0, reviewed: 0, failed: 0 };
  for (const r of listedRoots(state)) {
    if (r.status === 'waiting_for_user') continue;
    if (WORKING.has(r.status)) c.working++;
    else if (unreviewed(r, options.seen, options.now)) {
      if (FAILED.has(r.status)) c.failed++;
      else c.unreviewed++;
    } else if (DONE.has(r.status) || FAILED.has(r.status)) c.reviewed++;
  }
  return c;
}

/** The rollup in words, only the states that have agents: "2 working · 1 needs you · 6 to review". */
export function rollupText(c: Rollup): string {
  const words = TEXT.agents.rollup;
  return ([['working', c.working], ['needs', c.needs], ['unreviewed', c.unreviewed], ['reviewed', c.reviewed], ['failed', c.failed]] as const)
    .filter(([, n]) => n > 0).map(([key, n]) => `${n} ${words[key]}`).join(' · ');
}

/** Tasks the list leaves out: Swarm's (its workers are Swarm's rows, views.js `visible`). */
function swarmTasks(state: PhoneState): Set<string> {
  const out = new Set<string>();
  for (const r of rowsOfTable(state.runs)) if (!r.parent_run_id && record(r)['swarm_membership']) out.add(r.task_id);
  return out;
}

/** Each task's newest top-level run: the agent a row stands for. */
function rootsOf(state: PhoneState): Map<string, Run> {
  const swarm = swarmTasks(state);
  const roots = new Map<string, Run>();
  for (const r of rowsOfTable(state.runs)) {
    if (r.parent_run_id || swarm.has(r.task_id)) continue;
    const now = roots.get(r.task_id);
    if (now === undefined || r.created_ms > now.created_ms) roots.set(r.task_id, r);
  }
  return roots;
}

/** The agents the counts are of: each listed task's newest top-level run, archived ones left out (rollup.js `agents`). */
function listedRoots(state: PhoneState): Run[] {
  return [...rootsOf(state).values()].filter(r => !taskOf(state, r.task_id)?.archived_ms);
}

/** Task ids whose title, repository, harness, model, account or prompt holds `query`, whatever the case. */
export function searchLocally(state: PhoneState, query: string): ReadonlyArray<string> {
  const q = query.trim().toLowerCase();
  if (!q) return [];
  const roots = rootsOf(state);
  const has = (value: unknown): boolean => typeof value === 'string' && value.toLowerCase().includes(q);
  return rowsOfTable(state.tasks).filter(t => {
    const r = roots.get(t.id);
    return has(t.title) || has(t.prompt) || has(t.repo_root) || (r !== undefined && (has(r.title) || has(r.harness) || has(TEXT.harness[r.harness]) || has(r.model) || has(accountLabel(profileOf(state, r.profile_id)))));
  }).map(t => t.id);
}

function visibleTasks(state: PhoneState, options: AgentsOptions, roots: Map<string, Run>, needs: ReadonlyArray<NeedsYou>): Task[] {
  const query = (options.query ?? '').trim();
  const found = query ? new Set([...searchLocally(state, query), ...(options.matches ?? [])]) : undefined;
  const needing = new Set(needs.map(n => runOf(state, n.run_id)?.task_id));
  const last = (t: Task): number => {
    const r = roots.get(t.id);
    return Math.max(t.created_ms || 0, r?.ended_ms || 0, r?.created_ms || 0, listActive(r?.status) ? options.now : 0);
  };
  return rowsOfTable(state.tasks)
    .filter(t => roots.has(t.id))
    // Search looks within the list shown: active agents, or archived ones when those are shown.
    .filter(t => (found === undefined || found.has(t.id)) && (options.showArchived ? !!t.archived_ms : !t.archived_ms))
    .filter(t => (options.filter === 'active' ? isActive(roots.get(t.id)?.status) : options.filter === 'needs' ? needing.has(t.id) : true))
    .map(t => ({ t, at: last(t) })).sort((a, b) => b.at - a.at).map(x => x.t);
}

/**
 * The mark of a run (views.js's file decoration): its badge, the badge's colour when the row takes
 * it, and the status in words. An agent at its end that was not reviewed yet has its own mark
 * (AC-254): ✦ for done work, a coloured ✕ for a failure; once reviewed it is the plain ✓ or an
 * uncoloured ✕.
 */
function mark(run: Run, options: AgentsOptions): Pick<AgentRow, 'status' | 'statusText' | 'badge' | 'badgeTone' | 'emphasized' | 'toReview'> {
  const status = run.status;
  const fresh = unreviewed(run, options.seen, options.now);
  if (fresh && DONE.has(status)) return { status, statusText: TEXT.agents.doneToReview, badge: TEXT.badgeToReview, badgeTone: 'green', emphasized: true, toReview: true };
  const cont = continuityState(status);
  const known = Object.hasOwn(TEXT.badge, status) ? status : 'unknown';
  const badge = cont !== undefined ? (cont.active ? TEXT.badgeWaiting : TEXT.badgeHandedOff) : (TEXT.badge[known] as string);
  const tone: BadgeTone = cont !== undefined ? (cont.active ? 'orange' : 'quiet') : (BADGE_TONE[known] as BadgeTone);
  const colored = status === 'waiting_for_user' || (fresh && FAILED.has(status));
  const ended = !run.parent_run_id && (DONE.has(status) || FAILED.has(status));
  const words = TEXT.listStatus[status] ?? cont?.text.toLowerCase() ?? status.replace(/_/g, ' ');
  // A failure once reviewed is quiet: its ✕ is no longer coloured.
  return { status, statusText: `${words}${ended ? (fresh ? TEXT.agents.toReview : TEXT.agents.reviewed) : ''}`, badge, badgeTone: ended && FAILED.has(status) && !fresh ? 'quiet' : tone, emphasized: colored, toReview: fresh };
}

function picture(harness: string): Pick<AgentRow, 'logo' | 'icon'> {
  const logo = logoForHarness(harness);
  return { logo, icon: logo ? null : harness === 'generic' ? 'terminal' : 'hubot' };
}

const NO_MARK = { status: null, statusText: null, badge: null, badgeTone: null, emphasized: false, toReview: false } as const;

let lastRows: { state: PhoneState; key: string; rows: ReadonlyArray<AgentRow> } | undefined;

/** The rows of the list, flat, with depth. Rows that did not change are the same objects as the last time. */
export function agentRows(state: PhoneState, options: AgentsOptions): ReadonlyArray<AgentRow> {
  const key = JSON.stringify([Math.floor(options.now / 1000), options.filter ?? 'all', options.query ?? '', [...(options.matches ?? [])], !!options.showArchived, [...(options.collapsed ?? [])], options.seen ?? {}, options.pinned ?? [], options.error ?? '']);
  const same = lastRows !== undefined && lastRows.key === key && lastRows.state.tasks === state.tasks && lastRows.state.runs === state.runs && lastRows.state.profiles === state.profiles && lastRows.state.workspaces === state.workspaces;
  if (same) return (lastRows as { rows: ReadonlyArray<AgentRow> }).rows;
  const built = build(state, options);
  const before = new Map((lastRows?.rows ?? []).map(r => [r.id, r]));
  const rows = built.map(row => {
    const was = before.get(row.id);
    return was !== undefined && sameRow(was, row) ? was : row;
  });
  lastRows = { state, key, rows };
  return rows;
}

/** Every field of a row is text, a number, a truth or nothing: two rows are the same when every field is. */
function sameRow(a: AgentRow, b: AgentRow): boolean {
  const x = a as unknown as Record<string, unknown>, y = b as unknown as Record<string, unknown>;
  for (const key in x) if (x[key] !== y[key]) return false;
  return true;
}

const HEADING = { logo: null, runId: null, taskId: null, archived: false, pinned: false } as const;

function build(state: PhoneState, options: AgentsOptions): AgentRow[] {
  const t = TEXT.agents;
  if (options.error) {
    return [{ id: 'notice:daemon', kind: 'notice', depth: 0, label: t.unavailable(options.error), description: '', tooltip: '', accessibilityLabel: t.unavailable(options.error), icon: 'warning', ...NO_MARK, ...HEADING, repo: null, expandable: false, expanded: false, context: '', active: false }];
  }
  const out: AgentRow[] = [];
  const roots = rootsOf(state);
  const collapsed = options.collapsed ?? new Set<string>();
  const pinned = options.pinned ?? [];
  const query = (options.query ?? '').trim();
  const all = needsYou(state, options);
  // The rollup by state (AC-255): a small summary row first, whatever is running, while the
  // whole list is shown. (VS Code's row opens a filter when clicked; the phone's filters are
  // the chips above the list, so the row only says the counts.)
  const plainList = !query && !options.showArchived && (options.filter ?? 'all') === 'all';
  const summary = plainList ? rollupText(rollup(state, options)) : '';
  if (summary) out.push({ id: 'section:rollup', kind: 'rollup', depth: 0, label: summary, description: '', tooltip: summary, accessibilityLabel: t.rollupLabel(summary), icon: 'pulse', ...NO_MARK, ...HEADING, repo: null, expandable: false, expanded: false, context: 'section-rollup', active: false });
  const needs = query || options.showArchived ? [] : options.filter === 'active' ? all.filter(n => isActive(runOf(state, n.run_id)?.status)) : all;
  if (needs.length) {
    const open = !collapsed.has('section:needs');
    out.push({ id: 'section:needs', kind: 'section', depth: 0, label: t.needsYou, description: String(needs.length), tooltip: '', accessibilityLabel: t.needsYouCount(needs.length), icon: 'bell-dot', ...NO_MARK, ...HEADING, badgeTone: 'orange', repo: null, expandable: true, expanded: open, context: 'section-needs', active: false });
    if (open) {
      for (const n of needs) {
        const run = runOf(state, n.run_id);
        if (run === undefined) continue;
        const task = taskOf(state, run.task_id);
        const title = task?.title || run.title;
        out.push({ id: 'needs:' + run.id, kind: 'needs', depth: 1, label: title, description: n.label, tooltip: `${title}\n${n.detail}`, accessibilityLabel: `${title}, ${n.label}: ${n.detail}`, ...picture(run.harness), ...mark(run, options), runId: run.id, taskId: task?.id ?? null, repo: task?.repo_root ?? null, expandable: false, expanded: false, context: 'needs', active: isActive(run.status), archived: !!task?.archived_ms, pinned: pinned.includes(run.id) });
      }
    }
  }
  if (options.filter === 'needs') return out;
  const tasks = visibleTasks(state, options, roots, all);
  const child = (run: Run, depth: number): void => {
    const kids = childrenOf(state, run.id);
    const id = 'run:' + run.id;
    const open = kids.length > 0 && !collapsed.has(id);
    const status = listStatusText(run.status);
    out.push({ id, kind: 'child', depth, label: run.title, description: listActive(run.status) ? '' : ago(run.ended_ms || run.created_ms, options.now), tooltip: `${run.title}\n${status} · ${t.nativeChild}${run.relation_confidence?.startsWith('exact') ? '' : t.inferred}`, accessibilityLabel: `${run.title}, ${status}, ${t.nativeChild}`, ...picture(run.harness), ...mark(run, options), runId: run.id, taskId: run.task_id, repo: taskOf(state, run.task_id)?.repo_root ?? null, expandable: kids.length > 0, expanded: open, context: 'agent-child', active: isActive(run.status), archived: false, pinned: false });
    if (open) for (const kid of kids) child(kid, depth + 1);
  };
  const byRepo = new Map<string, Task[]>();
  for (const x of tasks) {
    const list = byRepo.get(x.repo_root);
    if (list === undefined) byRepo.set(x.repo_root, [x]);
    else list.push(x);
  }
  for (const [repo, mine] of byRepo) {
    const theirRoots = mine.map(x => roots.get(x.id) as Run);
    const active = theirRoots.filter(r => listActive(r.status)).length;
    // AC-256: the repository counts finished work not reviewed yet too, so one that just finished
    // several agents never looks untouched; it reaches nothing once all are reviewed.
    const fresh = theirRoots.filter(r => !listActive(r.status) && unreviewed(r, options.seen, options.now));
    const toReview = fresh.filter(r => DONE.has(r.status)).length, failed = fresh.length - toReview;
    const id = 'repo:' + repo;
    const open = !collapsed.has(id);
    const description = [active && String(active), toReview && t.repoToReview(toReview), failed && t.repoFailed(failed)].filter(Boolean).join(' · ');
    const tooltip = [repo, active && t.repoWorking(active), toReview && t.repoDoneToReview(toReview), failed && t.repoFailedToReview(failed)].filter(Boolean).join('\n');
    out.push({ id, kind: 'repo', depth: 0, label: basename(repo), description, tooltip, accessibilityLabel: t.repoLabel(basename(repo), mine.length, active, toReview, failed), icon: 'repo', ...NO_MARK, ...HEADING, repo, expandable: true, expanded: open, context: 'repo', active: active > 0 });
    if (!open) continue;
    for (const task of mine) {
      const run = roots.get(task.id) as Run;
      const kids = childrenOf(state, run.id);
      const rowId = 'agent:' + task.id;
      const expanded = kids.length > 0 && !collapsed.has(rowId);
      const account = profileOf(state, run.profile_id);
      const ws = workspaceOf(state, run.workspace_id);
      const status = listStatusText(run.status);
      const going = listActive(run.status);
      const isPinned = pinned.includes(run.id);
      const marked = mark(run, options);
      const harness = harnessName(run.harness);
      const tooltip = [task.title, `${status}${run.exit_reason && !going ? ` — ${plain(run.exit_reason, 200)}` : ''}`, [harness, accountLabel(account), run.model].filter(Boolean).join(' · '), ws ? `${ws.kind === 'current' ? t.currentCheckout : ws.branch} · ${basename(task.repo_root)}` : ''].filter(Boolean).join('\n');
      // The ✦ badge marks it; the accessible name says "to review" too.
      out.push({ id: rowId, kind: 'agent', depth: 1, label: task.title, description: [going ? '' : ago(run.ended_ms || run.created_ms, options.now), accountBrief(account)].filter(Boolean).join(' · '), tooltip, accessibilityLabel: `${task.title}, ${status}${!going && marked.toReview ? t.toReview : ''}, ${harness}${account ? ', ' + accountLabel(account) : ''}`, ...picture(run.harness), ...marked, runId: run.id, taskId: task.id, repo, expandable: kids.length > 0, expanded, context: `agent-${going ? 'active' : 'done'}${task.archived_ms ? '-archived' : ''}${isPinned ? '-pinned' : ''}`, active: going, archived: !!task.archived_ms, pinned: isPinned });
      if (expanded) for (const kid of kids) child(kid, 2);
    }
  }
  return out;
}

/** What to say when the list has no rows; `null` when it has rows. */
export function emptyText(state: PhoneState, options: AgentsOptions): string | null {
  if (agentRows(state, options).length) return null;
  if ((options.query ?? '').trim()) return PHONE_ONLY.noMatches;
  if (options.showArchived) return PHONE_ONLY.noArchived;
  if (options.filter === 'active') return PHONE_ONLY.noActive;
  if (options.filter === 'needs') return PHONE_ONLY.nothingNeedsYou;
  return TEXT.agents.empty;
}

export interface RunHeader {
  readonly runId: string;
  readonly title: string;
  readonly status: string;
  /** "Running", "Needs you", "Done". */
  readonly statusText: string;
  /** 'dot' while it goes; else a codicon's name: check, error, shield, bell-dot, circle-slash. */
  readonly statusIcon: string;
  readonly logo: LogoKey | null;
  readonly icon: string | null;
  /** The account it runs on (AC-235): provider and plan, the shortened email, whose login; the harness when it has none. */
  readonly account: string;
  /** The account where room is tight ("Claude Max · bil…@testbox.com"); empty when it has none. */
  readonly accountShort: string;
  readonly accountTooltip: string;
  readonly model: string | null;
  /** The branch's last part, or "current checkout". */
  readonly branch: string | null;
  readonly branchIcon: string | null;
  readonly branchTooltip: string | null;
  /** Why it ended, for a run that failed, was stopped or was lost. */
  readonly exitReason: string | null;
  readonly child: boolean;
  readonly active: boolean;
  readonly archived: boolean;
  readonly canSend: boolean;
  readonly canStop: boolean;
  /** What the message field says while it is empty. */
  readonly placeholder: string;
}

/** The head of a run's conversation (chat.js `setRun`): title, status, and one quiet line. */
export function runHeader(state: PhoneState, runId: string): RunHeader | undefined {
  const run = runOf(state, runId);
  if (run === undefined) return undefined;
  const task = taskOf(state, run.task_id);
  const account = profileOf(state, run.profile_id);
  const ws = workspaceOf(state, run.workspace_id);
  const harness = TEXT.harness[run.harness] || run.harness;
  const child = !!run.parent_run_id;
  const going = isActive(run.status);
  const abilities = record(run.capabilities);
  const takesFollowUps = !String(abilities['follow_up'] || '').startsWith('unsupported');
  const stops = !String(abilities['interrupt'] || '').startsWith('unsupported');
  const busy = going && run.harness !== 'generic';
  const worktree = ws?.kind === 'worktree';
  const icons: Readonly<Record<string, string>> = { waiting_for_user: run.attention?.kind === 'permission' ? 'shield' : 'bell-dot', completed: 'check', failed: 'error', disconnected: 'debug-disconnect', interrupted: 'circle-slash' };
  return {
    runId: run.id, title: run.title || firstLine(task?.prompt) || TEXT.chat.agent, status: run.status, statusText: statusText(run.status),
    statusIcon: ['running', 'starting', 'queued'].includes(run.status) ? 'dot' : icons[run.status] || 'question', ...picture(run.harness),
    account: accountLabel(account) || harness, accountShort: accountShort(account), accountTooltip: `${harness}${run.harness_version ? ' ' + run.harness_version : ''}${account ? '\nAccount: ' + accountLabel(account) : ''}`, model: run.model || null,
    branch: ws ? (worktree ? basename(ws.branch) : TEXT.chat.currentCheckout) : null, branchIcon: ws ? (worktree ? 'git-branch' : 'repo') : null,
    branchTooltip: ws ? `${worktree ? ws.branch : TEXT.chat.currentCheckoutTitle}\n${ws.path}` : null,
    exitReason: run.exit_reason && /failed|interrupted|disconnected/.test(run.status) ? run.exit_reason : null,
    child, active: going, archived: !!task?.archived_ms, canSend: !child && takesFollowUps, canStop: !child && going && stops,
    placeholder: child ? TEXT.chat.throughParent : !takesFollowUps ? TEXT.chat.noFollowUps(harness) : busy ? TEXT.chat.whenItFinishes : TEXT.chat.reply,
  };
}
