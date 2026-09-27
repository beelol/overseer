import type { ConnectionState, GatewayInfo, OutboxEntry } from '@/core';
import { store, type DaemonEvent } from '@/model';
import { createFakePlatform } from '@/platform/fake';
import type { State } from '@/protocol';
import { Session, type Connection, type SessionCache } from '@/session';

type Handler = (...args: never[]) => void;

/** A connection a test drives by hand. */
class FakeConnection implements Connection {
  state: ConnectionState = 'unpaired';
  lastContact: number | null = null;
  gateway: GatewayInfo | null = null;
  hello: Record<string, unknown> | null = null;
  cursor = 0;
  entries: OutboxEntry[] = [];
  readonly asked: { method: string; params: unknown }[] = [];
  readonly handlers = new Map<string, Set<Handler>>();
  answers: Record<string, (params: never) => unknown> = {};
  woken = 0;

  outbox = () => this.entries;
  dismiss = (id: string) => void (this.entries = this.entries.filter((e) => e.requestId !== id));
  on(event: string, listener: Handler): () => void {
    const set = this.handlers.get(event) ?? new Set();
    set.add(listener);
    this.handlers.set(event, set);
    return () => void set.delete(listener);
  }
  emit(event: string, ...args: unknown[]): void {
    for (const h of this.handlers.get(event) ?? []) (h as (...a: unknown[]) => void)(...args);
  }
  start = async () => undefined;
  stop = async () => undefined;
  wake = () => void this.woken++;
  pair = async () => {
    this.gateway = GATEWAY;
    return GATEWAY;
  };
  forget = async () => {
    this.gateway = null;
    this.emit('forgotten', 'forgotten');
  };
  setDiscovered = () => undefined;
  request = (async (method: string, params: unknown) => {
    this.asked.push({ method, params });
    const answer = this.answers[method];
    if (!answer) throw new Error(`no answer for ${method}`);
    return answer(params as never);
  }) as Connection['request'];

  go(state: ConnectionState): void {
    const before = this.state;
    this.state = state;
    this.emit('state', state, before);
  }
}

const GATEWAY: GatewayInfo = { deviceId: 'd1', deviceName: 'Phone', platform: 'ios', gatewayName: 'Mac', gatewayFingerprint: '0123456789abcdef', scope: 'full', pairedAt: 1 };

const run = (id: string, status = 'running', extra: Record<string, unknown> = {}) => ({ id, task_id: 't1', harness: 'claude', workspace_id: 'w1', status, created_ms: 1, title: `run ${id}`, capabilities: {}, process_generation: 1, ...extra });
const STATE = {
  cursor: 10,
  daemon: { version: '0', protocol: 1 },
  tasks: [{ id: 't1', title: 'Task', repo_root: '/Users/owner/shop', created_ms: 1, archived: false }],
  runs: [run('r1')],
  workspaces: [],
  profiles: [],
  turns: [],
} as unknown as State;

const event = (seq: number, kind: string, payload: unknown, runId = 'r1'): DaemonEvent => ({ seq, ts: seq, run_id: runId, task_id: 't1', kind, source: 'daemon', payload }) as unknown as DaemonEvent;

function make(options: { cached?: State } = {}) {
  const platform = createFakePlatform();
  const cache = platform.capabilities.keyValue.scope<SessionCache>('cache');
  if (options.cached) {
    cache.set('state', JSON.stringify(options.cached));
    cache.set('stateAt', 500);
  }
  const connection = new FakeConnection();
  connection.answers['state'] = () => STATE;
  connection.answers['events.list'] = () => ({ events: [] });
  const frames: (() => void)[] = [];
  let now = 1_000;
  const session = new Session({
    connection,
    cache,
    now: () => now,
    nextFrame: (callback) => {
      frames.push(callback);
      return () => void frames.splice(frames.indexOf(callback), 1);
    },
    cacheEveryMs: 0,
  });
  return {
    session,
    connection,
    cache,
    frame: () => frames.splice(0).forEach((f) => f()),
    frames,
    tick: (ms: number) => void (now += ms),
  };
}

const settle = () => new Promise((resolve) => setImmediate(resolve));

describe('the session', () => {
  test('opens on what the phone stored, marked as stored, before anything is connected', async () => {
    const { session } = make({ cached: STATE });
    const first = session.getSnapshot();
    expect(first.ready).toBe(false);
    expect(first.fromCache).toBe(true);
    expect(first.stateAt).toBe(500);
    expect(store.run(first.state, 'r1')?.title).toBe('run r1');
    await session.start();
    expect(session.getSnapshot().ready).toBe(true);
    expect(session.getSnapshot().paired).toBe(false);
  });

  test('a cache that cannot be read is no cache, not an error', async () => {
    const { session, cache } = make();
    cache.set('state', '{not json');
    cache.set('stateAt', 5);
    const again = new Session({ connection: new FakeConnection(), cache, now: () => 1, nextFrame: () => () => undefined });
    expect(again.getSnapshot().fromCache).toBe(false);
    expect(session.getSnapshot().fromCache).toBe(false);
  });

  test('loads the state when the connection comes up, and stores it for the next launch', async () => {
    const { session, connection, cache } = make();
    connection.gateway = GATEWAY;
    await session.start();
    connection.go('online');
    await settle();
    const s = session.getSnapshot();
    expect(s.connection).toBe('online');
    expect(s.paired).toBe(true);
    expect(s.scope).toBe('full');
    expect(s.fromCache).toBe(false);
    expect(s.stateAt).toBe(1_000);
    expect(store.run(s.state, 'r1')).toBeDefined();
    expect(JSON.parse(cache.get('state') ?? '{}').runs).toHaveLength(1);
  });

  test('applies a burst of events as one change per frame', async () => {
    const { session, connection, frame, frames } = make();
    connection.gateway = GATEWAY;
    await session.start();
    connection.go('online');
    await settle();
    let draws = 0;
    session.subscribe(() => draws++);
    for (let i = 0; i < 200; i++) connection.emit('event', event(11 + i, 'status', { status: i % 2 ? 'running' : 'waiting_for_user' }), { live: true });
    expect(frames).toHaveLength(1);
    expect(draws).toBe(0);
    frame();
    expect(draws).toBe(1);
  });

  test('events that arrive while the state is loading are applied after it', async () => {
    const { session, connection, frame } = make();
    connection.gateway = GATEWAY;
    let release: (state: State) => void = () => undefined;
    connection.answers['state'] = () => new Promise<State>((resolve) => (release = resolve));
    await session.start();
    connection.go('online');
    connection.emit('event', event(11, 'status', { status: 'completed' }), { live: true });
    release(STATE);
    await settle();
    frame();
    expect(store.run(session.getSnapshot().state, 'r1')?.status).toBe('completed');
  });

  test('when the Mac no longer has what was missed, the state is loaded again and says so', async () => {
    const { session, connection } = make();
    connection.gateway = GATEWAY;
    await session.start();
    connection.go('online');
    await settle();
    const asked = connection.asked.filter((a) => a.method === 'state').length;
    connection.emit('truncated', { cursor: 99 });
    await settle();
    expect(connection.asked.filter((a) => a.method === 'state').length).toBe(asked + 1);
    expect(session.getSnapshot().historyLost).toBe(true);
  });

  test('a conversation loads its history once and then follows live events', async () => {
    const { session, connection, frame } = make();
    connection.gateway = GATEWAY;
    connection.answers['events.list'] = () => ({ events: [event(3, 'user_message', { text: 'hello' }), event(4, 'assistant_text', { text: 'hi' })] });
    await session.start();
    connection.go('online');
    await settle();
    const handle = session.conversation('r1');
    let draws = 0;
    const off = handle.subscribe(() => draws++);
    expect(handle.getSnapshot().loading).toBe(true);
    await settle();
    expect(handle.getSnapshot().loading).toBe(false);
    expect(handle.getSnapshot().error).toBeNull();
    const loaded = handle.getSnapshot().conversation;
    connection.emit('event', event(11, 'assistant_text', { text: 'more' }), { live: true });
    frame();
    expect(handle.getSnapshot().conversation).not.toBe(loaded);
    expect(connection.asked.filter((a) => a.method === 'events.list')).toHaveLength(1);
    expect(draws).toBeGreaterThan(0);
    off();
  });

  test('a history that cannot be loaded is said, and the conversation still follows live events', async () => {
    const { session, connection } = make();
    connection.gateway = GATEWAY;
    connection.answers['events.list'] = () => {
      throw new Error('the Mac is unreachable');
    };
    await session.start();
    const handle = session.conversation('r1');
    handle.subscribe(() => undefined);
    await settle();
    expect(handle.getSnapshot().loading).toBe(false);
    expect(handle.getSnapshot().error).toBe('the Mac is unreachable');
  });

  test('a history that failed is asked for again when the owner tries again', async () => {
    const { session, connection } = make();
    connection.gateway = GATEWAY;
    connection.answers['events.list'] = () => {
      throw new Error('the Mac is unreachable');
    };
    await session.start();
    const handle = session.conversation('r1');
    handle.subscribe(() => undefined);
    await settle();
    expect(handle.getSnapshot().error).toBe('the Mac is unreachable');
    connection.answers['events.list'] = () => ({ events: [event(3, 'user_message', { text: 'hello' })] });
    await session.reloadConversation('r1');
    expect(handle.getSnapshot().error).toBeNull();
    expect(handle.getSnapshot().loading).toBe(false);
  });

  test('forgetting the Mac removes what was stored about it', async () => {
    const { session, connection, cache } = make();
    connection.gateway = GATEWAY;
    await session.start();
    connection.go('online');
    await settle();
    expect(cache.get('state')).not.toBeNull();
    await session.forget();
    expect(cache.get('state')).toBeNull();
    expect(session.getSnapshot().paired).toBe(false);
    expect(session.getSnapshot().state.runs.byId.size).toBe(0);
  });

  test('notification switches show at once and go back when the Mac refuses', async () => {
    const { session, connection } = make();
    connection.gateway = GATEWAY;
    await session.start();
    connection.answers['device.notifications'] = () => {
      throw new Error('refused');
    };
    expect(session.getSnapshot().notifications.enabled).toBe(false);
    const changing = session.setNotifications({ enabled: true });
    expect(session.getSnapshot().notifications.enabled).toBe(true);
    await expect(changing).rejects.toThrow('refused');
    expect(session.getSnapshot().notifications.enabled).toBe(false);
    connection.answers['device.notifications'] = () => ({ enabled: false, show_text: false, kinds: { permission: true, question: true, failure: false, finished: true }, environment: 'device' });
    await session.setNotifications({ enabled: false, kinds: { failure: false } });
    expect(session.getSnapshot().notifications).toEqual({ enabled: false, show_text: false, kinds: { permission: true, question: true, failure: false, finished: true } });
  });

  test('a switch changed before the first connection reaches the Mac when it is made, even after a relaunch', async () => {
    const { session, connection, cache } = make();
    connection.gateway = GATEWAY;
    connection.state = 'connecting';
    await session.start();
    await session.setNotificationsSoon({ enabled: true });
    expect(session.getSnapshot().notifications.enabled).toBe(true);
    expect(connection.asked.filter((a) => a.method === 'device.notifications')).toEqual([]);
    await session.stop();

    // The app was closed before it connected: the next launch still owes it to the Mac.
    const again = new Session({ connection, cache, now: () => 1, nextFrame: () => () => undefined, cacheEveryMs: 0 });
    connection.hello = { notifications: { enabled: false } };
    await again.start();
    expect(again.getSnapshot().notifications.enabled).toBe(true);
    const sent: unknown[] = [];
    connection.answers['device.notifications'] = (change: unknown) => {
      sent.push(change);
      return { enabled: true, show_text: false, kinds: {}, environment: 'device' };
    };
    connection.go('online');
    await settle();
    expect(sent).toEqual([{ enabled: true }]);
    expect(again.getSnapshot().notifications.enabled).toBe(true);
    // Paid: the next connection sends nothing again.
    connection.go('reconnecting');
    connection.go('online');
    await settle();
    expect(sent).toHaveLength(1);
    await again.stop();
  });

  test('what the Mac lets this phone do changes at once, and only for this phone', async () => {
    const { session, connection } = make();
    connection.gateway = GATEWAY;
    await session.start();
    connection.go('online');
    await settle();
    expect(session.getSnapshot().scope).toBe('full');
    connection.emit('event', event(11, 'device_scope', { device: 'another', scope: 'watch' }), { live: true });
    expect(session.getSnapshot().scope).toBe('full');
    connection.emit('event', event(12, 'device_scope', { device: 'd1', scope: 'watch' }), { live: true });
    expect(session.getSnapshot().scope).toBe('watch');
  });

  test('reviewed marks are asked of the Mac once and then follow its events', async () => {
    const { session, connection, frame } = make();
    connection.gateway = GATEWAY;
    connection.answers['review.marks'] = () => ({ run_id: 'r1', keys: ['k1'], marks: [{ key: 'k1', path: 'a.txt', at_ms: 5, by: 'the Mac' }] });
    await session.start();
    connection.go('online');
    await settle();
    await session.loadMarks('r1');
    expect(store.marksOf(session.getSnapshot().state, 'r1').map((m) => m.key)).toEqual(['k1']);
    connection.emit('event', { ...event(11, 'review_mark', { key: 'k2', path: 'b.txt', reviewed: true }), source: 'phone:Phone' }, { live: true });
    frame();
    expect(store.marksOf(session.getSnapshot().state, 'r1').map((m) => m.key).sort()).toEqual(['k1', 'k2']);
  });

  test('it counts how the stream arrived: a gap, a duplicate, and a start the Mac announced', async () => {
    const { session, connection } = make();
    connection.gateway = GATEWAY;
    connection.cursor = 10;
    await session.start();
    connection.go('online');
    await settle();
    for (const seq of [11, 12, 13]) connection.emit('event', event(seq, 'output', {}), { live: true });
    expect(session.streamStats()).toEqual({ count: 3, last: 13, gaps: 0, duplicates: 0, truncated: 0 });
    connection.emit('event', event(15, 'output', {}), { live: true });
    connection.emit('event', event(15, 'output', {}), { live: true });
    expect(session.streamStats()).toMatchObject({ gaps: 1, duplicates: 1, last: 15 });
    connection.emit('truncated', { cursor: 90 });
    connection.emit('event', event(91, 'output', {}), { live: true });
    expect(session.streamStats()).toMatchObject({ gaps: 1, duplicates: 1, truncated: 1, last: 91 });
  });

  test('it times a line from the Mac to the display: news only, at the frame that shows it', async () => {
    const platform = createFakePlatform();
    const connection = new FakeConnection();
    connection.gateway = GATEWAY;
    connection.answers['state'] = () => STATE;
    const frames: (() => void)[] = [];
    const delays: number[] = [];
    let now = 5_000;
    const session = new Session({ connection, cache: platform.capabilities.keyValue.scope<SessionCache>('cache'), now: () => now, nextFrame: (f) => (frames.push(f), () => undefined), onDelay: (ms) => delays.push(ms) });
    await session.start();
    connection.go('online');
    await settle();
    connection.emit('event', { ...event(11, 'output', { text: 'old' }), ts: 1_000 }, { live: false });
    connection.emit('event', { ...event(12, 'output', { text: 'new' }), ts: 4_900 }, { live: true });
    now = 5_020;
    frames.splice(0).forEach((f) => f());
    now = 5_036;
    frames.splice(0).forEach((f) => f());
    expect(delays).toEqual([136]);
  });

  test('coming to the front tries the Mac at once', async () => {
    const { session, connection } = make();
    await session.start();
    session.wake();
    expect(connection.woken).toBe(1);
  });
});
