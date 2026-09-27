import { act, fireEvent, screen } from '@testing-library/react-native';

import {
  encodePairingCode,
  fingerprint,
  PairingError,
  type ConnectAttempt,
  type GatewayInfo,
} from '@/core';
import { unsupported } from '@/platform';
import { FAKE_IPHONE } from '@/platform/fake';
import { routes } from '@/routes';
import { PairScreen } from '@/screens/PairScreen';
import { createTestApp, FAKE_GATEWAY, type TestApp, type TestAppOptions } from '@/testing';
import { router } from '@/testing/router';

jest.mock('expo-router', () => require('@/testing/router').mockRouter());

const KEY = Uint8Array.from({ length: 32 }, (_, i) => i + 1);
const CODE = encodePairingCode(
  {
    gatewayPublicKey: KEY,
    secret: Uint8Array.from({ length: 16 }, (_, i) => 200 - i),
    port: 47810,
    addresses: ['192.168.1.20'],
  },
  { group: 4 },
);

const CODE_DID_NOT_WORK =
  'This code did not work. A code works once, for two minutes. Get a new one on the Mac.';
const COULD_NOT_REACH =
  'Could not reach the Mac. Check that the phone is on the same network and phone access is on.';

function attempt(outcome: ConnectAttempt['outcome'], host = '192.168.1.20'): ConnectAttempt {
  return {
    address: { host, port: 47810 },
    url: `ws://${host}:47810/v1`,
    kind: 'pairing',
    outcome,
    at: 1,
  };
}

/** A pairing the test ends by hand: the Mac's owner has not decided yet. */
function undecided(app: TestApp) {
  const calls: { code: string; name: string; platform: string }[] = [];
  let confirm: (gateway: GatewayInfo) => void = () => undefined;
  let decline: (error: unknown) => void = () => undefined;
  app.connection.pairing = (code, name, platform) => {
    calls.push({ code, name, platform });
    return new Promise<GatewayInfo>((resolve, reject) => {
      confirm = resolve;
      decline = reject;
    });
  };
  return {
    calls,
    confirm: async () => {
      confirm(FAKE_GATEWAY);
      await app.settle();
    },
    decline: async (error: unknown) => {
      decline(error);
      await app.settle();
    },
  };
}

async function open(options: TestAppOptions = {}): Promise<TestApp> {
  const app = await createTestApp({ paired: false, ...options });
  app.connection.answers['device.notifications'] = () => ({
    enabled: true,
    show_text: false,
    kinds: {},
    environment: 'simulator',
  });
  await app.render(<PairScreen />);
  return app;
}

async function typeAndPair(app: TestApp, code: string): Promise<void> {
  await fireEvent.changeText(screen.getByTestId('pair.code'), code);
  await fireEvent.press(screen.getByTestId('pair.submit'));
  await app.settle();
}

/** Holds a code in front of the camera. */
async function show(app: TestApp, text: string): Promise<void> {
  await act(async () => app.platform.fakes.camera.show(text));
  await app.settle();
}

beforeEach(() => router.reset());

describe('pair with your Mac', () => {
  test('a simulator has the field from the start, the phone named, and neither a way back nor a connection line', async () => {
    await open();
    expect(screen.getByTestId('pair.title')).toHaveTextContent('Pair with your Mac');
    expect(screen.getByTestId('pair.step.mac')).toHaveTextContent(
      'On the Mac, open Overseer and choose Pair a Phone.',
    );
    expect(screen.getByTestId('pair.step.scan')).toHaveTextContent('Scan the code.');
    expect(screen.getByTestId('pair.code')).toBeTruthy();
    expect(screen.getByTestId('pair.name').props.value).toBe('iPhone 17 Pro');
    expect(screen.queryByTestId('pair.back')).toBeNull();
    expect(screen.queryByTestId('pair.camera.allow')).toBeNull();
    expect(screen.queryByTestId('connection.reconnecting')).toBeNull();
    expect(screen.queryByTestId('connection.unreachable')).toBeNull();
    expect(screen.getByTestId('pair.submit')).toBeDisabled();
  });

  test('a typed code pairs: it waits for the Mac, asks for notifications with the reason first, then opens Agents', async () => {
    const app = await open();
    const mac = undecided(app);
    await typeAndPair(app, `  ${CODE.toLowerCase()}\n`);

    expect(mac.calls).toEqual([
      { code: CODE.toLowerCase(), name: 'iPhone 17 Pro', platform: 'ios' },
    ]);
    expect(screen.getByTestId('pair.waiting')).toHaveTextContent('Confirm on your Mac');
    expect(screen.queryByTestId('pair.mac')).toBeNull();
    expect(screen.queryByTestId('pair.code')).toBeNull();
    expect(router.replaced).toEqual([]);

    await mac.confirm();
    expect(app.session.getSnapshot().paired).toBe(true);
    expect(screen.getByTestId('pair.notifications.reason')).toHaveTextContent(
      'Overseer can tell you when an agent needs you.',
    );
    expect(app.platform.fakes.push.prompts()).toBe(0);
    expect(router.replaced).toEqual([]);

    await fireEvent.press(screen.getByTestId('pair.notifications.allow'));
    await app.settle();
    expect(app.platform.fakes.push.prompts()).toBe(1);
    // The owner said yes: notifications are on for this phone, and the Mac knows where to send,
    // once the first connection after pairing is made.
    expect(app.session.getSnapshot().notifications.enabled).toBe(true);
    expect(app.connection.calls('device.notifications')).toEqual([]);
    await act(async () => app.connection.go('online'));
    await app.settle();
    expect(app.connection.calls('device.notifications')).toEqual([
      { enabled: true, token: 'booted', environment: 'simulator' },
    ]);
    expect(router.replaced).toEqual([routes.agents]);
  });

  test('Not now asks the system nothing and opens Agents', async () => {
    const app = await open();
    await typeAndPair(app, CODE);
    await fireEvent.press(screen.getByTestId('pair.notifications.later'));
    await app.settle();
    expect(app.platform.fakes.push.prompts()).toBe(0);
    expect(app.connection.calls('device.notifications')).toEqual([]);
    expect(router.replaced).toEqual([routes.agents]);
  });

  test('where the system delivers no notifications nothing is asked', async () => {
    const app = await open({ support: { push: unsupported('No push in this test.') } });
    await typeAndPair(app, CODE);
    expect(screen.queryByTestId('pair.notifications.reason')).toBeNull();
    expect(router.replaced).toEqual([routes.agents]);
    // The app shows what needs the owner itself: its switch is on, and the Mac learns it at
    // the first connection after pairing, which is not made yet when pairing ends.
    expect(app.connection.calls('device.notifications')).toEqual([]);
    await act(async () => app.connection.go('online'));
    await app.settle();
    expect(app.connection.calls('device.notifications')).toEqual([{ enabled: true }]);
  });

  test('a phone that answered the system before is not asked again', async () => {
    const app = await open();
    app.platform.fakes.push.answerRequestWith('denied');
    await app.platform.capabilities.push.requestPermission();
    await typeAndPair(app, CODE);
    expect(screen.queryByTestId('pair.notifications.reason')).toBeNull();
    expect(app.platform.fakes.push.prompts()).toBe(1);
    expect(router.replaced).toEqual([routes.agents]);
  });

  test('the name can be changed, and an empty name is the device again', async () => {
    const app = await open();
    const mac = undecided(app);
    await fireEvent.changeText(screen.getByTestId('pair.name'), "  Bilal's phone ");
    await typeAndPair(app, CODE);
    expect(mac.calls[0]?.name).toBe("Bilal's phone");
    await mac.decline(new PairingError([attempt('refused')]));

    await fireEvent.changeText(screen.getByTestId('pair.name'), '   ');
    await typeAndPair(app, CODE);
    expect(mac.calls[1]?.name).toBe('iPhone 17 Pro');
  });

  test('what is not a pairing code is never sent', async () => {
    const app = await open();
    const mac = undecided(app);
    for (const wrong of ['hello', 'OVSR1-ABCD', 'OVSR2-ABCDEFGH', `${CODE}A`]) {
      await typeAndPair(app, wrong);
      expect(screen.getByTestId('pair.error')).toHaveTextContent(CODE_DID_NOT_WORK);
    }
    expect(mac.calls).toEqual([]);
    expect(screen.getByTestId('pair.code')).toBeTruthy();
  });

  test('a Mac that refused says the code did not work, and the code can be tried again', async () => {
    const app = await open();
    const mac = undecided(app);
    await typeAndPair(app, CODE);
    await mac.decline(new PairingError([attempt('unreachable', '10.0.0.9'), attempt('refused')]));

    expect(screen.getByTestId('pair.error')).toHaveTextContent(CODE_DID_NOT_WORK);
    expect(screen.queryByTestId('pair.waiting')).toBeNull();
    expect(app.session.getSnapshot().paired).toBe(false);
    expect(router.replaced).toEqual([]);

    await fireEvent.press(screen.getByTestId('pair.submit'));
    await app.settle();
    expect(mac.calls).toHaveLength(2);
    expect(screen.queryByTestId('pair.error')).toBeNull();
    expect(screen.getByTestId('pair.waiting')).toBeTruthy();
  });

  test('an impostor and a Mac of another version are the code that did not work', async () => {
    for (const outcome of ['impostor', 'incompatible'] as const) {
      const app = await open();
      const mac = undecided(app);
      await typeAndPair(app, CODE);
      await mac.decline(new PairingError([attempt(outcome)]));
      expect(screen.getByTestId('pair.error')).toHaveTextContent(CODE_DID_NOT_WORK);
      screen.unmount();
    }
  });

  test('a Mac that did not answer says it could not be reached', async () => {
    const app = await open();
    const mac = undecided(app);
    await typeAndPair(app, CODE);
    await mac.decline(new PairingError([attempt('unreachable'), attempt('timeout', '127.0.0.1')]));
    expect(screen.getByTestId('pair.error')).toHaveTextContent(COULD_NOT_REACH);
    expect(router.replaced).toEqual([]);
  });

  test('a code with no address that answered says the Mac could not be reached', async () => {
    const app = await open();
    const mac = undecided(app);
    await typeAndPair(app, CODE);
    await mac.decline(new PairingError([]));
    expect(screen.getByTestId('pair.error')).toHaveTextContent(COULD_NOT_REACH);
  });
});

describe('pair with your Mac, with a camera', () => {
  test('the camera is explained in one sentence before the system is asked, then it scans', async () => {
    const app = await open({ launch: FAKE_IPHONE });
    const mac = undecided(app);
    expect(screen.getByTestId('pair.camera.reason')).toHaveTextContent(
      'Overseer uses the camera to scan the code on your Mac.',
    );
    expect(app.platform.fakes.camera.activeScanners()).toBe(0);
    expect(screen.queryByTestId('pair.code')).toBeNull();

    await fireEvent.press(screen.getByTestId('pair.camera.allow'));
    await app.settle();
    expect(app.platform.fakes.camera.activeScanners()).toBe(1);

    // Another code in front of the camera is not Overseer's: nothing happens.
    await show(app, 'https://example.invalid/menu');
    expect(mac.calls).toEqual([]);
    expect(screen.queryByTestId('pair.error')).toBeNull();

    await act(async () => {
      app.platform.fakes.camera.show(CODE);
      app.platform.fakes.camera.show(CODE);
    });
    await app.settle();
    expect(mac.calls).toEqual([{ code: CODE, name: 'iPhone 17 Pro', platform: 'ios' }]);
    expect(screen.getByTestId('pair.waiting')).toBeTruthy();
    expect(app.platform.fakes.camera.activeScanners()).toBe(0);

    await mac.confirm();
    expect(app.platform.fakes.haptics.played()).toContain('confirm');
    expect(screen.getByTestId('pair.notifications.reason')).toBeTruthy();
  });

  test('Type the code is there beside the camera', async () => {
    const app = await open({ launch: FAKE_IPHONE });
    const mac = undecided(app);
    await fireEvent.press(screen.getByTestId('pair.type'));
    await typeAndPair(app, CODE);
    expect(mac.calls).toHaveLength(1);
  });

  test('a refused camera leaves the field, and says so', async () => {
    const app = await createTestApp({ paired: false, launch: FAKE_IPHONE });
    app.platform.fakes.camera.answerRequestWith('denied');
    await app.render(<PairScreen />);
    await fireEvent.press(screen.getByTestId('pair.camera.allow'));
    await app.settle();
    expect(screen.getByTestId('pair.camera.off')).toHaveTextContent(
      'The camera is not allowed for Overseer. Type the code.',
    );
    expect(screen.getByTestId('pair.code')).toBeTruthy();
    expect(app.platform.fakes.camera.activeScanners()).toBe(0);
  });

  test('after a code that failed the camera waits to be asked again', async () => {
    const app = await open({ launch: FAKE_IPHONE });
    const mac = undecided(app);
    await fireEvent.press(screen.getByTestId('pair.camera.allow'));
    await app.settle();
    await show(app, CODE);
    await mac.decline(new PairingError([attempt('refused')]));

    expect(screen.getByTestId('pair.error')).toHaveTextContent(CODE_DID_NOT_WORK);
    expect(app.platform.fakes.haptics.played()).toContain('reject');
    expect(app.platform.fakes.camera.activeScanners()).toBe(0);
    await fireEvent.press(screen.getByTestId('pair.scan'));
    await app.settle();
    expect(app.platform.fakes.camera.activeScanners()).toBe(1);
  });

  test('while it waits it names the Mac the network shows for the key of the code', async () => {
    const app = await open({ launch: FAKE_IPHONE });
    undecided(app);
    app.platform.fakes.discovery.announce([
      {
        host: '192.168.1.77',
        port: 47810,
        serviceName: 'Overseer on Other',
        txt: { fp: 'ffffffffffffffff' },
      },
      {
        host: '192.168.1.99',
        port: 47810,
        serviceName: 'Overseer on Studio',
        txt: { fp: fingerprint(KEY) },
      },
    ]);
    expect(app.platform.fakes.discovery.browsers()).toBe(0);
    await fireEvent.press(screen.getByTestId('pair.type'));
    await typeAndPair(app, CODE);
    expect(screen.getByTestId('pair.waiting')).toHaveTextContent('Confirm on your Mac');
    expect(screen.getByTestId('pair.mac')).toHaveTextContent('Studio');
    expect(app.platform.fakes.discovery.browsers()).toBe(1);
  });
});
