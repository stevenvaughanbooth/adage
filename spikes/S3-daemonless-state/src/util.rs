//! Small helpers: CRC32 (IEEE), a seeded PRNG, a little-endian codec, hex, clocks.

use std::time::{SystemTime, UNIX_EPOCH};

const fn crc_table() -> [u32; 256] {
    let mut t = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            k += 1;
        }
        t[i] = c;
        i += 1;
    }
    t
}
static CRC_T: [u32; 256] = crc_table();

/// Incremental CRC32: start with `!0`, feed chunks, finish with `!state`.
pub fn crc32_update(mut c: u32, data: &[u8]) -> u32 {
    for &b in data {
        c = CRC_T[((c ^ b as u32) & 0xff) as usize] ^ (c >> 8);
    }
    c
}
pub fn crc32(data: &[u8]) -> u32 {
    !crc32_update(!0, data)
}

/// splitmix64 — deterministic, seedable, good enough for workloads and property cases.
#[derive(Clone)]
pub struct Rng(u64);
impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed ^ 0x5DEE_CE66_D1CE_4E5B)
    }
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 { 0 } else { self.next_u64() % n }
    }
    pub fn range(&mut self, lo: u64, hi_incl: u64) -> u64 {
        lo + self.below(hi_incl - lo + 1)
    }
    pub fn chance(&mut self, p: f64) -> bool {
        ((self.next_u64() >> 11) as f64 / (1u64 << 53) as f64) < p
    }
    pub fn bytes(&mut self, n: usize) -> Vec<u8> {
        // printable-ish bytes, like text heads
        (0..n).map(|_| b' ' + (self.below(95) as u8)).collect()
    }
    pub fn shuffle<T>(&mut self, v: &mut [T]) {
        for i in (1..v.len()).rev() {
            let j = self.below(i as u64 + 1) as usize;
            v.swap(i, j);
        }
    }
}

pub fn hash64(x: u64) -> u64 {
    Rng::new(x).next_u64()
}

/// Little-endian writer.
#[derive(Default)]
pub struct W(pub Vec<u8>);
impl W {
    pub fn u8(&mut self, v: u8) { self.0.push(v) }
    pub fn u16(&mut self, v: u16) { self.0.extend_from_slice(&v.to_le_bytes()) }
    pub fn u32(&mut self, v: u32) { self.0.extend_from_slice(&v.to_le_bytes()) }
    pub fn u64(&mut self, v: u64) { self.0.extend_from_slice(&v.to_le_bytes()) }
    pub fn f32(&mut self, v: f32) { self.0.extend_from_slice(&v.to_le_bytes()) }
    pub fn bool(&mut self, v: bool) { self.0.push(v as u8) }
    pub fn bytes(&mut self, b: &[u8]) {
        self.u32(b.len() as u32);
        self.0.extend_from_slice(b)
    }
}

/// Little-endian reader; every read is bounds-checked and returns None on underrun.
pub struct R<'a> {
    pub b: &'a [u8],
    pub p: usize,
}
impl<'a> R<'a> {
    pub fn new(b: &'a [u8]) -> Self { R { b, p: 0 } }
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        if self.p + n > self.b.len() { return None; }
        let s = &self.b[self.p..self.p + n];
        self.p += n;
        Some(s)
    }
    pub fn u8(&mut self) -> Option<u8> { Some(self.take(1)?[0]) }
    pub fn u16(&mut self) -> Option<u16> { Some(u16::from_le_bytes(self.take(2)?.try_into().ok()?)) }
    pub fn u32(&mut self) -> Option<u32> { Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?)) }
    pub fn u64(&mut self) -> Option<u64> { Some(u64::from_le_bytes(self.take(8)?.try_into().ok()?)) }
    pub fn f32(&mut self) -> Option<f32> { Some(f32::from_le_bytes(self.take(4)?.try_into().ok()?)) }
    pub fn bool(&mut self) -> Option<bool> { Some(self.u8()? != 0) }
    pub fn bytes(&mut self) -> Option<Vec<u8>> {
        let n = self.u32()? as usize;
        Some(self.take(n)?.to_vec())
    }
    pub fn done(&self) -> bool { self.p == self.b.len() }
}

pub fn to_hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}
pub fn from_hex(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 { return None; }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok()).collect()
}

pub fn wall_ns() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(0)
}

pub fn loadavg() -> String {
    std::process::Command::new("sysctl")
        .args(["-n", "vm.loadavg"])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().trim_matches(|c| c == '{' || c == '}').trim().to_string())
        .unwrap_or_default()
}

/// Nearest-rank percentile over a sorted slice (ns).
pub fn pct(sorted: &[u64], p: f64) -> u64 {
    if sorted.is_empty() { return 0; }
    let rank = ((p / 100.0) * sorted.len() as f64).ceil() as usize;
    sorted[rank.clamp(1, sorted.len()) - 1]
}
