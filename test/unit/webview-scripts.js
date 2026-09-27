// Parses every webview script Overseer ships, including the inline scripts generated inside
// template strings (where a stray quote only breaks at runtime, silently blanking a panel).
// Run: node test/unit/webview-scripts.js
const fs = require('fs');
const path = require('path');
const Module = require('module');

const ext = path.resolve(__dirname, '../../extension');
const stubs = { vscode: { ViewColumn: { One: 1, Two: 2, Three: 3 }, Uri: { joinPath: () => ({}) } } };
const load = Module._load;
Module._load = function (request, ...rest) { return stubs[request] || load.call(this, request, ...rest); };
let failures = 0;
const parse = (label, code) => { try { new Function(code); console.log('ok  ', label); } catch (e) { failures++; console.log('FAIL', label, '-', e.message); } };

for (const f of fs.readdirSync(path.join(ext, 'media')).filter(f => f.endsWith('.js'))) parse(`media/${f}`, fs.readFileSync(path.join(ext, 'media', f), 'utf8'));
// Inline scripts in the generated page shell (src/webview-html.js) used by every Overseer webview.
stubs.vscode.Uri.joinPath = (...parts) => ({ parts });
const { page } = require(path.join(ext, 'src/webview-html.js'));
const html = page({ cspSource: 'csp', asWebviewUri: u => ({ toString: () => 'x.js' }) }, {}, { title: 'Test', chat: true, js: ['dashboard.js'], body: '<main></main>' });
const inline = [...html.matchAll(/<script nonce="[^"]+">([\s\S]*?)<\/script>/g)].map(m => m[1]);
if (!inline.length) { failures++; console.log('FAIL', 'src/webview-html.js: no inline script found'); }
inline.forEach((code, i) => parse(`src/webview-html.js inline script ${i + 1}`, code));
console.log(failures ? `${failures} webview script(s) do not parse` : 'all webview scripts parse');
process.exit(failures ? 1 : 0);
