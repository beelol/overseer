/**
 * What the tests of the conversation screen start from: the recorded sessions of the fixture
 * agents (phone/model/test/fixtures), a connection that keeps its outbox as the real one does,
 * and a list that has a size. Tests only; the app never loads this file.
 */
import { act } from '@testing-library/react-native';
import fs from 'node:fs';
import path from 'node:path';

import type { OutboxEntry } from '@/core';
import type { DaemonEvent } from '@/model';
import { METHOD_CLASS, type State } from '@/protocol';
import type { Connection } from '@/session';
import type { FakeConnection } from '@/testing';

export interface Recording {
  readonly scenario: string;
  readonly marks: Readonly<Record<string, string | number>>;
  readonly initial: State;
  readonly events: readonly DaemonEvent[];
  readonly checkpoints: readonly { readonly cursor: number; readonly mark?: string; readonly state: State }[];
  readonly final: State;
}

const FIXTURES = path.resolve(__dirname, '../../../model/test/fixtures');

/** A recorded session of a fixture agent, as the daemon sent it. */
export function recording(name: string): Recording {
  return JSON.parse(fs.readFileSync(path.join(FIXTURES, `${name}.json`), 'utf8')) as Recording;
}

/** The agent a recording is of. */
export function rootOf(r: Recording): string {
  return String(r.marks['root']);
}

/** The daemon's state at a checkpoint of the recording, by its mark. */
export function stateAt(r: Recording, mark: string): State {
  const found = r.checkpoints.find((c) => c.mark === mark);
  if (!found) throw new Error(`${r.scenario} has no checkpoint "${mark}"`);
  return found.state;
}

/** The Mac answers a conversation's history with the recording's events, up to `through`. */
export function answerHistory(connection: FakeConnection, events: readonly DaemonEvent[], through = Number.MAX_SAFE_INTEGER): void {
  connection.answers['events.list'] = (params: never) => {
    const p = params as { run_id?: string; after?: number; limit?: number };
    const after = p.after ?? 0;
    return { events: events.filter((e) => e.run_id === p.run_id && e.seq > after && e.seq <= through).slice(0, p.limit ?? 5000) };
  };
}

/** The events of a recording that come after `through`, as they arrive live. */
export function after(r: Recording, through: number): DaemonEvent[] {
  return r.events.filter((e) => e.seq > through);
}

/**
 * Makes the test's connection keep its outbox as `PhoneClient` does: a request that changes
 * something is in the outbox the moment it is made (sending), and stays there with its answer
 * (done) or its error (failed) until it is dismissed.
 */
export function keepOutbox(connection: FakeConnection): void {
  const ask = connection.request;
  let made = 0;
  const put = (entry: OutboxEntry): void => connection.setOutbox([...connection.entries.filter((e) => e.requestId !== entry.requestId), entry]);
  connection.request = (async (method: string, params: unknown, options?: { requestId?: string }) => {
    if ((METHOD_CLASS as Record<string, string>)[method] !== 'control') return ask(method as never, params as never, options);
    made += 1;
    const entry: OutboxEntry = { requestId: options?.requestId ?? `request-${made}`, method, params: params as OutboxEntry['params'], createdAt: made, state: 'sending', attempts: 1, firstSentAt: made };
    put(entry);
    try {
      const result = await ask(method as never, params as never, options);
      put({ ...entry, state: 'done', result: result as NonNullable<OutboxEntry['result']> });
      return result;
    } catch (error) {
      const e = error as { code?: string; message?: string; data?: NonNullable<OutboxEntry['error']>['data'] };
      put({ ...entry, state: 'failed', error: { code: e.code ?? 'failed', message: e.message ?? '', ...(e.data === undefined ? {} : { data: e.data }) } });
      throw error;
    }
  }) as Connection['request'];
}

/** The size of what the list measures under Jest, where nothing is laid out. */
export const measured = { width: 400, height: 900, row: 40 };

/**
 * For `jest.mock('@shopify/flash-list/dist/recyclerview/utils/measureLayout', …)`: the list is
 * 400 by 900 and a row is `measured.row` high, so the list builds the rows that fit.
 */
export function measuring(): Record<string, unknown> {
  const real = jest.requireActual<Record<string, unknown>>('@shopify/flash-list/dist/recyclerview/utils/measureLayout');
  return {
    ...real,
    measureParentSize: () => ({ x: 0, y: 0, width: measured.width, height: measured.height }),
    measureFirstChildLayout: () => ({ x: 0, y: 0, width: measured.width, height: measured.height }),
    measureItemLayout: () => ({ x: 0, y: 0, width: measured.width, height: measured.row }),
  };
}

/** Lets the list draw: it builds its rows over a few frames. */
export async function frames(count = 4): Promise<void> {
  for (let i = 0; i < count; i++) {
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 20));
    });
  }
}

/**
 * For a file of tests of the screen. A test may take long on a busy Mac, and one that is cut
 * short draws into the tests after it: so each gets a minute. And the session keeps a
 * conversation for half a minute after its screen closed, with a timer that would hold the run
 * open for as long: here a long timer no longer does.
 */
export function patience(): void {
  jest.setTimeout(60_000);
  const real = global.setTimeout;
  const patient = ((callback: (...args: unknown[]) => void, ms?: number, ...args: unknown[]) => {
    const timer = real(callback, ms, ...args);
    if ((ms ?? 0) >= 5_000) timer.unref();
    return timer;
  }) as unknown as typeof setTimeout;
  beforeAll(() => {
    global.setTimeout = Object.assign(patient, real);
  });
  afterAll(() => {
    global.setTimeout = real;
  });
}

/** Every test id on screen, in the order of the tree. */
export function idsOf(tree: unknown): string[] {
  const out: string[] = [];
  const walk = (node: unknown): void => {
    if (node === null || typeof node !== 'object') return;
    if (Array.isArray(node)) return node.forEach(walk);
    const n = node as { props?: Record<string, unknown>; children?: unknown };
    if (typeof n.props?.['testID'] === 'string') out.push(n.props['testID']);
    walk(n.children);
  };
  walk(tree);
  return out;
}

/** The words of everything under an element of the screen, joined. */
export function wordsOf(node: unknown): string {
  if (node === null || node === undefined) return '';
  if (typeof node === 'string') return node;
  if (typeof node === 'number') return String(node);
  if (Array.isArray(node)) return node.map(wordsOf).join('');
  if (typeof node !== 'object') return '';
  const element = node as { props?: Record<string, unknown>; children?: unknown };
  // An icon is a glyph of the icon font: decoration, not words.
  if (element.props?.['importantForAccessibility'] === 'no') return '';
  return wordsOf(element.children);
}
