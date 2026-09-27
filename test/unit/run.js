// Runs every extension unit test in test/unit and reports how many passed (used by npm test and scripts/test-all).
const fs = require('fs');
const path = require('path');
const cp = require('child_process');
const files = fs.readdirSync(__dirname).filter(f => f.endsWith('.js') && f !== 'run.js').sort();
let passed = 0;
for (const f of files) {
  const r = cp.spawnSync(process.execPath, [path.join(__dirname, f)], { encoding: 'utf8' });
  const ok = r.status === 0;
  if (ok) passed++;
  console.log(`${ok ? 'ok    ' : 'FAILED'} ${f}${ok ? '' : '\n' + (r.stdout + r.stderr).split('\n').slice(-15).join('\n')}`);
}
console.log(`${passed} of ${files.length} passed`);
process.exit(passed === files.length ? 0 : 1);
