/**
 * The Mac's plans for merging back and for a pull request. The protocol's description does not
 * say what they hold (`result: unknown`), so each is read field by field, never assumed
 * (daemon/src/merge.rs, daemon/src/pr.rs).
 */
import { number, record } from '@/model';

// `text(v)` of the model's types cannot be reached through `@/model`: the namespace `text`
// (the words) has the same name and wins. Reported.
const text = (value: unknown): string | undefined => (typeof value === 'string' ? value : undefined);
const strings = (value: unknown): readonly string[] => (Array.isArray(value) ? value.filter((item): item is string => typeof item === 'string') : []);

export type MergeState = 'idle' | 'ready' | 'resolving' | 'resolved';

export interface MergeRefused {
  readonly ok: false;
  /** Why not, in the Mac's words. */
  readonly reason: string;
}

export interface MergeReady {
  readonly ok: true;
  /** idle: nothing prepared. resolving: conflicts in the worktree. resolved: none left. ready: the target can take the branch. */
  readonly state: MergeState;
  readonly branch: string;
  readonly target: string;
  readonly repo: string;
  readonly runId: string | null;
  /** Files of the worktree that are not committed. */
  readonly uncommitted: readonly string[];
  readonly conflicts: readonly string[];
  /** Why the merge cannot be completed now, in the Mac's words. */
  readonly blockers: readonly string[];
  readonly canComplete: boolean;
}

export type MergePlan = MergeRefused | MergeReady;

export function mergePlan(value: unknown): MergePlan {
  const p = record(value);
  if (p['ok'] !== true) return { ok: false, reason: text(p['reason']) ?? '' };
  const state = text(p['state']);
  return {
    ok: true,
    state: state === 'ready' || state === 'resolving' || state === 'resolved' ? state : 'idle',
    branch: text(p['branch']) ?? '',
    target: text(p['target']) ?? '',
    repo: text(p['repo']) ?? '',
    runId: text(p['run_id']) ?? null,
    uncommitted: strings(p['worktree_uncommitted']),
    conflicts: strings(p['conflicts']),
    blockers: strings(p['blockers']),
    canComplete: p['can_complete'] === true,
  };
}

/** What `workspace.merge_prepare` and `workspace.merge_resolved` answer. */
export interface MergeStep {
  readonly state: string;
  /** Files with conflicts. */
  readonly files: readonly string[];
  /** Files that still hold conflict marks. */
  readonly remaining: readonly string[];
  /** Whether the conflicts were sent to the agent, and why not. */
  readonly sent: boolean;
  readonly why: string;
}

export function mergeStep(value: unknown): MergeStep {
  const p = record(value);
  const handoff = record(p['handoff']);
  return { state: text(p['state']) ?? '', files: strings(p['files']), remaining: strings(p['remaining']), sent: handoff['sent'] === true, why: text(handoff['why']) ?? '' };
}

/** What `workspace.merge_complete` answers. */
export interface Merged {
  readonly branch: string;
  readonly target: string;
  /** The first ten characters of the merge commit. */
  readonly commit: string;
}

export function merged(value: unknown, plan: MergeReady): Merged {
  const p = record(value);
  return { branch: text(p['branch']) ?? plan.branch, target: text(p['target']) ?? plan.target, commit: (text(p['commit']) ?? '').slice(0, 10) };
}

export interface PullRefused {
  readonly ok: false;
  readonly reason: string;
}

export interface PullReady {
  readonly ok: true;
  readonly title: string;
  readonly branch: string;
  /** The branch the pull request asks to be merged into. */
  readonly target: string;
  readonly remote: string;
  /** `owner/repo` on GitHub. */
  readonly repo: string;
  readonly uncommitted: readonly string[];
  /** The subjects of the commits, newest first. */
  readonly commits: readonly string[];
}

export type PullPlan = PullRefused | PullReady;

export function pullPlan(value: unknown): PullPlan {
  const p = record(value);
  if (p['ok'] !== true) return { ok: false, reason: text(p['reason']) ?? '' };
  const owner = text(p['owner']) ?? '';
  const repo = text(p['repo']) ?? '';
  return {
    ok: true,
    title: text(p['title']) ?? '',
    branch: text(p['branch']) ?? '',
    target: text(p['target']) ?? '',
    remote: text(p['remote']) ?? '',
    repo: owner && repo ? `${owner}/${repo}` : repo,
    uncommitted: strings(p['uncommitted']),
    commits: strings(p['commits']),
  };
}

/** What `workspace.pr_open` answers, as far as the screen shows it. */
export interface Opened {
  readonly url: string;
  readonly number: number;
  /** True when the branch had an open pull request already. */
  readonly reused: boolean;
}

export function opened(value: unknown): Opened {
  const p = record(value);
  return { url: text(p['url']) ?? '', number: number(p['number']) ?? 0, reused: p['reused'] === true };
}
