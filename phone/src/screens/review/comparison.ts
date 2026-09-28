import { useMemo } from 'react';

import { review } from '@/model';
import { useCapabilities, type SyncStore } from '@/platform';
import type { Session } from '@/session';

import type { Chosen, OptionsResult } from './data';

/** What the review keeps on the phone: the comparison chosen for each run, and whether long lines wrap. */
export type ReviewStore = {
  comparisons: Record<string, Chosen>;
  wrap: boolean;
};

/** As many runs keep their comparison as in VS Code. */
const KEPT = 200;

export function useReviewStore(): SyncStore<ReviewStore> {
  const { keyValue } = useCapabilities();
  return useMemo(() => keyValue.scope<ReviewStore>('review'), [keyValue]);
}

export function chosenFor(kept: SyncStore<ReviewStore>, runId: string): Chosen | undefined {
  const found = kept.get('comparisons')?.[runId];
  return found && typeof found.mode === 'string' ? { mode: found.mode, branch: found.branch ?? null } : undefined;
}

export function keepChosen(kept: SyncStore<ReviewStore>, runId: string, chosen: Chosen): void {
  const others = Object.entries(kept.get('comparisons') ?? {}).filter(([id]) => id !== runId);
  kept.set('comparisons', Object.fromEntries([...others, [runId, chosen] as const].slice(-KEPT)));
}

/** The name of a comparison in a route: the model's key, `mode:branch`. */
export const keyOf = (chosen: Chosen): string => `${chosen.mode}:${chosen.branch ?? ''}`;

/** The comparison a route names, or nothing when it names none. */
export function fromKey(key: string | undefined): Chosen | undefined {
  if (!key) return undefined;
  const at = key.lastIndexOf(':');
  if (at < 0) return { mode: key, branch: null };
  if (at === 0) return undefined;
  return { mode: key.slice(0, at), branch: key.slice(at + 1) || null };
}

export interface Resolved {
  readonly options: OptionsResult;
  readonly choices: readonly review.ComparisonChoice[];
  /** The comparison in use, or `null` when none of them is available. */
  readonly current: review.ComparisonChoice | null;
  /** Why there is none. */
  readonly why: string;
}

export function resolve(options: OptionsResult, chosen: Chosen | undefined): Resolved {
  const choices = review.comparisonChoices(options.options, chosen);
  const current = choices.find((choice) => choice.selected && choice.available && choice.base) ?? null;
  const wanted = options.options.find((o) => (chosen ? o.mode === chosen.mode : o.default)) ?? options.options[0];
  return { options, choices, current, why: current ? '' : (wanted?.detail ?? '') };
}

/** The comparisons the Mac offers for a run, with the one in use. */
export async function askComparison(session: Session, runId: string, chosen: Chosen | undefined): Promise<Resolved> {
  const options = await session.request('comparison.options', { run_id: runId, branch: chosen?.branch ?? null });
  return resolve(options, chosen);
}

/** A test id for a choice: `latest_run`, `branch_merge_base.main`. */
export const choiceId = (choice: { readonly mode: string; readonly branch: string | null }): string => (choice.branch ? `${choice.mode}.${choice.branch}` : choice.mode);
