/**
 * The parts of the device's own implementations that differ per platform, run on the Mac with
 * the libraries mocked. Tests inside the platform layer may name a platform's file.
 */
import type { LaunchCapability } from '@/platform';

type Device = { isDevice: boolean; osVersion: string | null; modelName: string | null };

function withDevice<T>(device: Device, load: () => T): T {
  let loaded: T | undefined;
  jest.isolateModules(() => {
    jest.doMock('expo-device', () => device);
    loaded = load();
  });
  return loaded as T;
}

const simulator: Device = { isDevice: false, osVersion: '26.5', modelName: 'iPhone 17 Pro' };
const emulator: Device = { isDevice: false, osVersion: '15', modelName: 'sdk_gphone64_arm64' };
const phone: Device = { isDevice: true, osVersion: '26.5', modelName: 'iPhone 17 Pro' };

function iosLaunch(device: Device): LaunchCapability {
  return withDevice(device, () =>
    (require('../native/launch.ios') as typeof import('../native/launch.ios')).createLaunch(),
  );
}

function androidLaunch(device: Device): LaunchCapability {
  return withDevice(device, () =>
    (
      require('../native/launch.android') as typeof import('../native/launch.android')
    ).createLaunch(),
  );
}

describe('launch', () => {
  test('the iOS simulator reaches the Mac at 127.0.0.1', async () => {
    const launch = iosLaunch(simulator);
    expect(launch.name).toBe('launch');
    expect(await launch.support()).toEqual({ supported: true });
    expect(launch.info()).toMatchObject({
      device: { platform: 'ios', systemVersion: '26.5', model: 'iPhone 17 Pro' },
      isSimulator: true,
      hostAddresses: ['127.0.0.1'],
    });
    expect(launch.info()).toBe(launch.info());
  });

  test('the Android emulator reaches the Mac at 10.0.2.2', () => {
    expect(androidLaunch(emulator).info()).toMatchObject({
      device: { platform: 'android', systemVersion: '15', model: 'sdk_gphone64_arm64' },
      isSimulator: true,
      hostAddresses: ['10.0.2.2'],
    });
  });

  test('a real phone adds no address, on either platform', () => {
    expect(iosLaunch(phone).info()).toMatchObject({ isSimulator: false, hostAddresses: [] });
    expect(androidLaunch(phone).info()).toMatchObject({ isSimulator: false, hostAddresses: [] });
  });

  test('says unknown where the system says nothing', () => {
    const info = iosLaunch({ isDevice: true, osVersion: null, modelName: null }).info();
    expect(info.device).toEqual({ platform: 'ios', systemVersion: 'unknown', model: 'unknown' });
  });

  test('reports the engine and the architecture the runtime itself declares', () => {
    const runtime = globalThis as {
      HermesInternal?: object;
      RN$Bridgeless?: boolean;
      nativeFabricUIManager?: object;
    };
    expect(iosLaunch(simulator).info().runtime).toEqual({
      engine: 'other',
      newArchitecture: false,
    });
    runtime.HermesInternal = {};
    runtime.RN$Bridgeless = true;
    runtime.nativeFabricUIManager = {};
    try {
      expect(iosLaunch(simulator).info().runtime).toEqual({
        engine: 'hermes',
        newArchitecture: true,
      });
    } finally {
      delete runtime.HermesInternal;
      delete runtime.RN$Bridgeless;
      delete runtime.nativeFabricUIManager;
    }
  });
});

describe('discovery browsing', () => {
  function browser(file: 'ios' | 'android', device: Device) {
    return withDevice(device, () => {
      const launch = file === 'ios' ? iosLaunchInside() : androidLaunchInside();
      const module =
        file === 'ios'
          ? (require('../native/discoveryBrowser.ios') as typeof import('../native/discoveryBrowser.ios'))
          : (require('../native/discoveryBrowser.android') as typeof import('../native/discoveryBrowser.android'));
      return module.createDiscoveryBrowser(launch);
    });
  }
  const iosLaunchInside = () =>
    (require('../native/launch.ios') as typeof import('../native/launch.ios')).createLaunch();
  const androidLaunchInside = () =>
    (
      require('../native/launch.android') as typeof import('../native/launch.android')
    ).createLaunch();

  test.each([
    ['ios', simulator, '127.0.0.1'],
    ['android', emulator, '10.0.2.2'],
  ] as const)(
    'on the %s simulator it reports the gap and names the address that works',
    (file, device, address) => {
      const support = browser(file, device).support();
      expect(support.supported).toBe(false);
      expect(support).toMatchObject({ reason: expect.stringContaining(address) });
    },
  );

  test.each([
    ['ios', phone],
    ['android', phone],
  ] as const)(
    'on a real %s phone it reports the gap and points to a manual address',
    (file, device) => {
      const found = browser(file, device);
      expect(found.support()).toMatchObject({
        supported: false,
        reason: expect.stringContaining('by hand'),
      });
      expect(() =>
        found.browse({ serviceType: '_overseer._tcp', defaultPort: 47810 }, () => undefined),
      ).toThrow('discovery is not supported here');
    },
  );
});

describe('push on Android in this gate', () => {
  test('reports unsupported with the reason, and every call says the same', async () => {
    const { createPush } =
      require('../native/push.android') as typeof import('../native/push.android');
    const push = createPush();
    const support = await push.support();
    expect(support).toMatchObject({
      supported: false,
      reason: expect.stringContaining('Firebase'),
    });
    await expect(push.requestPermission()).rejects.toThrow('push is not supported here');
    await expect(push.deviceToken()).rejects.toThrow('push is not supported here');
    expect(() => push.onReceived(() => undefined)).toThrow('push is not supported here');
  });
});

describe('haptics', () => {
  const moments = ['selection', 'confirm', 'reject', 'warning', 'impact'] as const;

  function mockHaptics() {
    const calls: string[] = [];
    const record = (name: string) => (value?: string) => {
      calls.push(value === undefined ? name : `${name}:${value}`);
      return Promise.resolve();
    };
    jest.doMock('expo-haptics', () => ({
      selectionAsync: record('selection'),
      notificationAsync: record('notification'),
      impactAsync: record('impact'),
      performAndroidHapticsAsync: record('android'),
      NotificationFeedbackType: { Success: 'success', Warning: 'warning', Error: 'error' },
      ImpactFeedbackStyle: { Medium: 'medium' },
      AndroidHaptics: {
        Segment_Tick: 'segment-tick',
        Confirm: 'confirm',
        Reject: 'reject',
        Long_Press: 'long-press',
        Context_Click: 'context-click',
      },
    }));
    return calls;
  }

  test('iOS plays each moment with the feedback generators', () => {
    jest.isolateModules(() => {
      const calls = mockHaptics();
      const { playMoment } =
        require('../native/hapticsMoments.ios') as typeof import('../native/hapticsMoments.ios');
      for (const moment of moments) void playMoment(moment);
      expect(calls).toEqual([
        'selection',
        'notification:success',
        'notification:error',
        'notification:warning',
        'impact:medium',
      ]);
    });
  });

  test("Android plays each moment with the system's view haptics", () => {
    jest.isolateModules(() => {
      const calls = mockHaptics();
      const { playMoment } =
        require('../native/hapticsMoments.android') as typeof import('../native/hapticsMoments.android');
      for (const moment of moments) void playMoment(moment);
      expect(calls).toEqual([
        'android:segment-tick',
        'android:confirm',
        'android:reject',
        'android:long-press',
        'android:context-click',
      ]);
    });
  });

  test('on a simulator nothing is played and nothing fails', async () => {
    jest.isolateModules(() => {
      const calls = mockHaptics();
      jest.doMock('expo-device', () => simulator);
      const { createHaptics } = require('../native/haptics') as typeof import('../native/haptics');
      const { createLaunch } = require('../native/launch') as typeof import('../native/launch.ios');
      const haptics = createHaptics(createLaunch());
      haptics.play('confirm');
      expect(calls).toEqual([]);
    });
  });

  test('on a phone a failed buzz is swallowed', async () => {
    await new Promise<void>((done) => {
      jest.isolateModules(() => {
        jest.doMock('expo-device', () => phone);
        jest.doMock('../native/hapticsMoments', () => ({
          playMoment: () => Promise.reject(new Error('no engine')),
        }));
        const { createHaptics } =
          require('../native/haptics') as typeof import('../native/haptics');
        const { createLaunch } =
          require('../native/launch') as typeof import('../native/launch.ios');
        const haptics = createHaptics(createLaunch());
        expect(() => haptics.play('confirm')).not.toThrow();
        setImmediate(done);
      });
    });
  });
});
