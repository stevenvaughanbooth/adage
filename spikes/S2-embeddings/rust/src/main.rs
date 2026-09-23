// S2 spike: confirm the in-process hook-client path for static embeddings.
// Loads minishlab/potion-base-8M via model2vec-rs from the local safetensors
// dir this spike already downloaded, times cold load and per-prompt embed
// latency, and checks determinism (encode the same batch twice).
//
// Usage: cargo run --release -- <model_dir> <sample_prompts_file>

use std::env;
use std::fs;
use std::time::Instant;

use model2vec_rs::model::StaticModel;

fn percentile(sorted_ms: &[f64], p: f64) -> f64 {
    if sorted_ms.is_empty() {
        return f64::NAN;
    }
    let idx = ((p / 100.0) * (sorted_ms.len() as f64 - 1.0)).round() as usize;
    sorted_ms[idx.min(sorted_ms.len() - 1)]
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = env::args().collect();
    let model_dir = args
        .get(1)
        .cloned()
        .unwrap_or_else(|| "../models/potion-base-8m".to_string());
    let sample_path = args
        .get(2)
        .cloned()
        .unwrap_or_else(|| "../data/normalized_sample.txt".to_string());

    let sentences: Vec<String> = fs::read_to_string(&sample_path)?
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.to_string())
        .collect();

    // ---- cold load ----
    let t0 = Instant::now();
    let model = StaticModel::from_pretrained(&model_dir, None, None, None)?;
    // first embedding, still counted toward "process start -> first
    // embedding ready" per the SPIKE's cold-load definition
    let _first = model.encode(&vec![sentences[0].clone()]);
    let cold_load_ms = t0.elapsed().as_secs_f64() * 1000.0;

    // ---- warmup ----
    for s in sentences.iter().take(20) {
        let _ = model.encode(&vec![s.clone()]);
    }

    // ---- per-prompt latency, batch size 1 ----
    let mut lat_ms: Vec<f64> = Vec::with_capacity(sentences.len());
    for s in sentences.iter() {
        let t = Instant::now();
        let _ = model.encode(&vec![s.clone()]);
        lat_ms.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    let mut sorted = lat_ms.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let p50 = percentile(&sorted, 50.0);
    let p95 = percentile(&sorted, 95.0);
    let p99 = percentile(&sorted, 99.0);

    // ---- determinism: encode the same 50 sentences twice ----
    let fixed: Vec<String> = sentences.iter().take(50).cloned().collect();
    let e1 = model.encode(&fixed);
    let e2 = model.encode(&fixed);
    let mut max_abs_diff: f32 = 0.0;
    for (row1, row2) in e1.iter().zip(e2.iter()) {
        for (a, b) in row1.iter().zip(row2.iter()) {
            let d = (a - b).abs();
            if d > max_abs_diff {
                max_abs_diff = d;
            }
        }
    }

    let dim = e1.first().map(|v| v.len()).unwrap_or(0);

    let result = serde_json::json!({
        "runtime": "rust/model2vec-rs",
        "model": "potion-base-8m",
        "dim": dim,
        "n_sentences": sentences.len(),
        "cold_load_ms": cold_load_ms,
        "latency_p50_ms": p50,
        "latency_p95_ms": p95,
        "latency_p99_ms": p99,
        "determinism_max_abs_diff": max_abs_diff,
    });
    println!("{}", result.to_string());

    Ok(())
}
