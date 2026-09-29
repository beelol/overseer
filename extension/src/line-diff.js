// Line diff for the agent's head (AC-233): the changes an agent made to a file, as hunks, for the
// inline annotations in its real file. Myers' O((N+M)D) algorithm on lines after trimming the
// common start and end; past MAX_D edits the middle is one replaced hunk (a rewrite reads as one).
const MAX_D = 1500;

function splitLines(text) {
  if (!text) return [];
  const lines = text.split(/\r\n|\r|\n/);
  // A final newline ends the last line; it does not start another.
  if (lines.length && lines[lines.length - 1] === '') lines.pop();
  return lines;
}

/**
 * Hunks turning `before` into `after`: { origStart, origLen, modStart, modLen } with 0-based starts
 * (origLen 0: lines added before origStart; modLen 0: lines removed before modStart).
 */
function diffLines(before, after) {
  const a = Array.isArray(before) ? before : splitLines(before);
  const b = Array.isArray(after) ? after : splitLines(after);
  let start = 0;
  while (start < a.length && start < b.length && a[start] === b[start]) start++;
  let endA = a.length, endB = b.length;
  while (endA > start && endB > start && a[endA - 1] === b[endB - 1]) { endA--; endB--; }
  if (start === endA && start === endB) return [];
  if (start === endA || start === endB) return [{ origStart: start, origLen: endA - start, modStart: start, modLen: endB - start }];
  const ops = myers(a, b, start, endA, start, endB);
  if (!ops) return [{ origStart: start, origLen: endA - start, modStart: start, modLen: endB - start }];
  return ops;
}

function myers(a, b, a0, a1, b0, b1) {
  const n = a1 - a0, m = b1 - b0, max = n + m, offset = max;
  let v = new Int32Array(2 * max + 2);
  const trace = [];
  let found = -1;
  for (let d = 0; d <= Math.min(max, MAX_D); d++) {
    trace.push(v.slice(offset - d - 1 < 0 ? 0 : offset - d - 1, offset + d + 2));
    for (let k = -d; k <= d; k += 2) {
      let x = (k === -d || (k !== d && v[offset + k - 1] < v[offset + k + 1])) ? v[offset + k + 1] : v[offset + k - 1] + 1;
      let y = x - k;
      while (x < n && y < m && a[a0 + x] === b[b0 + y]) { x++; y++; }
      v[offset + k] = x;
      if (x >= n && y >= m) { found = d; break; }
    }
    if (found >= 0) break;
  }
  if (found < 0) return null;
  // Walk back through the saved frontiers: each step is one removal or one addition.
  const edits = []; // { kind: 'del' | 'add', x, y } in local coordinates
  let x = n, y = m;
  for (let d = found; d > 0; d--) {
    const saved = trace[d];
    const base = offset - d - 1 < 0 ? 0 : offset - d - 1;
    const at = k => saved[offset + k - base];
    const k = x - y;
    const down = k === -d || (k !== d && at(k - 1) < at(k + 1));
    const prevK = down ? k + 1 : k - 1;
    const prevX = at(prevK), prevY = prevX - prevK;
    while (x > prevX + (down ? 0 : 1) && y > prevY + (down ? 1 : 0)) { x--; y--; }
    if (down) edits.push({ kind: 'add', x: prevX, y: prevY }); else edits.push({ kind: 'del', x: prevX, y: prevY });
    x = prevX; y = prevY;
  }
  edits.reverse();
  // Consecutive edits with no common line between them form one hunk.
  const hunks = [];
  let cur;
  for (const e of edits) {
    const oa = a0 + e.x, ob = b0 + e.y;
    if (cur && cur.origStart + cur.origLen === oa && cur.modStart + cur.modLen === ob) {
      if (e.kind === 'del') cur.origLen++; else cur.modLen++;
    } else {
      cur = { origStart: oa, origLen: e.kind === 'del' ? 1 : 0, modStart: ob, modLen: e.kind === 'add' ? 1 : 0 };
      hunks.push(cur);
    }
  }
  return hunks;
}

module.exports = { diffLines, splitLines };
