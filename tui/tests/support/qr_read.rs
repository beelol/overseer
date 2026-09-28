//! Test-only QR reader for a clean module matrix, and for a code drawn with half blocks. It
//! reads the format information (with its check), removes the mask, reads the codewords,
//! checks every block's Reed-Solomon syndromes and parses the segments. It corrects nothing:
//! one wrong module fails. It shares no code with `overseer_tui::qr`.
#![allow(dead_code)]

pub struct Read {
    pub text: String,
    pub version: usize,
    pub level: String,
    pub mask: u8,
}

const ECC: [[usize; 41]; 4] = [
    [0, 10, 16, 26, 18, 24, 16, 18, 22, 22, 26, 30, 22, 22, 24, 24, 28, 28, 26, 26, 26, 26, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28],
    [0, 7, 10, 15, 20, 26, 18, 20, 24, 30, 18, 20, 24, 26, 30, 22, 24, 28, 30, 28, 28, 28, 28, 30, 30, 26, 28, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30],
    [0, 17, 28, 22, 16, 22, 28, 26, 26, 24, 28, 24, 28, 22, 24, 24, 30, 28, 28, 26, 28, 30, 24, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30],
    [0, 13, 22, 18, 26, 18, 24, 18, 22, 20, 24, 28, 26, 24, 20, 30, 24, 28, 28, 26, 30, 28, 30, 30, 30, 30, 28, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30],
];
const BLOCKS: [[usize; 41]; 4] = [
    [0, 1, 1, 1, 2, 2, 4, 4, 4, 5, 5, 5, 8, 9, 9, 10, 10, 11, 13, 14, 16, 17, 17, 18, 20, 21, 23, 25, 26, 28, 29, 31, 33, 35, 37, 38, 40, 43, 45, 47, 49],
    [0, 1, 1, 1, 1, 1, 2, 2, 2, 2, 4, 4, 4, 4, 4, 6, 6, 6, 6, 7, 8, 8, 9, 9, 10, 12, 12, 12, 13, 14, 15, 16, 17, 18, 19, 19, 20, 21, 22, 24, 25],
    [0, 1, 1, 2, 4, 4, 4, 5, 6, 8, 8, 11, 11, 16, 16, 18, 16, 19, 21, 25, 25, 25, 34, 30, 32, 35, 37, 40, 42, 45, 48, 51, 54, 57, 60, 63, 66, 70, 74, 77, 81],
    [0, 1, 1, 2, 2, 4, 4, 6, 6, 8, 8, 8, 10, 12, 16, 12, 17, 16, 18, 21, 20, 23, 23, 25, 27, 29, 34, 34, 35, 38, 40, 43, 45, 48, 51, 53, 56, 59, 62, 65, 68],
];
// By the two format bits: 00 M, 01 L, 10 H, 11 Q.
const LEVELS: [&str; 4] = ["M", "L", "H", "Q"];

fn gf_tables() -> ([u8; 512], [u8; 256]) {
    let (mut exp, mut log) = ([0u8; 512], [0u8; 256]);
    let mut x = 1u32;
    for (i, e) in exp.iter_mut().enumerate().take(255) {
        *e = x as u8;
        log[x as usize] = i as u8;
        x <<= 1;
        if x & 0x100 != 0 {
            x ^= 0x11d;
        }
    }
    let (low, high) = exp.split_at_mut(255);
    high.copy_from_slice(&[&low[..], &low[..2]].concat());
    (exp, log)
}

fn check_word(value: u32, poly: u32, check: u32, total: u32) -> u32 {
    let mut v = value << check;
    for i in (check..total).rev() {
        if v & (1 << i) != 0 {
            v ^= poly << (i - check);
        }
    }
    (value << check) | v
}

fn masked(mask: u8, r: usize, c: usize) -> bool {
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

/// The modules of a code drawn by `Qr::half_blocks`, without the quiet zone.
pub fn from_half_blocks(lines: &[String], quiet: usize) -> Result<Vec<Vec<bool>>, String> {
    let side = lines.first().map(|l| l.chars().count()).ok_or("nothing drawn")?;
    if side <= quiet * 2 || lines.len() != side.div_ceil(2) {
        return Err(format!("{} lines of {side} columns is not a drawn code", lines.len()));
    }
    let mut grid = vec![vec![false; side]; lines.len() * 2];
    for (y, line) in lines.iter().enumerate() {
        if line.chars().count() != side {
            return Err(format!("line {y} is {} columns, not {side}", line.chars().count()));
        }
        for (x, ch) in line.chars().enumerate() {
            let (top, bottom) = match ch {
                '█' => (true, true),
                '▀' => (true, false),
                '▄' => (false, true),
                ' ' => (false, false),
                other => return Err(format!("{other:?} is not part of a drawn code")),
            };
            grid[y * 2][x] = top;
            grid[y * 2 + 1][x] = bottom;
        }
    }
    let size = side - quiet * 2;
    // The quiet zone is light.
    for (r, row) in grid.iter().enumerate().take(side) {
        for (c, dark) in row.iter().enumerate() {
            let inside = r >= quiet && r < quiet + size && c >= quiet && c < quiet + size;
            if !inside && *dark {
                return Err(format!("the quiet zone is dark at {r},{c}"));
            }
        }
    }
    Ok((0..size).map(|r| (0..size).map(|c| grid[r + quiet][c + quiet]).collect()).collect())
}

pub fn decode(rows: &[Vec<bool>]) -> Result<Read, String> {
    let size = rows.len();
    if size < 21 || (size - 17) % 4 != 0 || rows.iter().any(|r| r.len() != size) {
        return Err(format!("not a QR code: {size} modules"));
    }
    let version = (size - 17) / 4;
    let at = |r: usize, c: usize| rows[r][c];
    for (r0, c0) in [(0, 0), (0, size - 7), (size - 7, 0)] {
        for r in 0..7usize {
            for c in 0..7usize {
                let ring = (r as isize - 3).abs().max((c as isize - 3).abs());
                if at(r0 + r, c0 + c) != (ring != 2) {
                    return Err("a finder pattern is damaged".into());
                }
            }
        }
    }
    let mut first: Vec<bool> = (0..6).map(|c| at(8, c)).collect();
    first.extend([at(8, 7), at(8, 8), at(7, 8)]);
    first.extend((0..6).rev().map(|r| at(r, 8)));
    let mut second: Vec<bool> = (0..7).map(|i| at(size - 1 - i, 8)).collect();
    second.extend((size - 8..size).map(|c| at(8, c)));
    let word = |bits: &[bool]| bits.iter().fold(0u32, |v, b| v << 1 | *b as u32) ^ 0x5412;
    let (f1, f2) = (word(&first), word(&second));
    if check_word(f1 >> 10, 0x537, 10, 15) != f1 || f1 != f2 {
        return Err("the format information fails its check".into());
    }
    if !at(size - 8, 8) {
        return Err("the dark module is missing".into());
    }
    let level = (f1 >> 13) as usize;
    let mask = (f1 >> 10 & 7) as u8;

    let mut function = vec![vec![false; size]; size];
    let mut mark = |r: usize, c: usize, h: usize, w: usize| {
        for row in function.iter_mut().skip(r).take(h) {
            for cell in row.iter_mut().skip(c).take(w) {
                *cell = true;
            }
        }
    };
    mark(0, 0, 9, 9);
    mark(0, size - 8, 9, 8);
    mark(size - 8, 0, 8, 9);
    mark(6, 0, 1, size);
    mark(0, 6, size, 1);
    let mut centers = Vec::new();
    if version > 1 {
        let n = version / 7 + 2;
        let step = if version == 32 { 26 } else { (size - 13).div_ceil(n * 2 - 2) * 2 };
        centers.push(6);
        let mut pos = size - 7;
        while centers.len() < n {
            centers.insert(1, pos);
            pos -= step;
        }
    }
    for &r in &centers {
        for &c in &centers {
            if (r == 6 && (c == 6 || c == size - 7)) || (r == size - 7 && c == 6) {
                continue;
            }
            mark(r - 2, c - 2, 5, 5);
            for y in 0..5usize {
                for x in 0..5usize {
                    let ring = (y as isize - 2).abs().max((x as isize - 2).abs());
                    if at(r + y - 2, c + x - 2) != (ring != 1) {
                        return Err("an alignment pattern is damaged".into());
                    }
                }
            }
        }
    }
    for i in 8..size - 8 {
        if at(6, i) != (i % 2 == 0) || at(i, 6) != (i % 2 == 0) {
            return Err("a timing pattern is damaged".into());
        }
    }
    if version >= 7 {
        mark(0, size - 11, 6, 3);
        mark(size - 11, 0, 3, 6);
        let (mut v1, mut v2) = (0u32, 0u32);
        for i in (0..18).rev() {
            v1 = v1 << 1 | at(i / 3, size - 11 + i % 3) as u32;
            v2 = v2 << 1 | at(size - 11 + i % 3, i / 3) as u32;
        }
        if v1 != v2 || check_word(v1 >> 12, 0x1f25, 12, 18) != v1 || (v1 >> 12) as usize != version {
            return Err("the version information is wrong".into());
        }
    }

    let mut bits = (16 * version + 128) * version + 64;
    if version >= 2 {
        let n = version / 7 + 2;
        bits -= (25 * n - 10) * n - 55;
        if version >= 7 {
            bits -= 36;
        }
    }
    let total = bits / 8;
    let mut codewords = vec![0u8; total];
    let mut bit = 0;
    let mut right = size as isize - 1;
    while right >= 1 {
        if right == 6 {
            right = 5;
        }
        for vert in 0..size {
            for j in 0..2 {
                let c = (right - j) as usize;
                let r = if (right + 1) & 2 == 0 { size - 1 - vert } else { vert };
                if function[r][c] || bit >= total * 8 {
                    continue;
                }
                if at(r, c) ^ masked(mask, r, c) {
                    codewords[bit >> 3] |= 1 << (7 - (bit & 7));
                }
                bit += 1;
            }
        }
        right -= 2;
    }
    if bit != total * 8 {
        return Err(format!("read {bit} data bits, expected {}", total * 8));
    }

    let (blocks, ecc) = (BLOCKS[level][version], ECC[level][version]);
    let short = total / blocks - ecc;
    let long_from = blocks - total % blocks;
    let mut data: Vec<Vec<u8>> = (0..blocks).map(|b| vec![0u8; short + usize::from(b >= long_from)]).collect();
    let mut check: Vec<Vec<u8>> = vec![vec![0u8; ecc]; blocks];
    let mut k = 0;
    for i in 0..=short {
        for block in data.iter_mut() {
            if i < block.len() {
                block[i] = codewords[k];
                k += 1;
            }
        }
    }
    for i in 0..ecc {
        for block in check.iter_mut() {
            block[i] = codewords[k];
            k += 1;
        }
    }
    let (exp, log) = gf_tables();
    let mul = |a: u8, b: u8| if a == 0 || b == 0 { 0 } else { exp[log[a as usize] as usize + log[b as usize] as usize] };
    for b in 0..blocks {
        for &root in exp.iter().take(ecc) {
            let mut v = 0u8;
            for &cw in data[b].iter().chain(&check[b]) {
                v = mul(v, root) ^ cw;
            }
            if v != 0 {
                return Err(format!("block {} fails its error-correction check", b + 1));
            }
        }
    }
    let stream: Vec<u8> = data.concat();
    let mut pos = 0;
    let mut take = |n: usize| -> u32 {
        let mut v = 0;
        for _ in 0..n {
            v = v << 1 | (stream[pos >> 3] >> (7 - (pos & 7)) & 1) as u32;
            pos += 1;
        }
        v
    };
    let mode = take(4);
    if mode != 4 {
        return Err(format!("segment mode {mode} is not byte mode"));
    }
    let n = take(if version <= 9 { 8 } else { 16 }) as usize;
    let bytes: Vec<u8> = (0..n).map(|_| take(8) as u8).collect();
    let text = String::from_utf8(bytes).map_err(|e| e.to_string())?;
    Ok(Read { text, version, level: LEVELS[level].to_string(), mask })
}
