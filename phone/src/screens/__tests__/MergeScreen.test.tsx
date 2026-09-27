import { act, fireEvent, screen } from '@testing-library/react-native';

import type { Params } from '@/protocol';
import { MergeScreen } from '@/screens/MergeScreen';
import { REFRESH_AFTER_MS } from '@/screens/review/activity';
import type { ReviewStore } from '@/screens/review/comparison';
import { comparisons, draw, letLongTimersGo, MERGE_BASE, refusal, RUN, stateWith, wait, WORKSPACE, workspace } from '@/screens/review/testing';
import { createTestApp, makeEvent, type TestApp } from '@/testing';
import { router } from '@/testing/router';

jest.mock('expo-router', () => require('@/testing/router').mockRouter());

type State = 'idle' | 'ready' | 'resolving' | 'resolved';

/** What the daemon's `merge_plan` answers (daemon/src/merge.rs). */
const planOf = (state: State, change: Record<string, unknown> = {}): Record<string, unknown> => ({
  ok: true, state, workspace, run_id: RUN, repo: '/Users/owner/shop', branch: 'overseer/fix-cart', target: 'main',
  worktree_uncommitted: state === 'idle' ? ['src/cart.ts', 'src/tax.ts'] : [], conflicts: state === 'resolving' ? ['src/cart.ts'] : [], source_branch: 'main', source_dirty: [],
  blockers: [], can_complete: state === 'ready', ...change,
});

let plan: Record<string, unknown> = planOf('idle');

async function open(options: Parameters<typeof createTestApp>[0] = {}): Promise<TestApp> {
  const app = await createTestApp({ state: stateWith(), ...options });
  app.connection.answers['workspace.merge_plan'] = () => plan;
  app.connection.answers['comparison.options'] = (p: Params<'comparison.options'>) => comparisons(p.branch ?? 'main');
  app.connection.answers['workspace.diff'] = (p: Params<'workspace.diff'>) => ({
    workspace_id: WORKSPACE, root: '/', base: p.base, current_tree: 't', index_tree: 'i', changes: [{ status: 'M', path: 'src/cart.ts' }, { status: 'A', path: 'src/tax.ts' }], status: null,
  });
  router.params = { run: RUN };
  return app;
}

beforeAll(letLongTimersGo);
beforeEach(() => {
  router.reset();
  plan = planOf('idle');
});

describe('merge back', () => {
  test('shows the plan and says what each step will do before anything is done', async () => {
    const app = await open();
    await draw(app, <MergeScreen />);

    expect(app.connection.calls('workspace.merge_plan')).toEqual([{ workspace_id: WORKSPACE }]);
    expect(screen.getByTestId('merge.plan.merge')).toHaveTextContent(/overseer\/fix-cart → main/);
    expect(screen.getByTestId('merge.plan.repository')).toHaveTextContent(/shop/);
    expect(screen.getByTestId('merge.plan.state')).toHaveTextContent(/Not prepared/);
    expect(screen.getByTestId('merge.step.1')).toHaveTextContent(/Commit 2 files that are not committed to overseer\/fix-cart\./);
    expect(screen.getByTestId('merge.step.1')).toHaveTextContent(/cart\.ts, tax\.ts/);
    expect(screen.getByTestId('merge.step.2')).toHaveTextContent('Merge main into overseer/fix-cart inside the worktree. Conflicts go back to the agent.');
    expect(screen.getByTestId('merge.step.3')).toHaveTextContent('You see what will land, then confirm. Nothing reaches main before that.');
    expect(screen.getByTestId('merge.next')).toHaveTextContent('Merge main into overseer/fix-cart inside the worktree. Conflicts go back to the agent.');

    // Nothing was done by opening the screen.
    expect(app.connection.calls('workspace.merge_prepare')).toHaveLength(0);
    expect(screen.queryByTestId('merge.complete')).toBeNull();
    expect(screen.queryByTestId('merge.abort')).toBeNull();
  });

  test('Prepare, then the merge is ready: what will land is shown, and Complete asks once', async () => {
    const app = await open();
    app.connection.answers['workspace.merge_prepare'] = () => {
      plan = planOf('ready');
      return { state: 'ready', target: 'main', branch: 'overseer/fix-cart' };
    };
    app.connection.answers['workspace.merge_complete'] = () => {
      plan = { ok: false, reason: 'Nothing to merge: overseer/fix-cart has no changes that are not already on main.' };
      return { merged: true, repo: '/Users/owner/shop', target: 'main', branch: 'overseer/fix-cart', before: 'a', commit: '0123456789abcdef' };
    };
    await draw(app, <MergeScreen />);

    await fireEvent.press(screen.getByTestId('merge.prepare'));
    await app.settle();
    expect(app.connection.calls('workspace.merge_prepare')).toEqual([{ workspace_id: WORKSPACE, handoff: true }]);
    expect(screen.getByTestId('merge.plan.state')).toHaveTextContent(/Ready to merge/);

    // What lands is the merge-base comparison with the target, as in VS Code.
    expect(app.connection.calls('comparison.options')).toEqual([{ run_id: RUN, branch: 'main' }]);
    expect(app.connection.calls('workspace.diff')).toEqual([{ workspace_id: WORKSPACE, base: `${MERGE_BASE}-main`, status: false }]);
    expect(screen.getByTestId('merge.lands')).toHaveTextContent(/2 files/);
    expect(screen.getByTestId('merge.lands.src/tax.ts')).toHaveTextContent(/tax\.ts/);
    expect(screen.getByTestId('merge.next')).toHaveTextContent('Merge overseer/fix-cart into main in shop. The worktree and overseer/fix-cart are kept.');

    await fireEvent.press(screen.getByTestId('merge.complete'));
    expect(app.connection.calls('workspace.merge_complete')).toHaveLength(0);
    expect(screen.getByText('Merge overseer/fix-cart into main in shop?')).toBeTruthy();
    expect(screen.getByText('2 files will land on main. This cannot be taken back from the phone.')).toBeTruthy();
    await fireEvent.press(screen.getByTestId('merge.ask.confirm'));
    await app.settle();

    expect(app.connection.calls('workspace.merge_complete')).toEqual([{ workspace_id: WORKSPACE }]);
    expect(screen.getByText('Merged overseer/fix-cart into main (0123456789). The worktree and the branch are kept.')).toBeTruthy();
    expect(screen.queryByTestId('merge.complete')).toBeNull();
  });

  test('cancelling the question sends nothing', async () => {
    plan = planOf('ready');
    const app = await open();
    await draw(app, <MergeScreen />);
    await fireEvent.press(screen.getByTestId('merge.complete'));
    await fireEvent.press(screen.getByTestId('merge.ask.cancel'));
    await app.settle();
    expect(app.connection.calls('workspace.merge_complete')).toHaveLength(0);
    expect(screen.getByTestId('merge.complete')).toBeTruthy();
  });

  test('conflicts are listed by file and go back to the agent; Continue finishes once they are resolved', async () => {
    const app = await open({ state: stateWith({ status: 'running' }) });
    app.connection.answers['workspace.merge_prepare'] = () => {
      plan = planOf('resolving');
      return { state: 'conflicts', files: ['src/cart.ts'], target: 'main', branch: 'overseer/fix-cart', handoff: { sent: true, run_id: RUN, turn: {} } };
    };
    let remaining = ['src/cart.ts'];
    app.connection.answers['workspace.merge_resolved'] = () => {
      if (remaining.length > 0) return { state: 'resolving', remaining };
      plan = planOf('ready');
      return { state: 'ready', resolved: ['src/cart.ts'] };
    };
    await draw(app, <MergeScreen />);
    plan = planOf('idle');
    await fireEvent.press(screen.getByTestId('merge.prepare'));
    await app.settle();

    expect(screen.getByTestId('merge.outcome')).toHaveTextContent('Sent to the agent. Continue when it finishes.');
    expect(screen.getByTestId('merge.plan.state')).toHaveTextContent(/Conflicts to resolve/);
    expect(screen.getByTestId('merge.conflict.src/cart.ts')).toHaveTextContent(/cart\.ts/);
    expect(screen.getByTestId('merge.next')).toHaveTextContent('Check that no conflict marks remain, then finish the merge in the worktree.');
    expect(screen.getByTestId('merge.abort')).toBeTruthy();

    // A conflicted file opens its changes.
    await fireEvent.press(screen.getByTestId('merge.conflict.src/cart.ts'));
    expect(router.pushed).toEqual([{ pathname: `/agent/${RUN}/file`, params: { path: 'src/cart.ts' } }]);

    await fireEvent.press(screen.getByTestId('merge.continue'));
    await app.settle();
    expect(app.connection.calls('workspace.merge_resolved')).toEqual([{ workspace_id: WORKSPACE }]);
    expect(screen.getByTestId('merge.outcome')).toHaveTextContent('Conflict marks remain in src/cart.ts.');

    remaining = [];
    await fireEvent.press(screen.getByTestId('merge.continue'));
    await app.settle();
    expect(screen.getByTestId('merge.plan.state')).toHaveTextContent(/Ready to merge/);
    expect(screen.getByTestId('merge.complete')).toBeTruthy();
    expect(screen.queryByTestId('merge.abort')).toBeNull();
  });

  test('Abort asks once, naming what is lost, and sends one request', async () => {
    plan = planOf('resolving');
    const app = await open();
    app.connection.answers['workspace.merge_abort'] = () => {
      plan = planOf('idle');
      return { aborted: true };
    };
    await draw(app, <MergeScreen />);

    await fireEvent.press(screen.getByTestId('merge.abort'));
    expect(app.connection.calls('workspace.merge_abort')).toHaveLength(0);
    expect(screen.getByText('Abort this merge?')).toBeTruthy();
    expect(screen.getByText(/What was resolved so far in the worktree is lost\./)).toBeTruthy();
    await fireEvent.press(screen.getByTestId('merge.ask.confirm'));
    await app.settle();

    expect(app.connection.calls('workspace.merge_abort')).toEqual([{ workspace_id: WORKSPACE }]);
    expect(screen.getByTestId('merge.outcome')).toHaveTextContent('The merge was aborted. Nothing reached the target branch.');
    expect(screen.getByTestId('merge.plan.state')).toHaveTextContent(/Not prepared/);
  });

  test('a merge that is blocked says why in the words of the Mac and offers no Complete', async () => {
    plan = planOf('ready', { can_complete: false, blockers: ['The source checkout /Users/owner/shop is on release — switch it to main to merge back.'] });
    const app = await open();
    await draw(app, <MergeScreen />);
    expect(screen.getByTestId('merge.blocker.0')).toHaveTextContent(/The source checkout \/Users\/owner\/shop is on release — switch it to main to merge back\./);
    expect(screen.queryByTestId('merge.complete')).toBeNull();
  });

  test('a merge that cannot be says why', async () => {
    plan = { ok: false, reason: 'This task works directly in the current checkout, so there is no separate branch to merge back.', workspace };
    const app = await open();
    await draw(app, <MergeScreen />);
    expect(screen.getByText('Merge back is unavailable: This task works directly in the current checkout, so there is no separate branch to merge back.')).toBeTruthy();
    expect(screen.queryByTestId('merge.prepare')).toBeNull();
  });

  test('what the Mac refuses during a step is said in its words', async () => {
    const app = await open();
    app.connection.answers['workspace.merge_prepare'] = () => {
      throw refusal('internal', 'git merge of main into overseer/fix-cart failed: unrelated histories');
    };
    await draw(app, <MergeScreen />);
    await fireEvent.press(screen.getByTestId('merge.prepare'));
    await app.settle();
    expect(screen.getByTestId('merge.outcome')).toHaveTextContent('git merge of main into overseer/fix-cart failed: unrelated histories');
    expect(screen.getByTestId('merge.prepare')).toBeTruthy();
  });

  test('the plan is asked again when the agent finishes resolving', async () => {
    plan = planOf('resolving');
    const app = await open({ state: stateWith({ status: 'running' }) });
    await draw(app, <MergeScreen />);
    expect(screen.getByTestId('merge.conflict.src/cart.ts')).toBeTruthy();

    plan = planOf('resolved');
    await app.events(makeEvent('turn_done', { ok: true }, { run_id: RUN }));
    await act(() => wait(REFRESH_AFTER_MS + 100));
    await app.settle();
    expect(app.connection.calls('workspace.merge_plan')).toHaveLength(2);
    expect(screen.getByTestId('merge.plan.state')).toHaveTextContent(/Conflicts resolved/);
    expect(screen.queryByTestId('merge.conflict.src/cart.ts')).toBeNull();
  });

  test('what will land opens the review on that comparison', async () => {
    plan = planOf('ready');
    const app = await open();
    await draw(app, <MergeScreen />);
    await fireEvent.press(screen.getByTestId('merge.lands'));
    expect(router.pushed).toEqual([`/agent/${RUN}/changes`]);
    expect(app.platform.capabilities.keyValue.scope<ReviewStore>('review').get('comparisons')).toEqual({ [RUN]: { mode: 'branch_merge_base', branch: 'main' } });

    await fireEvent.press(screen.getByTestId('merge.lands.src/cart.ts'));
    expect(router.pushed.at(-1)).toEqual({ pathname: `/agent/${RUN}/file`, params: { path: 'src/cart.ts', comparison: 'branch_merge_base:main' } });
  });

  test('while the Mac cannot be reached nothing is asked, and the plan arrives when it is back', async () => {
    const app = await open({ connection: 'unreachable' });
    await draw(app, <MergeScreen />);
    expect(screen.getByTestId('merge.empty')).toHaveTextContent('Shown when the Mac is reached.');
    expect(app.connection.calls('workspace.merge_plan')).toHaveLength(0);
    await act(async () => app.connection.go('online'));
    await app.settle();
    await app.settle();
    expect(screen.getByTestId('merge.plan.merge')).toBeTruthy();
  });

  test('a phone that may only watch sees the plan and no step', async () => {
    plan = planOf('resolving');
    const app = await open({ scope: 'watch' });
    await draw(app, <MergeScreen />);
    expect(screen.getByTestId('merge.plan.merge')).toBeTruthy();
    expect(screen.getByTestId('merge.conflict.src/cart.ts')).toBeTruthy();
    expect(screen.queryByTestId('merge.continue')).toBeNull();
    expect(screen.queryByTestId('merge.abort')).toBeNull();
    expect(screen.queryByTestId('merge.prepare')).toBeNull();
    expect(screen.getByTestId('watch.line')).toBeTruthy();
  });
});
