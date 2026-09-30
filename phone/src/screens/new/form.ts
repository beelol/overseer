import { store, text } from '@/model';
import type { Params } from '@/protocol';

import type { HarnessOptions } from './options';
import { WORDS } from './words';

type PhoneState = store.PhoneState;

export type Where = 'worktree' | 'current';

export interface RepoChoice {
  /** The repository's folder on the Mac: what `task.create` takes. */
  readonly root: string;
  readonly name: string;
  readonly branch: string | null;
}

export interface HarnessChoice {
  readonly harness: string;
  /** "Claude Code", "Codex", "OpenCode". */
  readonly label: string;
  readonly version: string | null;
  readonly options: HarnessOptions;
}

export interface AccountChoice {
  readonly id: string;
  readonly name: string;
  /** The harnesses this account signs in. */
  readonly harnesses: readonly string[];
  /** `null` while the Mac has not said. */
  readonly signedIn: boolean | null;
  readonly plan: string | null;
  /** The email with its local part shortened ("bil…@testbox.com", AC-235), once the Mac has read it. */
  readonly email?: string | null;
}

export interface Choices {
  readonly repos: readonly RepoChoice[];
  readonly harnesses: readonly HarnessChoice[];
  readonly accounts: readonly AccountChoice[];
}

/** What the owner chose, as it is kept for the next time. Empty means "not chosen" or "default". */
export type Form = {
  repo: string;
  harness: string;
  account: string;
  model: string;
  effort: string;
  mode: string;
  where: Where;
};

export const NOTHING_CHOSEN: Form = Object.freeze({ repo: '', harness: '', account: '', model: '', effort: '', mode: '', where: 'worktree' });

/** The form as it stands: every choice checked against what the Mac offers now. */
export interface Chosen {
  readonly repo: RepoChoice | null;
  readonly harness: HarnessChoice | null;
  readonly account: AccountChoice | null;
  /** The accounts of the chosen agent, signed-in ones first. */
  readonly accounts: readonly AccountChoice[];
  readonly model: string;
  readonly effort: string;
  readonly mode: string;
  readonly where: Where;
}

/** Signed-in accounts first; among equals the Mac's order holds. */
export function accountsFor(harness: string, accounts: readonly AccountChoice[]): readonly AccountChoice[] {
  const rank = (account: AccountChoice): number => (account.signedIn === true ? 0 : account.signedIn === null ? 1 : 2);
  return accounts
    .filter((account) => account.harnesses.includes(harness))
    .map((account, at) => ({ account, at }))
    .sort((a, b) => rank(a.account) - rank(b.account) || a.at - b.at)
    .map((entry) => entry.account);
}

/** The agent started last, anywhere: what a phone that never started one begins with. */
function lastStarted(state: PhoneState): { readonly harness: string; readonly account: string; readonly repo: string } | null {
  const runs = store.rows(state.runs);
  for (let at = runs.length - 1; at >= 0; at--) {
    const run = runs[at];
    if (!run || run.parent_run_id) continue;
    return { harness: run.harness, account: run.profile_id ?? '', repo: store.task(state, run.task_id)?.repo_root ?? '' };
  }
  return null;
}

/**
 * What is chosen: the owner's choice where the Mac still offers it, else what was chosen the
 * last time, else the agent started last, else the first there is.
 */
export function choose(form: Form, choices: Choices, state: PhoneState): Chosen {
  const last = lastStarted(state);
  const repo = choices.repos.find((r) => r.root === form.repo) ?? choices.repos.find((r) => r.root === last?.repo) ?? choices.repos[0] ?? null;
  const harness = choices.harnesses.find((h) => h.harness === form.harness) ?? choices.harnesses.find((h) => h.harness === last?.harness) ?? choices.harnesses[0] ?? null;
  const accounts = harness ? accountsFor(harness.harness, choices.accounts) : [];
  const account = accounts.find((a) => a.id === form.account) ?? (form.account ? undefined : accounts.find((a) => a.id === last?.account && a.signedIn !== false)) ?? accounts[0] ?? null;
  const options = harness?.options;
  return {
    repo,
    harness,
    account,
    accounts,
    model: options?.models ? form.model : '',
    effort: options?.efforts?.includes(form.effort) ? form.effort : '',
    mode: options?.modes?.includes(form.mode) ? form.mode : '',
    where: form.where === 'current' ? 'current' : 'worktree',
  };
}

/** What is missing, in one sentence; `null` when the agent can start. */
export function missing(chosen: Chosen, task: string): string | null {
  if (!chosen.repo) return WORDS.missing.repository;
  if (!chosen.harness) return WORDS.missing.agent;
  if (!chosen.account) return WORDS.missing.account;
  if (chosen.account.signedIn === false) return WORDS.missing.signedIn;
  if (!task.trim()) return WORDS.missing.task;
  return null;
}

/**
 * A short title from the first line of the task, as VS Code makes it
 * (extension/src/task-launcher.js `titleFor`). The whole task stays on the agent.
 */
export function titleFor(task: string): string {
  const line = task
    .split('\n')
    .map((l) => l.trim())
    .find(Boolean);
  if (!line) return '';
  const clean = line.replace(/\s+/g, ' ');
  if (clean.length <= 60) return clean;
  const cut = clean.slice(0, 60);
  const space = cut.lastIndexOf(' ');
  return (space > 30 ? cut.slice(0, space) : cut).replace(/[,.;:]$/, '') + '…';
}

/** The request that starts the agent: only what the protocol lets a phone send. */
export function paramsOf(chosen: Chosen, task: string): Params<'task.create'> | null {
  if (!chosen.repo || !chosen.harness || !chosen.account) return null;
  return {
    repo: chosen.repo.root,
    harness: chosen.harness.harness,
    prompt: task,
    title: titleFor(task),
    workspace_mode: chosen.where,
    profile_id: chosen.account.id,
    ...(chosen.model ? { model: chosen.model } : {}),
    ...(chosen.effort ? { effort: chosen.effort } : {}),
    ...(chosen.mode ? { permission_mode: chosen.mode } : {}),
  };
}

/** What is kept for the next time. */
export function formOf(chosen: Chosen): Form {
  return { repo: chosen.repo?.root ?? '', harness: chosen.harness?.harness ?? '', account: chosen.account?.id ?? '', model: chosen.model, effort: chosen.effort, mode: chosen.mode, where: chosen.where };
}

/** "Claude Code" for `claude`; a harness VS Code has no name for keeps its own. */
export function harnessLabel(harness: string): string {
  return text.TEXT.harness[harness] ?? harness;
}

/** "Ask first" for `manual`; a mode VS Code has no name for keeps its own. */
export function modeLabel(mode: string): string {
  return WORDS.mode[mode] ?? mode;
}
