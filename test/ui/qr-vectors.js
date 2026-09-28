// Writes tui/tests/support/qr-vectors.json: QR codes as the encoder VS Code ships draws them
// (extension/media/vendor/toqr.js), each read back by test/ui/qr-decode.js. The terminal UI's own
// encoder (tui/src/qr.rs) must draw the same modules for the same text, level and mask.
// Run: node test/ui/qr-vectors.js
const fs = require('fs');
const path = require('path');
const { qrMatrix } = require('../../extension/src/phone-text');
const { decode } = require('./qr-decode');

const A = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ234567';
const code = n => { let s = 'OVSR1-'; for (let i = 0; s.length < n; i++) s += A[(i * 13 + n * 7) % 32]; return s; };
const texts = ['OVSR1-TEST', code(60), code(134), code(135), code(154), code(190), code(260), 'Zoë’s phone 📱', code(700)];
const vectors = [];
for (const level of ['L', 'M', 'Q', 'H']) {
  for (const text of texts) {
    const m = qrMatrix(text, level);
    const read = decode(m.rows);
    if (read.text !== text || read.level !== level) throw new Error(`the reader disagrees for ${level} ${text.length}`);
    vectors.push({ text, level, version: read.version, mask: read.mask, rows: m.rows.map(r => r.join('')) });
  }
}
const out = { source: 'toqr 0.1.1 (extension/media/vendor/toqr.js), read back by test/ui/qr-decode.js; regenerate with node test/ui/qr-vectors.js', vectors };
fs.writeFileSync(path.join(__dirname, '../../tui/tests/support/qr-vectors.json'), JSON.stringify(out) + '\n');
console.log(`${vectors.length} vectors, versions ${[...new Set(vectors.map(v => v.version))].sort((a, b) => a - b).join(', ')}`);
