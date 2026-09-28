import { act, fireEvent, screen } from '@testing-library/react-native';

import { createLive, unsupported, useLive } from '@/platform';
import { createTestApp, type TestApp } from '@/testing';

import { AppLock } from '../AppLock';

jest.mock('expo-router', () => require('@/testing/router').mockRouter());

/** The lock under a door that the test opens. */
const door = createLive(true);
function UnderTheDoor() {
  return <AppLock behindDoor={useLive(door)} />;
}

async function locked(options: Parameters<typeof createTestApp>[0] = {}): Promise<TestApp> {
  const app = await createTestApp(options);
  app.platform.fakes.keyValue.items.set('settings.appLock', 'true');
  return app;
}

describe('the app lock (AC-130)', () => {
  test('off, as it is unless the owner turns it on: nothing covers the app and nothing is asked', async () => {
    const app = await createTestApp();
    await app.render(<AppLock behindDoor={false} />);
    expect(screen.queryByTestId('lock.screen')).toBeNull();
    expect(app.platform.fakes.deviceUnlock.requests()).toHaveLength(0);
  });

  test('on: the app is covered from the first frame and the device unlock is asked for once', async () => {
    const app = await locked();
    app.platform.fakes.deviceUnlock.answerWith({ ok: true });
    await app.render(<AppLock behindDoor={false} />);
    await app.settle();
    expect(app.platform.fakes.deviceUnlock.requests()).toEqual([{ reason: 'Unlock Overseer' }]);
    expect(screen.queryByTestId('lock.screen')).toBeNull();
  });

  test('a failed unlock keeps it covered and says so; Unlock asks again', async () => {
    const app = await locked();
    app.platform.fakes.deviceUnlock.answerWith({ ok: false, cause: 'failed' });
    await app.render(<AppLock behindDoor={false} />);
    await app.settle();
    expect(screen.getByTestId('lock.screen')).toBeTruthy();
    expect(screen.getByTestId('lock.failed')).toHaveTextContent('The unlock did not work. Try again.');
    app.platform.fakes.deviceUnlock.answerWith({ ok: true });
    await fireEvent.press(screen.getByTestId('lock.unlock'));
    await app.settle();
    expect(app.platform.fakes.deviceUnlock.requests()).toHaveLength(2);
    expect(screen.queryByTestId('lock.screen')).toBeNull();
  });

  test('a cancelled unlock is not asked for again by itself', async () => {
    const app = await locked();
    app.platform.fakes.deviceUnlock.answerWith({ ok: false, cause: 'cancelled' });
    await app.render(<AppLock behindDoor={false} />);
    await app.settle();
    await app.settle();
    expect(app.platform.fakes.deviceUnlock.requests()).toHaveLength(1);
    expect(screen.getByTestId('lock.screen')).toBeTruthy();
    expect(screen.queryByTestId('lock.failed')).toBeNull();
  });

  test('the prompt waits for the door to open', async () => {
    const app = await locked();
    app.platform.fakes.deviceUnlock.answerWith({ ok: true });
    await app.render(<UnderTheDoor />);
    await app.settle();
    expect(app.platform.fakes.deviceUnlock.requests()).toHaveLength(0);
    expect(screen.getByTestId('lock.screen')).toBeTruthy();
    await act(async () => door.set(false));
    await app.settle();
    expect(app.platform.fakes.deviceUnlock.requests()).toHaveLength(1);
    expect(screen.queryByTestId('lock.screen')).toBeNull();
  });

  test('leaving the app covers it again; coming back asks again', async () => {
    const app = await locked();
    app.platform.fakes.deviceUnlock.answerWith({ ok: true });
    await app.render(<AppLock behindDoor={false} />);
    await app.settle();
    expect(screen.queryByTestId('lock.screen')).toBeNull();
    await act(async () => app.platform.fakes.appState.set('background'));
    expect(screen.getByTestId('lock.screen')).toBeTruthy();
    await act(async () => app.platform.fakes.appState.set('foreground'));
    await app.settle();
    expect(app.platform.fakes.deviceUnlock.requests()).toHaveLength(2);
    expect(screen.queryByTestId('lock.screen')).toBeNull();
  });

  test('a device that can no longer unlock is not locked out', async () => {
    const app = await locked({ support: { deviceUnlock: unsupported('No passcode, face or fingerprint is set up on this device.') } });
    await app.render(<AppLock behindDoor={false} />);
    await app.settle();
    expect(app.platform.fakes.deviceUnlock.requests()).toHaveLength(0);
    expect(screen.queryByTestId('lock.screen')).toBeNull();
  });
});
