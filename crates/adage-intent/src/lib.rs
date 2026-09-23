//! Intent families (Design §8.2.1; spikes S2 and S7).
//!
//! * [`normalize`] — prompt normalisation before embedding: typed placeholders for IDs, paths,
//!   SHAs, versions, URLs; fenced code stripped; the head of the prompt kept.
//! * [`model`] — the pinned static embedding model (potion-base-8M), loaded in-process with no
//!   runtime dependencies beyond std: a WordPiece tokenizer, a lookup table read from
//!   safetensors, mean pooling, L2 normalisation. Rows are read on demand, so a hook process
//!   touches a few kilobytes, not the 30 MB table.
//! * [`cluster`] — the method S7 recommends: exact cosine kNN, then deterministic Louvain on the
//!   kNN graph thresholded at a similarity, then a minimum family size.
//! * [`quantize`] / [`dequantize`] — centroids travel in tracks as integers (units of 1e-4), which
//!   serialise exactly and validate simply.
//!
//! Everything here is deterministic: same input, same bytes.

pub mod cluster;
pub mod model;
pub mod normalize;
pub mod purity;
pub mod tokenize;

/// Centroid components are stored as integers in units of 1/`SCALE`.
pub const SCALE: f32 = 10_000.0;

/// Quantise a unit vector for storage in a track.
pub fn quantize(v: &[f32]) -> Vec<i16> {
    v.iter().map(|x| (x * SCALE).round().clamp(-SCALE, SCALE) as i16).collect()
}

/// A stored centroid as a unit vector.
pub fn dequantize(q: &[i16]) -> Vec<f32> {
    let v: Vec<f32> = q.iter().map(|&x| x as f32 / SCALE).collect();
    unit(v)
}

/// Scale to unit length (a zero vector stays zero).
pub fn unit(mut v: Vec<f32>) -> Vec<f32> {
    let n = v.iter().map(|x| (*x as f64) * (*x as f64)).sum::<f64>().sqrt();
    if n > 0.0 {
        for x in v.iter_mut() {
            *x = (*x as f64 / n) as f32;
        }
    }
    v
}

/// Cosine similarity of two unit vectors (fixed summation order; symmetric).
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() {
        return f32::NEG_INFINITY;
    }
    cluster::dot(a, b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quantisation_round_trips_to_a_unit_vector() {
        let v = unit(vec![0.3, -0.4, 0.5, 0.1]);
        let back = dequantize(&quantize(&v));
        assert!(cosine(&v, &back) > 0.9999);
        assert!(quantize(&v).iter().zip(quantize(&back)).all(|(a, b)| (a - b).abs() <= 1));
    }
}
