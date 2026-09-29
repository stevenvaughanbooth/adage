//! Anti-unification of sample values into run skeletons (Track G §4.2).
//!
//! 1. Values with one punctuation skeleton unify **pointwise**: each alphanumeric position takes
//!    the MDL choice along the lattice ([`super::lattice`]); punctuation stays literal.
//! 2. Values with different skeletons first try **star folding**: every skeleton is the shortest
//!    one with a repeated group `(sep alnum)` inserted at one fixed place, zero or more times
//!    (`fix/login-42` and `fix/cache-retry-7` become `fix '/' L ('-' L){0,1} '-' D`). Every
//!    valid place is tried and the cheapest (MDL) kept.
//! 3. Otherwise the type is a **union** of the pointwise skeletons, when there are at most 3.

use super::lattice::{choose, star, Mode};
use super::skeleton::{items, signature, Item, Sig};
use adage_core::vtype::{Run, Runs, MAX_BRANCHES};
use std::collections::BTreeMap;

/// A generalisation of some values, with its description length in bits.
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub runs: Runs,
    pub bits: f64,
}

type Parsed<'a> = (Vec<Sig>, Vec<Item<'a>>);

fn pointwise(vals: &[&Parsed<'_>], mode: Mode) -> Option<(Vec<Run>, f64)> {
    let sig = &vals.first()?.0;
    let mut runs = Vec::with_capacity(sig.len());
    let mut bits = 0.0;
    for (j, s) in sig.iter().enumerate() {
        match s {
            Sig::Punct(c) => {
                runs.push(Run::Lit { text: c.to_string() });
                bits += 8.0;
            }
            Sig::Alnum => {
                let strings: Vec<&str> = vals
                    .iter()
                    .map(|(_, it)| match it[j] {
                        Item::Alnum(s) => s,
                        Item::Punct(_) => "",
                    })
                    .collect();
                let (r, b) = choose(&strings, mode);
                runs.push(r);
                bits += b;
            }
        }
    }
    Some((runs, bits))
}

/// Places `(p, sep)` where `b` is `a[..p] ++ (sep alnum)^k ++ a[p..]` for some k ≥ 1.
fn insertions(a: &[Sig], b: &[Sig]) -> Vec<(usize, char)> {
    if b.len() <= a.len() || !(b.len() - a.len()).is_multiple_of(2) {
        return Vec::new();
    }
    let k = (b.len() - a.len()) / 2;
    let mut out = Vec::new();
    for p in 0..=a.len() {
        // The group must sit between an alphanumeric run (or the start) and punctuation (or the
        // end), so runs keep alternating with punctuation.
        if (p > 0 && a[p - 1] != Sig::Alnum) || (p < a.len() && a[p] == Sig::Alnum) {
            continue;
        }
        let Sig::Punct(sep) = b[p] else { continue };
        let group_ok = (0..k).all(|r| b[p + 2 * r] == Sig::Punct(sep) && b[p + 2 * r + 1] == Sig::Alnum);
        if group_ok && b[..p] == a[..p] && b[p + 2 * k..] == a[p..] {
            out.push((p, sep));
        }
    }
    out
}

fn alnum<'a>(i: &Item<'a>) -> &'a str {
    match i {
        Item::Alnum(s) => s,
        Item::Punct(_) => "",
    }
}

fn fold(parsed: &[Parsed<'_>], groups: &BTreeMap<Vec<Sig>, Vec<usize>>, mode: Mode) -> Option<(Vec<Run>, f64)> {
    let base = groups.keys().min_by(|x, y| x.len().cmp(&y.len()).then_with(|| x.cmp(y)))?;
    let mut places: Option<Vec<(usize, char)>> = None;
    for sig in groups.keys().filter(|s| *s != base) {
        let here = insertions(base, sig);
        places = Some(match places {
            None => here,
            Some(prev) => prev.into_iter().filter(|x| here.contains(x)).collect(),
        });
    }
    let mut best: Option<(Vec<Run>, f64)> = None;
    for (p, sep) in places.unwrap_or_default() {
        let n = parsed.len();
        let mut prefix: Vec<Vec<&str>> = vec![Vec::with_capacity(n); p];
        let mut suffix: Vec<Vec<&str>> = vec![Vec::with_capacity(n); base.len() - p];
        let mut reps: Vec<Vec<&str>> = Vec::with_capacity(n);
        for (sig, it) in parsed {
            let k = (sig.len() - base.len()) / 2;
            for (j, slot) in prefix.iter_mut().enumerate() {
                slot.push(alnum(&it[j]));
            }
            reps.push((0..k).map(|r| alnum(&it[p + 2 * r + 1])).collect());
            for (j, slot) in suffix.iter_mut().enumerate() {
                slot.push(alnum(&it[p + 2 * k + j]));
            }
        }
        let mut runs = Vec::new();
        let mut bits = 0.0;
        for (j, strings) in prefix.iter().enumerate() {
            match base[j] {
                Sig::Punct(c) => {
                    runs.push(Run::Lit { text: c.to_string() });
                    bits += 8.0;
                }
                Sig::Alnum => {
                    let (r, b) = choose(strings, mode);
                    runs.push(r);
                    bits += b;
                }
            }
        }
        let Some((s, b)) = star(sep, &reps) else { continue };
        runs.push(s);
        bits += b;
        for (j, strings) in suffix.iter().enumerate() {
            match base[p + j] {
                Sig::Punct(c) => {
                    runs.push(Run::Lit { text: c.to_string() });
                    bits += 8.0;
                }
                Sig::Alnum => {
                    let (r, b) = choose(strings, mode);
                    runs.push(r);
                    bits += b;
                }
            }
        }
        if best.as_ref().is_none_or(|(_, bb)| bits < *bb) {
            best = Some((runs, bits));
        }
    }
    best
}

/// Generalise distinct, sorted sample values. `None` when they have more than
/// [`MAX_BRANCHES`] skeletons that do not fold.
pub fn generalise(values: &[String], mode: Mode) -> Option<Candidate> {
    let parsed: Vec<Parsed<'_>> = values
        .iter()
        .map(|v| {
            let it = items(v);
            (signature(&it), it)
        })
        .collect();
    let mut groups: BTreeMap<Vec<Sig>, Vec<usize>> = BTreeMap::new();
    for (i, (sig, _)) in parsed.iter().enumerate() {
        groups.entry(sig.clone()).or_default().push(i);
    }
    if groups.len() == 1 {
        let all: Vec<&Parsed<'_>> = parsed.iter().collect();
        let (b, bits) = pointwise(&all, mode)?;
        return Some(Candidate { runs: Runs { branches: vec![b] }, bits });
    }
    let folded = fold(&parsed, &groups, mode).map(|(b, bits)| Candidate { runs: Runs { branches: vec![b] }, bits });
    let union = (groups.len() <= MAX_BRANCHES).then(|| union(&parsed, &groups, mode, values.len())).flatten();
    // Both apply: the cheaper one (a tie keeps the fold).
    match (folded, union) {
        (Some(f), Some(u)) => Some(if u.bits < f.bits { u } else { f }),
        (f, u) => f.or(u),
    }
}

fn union(parsed: &[Parsed<'_>], groups: &BTreeMap<Vec<Sig>, Vec<usize>>, mode: Mode, n: usize) -> Option<Candidate> {
    let mut branches = Vec::new();
    let mut bits = n as f64 * (groups.len() as f64).log2();
    for idx in groups.values() {
        let vals: Vec<&Parsed<'_>> = idx.iter().map(|&i| &parsed[i]).collect();
        let (b, c) = pointwise(&vals, mode)?;
        branches.push(b);
        bits += c;
    }
    branches.sort();
    Some(Candidate { runs: Runs { branches }, bits })
}

#[cfg(test)]
mod tests {
    use super::*;
    use adage_core::vtype::to_regex;

    fn gen(vs: &[&str]) -> String {
        let mut v: Vec<String> = vs.iter().map(|s| s.to_string()).collect();
        v.sort();
        to_regex(&generalise(&v, Mode::Mdl).unwrap().runs)
    }

    #[test]
    fn pointwise_prefix_dash() {
        assert_eq!(gen(&["BIL-1041", "BIL-1046", "BIL-2042"]), r"^(?:BIL\-[0-9]{4})$");
        assert_eq!(gen(&["BIL-104", "BIL-1046", "BIL-2042"]), r"^(?:BIL\-[0-9]{3,4})$");
    }

    #[test]
    fn star_folding() {
        assert_eq!(
            gen(&["fix/login-42", "fix/cache-retry-7", "fix/audit-913"]),
            r"^(?:fix/[a-z]{5}(?:\-[a-z]{5}){0,1}\-[0-9]{1,3})$"
        );
        // Few values: literal sets are cheaper than classes, and the extra group is a star.
        assert_eq!(gen(&["a/b-c", "a/b-c-d", "a/bb-cc-d"]), r"^(?:a/(?:b|bb)\-(?:c|cc)(?:\-[a-z]{1}){0,1})$");
    }

    #[test]
    fn union_of_two_trackers() {
        assert_eq!(gen(&["BIL-1041", "BIL-2046", "482", "517", "BIL-311"]), r"^(?:BIL\-[0-9]{3,4}|[0-9]{3})$");
        // Four unrelated skeletons: no type.
        let v: Vec<String> = ["a", "a-b", "a.b.c", "a@b"].iter().map(|s| s.to_string()).collect();
        assert!(generalise(&v, Mode::Mdl).is_none());
    }
}
