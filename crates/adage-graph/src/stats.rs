//! Posterior bounds (Track G §5): the regularised incomplete beta function, the Beta quantile by
//! bisection on it, and the Wilson lower bound kept for parity with today's learner.
//!
//! Pure `f64` arithmetic in a fixed order, so results are bit-for-bit reproducible.

/// ln Γ(x) for x > 0 (Lanczos, g = 7, n = 9; |error| < 1e-13 on the range used here).
pub fn ln_gamma(x: f64) -> f64 {
    const G: f64 = 7.0;
    const C: [f64; 9] = [
        0.999_999_999_999_809_9,
        676.520_368_121_885_1,
        -1_259.139_216_722_402_8,
        771.323_428_777_653_1,
        -176.615_029_162_140_6,
        12.507_343_278_686_905,
        -0.138_571_095_265_720_12,
        9.984_369_578_019_572e-6,
        1.505_632_735_149_311_6e-7,
    ];
    if x < 0.5 {
        // Reflection.
        let pi = std::f64::consts::PI;
        return (pi / (pi * x).sin()).ln() - ln_gamma(1.0 - x);
    }
    let x = x - 1.0;
    let mut a = C[0];
    let t = x + G + 0.5;
    for (i, c) in C.iter().enumerate().skip(1) {
        a += c / (x + i as f64);
    }
    0.5 * (2.0 * std::f64::consts::PI).ln() + (x + 0.5) * t.ln() - t + a.ln()
}

/// Continued fraction for the incomplete beta (modified Lentz).
fn beta_cf(a: f64, b: f64, x: f64) -> f64 {
    const TINY: f64 = 1e-300;
    const EPS: f64 = 1e-15;
    let (qab, qap, qam) = (a + b, a + 1.0, a - 1.0);
    let mut c = 1.0;
    let mut d = 1.0 - qab * x / qap;
    if d.abs() < TINY {
        d = TINY;
    }
    d = 1.0 / d;
    let mut h = d;
    for m in 1..=500 {
        let m = m as f64;
        let m2 = 2.0 * m;
        let aa = m * (b - m) * x / ((qam + m2) * (a + m2));
        d = 1.0 + aa * d;
        if d.abs() < TINY {
            d = TINY;
        }
        c = 1.0 + aa / c;
        if c.abs() < TINY {
            c = TINY;
        }
        d = 1.0 / d;
        h *= d * c;
        let aa = -(a + m) * (qab + m) * x / ((a + m2) * (qap + m2));
        d = 1.0 + aa * d;
        if d.abs() < TINY {
            d = TINY;
        }
        c = 1.0 + aa / c;
        if c.abs() < TINY {
            c = TINY;
        }
        d = 1.0 / d;
        let del = d * c;
        h *= del;
        if (del - 1.0).abs() < EPS {
            break;
        }
    }
    h
}

/// The regularised incomplete beta I_x(a, b), for a, b > 0 and x in [0, 1].
pub fn inc_beta(a: f64, b: f64, x: f64) -> f64 {
    inc_beta_with(ln_beta_front(a, b), a, b, x)
}

/// The part of [`inc_beta`]'s front factor that depends only on `a` and `b`, so a caller that
/// evaluates it at many `x` computes it once. Kept as the first terms of the same left-to-right
/// sum, so the result is bit-for-bit what [`inc_beta`] computed before.
fn ln_beta_front(a: f64, b: f64) -> f64 {
    ln_gamma(a + b) - ln_gamma(a) - ln_gamma(b)
}

fn inc_beta_with(ln_beta: f64, a: f64, b: f64, x: f64) -> f64 {
    if x <= 0.0 {
        return 0.0;
    }
    if x >= 1.0 {
        return 1.0;
    }
    let ln_front = ln_beta + a * x.ln() + b * (1.0 - x).ln();
    let front = ln_front.exp();
    if x < (a + 1.0) / (a + b + 2.0) {
        front * beta_cf(a, b, x) / a
    } else {
        1.0 - front * beta_cf(b, a, 1.0 - x) / b
    }
}

/// The `q` quantile of Beta(a, b), by bisection on [`inc_beta`] (100 halvings: exact to f64).
pub fn beta_quantile(q: f64, a: f64, b: f64) -> f64 {
    if !(a > 0.0 && b > 0.0) {
        return f64::NAN;
    }
    let q = q.clamp(0.0, 1.0);
    let ln_beta = ln_beta_front(a, b);
    let (mut lo, mut hi) = (0.0f64, 1.0f64);
    for _ in 0..100 {
        // Once the interval has collapsed, every further halving leaves it as it is.
        if lo == hi {
            break;
        }
        let mid = 0.5 * (lo + hi);
        if inc_beta_with(ln_beta, a, b, mid) < q {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    0.5 * (lo + hi)
}

/// The Beta posterior of `s` successes in `n` trials (both may be discounted weights) with a
/// prior of `kappa` pseudo-trials at mean `m`: (α, β).
pub fn posterior(s: f64, n: f64, m: f64, kappa: f64) -> (f64, f64) {
    let m = m.clamp(1e-6, 1.0 - 1e-6);
    (s + kappa * m, (n - s).max(0.0) + kappa * (1.0 - m))
}

/// z of the Wilson bound `adage_tracks::model::wilson_lower` uses (two-sided 95%).
pub const WILSON_Z: f64 = 1.959_964;

/// The Wilson score lower bound at z (the parity switch: [`WILSON_Z`] reproduces today's).
pub fn wilson_lower(s: f64, n: f64, z: f64) -> f64 {
    if n <= 0.0 {
        return 0.0;
    }
    let p = s / n;
    let z2 = z * z;
    let centre = p + z2 / (2.0 * n);
    let margin = z * (p * (1.0 - p) / n + z2 / (4.0 * n * n)).sqrt();
    ((centre - margin) / (1.0 + z2 / n)).max(0.0)
}

/// The Gamma posterior of a Poisson mean (tool calls to a goal event, outcome-optimisation §2.3)
/// from `n` observations summing to `sum`, with a *supplied* parent prior of `kappa`
/// pseudo-observations at mean `m`: (shape α, rate β), mean α / β.
pub fn gamma_poisson(sum: f64, n: f64, m: f64, kappa: f64) -> (f64, f64) {
    let kappa = kappa.max(1e-6);
    (sum.max(0.0) + kappa * m.max(1e-6), n.max(0.0) + kappa)
}

/// The mean and standard deviation of a Gamma(α, β) posterior mean.
pub fn gamma_moments((a, b): (f64, f64)) -> (f64, f64) {
    (a / b, a.sqrt() / b)
}

/// The mean and variance of Beta(a, b).
pub fn beta_moments(a: f64, b: f64) -> (f64, f64) {
    let n = a + b;
    (a / n, a * b / (n * n * (n + 1.0)))
}

/// The prior concentration of a Gamma–Poisson hierarchy fitted by the method of moments across
/// units (outcome-optimisation §2.3: κ is fitted, not fixed): each unit is (sum, n) of one child
/// (a repository, say). With the pooled mean m, the between-unit variance of the unit means
/// beyond their Poisson sampling variance is τ², and the prior Gamma(m²/τ², m/τ²) has κ = m/τ².
/// When the units vary no more than sampling explains, `max_kappa` (full pooling). `None` with
/// fewer than two units that have data.
pub fn fit_kappa(units: &[(f64, f64)], max_kappa: f64) -> Option<f64> {
    let units: Vec<(f64, f64)> = units.iter().copied().filter(|&(_, n)| n > 0.0).collect();
    if units.len() < 2 {
        return None;
    }
    let (s, n): (f64, f64) = units.iter().fold((0.0, 0.0), |(a, b), &(x, y)| (a + x, b + y));
    let m = s / n;
    let k = units.len() as f64;
    let means: Vec<f64> = units.iter().map(|&(x, y)| x / y).collect();
    let between = means.iter().map(|x| (x - m) * (x - m)).sum::<f64>() / (k - 1.0);
    let sampling = units.iter().map(|&(_, y)| m / y).sum::<f64>() / k;
    let tau2 = between - sampling;
    if tau2 <= 0.0 || m <= 0.0 {
        return Some(max_kappa);
    }
    Some((m / tau2).clamp(1e-3, max_kappa))
}

/// The standard normal distribution function (Abramowitz and Stegun 26.2.17; |error| < 7.5e-8).
pub fn normal_cdf(x: f64) -> f64 {
    let t = 1.0 / (1.0 + 0.231_641_9 * x.abs());
    let poly =
        t * (0.319_381_530 + t * (-0.356_563_782 + t * (1.781_477_937 + t * (-1.821_255_978 + t * 1.330_274_429))));
    let tail = (-0.5 * x * x).exp() / (2.0 * std::f64::consts::PI).sqrt() * poly;
    if x >= 0.0 {
        1.0 - tail
    } else {
        tail
    }
}

/// The value of a directive (outcome-optimisation §2.4), in tool calls:
/// `A = p_follow · (Q(∅) − Q(Sat d)) − C(d) − p_misfire · m`, with `Q(∅)` and `Q(Sat d)` Gamma
/// posteriors of calls to the goal, `p_follow` and `p_misfire` Beta posteriors, `cost` the
/// directive's own cost and `misfire_cost` what a misfire costs. The posterior of A by moments
/// (the factors independent), read as normal.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Advantage {
    pub mean: f64,
    pub sd: f64,
    /// P(A > 0).
    pub p_positive: f64,
    /// The one-sided 90% lower bound.
    pub lower: f64,
    /// Q(∅) and Q(Sat d), posterior means.
    pub q_null: f64,
    pub q_sat: f64,
}

/// z of the one-sided 90% bound an advantage is judged by.
pub const ADVANTAGE_Z: f64 = 1.281_551_6;

pub fn advantage(
    q_null: (f64, f64),
    q_sat: (f64, f64),
    follow: (f64, f64),
    misfire: (f64, f64),
    cost: f64,
    misfire_cost: f64,
) -> Advantage {
    let (m0, s0) = gamma_moments(q_null);
    let (m1, s1) = gamma_moments(q_sat);
    let (pf, vf) = beta_moments(follow.0, follow.1);
    let (pm, vm) = beta_moments(misfire.0, misfire.1);
    let d = m0 - m1;
    let vd = s0 * s0 + s1 * s1;
    let mean = pf * d - cost - pm * misfire_cost;
    let var = pf * pf * vd + vf * d * d + vf * vd + misfire_cost * misfire_cost * vm;
    let sd = var.max(0.0).sqrt();
    let p_positive = if sd > 0.0 {
        normal_cdf(mean / sd)
    } else if mean > 0.0 {
        1.0
    } else {
        0.0
    };
    Advantage { mean, sd, p_positive, lower: mean - ADVANTAGE_Z * sd, q_null: m0, q_sat: m1 }
}

/// [`advantage`] where `Q(∅)` is a mixture: with no directive, a share `p_sat` of the agents
/// already take an action in `Sat(d)` (cost `Q(Sat d)`) and the rest do something else (cost
/// `Q(other)`), so `Q(∅) − Q(Sat d) = (1 − p_sat) · (Q(other) − Q(Sat d))`: a directive for what
/// agents do anyway is worth nothing (Track G's `1 − p_soon`, derived). `sat_share` is p_sat's Beta
/// posterior.
pub fn advantage_mix(
    q_other: (f64, f64),
    q_sat: (f64, f64),
    sat_share: (f64, f64),
    follow: (f64, f64),
    misfire: (f64, f64),
    cost: f64,
    misfire_cost: f64,
) -> Advantage {
    let (m0, s0) = gamma_moments(q_other);
    let (m1, s1) = gamma_moments(q_sat);
    let (pf, vf) = beta_moments(follow.0, follow.1);
    let (ps, vs) = beta_moments(sat_share.0, sat_share.1);
    let (pm, vm) = beta_moments(misfire.0, misfire.1);
    // f = p_follow · (1 − p_sat), independent factors.
    let (q, vq) = (1.0 - ps, vs);
    let f = pf * q;
    let vfq = (vf + pf * pf) * (vq + q * q) - f * f;
    let d = m0 - m1;
    let vd = s0 * s0 + s1 * s1;
    let mean = f * d - cost - pm * misfire_cost;
    let var = f * f * vd + vfq * d * d + vfq * vd + misfire_cost * misfire_cost * vm;
    let sd = var.max(0.0).sqrt();
    let p_positive = if sd > 0.0 {
        normal_cdf(mean / sd)
    } else if mean > 0.0 {
        1.0
    } else {
        0.0
    };
    Advantage { mean, sd, p_positive, lower: mean - ADVANTAGE_Z * sd, q_null: ps * m1 + q * m0, q_sat: m1 }
}

/// Median of a list (the lower middle for an even count, so it is an observed value).
pub fn median(v: &mut [f64]) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(|a, b| a.total_cmp(b));
    v[(v.len() - 1) / 2]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol
    }

    #[test]
    fn ln_gamma_matches_factorials() {
        let mut f = 1.0f64;
        for n in 1..20 {
            assert!(close(ln_gamma(n as f64), f.ln(), 1e-10), "ln Γ({n})");
            f *= n as f64;
        }
        assert!(close(ln_gamma(0.5), std::f64::consts::PI.sqrt().ln(), 1e-12));
    }

    #[test]
    fn incomplete_beta_known_values() {
        // Closed forms: I_x(1, 1) = x; I_x(a, 1) = x^a; I_x(1, b) = 1 − (1 − x)^b.
        for &x in &[0.1, 0.37, 0.5, 0.9] {
            assert!(close(inc_beta(1.0, 1.0, x), x, 1e-12));
            assert!(close(inc_beta(3.0, 1.0, x), x.powi(3), 1e-12));
            assert!(close(inc_beta(1.0, 4.0, x), 1.0 - (1.0 - x).powi(4), 1e-12));
        }
        // I_0.5(a, a) = 0.5. For integers, I_x(a, b) = P(Binomial(a + b − 1, x) ≥ a):
        // I_0.4(2, 3) = 0.5248 and I_0.3(10, 20) = 0.364004081071943.
        assert!(close(inc_beta(7.5, 7.5, 0.5), 0.5, 1e-12));
        assert!(close(inc_beta(2.0, 3.0, 0.4), 0.5248, 1e-12));
        assert!(close(inc_beta(10.0, 20.0, 0.3), 0.364_004_081_071_943_5, 1e-12));
    }

    /// The quantile as it was computed before `ln_beta_front` was hoisted and the bisection
    /// stopped at a collapsed interval: the scores, and so the learned tracks, depend on it.
    fn beta_quantile_reference(q: f64, a: f64, b: f64) -> f64 {
        if !(a > 0.0 && b > 0.0) {
            return f64::NAN;
        }
        let q = q.clamp(0.0, 1.0);
        let (mut lo, mut hi) = (0.0f64, 1.0f64);
        for _ in 0..100 {
            let mid = 0.5 * (lo + hi);
            let ln_front = ln_gamma(a + b) - ln_gamma(a) - ln_gamma(b) + a * mid.ln() + b * (1.0 - mid).ln();
            let front = ln_front.exp();
            let i = if mid <= 0.0 {
                0.0
            } else if mid >= 1.0 {
                1.0
            } else if mid < (a + 1.0) / (a + b + 2.0) {
                front * beta_cf(a, b, mid) / a
            } else {
                1.0 - front * beta_cf(b, a, 1.0 - mid) / b
            };
            if i < q {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        0.5 * (lo + hi)
    }

    #[test]
    fn beta_quantile_is_bit_for_bit_the_reference() {
        let mut n = 0;
        for &q in &[0.0, 1e-9, 0.01, 0.05, 0.1, 0.5, 0.9, 0.95, 0.999, 1.0] {
            for &a in &[0.05, 0.5, 1.0, 1.7, 3.0, 10.0, 41.5, 400.0, 5000.0] {
                for &b in &[0.05, 0.5, 1.0, 2.25, 7.0, 33.0, 250.0, 9000.0] {
                    let (x, y) = (beta_quantile(q, a, b), beta_quantile_reference(q, a, b));
                    assert_eq!(x.to_bits(), y.to_bits(), "q={q} a={a} b={b}: {x} vs {y}");
                    n += 1;
                }
            }
        }
        assert_eq!(n, 720);
    }

    #[test]
    fn beta_quantiles_known_values() {
        // Beta(1, 1): uniform.
        assert!(close(beta_quantile(0.05, 1.0, 1.0), 0.05, 1e-12));
        // Beta(a, 1): q^(1/a).
        assert!(close(beta_quantile(0.05, 4.0, 1.0), 0.05f64.powf(0.25), 1e-12));
        // From the binomial identity (scipy.stats.beta.ppf agrees): ppf(0.05, 10, 2) =
        // 0.635640510752757, ppf(0.05, 20, 5) = 0.658192650595884.
        assert!(close(beta_quantile(0.05, 10.0, 2.0), 0.635_640_510_752_757_4, 1e-12));
        assert!(close(beta_quantile(0.05, 20.0, 5.0), 0.658_192_650_595_883_7, 1e-12));
        // Symmetric: the median of Beta(a, a) is 1/2.
        assert!(close(beta_quantile(0.5, 3.3, 3.3), 0.5, 1e-12));
        // It inverts inc_beta.
        let x = beta_quantile(0.05, 8.2, 3.1);
        assert!(close(inc_beta(8.2, 3.1, x), 0.05, 1e-12));
    }

    #[test]
    fn wilson_parity_values() {
        // adage_tracks::model::wilson_lower(50, 50) = 0.9287; 9 of 10 at z = 1.645: 0.65225.
        assert!(close(wilson_lower(50.0, 50.0, WILSON_Z), 0.9287, 1e-3));
        assert!(close(wilson_lower(9.0, 10.0, 1.645), 0.652_254_890_790_421_8, 1e-12));
        assert_eq!(wilson_lower(0.0, 0.0, 1.645), 0.0);
        // More evidence, same rate: a higher bound, as with the Beta bound.
        assert!(wilson_lower(90.0, 100.0, 1.645) > wilson_lower(9.0, 10.0, 1.645));
        let (a, b) = posterior(9.0, 10.0, 0.5, 2.0);
        let (a2, b2) = posterior(90.0, 100.0, 0.5, 2.0);
        assert!(beta_quantile(0.05, a2, b2) > beta_quantile(0.05, a, b));
    }
}
