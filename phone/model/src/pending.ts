// Messages on their way: what the owner typed and the daemon has not started as a turn yet.
//
// The connection library keeps them in its outbox (queued while there is no connection, sending
// once it goes out). Here each shows as a bubble at the end of the conversation, marked "Queued"
// or "Sending". When the daemon starts the turn, its event carries the request's id (the
// `remote_command` event just before `turn_started`), and the turn's own bubble takes the same
// key: one row from the moment it was typed, never two.

import { visibleRows } from './conversation.ts';
import type { Conversation, Row, UserRow } from './conversation.ts';
import { mapGet, vecGet } from './persistent.ts';
import { PHONE_ONLY, TEXT } from './text.ts';
import { record } from './types.ts';
import type { OutboxEntry } from './types.ts';

/** The key of a message's row, before and after it became a turn. */
export const keyOf = (requestId: string): string => `sent:${requestId}`;

function messageOf(entry: OutboxEntry): { run: string; text: string } | undefined {
  if (entry.method !== 'run.follow_up') return undefined;
  const p = record(entry.params);
  return typeof p['run_id'] === 'string' && typeof p['prompt'] === 'string' ? { run: p['run_id'], text: p['prompt'] } : undefined;
}

/** True while an entry still shows as its own bubble. */
export function isWaiting(conversation: Conversation, entry: OutboxEntry): boolean {
  const message = messageOf(entry);
  if (message === undefined || message.run !== conversation.rootId) return false;
  if (mapGet(conversation.landed, entry.requestId) !== undefined) return false;
  if (entry.state !== 'done') return true;
  // Accepted by the daemon: its turn is in the stream, or about to be. Without the event that
  // names the request (older history), the turn is known by its words.
  for (let i = conversation.rows.length - 1; i >= 0; i--) {
    const row = vecGet(conversation.rows, i) as Row;
    if (row.kind === 'user' && row.requestId === null && row.text === message.text) return false;
  }
  return true;
}

/** A bubble for each message to this agent that has not become a turn yet, oldest first. */
export function pendingRows(conversation: Conversation, outbox: ReadonlyArray<OutboxEntry>): ReadonlyArray<UserRow> {
  const out: UserRow[] = [];
  const turn = conversation.turns.length;
  for (const entry of [...outbox].sort((a, b) => a.createdAt - b.createdAt)) {
    if (!isWaiting(conversation, entry)) continue;
    const message = messageOf(entry) as { run: string; text: string };
    if (!message.text || out.some(r => r.requestId === entry.requestId)) continue;
    out.push({
      kind: 'user', key: keyOf(entry.requestId), depth: 0, turn, turnNumber: '', run: conversation.rootId, parent: 'pending', seq: 0,
      label: TEXT.conversation.you, text: message.text, sent: entry.state === 'queued' ? 'queued' : entry.state === 'failed' ? 'failed' : 'sending', requestId: entry.requestId,
    });
  }
  return out;
}

const joined = new WeakMap<ReadonlyArray<Row>, WeakMap<ReadonlyArray<OutboxEntry>, ReadonlyArray<Row>>>();

/** The visible rows, then the bubbles on their way. The same array until the rows or the outbox change. */
export function withPending(conversation: Conversation, outbox: ReadonlyArray<OutboxEntry>, toggled?: ReadonlySet<string>): ReadonlyArray<Row> {
  const shown = visibleRows(conversation, toggled);
  let byOutbox = joined.get(shown);
  if (byOutbox === undefined) joined.set(shown, byOutbox = new WeakMap());
  const known = byOutbox.get(outbox);
  if (known !== undefined) return known;
  const waiting = pendingRows(conversation, outbox);
  const out = waiting.length ? [...shown, ...waiting] : shown;
  byOutbox.set(outbox, out);
  return out;
}

/** "Queued", "Sending", "Not sent", or nothing for a message that was sent. */
export function sentLabel(row: UserRow): string {
  return row.sent === 'queued' ? TEXT.chat.queued : row.sent === 'sending' ? PHONE_ONLY.sending : row.sent === 'failed' ? PHONE_ONLY.notSent : '';
}
