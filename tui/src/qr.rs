//! A QR code encoder for the pairing code, and its drawing with half-block characters.
//!
//! Written from the QR standard (ISO/IEC 18004): byte mode, versions 1 to 40, the four
//! error-correction levels, the eight masks with the standard's penalty rules. It is here, and
//! not a crate, so the terminal UI gains no dependency; `tests/phone.rs` checks it module for
//! module against the encoder VS Code ships and reads the drawn code back from the screen.

/// Error-correction level. A screen has no damage to recover from, so L or M is enough.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    L,
    M,
    Q,
    H,
}

impl Level {
    fn index(self) -> usize {
        match self {
            Level::L => 0,
            Level::M => 1,
            Level::Q => 2,
            Level::H => 3,
        }
    }
    /// The two bits of the format information.
    fn bits(self) -> u32 {
        match self {
            Level::L => 1,
            Level::M => 0,
            Level::Q => 3,
            Level::H => 2,
        }
    }
}

#[rustfmt::skip]
const ECC_PER_BLOCK: [[u8; 41]; 4] = [
    [0, 7, 10, 15, 20, 26, 18, 20, 24, 30, 18, 20, 24, 26, 30, 22, 24, 28, 30, 28, 28, 28, 28, 30, 30, 26, 28, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30],
    [0, 10, 16, 26, 18, 24, 16, 18, 22, 22, 26, 30, 22, 22, 24, 24, 28, 28, 26, 26, 26, 26, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28],
    [0, 13, 22, 18, 26, 18, 24, 18, 22, 20, 24, 28, 26, 24, 20, 30, 24, 28, 28, 26, 30, 28, 30, 30, 30, 30, 28, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30],
    [0, 17, 28, 22, 16, 22, 28, 26, 26, 24, 28, 24, 28, 22, 24, 24, 30, 28, 28, 26, 28, 30, 24, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30],
];

#[rustfmt::skip]
const BLOCKS: [[u8; 41]; 4] = [
    [0, 1, 1, 1, 1, 1, 2, 2, 2, 2, 4, 4, 4, 4, 4, 6, 6, 6, 6, 7, 8, 8, 9, 9, 10, 12, 12, 12, 13, 14, 15, 16, 17, 18, 19, 19, 20, 21, 22, 24, 25],
    [0, 1, 1, 1, 2, 2, 4, 4, 4, 5, 5, 5, 8, 9, 9, 10, 10, 11, 13, 14, 16, 17, 17, 18, 20, 21, 23, 25, 26, 28, 29, 31, 33, 35, 37, 38, 40, 43, 45, 47, 49],
    [0, 1, 1, 2, 2, 4, 4, 6, 6, 8, 8, 8, 10, 12, 16, 12, 17, 16, 18, 21, 20, 23, 23, 25, 27, 29, 34, 34, 35, 38, 40, 43, 45, 48, 51, 53, 56, 59, 62, 65, 68],
    [0, 1, 1, 2, 4, 4, 4, 5, 6, 8, 8, 11, 11, 16, 16, 18, 16, 19, 21, 25, 25, 25, 34, 30, 32, 35, 37, 40, 42, 45, 48, 51, 54, 57, 60, 63, 66, 70, 74, 77, 81],
];

/// A QR code: `size` by `size` modules, `true` is dark.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Qr {
    pub size: usize,
    pub version: usize,
    pub level: Level,
    pub mask: u8,
    modules: Vec<bool>,
}

impl Qr {
    pub fn dark(&self, row: usize, col: usize) -> bool {
        row < self.size && col < self.size && self.modules[row * self.size + col]
    }

    /// Rows of `0` and `1`, for tests and comparisons.
    pub fn rows(&self) -> Vec<String> {
        (0..self.size).map(|r| (0..self.size).map(|c| if self.dark(r, c) { '1' } else { '0' }).collect()).collect()
    }

    /// The code drawn with half blocks: one character is one module wide and two modules
    /// high, inside a quiet zone of `quiet` modules. Draw it dark on light: `█` is two dark
    /// modules, `▀` a dark one over a light one, `▄` the reverse, a space two light ones.
    pub fn half_blocks(&self, quiet: usize) -> Vec<String> {
        let side = self.size + quiet * 2;
        let at = |r: usize, c: usize| r >= quiet && c >= quiet && self.dark(r - quiet, c - quiet);
        (0..side.div_ceil(2))
            .map(|line| {
                (0..side)
                    .map(|c| match (at(line * 2, c), line * 2 + 1 < side && at(line * 2 + 1, c)) {
                        (true, true) => '█',
                        (true, false) => '▀',
                        (false, true) => '▄',
                        (false, false) => ' ',
                    })
                    .collect()
            })
            .collect()
    }
}

/// Columns and lines `half_blocks` needs for a code of `size` modules.
pub fn drawn_size(size: usize, quiet: usize) -> (usize, usize) {
    let side = size + quiet * 2;
    (side, side.div_ceil(2))
}

/// Modules per side of the code that holds `len` bytes at `level`, or `None` when nothing does.
pub fn size_for(len: usize, level: Level) -> Option<usize> {
    version_for(len, level).map(|v| v * 4 + 17)
}

fn total_codewords(version: usize) -> usize {
    let mut bits = (16 * version + 128) * version + 64;
    if version >= 2 {
        let n = version / 7 + 2;
        bits -= (25 * n - 10) * n - 55;
        if version >= 7 {
            bits -= 36;
        }
    }
    bits / 8
}

fn data_codewords(version: usize, level: Level) -> usize {
    total_codewords(version) - ECC_PER_BLOCK[level.index()][version] as usize * BLOCKS[level.index()][version] as usize
}

fn count_bits(version: usize) -> usize {
    if version <= 9 {
        8
    } else {
        16
    }
}

fn version_for(len: usize, level: Level) -> Option<usize> {
    (1..=40).find(|&v| 4 + count_bits(v) + len * 8 <= data_codewords(v, level) * 8)
}

fn alignment_centers(version: usize) -> Vec<usize> {
    if version == 1 {
        return Vec::new();
    }
    let size = version * 4 + 17;
    let n = version / 7 + 2;
    let step = if version == 32 { 26 } else { (size - 13).div_ceil(n * 2 - 2) * 2 };
    let mut out = vec![6];
    let mut pos = size - 7;
    while out.len() < n {
        out.insert(1, pos);
        pos -= step;
    }
    out
}

// GF(256) with the QR polynomial x^8 + x^4 + x^3 + x^2 + 1.
fn gf_mul(a: u8, b: u8) -> u8 {
    let (mut x, mut y, mut z) = (a as u32, b as u32, 0u32);
    while y != 0 {
        if y & 1 != 0 {
            z ^= x;
        }
        x <<= 1;
        if x & 0x100 != 0 {
            x ^= 0x11d;
        }
        y >>= 1;
    }
    z as u8
}

/// The error-correction codewords of one block.
fn reed_solomon(data: &[u8], n: usize) -> Vec<u8> {
    // The generator: (x - 2^0)(x - 2^1)…(x - 2^(n-1)), highest power first, without its leading 1.
    let mut gen = vec![0u8; n];
    gen[n - 1] = 1;
    let mut root = 1u8;
    for _ in 0..n {
        for j in 0..n {
            gen[j] = gf_mul(gen[j], root);
            if j + 1 < n {
                gen[j] ^= gen[j + 1];
            }
        }
        root = gf_mul(root, 2);
    }
    let mut rest = vec![0u8; n];
    for &b in data {
        let factor = b ^ rest.remove(0);
        rest.push(0);
        for (r, g) in rest.iter_mut().zip(&gen) {
            *r ^= gf_mul(*g, factor);
        }
    }
    rest
}

fn mask_at(mask: u8, r: usize, c: usize) -> bool {
    match mask {
        0 => (r + c) % 2 == 0,
        1 => r % 2 == 0,
        2 => c % 3 == 0,
        3 => (r + c) % 3 == 0,
        4 => (r / 2 + c / 3) % 2 == 0,
        5 => (r * c) % 2 + (r * c) % 3 == 0,
        6 => ((r * c) % 2 + (r * c) % 3) % 2 == 0,
        _ => ((r + c) % 2 + (r * c) % 3) % 2 == 0,
    }
}

fn bch(value: u32, poly: u32, check: u32, total: u32) -> u32 {
    let mut v = value << check;
    for i in (check..total).rev() {
        if v & (1 << i) != 0 {
            v ^= poly << (i - check);
        }
    }
    (value << check) | v
}

struct Grid {
    size: usize,
    dark: Vec<bool>,
    /// Finder, timing and alignment patterns, format and version information: not data.
    function: Vec<bool>,
}

impl Grid {
    fn set(&mut self, r: usize, c: usize, dark: bool) {
        self.dark[r * self.size + c] = dark;
        self.function[r * self.size + c] = true;
    }

    fn finder(&mut self, r0: isize, c0: isize) {
        for dr in -1..=7isize {
            for dc in -1..=7isize {
                let (r, c) = (r0 + dr, c0 + dc);
                if r < 0 || c < 0 || r >= self.size as isize || c >= self.size as isize {
                    continue;
                }
                let inside = (0..7).contains(&dr) && (0..7).contains(&dc);
                let ring = (dr - 3).abs().max((dc - 3).abs());
                self.set(r as usize, c as usize, inside && ring != 2);
            }
        }
    }

    fn format(&mut self, level: Level, mask: u8) {
        let word = bch(level.bits() << 3 | mask as u32, 0x537, 10, 15) ^ 0x5412;
        let bit = |i: u32| word >> i & 1 != 0;
        let n = self.size;
        // Around the top-left finder, most significant bit first along row 8.
        for i in 0..6 {
            self.set(8, i, bit(14 - i as u32));
        }
        self.set(8, 7, bit(8));
        self.set(8, 8, bit(7));
        self.set(7, 8, bit(6));
        for i in 0..6 {
            self.set(5 - i, 8, bit(5 - i as u32));
        }
        // The second copy: down the bottom-left finder's side, then along the top-right's.
        for i in 0..7 {
            self.set(n - 1 - i, 8, bit(14 - i as u32));
        }
        for i in 0..8 {
            self.set(8, n - 8 + i, bit(7 - i as u32));
        }
        self.set(n - 8, 8, true);
    }

    fn version(&mut self, version: usize) {
        if version < 7 {
            return;
        }
        let word = bch(version as u32, 0x1f25, 12, 18);
        let n = self.size;
        for i in 0..18 {
            let dark = word >> i & 1 != 0;
            self.set(i / 3, n - 11 + i % 3, dark);
            self.set(n - 11 + i % 3, i / 3, dark);
        }
    }

    /// The standard's four penalty rules.
    fn penalty(&self) -> u32 {
        let n = self.size;
        let at = |r: usize, c: usize| self.dark[r * n + c];
        let mut total = 0u32;
        // Runs of five or more of one colour in a row or column, and finder-like patterns.
        for line in 0..n {
            for across in [true, false] {
                let get = |i: usize| if across { at(line, i) } else { at(i, line) };
                let mut run = 1;
                for i in 1..n {
                    if get(i) == get(i - 1) {
                        run += 1;
                    } else {
                        if run >= 5 {
                            total += run - 2;
                        }
                        run = 1;
                    }
                }
                if run >= 5 {
                    total += run - 2;
                }
                // Dark, light, three dark, light, dark with four light modules on either side.
                let mut window = 0u32;
                for i in 0..n {
                    window = (window << 1 | get(i) as u32) & 0x7ff;
                    if i >= 10 && (window == 0b101_1101_0000 || window == 0b000_0101_1101) {
                        total += 40;
                    }
                }
            }
        }
        // Blocks of two by two of one colour.
        for r in 0..n - 1 {
            for c in 0..n - 1 {
                let v = at(r, c);
                if v == at(r, c + 1) && v == at(r + 1, c) && v == at(r + 1, c + 1) {
                    total += 3;
                }
            }
        }
        // The share of dark modules, in steps of five percent from one half.
        let dark = self.dark.iter().filter(|d| **d).count();
        let percent = dark * 100 / (n * n);
        let (low, high) = (percent / 5 * 5, percent / 5 * 5 + 5);
        total += 10 * ((50i32 - low as i32).abs().min((50i32 - high as i32).abs()) as u32 / 5);
        total
    }
}

/// The QR code of `data` in byte mode. `None` when it does not fit the largest code.
pub fn encode(data: &[u8], level: Level) -> Option<Qr> {
    encode_with_mask(data, level, None)
}

/// As `encode`, with the mask given (tests compare with another encoder's choice).
pub fn encode_with_mask(data: &[u8], level: Level, mask: Option<u8>) -> Option<Qr> {
    let version = version_for(data.len(), level)?;
    let size = version * 4 + 17;
    let capacity = data_codewords(version, level);

    // The bit stream: mode, count, the bytes, a terminator, then padding to the capacity.
    let mut bits: Vec<bool> = Vec::with_capacity(capacity * 8);
    let mut push = |value: u32, n: usize| {
        for i in (0..n).rev() {
            bits.push(value >> i & 1 != 0);
        }
    };
    push(0b0100, 4);
    push(data.len() as u32, count_bits(version));
    for &b in data {
        push(b as u32, 8);
    }
    let terminator = (capacity * 8 - bits.len()).min(4);
    bits.extend(std::iter::repeat_n(false, terminator));
    while bits.len() % 8 != 0 {
        bits.push(false);
    }
    let mut codewords: Vec<u8> = bits.chunks(8).map(|b| b.iter().fold(0u8, |v, &bit| v << 1 | bit as u8)).collect();
    for pad in [0xec, 0x11].iter().cycle() {
        if codewords.len() >= capacity {
            break;
        }
        codewords.push(*pad);
    }

    // Blocks with their error correction, interleaved.
    let blocks = BLOCKS[level.index()][version] as usize;
    let ecc = ECC_PER_BLOCK[level.index()][version] as usize;
    let total = total_codewords(version);
    let short = total / blocks - ecc;
    let long_from = blocks - total % blocks;
    let mut parts: Vec<(Vec<u8>, Vec<u8>)> = Vec::with_capacity(blocks);
    let mut at = 0;
    for b in 0..blocks {
        let len = short + usize::from(b >= long_from);
        let part = codewords[at..at + len].to_vec();
        at += len;
        let check = reed_solomon(&part, ecc);
        parts.push((part, check));
    }
    let mut stream: Vec<u8> = Vec::with_capacity(total);
    for i in 0..=short {
        for (part, _) in &parts {
            if i < part.len() {
                stream.push(part[i]);
            }
        }
    }
    for i in 0..ecc {
        for (_, check) in &parts {
            stream.push(check[i]);
        }
    }

    // Function patterns.
    let mut grid = Grid { size, dark: vec![false; size * size], function: vec![false; size * size] };
    for i in 0..size {
        grid.set(6, i, i % 2 == 0);
        grid.set(i, 6, i % 2 == 0);
    }
    grid.finder(0, 0);
    grid.finder(0, size as isize - 7);
    grid.finder(size as isize - 7, 0);
    let centers = alignment_centers(version);
    for &r in &centers {
        for &c in &centers {
            // The three corners hold finder patterns.
            if (r == 6 && (c == 6 || c == size - 7)) || (r == size - 7 && c == 6) {
                continue;
            }
            for dr in 0..5 {
                for dc in 0..5 {
                    let ring = (dr as isize - 2).abs().max((dc as isize - 2).abs());
                    grid.set(r - 2 + dr, c - 2 + dc, ring != 1);
                }
            }
        }
    }
    grid.format(level, 0);
    grid.version(version);

    // The codewords, in the standard's zigzag from the bottom right.
    let mut i = 0;
    let mut right = size as isize - 1;
    while right >= 1 {
        if right == 6 {
            right = 5;
        }
        for vert in 0..size {
            for j in 0..2 {
                let c = (right - j) as usize;
                let upward = (right + 1) & 2 == 0;
                let r = if upward { size - 1 - vert } else { vert };
                if !grid.function[r * size + c] && i < stream.len() * 8 {
                    grid.dark[r * size + c] = stream[i >> 3] >> (7 - (i & 7)) & 1 != 0;
                    i += 1;
                }
            }
        }
        right -= 2;
    }

    let apply = |grid: &mut Grid, mask: u8| {
        for r in 0..size {
            for c in 0..size {
                if !grid.function[r * size + c] && mask_at(mask, r, c) {
                    grid.dark[r * size + c] ^= true;
                }
            }
        }
    };
    let chosen = mask.unwrap_or_else(|| {
        let mut best = (0u8, u32::MAX);
        for m in 0..8 {
            apply(&mut grid, m);
            grid.format(level, m);
            let p = grid.penalty();
            if p < best.1 {
                best = (m, p);
            }
            apply(&mut grid, m);
        }
        best.0
    });
    apply(&mut grid, chosen);
    grid.format(level, chosen);
    Some(Qr { size, version, level, mask: chosen, modules: grid.dark })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_and_capacities_follow_the_standard() {
        assert_eq!(total_codewords(1), 26);
        assert_eq!(total_codewords(7), 196);
        assert_eq!(total_codewords(40), 3706);
        assert_eq!(data_codewords(1, Level::L), 19);
        assert_eq!(data_codewords(8, Level::M), 154);
        assert_eq!(data_codewords(40, Level::H), 1276);
        // Byte capacity: 17 bytes in version 1-L, 134 in 6-L, 152 in 8-M.
        assert_eq!(version_for(17, Level::L), Some(1));
        assert_eq!(version_for(18, Level::L), Some(2));
        assert_eq!(version_for(134, Level::L), Some(6));
        assert_eq!(version_for(135, Level::L), Some(7));
        assert_eq!(version_for(152, Level::M), Some(8));
        assert_eq!(version_for(2953, Level::L), Some(40));
        assert_eq!(version_for(2954, Level::L), None);
        assert_eq!(size_for(134, Level::M), Some(49));
        assert_eq!(alignment_centers(1), Vec::<usize>::new());
        assert_eq!(alignment_centers(7), vec![6, 22, 38]);
        assert_eq!(alignment_centers(32), vec![6, 34, 60, 86, 112, 138]);
    }

    #[test]
    fn reed_solomon_matches_a_known_block() {
        // The standard's worked example "01234567" at 1-M: 16 data codewords, 10 for correction.
        let data = [0x10, 0x20, 0x0c, 0x56, 0x61, 0x80, 0xec, 0x11, 0xec, 0x11, 0xec, 0x11, 0xec, 0x11, 0xec, 0x11];
        assert_eq!(reed_solomon(&data, 10), vec![0xa5, 0x24, 0xd4, 0xc1, 0xed, 0x36, 0xc7, 0x87, 0x2c, 0x55]);
    }

    #[test]
    fn format_information_matches_the_standard() {
        // Level M with mask 5: 100000011001110 (the standard's example).
        assert_eq!(bch(0b00101, 0x537, 10, 15) ^ 0x5412, 0b100_0000_1100_1110);
        assert_eq!(bch(7, 0x1f25, 12, 18), 0x07c94);
    }

    #[test]
    fn half_blocks_draw_two_rows_per_line() {
        let qr = encode(b"OVSR1-TEST", Level::L).unwrap();
        assert_eq!(qr.size, 21);
        let lines = qr.half_blocks(2);
        assert_eq!(drawn_size(21, 2), (25, 13));
        assert_eq!(lines.len(), 13);
        assert!(lines.iter().all(|l| l.chars().count() == 25));
        assert_eq!(lines[0].trim(), "", "the quiet zone is light");
        // The top-left finder: a dark row, then the ring's sides.
        assert!(lines[1].starts_with("  █▀▀▀▀▀█ "), "{:?}", lines[1]);
    }
}
