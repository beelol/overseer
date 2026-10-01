/**
 * What the tests of the review screens start from: a run in its worktree, the comparisons the
 * Mac offers for it, and hunks with the keys the daemon gives them. Tests only.
 */
import { act } from '@testing-library/react-native';
import type { ReactElement } from 'react';

import { review, type Hunk } from '@/model';
import type { Result, Run, State, Task, Workspace } from '@/protocol';
import { EMPTY_STATE, type TestApp } from '@/testing';

export const RUN = 'r1';
export const WORKSPACE = 'w1';
export const LATEST = 'aaaaaaaaaa1111111111';
export const TASK_START = 'bbbbbbbbbb2222222222';
export const MERGE_BASE = 'cccccccccc3333333333';
export const TIP = 'dddddddddd4444444444';

export const workspace: Workspace = { id: WORKSPACE, path: '/Users/owner/shop-wt', repo_root: '/Users/owner/shop', common_dir: '/Users/owner/shop/.git', kind: 'worktree', branch: 'overseer/fix-cart', initial_dirty: null, created_ms: 1 };
export const task: Task = { id: 't1', title: 'Fix the cart', prompt: 'Fix the cart total', repo_root: '/Users/owner/shop', workspace_id: WORKSPACE, created_ms: 1 };
export const run: Run = { id: RUN, task_id: 't1', harness: 'claude', workspace_id: WORKSPACE, status: 'completed', created_ms: 1, title: 'Fix the cart', capabilities: {}, process_generation: 1 };

export function stateWith(change: Partial<Run> = {}): State {
  return { ...EMPTY_STATE, tasks: [task], runs: [{ ...run, ...change }], workspaces: [workspace], turns: {} };
}

/** What `comparison.options` answers, for the target branch `branch`. */
export function comparisons(branch = 'main'): Result<'comparison.options'> {
  return {
    run_id: RUN,
    workspace,
    head: 'eeeeeeeeee5555555555',
    branch: 'overseer/fix-cart',
    branches: ['main', 'release', 'overseer/fix-cart'],
    // An agent's own worktree: its changes are the agent's alone.
    folder_edits: false,
    options: [
      { mode: 'latest_run', label: 'Latest run', base: LATEST, available: true, default: true, detail: 'run-start snapshot s-2' },
      { mode: 'task_start', label: 'Since task start', base: TASK_START, available: true, detail: 'task-start snapshot s-1' },
      { mode: 'fork', label: 'Original fork', available: false, detail: 'unknown: no fork commit was recorded' },
      { mode: 'branch_merge_base', branch, label: `Merge-base with ${branch} (PR-style)`, base: `${MERGE_BASE}-${branch}`, available: true, detail: `merge-base(${branch})` },
      { mode: 'branch_tip', branch, label: `Tip of ${branch} (direct)`, base: `${TIP}-${branch}`, available: true, detail: `${branch} at its tip` },
    ],
  };
}

/** A hunk as the daemon sends it, with the key the daemon gives it. */
export function hunk(path: string, at: number, removed: string[], added: string[], reviewed = false): Hunk {
  return { key: review.hunkKey(path, removed, added), base_start: at, base_lines: removed, modified_start: at, modified_lines: added, reviewed };
}

/** What `workspace.hunks` answers for a file shown as text. */
export function hunksOf(path: string, base: string, hunks: Hunk[]): Result<'workspace.hunks'> {
  return { workspace_id: WORKSPACE, path, base, shown: true, hunks, before: { exists: true, kind: 'text', lines: 40 }, now: { exists: true, kind: 'text', lines: 41 } };
}

/** What `workspace.hunks` answers for a file that cannot be shown. */
export function notShown(path: string, base: string, kind: string, why: string): Result<'workspace.hunks'> {
  return { workspace_id: WORKSPACE, path, base, shown: false, why, hunks: [], before: { exists: true, kind }, now: { exists: true, kind } };
}

/** A refusal as the connection library gives it: the Mac's code and words. */
export function refusal(code: string, message: string): Error {
  return Object.assign(new Error(message), { code, name: 'RequestError' });
}

/** Lets the time the review waits before asking again go by. */
export const wait = (ms: number): Promise<void> => new Promise((resolve) => setTimeout(resolve, ms));

/**
 * The session keeps a conversation nobody looks at for half a minute, on a timer nothing
 * clears; Jest would wait for it after the last test. Long timers no longer hold the process.
 * (Reported: the session should clear that timer when it stops.)
 */
export function letLongTimersGo(): void {
  const real = globalThis.setTimeout;
  const patient = ((handler: () => void, ms?: number, ...rest: unknown[]) => {
    const timer: unknown = real(handler, ms, ...rest);
    if ((ms ?? 0) >= 10_000 && typeof timer === 'object' && timer !== null && 'unref' in timer && typeof timer.unref === 'function') timer.unref();
    return timer;
  }) as unknown as typeof setTimeout;
  globalThis.setTimeout = Object.assign(patient, real);
}

/**
 * Draws a screen and lets its list finish its first frames: the list measures itself on the
 * frames after it was drawn, and a test would otherwise end in the middle of them.
 */
export async function draw(app: TestApp, ui: ReactElement): Promise<Awaited<ReturnType<TestApp['render']>>> {
  const drawn = await app.render(ui);
  await act(() => wait(50));
  await app.settle();
  return drawn;
}
