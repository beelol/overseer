// Store parity: the phone's state, kept current from events alone, against the daemon's own
// `state` at every moment a recording has one.
import { describe, expect, it } from 'vitest';
import { apply, load, snapshot } from '../src/store.ts';
import type { PhoneState } from '../src/store.ts';
import type { State } from '../src/types.ts';
import { differences, fixtures, moments } from './helpers/fixtures.ts';
import type { Fixture } from './helpers/fixtures.ts';

const PARTS = ['tasks', 'runs', 'workspaces', 'profiles', 'turns'] as const;

/**
 * Times the daemon reads from its clock a moment before or after it writes the event, and does
 * not put in the event. The phone uses the event's time, so these may differ by the time between
 * the two readings. They must both be set or both be empty, and be within 50 ms.
 */
const NEAR = [/^tasks\[\d+\]\.archived_ms$/, /^workspaces\[\d+\]\.removed_ms$/, /^turns\.[^.]+\[\d+\]\.ended_ms$/, /^runs\[\d+\]\.ended_ms$/];

function compare(phone: PhoneState, daemon: State): { exact: string[]; near: string[] } {
  const mine = snapshot(phone) as unknown as Record<string, unknown>;
  const theirs = daemon as unknown as Record<string, unknown>;
  const exact: string[] = [], near: string[] = [];
  for (const part of PARTS) {
    for (const d of differences(mine[part], theirs[part], part)) {
      const [where = '', values = ''] = d.split(': ');
      const [a, b] = values.split(' ≠ ').map(Number);
      if (NEAR.some(r => r.test(where)) && a !== undefined && b !== undefined && Number.isFinite(a) && Number.isFinite(b) && Math.abs(a - b) <= 50) near.push(d);
      else exact.push(d);
    }
  }
  return { exact, near };
}

function replay(f: Fixture, from: { cursor: number; state: State }): { checked: number; exact: string[]; near: string[] } {
  let phone = load(from.state);
  const later = moments(f).filter(m => m.cursor >= from.cursor);
  const out = { checked: 0, exact: [] as string[], near: [] as string[] };
  const check = (cursor: number): void => {
    for (const m of later.filter(x => x.cursor === cursor)) {
      const r = compare(phone, m.state);
      out.checked++;
      out.exact.push(...r.exact.map(d => `after event ${cursor}${m.mark ? ` (${m.mark})` : ''}: ${d}`));
      out.near.push(...r.near.map(d => `after event ${cursor}: ${d}`));
    }
  };
  check(from.cursor);
  for (const event of f.events) {
    if (event.seq <= from.cursor) continue;
    phone = apply(phone, event);
    expect(phone.cursor).toBe(event.seq);
    check(event.seq);
  }
  return out;
}

describe('store parity with the daemon', () => {
  const summary: string[] = [];

  for (const f of fixtures) {
    it(`${f.scenario}: from the first state, every checkpoint equals the daemon's state`, () => {
      const r = replay(f, { cursor: f.initial.cursor, state: f.initial });
      summary.push(`${f.scenario.padEnd(20)} events ${String(f.events.length).padStart(3)}  states compared ${String(r.checked).padStart(2)}  differences ${r.exact.length}  times within 50 ms ${r.near.length}`);
      expect(r.exact).toEqual([]);
      expect(r.checked).toBe(moments(f).length);
    });

    it(`${f.scenario}: from every later state too`, () => {
      for (const from of f.checkpoints) {
        const r = replay(f, from);
        expect(r.exact, `loaded at event ${from.cursor}`).toEqual([]);
      }
    });
  }

  it('prints what was compared', () => {
    console.log(['Store parity (phone state from events, against the daemon\'s state):', ...summary].join('\n  '));
  });
});

describe('the reducer', () => {
  const f = fixtures.find(x => x.scenario === 'nine-agents') as Fixture;

  it('never changes the state it was given', () => {
    let phone = load(f.initial);
    const frozen = (value: unknown): void => { if (value !== null && typeof value === 'object' && !Object.isFrozen(value)) { Object.freeze(value); for (const v of Object.values(value)) frozen(v); } };
    for (const event of f.events) {
      frozen(phone);
      phone = apply(phone, event);
    }
    expect(compare(phone, f.final).exact).toEqual([]);
  });

  it('applies an event once, however often it arrives', () => {
    let phone = load(f.initial);
    for (const event of f.events) {
      phone = apply(phone, event);
      expect(apply(phone, event)).toBe(phone);
    }
    let again = phone;
    for (const event of f.events) again = apply(again, event);
    expect(again).toBe(phone);
  });

  it('keeps every record it did not change', () => {
    let phone = load(f.initial);
    for (const event of f.events) {
      const before = phone;
      phone = apply(phone, event);
      if (['output', 'tool', 'tool_result', 'usage', 'file_activity', 'error'].includes(event.kind)) {
        for (const part of ['tasks', 'runs', 'workspaces', 'profiles', 'turns', 'kids'] as const) expect(phone[part], `${event.kind} ${event.seq} ${part}`).toBe(before[part]);
      }
    }
  });

  it('ignores what it does not understand', () => {
    const phone = load(f.final);
    const odd = [
      { seq: phone.cursor + 1, ts: 1, kind: 'status', source: 'x', confidence: 'exact', payload: null, run_id: 'r-nobody' },
      { seq: phone.cursor + 2, ts: 1, kind: 'child', source: 'x', confidence: 'exact', payload: { child: 7 } },
      { seq: phone.cursor + 3, ts: 1, kind: 'a_kind_from_the_future', source: 'x', confidence: 'exact', payload: 'text' },
      { seq: phone.cursor + 4, ts: 1, kind: 'turn_started', source: 'x', confidence: 'exact', payload: { turn: {} } },
    ];
    let next = phone;
    for (const event of odd) next = apply(next, event);
    expect(next.cursor).toBe(phone.cursor + 4);
    expect(compare({ ...next, cursor: phone.cursor }, f.final).exact).toEqual([]);
  });
});
