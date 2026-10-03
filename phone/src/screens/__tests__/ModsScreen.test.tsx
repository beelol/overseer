import { act, fireEvent, screen } from '@testing-library/react-native';

import { useLayoutEffect, useState } from 'react';

import type { AppliedMods, Result, TurnModSnapshot } from '@/protocol';
import { ModsScreen } from '@/screens/ModsScreen';
import { wordsOf } from '@/screens/conversation/testing';
import { stateWith, wait } from '@/screens/review/testing';
import { createTestApp, makeEvent, type TestApp } from '@/testing';
import { router } from '@/testing/router';

jest.mock('expo-router', () => require('@/testing/router').mockRouter());
beforeEach(() => router.reset());

const version = {
  id: 'clear-prose',
  version: '1.0.0',
  fingerprint: 'pinned-original-sha',
  manifest: { name: 'Clear prose' },
  source: 'bundled:clear-prose',
  installed_ms: 1,
  files: [{ path: 'style.md', bytes: 32, sha256: 'style-sha' }],
};
const support = {
  delivery: 'message_text',
  native_configuration: 'unverified',
  installed_runtime_qualification: 'unverified',
  global_text_suppression: 'unsupported',
  dynamic_local_models: 'unsupported',
  children: 'unknown',
};
const library: Result<'mods.list'> = {
  revision: 3,
  installed: [version],
  bindings: [],
  available_bundled: [version],
  unavailable: [
    { id: 'less-tool-noise', reason: 'Planned; external transformers are not implemented' },
  ],
  support,
};
const context = {
  run_id: 'r1',
  role: 'agent',
  repo_key: '/synthetic/repo/.git',
  harness: 'claude',
  model: 'synthetic-model',
  native_thread_exists: false,
  local_model_selection: false,
};
const desired: AppliedMods['desired'] = {
  revision: 3,
  versions: [version],
  rules_text: '',
  style_text: 'Write complete sentences.',
  decisions: [
    {
      binding_id: 'b1',
      mod_id: 'clear-prose',
      fingerprint: version.fingerprint,
      status: 'selected',
      reason: 'Owner enabled this agent binding',
      required: false,
      delivery: 'message_text',
      activation: 'next_turn',
      children: 'unknown',
    },
  ],
};
const applied: AppliedMods = {
  context,
  desired,
  last_turn: null,
  pending: true,
  support,
  notice:
    'Native configuration and child inheritance are unqualified. Disabling does not erase earlier instructions.',
};
const turn = (outcome: TurnModSnapshot['outcome']): TurnModSnapshot => ({
  turn_id: 't1',
  run_id: 'r1',
  plan: desired,
  context,
  binding_snapshot: [],
  delivery: 'message_text',
  transport: 'claude',
  activation: 'next_turn',
  children: 'unknown',
  text: 'token=[REDACTED]',
  digest: 'original-private-digest',
  added_bytes: 123,
  applied_fingerprints: outcome === 'transport_accepted' ? [version.fingerprint] : [],
  planned_fingerprints: [version.fingerprint],
  outcome,
  outcome_ms: 1,
  outcome_detail: 'fixture outcome',
  text_redacted: true,
});

async function open(options: Parameters<typeof createTestApp>[0] = {}): Promise<TestApp> {
  const app = await createTestApp({ state: stateWith(), ...options });
  app.connection.answers['mods.list'] = () => library;
  app.connection.answers['mods.why'] = () => applied;
  router.params = { run: 'r1' };
  return app;
}
const words = (id: string): string => wordsOf(screen.getByTestId(id));
const afterNews = async (app: TestApp): Promise<void> => {
  await act(() => wait(650));
  await app.settle();
};
function later<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

test.each(['full', 'watch'] as const)(
  'installed, desired and last recorded delivery are distinct for %s',
  async (scope) => {
    const app = await open({ scope });
    await app.render(<ModsScreen />);
    expect(words('mods.installed')).toContain('Clear prose');
    expect(words('mods.desired')).toContain('Owner enabled this agent binding');
    expect(words('mods.pending')).toContain('Pending');
    expect(words('mods.last')).toContain('No recorded turn delivery');
    expect(words('mods.support')).toContain('native_configuration: unverified');
    expect(words('mods.planned')).toContain('external transformers are not implemented');
    expect(screen.queryByText(/tokens saved|savings/i)).toBeNull();
    expect(
      app.connection.asked.filter((c) => c.method.startsWith('mods.')).map((c) => c.method),
    ).toEqual(['mods.list', 'mods.why']);
    expect(app.connection.entries).toEqual([]);
    expect(
      screen.queryAllByRole('button').map((button) => button.props.accessibilityLabel),
    ).toEqual(expect.arrayContaining(['Back', 'Refresh Mods']));
    expect(screen.queryByLabelText(/Install|Enable|Disable|Remove/)).toBeNull();
  },
);

test.each([
  ['prepared', 'Prepared; delivery is not confirmed'],
  ['failed_before_effect', 'Failed before delivery'],
  ['uncertain_after_effect', 'Delivery outcome is uncertain'],
  ['transport_accepted', 'Transport accepted'],
] as const)('last %s outcome is not inferred from desired selection', async (outcome, label) => {
  const app = await open();
  app.connection.answers['mods.why'] = () => ({
    ...applied,
    pending: outcome !== 'transport_accepted',
    last_turn: turn(outcome),
  });
  await app.render(<ModsScreen />);
  expect(words('mods.last')).toContain(label);
  expect(words('mods.last')).toContain('original-private-digest');
  expect(words('mods.last')).toContain('123 original bytes');
  expect(words('mods.last')).toContain('Public text is redacted');
  expect(words('mods.last')).toContain('token=[REDACTED]');
  if (outcome !== 'transport_accepted')
    expect(words('mods.last')).not.toContain('Applied: pinned-original-sha');
});

test('global and current-run news refresh reliably at fixed time; unrelated runs do not', async () => {
  const app = await open();
  await app.render(<ModsScreen />);
  await app.events(makeEvent('mods_changed', { revision: 4 }));
  await app.events(makeEvent('mods_changed', { revision: 5 }));
  await afterNews(app);
  expect(app.connection.calls('mods.list')).toHaveLength(2);
  await app.events(makeEvent('mods_applied', { snapshot: {} }, { run_id: 'other' }));
  await afterNews(app);
  expect(app.connection.calls('mods.why')).toHaveLength(2);
  app.connection.answers['mods.why'] = () => ({
    ...applied,
    last_turn: turn('transport_accepted'),
    pending: false,
  });
  await app.events(makeEvent('mods_applied', { snapshot: {} }, { run_id: 'r1' }));
  await afterNews(app);
  expect(app.connection.calls('mods.why')).toHaveLength(3);
  expect(words('mods.last')).toContain('Transport accepted');
});

test('offline retains stale facts, discards late replies and refreshes on reconnect', async () => {
  const app = await open();
  await app.render(<ModsScreen />);
  const delayed = later<AppliedMods>();
  app.connection.answers['mods.why'] = () => delayed.promise;
  await fireEvent.press(screen.getByLabelText('Refresh Mods'));
  await act(async () => app.connection.go('unreachable'));
  await act(async () => delayed.resolve({ ...applied, notice: 'LATE INVALID RESPONSE' }));
  await app.settle();
  expect(words('mods.stale')).toContain('Offline');
  expect(words('mods.notice')).not.toContain('LATE INVALID RESPONSE');
  const before = app.connection.calls('mods.why').length;
  await app.events(makeEvent('mods_changed', { revision: 9 }));
  await afterNews(app);
  expect(app.connection.calls('mods.why')).toHaveLength(before);
  app.connection.answers['mods.why'] = () => ({ ...applied, notice: 'Fresh from the Mac' });
  await act(async () => app.connection.go('online'));
  await app.settle();
  expect(words('mods.notice')).toBe('Fresh from the Mac');
  expect(app.connection.calls('mods.why')).toHaveLength(before + 1);
});

test('route change clears the previous run and drops its pending result', async () => {
  const app = await open();
  const delayed = later<AppliedMods>();
  app.connection.answers['mods.why'] = () => delayed.promise;
  let redraw!: () => void;
  function Host() {
    const [, change] = useState(0);
    redraw = () => change((n) => n + 1);
    return <ModsScreen />;
  }
  await app.render(<Host />);
  router.params = { run: 'r2' };
  app.connection.answers['mods.why'] = () => ({
    ...applied,
    context: { ...context, run_id: 'r2' },
    notice: 'Second run',
  });
  await act(async () => redraw());
  await app.settle();
  await act(async () => delayed.resolve({ ...applied, notice: 'First run late' }));
  await app.settle();
  expect(words('mods.notice')).toBe('Second run');
  expect(app.connection.calls('mods.why')).toEqual([{ run_id: 'r1' }, { run_id: 'r2' }]);
});

test('read failures are independent and retained data is labelled stale until fresh confirmation', async () => {
  const app = await open();
  app.connection.answers['mods.list'] = () => {
    throw new Error('Library is unavailable');
  };
  await app.render(<ModsScreen />);
  expect(words('mods.library.error')).toContain('Library is unavailable');
  expect(words('mods.desired')).toContain('Owner enabled');
  app.connection.answers['mods.list'] = () => library;
  app.connection.answers['mods.why'] = () => {
    throw new Error('Unknown run target');
  };
  await fireEvent.press(screen.getByLabelText('Refresh Mods'));
  await app.settle();
  expect(words('mods.applied.error')).toContain('Unknown run target');
  expect(words('mods.installed')).toContain('Clear prose');
  expect(words('mods.stale')).toContain('could not be refreshed');
});

test('missing route and initial offline never issue an invalid read', async () => {
  const app = await open({ connection: 'unreachable' });
  router.params = {};
  await app.render(<ModsScreen />);
  expect(words('mods.empty')).toContain('Choose an agent');
  expect(app.connection.calls('mods.list')).toHaveLength(0);
  expect(app.connection.calls('mods.why')).toHaveLength(0);
});

test('forgetting the Mac clears retained private inspection data', async () => {
  const app = await open();
  await app.render(<ModsScreen />);
  await act(async () => app.session.forget());
  await app.settle();
  expect(screen.queryByText('Write complete sentences.')).toBeNull();
  expect(words('mods.installed')).not.toContain('Clear prose');
});

test('own scopes and unsupported desired facts remain inspection, including explicit off', async () => {
  const app = await open({ scope: 'watch' });
  app.connection.answers['mods.list'] = () => ({
    ...library,
    bindings: [
      {
        id: 'all',
        mod_id: version.id,
        version: version.version,
        fingerprint: version.fingerprint,
        scope: { kind: 'all_agents' },
        enabled: false,
        locked: true,
        required: true,
        filters: { models: ['future-model'], accounts: [], harnesses: [] },
        actor: 'owner',
        changed_ms: 1,
      },
      {
        id: 'self',
        mod_id: version.id,
        version: version.version,
        fingerprint: version.fingerprint,
        scope: { kind: 'overseer' },
        enabled: true,
        locked: false,
        required: false,
        filters: { models: [], accounts: [], harnesses: [] },
        actor: 'owner',
        changed_ms: 1,
      },
    ],
  });
  app.connection.answers['mods.why'] = () => ({
    ...applied,
    desired: {
      ...desired,
      decisions: desired.decisions.map((decision) => ({
        ...decision,
        status: 'unqualified',
        reason: 'Dynamic local delivery is unsupported',
      })),
    },
  });
  await app.render(<ModsScreen />);
  expect(words('mods.bindings')).toContain('All agents (excluding Overseer)');
  expect(words('mods.bindings')).toContain('Explicitly disabled · Locked · Required');
  expect(words('mods.bindings')).toContain('Overseer session');
  expect(words('mods.bindings')).toContain('future-model');
  expect(words('mods.desired')).toContain('Dynamic local delivery is unsupported');
  expect(words('mods.last')).toContain('No recorded turn delivery');
});

test('initial offline with a valid agent does not read and a later manual refresh does', async () => {
  const app = await open({ connection: 'unreachable' });
  await app.render(<ModsScreen />);
  expect(words('mods.empty')).toContain('Shown when the Mac is reached');
  expect(app.connection.calls('mods.why')).toHaveLength(0);
  await act(async () => app.connection.go('online'));
  await app.settle();
  await fireEvent.press(screen.getByLabelText('Refresh Mods'));
  await app.settle();
  expect(app.connection.calls('mods.why')).toHaveLength(2);
});

test('closing the screen unsubscribes and cancels a queued refresh without polling', async () => {
  const app = await open();
  const drawn = await app.render(<ModsScreen />);
  await app.events(makeEvent('mods_changed', { revision: 4 }));
  await drawn.unmount();
  await afterNews(app);
  await app.events(makeEvent('mods_applied', {}, { run_id: 'r1' }));
  await afterNews(app);
  expect(app.connection.calls('mods.why')).toHaveLength(1);
});

test('unknown library entries and literal markup do not become actions', async () => {
  const app = await open();
  app.connection.answers['mods.list'] = () => ({
    ...library,
    installed: [null, { ...version, manifest: { name: '<script>synthetic</script>' } }],
  });
  await app.render(<ModsScreen />);
  expect(words('mods.installed')).toContain('Unknown mod');
  expect(words('mods.installed')).toContain('<script>synthetic</script>');
  expect(screen.queryAllByRole('link')).toHaveLength(0);
  expect(app.connection.asked.filter((c) => c.method.startsWith('mods.'))).toHaveLength(2);
});

test('an old reply resolved during the new target commit never supplies its view', async () => {
  const app = await open();
  const first = later<AppliedMods>();
  const second = later<AppliedMods>();
  app.connection.answers['mods.why'] = (params: never) =>
    (params as { run_id: string }).run_id === 'r1' ? first.promise : second.promise;
  let redraw!: () => void;
  function Host() {
    const [, change] = useState(0);
    redraw = () => change((n) => n + 1);
    useLayoutEffect(() => {
      if (router.params.run === 'r2')
        first.resolve({ ...applied, notice: 'OLD TARGET DURING COMMIT' });
    });
    return <ModsScreen />;
  }
  await app.render(<Host />);
  router.params = { run: 'r2' };
  await act(async () => redraw());
  await app.settle();
  expect(screen.queryByText('OLD TARGET DURING COMMIT')).toBeNull();
  expect(words('mods.desired')).toContain('The plan for this agent has not been loaded');
  await act(async () =>
    second.resolve({
      ...applied,
      context: { ...context, run_id: 'r2' },
      notice: 'Current second target',
    }),
  );
  await app.settle();
  expect(words('mods.notice')).toBe('Current second target');
});
