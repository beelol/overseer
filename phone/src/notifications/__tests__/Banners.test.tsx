import { act, fireEvent, screen } from '@testing-library/react-native';

import type { DaemonEvent } from '@/model';
import { unsupported } from '@/platform';
import { recording } from '@/screens/conversation/testing';
import { createTestApp, type TestApp } from '@/testing';

import { Notifications } from '../Notifications';

jest.mock('expo-router', () => require('@/testing/router').mockRouter());

const r = recording('permission-allow');
const upTo = (seq: number): DaemonEvent[] => r.events.filter((e) => e.seq <= seq) as DaemonEvent[];
const between = (from: number, to: number): DaemonEvent[] => r.events.filter((e) => e.seq > from && e.seq <= to) as DaemonEvent[];
/** The permission request and the agent waiting for it: events 11 and 12 of the recording. */
const ASKING = 12;

async function open(): Promise<TestApp> {
  // A phone without push, as Android is in this gate: the app shows what needs the owner itself.
  const app = await createTestApp({ state: r.initial, support: { push: unsupported('Push is not built for Android in this gate.') } });
  await app.render(<Notifications />);
  await app.settle();
  return app;
}

describe('a banner for what needs the owner (AC-129)', () => {
  test('stays until the agent no longer waits', async () => {
    const app = await open();
    await app.events(...upTo(ASKING));
    expect(screen.getByTestId('notification.banner')).toBeTruthy();
    // Longer than a banner for news stays.
    await act(() => new Promise((resolve) => setTimeout(resolve, 6_500)));
    expect(screen.getByTestId('notification.banner')).toBeTruthy();
    await app.events(...between(ASKING, 14));
    expect(screen.queryByTestId('notification.banner')).toBeNull();
  }, 15_000);

  test('can be dismissed without opening the agent', async () => {
    const app = await open();
    await app.events(...upTo(ASKING));
    await fireEvent.press(screen.getByTestId('notification.banner.dismiss'));
    expect(screen.queryByTestId('notification.banner')).toBeNull();
  });
});
