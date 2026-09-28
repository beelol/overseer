/**
 * The hunks marked reviewed for a run. They are the Mac's: loaded once with `review.marks`,
 * asked again when a mark was made elsewhere, and changed here at once while the request is on
 * its way.
 *
 * The session's store keeps marks current from events once it holds them (`store.loadMarks`),
 * but nothing loads them into it yet. Until the session does, they are kept here, one copy for
 * the screens of a run; when the store holds them, the store's are used.
 */
import { useCallback, useEffect, useMemo, useSyncExternalStore } from 'react';

import { review, store, type Hunk } from '@/model';
import { useSession, useSessionValue, type Session } from '@/session';

import { useActivity, useRefreshOn } from './activity';
import { memoryOf } from './data';

export interface MarksSnapshot {
  /** The keys marked reviewed, with what was changed here and is still on its way. */
  readonly keys: ReadonlySet<string>;
  /** False until the Mac said which hunks are marked. */
  readonly loaded: boolean;
}

const NONE: ReadonlySet<string> = new Set();
/** What `store.marksOf` returns for a run whose marks the store does not hold. */
const NOT_HELD = store.marksOf(store.EMPTY, '');

export class RunMarks {
  private known: ReadonlySet<string> = NONE;
  private loaded = false;
  /** Changes made here and not answered yet: what each key should become, and how many requests wait. */
  private readonly wanted = new Map<string, { readonly reviewed: boolean; readonly waiting: number }>();
  private current: MarksSnapshot = { keys: NONE, loaded: false };
  private readonly listeners = new Set<() => void>();
  private asking: Promise<void> | null = null;
  private again = false;

  constructor(private readonly ask: () => Promise<readonly string[]>) {}

  getSnapshot = (): MarksSnapshot => this.current;

  subscribe = (listener: () => void): (() => void) => {
    this.listeners.add(listener);
    return () => void this.listeners.delete(listener);
  };

  get isLoaded(): boolean {
    return this.loaded;
  }

  /** Asks the Mac once. Later calls do nothing. */
  ensure(): Promise<void> {
    if (this.loaded) return Promise.resolve();
    return this.asking ?? this.load();
  }

  /** Asks the Mac. A call while an answer is on its way asks once more after it. */
  load(): Promise<void> {
    if (this.asking) {
      this.again = true;
      return this.asking;
    }
    const asking = (async () => {
      try {
        do {
          this.again = false;
          this.known = new Set(await this.ask());
          this.loaded = true;
          this.publish();
        } while (this.again);
      } catch {
        // Not reachable now: what is known stays, and the next news asks again.
      } finally {
        this.asking = null;
      }
    })();
    this.asking = asking;
    return asking;
  }

  /** The session's store holds this run's marks: they are the Mac's word. */
  adopt(keys: readonly string[]): void {
    this.known = new Set(keys);
    this.loaded = true;
    this.publish();
  }

  /** Shows the change at once and sends it. Rejects with the Mac's refusal, the change taken back. */
  async change(key: string, reviewed: boolean, send: () => Promise<unknown>): Promise<void> {
    this.wanted.set(key, { reviewed, waiting: (this.wanted.get(key)?.waiting ?? 0) + 1 });
    this.publish();
    try {
      await send();
      this.known = withKey(this.known, key, reviewed);
    } finally {
      const now = this.wanted.get(key);
      if (now && now.waiting > 1) this.wanted.set(key, { ...now, waiting: now.waiting - 1 });
      else this.wanted.delete(key);
      this.publish();
    }
  }

  /**
   * What the Mac said about these hunks when it sent them (`workspace.hunks` marks each). It
   * counts until the marks themselves are loaded; from then on they are kept current by news.
   */
  learn(hunks: readonly Hunk[]): void {
    if (this.loaded) return;
    let known = this.known;
    for (const hunk of hunks) known = withKey(known, hunk.key, hunk.reviewed);
    if (known === this.known) return;
    this.known = known;
    this.publish();
  }

  /** A hunk was put back: it has no mark any more. */
  forget(key: string): void {
    if (!this.known.has(key)) return;
    this.known = withKey(this.known, key, false);
    this.publish();
  }

  private publish(): void {
    let keys = this.known;
    for (const [key, wish] of this.wanted) keys = withKey(keys, key, wish.reviewed);
    if (same(keys, this.current.keys) && this.loaded === this.current.loaded) return;
    this.current = { keys, loaded: this.loaded };
    for (const listener of [...this.listeners]) listener();
  }
}

function withKey(keys: ReadonlySet<string>, key: string, reviewed: boolean): ReadonlySet<string> {
  if (keys.has(key) === reviewed) return keys;
  const next = new Set(keys);
  if (reviewed) next.add(key);
  else next.delete(key);
  return next;
}

function same(a: ReadonlySet<string>, b: ReadonlySet<string>): boolean {
  if (a === b) return true;
  if (a.size !== b.size) return false;
  for (const key of a) if (!b.has(key)) return false;
  return true;
}

function marksFor(session: Session, runId: string): RunMarks {
  const runs = memoryOf(session).runs;
  const found = runs.get(runId);
  if (found instanceof RunMarks) return found;
  const marks = new RunMarks(async () => (await session.request('review.marks', { run_id: runId })).keys);
  runs.set(runId, marks);
  return marks;
}

export interface Marks extends MarksSnapshot {
  /** Marks a hunk reviewed: shown at once, sent once. */
  accept(path: string, hunk: Hunk): Promise<void>;
  /** Takes the mark away: shown at once, sent once. */
  unaccept(key: string): Promise<void>;
  /** A hunk was put back. */
  forget(key: string): void;
  /** Hunks arrived from the Mac, each saying whether it is marked. */
  learn(hunks: readonly Hunk[]): void;
}

/** The reviewed marks of a run, live. */
export function useMarks(runId: string): Marks {
  const session = useSession();
  const marks = useMemo(() => marksFor(session, runId), [session, runId]);
  const held = useSessionValue((s) => store.marksOf(s.state, runId));
  const online = useSessionValue((s) => s.connection === 'online');
  const snapshot = useSyncExternalStore(marks.subscribe, marks.getSnapshot, marks.getSnapshot);
  const activity = useActivity(runId);
  const inStore = held !== NOT_HELD;

  useEffect(() => {
    if (inStore) marks.adopt(held.map((mark) => mark.key));
  }, [inStore, held, marks]);

  useEffect(() => {
    if (!inStore && online && runId) void marks.ensure();
  }, [inStore, online, runId, marks]);

  // A mark made on the Mac, or a hunk put back there.
  const reload = useCallback(() => {
    if (!inStore) void marks.load();
  }, [inStore, marks]);
  useRefreshOn(`${activity.marks}:${activity.rejects}`, activity.ready, reload);

  const accept = useCallback(
    (path: string, hunk: Hunk) => {
      const params = review.acceptParams(path, hunk);
      return marks.change(hunk.key, true, () => session.request('review.accept', { ...params, run_id: runId, modified_lines: [...params.modified_lines], base_lines: [...params.base_lines] }));
    },
    [marks, session, runId],
  );
  const unaccept = useCallback((key: string) => marks.change(key, false, () => session.request('review.unaccept', { run_id: runId, key })), [marks, session, runId]);
  const forget = useCallback((key: string) => marks.forget(key), [marks]);
  const learn = useCallback((hunks: readonly Hunk[]) => marks.learn(hunks), [marks]);

  return useMemo(() => ({ keys: snapshot.keys, loaded: snapshot.loaded, accept, unaccept, forget, learn }), [snapshot, accept, unaccept, forget, learn]);
}
