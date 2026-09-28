import { act, fireEvent, screen } from '@testing-library/react-native';

import type { Params } from '@/protocol';
import { ChangesScreen } from '@/screens/ChangesScreen';
import { REFRESH_AFTER_MS } from '@/screens/review/activity';
import type { ReviewStore } from '@/screens/review/comparison';
import { comparisons, draw, hunk, hunksOf, LATEST, letLongTimersGo, notShown, RUN, stateWith, TASK_START, TIP, wait, WORKSPACE } from '@/screens/review/testing';
import { createTestApp, makeEvent, type TestApp } from '@/testing';
import { router } from '@/testing/router';

jest.mock('expo-router', () => require('@/testing/router').mockRouter());
jest.mock('@shopify/flash-list/dist/recyclerview/utils/measureLayout', () => ({
  ...jest.requireActual('@shopify/flash-list/dist/recyclerview/utils/measureLayout'),
  measureParentSize: () => ({ x: 0, y: 0, width: 400, height: 900 }),
  measureFirstChildLayout: () => ({ x: 0, y: 0, width: 400, height: 900 }),
  measureItemLayout: () => ({ x: 0, y: 0, width: 400, height: 48 }),
}));

const CART = [hunk('src/cart.ts', 12, ['const total = 0;'], ['const total = sum(items);', 'const tax = total * rate;']), hunk('src/cart.ts', 30, ['return total;'], ['return total + tax;'])];
const NEW = [hunk('src/tax.ts', 1, [], ['export const rate = 0.2;', 'export const free = 50;', ''])];
const FIRST = CART[0]?.key ?? '';
const SECOND = CART[1]?.key ?? '';

const THREE = [{ status: 'M', path: 'src/cart.ts' }, { status: 'A', path: 'src/tax.ts' }, { status: 'D', path: 'logo.png' }];

async function open(options: Parameters<typeof createTestApp>[0] = {}): Promise<TestApp> {
  const app = await createTestApp({ state: stateWith(), ...options });
  app.connection.answers['comparison.options'] = (p: Params<'comparison.options'>) => comparisons(p.branch ?? 'main');
  app.connection.answers['workspace.diff'] = (p: Params<'workspace.diff'>) => ({
    workspace_id: WORKSPACE, root: '/Users/owner/shop-wt', base: p.base, current_tree: 't', index_tree: 'i', head: 'h',
    changes: p.base === LATEST ? THREE : [{ status: 'M', path: 'src/cart.ts' }],
    status: { branch: 'overseer/fix-cart', head: 'h', staged: [], unstaged: [], untracked: [], conflicted: [] },
  });
  app.connection.answers['workspace.hunks'] = (p: Params<'workspace.hunks'>) =>
    p.path === 'src/cart.ts' ? hunksOf(p.path, p.base, CART) : p.path === 'src/tax.ts' ? hunksOf(p.path, p.base, NEW) : notShown(p.path, p.base, 'binary', 'a binary file');
  app.connection.answers['review.marks'] = () => ({ run_id: RUN, keys: [FIRST], marks: [{ key: FIRST, path: 'src/cart.ts', at_ms: 1, by: 'the Mac' }] });
  router.params = { run: RUN };
  return app;
}

/** The time the review waits after news before it asks the Mac again. */
const afterNews = async (app: TestApp): Promise<void> => {
  await act(() => wait(REFRESH_AFTER_MS + 100));
  await app.settle();
};

beforeAll(letLongTimersGo);
beforeEach(() => router.reset());

describe('the changed files of an agent', () => {
  test('lists the files by folder, with their letters, counts and how much is reviewed', async () => {
    const app = await open();
    await draw(app, <ChangesScreen />);
    await app.settle();

    expect(screen.getByTestId('changes.title')).toHaveTextContent('Changes');
    expect(screen.getByTestId('changes.subtitle')).toHaveTextContent('Fix the cart');
    expect(screen.getByTestId('changes.comparison.label')).toHaveTextContent('Latest run');
    expect(screen.getByTestId('changes.folder.src/')).toBeTruthy();
    expect(screen.getByTestId('changes.row.src/cart.ts.status')).toHaveTextContent('M');
    expect(screen.getByTestId('changes.row.src/tax.ts.status')).toHaveTextContent('A');
    expect(screen.getByTestId('changes.row.logo.png.status')).toHaveTextContent('D');

    expect(screen.getByTestId('changes.row.src/cart.ts.added')).toHaveTextContent('+3');
    expect(screen.getByTestId('changes.row.src/cart.ts.removed')).toHaveTextContent('−2');
    expect(screen.getByTestId('changes.row.src/cart.ts.reviewed')).toHaveTextContent('1 of 2 reviewed');
    expect(screen.getByTestId('changes.row.src/tax.ts.added')).toHaveTextContent('+3');
    expect(screen.getByTestId('changes.row.src/tax.ts.reviewed')).toHaveTextContent('0 of 1 reviewed');
    expect(screen.getByLabelText(/logo\.png, D, Deleted file, Not shown: a binary file\./)).toBeTruthy();

    // The summary counts every file once its hunks are known.
    expect(screen.getByTestId('changes.summary')).toHaveTextContent('3 files+6−2');
    expect(app.connection.calls('review.marks')).toEqual([{ run_id: RUN }]);
    expect(app.connection.calls('workspace.diff')).toEqual([{ workspace_id: WORKSPACE, base: LATEST, status: true }]);
    expect(app.connection.calls('workspace.hunks')).toHaveLength(3);
  });

  test('the letters take the colours of the theme by kind', async () => {
    const app = await open({ appearance: 'dark' });
    await draw(app, <ChangesScreen />);
    const { palettes } = require('@/theme/tokens.generated') as typeof import('@/theme/tokens.generated');
    expect(screen.getByTestId('changes.row.src/cart.ts.status')).toHaveStyle({ color: palettes.dark.amber });
    expect(screen.getByTestId('changes.row.src/tax.ts.status')).toHaveStyle({ color: palettes.dark.green });
    expect(screen.getByTestId('changes.row.logo.png.status')).toHaveStyle({ color: palettes.dark.red });
  });

  test('choosing a comparison asks the Mac again and is kept for the run', async () => {
    const app = await open();
    const first = await draw(app, <ChangesScreen />);
    await fireEvent.press(screen.getByTestId('changes.comparison'));

    // One that is unavailable says why and is not a control.
    const fork = screen.getByTestId('changes.comparison.menu.fork');
    expect(fork).toHaveTextContent(/Original fork/);
    expect(fork).toHaveTextContent(/unavailable/);
    expect(fork).toHaveTextContent(/no fork commit was recorded/);
    expect(fork.props.onPress).toBeUndefined();
    expect(fork.props.accessibilityRole).not.toBe('button');

    await fireEvent.press(screen.getByTestId('changes.comparison.menu.task_start'));
    await app.settle();
    expect(app.connection.calls('workspace.diff')).toEqual([
      { workspace_id: WORKSPACE, base: LATEST, status: true },
      { workspace_id: WORKSPACE, base: TASK_START, status: true },
    ]);
    expect(screen.getByTestId('changes.comparison.label')).toHaveTextContent('Since task start');
    expect(screen.queryByTestId('changes.row.src/tax.ts')).toBeNull();
    expect(screen.getByTestId('changes.row.src/cart.ts')).toBeTruthy();
    expect(app.platform.capabilities.keyValue.scope<ReviewStore>('review').get('comparisons')).toEqual({ [RUN]: { mode: 'task_start', branch: null } });

    // Opened again, the run has the comparison that was chosen for it.
    await first.unmount();
    await draw(app, <ChangesScreen />);
    await app.settle();
    expect(screen.getByTestId('changes.comparison.label')).toHaveTextContent('Since task start');
    expect(app.connection.calls('workspace.diff').at(-1)).toEqual({ workspace_id: WORKSPACE, base: TASK_START, status: true });
  });

  test('another branch is chosen in two steps: the branch, then how to compare with it', async () => {
    const app = await open();
    await draw(app, <ChangesScreen />);
    await fireEvent.press(screen.getByTestId('changes.comparison'));
    await fireEvent.press(screen.getByTestId('changes.comparison.menu.other'));
    await fireEvent.press(screen.getByTestId('changes.comparison.menu.branch.release'));
    await fireEvent.press(screen.getByTestId('changes.comparison.menu.kind.branch_tip'));
    await app.settle();
    expect(app.connection.calls('comparison.options').at(-1)).toEqual({ run_id: RUN, branch: 'release' });
    expect(app.connection.calls('workspace.diff').at(-1)).toEqual({ workspace_id: WORKSPACE, base: `${TIP}-release`, status: true });
    expect(screen.getByTestId('changes.comparison.label')).toHaveTextContent('Tip of release (direct)');
  });

  test('the changes are asked again while the agent edits, once for edits that follow each other', async () => {
    const app = await open({ state: stateWith({ status: 'running' }) });
    await draw(app, <ChangesScreen />);
    await app.settle();
    expect(app.connection.calls('workspace.diff')).toHaveLength(1);

    await app.events(makeEvent('file_activity', { paths: ['src/cart.ts'] }, { run_id: RUN, source: 'claude' }));
    await app.events(makeEvent('file_activity', { paths: ['src/tax.ts'] }, { run_id: RUN, source: 'claude' }));
    expect(app.connection.calls('workspace.diff')).toHaveLength(1);
    await afterNews(app);
    expect(app.connection.calls('workspace.diff')).toHaveLength(2);

    // An edit of another agent is not news here.
    await app.events(makeEvent('file_activity', { paths: ['x.ts'] }, { run_id: 'someone-else', source: 'claude' }));
    await afterNews(app);
    expect(app.connection.calls('workspace.diff')).toHaveLength(2);

    // The end of a turn is.
    await app.events(makeEvent('turn_done', { ok: true }, { run_id: RUN }));
    await afterNews(app);
    expect(app.connection.calls('workspace.diff')).toHaveLength(3);
  });

  test('pulling down asks again', async () => {
    const app = await open();
    await draw(app, <ChangesScreen />);
    await act(async () => {
      await fireEvent(screen.getByTestId('changes.list'), 'refresh');
    });
    await app.settle();
    expect(app.connection.calls('workspace.diff')).toHaveLength(2);
  });

  test('a mark made on the Mac shows by itself', async () => {
    const app = await open();
    await draw(app, <ChangesScreen />);
    await app.settle();
    expect(screen.getByTestId('changes.row.src/cart.ts.reviewed')).toHaveTextContent('1 of 2 reviewed');

    app.connection.answers['review.marks'] = () => ({ run_id: RUN, keys: [FIRST, SECOND], marks: [] });
    await app.events(makeEvent('review_mark', { key: SECOND, path: 'src/cart.ts', reviewed: true }, { run_id: RUN, source: 'user' }));
    await afterNews(app);
    expect(screen.getByTestId('changes.row.src/cart.ts.reviewed')).toHaveTextContent('2 of 2 reviewed');
  });

  test('the filter keeps the files whose path holds what was typed', async () => {
    const app = await open();
    await draw(app, <ChangesScreen />);
    await fireEvent.changeText(screen.getByTestId('changes.filter'), 'tax');
    expect(screen.getByTestId('changes.row.src/tax.ts')).toBeTruthy();
    expect(screen.queryByTestId('changes.row.src/cart.ts')).toBeNull();
    expect(screen.queryByTestId('changes.row.logo.png')).toBeNull();

    await fireEvent.changeText(screen.getByTestId('changes.filter'), 'nothing-like-it');
    expect(screen.getByTestId('changes.empty')).toHaveTextContent('No files match.');
    await fireEvent.press(screen.getByTestId('changes.filter.clear'));
    expect(screen.getByTestId('changes.row.src/cart.ts')).toBeTruthy();
  });

  test('a folder folds and unfolds', async () => {
    const app = await open();
    await draw(app, <ChangesScreen />);
    await fireEvent.press(screen.getByTestId('changes.folder.src/'));
    expect(screen.queryByTestId('changes.row.src/cart.ts')).toBeNull();
    expect(screen.getByTestId('changes.row.logo.png')).toBeTruthy();
    await fireEvent.press(screen.getByTestId('changes.folder.src/'));
    expect(screen.getByTestId('changes.row.src/cart.ts')).toBeTruthy();
  });

  test('a file opens its changes against the comparison in use', async () => {
    const app = await open();
    await draw(app, <ChangesScreen />);
    await fireEvent.press(screen.getByTestId('changes.row.src/cart.ts'));
    expect(router.pushed).toEqual([{ pathname: `/agent/${RUN}/file`, params: { path: 'src/cart.ts', comparison: 'latest_run:' } }]);
  });

  test('nothing changed is one sentence', async () => {
    const app = await open();
    app.connection.answers['workspace.diff'] = (p: Params<'workspace.diff'>) => ({ workspace_id: WORKSPACE, root: '/', base: p.base, current_tree: 't', index_tree: 'i', changes: [], status: null });
    await draw(app, <ChangesScreen />);
    await app.settle();
    expect(screen.getByTestId('changes.empty')).toHaveTextContent('No changes for this comparison.');
    expect(screen.queryByTestId('changes.summary')).toBeNull();
  });

  test('what the Mac refuses is said in its words', async () => {
    const app = await open();
    app.connection.answers['workspace.diff'] = () => {
      throw new Error('comparison base aaaa is not available in this repository');
    };
    await draw(app, <ChangesScreen />);
    await app.settle();
    expect(screen.getByTestId('changes.empty')).toHaveTextContent('comparison base aaaa is not available in this repository');
  });

  test('a phone that may only watch sees the same list', async () => {
    const app = await open({ scope: 'watch' });
    await draw(app, <ChangesScreen />);
    await app.settle();
    expect(screen.getByTestId('changes.row.src/cart.ts.reviewed')).toHaveTextContent('1 of 2 reviewed');
    expect(screen.getByTestId('changes.comparison')).toBeTruthy();
  });

  test('while the Mac cannot be reached nothing is asked, and it is asked when it is back', async () => {
    const app = await open({ connection: 'unreachable' });
    await draw(app, <ChangesScreen />);
    expect(screen.getByTestId('connection.unreachable')).toBeTruthy();
    expect(screen.getByTestId('changes.empty')).toHaveTextContent('Shown when the Mac is reached.');
    expect(app.connection.calls('workspace.diff')).toHaveLength(0);

    await act(async () => app.connection.go('online'));
    await app.settle();
    await app.settle();
    expect(app.connection.calls('workspace.diff')).toHaveLength(1);
    expect(screen.getByTestId('changes.row.src/cart.ts')).toBeTruthy();

    // Not reachable again: what was shown stays, marked with its age.
    await act(async () => app.connection.go('unreachable'));
    expect(screen.getByTestId('changes.row.src/cart.ts')).toBeTruthy();
    expect(screen.getByTestId('changes.age')).toHaveTextContent(/^As of /);
  });
});
