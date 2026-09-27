// Markdown parity where the written inputs do not reach: pieces of Markdown put together from a
// seed, in orders nobody would write, rendered by VS Code's real files and read by the phone.
import { describe, expect, it } from 'vitest';
import { parse } from '../src/markdown.ts';
import { describeMarkdown } from './helpers/describe.ts';
import { seeded } from './helpers/random.ts';
import { page, readMarkdown } from './helpers/vscode.ts';

const INLINE = ['word', 'two words', '**bold**', '*em*', '_em_', '__bold__', '`code`', '[link](https://example.com)', 'https://example.com/x', '~~gone~~', 'a_b', '*', '**', '_', '`', '\\*', '&amp;', 'x < y', '!', '[', ']', '(', ')', ' ', '  ', ',', '.', '1.', '-', '#', '>', 'snake_case', '***', '~', 'a@b.co', '&copy;', '![pic](https://example.com/p.png)', '<https://auto.example>', 'www.example.org', '"quoted"'];
const BLOCKS = ['# Heading', '## Two', 'paragraph text', '- item', '- item two', '  - nested', '    - deeper', '1. one', '2. two', '   continued', '> quote', '> more', '```', '```js', 'code line', '---', '| a | b |', '| --- | --- |', '| 1 | 2 |', '', '', '    indented', '* star', '+ plus', '- [ ] task', '- [x] done', 'Title', '=====', '3) three', '  text two spaces', '   text three spaces', '~~~', '[ref]: https://example.com/ref', '[ref]', '***'];
const INPUTS = 2500;

describe('Markdown parity with VS Code, on inputs made from a seed', () => {
  it(`${INPUTS} inputs have the same structure`, () => {
    const p = page('/Users/fixture');
    const render = (src: string): string[] => {
      const target = p.document.createElement('div');
      (p.window as unknown as { OverseerMarkdown: { render(t: HTMLElement, s: string, o: unknown): void } }).OverseerMarkdown.render(target, src, {});
      return readMarkdown(target);
    };
    const rnd = seeded(20260926);
    const pick = <T>(l: ReadonlyArray<T>): T => l[Math.floor(rnd() * l.length)] as T;
    let lines = 0;
    const different: string[] = [];
    for (let i = 0; i < INPUTS; i++) {
      const src = i % 2
        ? Array.from({ length: 2 + Math.floor(rnd() * 6) }, () => pick(INLINE)).join(rnd() < 0.5 ? ' ' : '')
        : Array.from({ length: 2 + Math.floor(rnd() * 7) }, () => (rnd() < 0.3 ? pick(BLOCKS) + ' ' + pick(INLINE) : pick(BLOCKS))).join('\n');
      const mine = describeMarkdown(parse(src, { home: '/Users/fixture' }));
      const theirs = render(src);
      lines += theirs.length;
      if (JSON.stringify(mine) !== JSON.stringify(theirs) && different.length < 10) different.push(`${JSON.stringify(src)}\n  phone:   ${JSON.stringify(mine)}\n  VS Code: ${JSON.stringify(theirs)}`);
    }
    console.log(`Markdown, made inputs: ${INPUTS} inputs, ${lines} lines compared, ${different.length} different`);
    expect(different).toEqual([]);
  }, 300_000);
});
