//! The pinned static embedding model: potion-base-8M (spike S2; implementation plan A6 "static
//! first, in-process").
//!
//! The model is **not** shipped with Adage. `adage models fetch` downloads the pinned files (or
//! `adage models install <dir>` takes them from a directory), verifies every SHA-256, and writes
//! the installed layout under `$ADAGE_HOME/models/potion-base-8m/`:
//!
//! ```text
//! manifest.json      id, dimensions, vocabulary size, sizes and digests of the two files below
//! model.safetensors  the 29528 x 256 f32 lookup table (30 MB), unchanged from upstream
//! vocab.txt          the WordPiece vocabulary, one token per line in id order, from tokenizer.json
//! ```
//!
//! Everything fails open: no model, a truncated file, or a different model means no embedding,
//! and no intent route fires. [`StaticModel::open`] reads only the manifest, the vocabulary, and
//! the safetensors header; embedding a prompt then reads just the rows of its tokens.

use crate::tokenize::WordPiece;
use anyhow::{bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs::File;
use std::path::{Path, PathBuf};

pub const MODEL_DIR: &str = "potion-base-8m";
const REVISION: &str = "bf8b056651a2c21b8d2565580b8569da283cab23";
const REPO: &str = "minishlab/potion-base-8M";

/// One pinned upstream file.
pub struct Pinned {
    pub name: &'static str,
    pub sha256: &'static str,
    pub bytes: u64,
}

/// The upstream files, pinned by digest (the digests S2 recorded).
pub const FILES: [Pinned; 3] = [
    Pinned {
        name: "model.safetensors",
        sha256: "f65d0f325faadc1e121c319e2faa41170d3fa07d8c89abd48ca5358d9a223de2",
        bytes: 30_236_760,
    },
    Pinned {
        name: "tokenizer.json",
        sha256: "e67e803f624fb4d67dea1c730d06e1067e1b14d830e2c2202569e3ef0f70bb50",
        bytes: 683_666,
    },
    Pinned {
        name: "config.json",
        sha256: "2a6ac0e9aaa356a68a5688070db78fc3a464fefe85d2f06a1905ce3718687553",
        bytes: 202,
    },
];

/// Where a pinned file is downloaded from.
pub fn url(name: &str) -> String {
    format!("https://huggingface.co/{REPO}/resolve/{REVISION}/{name}")
}

/// The model id recorded in every intent trigger: a mismatch at runtime disables intent routes.
pub fn pinned_id() -> String {
    format!("potion-base-8m@{}", &FILES[0].sha256[..12])
}

pub fn dir_in(models_root: &Path) -> PathBuf {
    models_root.join(MODEL_DIR)
}

const MAX_TOKENS: usize = 512;
const MAX_VOCAB_BYTES: u64 = 4 << 20;
const MAX_HEADER: u64 = 1 << 20;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FileEntry {
    pub sha256: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Manifest {
    pub id: String,
    pub dim: usize,
    pub vocab_size: usize,
    pub unk: u32,
    pub max_chars: usize,
    pub files: std::collections::BTreeMap<String, FileEntry>,
}

enum Rows {
    Loaded(Vec<f32>),
    File { file: File, offset: u64 },
}

pub struct StaticModel {
    pub id: String,
    pub dim: usize,
    wp: WordPiece,
    rows: Rows,
}

fn sha256_file(p: &Path) -> Result<String> {
    use std::io::Read;
    let mut f = File::open(p).with_context(|| format!("open {}", p.display()))?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

fn read_limited(p: &Path, max: u64) -> Result<Vec<u8>> {
    let len = std::fs::metadata(p).with_context(|| format!("{}", p.display()))?.len();
    ensure!(len <= max, "{} is larger than {max} bytes", p.display());
    Ok(std::fs::read(p)?)
}

/// Parse a safetensors header; returns (data start offset, rows, cols) of the single f32 matrix.
fn safetensors_header(file: &mut File, file_len: u64) -> Result<(u64, usize, usize)> {
    use std::io::Read;
    let mut len8 = [0u8; 8];
    file.read_exact(&mut len8)?;
    let hlen = u64::from_le_bytes(len8);
    ensure!(hlen <= MAX_HEADER && 8 + hlen <= file_len, "bad safetensors header length");
    let mut h = vec![0u8; hlen as usize];
    file.read_exact(&mut h)?;
    let v: serde_json::Value = serde_json::from_slice(&h)?;
    let obj = v.as_object().context("safetensors header is not an object")?;
    let tensors: Vec<(&String, &serde_json::Value)> = obj.iter().filter(|(k, _)| *k != "__metadata__").collect();
    ensure!(tensors.len() == 1, "expected exactly one tensor");
    let t = tensors[0].1;
    ensure!(t["dtype"] == "F32", "embedding table is not f32");
    let shape: Vec<u64> = t["shape"].as_array().context("shape")?.iter().filter_map(|x| x.as_u64()).collect();
    let off: Vec<u64> = t["data_offsets"].as_array().context("offsets")?.iter().filter_map(|x| x.as_u64()).collect();
    ensure!(shape.len() == 2 && off.len() == 2, "bad tensor shape");
    let (r, c) = (shape[0], shape[1]);
    ensure!(r > 0 && (1..=4096).contains(&c) && r <= 1 << 22, "implausible table shape");
    ensure!(off[1] >= off[0] && off[1] - off[0] == r * c * 4, "offsets do not match the shape");
    let start = 8 + hlen + off[0];
    ensure!(start + r * c * 4 <= file_len, "table extends past the end of the file");
    Ok((start, r as usize, c as usize))
}

impl StaticModel {
    /// Open an installed model for embedding a few prompts (the hook path): rows are read on
    /// demand. Any inconsistency is an error, and the caller fails open.
    pub fn open(dir: &Path) -> Result<StaticModel> {
        Self::open_with(dir, false)
    }

    /// Open and read the whole table into memory (the learner, which embeds many prompts).
    pub fn load(dir: &Path) -> Result<StaticModel> {
        Self::open_with(dir, true)
    }

    fn open_with(dir: &Path, load_all: bool) -> Result<StaticModel> {
        let m: Manifest = serde_json::from_slice(&read_limited(&dir.join("manifest.json"), 64 << 10)?)?;
        for (name, e) in &m.files {
            let len = std::fs::metadata(dir.join(name)).with_context(|| name.clone())?.len();
            ensure!(len == e.bytes, "{name} is {len} bytes, the manifest says {}", e.bytes);
        }
        let vocab_bytes = read_limited(&dir.join("vocab.txt"), MAX_VOCAB_BYTES)?;
        let vocab_text = String::from_utf8(vocab_bytes).context("vocab.txt is not UTF-8")?;
        let mut vocab = HashMap::with_capacity(m.vocab_size);
        for (i, tok) in vocab_text.split('\n').enumerate() {
            if i < m.vocab_size {
                vocab.insert(tok.to_string(), i as u32);
            }
        }
        ensure!(
            vocab.len() == m.vocab_size,
            "vocabulary has {} tokens, the manifest says {}",
            vocab.len(),
            m.vocab_size
        );
        ensure!((m.unk as usize) < m.vocab_size, "unk id out of range");
        let path = dir.join("model.safetensors");
        let mut file = File::open(&path)?;
        let file_len = file.metadata()?.len();
        let (offset, rows, cols) = safetensors_header(&mut file, file_len)?;
        ensure!(
            rows == m.vocab_size && cols == m.dim,
            "table is {rows}x{cols}, the manifest says {}x{}",
            m.vocab_size,
            m.dim
        );
        let rows_store = if load_all {
            use std::io::{Read, Seek, SeekFrom};
            file.seek(SeekFrom::Start(offset))?;
            let mut buf = vec![0u8; rows * cols * 4];
            file.read_exact(&mut buf)?;
            Rows::Loaded(buf.as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)).collect())
        } else {
            Rows::File { file, offset }
        };
        Ok(StaticModel {
            id: m.id,
            dim: m.dim,
            wp: WordPiece { vocab, unk: m.unk, max_chars: m.max_chars },
            rows: rows_store,
        })
    }

    fn row(&self, id: u32, out: &mut [f32]) -> Result<()> {
        let d = self.dim;
        match &self.rows {
            Rows::Loaded(v) => out.copy_from_slice(&v[id as usize * d..(id as usize + 1) * d]),
            Rows::File { file, offset } => {
                let mut buf = vec![0u8; d * 4];
                read_at(file, &mut buf, offset + id as u64 * d as u64 * 4)?;
                for (o, c) in out.iter_mut().zip(buf.as_chunks::<4>().0) {
                    *o = f32::from_le_bytes(*c);
                }
            }
        }
        Ok(())
    }

    /// Embed an already-normalised prompt: WordPiece ids (unknown tokens dropped, at most 512),
    /// mean of their rows, L2-normalised — model2vec's `encode`. `None` if no token is known.
    pub fn embed(&self, normalized: &str) -> Option<Vec<f32>> {
        let ids: Vec<u32> = self.wp.encode(normalized, MAX_TOKENS).into_iter().filter(|&i| i != self.wp.unk).collect();
        if ids.is_empty() {
            return None;
        }
        let mut acc = vec![0f64; self.dim];
        let mut row = vec![0f32; self.dim];
        let mut cache: HashMap<u32, Vec<f32>> = HashMap::new();
        for id in &ids {
            if !cache.contains_key(id) {
                self.row(*id, &mut row).ok()?;
                cache.insert(*id, row.clone());
            }
            let r = &cache[id];
            for (a, x) in acc.iter_mut().zip(r) {
                *a += *x as f64;
            }
        }
        let n = ids.len() as f64;
        let v: Vec<f32> = acc.iter().map(|a| (a / n) as f32).collect();
        if v.iter().all(|x| *x == 0.0) || v.iter().any(|x| !x.is_finite()) {
            return None;
        }
        Some(crate::unit(v))
    }
}

#[cfg(unix)]
fn read_at(f: &File, buf: &mut [u8], off: u64) -> std::io::Result<()> {
    use std::os::unix::fs::FileExt;
    f.read_exact_at(buf, off)
}

/// `seek_read` may return fewer bytes than asked; loop to the full length like `read_exact_at`.
#[cfg(windows)]
fn read_at(f: &File, mut buf: &mut [u8], mut off: u64) -> std::io::Result<()> {
    use std::os::windows::fs::FileExt;
    while !buf.is_empty() {
        match f.seek_read(buf, off) {
            Ok(0) => return Err(std::io::ErrorKind::UnexpectedEof.into()),
            Ok(n) => {
                buf = &mut buf[n..];
                off += n as u64;
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

#[cfg(not(any(unix, windows)))]
fn read_at(f: &File, buf: &mut [u8], off: u64) -> std::io::Result<()> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = f.try_clone()?;
    f.seek(SeekFrom::Start(off))?;
    f.read_exact(buf)
}

#[derive(Deserialize)]
struct TokJson {
    normalizer: serde_json::Value,
    pre_tokenizer: serde_json::Value,
    model: TokModel,
}

#[derive(Deserialize)]
struct TokModel {
    #[serde(rename = "type")]
    kind: String,
    unk_token: String,
    continuing_subword_prefix: String,
    max_input_chars_per_word: usize,
    vocab: HashMap<String, u32>,
}

/// Verify the pinned upstream files in `src` and install them under `models_root`. The new
/// directory is written beside the old one and renamed into place.
pub fn install(src: &Path, models_root: &Path) -> Result<Manifest> {
    for f in &FILES {
        let p = src.join(f.name);
        let len = std::fs::metadata(&p).with_context(|| format!("{} is missing", p.display()))?.len();
        ensure!(len == f.bytes, "{} is {len} bytes; the pinned file is {}", f.name, f.bytes);
        let got = sha256_file(&p)?;
        ensure!(got == f.sha256, "{}: sha256 {got} does not match the pinned {}", f.name, f.sha256);
    }
    let tok: TokJson = serde_json::from_slice(&std::fs::read(src.join("tokenizer.json"))?)?;
    ensure!(tok.normalizer["type"] == "BertNormalizer" && tok.normalizer["lowercase"] == true, "unexpected normalizer");
    ensure!(tok.pre_tokenizer["type"] == "BertPreTokenizer", "unexpected pre-tokenizer");
    ensure!(tok.model.kind == "WordPiece" && tok.model.continuing_subword_prefix == "##", "unexpected tokenizer model");
    let n = tok.model.vocab.len();
    let mut by_id: Vec<Option<&str>> = vec![None; n];
    for (t, &i) in &tok.model.vocab {
        ensure!((i as usize) < n && by_id[i as usize].is_none(), "vocabulary ids are not dense");
        ensure!(!t.contains('\n'), "a token contains a newline");
        by_id[i as usize] = Some(t);
    }
    let vocab_txt: String = by_id.iter().map(|t| t.unwrap_or_default()).collect::<Vec<_>>().join("\n");
    let unk = *tok.model.vocab.get(&tok.model.unk_token).context("unk token missing")?;
    let cfg: serde_json::Value = serde_json::from_slice(&std::fs::read(src.join("config.json"))?)?;
    let dim = cfg["hidden_dim"].as_u64().context("hidden_dim")? as usize;
    ensure!(cfg["normalize"] == true, "model does not normalise");
    write_installed(
        &dir_in(models_root),
        &pinned_id(),
        dim,
        unk,
        tok.model.max_input_chars_per_word,
        &vocab_txt,
        &std::fs::read(src.join("model.safetensors"))?,
    )
}

/// Write an installed model directory (manifest, vocabulary, table) atomically. `install` uses it
/// for the pinned model; tests use it for small fixture models.
pub fn write_installed(
    dir: &Path,
    id: &str,
    dim: usize,
    unk: u32,
    max_chars: usize,
    vocab_txt: &str,
    safetensors: &[u8],
) -> Result<Manifest> {
    let parent = dir.parent().context("model dir has no parent")?;
    std::fs::create_dir_all(parent)?;
    let tmp = parent.join(format!(
        ".{}.tmp{}",
        dir.file_name().and_then(|n| n.to_str()).unwrap_or("model"),
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp)?;
    std::fs::write(tmp.join("vocab.txt"), vocab_txt)?;
    std::fs::write(tmp.join("model.safetensors"), safetensors)?;
    let mut files = std::collections::BTreeMap::new();
    for name in ["model.safetensors", "vocab.txt"] {
        let p = tmp.join(name);
        files.insert(name.to_string(), FileEntry { sha256: sha256_file(&p)?, bytes: std::fs::metadata(&p)?.len() });
    }
    let m = Manifest { id: id.to_string(), dim, vocab_size: vocab_txt.split('\n').count(), unk, max_chars, files };
    std::fs::write(tmp.join("manifest.json"), serde_json::to_vec_pretty(&m)?)?;
    // Validate what we wrote before it replaces anything.
    StaticModel::open(&tmp)?;
    if dir.exists() {
        std::fs::remove_dir_all(dir)?;
    }
    std::fs::rename(&tmp, dir)?;
    Ok(m)
}

/// A safetensors file holding one f32 matrix named `embeddings`.
pub fn safetensors_bytes(rows: &[Vec<f32>]) -> Vec<u8> {
    let (r, c) = (rows.len(), rows.first().map(|x| x.len()).unwrap_or(0));
    let header = format!(r#"{{"embeddings":{{"dtype":"F32","shape":[{r},{c}],"data_offsets":[0,{}]}}}}"#, r * c * 4);
    let mut out = Vec::with_capacity(8 + header.len() + r * c * 4);
    out.extend_from_slice(&(header.len() as u64).to_le_bytes());
    out.extend_from_slice(header.as_bytes());
    for row in rows {
        for x in row {
            out.extend_from_slice(&x.to_le_bytes());
        }
    }
    out
}

/// Installed-model status for `adage models status`: `Ok(manifest)` or why it cannot be used.
pub fn status(models_root: &Path) -> Result<Manifest> {
    let dir = dir_in(models_root);
    if !dir.join("manifest.json").exists() {
        bail!("not installed (run `adage models fetch`)");
    }
    let m: Manifest = serde_json::from_slice(&read_limited(&dir.join("manifest.json"), 64 << 10)?)?;
    for (name, e) in &m.files {
        let got = sha256_file(&dir.join(name))?;
        ensure!(got == e.sha256, "{name} does not match its recorded digest; reinstall");
    }
    StaticModel::open(&dir)?;
    Ok(m)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A tiny model: each word points along its own axis; `pad` words are near zero.
    pub fn fixture(dir: &Path) -> StaticModel {
        let words = ["[UNK]", "fix", "tests", "failing", "docs", "write", "readme", "the", "##s"];
        let dim = 4;
        let rows: Vec<Vec<f32>> = words
            .iter()
            .map(|w| match *w {
                "fix" | "tests" | "failing" => vec![1.0, 0.1, 0.0, 0.0],
                "docs" | "write" | "readme" => vec![0.0, 0.1, 1.0, 0.0],
                _ => vec![0.0, 0.0, 0.0, 0.1],
            })
            .collect();
        write_installed(dir, "fixture@000000000000", dim, 0, 100, &words.join("\n"), &safetensors_bytes(&rows))
            .unwrap();
        StaticModel::open(dir).unwrap()
    }

    #[test]
    fn fixture_model_embeds_by_meaning() {
        let d = tempfile::tempdir().unwrap();
        let m = fixture(&d.path().join("m"));
        let a = m.embed("fix the failing tests").unwrap();
        let b = m.embed("Tests failing, fix!").unwrap();
        let c = m.embed("write the readme docs").unwrap();
        assert!(crate::cosine(&a, &b) > 0.99);
        assert!(crate::cosine(&a, &c) < 0.2);
        assert!(m.embed("zzz qqq").is_none(), "only unknown tokens: no vector");
        let full = StaticModel::load(&d.path().join("m")).unwrap();
        assert_eq!(full.embed("fix the failing tests").unwrap(), a, "lazy and loaded rows agree");
    }

    #[test]
    fn damaged_installs_fail_to_open() {
        let d = tempfile::tempdir().unwrap();
        let dir = d.path().join("m");
        fixture(&dir);
        let f = dir.join("model.safetensors");
        let bytes = std::fs::read(&f).unwrap();
        std::fs::write(&f, &bytes[..bytes.len() - 4]).unwrap();
        assert!(StaticModel::open(&dir).is_err(), "truncated table");
        assert!(StaticModel::open(&d.path().join("missing")).is_err());
        assert!(status(d.path()).is_err());
    }

    #[test]
    fn install_refuses_unpinned_files() {
        let d = tempfile::tempdir().unwrap();
        for f in &FILES {
            std::fs::write(d.path().join(f.name), b"not the model").unwrap();
        }
        assert!(install(d.path(), &d.path().join("models")).is_err());
        assert!(!dir_in(&d.path().join("models")).exists());
    }
}
