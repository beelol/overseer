const assert = require('assert');
const Module = require('module');

const calls = [];
let choice;
let answer;
let save;
let provider;
const vscode = {
  Uri: { parse: value => ({ scheme: value.split(':')[0], path: value.slice(value.indexOf(':') + 1), toString: () => value }) },
  window: {
    showQuickPick: async (items, options) => { calls.push(['pick', items, options]); return choice; },
    showWarningMessage: async (...args) => { calls.push(['warn', args]); return answer; },
    showSaveDialog: async options => { calls.push(['save', options]); return save; },
    showInformationMessage: async message => { calls.push(['info', message]); },
    showTextDocument: async document => { calls.push(['show', document]); },
  },
  workspace: {
    registerTextDocumentContentProvider: (scheme, value) => { calls.push(['provider', scheme]); provider = value; return { dispose() {} }; },
    onDidCloseTextDocument: () => ({ dispose() {} }),
    openTextDocument: async uri => { calls.push(['document', uri]); return { uri, isDirty: false }; },
  },
};
const originalLoad = Module._load;
Module._load = function (request, ...rest) { return request === 'vscode' ? vscode : originalLoad.call(this, request, ...rest); };
const { AutoUsage, rows, report } = require('../../extension/src/auto-usage');
Module._load = originalLoad;

const record = { model: 'gpt-6-sol', effort: 'medium', status: 'completed', ended_ms: 1_000_000,
  usage: { input_tokens: 42, output_tokens: 7, cost_usd: null },
  subscription_window_draw: 'unverified', prompt: 'SECRET PROMPT', password: 'SECRET TOKEN' };
const requests = [];
const client = { request: async (method, params) => { requests.push([method, params]);
  if (method === 'auto.usage.work.list') return { work_units: [record], learning_paused: false };
  if (method === 'auto.usage.summary') return { aggregates: [], learning_paused: false };
  if (method === 'auto.usage.clear') return { deleted: 1 };
  if (method === 'auto.usage.export') return { path: params.path, count: 1 };
  throw new Error(`unexpected ${method}`);
} };

(async () => {
  const menu = rows([record], false);
  assert.match(menu[0].detail, /49 reported tokens/);
  assert.match(menu[0].detail, /subscription draw unverified/i);
  assert.ok(!JSON.stringify(menu).includes('SECRET'));
  assert.ok(!report(record).includes('SECRET'));
  assert.match(report(record), /not a subscription charge/i);
  assert.ok(!report({ ...record, usage: null }).includes('0 reported tokens'));
  const manifest = require('../../extension/package.json');
  assert.ok(manifest.contributes.commands.some(command => command.command === 'overseer.autoUsage'));
  // In the command palette (not hidden there), not in the Accounts header: an overflow menu in
  // that header sits where the side bar's first-click checks put focus (scenario-first-click).
  assert.ok(!(manifest.contributes.menus.commandPalette || []).some(item => item.command === 'overseer.autoUsage'));
  assert.ok(!manifest.contributes.menus['view/title'].some(item => item.command === 'overseer.autoUsage'));

  const context = { subscriptions: [] };
  const usage = new AutoUsage(client, context);
  choice = menu[0];
  await usage.show();
  assert.deepEqual(requests.map(x => x[0]), ['auto.usage.work.list']);
  const opened = calls.find(x => x[0] === 'document')[1];
  assert.equal(opened.scheme, 'overseer-auto-usage');
  assert.ok(calls.some(x => x[0] === 'provider' && x[1] === 'overseer-auto-usage'));
  assert.match(provider.provideTextDocumentContent(opened), /49 tokens \(42 input, 7 output\)/);
  assert.match(provider.provideTextDocumentContent(opened), /not a subscription charge/i);

  requests.length = 0; calls.length = 0;
  choice = { action: 'clear' }; answer = undefined;
  await usage.show();
  assert.ok(!requests.some(x => x[0] === 'auto.usage.clear'), 'cancelling keeps local learning');
  answer = 'Clear local Auto usage';
  await usage.show();
  assert.ok(requests.some(x => x[0] === 'auto.usage.clear'));
  assert.ok(calls.some(x => x[0] === 'warn' && x[1][1].modal === true));

  requests.length = 0; calls.length = 0;
  choice = { action: 'export' }; save = { scheme: 'file', fsPath: '/tmp/auto-usage.json' };
  await usage.show();
  assert.ok(requests.some(x => x[0] === 'auto.usage.export' && x[1].path === '/tmp/auto-usage.json'));
  assert.ok(calls.some(x => x[0] === 'save'));
  requests.length = 0; save = { scheme: 'vscode-remote', fsPath: '/tmp/remote.json' };
  await usage.show();
  assert.ok(!requests.some(x => x[0] === 'auto.usage.export'), 'a remote URI must not become a local daemon path');
  console.log('Auto local usage inspection, clear, and export passed');
})().catch(error => { console.error(error); process.exitCode = 1; });
