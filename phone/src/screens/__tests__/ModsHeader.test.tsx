import { fireEvent, screen } from '@testing-library/react-native';

import { agents, store } from '@/model';
import { routes } from '@/routes';
import { HeaderActions } from '@/screens/conversation/HeaderActions';
import { RUN, stateWith } from '@/screens/review/testing';
import { createTestApp } from '@/testing';
import { router } from '@/testing/router';

jest.mock('expo-router', () => require('@/testing/router').mockRouter());
beforeEach(() => router.reset());

test.each([
  ['full', 'running', false],
  ['watch', 'running', false],
  ['watch', 'running', true],
  ['full', 'completed', true],
] as const)(
  'Mods is inspection for %s/%s/child=%s without a worktree',
  async (scope, status, child) => {
    const app = await createTestApp({ scope });
    const state = stateWith({ status, ...(child ? { parent_run_id: 'parent' } : {}) });
    const header = agents.runHeader(store.load(state), RUN);
    await app.render(
      <HeaderActions
        runId={RUN}
        header={header}
        taskId={undefined}
        workspace={undefined}
        changes={null}
        watch={scope === 'watch'}
        onStop={jest.fn()}
      />,
    );
    await fireEvent.press(screen.getByTestId('agent.more'));
    await fireEvent.press(screen.getByTestId('agent.more.mods'));
    expect(router.pushed).toEqual([`/agent/${RUN}/mods`]);
    expect(app.connection.asked.filter((c) => c.method.startsWith('mods.'))).toEqual([]);
  },
);

test('the route encodes one run component', () => {
  expect(routes.mods('run /?')).toBe('/agent/run%20%2F%3F/mods');
});

test('a route target can be inspected before its ordinary header is available', async () => {
  const app = await createTestApp({ scope: 'watch' });
  await app.render(
    <HeaderActions
      runId={RUN}
      header={undefined}
      taskId={undefined}
      workspace={undefined}
      changes={null}
      watch
      onStop={jest.fn()}
    />,
  );
  await fireEvent.press(screen.getByTestId('agent.more'));
  await fireEvent.press(screen.getByTestId('agent.more.mods'));
  expect(router.pushed).toEqual([`/agent/${RUN}/mods`]);
  expect(app.connection.asked.filter((c) => c.method.startsWith('mods.'))).toEqual([]);
});
