//! Diagnostic for the exact production CPU embedder, not an accuracy benchmark.
//!
//! cargo run --release -p adage-intent --example reflex_embedding_probe -- MODEL_DIR [REPEATS]
//! Reports warm-filesystem open+normalize+embed latency separately from loaded embedding.
//! Deliberately contrasting sentences expose why vectors cannot carry execution order/authority.

use adage_intent::{cosine, model::StaticModel, normalize::normalize};
use anyhow::{ensure, Context, Result};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::path::Path;
use std::time::Instant;

fn percentile(values: &[f64], q: f64) -> f64 {
    let mut v = values.to_vec();
    v.sort_by(f64::total_cmp);
    v[((v.len() as f64 * q).ceil() as usize).saturating_sub(1).min(v.len() - 1)]
}

fn summary(values: &[f64]) -> serde_json::Value {
    json!({"n": values.len(), "p50_ms": percentile(values, 0.50), "p95_ms": percentile(values, 0.95)})
}

fn verify_model(dir: &Path) -> Result<serde_json::Value> {
    let manifest: serde_json::Value = serde_json::from_slice(&std::fs::read(dir.join("manifest.json"))?)?;
    ensure!(manifest["id"] == adage_intent::model::pinned_id(), "not the pinned production model");
    let files = manifest["files"].as_object().context("manifest files")?;
    ensure!(files.contains_key("model.safetensors") && files.contains_key("vocab.txt"), "missing model assets");
    for (name, metadata) in files {
        ensure!(matches!(name.as_str(), "model.safetensors" | "vocab.txt"), "unexpected asset");
        let bytes = std::fs::read(dir.join(name))?;
        let digest = format!("{:x}", Sha256::digest(&bytes));
        ensure!(metadata["sha256"].as_str() == Some(digest.as_str()), "asset checksum mismatch: {name}");
        ensure!(metadata["bytes"].as_u64() == Some(bytes.len() as u64), "asset length mismatch: {name}");
        if name == "model.safetensors" {
            ensure!(digest == adage_intent::model::FILES[0].sha256, "weights differ from pinned bytes");
        }
    }
    Ok(manifest)
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    ensure!((2..=3).contains(&args.len()), "usage: reflex_embedding_probe MODEL_DIR [REPEATS]");
    let dir = Path::new(&args[1]);
    let repeats: usize = args.get(2).map(|s| s.parse()).transpose()?.unwrap_or(100);
    ensure!((10..=10_000).contains(&repeats), "repeats must be 10..10000");
    let manifest = verify_model(dir)?;
    let pairs = [
        (
            "paraphrase",
            "Run the tests for the changed package.",
            "Verify the modified package using its test suite.",
            "Potentially equivalent intent; exact tool and inputs still require structured binding.",
        ),
        (
            "opposite_order",
            "Run generation before verification.",
            "Run verification before generation.",
            "Different execution order despite the same token multiset; order must be stored structurally.",
        ),
        (
            "negated_permission",
            "Run the tests for the changed package.",
            "Do not run the tests for the changed package.",
            "Opposite instruction; cosine similarity is not execution permission.",
        ),
        (
            "different_bound_identifier",
            "Look up INC-94 and read src/alpha.ts.",
            "Look up INC-203 and read src/beta.ts.",
            "Normalization intentionally removes identities; preserve original typed values separately.",
        ),
    ];
    let started = Instant::now();
    let loaded = StaticModel::load(dir)?;
    let load_ms = started.elapsed().as_secs_f64() * 1000.0;
    let mut contrasts = Vec::new();
    for (name, a, b, meaning) in pairs {
        let an = normalize(a);
        let bn = normalize(b);
        let av = loaded.embed(&an).context("missing diagnostic embedding")?;
        let bv = loaded.embed(&bn).context("missing diagnostic embedding")?;
        contrasts.push(json!({
            "case": name, "a": a, "b": b, "normalized_a": an, "normalized_b": bn,
            "cosine": cosine(&av, &bv), "maximum_component_difference": av.iter().zip(&bv).map(|(x,y)| (x-y).abs()).fold(0.0_f32, f32::max),
            "interpretation": meaning,
        }));
    }
    let samples = [
        "Run the tests for the changed package.",
        "Resolve INC-94 using the repository verification procedure.",
        "After generation completes, verify the outputs before updating the ticket.",
    ];
    let mut lazy = Vec::with_capacity(repeats);
    let mut hot = Vec::with_capacity(repeats);
    for i in 0..repeats {
        let input = samples[i % samples.len()];
        let start = Instant::now();
        let model = StaticModel::open(dir)?;
        let actual = model.embed(&normalize(input)).context("lazy embedding")?;
        lazy.push(start.elapsed().as_secs_f64() * 1000.0);
        let start = Instant::now();
        let expected = loaded.embed(&normalize(input)).context("loaded embedding")?;
        hot.push(start.elapsed().as_secs_f64() * 1000.0);
        ensure!(actual == expected, "lazy/loaded path output mismatch");
        std::hint::black_box(actual);
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "schema": 1,
            "claim_scope": "CPU production-path timing and authored semantic contrasts; not held-out accuracy, task benefit, process-spawn latency, or cold-disk timing",
            "model": {"id": loaded.id, "dimension": loaded.dim, "assets": manifest["files"], "digests_verified": true},
            "environment": {"os": std::env::consts::OS, "arch": std::env::consts::ARCH, "debug_assertions": cfg!(debug_assertions)},
            "timing_conditions": "Asset verification and diagnostic loading warm the filesystem cache; p95 uses nearest-rank order statistic; normalization included in both paths; model destructor not timed",
            "loaded_model_open_ms": load_ms,
            "lazy_open_normalize_embed": summary(&lazy),
            "loaded_normalize_embed": summary(&hot),
            "lazy_loaded_vectors_equal": true,
            "diagnostic_contrasts": contrasts,
            "external_model_requests": 0,
            "network_requests": 0
        }))?
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentile_uses_nearest_rank() {
        assert_eq!(percentile(&[4.0, 1.0, 3.0, 2.0], 0.5), 2.0);
        assert_eq!(percentile(&[4.0, 1.0, 3.0, 2.0], 0.95), 4.0);
    }
}
