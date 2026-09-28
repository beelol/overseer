// Incremental equals batch: a conversation built one event at a time, or in pieces cut anywhere,
// is the conversation built from all events at once. And what `append` says changed is what changed.
import { describe, expect, it } from 'vitest';
import { append, appendAll, build, create, rowsOf, setRun, visibleRows } from '../src/conversation.ts';
import type { Appended, Conversation, Row } from '../src/conversation.ts';
import { describeConversation } from './helpers/describe.ts';
import { family, fixtures, roots } from './helpers/fixtures.ts';
import type { Fixture } from './helpers/fixtures.ts';
import { ROOT, seeded, stream } from './helpers/random.ts';
import type { Run } from '../src/types.ts';

function start(f: Fixture, root: string): { conversation: Conversation; run: Run; children: Run[] } {
  const run = f.final.runs.find(r => r.id === root) as Run;
  const ids = family(f.final, root);
  const children = f.final.runs.filter(r => r.id !== root && ids.has(r.id));
  return { conversation: setRun(create({ rootId: root, home: '/fixture' }), run, children).conversation, run, children };
}

/** What the rows say, and nothing about how they were built. */
const shown = (c: Conversation): string => JSON.stringify({ rows: rowsOf(c), working: c.working, banner: c.banner });

/** Checks that `changed` and `movedFrom` say exactly how the list differs from the one before. */
function checkChange(before: ReadonlyArray<Row>, change: Appended, where: string): number {
  const after = rowsOf(change.conversation);
  const changed = new Set(change.changed);
  expect([...change.changed], `${where}: in rising order, each once`).toEqual([...changed].sort((a, b) => a - b));
  for (const i of changed) expect(i >= 0 && i < after.length, `${where}: ${i} is a place in the list`).toBe(true);
  const was = new Map(before.map(r => [r.key, r]));
  after.forEach((row, i) => {
    const old = was.get(row.key);
    // A row that is not said to have changed is the same object as before.
    if (!changed.has(i)) expect(old === row, `${where}: row ${i} (${row.key}) changed without being named`).toBe(true);
  });
  // Before `movedFrom`, every row sits where it sat.
  const limit = change.movedFrom < 0 ? Math.min(before.length, after.length) : change.movedFrom;
  for (let i = 0; i < limit; i++) if (!changed.has(i)) expect(after[i] === before[i], `${where}: row ${i} moved without being named`).toBe(true);
  if (change.movedFrom < 0) expect(after.length >= before.length, `${where}: rows went away without being named`).toBe(true);
  return changed.size;
}

describe('incremental equals batch', () => {
  for (const f of fixtures) {
    it(`${f.scenario}: one event at a time, in pieces, and all at once give the same conversation`, () => {
      for (const root of roots(f)) {
        const { conversation: empty, run, children } = start(f, root);
        const batch = build({ rootId: root, home: '/fixture' }, f.events, run, children);
        let one = empty;
        for (const event of f.events) {
          const before = rowsOf(one);
          const change = append(one, event);
          checkChange(before, change, `${f.scenario} event ${event.seq}`);
          one = change.conversation;
        }
        expect(shown(one)).toBe(shown(batch));
        expect(describeConversation(one)).toEqual(describeConversation(batch));
        const rnd = seeded(f.events.length * 31 + root.length);
        for (let round = 0; round < 12; round++) {
          let pieces = empty;
          for (let at = 0; at < f.events.length;) {
            const size = 1 + Math.floor(rnd() * Math.max(1, f.events.length / 3));
            const before = rowsOf(pieces);
            const change = appendAll(pieces, f.events.slice(at, at + size));
            checkChange(before, change, `${f.scenario} events ${at} to ${at + size}`);
            pieces = change.conversation;
            at += size;
          }
          expect(shown(pieces)).toBe(shown(batch));
        }
      }
    });
  }

  it('made streams: the same, with the state read again in between', () => {
    for (let seed = 1; seed <= 40; seed++) {
      const steps = stream(seed, 150);
      let one = create({ rootId: ROOT });
      let most = 0;
      for (const step of steps) {
        const before = rowsOf(one);
        const change = step.refresh ? setRun(one, step.refresh.run, step.refresh.children) : append(one, step.event!);
        most = Math.max(most, checkChange(before, change, `seed ${seed}`));
        one = change.conversation;
      }
      // All at once: the events between two readings of the state as one piece.
      let batch = create({ rootId: ROOT });
      let held = [];
      for (const step of steps) {
        if (step.event) { held.push(step.event); continue; }
        batch = appendAll(batch, held).conversation;
        held = [];
        batch = setRun(batch, step.refresh!.run, step.refresh!.children).conversation;
      }
      batch = appendAll(batch, held).conversation;
      expect(shown(one), `seed ${seed}`).toBe(shown(batch));
    }
  });

  it('leaves the conversation it was given as it was', () => {
    const f = fixtures.find(x => x.scenario === 'nested') as Fixture;
    const root = String(f.marks['root']);
    let c = start(f, root).conversation;
    const versions: Array<{ c: Conversation; shown: string; rows: ReadonlyArray<Row> }> = [];
    for (const event of f.events) {
      versions.push({ c, shown: shown(c), rows: rowsOf(c) });
      c = append(c, event).conversation;
    }
    for (const v of versions) {
      expect(shown(v.c)).toBe(v.shown);
      expect(rowsOf(v.c)).toBe(v.rows);
    }
  });

  it('an event seen before, or of another agent, changes nothing', () => {
    const f = fixtures.find(x => x.scenario === 'showcase') as Fixture;
    const root = String(f.marks['root']);
    let c = start(f, root).conversation;
    for (const event of f.events) c = append(c, event).conversation;
    for (const event of f.events) {
      const again = append(c, event);
      expect(again.conversation).toBe(c);
      expect(again.changed).toEqual([]);
    }
    const other = append(c, { seq: 9999, ts: 1, kind: 'output', source: 'harness', confidence: 'exact', run_id: 'r-another-agent', task_id: 't', payload: { role: 'assistant', text: 'not here' } });
    expect(other.conversation).toBe(c);
  });

  it('shows folds closed and children open until they are toggled', () => {
    const f = fixtures.find(x => x.scenario === 'showcase') as Fixture;
    const showcase = build({ rootId: String(f.marks['root']) }, f.events);
    const all = rowsOf(showcase);
    const fold = all.find(r => r.kind === 'steps') as Row;
    const closed = visibleRows(showcase);
    expect(closed.filter(r => r.kind === 'tool')).toHaveLength(0);
    expect(closed.filter(r => r.kind === 'edit')).toHaveLength(2);
    expect(visibleRows(showcase)).toBe(closed);
    const open = visibleRows(showcase, new Set([fold.key]));
    expect(open.filter(r => r.kind === 'tool')).toHaveLength(6);
    expect(open).toEqual(all);
    const n = fixtures.find(x => x.scenario === 'nested') as Fixture;
    const nested = build({ rootId: String(n.marks['root']) }, n.events);
    const child = rowsOf(nested).find(r => r.kind === 'child' && r.depth === 1) as Row;
    expect(visibleRows(nested)).toEqual(rowsOf(nested));
    expect(visibleRows(nested, new Set([child.key])).map(r => r.kind)).toEqual(['user', 'tool', 'child', 'message', 'footer']);
  });
});
