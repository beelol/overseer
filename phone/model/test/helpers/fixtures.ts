// The recordings of a real daemon (scripts/record.mjs) as the tests read them.
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import type { DaemonEvent, State } from '../../src/types.ts';

export interface Checkpoint {
  cursor: number;
  mark?: string;
  state: State;
}

export interface Call {
  method: string;
  params: unknown;
  result?: unknown;
  error?: { code?: string; message: string };
  /** The last event the daemon had written when it answered. */
  cursor: number;
}

export interface Fixture {
  scenario: string;
  marks: Record<string, string | number>;
  calls: Call[];
  initial: State;
  events: DaemonEvent[];
  checkpoints: Checkpoint[];
  final: State;
}

export const here = path.dirname(fileURLToPath(import.meta.url));
export const repoRoot = path.resolve(here, '../../../..');
const dir = path.resolve(here, '../fixtures');

export const fixtureNames: string[] = fs.readdirSync(dir).filter(f => f.endsWith('.json')).map(f => f.replace(/\.json$/, '')).sort();

export function fixture(name: string): Fixture {
  const recorded = JSON.parse(fs.readFileSync(path.join(dir, `${name}.json`), 'utf8')) as Fixture;
  return { ...recorded, calls: recorded.calls ?? [] };
}

export const fixtures: Fixture[] = fixtureNames.map(fixture);

/** Every moment a recording has the daemon's state for: the start, each checkpoint and the end. */
export function moments(f: Fixture): Checkpoint[] {
  return [{ cursor: f.initial.cursor, mark: 'start', state: f.initial }, ...f.checkpoints, { cursor: f.final.cursor, mark: 'end', state: f.final }];
}

/** The top-level runs of a recording, in the daemon's order. */
export function roots(f: Fixture): string[] {
  return f.final.runs.filter(r => !r.parent_run_id).map(r => r.id);
}

/** A run and everything under it, from a state. */
export function family(state: State, root: string): Set<string> {
  const ids = new Set([root]);
  for (let grew = true; grew;) {
    grew = false;
    for (const r of state.runs) if (r.parent_run_id && ids.has(r.parent_run_id) && !ids.has(r.id)) { ids.add(r.id); grew = true; }
  }
  return ids;
}

/** Where two values differ, as paths. */
export function differences(a: unknown, b: unknown, at = ''): string[] {
  if (Object.is(a, b)) return [];
  if (a === null || b === null || typeof a !== 'object' || typeof b !== 'object') return [`${at || '(value)'}: ${JSON.stringify(a)} ≠ ${JSON.stringify(b)}`];
  if (Array.isArray(a) !== Array.isArray(b)) return [`${at}: one is a list`];
  const out: string[] = [];
  if (Array.isArray(a) && Array.isArray(b)) {
    if (a.length !== b.length) out.push(`${at}.length: ${a.length} ≠ ${b.length}`);
    for (let i = 0; i < Math.min(a.length, b.length); i++) out.push(...differences(a[i], b[i], `${at}[${i}]`));
    return out;
  }
  const ra = a as Record<string, unknown>, rb = b as Record<string, unknown>;
  for (const key of new Set([...Object.keys(ra), ...Object.keys(rb)])) {
    if (!(key in ra)) out.push(`${at}.${key}: missing on the left, ${JSON.stringify(rb[key])} on the right`);
    else if (!(key in rb)) out.push(`${at}.${key}: ${JSON.stringify(ra[key])} on the left, missing on the right`);
    else out.push(...differences(ra[key], rb[key], `${at}.${key}`));
  }
  return out;
}
