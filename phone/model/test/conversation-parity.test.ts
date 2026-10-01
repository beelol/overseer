// Conversation parity with VS Code: the extension's real conversation.js in a page, and the
// phone's model, fed the same events; what a person would see must be the same, row by row.
import { describe, expect, it } from 'vitest';
import { append, create, setRun } from '../src/conversation.ts';
import type { Conversation } from '../src/conversation.ts';
import { apply, descendantsOf, load, run as runOf } from '../src/store.ts';
import type { DaemonEvent, Run, State } from '../src/types.ts';
import { describeConversation } from './helpers/describe.ts';
import { differences, family, fixtures, moments, roots } from './helpers/fixtures.ts';
import type { Fixture } from './helpers/fixtures.ts';
import { chat } from './helpers/vscode.ts';

const HOME = '/fixture';

interface Tally { compared: number; rows: number; differences: string[] }

/** VS Code's feed passes on the events of a run and of its descendants (extension/src/run-feed.js). */
function feed(root: string, known: Iterable<string>): (event: DaemonEvent) => boolean {
  const ids = new Set([root, ...known]);
  return event => {
    if (!event.run_id || !ids.has(event.run_id)) return false;
    const child = (event.payload as { child?: { id?: string } } | null)?.child?.id;
    if (event.kind === 'child' && child) ids.add(child);
    return true;
  };
}

function compare(mine: Conversation, theirs: ReturnType<typeof chat>, where: string, tally: Tally): void {
  const a = describeConversation(mine), b = theirs.read();
  tally.compared++;
  tally.rows += b.lines.length;
  for (const d of differences(a, b)) tally.differences.push(`${where}: ${d}`);
}

/** A panel opened at a moment: the run as the state has it, then its history. */
function opened(f: Fixture, root: string, state: State, cursor: number, tally: Tally, where: string): void {
  const run = state.runs.find(r => r.id === root);
  if (run === undefined) return;
  const ids = family(state, root);
  const children = state.runs.filter(r => r.id !== root && ids.has(r.id));
  const theirs = chat(HOME);
  let mine = create({ rootId: root, home: HOME });
  theirs.setRun(run, children);
  mine = setRun(mine, run, children).conversation;
  const passes = feed(root, ids);
  for (const event of f.events) {
    if (event.seq > cursor) break;
    // The phone is given every event and picks its own; VS Code is given what its feed passes on.
    mine = append(mine, event).conversation;
    if (passes(event)) theirs.add(event);
  }
  theirs.setRun(run, children);
  mine = setRun(mine, run, children).conversation;
  compare(mine, theirs, where, tally);
}

/** A panel that stays open while the agent works. `refresh` says after which events the state is read again. */
function live(f: Fixture, root: string, refresh: 'every event' | 'start and end', tally: Tally): void {
  let state = load(f.initial);
  let theirs: ReturnType<typeof chat> | undefined;
  let mine: Conversation | undefined;
  let passes = feed(root, []);
  const refreshBoth = (): void => {
    const run = runOf(state, root) as Run;
    const children = descendantsOf(state, root);
    (theirs as ReturnType<typeof chat>).setRun(run, children);
    mine = setRun(mine as Conversation, run, children).conversation;
  };
  for (const event of f.events) {
    state = apply(state, event);
    if (theirs === undefined) {
      if (runOf(state, root) === undefined) continue;
      theirs = chat(HOME);
      mine = create({ rootId: root, home: HOME });
      passes = feed(root, []);
      refreshBoth();
    }
    mine = append(mine as Conversation, event).conversation;
    if (passes(event)) theirs.add(event);
    if (refresh === 'every event') {
      refreshBoth();
      compare(mine, theirs, `after event ${event.seq} (${event.kind})`, tally);
    }
  }
  if (theirs === undefined || mine === undefined) return;
  refreshBoth();
  compare(mine, theirs, 'at the end', tally);
}

describe('conversation parity with VS Code', () => {
  const summary: string[] = [];

  for (const f of fixtures) {
    it(`${f.scenario}: the same rows as VS Code's chat`, () => {
      const tally: Tally = { compared: 0, rows: 0, differences: [] };
      for (const root of roots(f)) {
        for (const m of moments(f)) opened(f, root, m.state, m.cursor, tally, `${root} opened after event ${m.cursor}${m.mark ? ` (${m.mark})` : ''}`);
        live(f, root, 'every event', tally);
        live(f, root, 'start and end', tally);
      }
      summary.push(`${f.scenario.padEnd(20)} agents ${String(roots(f).length).padStart(2)}  conversations compared ${String(tally.compared).padStart(4)}  rows compared ${String(tally.rows).padStart(6)}  differences ${tally.differences.length}`);
      expect(tally.differences.slice(0, 20)).toEqual([]);
      expect(tally.compared).toBeGreaterThan(0);
    });
  }

  // What the phone does not draw, on purpose. VS Code draws a card for Overseer's proposals and its
  // message cards (Overseer's own conversation: on the phone that is AC-128, not built yet) and for
  // Auto's routing decisions (unfinished and off by default, AC-204), and a card while an agent waits
  // for a connection or is back online (it needs the daemon's Continuity status, which the phone
  // does not ask for yet). The phone draws nothing for them rather than a stray line; when one of
  // these reaches the phone, this is where it starts.
  it('draws nothing for what only VS Code has a card for yet, and VS Code still draws it', () => {
    const at = (seq: number, kind: string, payload: unknown, source = 'overseer'): DaemonEvent => ({ seq, ts: seq, task_id: 't-1', run_id: 'r-root', kind, source, confidence: 'exact', payload } as DaemonEvent);
    const run = { id: 'r-root', task_id: 't-1', parent_run_id: null, harness: 'claude', status: 'running', title: 'root', created_ms: 1 } as unknown as Run;
    const cards: Array<[DaemonEvent, string]> = [
      [at(3, 'proposal', { id: 'p-1', what: 'Stop the agent', state: 'open', actions: [{ kind: 'stop' }] }), '.proposal'],
      [at(4, 'overseer_message', { message: { source: 'agent', text: 'Done with the tests', card: { kind: 'done', left_out: 'docs' } } }), '.card'],
      [at(5, 'auto_decision', { chosen: { harness: 'codex' }, reasons: ['cheaper'] }), '.auto-decision, .auto'],
      [at(6, 'status', { status: 'waiting_for_connection', reason: 'offline' }, 'daemon'), '.cont-card'],
    ];
    for (const [event, drawn] of cards) {
      const theirs = chat(HOME);
      theirs.setRun(run, []);
      theirs.add(at(2, 'turn_started', { turn: { id: 'u-1', run_id: 'r-root', n: 1, prompt: 'go' } }, 'daemon'));
      theirs.add(event);
      expect(theirs.root.querySelector(drawn), `VS Code draws ${event.kind}`).not.toBeNull();
      let mine = setRun(create({ rootId: 'r-root', home: HOME }), run, []).conversation;
      mine = append(mine, at(2, 'turn_started', { turn: { id: 'u-1', run_id: 'r-root', n: 1, prompt: 'go' } }, 'daemon')).conversation;
      expect(append(mine, event).changed, `the phone draws nothing for ${event.kind}`).toEqual([]);
    }
  });

  it('prints what was compared', () => {
    console.log(['Conversation parity (the phone\'s rows against the page of the real conversation.js):', ...summary].join('\n  '));
  });
});
