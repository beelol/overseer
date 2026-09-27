/**
 * What a screen's test starts from: the fakes of the platform layer, a connection the test
 * drives by hand, and the app's providers around the screen. Tests only; the app never loads it.
 */
import { act, render } from '@testing-library/react-native';
import type { ReactElement } from 'react';
import { SafeAreaProvider } from 'react-native-safe-area-context';

import type { DaemonEvent } from '@/model';
import { PlatformProvider } from '@/platform';
import { createFakePlatform, type FakeOptions, type FakePlatform } from '@/platform/fake';
import type { State } from '@/protocol';
import { Session, SessionProvider, type SessionCache } from '@/session';

import { FAKE_GATEWAY, FakeConnection } from './FakeConnection';

export { FAKE_GATEWAY, FakeConnection };

const frame = { x: 0, y: 0, width: 402, height: 874 };
const insets = { top: 0, left: 0, right: 0, bottom: 0 };

export interface TestAppOptions extends FakeOptions {
  /** The daemon's state the Mac answers with. */
  readonly state?: State;
  /** False leaves the phone unpaired. Paired and connected unless given. */
  readonly paired?: boolean;
  /** `watch` for a phone that may only watch. */
  readonly scope?: 'full' | 'watch';
  /** The connection's state once started. `online` unless given. */
  readonly connection?: FakeConnection['state'];
}

export interface TestApp {
  readonly platform: FakePlatform;
  readonly connection: FakeConnection;
  readonly session: Session;
  /** Draws `ui` inside the app's providers. */
  render(ui: ReactElement): ReturnType<typeof render>;
  /** Events arrive from the Mac and the frame that applies them is drawn. */
  events(...events: DaemonEvent[]): Promise<void>;
  /** Lets promises and the next frame run. */
  settle(): Promise<void>;
  setTime(ms: number): void;
}

/** An empty daemon state, for tests that add their own records. */
export const EMPTY_STATE = { cursor: 0, daemon: { version: '0.0.0', protocol: 1 }, tasks: [], runs: [], workspaces: [], profiles: [], turns: [] } as unknown as State;

let seq = 1_000;
/** An event as the daemon sends it, with the next sequence number. */
export function makeEvent(kind: string, payload: unknown, ids: { run_id?: string; task_id?: string; source?: string; ts?: number } = {}): DaemonEvent {
  seq += 1;
  return { seq, ts: ids.ts ?? seq, kind, source: ids.source ?? 'daemon', confidence: 'exact', payload, run_id: ids.run_id ?? null, task_id: ids.task_id ?? null } as DaemonEvent;
}

export async function createTestApp(options: TestAppOptions = {}): Promise<TestApp> {
  const platform = createFakePlatform(options);
  const connection = new FakeConnection();
  const state = options.state ?? EMPTY_STATE;
  connection.answers['state'] = () => state;
  connection.answers['events.list'] = () => ({ events: [] });
  if (options.paired !== false) {
    connection.gateway = { ...FAKE_GATEWAY, scope: options.scope ?? 'full' };
    connection.hello = { device: { id: 'd1', name: 'Phone', scope: options.scope ?? 'full', platform: 'ios' }, mac_notifications: true };
    connection.lastContact = 1_000;
  }
  const frames: (() => void)[] = [];
  let now = 1_000_000;
  const session = new Session({
    connection,
    cache: platform.capabilities.keyValue.scope<SessionCache>('cache'),
    now: () => now,
    nextFrame: (callback) => {
      frames.push(callback);
      return () => void frames.splice(frames.indexOf(callback), 1);
    },
    cacheEveryMs: 0,
  });
  const settle = async (): Promise<void> => {
    await act(async () => {
      await new Promise((resolve) => setImmediate(resolve));
      frames.splice(0).forEach((f) => f());
      await new Promise((resolve) => setImmediate(resolve));
    });
  };
  await session.start();
  if (options.paired !== false) connection.go(options.connection ?? 'online');
  await new Promise((resolve) => setImmediate(resolve));
  return {
    platform,
    connection,
    session,
    render: async (ui) => {
      const drawn = await render(
        <SafeAreaProvider initialMetrics={{ frame, insets }}>
          <PlatformProvider capabilities={platform.capabilities}>
            <SessionProvider session={session}>{ui}</SessionProvider>
          </PlatformProvider>
        </SafeAreaProvider>,
      );
      await settle();
      return drawn;
    },
    events: async (...events) => {
      await act(async () => {
        for (const event of events) connection.emit('event', event, { live: true });
        frames.splice(0).forEach((f) => f());
      });
    },
    settle,
    setTime: (ms) => void (now = ms),
  };
}

/**
 * The router a screen's test uses in place of expo-router's:
 *
 *   jest.mock('expo-router', () => require('@/testing/router').mockRouter());
 *   import { router } from '@/testing/router';   // router.pushed, router.replaced, router.params
 */
