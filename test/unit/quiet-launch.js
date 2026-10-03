// Quiet-launch breakpoint discovery only: never launches VS Code or executes installed source.
const assert = require('assert');
const fs = require('fs');
const os = require('os');
const path = require('path');
const { pathToFileURL } = require('url');
const { quietWindowBreakpoints } = require('../ui/quiet-launch');
const root = fs.mkdtempSync(path.join(os.tmpdir(), 'overseer-quiet-launch-'));
let passed = 0, failed = 0;
const test = (name, fn) => { try { fn(); passed++; console.log('ok  ', name); } catch (e) { failed++; console.error('FAIL', name, '-', e.message); } };
const fixture = (name, files) => {
  const app = path.join(root, name + '.app');
  const out = path.join(app, 'Contents/Resources/app/out');
  fs.mkdirSync(out, { recursive: true });
  for (const [file, source] of Object.entries(files)) fs.writeFileSync(path.join(out, file), source);
  return app;
};
try {
  test('old main constructor is paused before its exact options are changed', () => {
    const app = fixture('Old VS Code', { 'main.js': 'bootstrap();\n  const win = new electron.BrowserWindow(opts);\n' });
    const [b] = quietWindowBreakpoints(app);
    assert.strictEqual(b.lineNumber, 1); assert.strictEqual(b.columnNumber, 14);
    assert.match(b.condition, /opts\.show = false/);
    assert.match(b.condition, /__overseerQuietShow\+\+/);
    assert.ok(new RegExp(b.urlRegex).test(pathToFileURL(path.join(app, 'Contents/Resources/app/out/main.js')).href));
  });
  test('imported mainImpl constructor is registered before it has been loaded', () => {
    const app = fixture('New VS Code', { 'main.js': 'await import("./mainImpl.js");', 'mainImpl.js': 'start();\nwin=new Jn.BrowserWindow(et);' });
    const [b] = quietWindowBreakpoints(app);
    assert.strictEqual(b.lineNumber, 1); assert.strictEqual(b.columnNumber, 4);
    assert.match(b.condition, /et\.show = false/);
    const target = path.join(app, 'Contents/Resources/app/out/mainImpl.js');
    for (const url of [target, 'file://' + target, pathToFileURL(target).href]) assert.ok(new RegExp(b.urlRegex).test(url), url);
    assert.ok(!new RegExp(b.urlRegex).test('file:///another/Code.app/Contents/Resources/app/out/mainImpl.js'));
  });
  test('unknown layout fails before launching instead of following arbitrary imports', () => {
    const app = fixture('Unknown Code', { 'main.js': 'await import("./external.js");', 'external.js': 'new E.BrowserWindow(opts)' });
    assert.throws(() => quietWindowBreakpoints(app), /window constructor not found/);
  });
  test('reading installed source never runs its top-level code', () => {
    const app = fixture('Read only Code', { 'main.js': 'globalThis.__quietSourceExecuted = true; new E.BrowserWindow(opts);' });
    quietWindowBreakpoints(app);
    assert.strictEqual(globalThis.__quietSourceExecuted, undefined);
  });
  test('source discovery has a bounded size', () => {
    const app = fixture('Huge Code', { 'main.js': '' });
    fs.truncateSync(path.join(app, 'Contents/Resources/app/out/main.js'), 16 * 1024 * 1024 + 1);
    assert.throws(() => quietWindowBreakpoints(app), /too large/);
  });
} finally { fs.rmSync(root, { recursive: true, force: true }); }
console.log(`${passed} of ${passed + failed} passed`);
process.exitCode = failed ? 1 : 0;
