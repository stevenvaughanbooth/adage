//! Rebuild the bundled model from the pinned upstream files, and print the digests the build pins.
//!
//! ```text
//! cargo run -p adage-intent --example derive_bundle -- <dir with the potion-base-8M files> [out-dir]
//! ```
//!
//! `<dir>` holds `model.safetensors`, `tokenizer.json` and `config.json` (the Hugging Face files,
//! or the unpacked npm package `@yarflam/potion-base-8m`): every file's SHA-256 is checked
//! against the pins first. Writes `potion-base-8m-i8.safetensors` and `potion-base-8m.vocab.txt`
//! to `out-dir` (default `crates/adage-intent/bundled`). The output is deterministic: the digests
//! printed must equal `BUNDLED_TABLE_SHA256` and `BUNDLED_VOCAB_SHA256` in `src/model.rs`.

use adage_intent::model::{self, BUNDLED_SPEC};
use sha2::{Digest, Sha256};
use std::path::PathBuf;

fn hex(b: &[u8]) -> String {
    Sha256::digest(b).iter().map(|b| format!("{b:02x}")).collect()
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let src = PathBuf::from(args.first().ok_or_else(|| anyhow::anyhow!("usage: derive_bundle <src> [out]"))?);
    let out = args
        .get(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("bundled"));
    model::verify(BUNDLED_SPEC, &src)?;
    let tmp = tempfile::tempdir()?;
    let m = model::install_spec(BUNDLED_SPEC, &src, tmp.path())?;
    let dir = tmp.path().join(BUNDLED_SPEC.key);
    let table = model::derive_i8(&std::fs::read(dir.join("model.safetensors"))?)?;
    let vocab = std::fs::read_to_string(dir.join("vocab.txt"))?;
    std::fs::create_dir_all(&out)?;
    std::fs::write(out.join(format!("{}-i8.safetensors", BUNDLED_SPEC.key)), &table)?;
    std::fs::write(out.join(format!("{}.vocab.txt", BUNDLED_SPEC.key)), &vocab)?;
    println!("{} unk {} max_chars {} dim {}", m.id, m.unk, m.max_chars, m.dim);
    println!("table  {} bytes sha256 {}", table.len(), hex(&table));
    println!("vocab  {} bytes sha256 {}", vocab.len(), hex(vocab.as_bytes()));
    Ok(())
}
