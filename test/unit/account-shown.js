// AC-235: you can always see which account an agent uses. The side bar's agent row, its tooltip and
// its accessible name carry the daemon's account label (provider and plan, the shortened email,
// whose login it is); the Mac's own login is "Mac's default login"; and no surface says only
// "Your login" (a check over the words of the extension, the TUI and the phone app).
// Run: node test/unit/account-shown.js
const fs = require('fs');
const path = require('path');
const Module = require('module');
const assert = require('assert');
const root = path.resolve(__dirname, '../..');

class TreeItem { constructor(label, state) { this.label = label; this.collapsibleState = state; } }
class MarkdownString { constructor(value) { this.value = value; } appendMarkdown(t) { this.value += t; } }
class EventEmitter { constructor() { this.event = () => ({ dispose() {} }); } fire() {} }
const vscode = { TreeItem, MarkdownString, EventEmitter, ThemeIcon: class {}, ThemeColor: class {}, TreeItemCollapsibleState: { None: 0, Collapsed: 1, Expanded: 2 },
  Uri: { from: x => x, joinPath: (b, ...m) => ({ path: [b.path, ...m].join('/') }) }, workspace: { getConfiguration: () => ({ get: (k, d) => d }) } };
const load = Module._load;
Module._load = function (request, ...rest) { return request === 'vscode' ? vscode : load.call(this, request, ...rest); };
const views = require(path.join(root, 'extension/src/views.js'));
Module._load = load;

const mac = { id: 'system-claude', name: 'claude (existing login)', harness: 'claude', is_system: true,
  account: { provider: 'Claude', plan: 'Max', email: 'bil…@testbox.com', default: true, name: "Mac's default login", label: "Claude Max · bil…@testbox.com · Mac's default login", short: 'Claude Max · bil…@testbox.com' } };
const work = { id: 'p-1', name: 'Work ChatGPT', harness: 'codex', is_system: false,
  account: { provider: 'ChatGPT', plan: 'Team', email: 'wor…@acme.example', default: false, name: 'Work ChatGPT', label: 'ChatGPT Team · wor…@acme.example · Work ChatGPT', short: 'ChatGPT Team · wor…@acme.example' } };

// The names and labels.
assert.strictEqual(views.accountName(mac), "Mac's default login");
assert.strictEqual(views.accountName({ is_system: true, name: 'codex (existing login)' }), "Mac's default login", 'an older daemon without labels still never says "Your login"');
assert.strictEqual(views.accountLabel(mac), mac.account.label);
assert.strictEqual(views.accountShort(work), work.account.short);

// The side bar's rows for an agent on each account.
const now = Date.now();
const state = {
  tasks: [{ id: 't1', title: 'Fix login', repo_root: '/r/app', workspace_id: 'w1' }, { id: 't2', title: 'Write docs', repo_root: '/r/app', workspace_id: 'w2' }],
  runs: [{ id: 'r1', task_id: 't1', harness: 'claude', profile_id: 'system-claude', status: 'running', created_ms: now, workspace_id: 'w1', title: 'Fix login' },
    { id: 'r2', task_id: 't2', harness: 'codex', profile_id: 'p-1', status: 'completed', created_ms: now - 60000, ended_ms: now - 30000, workspace_id: 'w2', title: 'Write docs' }],
  workspaces: [{ id: 'w1', kind: 'worktree', branch: 'overseer/fix-login' }, { id: 'w2', kind: 'worktree', branch: 'overseer/docs' }],
  profiles: [mac, work], turns: {},
};
const model = new views.Model({ request: async () => state });
model.state = model.all = state;
const bar = new views.AgentsProvider(model, undefined, undefined, {});
const rows = [];
const walk = (node, depth) => { for (const n of bar.getChildren(node)) { rows.push(n); if (depth < 4) walk(n, depth + 1); } };
walk(undefined, 0);
const agentRow = id => rows.find(n => n.run && n.run.id === id && n.task);
for (const [id, p] of [['r1', mac], ['r2', work]]) {
  const row = agentRow(id);
  assert.ok(row, `the side bar lists ${id}`);
  assert.ok(row.item.description.includes(`${p.account.plan} · ${p.account.email}`), `row description names the account: ${row.item.description}`);
  assert.ok(row.item.tooltip.value.includes(p.account.label), `tooltip names the account: ${row.item.tooltip.value}`);
  assert.ok(row.item.accessibilityInformation.label.includes(p.account.label), 'the accessible name says the account');
}

// No surface says only "Your login": the words of the extension, the TUI and the phone app.
const hits = [];
const scan = dir => {
  for (const e of fs.readdirSync(path.join(root, dir), { withFileTypes: true })) {
    const rel = path.join(dir, e.name);
    if (e.isDirectory()) { if (!['node_modules', 'vendor', '__tests__', 'branch-diff'].includes(e.name)) scan(rel); continue; }
    if (!/\.(js|ts|tsx|rs|json)$/.test(e.name)) continue;
    const text = fs.readFileSync(path.join(root, rel), 'utf8');
    for (const [i, line] of text.split('\n').entries()) if (/\bYour login\b/.test(line) && !/^\s*(\/\/|\*|\/\*)/.test(line)) hits.push(`${rel}:${i + 1}: ${line.trim().slice(0, 120)}`);
  }
};
for (const dir of ['extension/src', 'extension/media', 'tui/src', 'phone/src', 'phone/app', 'phone/model/src']) scan(dir);
for (const h of hits) console.log('FAIL', h);
assert.strictEqual(hits.length, 0, `${hits.length} place(s) still say "Your login"`);
console.log('every agent names its account; nothing says only "Your login"');
