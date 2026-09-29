// AC-260: the repository chip's picker, host and webview sides, without VS Code.
// Host: a typed path (absolute or ~) becomes its Git root or a plain refusal, never a dialog; Tab
// completion lists folders; added repositories are remembered and listed with the nearby ones.
// Webview: the fuzzy match the picker filters with.
const assert = require('assert');
const Module = require('module');
const fs = require('fs');
const os = require('os');
const path = require('path');
const cp = require('child_process');

const dialogs = [];
const vscode = { workspace: { isTrusted: true, textDocuments: [], workspaceFolders: [], getConfiguration: () => ({ get: (k, d) => d }) },
  window: { showOpenDialog: async () => { dialogs.push('open'); return undefined; } } };
const originalLoad = Module._load;
Module._load = function (request, ...rest) { return request === 'vscode' ? vscode : originalLoad.call(this, request, ...rest); };
const { TaskLauncher } = require('../../extension/src/task-launcher');
Module._load = originalLoad;

const root = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'ovs-repo-picker-')));
const git = (dir, ...a) => cp.execFileSync('git', a, { cwd: dir, stdio: 'ignore' });
const repo = dir => { fs.mkdirSync(dir, { recursive: true }); git(dir, 'init', '-q', '-b', 'main'); return dir; };

function launcher() {
  const saved = new Map();
  const context = { globalState: { get: (k, d) => saved.has(k) ? saved.get(k) : d, update: async (k, v) => { saved.set(k, v); } } };
  const client = { request: async (method, params) => { if (method === 'repo.inspect') return fs.existsSync(path.join(params.path, '.git')) ? { branch: 'main', branches: ['main'] } : Promise.reject(new Error('not a repo')); return {}; } };
  return { l: new TaskLauncher(context, client, { state: { tasks: [] } }, async () => {}), saved };
}

(async () => {
  try {
    const site = repo(path.join(root, 'work', 'site'));
    const notes = repo(path.join(root, 'work', 'notes'));
    fs.mkdirSync(path.join(notes, 'docs', 'deep'), { recursive: true });
    fs.mkdirSync(path.join(root, 'work', 'plain-folder'));
    const far = repo(path.join(root, 'elsewhere', 'far-repo'));
    const { l, saved } = launcher();

    // A typed path: its Git root, from a folder inside it too.
    const added = await l.addRepo(path.join(notes, 'docs', 'deep'));
    assert.equal(added.path, notes); assert.equal(added.name, 'notes'); assert.equal(added.branch, 'main');
    // Remembered, most recent first, no duplicates.
    await l.addRepo(far); await l.addRepo(notes + '/');
    assert.deepEqual(saved.get('overseer.knownRepos'), [notes, far]);
    // ~ is the home folder.
    const home = os.homedir();
    await assert.rejects(l.addRepo('~/.overseer-no-such-folder-' + process.pid), new RegExp('No folder at ' + home.replace(/[.*+?^${}()|[\]\\]/g, '\\$&') + '/'));
    if (root.startsWith(home + '/')) assert.equal((await l.addRepo('~/' + path.relative(home, site))).path, site);
    // Refusals are plain words, never a dialog.
    await assert.rejects(l.addRepo(''), /Type the path/);
    await assert.rejects(l.addRepo('relative/path'), /full path/);
    await assert.rejects(l.addRepo(path.join(root, 'missing')), /No folder at/);
    await assert.rejects(l.addRepo(path.join(root, 'work', 'plain-folder')), /not in a Git repository/);
    assert.equal(dialogs.length, 0, 'no dialog for a typed path');

    // Tab completion: folders under the typed prefix, Git repositories marked, hidden ones skipped.
    fs.mkdirSync(path.join(root, 'work', '.hidden'));
    const hints = await l.pathHints(path.join(root, 'work') + '/');
    assert.deepEqual(hints.map(h => [path.basename(h.path.replace(/\/$/, '')), h.git]), [['notes', true], ['plain-folder', false], ['site', true]]);
    assert.ok(hints.every(h => h.path.endsWith('/')));
    const pre = await l.pathHints(path.join(root, 'work', 'NO'));
    assert.deepEqual(pre.map(h => h.path), [notes + '/'], 'prefix match ignores case');
    assert.deepEqual(await l.pathHints('not/absolute'), []);
    assert.deepEqual(await l.pathHints(path.join(root, 'nope', 'x')), []);
    if (root.startsWith(home + '/')) {
      const tilde = await l.pathHints('~/' + path.relative(home, path.join(root, 'work')) + '/si');
      assert.deepEqual(tilde.map(h => h.path), ['~/' + path.relative(home, site) + '/'], 'a ~ path completes as ~');
    }

    // Known repositories: open folders, then the remembered ones, then Git repositories beside an open one.
    vscode.workspace.workspaceFolders = [{ uri: { fsPath: site } }];
    const listed = await l.repos();
    const by = Object.fromEntries(listed.map(r => [r.name, r.source]));
    assert.equal(listed[0].path, site); assert.equal(by.site, 'open folder');
    assert.equal(by.notes, 'recent'); assert.equal(by['far-repo'], 'recent');
    assert.equal(listed.filter(r => r.name === 'notes').length, 1);
    assert.ok(!('plain-folder' in by), 'a folder that is not a repository is not offered');
    // A remembered repository that is gone is left out.
    fs.rmSync(far, { recursive: true, force: true });
    assert.ok(!(await l.repos()).some(r => r.name === 'far-repo'));
    // Nearby only: a sibling nobody added.
    const { l: fresh } = launcher();
    const nearby = await fresh.repos();
    assert.equal(nearby.find(r => r.name === 'notes')?.source, 'nearby');

    // Webview: the fuzzy match (letters in order; a name match beats a path-only match).
    const win = { OverseerUI: { el: () => ({}) } };
    new Function('window', fs.readFileSync(path.join(__dirname, '../../extension/media/composer.js'), 'utf8'))(win);
    const { fuzzy } = win.OverseerComposer;
    assert.ok(fuzzy('frst', 'friction-site') > 0);
    assert.equal(fuzzy('xyz', 'friction-site'), 0);
    assert.ok(fuzzy('', 'anything') > 0);
    assert.ok(fuzzy('site', 'site') > fuzzy('site', 'some-internal-test-emitter'), 'contiguous beats scattered');
    assert.ok(fuzzy('not', 'notes') > fuzzy('not', 'a-note-taker'), 'a start-of-name match ranks first');
    assert.ok(fuzzy('NOTES', 'notes') > 0, 'case does not matter');
    console.log('repo picker: ok');
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
})().catch(e => { console.error(e); process.exit(1); });
