// Plain words: the phone's port against the extension's real plain-words.js, on every raw text the
// recordings hold (errors, reasons, summaries, exit reasons) and on texts made for each of its rules.
import { createRequire } from 'node:module';
import path from 'node:path';
import { describe, expect, it } from 'vitest';
import { harnessName, plain, plainTool } from '../src/plain.ts';
import { fixtures, repoRoot } from './helpers/fixtures.ts';

const theirs = createRequire(import.meta.url)(path.join(repoRoot, 'extension/media/plain-words.js')) as { plain(t: unknown, max?: number): string; tool(n: unknown): string; harness(id: unknown): string };

/** Every string of the recordings that the chat or the list passes through plain words. */
function recorded(): string[] {
  const out = new Set<string>();
  for (const f of fixtures) {
    for (const r of f.final.runs) if (r.exit_reason) out.add(r.exit_reason);
    for (const e of f.events) {
      const p = (e.payload ?? {}) as Record<string, unknown>;
      for (const key of ['message', 'reason', 'summary']) if (typeof p[key] === 'string') out.add(p[key] as string);
    }
  }
  return [...out];
}

const MADE = [
  '', '   ', null, undefined, 42, 'NOT_FOR_OVERSEER', 'NOT_FOR_OVERSEER.', 'Done: all tests pass', '[rate_limit] slow down', 'rate-limited by the provider', 'HTTP 429 Too Many Requests', 'quota exceeded for this month',
  'turn reported failure; last error: boom', 'API Error: bad request', 'error sending request for url (https://api.example.com/v1)', 'merge_back failed: conflicts in 2 files', 'frobnicate failed: nope',
  'Error: something broke', 'panicked at src/main.rs:12:5', 'fatal! it stopped', 'boom\nCaused by: deeper\n  0: frames', 'thrown at src/a.ts:10:3 and more', 'bad {"code": 7, "why": "x"} payload',
  'Connection Failed: Connect error: timed out', 'read failed (os error 2)', 'Connection refused', 'ECONNREFUSED 127.0.0.1:4000', 'ENOENT: no such file', 'see http://127.0.0.1:8080/x for more', 'got HTTP 502 from upstream', 'status (503) again',
  'agent (r-0123456789ab) stopped', 'r-0123456789abcdef went away', 'p-abcdef012345 and w-0123abcd9876', 'called mcp__linear__get_issue then mcp__overseer__propose', 'the codex harness exited', 'the codex-app harness exited',
  'claude and codex and opencode-serve and codex-app', 'path /usr/bin/claude stays', 'user@claude stays', 'SOME_CONSTANT_NAME happened', 'state waiting_for_user now', 'handed_off and cancel_requested',
  'ramCeilingPercent too low', 'qwen3-coder is loading', 'x'.repeat(700), 'a sentence that is long '.repeat(20), ': ; leading punctuation', 'spaces   inside  ,  here .', 'empty () parentheses',
];

describe('plain words are the extension\'s', () => {
  it('say every recorded text as plain-words.js says it', () => {
    const texts = [...recorded(), ...MADE];
    const different: string[] = [];
    for (const t of texts) for (const max of [undefined, 160, 200, 400, 600]) {
      const a = max === undefined ? plain(t) : plain(t, max), b = max === undefined ? theirs.plain(t) : theirs.plain(t, max);
      if (a !== b) different.push(`${JSON.stringify(t)} (${max}): ${JSON.stringify(a)} ≠ ${JSON.stringify(b)}`);
    }
    console.log(`Plain words: ${texts.length} texts compared with the real plain-words.js, ${different.length} different`);
    expect(different).toEqual([]);
  });

  it('name tools and harnesses as plain-words.js names them', () => {
    for (const n of ['mcp__linear__get_issue', 'mcp__overseer__propose', 'mcp__my-server_x__do_it', 'mcp__a__b__c', 'Read', '', null, undefined, 'mcp__only']) expect(plainTool(n), String(n)).toBe(theirs.tool(n));
    for (const h of ['claude', 'codex', 'codex-app', 'opencode', 'opencode-serve', 'generic', 'devin', '', null, undefined]) expect(harnessName(h), String(h)).toBe(theirs.harness(h));
  });
});
