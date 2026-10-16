//! s7c — cluster an embedding file and write labels.
//!
//! ```text
//! s7c <emb.bin> <labels.bin> --method louvain|knn_dbscan|hdbscan_crate
//!     [--k 10] [--eps 0.6] [--res 2.0] [--min-size 10] [--min-pts 5] [--mcs 20]
//!     [--threads N] [--groups <groups.bin>] [--canonical]
//! ```
//! `emb.bin`: u32 n, u32 d, then n*d little-endian f32 (unit rows). `groups.bin`: n u32 group ids
//! (e.g. repository) — each group is clustered on its own. `--canonical` sorts rows by their
//! vector bytes before clustering (what the learner does with sorted unique prompts), making the
//! output invariant to input order. Labels: n little-endian i32, -1 = noise. Prints timing JSON.

mod cluster;

use std::time::Instant;

fn arg<T: std::str::FromStr>(args: &[String], name: &str, default: T) -> T {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).and_then(|v| v.parse().ok()).unwrap_or(default)
}

fn read_u32s(b: &[u8]) -> Vec<u32> {
    b.chunks_exact(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect()
}

struct Params {
    method: String,
    k: usize,
    eps: f32,
    res: f64,
    min_size: usize,
    min_pts: usize,
    mcs: usize,
    threads: usize,
}

#[derive(Default)]
struct Timing {
    knn_ms: f64,
    cluster_ms: f64,
}

fn cluster_one(emb: &[f32], n: usize, d: usize, p: &Params, t: &mut Timing) -> Vec<i64> {
    if n == 0 {
        return vec![];
    }
    if p.method == "hdbscan_crate" {
        let t0 = Instant::now();
        let data: Vec<Vec<f32>> = (0..n).map(|i| emb[i * d..(i + 1) * d].to_vec()).collect();
        let hp = hdbscan::HdbscanHyperParams::builder().min_cluster_size(p.mcs).min_samples(p.min_pts).build();
        let labels = hdbscan::Hdbscan::new(&data, hp).cluster().map(|v| v.into_iter().map(|x| x as i64).collect()).unwrap_or_else(|_| vec![-1; n]);
        t.cluster_ms += t0.elapsed().as_secs_f64() * 1e3;
        return cluster::finish(&labels, 1);
    }
    let t0 = Instant::now();
    let kk = p.k.max(p.min_pts);
    let (idx, sim) = cluster::knn(emb, n, d, kk, p.threads);
    let kk = kk.min(n.saturating_sub(1));
    t.knn_ms += t0.elapsed().as_secs_f64() * 1e3;
    let t1 = Instant::now();
    let labels: Vec<i64> = match p.method.as_str() {
        "louvain" => {
            // Graph uses the first k neighbours only.
            let k = p.k.min(kk);
            let mut i2 = Vec::with_capacity(n * k);
            let mut s2 = Vec::with_capacity(n * k);
            for r in 0..n {
                i2.extend_from_slice(&idx[r * kk..r * kk + k]);
                s2.extend_from_slice(&sim[r * kk..r * kk + k]);
            }
            let g = cluster::graph(&i2, &s2, n, k, p.eps);
            cluster::louvain(g, p.res, 10).into_iter().map(|c| c as i64).collect()
        }
        "knn_dbscan" => cluster::knn_dbscan(&idx, &sim, n, kk, p.eps, p.min_pts),
        m => panic!("unknown method {m}"),
    };
    let out = cluster::finish(&labels, p.min_size);
    t.cluster_ms += t1.elapsed().as_secs_f64() * 1e3;
    out
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let t_all = Instant::now();
    let raw = std::fs::read(&args[1]).expect("read emb");
    let n = u32::from_le_bytes(raw[0..4].try_into().unwrap()) as usize;
    let d = u32::from_le_bytes(raw[4..8].try_into().unwrap()) as usize;
    let emb: Vec<f32> = raw[8..].chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
    drop(raw);
    assert_eq!(emb.len(), n * d);
    let p = Params {
        method: arg(&args, "--method", "louvain".to_string()),
        k: arg(&args, "--k", 10),
        eps: arg(&args, "--eps", 0.6),
        res: arg(&args, "--res", 2.0),
        min_size: arg(&args, "--min-size", 10),
        min_pts: arg(&args, "--min-pts", 5),
        mcs: arg(&args, "--mcs", 20),
        threads: arg(&args, "--threads", std::thread::available_parallelism().map(|x| x.get()).unwrap_or(4)),
    };
    let groups: Vec<u32> = match args.iter().position(|a| a == "--groups") {
        Some(i) => read_u32s(&std::fs::read(&args[i + 1]).expect("read groups")),
        None => vec![0; n],
    };
    let canonical = args.iter().any(|a| a == "--canonical");

    // Rows per group, in input order or canonical (vector-bytes) order.
    let mut by_group: std::collections::BTreeMap<u32, Vec<usize>> = Default::default();
    for (i, &g) in groups.iter().enumerate() {
        by_group.entry(g).or_default().push(i);
    }
    let mut t = Timing::default();
    let mut labels = vec![-1i64; n];
    let mut next = 0i64;
    for (_, mut rows) in by_group {
        if canonical {
            rows.sort_by(|&a, &b| {
                let (x, y) = (&emb[a * d..(a + 1) * d], &emb[b * d..(b + 1) * d]);
                x.iter().map(|v| v.to_bits()).cmp(y.iter().map(|v| v.to_bits())).then(a.cmp(&b))
            });
        }
        let mut sub = Vec::with_capacity(rows.len() * d);
        for &r in &rows {
            sub.extend_from_slice(&emb[r * d..(r + 1) * d]);
        }
        let l = cluster_one(&sub, rows.len(), d, &p, &mut t);
        let mut max = -1;
        for (pos, &r) in rows.iter().enumerate() {
            if l[pos] >= 0 {
                labels[r] = next + l[pos];
                max = max.max(l[pos]);
            }
        }
        next += max + 1;
    }
    let bytes: Vec<u8> = labels.iter().flat_map(|&l| (l as i32).to_le_bytes()).collect();
    std::fs::write(&args[2], bytes).expect("write labels");
    println!(
        "{{\"n\":{n},\"d\":{d},\"knn_ms\":{:.1},\"cluster_ms\":{:.1},\"total_ms\":{:.1},\"clusters\":{next},\"threads\":{}}}",
        t.knn_ms,
        t.cluster_ms,
        t_all.elapsed().as_secs_f64() * 1e3,
        p.threads
    );
}
