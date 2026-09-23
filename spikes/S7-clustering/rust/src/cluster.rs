//! Deterministic intent clustering, std only: exact cosine kNN, then Louvain community detection
//! on the kNN graph thresholded at `eps`, then a minimum family size. Everything that could vary
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
    let (ca, cb) = (a.chunks_exact(8), b.chunks_exact(8));
    let (ra, rb) = (ca.remainder(), cb.remainder());
    for (x, y) in ca.zip(cb) {
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
                        for x in 0..m {
                            let i = start + r + x;
                            if i == j {
                                continue;
                            }
                            let v = dot(q[x], row);
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

/// kNN-DBSCAN: core if the `min_pts`-th neighbour has sim >= eps; clusters are connected
/// components of core points over kNN edges with sim >= eps; border points join their most
/// similar core neighbour.
pub fn knn_dbscan(idx: &[u32], sim: &[f32], n: usize, k: usize, eps: f32, min_pts: usize) -> Vec<i64> {
    let core: Vec<bool> = (0..n).map(|i| sim[i * k + min_pts - 1] >= eps).collect();
    let mut parent: Vec<usize> = (0..n).collect();
    fn find(p: &mut [usize], mut x: usize) -> usize {
        while p[x] != x {
            p[x] = p[p[x]];
            x = p[x];
        }
        x
    }
    for i in 0..n {
        if !core[i] {
            continue;
        }
        for t in 0..k {
            if sim[i * k + t] < eps {
                break;
            }
            let j = idx[i * k + t] as usize;
            if core[j] {
                let (a, b) = (find(&mut parent, i), find(&mut parent, j));
                if a != b {
                    let (lo, hi) = (a.min(b), a.max(b));
                    parent[hi] = lo;
                }
            }
        }
    }
    let mut labels = vec![-1i64; n];
    for i in 0..n {
        if core[i] {
            labels[i] = find(&mut parent, i) as i64;
        }
    }
    for i in 0..n {
        if core[i] {
            continue;
        }
        for t in 0..k {
            if sim[i * k + t] < eps {
                break;
            }
            let j = idx[i * k + t] as usize;
            if core[j] {
                labels[i] = labels[j];
                break;
            }
        }
    }
    labels
}
