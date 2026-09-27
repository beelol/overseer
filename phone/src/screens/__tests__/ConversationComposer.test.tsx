import { act, fireEvent, screen } from '@testing-library/react-native';

import type { DaemonEvent } from '@/model';
import { FAKE_IPHONE } from '@/platform/fake';
import type { State } from '@/protocol';
import { ConversationScreen } from '@/screens/ConversationScreen';
import { rowId } from '@/screens/conversation/ids';
import { after, answerHistory, frames, idsOf, keepOutbox, patience, measured, recording, rootOf, wordsOf, type Recording } from '@/screens/conversation/testing';
import { createTestApp, makeEvent, type TestApp } from '@/testing';
import { router } from '@/testing/router';

jest.mock('expo-router', () => require('@/testing/router').mockRouter());
jest.mock('@shopify/flash-list/dist/recyclerview/utils/measureLayout', () => require('@/screens/conversation/testing').measuring());

const mockSaved: { side: number; quality: number; length: number }[] = [];
let mockPicked: { uri: string; width: number; height: number; fileName: string } | null = { uri: 'file:///picked.png', width: 4000, height: 3000, fileName: 'IMG_0042.PNG' };
/** Characters of base64 for a side of one point at full quality: a photo of 4000 points is far too large. */
let mockWeight = 800;

jest.mock('expo-image-picker', () => ({
  requestCameraPermissionsAsync: jest.fn(async () => ({ granted: true })),
  launchCameraAsync: jest.fn(async () => (mockPicked ? { canceled: false, assets: [mockPicked] } : { canceled: true, assets: null })),
  launchImageLibraryAsync: jest.fn(async () => (mockPicked ? { canceled: false, assets: [mockPicked] } : { canceled: true, assets: null })),
}));
jest.mock('expo-image-manipulator', () => ({
  SaveFormat: { JPEG: 'jpeg', PNG: 'png', WEBP: 'webp' },
  ImageManipulator: {
    manipulate: () => {
      let side = 4000;
      const context = {
        resize: (size: { width?: number; height?: number }) => {
          side = size.width ?? size.height ?? side;
          return context;
        },
        renderAsync: async () => ({
          width: side,
          height: side * 0.75,
          saveAsync: async (options: { compress: number }) => {
            const length = Math.round(side * options.compress * mockWeight);
            mockSaved.push({ side, quality: options.compress, length });
            return { uri: `file:///fitted-${side}-${options.compress}.jpg`, width: side, height: side * 0.75, base64: 'A'.repeat(length) };
          },
        }),
      };
      return context;
    },
  },
}));

patience();

beforeEach(() => {
  router.reset();
  measured.row = 20;
  mockSaved.length = 0;
  mockPicked = { uri: 'file:///picked.png', width: 4000, height: 3000, fileName: 'IMG_0042.PNG' };
  mockWeight = 800;
});

interface Opened {
  readonly app: TestApp;
  readonly r: Recording;
  readonly run: string;
}

async function open(name: string, options: { state?: State; through?: number; before?: (app: TestApp, run: string) => void } = {}): Promise<Opened> {
  const r = recording(name);
  const app = await createTestApp({ state: options.state ?? r.final, launch: FAKE_IPHONE });
  answerHistory(app.connection, r.events, options.through);
  app.connection.answers['workspace.changes'] = () => ({ files: 0, added: 0, removed: 0, names: [] });
  keepOutbox(app.connection);
  options.before?.(app, rootOf(r));
  router.params = { run: rootOf(r) };
  await app.render(<ConversationScreen />);
  await frames();
  return { app, r, run: rootOf(r) };
}

async function arrive(app: TestApp, ...events: DaemonEvent[]): Promise<void> {
  await app.events(...events);
  await frames();
}

/** The agent of the recording while its first turn runs. */
function working(r: Recording): State {
  const found = r.checkpoints.find((c) => c.state.runs[0]?.status === 'running');
  if (!found) throw new Error(`${r.scenario} never runs`);
  return found.state;
}

const words = (testID: string): string => wordsOf(screen.getByTestId(testID));
const bubbles = (): string[] => idsOf(screen.toJSON()).filter((id) => /^agent\.row\.(sent:[^.]+|turn:\d+:user)$/.test(id));
const write = (text: string) => fireEvent.changeText(screen.getByTestId('agent.composer.text'), text);
const send = async (): Promise<void> => {
  await fireEvent.press(screen.getByTestId('agent.composer.send'));
  await frames();
};

/** The daemon starts the turn of a message a phone sent. */
function started(run: string, requestId: string, prompt: string, n: number): DaemonEvent[] {
  return [
    makeEvent('remote_command', { method: 'run.follow_up', device: 'd1', request_id: requestId }, { run_id: run, source: 'phone:Phone' }),
    makeEvent('turn_started', { turn: { id: `u-${n}`, run_id: run, n, prompt, started_ms: 1, ended_ms: null, status: 'running' } }, { run_id: run }),
    makeEvent('status', { status: 'running' }, { run_id: run }),
  ];
}

describe('sending', () => {
  test('nothing written, nothing sent', async () => {
    const { app } = await open('echo-follow-up');
    expect(screen.getByTestId('agent.composer.send').props.accessibilityState).toMatchObject({ disabled: true });
    await write('   ');
    await send();
    expect(app.connection.calls('run.follow_up')).toHaveLength(0);
    expect(screen.getByLabelText('Send')).toBeTruthy();
    expect(screen.queryByTestId('agent.composer.stop')).toBeNull();
  });

  test('the message shows at once, marked Sending, before the Mac answers; then it is the message, once', async () => {
    const { app, run } = await open('echo-follow-up');
    let accepted: (value: unknown) => void = () => undefined;
    app.connection.answers['run.follow_up'] = () => new Promise((resolve) => (accepted = resolve));
    expect(bubbles()).toHaveLength(2);
    await write('And a third time');
    await send();

    expect(app.connection.calls('run.follow_up')).toEqual([{ run_id: run, prompt: 'And a third time' }]);
    const requestId = String(app.connection.entries[0]?.requestId);
    expect(requestId).toMatch(/^[0-9a-f-]{36}$/);
    const bubble = rowId(`sent:${requestId}`);
    expect(words(bubble)).toBe('And a third time');
    expect(words(`${bubble}.mark`)).toBe('Sending');
    expect(bubbles()).toHaveLength(3);
    expect(screen.getByTestId('agent.composer.text').props.value).toBe('');
    expect(app.platform.fakes.haptics.played()).toContain('confirm');

    // The daemon starts the turn: the same row is now the turn's own message.
    await arrive(app, ...started(run, requestId, 'And a third time', 3));
    expect(bubbles()).toHaveLength(3);
    expect(screen.getAllByText('And a third time')).toHaveLength(1);
    expect(screen.queryByTestId(`${bubble}.mark`)).toBeNull();

    await act(async () => accepted({ id: 'u-3', run_id: run, n: 3, prompt: 'And a third time', started_ms: 1, status: 'running' }));
    await frames();
    expect(bubbles()).toHaveLength(3);
    expect(screen.getAllByText('And a third time')).toHaveLength(1);
    expect(app.connection.calls('run.follow_up')).toHaveLength(1);
    expect(words(rowId(`note:${lastNote()}`))).toBe('Message from Phone');
  });

  test('a message the Mac did not take says Not sent; Try again sends it as a new message', async () => {
    const { app, run } = await open('echo-follow-up');
    app.connection.answers['run.follow_up'] = () => {
      throw Object.assign(new Error('workspace was removed'), { code: 'failed' });
    };
    await write('Once more');
    await send();
    const first = String(app.connection.entries[0]?.requestId);
    expect(words(`${rowId(`sent:${first}`)}.mark`)).toBe('Not sent');

    app.connection.answers['run.follow_up'] = () => new Promise(() => undefined);
    await fireEvent.press(screen.getByTestId(`${rowId(`sent:${first}`)}.retry`));
    await frames();
    expect(app.connection.calls('run.follow_up')).toEqual([
      { run_id: run, prompt: 'Once more' },
      { run_id: run, prompt: 'Once more' },
    ]);
    expect(screen.queryByTestId(rowId(`sent:${first}`))).toBeNull();
    const second = String(app.connection.entries[0]?.requestId);
    expect(second).not.toBe(first);
    expect(words(`${rowId(`sent:${second}`)}.mark`)).toBe('Sending');
    expect(screen.getAllByText('Once more')).toHaveLength(1);
  });

  test('Remove takes a message that was not sent off the list', async () => {
    const { app } = await open('echo-follow-up');
    app.connection.answers['run.follow_up'] = () => {
      throw Object.assign(new Error('workspace was removed'), { code: 'failed' });
    };
    await write('Once more');
    await send();
    const id = rowId(`sent:${String(app.connection.entries[0]?.requestId)}`);
    await fireEvent.press(screen.getByTestId(`${id}.remove`));
    await frames();
    expect(screen.queryByTestId(id)).toBeNull();
    expect(app.connection.entries).toHaveLength(0);
    expect(app.connection.calls('run.follow_up')).toHaveLength(1);
  });

  test('the choices of this message go with it: model, effort, permission mode', async () => {
    const { app, run } = await open('echo-follow-up');
    app.connection.answers['run.follow_up'] = () => new Promise(() => undefined);
    expect(screen.getByLabelText('Model: Default')).toBeTruthy();
    await fireEvent.press(screen.getByTestId('agent.composer.model'));
    await fireEvent.press(screen.getByTestId('agent.composer.model.opus'));
    await fireEvent.press(screen.getByTestId('agent.composer.effort'));
    await fireEvent.press(screen.getByTestId('agent.composer.effort.high'));
    await fireEvent.press(screen.getByTestId('agent.composer.mode'));
    await fireEvent.press(screen.getByTestId('agent.composer.mode.plan'));
    expect(screen.getByLabelText('Model: opus')).toBeTruthy();
    expect(screen.getByLabelText('Effort: high')).toBeTruthy();
    expect(screen.getByLabelText('Permissions: Plan only')).toBeTruthy();
    await write('Plan the migration');
    await send();
    expect(app.connection.calls('run.follow_up')).toEqual([{ run_id: run, prompt: 'Plan the migration', model: 'opus', effort: 'high', permission_mode: 'plan' }]);
  });

  test('the model chip says the model the agent runs with', async () => {
    await open('showcase');
    expect(screen.getByLabelText('Model: fixture-large')).toBeTruthy();
    expect(wordsOf(screen.getByTestId('agent.composer.model'))).toBe('fixture-large');
  });

  test('an agent without the abilities offers no choices and no image', async () => {
    const r = recording('echo-follow-up');
    const plain = { ...r.final, runs: r.final.runs.map((run) => ({ ...run, capabilities: { follow_up: 'supported', interrupt: 'supported', model: 'unsupported' } })) };
    await open('echo-follow-up', { state: plain });
    expect(screen.queryByTestId('agent.composer.model')).toBeNull();
    expect(screen.queryByTestId('agent.composer.effort')).toBeNull();
    expect(screen.queryByTestId('agent.composer.mode')).toBeNull();
    expect(screen.queryByTestId('agent.composer.attach')).toBeNull();
    expect(screen.getByTestId('agent.composer.send')).toBeTruthy();
  });

  test('an agent that takes no messages says so where the message would be written', async () => {
    const r = recording('echo-follow-up');
    const mute = { ...r.final, runs: r.final.runs.map((run) => ({ ...run, capabilities: { follow_up: 'unsupported (one turn only)' } })) };
    const { app } = await open('echo-follow-up', { state: mute });
    const field = screen.getByTestId('agent.composer.text');
    expect(field.props.editable).toBe(false);
    expect(field.props.placeholder).toBe('Claude Code does not take follow-ups');
    await send();
    expect(app.connection.calls('run.follow_up')).toHaveLength(0);
  });
});

describe('while the agent works', () => {
  test('Send queues the message; it is sent once, when the turn has ended', async () => {
    const r = recording('echo-follow-up');
    const { app, run } = await open('echo-follow-up', { state: working(r), through: 9 });
    app.connection.answers['run.follow_up'] = () => new Promise(() => undefined);
    expect(screen.getByLabelText('Queue message')).toBeTruthy();
    expect(screen.getByTestId('agent.composer.text').props.placeholder).toBe('Message for when it finishes');
    await write('Then run the tests');
    await send();

    expect(app.connection.calls('run.follow_up')).toHaveLength(0);
    const queued = bubbles().filter((id) => id.startsWith('agent.row.sent:'));
    expect(queued).toHaveLength(1);
    const bubble = String(queued[0]);
    expect(words(bubble)).toBe('Then run the tests');
    expect(words(`${bubble}.mark`)).toBe('Queued');

    // The turn ends: the message goes, with the id it was written with.
    await arrive(app, ...after(r, 9).filter((e) => e.seq <= 11));
    expect(app.connection.calls('run.follow_up')).toEqual([{ run_id: run, prompt: 'Then run the tests' }]);
    expect(rowId(`sent:${String(app.connection.entries[0]?.requestId)}`)).toBe(bubble);
    expect(words(`${bubble}.mark`)).toBe('Sending');
    expect(screen.getAllByText('Then run the tests')).toHaveLength(1);

    await arrive(app, ...started(run, String(app.connection.entries[0]?.requestId), 'Then run the tests', 2));
    expect(screen.getAllByText('Then run the tests')).toHaveLength(1);
    expect(screen.queryByTestId(`${bubble}.mark`)).toBeNull();
    expect(app.connection.calls('run.follow_up')).toHaveLength(1);
  });

  test('two queued messages go one turn at a time, in the order they were written', async () => {
    const r = recording('echo-follow-up');
    const { app, run } = await open('echo-follow-up', { state: working(r), through: 9 });
    app.connection.answers['run.follow_up'] = () => ({ id: 'u', run_id: run, n: 2, prompt: '', started_ms: 1, status: 'running' });
    await write('First');
    await send();
    await write('Second');
    await send();
    expect(app.connection.calls('run.follow_up')).toHaveLength(0);

    await arrive(app, ...after(r, 9).filter((e) => e.seq <= 11));
    expect(app.connection.calls('run.follow_up')).toEqual([{ run_id: run, prompt: 'First' }]);
    const first = String(app.connection.entries[0]?.requestId);
    await arrive(app, ...started(run, first, 'First', 2));
    expect(app.connection.calls('run.follow_up')).toHaveLength(1);
    expect(words(`${rowId(`sent:${secondOf(app, first)}`)}.mark`)).toBe('Queued');

    await arrive(app, makeEvent('turn_done', { ok: true, summary: 'done' }, { run_id: run }), makeEvent('status', { status: 'completed' }, { run_id: run }));
    expect(app.connection.calls('run.follow_up')).toEqual([
      { run_id: run, prompt: 'First' },
      { run_id: run, prompt: 'Second' },
    ]);
  });

  test('a queued message can be taken back', async () => {
    const r = recording('echo-follow-up');
    const { app } = await open('echo-follow-up', { state: working(r), through: 9 });
    await write('Never mind');
    await send();
    const bubble = String(bubbles().find((id) => id.startsWith('agent.row.sent:')));
    await fireEvent.press(screen.getByTestId(`${bubble}.cancel`));
    await frames();
    expect(screen.queryByTestId(bubble)).toBeNull();
    await arrive(app, ...after(r, 9).filter((e) => e.seq <= 11));
    expect(app.connection.calls('run.follow_up')).toHaveLength(0);
  });

  test('Stop stops the turn', async () => {
    const r = recording('echo-follow-up');
    const { app, run } = await open('echo-follow-up', { state: working(r), through: 9 });
    app.connection.answers['run.interrupt'] = () => ({ ok: true });
    await fireEvent.press(screen.getByTestId('agent.composer.stop'));
    expect(app.connection.calls('run.interrupt')).toEqual([{ run_id: run }]);
    expect(app.platform.fakes.haptics.played()).toContain('impact');
    await arrive(app, ...after(r, 9).filter((e) => e.seq <= 11));
    expect(screen.queryByTestId('agent.composer.stop')).toBeNull();
    expect(screen.getByLabelText('Send')).toBeTruthy();
  });
});

describe('the draft', () => {
  test('what is written and not sent is kept for this agent', async () => {
    const { app, run } = await open('echo-follow-up');
    await write('half a thought');
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 500));
    });
    expect(app.platform.fakes.keyValue.items.get(`drafts.${run}`)).toBe(JSON.stringify('half a thought'));
  });

  test('it is there when the agent is opened again, and gone once it was sent', async () => {
    const { app, run } = await open('echo-follow-up', { before: (a, id) => a.platform.capabilities.keyValue.scope<Record<string, string>>('drafts').set(id, 'half a thought') });
    app.connection.answers['run.follow_up'] = () => new Promise(() => undefined);
    expect(screen.getByTestId('agent.composer.text').props.value).toBe('half a thought');
    await send();
    expect(app.connection.calls('run.follow_up')).toEqual([{ run_id: run, prompt: 'half a thought' }]);
    expect(app.platform.fakes.keyValue.items.has(`drafts.${run}`)).toBe(false);
  });
});

describe('an image', () => {
  const attach = async (): Promise<void> => {
    await fireEvent.press(screen.getByTestId('agent.composer.attach'));
    await act(async () => undefined);
    await fireEvent.press(screen.getByTestId('agent.composer.attach.library'));
    await frames();
  };

  test('is made to fit: 1600 on its long side, then lower quality until it is under 700 KB', async () => {
    const { app, run } = await open('echo-follow-up');
    app.connection.answers['run.follow_up'] = () => new Promise(() => undefined);
    await attach();
    expect(mockSaved.map((s) => [s.side, s.quality])).toEqual([
      [1600, 0.8],
      [1600, 0.6],
      [1600, 0.45],
    ]);
    expect(mockSaved[2]?.length).toBeLessThanOrEqual(700 * 1024);
    expect(screen.getByTestId('agent.composer.image.0')).toBeTruthy();
    expect(screen.getByLabelText('IMG_0042.jpg')).toBeTruthy();

    await write('What is wrong in this screenshot?');
    await send();
    const sent = app.connection.calls('run.follow_up') as { run_id: string; prompt: string; images: { mime: string; data: string; name: string }[] }[];
    expect(sent).toHaveLength(1);
    expect(sent[0]?.run_id).toBe(run);
    expect(sent[0]?.images).toEqual([{ mime: 'image/jpeg', name: 'IMG_0042.jpg', data: 'A'.repeat(576_000) }]);
    expect(JSON.stringify({ id: 1, method: 'run.follow_up', params: sent[0], request_id: 'x'.repeat(36) }).length).toBeLessThan(1024 * 1024);
    expect(screen.queryByTestId('agent.composer.image.0')).toBeNull();
  });

  test('that still does not fit is made smaller', async () => {
    mockWeight = 2400;
    await open('echo-follow-up');
    await attach();
    const last = mockSaved[mockSaved.length - 1];
    expect(last?.side).toBeLessThan(1600);
    expect(last?.length).toBeLessThanOrEqual(700 * 1024);
    expect(screen.getByTestId('agent.composer.image.0')).toBeTruthy();
  });

  test('that cannot be made to fit is not attached, and the composer says so', async () => {
    mockWeight = 100_000;
    await open('echo-follow-up');
    await attach();
    expect(screen.queryByTestId('agent.composer.image.0')).toBeNull();
    expect(words('agent.composer.notice')).toBe('This image is too large to send.');
  });

  test('can be removed before it is sent', async () => {
    const { app, run } = await open('echo-follow-up');
    app.connection.answers['run.follow_up'] = () => new Promise(() => undefined);
    await attach();
    await fireEvent.press(screen.getByTestId('agent.composer.image.0.remove'));
    expect(screen.queryByTestId('agent.composer.image.0')).toBeNull();
    await write('Without it');
    await send();
    expect(app.connection.calls('run.follow_up')).toEqual([{ run_id: run, prompt: 'Without it' }]);
  });

  test('two images share the room of one request', async () => {
    const { app } = await open('echo-follow-up');
    app.connection.answers['run.follow_up'] = () => new Promise(() => undefined);
    await attach();
    // What is left of the request's room is what the second image has to fit in.
    mockWeight = 300;
    await attach();
    expect(screen.getByTestId('agent.composer.image.1')).toBeTruthy();
    expect(mockSaved[mockSaved.length - 1]?.length).toBeLessThanOrEqual(700 * 1024 - 576_000);
    await write('Compare these');
    await send();
    const sent = app.connection.calls('run.follow_up') as { images: { data: string }[] }[];
    const total = (sent[0]?.images ?? []).reduce((n, image) => n + image.data.length, 0);
    expect(sent[0]?.images).toHaveLength(2);
    expect(total).toBeLessThanOrEqual(700 * 1024);
  });

  test('nothing chosen, nothing attached; a simulator offers no camera', async () => {
    mockPicked = null;
    await open('echo-follow-up');
    await fireEvent.press(screen.getByTestId('agent.composer.attach'));
    await act(async () => undefined);
    expect(screen.getByTestId('agent.composer.attach.camera')).toBeTruthy();
    await fireEvent.press(screen.getByTestId('agent.composer.attach.library'));
    await frames();
    expect(screen.queryByTestId('agent.composer.image.0')).toBeNull();
    expect(screen.queryByTestId('agent.composer.notice')).toBeNull();
  });
});

/** The newest note's sequence number on screen. */
function lastNote(): number {
  const notes = idsOf(screen.toJSON())
    .map((id) => /^agent\.row\.note:(\d+)$/.exec(id)?.[1])
    .filter((n): n is string => n !== undefined)
    .map(Number);
  return Math.max(...notes);
}

/** The request id of the message that still waits on the phone. */
function secondOf(app: TestApp, first: string): string {
  const waiting = idsOf(screen.toJSON())
    .map((id) => /^agent\.row\.sent:([^.]+)$/.exec(id)?.[1])
    .filter((id): id is string => id !== undefined && id !== first);
  if (waiting.length !== 1) throw new Error(`expected one waiting message, found ${waiting.length} (${app.connection.entries.length} in the outbox)`);
  return String(waiting[0]);
}
