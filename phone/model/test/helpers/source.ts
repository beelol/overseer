// Pieces of the extension's source, taken from the files as they stand, for the parts of VS Code
// that cannot be loaded whole outside VS Code (they need its API, its Git extension or Monaco).
import fs from 'node:fs';
import path from 'node:path';
import { repoRoot } from './fixtures.ts';

const read = (file: string): string => fs.readFileSync(path.join(repoRoot, file), 'utf8');

/** From `from` in the text to the bracket that closes the first one opened after it. */
function balanced(source: string, from: number, open: string, close: string): string {
  let depth = 0;
  for (let i = source.indexOf(open, from); i >= 0 && i < source.length; i++) {
    const ch = source[i];
    if (ch === '\'' || ch === '"' || ch === '`') {
      // Skip what is written in quotes.
      for (i++; i < source.length && source[i] !== ch; i++) if (source[i] === '\\') i++;
      continue;
    }
    if (ch === open) depth++;
    else if (ch === close && --depth === 0) return source.slice(from, i + 1);
  }
  throw new Error('the source does not close what it opens');
}

/** The source of a function declared in a file, from its name to its closing brace. */
export function functionSource(file: string, name: string): string {
  const source = read(file);
  const start = source.indexOf(`function ${name}(`);
  if (start < 0) throw new Error(`${file} has no function ${name}`);
  return balanced(source, start, '{', '}');
}

/** The object written after `const NAME =` in a file. */
export function constant(file: string, name: string): Record<string, unknown> {
  const source = read(file);
  const start = source.indexOf(`const ${name} = {`);
  if (start < 0) throw new Error(`${file} has no ${name}`);
  return new Function(`return ${balanced(source, start + `const ${name} = `.length, '{', '}')};`)() as Record<string, unknown>;
}

/** What is written in the parentheses that follow `before` in a file. */
export function argument(file: string, before: string): string {
  const source = read(file);
  const start = source.indexOf(before);
  if (start < 0) throw new Error(`${file} has no ${before}`);
  return balanced(source, start + before.length - 1, '(', ')').slice(1, -1);
}
