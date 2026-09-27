const assert = require('assert');
const Module = require('module');
const path = require('path');

const vscode = { workspace: { isTrusted: true, textDocuments: [], workspaceFolders: [] }, window: {} };
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
    { id: 'p-local', harnesses: ['opencode'], name: 'Local OpenCode' }],
    profileStatus: new Map([['p-codex', { logged_in: true }], ['p-claude', { logged_in: true }],
      ['p-local', { installed: true, logged_in: false }]]),
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

  const y = fixture({ state: 'paused', decision: { selected: null } });
  await assert.rejects(y.launcher.start(f), /no eligible route|paused/i);
  const pending = y.saved.get('overseer.autoPendingStart');
  assert.equal(pending.workUnitId, y.calls[1][1].work_unit_id);
  await assert.rejects(y.launcher.start(f), /no eligible route|paused/i);
  assert.equal(y.calls[3][1].work_unit_id, pending.workUnitId, 'retry must reuse its admitted work-unit identity');

  const z = fixture();
  assert.equal(await z.launcher.start({ repo: f.repo, harness: 'codex', account: 'p-codex', prompt: 'manual' }), 'r-manual');
  assert.equal(z.calls[0][0], 'task.create', 'manual start must retain its existing daemon path');
  console.log('task launcher Auto and manual starts passed');
})().catch(error => { console.error(error); process.exitCode = 1; });
