/**
 * What the review screens remember while the app runs, by connection: the last changed files
 * of a run, the hunks of the files that were looked at, and the comparisons the Mac offered.
 * A screen opens on what is remembered and asks the Mac again. Nothing here is stored on the
 * phone, and nothing is the only copy.
 */
import type { Change, Result } from '@/protocol';
import type { Session } from '@/session';

export type HunksResult = Result<'workspace.hunks'>;
export type OptionsResult = Result<'comparison.options'>;

/** What the owner chose to compare with. */
export interface Chosen {
  readonly mode: string;
  readonly branch: string | null;
}

export interface StoredChanges {
  readonly options: OptionsResult;
  readonly chosen: Chosen | undefined;
  readonly base: string | null;
  readonly changes: readonly Change[];
  readonly conflicted: readonly string[];
  /** When the Mac answered, in milliseconds since 1970. */
  readonly at: number;
}

/** The newest `max` values; the oldest leave first. */
export class Newest<V> {
  private readonly map = new Map<string, V>();

  constructor(private readonly max: number) {}

  get(key: string): V | undefined {
    return this.map.get(key);
  }

  set(key: string, value: V): void {
    this.map.delete(key);
    this.map.set(key, value);
    for (const oldest of this.map.keys()) {
      if (this.map.size <= this.max) break;
      this.map.delete(oldest);
    }
  }

  delete(key: string): void {
    this.map.delete(key);
  }
}

export interface ReviewMemory {
  /** By run. */
  readonly changes: Newest<StoredChanges>;
  /** By `hunksKey`. */
  readonly hunks: Newest<HunksResult>;
  /** By run: whatever the review keeps for one run (its marks). */
  readonly runs: Map<string, unknown>;
}

const memories = new WeakMap<Session, ReviewMemory>();

export function memoryOf(session: Session): ReviewMemory {
  let memory = memories.get(session);
  if (!memory) {
    memory = { changes: new Newest(20), hunks: new Newest(200), runs: new Map() };
    memories.set(session, memory);
  }
  return memory;
}

export const hunksKey = (workspaceId: string, base: string, path: string): string => `${workspaceId}\n${base}\n${path}`;
