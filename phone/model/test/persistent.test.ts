// The two structures the store and the conversation are built on.
import { describe, expect, it } from 'vitest';
import { mapDelete, mapGet, mapHas, mapSet, mapValues, pmap, pvec, vecArray, vecFrom, vecGet, vecPush, vecSet, vecSplice } from '../src/persistent.ts';
import type { PVec } from '../src/persistent.ts';
import { seeded } from './helpers/random.ts';

describe('a map that shares what it did not change', () => {
  it('holds what a Map holds, through any changes', () => {
    const rnd = seeded(3);
    let mine = pmap<number>();
    const theirs = new Map<string, number>();
    const versions: Array<{ map: typeof mine; copy: Map<string, number> }> = [];
    for (let i = 0; i < 5000; i++) {
      const key = String(['r-', 'toolu_', 'constructor', '__proto__', 'hasOwnProperty', ''][Math.floor(rnd() * 6)]) + Math.floor(rnd() * 400);
      if (rnd() < 0.7) { mine = mapSet(mine, key, i); theirs.set(key, i); } else { mine = mapDelete(mine, key); theirs.delete(key); }
      if (i % 500 === 0) versions.push({ map: mine, copy: new Map(theirs) });
    }
    expect(mine.size).toBe(theirs.size);
    for (const [k, v] of theirs) { expect(mapGet(mine, k)).toBe(v); expect(mapHas(mine, k)).toBe(true); }
    expect(mapValues(mine).sort((a, b) => a - b)).toEqual([...theirs.values()].sort((a, b) => a - b));
    expect(mapGet(mine, 'never set')).toBeUndefined();
    expect(mapGet(mine, 'toString')).toBeUndefined();
    // Every earlier version is as it was.
    for (const v of versions) { expect(v.map.size).toBe(v.copy.size); for (const [k, n] of v.copy) expect(mapGet(v.map, k)).toBe(n); }
  });

  it('gives itself back when nothing changes', () => {
    const m = mapSet(pmap<string>(), 'a', 'x');
    expect(mapSet(m, 'a', 'x')).toBe(m);
    expect(mapDelete(m, 'b')).toBe(m);
    expect(mapDelete(mapSet(m, 'b', 'y'), 'b').size).toBe(1);
  });
});

describe('a list that shares what it did not change', () => {
  const same = (v: PVec<number>, a: number[]): void => {
    expect(v.length).toBe(a.length);
    expect(vecArray(v)).toEqual(a);
    expect(v.chunks.every((c, i) => c.length === 64 || (i === v.chunks.length - 1 && c.length > 0))).toBe(true);
    for (const i of [0, 1, 63, 64, 65, a.length - 1, a.length]) expect(vecGet(v, i)).toBe(a[i]);
  };

  it('holds what an array holds, through any changes', () => {
    const rnd = seeded(11);
    let mine = pvec<number>();
    const theirs: number[] = [];
    const versions: Array<{ v: PVec<number>; copy: number[] }> = [];
    for (let i = 0; i < 3000; i++) {
      const roll = rnd();
      if (roll < 0.6 || !theirs.length) { mine = vecPush(mine, i); theirs.push(i); }
      else if (roll < 0.75) { const at = Math.floor(rnd() * theirs.length); mine = vecSet(mine, at, -i); theirs[at] = -i; }
      else {
        const at = Math.floor(rnd() * (theirs.length + 1)), remove = Math.min(theirs.length - at, Math.floor(rnd() * 5)), add = Array.from({ length: Math.floor(rnd() * 5) }, (_, k) => i * 10 + k);
        mine = vecSplice(mine, at, remove, add);
        theirs.splice(at, remove, ...add);
      }
      if (i % 300 === 0) { same(mine, theirs); versions.push({ v: mine, copy: [...theirs] }); }
    }
    same(mine, theirs);
    for (const v of versions) expect(vecArray(v.v)).toEqual(v.copy);
    expect(vecArray(mine)).toBe(vecArray(mine));
    same(vecFrom(theirs), theirs);
  });

  it('shares every piece before a change', () => {
    const v = vecFrom(Array.from({ length: 1000 }, (_, i) => i));
    const pushed = vecPush(v, 1000);
    expect(pushed.chunks.filter((c, i) => c === v.chunks[i]).length).toBe(v.chunks.length - 1);
    const set = vecSet(v, 500, -1);
    expect(set.chunks.filter((c, i) => c === v.chunks[i]).length).toBe(v.chunks.length - 1);
    expect(vecSet(v, 500, 500)).toBe(v);
    const spliced = vecSplice(v, 900, 0, [1, 2, 3]);
    expect(spliced.chunks.slice(0, 14).every((c, i) => c === v.chunks[i])).toBe(true);
    expect(vecSplice(v, 10, 0, [])).toBe(v);
    expect(() => vecSet(v, 1000, 1)).toThrow(RangeError);
    expect(() => vecSplice(v, 999, 5, [])).toThrow(RangeError);
  });
});
