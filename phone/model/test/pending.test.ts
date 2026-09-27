// Messages on their way: queued, sending, then the turn the daemon starts. One bubble throughout.
import { describe, expect, it } from 'vitest';
import { append, create, rowsOf, setRun } from '../src/conversation.ts';
import type { Conversation, Row, UserRow } from '../src/conversation.ts';
import { isWaiting, keyOf, pendingRows, sentLabel, withPending } from '../src/pending.ts';
import type { DaemonEvent, OutboxEntry, Run } from '../src/types.ts';
import { fixture } from './helpers/fixtures.ts';
import { chat } from './helpers/vscode.ts';
import { describeConversation } from './helpers/describe.ts';

const f = fixture('echo-follow-up');
const root = String(f.marks['root']);
const run = f.final.runs.find(r => r.id === root) as Run;
const second = f.events.filter(e => e.kind === 'turn_started')[1] as DaemonEvent;
const PROMPT = (second.payload as { turn: { prompt: string } }).turn.prompt;
const REQUEST = 'c0ffee00-1234-4abc-8def-000000000001';

/**
 * The recording's follow-up was sent from the Mac. From a phone the daemon writes one event more,
 * just before the turn: `remote_command`, with the request's id and the phone as its source
 * (daemon/src/gateway/remote.rs). It is put in here, shaped as the daemon writes it.
 */
const fromPhone: DaemonEvent[] = [
  ...f.events.filter(e => e.seq < second.seq),
  { seq: second.seq, ts: second.ts - 1, task_id: null, run_id: root, kind: 'remote_command', source: "phone:Bilal's iPhone", confidence: 'exact', payload: { method: 'run.follow_up', device: 'd-1', request_id: REQUEST } },
  ...f.events.filter(e => e.seq >= second.seq).map(e => ({ ...e, seq: e.seq + 1 })),
];

const entry = (state: OutboxEntry['state'], more: Partial<OutboxEntry> = {}): OutboxEntry => ({ requestId: REQUEST, method: 'run.follow_up', params: { run_id: root, prompt: PROMPT }, state, createdAt: 1000, ...more });
const bubbles = (rows: ReadonlyArray<Row>): UserRow[] => rows.filter((r): r is UserRow => r.kind === 'user');

function upTo(events: DaemonEvent[], seq: number): Conversation {
  let c = setRun(create({ rootId: root }), run, []).conversation;
  for (const e of events) if (e.seq <= seq) c = append(c, e).conversation;
  return c;
}

describe('messages on their way', () => {
  it('queued, sending, sent: one bubble with one key throughout', () => {
    const seen: Array<{ keys: string[]; sent: string; label: string }> = [];
    const look = (c: Conversation, outbox: OutboxEntry[]): void => {
      const mine = bubbles(withPending(c, outbox)).filter(b => b.text === PROMPT);
      seen.push({ keys: mine.map(b => b.key), sent: mine.map(b => b.sent).join(), label: mine.map(sentLabel).join() });
    };
    const before = upTo(fromPhone, second.seq - 1);
    look(before, [entry('queued')]);
    look(before, [entry('sending')]);
    // The daemon took the request: its event arrives, the answer has not.
    look(upTo(fromPhone, second.seq), [entry('sending')]);
    // The turn starts. The answer may come before or after it.
    look(upTo(fromPhone, second.seq + 1), [entry('sending')]);
    look(upTo(fromPhone, second.seq + 1), [entry('done')]);
    look(upTo(fromPhone, second.seq + 1), []);
    const end = upTo(fromPhone, Infinity);
    look(end, [entry('done')]);
    expect(seen.map(s => s.keys)).toEqual(Array.from({ length: 7 }, () => [keyOf(REQUEST)]));
    expect(seen.map(s => s.sent)).toEqual(['queued', 'sending', 'sending', 'sent', 'sent', 'sent', 'sent']);
    expect(seen.map(s => s.label)).toEqual(['Queued', 'Sending', 'Sending', '', '', '', '']);
    expect(bubbles(rowsOf(end)).map(b => [b.text, b.requestId])).toEqual([['Say what you received', null], [PROMPT, REQUEST]]);
  });

  it('the answer before the event: the bubble stays until the turn is there', () => {
    const before = upTo(fromPhone, second.seq - 1);
    const waiting = bubbles(withPending(before, [entry('done')]));
    expect(waiting.map(b => [b.text, b.sent])).toEqual([['Say what you received', 'sent'], [PROMPT, 'sending']]);
  });

  it('a turn known only by its words is not shown twice', () => {
    // History without the event that names the request.
    const end = upTo(f.events, Infinity);
    expect(bubbles(withPending(end, [entry('done')])).filter(b => b.text === PROMPT)).toHaveLength(1);
    expect(isWaiting(end, entry('done'))).toBe(false);
    // Not yet accepted by the daemon: another message with the same words is on its way.
    expect(bubbles(withPending(end, [entry('queued', { requestId: 'another-request-1' })])).filter(b => b.text === PROMPT)).toHaveLength(2);
  });

  it('a message that could not be sent says so', () => {
    const before = upTo(fromPhone, second.seq - 1);
    const [row] = pendingRows(before, [entry('failed')]);
    expect(row).toMatchObject({ kind: 'user', sent: 'failed', text: PROMPT, depth: 0, run: root });
    expect(sentLabel(row as UserRow)).toBe('Not sent');
  });

  it('shows messages to this agent only, oldest first, each once', () => {
    const before = upTo(fromPhone, second.seq - 1);
    const outbox: OutboxEntry[] = [
      entry('queued', { requestId: 'request-b', params: { run_id: root, prompt: 'second' }, createdAt: 2000 }),
      entry('sending', { requestId: 'request-a', params: { run_id: root, prompt: 'first' }, createdAt: 1000 }),
      entry('queued', { requestId: 'request-a', params: { run_id: root, prompt: 'first' }, createdAt: 1000 }),
      entry('queued', { requestId: 'request-c', params: { run_id: 'r-another', prompt: 'for another agent' } }),
      entry('queued', { requestId: 'request-d', method: 'run.interrupt', params: { run_id: root } }),
      entry('queued', { requestId: 'request-e', method: 'task.create', params: { repo: '/r', harness: 'claude', prompt: 'a new agent' } }),
      entry('queued', { requestId: 'request-f', params: { run_id: root, prompt: '' } }),
      entry('queued', { requestId: 'request-g', params: 'not a record' }),
    ];
    expect(pendingRows(before, outbox).map(r => [r.text, r.sent, r.key])).toEqual([['first', 'sending', 'sent:request-a'], ['second', 'queued', 'sent:request-b']]);
    const rows = withPending(before, outbox);
    expect(new Set(rows.map(r => r.key)).size).toBe(rows.length);
    expect(rows.slice(-2).map(r => r.kind)).toEqual(['user', 'user']);
  });

  it('gives the same list until the rows or the outbox change', () => {
    const c = upTo(fromPhone, second.seq - 1);
    const outbox = [entry('queued')];
    expect(withPending(c, outbox)).toBe(withPending(c, outbox));
    const none: OutboxEntry[] = [];
    expect(withPending(c, none)).toBe(withPending(c, none));
    expect(withPending(c, [entry('sending')])).not.toBe(withPending(c, outbox));
  });

  it('shows what VS Code shows for a message sent from a phone', () => {
    const theirs = chat();
    theirs.setRun(run, []);
    let mine = setRun(create({ rootId: root }), run, []).conversation;
    // VS Code's feed passes on the agent's own events; the phone picks them itself.
    for (const e of fromPhone) { if (e.run_id === root) theirs.add(e); mine = append(mine, e).conversation; }
    expect(describeConversation(mine)).toEqual(theirs.read());
    expect(describeConversation(mine).lines.filter(l => l.kind === 'note').map(l => l['text'])).toEqual(["Message from Bilal's iPhone"]);
  });
});
