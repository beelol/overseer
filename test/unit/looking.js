// What the window tells the daemon about the agent it shows (ui.focus, AC-129): the helper in
// extension/src/phone-access.js with a stand-in for VS Code. It speaks when the window gains or
// loses focus, when another agent is shown and once per connection, and never says the same twice.
// Run: node test/unit/looking.js
const assert = require('assert');
const path = require('path');
const Module = require('module');
const { EventEmitter } = require('events');

const listeners = { window: [], tabs: [], groups: [] };
const on = list => fn => { list.push(fn); return { dispose() {} }; };
const vscode = {
  window: { state: { focused: true }, onDidChangeWindowState: on(listeners.window), tabGroups: { onDidChangeTabs: on(listeners.tabs), onDidChangeTabGroups: on(listeners.groups) } },
  EventEmitter: class { constructor() { this.fns = []; this.event = fn => { this.fns.push(fn); return { dispose() {} }; }; } fire(v) { for (const f of this.fns) f(v); } },
};
const load = Module._load;
Module._load = function (request, ...rest) { return request === 'vscode' ? vscode : load.call(this, request, ...rest); };
const { Looking } = require(path.resolve(__dirname, '../../extension/src/phone-access.js'));

let failures = 0, passed = 0;
const test = async (name, fn) => { try { await fn(); passed++; console.log('ok  ', name); } catch (e) { failures++; console.log('FAIL', name, '-', e.message); } };

function setup({ fail } = {}) {
  vscode.window.state.focused = true;
  for (const l of Object.values(listeners)) l.length = 0;
  const client = new EventEmitter();
  client.connected = true;
  client.sent = [];
  client.request = (method, params) => { client.sent.push({ method, params }); return fail ? Promise.reject(Object.assign(new Error(fail), { code: fail })) : Promise.resolve({ ok: true }); };
  const runs = { root: { id: 'root' }, child: { id: 'child', parent_run_id: 'root' }, other: { id: 'other' } };
  const changed = new vscode.EventEmitter();
  const model = { run: id => runs[id], rootRun: r => (r.parent_run_id ? runs[r.parent_run_id] : r), onDidChange: changed.event };
  const center = { panel: { visible: true }, mode: 'chat', chatRun: 'root' };
  const outputs = { panels: new Map() };
  const state = { selected: 'root' };
  const said = [];
  const looking = new Looking({ subscriptions: [] }, client, { model, center, outputs, selected: () => state.selected, say: m => said.push(m) });
  const last = () => client.sent[client.sent.length - 1]?.params;
  return { client, model, center, outputs, state, looking, changed, said, last, focus: focused => { vscode.window.state.focused = focused; for (const f of listeners.window) f({ focused }); }, tabs: () => { for (const f of listeners.tabs) f({}); } };
}

(async () => {
  await test('on a connection it says which agent the focused window shows', () => {
    const t = setup();
    t.client.emit('connected');
    assert.deepStrictEqual(t.client.sent, [{ method: 'ui.focus', params: { run_id: 'root', focused: true } }]);
  });
  await test('the window loses focus: focused false, no agent; it gains focus: the agent again', () => {
    const t = setup();
    t.client.emit('connected');
    t.focus(false);
    assert.deepStrictEqual(t.last(), { run_id: null, focused: false });
    t.focus(true);
    assert.deepStrictEqual(t.last(), { run_id: 'root', focused: true });
    assert.strictEqual(t.client.sent.length, 3);
  });
  await test('another agent is shown: the daemon is told once, however often the state is read', () => {
    const t = setup();
    t.client.emit('connected');
    t.state.selected = 'other'; t.center.chatRun = 'other';
    t.changed.fire(); t.changed.fire(); t.tabs(); t.changed.fire();
    assert.strictEqual(t.client.sent.length, 2);
    assert.deepStrictEqual(t.last(), { run_id: 'other', focused: true });
  });
  await test("a child's conversation is its parent's: the top-level agent is named", () => {
    const t = setup();
    t.state.selected = 'child'; t.center.chatRun = 'child';
    t.client.emit('connected');
    assert.deepStrictEqual(t.last(), { run_id: 'root', focused: true });
  });
  await test('the grid, the new-agent form and a hidden view show no single agent', () => {
    for (const change of [c => { c.mode = 'grid'; }, c => { c.mode = 'composer'; }, c => { c.panel.visible = false; }, c => { c.panel = undefined; }, c => { c.chatRun = undefined; }]) {
      const t = setup();
      change(t.center);
      t.client.emit('connected');
      assert.deepStrictEqual(t.last(), { run_id: null, focused: false });
    }
  });
  await test('a chat opened to the side counts when it is on screen', () => {
    const t = setup();
    t.center.panel.visible = false;
    t.outputs.panels.set('other', { panel: { visible: true } });
    t.client.emit('connected');
    assert.deepStrictEqual(t.last(), { run_id: 'other', focused: true }, 'the chat on screen, when the selected one is not');
    t.outputs.panels.set('root', { panel: { visible: true } });
    t.changed.fire();
    assert.deepStrictEqual(t.last(), { run_id: 'root', focused: true }, 'the selected agent, when its chat is on screen too');
    t.outputs.panels.get('root').panel.visible = false; t.outputs.panels.get('other').panel.visible = false;
    t.tabs();
    assert.deepStrictEqual(t.last(), { run_id: null, focused: false });
  });
  await test('an agent that is gone is not named', () => {
    const t = setup();
    t.state.selected = 'deleted'; t.center.chatRun = 'deleted';
    t.client.emit('connected');
    assert.deepStrictEqual(t.last(), { run_id: null, focused: false });
  });
  await test('a new connection is told again; with no connection nothing is sent', () => {
    const t = setup();
    t.client.emit('connected');
    t.client.connected = false;
    t.focus(false);
    assert.strictEqual(t.client.sent.length, 1);
    t.client.connected = true;
    vscode.window.state.focused = true;
    t.client.emit('connected');
    assert.strictEqual(t.client.sent.length, 2);
    assert.deepStrictEqual(t.last(), { run_id: 'root', focused: true });
  });
  await test('a daemon without the method is left alone; another failure is said and tried again', async () => {
    const old = setup({ fail: 'unknown_method' });
    old.client.emit('connected');
    await new Promise(r => setImmediate(r));
    old.changed.fire();
    assert.strictEqual(old.client.sent.length, 1);
    assert.deepStrictEqual(old.said, []);
    const broken = setup({ fail: 'failed' });
    broken.client.emit('connected');
    await new Promise(r => setImmediate(r));
    assert.deepStrictEqual(broken.said, ['ui.focus: failed']);
    broken.changed.fire();
    assert.strictEqual(broken.client.sent.length, 2, 'asked again at the next change');
  });
  console.log(failures ? `${failures} of ${passed + failures} failed` : `${passed} passed`);
  process.exit(failures ? 1 : 0);
})();
