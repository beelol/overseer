import { act, fireEvent, screen } from '@testing-library/react-native';
import * as Clipboard from 'expo-clipboard';
import * as Linking from 'expo-linking';

import { PullRequestScreen } from '@/screens/PullRequestScreen';
import { draw, letLongTimersGo, refusal, RUN, stateWith, WORKSPACE, workspace } from '@/screens/review/testing';
import { createTestApp, type TestApp } from '@/testing';
import { router } from '@/testing/router';

jest.mock('expo-router', () => require('@/testing/router').mockRouter());
jest.mock('expo-clipboard', () => ({ setStringAsync: jest.fn(async () => true) }));
jest.mock('expo-linking', () => ({ openURL: jest.fn(async () => true) }));

const ADDRESS = 'https://github.com/owner/shop/pull/12';

/** What the daemon's `pr_plan` answers (daemon/src/pr.rs). */
const PLAN = {
  ok: true, workspace, run_id: RUN, title: 'Fix the cart', prompt: 'Fix the cart total', harness: 'claude', model: 'sonnet',
  remote: 'origin', remote_url: 'https://github.com/owner/shop.git', owner: 'owner', repo: 'shop', branch: 'overseer/fix-cart', target: 'main', base_ref: 'main',
  uncommitted: ['src/tax.ts'], commits: ['Add the tax to the total', 'Sum the items'],
};
const OPENED = { url: ADDRESS, number: 12, reused: false, branch: 'overseer/fix-cart', target: 'main', repo: 'owner/shop', committed: true, head: 'abc' };

async function open(options: Parameters<typeof createTestApp>[0] = {}): Promise<TestApp> {
  const app = await createTestApp({ state: stateWith(), ...options });
  app.connection.answers['workspace.pr_plan'] = () => PLAN;
  app.connection.answers['workspace.pr_open'] = () => OPENED;
  router.params = { run: RUN };
  return app;
}

beforeAll(letLongTimersGo);
beforeEach(() => router.reset());

describe('a pull request', () => {
  test('shows the plan: branch, base, remote and commits, with the title filled in from it', async () => {
    const app = await open();
    await draw(app, <PullRequestScreen />);

    expect(app.connection.calls('workspace.pr_plan')).toEqual([{ workspace_id: WORKSPACE }]);
    expect(screen.getByTestId('pr.plan.branch')).toHaveTextContent(/overseer\/fix-cart/);
    expect(screen.getByTestId('pr.plan.base')).toHaveTextContent(/main/);
    expect(screen.getByTestId('pr.plan.remote')).toHaveTextContent(/origin/);
    expect(screen.getByTestId('pr.plan.remote')).toHaveTextContent(/owner\/shop/);
    expect(screen.getByTestId('pr.plan.commits')).toHaveTextContent(/Commits/);
    expect(screen.getByTestId('pr.plan.commits')).toHaveTextContent(/2/);
    expect(screen.getByTestId('pr.commit.0')).toHaveTextContent('Add the tax to the total');
    expect(screen.getByTestId('pr.commit.1')).toHaveTextContent('Sum the items');
    expect(screen.getByText('1 file will be committed to overseer/fix-cart first.')).toBeTruthy();

    expect(screen.getByTestId('pr.field.title').props.value).toBe('Fix the cart');
    expect(screen.getByTestId('pr.field.body').props.value).toBe('');
    // What Open will do is said before it is pressed.
    expect(screen.getByTestId('pr.next')).toHaveTextContent('Pushes overseer/fix-cart to origin and opens a pull request on owner/shop. Nothing is merged.');
    expect(app.connection.calls('workspace.pr_open')).toHaveLength(0);
  });

  test('Open sends the title and the description, once, and shows the address', async () => {
    const app = await open();
    await draw(app, <PullRequestScreen />);
    await fireEvent.changeText(screen.getByTestId('pr.field.title'), '  Cart: add the tax  ');
    await fireEvent.changeText(screen.getByTestId('pr.field.body'), 'The total now holds the tax.');
    await fireEvent.press(screen.getByTestId('pr.open'));
    await app.settle();

    expect(app.connection.calls('workspace.pr_open')).toEqual([{ workspace_id: WORKSPACE, title: 'Cart: add the tax', body: 'The total now holds the tax.' }]);
    expect(screen.getByTestId('pr.opened')).toHaveTextContent('Pull request #12 is open.');
    expect(screen.getByText('Nothing was merged.')).toBeTruthy();
    expect(screen.getByTestId('pr.url')).toHaveTextContent(new RegExp(ADDRESS.replace(/[/.]/g, '\\$&')));
    expect(screen.queryByTestId('pr.open')).toBeNull();
    expect(screen.queryByTestId('pr.field.title')).toBeNull();

    await fireEvent.press(screen.getByTestId('pr.browser'));
    expect(Linking.openURL).toHaveBeenCalledWith(ADDRESS);
    await act(async () => {
      await fireEvent.press(screen.getByTestId('pr.copy'));
    });
    expect(Clipboard.setStringAsync).toHaveBeenCalledWith(ADDRESS);
    expect(screen.getByTestId('pr.copy')).toHaveTextContent(/Copied$/);
  });

  test('an empty description is left to the Mac to write', async () => {
    const app = await open();
    await draw(app, <PullRequestScreen />);
    await fireEvent.press(screen.getByTestId('pr.open'));
    await app.settle();
    expect(app.connection.calls('workspace.pr_open')).toEqual([{ workspace_id: WORKSPACE, title: 'Fix the cart' }]);
  });

  test('a title the owner wrote is not replaced when the plan arrives again', async () => {
    const app = await open({ connection: 'unreachable' });
    await draw(app, <PullRequestScreen />);
    expect(app.connection.calls('workspace.pr_plan')).toHaveLength(0);
    await act(async () => app.connection.go('online'));
    await app.settle();
    await fireEvent.changeText(screen.getByTestId('pr.field.title'), 'My own title');

    await act(async () => app.connection.go('reconnecting'));
    await act(async () => app.connection.go('online'));
    await app.settle();
    expect(app.connection.calls('workspace.pr_plan')).toHaveLength(2);
    expect(screen.getByTestId('pr.field.title').props.value).toBe('My own title');
  });

  test('a pull request that was open already says so', async () => {
    const app = await open();
    app.connection.answers['workspace.pr_open'] = () => ({ ...OPENED, reused: true });
    await draw(app, <PullRequestScreen />);
    await fireEvent.press(screen.getByTestId('pr.open'));
    await app.settle();
    expect(screen.getByTestId('pr.opened')).toHaveTextContent('Pull request #12 was open already.');
  });

  test('a failure is said in the words of the Mac, and Open can be pressed again', async () => {
    const app = await open();
    app.connection.answers['workspace.pr_open'] = () => {
      throw refusal('mac_setup', 'The GitHub CLI on the Mac is not signed in. Run gh auth login there.');
    };
    await draw(app, <PullRequestScreen />);
    await fireEvent.press(screen.getByTestId('pr.open'));
    await app.settle();

    expect(screen.getByTestId('pr.error')).toHaveTextContent('The GitHub CLI on the Mac is not signed in. Run gh auth login there.');
    expect(screen.queryByTestId('pr.opened')).toBeNull();
    expect(screen.getByTestId('pr.field.title').props.value).toBe('Fix the cart');

    app.connection.answers['workspace.pr_open'] = () => OPENED;
    await fireEvent.press(screen.getByTestId('pr.open'));
    await app.settle();
    expect(screen.queryByTestId('pr.error')).toBeNull();
    expect(screen.getByTestId('pr.opened')).toBeTruthy();
    expect(app.connection.calls('workspace.pr_open')).toHaveLength(2);
  });

  test('a failure of several lines is said as its first line', async () => {
    const app = await open();
    app.connection.answers['workspace.pr_open'] = () => {
      throw new Error('git push failed: remote: Permission to owner/shop.git denied.\nfatal: unable to access the remote');
    };
    await draw(app, <PullRequestScreen />);
    await fireEvent.press(screen.getByTestId('pr.open'));
    await app.settle();
    expect(screen.getByTestId('pr.error')).toHaveTextContent('git push failed: remote: Permission to owner/shop.git denied.');
  });

  test('Open waits for a title', async () => {
    const app = await open();
    await draw(app, <PullRequestScreen />);
    await fireEvent.changeText(screen.getByTestId('pr.field.title'), '   ');
    await fireEvent.press(screen.getByTestId('pr.open'));
    await app.settle();
    expect(app.connection.calls('workspace.pr_open')).toHaveLength(0);
  });

  test('a pull request that cannot be opened says why', async () => {
    const app = await open();
    app.connection.answers['workspace.pr_plan'] = () => ({ ok: false, reason: 'The remote origin (https://gitlab.com/owner/shop.git) is not on GitHub; Open PR only supports github.com remotes.' });
    await draw(app, <PullRequestScreen />);
    expect(screen.getByText('The remote origin (https://gitlab.com/owner/shop.git) is not on GitHub; Open PR only supports github.com remotes.')).toBeTruthy();
    expect(screen.queryByTestId('pr.open')).toBeNull();
    expect(screen.queryByTestId('pr.field.title')).toBeNull();
  });

  test('a phone that may only watch sees the plan and no Open', async () => {
    const app = await open({ scope: 'watch' });
    await draw(app, <PullRequestScreen />);
    expect(screen.getByTestId('pr.plan.branch')).toBeTruthy();
    expect(screen.getByTestId('pr.plan.title')).toHaveTextContent(/Fix the cart/);
    expect(screen.queryByTestId('pr.open')).toBeNull();
    expect(screen.queryByTestId('pr.field.title')).toBeNull();
    expect(screen.queryByTestId('pr.field.body')).toBeNull();
    expect(screen.getByTestId('watch.line')).toBeTruthy();
  });

  test('an address that is not a web address is not opened', async () => {
    (Linking.openURL as jest.Mock).mockClear();
    const app = await open();
    app.connection.answers['workspace.pr_open'] = () => ({ ...OPENED, url: 'javascript:alert(1)' });
    await draw(app, <PullRequestScreen />);
    await fireEvent.press(screen.getByTestId('pr.open'));
    await app.settle();
    await fireEvent.press(screen.getByTestId('pr.browser'));
    await app.settle();
    expect(Linking.openURL).not.toHaveBeenCalled();
  });
});
