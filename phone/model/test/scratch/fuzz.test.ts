import { it } from 'vitest';
import { parse } from '../../src/markdown.ts';
import { describeMarkdown } from '../helpers/describe.ts';
import { seeded } from '../helpers/random.ts';
import { page, readMarkdown } from '../helpers/vscode.ts';

const INLINE = ['word', 'two words', '**bold**', '*em*', '_em_', '__bold__', '`code`', '[link](https://example.com)', 'https://example.com/x', '~~gone~~', 'a_b', '*', '**', '_', '`', '\\*', '&amp;', '<b>', 'x < y', '!', '[', ']', '(', ')', ' ', '  ', ',', '.', '1.', '-', '#', '>', 'snake_case', '***', '~', 'a@b.co'];
const BLOCKS = ['# Heading', '## Two', 'paragraph text', '- item', '- item two', '  - nested', '    - deeper', '1. one', '2. two', '   continued', '> quote', '> more', '```', '```js', 'code line', '---', '| a | b |', '| --- | --- |', '| 1 | 2 |', '', '', '    indented', '* star', '+ plus', '- [ ] task', '- [x] done', 'Title', '=====', '3) three', '  text two spaces', '   text three spaces', '~~~'];

it('fuzz', () => {
  const p = page('/Users/fixture');
  const render = (src: string): string[] => {
    const target = p.document.createElement('div');
    (p.window as unknown as { OverseerMarkdown: { render(t: HTMLElement, s: string, o: unknown): void } }).OverseerMarkdown.render(target, src, {});
    return readMarkdown(target);
  };
  const rnd = seeded(7);
  const pick = <T>(l: ReadonlyArray<T>): T => l[Math.floor(rnd() * l.length)] as T;
  let bad = 0, total = 0;
  const shown: string[] = [];
  for (let i = 0; i < 3000; i++) {
    let src: string;
    if (i % 2) src = Array.from({ length: 2 + Math.floor(rnd() * 6) }, () => pick(INLINE)).join(rnd() < 0.5 ? ' ' : '');
    else src = Array.from({ length: 2 + Math.floor(rnd() * 7) }, () => (rnd() < 0.3 ? pick(BLOCKS) + ' ' + pick(INLINE) : pick(BLOCKS))).join('\n');
    total++;
    const mine = JSON.stringify(describeMarkdown(parse(src, { home: '/Users/fixture' })));
    const theirs = JSON.stringify(render(src));
    if (mine !== theirs) { bad++; if (shown.length < 40) shown.push(`${i % 2 ? 'INLINE' : 'BLOCK'} ${JSON.stringify(src)}\n   phone   ${mine}\n   vscode  ${theirs}`); }
  }
  console.log(`fuzz: ${bad} of ${total} differ\n` + shown.join('\n'));
});
