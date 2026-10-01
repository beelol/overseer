// Review: the changed-files list against the tree VS Code's review draws, the comparison choices
// against the ones its picker lists, the keys of hunks against the daemon's and VS Code's, and the
// reviewed marks against what the daemon answers.
//
// VS Code's review cannot be loaded whole outside VS Code (it needs Monaco and the Git extension).
// The functions compared are taken from its files as they stand and run as they are: the
// navigator's tree (`renderTree` and what it calls) and `hunkHash` of
// extension/branch-diff/review/browser.js in a page, and `statusLetter`, `STATUS` and the picker's
// items of extension/src/review.js.
//
// VS Code's navigator lists what its view shows (AC-264): Changed in Diffs, the files the agent
// changed, and All files in Follow, the whole worktree a folder at a time. The phone's review is the
// changed files (AC-126: "the changed files with status and counts"); browsing every file of the
// worktree is not part of the phone's gate, so the tree is compared as Diffs lists it (Changed).
import { JSDOM } from 'jsdom';
import { describe, expect, it } from 'vitest';
import { acceptParams, branchChoices, changedFiles, changesSummary, comparisonChoices, editTarget, fileDiff, hunkKey, splitLine, statusLetter } from '../src/review.ts';
import type { FileRow } from '../src/review.ts';
import { apply, load, loadMarks, marksOf } from '../src/store.ts';
import { TEXT } from '../src/text.ts';
import type { Change, Comparison, Hunk, Mark } from '../src/types.ts';
import { differences, fixture } from './helpers/fixtures.ts';
import type { Call } from './helpers/fixtures.ts';
import { argument, constant, functionSource } from './helpers/source.ts';

const BROWSER = 'extension/branch-diff/review/browser.js';
const REVIEW = 'extension/src/review.js';
const recorded = fixture('review');
const calls = (method: string): Call[] => recorded.calls.filter(c => c.method === method);
const base = String(recorded.marks['base']);

/** VS Code's letter for a change of the daemon: its STATUS table, then its statusLetter. */
const STATUS = constant(REVIEW, 'STATUS') as Record<string, number>;
const theirLetter = new Function(`${functionSource(REVIEW, 'statusLetter')}; return statusLetter;`)() as (n: number) => string;
const theirHash = new Function(`${functionSource(BROWSER, 'hunkHash')}; return hunkHash;`)() as (text: string) => string;

interface TreeRow { kind: 'folder' | 'file'; depth: number; name: string; key: string; status: string | null; tooltip: string | null; accessibilityLabel: string | null; icon: string | null; expanded: boolean }

/** The tree VS Code's review draws for these entries, read from the page. */
function theirTree(entries: Array<{ id: string; path: string; status: string; conflicted?: boolean }>, query = '', closed: string[] = []): TreeRow[] {
  const dom = new JSDOM('<!doctype html><body><span id="list-title"></span><div id="navigator"><div id="tree"></div></div></body>');
  const document = dom.window.document;
  const TREE = ['node', 'changedEntries', 'changesOnly', 'countsText', 'fileButton', 'openPath', 'populateChanges', 'renderTree'];
  const draw = new Function('document', 'snapshot', 'filter', 'closedFolders', 'selected', 'jump', 'persist', 'tree', 'view', 'rows', 'listTitle', 'vscode',
    `${TREE.map(name => functionSource(BROWSER, name)).join('\n')}\nrenderTree();`);
  const tree = document.getElementById('tree') as HTMLElement;
  // No diff is drawn yet, so no row has its counts: the tree lists what changed, with its letter.
  draw(document, { entries }, { value: query }, new Set(closed), undefined, () => {}, () => {}, tree, 'diffs', new Map(), document.getElementById('list-title'), { postMessage: () => {} });
  if (document.body.dataset['nav'] !== 'changes') throw new Error('the review is not listing Changed');
  const out: TreeRow[] = [];
  const walk = (parent: Element, depth: number): void => {
    for (const el of parent.children) {
      if (el.matches('details.folder')) {
        const summary = el.querySelector(':scope > summary') as HTMLElement;
        const open = (el as HTMLDetailsElement).open;
        out.push({ kind: 'folder', depth, name: summary.textContent ?? '', key: summary.title, status: null, tooltip: summary.title, accessibilityLabel: null, icon: null, expanded: open });
        // A closed folder shows nothing of what it holds.
        if (open) walk(el.querySelector(':scope > .folder-children') as HTMLElement, depth + 1);
      } else if (el.matches('button.file')) {
        const icon = [...(el.querySelector('.codicon')?.classList ?? [])].find(c => c.startsWith('codicon-'))?.slice(8) ?? null;
        out.push({ kind: 'file', depth, name: el.querySelector('.file-name')?.textContent ?? '', key: (el as HTMLElement).dataset['id'] ?? '', status: el.querySelector('.status')?.textContent ?? null, tooltip: el.getAttribute('title'), accessibilityLabel: el.getAttribute('aria-label'), icon, expanded: true });
      }
    }
  };
  walk(tree, 0);
  return out;
}

const mine = (rows: ReadonlyArray<FileRow>): TreeRow[] => rows.map(r => ({ kind: r.kind, depth: r.depth, name: r.name, key: r.key, status: r.status, tooltip: r.tooltip, accessibilityLabel: r.kind === 'file' ? r.accessibilityLabel : null, icon: r.kind === 'file' ? r.icon : null, expanded: r.expanded }));
const entriesOf = (changes: ReadonlyArray<Change>, conflicted: string[] = []): Array<{ id: string; path: string; status: string; conflicted: boolean }> => [
  ...changes.map(c => ({ id: c.path, path: c.path, status: theirLetter(STATUS[c.status] ?? 5), conflicted: conflicted.includes(c.path) })),
  ...conflicted.filter(p => !changes.some(c => c.path === p)).map(p => ({ id: p, path: p, status: theirLetter(STATUS['U'] as number), conflicted: true })),
];

describe('review: the changed files', () => {
  const diffs = calls('workspace.diff').map(c => (c.result as { changes: Change[] }).changes);
  const made: Change[] = [
    { status: 'M', path: 'README.md' }, { status: 'A', path: 'src/auth/session.ts' }, { status: 'D', path: 'src/auth/old/legacy.ts' }, { status: 'R', path: 'src/ui/Button.tsx', old_path: 'src/ui/button.tsx' },
    { status: 'T', path: 'bin/run' }, { status: 'C', path: 'src/auth/copy.ts', old_path: 'src/auth/session.ts' }, { status: 'M', path: 'docs/guides/deep/er/file.md' }, { status: 'M', path: 'src/ui/Card.tsx' }, { status: 'M', path: 'a b/c d.txt' },
  ];

  it('gives every change the letter VS Code gives it', () => {
    for (const status of ['A', 'D', 'R', 'M', 'T', 'C', 'U', 'X', '']) expect(statusLetter(status), status).toBe(theirLetter(STATUS[status] ?? 5));
  });

  it('lists the recorded changes as VS Code\'s tree lists them', () => {
    expect(diffs.length).toBe(2);
    let rows = 0;
    for (const changes of diffs) {
      const theirs = theirTree(entriesOf(changes));
      rows += theirs.length;
      expect(differences(mine(changedFiles(changes)), theirs)).toEqual([]);
    }
    console.log(`Review, changed files of the recording: ${diffs.length} lists, ${rows} rows compared with the tree of the real browser.js, 0 differences`);
    expect(changedFiles(diffs[0] as Change[]).map(r => `${'  '.repeat(r.depth)}${r.name}${r.status ? ' ' + r.status : ''}`)).toEqual([
      'docs', '  guides', '    sessions.md M', '  old.md D', 'src', '  auth', '    session-refresh-coordinator.ts A', 'README.md M', 'a.txt M', 'picture.bin A',
    ]);
  });

  it('lists made changes the same: folders in folders, a search, folders closed, files in conflict', () => {
    const cases: Array<{ query?: string; closed?: string[]; conflicted?: string[] }> = [{}, { query: 'AUTH' }, { query: 'tsx' }, { closed: ['src/', 'docs/guides/'] }, { closed: ['src/auth/'], query: 'session' }, { conflicted: ['README.md', 'not/in/the/diff.txt'] }, { query: 'nothing matches this' }];
    let rows = 0;
    for (const c of cases) {
      const theirs = theirTree(entriesOf(made, c.conflicted), c.query, c.closed);
      rows += theirs.length;
      const ours = changedFiles(made, { query: c.query, collapsed: new Set(c.closed), conflicted: c.conflicted });
      expect(differences(mine(ours), theirs), JSON.stringify(c)).toEqual([]);
    }
    console.log(`Review, changed files made for the test: ${cases.length} lists, ${rows} rows compared, 0 differences`);
  });

  it('counts lines and reviewed hunks for the files whose hunks are known', () => {
    const hunks: Record<string, Hunk[]> = {};
    for (const c of calls('workspace.hunks').slice(0, 6)) hunks[(c.params as { path: string }).path] = (c.result as { hunks: Hunk[] }).hunks;
    const rows = changedFiles(diffs[0] as Change[], { hunks });
    const file = (path: string): FileRow => rows.find(r => r.path === path) as FileRow;
    expect(file('README.md')).toMatchObject({ added: 4, removed: 0, hunks: 1, reviewed: 0, statusText: null });
    expect(file('docs/guides/sessions.md')).toMatchObject({ added: 3, removed: 1, hunks: 1 });
    expect(file('docs/old.md')).toMatchObject({ added: 0, removed: 1, status: 'D', statusText: 'Deleted file' });
    expect(file('src/auth/session-refresh-coordinator.ts')).toMatchObject({ added: 8, removed: 0, status: 'A', statusText: 'New file' });
    expect(file('picture.bin')).toMatchObject({ added: 0, removed: 0, hunks: 0 });
    expect(changedFiles(diffs[0] as Change[]).every(r => r.added === null && r.hunks === null)).toBe(true);
    // The daemon's own count of the same comparison.
    const counted = calls('workspace.changes')[0]?.result as { files: number; added: number; removed: number; names: string[] };
    expect(rows.filter(r => r.kind === 'file')).toHaveLength(counted.files);
    expect(rows.reduce((n, r) => n + (r.added ?? 0), 0)).toBe(counted.added);
    expect(rows.reduce((n, r) => n + (r.removed ?? 0), 0)).toBe(counted.removed);
  });

  it('says what changed under a conversation, as the chat\'s bar says it', () => {
    const counted = calls('workspace.changes')[0]?.result as { files: number; added: number; removed: number; names: string[] };
    expect(changesSummary(counted)).toEqual({ files: 6, text: '6 files', added: '+16', removed: '−2', names: 'README.md, a.txt, docs/guides/sessions.md, …', tooltip: 'Review changes\n' + counted.names.join('\n') });
    expect(changesSummary({ files: 1, names: ['a.txt'] })).toMatchObject({ text: '1 file', added: '', removed: '', names: 'a.txt' });
    expect(changesSummary({ files: 0 })).toBeNull();
    expect(changesSummary(null)).toBeNull();
  });
});

describe('review: the comparison to choose', () => {
  const options = (calls('comparison.options')[0]?.result as { options: Comparison[]; branches: string[] }).options;

  it('lists what VS Code\'s picker lists, with its labels', () => {
    const item = new Function(`return ${argument(REVIEW, 'const items = opts.options.map(')};`)() as (o: Comparison) => { label: string; description: string; detail: string };
    const other = new Function(`return ${argument(REVIEW, 'items.push(')};`)() as { label: string; detail: string };
    const theirs = [...options.map(item).map(i => ({ label: i.label.replace(/^\$\([a-z-]+\) /, ''), description: i.description, detail: i.detail })), { label: other.label.replace(/^\$\([a-z-]+\) /, ''), description: '', detail: other.detail }];
    const ours = comparisonChoices(options).map(c => ({ label: c.label, description: c.description, detail: c.detail }));
    expect(ours).toEqual(theirs);
    console.log(`Review, comparisons: ${theirs.length} choices compared with the items of the real picker: ${ours.map(c => c.label).join(' · ')}`);
    expect(ours.map(c => c.label)).toEqual(['Latest run', 'Since task start', 'Original fork', 'Merge-base with main (PR-style)', 'Tip of main (direct)', 'Other branch…']);
    expect([TEXT.comparison.latestRun, TEXT.comparison.sinceTaskStart, TEXT.comparison.originalFork]).toEqual(['Latest run', 'Since task start', 'Original fork']);
  });

  it('marks the one chosen, and the default when none was', () => {
    expect(comparisonChoices(options).filter(c => c.selected).map(c => c.mode)).toEqual(['latest_run']);
    expect(comparisonChoices(options, { mode: 'task_start' }).filter(c => c.selected).map(c => c.mode)).toEqual(['task_start']);
    expect(comparisonChoices(options, { mode: 'branch_tip', branch: 'main' }).filter(c => c.selected).map(c => c.key)).toEqual(['branch_tip:main']);
    expect(comparisonChoices(options, { mode: 'not a mode' }).filter(c => c.selected).map(c => c.mode)).toEqual(['latest_run']);
    const gone = options.map(o => (o.mode === 'latest_run' ? { ...o, available: false, base: null, detail: 'no run-start snapshot recorded' } : o));
    expect(comparisonChoices(gone)[0]).toMatchObject({ description: 'unavailable', available: false, selected: false });
    expect(comparisonChoices(gone).find(c => c.selected)?.mode).toBe('task_start');
    expect(comparisonChoices(options).find(c => c.mode === 'task_start')?.base).toBe(base);
    expect(branchChoices('main')).toEqual({ title: 'Compare with main', choices: [{ label: 'Merge-base (PR-style)', mode: 'branch_merge_base', branch: 'main' }, { label: 'Branch tip (direct)', mode: 'branch_tip', branch: 'main' }] });
  });
});

describe('review: hunks', () => {
  const results = calls('workspace.hunks').map(c => c.result as { path: string; shown: boolean; why?: string; hunks: Hunk[]; before: { exists: boolean; kind: string }; now: { exists: boolean; kind: string } });

  it('names a hunk with the key the daemon and VS Code give it', () => {
    let keys = 0;
    for (const r of results) {
      for (const h of r.hunks) {
        keys++;
        expect(hunkKey(r.path, h.base_lines, h.modified_lines), `${r.path} against the daemon`).toBe(h.key);
        expect(hunkKey(r.path, h.base_lines, h.modified_lines), `${r.path} against VS Code`).toBe(theirHash(`${r.path}\u0000${h.base_lines.join('\n')}\u0000${h.modified_lines.join('\n')}`));
      }
    }
    for (const [path, a, b] of [['a.txt', ['a'], ['b']], ['src/é.ts', [], ['let x = \'😀\';', '}']], ['', [], []], ['long', ['x'.repeat(5000)], ['y'.repeat(5000)]]] as Array<[string, string[], string[]]>) {
      keys++;
      expect(hunkKey(path, a, b)).toBe(theirHash(`${path}\u0000${a.join('\n')}\u0000${b.join('\n')}`));
    }
    console.log(`Review, keys of hunks: ${keys} keys, the same as the daemon's and as VS Code's hunkHash`);
    expect(keys).toBeGreaterThan(8);
  });

  it('shows a hunk as its removed lines, then its added lines, each with its own line number', () => {
    const guide = fileDiff(results.find(r => r.path === 'docs/guides/sessions.md') as typeof results[number]);
    expect(guide).toMatchObject({ path: 'docs/guides/sessions.md', shown: true, added: 3, removed: 1, reviewed: 0, note: null });
    const hunk = guide.hunks[0]!;
    expect(hunk).toMatchObject({ index: 1, reviewed: false, where: 'lines 3–5', label: 'Hunk 1 of docs/guides/sessions.md, lines 3–5', baseStart: 3, modifiedStart: 3, removed: 1, added: 3 });
    expect(hunk.rows.map(r => [r.kind, r.baseLine, r.modifiedLine])).toEqual([['removed', 3, null], ['added', null, 3], ['added', null, 4], ['added', null, 5]]);
    expect(hunk.rows[0]?.text).toBe('Old text.');
    expect(new Set(hunk.rows.map(r => r.key)).size).toBe(4);
    expect(hunk.accept).toEqual({ label: 'Accept hunk 1', reviewed: true });
    expect(hunk.reject).toEqual({ label: 'Reject hunk 1' });
  });

  it('says where a removal sits, what a new or deleted file is, and why a file is not shown', () => {
    const old = fileDiff(results.find(r => r.path === 'docs/old.md') as typeof results[number]);
    expect(old.hunks[0]).toMatchObject({ where: 'deletion after line 0', removed: 1, added: 0 });
    expect(old.note).toBe('Deleted file');
    expect(fileDiff(results.find(r => r.path.endsWith('coordinator.ts')) as typeof results[number]).note).toBe('New file');
    const picture = fileDiff(results.find(r => r.path === 'picture.bin') as typeof results[number]);
    expect(picture).toMatchObject({ shown: false, why: 'a binary file', note: 'Not shown: a binary file.', hunks: [] });
    expect(editTarget(picture)).toBeNull();
  });

  it('shows a hunk as reviewed when the daemon says so, or when a mark made since says so', () => {
    const readme = results.filter(r => r.path === 'README.md');
    expect(readme).toHaveLength(2);
    const before = fileDiff(readme[0]!), after = fileDiff(readme[1]!);
    expect(before.hunks[0]?.reviewed).toBe(false);
    expect(after.hunks[0]).toMatchObject({ reviewed: true, accept: { label: 'Hunk 1 accepted; tap to take the accept back', reviewed: false } });
    expect(after.reviewed).toBe(1);
    const key = readme[0]!.hunks[0]!.key;
    expect(fileDiff(readme[0]!, [key]).hunks[0]?.reviewed).toBe(true);
    expect(fileDiff(readme[1]!, []).hunks[0]?.reviewed).toBe(false);
    expect(editTarget(before)).toEqual({ hunk: key, line: 8 });
    expect(editTarget(after)).toEqual({ hunk: key, line: 8 });
  });

  it('asks the daemon to mark a hunk the way the recording asked', () => {
    const asked = calls('review.accept')[0]?.params as Record<string, unknown>;
    const hunk = results.find(r => r.path === 'README.md')?.hunks[0] as Hunk;
    expect({ run_id: asked['run_id'], ...acceptParams('README.md', hunk) }).toEqual(asked);
    expect(acceptParams('a.txt', hunk, 'the line it sits at')).toMatchObject({ anchor: 'the line it sits at' });
  });

  it('cuts a long line into pieces that join back into the line', () => {
    const long = results.find(r => r.path === 'docs/guides/sessions.md')?.hunks[0]?.modified_lines[0] as string;
    expect(long.length).toBeGreaterThan(300);
    for (const width of [1, 7, 20, 40, 80, 1000]) {
      const pieces = splitLine(long, width);
      expect(pieces.join(''), `width ${width}`).toBe(long);
      expect(Math.max(...pieces.map(p => p.length)), `width ${width}`).toBeLessThanOrEqual(width);
    }
    expect(splitLine(long, 40).every((p, i, all) => i === all.length - 1 || /[\s,:]$/.test(p))).toBe(true);
    expect(splitLine('const veryLongIdentifierWithoutAnyBreak = 1;', 10)).toEqual(['const ', 'veryLongId', 'entifierWi', 'thoutAnyBr', 'eak = 1;']);
    expect(splitLine('a/b/c/d/e/f/g/h/i/j', 8)).toEqual(['a/b/c/d/', 'e/f/g/h/', 'i/j']);
    expect(splitLine('', 10)).toEqual(['']);
    expect(splitLine('short', 10)).toEqual(['short']);
    const faces = '😀'.repeat(10);
    expect(splitLine(faces, 5).join('')).toBe(faces);
    expect(splitLine(faces, 5).every(p => !/^[\udc00-\udfff]/.test(p))).toBe(true);
  });
});

describe('review: marks kept current from events', () => {
  it('after every event the marks are what the daemon answers', () => {
    const run = String(recorded.marks['root']);
    const answers = calls('review.marks');
    expect(answers.length).toBe(5);
    let compared = 0, near = 0;
    for (const from of answers) {
      // Marks loaded at some moment, then kept current from the events after it.
      let state = loadMarks(load({ ...recorded.final, cursor: from.cursor }), run, (from.result as { marks: Mark[] }).marks);
      for (const event of recorded.events) {
        state = apply(state, event);
        for (const answer of answers.filter(a => a.cursor === event.seq && a.cursor > from.cursor)) {
          const theirs = (answer.result as { marks: Mark[] }).marks;
          const ours = [...marksOf(state, run)];
          compared++;
          for (const d of differences(ours, theirs)) {
            // The time of a mark is the daemon's clock a moment before the event: within 50 ms.
            const [where = '', values = ''] = d.split(': ');
            const [a, b] = values.split(' ≠ ').map(Number);
            expect(/\.at_ms$/.test(where) && Math.abs((a as number) - (b as number)) <= 50, d).toBe(true);
            near++;
          }
        }
      }
    }
    console.log(`Review, marks: ${compared} comparisons with what the daemon answered to review.marks, ${near} times within 50 ms, 0 other differences`);
    expect(compared).toBeGreaterThanOrEqual(10);
    expect(marksOf(load(recorded.final), run)).toEqual([]);
  });
});
