// AC-56: every text foreground/background pair in the Overseer Dark and Light themes meets WCAG
// AA contrast (4.5:1), and the generated theme files are current with design/tokens.js.
const fs = require('fs');
const path = require('path');
const { palettes } = require('../../extension/design/tokens');
const { colors, textPairs, contrast, theme } = require('../../extension/design/build-themes');

const root = path.join(__dirname, '../../extension');
let failures = 0, checked = 0;
for (const [key, name] of [['dark', 'Overseer Dark'], ['light', 'Overseer Light']]) {
  const p = palettes[key];
  const c = colors(p);
  for (const pair of textPairs(p, c)) {
    checked++;
    const ratio = contrast(pair.fg, pair.bg);
    if (ratio < 4.5) { failures++; console.log(`FAIL ${name}: ${pair.what} ${pair.fg} on ${pair.bg} = ${ratio.toFixed(2)}`); }
  }
  const file = path.join(root, `themes/overseer-${key}-color-theme.json`);
  const current = fs.existsSync(file) && fs.readFileSync(file, 'utf8') === JSON.stringify(theme(name, p), null, 2) + '\n';
  if (!current) { failures++; console.log(`FAIL ${file} is stale: run node extension/design/build-themes.js`); }
}
console.log(`${checked} text pairs checked, ${failures} failures`);
process.exit(failures ? 1 : 0);
