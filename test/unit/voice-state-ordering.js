// A deferred snapshot must not replace a newer live Voice state.
const assert = require('assert');
const Module = require('module');
const { EventEmitter } = require('events');
const tests = []; const test = (name, fn) => tests.push([name, fn]);
const clone = v => JSON.parse(JSON.stringify(v));
const snapshot = (state = 'starting') => ({ enabled: state !== 'off', state, muted: false, available: true, simulated: true, target: 'overseer', targeted: [], model: { downloaded: true }, listener: { running: state !== 'off', restarts: 0, last_error: null } });

function host(initial) {
  const posts = []; const contexts = []; const status = { show() {}, hide() {}, dispose() {} };
  const vscode = {
    StatusBarAlignment: { Left: 1 },
    window: { createStatusBarItem: () => status, showInformationMessage: async () => { throw new Error('download prompt is not expected'); } },
    workspace: { onDidChangeConfiguration: () => ({ dispose() {} }), getConfiguration: () => ({ get: () => 'off' }) },
    commands: { executeCommand: async (...args) => { contexts.push(args); } },
  };
  const load = Module._load;
  Module._load = function (name, ...rest) { return name === 'vscode' ? vscode : load.call(this, name, ...rest); };
  let Voice; try { ({ Voice } = require('../../extension/src/voice')); } finally { Module._load = load; }
  const client = new EventEmitter(); const calls = [];
  let current = clone(initial); let deferred;
  client.request = async (method, params) => {
    calls.push({ method, params });
    if (method === 'voice.get') {
      const value = clone(current);
      if (deferred) { const hold = deferred; deferred = null; return new Promise(resolve => { hold.release = () => resolve(value); }); }
      return value;
    }
    if (method === 'voice.set') { current = snapshot(params.enabled ? 'listening' : 'off'); return clone(current); }
    if (method === 'voice.requests') return { requests: [] };
    throw new Error(`unexpected RPC ${method}`);
  };
  const voice = new Voice({ subscriptions: [] }, client, { view: () => ({ webview: { postMessage: m => posts.push(clone(m)) } }) });
  voice.voice = clone(initial);
  return { voice, client, calls, posts, contexts, status, deferGet() { const hold = {}; deferred = hold; return hold; } };
}

test('late_starting_snapshot_cannot_replace_live_listening', async () => {
  const { voice, client, posts, status, deferGet } = host(snapshot());
  const hold = deferGet(); const refresh = voice.refresh();
  assert.strictEqual(typeof hold.release, 'function', 'voice.get has captured the Starting snapshot');
  client.emit('voice', { kind: 'state', state: 'listening' });
  assert.strictEqual(voice.summary().state, 'listening', 'live event reaches the real host');
  hold.release(); await refresh;
  assert.strictEqual(voice.summary().state, 'listening', 'older snapshot cannot regress the host');
  assert.match(status.text, /Listening/);
  assert.strictEqual(posts.filter(p => p.m.type === 'snapshot').at(-1).m.voice.state, 'listening', 'webview receives the current state');
});

test('off_state_then_toggle_explicitly_reenables', async () => {
  const { voice, client, calls, posts } = host(snapshot('listening'));
  client.emit('voice', { kind: 'state', state: 'off', reason: 'The listener stopped four times.' });
  assert.strictEqual(voice.summary().on, false);
  assert.strictEqual(voice.summary().stopped, 'The listener stopped four times.');
  await voice.toggle();
  assert.deepStrictEqual(calls.filter(c => c.method === 'voice.set'), [{ method: 'voice.set', params: { enabled: true } }]);
  assert.strictEqual(voice.summary().state, 'listening');
  assert.strictEqual(voice.summary().on, true);
  assert.strictEqual(posts.filter(p => p.m.type === 'snapshot').at(-1).m.voice.state, 'listening');
});

(async () => {
  let failed = 0;
  for (const [name, fn] of tests) {
    try { await fn(); console.log(`PASS ${name}`); }
    catch (error) { failed++; console.error(`FAIL ${name}\n${error.stack}`); }
  }
  console.log(`${tests.length - failed}/${tests.length} Voice state ordering tests passed`);
  process.exitCode = failed ? 1 : 0;
})();
