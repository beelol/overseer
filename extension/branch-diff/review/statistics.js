// Version-specific adapter for pinned Monaco 0.56.0. Keep endpoint/count regressions
// when changing this import; do not substitute a second diff implementation.
import { DefaultLinesDiffComputer } from 'monaco-editor/editor/common/diff/defaultLinesDiffComputer/defaultLinesDiffComputer.js';
const computer = new DefaultLinesDiffComputer();
export function statistics(original, modified) {
  const split = text => text.split(/\r\n|\r|\n/);
  const before = split(original), after = split(modified);
  const started = performance.now();
  const result = computer.computeDiff(before, after, { ignoreTrimWhitespace: false,
    computeMoves: false, maxComputationTimeMs: 500 });
  if (result.hitTimeout || performance.now() - started > 500) return { reason: 'Diff classification exceeded its 500 ms budget. Counts are unavailable.' };
  // Monaco models include a phantom final line after a terminating newline.
  const lines = (text, value) => text ? value.length - (/[\r\n]$/.test(text) ? 1 : 0) : 0;
  const count = (range, limit) => Math.max(0, Math.min(range.endLineNumberExclusive - 1, limit) - range.startLineNumber + 1);
  return { additions: result.changes.reduce((n, change) => n + count(change.modified, lines(modified, after)), 0),
    deletions: result.changes.reduce((n, change) => n + count(change.original, lines(original, before)), 0) };
}
