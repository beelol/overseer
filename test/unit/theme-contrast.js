// AC-56, AC-103: every text foreground/background pair in the Overseer Dark, Light and Overseer
// themes (gradient stops included) meets WCAG AA contrast (4.5:1), the generated theme files are
// current with design/tokens.js, and package.json contributes every overseer.* gradient color.
const fs = require('fs');
const path = require('path');
const { palettes } = require('../../extension/design/tokens');
const { colors, gradientColors, textPairs, contrast, theme } = require('../../extension/design/build-themes');

const root = path.join(__dirname, '../../extension');
let failures = 0, checked = 0;
const files = { dark: 'overseer-dark', light: 'overseer-light', overseer: 'overseer' };
for (const [key, name] of [['dark', 'Overseer Dark'], ['light', 'Overseer Light'], ['overseer', 'Overseer']]) {
  const p = palettes[key];
  const c = colors(p);
  for (const pair of textPairs(p, c)) {
    checked++;
    const ratio = contrast(pair.fg, pair.bg);
    if (ratio < 4.5) { failures++; console.log(`FAIL ${name}: ${pair.what} ${pair.fg} on ${pair.bg} = ${ratio.toFixed(2)}`); }
  }
  const file = path.join(root, `themes/${files[key]}-color-theme.json`);
  const current = fs.existsSync(file) && fs.readFileSync(file, 'utf8') === JSON.stringify(theme(name, p), null, 2) + '\n';
  if (!current) { failures++; console.log(`FAIL ${file} is stale: run node extension/design/build-themes.js`); }
}
const pkg = JSON.parse(fs.readFileSync(path.join(root, 'package.json'), 'utf8'));
const contributed = new Set((pkg.contributes.colors || []).map(c => c.id));
for (const id of Object.keys(gradientColors(palettes.overseer))) {
  if (!contributed.has(id)) { failures++; console.log(`FAIL package.json does not contribute the color ${id}`); }
}
for (const t of pkg.contributes.themes) {
  if (!fs.existsSync(path.join(root, t.path))) { failures++; console.log(`FAIL theme ${t.label} has no file at ${t.path}`); }
}
console.log(`${checked} text pairs checked, ${failures} failures`);
process.exit(failures ? 1 : 0);
