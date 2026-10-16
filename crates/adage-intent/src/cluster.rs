//! Deterministic intent clustering, as spike S7 recommends, std only: exact cosine kNN, then
//! Louvain community detection on the kNN graph thresholded at `eps`, then a minimum family size. Everything that could vary
//! run to run is pinned: the kNN is exact with (similarity desc, index asc) order; per-row work is
//! independent of thread count; Louvain visits nodes in index order, scans candidate communities
//! in ascending id order, and moves only on a strictly better gain; aggregation numbers
//! communities by first appearance. Input order is canonicalised by the caller (sorted unique
//! normalised prompts), which makes the result invariant under input reordering.

use std::collections::HashMap;

/// Dot product with a fixed summation order (8 lanes, then lanes in order): deterministic and
/// symmetric, so sim(i, j) == sim(j, i) bit for bit.
#[inline]
pub fn dot(a: &[f32], b: &[f32]) -> f32 {
    let mut acc = [0f32; 8];
    let ((ca, ra), (cb, rb)) = (a.as_chunks::<8>(), b.as_chunks::<8>());
    for (x, y) in ca.iter().zip(cb) {
        for l in 0..8 {
            acc[l] += x[l] * y[l];
        }
    }
    let mut s = 0f32;
    for (x, y) in ra.iter().zip(rb) {
        s += x * y;
    }
    ((acc[0] + acc[1]) + (acc[2] + acc[3])) + ((acc[4] + acc[5]) + (acc[6] + acc[7])) + s
}

/// Exact top-`k` neighbours (self excluded) for every row of `emb` (n x d, row-major, unit
/// vectors). Returns row-major `(idx, sim)` of n x k, ordered by (similarity desc, index asc).
pub fn knn(emb: &[f32], n: usize, d: usize, k: usize, threads: usize) -> (Vec<u32>, Vec<f32>) {
    let k = k.min(n.saturating_sub(1));
    let mut idx = vec![0u32; n * k];
    let mut sim = vec![f32::NEG_INFINITY; n * k];
    if k == 0 {
        return (idx, sim);
    }
    let threads = threads.max(1);
    let chunk = n.div_ceil(threads).max(1);
    std::thread::scope(|s| {
        for (t, (ic, sc)) in idx.chunks_mut(chunk * k).zip(sim.chunks_mut(chunk * k)).enumerate() {
            s.spawn(move || {
                let start = t * chunk;
                let rows = ic.len() / k;
                // Four query rows per pass over the data: each candidate row is loaded once.
                let mut r = 0;
                while r < rows {
                    let m = (rows - r).min(4);
                    let q: Vec<&[f32]> = (0..m).map(|x| &emb[(start + r + x) * d..(start + r + x + 1) * d]).collect();
                    for j in 0..n {
                        let row = &emb[j * d..(j + 1) * d];
                        for (x, qx) in q.iter().enumerate() {
                            let i = start + r + x;
                            if i == j {
                                continue;
                            }
                            let v = dot(qx, row);
                            let base = (r + x) * k;
                            let (ii, ss) = (&mut ic[base..base + k], &mut sc[base..base + k]);
                            if v > ss[k - 1] {
                                // Insert after every entry with sim >= v (ties keep the lower index first).
                                let mut p = k - 1;
                                while p > 0 && ss[p - 1] < v {
                                    ss[p] = ss[p - 1];
                                    ii[p] = ii[p - 1];
                                    p -= 1;
                                }
                                ss[p] = v;
                                ii[p] = j as u32;
                            }
                        }
                    }
                    r += m;
                }
            });
        }
    });
    (idx, sim)
}

/// Undirected weighted graph from kNN edges with similarity >= eps; adjacency sorted by neighbour.
pub fn graph(idx: &[u32], sim: &[f32], n: usize, k: usize, eps: f32) -> Vec<Vec<(u32, f64)>> {
    let mut adj: Vec<Vec<(u32, f64)>> = vec![Vec::new(); n];
    for i in 0..n {
        for t in 0..k {
            let s = sim[i * k + t];
            if s < eps {
                break;
            }
            let j = idx[i * k + t] as usize;
            adj[i].push((j as u32, s as f64));
            adj[j].push((i as u32, s as f64));
        }
    }
    for a in adj.iter_mut() {
        a.sort_by(|x, y| x.0.cmp(&y.0).then(y.1.total_cmp(&x.1)));
        a.dedup_by_key(|e| e.0); // keeps the max weight (they are equal: sim is symmetric)
    }
    adj
}

/// Deterministic Louvain (modularity with a resolution parameter). Returns a community per node.
pub fn louvain(adj: Vec<Vec<(u32, f64)>>, resolution: f64, max_levels: usize) -> Vec<u32> {
    let n = adj.len();
    let mut members: Vec<Vec<u32>> = (0..n as u32).map(|i| vec![i]).collect();
    let mut g = adj;
    for _ in 0..max_levels {
        let nn = g.len();
        let deg: Vec<f64> = g.iter().map(|a| a.iter().map(|e| e.1).sum()).collect();
        let m2: f64 = deg.iter().sum();
        if m2 == 0.0 {
            break;
        }
        let mut comm: Vec<u32> = (0..nn as u32).collect();
        let mut tot = deg.clone();
        let mut moved_any = false;
        let mut links: Vec<f64> = vec![0.0; nn];
        let mut seen: Vec<bool> = vec![false; nn];
        let mut touched: Vec<u32> = Vec::new();
        loop {
            let mut improved = false;
            for i in 0..nn {
                let ci = comm[i];
                for &(j, w) in &g[i] {
                    if j as usize == i {
                        continue;
                    }
                    let c = comm[j as usize];
                    if !seen[c as usize] {
                        seen[c as usize] = true;
                        touched.push(c);
                    }
                    links[c as usize] += w;
                }
                tot[ci as usize] -= deg[i];
                let mut best_c = ci;
                let mut best_gain = links[ci as usize] - resolution * deg[i] * tot[ci as usize] / m2;
                touched.sort_unstable();
                for &c in &touched {
                    let gain = links[c as usize] - resolution * deg[i] * tot[c as usize] / m2;
                    if gain > best_gain + 1e-12 {
                        best_c = c;
                        best_gain = gain;
                    }
                }
                for &c in &touched {
                    links[c as usize] = 0.0;
                    seen[c as usize] = false;
                }
                touched.clear();
                tot[best_c as usize] += deg[i];
                if best_c != ci {
                    comm[i] = best_c;
                    improved = true;
                    moved_any = true;
                }
            }
            if !improved {
                break;
            }
        }
        if !moved_any {
            break;
        }
        // Aggregate: communities numbered by first appearance in node order.
        let mut ids: HashMap<u32, u32> = HashMap::new();
        for &c in &comm {
            let l = ids.len() as u32;
            ids.entry(c).or_insert(l);
        }
        let m = ids.len();
        let mut new_members: Vec<Vec<u32>> = vec![Vec::new(); m];
        for i in 0..nn {
            new_members[ids[&comm[i]] as usize].extend_from_slice(&members[i]);
        }
        let mut agg: Vec<HashMap<u32, f64>> = vec![HashMap::new(); m];
        for i in 0..nn {
            let a = ids[&comm[i]];
            for &(j, w) in &g[i] {
                *agg[a as usize].entry(ids[&comm[j as usize]]).or_insert(0.0) += w;
            }
        }
        g = agg
            .into_iter()
            .map(|h| {
                let mut v: Vec<(u32, f64)> = h.into_iter().collect();
                v.sort_by_key(|e| e.0);
                v
            })
            .collect();
        members = new_members;
    }
    let mut out = vec![0u32; n];
    for (c, mem) in members.iter().enumerate() {
        for &p in mem {
            out[p as usize] = c as u32;
        }
    }
    out
}

/// Clusters smaller than `min_size` become noise (-1); the rest are renumbered by their smallest
/// member index.
pub fn finish(labels: &[i64], min_size: usize) -> Vec<i64> {
    let mut count: HashMap<i64, usize> = HashMap::new();
    for &l in labels {
        if l >= 0 {
            *count.entry(l).or_default() += 1;
        }
    }
    let mut ids: HashMap<i64, i64> = HashMap::new();
    labels
        .iter()
        .map(|&l| {
            if l < 0 || count[&l] < min_size {
                -1
            } else {
                let x = ids.len() as i64;
                *ids.entry(l).or_insert(x)
            }
        })
        .collect()
}

/// Clustering parameters (S7: frozen on a development corpus, validated on held-out corpora).
#[derive(Debug, Clone)]
pub struct Params {
    /// Neighbours per prompt in the graph.
    pub k: usize,
    /// Minimum cosine similarity for a graph edge.
    pub eps: f32,
    /// Louvain resolution: higher gives smaller, purer families.
    pub resolution: f64,
    /// Families smaller than this are noise.
    pub min_size: usize,
}

impl Default for Params {
    fn default() -> Self {
        Params { k: 10, eps: 0.6, resolution: 2.0, min_size: 5 }
    }
}

/// Cluster unit vectors, given in the caller's canonical order, into families; -1 is noise.
pub fn families(vectors: &[Vec<f32>], p: &Params) -> Vec<i64> {
    let n = vectors.len();
    let Some(d) = vectors.first().map(|v| v.len()) else { return vec![] };
    if n < 2 || vectors.iter().any(|v| v.len() != d) {
        return vec![-1; n];
    }
    let flat: Vec<f32> = vectors.iter().flatten().copied().collect();
    let threads = std::thread::available_parallelism().map(|x| x.get()).unwrap_or(1).min(8);
    let (idx, sim) = knn(&flat, n, d, p.k, threads);
    let k = p.k.min(n - 1);
    let g = graph(&idx, &sim, n, k, p.eps);
    let comm: Vec<i64> = louvain(g, p.resolution, 10).into_iter().map(|c| c as i64).collect();
    finish(&comm, p.min_size)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Deterministic jittered points around a centre, as unit vectors.
    pub fn blob(center: &[f32], n: usize, jitter: f32, seed: u64) -> Vec<Vec<f32>> {
        let mut s = seed;
        (0..n)
            .map(|_| {
                let v: Vec<f32> = center
                    .iter()
                    .map(|c| {
                        s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                        c + jitter * (((s >> 33) as f32 / (1u64 << 31) as f32) - 0.5)
                    })
                    .collect();
                crate::unit(v)
            })
            .collect()
    }

    pub fn two_blobs() -> Vec<Vec<f32>> {
        let mut v = blob(&[1.0, 0.0, 0.0, 0.0], 20, 0.3, 1);
        v.extend(blob(&[0.0, 0.0, 1.0, 0.0], 20, 0.3, 2));
        v
    }

    #[test]
    fn finds_two_families_and_is_thread_count_independent() {
        let v = two_blobs();
        let l = families(&v, &Params::default());
        // Resolution 2 may cut a blob in two (behaviour merges them back); it never mixes blobs.
        assert!(l.iter().all(|&x| x >= 0));
        let (a, b): (std::collections::BTreeSet<i64>, std::collections::BTreeSet<i64>) =
            (l[..20].iter().copied().collect(), l[20..].iter().copied().collect());
        assert!(a.is_disjoint(&b), "{l:?}");
        let one = families(&v, &Params { resolution: 1.0, ..Default::default() });
        assert_eq!(one, [vec![0; 20], vec![1; 20]].concat(), "at resolution 1, one family per blob");
        let flat: Vec<f32> = v.iter().flatten().copied().collect();
        assert_eq!(knn(&flat, 40, 4, 10, 1), knn(&flat, 40, 4, 10, 7));
        assert_eq!(families(&v, &Params::default()), l, "rerun: same labels");
    }

    #[test]
    fn dot_is_symmetric_bit_for_bit() {
        let v = two_blobs();
        for a in &v {
            for b in &v {
                assert_eq!(dot(a, b).to_bits(), dot(b, a).to_bits());
            }
        }
    }

    #[test]
    fn degenerate_inputs() {
        assert!(families(&[], &Params::default()).is_empty());
        assert_eq!(families(&[vec![1.0, 0.0]], &Params::default()), vec![-1]);
        assert_eq!(families(&[vec![1.0, 0.0], vec![1.0]], &Params::default()), vec![-1, -1]);
    }
}
