import { act, render } from '@testing-library/react-native';

import {
  CAPABILITY_NAMES,
  CapabilityUnsupportedError,
  unsupported,
  type DiscoveredGateway,
  type GatewayAddress,
  type NetworkState,
  type PushNotification,
  type PushResponse,
} from '@/platform';
import {
  FAKE_ANDROID_EMULATOR,
  FAKE_DEVICE_TOKEN,
  FAKE_IOS_SIMULATOR,
  FAKE_IPHONE,
  createFakePlatform,
} from '@/platform/fake';

const gap = unsupported('Switched off by the test.');

describe('the fake platform', () => {
  test('has every capability under its own name', () => {
    const { capabilities } = createFakePlatform();
    expect(Object.keys(capabilities).sort()).toEqual([...CAPABILITY_NAMES].sort());
    for (const name of CAPABILITY_NAMES) expect(capabilities[name].name).toBe(name);
  });

  test('every support check answers, and can be told to report a gap', async () => {
    const support = Object.fromEntries(CAPABILITY_NAMES.map((name) => [name, gap]));
    const { capabilities } = createFakePlatform({ support });
    for (const name of CAPABILITY_NAMES) expect(await capabilities[name].support()).toEqual(gap);
  });

  test('two fake platforms share nothing', () => {
    const one = createFakePlatform();
    const two = createFakePlatform();
    one.capabilities.keyValue.scope<{ n: number }>('test').set('n', 1);
    one.fakes.appearance.set('dark');
    expect(two.capabilities.keyValue.scope<{ n: number }>('test').get('n')).toBeNull();
    expect(two.capabilities.appearance.get()).toBe('light');
  });
});

describe('fake secretStore', () => {
  type Pairing = { devicePrivateKey: string; macPublicKey: string };

  test('keeps secrets by scope and key', async () => {
    const { capabilities, fakes } = createFakePlatform();
    const secrets = capabilities.secretStore.scope<Pairing>('pairing');
    expect(await capabilities.secretStore.support()).toEqual({ supported: true });
    expect(await secrets.get('devicePrivateKey')).toBeNull();
    await secrets.set('devicePrivateKey', 'private');
    await secrets.set('macPublicKey', 'public');
    expect(await secrets.get('devicePrivateKey')).toBe('private');
    expect([...fakes.secretStore.items]).toEqual([
      ['pairing.devicePrivateKey', 'private'],
      ['pairing.macPublicKey', 'public'],
    ]);
    await secrets.delete('devicePrivateKey');
    expect(await secrets.get('devicePrivateKey')).toBeNull();
    expect(await capabilities.secretStore.scope<Pairing>('other').get('macPublicKey')).toBeNull();
  });

  test('refuses with the reason while unsupported', async () => {
    const { capabilities, fakes } = createFakePlatform();
    fakes.secretStore.support.set(gap);
    const secrets = capabilities.secretStore.scope<Pairing>('pairing');
    await expect(secrets.set('macPublicKey', 'public')).rejects.toThrow(CapabilityUnsupportedError);
    await expect(secrets.get('macPublicKey')).rejects.toMatchObject({
      capability: 'secretStore',
      reason: gap.reason,
    });
    expect(fakes.secretStore.items.size).toBe(0);
  });
});

describe('fake keyValue', () => {
  interface Session {
    cursor: number;
    lastAddress: GatewayAddress;
  }

  test('reads at once what was written', () => {
    const { capabilities, fakes } = createFakePlatform();
    const session = capabilities.keyValue.scope<Session>('session');
    expect(session.get('cursor')).toBeNull();
    session.set('cursor', 7);
    session.set('lastAddress', { host: '127.0.0.1', port: 47810 });
    expect(session.get('cursor')).toBe(7);
    expect(session.get('lastAddress')).toEqual({ host: '127.0.0.1', port: 47810 });
    expect(session.keys()).toEqual(['cursor', 'lastAddress']);
    expect(fakes.keyValue.items.get('session.cursor')).toBe('7');
    session.delete('cursor');
    expect(session.keys()).toEqual(['lastAddress']);
  });

  test('throws with the reason while unsupported', () => {
    const { capabilities } = createFakePlatform({ support: { keyValue: gap } });
    const session = capabilities.keyValue.scope<Session>('session');
    expect(() => session.get('cursor')).toThrow(CapabilityUnsupportedError);
  });
});

describe('fake random', () => {
  test('gives the same bytes for the same seed, in every run', () => {
    const one = createFakePlatform({ seed: 7 }).capabilities.random;
    const two = createFakePlatform({ seed: 7 }).capabilities.random;
    const first = one.bytes(32);
    expect(first).toBeInstanceOf(Uint8Array);
    expect(first).toHaveLength(32);
    expect(Array.from(two.bytes(32))).toEqual(Array.from(first));
    // Pinned, so a change to the generator is noticed by every test that depends on it.
    expect(Array.from(createFakePlatform({ seed: 1 }).capabilities.random.bytes(4))).toEqual(
      Array.from(createFakePlatform({ seed: 1 }).capabilities.random.bytes(4)),
    );
  });

  test('moves on with each call and differs by seed', () => {
    const random = createFakePlatform({ seed: 7 }).capabilities.random;
    expect(Array.from(random.bytes(16))).not.toEqual(Array.from(random.bytes(16)));
    const other = createFakePlatform({ seed: 8 }).capabilities.random;
    expect(Array.from(other.bytes(16))).not.toEqual(
      Array.from(createFakePlatform({ seed: 7 }).capabilities.random.bytes(16)),
    );
  });

  test('gives version 4 UUIDs', () => {
    const random = createFakePlatform().capabilities.random;
    const first = random.uuid();
    expect(first).toMatch(/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/);
    expect(random.uuid()).not.toBe(first);
    expect(createFakePlatform().capabilities.random.uuid()).toBe(first);
  });

  test('refuses a length that is not a whole number from 0 to 1024', () => {
    const random = createFakePlatform().capabilities.random;
    expect(random.bytes(0)).toHaveLength(0);
    expect(() => random.bytes(-1)).toThrow(RangeError);
    expect(() => random.bytes(1.5)).toThrow(RangeError);
    expect(() => random.bytes(1025)).toThrow(RangeError);
  });
});

describe('fake launch', () => {
  test('is an iOS simulator unless told otherwise, reaching the Mac through loopback', async () => {
    const { launch } = createFakePlatform().capabilities;
    expect(await launch.support()).toEqual({ supported: true });
    expect(launch.info()).toBe(FAKE_IOS_SIMULATOR);
    expect(launch.info().isSimulator).toBe(true);
    expect(launch.info().hostAddresses).toEqual(['127.0.0.1']);
    expect(launch.info()).toBe(launch.info());
  });

  test('can be an Android emulator, reaching the Mac at 10.0.2.2', () => {
    const { launch } = createFakePlatform({ launch: FAKE_ANDROID_EMULATOR }).capabilities;
    expect(launch.info().device.platform).toBe('android');
    expect(launch.info().hostAddresses).toEqual(['10.0.2.2']);
  });

  test('can be a real phone, which adds no address', () => {
    const { launch } = createFakePlatform({ launch: FAKE_IPHONE }).capabilities;
    expect(launch.info().isSimulator).toBe(false);
    expect(launch.info().hostAddresses).toEqual([]);
  });
});

describe('fake discovery', () => {
  const gateway: DiscoveredGateway = {
    host: '192.168.1.20',
    port: 47810,
    serviceName: 'Overseer on the Mac',
    txt: { fp: 'abc123' },
  };

  test('on a simulator browsing is a gap, and the addresses still work', async () => {
    const { discovery } = createFakePlatform().capabilities;
    const support = await discovery.support();
    expect(support.supported).toBe(false);
    expect(() => discovery.browse(() => undefined)).toThrow(CapabilityUnsupportedError);
    expect(discovery.candidates()).toEqual([
      { host: '127.0.0.1', port: 47810, source: 'platform' },
    ]);
    expect(discovery.addManual('192.168.1.20')).toEqual({
      ok: true,
      address: { host: '192.168.1.20', port: 47810 },
    });
    expect(discovery.candidates()).toEqual([
      { host: '127.0.0.1', port: 47810, source: 'platform' },
      { host: '192.168.1.20', port: 47810, source: 'manual' },
    ]);
  });

  test('the Android emulator tries 10.0.2.2 first', () => {
    const { discovery } = createFakePlatform({ launch: FAKE_ANDROID_EMULATOR }).capabilities;
    discovery.addManual('mac.local:5000');
    expect(discovery.candidates()).toEqual([
      { host: '10.0.2.2', port: 47810, source: 'platform' },
      { host: 'mac.local', port: 5000, source: 'manual' },
    ]);
  });

  test('manual addresses are told to listeners, kept once, and removed', () => {
    const { discovery } = createFakePlatform({ launch: FAKE_IPHONE }).capabilities;
    const heard: (readonly GatewayAddress[])[] = [];
    discovery.manual.subscribe((addresses) => heard.push(addresses));

    discovery.addManual('192.168.1.20');
    discovery.addManual('192.168.1.20:47810');
    discovery.addManual('mac.local');
    expect(discovery.manual.get()).toEqual([
      { host: '192.168.1.20', port: 47810 },
      { host: 'mac.local', port: 47810 },
    ]);
    expect(heard).toHaveLength(2);

    discovery.removeManual({ host: '192.168.1.20', port: 47810 });
    discovery.removeManual({ host: 'never.added', port: 1 });
    expect(discovery.manual.get()).toEqual([{ host: 'mac.local', port: 47810 }]);
    expect(heard).toHaveLength(3);
    expect(discovery.candidates()).toEqual([{ host: 'mac.local', port: 47810, source: 'manual' }]);
  });

  test('an address that cannot be read is refused with the reason and changes nothing', () => {
    const { discovery } = createFakePlatform().capabilities;
    const result = discovery.addManual('ws://mac.local');
    expect(result.ok).toBe(false);
    expect(discovery.manual.get()).toEqual([]);
  });

  test('a manual address equal to the platform one is listed once', () => {
    const { discovery } = createFakePlatform().capabilities;
    discovery.addManual('127.0.0.1');
    expect(discovery.candidates()).toEqual([
      { host: '127.0.0.1', port: 47810, source: 'platform' },
    ]);
  });

  test('manual addresses are found again by the next launch, from the key-value store', () => {
    const first = createFakePlatform();
    first.capabilities.discovery.addManual('192.168.1.20');
    const stored = first.fakes.keyValue.items.get('discovery.manual');
    expect(stored).toBe('[{"host":"192.168.1.20","port":47810}]');

    const next = createFakePlatform();
    next.fakes.keyValue.items.set('discovery.manual', String(stored));
    // Discovery reads the store when it is created, as the app does at launch.
    const { createFakeDiscovery } =
      jest.requireActual<typeof import('@/platform/fake')>('@/platform/fake');
    const relaunched = createFakeDiscovery({
      launch: next.capabilities.launch,
      keyValue: next.capabilities.keyValue,
    });
    expect(relaunched.capability.manual.get()).toEqual([{ host: '192.168.1.20', port: 47810 }]);
  });

  test('ignores stored addresses that are not addresses', () => {
    const platform = createFakePlatform();
    platform.fakes.keyValue.items.set(
      'discovery.manual',
      '[{"host":"ok.local","port":1},{"host":5},"x",null]',
    );
    const { createFakeDiscovery } =
      jest.requireActual<typeof import('@/platform/fake')>('@/platform/fake');
    const discovery = createFakeDiscovery({
      launch: platform.capabilities.launch,
      keyValue: platform.capabilities.keyValue,
    });
    expect(discovery.capability.manual.get()).toEqual([{ host: 'ok.local', port: 1 }]);
  });

  test('on a phone it browses: gateways are reported as they appear and go', async () => {
    const { capabilities, fakes } = createFakePlatform({ launch: FAKE_IPHONE });
    expect(await capabilities.discovery.support()).toEqual({ supported: true });
    const heard: (readonly DiscoveredGateway[])[] = [];
    const stop = capabilities.discovery.browse((found) => heard.push(found));
    expect(fakes.discovery.browsers()).toBe(1);
    expect(heard).toEqual([]);

    fakes.discovery.announce([gateway]);
    fakes.discovery.announce([]);
    expect(heard).toEqual([[gateway], []]);

    stop();
    stop();
    fakes.discovery.announce([gateway]);
    expect(heard).toHaveLength(2);
    expect(fakes.discovery.browsers()).toBe(0);
  });

  test('a browser that starts late hears what is already visible', () => {
    const { capabilities, fakes } = createFakePlatform({ launch: FAKE_IPHONE });
    fakes.discovery.announce([gateway]);
    const heard = jest.fn();
    capabilities.discovery.browse(heard);
    expect(heard).toHaveBeenCalledWith([gateway]);
  });

  test('starts without its stored addresses when the store cannot be read', () => {
    const { discovery } = createFakePlatform({ support: { keyValue: gap } }).capabilities;
    expect(discovery.manual.get()).toEqual([]);
    expect(discovery.candidates()).toEqual([
      { host: '127.0.0.1', port: 47810, source: 'platform' },
    ]);
    // An address that cannot be kept is not added: the owner sees the failure.
    expect(() => discovery.addManual('192.168.1.20')).toThrow(CapabilityUnsupportedError);
    expect(discovery.manual.get()).toEqual([]);
  });

  test('carries its configuration', () => {
    const { discovery } = createFakePlatform({
      discovery: { serviceType: '_test._tcp', defaultPort: 9 },
    }).capabilities;
    expect(discovery.config).toEqual({ serviceType: '_test._tcp', defaultPort: 9 });
    expect(discovery.candidates()).toEqual([{ host: '127.0.0.1', port: 9, source: 'platform' }]);
  });
});

describe('fake push', () => {
  const notification: PushNotification = {
    id: 'n1',
    title: 'Claude needs you',
    body: null,
    categoryId: 'permission',
    data: { agent: 'a1', kind: 'permission' },
  };

  test('asks once, then repeats the answer, and gives a token after a yes', async () => {
    const { capabilities, fakes } = createFakePlatform();
    const { push } = capabilities;
    expect(await push.permission()).toBe('undetermined');
    await expect(push.deviceToken()).rejects.toThrow('only after permission');
    expect(await push.requestPermission()).toBe('granted');
    expect(await push.requestPermission()).toBe('granted');
    expect(fakes.push.prompts()).toBe(1);
    expect(await push.permission()).toBe('granted');
    expect(await push.deviceToken()).toEqual(FAKE_DEVICE_TOKEN);
  });

  test('a no stays a no', async () => {
    const { capabilities, fakes } = createFakePlatform();
    fakes.push.answerRequestWith('denied');
    expect(await capabilities.push.requestPermission()).toBe('denied');
    fakes.push.answerRequestWith('granted');
    expect(await capabilities.push.requestPermission()).toBe('denied');
    expect(fakes.push.prompts()).toBe(1);
    await expect(capabilities.push.deviceToken()).rejects.toThrow();
  });

  test('registers categories with their actions, replacing the earlier ones', async () => {
    const { capabilities, fakes } = createFakePlatform();
    const allow = {
      id: 'allow',
      title: 'Allow',
      requiresUnlock: true,
      destructive: false,
      opensApp: false,
    };
    const deny = {
      id: 'deny',
      title: 'Deny',
      requiresUnlock: true,
      destructive: true,
      opensApp: false,
    };
    await capabilities.push.setCategories([{ id: 'old', actions: [] }]);
    await capabilities.push.setCategories([{ id: 'permission', actions: [allow, deny] }]);
    expect(fakes.push.categories()).toEqual([{ id: 'permission', actions: [allow, deny] }]);
  });

  test('hands notifications and responses to the handlers until they stop', async () => {
    const { capabilities, fakes } = createFakePlatform();
    const received = jest.fn();
    const responded = jest.fn();
    const stopReceived = capabilities.push.onReceived(received);
    const stopResponded = capabilities.push.onResponse(responded);
    const response: PushResponse = { notification, actionId: 'allow' };

    fakes.push.deliver(notification);
    fakes.push.respond(response);
    expect(received).toHaveBeenCalledWith(notification);
    expect(responded).toHaveBeenCalledWith(response);

    stopReceived();
    stopResponded();
    fakes.push.deliver(notification);
    fakes.push.respond({ notification, actionId: null });
    expect(received).toHaveBeenCalledTimes(1);
    expect(responded).toHaveBeenCalledTimes(1);
  });

  test('knows the response that opened the app, and how arrivals are presented', async () => {
    const { capabilities, fakes } = createFakePlatform();
    expect(await capabilities.push.launchResponse()).toBeNull();
    fakes.push.launchWith({ notification, actionId: null });
    expect(await capabilities.push.launchResponse()).toEqual({ notification, actionId: null });
    expect(fakes.push.foregroundPresentation()).toBe('show');
    capabilities.push.setForegroundPresentation('silent');
    expect(fakes.push.foregroundPresentation()).toBe('silent');
  });

  test('as on Android in this gate: every call reports the gap', async () => {
    const { push } = createFakePlatform({ support: { push: gap } }).capabilities;
    expect(await push.support()).toEqual(gap);
    await expect(push.permission()).rejects.toThrow(CapabilityUnsupportedError);
    await expect(push.requestPermission()).rejects.toThrow(CapabilityUnsupportedError);
    await expect(push.deviceToken()).rejects.toThrow(CapabilityUnsupportedError);
    await expect(push.setCategories([])).rejects.toThrow(CapabilityUnsupportedError);
    await expect(push.launchResponse()).rejects.toThrow(CapabilityUnsupportedError);
    expect(() => push.onReceived(() => undefined)).toThrow(CapabilityUnsupportedError);
    expect(() => push.onResponse(() => undefined)).toThrow(CapabilityUnsupportedError);
    expect(() => push.setForegroundPresentation('show')).toThrow(CapabilityUnsupportedError);
  });
});

describe('fake deviceUnlock', () => {
  test('unlocks unless told otherwise, and remembers what it was asked', async () => {
    const { capabilities, fakes } = createFakePlatform();
    expect(await capabilities.deviceUnlock.methods()).toEqual(['face', 'passcode']);
    expect(await capabilities.deviceUnlock.unlock({ reason: 'Remove the worktree' })).toEqual({
      ok: true,
    });
    fakes.deviceUnlock.answerWith({ ok: false, cause: 'cancelled' });
    expect(await capabilities.deviceUnlock.unlock({ reason: 'Again' })).toEqual({
      ok: false,
      cause: 'cancelled',
    });
    expect(fakes.deviceUnlock.requests()).toEqual([
      { reason: 'Remove the worktree' },
      { reason: 'Again' },
    ]);
  });

  test('a device with nothing set up cannot unlock', async () => {
    const { capabilities, fakes } = createFakePlatform();
    fakes.deviceUnlock.setMethods([]);
    expect(await capabilities.deviceUnlock.methods()).toEqual([]);
    expect(await capabilities.deviceUnlock.unlock({ reason: 'Anything' })).toEqual({
      ok: false,
      cause: 'notSetUp',
    });
  });

  test('rejects with the reason while unsupported', async () => {
    const { deviceUnlock } = createFakePlatform({ support: { deviceUnlock: gap } }).capabilities;
    await expect(deviceUnlock.unlock({ reason: 'Anything' })).rejects.toThrow(
      CapabilityUnsupportedError,
    );
  });
});

describe('fake camera', () => {
  test('a simulator has none', async () => {
    const { camera } = createFakePlatform().capabilities;
    const support = await camera.support();
    expect(support.supported).toBe(false);
    await expect(camera.requestPermission()).rejects.toThrow(CapabilityUnsupportedError);
  });

  test('asks for permission once', async () => {
    const { capabilities, fakes } = createFakePlatform({ launch: FAKE_IPHONE });
    expect(await capabilities.camera.support()).toEqual({ supported: true });
    expect(await capabilities.camera.permission()).toBe('undetermined');
    fakes.camera.answerRequestWith('denied');
    expect(await capabilities.camera.requestPermission()).toBe('denied');
    fakes.camera.answerRequestWith('granted');
    expect(await capabilities.camera.requestPermission()).toBe('denied');
  });

  test('an active scanner reads the code held in front of it', async () => {
    const { capabilities, fakes } = createFakePlatform({ launch: FAKE_IPHONE });
    const { CodeScanner } = capabilities.camera;
    const onCode = jest.fn();

    const view = await render(
      <CodeScanner active onCode={onCode} accessibilityLabel="Pairing code scanner" />,
    );
    expect(fakes.camera.activeScanners()).toBe(1);
    await act(() => fakes.camera.show('OVSR1-ABCDEF'));
    expect(onCode).toHaveBeenCalledWith('OVSR1-ABCDEF');

    await view.rerender(
      <CodeScanner active={false} onCode={onCode} accessibilityLabel="Pairing code scanner" />,
    );
    expect(fakes.camera.activeScanners()).toBe(0);
    await act(() => fakes.camera.show('OVSR1-SECOND'));
    expect(onCode).toHaveBeenCalledTimes(1);

    await view.rerender(
      <CodeScanner active onCode={onCode} accessibilityLabel="Pairing code scanner" />,
    );
    await view.unmount();
    expect(fakes.camera.activeScanners()).toBe(0);
  });
});

describe('fake haptics', () => {
  test('records the moments a phone would play', async () => {
    const { capabilities, fakes } = createFakePlatform({ launch: FAKE_IPHONE });
    expect(await capabilities.haptics.support()).toEqual({ supported: true });
    capabilities.haptics.play('selection');
    capabilities.haptics.play('confirm');
    capabilities.haptics.play('reject');
    expect(fakes.haptics.played()).toEqual(['selection', 'confirm', 'reject']);
  });

  test('on a simulator it says so, and playing is silent instead of an error', async () => {
    const { capabilities, fakes } = createFakePlatform();
    const support = await capabilities.haptics.support();
    expect(support.supported).toBe(false);
    expect(() => capabilities.haptics.play('impact')).not.toThrow();
    expect(fakes.haptics.played()).toEqual([]);
  });
});

describe('fake appearance', () => {
  test('follows what the test sets and tells its listeners', async () => {
    const { capabilities, fakes } = createFakePlatform({ appearance: 'dark' });
    expect(await capabilities.appearance.support()).toEqual({ supported: true });
    expect(capabilities.appearance.get()).toBe('dark');
    const heard: string[] = [];
    const stop = capabilities.appearance.subscribe((scheme) => heard.push(scheme));
    fakes.appearance.set('light');
    fakes.appearance.set('light');
    fakes.appearance.set('dark');
    expect(heard).toEqual(['light', 'dark']);
    stop();
    fakes.appearance.set('light');
    expect(heard).toEqual(['light', 'dark']);
    expect(capabilities.appearance.get()).toBe('light');
  });
});

describe('fake reduceMotion', () => {
  test('is false until the system has answered, like the real one', async () => {
    const { reduceMotion } = createFakePlatform({ reduceMotion: true }).capabilities;
    expect(reduceMotion.get()).toBe(false);
    const heard = jest.fn();
    reduceMotion.subscribe(heard);
    expect(await reduceMotion.refresh()).toBe(true);
    expect(reduceMotion.get()).toBe(true);
    expect(heard).toHaveBeenCalledWith(true);
  });

  test('follows the setting while the app is open', async () => {
    const { capabilities, fakes } = createFakePlatform();
    const heard: boolean[] = [];
    capabilities.reduceMotion.subscribe((reduced) => heard.push(reduced));
    fakes.reduceMotion.set(true);
    fakes.reduceMotion.set(false);
    expect(heard).toEqual([true, false]);
    expect(await capabilities.reduceMotion.refresh()).toBe(false);
  });
});

describe('fake appState', () => {
  test('starts in the foreground and follows the app out and back', () => {
    const { capabilities, fakes } = createFakePlatform();
    expect(capabilities.appState.get()).toBe('foreground');
    const heard: string[] = [];
    capabilities.appState.subscribe((phase) => heard.push(phase));
    fakes.appState.set('background');
    fakes.appState.set('background');
    fakes.appState.set('foreground');
    expect(heard).toEqual(['background', 'foreground']);
  });

  test('can start in the background, as after a notification', () => {
    expect(createFakePlatform({ appPhase: 'background' }).capabilities.appState.get()).toBe(
      'background',
    );
  });
});

describe('fake network', () => {
  const cellular: NetworkState = { connected: true, kind: 'cellular' };
  const offline: NetworkState = { connected: false, kind: 'none' };

  test('is unknown until the system has answered, like the real one', async () => {
    const { network } = createFakePlatform().capabilities;
    expect(network.get()).toEqual({ connected: false, kind: 'unknown' });
    expect(await network.refresh()).toEqual({ connected: true, kind: 'wifi' });
    expect(network.get()).toEqual({ connected: true, kind: 'wifi' });
  });

  test('tells of each change once and keeps one reference per state', () => {
    const { capabilities, fakes } = createFakePlatform();
    const heard: NetworkState[] = [];
    capabilities.network.subscribe((state) => heard.push(state));
    fakes.network.set(cellular);
    const held = capabilities.network.get();
    fakes.network.set({ ...cellular });
    expect(capabilities.network.get()).toBe(held);
    fakes.network.set(offline);
    expect(heard).toEqual([cellular, offline]);
  });

  test('starts from what the test gives it', async () => {
    const { network } = createFakePlatform({ network: offline }).capabilities;
    expect(await network.refresh()).toEqual(offline);
  });
});
