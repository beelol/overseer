const assert = require('assert');
const Module = require('module');
const path = require('path');

// Auto routing is behind `overseer.experimental.autoRouting` (AC-204).
const settings = { 'experimental.autoRouting': true };
const vscode = { workspace: { isTrusted: true, textDocuments: [], workspaceFolders: [],
  getConfiguration: () => ({ get: (key, fallback) => key in settings ? settings[key] : fallback }) }, window: {} };
const originalLoad = Module._load;
Module._load = function (request, ...rest) { return request === 'vscode' ? vscode : originalLoad.call(this, request, ...rest); };
const { TaskLauncher } = require('../../extension/src/task-launcher');
Module._load = originalLoad;

function fixture(reply) {
  const saved = new Map();
  const calls = [];
  const context = { globalState: { get: (key, fallback) => saved.has(key) ? saved.get(key) : fallback,
    update: async (key, value) => { saved.set(key, value); } } };
  const model = { accounts: [{ id: 'p-codex', harnesses: ['codex'], name: 'Codex' },
    { id: 'p-claude', harnesses: ['claude'], name: 'Claude' },
    { id: 'p-local', harnesses: ['opencode'], name: 'Local OpenCode' },
    { id: 'local-ollama', harnesses: ['opencode'], name: 'Continuity local models' }],
    profileStatus: new Map([['p-codex', { logged_in: true }], ['p-claude', { logged_in: true }],
      ['p-local', { installed: true, logged_in: false }], ['local-ollama', { installed: true, logged_in: false }]]),
    refresh: async () => { calls.push(['refresh']); } };
  const client = { request: async (method, params) => { calls.push([method, params]);
    if (method === 'auto.start') return reply;
    if (method === 'task.create') return { run: { id: 'r-manual' } };
    return { enabled: true };
  } };
  return { launcher: new TaskLauncher(context, client, model, async () => {}), calls, saved };
}

(async () => {
  const f = { repo: '/tmp/overseer-auto-test-repo', routing: 'auto', mode: 'worktree',
    prompt: 'Inspect the browser flow', preferredHarness: 'codex-app' };
  const x = fixture({ state: 'launch_pending', run: { id: 'r-auto' },
    decision: { selected: 'codex/sol/medium' } });
  assert.equal(await x.launcher.start(f), 'r-auto');
  assert.deepEqual(x.calls.slice(0, 2).map(c => c[0]), ['auto.mode.set', 'auto.start']);
  const request = x.calls[1][1];
  assert.equal(request.preferred_harness, 'codex-app');
  assert.deepEqual(request.allowed_profiles, ['p-codex', 'p-claude', 'p-local']);
  assert.equal(request.prompt, f.prompt);
  assert.equal(request.workspace_mode, 'worktree');
  assert.match(request.work_unit_id, /^[A-Za-z0-9_-]+$/);
  assert.equal(x.saved.get('overseer.autoPendingStart'), undefined);

  const y = fixture({ state: 'paused', decision: { selected: null, reason: 'no_eligible_route',
    exclusions: [{ route_id: 'codex/sol', reason: 'quota_exhausted' }] } });
  await assert.rejects(y.launcher.start(f), /allowance is exhausted/i);
  const pending = y.saved.get('overseer.autoPendingStart');
  assert.equal(pending.workUnitId, y.calls[1][1].work_unit_id);
  await assert.rejects(y.launcher.start(f), /allowance is exhausted/i);
  assert.equal(y.calls[3][1].work_unit_id, pending.workUnitId, 'retry must reuse its admitted work-unit identity');

  // Off (the default): an Auto start is refused before any daemon request.
  settings['experimental.autoRouting'] = false;
  const off = fixture({ run: { id: 'never' } });
  await assert.rejects(off.launcher.start(f), /overseer\.experimental\.autoRouting/);
  assert.equal(off.calls.length, 0, 'Auto routing off: no auto.mode.set and no auto.start');
  settings['experimental.autoRouting'] = true;

  const z = fixture();
  assert.equal(await z.launcher.start({ repo: f.repo, harness: 'codex', account: 'p-codex', prompt: 'manual' }), 'r-manual');
  assert.equal(z.calls[0][0], 'task.create', 'manual start must retain its existing daemon path');
  console.log('task launcher Auto and manual starts passed');
})().catch(error => { console.error(error); process.exitCode = 1; });
