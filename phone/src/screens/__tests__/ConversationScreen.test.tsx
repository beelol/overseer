import { act, fireEvent, screen } from '@testing-library/react-native';

import type { DaemonEvent } from '@/model';
import { FAKE_IPHONE } from '@/platform/fake';
import type { State } from '@/protocol';
import { ConversationScreen } from '@/screens/ConversationScreen';
import { EDITED_HUNK, rowId } from '@/screens/conversation/ids';
import { after, answerHistory, frames, idsOf, keepOutbox, patience, measured, recording, rootOf, stateAt, wordsOf, type Recording } from '@/screens/conversation/testing';
import { routes } from '@/routes';
import { createTestApp, makeEvent, type TestApp } from '@/testing';
import { router } from '@/testing/router';

jest.mock('expo-router', () => require('@/testing/router').mockRouter());
jest.mock('@shopify/flash-list/dist/recyclerview/utils/measureLayout', () => require('@/screens/conversation/testing').measuring());

patience();

beforeEach(() => {
  router.reset();
  measured.row = 20;
});

interface Opened {
  readonly app: TestApp;
  readonly r: Recording;
  readonly run: string;
}

/** Opens the conversation of a recording: the Mac is at `state` and its history goes up to `through`. */
async function open(name: string, options: { state?: State; through?: number; scope?: 'full' | 'watch' } = {}): Promise<Opened> {
  const r = recording(name);
  const app = await createTestApp({ state: options.state ?? r.final, launch: FAKE_IPHONE, ...(options.scope ? { scope: options.scope } : {}) });
  answerHistory(app.connection, r.events, options.through);
  app.connection.answers['workspace.changes'] = () => ({ files: 0, added: 0, removed: 0, names: [] });
  keepOutbox(app.connection);
  router.params = { run: rootOf(r) };
  await app.render(<ConversationScreen />);
  await frames();
  return { app, r, run: rootOf(r) };
}

/** Live events, then the frames the list needs to draw them. */
async function arrive(app: TestApp, ...events: DaemonEvent[]): Promise<void> {
  await app.events(...events);
  await frames();
}

const words = (testID: string): string => wordsOf(screen.getByTestId(testID));
const rows = (): string[] => idsOf(screen.toJSON()).filter((id) => /^agent\.row\.[^.]+$/.test(id));
const bubbles = (): string[] => rows().filter((id) => id.includes('user') || id.includes('sent:'));

describe('the header', () => {
  test('says the title, the status in words, and leads back', async () => {
    const { app } = await open('showcase');
    expect(words('agent.title')).toBe('Refresh sessions once');
    expect(words('agent.subtitle')).toBe("Done · Mac's default login");
    await fireEvent.press(screen.getByTestId('agent.back'));
    expect(router.backs).toBe(1);
    expect(app.connection.calls('events.list')).toHaveLength(1);
  });

  test('Changes carries the count of changed files and opens them', async () => {
    const r = recording('showcase');
    const app = await createTestApp({ state: r.final });
    answerHistory(app.connection, r.events);
    app.connection.answers['workspace.changes'] = () => ({ files: 3, added: 10, removed: 2, names: ['a.ts', 'b.ts', 'c.ts'] });
    router.params = { run: rootOf(r) };
    await app.render(<ConversationScreen />);
    await frames();
    expect(app.connection.calls('workspace.changes')).toEqual([{ workspace_id: r.final.workspaces[0]?.id }]);
    expect(screen.getByLabelText('Changes, 3 files')).toBeTruthy();
    expect(words('agent.changes')).toContain('3');
    await fireEvent.press(screen.getByTestId('agent.changes'));
    expect(router.pushed).toEqual([routes.changes(rootOf(r))]);
  });

  test('the count is asked again, once, a moment after edits arrive and when the turn ends', async () => {
    const r = recording('showcase');
    const { app } = await open('showcase', { state: stateOf(r, 10), through: 10 });
    expect(app.connection.calls('workspace.changes')).toHaveLength(1);
    // Two edits and what lies between them, in two frames one after the other: one question.
    await app.events(...after(r, 10).filter((e) => e.seq <= 18));
    await app.events(...after(r, 18).filter((e) => e.seq <= 22));
    await wait(700);
    expect(app.connection.calls('workspace.changes')).toHaveLength(2);
    await app.events(...after(r, 22));
    await wait(700);
    expect(app.connection.calls('workspace.changes')).toHaveLength(3);
  });

  test('More holds Stop while the agent works', async () => {
    const r = recording('echo-follow-up');
    const { app, run } = await open('echo-follow-up', { state: stateOf(r, 10), through: 9 });
    app.connection.answers['run.interrupt'] = () => ({ ok: true });
    await fireEvent.press(screen.getByTestId('agent.more'));
    expect(screen.queryByTestId('agent.more.merge')).toBeNull();
    expect(screen.queryByTestId('agent.more.archive')).toBeNull();
    await fireEvent.press(screen.getByTestId('agent.more.stop'));
    expect(app.connection.calls('run.interrupt')).toEqual([{ run_id: run }]);
  });

  test('More leads to Merge back and Pull request once the agent has stopped', async () => {
    const { run } = await open('showcase');
    await fireEvent.press(screen.getByTestId('agent.more'));
    expect(screen.queryByTestId('agent.more.stop')).toBeNull();
    await fireEvent.press(screen.getByTestId('agent.more.merge'));
    await fireEvent.press(screen.getByTestId('agent.more'));
    await fireEvent.press(screen.getByTestId('agent.more.pr'));
    expect(router.pushed).toEqual([routes.merge(run), routes.pr(run)]);
  });

  test('Clean up names the uncommitted files the Mac lists, asks once, then removes', async () => {
    const { app, r } = await open('showcase');
    const workspace = r.final.workspaces[0];
    app.connection.answers['workspace.cleanup_plan'] = () => ({
      workspace,
      active_runs: [],
      removable: true,
      reason: '',
      dirty: { staged: [{ status: 'M', path: 'src/a.ts' }], unstaged: [{ status: 'M', path: 'src/a.ts' }, { status: 'M', path: 'README.md' }], untracked: ['notes.txt'], conflicted: [] },
    });
    app.connection.answers['workspace.cleanup'] = () => ({ ok: true });
    await fireEvent.press(screen.getByTestId('agent.more'));
    await fireEvent.press(screen.getByTestId('agent.more.cleanup'));
    await app.settle();
    expect(app.connection.calls('workspace.cleanup_plan')).toEqual([{ workspace_id: workspace?.id }]);
    expect(screen.getByText(`Remove the worktree ${workspace?.branch}?`)).toBeTruthy();
    expect(screen.getByText('3 uncommitted files will be lost: src/a.ts, README.md, notes.txt')).toBeTruthy();
    expect(app.connection.calls('workspace.cleanup')).toHaveLength(0);
    await fireEvent.press(screen.getByTestId('agent.more.cleanup.confirm'));
    expect(app.connection.calls('workspace.cleanup')).toEqual([{ workspace_id: workspace?.id, discard_dirty: true }]);
  });

  test('with the unlock before changes on, Clean up needs the unlock', async () => {
    const { app, r } = await open('showcase');
    const workspace = r.final.workspaces[0];
    app.platform.fakes.keyValue.items.set('settings.unlockBeforeChanges', 'true');
    app.connection.answers['workspace.cleanup_plan'] = () => ({ workspace, active_runs: [], removable: true, reason: '', dirty: { staged: [], unstaged: [], untracked: ['notes.txt'], conflicted: [] } });
    app.connection.answers['workspace.cleanup'] = () => ({ ok: true });
    app.platform.fakes.deviceUnlock.answerWith({ ok: false, cause: 'failed' });
    await fireEvent.press(screen.getByTestId('agent.more'));
    await fireEvent.press(screen.getByTestId('agent.more.cleanup'));
    await app.settle();
    await fireEvent.press(screen.getByTestId('agent.more.cleanup.confirm'));
    await app.settle();
    expect(app.platform.fakes.deviceUnlock.requests()).toEqual([{ reason: 'Clean up' }]);
    expect(app.connection.calls('workspace.cleanup')).toHaveLength(0);
    expect(screen.getByText('The unlock did not work. Nothing was changed.')).toBeTruthy();

    await fireEvent.press(screen.getByTestId('agent.more.cleanup.no.ok'));
    app.platform.fakes.deviceUnlock.answerWith({ ok: true });
    await fireEvent.press(screen.getByTestId('agent.more'));
    await fireEvent.press(screen.getByTestId('agent.more.cleanup'));
    await app.settle();
    await fireEvent.press(screen.getByTestId('agent.more.cleanup.confirm'));
    await app.settle();
    expect(app.connection.calls('workspace.cleanup')).toEqual([{ workspace_id: workspace?.id, discard_dirty: true }]);
  });

  test('Clean up that the Mac refuses says why and removes nothing', async () => {
    const { app, r } = await open('showcase');
    app.connection.answers['workspace.cleanup_plan'] = () => ({ workspace: r.final.workspaces[0], active_runs: ['r1'], removable: false, reason: 'an agent is still working in it' });
    await fireEvent.press(screen.getByTestId('agent.more'));
    await fireEvent.press(screen.getByTestId('agent.more.cleanup'));
    await app.settle();
    expect(screen.getByText('an agent is still working in it')).toBeTruthy();
    expect(screen.queryByTestId('agent.more.cleanup.confirm')).toBeNull();
    expect(app.connection.calls('workspace.cleanup')).toHaveLength(0);
  });

  test('Archive archives the task and goes back to the agents', async () => {
    const { app, r } = await open('showcase');
    app.connection.answers['task.archive'] = () => ({ task_id: r.final.tasks[0]?.id, archived_ms: 1 });
    await fireEvent.press(screen.getByTestId('agent.more'));
    await fireEvent.press(screen.getByTestId('agent.more.archive'));
    expect(app.connection.calls('task.archive')).toEqual([{ task_id: r.final.tasks[0]?.id, archived: true }]);
    expect(router.backs).toBe(1);
  });
});

describe('the rows', () => {
  test('a turn: your message, the reply as formatted text, folded steps, edit chips, the quiet end', async () => {
    const { run } = await open('showcase');
    expect(words(rowId('turn:0:user'))).toBe('Make expired sessions refresh once');
    expect(words(rowId('msg:8'))).toContain("I'll start by reading how sign-in works today");
    expect(words(rowId('steps:9'))).toContain('6 steps');
    expect(words(`${rowId('steps:9')}.verbs`)).toBe('Read · Searched · Found files · Created · Edited · Ran');
    expect(screen.getByLabelText('session-refresh-coordinator.ts, Open at the edited hunk in the review')).toBeTruthy();
    expect(words(`${rowId('turn:0:foot')}.state`)).toBe('Done');
    expect(words(`${rowId('turn:0:foot')}.usage`)).toBe('20k tokens · $0.04');
    // The reply's Markdown: a heading, a list, a table that scrolls sideways, a code block.
    const reply = rowId('msg:29');
    expect(words(reply)).toContain('Done: sessions refresh once');
    expect(screen.getByTestId(`${reply}.md.3.table`).props.horizontal).toBe(true);
    expect(words(`${reply}.md.4.text`)).toBe('const token = await coordinator.refresh(() => api.refreshToken());');
    expect(run).toBe('r-5d45abbb0f30');
  });

  test('steps unfold on a tap and fold again', async () => {
    await open('showcase');
    const tools = (): string[] => rows().filter((id) => id.startsWith('agent.row.tool:'));
    expect(tools()).toHaveLength(0);
    expect(screen.getByTestId(rowId('steps:9')).props.accessibilityState).toMatchObject({ expanded: false });
    await fireEvent.press(screen.getByTestId(rowId('steps:9')));
    await frames();
    expect(tools()).toHaveLength(6);
    expect(screen.getByTestId(rowId('steps:9')).props.accessibilityState).toMatchObject({ expanded: true });
    expect(screen.getByLabelText('Read README.md, Done')).toBeTruthy();
    expect(screen.getByLabelText('Created session-refresh-coordinator.ts, +8 −0')).toBeTruthy();
    await fireEvent.press(screen.getByTestId(rowId('steps:9')));
    await frames();
    expect(tools()).toHaveLength(0);
  });

  test('a tool row opens what it was given and what it returned', async () => {
    await open('showcase');
    await fireEvent.press(screen.getByTestId(rowId('steps:9')));
    await frames();
    expect(screen.queryByTestId('agent.tool.input')).toBeNull();
    await fireEvent.press(screen.getByLabelText(/^Ran npm test/));
    expect(words('agent.tool.input')).toContain('npm test');
    expect(words('agent.tool.output')).toContain('shares one refresh between callers');
    await fireEvent.press(screen.getByTestId('agent.tool.close'));
    expect(screen.queryByTestId('agent.tool.input')).toBeNull();
  });

  test('an edit chip opens the file where the agent edited it', async () => {
    const { run, r } = await open('showcase');
    await fireEvent.press(screen.getByTestId(`${rowId('edit:22')}.file.0`));
    const edited = r.events.find((e) => e.seq === 22)?.payload as { paths: string[] };
    expect(router.pushed).toEqual([routes.file(run, String(edited.paths[0]), { hunk: EDITED_HUNK })]);
  });

  test('children are nested one deeper, fold and unfold', async () => {
    await open('nested');
    const child = rowId('child:r-222956848452');
    const grandchild = rowId('child:r-ec4358eb5740');
    expect(screen.getByLabelText('child task, Done')).toBeTruthy();
    expect(words(`${grandchild}.status`)).toBe('Done');
    expect(words(rowId('msg:9'))).toBe('grandchild says hi');
    expect(words(rowId('msg:17'))).toBe('child done');
    // One deeper for each child, with the line of every child that holds the row.
    const indent = (id: string): number => Number(Object.assign({}, ...[screen.getByTestId(`${rowId(id)}.frame`).props.style].flat(3).filter(Boolean)).paddingLeft);
    expect(indent('msg:17')).toBeGreaterThan(indent('child:r-222956848452'));
    expect(indent('msg:9')).toBeGreaterThan(indent('msg:17'));
    expect(indent('msg:20')).toBe(indent('turn:0:user'));
    const linesOf = (id: string): string[] => idsOf(screen.toJSON()).filter((other) => other.startsWith(`${rowId(id)}.line.`));
    expect(linesOf('msg:9')).toEqual([`${rowId('msg:9')}.line.2`, `${rowId('msg:9')}.line.1`]);
    expect(linesOf('msg:17')).toEqual([`${rowId('msg:17')}.line.1`]);
    expect(linesOf('child:r-222956848452')).toEqual([`${rowId('child:r-222956848452')}.line.1`]);
    expect(linesOf('msg:20')).toEqual([]);
    await fireEvent.press(screen.getByTestId(grandchild));
    await frames();
    expect(screen.queryByTestId(rowId('msg:9'))).toBeNull();
    expect(screen.getByTestId(rowId('msg:17'))).toBeTruthy();
    await fireEvent.press(screen.getByTestId(child));
    await frames();
    expect(screen.queryByTestId(grandchild)).toBeNull();
    expect(screen.queryByTestId(rowId('msg:17'))).toBeNull();
    expect(words(rowId('msg:20'))).toBe('all done');
    await fireEvent.press(screen.getByTestId(child));
    await frames();
    expect(screen.getByTestId(rowId('msg:17'))).toBeTruthy();
  });

  test('an error says what failed; the turn ends as failed', async () => {
    const { r } = await open('failed-reason');
    const failed = r.events.find((e) => e.kind === 'error');
    const id = rowId(`err:${failed?.seq}`);
    expect(words(id)).toContain((failed?.payload as { message: string }).message);
    expect(screen.queryByTestId(`${id}.signin`)).toBeNull();
    expect(words(`${rowId('turn:0:foot')}.state`)).toMatch(/^Failed/);
  });

  test('signed out: the error leads to the accounts', async () => {
    const { r } = await open('auth');
    const failed = r.events.find((e) => e.kind === 'error' && (e.payload as { class: string }).class === 'auth');
    expect(screen.getAllByText('Signed out')).toHaveLength(3);
    await fireEvent.press(screen.getByTestId(`${rowId(`err:${failed?.seq}`)}.signin`));
    expect(router.pushed).toEqual([routes.accounts]);
  });

  test('a thought is one line until it is opened; what was done from a phone is a quiet line', async () => {
    const { app, run } = await open('echo-follow-up');
    const thought = makeEvent('output', { role: 'reasoning', text: 'Compare **both** files first.' }, { run_id: run });
    const stopped = makeEvent('remote_command', { method: 'run.interrupt', device: 'd1' }, { run_id: run, source: 'phone:Phone' });
    await arrive(app, thought, stopped);
    const id = rowId(`think:${thought.seq}`);
    expect(words(id)).toBe('Thinking');
    expect(screen.queryByTestId(`${id}.text`)).toBeNull();
    await fireEvent.press(screen.getByTestId(id));
    expect(words(`${id}.text`)).toBe('Compare both files first.');
    await fireEvent.press(screen.getByTestId(id));
    expect(screen.queryByTestId(`${id}.text`)).toBeNull();
    expect(words(rowId(`note:${stopped.seq}`))).toBe('Stopped from Phone');
  });

  test('what Overseer does to the agent is a quiet line; its briefing opens on a tap', async () => {
    const { app, run } = await open('echo-follow-up');
    const held = makeEvent('hold', { reason: 'two agents write the same file' }, { run_id: run, source: 'overseer' });
    const briefing = makeEvent('briefing', { text: 'The owner wants small commits.' }, { run_id: run, source: 'overseer' });
    const quiet = makeEvent('overseer_tool_call', { name: 'hold' }, { run_id: run, source: 'overseer' });
    await arrive(app, held, briefing, quiet);
    expect(words(rowId(`note:${held.seq}`))).toBe('Held by Overseer: two agents write the same file');
    expect(words(rowId(`note:${briefing.seq}`))).toBe('Overseer added a briefing');
    expect(screen.queryByTestId(`${rowId(`note:${briefing.seq}`)}.detail`)).toBeNull();
    await fireEvent.press(screen.getByTestId(rowId(`note:${briefing.seq}`)));
    expect(words(`${rowId(`note:${briefing.seq}`)}.detail`)).toBe('The owner wants small commits.');
    expect(screen.queryByTestId(rowId(`note:${quiet.seq}`))).toBeNull();
  });

  test('a sub-agent says beside its title what it reported using', async () => {
    const { app } = await open('nested');
    const child = rowId('child:r-222956848452');
    expect(screen.queryByTestId(`${child}.usage`)).toBeNull();
    await arrive(app, makeEvent('usage', { usage: { input_tokens: 18423, output_tokens: 1204 } }, { run_id: 'r-222956848452' }));
    expect(words(`${child}.usage`)).toBe('20k reported tokens');
    expect(screen.getByLabelText('child task, 20k reported tokens, Done')).toBeTruthy();
  });

  test('Continuity: a lost connection is one quiet line that counts the attempts; a handoff opens the other agent', async () => {
    const { app, run } = await open('echo-follow-up');
    const lost = makeEvent('error', { class: 'network', message: 'error sending request for url (https://api.openai.com/v1/responses)' }, { run_id: run });
    await arrive(app, lost, makeEvent('error', { class: 'network', message: 'Connection refused (os error 61)' }, { run_id: run }));
    const line = rowId(`cont:${lost.seq}`);
    expect(words(line)).toBe('The connection was lost; the agent keeps trying to reconnect. · 2 attempts');
    expect(screen.getByTestId(line).props.accessibilityLabel).toBe('The connection was lost; the agent keeps trying to reconnect. · 2 attempts, Could not connect');
    expect(idsOf(screen.toJSON()).filter((id) => id.startsWith(rowId('err:')))).toEqual([]);
    const handoff = makeEvent('handoff', { predecessor: run, successor: 'r-successor', reason: 'offline' }, { run_id: run });
    await arrive(app, handoff);
    expect(words(rowId(`cont:${handoff.seq}`))).toBe('The work continues in another agent.');
    await fireEvent.press(screen.getByTestId(`${rowId(`cont:${handoff.seq}`)}.link`));
    expect(router.pushed).toEqual([routes.agent('r-successor')]);
  });

  test('while the agent works the line under the rows says what it does', async () => {
    const r = recording('showcase');
    const { app } = await open('showcase', { state: stateOf(r, 10), through: 10 });
    expect(screen.getByTestId('agent.working')).toBeTruthy();
    await arrive(app, ...after(r, 10));
    expect(screen.queryByTestId('agent.working')).toBeNull();
    expect(words('agent.subtitle')).toBe("Done · Mac's default login");
  });
});

describe('a permission request', () => {
  const pending = async (scope?: 'watch') => {
    const r = recording('permission-allow');
    return open('permission-allow', { state: stateAt(r, 'permission pending'), through: 12, ...(scope ? { scope } : {}) });
  };
  const card = rowId('perm:11');

  test('says what the agent wants and on what, with Allow and Deny', async () => {
    await pending();
    expect(words(`${card}.text`)).toBe('Allow Create perm.txt?');
    expect(words(`${card}.preview`)).toBe('allowed');
    expect(words(`${card}.full`)).toContain('/perm.txt');
    expect(screen.getByLabelText('Allow once')).toBeTruthy();
    expect(screen.getByLabelText('Deny')).toBeTruthy();
    await fireEvent.press(screen.getByTestId('agent.permission.request'));
    expect(words('agent.permission.request.text')).toContain('"file_path"');
  });

  test('Allow sends one answer, shows the choice at once, then says by whom', async () => {
    const { app, r, run } = await pending();
    let taken: (value: unknown) => void = () => undefined;
    app.connection.answers['run.permission'] = () => new Promise((resolve) => (taken = resolve));
    await fireEvent.press(screen.getByTestId('agent.permission.allow'));
    expect(app.connection.calls('run.permission')).toEqual([{ run_id: run, request_id: 'req-1', allow: true }]);
    // Before the Mac answered.
    expect(words(`${card}.text`)).toBe('Allowed · Create perm.txt');
    expect(words(`${card}.mark`)).toBe('Sending');
    expect(screen.queryByTestId('agent.permission.allow')).toBeNull();
    expect(app.platform.fakes.haptics.played()).toContain('confirm');
    await act(async () => taken({ ok: true }));
    const answered = r.events.find((e) => e.kind === 'permission_answered');
    await arrive(app, { ...(answered as DaemonEvent), payload: { request_id: 'req-1', allow: true, by: 'phone:Phone' } }, ...after(r, 13));
    expect(words(`${card}.text`)).toBe('Allowed from Phone · Create perm.txt');
    expect(screen.queryByTestId(`${card}.mark`)).toBeNull();
    expect(app.connection.calls('run.permission')).toHaveLength(1);
  });

  test('Deny can carry a sentence: the field appears on Deny, then Deny sends', async () => {
    const { app, run } = await pending();
    app.connection.answers['run.permission'] = () => ({ ok: true });
    expect(screen.queryByTestId('agent.permission.message')).toBeNull();
    await fireEvent.press(screen.getByTestId('agent.permission.deny'));
    expect(app.connection.calls('run.permission')).toHaveLength(0);
    await fireEvent.changeText(screen.getByTestId('agent.permission.message'), '  Use the staging key instead.  ');
    await fireEvent.press(screen.getByTestId('agent.permission.deny'));
    await app.settle();
    expect(app.connection.calls('run.permission')).toEqual([{ run_id: run, request_id: 'req-1', allow: false, message: 'Use the staging key instead.' }]);
    expect(words(`${card}.text`)).toBe('Denied · Create perm.txt');
    expect(app.platform.fakes.haptics.played()).toContain('reject');
  });

  test('Deny without a sentence sends none', async () => {
    const { app, run } = await pending();
    app.connection.answers['run.permission'] = () => ({ ok: true });
    await fireEvent.press(screen.getByTestId('agent.permission.deny'));
    await fireEvent.press(screen.getByTestId('agent.permission.deny'));
    expect(app.connection.calls('run.permission')).toEqual([{ run_id: run, request_id: 'req-1', allow: false }]);
  });

  test('answered on the Mac first: the card updates by itself', async () => {
    const { app, r } = await pending();
    await arrive(app, ...after(r, 12).filter((e) => e.seq <= 14));
    expect(words(`${card}.text`)).toBe('Allowed on the Mac · Create perm.txt');
    expect(screen.queryByTestId('agent.permission.allow')).toBeNull();
    expect(screen.queryByTestId('agent.permission.deny')).toBeNull();
    expect(app.connection.calls('run.permission')).toHaveLength(0);
  });

  test('already answered: the card becomes what was answered first, by whom, with no error', async () => {
    const { app } = await pending();
    app.connection.answers['run.permission'] = () => {
      throw Object.assign(new Error('this request was already denied by the Mac'), { code: 'already_answered', data: { allow: false, by: 'the Mac', ts: 1 } });
    };
    await fireEvent.press(screen.getByTestId('agent.permission.allow'));
    await app.settle();
    expect(app.connection.calls('run.permission')).toHaveLength(1);
    expect(words(`${card}.text`)).toBe('Denied on the Mac · Create perm.txt');
    expect(screen.queryByTestId(`${card}.failed`)).toBeNull();
    expect(screen.queryByTestId('agent.permission.allow')).toBeNull();
    expect(screen.queryByText(/already/)).toBeNull();
  });

  test('an answer the Mac did not take leaves the request open', async () => {
    const { app } = await pending();
    app.connection.answers['run.permission'] = () => {
      throw Object.assign(new Error('run has no pending permission request'), { code: 'failed' });
    };
    await fireEvent.press(screen.getByTestId('agent.permission.allow'));
    await app.settle();
    expect(words(`${card}.failed`)).toBe('Not sent');
    expect(screen.getByTestId('agent.permission.allow')).toBeTruthy();
  });

  test('a request from the history is drawn in place and is not felt', async () => {
    const { app } = await pending();
    expect(words(`${card}.text`)).toBe('Allow Create perm.txt?');
    expect(app.platform.fakes.haptics.played()).not.toContain('warning');
  });

  test('a request that arrives live is felt', async () => {
    const r = recording('permission-allow');
    const { app } = await open('permission-allow', { state: stateAt(r, 'permission pending'), through: 10 });
    expect(screen.queryByTestId(card)).toBeNull();
    await arrive(app, ...after(r, 10).filter((e) => e.seq <= 12));
    expect(words(`${card}.text`)).toBe('Allow Create perm.txt?');
    expect(app.platform.fakes.haptics.played()).toEqual(['warning']);
  });

  test('a phone that may only watch sees the request and cannot answer it', async () => {
    await pending('watch');
    expect(words(`${card}.text`)).toBe('Allow Create perm.txt?');
    expect(screen.queryByTestId('agent.permission.allow')).toBeNull();
    expect(screen.queryByTestId('agent.permission.deny')).toBeNull();
  });
});

describe('what a watch-only phone sees', () => {
  test('everything, and no control that changes something', async () => {
    const r = recording('echo-follow-up');
    await open('echo-follow-up', { state: stateOf(r, 10), through: 9, scope: 'watch' });
    expect(screen.getByTestId('watch.line')).toBeTruthy();
    expect(screen.queryByTestId('agent.composer.text')).toBeNull();
    expect(screen.queryByTestId('agent.composer.send')).toBeNull();
    expect(screen.queryByTestId('agent.composer.stop')).toBeNull();
    // Working, so More would hold Stop only: it is not there at all.
    expect(screen.queryByTestId('agent.more')).toBeNull();
    expect(words(rowId('turn:0:user'))).toBe('Say what you received');
    expect(screen.getByTestId('agent.changes')).toBeTruthy();
  });

  test('More keeps what only shows: no Clean up, no Archive', async () => {
    await open('showcase', { scope: 'watch' });
    await fireEvent.press(screen.getByTestId('agent.more'));
    expect(screen.getByTestId('agent.more.merge')).toBeTruthy();
    expect(screen.queryByTestId('agent.more.cleanup')).toBeNull();
    expect(screen.queryByTestId('agent.more.archive')).toBeNull();
  });
});

describe('loading and errors', () => {
  test('while the history is on its way: a quiet placeholder', async () => {
    const r = recording('showcase');
    const app = await createTestApp({ state: r.final });
    let arrived: (value: unknown) => void = () => undefined;
    app.connection.answers['events.list'] = () => new Promise((resolve) => (arrived = resolve));
    router.params = { run: rootOf(r) };
    await app.render(<ConversationScreen />);
    expect(screen.getByTestId('agent.loading')).toBeTruthy();
    expect(words('agent.title')).toBe('Refresh sessions once');
    await act(async () => arrived({ events: r.events.filter((e) => e.run_id === rootOf(r)) }));
    await frames();
    expect(screen.queryByTestId('agent.loading')).toBeNull();
    expect(words(rowId('turn:0:user'))).toBe('Make expired sessions refresh once');
  });

  test('a history that could not be loaded is one quiet line with Try again', async () => {
    const r = recording('showcase');
    const app = await createTestApp({ state: r.final });
    app.connection.answers['events.list'] = () => {
      throw new Error('there is no connection to the Mac');
    };
    router.params = { run: rootOf(r) };
    await app.render(<ConversationScreen />);
    await frames();
    expect(screen.getByText('Could not load the conversation.')).toBeTruthy();
    expect(screen.queryByText(/no connection/)).toBeNull();
    // Try again asks the Mac for the history again.
    const asked = app.connection.calls('events.list').length;
    await fireEvent.press(screen.getByTestId('agent.error.retry'));
    expect(app.connection.calls('events.list').length).toBeGreaterThan(asked);
  });

  test('older messages the Mac no longer keeps are said so', async () => {
    const r = recording('showcase');
    const app = await createTestApp({ state: r.final });
    const root = rootOf(r);
    answerHistory(app.connection, [{ seq: 2, ts: 2, kind: 'retention', source: 'daemon', confidence: 'exact', payload: {}, run_id: root, task_id: null } as DaemonEvent, ...r.events.filter((e) => e.seq > 2)]);
    router.params = { run: root };
    await app.render(<ConversationScreen />);
    await frames();
    expect(words('agent.truncated')).toBe('Older messages are no longer kept on the Mac.');
  });

  test('an agent the Mac does not have says so', async () => {
    const app = await createTestApp();
    router.params = { run: 'r-gone' };
    await app.render(<ConversationScreen />);
    await frames();
    expect(screen.getByText('This agent is not on the Mac any more.')).toBeTruthy();
    expect(screen.queryByTestId('agent.composer.text')).toBeNull();
  });

  test('no agent chosen says so', async () => {
    const app = await createTestApp();
    router.params = {};
    await app.render(<ConversationScreen />);
    expect(screen.getByText('No agent was chosen.')).toBeTruthy();
    expect(app.connection.calls('events.list')).toHaveLength(0);
  });
});

/** The daemon's state of a recording when its cursor was at `cursor`. */
function stateOf(r: Recording, cursor: number): State {
  const found = r.checkpoints.find((c) => c.cursor === cursor);
  if (!found) throw new Error(`${r.scenario} has no checkpoint at ${cursor}`);
  return found.state;
}

async function wait(ms: number): Promise<void> {
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, ms));
  });
}

void bubbles;
