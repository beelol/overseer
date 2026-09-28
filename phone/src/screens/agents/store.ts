import type { SyncStore } from '@/platform';

import { useStore } from './stored';

/** What the agents list keeps on the phone between launches (`keyValue.scope('agents')`). */
export type AgentsStore = {
  /** When the owner last opened each agent here, by run id, in milliseconds since 1970. */
  seen: Record<string, number>;
  /** The agents the owner pinned, by run id. */
  pinned: string[];
  /** The headings the owner folded, by row id. */
  collapsed: string[];
};

export function useAgentsStore(): SyncStore<AgentsStore> {
  return useStore<AgentsStore>('agents');
}

/** How many opened agents are remembered: the newest are kept. */
export const SEEN_KEPT = 800;

/** `seen` with `runId` opened at `now`, and only the newest entries kept. */
export function withSeen(seen: Readonly<Record<string, number>>, runId: string, now: number): Record<string, number> {
  const next = { ...seen, [runId]: now };
  const ids = Object.keys(next);
  if (ids.length <= SEEN_KEPT) return next;
  const newest = ids.sort((a, b) => (next[b] ?? 0) - (next[a] ?? 0)).slice(0, SEEN_KEPT);
  return Object.fromEntries(newest.map((id) => [id, next[id] ?? 0]));
}

/** `list` with `id` in it or not. The same order, no id twice. */
export function toggled(list: readonly string[], id: string, on: boolean): string[] {
  const rest = list.filter((x) => x !== id);
  return on ? [...rest, id] : rest;
}
