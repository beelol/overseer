import { isActive, store, type OutboxEntry, type Run } from '@/model';
import type { Capabilities, SyncStore } from '@/platform';
import type { Params } from '@/protocol';
import type { Session } from '@/session';

export type FollowUp = Params<'run.follow_up'>;

interface HeldMessage {
  readonly requestId: string;
  readonly params: FollowUp;
  readonly createdAt: number;
}

type Kept = SyncStore<{ messages: string }>;
type Listener = () => void;

/**
 * True while the daemon would refuse a message to this agent: it takes one when the turn has
 * ended (a program, which reads its input while it runs, takes one at any time).
 */
export function working(run: Pick<Run, 'status' | 'harness'> | undefined): boolean {
  return run !== undefined && isActive(run.status) && run.harness !== 'generic';
}

/**
 * Messages written while the agent works. Each is kept on the phone, shown as queued, and sent
 * once when the turn has ended, in the order they were written and one turn at a time. A
 * message keeps the request id it was given when it was written, so its bubble is one row from
 * the moment it was typed until it is the turn's own.
 *
 * The daemon has no queue of its own (VS Code keeps one too, in its extension). This belongs
 * beside the connection, in `src/session`, so that a message is also sent while no conversation
 * is on screen; see the report of the conversation screen.
 */
export class Held {
  private items: readonly HeldMessage[];
  private shown: readonly OutboxEntry[];
  private readonly listeners = new Set<Listener>();
  /** By run: the message sent last, and how many turns the run had then. */
  private readonly sent = new Map<string, { readonly requestId: string; readonly turns: number }>();
  private stopWatching: (() => void) | null = null;
  private flushing = false;
  private again = false;

  constructor(
    private readonly session: Session,
    private readonly kept: Kept | null,
    private readonly now: () => number,
  ) {
    this.items = read(kept);
    this.shown = this.items.map(asEntry);
    this.watch();
  }

  /** The messages that wait, in the shape of the outbox: `pending.withPending` takes them as they are. */
  getSnapshot = (): readonly OutboxEntry[] => this.shown;

  subscribe = (listener: Listener): (() => void) => {
    this.listeners.add(listener);
    return () => void this.listeners.delete(listener);
  };

  /** True for a message that still waits on the phone and can be taken back. */
  holds(requestId: string): boolean {
    return this.items.some((item) => item.requestId === requestId);
  }

  /** Sends the message, now or when the agent's turn has ended. */
  send(requestId: string, params: FollowUp): void {
    this.replace([...this.items, { requestId, params, createdAt: this.now() }]);
    this.flush();
  }

  /** Takes back a message that still waits. */
  cancel(requestId: string): void {
    if (this.holds(requestId)) this.replace(this.items.filter((item) => item.requestId !== requestId));
  }

  private readonly flush = (): void => {
    if (this.flushing) {
      this.again = true;
      return;
    }
    this.flushing = true;
    try {
      do {
        this.again = false;
        this.pass();
      } while (this.again);
    } finally {
      this.flushing = false;
    }
  };

  private pass(): void {
    if (this.items.length === 0) return;
    const { state, outbox } = this.session.getSnapshot();
    const gone = new Set<string>();
    const started = new Set<string>();
    for (const item of this.items) {
      const runId = item.params.run_id;
      if (started.has(runId)) continue;
      const run = store.run(state, runId);
      if (run === undefined) continue;
      const turns = store.turnsOf(state, runId).length;
      const last = this.sent.get(runId);
      if (last !== undefined) {
        const failed = outbox.some((entry) => entry.requestId === last.requestId && entry.state === 'failed');
        // Its turn has to have started and ended before the next message may follow it.
        if (!failed && turns <= last.turns) continue;
        if (!failed && working(run)) continue;
        this.sent.delete(runId);
      }
      if (working(run)) continue;
      started.add(runId);
      gone.add(item.requestId);
      this.sent.set(runId, { requestId: item.requestId, turns });
      this.session.request('run.follow_up', item.params, { requestId: item.requestId }).catch(() => {
        // Not taken: no turn will come of it, and the next message need not wait for one.
        if (this.sent.get(runId)?.requestId === item.requestId) {
          this.sent.delete(runId);
          this.flush();
        }
      });
    }
    if (gone.size > 0) this.replace(this.items.filter((item) => !gone.has(item.requestId)));
  }

  private replace(items: readonly HeldMessage[]): void {
    this.items = items;
    this.shown = items.map(asEntry);
    try {
      if (items.length === 0) this.kept?.delete('messages');
      else this.kept?.set('messages', JSON.stringify(items));
    } catch {
      // Not stored: the message is still sent while the app stays open.
    }
    this.watch();
    for (const listener of [...this.listeners]) listener();
  }

  /** Follows the agents for as long as something waits. */
  private watch(): void {
    if (this.items.length > 0 && this.stopWatching === null) this.stopWatching = this.session.subscribe(this.flush);
    else if (this.items.length === 0 && this.stopWatching !== null) {
      this.stopWatching();
      this.stopWatching = null;
    }
  }
}

function asEntry(item: HeldMessage): OutboxEntry {
  return { requestId: item.requestId, method: 'run.follow_up', params: item.params, state: 'queued', createdAt: item.createdAt };
}

function read(kept: Kept | null): readonly HeldMessage[] {
  try {
    const text = kept?.get('messages');
    if (typeof text !== 'string') return [];
    const list: unknown = JSON.parse(text);
    if (!Array.isArray(list)) return [];
    return list.filter((item): item is HeldMessage => {
      const m = item as Partial<HeldMessage> | null;
      return !!m && typeof m.requestId === 'string' && typeof m.createdAt === 'number' && typeof m.params === 'object' && m.params !== null && typeof m.params.run_id === 'string' && typeof m.params.prompt === 'string';
    });
  } catch {
    return [];
  }
}

const bySession = new WeakMap<Session, Held>();

/** The one keeper of waiting messages of a connection. */
export function heldFor(session: Session, capabilities: Pick<Capabilities, 'keyValue'>, now: () => number = Date.now): Held {
  const known = bySession.get(session);
  if (known) return known;
  let kept: Kept | null = null;
  try {
    kept = capabilities.keyValue.scope<{ messages: string }>('held');
  } catch {
    kept = null;
  }
  const held = new Held(session, kept, now);
  bySession.set(session, held);
  return held;
}
