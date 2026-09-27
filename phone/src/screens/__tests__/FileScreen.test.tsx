import { act, fireEvent, screen, within } from '@testing-library/react-native';

import type { Params } from '@/protocol';
import { FileScreen } from '@/screens/FileScreen';
import { REFRESH_AFTER_MS } from '@/screens/review/activity';
import type { ReviewStore } from '@/screens/review/comparison';
import { comparisons, draw, hunk, hunksOf, LATEST, letLongTimersGo, notShown, refusal, RUN, stateWith, TASK_START, wait, WORKSPACE } from '@/screens/review/testing';
import { createTestApp, makeEvent, type TestApp } from '@/testing';
import { router } from '@/testing/router';
import { palettes, phone } from '@/theme/tokens.generated';
import { faded } from '@/theme';

jest.mock('expo-router', () => require('@/testing/router').mockRouter());
jest.mock('@shopify/flash-list/dist/recyclerview/utils/measureLayout', () => ({
  ...jest.requireActual('@shopify/flash-list/dist/recyclerview/utils/measureLayout'),
  measureParentSize: () => ({ x: 0, y: 0, width: 400, height: 900 }),
  measureFirstChildLayout: () => ({ x: 0, y: 0, width: 400, height: 900 }),
  measureItemLayout: () => ({ x: 0, y: 0, width: 400, height: 20 }),
}));

const PATH = 'src/cart.ts';
const ONE = hunk(PATH, 12, ['const total = 0; // nothing yet'], ['const total = sum(items);', 'const label = "Total";', 'const tax = total * 0.2;']);
const TWO = hunk(PATH, 30, ['return total;'], ['return total + tax;']);
const ADDED = hunk(PATH, 50, [], ['export default cart;']);

let hunks = [ONE, TWO];

async function open(params: Record<string, string> = {}, options: Parameters<typeof createTestApp>[0] = {}): Promise<TestApp> {
  const app = await createTestApp({ state: stateWith(), appearance: 'dark', ...options });
  app.connection.answers['comparison.options'] = (p: Params<'comparison.options'>) => comparisons(p.branch ?? 'main');
  app.connection.answers['workspace.hunks'] = (p: Params<'workspace.hunks'>) => hunksOf(p.path, p.base, hunks);
  app.connection.answers['review.marks'] = () => ({ run_id: RUN, keys: [], marks: [] });
  app.connection.answers['review.accept'] = (p: Params<'review.accept'>) => ({ key: p.key, reviewed: true });
  app.connection.answers['review.unaccept'] = (p: Params<'review.unaccept'>) => ({ key: p.key, reviewed: false, changed: true });
  app.connection.answers['review.reject'] = (p: Params<'review.reject'>) => ({ path: p.path, rejected: true, file_removed: false });
  router.params = { run: RUN, path: PATH, ...params };
  return app;
}

const heading = (key: string) => within(screen.getAllByTestId(`file.hunk.${key}`)[0] as ReturnType<typeof screen.getByTestId>);
const afterNews = async (app: TestApp): Promise<void> => {
  await act(() => wait(REFRESH_AFTER_MS + 100));
  await app.settle();
};

beforeAll(letLongTimersGo);
beforeEach(() => {
  router.reset();
  hunks = [ONE, TWO];
});

describe("a file's changes", () => {
  test('shows each hunk: where it is, the removed lines, then the added ones, with their numbers', async () => {
    const app = await open();
    await draw(app, <FileScreen />);
    await app.settle();

    expect(screen.getByTestId('file.title')).toHaveTextContent('cart.ts');
    expect(screen.getByTestId('file.subtitle')).toHaveTextContent('src/');
    expect(app.connection.calls('workspace.hunks')).toEqual([{ workspace_id: WORKSPACE, path: PATH, base: LATEST, run_id: RUN }]);

    expect(heading(ONE.key).getByTestId(`file.hunk.${ONE.key}.where`)).toHaveTextContent('Lines 12 to 14');
    expect(heading(TWO.key).getByTestId(`file.hunk.${TWO.key}.where`)).toHaveTextContent('Line 30');
    expect(screen.getAllByLabelText(`Hunk 1 of ${PATH}, lines 12–14`).length).toBeGreaterThan(0);
    expect(screen.getByTestId('file.about')).toHaveTextContent('Latest run+4−20 of 2 reviewed');

    const removed = screen.getByTestId(`file.line.${ONE.key}:-0`);
    const added = screen.getByTestId(`file.line.${ONE.key}:+2`);
    expect(removed).toHaveStyle({ backgroundColor: faded(palettes.dark.removedBg, phone.opacity.diffTint) });
    expect(added).toHaveStyle({ backgroundColor: faded(palettes.dark.addedBg, phone.opacity.diffTint) });
    expect(removed).toHaveTextContent('12−const total = 0; // nothing yet');
    expect(added).toHaveTextContent('14+const tax = total * 0.2;');
    expect(screen.getByLabelText('Removed, line 12: const total = 0; // nothing yet')).toBeTruthy();

    // Removed lines come before added ones.
    const drawn = screen.getAllByTestId(/^file\.line\.[0-9a-f]{16}:[-+]\d+$/).map((row) => String(row.props.testID));
    expect(drawn.slice(0, 4)).toEqual([`file.line.${ONE.key}:-0`, `file.line.${ONE.key}:+0`, `file.line.${ONE.key}:+1`, `file.line.${ONE.key}:+2`]);
  });

  test('colours comments, strings, numbers and keywords with the colours of the theme', async () => {
    const app = await open();
    await draw(app, <FileScreen />);
    await app.settle();
    const syntax = palettes.dark.syntax;
    const removed = within(screen.getByTestId(`file.line.${ONE.key}:-0`));
    expect(removed.getByText('const')).toHaveStyle({ color: syntax.keyword });
    expect(removed.getByText('0')).toHaveStyle({ color: syntax.number });
    expect(removed.getByText('// nothing yet')).toHaveStyle({ color: syntax.comment });
    expect(within(screen.getByTestId(`file.line.${ONE.key}:+1`)).getByText('"Total"')).toHaveStyle({ color: syntax.string });
    expect(within(screen.getByTestId(`file.line.${ONE.key}:+2`)).getByText('0.2')).toHaveStyle({ color: syntax.number });
  });

  test('Accept shows at once and sends one request; pressed again it takes the mark away', async () => {
    const app = await open();
    let answer: (value: unknown) => void = () => undefined;
    app.connection.answers['review.accept'] = () => new Promise((resolve) => (answer = resolve));
    await draw(app, <FileScreen />);
    await app.settle();

    expect(heading(ONE.key).getByTestId('file.hunk.accept')).toHaveTextContent('Accept');
    expect(heading(ONE.key).getByLabelText('Accept hunk 1')).toBeTruthy();
    await fireEvent.press(heading(ONE.key).getByTestId('file.hunk.accept'));

    // The Mac has not answered yet.
    expect(heading(ONE.key).getByTestId('file.hunk.accept')).toHaveTextContent(/Reviewed$/);
    expect(heading(ONE.key).getByLabelText('Unmark reviewed hunk 1')).toBeTruthy();
    expect(heading(TWO.key).getByTestId('file.hunk.accept')).toHaveTextContent('Accept');
    expect(screen.getByTestId('file.reviewed')).toHaveTextContent('1 of 2 reviewed');
    expect(app.connection.calls('review.accept')).toEqual([{ run_id: RUN, path: PATH, key: ONE.key, modified_start: 12, modified_lines: ONE.modified_lines, base_lines: ONE.base_lines }]);

    await act(async () => answer({ key: ONE.key, reviewed: true }));
    await app.settle();
    expect(heading(ONE.key).getByTestId('file.hunk.accept')).toHaveTextContent(/Reviewed$/);

    await fireEvent.press(heading(ONE.key).getByTestId('file.hunk.accept'));
    expect(heading(ONE.key).getByTestId('file.hunk.accept')).toHaveTextContent('Accept');
    await app.settle();
    expect(app.connection.calls('review.unaccept')).toEqual([{ run_id: RUN, key: ONE.key }]);
    expect(app.connection.calls('review.accept')).toHaveLength(1);
    expect(screen.getByTestId('file.reviewed')).toHaveTextContent('0 of 2 reviewed');
  });

  test('a hunk the Mac already holds as reviewed shows so', async () => {
    const app = await open();
    app.connection.answers['review.marks'] = () => ({ run_id: RUN, keys: [TWO.key], marks: [] });
    await draw(app, <FileScreen />);
    await app.settle();
    expect(heading(TWO.key).getByTestId('file.hunk.accept')).toHaveTextContent(/Reviewed$/);
    expect(heading(ONE.key).getByTestId('file.hunk.accept')).toHaveTextContent('Accept');
  });

  test('a mark made on the Mac shows by itself', async () => {
    const app = await open();
    await draw(app, <FileScreen />);
    await app.settle();
    expect(heading(TWO.key).getByTestId('file.hunk.accept')).toHaveTextContent('Accept');

    app.connection.answers['review.marks'] = () => ({ run_id: RUN, keys: [TWO.key], marks: [] });
    await app.events(makeEvent('review_mark', { key: TWO.key, path: PATH, reviewed: true }, { run_id: RUN, source: 'user' }));
    await afterNews(app);
    expect(heading(TWO.key).getByTestId('file.hunk.accept')).toHaveTextContent(/Reviewed$/);
    expect(app.connection.calls('review.accept')).toHaveLength(0);
  });

  test('Accept refused because the file changed is taken back, said in one sentence, and the hunks are asked again', async () => {
    const app = await open();
    app.connection.answers['review.accept'] = () => {
      throw refusal('conflict', `Not marked reviewed: ${PATH} changed while you were accepting this hunk (conflict). Review the current content.`);
    };
    await draw(app, <FileScreen />);
    await app.settle();
    await fireEvent.press(heading(ONE.key).getByTestId('file.hunk.accept'));
    await app.settle();
    expect(heading(ONE.key).getByTestId('file.hunk.accept')).toHaveTextContent('Accept');
    expect(screen.getByTestId('file.notice')).toHaveTextContent('Not marked as reviewed, because the file changed since.');
    expect(app.connection.calls('workspace.hunks')).toHaveLength(2);
  });

  test('Reject asks once, names the lines, and sends one request', async () => {
    const app = await open();
    await draw(app, <FileScreen />);
    await app.settle();

    await fireEvent.press(heading(TWO.key).getByTestId('file.hunk.reject'));
    expect(screen.getByText('Put this line back?')).toBeTruthy();
    await fireEvent.press(screen.getByTestId('file.reject.cancel'));
    await app.settle();
    expect(app.connection.calls('review.reject')).toHaveLength(0);

    hunks = [ONE, ADDED];
    await fireEvent.press(heading(TWO.key).getByTestId('file.hunk.reject'));
    await fireEvent.press(screen.getByTestId('file.reject.confirm'));
    await app.settle();
    expect(app.connection.calls('review.reject')).toEqual([{ workspace_id: WORKSPACE, path: PATH, base: LATEST, key: TWO.key }]);
    // The file is asked for again, and shows as it stands now.
    expect(app.connection.calls('workspace.hunks')).toHaveLength(2);
    expect(screen.queryByTestId(`file.hunk.${TWO.key}`)).toBeNull();
    expect(screen.getAllByTestId(`file.hunk.${ADDED.key}`).length).toBeGreaterThan(0);
    expect(screen.queryByTestId('file.notice')).toBeNull();
  });

  test('the question counts the lines that come back, or the added lines that go', async () => {
    const app = await open();
    hunks = [ONE, ADDED];
    await draw(app, <FileScreen />);
    await app.settle();
    await fireEvent.press(heading(ONE.key).getByTestId('file.hunk.reject'));
    expect(screen.getByText('Put this line back?')).toBeTruthy();
    expect(screen.getByText('The 3 lines the agent wrote in their place are removed from the file.')).toBeTruthy();
    expect(screen.getByTestId('file.reject.confirm')).toHaveTextContent('Put back');
    await fireEvent.press(screen.getByTestId('file.reject.cancel'));

    await fireEvent.press(heading(ADDED.key).getByTestId('file.hunk.reject'));
    expect(screen.getByText('Take this added line out?')).toBeTruthy();
  });

  test('"Put these 3 lines back?" for a hunk that removed three', async () => {
    const app = await open();
    hunks = [hunk(PATH, 5, ['a', 'b', 'c'], [])];
    await draw(app, <FileScreen />);
    await app.settle();
    const key = hunks[0]?.key ?? '';
    expect(heading(key).getByTestId(`file.hunk.${key}.where`)).toHaveTextContent('After line 5');
    await fireEvent.press(heading(key).getByTestId('file.hunk.reject'));
    expect(screen.getByText('Put these 3 lines back?')).toBeTruthy();
  });

  test('when the file changed since, Reject says so in one sentence and the hunks are asked again', async () => {
    const app = await open();
    app.connection.answers['review.reject'] = () => {
      throw refusal('conflict', `Not rejected: ${PATH} changed while you were rejecting this hunk (conflict). Review the current content.`);
    };
    await draw(app, <FileScreen />);
    await app.settle();
    await fireEvent.press(heading(ONE.key).getByTestId('file.hunk.reject'));
    await fireEvent.press(screen.getByTestId('file.reject.confirm'));
    await app.settle();

    expect(app.connection.calls('review.reject')).toHaveLength(1);
    expect(screen.getByTestId('file.notice')).toHaveTextContent('Nothing was put back, because the file changed since.');
    expect(app.connection.calls('workspace.hunks')).toHaveLength(2);
    expect(screen.getAllByTestId(`file.hunk.${ONE.key}`).length).toBeGreaterThan(0);
  });

  test.each([
    ['a binary file', 'binary', 'Not shown: a binary file.'],
    ['larger than 2 MB', 'too_large', 'Not shown: larger than 2 MB.'],
    ['a link to ../secrets; links are not followed', 'link', 'Not shown: a link to ../secrets; links are not followed.'],
  ])('a file that cannot be shown says why: %s', async (why, kind, said) => {
    const app = await open();
    app.connection.answers['workspace.hunks'] = (p: Params<'workspace.hunks'>) => notShown(p.path, p.base, kind, why);
    await draw(app, <FileScreen />);
    await app.settle();
    // The icon is a character of the icon font, in front of the sentence.
    expect(within(screen.getByTestId('file.notshown')).getByText(said)).toBeTruthy();
    expect(screen.queryByTestId('file.list')).toBeNull();
    expect(screen.queryByTestId('file.hunk.accept')).toBeNull();
  });

  test('a phone that may only watch sees the hunks and no Accept or Reject', async () => {
    const app = await open({}, { scope: 'watch' });
    app.connection.answers['review.marks'] = () => ({ run_id: RUN, keys: [ONE.key], marks: [] });
    await draw(app, <FileScreen />);
    await app.settle();
    expect(screen.getAllByTestId(`file.hunk.${ONE.key}`).length).toBeGreaterThan(0);
    expect(screen.getByTestId(`file.line.${ONE.key}:+0`)).toBeTruthy();
    expect(heading(ONE.key).getByTestId(`file.hunk.${ONE.key}.reviewed`)).toHaveTextContent(/Reviewed$/);
    expect(screen.queryByTestId('file.hunk.accept')).toBeNull();
    expect(screen.queryByTestId('file.hunk.reject')).toBeNull();
    expect(screen.getByTestId('watch.line')).toHaveTextContent('This phone may watch. Change it on the Mac.');
  });

  test('opened with a hunk, it starts at that hunk; a large file is drawn a screen at a time', async () => {
    const lines = (from: number, word: string): string[] => Array.from({ length: 1500 }, (_, i) => `${word}(${from + i});`);
    const big = [hunk(PATH, 1, [], lines(1, 'first')), hunk(PATH, 2000, [], lines(2000, 'second')), hunk(PATH, 4000, [], lines(4000, 'third'))];
    hunks = big;
    const third = big[2]?.key ?? '';
    const app = await open({ hunk: third });
    await draw(app, <FileScreen />);
    await app.settle();

    expect(screen.getAllByTestId(`file.hunk.${third}`).length).toBeGreaterThan(0);
    expect(screen.getByTestId(`file.line.${third}:+0`)).toBeTruthy();
    expect(screen.queryByTestId(`file.line.${big[0]?.key}:+0`)).toBeNull();
    // 4,500 lines, and only what a screen holds is built.
    expect(screen.getAllByTestId(/^file\.line\.[0-9a-f]{16}:[-+]\d+$/).length).toBeLessThan(200);
  });

  test('opened with a line, it starts at the hunk that holds it', async () => {
    const lines = (from: number): string[] => Array.from({ length: 400 }, (_, i) => `line(${from + i});`);
    hunks = [hunk(PATH, 1, [], lines(1)), hunk(PATH, 1000, [], lines(1000))];
    const second = hunks[1]?.key ?? '';
    const app = await open({ hunk: '1100' });
    await draw(app, <FileScreen />);
    await app.settle();
    expect(screen.getByTestId(`file.line.${second}:+0`)).toBeTruthy();
  });

  test('opened from an edit of the conversation, it starts at the first hunk not reviewed yet', async () => {
    const lines = (from: number): string[] => Array.from({ length: 400 }, (_, i) => `line(${from + i});`);
    hunks = [hunk(PATH, 1, [], lines(1)), hunk(PATH, 1000, [], lines(1000))];
    const [first, second] = hunks;
    const app = await open({ hunk: 'edited' });
    // The marks answer late: the hunks themselves say which of them is marked.
    app.connection.answers['review.marks'] = async () => {
      await wait(30);
      return { run_id: RUN, keys: [first?.key ?? ''], marks: [] };
    };
    app.connection.answers['workspace.hunks'] = (p: Params<'workspace.hunks'>) => hunksOf(p.path, p.base, hunks.map((h, i) => ({ ...h, reviewed: i === 0 })));
    await draw(app, <FileScreen />);
    expect(screen.getByTestId(`file.line.${second?.key}:+0`)).toBeTruthy();
    expect(screen.queryByTestId(`file.line.${first?.key}:+0`)).toBeNull();
  });

  test('long lines wrap where words end, or scroll sideways; the choice is kept', async () => {
    const long = `const message = ${Array.from({ length: 30 }, (_, i) => `part${i}`).join(' + ')};`;
    hunks = [hunk(PATH, 3, [], [long])];
    const key = hunks[0]?.key ?? '';
    const app = await open();
    const drawn = await draw(app, <FileScreen />);
    await app.settle();

    // Wrapped: the line is several pieces, and together they are the line.
    const row = screen.getByTestId(`file.line.${key}:+0`);
    expect(row).toHaveTextContent(`3+${long}`);
    expect(screen.queryByTestId('file.sideways')).toBeNull();
    expect(screen.getByLabelText('Scroll long lines sideways')).toBeTruthy();

    await fireEvent.press(screen.getByTestId('file.wrap'));
    expect(screen.getByTestId('file.sideways')).toBeTruthy();
    expect(screen.getByLabelText('Wrap long lines')).toBeTruthy();
    expect(app.platform.capabilities.keyValue.scope<ReviewStore>('review').get('wrap')).toBe(false);

    await drawn.unmount();
    await draw(app, <FileScreen />);
    await app.settle();
    expect(screen.getByTestId('file.sideways')).toBeTruthy();
  });

  test('the comparison the route names is the one the file is compared with', async () => {
    const app = await open({ comparison: 'task_start:' });
    await draw(app, <FileScreen />);
    await app.settle();
    expect(app.connection.calls('workspace.hunks')).toEqual([{ workspace_id: WORKSPACE, path: PATH, base: TASK_START, run_id: RUN }]);
    expect(screen.getByTestId('file.about')).toHaveTextContent(/Since task start/);
  });

  test('the file is asked for again while the agent edits', async () => {
    const app = await open({}, { state: stateWith({ status: 'running' }) });
    await draw(app, <FileScreen />);
    await app.settle();
    hunks = [ONE, TWO, ADDED];
    await app.events(makeEvent('file_activity', { paths: [PATH] }, { run_id: RUN, source: 'claude' }));
    await afterNews(app);
    expect(app.connection.calls('workspace.hunks')).toHaveLength(2);
    expect(screen.getAllByTestId(`file.hunk.${ADDED.key}`).length).toBeGreaterThan(0);
  });
});
