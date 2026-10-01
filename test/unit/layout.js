// Layout decisions (extension/src/layout.js): which tabs are Overseer's, when Overseer opens beside
// the owner's groups instead of rebuilding the layout (AC-244).
// Run: node test/unit/layout.js
const assert = require('assert');
const path = require('path');
const { isOurs, ownerGroups, besideOwner } = require(path.resolve(__dirname, '../../extension/src/layout.js'));

let failures = 0, passed = 0;
const test = (name, fn) => { try { fn(); passed++; console.log('ok  ', name); } catch (e) { failures++; console.log('FAIL', name, '-', e.message); } };

class TabInputText { constructor(uri) { this.uri = uri; } }
class TabInputCustom { constructor(uri, viewType) { this.uri = uri; this.viewType = viewType; } }
class TabInputWebview { constructor(viewType) { this.viewType = viewType; } }
const uri = s => ({ toString: () => s });
const tab = (label, input, extra = {}) => ({ label, input, isActive: false, isPinned: false, isPreview: false, isDirty: false, ...extra });
const file = (name, extra) => tab(name, new TabInputText(uri('file:///repo/' + name)), extra);
const chat = tab('Overseer', new TabInputWebview('mainThreadWebview-overseer.center'));
const review = tab('Review: Fix it', new TabInputWebview('mainThreadWebview-overseer.review'));

test('Overseer\'s view, reviews, chats and the chat editor are Overseer\'s; files, diffs and other webviews are not', () => {
  assert.ok(isOurs(chat) && isOurs(review));
  assert.ok(isOurs(tab('Agent', new TabInputWebview('mainThreadWebview-overseer.output'))));
  assert.ok(isOurs(tab('x.overseer-chat', new TabInputCustom(uri('overseer-chat:/r/x'), 'overseer.chatEditor'))));
  assert.ok(!isOurs(file('a.txt')));
  assert.ok(!isOurs(tab('Welcome', undefined)));
  assert.ok(!isOurs(tab('Markdown Preview', new TabInputWebview('mainThreadWebview-markdown.preview'))));
});

test('AC-244: two groups with the owner\'s files: Overseer opens beside them', () => {
  const groups = [{ viewColumn: 1, tabs: [file('a.txt')] }, { viewColumn: 2, tabs: [file('b.txt')] }];
  assert.strictEqual(besideOwner(groups), true);
  assert.strictEqual(ownerGroups(groups).length, 2);
});

test('AC-244: one group, or only Overseer\'s own views: the usual arrangement (nothing of the owner\'s to merge)', () => {
  assert.strictEqual(besideOwner([{ viewColumn: 1, tabs: [file('a.txt')] }]), false);
  assert.strictEqual(besideOwner([{ viewColumn: 1, tabs: [review] }, { viewColumn: 2, tabs: [chat] }]), false);
  assert.strictEqual(besideOwner([{ viewColumn: 1, tabs: [] }]), false);
});

test('AC-244: a file opened from the review into the review\'s group keeps the usual arrangement', () => {
  assert.strictEqual(besideOwner([{ viewColumn: 1, tabs: [review, file('a.txt')] }, { viewColumn: 2, tabs: [chat] }]), false);
});

test('AC-244: once Overseer sits beside two owner groups, it stays beside them (review and chat added)', () => {
  const groups = [{ viewColumn: 1, tabs: [file('a.txt')] }, { viewColumn: 2, tabs: [file('b.txt')] }, { viewColumn: 3, tabs: [review] }, { viewColumn: 4, tabs: [chat] }];
  assert.strictEqual(besideOwner(groups), true);
});

console.log(`${passed} of ${passed + failures} passed`);
process.exit(failures ? 1 : 0);
