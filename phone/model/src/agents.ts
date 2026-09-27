// The agents list: the rows of VS Code's side bar (extension/src/views.js), flat, with a depth.
//
// "Needs you" comes first, then the agents by repository, newest activity first, each with its
// native children and their children under it. Archived agents are hidden until asked for. The
// labels, the words for a status, the marks and the logos are the side bar's.
// test/agents-parity.test.ts loads the real views.js and compares row by row.

import { childrenOf, profile as profileOf, rows as rowsOfTable, run as runOf, task as taskOf, workspace as workspaceOf } from './store.ts';
import type { PhoneState } from './store.ts';
import { ago, basename, firstLine, listStatusText, PHONE_ONLY, statusText, TEXT } from './text.ts';
import { isActive, record } from './types.ts';
import type { Run, Task } from './types.ts';

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
  /** When the owner last opened each run, by run id. */
  readonly seen?: Readonly<Record<string, number>>;
  /** Changed files of finished runs, by run id (`workspace.changes`). */
  readonly changed?: Readonly<Record<string, number>>;
  readonly pinned?: ReadonlyArray<string>;
  /** Said in place of the list when the daemon cannot be reached. */
  readonly error?: string;
}

export interface AgentRow {
  /** VS Code's tree item id: section:needs, needs:<run>, repo:<path>, agent:<task>, run:<run>. */
  readonly id: string;
  readonly kind: 'section' | 'needs' | 'repo' | 'agent' | 'child' | 'notice';
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
  /** "working", "needs you", "done". */
  readonly statusText: string | null;
  /** The mark of the status: ●, !, ✓. */
  readonly badge: string | null;
  readonly badgeTone: BadgeTone | null;
  /** The row takes the badge's colour: failed, disconnected, needs you. */
  readonly emphasized: boolean;
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
  /** 0 waits for an answer, 1 failed, 2 finished with changes to review. */
  readonly rank: number;
  readonly label: string;
  readonly detail: string;
}

const HARNESS_LOGO: Readonly<Record<string, LogoKey>> = { claude: 'claudecode', codex: 'codex', 'codex-app': 'codex', opencode: 'opencode' };
const PROVIDER_LOGO: Readonly<Record<string, LogoKey>> = { anthropic: 'claude', openai: 'openai', local: 'opencode', github: 'github' };
const BADGE_TONE: Readonly<Record<string, BadgeTone>> = { queued: 'yellow', starting: 'blue', running: 'blue', waiting_for_user: 'orange', completed: 'green', failed: 'red', interrupted: 'quiet', disconnected: 'red', unknown: 'purple' };
const WEEK = 7 * 86400000;

export const logoForHarness = (harness: string): LogoKey | null => HARNESS_LOGO[harness] ?? null;
export const logoForProvider = (provider: string): LogoKey | null => PROVIDER_LOGO[provider] ?? null;

/** The agents that need the owner, most urgent first (extension.js `attention`). */
export function needsYou(state: PhoneState, options: Pick<AgentsOptions, 'now' | 'seen' | 'changed'>): ReadonlyArray<NeedsYou> {
  const runs = rowsOfTable(state.runs).filter(r => !r.parent_run_id);
  const when = (r: Run): number => r.ended_ms || r.created_ms;
  // Only the 40 most recently finished runs are looked at for changes to review.
  const recent = new Set(runs.filter(r => r.status === 'completed').sort((a, b) => when(b) - when(a)).slice(0, 40).map(r => r.id));
  const out: NeedsYou[] = [];
  const t = TEXT.agents;
  for (const r of runs) {
    if (taskOf(state, r.task_id)?.archived_ms) continue;
    const seen = options.seen?.[r.id] || 0;
    if (r.status === 'waiting_for_user') {
      const asks = r.attention?.kind === 'permission';
      out.push({ run_id: r.id, rank: 0, label: asks ? t.approve : t.reply, detail: asks ? t.wantsToUse(String(r.attention?.tool)) : t.waitingForReply });
    } else if (['failed', 'disconnected'].includes(r.status) && seen < when(r)) {
      out.push({ run_id: r.id, rank: 1, label: t.failed, detail: r.exit_reason || t.agentFailed });
    } else if (r.status === 'completed' && recent.has(r.id) && seen < when(r) && options.now - when(r) < WEEK) {
      const n = options.changed?.[r.id];
      if (n) out.push({ run_id: r.id, rank: 2, label: t.review, detail: t.filesChanged(n) });
    }
  }
  return out.sort((a, b) => a.rank - b.rank);
}

/** For a badge and a status line: runs still going, and agents that need the owner. */
export function counts(state: PhoneState, options: Pick<AgentsOptions, 'now' | 'seen' | 'changed'>): { readonly active: number; readonly needs: number } {
  return { active: rowsOfTable(state.runs).filter(r => isActive(r.status)).length, needs: needsYou(state, options).length };
}

/** Each task's newest top-level run: the agent a row stands for. */
function rootsOf(state: PhoneState): Map<string, Run> {
  const roots = new Map<string, Run>();
  for (const r of rowsOfTable(state.runs)) {
    if (r.parent_run_id) continue;
    const now = roots.get(r.task_id);
    if (now === undefined || r.created_ms > now.created_ms) roots.set(r.task_id, r);
  }
  return roots;
}

/** Task ids whose title, repository, harness, model, account or prompt holds `query`, whatever the case. */
export function searchLocally(state: PhoneState, query: string): ReadonlyArray<string> {
  const q = query.trim().toLowerCase();
  if (!q) return [];
  const roots = rootsOf(state);
  const has = (value: unknown): boolean => typeof value === 'string' && value.toLowerCase().includes(q);
  return rowsOfTable(state.tasks).filter(t => {
    const r = roots.get(t.id);
    return has(t.title) || has(t.prompt) || has(t.repo_root) || (r !== undefined && (has(r.title) || has(r.harness) || has(TEXT.harness[r.harness]) || has(r.model) || has(profileOf(state, r.profile_id)?.name)));
  }).map(t => t.id);
}

function visibleTasks(state: PhoneState, options: AgentsOptions, roots: Map<string, Run>, needs: ReadonlyArray<NeedsYou>): Task[] {
  const query = (options.query ?? '').trim();
  const found = query ? new Set([...searchLocally(state, query), ...(options.matches ?? [])]) : undefined;
  const needing = new Set(needs.map(n => runOf(state, n.run_id)?.task_id));
  const last = (t: Task): number => {
    const r = roots.get(t.id);
    return Math.max(t.created_ms || 0, r?.ended_ms || 0, r?.created_ms || 0, isActive(r?.status) ? options.now : 0);
  };
  return rowsOfTable(state.tasks)
    .filter(t => roots.has(t.id))
    // Search looks within the list shown: active agents, or archived ones when those are shown.
    .filter(t => (found === undefined || found.has(t.id)) && (options.showArchived ? !!t.archived_ms : !t.archived_ms))
    .filter(t => (options.filter === 'active' ? isActive(roots.get(t.id)?.status) : options.filter === 'needs' ? needing.has(t.id) : true))
    .map(t => ({ t, at: last(t) })).sort((a, b) => b.at - a.at).map(x => x.t);
}

function mark(status: string): Pick<AgentRow, 'status' | 'statusText' | 'badge' | 'badgeTone' | 'emphasized'> {
  const known = Object.hasOwn(TEXT.badge, status) ? status : 'unknown';
  return { status, statusText: listStatusText(status), badge: TEXT.badge[known] as string, badgeTone: BADGE_TONE[known] as BadgeTone, emphasized: ['failed', 'disconnected', 'waiting_for_user'].includes(status) };
}

function picture(harness: string): Pick<AgentRow, 'logo' | 'icon'> {
  const logo = logoForHarness(harness);
  return { logo, icon: logo ? null : harness === 'generic' ? 'terminal' : 'hubot' };
}

const NO_MARK = { status: null, statusText: null, badge: null, badgeTone: null, emphasized: false } as const;

let lastRows: { state: PhoneState; key: string; rows: ReadonlyArray<AgentRow> } | undefined;

/** The rows of the list, flat, with depth. Rows that did not change are the same objects as the last time. */
export function agentRows(state: PhoneState, options: AgentsOptions): ReadonlyArray<AgentRow> {
  const key = JSON.stringify([Math.floor(options.now / 1000), options.filter ?? 'all', options.query ?? '', [...(options.matches ?? [])], !!options.showArchived, [...(options.collapsed ?? [])], options.seen ?? {}, options.changed ?? {}, options.pinned ?? [], options.error ?? '']);
  const same = lastRows !== undefined && lastRows.key === key && lastRows.state.tasks === state.tasks && lastRows.state.runs === state.runs && lastRows.state.profiles === state.profiles && lastRows.state.workspaces === state.workspaces;
  if (same) return (lastRows as { rows: ReadonlyArray<AgentRow> }).rows;
  const built = build(state, options);
  const before = new Map((lastRows?.rows ?? []).map(r => [r.id, r]));
  const rows = built.map(row => {
    const was = before.get(row.id);
    return was !== undefined && JSON.stringify(was) === JSON.stringify(row) ? was : row;
  });
  lastRows = { state, key, rows };
  return rows;
}

function build(state: PhoneState, options: AgentsOptions): AgentRow[] {
  const t = TEXT.agents;
  if (options.error) {
    return [{ id: 'notice:daemon', kind: 'notice', depth: 0, label: t.unavailable(options.error), description: '', tooltip: '', accessibilityLabel: t.unavailable(options.error), logo: null, icon: 'warning', ...NO_MARK, runId: null, taskId: null, repo: null, expandable: false, expanded: false, context: '', active: false, archived: false, pinned: false }];
  }
  const out: AgentRow[] = [];
  const roots = rootsOf(state);
  const collapsed = options.collapsed ?? new Set<string>();
  const pinned = options.pinned ?? [];
  const query = (options.query ?? '').trim();
  const all = needsYou(state, options);
  const needs = query || options.showArchived ? [] : options.filter === 'active' ? all.filter(n => isActive(runOf(state, n.run_id)?.status)) : all;
  if (needs.length) {
    const open = !collapsed.has('section:needs');
    out.push({ id: 'section:needs', kind: 'section', depth: 0, label: t.needsYou, description: String(needs.length), tooltip: '', accessibilityLabel: t.needsYouCount(needs.length), logo: null, icon: 'bell-dot', ...NO_MARK, badgeTone: 'orange', runId: null, taskId: null, repo: null, expandable: true, expanded: open, context: 'section-needs', active: false, archived: false, pinned: false });
    if (open) {
      for (const n of needs) {
        const run = runOf(state, n.run_id);
        if (run === undefined) continue;
        const task = taskOf(state, run.task_id);
        const title = task?.title || run.title;
        out.push({ id: 'needs:' + run.id, kind: 'needs', depth: 1, label: title, description: n.label, tooltip: `${title}\n${n.detail}`, accessibilityLabel: `${title}, ${n.label}: ${n.detail}`, ...picture(run.harness), ...mark(run.status), runId: run.id, taskId: task?.id ?? null, repo: task?.repo_root ?? null, expandable: false, expanded: false, context: 'needs', active: isActive(run.status), archived: !!task?.archived_ms, pinned: pinned.includes(run.id) });
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
    out.push({ id, kind: 'child', depth, label: run.title, description: isActive(run.status) ? '' : ago(run.ended_ms || run.created_ms, options.now), tooltip: `${run.title}\n${status} · ${t.nativeChild}${run.relation_confidence?.startsWith('exact') ? '' : t.inferred}`, accessibilityLabel: `${run.title}, ${status}, ${t.nativeChild}`, ...picture(run.harness), ...mark(run.status), runId: run.id, taskId: run.task_id, repo: taskOf(state, run.task_id)?.repo_root ?? null, expandable: kids.length > 0, expanded: open, context: 'agent-child', active: isActive(run.status), archived: false, pinned: false });
    if (open) for (const kid of kids) child(kid, depth + 1);
  };
  for (const repo of new Set(tasks.map(x => x.repo_root))) {
    const mine = tasks.filter(x => x.repo_root === repo);
    const active = mine.filter(x => isActive(roots.get(x.id)?.status)).length;
    const id = 'repo:' + repo;
    const open = !collapsed.has(id);
    out.push({ id, kind: 'repo', depth: 0, label: basename(repo), description: active ? String(active) : '', tooltip: repo, accessibilityLabel: t.repoLabel(basename(repo), mine.length, active), logo: null, icon: 'repo', ...NO_MARK, runId: null, taskId: null, repo, expandable: true, expanded: open, context: 'repo', active: active > 0, archived: false, pinned: false });
    if (!open) continue;
    for (const task of mine) {
      const run = roots.get(task.id) as Run;
      const kids = childrenOf(state, run.id);
      const rowId = 'agent:' + task.id;
      const expanded = kids.length > 0 && !collapsed.has(rowId);
      const account = profileOf(state, run.profile_id);
      const ws = workspaceOf(state, run.workspace_id);
      const status = listStatusText(run.status);
      const going = isActive(run.status);
      const isPinned = pinned.includes(run.id);
      const tooltip = [task.title, `${status}${run.exit_reason && !going ? ` — ${run.exit_reason}` : ''}`, [run.harness, account?.name, run.model].filter(Boolean).join(' · '), ws ? `${ws.kind === 'current' ? t.currentCheckout : ws.branch} · ${basename(task.repo_root)}` : ''].filter(Boolean).join('\n');
      out.push({ id: rowId, kind: 'agent', depth: 1, label: task.title, description: going ? '' : ago(run.ended_ms || run.created_ms, options.now), tooltip, accessibilityLabel: `${task.title}, ${status}, ${run.harness}${account ? ', ' + account.name : ''}`, ...picture(run.harness), ...mark(run.status), runId: run.id, taskId: task.id, repo, expandable: kids.length > 0, expanded, context: `agent-${going ? 'active' : 'done'}${task.archived_ms ? '-archived' : ''}${isPinned ? '-pinned' : ''}`, active: going, archived: !!task.archived_ms, pinned: isPinned });
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
  /** The account's name, or the harness when it has none. */
  readonly account: string;
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
    account: account?.name || harness, accountTooltip: `${harness}${run.harness_version ? ' ' + run.harness_version : ''}${account ? ' · ' + account.name : ''}`, model: run.model || null,
    branch: ws ? (worktree ? basename(ws.branch) : TEXT.chat.currentCheckout) : null, branchIcon: ws ? (worktree ? 'git-branch' : 'repo') : null,
    branchTooltip: ws ? `${worktree ? ws.branch : TEXT.chat.currentCheckoutTitle}\n${ws.path}` : null,
    exitReason: run.exit_reason && /failed|interrupted|disconnected/.test(run.status) ? run.exit_reason : null,
    child, active: going, archived: !!task?.archived_ms, canSend: !child && takesFollowUps, canStop: !child && going && stops,
    placeholder: child ? TEXT.chat.throughParent : !takesFollowUps ? TEXT.chat.noFollowUps(harness) : busy ? TEXT.chat.whenItFinishes : TEXT.chat.reply,
  };
}
