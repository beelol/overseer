import { act, fireEvent, screen } from '@testing-library/react-native';
import { setStringAsync } from 'expo-clipboard';
import { openURL } from 'expo-linking';

import { RequestError } from '@/core';
import type { Profile } from '@/model';
import type { State } from '@/protocol';
import { AccountsScreen } from '@/screens/AccountsScreen';
import {
  byProvider,
  hasCode,
  readList,
  readSignIn,
  readStatus,
  readUsage,
  resetTime,
  stateText,
  usageText,
  usageTone,
} from '@/screens/accounts/accounts';
import {
  createTestApp,
  EMPTY_STATE,
  makeEvent,
  type TestApp,
  type TestAppOptions,
} from '@/testing';
import { router } from '@/testing/router';
import { phone } from '@/theme/tokens.generated';

jest.mock('expo-router', () => require('@/testing/router').mockRouter());
jest.mock('expo-linking', () => ({ openURL: jest.fn(async () => true) }));
jest.mock('expo-clipboard', () => ({ setStringAsync: jest.fn(async () => true) }));

const profile = (
  id: string,
  name: string,
  harness: string,
  is_system = false,
  created_ms = 1,
): Profile => ({
  id,
  name,
  harness,
  home: is_system ? null : `/Users/b/.overseer/profiles/${id}`,
  is_system,
  created_ms,
});

const WORK = profile('p-work', 'Work', 'codex', false, 1);
const DESKTOP = profile('p-desktop', 'ChatGPT app', 'codex', true, 2);
const CLAUDE = profile('p-claude', 'Personal', 'claude', false, 3);
const LOCAL = profile('p-local', 'Local', 'opencode', true, 4);

const stateWith = (...profiles: Profile[]): State => ({ ...EMPTY_STATE, profiles }) as State;

const PROVIDERS = [
  {
    id: 'openai',
    label: 'OpenAI / ChatGPT',
    harnesses: ['codex', 'codex-app'],
    available: true,
    sign_in: 'ChatGPT sign-in in the browser, or a device code',
    why: null,
  },
  {
    id: 'anthropic',
    label: 'Anthropic / Claude',
    harnesses: ['claude'],
    available: true,
    sign_in: 'Claude account sign-in (claude auth login)',
    why: null,
  },
  {
    id: 'local',
    label: 'OpenCode (local models)',
    harnesses: ['opencode'],
    available: true,
    sign_in: 'none',
    why: null,
  },
  {
    id: 'devin',
    label: 'Devin',
    harnesses: [],
    available: false,
    why: 'Devin has no account-login CLI yet',
  },
];
const PROVIDER: Record<string, string> = {
  codex: 'openai',
  claude: 'anthropic',
  opencode: 'local',
};

/** The time of a reset later today, or on another day, by the phone's own clock. */
const LATER_TODAY = (() => {
  const at = new Date();
  at.setHours(23, 58, 0, 0);
  return at.getTime();
})();
const IN_THREE_DAYS = LATER_TODAY + 3 * 24 * 60 * 60 * 1000;

interface Mac {
  signedIn: Record<string, boolean>;
  status: Record<string, Record<string, unknown>>;
  usage: Record<string, unknown>;
  login: (params: { id: string }) => unknown;
}

/** The Mac's side of the accounts: what it answers to each method, changed by the test. */
function macOf(app: TestApp, profiles: readonly Profile[]): Mac {
  const mac: Mac = {
    signedIn: {},
    status: {},
    usage: {},
    login: ({ id }) => ({
      profile_id: id,
      finished: false,
      url: 'https://auth.openai.com/codex/device',
      code: 'ABCD-EFGH1',
      valid_ms: 15 * 60_000,
    }),
  };
  app.connection.answers['account.list'] = () => ({
    accounts: profiles.map((p) => ({
      id: p.id,
      name: p.name,
      provider: PROVIDER[p.harness],
      harness_family: p.harness,
      harnesses: [p.harness],
      kind: p.is_system ? 'follows-app' : 'fixed',
      removable: !p.is_system,
    })),
    providers: PROVIDERS,
  });
  app.connection.answers['profile.status'] = ({ id }: { id: string }) => ({
    profile_id: id,
    installed: true,
    logged_in: mac.signedIn[id] === true,
    method: 'chatgpt-account',
    ...mac.status[id],
  });
  app.connection.answers['account.usage'] = ({ id }: { id: string }) =>
    mac.usage[id] ?? { reported: false };
  app.connection.answers['profile.device_login'] = (params: { id: string }) => mac.login(params);
  return mac;
}

interface Opened {
  readonly app: TestApp;
  readonly mac: Mac;
}

async function open(
  profiles: readonly Profile[],
  set: (mac: Mac) => void = () => undefined,
  options: TestAppOptions = {},
  pollMs = 15,
): Promise<Opened> {
  const app = await createTestApp({ state: stateWith(...profiles), ...options });
  const mac = macOf(app, profiles);
  set(mac);
  await app.render(<AccountsScreen pollMs={pollMs} />);
  return { app, mac };
}

async function wait(app: TestApp, ms: number): Promise<void> {
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, ms));
  });
  await app.settle();
}

const at = (id: string) => screen.getByTestId(id);
const gone = (id: string) => screen.queryByTestId(id) === null;

beforeEach(() => router.reset());

describe('accounts: what it shows', () => {
  test('the accounts by provider, each signed in or not, with its plan and its usage', async () => {
    const { app } = await open([WORK, DESKTOP, CLAUDE, LOCAL], (mac) => {
      mac.signedIn = { 'p-work': true, 'p-claude': true, 'p-local': true };
      mac.status['p-work'] = { identity: { fingerprint: 'a1b2c3d4e5', plan: 'team' } };
      mac.status['p-claude'] = {
        method: 'claude.ai',
        identity: { fingerprint: 'ffee', plan: 'max' },
      };
      mac.usage['p-work'] = {
        reported: true,
        source: 'Codex session log',
        plan: 'team',
        limited: false,
        windows: [
          { label: '5 hours', used: 0.12, resets_at_ms: LATER_TODAY },
          { label: 'week', used: 0.615, resets_at_ms: IN_THREE_DAYS },
        ],
      };
    });

    expect(at('accounts.title')).toHaveTextContent('Accounts');
    const titles = screen.getAllByRole('header').map((header) => String(header.props.children));
    expect(titles).toEqual(['Accounts', 'CHATGPT', 'CLAUDE', 'OPENCODE']);

    expect(at('accounts.row.p-work')).toHaveTextContent(/Work/);
    expect(at('accounts.state.p-work')).toHaveTextContent('Signed in · Team');
    expect(at('accounts.usage.p-work.0')).toHaveTextContent(
      `5 hours · 12% used · resets ${resetTime(LATER_TODAY, Date.now())}`,
    );
    expect(at('accounts.usage.p-work.1')).toHaveTextContent(
      `week · 62% used · resets ${resetTime(IN_THREE_DAYS, Date.now())}`,
    );
    expect(gone('accounts.limit.p-work')).toBe(true);

    expect(at('accounts.state.p-desktop')).toHaveTextContent('Signed out');
    expect(at('accounts.state.p-claude')).toHaveTextContent('Signed in · Max');
    expect(at('accounts.usage.p-claude.none')).toHaveTextContent('Usage not reported');
    expect(at('accounts.state.p-local')).toHaveTextContent('Signed in');

    expect(app.connection.calls('account.list')).toHaveLength(1);
    expect(app.connection.calls('profile.status')).toEqual([
      { id: 'p-work' },
      { id: 'p-desktop' },
      { id: 'p-claude' },
      { id: 'p-local' },
    ]);
    expect(app.connection.calls('account.usage')).toHaveLength(4);
    // Who the account belongs to stays on the Mac: nothing of it is drawn.
    expect(screen.queryByText(/a1b2c3d4/)).toBeNull();
    expect(gone('watch.line')).toBe(true);
  });

  test('an account at its limit says so', async () => {
    await open([WORK], (mac) => {
      mac.signedIn['p-work'] = true;
      mac.usage['p-work'] = {
        reported: true,
        limited: true,
        windows: [{ label: '5 hours', used: 1, resets_at_ms: LATER_TODAY }],
      };
    });
    expect(at('accounts.limit.p-work')).toHaveTextContent('Usage limit reached');
    expect(at('accounts.usage.p-work.0')).toHaveTextContent(/5 hours · 100% used · resets /);
  });

  test('an account set up with an API key is signed out, and says why', async () => {
    await open([WORK], (mac) => {
      mac.status['p-work'] = {
        logged_in: true,
        method: 'api-key (not allowed by Overseer)',
        detail: 'This profile uses an API key.',
      };
    });
    expect(at('accounts.state.p-work')).toHaveTextContent('Signed out');
    expect(at('accounts.note.p-work')).toHaveTextContent(
      'This account uses an API key. Overseer needs an account sign-in.',
    );
  });

  test('a program that is not on the Mac offers no sign-in', async () => {
    await open([WORK], (mac) => {
      mac.status['p-work'] = { installed: false, detail: 'codex not installed' };
    });
    expect(at('accounts.state.p-work')).toHaveTextContent('Not installed on the Mac');
    expect(gone('accounts.signin.p-work')).toBe(true);
    expect(gone('accounts.onmac.p-work')).toBe(true);
  });

  test('no accounts yet', async () => {
    await open([]);
    expect(at('accounts.empty')).toHaveTextContent(/No accounts yet\.$/);
    expect(screen.getByText('No accounts yet.')).toBeTruthy();
  });

  test('not connected, the stored accounts stay and are checked when the Mac is back', async () => {
    // The Mac was reached before: what it said then is on the phone.
    const app = await createTestApp({ state: stateWith(WORK) });
    macOf(app, [WORK]).signedIn['p-work'] = true;
    app.connection.go('unreachable');
    await app.render(<AccountsScreen />);
    expect(at('connection.unreachable')).toBeTruthy();
    expect(at('accounts.row.p-work')).toHaveTextContent(/Work/);
    expect(at('accounts.state.p-work')).toHaveTextContent('Not checked');
    expect(app.connection.calls('profile.status')).toEqual([]);

    await act(async () => app.connection.go('online'));
    await app.settle();
    expect(at('accounts.state.p-work')).toHaveTextContent('Signed in');
  });

  test('an account added on the Mac appears and is checked', async () => {
    const { app, mac } = await open([WORK], (m) => (m.signedIn['p-work'] = true));
    mac.signedIn['p-claude'] = true;
    await app.events(makeEvent('profile', { profile: CLAUDE, action: 'created' }));
    await app.settle();
    expect(at('accounts.state.p-claude')).toHaveTextContent('Signed in');

    await app.events(makeEvent('profile', { profile_id: 'p-claude', action: 'removed' }));
    await app.settle();
    expect(gone('accounts.row.p-claude')).toBe(true);
  });

  test('pulling down asks the Mac again', async () => {
    const { app, mac } = await open([WORK]);
    expect(at('accounts.state.p-work')).toHaveTextContent('Signed out');
    mac.signedIn['p-work'] = true;
    await act(async () => at('accounts.list').props.refreshControl.props.onRefresh());
    await app.settle();
    expect(at('accounts.state.p-work')).toHaveTextContent('Signed in');
    expect(app.connection.calls('account.list')).toHaveLength(2);
  });
});

describe('accounts: sign in with a code', () => {
  test('the code is large and can be selected, the page opens, the code is copied, and it finishes by itself', async () => {
    const { app, mac } = await open([WORK, CLAUDE]);
    const dismissed = jest.spyOn(app.connection, 'dismiss');
    expect(gone('accounts.signin')).toBe(true);
    await fireEvent.press(screen.getByLabelText('Sign in, Work'));
    await app.settle();

    expect(app.connection.calls('profile.device_login')).toEqual([{ id: 'p-work' }]);
    const asked = app.connection.asked.find((a) => a.method === 'profile.device_login');
    const requestId = (asked?.options as { requestId?: string } | undefined)?.requestId;
    expect(requestId).toMatch(/^[0-9a-f-]{36}$/);
    // The answer holds the code: it does not stay among the answered requests.
    expect(dismissed).toHaveBeenCalledWith(requestId);

    const code = at('accounts.signin.code');
    expect(code).toHaveTextContent('ABCD-EFGH1');
    expect(code.props.selectable).toBe(true);
    expect(code.props.accessibilityLabel).toBe('Code A B C D - E F G H 1');
    expect(code).toHaveStyle({ fontSize: phone.font.code });

    await fireEvent.press(at('accounts.signin.open'));
    expect(openURL).toHaveBeenCalledWith('https://auth.openai.com/codex/device');
    await fireEvent.press(at('accounts.signin.copy'));
    await app.settle();
    expect(setStringAsync).toHaveBeenCalledWith('ABCD-EFGH1');
    expect(screen.getByLabelText('Copied')).toBeTruthy();

    // Not finished yet: the sheet stays.
    await wait(app, 40);
    expect(at('accounts.signin.code')).toBeTruthy();
    expect(at('accounts.state.p-work')).toHaveTextContent('Signed out');

    // The person finished in the browser. Nothing is pressed.
    mac.signedIn['p-work'] = true;
    mac.status['p-work'] = { identity: { plan: 'plus' } };
    await wait(app, 40);
    expect(gone('accounts.signin.code')).toBe(true);
    expect(at('accounts.state.p-work')).toHaveTextContent('Signed in · Plus');
    expect(gone('accounts.signin.p-work')).toBe(true);
    expect(app.connection.calls('profile.device_login')).toHaveLength(1);
    // Nothing of the sign-in was kept on the phone.
    expect([...app.platform.fakes.keyValue.items.values()].join(' ')).not.toContain('ABCD-EFGH1');
    expect([...app.platform.fakes.secretStore.items.values()].join(' ')).not.toContain(
      'ABCD-EFGH1',
    );
  });

  test('once the sheet is closed the Mac is not asked again', async () => {
    const { app } = await open([WORK]);
    await fireEvent.press(at('accounts.signin.p-work'));
    await app.settle();
    await wait(app, 40);
    expect(app.connection.calls('profile.status').length).toBeGreaterThan(1);
    await fireEvent.press(screen.getByLabelText('Close'));
    await app.settle();
    const asked = app.connection.calls('profile.status').length;
    await wait(app, 60);
    expect(app.connection.calls('profile.status')).toHaveLength(asked);
    expect(gone('accounts.signin.code')).toBe(true);
  });

  test('away in the browser nothing is asked; back in the app it finishes', async () => {
    const { app, mac } = await open([WORK]);
    await fireEvent.press(at('accounts.signin.p-work'));
    await app.settle();
    await act(async () => app.platform.fakes.appState.set('background'));
    const asked = app.connection.calls('profile.status').length;
    mac.signedIn['p-work'] = true;
    await wait(app, 50);
    expect(app.connection.calls('profile.status')).toHaveLength(asked);
    expect(at('accounts.signin.code')).toBeTruthy();

    await act(async () => app.platform.fakes.appState.set('foreground'));
    await wait(app, 40);
    expect(gone('accounts.signin.code')).toBe(true);
    expect(at('accounts.state.p-work')).toHaveTextContent('Signed in');
  });

  test('an account that is signed in already closes at once', async () => {
    const { app, mac } = await open([WORK]);
    mac.login = ({ id }) => {
      mac.signedIn[id] = true;
      return {
        profile_id: id,
        finished: true,
        exit: 0,
        logged_in: true,
        output: 'Logged in using ChatGPT',
      };
    };
    await fireEvent.press(at('accounts.signin.p-work'));
    await app.settle();
    expect(gone('accounts.signin.code')).toBe(true);
    expect(gone('accounts.signin.asking')).toBe(true);
    expect(at('accounts.state.p-work')).toHaveTextContent('Signed in');
    expect(screen.queryByText(/Logged in using/)).toBeNull();
  });

  test('a code that is too old offers a new one', async () => {
    const { app, mac } = await open([WORK]);
    mac.login = ({ id }) => ({
      profile_id: id,
      finished: false,
      url: 'https://auth.openai.com/codex/device',
      code: 'OLD1-CODE2',
      valid_ms: 10,
    });
    await fireEvent.press(at('accounts.signin.p-work'));
    await app.settle();
    expect(at('accounts.signin.code')).toHaveTextContent('OLD1-CODE2');
    await wait(app, 60);
    expect(at('accounts.signin.old')).toHaveTextContent('This code is too old.');
    expect(gone('accounts.signin.code')).toBe(true);

    mac.login = ({ id }) => ({
      profile_id: id,
      finished: false,
      url: 'https://auth.openai.com/codex/device',
      code: 'NEW1-CODE2',
      valid_ms: 60_000,
    });
    await fireEvent.press(screen.getByLabelText('Get a new code'));
    await app.settle();
    expect(at('accounts.signin.code')).toHaveTextContent('NEW1-CODE2');
    expect(app.connection.calls('profile.device_login')).toHaveLength(2);
  });

  test('a sign-in that did not start says so and can be tried again', async () => {
    const { app, mac } = await open([WORK]);
    mac.login = () => {
      throw new RequestError({ code: 'mac_setup', message: 'codex is not installed on the Mac' });
    };
    await fireEvent.press(at('accounts.signin.p-work'));
    await app.settle();
    expect(at('accounts.signin.failed')).toHaveTextContent('The sign-in did not start.');

    mac.login = ({ id }) => ({
      profile_id: id,
      finished: true,
      exit: 1,
      logged_in: false,
      output: 'error',
    });
    await fireEvent.press(screen.getByLabelText('Try again'));
    await app.settle();
    expect(at('accounts.signin.failed')).toBeTruthy();
    expect(app.connection.calls('profile.device_login')).toHaveLength(2);
  });

  test('an address that is not a page of the web is never opened', async () => {
    const { app, mac } = await open([WORK]);
    for (const url of [
      'javascript:alert(1)',
      'http://auth.example.invalid/device',
      'overseer://pair',
      '',
    ]) {
      mac.login = ({ id }) => ({
        profile_id: id,
        finished: false,
        url,
        code: 'ABCD-EFGH1',
        valid_ms: 60_000,
      });
      await fireEvent.press(
        gone('accounts.signin.again') ? at('accounts.signin.p-work') : at('accounts.signin.again'),
      );
      await app.settle();
      expect(at('accounts.signin.failed')).toBeTruthy();
      expect(gone('accounts.signin.open')).toBe(true);
    }
    expect(openURL).not.toHaveBeenCalled();
  });
});

describe('accounts: a provider without a code', () => {
  test('says Sign in on the Mac, and offers nothing to press', async () => {
    const { app } = await open([CLAUDE, DESKTOP, WORK]);
    expect(at('accounts.onmac.p-claude')).toHaveTextContent('Sign in on the Mac.');
    expect(gone('accounts.signin.p-claude')).toBe(true);
    // The desktop app's own login is never changed from a phone, whatever its provider.
    expect(at('accounts.onmac.p-desktop')).toHaveTextContent('Sign in on the Mac.');
    expect(gone('accounts.signin.p-desktop')).toBe(true);
    expect(at('accounts.signin.p-work')).toBeTruthy();
    expect(gone('accounts.onmac.p-work')).toBe(true);
    expect(app.connection.calls('profile.device_login')).toEqual([]);
  });

  test('a Mac that gives no code says Sign in on the Mac', async () => {
    const { app, mac } = await open([WORK]);
    mac.login = () => {
      throw new RequestError({
        code: 'mac_only',
        message: 'Sign in on the Mac: codex did not offer a code.',
      });
    };
    await fireEvent.press(at('accounts.signin.p-work'));
    await app.settle();
    expect(at('accounts.signin.onmac')).toHaveTextContent('Sign in on the Mac.');
    expect(gone('accounts.signin.again')).toBe(true);
    expect(gone('accounts.signin.code')).toBe(true);
    await fireEvent.press(at('accounts.signin.done'));
    await app.settle();
    expect(gone('accounts.signin.onmac')).toBe(true);
  });
});

describe('accounts: a phone that may only watch', () => {
  test('sees every account and its usage, and no Sign in', async () => {
    const { app } = await open(
      [WORK, CLAUDE],
      (mac) => {
        mac.signedIn['p-claude'] = true;
        mac.usage['p-claude'] = {
          reported: true,
          limited: false,
          windows: [{ label: '5 hours', used: 0.85, resets_at_ms: LATER_TODAY }],
        };
      },
      { scope: 'watch' },
    );
    expect(at('accounts.state.p-work')).toHaveTextContent('Signed out');
    expect(at('accounts.state.p-claude')).toHaveTextContent('Signed in');
    expect(at('accounts.usage.p-claude.0')).toHaveTextContent(/5 hours · 85% used/);
    expect(gone('accounts.signin.p-work')).toBe(true);
    expect(screen.queryByLabelText('Sign in, Work')).toBeNull();
    expect(at('watch.line')).toHaveTextContent('This phone may watch. Change it on the Mac.');
    expect(gone('accounts.signin')).toBe(true);
    expect(app.connection.calls('profile.device_login')).toEqual([]);
  });
});

describe('accounts: what the Mac answers, read', () => {
  const account = (p: Profile) => byProvider([p], null)[0]?.accounts[0];

  test('providers come in the order of the Mac, by the names VS Code lists them under', () => {
    const listed = readList({
      accounts: [
        { id: 'p-work', provider: 'openai' },
        { id: 'p-claude', provider: 'anthropic' },
        { id: 7 },
        null,
      ],
      providers: [
        { id: 'anthropic', label: 'Anthropic / Claude' },
        { id: 'openai', label: 'OpenAI / ChatGPT' },
        { label: 'nameless' },
      ],
    });
    expect(
      byProvider([WORK, DESKTOP, CLAUDE], listed).map((g) => [
        g.id,
        g.name,
        g.logo,
        g.accounts.map((a) => a.id),
      ]),
    ).toEqual([
      ['anthropic', 'Claude', 'claude', ['p-claude']],
      ['openai', 'ChatGPT', 'openai', ['p-work', 'p-desktop']],
    ]);
    // Before the Mac answered, and for a provider it does not list.
    expect(
      byProvider([CLAUDE, WORK, profile('p-x', 'Other', 'devin')], null).map((g) => [g.id, g.name]),
    ).toEqual([
      ['openai', 'ChatGPT'],
      ['anthropic', 'Claude'],
      ['devin', 'devin'],
    ]);
    expect(readList('nonsense')).toEqual({ providerOf: new Map(), providers: [] });
  });

  test('only an account of its own with a provider that gives a code signs in from the phone', () => {
    const own = account(WORK);
    const desktop = account(DESKTOP);
    const claude = account(CLAUDE);
    expect(own && hasCode(own)).toBe(true);
    expect(desktop && hasCode(desktop)).toBe(false);
    expect(claude && hasCode(claude)).toBe(false);
  });

  test('a signed-in account says its plan and its shortened email, as the Mac names them (AC-235)', () => {
    const status = { installed: true, signedIn: true, plan: 'max', apiKey: false };
    expect(stateText(status, undefined, false, { plan: 'Max', email: 'bil…@testbox.com' })).toBe('Signed in · Max · bil…@testbox.com');
    expect(stateText(status, undefined, false, { plan: null, email: null })).toBe('Signed in · Max');
    const [group] = byProvider([{ ...profile('system-claude', 'claude (existing login)', 'claude', true), account: { provider: 'Claude', plan: 'Max', email: 'bil…@testbox.com', default: true, name: "Mac's default login", label: "Claude Max · bil…@testbox.com · Mac's default login", short: 'Claude Max · bil…@testbox.com' } }], null);
    expect(group?.accounts[0]).toMatchObject({ name: "Mac's default login", email: 'bil…@testbox.com', plan: 'Max' });
  });

  test('the sign-in state keeps the plan and nothing else of the identity', () => {
    expect(
      readStatus({
        installed: true,
        logged_in: true,
        method: 'chatgpt-account',
        identity: { fingerprint: 'abc', plan: 'pro', email: 'a@b.c' },
        detail: 'Logged in',
      }),
    ).toEqual({ installed: true, signedIn: true, plan: 'pro', apiKey: false });
    expect(readStatus({ installed: false, logged_in: false })).toEqual({
      installed: false,
      signedIn: false,
      plan: null,
      apiKey: false,
    });
    expect(readStatus({ logged_in: true, method: 'API key' })).toMatchObject({
      signedIn: false,
      apiKey: true,
    });
    expect(readStatus(null)).toEqual({
      installed: true,
      signedIn: false,
      plan: null,
      apiKey: false,
    });
  });

  test('usage is only what is reported', () => {
    expect(readUsage({ reported: false })).toEqual({
      reported: false,
      plan: null,
      limited: false,
      windows: [],
    });
    expect(
      readUsage({
        reported: true,
        plan: 'team',
        limited: true,
        windows: [{ label: 'week', used: 1.4, resets_at_ms: 5 }, { label: 'bad' }, 'x'],
      }),
    ).toEqual({
      reported: true,
      plan: 'team',
      limited: true,
      windows: [{ label: 'week', used: 1, resetsAt: 5 }],
    });
  });

  test('a reset today is a time, a later one has its day, and one that has passed is not said', () => {
    const now = new Date(2026, 8, 26, 10, 0).getTime();
    const today = new Date(2026, 8, 26, 15, 40).getTime();
    const tuesday = new Date(2026, 8, 29, 9, 5).getTime();
    const nextMonth = new Date(2026, 9, 20, 9, 5).getTime();
    expect(resetTime(today, now)).toMatch(/^(3:40\s?PM|15:40)$/);
    expect(resetTime(tuesday, now)).toMatch(/^\S+ (9:05\s?AM|0?9:05)$/);
    expect(resetTime(nextMonth, now)).toMatch(/20/);
    expect(usageText({ label: '5 hours', used: 0.125, resetsAt: today }, now)).toBe(
      `5 hours · 13% used · resets ${resetTime(today, now)}`,
    );
    expect(usageText({ label: 'week', used: 0.5, resetsAt: now - 1 }, now)).toBe('week · 50% used');
    expect(usageText({ label: 'week', used: 0, resetsAt: null }, now)).toBe('week · 0% used');
  });

  test('usage reads fine, close to its limit, or at it', () => {
    expect(usageTone({ label: 'week', used: 0.5, resetsAt: null }, false)).toBe('accent');
    expect(usageTone({ label: 'week', used: 0.8, resetsAt: null }, false)).toBe('amber');
    expect(usageTone({ label: 'week', used: 0.9, resetsAt: null }, true)).toBe('red');
    expect(usageTone({ label: 'week', used: 1, resetsAt: null }, false)).toBe('red');
    expect(usageTone({ label: '5 hours', used: 0.1, resetsAt: null }, true)).toBe('accent');
  });

  test('the answer of a sign-in is a code with a page of the web, signed in, or failed', () => {
    expect(
      readSignIn({
        finished: false,
        url: 'https://auth.openai.com/codex/device',
        code: 'ABCD-EFGH1',
        valid_ms: 900000,
      }),
    ).toEqual({
      kind: 'code',
      url: 'https://auth.openai.com/codex/device',
      code: 'ABCD-EFGH1',
      validMs: 900000,
    });
    expect(readSignIn({ finished: true, logged_in: true })).toEqual({ kind: 'signedIn' });
    expect(readSignIn({ finished: true, logged_in: false, exit: 1 })).toEqual({ kind: 'failed' });
    expect(readSignIn({ finished: false, url: 'https://auth.openai.com/codex/device' })).toEqual({
      kind: 'failed',
    });
    expect(readSignIn({ finished: false, url: 'https://a b', code: 'ABCD-EFGH1' })).toEqual({
      kind: 'failed',
    });
    expect(readSignIn(undefined)).toEqual({ kind: 'failed' });
  });
});
