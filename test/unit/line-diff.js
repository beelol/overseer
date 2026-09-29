// The agent's head (AC-233): extension/src/line-diff.js, the line diff behind the inline
// annotations. Hunks must rebuild the new text from the old one exactly, and small edits must stay
// small (an added line is one added line, not a replaced block).
// Run: node test/unit/line-diff.js
const assert = require('assert');
const path = require('path');
const { diffLines, splitLines } = require(path.resolve(__dirname, '../../extension/src/line-diff.js'));

let failures = 0, passed = 0;
const test = (name, fn) => { try { fn(); passed++; console.log('ok  ', name); } catch (e) { failures++; console.log('FAIL', name, '\n    ' + e.message); } };

/** Applies hunks to the old lines; must give the new lines. */
function apply(before, after, hunks) {
  const a = splitLines(before), b = splitLines(after), out = [];
  let i = 0;
  for (const h of hunks) {
    while (i < h.origStart) out.push(a[i++]);
    for (let k = 0; k < h.modLen; k++) out.push(b[h.modStart + k]);
    i += h.origLen;
  }
  while (i < a.length) out.push(a[i++]);
  return out;
}
const lines = n => Array.from({ length: n }, (_, i) => `L${i + 1}`).join('\n') + '\n';

test('identical texts have no hunks', () => assert.deepStrictEqual(diffLines('a\nb\n', 'a\nb\n'), []));
test('one line added in the middle is one added hunk', () => {
  const before = 'a\nb\nc\n', after = 'a\nb\nnew\nc\n';
  assert.deepStrictEqual(diffLines(before, after), [{ origStart: 2, origLen: 0, modStart: 2, modLen: 1 }]);
});
test('one line removed is one removed hunk', () => {
  assert.deepStrictEqual(diffLines('a\nb\nc\n', 'a\nc\n'), [{ origStart: 1, origLen: 1, modStart: 1, modLen: 0 }]);
});
test('one line changed is a one-for-one hunk', () => {
  assert.deepStrictEqual(diffLines(lines(10), lines(10).replace('L7\n', 'L7 changed\n')), [{ origStart: 6, origLen: 1, modStart: 6, modLen: 1 }]);
});
test('a new file is all added', () => assert.deepStrictEqual(diffLines('', 'x\ny\n'), [{ origStart: 0, origLen: 0, modStart: 0, modLen: 2 }]));
test('distant edits are separate hunks and rebuild the text', () => {
  const before = lines(300);
  const after = before.replace('L10\n', 'L10 x\n').replace('L150\n', '').replace('L280\n', 'L280\nadded\n');
  const hunks = diffLines(before, after);
  assert.strictEqual(hunks.length, 3, JSON.stringify(hunks));
  assert.deepStrictEqual(apply(before, after, hunks), splitLines(after));
});
test('interleaved edits rebuild the text exactly (random cases)', () => {
  let seed = 7; const rnd = n => { seed = (seed * 1103515245 + 12345) & 0x7fffffff; return seed % n; };
  for (let t = 0; t < 300; t++) {
    const a = Array.from({ length: rnd(40) }, () => 'w' + rnd(6));
    const b = a.flatMap(l => { const r = rnd(10); return r === 0 ? [] : r === 1 ? [l, 'n' + rnd(6)] : r === 2 ? ['c' + rnd(6)] : [l]; });
    const before = a.join('\n') + (a.length ? '\n' : ''), after = b.join('\n') + (b.length ? '\n' : '');
    assert.deepStrictEqual(apply(before, after, diffLines(before, after)), splitLines(after), `case ${t}`);
  }
});
test('a rewrite past the edit budget is one replaced hunk that still rebuilds the text', () => {
  const before = Array.from({ length: 4000 }, (_, i) => 'a' + i).join('\n');
  const after = Array.from({ length: 4000 }, (_, i) => 'b' + i).join('\n');
  const hunks = diffLines(before, after);
  assert.strictEqual(hunks.length, 1);
  assert.deepStrictEqual(apply(before, after, hunks), splitLines(after));
});

console.log(`${passed} passed, ${failures} failed`);
process.exit(failures ? 1 : 0);
