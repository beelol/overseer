// The words: every sentence copied from the extension is still in the file it was copied from,
// and the small functions that shape text give what the extension's give.
import fs from 'node:fs';
import path from 'node:path';
import { afterAll, beforeAll, describe, expect, it, vi } from 'vitest';
import { ago, agoInWords, basename, compact, COPIED, duration, firstLine, grouped, PHONE_ONLY, shortPath, statusText, TEXT } from '../src/text.ts';
import { repoRoot } from './helpers/fixtures.ts';
import { constant, functionSource } from './helpers/source.ts';
import { page } from './helpers/vscode.ts';

describe('the words are the extension\'s', () => {
  it('every copied sentence is in the file it was copied from', () => {
    const files = new Map<string, string>();
    const missing: string[] = [];
    for (const { text, from } of COPIED) {
      if (!files.has(from)) files.set(from, fs.readFileSync(path.join(repoRoot, from), 'utf8'));
      if (!(files.get(from) as string).includes(text)) missing.push(`${JSON.stringify(text)} is not in ${from}`);
    }
    console.log(`Words: ${COPIED.length} sentences copied from ${files.size} files of the extension and the daemon, ${missing.length} no longer there`);
    expect(missing).toEqual([]);
    expect(COPIED.length).toBeGreaterThan(150);
  });

  it('names a status as the chat and the side bar name it', () => {
    const p = page();
    const ui = (p.window as unknown as { OverseerUI: { statusText(s: unknown): string; HARNESS: Record<string, string> } }).OverseerUI;
    for (const status of ['queued', 'starting', 'running', 'waiting_for_user', 'completed', 'failed', 'interrupted', 'disconnected', 'unknown', 'waiting_for_connection', 'waiting_for_memory', 'handed_off', 'a_new_status', '', undefined, null]) expect(statusText(status), String(status)).toBe(ui.statusText(status));
    expect(TEXT.harness).toEqual(ui.HARNESS);
    expect(TEXT.listStatus).toEqual(constant('extension/src/views.js', 'STATUS_TEXT'));
    // Continuity's states (Gate L), which the chat and the side bar take from continuity-text.js.
    const states = constant('extension/media/continuity-text.js', 'STATES') as Record<string, { text: string; icon: string; active: boolean }>;
    expect(Object.fromEntries(Object.entries(TEXT.continuityStates).map(([k, v]) => [k, { ...v }]))).toEqual(Object.fromEntries(Object.entries(states).map(([k, v]) => [k, { text: v.text, icon: v.icon, active: v.active }])));
    expect(TEXT.badge).toEqual(Object.fromEntries(Object.entries(constant('extension/src/views.js', 'STATUS_BADGE')).map(([k, v]) => [k, (v as string[])[0]])));
    expect(TEXT.conversation.errorTitle).toEqual(new Function(`return ${/const TITLES = (\{[^}]*\})/.exec(fs.readFileSync(path.join(repoRoot, 'extension/media/conversation.js'), 'utf8'))?.[1]};`)());
    expect(TEXT.conversation.fromPhone).toEqual(constant('extension/media/conversation.js', 'FROM_PHONE'));
  });
});

describe('the small functions give what the extension\'s give', () => {
  const NOW = 1_790_000_000_000;
  beforeAll(() => { vi.useFakeTimers({ now: NOW, toFake: ['Date'] }); });
  afterAll(() => { vi.useRealTimers(); });

  it('how long ago, as the side bar says it and as a sentence says it', () => {
    const theirs = new Function(`${functionSource('extension/src/views.js', 'ago')}; return ago;`)() as (ms: number) => string;
    const words = new Function(`${functionSource('extension/src/phone-text.js', 'ago')}; return ago;`)() as (ms: number, now: number) => string;
    for (const s of [0, 1, 44, 45, 59, 60, 61, 119, 120, 3599, 3600, 3601, 7200, 86399, 86400, 86401, 3 * 86400, 6 * 86400 + 86399, 400 * 86400]) {
      expect(ago(NOW - s * 1000, NOW), `${s} s`).toBe(theirs(NOW - s * 1000));
      // The sentence form names a date after a week; the phone keeps counting days.
      if (s < 7 * 86400) expect(agoInWords(NOW - s * 1000, NOW), `${s} s`).toBe(words(NOW - s * 1000, NOW));
    }
    expect(ago(NOW + 5000, NOW)).toBe(theirs(NOW + 5000));
    for (const nothing of [0, null, undefined]) { expect(ago(nothing, NOW)).toBe(''); expect(agoInWords(nothing, NOW)).toBe(''); }
    expect([ago(NOW - 120_000, NOW), agoInWords(NOW - 120_000, NOW), agoInWords(NOW - 30 * 86400_000, NOW)]).toEqual(['2m', '2m ago', '30d ago']);
  });

  it('durations, numbers, paths and first lines', () => {
    const p = page('/Users/fixture');
    const ui = (p.window as unknown as { OverseerUI: { duration(n: unknown): string; compact(n: unknown): string; basename(s: unknown): string; firstLine(s: unknown, max?: number): string; shortPath(s: unknown, keep?: number): string } }).OverseerUI;
    for (const ms of [0, 1, 499, 500, 999, 59_499, 59_500, 60_000, 61_000, 3_599_000, 3_600_000, 3_661_000, 90_000_000]) expect(duration(ms), `${ms} ms`).toBe(ui.duration(ms));
    expect(duration(null)).toBe(ui.duration(null));
    expect(duration(undefined)).toBe(ui.duration(undefined));
    for (const n of [0, 1, 999, 1000, 1001, 1049, 1050, 9999, 10_000, 19_627, 999_499, 999_500, 1_000_000, 2_500_000, 12_345_678]) expect(compact(n), String(n)).toBe(ui.compact(n));
    for (const n of [0, 7, 999, 1000, 18_423, 1_234_567, 9_321, 100_000]) expect(grouped(n), String(n)).toBe(n.toLocaleString('en-US'));
    for (const s of ['/a/b/c.txt', 'c.txt', '/a/b/', '', 'a//b', '/', undefined, null, 'C:\\x\\y']) expect(basename(s), String(s)).toBe(ui.basename(s));
    for (const s of ['one line', '\n\n  \nsecond is first\nthird', '', undefined, 'x'.repeat(200), '   padded   ']) {
      expect(firstLine(s), String(s)).toBe(ui.firstLine(s));
      expect(firstLine(s, 20), String(s)).toBe(ui.firstLine(s, 20));
    }
    for (const s of ['/Users/fixture/projects/shop/src/auth/session.ts', '/Users/fixture/a.txt', '/Users/fixture/a/b', '/Users/fixture/a/b/c', '/opt/x/y/z/w.txt', '/opt/x', 'relative/path/to/file.ts', '', undefined, '/Users/fixtures/not/home/x.ts']) {
      expect(shortPath(s, '/Users/fixture'), String(s)).toBe(ui.shortPath(s));
      expect(shortPath(s, '/Users/fixture', 1), String(s)).toBe(ui.shortPath(s, 1));
    }
  });

  it('has words of its own where VS Code has none, short and plain', () => {
    const own = [PHONE_ONLY.filter.all, PHONE_ONLY.filter.active, PHONE_ONLY.filter.needs, PHONE_ONLY.sending, PHONE_ONLY.notSent, PHONE_ONLY.noMatches, PHONE_ONLY.noActive, PHONE_ONLY.nothingNeedsYou, PHONE_ONLY.noArchived, PHONE_ONLY.newFile, PHONE_ONLY.deletedFile, PHONE_ONLY.conflicted, PHONE_ONLY.notShown('a binary file'), PHONE_ONLY.renamedFrom('a.ts'), PHONE_ONLY.answeredBy('the Mac')];
    for (const words of own) { expect(words.length).toBeGreaterThan(0); expect(words.length).toBeLessThan(40); }
    expect(PHONE_ONLY.filter.needs).toBe('Needs you');
  });
});
