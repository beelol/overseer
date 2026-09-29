// Layout decisions (extension/src/layout.js): which tabs are Overseer's, when Overseer opens beside
// the owner's groups instead of rebuilding the layout (AC-244), what the workspace keeps of the
// owner's tabs to put them back (AC-250), and its three columns sized for the screen.
// Run: node test/unit/layout.js
const assert = require('assert');
const path = require('path');
const { isOurs, ownerGroups, besideOwner, snapshotTabs, straysToClose, workspaceColumns } = require(path.resolve(__dirname, '../../extension/src/layout.js'));

let failures = 0, passed = 0;
const test = (name, fn) => { try { fn(); passed++; console.log('ok  ', name); } catch (e) { failures++; console.log('FAIL', name, '-', e.message); } };

class TabInputText { constructor(uri) { this.uri = uri; } }
class TabInputTextDiff { constructor(original, modified) { this.original = original; this.modified = modified; } }
class TabInputCustom { constructor(uri, viewType) { this.uri = uri; this.viewType = viewType; } }
class TabInputWebview { constructor(viewType) { this.viewType = viewType; } }
const kinds = { TabInputText, TabInputTextDiff, TabInputCustom };
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

test('AC-250: the snapshot keeps each group\'s tabs in order with the active one; Overseer\'s tabs are left out', () => {
  const groups = [
    { viewColumn: 1, isActive: true, tabs: [file('a.txt'), file('b.txt', { isActive: true, isPinned: true }), chat] },
    { viewColumn: 2, isActive: false, tabs: [tab('a.txt ↔ b.txt', new TabInputTextDiff(uri('file:///repo/a.txt'), uri('file:///repo/b.txt')), { isActive: true }), tab('Welcome', undefined)] },
  ];
  const snap = snapshotTabs(groups, kinds);
  assert.deepStrictEqual(snap.map(g => g.tabs.map(t => t.label)), [['a.txt', 'b.txt'], ['a.txt ↔ b.txt', 'Welcome']]);
  assert.deepStrictEqual(snap[0].tabs[1], { label: 'b.txt', active: true, pinned: true, preview: false, dirty: false, kind: 'text', uri: 'file:///repo/b.txt' });
  assert.strictEqual(snap[1].tabs[0].kind, 'diff');
  assert.strictEqual(snap[1].tabs[0].modified, 'file:///repo/b.txt');
  assert.strictEqual(snap[1].tabs[1].kind, 'other');
  assert.strictEqual(snap[0].active, true);
});

test('AC-250: unsaved tabs are never closed; everything else of the owner\'s is', () => {
  const snap = snapshotTabs([{ viewColumn: 1, tabs: [file('a.txt'), file('draft.txt', { isDirty: true }), tab('Welcome', undefined)] }], kinds);
  assert.deepStrictEqual(straysToClose(snap).map(t => t.label), ['a.txt', 'Welcome']);
});

test('AC-250: at 1440 px the side bar gives way and each side column keeps at least 380 px', () => {
  // 1440 wide: activity bar 48, side bar 300 → editor 1092.
  const c = workspaceColumns(1092, 300);
  assert.strictEqual(c.hideSideBar, true);
  assert.ok(Math.abs(c.sizes.reduce((a, b) => a + b, 0) - 1) < 1e-9);
  assert.ok(c.sizes[0] * 1392 >= 379 && c.sizes[2] * 1392 >= 379, JSON.stringify(c));
  assert.ok(c.sizes[1] > c.sizes[0], 'the review is the widest column');
});

test('AC-250: at 1920 px the side bar stays and the review is the widest column', () => {
  const c = workspaceColumns(1572, 300);
  assert.strictEqual(c.hideSideBar, false);
  assert.ok(c.sizes[0] * 1572 >= 379 && c.sizes[1] > c.sizes[0], JSON.stringify(c));
});

test('AC-250: an unknown width falls back to thirds-ish without hiding anything', () => {
  const c = workspaceColumns(undefined, 0);
  assert.strictEqual(c.hideSideBar, false);
  assert.deepStrictEqual(c.sizes.map(x => Math.round(x * 10) / 10), [0.3, 0.4, 0.3]);
});

console.log(`${passed} of ${passed + failures} passed`);
process.exit(failures ? 1 : 0);
