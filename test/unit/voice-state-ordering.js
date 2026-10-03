// A deferred snapshot must not replace a newer live Voice state.
const assert = require('assert');
const Module = require('module');
const { EventEmitter } = require('events');
const tests = []; const test = (name, fn) => tests.push([name, fn]);
const clone = v => JSON.parse(JSON.stringify(v));
const snapshot = (state = 'starting') => ({ enabled: state !== 'off', state, muted: false, available: true, simulated: true, target: 'overseer', targeted: [], model: { downloaded: true }, listener: { running: state !== 'off', restarts: 0, last_error: null } });
const drain = () => new Promise(resolve => setImmediate(resolve));

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
  let Voice; try { delete require.cache[require.resolve('../../extension/src/voice')]; ({ Voice } = require('../../extension/src/voice')); } finally { Module._load = load; }
  const client = new EventEmitter(); const calls = [];
  let current = clone(initial || snapshot()); const gets = []; const rosters = [];
  let roster = [];
  client.request = async (method, params) => {
    calls.push({ method, params });
    if (method === 'voice.get') {
      const value = clone(current);
      if (gets.length) { const hold = gets.shift(); return new Promise((resolve, reject) => { hold.release = () => resolve(value); hold.reject = () => reject(new Error('synthetic old get failure')); }); }
      return value;
    }
    if (method === 'voice.set') { current = snapshot(params.enabled ? 'listening' : 'off'); return clone(current); }
    if (method === 'voice.requests') return { requests: [] };
    if (method === 'agents.roster') { const value = { roster: clone(roster) }; if (rosters.length) { const hold = rosters.shift(); return new Promise(resolve => { hold.release = () => resolve(value); }); } return value; }
    throw new Error(`unexpected RPC ${method}`);
  };
  const voice = new Voice({ subscriptions: [] }, client, { view: () => ({ webview: { postMessage: m => posts.push(clone(m)) } }) });
  voice.voice = clone(initial);
  return { voice, client, calls, posts, contexts, status,
    setCurrent(value) { current = clone(value); }, setRoster(value) { roster = clone(value); },
    deferGet() { const hold = {}; gets.push(hold); return hold; },
    deferRoster() { const hold = {}; rosters.push(hold); return hold; },
  };
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

test('initial_live_state_and_target_keep_snapshot_static_fields', async () => {
  const h = host(null); h.setRoster([{ id: 'b', title: 'Agent B' }]);
  const hold = h.deferGet(); const refreshing = h.voice.refresh();
  h.client.emit('voice', { kind: 'state', state: 'listening' });
  h.client.emit('voice', { kind: 'target', target: 'b' });
  hold.release(); await refreshing; await drain();
  assert.strictEqual(h.voice.summary().state, 'listening');
  assert.strictEqual(h.voice.voice.target, 'b');
  assert.strictEqual(h.voice.voice.target_title, 'Agent B');
  assert.strictEqual(h.voice.voice.available, true);
  assert.strictEqual(h.voice.voice.simulated, true);
  assert.deepStrictEqual(h.voice.voice.model, { downloaded: true });
  assert.deepStrictEqual(h.voice.voice.listener, snapshot().listener);
});

test('newest_refresh_wins_when_older_success_arrives_last', async () => {
  const h = host(snapshot()); const old = h.deferGet(); const oldRefresh = h.voice.refresh();
  h.setCurrent(snapshot('listening')); const fresh = h.deferGet(); const freshRefresh = h.voice.refresh();
  fresh.release(); await freshRefresh; const posts = h.posts.length;
  old.release(); await oldRefresh;
  assert.strictEqual(h.voice.summary().state, 'listening');
  assert.strictEqual(h.posts.length, posts, 'older request cannot publish again');
});

test('older_failed_refresh_cannot_clear_newer_success', async () => {
  const h = host(snapshot()); const old = h.deferGet(); const oldRefresh = h.voice.refresh();
  h.setCurrent(snapshot('listening')); await h.voice.refresh(); const posts = h.posts.length;
  old.reject(); await oldRefresh;
  assert.strictEqual(h.voice.summary().state, 'listening');
  assert.strictEqual(h.posts.length, posts);
});

test('disconnect_invalidates_held_snapshot_without_resurrection', async () => {
  const h = host(snapshot()); const old = h.deferGet(); const oldRefresh = h.voice.refresh();
  h.client.emit('disconnected'); const posts = h.posts.length;
  old.release(); await oldRefresh;
  assert.strictEqual(h.voice.voice, null);
  assert.strictEqual(h.voice.summary().on, false);
  assert.strictEqual(h.posts.length, posts);
});

test('prior_connection_error_cannot_clear_reconnected_snapshot', async () => {
  const h = host(snapshot()); const old = h.deferGet(); const oldRefresh = h.voice.refresh();
  h.client.emit('disconnected'); h.setCurrent(snapshot('listening'));
  const fresh = h.deferGet(); h.client.emit('connected');
  assert.strictEqual(typeof fresh.release, 'function'); fresh.release(); await drain();
  assert.strictEqual(h.voice.summary().state, 'listening', 'new connection is a valid control');
  old.reject(); await oldRefresh;
  assert.strictEqual(h.voice.summary().state, 'listening');
});

test('older_target_title_lookup_cannot_relabel_newer_target', async () => {
  const h = host(snapshot('listening')); h.setRoster([{ id: 'a', title: 'Agent A' }]);
  const old = h.deferRoster(); h.client.emit('voice', { kind: 'target', target: 'a' });
  assert.strictEqual(typeof old.release, 'function');
  h.setRoster([{ id: 'b', title: 'Agent B' }]); h.client.emit('voice', { kind: 'target', target: 'b' });
  await drain(); assert.strictEqual(h.voice.voice.target_title, 'Agent B', 'new lookup reaches the host');
  const posts = h.posts.length; old.release(); await drain();
  assert.strictEqual(h.voice.voice.target, 'b');
  assert.strictEqual(h.voice.voice.target_title, 'Agent B');
  assert.strictEqual(h.posts.length, posts, 'older target must not be republished');
});

test('target_title_lookup_after_disconnect_is_inert', async () => {
  const h = host({ ...snapshot('listening'), target: 'a' }); h.setRoster([{ id: 'a', title: 'Agent A' }]);
  const old = h.deferRoster(); const title = h.voice.titleTarget();
  const safe = assert.doesNotReject(title);
  h.client.emit('disconnected'); old.release(); await safe;
  assert.strictEqual(h.voice.voice, null);
  assert.strictEqual(h.voice.summary().on, false);
});

test('live_off_during_title_lookup_remains_off_in_snapshot', async () => {
  const h = host({ ...snapshot('listening'), target: 'a' }); h.setRoster([{ id: 'a', title: 'Agent A' }]);
  const title = h.deferRoster(); const refreshing = h.voice.refresh();
  await drain(); assert.strictEqual(typeof title.release, 'function');
  h.client.emit('voice', { kind: 'state', state: 'off', reason: 'Synthetic stop.' });
  title.release(); await refreshing;
  assert.strictEqual(h.voice.summary().on, false);
  const latest = h.posts.filter(p => p.m.type === 'snapshot').at(-1).m.voice;
  assert.strictEqual(latest.enabled, false); assert.strictEqual(latest.reason, 'Synthetic stop.');
});

test('live_targets_cannot_be_replaced_by_deferred_badges', async () => {
  const h = host(snapshot('listening')); const hold = h.deferGet(); const refreshing = h.voice.refresh();
  h.client.emit('voice', { kind: 'targets', runs: ['b'] });
  hold.release(); await refreshing;
  assert.deepStrictEqual([...h.voice.targeted], ['b']);
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
