import { review } from '@/model';
import { EDITED_HUNK, fileItems, LONGEST_LINE, placeOfHunk } from '@/screens/review/file';
import { cut, languageOf, tokenize, type Token } from '@/screens/review/syntax';
import { hunk, hunksOf } from '@/screens/review/testing';

const kinds = (tokens: readonly Token[]): string[] => tokens.filter((t) => t.kind !== 'plain').map((t) => `${t.kind}:${t.text}`);
const joined = (tokens: readonly Token[]): string => tokens.map((t) => t.text).join('');

describe('the colours of a line', () => {
  test('the language comes from the name of the file', () => {
    expect(languageOf('src/a.ts')?.name).toBe('script');
    expect(languageOf('daemon/src/review.rs')?.name).toBe('rust');
    expect(languageOf('tools/run.py')?.name).toBe('python');
    expect(languageOf('Dockerfile')?.name).toBe('config');
    expect(languageOf('docs/notes.md')).toBeNull();
    expect(languageOf('LICENSE')).toBeNull();
  });

  test.each([
    ['src/a.ts', 'const n = 42; // the answer', ['keyword:const', 'number:42', 'comment:// the answer']],
    ['src/a.ts', "import { a } from './a';", ['keyword:import', 'keyword:from', "string:'./a'"]],
    ['src/a.ts', 'const s = "a \\" b" + `c`;', ['keyword:const', 'string:"a \\" b"', 'string:`c`']],
    ['src/a.ts', 'call(/* why */ 0x1f, 1.5e3)', ['comment:/* why */', 'number:0x1f', 'number:1.5e3']],
    ['src/a.ts', ' * @param name what it is', ['comment: * @param name what it is']],
    ['src/a.ts', 'const item2 = items[2];', ['keyword:const', 'number:2']],
    ['src/lib.rs', 'pub fn key(path: &str) -> String { // same as VS Code', ['keyword:pub', 'keyword:fn', 'comment:// same as VS Code']],
    ['run.py', "def total(items):  # sum of 'all'", ['keyword:def', "comment:# sum of 'all'"]],
    ['run.py', 'colour = "#fff" if x else None', ['string:"#fff"', 'keyword:if', 'keyword:else', 'keyword:None']],
    ['schema.sql', "SELECT id FROM runs WHERE title = 'a' -- newest", ['keyword:SELECT', 'keyword:FROM', 'keyword:WHERE', "string:'a'", 'comment:-- newest']],
    ['config.yaml', 'enabled: true # for now', ['keyword:true', 'comment:# for now']],
    ['page.html', '<p>hello</p> <!-- greeting -->', ['comment:<!-- greeting -->']],
  ])('%s: %s', (path, line, expected) => {
    const tokens = tokenize(line, languageOf(path));
    expect(kinds(tokens)).toEqual(expected);
    expect(joined(tokens)).toBe(line);
  });

  test('a file of no known language is plain, and an empty line is nothing', () => {
    expect(tokenize('const x = 1;', null)).toEqual([{ kind: 'plain', text: 'const x = 1;' }]);
    expect(tokenize('', languageOf('a.ts'))).toEqual([]);
  });

  test('a string that does not end on its line runs to the end of the line', () => {
    const tokens = tokenize('const s = "open', languageOf('a.ts'));
    expect(kinds(tokens)).toEqual(['keyword:const', 'string:"open']);
  });

  test('the tokens are cut where a long line is broken, and nothing is lost', () => {
    const line = 'const message = "a long sentence that does not fit across"; // said once';
    const pieces = review.splitLine(line, 24);
    const rows = cut(tokenize(line, languageOf('a.ts')), pieces);
    expect(rows).toHaveLength(pieces.length);
    expect(rows.map(joined)).toEqual(pieces);
    // The string keeps its colour on every piece it runs over.
    expect(rows.flat().filter((t) => t.kind === 'string').map((t) => t.text).join('')).toBe('"a long sentence that does not fit across"');
  });
});

describe('the rows of a file', () => {
  const path = 'src/a.ts';
  const first = hunk(path, 3, ['a'], ['b', 'c']);
  const second = hunk(path, 40, [], ['\tindented', `x${'y'.repeat(LONGEST_LINE + 500)}`]);
  const diff = review.fileDiff(hunksOf(path, 'base', [first, second]));

  test('a heading for each hunk, then its removed and its added lines', () => {
    const list = fileItems(diff, new Set());
    expect(list.items.map((item) => item.key)).toEqual([`hunk:${first.key}`, `${first.key}:-0`, `${first.key}:+0`, `${first.key}:+1`, `hunk:${second.key}`, `${second.key}:+0`, `${second.key}:+1`]);
    expect(list.headings).toEqual([0, 4]);
    expect(list.starts.get(second.key)).toBe(4);
    expect(list.digits).toBe(2);
    expect(list.language?.name).toBe('script');
  });

  test('a tab is two spaces, and a very long line shows its start and says how much more there is', () => {
    const list = fileItems(diff, new Set());
    const indented = list.items[5];
    const long = list.items[6];
    expect(indented?.kind === 'line' ? indented.text : null).toBe('  indented');
    expect(long?.kind === 'line' ? [long.text.length, long.more] : null).toEqual([LONGEST_LINE, 501]);
    expect(list.longest).toBe(LONGEST_LINE);
  });

  test('a hunk that is being put back is not in the list', () => {
    const list = fileItems(diff, new Set([first.key]));
    expect(list.items.map((item) => item.key)).toEqual([`hunk:${second.key}`, `${second.key}:+0`, `${second.key}:+1`]);
  });

  test('a route names a hunk by its key or by a line', () => {
    const list = fileItems(diff, new Set());
    expect(placeOfHunk(diff, list, undefined)).toBe(0);
    expect(placeOfHunk(diff, list, second.key)).toBe(4);
    expect(placeOfHunk(diff, list, '4')).toBe(0);
    expect(placeOfHunk(diff, list, '20')).toBe(4);
    expect(placeOfHunk(diff, list, '41')).toBe(4);
    expect(placeOfHunk(diff, list, '900')).toBe(0);
    expect(placeOfHunk(diff, list, 'not-a-hunk')).toBe(0);
  });

  test('an edit of the conversation opens at the first hunk that is not reviewed', () => {
    const list = fileItems(diff, new Set());
    expect(placeOfHunk(diff, list, EDITED_HUNK)).toBe(0);
    const later = review.fileDiff(hunksOf(path, 'base', [first, second]), [first.key]);
    expect(placeOfHunk(later, fileItems(later, new Set()), EDITED_HUNK)).toBe(4);
  });
});
