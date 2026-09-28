import { holdable, type Session, type SessionSnapshot } from '@/session';

/** Just enough of a session: a snapshot that changes, its listeners, and a request. */
function fake() {
  let snapshot = { connection: 'connecting' } as unknown as SessionSnapshot;
  const listeners = new Set<() => void>();
  const asked: string[] = [];
  const session = {
    getSnapshot: () => snapshot,
    subscribe: (listener: () => void) => {
      listeners.add(listener);
      return () => void listeners.delete(listener);
    },
    wake(this: { woken: number }) {
      this.woken += 1;
    },
    woken: 0,
    request: async (method: string) => {
      asked.push(method);
      return 'answered';
    },
  };
  const change = (connection: string) => {
    snapshot = { connection } as unknown as SessionSnapshot;
    for (const listener of [...listeners]) listener();
  };
  return { session: session as unknown as Session, raw: session, change, asked };
}

describe('the session as the screens read it', () => {
  test('passes every change on while it is not held', () => {
    const { session, change } = fake();
    const screens = holdable(session);
    const seen: string[] = [];
    screens.subscribe(() => seen.push(screens.getSnapshot().connection));
    change('online');
    expect(seen).toEqual(['online']);
  });

  test('holds still while held, and draws what came meanwhile once released', () => {
    const { session, change } = fake();
    const screens = holdable(session);
    let calls = 0;
    screens.subscribe(() => (calls += 1));
    screens.hold();
    change('online');
    change('unreachable');
    change('online');
    expect(calls).toBe(0);
    expect(screens.getSnapshot().connection).toBe('connecting');
    expect(session.getSnapshot().connection).toBe('online');
    screens.release();
    expect(calls).toBe(1);
    expect(screens.getSnapshot().connection).toBe('online');
    screens.release();
    expect(calls).toBe(1);
  });

  test('a screen that went away while held is not called', () => {
    const { session, change } = fake();
    const screens = holdable(session);
    let calls = 0;
    const off = screens.subscribe(() => (calls += 1));
    screens.hold();
    change('online');
    off();
    screens.release();
    expect(calls).toBe(0);
  });

  test('requests and everything else go straight to the session, held or not', async () => {
    const { session, raw, asked } = fake();
    const screens = holdable(session);
    screens.hold();
    await expect(screens.request('state', {})).resolves.toBe('answered');
    screens.wake();
    expect(asked).toEqual(['state']);
    expect(raw.woken).toBe(1);
  });
});
