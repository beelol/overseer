// Mods management uses daemon truth; previews cannot silently enable text.
const assert = require('assert');
const Module = require('module');
const { EventEmitter } = require('events');
const tests = []; const test = (name, fn) => tests.push([name, fn]);
const version = { id: 'clear-prose', version: '1', fingerprint: 'fp', source: 'bundled:clear-prose', manifest: { name: 'Clear prose', summary: '<script>bad</script>', homepage: 'javascript:bad' }, files: [] };
const list = () => ({ revision: 7, installed: [version], available_bundled: [version], bindings: [], support: { children: 'unknown', native_configuration: 'unverified' } });
const words = () => require('../../extension/src/mods-text');
const disposable = () => ({ dispose() {} });
let choice = true;
const vscode = { workspace: { isTrusted: true, onDidGrantWorkspaceTrust: disposable }, ViewColumn: { Active: 1 },
  Uri: { joinPath: (_, ...p) => ({ toString: () => p.join('/') }), parse: s => s }, env: { openExternal: async () => true },
  window: { showWarningMessage: async (_, __, yes) => choice ? yes : undefined, showOpenDialog: async () => undefined } };
function host() {
  const load = Module._load; Module._load = function (name, ...rest) { return name === 'vscode' ? vscode : load.call(this, name, ...rest); };
  let ModsPanel; try { ({ ModsPanel } = require('../../extension/src/mods')); } finally { Module._load = load; }
  const client = new EventEmitter(); client.connected = true; client.calls = [];
  client.request = async (method, params) => { client.calls.push({ method, params }); if (method === 'mods.list') return list(); if (method === 'mods.preview') return { id: 'preview1', version, contents: { 'style.md': '<img onerror=bad>' }, previous: [] }; return { revision: 8 }; };
  const h = new ModsPanel({ context: { subscriptions: [], extensionUri: {} }, client, model: { all: { runs: [], workspaces: [] } }, log() {} });
  h.data = list(); return { h, client };
}
test('desired differs from last accepted turn without claiming application', () => {
  const a = words().applied({ pending: true, desired: { versions: [version], decisions: [] }, last_turn: { outcome: 'transport_accepted', delivery: 'message_text', added_bytes: 120, applied_fingerprints: ['old'] } });
  assert.match(a.state, /Pending/); assert.match(a.last, /accepted/i); assert.match(a.desired, /Clear prose/); assert.match(a.coverage, /unknown/i);
  const failed = words().applied({ pending: true, desired: { versions: [version], decisions: [] }, last_turn: { outcome: 'failed_before_effect', delivery: 'message_text', applied_fingerprints: ['fp'] } });
  assert.doesNotMatch(failed.last, /delivered|accepted/i);
});
test('unsupported decisions and next-thread changes stay explicit', () => {
  const a = words().applied({ pending: true, desired: { versions: [], decisions: [{ mod_id: 'clear-prose', status: 'unqualified', reason: 'Dynamic local model unsupported', delivery: 'unsupported', activation: 'next_thread', children: 'unknown' }] }, last_turn: null });
  assert.match(a.state, /thread/); assert.match(a.decisions[0], /unsupported/); assert.match(a.last, /No recorded/);
});
test('library copy distinguishes installed from enabled and makes no savings claim', () => {
  const a = words().library(list()); assert.strictEqual(a[0].state, 'Installed; no enabled bindings'); assert.match(a[0].pin, /1.*fp/);
  assert.match(words().NOISE, /Planned; not available/); assert.match(words().QUALIFICATION, /not measured/);
});
test('HTML and links reject active content', () => {
  assert.strictEqual(words().escape('<script>"&\''), '&lt;script&gt;&quot;&amp;&#39;');
  for (const v of ['javascript:alert(1)', 'file:///tmp/a', 'data:text/html,x', '//evil', 'https://u:p@host/']) assert.strictEqual(words().safeUrl(v), null);
  assert.strictEqual(words().safeUrl('https://example.com/docs'), 'https://example.com/docs');
});
test('preview installs only after confirmation and never binds', async () => {
  const { h, client } = host(); await h.handle({ action: 'preview', source: 'bundled:clear-prose', operation: 'install' }); assert.strictEqual(client.calls[0].method, 'mods.preview');
  choice = false; await h.handle({ action: 'install', previewId: 'preview1' }); assert.ok(!client.calls.some(c => c.method === 'mods.install'));
  choice = true; await h.handle({ action: 'install', previewId: 'preview1' }); assert.deepStrictEqual(client.calls.find(c => c.method === 'mods.install').params, { preview_id: 'preview1', confirm: true }); assert.ok(!client.calls.some(c => c.method === 'mods.bind'));
});
test('stale UI revision is refused rather than silently rebased', async () => {
  const { h, client } = host(); await assert.rejects(h.handle({ action: 'bind', revision: 6, fingerprint: 'fp', scope: { kind: 'overseer' }, enabled: true }), /changed|refresh/i); assert.strictEqual(client.calls.length, 0);
  await h.handle({ action: 'bind', revision: 7, fingerprint: 'fp', scope: { kind: 'overseer' }, enabled: true }); const c = client.calls.find(c => c.method === 'mods.bind'); assert.strictEqual(c.params.expected_revision, 7); assert.strictEqual(c.params.binding.scope.kind, 'overseer');
});
test('untrusted workspace refuses forged mutation messages before RPC', async () => {
  const { h, client } = host(); vscode.workspace.isTrusted = false;
  try { for (const action of ['preview', 'install', 'bind', 'unbind', 'remove']) await assert.rejects(h.handle({ action }), /trusted workspace/i); assert.strictEqual(client.calls.length, 0); await h.handle({ action: 'refresh' }); assert.ok(client.calls.some(c => c.method === 'mods.list')); }
  finally { vscode.workspace.isTrusted = true; }
});
test('disconnect invalidates preview and reconnect reads authoritative state', async () => {
  const { h, client } = host(); await h.handle({ action: 'preview', source: 'bundled:clear-prose', operation: 'install' }); client.emit('disconnected');
  await assert.rejects(h.handle({ action: 'install', previewId: 'preview1' }), /preview|connect/i);
  client.connected = true; await h.refresh(); assert.strictEqual(h.data.revision, 7); h.dispose();
});
test('cancelled confirmation releases the webview loading state', async () => {
  const { h } = host(); const sent = []; h.panel = { webview: { postMessage: m => sent.push(m) } }; await h.handle({ action: 'preview', source: 'bundled:clear-prose', operation: 'install' }); sent.length = 0;
  choice = false; await h.handle({ action: 'install', previewId: 'preview1' }); assert.ok(sent.length, 'cancel must redraw so controls become usable again'); choice = true;
});
test('removal confirmation carries displayed revision and preserves scope until accepted', async () => {
  const { h, client } = host(); choice = false; await h.handle({ action: 'remove', revision: 7, fingerprint: 'fp' }); assert.ok(!client.calls.some(c => c.method === 'mods.remove'));
  choice = true; await h.handle({ action: 'remove', revision: 7, fingerprint: 'fp' }); assert.deepStrictEqual(client.calls.find(c => c.method === 'mods.remove').params, { mod_id: 'clear-prose', fingerprint: 'fp', confirm: true, expected_revision: 7 });
});
test('binding edits preserve owner filters and locks; unbind removes override', async () => {
  const { h, client } = host(); h.data.bindings = [{ id: 'b1', fingerprint: 'fp', scope: { kind: 'watchers' }, enabled: true, required: true, locked: true, filters: { harnesses: ['claude'], accounts: ['a1'], models: ['m1'] } }];
  await h.handle({ action: 'bind', revision: 7, bindingId: 'b1', fingerprint: 'fp', enabled: false }); const b = client.calls.find(c => c.method === 'mods.bind').params.binding; assert.strictEqual(b.locked, true); assert.strictEqual(b.required, true); assert.deepStrictEqual(b.filters.models, ['m1']); assert.strictEqual(b.scope.kind, 'watchers');
  h.data.bindings = [{ id: 'b1' }]; await h.handle({ action: 'unbind', revision: 7, bindingId: 'b1' }); assert.deepStrictEqual(client.calls.find(c => c.method === 'mods.unbind').params, { binding_id: 'b1', expected_revision: 7 });
});
test('unsafe external links never reach VS Code opener', async () => { const { h } = host(); await assert.rejects(h.handle({ action: 'link', url: 'javascript:alert(1)' }), /HTTP/); });
test('a run without a snapshot never claims matching delivery', () => { const a = words().applied({ pending: false, desired: { versions: [], decisions: [] }, last_turn: null }); assert.doesNotMatch(a.state, /match/); assert.match(a.last, /No recorded/); });
test('prepared and uncertain snapshots never read as accepted delivery', () => {
  for (const outcome of ['prepared', 'uncertain_after_effect']) { const a = words().applied({ pending: true, desired: { versions: [], decisions: [] }, last_turn: { outcome, delivery: 'message_text' } }); assert.doesNotMatch(a.last, /accepted by transport|text delivered/i); }
});
(async () => { let failed = 0; for (const [name, fn] of tests) { try { await fn(); console.log('PASS', name); } catch (e) { failed++; console.error('FAIL', name, e.message); } } console.log(`${tests.length - failed}/${tests.length} Mods checks passed`); process.exitCode = failed ? 1 : 0; })();
