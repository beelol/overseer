// You hear about it outside VS Code (AC-240): extension/src/notices.js with a stand-in for VS Code.
// The window tells the daemon whether it has the OS focus and which kinds the owner wants, once per
// connection and on every change; a notification's click URL names the agent it opens.
// Run: node test/unit/notices.js
const assert = require('assert');
const path = require('path');
const Module = require('module');
const { EventEmitter } = require('events');

const listeners = { window: [], config: [] };
const config = {};
const vscode = {
  window: { state: { focused: true }, onDidChangeWindowState: fn => { listeners.window.push(fn); return { dispose() {} }; } },
  workspace: { getConfiguration: () => ({ get: (k, d) => (k in config ? config[k] : d) }), onDidChangeConfiguration: fn => { listeners.config.push(fn); return { dispose() {} }; } },
};
const load = Module._load;
Module._load = function (request, ...rest) { return request === 'vscode' ? vscode : load.call(this, request, ...rest); };
const { Notices, kindsFrom, agentFromUri, uriAction } = require(path.resolve(__dirname, '../../extension/src/notices.js'));

let failures = 0, passed = 0;
const test = async (name, fn) => { try { await fn(); passed++; console.log('ok  ', name); } catch (e) { failures++; console.log('FAIL', name, '-', e.message); } };

function setup({ fail } = {}) {
  vscode.window.state.focused = true;
  for (const k of Object.keys(config)) delete config[k];
  listeners.window.length = 0; listeners.config.length = 0;
  const client = new EventEmitter();
  client.connected = true; client.sent = [];
  client.request = (method, params) => { client.sent.push({ method, params }); return fail ? Promise.reject(Object.assign(new Error(fail), { code: fail })) : Promise.resolve({ ok: true }); };
  const said = [];
  const notices = new Notices({ subscriptions: [] }, client, { say: m => said.push(m) });
  const focus = focused => { vscode.window.state.focused = focused; for (const f of listeners.window) f({ focused }); };
  const change = (key, value) => { config[key] = value; for (const f of listeners.config) f({ affectsConfiguration: s => key.startsWith(s.replace(/^overseer\./, '')) }); };
  return { client, notices, focus, change, said, sent: m => client.sent.filter(x => x.method === m).map(x => x.params) };
}

(async () => {
  await test('on a connection it says whether the window is focused and which kinds the owner wants', () => {
    const t = setup();
    t.client.emit('connected');
    assert.deepStrictEqual(t.sent('ui.window'), [{ focused: true }]);
    assert.deepStrictEqual(t.sent('notices.set'), [{ kinds: ['permission', 'question', 'failure', 'finished'] }]);
  });
  await test('switching to another app and back: unfocused, then focused, each said once', () => {
    const t = setup();
    t.client.emit('connected');
    t.focus(false); t.focus(false); t.focus(true);
    assert.deepStrictEqual(t.sent('ui.window'), [{ focused: true }, { focused: false }, { focused: true }]);
  });
  await test('a new connection says it again (the daemon keeps it per connection)', () => {
    const t = setup();
    t.client.emit('connected'); t.client.emit('connected');
    assert.strictEqual(t.sent('ui.window').length, 2);
    assert.strictEqual(t.sent('notices.set').length, 2);
  });
  await test('the settings choose the kinds: Needs you covers permission and question', () => {
    assert.deepStrictEqual(kindsFrom({ get: (k, d) => ({ 'notifications.finished': false }[k] ?? d) }), ['permission', 'question', 'failure']);
    assert.deepStrictEqual(kindsFrom({ get: (k, d) => ({ 'notifications.needsYou': false, 'notifications.failed': false }[k] ?? d) }), ['finished']);
    const t = setup();
    t.client.emit('connected');
    t.change('notifications.finished', false);
    assert.deepStrictEqual(t.sent('notices.set').pop(), { kinds: ['permission', 'question', 'failure'] });
  });
  await test('a daemon from before these methods is not an error worth saying', async () => {
    const t = setup({ fail: 'unknown_method' });
    t.client.emit('connected');
    await new Promise(r => setImmediate(r));
    assert.deepStrictEqual(t.said, []);
  });
  await test('a notification\'s click URL names the agent it opens; anything else opens none', () => {
    assert.strictEqual(agentFromUri({ path: '/open-agent', query: 'run=r-3bdbf180f5d7' }), 'r-3bdbf180f5d7');
    assert.strictEqual(agentFromUri({ path: '/open-center', query: '' }), undefined);
    assert.strictEqual(agentFromUri({ path: '/open-agent', query: 'run=../../x' }), undefined);
    assert.strictEqual(agentFromUri({ path: '/open-agent', query: '' }), undefined);
  });
  await test("the menu-bar item's links (AC-262): an agent, the view filtered, the workspace, Talk to Overseer", () => {
    assert.deepStrictEqual(uriAction({ path: '/open-agent', query: 'run=r-1' }), { kind: 'agent', run: 'r-1' });
    assert.strictEqual(uriAction({ path: '/open-agent', query: 'run=../x' }), undefined);
    assert.deepStrictEqual(uriAction({ path: '/open-center', query: '' }), { kind: 'center', filter: undefined, repo: undefined });
    assert.deepStrictEqual(uriAction({ path: '/open-center', query: 'filter=needs' }), { kind: 'center', filter: 'needs', repo: undefined });
    assert.deepStrictEqual(uriAction({ path: '/open-center', query: 'repo=overseer&filter=bogus' }), { kind: 'center', filter: undefined, repo: 'overseer' });
    assert.deepStrictEqual(uriAction({ path: '/open-workspace', query: '' }), { kind: 'workspace' });
    assert.deepStrictEqual(uriAction({ path: '/talk', query: '' }), { kind: 'talk' });
    assert.strictEqual(uriAction({ path: '/rm-rf', query: '' }), undefined);
  });
  console.log(`${passed} passed, ${failures} failed`);
  process.exit(failures ? 1 : 0);
})();
