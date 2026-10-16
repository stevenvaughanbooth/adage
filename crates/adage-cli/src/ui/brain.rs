//! Adage's brain as a lit solid drawn in text: the site hero's brain (`web/site/src/hero/scene.js`)
//! ported sample for sample: the same lobes, fissures, cerebellum and stem, folded by the same
//! seeded, domain-warped noise.
//!
//! The surface is sampled once into a point cloud with normals ([`Brain::new`]); a frame only
//! rotates it, splats it into a depth buffer at one terminal cell per sample, and reduces the
//! front surface's samples in each cell to one light value ([`Brain::render`]). Lighting follows
//! the hero (a key light from the upper left, Lambert diffuse, a specular glint, a Fresnel rim,
//! cavities darkened), drawn for a grid of cells rather than pixels:
//!
//! * a strong key and little ambient, so the lit side is dense and the far side sparse, and the
//!   volume reads at a glance;
//! * the folds drawn at two thirds of the hero's frequency, so a gyrus spans a few cells and a
//!   sulcus is a winding line between lit ridges rather than noise, and each groove's darkening
//!   kept at a cell's darkest, so a groove crossing a cell darkens all of it;
//! * the hemispheres set a little further apart and the walls between them darkened, so the
//!   longitudinal fissure is a seam a cell or two wide.
//!
//! Every function here is pure and every number is computed with IEEE arithmetic and the
//! polynomials in [`math`] (never the platform's `sin` or `exp`), so a pose and a size give the
//! same cells on every machine: the frames are snapshot-tested.

/// A point or direction: x to the brain's left, y up, z forward (towards the frontal lobes); in
/// view space x is right, y up and z towards the viewer.
type V3 = [f32; 3];

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn scale(a: V3, k: f32) -> V3 {
    [a[0] * k, a[1] * k, a[2] * k]
}
fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn normalize(a: V3) -> V3 {
    let l = dot(a, a).sqrt();
    if l > 1e-12 {
        scale(a, 1.0 / l)
    } else {
        a
    }
}
fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}
/// `exp(-(x / w)²)`: the hero's grooves and swellings are all this shape.
fn bell(x: f32, w: f32) -> f32 {
    let r = x / w;
    math::exp(-(r * r))
}

/// The few transcendental functions the brain needs, as plain polynomials: the platform's
/// `sin` and `exp` differ in their last bits between systems, and a frame must not.
pub mod math {
    use std::f32::consts::{FRAC_PI_2, PI};

    /// `e^x`, to about 1e-7 relative.
    pub fn exp(x: f32) -> f32 {
        if x < -87.0 {
            return 0.0;
        }
        let x = x.min(88.0);
        let k = (x * std::f32::consts::LOG2_E).round();
        // ln 2 split in two, so k·ln 2 is subtracted without losing r's low bits.
        let r = x - k * 0.693_145_75 - k * 1.428_606_8e-6;
        let p = 1.0 + r * (1.0 + r * (0.5 + r * (1.0 / 6.0 + r * (1.0 / 24.0 + r * (1.0 / 120.0 + r / 720.0)))));
        p * f32::from_bits(((k as i32 + 127) as u32) << 23)
    }

    /// `(sin x, cos x)`, to about 1e-7.
    pub fn sin_cos(x: f32) -> (f32, f32) {
        let q = (x / FRAC_PI_2).round();
        let r = x - q * 1.570_796_4 + q * 4.371_139e-8;
        let r2 = r * r;
        let s = r * (1.0 + r2 * (-1.0 / 6.0 + r2 * (1.0 / 120.0 + r2 * (-1.0 / 5040.0 + r2 / 362_880.0))));
        let c = 1.0 + r2 * (-0.5 + r2 * (1.0 / 24.0 + r2 * (-1.0 / 720.0 + r2 / 40_320.0)));
        match (q as i64).rem_euclid(4) {
            0 => (s, c),
            1 => (c, -s),
            2 => (-s, -c),
            _ => (-c, s),
        }
    }

    pub fn sin(x: f32) -> f32 {
        sin_cos(x).0
    }

    /// `atan2(y, x)`, to about 1e-5.
    pub fn atan2(y: f32, x: f32) -> f32 {
        let (ax, ay) = (x.abs(), y.abs());
        if ax == 0.0 && ay == 0.0 {
            return 0.0;
        }
        let z = ax.min(ay) / ax.max(ay);
        let z2 = z * z;
        let mut a = z * (0.999_866 + z2 * (-0.330_299_5 + z2 * (0.180_141 + z2 * (-0.085_133 + z2 * 0.020_835_1))));
        if ay > ax {
            a = FRAC_PI_2 - a;
        }
        if x < 0.0 {
            a = PI - a;
        }
        if y < 0.0 {
            -a
        } else {
            a
        }
    }
}

/// The brain's centre height, as in the hero.
const BY: f32 = 0.62;

/// Improved Perlin noise (Ken Perlin, 2002), seeded as the hero seeds it (a Park–Miller shuffle
/// of the permutation), so the folds fall where the site's do.
struct Noise {
    p: [u8; 512],
}

impl Noise {
    fn new(seed: u64) -> Noise {
        let mut s = seed;
        let mut rnd = move || {
            s = (s * 16807) % 2_147_483_647;
            s as f64 / 2_147_483_647.0
        };
        let mut base: [u8; 256] = std::array::from_fn(|i| i as u8);
        for i in (1..256).rev() {
            let j = (rnd() * (i as f64 + 1.0)).floor() as usize;
            base.swap(i, j);
        }
        Noise { p: std::array::from_fn(|i| base[i & 255]) }
    }

    fn at(&self, x: f32, y: f32, z: f32) -> f32 {
        fn fade(t: f32) -> f32 {
            t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
        }
        fn lerp(a: f32, b: f32, t: f32) -> f32 {
            a + t * (b - a)
        }
        fn grad(h: u8, x: f32, y: f32, z: f32) -> f32 {
            let h = h & 15;
            let u = if h < 8 { x } else { y };
            let v = match h {
                0..=3 => y,
                12 | 14 => x,
                _ => z,
            };
            (if h & 1 != 0 { -u } else { u }) + (if h & 2 != 0 { -v } else { v })
        }
        let (fx, fy, fz) = (x.floor(), y.floor(), z.floor());
        let cell = |f: f32| (f as i32 & 255) as usize;
        let (xi, yi, zi) = (cell(fx), cell(fy), cell(fz));
        let (x, y, z) = (x - fx, y - fy, z - fz);
        let (u, v, w) = (fade(x), fade(y), fade(z));
        let p = |i: usize| self.p[i] as usize;
        let a = p(xi) + yi;
        let (aa, ab) = (p(a) + zi, p(a + 1) + zi);
        let b = p(xi + 1) + yi;
        let (ba, bb) = (p(b) + zi, p(b + 1) + zi);
        let g = |i: usize, x: f32, y: f32, z: f32| grad(self.p[i], x, y, z);
        lerp(
            lerp(
                lerp(g(aa, x, y, z), g(ba, x - 1.0, y, z), u),
                lerp(g(ab, x, y - 1.0, z), g(bb, x - 1.0, y - 1.0, z), u),
                v,
            ),
            lerp(
                lerp(g(aa + 1, x, y, z - 1.0), g(ba + 1, x - 1.0, y, z - 1.0), u),
                lerp(g(ab + 1, x, y - 1.0, z - 1.0), g(bb + 1, x - 1.0, y - 1.0, z - 1.0), u),
                v,
            ),
            w,
        )
    }
}

/// One surface sample: where it is, which way it faces (folded, and with the fine folds
/// smoothed away), how open it is (1 on a ridge, down to about 0.2 deep in a fold or under
/// another part), and how far the cord has faded out.
#[derive(Debug, Clone, Copy)]
struct Sample {
    p: V3,
    n: V3,
    smooth: V3,
    ao: f32,
    fade: f32,
}

/// A grid of surface points, rows by columns, its columns wrapping round: each point with the
/// same point without the fine folds, its outward direction (to orient normals), its cavity
/// (the hero's `cav`, widened for text) and how far the cord has faded out.
struct Grid {
    rows: usize,
    cols: usize,
    p: Vec<V3>,
    smooth: Vec<V3>,
    out: Vec<V3>,
    cav: Vec<f32>,
    fade: Vec<f32>,
}

impl Grid {
    fn with(rows: usize, cols: usize) -> Grid {
        let n = rows * cols;
        Grid {
            rows,
            cols,
            p: Vec::with_capacity(n),
            smooth: Vec::with_capacity(n),
            out: Vec::with_capacity(n),
            cav: Vec::with_capacity(n),
            fade: Vec::with_capacity(n),
        }
    }

    fn push(&mut self, p: V3, smooth: V3, out: V3, cav: f32, fade: f32) {
        self.p.push(p);
        self.smooth.push(smooth);
        self.out.push(out);
        self.cav.push(cav);
        self.fade.push(fade);
    }

    /// The normal at (r, c) of the surface `p` lays out, from its neighbours, oriented outward.
    fn normal(&self, p: &[V3], r: usize, c: usize) -> V3 {
        let at = |r: usize, c: usize| p[r * self.cols + c];
        let du = sub(at(r, (c + 1) % self.cols), at(r, (c + self.cols - 1) % self.cols));
        let dv = sub(at((r + 1).min(self.rows - 1), c), at(r.saturating_sub(1), c));
        let out = self.out[r * self.cols + c];
        let n = cross(du, dv);
        // At a pole the neighbours coincide: the outward direction stands in.
        let n = if dot(n, n) < 1e-14 { out } else { normalize(n) };
        if dot(n, out) < 0.0 {
            scale(n, -1.0)
        } else {
            n
        }
    }

    /// Each kept point as a sample. `keep(row, col)` thins the rows near a pole, where a
    /// sphere's grid crowds together.
    fn samples(&self, keep: impl Fn(usize, usize) -> bool, out: &mut Vec<Sample>) {
        for r in 0..self.rows {
            for c in (0..self.cols).filter(|&c| keep(r, c)) {
                let i = r * self.cols + c;
                out.push(Sample {
                    p: self.p[i],
                    n: self.normal(&self.p, r, c),
                    smooth: self.normal(&self.smooth, r, c),
                    ao: 0.28 + 0.72 * smoothstep(-0.9, 0.7, self.cav[i]),
                    fade: self.fade[i],
                });
            }
        }
    }
}

/// A unit sphere's grid as three.js lays one out (`SphereGeometry(1, cols, rows - 1)`), less the
/// seam's repeated column: (row, col) → direction.
fn sphere_dir(rows: usize, cols: usize, r: usize, c: usize) -> V3 {
    use std::f32::consts::{PI, TAU};
    let (sp, cp) = math::sin_cos(c as f32 / cols as f32 * TAU);
    let (st, ct) = math::sin_cos(r as f32 / (rows - 1) as f32 * PI);
    [-cp * st, ct, sp * st]
}

/// Keep every point at the equator and fewer towards the poles, so samples cover the sphere
/// about evenly.
fn sphere_keep(rows: usize) -> impl Fn(usize, usize) -> bool {
    move |r, c| {
        let s = math::sin(r as f32 / (rows - 1) as f32 * std::f32::consts::PI);
        if s < 1e-4 {
            c == 0
        } else {
            c % ((1.0 / s) as usize).max(1) == 0
        }
    }
}

/// The folds' frequency: the hero's is 2.7, too fine for a grid of cells to draw.
const FOLDS: f32 = 1.7;
/// How wide a groove's darkening is, against the hero's groove: narrower, as a cell crossed by
/// any of it is darkened ([`Brain::render`]), so the line stays about one cell wide.
const GROOVE_LINE: f32 = 0.6;
/// How far each hemisphere's centre is from the midline: the hero's 0.36 leaves a fissure too
/// narrow to see in a cell.
const SPREAD: f32 = 0.46;

/// One hemisphere (`side` -1 or 1): a sphere shaped into lobes, then folded.
fn hemisphere(noise: &Noise, side: f32, rows: usize, cols: usize) -> Grid {
    let mut g = Grid::with(rows, cols);
    for r in 0..rows {
        for c in 0..cols {
            let d = sphere_dir(rows, cols, r, c);
            let lat = d[0] * side; // > 0: the outer face
            let mut sx = if lat < 0.0 { 0.33 } else { 0.8 };
            let mut sy = if d[1] > 0.0 { 0.9 } else { 0.5 };
            sx *= 1.0 - 0.2 * (-d[2]).max(0.0) - 0.12 * (d[2] - 0.45).max(0.0);
            sy *= 1.0 - 0.14 * (-d[2] - 0.3).max(0.0);
            let mut q = [side * SPREAD + d[0] * sx, BY + d[1] * sy, d[2] * 1.4];
            // The temporal lobe: low, lateral, forward of centre.
            let dz = d[2] - 0.25;
            let tl = (lat - 0.1).max(0.0) * (0.2 - d[1]).max(0.0) * math::exp(-(dz * dz) / 0.28);
            q[1] -= tl * 0.55;
            q[0] += side * tl * 0.12;
            // The frontal base tucks up; the occipital pole points back.
            if d[1] < -0.2 && d[2] > 0.3 {
                q[1] += 0.08 * (d[2] - 0.3);
            }
            // Sulci are the zero-set of domain-warped noise: narrow grooves, broad rounded gyri,
            // gentler on the medial wall and underside.
            let k = 1.3;
            let wx = q[0] + 0.35 * noise.at(q[0] * k, q[1] * k + 9.0, q[2] * k);
            let wy = q[1] + 0.35 * noise.at(q[0] * k + 4.0, q[1] * k, q[2] * k);
            let wz = q[2] + 0.35 * noise.at(q[0] * k, q[1] * k, q[2] * k + 7.0);
            let n1 = noise.at(wx * FOLDS + 3.0, wy * FOLDS, wz * FOLDS);
            let n2 = noise.at(wx * 5.6, wy * 5.6 + 5.0, wz * 5.6);
            let soft = if lat < -0.35 {
                0.45
            } else if d[1] < -0.55 {
                0.6
            } else {
                1.0
            };
            // The lateral (Sylvian) fissure above the temporal lobe; the central sulcus over the top.
            let sy0 = if lat > 0.2 && d[2] > -0.45 {
                bell(d[1] - (-0.05 + 0.3 * d[2]), 0.06) * ((d[2] + 0.45) / 0.25).min(1.0)
            } else {
                0.0
            };
            let cen = |w: f32| {
                if d[1] > 0.05 && lat > -0.2 {
                    bell(d[2] - (0.08 - 0.22 * d[1]), 0.05 * w)
                } else {
                    0.0
                }
            };
            let ce0 = cen(1.0);
            let groove = (bell(n1, 0.075) * 0.8 + bell(n2, 0.06) * 0.35) * soft * (1.0 - sy0) * (1.0 - ce0);
            let folded = add(q, scale(d, -groove * 0.09 - sy0 * 0.16 - ce0 * 0.08 + 0.01));
            let smooth = add(q, scale(d, -sy0 * 0.16 + 0.01));
            // The darkening: the main folds only (the fine octave would only be noise), and the
            // walls of the longitudinal fissure, near the midline.
            let ce1 = cen(GROOVE_LINE);
            let groove = bell(n1, 0.075 * GROOVE_LINE) * 0.8 * soft * (1.0 - sy0) * (1.0 - ce1);
            let fissure = 3.0 * smoothstep(0.26, 0.06, folded[0].abs());
            g.push(folded, smooth, d, 0.6 - groove * 2.6 - sy0 * 3.0 - ce1 * 2.2 - fissure, 1.0);
        }
    }
    g
}

/// The cerebellum: finely foliated, tucked under the occipital lobes.
fn cerebellum(noise: &Noise, rows: usize, cols: usize) -> Grid {
    let (cy, cz) = (BY - 0.58, -0.84);
    let mut g = Grid::with(rows, cols);
    for r in 0..rows {
        for c in 0..cols {
            let d = sphere_dir(rows, cols, r, c);
            let x = d[0] * 0.74 * (1.0 - 0.1 * d[1].abs());
            let y = d[1] * if d[1] > 0.0 { 0.24 } else { 0.34 } + cy;
            let z = d[2] * 0.46 + cz;
            let vermis = bell(d[0], 0.16) * (0.1 * (-d[2]).max(0.0) + 0.06 * (-d[1]).max(0.0));
            let phi = math::atan2(d[1], -d[2]);
            let folia = math::sin(phi * 30.0 + noise.at(d[0] * 2.5, d[1] * 2.5, d[2] * 2.5) * 1.6);
            let hfis = bell(phi - 0.1, 0.05) * (1.0 - bell(d[0], 0.2));
            let k = folia * 0.5;
            let at = |rr: f32| [x * rr, cy + (y - cy) * rr, cz + (z - cz) * rr];
            let folded = at(1.0 + k * 0.018 - vermis - hfis * 0.05);
            let smooth = at(1.0 - vermis - hfis * 0.05);
            g.push(folded, smooth, d, k * 1.4 - vermis * 12.0 - hfis * 3.0, 1.0);
        }
    }
    g
}

/// Where the cord's axis is at height `y`.
fn cord_at(y: f32) -> V3 {
    if y > -1.15 {
        let t = (0.2 - y) / 1.35;
        [0.0, y, -0.34 - 0.24 * t]
    } else {
        [0.0, y, -0.58 - 0.035 * math::sin((y + 1.15) * 0.9) + (y + 1.15) * 0.012]
    }
}

/// The cord's radius at height `y`.
fn cord_r(y: f32) -> f32 {
    if y > -0.35 {
        0.26 - 0.06 * ((0.2 - y) / 0.55)
    } else if y > -1.15 {
        0.2 - 0.07 * ((-0.35 - y) / 0.8) + 0.035 * bell(y + 0.55, 0.18)
    } else {
        0.125 - 0.02 * ((-1.15 - y) / 3.2).min(1.0)
    }
}

/// Where the drawn cord ends, and where it starts fading out towards that end. The hero's cord
/// runs on down the spine; here it trails off below the brainstem.
const CORD_END: f32 = -0.7;
const CORD_FADE: f32 = -0.2;

/// The brainstem and the top of the spinal cord: one tapering tube, the pons swelling at its
/// front and the anterior fissure down its middle.
fn stem(noise: &Noise, rows: usize, cols: usize) -> Grid {
    let mut g = Grid::with(rows, cols);
    for r in 0..rows {
        let y = 0.25 + (CORD_END - 0.25) * (r as f32 / (rows - 1) as f32);
        let (axis, rad) = (cord_at(y), cord_r(y));
        for c in 0..cols {
            let a = c as f32 / cols as f32 * std::f32::consts::TAU;
            let (sa, ca) = math::sin_cos(a);
            let fiss = math::exp(-(ca * ca + (sa - 1.0) * (sa - 1.0)) / 0.02) * 0.012;
            let front = sa.max(0.0);
            let pons = 0.11 * bell(y + 0.12, 0.15) * front * front.sqrt();
            let ring = |rr: f32, pons: f32| [axis[0] + ca * (rr + pons * 0.6), y, axis[2] + sa * (rr + pons) * 0.92];
            let cav = -fiss * 60.0 + noise.at(a * 2.0, y * 6.0, 0.0) * 0.3;
            g.push(ring(rad - fiss, pons), ring(rad, pons), [ca, 0.0, sa], cav, smoothstep(CORD_END, CORD_FADE, y));
        }
    }
    g
}

/// Where the parts shadow one another (the cerebellum under the occipital lobes, the stem
/// between them, a fold's walls): each sample looks out along its normal through a coarse grid
/// of where the surface is, and what it meets close by darkens it.
fn occlude(samples: &mut [Sample]) {
    const N: usize = 72;
    const STRENGTH: f32 = 0.3;
    let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
    for s in samples.iter() {
        for a in 0..3 {
            lo[a] = lo[a].min(s.p[a]);
            hi[a] = hi[a].max(s.p[a]);
        }
    }
    let cell = (0..3).map(|a| hi[a] - lo[a]).fold(0.0, f32::max) * 1.02 / N as f32;
    let index = |p: V3| -> Option<usize> {
        let mut i = 0;
        for a in 0..3 {
            let v = ((p[a] - lo[a]) / cell).floor();
            if !(0.0..N as f32).contains(&v) {
                return None;
            }
            i = i * N + v as usize;
        }
        Some(i)
    };
    let mut grid = vec![false; N * N * N];
    for s in samples.iter() {
        if let Some(i) = index(s.p) {
            grid[i] = true;
        }
    }
    // Probes from two cells out (nearer would meet the sample's own surface), nearer ones
    // counting for more.
    const STEPS: [f32; 5] = [2.0, 3.0, 4.5, 6.5, 9.0];
    let most: f32 = (0..STEPS.len()).map(|j| 1.0 / (1.0 + j as f32 * 0.5)).sum();
    for s in samples.iter_mut() {
        let mut hit = 0.0;
        for (j, d) in STEPS.iter().enumerate() {
            if index(add(s.p, scale(s.n, d * cell))).is_some_and(|i| grid[i]) {
                hit += 1.0 / (1.0 + j as f32 * 0.5);
            }
        }
        s.ao *= 1.0 - STRENGTH * hit / most;
    }
}

/// The key light, fixed to the viewer: from the left, a little above, in front.
const LIGHT: V3 = [-0.5, 0.3, 0.8];

/// How the brain is turned: `yaw` about the vertical axis (0 faces the frontal lobes to the
/// viewer; -π/2 is the profile, frontal lobes to the left), then `pitch` tilting the top
/// towards the viewer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pose {
    pub yaw: f32,
    pub pitch: f32,
}

/// Where the brain sits on a canvas: columns per world unit, and where the centre of its turn
/// lands ([`Brain::fit`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fit {
    pub scale: f32,
    pub cx: f32,
    pub cy: f32,
}

/// One cell of a frame: how much light its front surface sends back (0 dark, about 1 lit, a
/// little more at a glint), how much of it there is (1, less where the cord fades out), and its
/// depth (towards the viewer is larger; `f32::NEG_INFINITY` where nothing was drawn).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cell {
    pub light: f32,
    pub alpha: f32,
    pub z: f32,
}

impl Cell {
    const EMPTY: Cell = Cell { light: 0.0, alpha: 0.0, z: f32::NEG_INFINITY };

    pub fn drawn(&self) -> bool {
        self.z > f32::NEG_INFINITY
    }
}

/// Per cell while a frame is drawn: the light summed without cavities, the cavities summed,
/// the darkest cavity, the fades summed, and how many samples.
#[derive(Debug, Clone, Copy)]
struct Acc {
    light: f32,
    ao: f32,
    ao_min: f32,
    fade: f32,
    n: f32,
}

impl Acc {
    const ZERO: Acc = Acc { light: 0.0, ao: 0.0, ao_min: 1.0, fade: 0.0, n: 0.0 };
}

/// A frame: `cols` by `rows` terminal cells, each about twice as tall as it is wide, with the
/// scratch space drawing it needs (kept, so a frame allocates nothing).
pub struct Canvas {
    pub cols: usize,
    pub rows: usize,
    cells: Vec<Cell>,
    acc: Vec<Acc>,
    hits: Vec<(u32, u32, f32)>,
}

impl Canvas {
    pub fn new(cols: usize, rows: usize) -> Canvas {
        Canvas {
            cols,
            rows,
            cells: vec![Cell::EMPTY; cols * rows],
            acc: vec![Acc::ZERO; cols * rows],
            hits: Vec::new(),
        }
    }

    pub fn at(&self, col: usize, row: usize) -> Cell {
        self.cells[row * self.cols + col]
    }
}

/// A pulse of light on the cord: where it is, which of the hero's three colours (0 fire,
/// 1 prime, 2 inhibit), and how bright (0 to 1).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spark {
    pub col: usize,
    pub row: usize,
    pub hue: usize,
    pub glow: f32,
}

/// The view: rotation, then a gentle perspective, onto the canvas.
struct Camera {
    m: [V3; 3],
    center: V3,
    fit: Fit,
}

impl Camera {
    /// How far the eye is from the centre of the turn, in world units.
    const DIST: f32 = 7.0;

    fn project(&self, p: V3) -> (f32, f32, f32) {
        let q = sub(p, self.center);
        let (x, y, z) = (dot(self.m[0], q), dot(self.m[1], q), dot(self.m[2], q));
        let k = Self::DIST / (Self::DIST - z) * self.fit.scale;
        // A cell is about twice as tall as it is wide.
        (self.fit.cx + x * k, self.fit.cy - y * k * 0.5, z)
    }
}

/// The brain, sampled once.
pub struct Brain {
    samples: Vec<Sample>,
    /// The centre of the turn: on the axis of symmetry, halfway along and halfway up.
    center: V3,
    /// The Fresnel rim and the specular glint, by cosine (0 to 1), in [`LUT`] steps.
    rim: Vec<f32>,
    glint: Vec<f32>,
}

const LUT: usize = 256;

impl Brain {
    /// Sample the surface: the cortex at the hero's own resolution, the rest to match.
    pub fn new() -> Brain {
        let noise = Noise::new(7);
        let mut samples = Vec::with_capacity(80_000);
        let (rows, cols) = (171, 240);
        for side in [-1.0, 1.0] {
            hemisphere(&noise, side, rows, cols).samples(sphere_keep(rows), &mut samples);
        }
        let (rows, cols) = (111, 150);
        cerebellum(&noise, rows, cols).samples(sphere_keep(rows), &mut samples);
        stem(&noise, 90, 48).samples(|_, _| true, &mut samples);
        occlude(&mut samples);

        let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
        for s in samples.iter().filter(|s| s.fade > 0.2) {
            for a in 0..3 {
                lo[a] = lo[a].min(s.p[a]);
                hi[a] = hi[a].max(s.p[a]);
            }
        }
        let center = [0.0, (lo[1] + hi[1]) * 0.5, (lo[2] + hi[2]) * 0.5];
        let steps = |f: fn(f32) -> f32| (0..LUT).map(|i| f(i as f32 / (LUT - 1) as f32)).collect();
        // (1 - cos)^2.5 for the hero's ^2.4; cos^36, by squaring.
        let rim = steps(|c| {
            let f = 1.0 - c;
            f * f * f.sqrt()
        });
        let glint = steps(|c| {
            let c4 = c * c * c * c;
            let c32 = c4 * c4 * c4 * c4 * c4 * c4 * c4 * c4;
            c32 * c4
        });
        Brain { samples, center, rim, glint }
    }

    fn camera(&self, pose: Pose, fit: Fit) -> Camera {
        let (sy, cy) = math::sin_cos(pose.yaw);
        let (sp, cp) = math::sin_cos(pose.pitch);
        // Rx(pitch) · Ry(yaw).
        let m = [[cy, 0.0, sy], [sp * sy, cp, -sp * cy], [-cp * sy, sp, cp * cy]];
        Camera { m, center: self.center, fit }
    }

    /// The scale and placement that fit the brain on a `cols` by `rows` canvas in every one of
    /// `poses`, so it neither grows nor drifts as it turns.
    pub fn fit(&self, poses: &[Pose], cols: usize, rows: usize) -> Fit {
        let unit = Fit { scale: 1.0, cx: 0.0, cy: 0.0 };
        let (mut x0, mut x1, mut y0, mut y1) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
        for &pose in poses {
            let cam = self.camera(pose, unit);
            for s in self.samples.iter().step_by(5).filter(|s| s.fade > 0.2) {
                let (x, y, _) = cam.project(s.p);
                x0 = x0.min(x);
                x1 = x1.max(x);
                y0 = y0.min(y);
                y1 = y1.max(y);
            }
        }
        if x0 >= x1 || y0 >= y1 {
            return Fit { scale: 1.0, cx: cols as f32 * 0.5, cy: rows as f32 * 0.5 };
        }
        let scale = (cols as f32 * 0.98 / (x1 - x0)).min(rows as f32 * 0.98 / (y1 - y0));
        Fit { scale, cx: cols as f32 * 0.5 - scale * (x0 + x1) * 0.5, cy: rows as f32 * 0.5 - scale * (y0 + y1) * 0.5 }
    }

    /// Draw the brain in `pose`, placed by `fit`, onto `canvas`.
    pub fn render(&self, pose: Pose, fit: Fit, canvas: &mut Canvas) {
        let (cols, rows) = (canvas.cols, canvas.rows);
        canvas.cells.fill(Cell::EMPTY);
        canvas.acc.fill(Acc::ZERO);
        canvas.hits.clear();
        if cols == 0 || rows == 0 {
            return;
        }
        let cam = self.camera(pose, fit);
        // Pass 1: the front surface's depth in each cell. A sample facing away is skipped,
        // less a margin, so a silhouette's last samples are kept.
        for (i, s) in self.samples.iter().enumerate() {
            if dot(cam.m[2], s.n) < -0.15 {
                continue;
            }
            let (x, y, z) = cam.project(s.p);
            if x < 0.0 || y < 0.0 || x >= cols as f32 || y >= rows as f32 {
                continue;
            }
            let at = y as usize * cols + x as usize;
            let c = &mut canvas.cells[at];
            c.z = c.z.max(z);
            canvas.hits.push((i as u32, at as u32, z));
        }
        // Pass 2: light the samples on that surface: within about a row and a half of it, so a
        // fold's walls count but the far hemisphere behind does not.
        let eps = 3.0 / fit.scale;
        let l = normalize(LIGHT);
        let h = normalize([l[0], l[1], l[2] + 1.0]);
        let lut = |t: &[f32], x: f32| t[(x.clamp(0.0, 1.0) * (LUT - 1) as f32) as usize];
        for &(i, at, z) in &canvas.hits {
            if z < canvas.cells[at as usize].z - eps {
                continue;
            }
            let s = &self.samples[i as usize];
            // Half the smooth form, half the folds: the lobes keep their volume and each gyrus
            // its lit side.
            let n = normalize(add(scale(s.smooth, 0.45), scale(s.n, 0.55)));
            let n = [dot(cam.m[0], n), dot(cam.m[1], n), dot(cam.m[2], n)];
            let ndl = dot(n, l);
            let lit = 0.04 + 0.1 * (ndl * 0.5 + 0.5) + ndl.max(0.0);
            let light = lit + 0.3 * lut(&self.glint, dot(n, h)) + 0.15 * lut(&self.rim, n[2]);
            let a = &mut canvas.acc[at as usize];
            a.light += light;
            a.ao += s.ao;
            a.ao_min = a.ao_min.min(s.ao);
            a.fade += s.fade;
            a.n += 1.0;
        }
        for (c, a) in canvas.cells.iter_mut().zip(&canvas.acc) {
            if a.n > 0.0 {
                // Mostly the cell's darkest cavity: a groove crossing a cell darkens all of it.
                let ao = a.ao / a.n * 0.4 + a.ao_min * 0.6;
                c.light = a.light / a.n * ao;
                c.alpha = a.fade / a.n;
            } else {
                c.z = f32::NEG_INFINITY;
            }
        }
        // A cell the samples skipped, inside the surface: the average of its neighbours.
        for row in 1..rows.saturating_sub(1) {
            for col in 1..cols - 1 {
                let at = row * cols + col;
                if canvas.cells[at].drawn() {
                    continue;
                }
                let around = [at - 1, at + 1, at - cols, at + cols].map(|j| canvas.cells[j]);
                let across = (around[0].drawn() && around[1].drawn()) || (around[2].drawn() && around[3].drawn());
                let drawn = around.iter().filter(|c| c.drawn());
                let k = drawn.clone().count();
                if k >= 3 || across {
                    canvas.cells[at] = Cell {
                        light: drawn.clone().map(|c| c.light).sum::<f32>() / k as f32,
                        alpha: drawn.clone().map(|c| c.alpha).sum::<f32>() / k as f32,
                        z: drawn.map(|c| c.z).fold(f32::NEG_INFINITY, f32::max),
                    };
                }
            }
        }
    }

    /// The pulses running up the cord into the brain at `t` seconds, as the hero's reflex arcs
    /// light it: fire, prime and inhibit, half a second apart, each trailing two fainter copies of
    /// itself 0.12 s behind one another. Only those in front of the cord are returned.
    pub fn sparks(&self, pose: Pose, fit: Fit, t: f32, canvas: &Canvas) -> Vec<Spark> {
        const PERIOD: f32 = 1.5;
        const TRAIL: usize = 3;
        let cam = self.camera(pose, fit);
        // The side of the cord facing the viewer, in the brain's own frame.
        let toward = math::atan2(cam.m[2][2], cam.m[2][0]);
        let mut out = Vec::new();
        for hue in 0..3 {
            for step in 0..TRAIL {
                let phase = (t - step as f32 * 0.12 + hue as f32 * 0.5) / PERIOD;
                if phase < 0.0 {
                    continue;
                }
                let phase = phase - phase.floor();
                // From the cord's faded end up into the brainstem, slowing as it arrives.
                let y = CORD_END + 0.05 + (0.05 - CORD_END - 0.05) * smoothstep(0.0, 1.0, phase);
                let rise = math::sin(phase * std::f32::consts::PI).max(0.0);
                let glow = rise * smoothstep(CORD_END, CORD_FADE, y).max(0.35) / (1.0 + step as f32 * 1.5);
                let (sa, ca) = math::sin_cos(toward + (hue as f32 - 1.0) * 0.55);
                let (axis, r) = (cord_at(y), cord_r(y) * 1.04);
                let (x, yy, z) = cam.project([axis[0] + ca * r, y, axis[2] + sa * r]);
                if x < 0.0 || yy < 0.0 || x >= canvas.cols as f32 || yy >= canvas.rows as f32 || glow < 0.05 {
                    continue;
                }
                let (col, row) = (x as usize, yy as usize);
                if z + 2.0 / fit.scale < canvas.at(col, row).z {
                    continue;
                }
                out.push(Spark { col, row, hue, glow });
            }
        }
        out
    }
}
