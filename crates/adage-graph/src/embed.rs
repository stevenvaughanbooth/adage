//! Node embeddings (Track G §1.2).
//!
//! Every vector is a unit vector, quantised to `i16` with `adage_intent::quantize` before it is
//! stored or compared for a decision, so what is stored is exact and portable.
//!
//! - **Prompt:** the pinned static model (potion-base-8M) on `adage_intent::normalize::normalize`
//!   of the text, exactly as intent families embed prompts. No model installed: no prompt
//!   embedding (the node keeps its exact label).
//! - **Act** ([`ACT_DIM`] = 384): [`HASH_DIM`] = 256 dimensions of signed feature hashing over the
//!   program, the identity, identity ⊕ option name for each option, template token uni- and
//!   bigrams, the positional count and each positional's kind, and the effect class; then
//!   [`POTION_HALF`] = 128 dimensions of the model's embedding of the identity's words (split on
//!   `/._-` and camelCase; the model's 256 dimensions folded in two), so `npm run check` and
//!   `make check` land near each other. The halves are weighted 0.75 and 0.25, then renormalised.
//!   Without a model the second half is zero: the vector is the hashed half alone.
//! - **Art** ([`ART_DIM`] = 256): hashed features of every directory prefix (weighted by depth),
//!   the extension, the stem's words and the sibling-manifest flags a caller supplies (a
//!   `Cargo.toml` in an ancestor makes the package root a feature), plus, with a model, 0.25 of
//!   the embedding of the path's words.
//!
//! **Feature hashing** is seeded with a fixed string and uses SHA-256: a feature's index is the
//! first four bytes of `SHA-256(SEED ‖ feature)` modulo the dimension, its sign the fifth byte's
//! low bit. No process-dependent hasher is involved, so the bytes are the same on every machine.

use adage_core::pattern::Shape;
use adage_intent::model::StaticModel;
use sha2::{Digest, Sha256};

pub const HASH_DIM: usize = 256;
pub const POTION_HALF: usize = 128;
pub const ACT_DIM: usize = HASH_DIM + POTION_HALF;
pub const ART_DIM: usize = 256;
/// The feature-hashing seed. Changing it changes every stored vector: bump [`EMBED_VERSION`].
pub const SEED: &str = "adage-graph/embed/1";
/// The embedding scheme's version, part of every facts cache key.
pub const EMBED_VERSION: u32 = 1;

/// Embeds nodes, with the static model when one is installed.
#[derive(Clone, Copy, Default)]
pub struct Embedder<'a> {
    model: Option<&'a StaticModel>,
}

impl<'a> Embedder<'a> {
    pub fn new(model: Option<&'a StaticModel>) -> Self {
        Embedder { model }
    }

    /// The model's id (`potion-base-8m@…`), or `None` without one: part of the facts cache key.
    pub fn model_id(&self) -> Option<&str> {
        self.model.map(|m| m.id.as_str())
    }

    /// A prompt's embedding, or `None` without a model (or when no token is known to it).
    pub fn prompt(&self, text: &str) -> Option<Vec<i16>> {
        let m = self.model?;
        let v = m.embed(&adage_intent::normalize::normalize(text))?;
        Some(adage_intent::quantize(&adage_intent::unit(v)))
    }

    /// An act's embedding: a command's argv (empty for a read or an edit), its template key and
    /// its effect class.
    pub fn act(&self, argv: &[String], key: &str, effect: &str) -> Vec<i16> {
        let mut feats: Vec<(String, f32)> = Vec::new();
        let mut words = String::new();
        if !effect.is_empty() {
            feats.push((format!("effect:{effect}"), 1.0));
        }
        match Shape::of_tokens(argv) {
            Some(s) => {
                let identity = s.identity.join(" ");
                feats.push((format!("program:{}", s.program), 1.0));
                feats.push((format!("identity:{identity}"), 1.5));
                for o in &s.options {
                    let name = o.split([' ', '=']).next().unwrap_or(o);
                    feats.push((format!("option:{identity}\u{1}{name}"), 0.5));
                }
                feats.push((format!("positionals:{}", s.positionals.len().min(8)), 0.5));
                for (i, p) in s.positionals.iter().enumerate().take(8) {
                    feats.push((format!("pos:{i}:{}", value_kind(p)), 0.5));
                }
                words = split_words(&identity);
            }
            None => {
                let kind = key.split_whitespace().next().unwrap_or("");
                feats.push((format!("kind:{kind}"), 1.5));
            }
        }
        let toks: Vec<&str> = key.split_whitespace().collect();
        for t in &toks {
            feats.push((format!("tok:{t}"), 0.5));
        }
        for w in toks.windows(2) {
            feats.push((format!("bi:{}\u{1}{}", w[0], w[1]), 0.5));
        }
        let hashed = hashed(&feats, HASH_DIM);
        let potion = self.words(&words).map(|v| fold(&v, POTION_HALF)).unwrap_or_else(|| vec![0.0; POTION_HALF]);
        let mut v: Vec<f32> = hashed.iter().map(|x| 0.75 * x).collect();
        v.extend(potion.iter().map(|x| 0.25 * x));
        adage_intent::quantize(&adage_intent::unit(v))
    }

    /// An artifact's embedding: a repository-relative path, and the sibling manifests found in
    /// its ancestors (`Cargo.toml`, `package.json`, `pyproject.toml`; the caller's knowledge of
    /// the tree, empty when unknown).
    pub fn art(&self, path: &str, manifests: &[(String, String)]) -> Vec<i16> {
        let mut feats: Vec<(String, f32)> = Vec::new();
        let segs: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        let (dirs, file) = segs.split_at(segs.len().saturating_sub(1));
        for d in 1..=dirs.len() {
            feats.push((format!("dir:{}", dirs[..d].join("/")), 1.0 + 0.5 * d as f32));
        }
        let file = file.first().copied().unwrap_or("");
        let (stem, ext) = match file.rsplit_once('.') {
            Some((s, e)) if !s.is_empty() => (s, e),
            _ => (file, ""),
        };
        feats.push((format!("ext:{ext}"), 2.0));
        for w in split_words(stem).split_whitespace() {
            feats.push((format!("stem:{w}"), 0.75));
        }
        for (dir, manifest) in manifests {
            feats.push((format!("manifest:{manifest}"), 0.75));
            feats.push((format!("package:{dir}"), 1.0));
        }
        let h = hashed(&feats, ART_DIM);
        let v: Vec<f32> = match self.words(&split_words(&segs.join(" "))) {
            Some(p) => {
                let p = fold(&p, ART_DIM);
                h.iter().zip(p.iter()).map(|(a, b)| 0.75 * a + 0.25 * b).collect()
            }
            None => h,
        };
        adage_intent::quantize(&adage_intent::unit(v))
    }

    /// Offline text (an output's head, a failure signature): the model's embedding, or `None`.
    pub fn text(&self, text: &str) -> Option<Vec<i16>> {
        self.prompt(text)
    }

    fn words(&self, words: &str) -> Option<Vec<f32>> {
        let m = self.model?;
        if words.trim().is_empty() {
            return None;
        }
        m.embed(&words.to_lowercase())
    }
}

/// What a positional operand looks like, coarsely: `num`, `path`, `flaglike`, `id`, `word`,
/// `other`.
pub fn value_kind(v: &str) -> &'static str {
    if !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit()) {
        "num"
    } else if v.contains('/') || v.rsplit_once('.').is_some_and(|(s, e)| !s.is_empty() && e.len() <= 5) {
        "path"
    } else if v.starts_with('-') {
        "flaglike"
    } else if v.bytes().any(|b| b.is_ascii_digit()) && v.bytes().any(|b| b.is_ascii_alphabetic()) {
        "id"
    } else if v.bytes().all(|b| b.is_ascii_alphabetic()) {
        "word"
    } else {
        "other"
    }
}

/// Words of an identifier or path: split on `/._-`, whitespace and camelCase, lowercased,
/// joined by single spaces.
pub fn split_words(s: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut prev_lower = false;
    for c in s.chars() {
        if c.is_whitespace() || "/._-:@=".contains(c) {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            prev_lower = false;
            continue;
        }
        if c.is_uppercase() && prev_lower && !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
        prev_lower = c.is_lowercase() || c.is_ascii_digit();
        cur.extend(c.to_lowercase());
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out.join(" ")
}

/// Signed feature hashing into `dim` dimensions (see the module documentation). Features are
/// summed in the order given; the caller's order is deterministic.
pub fn hashed(features: &[(String, f32)], dim: usize) -> Vec<f32> {
    let mut v = vec![0f32; dim];
    for (f, w) in features {
        let mut h = Sha256::new();
        h.update(SEED.as_bytes());
        h.update([0u8]);
        h.update(f.as_bytes());
        let d = h.finalize();
        let i = u32::from_le_bytes([d[0], d[1], d[2], d[3]]) as usize % dim;
        let sign = if d[4] & 1 == 0 { 1.0 } else { -1.0 };
        v[i] += sign * w;
    }
    adage_intent::unit(v)
}

/// Fold a vector to `dim` dimensions by summing components `i`, `i + dim`, … (fixed order), then
/// renormalise.
pub fn fold(v: &[f32], dim: usize) -> Vec<f32> {
    let mut out = vec![0f32; dim];
    for (i, x) in v.iter().enumerate() {
        out[i % dim] += x;
    }
    adage_intent::unit(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(s: &str) -> Vec<String> {
        s.split(' ').map(str::to_string).collect()
    }

    fn cos(a: &[i16], b: &[i16]) -> f32 {
        adage_intent::cosine(&adage_intent::dequantize(a), &adage_intent::dequantize(b))
    }

    #[test]
    fn hashed_acts_are_unit_deterministic_and_structural() {
        let e = Embedder::new(None);
        let a = e.act(&argv("pytest tests/a.py"), "exec pytest {path}", "verify");
        assert_eq!(a.len(), ACT_DIM);
        assert_eq!(a, e.act(&argv("pytest tests/a.py"), "exec pytest {path}", "verify"));
        let b = e.act(&argv("pytest tests/b.py"), "exec pytest {path}", "verify");
        let c = e.act(&argv("make gen"), "exec make gen", "regenerate");
        assert!(cos(&a, &b) > 0.95, "{}", cos(&a, &b));
        assert!(cos(&a, &c) < 0.5, "{}", cos(&a, &c));
        // The potion half is zero without a model.
        assert!(a[HASH_DIM..].iter().all(|&x| x == 0));
    }

    #[test]
    fn arts_share_directories_and_extensions() {
        let e = Embedder::new(None);
        let a = e.art("schema/invoice.json", &[]);
        let b = e.art("schema/customer.json", &[]);
        let c = e.art("src/lib.rs", &[]);
        assert_eq!(a.len(), ART_DIM);
        assert!(cos(&a, &b) > cos(&a, &c));
        assert!(e.prompt("anything").is_none());
    }

    #[test]
    fn words_split() {
        assert_eq!(split_words("scripts/wi-lookup"), "scripts wi lookup");
        assert_eq!(split_words("runCheckAll.py"), "run check all py");
        assert_eq!(value_kind("412"), "num");
        assert_eq!(value_kind("BIL-12"), "id");
        assert_eq!(value_kind("tests/a.py"), "path");
    }

    /// A tiny installed model (adage-intent's own fixture is private to its tests).
    pub(crate) fn fixture(dir: &std::path::Path) -> StaticModel {
        use adage_intent::model::{safetensors_bytes, write_installed};
        let words = ["[UNK]", "npm", "run", "check", "make", "the", "checks", "lookup", "wi"];
        let rows: Vec<Vec<f32>> =
            (0..words.len()).map(|i| (0..4).map(|j| if i % 4 == j { 1.0 } else { 0.1 }).collect()).collect();
        write_installed(dir, "fixture@000000000000", 4, 0, 100, &words.join("\n"), &safetensors_bytes(&rows)).unwrap();
        StaticModel::open(dir).unwrap()
    }

    #[test]
    fn a_model_fills_the_potion_half() {
        let d = tempfile::tempdir().unwrap();
        let m = fixture(&d.path().join("m"));
        let e = Embedder::new(Some(&m));
        let a = e.act(&argv("npm run check"), "exec npm run check", "verify");
        assert!(a[HASH_DIM..].iter().any(|&x| x != 0));
        assert!(e.prompt("run the checks").is_some());
    }
}
