//! Parameter spaces and deterministic sampling for `adage eval bench sweep`.
//!
//! ```toml
//! name = "learner"
//! suite = "core"            # optional; --suite overrides
//!
//! [[dims]]
//! param = "learn.min_support"   # a dotted path into EngineParams
//! kind = "int"                  # int | float | fraction | choice
//! min = 2
//! max = 8
//!
//! [[dims]]
//! param = "learn.fire_lower_bound"
//! kind = "float"
//! min = 0.4
//! max = 0.9
//! step = 0.05                   # optional: snap sampled values to this grid
//!
//! [[dims]]
//! param = "learn.dead_end_consistency"
//! kind = "fraction"             # sampled as a share, written as [num, den]
//! min = 0.7
//! max = 1.0
//! den = 20
//! ```
//!
//! Strategies: `grid` (each dimension's `grid` values, else min, midpoint and max; the full
//! product), `random:N` (uniform, a SplitMix64 stream from the seed), `sobol:N` (a Sobol sequence
//! with a seeded digital shift; up to [`SOBOL_DIMS`] dimensions) and `halton:N` (a Halton sequence
//! with a seeded rotation). The same space, strategy and seed give the same configurations.

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use serde_json::{json, Map, Value};

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Space {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub suite: Option<String>,
    pub dims: Vec<Dim>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Int,
    Float,
    Fraction,
    Choice,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Dim {
    pub param: String,
    pub kind: Kind,
    #[serde(default)]
    pub min: Option<f64>,
    #[serde(default)]
    pub max: Option<f64>,
    #[serde(default)]
    pub step: Option<f64>,
    /// `fraction`: the denominator of `[num, den]`.
    #[serde(default)]
    pub den: Option<u32>,
    /// `choice`: the values.
    #[serde(default)]
    pub values: Vec<Value>,
    /// `grid`: the values to try (default: min, midpoint, max).
    #[serde(default)]
    pub grid: Vec<Value>,
}

impl Space {
    pub fn load(path: &std::path::Path) -> Result<Space> {
        let text = std::fs::read_to_string(path).with_context(|| path.display().to_string())?;
        let s: Space = toml::from_str(&text).with_context(|| path.display().to_string())?;
        s.validate().with_context(|| path.display().to_string())?;
        Ok(s)
    }

    pub fn validate(&self) -> Result<()> {
        if self.dims.is_empty() {
            bail!("no dims");
        }
        for d in &self.dims {
            match d.kind {
                Kind::Choice if d.values.is_empty() => bail!("{}: a choice needs values", d.param),
                Kind::Choice => {}
                _ => {
                    let (Some(a), Some(b)) = (d.min, d.max) else { bail!("{}: needs min and max", d.param) };
                    if a > b || !a.is_finite() || !b.is_finite() {
                        bail!("{}: min must not exceed max", d.param);
                    }
                    if d.kind == Kind::Fraction && d.den.is_none_or(|x| x == 0) {
                        bail!("{}: a fraction needs den >= 1", d.param);
                    }
                    if d.kind == Kind::Int && (a.fract() != 0.0 || b.fract() != 0.0) {
                        bail!("{}: int bounds must be whole numbers", d.param);
                    }
                }
            }
            // The override must be a real parameter.
            adage_tracks::params::EngineParams::default()
                .with_overrides(&overrides(&[(d.param.as_str(), self.value(d, 0.5))]))
                .with_context(|| format!("dim {}", d.param))?;
        }
        Ok(())
    }

    /// The value of dimension `d` at `u` in [0, 1).
    pub fn value(&self, d: &Dim, u: f64) -> Value {
        let u = u.clamp(0.0, 1.0 - f64::EPSILON);
        let (a, b) = (d.min.unwrap_or(0.0), d.max.unwrap_or(0.0));
        match d.kind {
            Kind::Int => {
                let n = (b - a) as i64 + 1;
                json!(a as i64 + ((u * n as f64).floor() as i64).min(n - 1))
            }
            Kind::Float => json!(snap(a + u * (b - a), d.step, a, b)),
            Kind::Fraction => {
                let den = d.den.unwrap_or(10);
                let x = snap(a + u * (b - a), d.step, a, b);
                let num = ((x * den as f64).round() as u32).min(den);
                json!([num, den])
            }
            Kind::Choice => {
                let n = d.values.len();
                d.values[((u * n as f64).floor() as usize).min(n - 1)].clone()
            }
        }
    }

    /// The grid values of a dimension.
    fn grid_values(&self, d: &Dim) -> Vec<Value> {
        if !d.grid.is_empty() {
            return d.grid.clone();
        }
        match d.kind {
            Kind::Choice => d.values.clone(),
            _ => {
                let mut v: Vec<Value> = Vec::new();
                for x in
                    [d.min.unwrap_or(0.0), (d.min.unwrap_or(0.0) + d.max.unwrap_or(0.0)) / 2.0, d.max.unwrap_or(0.0)]
                {
                    let span = d.max.unwrap_or(0.0) - d.min.unwrap_or(0.0);
                    let u = if span > 0.0 { (x - d.min.unwrap_or(0.0)) / span } else { 0.0 };
                    let val = self.value(d, u);
                    if !v.contains(&val) {
                        v.push(val);
                    }
                }
                v
            }
        }
    }

    /// Sample configurations: each is a list of (param, value).
    pub fn sample(&self, strategy: &str, seed: u64) -> Result<Vec<Vec<(String, Value)>>> {
        let dims = self.dims.len();
        let points: Vec<Vec<f64>> = if strategy == "grid" {
            let mut configs: Vec<Vec<(String, Value)>> = vec![vec![]];
            for d in &self.dims {
                let mut next = Vec::new();
                for c in &configs {
                    for v in self.grid_values(d) {
                        let mut c = c.clone();
                        c.push((d.param.clone(), v));
                        next.push(c);
                    }
                }
                configs = next;
                if configs.len() > MAX_CONFIGS {
                    bail!("the grid has more than {MAX_CONFIGS} configurations; give dims a smaller `grid`");
                }
            }
            return Ok(configs);
        } else if let Some(n) = strategy.strip_prefix("random:") {
            let n = count(n)?;
            let mut rng = SplitMix64(seed);
            (0..n).map(|_| (0..dims).map(|_| rng.unit()).collect()).collect()
        } else if let Some(n) = strategy.strip_prefix("sobol:") {
            sobol(count(n)?, dims, seed)?
        } else if let Some(n) = strategy.strip_prefix("halton:") {
            halton(count(n)?, dims, seed)
        } else {
            bail!("unknown strategy {strategy:?} (grid, random:N, sobol:N, halton:N)")
        };
        Ok(points
            .into_iter()
            .map(|p| self.dims.iter().zip(p).map(|(d, u)| (d.param.clone(), self.value(d, u))).collect())
            .collect())
    }
}

/// A sweep of more configurations than this is refused.
pub const MAX_CONFIGS: usize = 10_000;

fn count(n: &str) -> Result<usize> {
    let n: usize = n.parse().with_context(|| format!("{n:?} is not a number of configurations"))?;
    if n == 0 || n > MAX_CONFIGS {
        bail!("between 1 and {MAX_CONFIGS} configurations");
    }
    Ok(n)
}

fn snap(x: f64, step: Option<f64>, a: f64, b: f64) -> f64 {
    let x = match step {
        Some(s) if s > 0.0 => a + ((x - a) / s).round() * s,
        _ => x,
    };
    // Four decimals: readable, and exactly reproducible.
    ((x.clamp(a, b)) * 1e4).round() / 1e4
}

/// A parameter override object from dotted paths: `learn.min_support` → `{"learn": {"min_support": v}}`.
pub fn overrides(pairs: &[(&str, Value)]) -> Value {
    let mut root = Map::new();
    for (path, v) in pairs {
        let parts: Vec<&str> = path.split('.').collect();
        let mut cur = &mut root;
        for p in &parts[..parts.len() - 1] {
            cur = cur.entry(p.to_string()).or_insert_with(|| Value::Object(Map::new())).as_object_mut().unwrap();
        }
        cur.insert(parts[parts.len() - 1].to_string(), v.clone());
    }
    Value::Object(root)
}

/// SplitMix64: a tiny, well-mixed, reproducible stream.
pub struct SplitMix64(pub u64);

impl SplitMix64 {
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// Uniform in [0, 1), 53 bits.
    pub fn unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// Sobol direction numbers (Joe and Kuo, new-joe-kuo-6.21201) for dimensions 2 to 13:
/// (degree s, polynomial coefficients a, initial m_1..m_s). Dimension 1 is the van der Corput
/// sequence.
const JOE_KUO: [(u32, u32, &[u32]); 12] = [
    (1, 0, &[1]),
    (2, 1, &[1, 3]),
    (3, 1, &[1, 3, 1]),
    (3, 2, &[1, 1, 1]),
    (4, 1, &[1, 1, 3, 3]),
    (4, 4, &[1, 3, 5, 13]),
    (5, 2, &[1, 1, 5, 5, 17]),
    (5, 4, &[1, 1, 5, 5, 5]),
    (5, 7, &[1, 1, 7, 11, 19]),
    (5, 11, &[1, 1, 5, 1, 1]),
    (5, 13, &[1, 1, 1, 3, 11]),
    (5, 14, &[1, 3, 5, 5, 31]),
];

/// Dimensions a Sobol sweep supports.
pub const SOBOL_DIMS: usize = JOE_KUO.len() + 1;

const BITS: u32 = 32;

fn directions(dim: usize) -> [u32; BITS as usize] {
    let mut v = [0u32; BITS as usize];
    if dim == 0 {
        for (i, x) in v.iter_mut().enumerate() {
            *x = 1 << (BITS - 1 - i as u32);
        }
        return v;
    }
    let (s, a, m) = JOE_KUO[dim - 1];
    let s = s as usize;
    for i in 0..s.min(BITS as usize) {
        v[i] = m[i] << (BITS - 1 - i as u32);
    }
    for i in s..BITS as usize {
        let mut x = v[i - s] ^ (v[i - s] >> s);
        for k in 1..s {
            if (a >> (s - 1 - k)) & 1 == 1 {
                x ^= v[i - k];
            }
        }
        v[i] = x;
    }
    v
}

/// `n` points of a `dims`-dimensional Sobol sequence (Gray-code order, starting at index 0), each
/// coordinate XOR-shifted by a value drawn from `seed` (a digital shift keeps the sequence's
/// uniformity; seed 0 is shifted too, deterministically).
pub fn sobol(n: usize, dims: usize, seed: u64) -> Result<Vec<Vec<f64>>> {
    if dims > SOBOL_DIMS {
        bail!("sobol supports up to {SOBOL_DIMS} dimensions (this space has {dims}); use halton:N");
    }
    let dirs: Vec<[u32; BITS as usize]> = (0..dims).map(directions).collect();
    let mut rng = SplitMix64(seed ^ SOBOL_SEED_MIX);
    let shift: Vec<u32> = (0..dims).map(|_| (rng.next_u64() >> 32) as u32).collect();
    let mut x = vec![0u32; dims];
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        if i > 0 {
            // The rightmost zero bit of i - 1 selects the direction number.
            let c = (!(i - 1)).trailing_zeros() as usize;
            for (xd, d) in x.iter_mut().zip(&dirs) {
                *xd ^= d[c.min(BITS as usize - 1)];
            }
        }
        out.push(x.iter().zip(&shift).map(|(v, s)| (v ^ s) as f64 / (1u64 << BITS) as f64).collect());
    }
    Ok(out)
}

/// Mixed into the seed of the Sobol shift, so that the shift differs from the random stream.
const SOBOL_SEED_MIX: u64 = 0x50b0_1c0d_e5ee_d000;

/// `n` points of a Halton sequence over the first `dims` primes, each coordinate rotated by a
/// seeded offset (a Cranley-Patterson rotation), skipping index 0.
pub fn halton(n: usize, dims: usize, seed: u64) -> Vec<Vec<f64>> {
    const PRIMES: [u32; 20] = [2, 3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37, 41, 43, 47, 53, 59, 61, 67, 71];
    let mut rng = SplitMix64(seed);
    let rot: Vec<f64> = (0..dims).map(|_| rng.unit()).collect();
    (1..=n)
        .map(|i| {
            (0..dims)
                .map(|d| {
                    let b = PRIMES[d % PRIMES.len()] as f64;
                    let (mut f, mut r, mut k) = (1.0, 0.0, i as f64);
                    while k > 0.0 {
                        f /= b;
                        r += f * (k % b);
                        k = (k / b).floor();
                    }
                    (r + rot[d]).fract()
                })
                .collect()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sobol_first_points_are_the_classic_ones_without_a_shift() {
        // Unshifted dimension 1 is van der Corput: 0, 1/2, 3/4, 1/4, 3/8, ...
        let dirs = directions(0);
        let mut x = 0u32;
        let mut got = vec![0.0];
        for i in 1..5usize {
            let c = (!(i - 1)).trailing_zeros() as usize;
            x ^= dirs[c];
            got.push(x as f64 / (1u64 << 32) as f64);
        }
        assert_eq!(got, vec![0.0, 0.5, 0.75, 0.25, 0.375]);
        // Dimension 2 (x + 1, m = 1): 0, 1/2, 1/4, 3/4, 3/8.
        let dirs = directions(1);
        let mut x = 0u32;
        let mut got = vec![0.0];
        for i in 1..5usize {
            let c = (!(i - 1)).trailing_zeros() as usize;
            x ^= dirs[c];
            got.push(x as f64 / (1u64 << 32) as f64);
        }
        assert_eq!(got, vec![0.0, 0.5, 0.25, 0.75, 0.375]);
    }

    #[test]
    fn sobol_points_stratify_every_dimension() {
        // The first 2^k points of a (shifted) Sobol sequence put exactly one point in each
        // interval [j/2^k, (j+1)/2^k) of every coordinate.
        let pts = sobol(16, SOBOL_DIMS, 7).unwrap();
        for d in 0..SOBOL_DIMS {
            let mut cells: Vec<usize> = pts.iter().map(|p| (p[d] * 16.0) as usize).collect();
            cells.sort();
            assert_eq!(cells, (0..16).collect::<Vec<_>>(), "dimension {d}");
        }
        assert_eq!(sobol(8, 3, 1).unwrap(), sobol(8, 3, 1).unwrap());
        assert_ne!(sobol(8, 3, 1).unwrap(), sobol(8, 3, 2).unwrap());
    }

    #[test]
    fn values_map_into_their_ranges() {
        let s: Space = toml::from_str(
            r#"
            name = "t"
            [[dims]]
            param = "learn.min_support"
            kind = "int"
            min = 2
            max = 8
            [[dims]]
            param = "learn.fire_lower_bound"
            kind = "float"
            min = 0.4
            max = 0.9
            step = 0.05
            [[dims]]
            param = "learn.dead_end_consistency"
            kind = "fraction"
            min = 0.7
            max = 1.0
            den = 20
            "#,
        )
        .unwrap();
        s.validate().unwrap();
        assert_eq!(s.value(&s.dims[0], 0.0), json!(2));
        assert_eq!(s.value(&s.dims[0], 0.9999), json!(8));
        assert_eq!(s.value(&s.dims[1], 0.5), json!(0.65));
        assert_eq!(s.value(&s.dims[2], 1.0), json!([20, 20]));
        assert_eq!(s.value(&s.dims[2], 0.0), json!([14, 20]));
        let grid = s.sample("grid", 0).unwrap();
        assert_eq!(grid.len(), 27);
        for strategy in ["random:5", "sobol:5", "halton:5"] {
            let a = s.sample(strategy, 3).unwrap();
            assert_eq!(a, s.sample(strategy, 3).unwrap(), "{strategy} is deterministic");
            assert_eq!(a.len(), 5);
        }
        assert_eq!(
            overrides(&[("learn.min_support", json!(3)), ("runtime.follow_window", json!(4))]),
            json!({"learn": {"min_support": 3}, "runtime": {"follow_window": 4}})
        );
    }
}
