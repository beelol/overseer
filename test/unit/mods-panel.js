// Execute the actual shipped webview with a small DOM. Assigning HTML is forbidden.
const assert = require('assert'), fs = require('fs'), vm = require('vm'), path = require('path');
class Node {
  constructor(tag, cls, text) { this.tagName = tag; this.className = cls || ''; this.children = []; this.dataset = {}; this.attrs = {}; this.listeners = {}; this.value = ''; this.id = ''; this.ownText = text || ''; this.classList = { add: c => { this.className += ' ' + c; } }; }
  set innerHTML(_) { throw new Error('Bundle text must never be assigned as HTML'); }
  get textContent() { return this.ownText + this.children.map(c => c.textContent).join(' '); }
  append(...nodes) { this.children.push(...nodes); }
  replaceChildren(...nodes) { this.children = nodes; }
  setAttribute(k, v) { this.attrs[k] = v; }
  addEventListener(k, cb) { (this.listeners[k] ||= []).push(cb); }
  dispatchEvent(e) { for (const cb of this.listeners[e.type] || []) cb({ target: this, preventDefault() {} }); }
  click() { if (!this.disabled) this.dispatchEvent({ type: 'click' }); }
  focus() { document.activeElement = this; }
  get lastChild() { return this.children.at(-1); }
  querySelectorAll(selector) { const all = this.children.flatMap(c => [c, ...c.querySelectorAll('*')]); const matches = (n, s) => s === '*' || (s === 'details[open]' ? n.tagName === 'details' && n.open : s === 'button[data-mutates]' ? n.tagName === 'button' && n.dataset.mutates : n.tagName === s); return all.filter(n => selector.split(',').some(s => matches(n, s))); }
  querySelector(s) { return this.querySelectorAll(s)[0]; }
}
const root = new Node('main'); root.id = 'mods';
const document = { activeElement: null, getElementById: id => [root, ...root.querySelectorAll('*')].find(n => n.id === id) };
const messages = [], listeners = {}, ui = { el: (tag, cls, text) => new Node(tag, cls, text) };
vm.runInNewContext(fs.readFileSync(path.join(__dirname, '../../extension/media/mods-panel.js'), 'utf8'), { document, window: { OverseerUI: ui, addEventListener: (k, cb) => { listeners[k] = cb; } }, acquireVsCodeApi: () => ({ postMessage: m => messages.push(m) }), Event: class { constructor(type) { this.type = type; } }, console });
const v = { id: 'clear-prose', version: '1', fingerprint: 'fp', source: '<script>source</script>', manifest: { name: 'Clear prose', summary: '<img onerror=bad>', homepage: 'javascript:bad' }, files: [] };
const T = require('../../extension/src/mods-text');
const data = { revision: 4, installed: [v], bindings: [], available_bundled: [v] };
const state = { type: 'mods', connected: true, trusted: true, data, library: T.library(data), applied: T.applied(), runs: [{ id: 'r1', title: 'One agent' }], repositories: [], story: [], noise: T.NOISE, qualification: T.QUALIFICATION };
const send = s => listeners.message({ data: s });
send(state);
assert.ok(root.textContent.includes('<img onerror=bad>'), 'untrusted summary is visible literally'); assert.strictEqual(root.querySelectorAll('img').length, 0);
const planned = root.querySelectorAll('div').find(n => n.className === 'mod-row' && n.textContent.startsWith('Less tool noise'));
assert.ok(planned); assert.strictEqual(planned.querySelectorAll('button,input').length, 0, 'planned bundle has no active switch or install');
assert.ok(!root.querySelectorAll('button').some(b => /source website/.test(b.textContent)), 'unsafe homepage omitted');
let enable = root.querySelectorAll('button').find(b => b.textContent === 'Enable for scope'); enable.click(); assert.strictEqual(messages.at(-1).action, 'bind'); assert.strictEqual(messages.at(-1).revision, 4);
send(state); const form = root.querySelector('form'); const input = form.querySelector('input'); input.checked = true; input.focus();
send({ ...state, data: { ...data, revision: 5 } }); assert.strictEqual(root.querySelector('form').querySelector('input').checked, true, 'event refresh preserves unfinished scope draft'); assert.strictEqual(document.activeElement.id, input.id, 'refresh preserves keyboard focus');
send({ ...state, trusted: false }); assert.ok(root.querySelectorAll('button[data-mutates]').every(b => b.disabled)); assert.ok(root.querySelector('form').querySelectorAll('input,select').every(n => n.disabled)); assert.match(root.textContent, /read-only/);
const p = { id: 'p1', version: v, operation: 'install', contents: { 'style.md': '</pre><script>bad</script>' }, files: [], previous: [] };
send({ ...state, preview: p }); assert.strictEqual(root.querySelectorAll('script').length, 0); assert.ok(root.textContent.includes('</pre><script>bad</script>'));
console.log('PASS actual webview: literal hostile text, inactive planned option, scoped revision, trust restrictions, preserved draft/focus and preview content');
