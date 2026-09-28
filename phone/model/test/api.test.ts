// The exports the screens build on (API.md). A name that goes away or changes its kind fails here.
import fs from 'node:fs';
import path from 'node:path';
import { describe, expect, it } from 'vitest';
import * as model from '../src/index.ts';
import { here } from './helpers/fixtures.ts';

const COMMITTED: Record<string, string[]> = {
  store: ['EMPTY', 'load', 'apply', 'applyAll', 'snapshot', 'loadMarks', 'markStopping', 'rows', 'run', 'task', 'workspace', 'profile', 'turnsOf', 'marksOf', 'childrenOf', 'descendantsOf', 'rootOf'],
  agents: ['agentRows', 'needsYou', 'counts', 'searchLocally', 'emptyText', 'logoForHarness', 'logoForProvider', 'runHeader'],
  conversation: ['create', 'setRun', 'append', 'appendAll', 'build', 'belongs', 'rowsOf', 'rowAt', 'rowCount', 'visibleRows', 'toolDetail', 'requestText', 'permissionActions', 'markdownOf', 'describe'],
  markdown: ['parse', 'plainText', 'safeHref', 'opensExternally', 'knownEntities'],
  review: ['statusLetter', 'changedFiles', 'changesSummary', 'comparisonChoices', 'branchChoices', 'fileDiff', 'hunkKey', 'splitLine', 'editTarget', 'acceptParams'],
  pending: ['pendingRows', 'withPending', 'isWaiting', 'sentLabel', 'keyOf'],
  text: ['TEXT', 'PHONE_ONLY', 'COPIED', 'statusText', 'listStatusText', 'ago', 'agoInWords', 'duration', 'compact', 'grouped', 'basename', 'firstLine', 'shortPath'],
};
const TOP = ['ACTIVE_STATUSES', 'isActive', 'isRun', 'isTurn', 'isAttention', 'isMark', 'record', 'textOf', 'numberOf', 'number'];

describe('the exports', () => {
  it('are the ones API.md commits to', () => {
    const all = model as unknown as Record<string, Record<string, unknown>>;
    for (const [space, names] of Object.entries(COMMITTED)) {
      for (const name of names) expect(all[space]?.[name], `${space}.${name}`).toBeDefined();
      // Nothing is exported that API.md does not name.
      expect(Object.keys(all[space] ?? {}).filter(n => !names.includes(n)), `${space} exports more than API.md says`).toEqual([]);
    }
    for (const name of TOP) expect(all[name], name).toBeDefined();
    expect(Object.keys(all).sort()).toEqual([...Object.keys(COMMITTED), ...TOP].sort());
    // The words' namespace, not the helper of the same name in src/types.ts.
    expect(typeof all['text']).toBe('object');
  });

  it('are all named in API.md', () => {
    const doc = fs.readFileSync(path.resolve(here, '../../API.md'), 'utf8');
    const missing: string[] = [];
    for (const names of Object.values(COMMITTED)) for (const name of names) if (!new RegExp(`\`${name}\\b`).test(doc)) missing.push(name);
    for (const name of TOP) if (!new RegExp(`\`${name}\\b`).test(doc)) missing.push(name);
    expect(missing).toEqual([]);
  });

  it('have no runtime dependency', () => {
    const pkg = JSON.parse(fs.readFileSync(path.resolve(here, '../../package.json'), 'utf8')) as { dependencies?: Record<string, string>; name: string; type: string; private: boolean };
    expect(pkg.dependencies ?? {}).toEqual({});
    expect(pkg).toMatchObject({ name: '@overseer/phone-model', type: 'module', private: true });
    // The sources ask for nothing but each other and the generated protocol types.
    const src = path.resolve(here, '../../src');
    for (const file of fs.readdirSync(src)) {
      const asked = [...fs.readFileSync(path.join(src, file), 'utf8').matchAll(/from '([^']+)'/g)].map(m => m[1] as string);
      for (const from of asked) expect(from.startsWith('./') || from === '../../protocol/protocol.generated.ts', `${file} asks for ${from}`).toBe(true);
    }
  });
});
