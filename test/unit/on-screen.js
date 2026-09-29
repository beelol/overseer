// Keys act only on what you can see (AC-242): extension/src/on-screen.js. ⌥⌘Y and ⌥⌘⌫ answer only
// the agent on screen and otherwise ask; palette commands with no agent given use the one on screen
// or ask, never an empty id.
// Run: node test/unit/on-screen.js
const assert = require('assert');
const path = require('path');
const { agentsOnScreen, permissionTarget, commandTarget } = require(path.resolve(__dirname, '../../extension/src/on-screen.js'));

let failures = 0, passed = 0;
const test = (name, fn) => { try { fn(); passed++; console.log('ok  ', name); } catch (e) { failures++; console.log('FAIL', name, '-', e.message); } };

const runs = [
  { id: 'a', created_ms: 1, status: 'waiting_for_user', attention: { kind: 'permission', request_id: 1, tool: 'Write' } },
  { id: 'b', created_ms: 2, status: 'waiting_for_user', attention: { kind: 'permission', request_id: 2, tool: 'Bash' } },
  { id: 'c', created_ms: 3, status: 'completed' },
  { id: 'c1', parent_run_id: 'c', created_ms: 4, status: 'waiting_for_user', attention: { kind: 'permission', request_id: 3, tool: 'Edit' } },
];
const byId = new Map(runs.map(r => [r.id, r]));
const model = { run: id => byId.get(id), rootRun: r => (r.parent_run_id ? byId.get(r.parent_run_id) : r) };
const rootOf = r => model.rootRun(r).id;

test('the Overseer view\'s chat, a chat taken out and a review count as on screen; hidden ones and the grid do not', () => {
  const outputs = { panels: new Map([['b', { panel: { visible: true } }], ['c', { panel: { visible: false } }]]) };
  const review = { manager: { panels: new Map([['c1', { visible: true }]]) } };
  assert.deepStrictEqual(agentsOnScreen({ model, center: { panel: { visible: true }, mode: 'chat', chatRun: 'a' }, outputs, review }).sort(), ['a', 'b', 'c']);
  assert.deepStrictEqual(agentsOnScreen({ model, center: { panel: { visible: true }, mode: 'grid', chatRun: 'a' } }), []);
  assert.deepStrictEqual(agentsOnScreen({ model, center: { panel: { visible: false }, mode: 'chat', chatRun: 'a' } }), []);
});

test('⌥⌘Y with agent A on screen answers A, never B', () => {
  assert.strictEqual(permissionTarget(runs, ['a'], rootOf).run.id, 'a');
});

test('a child\'s request is answered when its parent\'s chat is on screen', () => {
  assert.strictEqual(permissionTarget(runs, ['c'], rootOf).run.id, 'c1');
});

test('with no waiting agent on screen, it asks which request (all of them), answering none', () => {
  const t = permissionTarget(runs, [], rootOf);
  assert.ok(!t.run); assert.deepStrictEqual(t.choose.map(r => r.id), ['a', 'b', 'c1']);
  const other = permissionTarget(runs, ['zzz'], rootOf);
  assert.ok(!other.run); assert.strictEqual(other.choose.length, 3);
});

test('with two waiting agents on screen, it asks between those two', () => {
  const t = permissionTarget(runs, ['a', 'b'], rootOf);
  assert.ok(!t.run); assert.deepStrictEqual(t.choose.map(r => r.id), ['a', 'b']);
});

test('nothing waiting: none', () => {
  assert.deepStrictEqual(permissionTarget([runs[2]], ['c'], rootOf), { none: true });
});

test('a palette command given an agent (a side bar row, a run id) uses it', () => {
  assert.deepStrictEqual(commandTarget('b', { runs, onScreen: ['a'] }), { id: 'b' });
  assert.deepStrictEqual(commandTarget({ run: { id: 'c' } }, { runs, onScreen: [] }), { id: 'c' });
});

test('with nothing given it uses the one agent on screen that fits', () => {
  assert.deepStrictEqual(commandTarget(undefined, { runs, onScreen: ['a'] }), { id: 'a' });
  const active = r => r.status !== 'completed';
  assert.deepStrictEqual(commandTarget(undefined, { runs, onScreen: ['a', 'c'], fits: active }), { id: 'a' });
});

test('with nothing given and nothing on screen it offers a choice of top-level agents that fit, the selected one first', () => {
  const t = commandTarget(undefined, { runs, onScreen: [], selected: 'a' });
  assert.deepStrictEqual(t.choose.map(r => r.id), ['a', 'c', 'b']);
  const stop = commandTarget(undefined, { runs, onScreen: [], fits: r => r.status === 'running' });
  assert.deepStrictEqual(stop, { choose: [] });
});

console.log(`${passed} passed, ${failures} failed`);
process.exit(failures ? 1 : 0);
