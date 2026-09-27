import type { ConnectionState, OutboxEntry } from '@/core';
import { conversation as conversations, store, type DaemonEvent } from '@/model';
import type { SyncStore } from '@/platform';
import type { PhoneMethod, Params, Result, State } from '@/protocol';

import type { Connection, ConversationSnapshot, NotificationSwitches, Scope, SessionSnapshot } from './types';

/** What the session keeps on the phone between launches, besides what the connection keeps. */
export type SessionCache = {
  /** The daemon's state as the phone last knew it, in the daemon's own shape, as JSON. */
  state: string;
  /** When that state was last confirmed by the Mac. */
  stateAt: number;
};

export interface SessionDeps {
  readonly connection: Connection;
  readonly cache: SyncStore<SessionCache>;
  readonly now: () => number;
  /**
   * Runs `callback` before the next frame is drawn and returns a way to cancel it.
   * Events are applied in one batch per frame, so a fast stream never draws more often than
   * the screen can show.
   */
  readonly nextFrame: (callback: () => void) => () => void;
  /** The least time between two writes of the cache, in milliseconds. */
  readonly cacheEveryMs?: number;
  readonly log?: (message: string) => void;
}

type PhoneState = store.PhoneState;
type Listener = () => void;

const HISTORY_PAGE = 5000;
const HISTORY_PAGES = 20;

interface OpenConversation {
  snapshot: ConversationSnapshot;
  readonly listeners: Set<Listener>;
  /** Events that arrived while the history was still loading. */
  waiting: DaemonEvent[];
  generation: number;
}

/**
 * The app's one connection to Overseer: the phone's copy of the daemon's state, kept current
 * from events, and the conversations that are open. Screens read snapshots and subscribe; they
 * never talk to the connection themselves except to send a request.
 */
export class Session {
  private snapshot: SessionSnapshot;
  private readonly listeners = new Set<Listener>();
  private readonly open = new Map<string, OpenConversation>();
  private pending: DaemonEvent[] = [];
  private cancelFrame: (() => void) | null = null;
  private readonly subscriptions: (() => void)[] = [];
  private cacheWrittenAt = 0;
  private cacheTimer: ReturnType<typeof setTimeout> | null = null;
  private loadGeneration = 0;
  private started = false;
  private switches: NotificationSwitches = ALL_ON;

  constructor(private readonly deps: SessionDeps) {
    const cached = readCache(deps.cache);
    this.snapshot = {
      ready: false,
      connection: deps.connection.state,
      paired: false,
      gateway: null,
      scope: null,
      lastContact: null,
      state: cached ? store.load(cached.state) : store.EMPTY,
      stateAt: cached?.stateAt ?? null,
      fromCache: cached !== null,
      historyLost: false,
      outbox: [],
      macNotifications: true,
      notifications: ALL_ON,
    };
  }

  getSnapshot = (): SessionSnapshot => this.snapshot;

  subscribe = (listener: Listener): (() => void) => {
    this.listeners.add(listener);
    return () => void this.listeners.delete(listener);
  };

  /** Reads what the phone stored and connects. Resolves when the stored pairing has been read. */
  async start(): Promise<void> {
    if (this.started) return;
    this.started = true;
    const c = this.deps.connection;
    this.subscriptions.push(
      c.on('state', (state) => this.onConnection(state)),
      c.on('event', (event) => this.onEvent(event)),
      c.on('truncated', () => this.onTruncated()),
      c.on('outbox', () => this.update({ outbox: c.outbox() })),
      c.on('paired', () => this.readConnection()),
      c.on('forgotten', () => this.onForgotten()),
    );
    await c.start();
    this.readConnection();
    this.update({ ready: true });
  }

  async stop(): Promise<void> {
    for (const off of this.subscriptions.splice(0)) off();
    this.cancelFrame?.();
    this.cancelFrame = null;
    this.flush();
    this.writeCache(true);
    this.started = false;
    await this.deps.connection.stop();
  }

  /** The app came to the front, or the network changed: try at once. */
  wake(): void {
    this.deps.connection.wake();
  }

  /** The app left the screen: what is known is stored now. */
  background(): void {
    this.flush();
    this.writeCache(true);
  }

  async pair(code: string, deviceName: string, platform: string): Promise<void> {
    await this.deps.connection.pair(code, deviceName, platform);
    this.readConnection();
  }

  async forget(): Promise<void> {
    await this.deps.connection.forget();
  }

  request<M extends PhoneMethod>(method: M, params: Params<M>, options?: { timeoutMs?: number; requestId?: string }): Promise<Result<M>> {
    return this.deps.connection.request(method, params, options);
  }

  dismiss(requestId: string): void {
    this.deps.connection.dismiss(requestId);
    this.update({ outbox: this.deps.connection.outbox() });
  }

  /**
   * This phone's notification switches, as the Mac last told them. Every kind is on until the
   * Mac said otherwise.
   */
  notificationSettings(): NotificationSwitches {
    return this.switches;
  }

  /** Changes this phone's switches on the Mac. What the Mac answers is what holds. */
  async setNotifications(change: { enabled?: boolean; show_text?: boolean; kinds?: Partial<NotificationSwitches['kinds']> }): Promise<NotificationSwitches> {
    const before = this.switches;
    // Shown at once; put back if the Mac refused.
    this.setSwitches({ ...before, ...(change.enabled === undefined ? {} : { enabled: change.enabled }), ...(change.show_text === undefined ? {} : { show_text: change.show_text }), kinds: { ...before.kinds, ...change.kinds } });
    try {
      const answered = await this.request('device.notifications', change);
      this.setSwitches(switchesOf(answered));
    } catch (error) {
      this.setSwitches(before);
      throw error;
    }
    return this.switches;
  }

  private setSwitches(next: NotificationSwitches): void {
    this.switches = next;
    this.update({ notifications: next });
  }

  /** Loads the daemon's state again. The screens' pull to refresh. */
  async reload(): Promise<void> {
    await this.loadState();
  }

  // ------------------------------------------------------------------ conversations

  conversation(runId: string): { getSnapshot: () => ConversationSnapshot; subscribe: (listener: Listener) => () => void } {
    return {
      getSnapshot: () => this.ensure(runId).snapshot,
      subscribe: (listener) => {
        const entry = this.ensure(runId);
        entry.listeners.add(listener);
        return () => {
          entry.listeners.delete(listener);
          // Kept for a moment: going back and forth between two agents does not load twice.
          if (entry.listeners.size === 0) setTimeout(() => entry.listeners.size === 0 && this.open.get(runId) === entry && this.open.delete(runId), 30_000);
        };
      },
    };
  }

  private ensure(runId: string): OpenConversation {
    const found = this.open.get(runId);
    if (found) return found;
    const home = homeOf(this.snapshot.state);
    const run = store.run(this.snapshot.state, runId);
    let built = conversations.create({ rootId: runId, home });
    if (run) built = conversations.setRun(built, run, store.descendantsOf(this.snapshot.state, runId)).conversation;
    const entry: OpenConversation = { snapshot: { runId, conversation: built, loading: true, error: null, truncated: false }, listeners: new Set(), waiting: [], generation: 0 };
    this.open.set(runId, entry);
    void this.loadHistory(runId, entry);
    return entry;
  }

  private async loadHistory(runId: string, entry: OpenConversation): Promise<void> {
    const generation = ++entry.generation;
    try {
      const ids = [runId, ...store.descendantsOf(this.snapshot.state, runId).map((r) => r.id)];
      const pages = await Promise.all(ids.map((id) => this.history(id)));
      if (this.open.get(runId) !== entry || entry.generation !== generation) return;
      const events = pages.flat().sort((a, b) => a.seq - b.seq);
      const home = homeOf(this.snapshot.state);
      const run = store.run(this.snapshot.state, runId);
      let next = conversations.build({ rootId: runId, home }, events, run, run ? store.descendantsOf(this.snapshot.state, runId) : []);
      // What arrived live while the history was on its way; events seen before change nothing.
      if (entry.waiting.length > 0) next = conversations.appendAll(next, entry.waiting).conversation;
      entry.waiting = [];
      this.setConversation(entry, { conversation: next, loading: false, error: null, truncated: events.some((e) => e.kind === 'retention') });
    } catch (error) {
      if (this.open.get(runId) !== entry || entry.generation !== generation) return;
      this.setConversation(entry, { loading: false, error: error instanceof Error ? error.message : String(error) });
    }
  }

  private async history(runId: string): Promise<DaemonEvent[]> {
    const out: DaemonEvent[] = [];
    let after = 0;
    for (let page = 0; page < HISTORY_PAGES; page++) {
      const list = await this.request('events.list', { run_id: runId, after, limit: HISTORY_PAGE });
      out.push(...list.events);
      const last = list.events[list.events.length - 1];
      if (list.events.length < HISTORY_PAGE || !last) break;
      after = last.seq;
    }
    return out;
  }

  private setConversation(entry: OpenConversation, change: Partial<ConversationSnapshot>): void {
    entry.snapshot = { ...entry.snapshot, ...change };
    for (const listener of [...entry.listeners]) listener();
  }

  // ------------------------------------------------------------------ the connection's events

  private readConnection(): void {
    const c = this.deps.connection;
    const hello = c.hello;
    const device = hello && typeof hello['device'] === 'object' && hello['device'] !== null ? (hello['device'] as Record<string, unknown>) : null;
    const scope = device?.['scope'] === 'watch' ? 'watch' : device?.['scope'] === 'full' ? 'full' : (c.gateway?.scope as Scope | undefined) ?? null;
    if (hello && hello['notifications'] !== undefined) this.switches = switchesOf(hello['notifications']);
    this.update({
      notifications: this.switches,
      connection: c.state,
      paired: c.gateway !== null,
      gateway: c.gateway,
      scope,
      lastContact: c.lastContact,
      outbox: c.outbox(),
      macNotifications: hello?.['mac_notifications'] !== false,
    });
  }

  private onConnection(state: ConnectionState): void {
    this.readConnection();
    if (state === 'online') void this.loadState();
    else this.writeCache(false);
  }

  private onForgotten(): void {
    this.pending = [];
    this.open.clear();
    this.deps.cache.delete('state');
    this.deps.cache.delete('stateAt');
    this.update({ state: store.EMPTY, stateAt: null, fromCache: false, historyLost: false, paired: false, gateway: null, scope: null, outbox: [] });
  }

  private onTruncated(): void {
    this.update({ historyLost: true });
    void this.loadState();
    for (const [runId, entry] of this.open) void this.loadHistory(runId, entry);
  }

  /** The daemon's state, asked of the Mac. What was shown from the cache is replaced by it. */
  private async loadState(): Promise<void> {
    const generation = ++this.loadGeneration;
    try {
      const loaded = await this.request('state', {});
      if (generation !== this.loadGeneration) return;
      // Events that arrived while the state was on its way are in `pending` and are applied
      // after it; those the state already holds change nothing.
      this.flushInto(store.load(loaded));
      this.update({ fromCache: false, stateAt: this.deps.now(), lastContact: this.deps.connection.lastContact });
      this.writeCache(true);
      for (const entry of this.open.values()) this.refreshRun(entry);
    } catch (error) {
      this.deps.log?.(`the state could not be loaded: ${error instanceof Error ? error.message : String(error)}`);
    }
  }

  private onEvent(event: DaemonEvent): void {
    this.ownEvent(event);
    this.pending.push(event);
    if (this.cancelFrame === null) {
      this.cancelFrame = this.deps.nextFrame(() => {
        this.cancelFrame = null;
        this.flush();
      });
    }
  }

  /**
   * What the Mac changed about this phone while it is connected: what it may do. The controls
   * follow at once; the Mac enforces the same whatever the app shows.
   */
  private ownEvent(event: DaemonEvent): void {
    if (event.kind !== 'device_scope') return;
    const payload = (event.payload !== null && typeof event.payload === 'object' ? event.payload : {}) as Record<string, unknown>;
    const mine = this.deps.connection.gateway?.deviceId;
    if (!mine || payload['device'] !== mine) return;
    if (payload['scope'] === 'watch' || payload['scope'] === 'full') this.update({ scope: payload['scope'] });
  }

  /** Applies everything that arrived since the last frame, as one change. */
  private flush(): void {
    if (this.pending.length === 0) return;
    this.flushInto(this.snapshot.state);
  }

  private flushInto(base: PhoneState): void {
    const batch = this.pending;
    this.pending = [];
    const next = batch.length > 0 ? store.applyAll(base, batch) : base;
    if (batch.length > 0) {
      for (const entry of this.open.values()) {
        const mine = batch.filter((event) => conversations.belongs(entry.snapshot.conversation, event) || isChildOf(next, entry.snapshot.runId, event));
        if (mine.length === 0) continue;
        if (entry.snapshot.loading) {
          entry.waiting.push(...mine);
          continue;
        }
        let conversation = entry.snapshot.conversation;
        const run = store.run(next, entry.snapshot.runId);
        if (run) conversation = conversations.setRun(conversation, run, store.descendantsOf(next, entry.snapshot.runId)).conversation;
        conversation = conversations.appendAll(conversation, mine).conversation;
        if (conversation !== entry.snapshot.conversation) this.setConversation(entry, { conversation });
      }
    }
    if (next !== this.snapshot.state || batch.length > 0) {
      this.update({ state: next, stateAt: batch.length > 0 && !this.snapshot.fromCache ? this.deps.now() : this.snapshot.stateAt, lastContact: this.deps.connection.lastContact });
      this.writeCache(false);
    }
  }

  private refreshRun(entry: OpenConversation): void {
    const run = store.run(this.snapshot.state, entry.snapshot.runId);
    if (!run || entry.snapshot.loading) return;
    const changed = conversations.setRun(entry.snapshot.conversation, run, store.descendantsOf(this.snapshot.state, entry.snapshot.runId));
    if (changed.conversation !== entry.snapshot.conversation) this.setConversation(entry, { conversation: changed.conversation });
  }

  private update(change: Partial<SessionSnapshot>): void {
    let different = false;
    for (const key of Object.keys(change) as (keyof SessionSnapshot)[]) {
      if (!same(this.snapshot[key], change[key])) different = true;
      else delete change[key];
    }
    if (!different) return;
    this.snapshot = { ...this.snapshot, ...change };
    for (const listener of [...this.listeners]) listener();
  }

  /** Stores the state for the next launch. Often at most; at once when `now` is true. */
  private writeCache(now: boolean): void {
    if (this.snapshot.fromCache || this.snapshot.stateAt === null) return;
    const every = this.deps.cacheEveryMs ?? 5_000;
    const write = (): void => {
      this.cacheTimer = null;
      this.cacheWrittenAt = this.deps.now();
      try {
        this.deps.cache.set('state', JSON.stringify(store.snapshot(this.snapshot.state)));
        this.deps.cache.set('stateAt', this.snapshot.stateAt ?? this.deps.now());
      } catch (error) {
        this.deps.log?.(`the state could not be stored: ${error instanceof Error ? error.message : String(error)}`);
      }
    };
    if (now) {
      if (this.cacheTimer !== null) clearTimeout(this.cacheTimer);
      write();
      return;
    }
    if (this.cacheTimer !== null) return;
    const wait = Math.max(0, every - (this.deps.now() - this.cacheWrittenAt));
    this.cacheTimer = setTimeout(write, wait);
  }
}

const ALL_ON: NotificationSwitches = Object.freeze({ enabled: true, show_text: false, kinds: Object.freeze({ permission: true, question: true, failure: true, finished: true }) });

function switchesOf(value: unknown): NotificationSwitches {
  const v = (value !== null && typeof value === 'object' ? value : {}) as Record<string, unknown>;
  const kinds = (v['kinds'] !== null && typeof v['kinds'] === 'object' ? v['kinds'] : {}) as Record<string, unknown>;
  return {
    enabled: v['enabled'] !== false,
    show_text: v['show_text'] === true,
    kinds: { permission: kinds['permission'] !== false, question: kinds['question'] !== false, failure: kinds['failure'] !== false, finished: kinds['finished'] !== false },
  };
}

/** The same value, or a gateway or an outbox with the same content. */
function same(a: unknown, b: unknown): boolean {
  if (Object.is(a, b)) return true;
  if (typeof a !== 'object' || typeof b !== 'object' || a === null || b === null) return false;
  if ('byId' in (a as object) || 'rows' in (a as object)) return false;
  if ('runs' in (a as object)) return false;
  try {
    return JSON.stringify(a) === JSON.stringify(b);
  } catch {
    return false;
  }
}

function readCache(cache: SyncStore<SessionCache>): { state: State; stateAt: number } | null {
  try {
    const stored = cache.get('state');
    const stateAt = cache.get('stateAt');
    if (typeof stored !== 'string' || typeof stateAt !== 'number') return null;
    const state = JSON.parse(stored) as State;
    if (!state || !Array.isArray(state.runs) || !Array.isArray(state.tasks)) return null;
    return { state, stateAt };
  } catch {
    return null;
  }
}

/** The Mac's home folder, as far as the state shows it: for "~" in long paths. */
function homeOf(state: PhoneState): string {
  for (const task of store.rows(state.tasks)) {
    const match = /^(\/Users\/[^/]+|\/home\/[^/]+)\//.exec(task.repo_root);
    if (match?.[1]) return match[1];
  }
  return '';
}

/** True for an event of a run that the state knows as a descendant of `rootId`. */
function isChildOf(state: PhoneState, rootId: string, event: DaemonEvent): boolean {
  if (!event.run_id || event.run_id === rootId) return event.run_id === rootId;
  return store.rootOf(state, event.run_id)?.id === rootId;
}

export type { OutboxEntry };
