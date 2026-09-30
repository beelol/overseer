// AC-243: what an agent's work became, in the words every surface uses (extension/media/landing-text.js;
// the TUI's `landing_text` in tui/src/model.rs says the same). Run: node test/unit/landing-text.js
const assert = require('assert');
const Text = require('../../extension/media/landing-text.js');

assert.strictEqual(Text.text({ state: 'merged', target: 'main', commit: '1a2b3c4d5e6f' }), 'Merged into main (1a2b3c4)');
assert.strictEqual(Text.text({ state: 'merged', target: 'release' }), 'Merged into release');
assert.strictEqual(Text.text({ state: 'conflicts', files: ['a.txt', 'b.txt'] }), 'Merge stopped: conflicts in a.txt, b.txt');
assert.strictEqual(Text.text({ state: 'conflicts', files: [] }), 'Merge stopped: conflicts');
assert.strictEqual(Text.text({ state: 'pr', url: 'https://github.com/o/r/pull/7' }), 'Pull request #7 open');
assert.strictEqual(Text.text({ state: 'pr', url: 'https://example.invalid/x' }), 'Pull request open');
assert.strictEqual(Text.text(null), '');
assert.strictEqual(Text.text({ state: 'something new' }), '');
assert.strictEqual(Text.mergeLabel('main'), 'Merge into main');
console.log('landing words ok');
