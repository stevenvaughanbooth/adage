//! Behavioural purity (Design §8.2.1 step 5): *embeddings propose; behaviour decides.*
//!
//! Each prompt carries its session's route — what the session did next — or `None`. Families
//! whose sessions behave differently are **split**, but only where the text can tell the groups
//! apart (a split the runtime cannot reproduce from the prompt is no split); families whose
//! sessions behave the same and whose centroids are close are **merged**; a family is usable only
//! if its dominant route clears the purity **bar**. S7 measured the effect on held-out prompts
//! (10k test corpus): directive precision 0.57 with no purity step, 0.68 with the bar alone, 0.87
//! with split + merge + bar; recall 0.51, 0.41, 0.69.

use crate::cluster::{dot, finish};
use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub struct Purity {
    /// A family is usable only if at least this share of its sessions followed its dominant route.
    pub bar: f64,
    /// Split only if leave-one-out nearest-centroid accuracy between route groups reaches this.
    pub sep: f64,
    /// Merge families with the same dominant route whose centroids are at least this similar.
    pub merge_tau: f32,
    /// Route groups considered for a split need this many members and this share of the family.
    pub min_group: usize,
    pub min_share: f64,
}

impl Default for Purity {
    fn default() -> Self {
        Purity { bar: 0.6, sep: 0.85, merge_tau: 0.8, min_group: 5, min_share: 0.15 }
    }
}

/// Mean of the given rows as a unit vector; f64 summation in member order.
pub fn centroid(vectors: &[Vec<f32>], members: &[usize]) -> Vec<f32> {
    let d = vectors.first().map(|v| v.len()).unwrap_or(0);
    let mut acc = vec![0f64; d];
    for &i in members {
        for (a, x) in acc.iter_mut().zip(&vectors[i]) {
            *a += *x as f64;
        }
    }
    crate::unit(acc.into_iter().map(|x| x as f32).collect())
}

/// Members of each family, in index order.
pub fn members(labels: &[i64]) -> BTreeMap<i64, Vec<usize>> {
    let mut m: BTreeMap<i64, Vec<usize>> = BTreeMap::new();
    for (i, &l) in labels.iter().enumerate() {
        if l >= 0 {
            m.entry(l).or_default().push(i);
        }
    }
    m
}

/// Each family's dominant route (ties: the lexicographically smallest), its share of the family,
/// and the family's size. Sessions with no route count toward the size and never dominate.
pub fn dominant(labels: &[i64], routes: &[Option<String>]) -> BTreeMap<i64, (Option<String>, f64, usize)> {
    members(labels)
        .into_iter()
        .map(|(c, ix)| {
            let mut count: BTreeMap<&str, usize> = BTreeMap::new();
            for &i in &ix {
                if let Some(r) = &routes[i] {
                    *count.entry(r).or_default() += 1;
                }
            }
            let best = count.iter().max_by(|a, b| a.1.cmp(b.1).then_with(|| b.0.cmp(a.0)));
            let v = match best {
                Some((r, n)) => (Some(r.to_string()), *n as f64 / ix.len() as f64, ix.len()),
                None => (None, 0.0, ix.len()),
            };
            (c, v)
        })
        .collect()
}

fn nearest(v: &[f32], cents: &[Vec<f32>]) -> usize {
    let mut best = 0;
    let mut bs = f32::NEG_INFINITY;
    for (j, c) in cents.iter().enumerate() {
        let s = dot(v, c);
        if s > bs {
            best = j;
            bs = s;
        }
    }
    best
}

/// Leave-one-out nearest-centroid accuracy among route groups.
fn separability(vectors: &[Vec<f32>], groups: &[Vec<usize>]) -> f64 {
    let d = vectors[groups[0][0]].len();
    let sums: Vec<Vec<f64>> = groups
        .iter()
        .map(|g| {
            let mut s = vec![0f64; d];
            for &i in g {
                for (a, x) in s.iter_mut().zip(&vectors[i]) {
                    *a += *x as f64;
                }
            }
            s
        })
        .collect();
    let (mut correct, mut total) = (0usize, 0usize);
    for (gi, g) in groups.iter().enumerate() {
        for &i in g {
            let mut best = usize::MAX;
            let mut bs = f64::NEG_INFINITY;
            for (hj, s) in sums.iter().enumerate() {
                let (mut num, mut nn) = (0f64, 0f64);
                for (a, x) in s.iter().zip(&vectors[i]) {
                    let c = if hj == gi { a - *x as f64 } else { *a };
                    num += c * *x as f64;
                    nn += c * c;
                }
                if nn == 0.0 {
                    continue;
                }
                let sim = num / nn.sqrt();
                if sim > bs {
                    best = hj;
                    bs = sim;
                }
            }
            correct += (best == gi) as usize;
            total += 1;
        }
    }
    correct as f64 / total.max(1) as f64
}

/// Split families whose sessions followed different routes, where the text separates them.
pub fn split(vectors: &[Vec<f32>], labels: &[i64], routes: &[Option<String>], p: &Purity) -> Vec<i64> {
    let mut out = labels.to_vec();
    let mut next = labels.iter().copied().max().unwrap_or(-1) + 1;
    for (c, ix) in members(labels) {
        let mut groups: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
        for &i in &ix {
            if let Some(r) = &routes[i] {
                groups.entry(r).or_default().push(i);
            }
        }
        let min = (p.min_group as f64).max(p.min_share * ix.len() as f64);
        let groups: Vec<Vec<usize>> = groups.into_values().filter(|g| g.len() as f64 >= min).collect();
        if groups.len() < 2 || separability(vectors, &groups) < p.sep {
            continue;
        }
        let cents: Vec<Vec<f32>> = groups.iter().map(|g| centroid(vectors, g)).collect();
        let mut ids = vec![c];
        for _ in 1..groups.len() {
            ids.push(next);
            next += 1;
        }
        for &i in &ix {
            out[i] = ids[nearest(&vectors[i], &cents)];
        }
    }
    finish(&out, 1)
}

/// Merge families with the same dominant route and similar centroids, most similar pair first
/// (ties: the lowest ids).
pub fn merge(vectors: &[Vec<f32>], labels: &[i64], routes: &[Option<String>], p: &Purity) -> Vec<i64> {
    let mut out = labels.to_vec();
    loop {
        let dom = dominant(&out, routes);
        let m = members(&out);
        let ids: Vec<i64> = dom.iter().filter(|(_, v)| v.0.is_some()).map(|(c, _)| *c).collect();
        let cents: BTreeMap<i64, Vec<f32>> = ids.iter().map(|c| (*c, centroid(vectors, &m[c]))).collect();
        let mut best: Option<(f32, i64, i64)> = None;
        for (a, &x) in ids.iter().enumerate() {
            for &y in &ids[a + 1..] {
                if dom[&x].0 != dom[&y].0 {
                    continue;
                }
                let s = dot(&cents[&x], &cents[&y]);
                if s >= p.merge_tau && best.is_none_or(|b| s > b.0) {
                    best = Some((s, x, y));
                }
            }
        }
        let Some((_, x, y)) = best else { return finish(&out, 1) };
        for l in out.iter_mut() {
            if *l == y {
                *l = x;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cluster::tests::{blob, two_blobs};

    fn r(s: &str) -> Option<String> {
        Some(s.to_string())
    }

    #[test]
    fn separable_behaviour_splits_and_inseparable_does_not() {
        let v = two_blobs();
        let one = vec![0i64; 40];
        let routes: Vec<Option<String>> = (0..40).map(|i| r(if i < 20 { "a" } else { "b" })).collect();
        let s = split(&v, &one, &routes, &Purity::default());
        assert_eq!(&s[..20], &[0; 20], "split along the text boundary");
        assert_eq!(&s[20..], &[1; 20]);
        // Same text, behaviour decided by something the text cannot show: no split...
        let mixed: Vec<Option<String>> = (0..40).map(|i| r(if i % 2 == 0 { "a" } else { "b" })).collect();
        assert_eq!(split(&v, &one, &mixed, &Purity::default()), one);
        // ...and the purity bar rejects the family.
        assert!(dominant(&one, &mixed)[&0].1 < Purity::default().bar);
    }

    #[test]
    fn same_route_close_families_merge_and_others_do_not() {
        let mut v = blob(&[1.0, 0.2, 0.0, 0.0], 10, 0.1, 3);
        v.extend(blob(&[1.0, 0.0, 0.2, 0.0], 10, 0.1, 4));
        v.extend(blob(&[0.0, 0.0, 0.0, 1.0], 10, 0.1, 5));
        let labels: Vec<i64> = (0..30).map(|i| i / 10).collect();
        let routes: Vec<Option<String>> = (0..30).map(|i| r(if i < 20 { "a" } else { "b" })).collect();
        let m = merge(&v, &labels, &routes, &Purity::default());
        assert_eq!(&m[..20], &[0; 20]);
        assert_eq!(&m[20..], &[1; 10]);
        let all_a: Vec<Option<String>> = (0..30).map(|_| r("a")).collect();
        assert_eq!(merge(&v, &labels, &all_a, &Purity::default())[25], 1, "far families never merge");
    }

    #[test]
    fn no_route_never_dominates() {
        let labels = vec![0i64; 4];
        let routes = vec![None, None, None, r("a")];
        let d = dominant(&labels, &routes);
        assert_eq!(d[&0], (r("a"), 0.25, 4));
    }
}
