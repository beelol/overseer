// Test-only QR code reader for a clean module matrix (rows of 0 and 1, 1 is dark, no quiet zone).
// It is written from the QR standard (ISO/IEC 18004) and shares no code or tables with the
// encoder the extension ships, so reading back what the encoder drew is a real check. It reads the
// format information (with its BCH check), removes the mask, reads the codewords, splits them
// into their blocks, checks every block's Reed-Solomon syndromes (a clean code has none), and
// parses the segments. It does not correct errors: a wrong module fails the check.
const ECC_PER_BLOCK = {
  L: [-1, 7, 10, 15, 20, 26, 18, 20, 24, 30, 18, 20, 24, 26, 30, 22, 24, 28, 30, 28, 28, 28, 28, 30, 30, 26, 28, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30],
  M: [-1, 10, 16, 26, 18, 24, 16, 18, 22, 22, 26, 30, 22, 22, 24, 24, 28, 28, 26, 26, 26, 26, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28],
  Q: [-1, 13, 22, 18, 26, 18, 24, 18, 22, 20, 24, 28, 26, 24, 20, 30, 24, 28, 28, 26, 30, 28, 30, 30, 30, 30, 28, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30],
  H: [-1, 17, 28, 22, 16, 22, 28, 26, 26, 24, 28, 24, 28, 22, 24, 24, 30, 28, 28, 26, 28, 30, 24, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30],
};
const BLOCKS = {
  L: [-1, 1, 1, 1, 1, 1, 2, 2, 2, 2, 4, 4, 4, 4, 4, 6, 6, 6, 6, 7, 8, 8, 9, 9, 10, 12, 12, 12, 13, 14, 15, 16, 17, 18, 19, 19, 20, 21, 22, 24, 25],
  M: [-1, 1, 1, 1, 2, 2, 4, 4, 4, 5, 5, 5, 8, 9, 9, 10, 10, 11, 13, 14, 16, 17, 17, 18, 20, 21, 23, 25, 26, 28, 29, 31, 33, 35, 37, 38, 40, 43, 45, 47, 49],
  Q: [-1, 1, 1, 2, 2, 4, 4, 6, 6, 8, 8, 8, 10, 12, 16, 12, 17, 16, 18, 21, 20, 23, 23, 25, 27, 29, 34, 34, 35, 38, 40, 43, 45, 48, 51, 53, 56, 59, 62, 65, 68],
  H: [-1, 1, 1, 2, 4, 4, 4, 5, 6, 8, 8, 11, 11, 16, 16, 18, 16, 19, 21, 25, 25, 25, 34, 30, 32, 35, 37, 40, 42, 45, 48, 51, 54, 57, 60, 63, 66, 70, 74, 77, 81],
};
const LEVELS = ['M', 'L', 'H', 'Q']; // by the two format bits: 00 M, 01 L, 10 H, 11 Q
const ALNUM = '0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ $%*+-./:';
const MASKS = [
  (r, c) => (r + c) % 2 === 0, (r, c) => r % 2 === 0, (r, c) => c % 3 === 0, (r, c) => (r + c) % 3 === 0,
  (r, c) => (Math.floor(r / 2) + Math.floor(c / 3)) % 2 === 0, (r, c) => (r * c) % 2 + (r * c) % 3 === 0,
  (r, c) => ((r * c) % 2 + (r * c) % 3) % 2 === 0, (r, c) => ((r + c) % 2 + (r * c) % 3) % 2 === 0,
];

// GF(256) with the QR polynomial x^8 + x^4 + x^3 + x^2 + 1.
const EXP = new Uint8Array(510), LOG = new Uint8Array(256);
for (let i = 0, x = 1; i < 255; i++) { EXP[i] = x; LOG[x] = i; x <<= 1; if (x & 0x100) x ^= 0x11d; }
for (let i = 255; i < 510; i++) EXP[i] = EXP[i - 255];
const mul = (a, b) => (a && b ? EXP[LOG[a] + LOG[b]] : 0);

/** `value` followed by its BCH check bits: `check` bits from the generator `poly`, `total` bits in all. */
function bch(value, poly, check, total) {
  let v = value << check;
  for (let i = total - 1; i >= check; i--) if (v & (1 << i)) v ^= poly << (i - check);
  return (value << check) | v;
}

function alignmentCenters(version) {
  if (version === 1) return [];
  const size = version * 4 + 17, n = Math.floor(version / 7) + 2;
  const step = version === 32 ? 26 : Math.ceil((size - 13) / (n * 2 - 2)) * 2;
  const out = [6];
  for (let pos = size - 7; out.length < n; pos -= step) out.splice(1, 0, pos);
  return out;
}

function totalCodewords(version) {
  let bits = (16 * version + 128) * version + 64;
  if (version >= 2) {
    const n = Math.floor(version / 7) + 2;
    bits -= (25 * n - 10) * n - 55;
    if (version >= 7) bits -= 36;
  }
  return Math.floor(bits / 8);
}

/** Reads a QR code. `rows[y][x]` is 1 for a dark module. Returns { text, bytes, version, level, mask }. */
function decode(rows) {
  const size = rows.length;
  if (size < 21 || (size - 17) % 4 || rows.some(r => r.length !== size)) throw new Error(`not a QR code: ${size} modules`);
  const version = (size - 17) / 4;
  const at = (r, c) => (rows[r][c] ? 1 : 0);

  // Finder patterns: three corners, 7 by 7, dark ring, light ring, dark 3 by 3.
  for (const [r0, c0] of [[0, 0], [0, size - 7], [size - 7, 0]]) {
    for (let r = 0; r < 7; r++) for (let c = 0; c < 7; c++) {
      const ring = Math.max(Math.abs(r - 3), Math.abs(c - 3));
      if (at(r0 + r, c0 + c) !== (ring === 2 ? 0 : 1)) throw new Error('a finder pattern is damaged');
    }
  }
  // Format information, both copies; each must pass its BCH check and they must agree.
  const copy1 = [], copy2 = [];
  for (let c = 0; c <= 5; c++) copy1.push(at(8, c));
  copy1.push(at(8, 7), at(8, 8), at(7, 8));
  for (let r = 5; r >= 0; r--) copy1.push(at(r, 8));
  for (let r = size - 1; r >= size - 7; r--) copy2.push(at(r, 8));
  for (let c = size - 8; c < size; c++) copy2.push(at(8, c));
  const word = bits => bits.reduce((v, b) => (v << 1) | b, 0) ^ 0x5412;
  const [f1, f2] = [word(copy1), word(copy2)];
  for (const f of [f1, f2]) if (bch(f >> 10, 0x537, 10, 15) !== f) throw new Error('the format information fails its check');
  if (f1 !== f2) throw new Error('the two copies of the format information differ');
  if (!at(size - 8, 8)) throw new Error('the dark module is missing');
  const level = LEVELS[f1 >> 13], mask = (f1 >> 10) & 7;

  // Function modules: everything that is not data.
  const fn = rows.map(() => new Uint8Array(size));
  const mark = (r, c, h, w) => { for (let y = r; y < r + h; y++) for (let x = c; x < c + w; x++) if (y >= 0 && x >= 0 && y < size && x < size) fn[y][x] = 1; };
  mark(0, 0, 9, 9); mark(0, size - 8, 9, 8); mark(size - 8, 0, 8, 9);
  mark(6, 0, 1, size); mark(0, 6, size, 1);
  const centers = alignmentCenters(version);
  for (const r of centers) for (const c of centers) {
    const corner = (r === 6 && c === 6) || (r === 6 && c === size - 7) || (r === size - 7 && c === 6);
    if (corner) continue;
    mark(r - 2, c - 2, 5, 5);
    for (let y = -2; y <= 2; y++) for (let x = -2; x <= 2; x++) if (at(r + y, c + x) !== (Math.max(Math.abs(y), Math.abs(x)) === 1 ? 0 : 1)) throw new Error('an alignment pattern is damaged');
  }
  for (let i = 8; i < size - 8; i++) if (at(6, i) !== (i + 1) % 2 || at(i, 6) !== (i + 1) % 2) throw new Error('a timing pattern is damaged');
  if (version >= 7) {
    mark(0, size - 11, 6, 3); mark(size - 11, 0, 3, 6);
    let v1 = 0, v2 = 0;
    for (let i = 17; i >= 0; i--) { v1 = (v1 << 1) | at(Math.floor(i / 3), size - 11 + (i % 3)); v2 = (v2 << 1) | at(size - 11 + (i % 3), Math.floor(i / 3)); }
    if (v1 !== v2 || bch(v1 >> 12, 0x1f25, 12, 18) !== v1 || (v1 >> 12) !== version) throw new Error('the version information is wrong');
  }

  // Codewords, in the standard's zigzag from the bottom right, mask removed.
  const total = totalCodewords(version);
  const codewords = new Uint8Array(total);
  let bit = 0;
  for (let right = size - 1; right >= 1; right -= 2) {
    if (right === 6) right = 5;
    for (let vert = 0; vert < size; vert++) {
      for (let j = 0; j < 2; j++) {
        const c = right - j;
        const upward = ((right + 1) & 2) === 0;
        const r = upward ? size - 1 - vert : vert;
        if (fn[r][c] || bit >= total * 8) continue;
        const v = at(r, c) ^ (MASKS[mask](r, c) ? 1 : 0);
        codewords[bit >> 3] |= v << (7 - (bit & 7));
        bit++;
      }
    }
  }
  if (bit !== total * 8) throw new Error(`read ${bit} data bits, expected ${total * 8}`);

  // Blocks: data codewords are interleaved, then error-correction codewords.
  const blocks = BLOCKS[level][version], ecc = ECC_PER_BLOCK[level][version];
  const short = Math.floor(total / blocks), longCount = total % blocks;
  const shortData = short - ecc;
  const data = Array.from({ length: blocks }, (_, i) => new Uint8Array(shortData + (i >= blocks - longCount ? 1 : 0)));
  const check = Array.from({ length: blocks }, () => new Uint8Array(ecc));
  let k = 0;
  for (let i = 0; i <= shortData; i++) for (let b = 0; b < blocks; b++) if (i < data[b].length) data[b][i] = codewords[k++];
  for (let i = 0; i < ecc; i++) for (let b = 0; b < blocks; b++) check[b][i] = codewords[k++];
  for (let b = 0; b < blocks; b++) {
    const block = [...data[b], ...check[b]];
    for (let s = 0; s < ecc; s++) {
      let v = 0;
      for (const cw of block) v = mul(v, EXP[s]) ^ cw;
      if (v !== 0) throw new Error(`block ${b + 1} fails its error-correction check`);
    }
  }
  const stream = new Uint8Array(data.reduce((n, d) => n + d.length, 0));
  let o = 0;
  for (const d of data) { stream.set(d, o); o += d.length; }

  // Segments.
  let pos = 0;
  const take = n => { let v = 0; for (let i = 0; i < n; i++) { v = (v << 1) | ((stream[pos >> 3] >> (7 - (pos & 7))) & 1); pos++; } return v; };
  const left = () => stream.length * 8 - pos;
  const bytes = [];
  while (left() >= 4) {
    const mode = take(4);
    if (mode === 0) break;
    if (mode === 4) {
      const n = take(version <= 9 ? 8 : 16);
      for (let i = 0; i < n; i++) bytes.push(take(8));
    } else if (mode === 2) {
      let n = take(version <= 9 ? 9 : version <= 26 ? 11 : 13);
      for (; n >= 2; n -= 2) { const v = take(11); bytes.push(ALNUM.charCodeAt(Math.floor(v / 45)), ALNUM.charCodeAt(v % 45)); }
      if (n) bytes.push(ALNUM.charCodeAt(take(6)));
    } else if (mode === 1) {
      let n = take(version <= 9 ? 10 : version <= 26 ? 12 : 14);
      for (; n >= 3; n -= 3) bytes.push(...String(take(10)).padStart(3, '0').split('').map(ch => ch.charCodeAt(0)));
      if (n === 2) bytes.push(...String(take(7)).padStart(2, '0').split('').map(ch => ch.charCodeAt(0)));
      else if (n === 1) bytes.push(48 + take(4));
    } else throw new Error(`segment mode ${mode} is not supported`);
  }
  return { text: new TextDecoder('utf-8', { fatal: true }).decode(new Uint8Array(bytes)), bytes: new Uint8Array(bytes), version, level, mask, size };
}

module.exports = { decode, alignmentCenters, totalCodewords };
