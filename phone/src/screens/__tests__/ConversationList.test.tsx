import { fireEvent, screen } from '@testing-library/react-native';

import type { DaemonEvent } from '@/model';
import { FAKE_IPHONE } from '@/platform/fake';
import { ConversationScreen } from '@/screens/ConversationScreen';
import { rowId } from '@/screens/conversation/ids';
import { answerHistory, frames, idsOf, keepOutbox, patience, measured, recording, rootOf, wordsOf } from '@/screens/conversation/testing';
import { createTestApp, makeEvent, type TestApp } from '@/testing';
import { router } from '@/testing/router';

jest.mock('expo-router', () => require('@/testing/router').mockRouter());
jest.mock('@shopify/flash-list/dist/recyclerview/utils/measureLayout', () => require('@/screens/conversation/testing').measuring());

/** Every reply that was drawn, by its row: a row that is drawn again is here again. */
const mockDrawn: string[] = [];
jest.mock('@/screens/conversation/Markdown', () => {
  const real = jest.requireActual<typeof import('@/screens/conversation/Markdown')>('@/screens/conversation/Markdown');
  const { createElement } = jest.requireActual<typeof import('react')>('react');
  return {
    ...real,
    Markdown: (props: { id: string }) => {
      mockDrawn.push(props.id);
      return createElement(real.Markdown, props as never);
    },
  };
});

patience();

beforeEach(() => {
  router.reset();
  measured.row = 40;
  mockDrawn.length = 0;
});

const ROWS = 5000;
/** Enough rows to scroll in, for what does not need five thousand. */
const SOME = 300;

/** A conversation of `rows` rows: one turn in which the agent says something `rows - 1` times. */
function long(run: string, task: string, rows: number): DaemonEvent[] {
  const events: DaemonEvent[] = [];
  const at = (seq: number, kind: string, payload: unknown): DaemonEvent => ({ seq, ts: seq, kind, source: 'harness', confidence: 'exact', payload, run_id: run, task_id: task }) as DaemonEvent;
  events.push(at(3, 'turn_started', { turn: { id: 'u-1', run_id: run, n: 1, prompt: 'Count for me', started_ms: 1, ended_ms: null, status: 'running' } }));
  for (let i = 1; i < rows; i++) events.push(at(3 + i, 'output', { role: 'assistant', text: `Line **${i}** of the count` }));
  return events;
}

async function openLong(rows = ROWS): Promise<{ app: TestApp; run: string; events: DaemonEvent[] }> {
  const r = recording('echo-follow-up');
  const run = rootOf(r);
  const state = r.checkpoints.find((c) => c.state.runs[0]?.status === 'running')?.state ?? r.final;
  const events = long(run, String(r.final.tasks[0]?.id), rows);
  const app = await createTestApp({ state: { ...state, cursor: 3 + rows }, launch: FAKE_IPHONE });
  answerHistory(app.connection, events);
  app.connection.answers['workspace.changes'] = () => ({ files: 0, added: 0, removed: 0, names: [] });
  keepOutbox(app.connection);
  router.params = { run };
  await app.render(<ConversationScreen />);
  await frames(6);
  return { app, run, events };
}

const drawnRows = (): string[] => idsOf(screen.toJSON()).filter((id) => /^agent\.row\.[^.]+$/.test(id));
const scrollTo = (y: number, height = SOME * measured.row) =>
  fireEvent.scroll(screen.getByTestId('agent.list'), { nativeEvent: { contentOffset: { x: 0, y }, contentSize: { width: measured.width, height }, layoutMeasurement: { width: measured.width, height: measured.height } } });

describe('a conversation of 5,000 rows', () => {
  test('builds a window of rows, at the newest', async () => {
    await openLong();
    const drawn = drawnRows();
    expect(drawn.length).toBeGreaterThan(measured.height / measured.row - 1);
    expect(drawn.length).toBeLessThan(80);
    expect(drawn).toContain(rowId(`msg:${3 + ROWS - 1}`));
    expect(drawn).not.toContain(rowId('msg:4'));
    expect(drawn).not.toContain(rowId('turn:0:user'));
    expect(wordsOf(screen.getByTestId(rowId(`msg:${3 + ROWS - 1}`)))).toBe(`Line ${ROWS - 1} of the count`);
    expect(screen.getByTestId('agent.working')).toBeTruthy();
  });

  test('a row that arrives is drawn, and no row that was there is drawn again', async () => {
    const { app, run } = await openLong();
    const before = drawnRows();
    mockDrawn.length = 0;
    const next = makeEvent('output', { role: 'assistant', text: 'One more' }, { run_id: run });
    await app.events({ ...next, seq: 3 + ROWS + 1 });
    await frames();
    expect(wordsOf(screen.getByTestId(rowId(`msg:${3 + ROWS + 1}`)))).toBe('One more');
    expect([...new Set(mockDrawn)]).toEqual([`${rowId(`msg:${3 + ROWS + 1}`)}.md`]);
    expect(drawnRows().length).toBeLessThan(80);
    expect(drawnRows().length).toBeGreaterThanOrEqual(before.length);
  });

  test('a hundred events in one frame draw their rows and nothing else', async () => {
    const { app, run } = await openLong();
    mockDrawn.length = 0;
    const burst = Array.from({ length: 100 }, (_, i) => ({ ...makeEvent('output', { role: 'assistant', text: `Burst ${i}` }, { run_id: run }), seq: 3 + ROWS + 1 + i }));
    await app.events(...burst);
    await frames();
    const again = [...new Set(mockDrawn)].filter((id) => Number(/msg:(\d+)/.exec(id)?.[1]) <= 3 + ROWS);
    expect(again).toEqual([]);
    expect(drawnRows().length).toBeLessThan(80);
    expect(wordsOf(screen.getByTestId(rowId(`msg:${3 + ROWS + 100}`)))).toBe('Burst 99');
  });
});

describe('staying at the newest', () => {
  test('at the end there is no button', async () => {
    await openLong(SOME);
    expect(screen.queryByTestId('agent.latest')).toBeNull();
    await scrollTo(SOME * measured.row - measured.height);
    expect(screen.queryByTestId('agent.latest')).toBeNull();
  });

  test('scrolled up, rows that arrive are announced by a small button that leads to them', async () => {
    const { app, run } = await openLong(SOME);
    await scrollTo(2000);
    expect(screen.getByLabelText('Jump to the latest message')).toBeTruthy();
    expect(wordsOf(screen.getByTestId('agent.latest'))).toBe('Latest');

    await app.events({ ...makeEvent('output', { role: 'assistant', text: 'While you read' }, { run_id: run }), seq: 3 + SOME + 1 });
    await frames();
    expect(wordsOf(screen.getByTestId('agent.latest'))).toBe('New messages');
    expect(screen.getByLabelText('New messages')).toBeTruthy();

    await fireEvent.press(screen.getByTestId('agent.latest'));
    expect(screen.queryByTestId('agent.latest')).toBeNull();
  });

  test('back at the end by hand, the button goes', async () => {
    await openLong(SOME);
    await scrollTo(2000);
    expect(screen.getByTestId('agent.latest')).toBeTruthy();
    await scrollTo(SOME * measured.row - measured.height - 10);
    expect(screen.queryByTestId('agent.latest')).toBeNull();
  });
});
