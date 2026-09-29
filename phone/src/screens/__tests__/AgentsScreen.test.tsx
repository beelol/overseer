import { act, fireEvent, render, screen, waitFor } from '@testing-library/react-native';
import { State } from 'react-native-gesture-handler';
import { fireGestureHandler, getByGestureTestId } from 'react-native-gesture-handler/jest-utils';
import { SafeAreaProvider } from 'react-native-safe-area-context';

import { agents, store, text } from '@/model';
import { PlatformProvider } from '@/platform';
import { createFakePlatform } from '@/platform/fake';
import type { State as DaemonState } from '@/protocol';
import { routes } from '@/routes';
import { AgentsScreen } from '@/screens/AgentsScreen';
import { listed } from '@/screens/agents/useAgentsList';
import { Session, SessionProvider, type SessionCache } from '@/session';
import { createTestApp, EMPTY_STATE, FAKE_GATEWAY, FakeConnection, makeEvent, type TestApp, type TestAppOptions } from '@/testing';
import { router } from '@/testing/router';

jest.mock('expo-router', () => require('@/testing/router').mockRouter());

/** A recorded session of nine agents: two going, three failed, one archived, one with children. */
const recorded = require('../../../model/test/fixtures/nine-agents.json') as { final: DaemonState; marks: Record<string, string> };
const NINE = recorded.final;
const RUN = recorded.marks as { showcase: string; nested: string; auth: string; ratelimit: string; archived: string; generic: string; failed: string; waiting: string; running: string };
const childOf = (runId: string): string => NINE.runs.find((run) => run.parent_run_id === runId)?.id ?? '';
const CHILD = childOf(RUN.nested);
const GRANDCHILD = childOf(CHILD);

/** A moment after the recording ended. */
const NOW = Math.max(...NINE.runs.map((run) => run.ended_ms ?? run.created_ms)) + 5_000;

const taskOf = (runId: string): string => NINE.runs.find((run) => run.id === runId)?.task_id ?? '';
/** What VoiceOver and TalkBack do not reach is found by a test that asks for it. */
const HIDDEN = { includeHiddenElements: true } as const;

const row = (runId: string): string => `agents.row.${runId}`;
const needs = (runId: string): string => `agents.needs.${runId}`;

async function open(options: TestAppOptions = {}, prepare: (app: TestApp) => void = () => undefined): Promise<TestApp> {
  const app = await createTestApp({ state: NINE, ...options });
  app.connection.answers['workspace.changes'] = () => ({ files: 0, added: 0, removed: 0, names: [] });
  app.connection.answers['search'] = () => ({ task_ids: [], ms: 1 });
  app.connection.answers['task.archive'] = (params: { task_id: string }) => ({ task_id: params.task_id, archived_ms: NOW });
  app.connection.answers['run.interrupt'] = () => ({});
  app.connection.answers['runs.stop_all'] = () => ({ interrupted: [RUN.waiting, RUN.running], failed: [] });
  prepare(app);
  await app.render(<AgentsScreen />);
  await loaded();
  return app;
}

/** The list says it has loaded a frame after it was drawn. */
async function loaded(): Promise<void> {
  await act(async () => void (await new Promise((resolve) => setTimeout(resolve, 40))));
}

/** The test ids of the list's rows, in the order they stand. */
function order(): string[] {
  const ids: string[] = [];
  for (const node of screen.getAllByTestId(/^agents\.(row|needs|repo|section)\.[^.]+$/)) ids.push(String(node.props.testID));
  return ids;
}

beforeEach(() => {
  router.reset();
  jest.spyOn(Date, 'now').mockReturnValue(NOW);
});

afterEach(() => {
  jest.restoreAllMocks();
});

describe('the agents list', () => {
  test('shows every agent of the state, children under their parents, and not the archived one', async () => {
    await open();
    expect(screen.getByTestId('agents.title')).toHaveTextContent('Agents');
    expect(screen.queryByTestId('agents.back')).toBeNull();
    for (const id of [RUN.showcase, RUN.nested, RUN.auth, RUN.ratelimit, RUN.generic, RUN.failed, RUN.waiting, RUN.running, CHILD, GRANDCHILD]) {
      expect(screen.getByTestId(row(id))).toBeTruthy();
    }
    expect(screen.queryByTestId(row(RUN.archived))).toBeNull();

    // The title on one line; under it the repository, the status in words and the time.
    expect(screen.getByTestId(`${row(RUN.running)}.title`)).toHaveTextContent('A slow migration');
    expect(screen.getByTestId(`${row(RUN.running)}.title`).props.numberOfLines).toBe(1);
    expect(screen.getByTestId(`${row(RUN.running)}.status`)).toHaveTextContent('shop · working');
    expect(screen.getByTestId(`${row(RUN.showcase)}.status`)).toHaveTextContent('shop · done · now');
    expect(screen.getByTestId(`${row(RUN.auth)}.status`)).toHaveTextContent('billing-service · failed · now');

    // Children are indented under their parent, each level further, with a line.
    expect(screen.queryByTestId(`${row(RUN.nested)}.under`)).toBeNull();
    const child = screen.getByTestId(`${row(CHILD)}.under`);
    const grandchild = screen.getByTestId(`${row(GRANDCHILD)}.under`);
    expect(widthOf(grandchild)).toBeGreaterThan(widthOf(child));
    const all = order();
    expect(all.indexOf(row(CHILD))).toBe(all.indexOf(row(RUN.nested)) + 1);
    expect(all.indexOf(row(GRANDCHILD))).toBe(all.indexOf(row(CHILD)) + 1);
  });

  test('is the model’s rows, in the model’s order', async () => {
    await open();
    const rows = agents.agentRows(store.load(NINE), { now: NOW, filter: 'all', query: '', matches: [], collapsed: new Set(), seen: {}, changed: {}, pinned: [] });
    const expected = rows.map((r) => (r.kind === 'section' ? 'agents.section.needs' : r.kind === 'repo' ? `agents.repo.${r.label}` : r.kind === 'needs' ? needs(String(r.runId)) : row(String(r.runId))));
    expect(order()).toEqual(expected);
  });

  // AC-246: Needs you is what waits for the owner's answer, as VS Code and the TUI count it; the
  // failed agents are in their repositories with the failure's mark, not in Needs you.
  test('puts the agent that waits for the owner first; the failed ones stay in their repositories', async () => {
    await open();
    const all = order();
    expect(all.slice(0, 2)).toEqual(['agents.section.needs', needs(RUN.waiting)]);
    for (const id of [RUN.auth, RUN.ratelimit, RUN.failed]) { expect(screen.queryByTestId(needs(id))).toBeNull(); expect(screen.getByTestId(row(id))).toBeTruthy(); }
    expect(screen.getByTestId(`${needs(RUN.waiting)}.status`)).toHaveTextContent('billing-service · Approve · Wants to use Write');
    // A row that needs the owner carries the mark, in the section and in its repository.
    expect(screen.getByTestId(`${needs(RUN.waiting)}.mark`)).toBeTruthy();
    expect(screen.getByTestId(`${row(RUN.waiting)}.mark`)).toBeTruthy();
    expect(screen.queryByTestId(`${row(RUN.running)}.mark`)).toBeNull();
  });

  test('filters by All, Active and Needs you, and says how many need the owner', async () => {
    await open();
    const counts = agents.counts(store.load(NINE), { now: NOW, seen: {}, changed: {} });
    expect(counts.needs).toBe(1);
    expect(screen.getByTestId('agents.filter.needs')).toHaveTextContent(`Needs you${counts.needs}`);
    expect(screen.getByTestId('agents.filter.needs').props.accessibilityLabel).toBe('Needs you, 1');
    expect(screen.getByTestId('agents.filter.all')).toHaveTextContent('All');
    expect(screen.getByTestId('agents.filter.all').props.accessibilityState).toMatchObject({ selected: true });

    await fireEvent.press(screen.getByTestId('agents.filter.active'));
    expect(screen.getByTestId('agents.filter.active').props.accessibilityState).toMatchObject({ selected: true });
    expect(screen.getByTestId(row(RUN.running))).toBeTruthy();
    expect(screen.getByTestId(row(RUN.waiting))).toBeTruthy();
    for (const id of [RUN.showcase, RUN.nested, RUN.auth, RUN.ratelimit, RUN.generic, RUN.failed]) expect(screen.queryByTestId(row(id))).toBeNull();
    // Of those that need the owner, only the one still going.
    expect(order().filter((id) => id.startsWith('agents.needs.'))).toEqual([needs(RUN.waiting)]);

    await fireEvent.press(screen.getByTestId('agents.filter.needs'));
    expect(order()).toEqual(['agents.section.needs', needs(RUN.waiting)]);

    await fireEvent.press(screen.getByTestId('agents.filter.all'));
    expect(screen.getByTestId(row(RUN.showcase))).toBeTruthy();
  });

  test('a filter with nothing in it says so', async () => {
    const quiet: DaemonState = { ...NINE, runs: NINE.runs.map((run) => ({ ...run, status: 'completed', attention: null, ended_ms: run.ended_ms ?? run.created_ms })) };
    const app = await open({ state: quiet }, (a) => a.platform.capabilities.keyValue.scope<{ seen: Record<string, number> }>('agents').set('seen', Object.fromEntries(quiet.runs.map((run) => [run.id, NOW]))));
    await fireEvent.press(screen.getByTestId('agents.filter.active'));
    expect(screen.getByTestId('agents.empty')).toHaveTextContent(text.PHONE_ONLY.noActive);
    await fireEvent.press(screen.getByTestId('agents.filter.needs'));
    expect(screen.getByTestId('agents.empty')).toHaveTextContent(text.PHONE_ONLY.nothingNeedsYou);
    expect(screen.queryByTestId('agents.empty.new')).toBeNull();
    expect(app.connection.calls('task.archive')).toEqual([]);
  });

  test('searches what the phone knows at once, and what the Mac finds a moment later', async () => {
    const app = await open();
    app.connection.answers['search'] = () => ({ task_ids: [taskOf(RUN.showcase)], ms: 2 });
    expect(screen.queryByTestId('agents.search.field')).toBeNull();
    await fireEvent.press(screen.getByTestId('agents.search'));
    await fireEvent.changeText(screen.getByTestId('agents.search.field'), 'migration');

    // At once, with nothing asked of the Mac yet.
    expect(app.connection.calls('search')).toEqual([]);
    expect(order().filter((id) => id.startsWith('agents.row.'))).toEqual([row(RUN.running), row(RUN.failed)]);
    expect(order().some((id) => id.startsWith('agents.needs.'))).toBe(false);
    expect(screen.getByTestId('agents.search.matches')).toHaveTextContent('2 matches for “migration”');

    // The Mac knows the words of the conversations: what it finds is added.
    await waitFor(() => expect(app.connection.calls('search')).toEqual([{ query: 'migration', limit: 200 }]));
    await waitFor(() => expect(screen.getByTestId(row(RUN.showcase))).toBeTruthy());
    expect(screen.getByTestId('agents.search.matches')).toHaveTextContent('3 matches for “migration”');

    await fireEvent.changeText(screen.getByTestId('agents.search.field'), 'no such agent');
    expect(screen.getByTestId('agents.empty')).toHaveTextContent(text.PHONE_ONLY.noMatches);

    await fireEvent.press(screen.getByTestId('agents.search.clear'));
    await loaded();
    expect(screen.queryByTestId('agents.search.field')).toBeNull();
    expect(screen.getByTestId(row(RUN.generic))).toBeTruthy();
  });

  test('typing quickly asks the Mac once, for what was typed last', async () => {
    const app = await open();
    await fireEvent.press(screen.getByTestId('agents.search'));
    for (const typed of ['t', 'ta', 'tax']) await fireEvent.changeText(screen.getByTestId('agents.search.field'), typed);
    await waitFor(() => expect(app.connection.calls('search')).toHaveLength(1));
    expect(app.connection.calls('search')).toEqual([{ query: 'tax', limit: 200 }]);
    expect(order().filter((id) => id.startsWith('agents.row.'))).toEqual([row(RUN.ratelimit)]);
  });

  test('a status that changes on the Mac changes the row while it is looked at', async () => {
    const app = await open();
    const running = screen.getByTestId(`${row(RUN.running)}.status`);
    expect(running).toHaveTextContent('shop · working');
    expect(screen.getByTestId('agents.filter.needs')).toHaveTextContent('Needs you1');

    await app.events(makeEvent('status', { status: 'completed', reason: 'turn completed' }, { run_id: RUN.running, task_id: taskOf(RUN.running), ts: NOW }));
    expect(screen.getByTestId(`${row(RUN.running)}.status`)).toHaveTextContent('shop · done · now');

    // The one that waited was answered on the Mac: it no longer needs the owner.
    await app.events(makeEvent('status', { status: 'running' }, { run_id: RUN.waiting, task_id: taskOf(RUN.waiting), ts: NOW }));
    expect(screen.queryByTestId(needs(RUN.waiting))).toBeNull();
    expect(screen.queryByTestId(`${row(RUN.waiting)}.mark`)).toBeNull();
    expect(screen.getByTestId(`${row(RUN.waiting)}.status`)).toHaveTextContent('billing-service · working');
    expect(screen.getByTestId('agents.filter.needs').props.accessibilityLabel).toBe('Needs you, 0');
  });

  test('a changed status cross-fades: the old words leave while the new ones arrive', async () => {
    const app = await open();
    await app.events(makeEvent('status', { status: 'completed', reason: 'turn completed' }, { run_id: RUN.running, task_id: taskOf(RUN.running), ts: NOW }));
    expect(screen.getByTestId(`${row(RUN.running)}.status`)).toHaveTextContent('shop · done · now');
    expect(screen.getAllByText('shop · working', HIDDEN)).toHaveLength(1);
    await waitFor(() => expect(screen.queryByText('shop · working', HIDDEN)).toBeNull());
    expect(screen.getByTestId(`${row(RUN.running)}.status`)).toHaveTextContent('shop · done · now');
  });

  test('what an agent writes does not draw the list again', async () => {
    const app = await open();
    const before = app.session.getSnapshot().state;
    const shown = listed(app.session.getSnapshot());
    const rows = agents.agentRows(shown, { now: NOW, filter: 'all', query: '', matches: [], collapsed: new Set(), seen: {}, changed: {}, pinned: [] });
    await app.events(makeEvent('output', { role: 'assistant', text: 'One more line.' }, { run_id: RUN.running, task_id: taskOf(RUN.running), ts: NOW }));
    const after = app.session.getSnapshot();
    expect(after.state).not.toBe(before);
    expect(after.state.cursor).toBeGreaterThan(before.cursor);
    // The list reads the same state as before, so its rows are the same rows.
    expect(listed(after)).toBe(shown);
    expect(agents.agentRows(listed(after), { now: NOW, filter: 'all', query: '', matches: [], collapsed: new Set(), seen: {}, changed: {}, pinned: [] })).toBe(rows);
  });

  test('a new agent started on the Mac appears in the list', async () => {
    const app = await open();
    const from = NINE.runs.find((run) => run.id === RUN.running);
    const task = NINE.tasks.find((t) => t.id === from?.task_id);
    const workspace = NINE.workspaces.find((w) => w.id === from?.workspace_id);
    if (!from || !task || !workspace) throw new Error('the recording has no running agent');
    await app.events(
      makeEvent(
        'task_created',
        { task: { ...task, id: 't-new', title: 'Add a changelog', created_ms: NOW }, run: { ...from, id: 'r-new', task_id: 't-new', title: 'Add a changelog', status: 'queued', created_ms: NOW }, workspace: { ...workspace, id: 'w-new', owner_run_id: 'r-new' } },
        { run_id: 'r-new', task_id: 't-new', ts: NOW },
      ),
    );
    expect(screen.getByTestId(`${row('r-new')}.title`)).toHaveTextContent('Add a changelog');
    expect(screen.getByTestId(`${row('r-new')}.status`)).toHaveTextContent('shop · queued');
  });

  test('tapping an agent opens its conversation and remembers when', async () => {
    const app = await open();
    await fireEvent.press(screen.getByTestId(row(RUN.failed)));
    expect(router.pushed).toEqual([routes.agent(RUN.failed)]);
    expect(app.platform.capabilities.keyValue.scope<{ seen: Record<string, number> }>('agents').get('seen')).toEqual({ [RUN.failed]: NOW });
    // Looking at an agent never changes what waits for the owner's answer.
    expect(screen.getByTestId('agents.filter.needs')).toHaveTextContent('Needs you1');
  });

  test('what was opened before is remembered from the last launch, and Needs you is still what waits', async () => {
    await open({}, (app) => app.platform.capabilities.keyValue.scope<{ seen: Record<string, number> }>('agents').set('seen', { [RUN.auth]: NOW, [RUN.ratelimit]: NOW }));
    expect(order().filter((id) => id.startsWith('agents.needs.'))).toEqual([needs(RUN.waiting)]);
  });

  test('a finished agent with changes is not in Needs you (it is to review, AC-254)', async () => {
    const app = await createTestApp({ state: NINE });
    const asked: string[] = [];
    const showcase = NINE.runs.find((run) => run.id === RUN.showcase);
    app.connection.answers['workspace.changes'] = (params: { workspace_id: string }) => {
      asked.push(params.workspace_id);
      return { files: params.workspace_id === showcase?.workspace_id ? 3 : 0, added: 10, removed: 2, names: [] };
    };
    await app.render(<AgentsScreen />);
    await loaded();
    await waitFor(() => expect(asked.length).toBeGreaterThan(0));
    expect(screen.queryByTestId(needs(RUN.showcase))).toBeNull();
    expect(screen.getByTestId('agents.filter.needs')).toHaveTextContent('Needs you1');
    // Once for each finished agent that is not archived, and never for a child.
    const finished = NINE.runs.filter((run) => !run.parent_run_id && run.status === 'completed' && run.id !== RUN.archived);
    expect([...asked].sort()).toEqual(finished.map((run) => run.workspace_id).sort());
  });

  test('a repository folds and unfolds, and stays folded', async () => {
    const app = await open();
    expect(screen.getByLabelText('shop, 5 agents, 1 active')).toBeTruthy();
    await fireEvent.press(screen.getByTestId('agents.repo.shop'));
    for (const id of [RUN.showcase, RUN.nested, RUN.generic, RUN.failed, RUN.running, CHILD]) expect(screen.queryByTestId(row(id))).toBeNull();
    expect(screen.getByTestId(row(RUN.auth))).toBeTruthy();
    expect(screen.getByTestId('agents.repo.shop').props.accessibilityState).toMatchObject({ expanded: false });
    expect(app.platform.capabilities.keyValue.scope<{ collapsed: string[] }>('agents').get('collapsed')).toEqual(['repo:/fixture/shop']);

    await fireEvent.press(screen.getByTestId('agents.repo.shop'));
    expect(screen.getByTestId(row(RUN.showcase))).toBeTruthy();
    expect(app.platform.capabilities.keyValue.scope<{ collapsed: string[] }>('agents').get('collapsed')).toEqual([]);

    await fireEvent.press(screen.getByTestId('agents.section.needs'));
    expect(order().some((id) => id.startsWith('agents.needs.'))).toBe(false);
  });

  test('pulling down loads the state again', async () => {
    const app = await open();
    expect(app.connection.calls('state')).toHaveLength(1);
    const list = screen.getByTestId('agents.list');
    await act(async () => (list.props.refreshControl as { props: { onRefresh: () => void } }).props.onRefresh());
    await app.settle();
    expect(app.connection.calls('state')).toHaveLength(2);
    expect(app.connection.woken).toBe(1);
  });
});

describe('what the list changes on the Mac', () => {
  test('a long press offers Pin, Archive and Stop; Pin is kept on the phone', async () => {
    const app = await open();
    await fireEvent(screen.getByTestId(row(RUN.running)), 'longPress');
    expect(screen.getByTestId('agents.actions.pin')).toHaveTextContent(/Pin$/);
    expect(screen.getByTestId('agents.actions.archive')).toHaveTextContent(/Archive$/);
    expect(screen.getByTestId('agents.actions.stop')).toHaveTextContent(/Stop$/);

    await fireEvent.press(screen.getByTestId('agents.actions.pin'));
    expect(screen.queryByTestId('agents.actions.pin')).toBeNull();
    expect(app.platform.capabilities.keyValue.scope<{ pinned: string[] }>('agents').get('pinned')).toEqual([RUN.running]);
    expect(app.connection.asked.filter((a) => !['state', 'workspace.changes'].includes(a.method))).toEqual([]);

    await fireEvent(screen.getByTestId(row(RUN.running)), 'longPress');
    expect(screen.getByTestId('agents.actions.pin')).toHaveTextContent(/Unpin$/);
    await fireEvent.press(screen.getByTestId('agents.actions.pin'));
    expect(app.platform.capabilities.keyValue.scope<{ pinned: string[] }>('agents').get('pinned')).toEqual([]);
  });

  test('an agent that is not going has no Stop, and a child has no actions', async () => {
    await open();
    await fireEvent(screen.getByTestId(row(RUN.showcase)), 'longPress');
    expect(screen.getByTestId('agents.actions.archive')).toBeTruthy();
    expect(screen.queryByTestId('agents.actions.stop')).toBeNull();
    await fireEvent.press(screen.getByTestId('agents.actions.close', HIDDEN));

    await fireEvent(screen.getByTestId(row(CHILD)), 'longPress');
    expect(screen.queryByTestId('agents.actions.pin')).toBeNull();
    expect(screen.queryByTestId(`${row(CHILD)}.archive`)).toBeNull();
  });

  test('Archive from the menu asks the Mac once, and the row is gone at once', async () => {
    const app = await open();
    // The Mac takes its time: what the owner did shows before it answers.
    let answer: (value: unknown) => void = () => undefined;
    app.connection.answers['task.archive'] = () => new Promise((resolve) => (answer = resolve));
    await fireEvent(screen.getByTestId(row(RUN.showcase)), 'longPress');
    await fireEvent.press(screen.getByTestId('agents.actions.archive'));
    expect(app.connection.calls('task.archive')).toEqual([{ task_id: taskOf(RUN.showcase), archived: true }]);
    expect(screen.queryByTestId(row(RUN.showcase))).toBeNull();
    expect(screen.getByTestId(row(RUN.generic))).toBeTruthy();

    answer({ task_id: taskOf(RUN.showcase), archived_ms: NOW });
    await app.events(makeEvent('task_archived', { archived: true }, { task_id: taskOf(RUN.showcase), ts: NOW }));
    expect(screen.queryByTestId(row(RUN.showcase))).toBeNull();
    expect(app.connection.calls('task.archive')).toHaveLength(1);
  });

  test('an archive the Mac refuses puts the row back and says so', async () => {
    const app = await open();
    app.connection.answers['task.archive'] = () => {
      throw new Error('the task has an active run');
    };
    await fireEvent(screen.getByTestId(row(RUN.running)), 'longPress');
    await fireEvent.press(screen.getByTestId('agents.actions.archive'));
    await app.settle();
    expect(screen.getByTestId(row(RUN.running))).toBeTruthy();
    expect(screen.getByTestId('agents.error')).toHaveTextContent('Archive: not sent. the task has an active run');
  });

  test('a swipe opens Archive; pressing it archives', async () => {
    const app = await open();
    const archive = `${row(RUN.generic)}.archive`;
    expect(screen.getByTestId(`${archive}.shown`, HIDDEN).props.pointerEvents).toBe('none');

    await act(async () => {
      fireGestureHandler(getByGestureTestId(`${archive}.swipe`), [{ state: State.BEGAN, translationX: 0 }, { state: State.ACTIVE, translationX: -30 }, { translationX: -70 }, { state: State.END, translationX: -70 }]);
    });
    expect(screen.getByTestId(`${archive}.shown`, HIDDEN).props.pointerEvents).toBe('auto');
    expect(app.connection.calls('task.archive')).toEqual([]);

    await fireEvent.press(screen.getByTestId(archive));
    expect(app.connection.calls('task.archive')).toEqual([{ task_id: taskOf(RUN.generic), archived: true }]);
    expect(screen.queryByTestId(row(RUN.generic))).toBeNull();
  });

  test('a short swipe does nothing, a swipe back closes, a far swipe archives', async () => {
    const app = await open();
    const archive = `${row(RUN.generic)}.archive`;
    const swipe = async (to: number): Promise<void> => {
      await act(async () => {
        fireGestureHandler(getByGestureTestId(`${archive}.swipe`), [{ state: State.BEGAN, translationX: 0 }, { state: State.ACTIVE, translationX: to / 2 }, { translationX: to }, { state: State.END, translationX: to }]);
      });
    };
    await swipe(-20);
    expect(screen.getByTestId(`${archive}.shown`, HIDDEN).props.pointerEvents).toBe('none');
    await swipe(-70);
    expect(screen.getByTestId(`${archive}.shown`, HIDDEN).props.pointerEvents).toBe('auto');
    await swipe(80);
    expect(screen.getByTestId(`${archive}.shown`, HIDDEN).props.pointerEvents).toBe('none');
    expect(app.connection.calls('task.archive')).toEqual([]);

    await swipe(-260);
    expect(app.connection.calls('task.archive')).toEqual([{ task_id: taskOf(RUN.generic), archived: true }]);
    expect(screen.queryByTestId(row(RUN.generic))).toBeNull();
  });

  test.each([
    ['the row follows the finger and stays open beside Archive', false, -88],
    ['with Reduce Motion the row stays where it is and Archive fades in over its end', true, 0],
  ])('%s', async (_name, reduceMotion, travelled) => {
    const app = await open({}, (a) => a.platform.fakes.reduceMotion.set(reduceMotion));
    const archive = `${row(RUN.generic)}.archive`;
    await act(async () => {
      fireGestureHandler(getByGestureTestId(`${archive}.swipe`), [{ state: State.BEGAN, translationX: 0 }, { state: State.ACTIVE, translationX: -30 }, { translationX: -70 }, { state: State.END, translationX: -70 }]);
    });
    await waitFor(() => expect(screen.getByTestId(`${archive}.travel`)).toHaveAnimatedStyle({ transform: [{ translateX: travelled }] }));
    await waitFor(() => expect(screen.getByTestId(`${archive}.shown`)).toHaveAnimatedStyle({ opacity: 1 }));
    await fireEvent.press(screen.getByTestId(archive));
    expect(app.connection.calls('task.archive')).toEqual([{ task_id: taskOf(RUN.generic), archived: true }]);
  });

  test('VoiceOver and TalkBack archive with the row’s own action', async () => {
    const app = await open();
    const target = screen.getByTestId(row(RUN.showcase));
    expect(target.props.accessibilityActions).toEqual([
      { name: 'longpress', label: 'Menu' },
      { name: 'archive', label: 'Archive' },
    ]);
    await fireEvent(target, 'accessibilityAction', { nativeEvent: { actionName: 'archive' } });
    expect(app.connection.calls('task.archive')).toEqual([{ task_id: taskOf(RUN.showcase), archived: true }]);
  });

  test('Stop asks the Mac to stop that agent, and the row says it is stopping', async () => {
    const app = await open();
    await fireEvent(screen.getByTestId(row(RUN.running)), 'longPress');
    await fireEvent.press(screen.getByTestId('agents.actions.stop'));
    expect(app.connection.calls('run.interrupt')).toEqual([{ run_id: RUN.running }]);
    expect(screen.getByTestId(`${row(RUN.running)}.status`)).toHaveTextContent('shop · stopping');
    // Asked once: the menu no longer offers it.
    await fireEvent(screen.getByTestId(row(RUN.running)), 'longPress');
    expect(screen.queryByTestId('agents.actions.stop')).toBeNull();
    await fireEvent.press(screen.getByTestId('agents.actions.close', HIDDEN));

    await app.events(makeEvent('status', { status: 'interrupted', reason: 'interrupted by user' }, { run_id: RUN.running, task_id: taskOf(RUN.running), ts: NOW }));
    expect(screen.getByTestId(`${row(RUN.running)}.status`)).toHaveTextContent('shop · stopped · now');
  });

  test('Stop all agents asks once, naming how many will stop', async () => {
    const app = await open();
    await fireEvent.press(screen.getByTestId('agents.menu'));
    expect(screen.getByTestId('agents.menu.new')).toHaveTextContent(/New agent$/);
    expect(screen.getByTestId('agents.menu.accounts')).toHaveTextContent(/Accounts$/);
    expect(screen.getByTestId('agents.menu.settings')).toHaveTextContent(/Settings$/);
    await fireEvent.press(screen.getByTestId('agents.menu.stop'));
    expect(app.connection.calls('runs.stop_all')).toEqual([]);

    const confirm = await screen.findByTestId('agents.stop.confirm');
    expect(screen.getByText('Stop 2 agents?')).toBeTruthy();
    expect(confirm).toHaveTextContent('Stop all agents');
    await fireEvent.press(screen.getByTestId('agents.stop.cancel'));
    expect(app.connection.calls('runs.stop_all')).toEqual([]);

    await fireEvent.press(screen.getByTestId('agents.menu'));
    await fireEvent.press(screen.getByTestId('agents.menu.stop'));
    await fireEvent.press(await screen.findByTestId('agents.stop.confirm'));
    expect(app.connection.calls('runs.stop_all')).toEqual([{}]);
    expect(screen.getByTestId(`${row(RUN.running)}.status`)).toHaveTextContent('shop · stopping');
    expect(screen.getByTestId(`${row(RUN.waiting)}.status`)).toHaveTextContent('billing-service · stopping');
  });

  test('with the unlock before changes on, Stop all needs the unlock', async () => {
    const app = await open();
    app.platform.fakes.keyValue.items.set('settings.unlockBeforeChanges', 'true');
    app.platform.fakes.deviceUnlock.answerWith({ ok: false, cause: 'failed' });
    await fireEvent.press(screen.getByTestId('agents.menu'));
    await fireEvent.press(screen.getByTestId('agents.menu.stop'));
    await fireEvent.press(await screen.findByTestId('agents.stop.confirm'));
    await app.settle();
    expect(app.platform.fakes.deviceUnlock.requests()).toEqual([{ reason: 'Stop all agents' }]);
    expect(app.connection.calls('runs.stop_all')).toEqual([]);
    expect(screen.getByTestId('agents.error')).toHaveTextContent('The unlock did not work. Nothing was changed.');

    app.platform.fakes.deviceUnlock.answerWith({ ok: true });
    await fireEvent.press(screen.getByTestId('agents.menu'));
    await fireEvent.press(screen.getByTestId('agents.menu.stop'));
    await fireEvent.press(await screen.findByTestId('agents.stop.confirm'));
    await app.settle();
    expect(app.connection.calls('runs.stop_all')).toEqual([{}]);
  });

  test('the menu leads to a new agent, the accounts and the settings', async () => {
    await open();
    for (const [item, route] of [['new', routes.newAgent], ['accounts', routes.accounts], ['settings', routes.settings]] as const) {
      await fireEvent.press(screen.getByTestId('agents.menu'));
      await fireEvent.press(screen.getByTestId(`agents.menu.${item}`));
      expect(router.pushed[router.pushed.length - 1]).toBe(route);
    }
    await fireEvent.press(screen.getByTestId('agents.new'));
    expect(router.pushed).toEqual([routes.newAgent, routes.accounts, routes.settings, routes.newAgent]);
  });

  test('with no agent going there is nothing to stop', async () => {
    const quiet: DaemonState = { ...NINE, runs: NINE.runs.map((run) => ({ ...run, status: 'completed', attention: null, ended_ms: run.ended_ms ?? run.created_ms })) };
    await open({ state: quiet });
    await fireEvent.press(screen.getByTestId('agents.menu'));
    expect(screen.getByTestId('agents.menu.new')).toBeTruthy();
    expect(screen.queryByTestId('agents.menu.stop')).toBeNull();
  });
});

describe('a phone that may only watch', () => {
  test('sees every agent and nothing that changes one', async () => {
    const app = await open({ scope: 'watch' });
    expect(screen.getByTestId(row(RUN.running))).toBeTruthy();
    expect(screen.getByTestId('agents.filter.needs')).toHaveTextContent('Needs you1');
    expect(screen.queryByTestId('agents.new')).toBeNull();
    expect(screen.getByTestId('watch.line')).toHaveTextContent('This phone may watch. Change it on the Mac.');
    expect(screen.queryByTestId(`${row(RUN.running)}.archive`)).toBeNull();
    expect(screen.getByTestId(row(RUN.running)).props.accessibilityActions).toEqual([{ name: 'longpress', label: 'Menu' }]);

    await fireEvent.press(screen.getByTestId('agents.menu'));
    expect(screen.queryByTestId('agents.menu.new')).toBeNull();
    expect(screen.queryByTestId('agents.menu.stop')).toBeNull();
    expect(screen.getByTestId('agents.menu.accounts')).toBeTruthy();
    expect(screen.getByTestId('agents.menu.settings')).toBeTruthy();
    await fireEvent.press(screen.getByTestId('agents.menu.close', HIDDEN));

    // Pin is the phone's own: it stays.
    await fireEvent(screen.getByTestId(row(RUN.running)), 'longPress');
    expect(screen.getByTestId('agents.actions.pin')).toBeTruthy();
    expect(screen.queryByTestId('agents.actions.archive')).toBeNull();
    expect(screen.queryByTestId('agents.actions.stop')).toBeNull();

    await fireEvent.press(screen.getByTestId(row(RUN.running)));
    expect(router.pushed).toEqual([routes.agent(RUN.running)]);
    expect(app.connection.asked.filter((a) => !['state', 'workspace.changes'].includes(a.method))).toEqual([]);
  });

  test('with no agents it says so and offers nothing', async () => {
    await open({ scope: 'watch', state: EMPTY_STATE });
    expect(screen.getByTestId('agents.empty')).toHaveTextContent(text.TEXT.agents.empty);
    expect(screen.queryByTestId('agents.empty.new')).toBeNull();
  });
});

describe('nothing yet, and not connected', () => {
  test('with no agents it says so and offers New agent', async () => {
    await open({ state: EMPTY_STATE });
    expect(agents.emptyText(store.load(EMPTY_STATE), { now: NOW })).toBe('No agent tasks yet.');
    expect(screen.getByTestId('agents.empty')).toHaveTextContent('No agent tasks yet.New agent');
    await fireEvent.press(screen.getByTestId('agents.empty.new'));
    expect(router.pushed).toEqual([routes.newAgent]);
    expect(screen.getByTestId('agents.new')).toBeTruthy();
    expect(screen.queryByTestId('agents.age')).toBeNull();
  });

  test('before anything is known it does not say there are no agents', async () => {
    await open({ connection: 'connecting' });
    expect(screen.queryByTestId('agents.empty')).toBeNull();
    expect(screen.getByTestId('connection.reconnecting')).toBeTruthy();
    expect(screen.queryByTestId('agents.age')).toBeNull();
  });

  test('while connected nothing is marked with an age', async () => {
    await open();
    expect(screen.queryByTestId('agents.age')).toBeNull();
  });

  test('when the Mac cannot be reached, what is stored stays, marked with its age', async () => {
    const app = await createTestApp({ state: NINE });
    app.setTime(NOW - 2 * 60_000);
    await app.session.reload();
    app.connection.answers['workspace.changes'] = () => ({ files: 0, added: 0, removed: 0, names: [] });
    await app.render(<AgentsScreen />);
    await loaded();
    expect(screen.queryByTestId('agents.age')).toBeNull();

    await act(async () => app.connection.go('unreachable'));
    expect(screen.getByTestId('agents.age')).toHaveTextContent('as of 2m ago');
    expect(screen.getByTestId('connection.unreachable')).toBeTruthy();
    expect(screen.getByTestId(row(RUN.running))).toBeTruthy();
    expect(screen.getByTestId('agents.new')).toBeTruthy();

    await act(async () => app.connection.go('online'));
    await app.settle();
    expect(screen.queryByTestId('agents.age')).toBeNull();
  });

  test('it opens on what the phone stored, with its age, before the Mac answered', async () => {
    const platform = createFakePlatform();
    const cache = platform.capabilities.keyValue.scope<SessionCache>('cache');
    cache.set('state', JSON.stringify(NINE));
    cache.set('stateAt', NOW - 3 * 3_600_000);
    const connection = new FakeConnection();
    connection.gateway = FAKE_GATEWAY;
    const session = new Session({ connection, cache, now: () => NOW, nextFrame: () => () => undefined });
    await session.start();
    connection.go('connecting');
    await render(
      <SafeAreaProvider initialMetrics={{ frame: { x: 0, y: 0, width: 402, height: 874 }, insets: { top: 0, left: 0, right: 0, bottom: 0 } }}>
        <PlatformProvider capabilities={platform.capabilities}>
          <SessionProvider session={session}>
            <AgentsScreen />
          </SessionProvider>
        </PlatformProvider>
      </SafeAreaProvider>,
    );
    await loaded();
    expect(session.getSnapshot().fromCache).toBe(true);
    expect(screen.getByTestId('agents.age')).toHaveTextContent('as of 3h ago');
    expect(screen.getByTestId(row(RUN.waiting))).toBeTruthy();
    expect(screen.getByTestId('agents.filter.needs')).toHaveTextContent('Needs you1');
    expect(connection.asked).toEqual([]);
  });

  test('what is archived while the Mac is away waits in the outbox and the row stays gone', async () => {
    const app = await open();
    await act(async () => app.connection.go('unreachable'));
    await act(async () => app.connection.setOutbox([{ requestId: 'request-1', method: 'task.archive', params: { task_id: taskOf(RUN.showcase), archived: true }, state: 'queued', createdAt: NOW, attempts: 0, firstSentAt: null }]));
    expect(screen.queryByTestId(row(RUN.showcase))).toBeNull();
    await act(async () => app.connection.setOutbox([{ requestId: 'request-2', method: 'run.interrupt', params: { run_id: RUN.running }, state: 'sending', createdAt: NOW, attempts: 1, firstSentAt: NOW }]));
    expect(screen.getByTestId(row(RUN.showcase))).toBeTruthy();
    expect(screen.getByTestId(`${row(RUN.running)}.status`)).toHaveTextContent('shop · stopping');
  });
});

function widthOf(node: { props: { style?: unknown } }): number {
  const styles = [node.props.style].flat(3) as ({ width?: number } | null | undefined)[];
  return styles.reduce((width, style) => (typeof style?.width === 'number' ? style.width : width), 0);
}
