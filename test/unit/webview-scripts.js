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
// Inline scripts in generated HTML.
const withHtml = file => {
  const src = fs.readFileSync(path.join(ext, file), 'utf8') + '\nmodule.exports.__html = html;';
  const m = { exports: {} };
  new Function('require', 'module', 'exports', '__dirname', src)(r => Module._load(r.startsWith('.') ? path.join(ext, path.dirname(file), r) : r, module), m, m.exports, path.join(ext, path.dirname(file)));
  return m.exports.__html;
};
const outputHtml = withHtml('src/output-panel.js')('nonce', 'csp', 'r-test', { js: 'x.js', css: 'x.css' });
const inline = [...outputHtml.matchAll(/<script nonce="nonce">([\s\S]*?)<\/script>/g)].map(m => m[1]);
if (!inline.length) { failures++; console.log('FAIL', 'src/output-panel.js: no inline script found'); }
inline.forEach((code, i) => parse(`src/output-panel.js inline script ${i + 1}`, code));
console.log(failures ? `${failures} webview script(s) do not parse` : 'all webview scripts parse');
process.exit(failures ? 1 : 0);
