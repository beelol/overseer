import fs from 'node:fs';
import path from 'node:path';

import { LEARNED_SCOPES, PHONE_OWN_SCOPES } from '../learned';

const SCREENS = path.join(__dirname, '..', '..', 'screens');

function* sources(dir: string): Generator<string> {
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      if (entry.name !== '__tests__') yield* sources(full);
    } else if (/\.tsx?$/.test(entry.name)) yield full;
  }
}

/** Every storage namespace a screen opens: `keyValue.scope<...>('name')` or `useStore<...>('name')`, with one level of nesting in the type. */
function namespacesUsed(): Map<string, string[]> {
  const found = new Map<string, string[]>();
  for (const file of sources(SCREENS)) {
    const text = fs.readFileSync(file, 'utf8');
    for (const match of text.matchAll(/\b(?:scope|useStore)(?:<(?:[^<>]|<[^<>]*>)*>)?\(\s*'([^']+)'/g)) {
      const name = match[1] ?? '';
      found.set(name, [...(found.get(name) ?? []), path.relative(SCREENS, file)]);
    }
  }
  return found;
}

describe('what the screens keep of the Mac', () => {
  test('every namespace a screen stores under is emptied when the Mac is forgotten, or is the phone\'s own', () => {
    const used = namespacesUsed();
    expect(used.size).toBeGreaterThan(0);
    const unknown = [...used].filter(([name]) => !LEARNED_SCOPES.includes(name) && !PHONE_OWN_SCOPES.includes(name));
    expect(unknown.map(([name, files]) => `${name} (${files.join(', ')})`)).toEqual([]);
  });

  test('a namespace is either learned from the Mac or the phone\'s own, never both', () => {
    expect(LEARNED_SCOPES.filter((name) => PHONE_OWN_SCOPES.includes(name))).toEqual([]);
  });

  test('every learned namespace is one a screen uses', () => {
    const used = namespacesUsed();
    expect(LEARNED_SCOPES.filter((name) => !used.has(name))).toEqual([]);
  });
});
