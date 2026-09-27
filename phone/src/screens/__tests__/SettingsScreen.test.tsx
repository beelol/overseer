import { act, fireEvent, screen } from '@testing-library/react-native';
import { openSettings } from 'expo-linking';

import { text } from '@/model';
import { unsupported } from '@/platform';
import { SettingsScreen } from '@/screens/SettingsScreen';
import { dayOf, inGroupsOfFour } from '@/screens/settings/format';
import { createTestApp, FAKE_GATEWAY, type TestApp, type TestAppOptions } from '@/testing';
import { router } from '@/testing/router';

jest.mock('expo-router', () => require('@/testing/router').mockRouter());
jest.mock('expo-linking', () => ({ openSettings: jest.fn(async () => undefined) }));

const SWITCHES = {
  enabled: true,
  show_text: false,
  kinds: { permission: true, question: true, failure: true, finished: true },
};
type Switches = typeof SWITCHES;
type Change = {
  enabled?: boolean;
  show_text?: boolean;
  kinds?: Partial<Switches['kinds']>;
  token?: string;
};

/** The Mac's side of `device.notifications`: it keeps the switches and answers with them. */
function mac(app: TestApp, start: Switches = SWITCHES) {
  let held = start;
  app.connection.answers['device.notifications'] = (change: Change) => {
    held = {
      enabled: change.enabled ?? held.enabled,
      show_text: change.show_text ?? held.show_text,
      kinds: { ...held.kinds, ...change.kinds },
    };
    return { ...held, environment: 'simulator' };
  };
  return { held: () => held };
}

interface Options extends TestAppOptions {
  /** What the Mac said when the phone connected. */
  readonly hello?: Record<string, unknown>;
  /** What the owner answered the system about notifications before. */
  readonly permission?: 'granted' | 'denied';
  readonly before?: (app: TestApp) => void;
}

async function open({ hello, permission, before, ...options }: Options = {}): Promise<TestApp> {
  const app = await createTestApp(options);
  if (hello) {
    app.connection.hello = { ...app.connection.hello, ...hello };
    app.connection.go('online');
    await new Promise((resolve) => setImmediate(resolve));
  }
  if (permission) {
    app.platform.fakes.push.answerRequestWith(permission);
    await app.platform.capabilities.push.requestPermission();
  }
  before?.(app);
  await app.render(<SettingsScreen />);
  return app;
}

const at = (id: string) => screen.getByTestId(id);
/** True for a switch that cannot be flipped: the system's switch carries `disabled` itself. */
const off = (id: string): boolean => at(id).props.disabled === true;
async function flip(app: TestApp, id: string, value: boolean): Promise<void> {
  await fireEvent(at(id), 'valueChange', value);
  await app.settle();
}

const KINDS = ['permission', 'question', 'failure', 'finished'] as const;

beforeEach(() => router.reset());

describe('settings: what it shows', () => {
  test('this phone, the Mac with its fingerprint in groups of four, and the appearance', async () => {
    await open({ permission: 'granted' });
    expect(at('settings.title')).toHaveTextContent('Settings');
    expect(at('settings.phone.name').props.accessibilityLabel).toBe('Name, Phone');
    expect(at('settings.phone.scope').props.accessibilityLabel).toBe('Access, Full control');
    expect(at('settings.phone.paired').props.accessibilityLabel).toBe(
      `Paired since, ${dayOf(FAKE_GATEWAY.pairedAt)}`,
    );
    expect(at('settings.mac.name').props.accessibilityLabel).toBe('Name, Mac');
    expect(at('settings.mac.fingerprint').props.accessibilityLabel).toBe(
      'Fingerprint, 0123 4567 89ab cdef',
    );
    expect(at('settings.appearance').props.accessibilityLabel).toBe('Follows your phone.');
    expect(screen.getByText('NOTIFICATIONS')).toBeTruthy();
    expect(screen.getByText('THIS PHONE')).toBeTruthy();
    expect(screen.getByText('THE MAC')).toBeTruthy();
    expect(screen.getByText('APPEARANCE')).toBeTruthy();
    expect(screen.getByText('SAFETY')).toBeTruthy();
  });

  test('the fingerprint and the day read as a person compares them', () => {
    expect(inGroupsOfFour('0123456789abcdef')).toBe('0123 4567 89ab cdef');
    expect(inGroupsOfFour('0123456789')).toBe('0123 4567 89');
    expect(inGroupsOfFour('')).toBe('');
    expect(dayOf(Date.UTC(2026, 8, 26, 12))).toMatch(/2026/);
  });

  test('the last contact is said in words', async () => {
    const app = await open({ permission: 'granted' });
    const seen = Date.now() - 5 * 60_000 - 5_000;
    await act(async () => {
      app.connection.lastContact = seen;
      app.connection.go('unreachable');
    });
    expect(text.agoInWords(seen, Date.now())).toBe('5m ago');
    expect(at('settings.mac.contact').props.accessibilityLabel).toBe('Last contact, 5m ago');
    expect(screen.getByTestId('connection.unreachable')).toBeTruthy();
  });

  test('a watch-only phone says so, and where to change it', async () => {
    const app = await open({ scope: 'watch', permission: 'granted' });
    mac(app);
    expect(at('settings.phone.scope').props.accessibilityLabel).toBe(
      'Access, Watch only, Change it on the Mac.',
    );
    // Its own notifications and its own pairing are the phone's to change.
    expect(off('settings.notifications.all')).toBe(false);
    for (const kind of KINDS) expect(off(`settings.notifications.${kind}`)).toBe(false);
    await flip(app, 'settings.notifications.finished', false);
    expect(app.connection.calls('device.notifications')).toEqual([{ kinds: { finished: false } }]);
    expect(at('settings.forget')).toBeTruthy();
  });
});

describe('settings: notifications', () => {
  test('the switches are the ones the Mac holds for this phone', async () => {
    await open({
      permission: 'granted',
      hello: {
        notifications: {
          enabled: true,
          show_text: true,
          kinds: { permission: true, question: false, failure: true, finished: false },
        },
      },
    });
    expect(at('settings.notifications.all')).toHaveProp('value', true);
    expect(at('settings.notifications.permission')).toHaveProp('value', true);
    expect(at('settings.notifications.question')).toHaveProp('value', false);
    expect(at('settings.notifications.failure')).toHaveProp('value', true);
    expect(at('settings.notifications.finished')).toHaveProp('value', false);
    expect(at('settings.notifications.text')).toHaveProp('value', true);
    expect(screen.getByLabelText('Permission requests')).toBeTruthy();
    expect(screen.getByLabelText('Questions')).toBeTruthy();
    expect(screen.getByLabelText('Errors')).toBeTruthy();
    expect(screen.getByLabelText('Finished')).toBeTruthy();
    expect(screen.getByLabelText('Show text in notifications')).toBeTruthy();
    expect(screen.queryByTestId('settings.notifications.mac')).toBeNull();
    expect(screen.queryByTestId('settings.notifications.allow')).toBeNull();
  });

  test('text in notifications is off until turned on', async () => {
    const app = await open({ permission: 'granted' });
    const held = mac(app);
    expect(at('settings.notifications.text')).toHaveProp('value', false);
    await flip(app, 'settings.notifications.text', true);
    expect(app.connection.calls('device.notifications')).toEqual([{ show_text: true }]);
    expect(at('settings.notifications.text')).toHaveProp('value', true);
    expect(held.held().show_text).toBe(true);
  });

  test('each kind is changed by itself', async () => {
    const app = await open({ permission: 'granted' });
    mac(app);
    for (const kind of KINDS) await flip(app, `settings.notifications.${kind}`, false);
    expect(app.connection.calls('device.notifications')).toEqual(
      KINDS.map((kind) => ({ kinds: { [kind]: false } })),
    );
    for (const kind of KINDS)
      expect(at(`settings.notifications.${kind}`)).toHaveProp('value', false);
    expect(at('settings.notifications.all')).toHaveProp('value', true);
  });

  test('all off shows at once, and the kinds wait for it to be on again', async () => {
    const app = await open({ permission: 'granted' });
    let answer: (value: unknown) => void = () => undefined;
    app.connection.answers['device.notifications'] = () =>
      new Promise((resolve) => (answer = resolve));
    await flip(app, 'settings.notifications.all', false);
    // The Mac has not answered yet.
    expect(at('settings.notifications.all')).toHaveProp('value', false);
    for (const kind of KINDS) expect(off(`settings.notifications.${kind}`)).toBe(true);
    expect(off('settings.notifications.text')).toBe(true);
    expect(off('settings.notifications.all')).toBe(false);

    await act(async () => answer({ ...SWITCHES, enabled: false, environment: 'simulator' }));
    await app.settle();
    expect(at('settings.notifications.all')).toHaveProp('value', false);
    expect(app.session.getSnapshot().notifications.enabled).toBe(false);
    expect(screen.queryByTestId('settings.notifications.refused')).toBeNull();
  });

  test('a change the Mac refuses is put back, and it says so', async () => {
    const app = await open({ permission: 'granted' });
    app.connection.answers['device.notifications'] = () => {
      throw new Error('refused');
    };
    await flip(app, 'settings.notifications.question', false);
    expect(at('settings.notifications.question')).toHaveProp('value', true);
    expect(at('settings.notifications.refused')).toHaveTextContent(
      'The Mac did not take this change.',
    );
    expect(app.session.getSnapshot().notifications.kinds.question).toBe(true);

    // The next change that the Mac takes clears the sentence.
    mac(app);
    await flip(app, 'settings.notifications.question', false);
    expect(at('settings.notifications.question')).toHaveProp('value', false);
    expect(screen.queryByTestId('settings.notifications.refused')).toBeNull();
  });

  test('a Mac that cannot be reached refuses the same way', async () => {
    const app = await open({ permission: 'granted', connection: 'unreachable' });
    delete app.connection.answers['device.notifications'];
    await flip(app, 'settings.notifications.all', false);
    expect(at('settings.notifications.all')).toHaveProp('value', true);
    expect(at('settings.notifications.refused')).toBeTruthy();
  });

  test('when the Mac turned them off for every phone it says so and nothing can be switched', async () => {
    const app = await open({ permission: 'granted', hello: { mac_notifications: false } });
    expect(at('settings.notifications.mac')).toHaveTextContent(
      'Notifications are off on the Mac for every phone.',
    );
    for (const id of ['all', ...KINDS, 'text']) {
      expect(off(`settings.notifications.${id}`)).toBe(true);
      expect(at(`settings.notifications.${id}`)).toHaveProp('value', false);
    }
    expect(screen.queryByTestId('settings.notifications.allow')).toBeNull();
    expect(app.connection.calls('device.notifications')).toEqual([]);
    // What this phone chose is kept for when the Mac turns them on again.
    expect(app.session.getSnapshot().notifications.enabled).toBe(true);
  });

  test('a phone that was never asked is offered Allow notifications, with the reason', async () => {
    const app = await open();
    mac(app);
    expect(at('settings.notifications.reason')).toHaveTextContent(
      'Overseer can tell you when an agent needs you.',
    );
    expect(app.platform.fakes.push.prompts()).toBe(0);
    await fireEvent.press(at('settings.notifications.allow'));
    await app.settle();
    expect(app.platform.fakes.push.prompts()).toBe(1);
    // The owner said yes: notifications are on for this phone, and the Mac knows where to send.
    expect(app.connection.calls('device.notifications')).toEqual([
      { enabled: true, token: 'booted', environment: 'simulator' },
    ]);
    expect(screen.queryByTestId('settings.notifications.allow')).toBeNull();
    expect(openSettings).not.toHaveBeenCalled();
  });

  test('after a refusal only the system can allow them: it opens the system settings', async () => {
    const app = await open({ permission: 'denied' });
    expect(at('settings.notifications.reason')).toHaveTextContent(
      'Notifications are not allowed for Overseer on this phone.',
    );
    await fireEvent.press(at('settings.notifications.allow'));
    await app.settle();
    expect(openSettings).toHaveBeenCalledTimes(1);
    expect(app.platform.fakes.push.prompts()).toBe(1);
  });

  test('it looks again when the app comes back from the system settings', async () => {
    const app = await open();
    expect(at('settings.notifications.allow')).toBeTruthy();
    await act(async () => app.platform.fakes.appState.set('background'));
    await app.platform.capabilities.push.requestPermission();
    await act(async () => app.platform.fakes.appState.set('foreground'));
    await app.settle();
    expect(screen.queryByTestId('settings.notifications.allow')).toBeNull();
  });

  test('where the system delivers none it says why, and the switches still rule what the app shows', async () => {
    const app = await open({ support: { push: unsupported('No push in this test.') } });
    mac(app);
    expect(at('settings.notifications.why')).toHaveTextContent('No push in this test.');
    expect(screen.queryByTestId('settings.notifications.allow')).toBeNull();
    await flip(app, 'settings.notifications.failure', false);
    expect(at('settings.notifications.failure')).toHaveProp('value', false);
  });
});

describe('settings: safety', () => {
  test('both are off until turned on, and turning one on asks for the unlock and keeps it on the phone', async () => {
    const app = await open({ permission: 'granted' });
    const { keyValue, deviceUnlock } = app.platform.fakes;
    expect(at('settings.safety.lock')).toHaveProp('value', false);
    expect(at('settings.safety.unlock')).toHaveProp('value', false);
    expect(screen.getByLabelText('App lock')).toBeTruthy();
    expect(
      screen.getByLabelText('Ask for unlock before changes that cannot be undone'),
    ).toBeTruthy();
    expect(off('settings.safety.lock')).toBe(false);
    expect(keyValue.items.has('settings.appLock')).toBe(false);

    await flip(app, 'settings.safety.lock', true);
    expect(deviceUnlock.requests()).toHaveLength(1);
    expect(at('settings.safety.lock')).toHaveProp('value', true);
    expect(at('settings.safety.unlock')).toHaveProp('value', false);
    expect(keyValue.items.get('settings.appLock')).toBe('true');
    expect(keyValue.items.has('settings.unlockBeforeChanges')).toBe(false);

    await flip(app, 'settings.safety.unlock', true);
    expect(keyValue.items.get('settings.unlockBeforeChanges')).toBe('true');
    await flip(app, 'settings.safety.lock', false);
    expect(deviceUnlock.requests()).toHaveLength(3);
    expect(keyValue.items.get('settings.appLock')).toBe('false');
    expect(at('settings.safety.lock')).toHaveProp('value', false);
    expect(at('settings.safety.unlock')).toHaveProp('value', true);
  });

  test('what was turned on before is on when Settings opens', async () => {
    await open({
      permission: 'granted',
      before: (app) => app.platform.fakes.keyValue.items.set('settings.appLock', 'true'),
    });
    expect(at('settings.safety.lock')).toHaveProp('value', true);
    expect(at('settings.safety.unlock')).toHaveProp('value', false);
  });

  test('an unlock that failed changes nothing and says so; one that was dismissed says nothing', async () => {
    const app = await open({ permission: 'granted' });
    app.platform.fakes.deviceUnlock.answerWith({ ok: false, cause: 'failed' });
    await flip(app, 'settings.safety.lock', true);
    expect(at('settings.safety.lock')).toHaveProp('value', false);
    expect(at('settings.safety.failed')).toHaveTextContent(
      'The unlock did not work. Nothing was changed.',
    );
    expect(app.platform.fakes.keyValue.items.has('settings.appLock')).toBe(false);

    app.platform.fakes.deviceUnlock.answerWith({ ok: false, cause: 'cancelled' });
    await flip(app, 'settings.safety.unlock', true);
    expect(at('settings.safety.unlock')).toHaveProp('value', false);
    expect(screen.queryByTestId('settings.safety.failed')).toBeNull();
  });

  test('a device with no unlock says why and cannot turn them on', async () => {
    const app = await open({
      permission: 'granted',
      support: {
        deviceUnlock: unsupported('No passcode, face or fingerprint is set up on this device.'),
      },
    });
    expect(at('settings.safety.why')).toHaveTextContent(
      'No passcode, face or fingerprint is set up on this device.',
    );
    expect(off('settings.safety.lock')).toBe(true);
    expect(off('settings.safety.unlock')).toBe(true);
    expect(app.platform.fakes.deviceUnlock.requests()).toHaveLength(0);
  });
});

describe('settings: forget this Mac', () => {
  test('it asks once, naming what is lost, and Cancel forgets nothing', async () => {
    const app = await open({ permission: 'granted' });
    await fireEvent.press(at('settings.forget'));
    expect(screen.getByText('Forget this Mac?')).toBeTruthy();
    expect(screen.getByText('You will pair again to use Overseer here.')).toBeTruthy();
    await fireEvent.press(at('settings.forget.ask.cancel'));
    await app.settle();
    expect(app.session.getSnapshot().paired).toBe(true);
    expect(app.connection.gateway).not.toBeNull();
  });

  test('confirmed, the pairing is gone', async () => {
    const app = await open({ permission: 'granted' });
    await fireEvent.press(at('settings.forget'));
    await fireEvent.press(at('settings.forget.ask.confirm'));
    await app.settle();
    expect(app.connection.gateway).toBeNull();
    expect(app.session.getSnapshot().paired).toBe(false);
    expect(app.session.getSnapshot().gateway).toBeNull();
    expect(app.platform.fakes.deviceUnlock.requests()).toHaveLength(0);
    expect(screen.queryByTestId('settings.forget.failed')).toBeNull();
  });

  test('with the unlock before changes on, forgetting needs the unlock', async () => {
    const app = await open({
      permission: 'granted',
      before: (a) => a.platform.fakes.keyValue.items.set('settings.unlockBeforeChanges', 'true'),
    });
    app.platform.fakes.deviceUnlock.answerWith({ ok: false, cause: 'failed' });
    await fireEvent.press(at('settings.forget'));
    await fireEvent.press(at('settings.forget.ask.confirm'));
    await app.settle();
    expect(app.platform.fakes.deviceUnlock.requests()).toEqual([{ reason: 'Forget this Mac' }]);
    expect(app.session.getSnapshot().paired).toBe(true);
    expect(at('settings.forget.failed')).toHaveTextContent(
      'The unlock did not work. Nothing was changed.',
    );

    app.platform.fakes.deviceUnlock.answerWith({ ok: true });
    await fireEvent.press(at('settings.forget'));
    await fireEvent.press(at('settings.forget.ask.confirm'));
    await app.settle();
    expect(app.session.getSnapshot().paired).toBe(false);
    expect(screen.queryByTestId('settings.forget.failed')).toBeNull();
  });

  test('a Mac that could not be forgotten says so', async () => {
    const app = await open({ permission: 'granted' });
    app.connection.forget = async () => {
      throw new Error('the keystore is locked');
    };
    await fireEvent.press(at('settings.forget'));
    await fireEvent.press(at('settings.forget.ask.confirm'));
    await app.settle();
    expect(at('settings.forget.failed')).toHaveTextContent(
      'This Mac could not be forgotten. Try again.',
    );
    expect(app.session.getSnapshot().paired).toBe(true);
  });
});
