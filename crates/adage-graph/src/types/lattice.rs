//! The lattice per run, and the MDL choice along it (Track G §4.2).
//!
//! literal → literal set (≤ 4) → class with exact length → class `{lo,hi}` → wider class.
//!
//! Costs, in bits (`L(G) + Σ_v L(v|G)` over the distinct sample values):
//!
//! | Run | `L(G)` | `L(v|G)` per value |
//! | --- | --- | --- |
//! | literal | 8 per char | 0 |
//! | literal set | 8 per char of every member | log2 |set| |
//! | class `{lo,hi}` | 4 + length code | len · log2 |class| + log2 (hi − lo + 1) |
//! | star `(sep c{lo,hi}){min,max}` | 8 + 4 + length code + repeat code | log2 (max − min + 1) + Σ repeats (len · log2 |c| + log2 (hi − lo + 1)) |
//!
//! The length code is `2 + 2 · log2 (hi − lo + 1)` bits (2 for an exact length). So a constant
//! prefix `BIL` costs 24 bits once against about 14 bits per value as `U{3}`, and is kept as a
//! literal from two values upward; three 4-digit values cost about 101 bits as a set and 46 as
//! `D{4}`. The length range is exactly the observed `[lo, hi]`, never widened.
//!
//! A wider class than the least general one never lowers the cost, so the MDL choice is always
//! the least general class; wider classes arise only from joins (`U ∪ D` → `W`).

use super::skeleton::{charset, min_class};
use adage_core::vtype::Run;
use std::collections::BTreeSet;

/// How to choose along the lattice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// The MDL choice.
    Mdl,
    /// The least general run the samples allow: a literal or literal set whenever there are at
    /// most 4 distinct strings. Used when the MDL choice accepts a negative.
    LeastGeneral,
}

pub const MAX_SET: usize = 4;

pub fn len_code(lo: usize, hi: usize) -> f64 {
    2.0 + 2.0 * ((hi - lo + 1) as f64).log2()
}

/// The cost of a class run over these strings (all alphanumeric), and the run.
pub fn class_run(strings: &[&str]) -> (Run, f64) {
    let mask = strings.iter().fold(0u64, |m, s| m | charset(s));
    let class = min_class(mask);
    let lo = strings.iter().map(|s| s.len()).min().unwrap_or(1);
    let hi = strings.iter().map(|s| s.len()).max().unwrap_or(1);
    let per_char = (class.size() as f64).log2();
    let range = ((hi - lo + 1) as f64).log2();
    let data: f64 = strings.iter().map(|s| s.len() as f64 * per_char + range).sum();
    let run = Run::Class { class, lo: lo.min(255) as u8, hi: hi.min(255) as u8 };
    (run, 4.0 + len_code(lo, hi) + data)
}

/// Choose the run for one alphanumeric position. `strings` holds the string at this position of
/// every distinct sample value (so repeats are counted once per value).
pub fn choose(strings: &[&str], mode: Mode) -> (Run, f64) {
    let distinct: BTreeSet<&str> = strings.iter().copied().collect();
    let n = strings.len() as f64;
    let mut best: Option<(Run, f64)> = None;
    if distinct.len() == 1 {
        let t = distinct.iter().next().copied().unwrap_or_default();
        best = Some((Run::Lit { text: t.to_string() }, 8.0 * t.len() as f64));
    } else if distinct.len() <= MAX_SET {
        let model: f64 = distinct.iter().map(|t| 8.0 * t.len() as f64).sum();
        let texts: Vec<String> = distinct.iter().map(|t| t.to_string()).collect();
        best = Some((Run::Set { texts }, model + n * (distinct.len() as f64).log2()));
    }
    if mode == Mode::LeastGeneral {
        if let Some(b) = best {
            return b;
        }
    }
    let class = class_run(strings);
    match best {
        // Ties keep the less general run.
        Some(b) if b.1 <= class.1 => b,
        _ => class,
    }
}

/// A star `(sep class{lo,hi}){min,max}` over the repeated runs of every value (`reps[v]` holds
/// value v's repeated strings).
pub fn star(sep: char, reps: &[Vec<&str>]) -> Option<(Run, f64)> {
    let all: Vec<&str> = reps.iter().flatten().copied().collect();
    if all.is_empty() {
        return None;
    }
    let (Run::Class { class, lo, hi }, _) = class_run(&all) else { return None };
    let min = reps.iter().map(Vec::len).min().unwrap_or(0);
    let max = reps.iter().map(Vec::len).max().unwrap_or(0);
    if max > 255 {
        return None;
    }
    let per_char = (class.size() as f64).log2();
    let range = ((hi - lo + 1) as f64).log2();
    let reps_code = ((max - min + 1) as f64).log2();
    let data: f64 =
        reps.iter().map(|r| reps_code + r.iter().map(|s| s.len() as f64 * per_char + range).sum::<f64>()).sum();
    let model = 8.0 + 4.0 + len_code(lo as usize, hi as usize) + len_code(min, max);
    Some((Run::Star { sep, class, lo, hi, min: min as u8, max: max as u8 }, model + data))
}

#[cfg(test)]
mod tests {
    use super::*;
    use adage_core::vtype::Class;

    #[test]
    fn constant_prefix_is_kept_from_two_values() {
        let (r, _) = choose(&["BIL", "BIL"], Mode::Mdl);
        assert_eq!(r, Run::Lit { text: "BIL".into() });
        // One value: the literal (24 bits) costs more than U{3} (4 + 2 + 14.1).
        let (r, _) = choose(&["BIL"], Mode::Mdl);
        assert_eq!(r, Run::Class { class: Class::U, lo: 3, hi: 3 });
    }

    #[test]
    fn three_four_digit_values_are_a_class() {
        let (r, bits) = choose(&["1041", "1046", "2042"], Mode::Mdl);
        assert_eq!(r, Run::Class { class: Class::D, lo: 4, hi: 4 });
        assert!((bits - 45.86).abs() < 0.1, "{bits}");
        let set: f64 = 3.0 * 4.0 * 8.0 + 3.0 * 3f64.log2();
        assert!(set > 97.0 && set > 2.0 * bits);
        // Least general keeps the set.
        let (r, _) = choose(&["1041", "1046", "2042"], Mode::LeastGeneral);
        assert!(matches!(r, Run::Set { .. }));
    }

    #[test]
    fn lengths_are_exact() {
        let (r, _) = choose(&["104", "10466", "2042"], Mode::Mdl);
        assert_eq!(r, Run::Class { class: Class::D, lo: 3, hi: 5 });
    }
}
