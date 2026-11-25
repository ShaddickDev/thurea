//! A small QR code encoder (byte mode, all versions), so `thurea setup-phone` can show the
//! setup code in any terminal on Linux or Windows without extra tools or crates.
//! Follows ISO/IEC 18004 the same way as Nayuki's reference "QR Code generator" library.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ecc {
    Low,
    Medium,
    Quartile,
    High,
}

impl Ecc {
    fn ord(self) -> usize {
        self as usize
    }
    fn format_bits(self) -> u32 {
        match self {
            Ecc::Low => 1,
            Ecc::Medium => 0,
            Ecc::Quartile => 3,
            Ecc::High => 2,
        }
    }
}

const ECC_PER_BLOCK: [[i8; 41]; 4] = [
    [-1, 7, 10, 15, 20, 26, 18, 20, 24, 30, 18, 20, 24, 26, 30, 22, 24, 28, 30, 28, 28, 28, 28, 30, 30, 26, 28, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30],
    [-1, 10, 16, 26, 18, 24, 16, 18, 22, 22, 26, 30, 22, 22, 24, 24, 28, 28, 26, 26, 26, 26, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28],
    [-1, 13, 22, 18, 26, 18, 24, 18, 22, 20, 24, 28, 26, 24, 20, 30, 24, 28, 28, 26, 30, 28, 30, 30, 30, 30, 28, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30],
    [-1, 17, 28, 22, 16, 22, 28, 26, 26, 24, 28, 24, 28, 22, 24, 24, 30, 28, 28, 26, 28, 30, 24, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30],
];

const NUM_BLOCKS: [[i8; 41]; 4] = [
    [-1, 1, 1, 1, 1, 1, 2, 2, 2, 2, 4, 4, 4, 4, 4, 6, 6, 6, 6, 7, 8, 8, 9, 9, 10, 12, 12, 12, 13, 14, 15, 16, 17, 18, 19, 19, 20, 21, 22, 24, 25],
    [-1, 1, 1, 1, 2, 2, 4, 4, 4, 5, 5, 5, 8, 9, 9, 10, 10, 11, 13, 14, 16, 17, 17, 18, 20, 21, 23, 25, 26, 28, 29, 31, 33, 35, 37, 38, 40, 43, 45, 47, 49],
    [-1, 1, 1, 2, 2, 4, 4, 6, 6, 8, 8, 8, 10, 12, 16, 12, 17, 16, 18, 21, 20, 23, 23, 25, 27, 29, 34, 34, 35, 38, 40, 43, 45, 48, 51, 53, 56, 59, 62, 65, 68],
    [-1, 1, 1, 2, 4, 4, 4, 5, 6, 8, 8, 11, 11, 16, 16, 18, 16, 19, 21, 25, 25, 25, 34, 30, 32, 35, 37, 40, 42, 45, 48, 51, 54, 57, 60, 63, 66, 70, 74, 77, 81],
];

/// A finished QR code: `size`×`size` modules, `true` = dark.
pub struct QrCode {
    pub version: usize,
    pub size: usize,
    modules: Vec<bool>,
    function: Vec<bool>,
}

fn raw_data_modules(ver: usize) -> usize {
    let mut r = (16 * ver + 128) * ver + 64;
    if ver >= 2 {
        let n = ver / 7 + 2;
        r -= (25 * n - 10) * n - 55;
        if ver >= 7 {
            r -= 36;
        }
    }
    r
}

fn data_codewords(ver: usize, ecc: Ecc) -> usize {
    raw_data_modules(ver) / 8 - ECC_PER_BLOCK[ecc.ord()][ver] as usize * NUM_BLOCKS[ecc.ord()][ver] as usize
}

fn gf_mul(x: u8, y: u8) -> u8 {
    let (x, y) = (x as u32, y as u32);
    let mut z: u32 = 0;
    for i in (0..8).rev() {
        z = (z << 1) ^ ((z >> 7) * 0x11D);
        z ^= ((y >> i) & 1) * x;
    }
    z as u8
}

fn rs_divisor(degree: usize) -> Vec<u8> {
    let mut r = vec![0u8; degree];
    r[degree - 1] = 1;
    let mut root = 1u8;
    for _ in 0..degree {
        for j in 0..degree {
            r[j] = gf_mul(r[j], root);
            if j + 1 < degree {
                r[j] ^= r[j + 1];
            }
        }
        root = gf_mul(root, 0x02);
    }
    r
}

fn rs_remainder(data: &[u8], div: &[u8]) -> Vec<u8> {
    let mut r = vec![0u8; div.len()];
    for &b in data {
        let f = b ^ r.remove(0);
        r.push(0);
        for (x, &y) in r.iter_mut().zip(div) {
            *x ^= gf_mul(y, f);
        }
    }
    r
}

impl QrCode {
    /// Encodes `data` at the smallest version that fits. `None` if it is too long for QR.
    pub fn encode(data: &[u8], ecc: Ecc) -> Option<QrCode> {
        let ver = (1..=40).find(|&v| {
            let count_bits = if v <= 9 { 8 } else { 16 };
            4 + count_bits + data.len() * 8 <= data_codewords(v, ecc) * 8 && (v > 9 || data.len() < 256)
        })?;
        let cap = data_codewords(ver, ecc) * 8;
        // Bit stream: byte mode, length, data, terminator, padding.
        let mut bits: Vec<bool> = Vec::with_capacity(cap);
        let push = |v: u32, n: usize, bits: &mut Vec<bool>| (0..n).rev().for_each(|i| bits.push((v >> i) & 1 == 1));
        push(0b0100, 4, &mut bits);
        push(data.len() as u32, if ver <= 9 { 8 } else { 16 }, &mut bits);
        for &b in data {
            push(b as u32, 8, &mut bits);
        }
        let term = (cap - bits.len()).min(4);
        push(0, term, &mut bits);
        let pad = (8 - bits.len() % 8) % 8;
        push(0, pad, &mut bits);
        let mut words: Vec<u8> = bits.chunks(8).map(|c| c.iter().fold(0u8, |a, &b| (a << 1) | b as u8)).collect();
        let mut alt = [0xECu8, 0x11].iter().cycle();
        while words.len() < cap / 8 {
            words.push(*alt.next().unwrap());
        }
        let all = Self::add_ecc(&words, ver, ecc);

        let size = ver * 4 + 17;
        let mut q = QrCode { version: ver, size, modules: vec![false; size * size], function: vec![false; size * size] };
        q.draw_function_patterns(ecc);
        q.draw_codewords(&all);
        // Pick the mask with the lowest penalty, as the standard asks.
        let mut best = (u32::MAX, 0);
        for m in 0..8 {
            q.apply_mask(m);
            q.draw_format(ecc, m);
            let p = q.penalty();
            if p < best.0 {
                best = (p, m);
            }
            q.apply_mask(m); // undo (XOR)
        }
        q.apply_mask(best.1);
        q.draw_format(ecc, best.1);
        Some(q)
    }

    fn add_ecc(data: &[u8], ver: usize, ecc: Ecc) -> Vec<u8> {
        let nblocks = NUM_BLOCKS[ecc.ord()][ver] as usize;
        let ecc_len = ECC_PER_BLOCK[ecc.ord()][ver] as usize;
        let raw = raw_data_modules(ver) / 8;
        let short = nblocks - raw % nblocks;
        let short_len = raw / nblocks;
        let div = rs_divisor(ecc_len);
        let mut blocks: Vec<Vec<u8>> = Vec::new();
        let mut k = 0;
        for i in 0..nblocks {
            let n = short_len - ecc_len + usize::from(i >= short);
            let mut b = data[k..k + n].to_vec();
            k += n;
            let e = rs_remainder(&b, &div);
            if i < short {
                b.push(0);
            }
            b.extend(e);
            blocks.push(b);
        }
        let mut out = Vec::with_capacity(raw);
        for i in 0..blocks[0].len() {
            for (j, b) in blocks.iter().enumerate() {
                if i != short_len - ecc_len || j >= short {
                    out.push(b[i]);
                }
            }
        }
        out
    }

    pub fn get(&self, x: usize, y: usize) -> bool {
        self.modules[y * self.size + x]
    }

    fn set_fn(&mut self, x: usize, y: usize, dark: bool) {
        self.modules[y * self.size + x] = dark;
        self.function[y * self.size + x] = true;
    }

    fn alignment_positions(&self) -> Vec<usize> {
        let v = self.version;
        if v == 1 {
            return vec![];
        }
        let n = v / 7 + 2;
        let step = (v * 8 + n * 3 + 5) / (n * 4 - 4) * 2;
        let mut r: Vec<usize> = (0..n - 1).map(|i| self.size - 7 - i * step).collect();
        r.push(6);
        r.reverse();
        r
    }

    fn draw_function_patterns(&mut self, ecc: Ecc) {
        let s = self.size;
        for i in 0..s {
            self.set_fn(6, i, i % 2 == 0);
            self.set_fn(i, 6, i % 2 == 0);
        }
        for (cx, cy) in [(3, 3), (s - 4, 3), (3, s - 4)] {
            for dy in -4i32..=4 {
                for dx in -4i32..=4 {
                    let (x, y) = (cx as i32 + dx, cy as i32 + dy);
                    if x >= 0 && y >= 0 && (x as usize) < s && (y as usize) < s {
                        let d = dx.abs().max(dy.abs());
                        self.set_fn(x as usize, y as usize, d != 2 && d != 4);
                    }
                }
            }
        }
        let pos = self.alignment_positions();
        let n = pos.len();
        for i in 0..n {
            for j in 0..n {
                if (i == 0 && j == 0) || (i == 0 && j == n - 1) || (i == n - 1 && j == 0) {
                    continue;
                }
                for dy in -2i32..=2 {
                    for dx in -2i32..=2 {
                        let d = dx.abs().max(dy.abs());
                        self.set_fn((pos[i] as i32 + dx) as usize, (pos[j] as i32 + dy) as usize, d != 1);
                    }
                }
            }
        }
        self.draw_format(ecc, 0);
        if self.version >= 7 {
            let v = self.version as u32;
            let mut rem = v;
            for _ in 0..12 {
                rem = (rem << 1) ^ ((rem >> 11) * 0x1F25);
            }
            let bits = (v << 12) | rem;
            for i in 0..18 {
                let dark = (bits >> i) & 1 == 1;
                let (a, b) = (s - 11 + i % 3, i / 3);
                self.set_fn(a, b, dark);
                self.set_fn(b, a, dark);
            }
        }
    }

    fn draw_format(&mut self, ecc: Ecc, mask: u8) {
        let data = (ecc.format_bits() << 3) | mask as u32;
        let mut rem = data;
        for _ in 0..10 {
            rem = (rem << 1) ^ ((rem >> 9) * 0x537);
        }
        let bits = ((data << 10) | rem) ^ 0x5412;
        let bit = |i: usize| (bits >> i) & 1 == 1;
        let s = self.size;
        for i in 0..=5 {
            self.set_fn(8, i, bit(i));
        }
        self.set_fn(8, 7, bit(6));
        self.set_fn(8, 8, bit(7));
        self.set_fn(7, 8, bit(8));
        for i in 9..15 {
            self.set_fn(14 - i, 8, bit(i));
        }
        for i in 0..8 {
            self.set_fn(s - 1 - i, 8, bit(i));
        }
        for i in 8..15 {
            self.set_fn(8, s - 15 + i, bit(i));
        }
        self.set_fn(8, s - 8, true);
    }

    fn draw_codewords(&mut self, data: &[u8]) {
        let s = self.size;
        let mut i = 0usize;
        let mut right = s as i32 - 1;
        while right >= 1 {
            if right == 6 {
                right = 5;
            }
            for vert in 0..s {
                for j in 0..2 {
                    let x = (right - j) as usize;
                    let up = (right + 1) & 2 == 0;
                    let y = if up { s - 1 - vert } else { vert };
                    if !self.function[y * s + x] && i < data.len() * 8 {
                        self.modules[y * s + x] = (data[i >> 3] >> (7 - (i & 7))) & 1 == 1;
                        i += 1;
                    }
                }
            }
            right -= 2;
        }
    }

    fn apply_mask(&mut self, m: u8) {
        let s = self.size;
        for y in 0..s {
            for x in 0..s {
                let inv = match m {
                    0 => (x + y) % 2 == 0,
                    1 => y % 2 == 0,
                    2 => x % 3 == 0,
                    3 => (x + y) % 3 == 0,
                    4 => (x / 3 + y / 2) % 2 == 0,
                    5 => x * y % 2 + x * y % 3 == 0,
                    6 => (x * y % 2 + x * y % 3) % 2 == 0,
                    _ => ((x + y) % 2 + x * y % 3) % 2 == 0,
                };
                if inv && !self.function[y * s + x] {
                    self.modules[y * s + x] ^= true;
                }
            }
        }
    }

    fn penalty(&self) -> u32 {
        let s = self.size;
        let mut p = 0u32;
        let line = |get: &dyn Fn(usize) -> bool, p: &mut u32| {
            let mut run = 1;
            for i in 1..s {
                if get(i) == get(i - 1) {
                    run += 1;
                    if run == 5 {
                        *p += 3;
                    } else if run > 5 {
                        *p += 1;
                    }
                } else {
                    run = 1;
                }
            }
            // Finder-like 1:1:3:1:1 with 4 light modules on a side (light outside the code counts).
            let at = |i: i32| i >= 0 && (i as usize) < s && get(i as usize);
            for i in -4i32..s as i32 {
                let core = [true, false, true, true, true, false, true];
                if (0..7).all(|k| at(i + k) == core[k as usize]) {
                    let before = (1..=4).all(|k| !at(i - k));
                    let after = (7..11).all(|k| !at(i + k));
                    if before || after {
                        *p += 40;
                    }
                }
            }
        };
        for y in 0..s {
            line(&|x| self.get(x, y), &mut p);
        }
        for x in 0..s {
            line(&|y| self.get(x, y), &mut p);
        }
        for y in 0..s - 1 {
            for x in 0..s - 1 {
                let c = self.get(x, y);
                if c == self.get(x + 1, y) && c == self.get(x, y + 1) && c == self.get(x + 1, y + 1) {
                    p += 3;
                }
            }
        }
        let dark = self.modules.iter().filter(|&&d| d).count();
        let total = s * s;
        let k = ((dark * 20).abs_diff(total * 10) + total - 1) / total;
        p + k.saturating_sub(1) as u32 * 10
    }

    /// For a terminal: two module rows per text line, dark on light (forced colours so it
    /// scans on dark terminal themes too), with the standard 4-module quiet zone.
    pub fn to_terminal(&self) -> String {
        let q = 4i32;
        let s = self.size as i32;
        let dark = |x: i32, y: i32| x >= 0 && y >= 0 && x < s && y < s && self.get(x as usize, y as usize);
        let mut out = String::new();
        let mut y = -q;
        while y < s + q {
            out.push_str("\x1b[30;107m");
            for x in -q..s + q {
                out.push(match (dark(x, y), dark(x, y + 1)) {
                    (true, true) => '█',
                    (true, false) => '▀',
                    (false, true) => '▄',
                    (false, false) => ' ',
                });
            }
            out.push_str("\x1b[0m\n");
            y += 2;
        }
        out
    }

    /// Rows of `0`/`1` (1 = dark), the 4-module quiet zone included: the format Omarchy's
    /// QR panels draw, one native rectangle per module.
    pub fn to_matrix(&self) -> Vec<String> {
        let (q, s) = (4i64, self.size as i64);
        (-q..s + q)
            .map(|y| (-q..s + q).map(|x| if x >= 0 && y >= 0 && x < s && y < s && self.get(x as usize, y as usize) { '1' } else { '0' }).collect())
            .collect()
    }

    /// A standalone SVG picture (black on white, quiet zone included).
    pub fn to_svg(&self) -> String {
        let n = self.size + 8;
        let mut path = String::new();
        for y in 0..self.size {
            for x in 0..self.size {
                if self.get(x, y) {
                    path.push_str(&format!("M{},{}h1v1h-1z", x + 4, y + 4));
                }
            }
        }
        format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {n} {n}\" width=\"{w}\" height=\"{w}\" shape-rendering=\"crispEdges\">\
<rect width=\"100%\" height=\"100%\" fill=\"#fff\"/><path d=\"{path}\" fill=\"#000\"/></svg>\n",
            w = n * 8
        )
    }

    /// A plain PGM picture (any viewer, and `zbarimg`, can read it).
    pub fn to_pgm(&self, scale: usize) -> Vec<u8> {
        let n = (self.size + 8) * scale;
        let mut out = format!("P5\n{n} {n}\n255\n").into_bytes();
        for py in 0..n {
            for px in 0..n {
                let (x, y) = ((px / scale) as i32 - 4, (py / scale) as i32 - 4);
                let d = x >= 0 && y >= 0 && (x as usize) < self.size && (y as usize) < self.size && self.get(x as usize, y as usize);
                out.push(if d { 0 } else { 255 });
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capacities_match_the_standard() {
        // Byte-mode capacities from the standard's tables (version, level, bytes).
        for (v, e, bytes) in [(1, Ecc::Low, 17), (1, Ecc::Medium, 14), (10, Ecc::Medium, 213), (20, Ecc::Medium, 666), (40, Ecc::Low, 2953), (40, Ecc::High, 1273)] {
            let count = if v <= 9 { 8 } else { 16 };
            assert_eq!((data_codewords(v, e) * 8 - 4 - count) / 8, bytes, "v{v} {e:?}");
        }
    }

    #[test]
    fn known_small_code() {
        let q = QrCode::encode(b"Thurea", Ecc::Medium).unwrap();
        assert_eq!((q.version, q.size), (1, 21));
        // Finder corners dark, separators light, dark module set.
        assert!(q.get(0, 0) && q.get(20, 0) && q.get(0, 20) && !q.get(7, 7) && q.get(8, 13));
    }

    #[test]
    fn too_long_is_refused() {
        assert!(QrCode::encode(&vec![b'a'; 3000], Ecc::Low).is_none());
    }
}
