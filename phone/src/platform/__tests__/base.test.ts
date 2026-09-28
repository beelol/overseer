import {
  CapabilityUnsupportedError,
  SUPPORTED,
  createLive,
  defineCapability,
  formatAddress,
  parseManualAddress,
  sameAddress,
  scopeAsyncStore,
  scopeSyncStore,
  unsupported,
  type HapticsCapability,
  type RawAsyncStore,
  type RawSyncStore,
} from '@/platform';

describe('a capability', () => {
  test('has its name, its support check and its API side by side', async () => {
    const played: string[] = [];
    const haptics = defineCapability<HapticsCapability>('haptics', async () => SUPPORTED, {
      play: (moment) => void played.push(moment),
    });
    expect(haptics.name).toBe('haptics');
    expect(await haptics.support()).toEqual({ supported: true });
    haptics.play('confirm');
    expect(played).toEqual(['confirm']);
    expect(Object.isFrozen(haptics)).toBe(true);
  });

  test('a gap carries its reason', () => {
    const gap = unsupported('No camera here.');
    expect(gap).toEqual({ supported: false, reason: 'No camera here.' });
    const error = new CapabilityUnsupportedError('camera', gap.reason);
    expect(error).toBeInstanceOf(Error);
    expect(error.capability).toBe('camera');
    expect(error.reason).toBe('No camera here.');
    expect(error.message).toBe('camera is not supported here: No camera here.');
  });
});

describe('a live value', () => {
  test('tells its listeners of each change, and only of changes', () => {
    const live = createLive('a');
    const heard: string[] = [];
    const stop = live.subscribe((value) => heard.push(value));
    live.set('a');
    live.set('b');
    live.set('b');
    live.set('c');
    expect(heard).toEqual(['b', 'c']);
    expect(live.get()).toBe('c');
    stop();
    live.set('d');
    expect(heard).toEqual(['b', 'c']);
  });

  test('keeps the same reference while the value is equal', () => {
    const first = { n: 1 };
    const live = createLive(first, (a, b) => a.n === b.n);
    const heard = jest.fn();
    live.subscribe(heard);
    live.set({ n: 1 });
    expect(live.get()).toBe(first);
    expect(heard).not.toHaveBeenCalled();
  });

  test('the same function subscribed twice is two subscriptions', () => {
    const live = createLive(0);
    const listener = jest.fn();
    const stopFirst = live.subscribe(listener);
    live.subscribe(listener);
    stopFirst();
    stopFirst();
    live.set(1);
    expect(listener).toHaveBeenCalledTimes(1);
  });

  test('a listener may unsubscribe another while a change is being told', () => {
    const live = createLive(0);
    const second = jest.fn();
    let stopSecond = (): void => undefined;
    live.subscribe(() => stopSecond());
    stopSecond = live.subscribe(second);
    expect(() => live.set(1)).not.toThrow();
  });
});

function memoryAsync(
  items = new Map<string, string>(),
): RawAsyncStore & { items: Map<string, string> } {
  return {
    items,
    getItem: async (key) => items.get(key) ?? null,
    setItem: async (key, value) => void items.set(key, value),
    deleteItem: async (key) => void items.delete(key),
  };
}

function memorySync(
  items = new Map<string, string>(),
): RawSyncStore & { items: Map<string, string> } {
  return {
    items,
    getItem: (key) => items.get(key) ?? null,
    setItem: (key, value) => void items.set(key, value),
    deleteItem: (key) => void items.delete(key),
    allKeys: () => [...items.keys()],
  };
}

describe('typed stores', () => {
  interface Session {
    cursor: number;
    addresses: readonly { host: string; port: number }[];
    name: string | null;
  }

  test('a synchronous store returns what was stored, with its type', () => {
    const raw = memorySync();
    const store = scopeSyncStore<Session>(raw, 'session');
    expect(store.get('cursor')).toBeNull();
    store.set('cursor', 42);
    store.set('addresses', [{ host: 'mac.local', port: 47810 }]);
    store.set('name', null);
    const cursor: number | null = store.get('cursor');
    expect(cursor).toBe(42);
    expect(store.get('addresses')).toEqual([{ host: 'mac.local', port: 47810 }]);
    expect(store.get('name')).toBeNull();
    expect(raw.items.get('session.cursor')).toBe('42');
  });

  test('scopes do not see each other', () => {
    const raw = memorySync();
    const one = scopeSyncStore<{ value: string }>(raw, 'one');
    const two = scopeSyncStore<{ value: string; other: string }>(raw, 'two');
    one.set('value', 'first');
    two.set('value', 'second');
    two.set('other', 'third');
    expect(one.get('value')).toBe('first');
    expect(one.keys()).toEqual(['value']);
    expect(two.keys()).toEqual(['other', 'value']);
    two.delete('value');
    two.delete('value');
    expect(two.keys()).toEqual(['other']);
    expect(one.get('value')).toBe('first');
  });

  test('a value that cannot be read is a missing value', () => {
    const raw = memorySync(new Map([['session.cursor', '{not json']]));
    expect(scopeSyncStore<Session>(raw, 'session').get('cursor')).toBeNull();
  });

  test('an asynchronous store returns what was stored', async () => {
    const raw = memoryAsync();
    const secrets = scopeAsyncStore<{ 'device.key': string }>(raw, 'pairing');
    expect(await secrets.get('device.key')).toBeNull();
    await secrets.set('device.key', 'c2VjcmV0');
    expect(await secrets.get('device.key')).toBe('c2VjcmV0');
    expect(raw.items.get('pairing.device.key')).toBe('c2VjcmV0');
    await secrets.delete('device.key');
    expect(await secrets.get('device.key')).toBeNull();
  });

  test('names the keystores would refuse are refused everywhere', () => {
    expect(() => scopeSyncStore(memorySync(), 'has.dot')).toThrow(TypeError);
    expect(() => scopeSyncStore(memorySync(), '')).toThrow(TypeError);
    expect(() => scopeAsyncStore(memoryAsync(), 'has space')).toThrow(TypeError);
    const store = scopeSyncStore<Record<string, string>>(memorySync(), 'ok');
    expect(() => store.get('a/b')).toThrow(TypeError);
    expect(() => store.set('', 'x')).toThrow(TypeError);
    expect(() => store.set('dots.are.fine', 'x')).not.toThrow();
  });
});

describe('manual addresses', () => {
  const parse = (input: string) => parseManualAddress(input, 47810);

  test.each([
    ['192.168.1.20', { host: '192.168.1.20', port: 47810 }],
    ['192.168.1.20:5000', { host: '192.168.1.20', port: 5000 }],
    ['  Mac.local  ', { host: 'mac.local', port: 47810 }],
    ['bilals-mac.local:47811', { host: 'bilals-mac.local', port: 47811 }],
    ['10.0.2.2', { host: '10.0.2.2', port: 47810 }],
    ['fe80::1', { host: 'fe80::1', port: 47810 }],
    ['[fe80::1]', { host: 'fe80::1', port: 47810 }],
    ['[fe80::1%en0]:47810', { host: 'fe80::1%en0', port: 47810 }],
    ['[::1]:1', { host: '::1', port: 1 }],
  ])('reads %p', (input, address) => {
    expect(parse(input)).toEqual({ ok: true, address });
  });

  test.each([
    '',
    '   ',
    'ws://192.168.1.20:47810',
    '192.168.1.20/v1',
    '192.168.1.20:0',
    '192.168.1.20:65536',
    '192.168.1.20:port',
    '192.168.1.20:',
    '192.168.1.256',
    '1.2.3.4.5:80x',
    'mac local',
    '-mac.local',
    '[fe80::1',
    '[fe80::1]47810',
    '[mac.local]:47810',
    'under_score.local',
  ])('refuses %p and says why', (input) => {
    const result = parse(input);
    expect(result.ok).toBe(false);
    if (!result.ok) expect(result.reason.length).toBeGreaterThan(10);
  });

  test('compares and writes addresses', () => {
    expect(sameAddress({ host: 'a', port: 1 }, { host: 'a', port: 1 })).toBe(true);
    expect(sameAddress({ host: 'a', port: 1 }, { host: 'a', port: 2 })).toBe(false);
    expect(formatAddress({ host: '10.0.2.2', port: 47810 })).toBe('10.0.2.2:47810');
    expect(formatAddress({ host: 'fe80::1', port: 47810 })).toBe('[fe80::1]:47810');
  });
});
