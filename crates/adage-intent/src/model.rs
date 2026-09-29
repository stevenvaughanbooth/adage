//! Static embedding models (spike S2; implementation plan A6 "static first, in-process"), and
//! which one a process uses.
//!
//! **Models Adage knows** ([`Spec`]), each pinned by the SHA-256 of every upstream file:
//!
//! - [`BASE_8M`], potion-base-8M (MIT, 29,528 × 256, f32): the default;
//! - [`CODE_16M`], potion-code-16M-v2 (MIT, 63,457 × 256, f16), distilled from CodeRankEmbed.
//!
//! **Where a model comes from** ([`resolve`], in this order):
//!
//! 1. **Installed** under `$ADAGE_HOME/models/<key>/` by `adage models fetch [<key>]` (from Hugging
//!    Face at a pinned revision, else the npm registry's byte-identical copy) or `adage models
//!    install <dir>`. Every file's SHA-256 is checked before anything is written:
//!
//!    ```text
//!    manifest.json      id, dimensions, vocabulary size, sizes and digests of the two files below
//!    model.safetensors  the lookup table (f32 or f16), unchanged from upstream
//!    vocab.txt          the WordPiece vocabulary, one token per line in id order, from tokenizer.json
//!    ```
//!
//! 2. **Bundled** in the binary (feature `bundled`, on by default): potion-base-8M's table
//!    quantised to int8 with one f32 scale per row ([`derive_i8`], 7.7 MB), and its vocabulary.
//!    The bundled bytes are pinned by SHA-256 ([`BUNDLED_TABLE_SHA256`]); a test checks them, and
//!    `adage models derive` rebuilds them from the upstream files. So a fresh install embeds with
//!    no download and no user action.
//! 3. **Hashed**: signed feature hashing of the prompt's words ([`hashed_embed`]). Always there,
//!    never silent: [`Resolved::source`] says why no model is in use, and `adage doctor` and the
//!    status card print it. Intent families need a real model; the hashed fallback serves only
//!    similarity that can do with lexical overlap (the case store, prompt → area routing).
//!
//! `ADAGE_EMBED_MODEL` (or `$ADAGE_HOME/models/active`, written by `adage models use`) selects:
//! `auto` (the default: an installed potion-base-8M, else the bundled one), a model key, or
//! `hashed`.
//!
//! Everything fails open: a truncated file or a different model means that source is skipped.
//! [`StaticModel::open`] reads only the manifest, the vocabulary, and the safetensors header;
//! embedding a prompt then reads just the rows of its tokens (for the bundled table, the pages of
//! the binary those rows are on).

use crate::tokenize::WordPiece;
use anyhow::{bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs::File;
use std::path::{Path, PathBuf};

pub const MODEL_DIR: &str = "potion-base-8m";

/// One pinned upstream file.
pub struct Pinned {
    pub name: &'static str,
    pub sha256: &'static str,
    pub bytes: u64,
}

/// The npm package that carries a model's upstream files byte for byte, and where in its tarball
/// each file is. The registry is reachable where Hugging Face is not (corporate proxies); the
/// files' SHA-256 pins, not the package, decide what is installed.
pub struct Npm {
    pub package: &'static str,
    pub version: &'static str,
    pub tarball: &'static str,
    pub tarball_bytes: u64,
    pub tarball_sha256: &'static str,
    /// (upstream file name, path inside the tarball).
    pub paths: &'static [(&'static str, &'static str)],
}

/// A static embedding model Adage knows.
pub struct Spec {
    /// Its key and installed directory name (`potion-base-8m`).
    pub key: &'static str,
    pub licence: &'static str,
    /// Hugging Face repository and pinned revision.
    pub hf_repo: &'static str,
    pub hf_revision: &'static str,
    /// The upstream files, `model.safetensors` first.
    pub files: &'static [Pinned],
    pub npm: Npm,
}

/// The upstream files of potion-base-8M, pinned by digest (the digests S2 recorded).
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

/// potion-base-8M (MinishLab, MIT). The npm copy (`@yarflam/potion-base-8m` 1.0.4, MIT) holds the
/// three files byte-identical to the pins (checked 2026-09-29).
pub const BASE_8M: Spec = Spec {
    key: "potion-base-8m",
    licence: "MIT",
    hf_repo: "minishlab/potion-base-8M",
    hf_revision: "bf8b056651a2c21b8d2565580b8569da283cab23",
    files: &FILES,
    npm: Npm {
        package: "@yarflam/potion-base-8m",
        version: "1.0.4",
        tarball: "https://registry.npmjs.org/@yarflam/potion-base-8m/-/potion-base-8m-1.0.4.tgz",
        tarball_bytes: 28_339_756,
        tarball_sha256: "bc613419765661159d9c30d4a611753288c6b96db984a70fb41c64a961c7e647",
        paths: &[
            ("model.safetensors", "package/models/model.safetensors"),
            ("tokenizer.json", "package/models/tokenizer.json"),
            ("config.json", "package/models/config.json"),
        ],
    },
};

/// potion-code-16M-v2 (MinishLab, MIT; distilled from nomic-ai/CodeRankEmbed): a static model
/// for natural-language → code retrieval. Its npm copy (`zvec-grep-potion-code-16m-v2-model`
/// 0.1.0, MIT) records the upstream revision and these digests in its own manifest.
pub const CODE_16M: Spec = Spec {
    key: "potion-code-16m-v2",
    licence: "MIT",
    hf_repo: "minishlab/potion-code-16M-v2",
    hf_revision: "e9d2a44ca6a05ac6685f3b23709ea57eb7352d5b",
    files: &[
        Pinned {
            name: "model.safetensors",
            sha256: "75cf7a6c2171b230ad19b1e7d8e0b1aee86da5a02af8e7cacedd9921d227623c",
            bytes: 32_490_072,
        },
        Pinned {
            name: "tokenizer.json",
            sha256: "107bbdcbad4bff1d299b7a4c3a2fb17c52890688b7dd0e4c9deab79d3c4f3d45",
            bytes: 1_024_340,
        },
    ],
    npm: Npm {
        package: "zvec-grep-potion-code-16m-v2-model",
        version: "0.1.0",
        tarball: "https://registry.npmjs.org/zvec-grep-potion-code-16m-v2-model/-/zvec-grep-potion-code-16m-v2-model-0.1.0.tgz",
        tarball_bytes: 30_564_858,
        tarball_sha256: "2a0c4607bd49ba1172508b4d481985dd4d0816f6edb41fa59024a433e5700872",
        paths: &[
            ("model.safetensors", "package/assets/model.safetensors"),
            ("tokenizer.json", "package/assets/tokenizer/tokenizer.json"),
        ],
    },
};

/// Every model Adage knows, the default first.
pub const SPECS: [&Spec; 2] = [&BASE_8M, &CODE_16M];

/// The spec of a key (`potion-base-8m`, `potion-code-16m-v2`; `potion-code-16m` names v2).
pub fn spec(key: &str) -> Option<&'static Spec> {
    let key = if key == "potion-code-16m" { CODE_16M.key } else { key };
    SPECS.iter().copied().find(|s| s.key == key)
}

impl Spec {
    /// The id recorded with everything learned with this model: key and the table's digest.
    pub fn id(&self) -> String {
        format!("{}@{}", self.key, &self.files[0].sha256[..12])
    }

    /// Download URL of an upstream file: `ADAGE_MODEL_BASE_URL/<name>` when that is set (a mirror
    /// holding the same files; the SHA-256 pins still decide what is installed), else the pinned
    /// revision on huggingface.co.
    pub fn url(&self, name: &str) -> String {
        match std::env::var("ADAGE_MODEL_BASE_URL").ok().map(|b| b.trim().trim_end_matches('/').to_string()) {
            Some(base) if !base.is_empty() => format!("{base}/{name}"),
            _ => format!("https://huggingface.co/{}/resolve/{}/{name}", self.hf_repo, self.hf_revision),
        }
    }

    pub fn bytes(&self) -> u64 {
        self.files.iter().map(|f| f.bytes).sum()
    }
}

/// [`Spec::url`] of the default model.
pub fn url(name: &str) -> String {
    BASE_8M.url(name)
}

/// The host of a download URL (`huggingface.co`), for messages.
pub fn host(url: &str) -> String {
    let rest = url.split("://").nth(1).unwrap_or(url);
    rest.split(['/', ':']).next().unwrap_or(rest).to_string()
}

/// The model id recorded in every intent trigger: a mismatch at runtime disables intent routes.
pub fn pinned_id() -> String {
    BASE_8M.id()
}

/// Ids of the models this build can embed with (installed or bundled): what a pack's intent
/// trigger may name.
pub fn known_id(id: &str) -> bool {
    SPECS.iter().any(|s| s.id() == id) || bundled_id().is_some_and(|b| b == id)
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

/// How the table's numbers are stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dtype {
    F32,
    F16,
    /// int8 with one f32 scale per row (a second tensor, `scales`).
    I8,
}

impl Dtype {
    fn size(self) -> u64 {
        match self {
            Dtype::F32 => 4,
            Dtype::F16 => 2,
            Dtype::I8 => 1,
        }
    }
}

/// Where the table is in its safetensors bytes.
#[derive(Debug, Clone, Copy)]
struct Table {
    offset: u64,
    rows: usize,
    cols: usize,
    dtype: Dtype,
    /// The start of the f32 row scales (int8 only).
    scales: Option<u64>,
}

enum Store {
    Loaded(Vec<f32>),
    File(File),
    Static(&'static [u8]),
}

pub struct StaticModel {
    pub id: String,
    pub dim: usize,
    wp: WordPiece,
    table: Table,
    store: Store,
}

pub(crate) fn sha256_bytes(b: &[u8]) -> String {
    Sha256::digest(b).iter().map(|b| format!("{b:02x}")).collect()
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

/// Parse a safetensors header (the JSON after the 8-byte length): the `embeddings` tensor (or the
/// only tensor), f32, f16 or int8; an int8 table needs a `scales` tensor of one f32 per row.
fn parse_header(h: &[u8], hlen: u64, file_len: u64) -> Result<Table> {
    let v: serde_json::Value = serde_json::from_slice(h)?;
    let obj = v.as_object().context("safetensors header is not an object")?;
    let tensors: Vec<(&String, &serde_json::Value)> = obj.iter().filter(|(k, _)| *k != "__metadata__").collect();
    let t = match obj.get("embeddings") {
        Some(t) => t,
        None => {
            ensure!(tensors.len() == 1, "expected exactly one tensor");
            tensors[0].1
        }
    };
    let dtype = match t["dtype"].as_str() {
        Some("F32") => Dtype::F32,
        Some("F16") => Dtype::F16,
        Some("I8") => Dtype::I8,
        other => bail!("embedding table dtype {other:?} is not F32, F16 or I8"),
    };
    let nums = |t: &serde_json::Value, k: &str| -> Result<Vec<u64>> {
        Ok(t[k].as_array().with_context(|| k.to_string())?.iter().filter_map(|x| x.as_u64()).collect())
    };
    let shape = nums(t, "shape")?;
    let off = nums(t, "data_offsets")?;
    ensure!(shape.len() == 2 && off.len() == 2, "bad tensor shape");
    let (r, c) = (shape[0], shape[1]);
    ensure!(r > 0 && (1..=4096).contains(&c) && r <= 1 << 22, "implausible table shape");
    ensure!(off[1] >= off[0] && off[1] - off[0] == r * c * dtype.size(), "offsets do not match the shape");
    let data = 8 + hlen;
    let start = data + off[0];
    ensure!(start + r * c * dtype.size() <= file_len, "table extends past the end of the file");
    let scales = if dtype == Dtype::I8 {
        let s = obj.get("scales").context("an int8 table needs a `scales` tensor")?;
        ensure!(s["dtype"] == "F32", "scales are not f32");
        let (sh, so) = (nums(s, "shape")?, nums(s, "data_offsets")?);
        ensure!(sh == [r] && so.len() == 2 && so[1] >= so[0] && so[1] - so[0] == r * 4, "bad scales tensor");
        ensure!(data + so[1] <= file_len, "scales extend past the end of the file");
        Some(data + so[0])
    } else {
        None
    };
    Ok(Table { offset: start, rows: r as usize, cols: c as usize, dtype, scales })
}

fn header_of_file(file: &mut File, file_len: u64) -> Result<Table> {
    use std::io::Read;
    let mut len8 = [0u8; 8];
    file.read_exact(&mut len8)?;
    let hlen = u64::from_le_bytes(len8);
    ensure!(hlen <= MAX_HEADER && 8 + hlen <= file_len, "bad safetensors header length");
    let mut h = vec![0u8; hlen as usize];
    file.read_exact(&mut h)?;
    parse_header(&h, hlen, file_len)
}

fn header_of_bytes(b: &[u8]) -> Result<Table> {
    ensure!(b.len() >= 8, "not a safetensors file");
    let hlen = u64::from_le_bytes(b[..8].try_into().unwrap_or_default());
    ensure!(hlen <= MAX_HEADER && 8 + hlen <= b.len() as u64, "bad safetensors header length");
    parse_header(&b[8..8 + hlen as usize], hlen, b.len() as u64)
}

/// IEEE 754 half → single precision (exact).
pub fn f16_to_f32(h: u16) -> f32 {
    let sign = ((h >> 15) as u32) << 31;
    let exp = ((h >> 10) & 0x1f) as u32;
    let man = (h & 0x3ff) as u32;
    let bits = match (exp, man) {
        (0, 0) => sign,
        (0, mut m) => {
            // Subnormal: normalise.
            let mut e = 127 - 15 + 1;
            while m & 0x400 == 0 {
                m <<= 1;
                e -= 1;
            }
            sign | (e << 23) | ((m & 0x3ff) << 13)
        }
        (31, m) => sign | 0x7f80_0000 | (m << 13),
        (e, m) => sign | ((e + 127 - 15) << 23) | (m << 13),
    };
    f32::from_bits(bits)
}

/// Decode one row's bytes (and its scale, for int8) into `out`.
fn decode(dtype: Dtype, bytes: &[u8], scale: f32, out: &mut [f32]) {
    match dtype {
        Dtype::F32 => {
            for (o, c) in out.iter_mut().zip(bytes.as_chunks::<4>().0) {
                *o = f32::from_le_bytes(*c);
            }
        }
        Dtype::F16 => {
            for (o, c) in out.iter_mut().zip(bytes.as_chunks::<2>().0) {
                *o = f16_to_f32(u16::from_le_bytes(*c));
            }
        }
        Dtype::I8 => {
            for (o, b) in out.iter_mut().zip(bytes) {
                *o = (*b as i8) as f32 * scale;
            }
        }
    }
}

fn parse_vocab(text: &str, size: usize) -> Result<HashMap<String, u32>> {
    let mut vocab = HashMap::with_capacity(size);
    for (i, tok) in text.split('\n').enumerate() {
        if i < size {
            vocab.insert(tok.to_string(), i as u32);
        }
    }
    ensure!(vocab.len() == size, "vocabulary has {} tokens, the manifest says {size}", vocab.len());
    Ok(vocab)
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
        let vocab = parse_vocab(&vocab_text, m.vocab_size)?;
        ensure!((m.unk as usize) < m.vocab_size, "unk id out of range");
        let path = dir.join("model.safetensors");
        let mut file = File::open(&path)?;
        let file_len = file.metadata()?.len();
        let table = header_of_file(&mut file, file_len)?;
        ensure!(
            table.rows == m.vocab_size && table.cols == m.dim,
            "table is {}x{}, the manifest says {}x{}",
            table.rows,
            table.cols,
            m.vocab_size,
            m.dim
        );
        let mut model = StaticModel {
            id: m.id,
            dim: m.dim,
            wp: WordPiece { vocab, unk: m.unk, max_chars: m.max_chars },
            table,
            store: Store::File(file),
        };
        if load_all {
            model.store = Store::Loaded(model.all_rows()?);
        }
        Ok(model)
    }

    /// A model whose table and vocabulary are in memory for the life of the process (the table
    /// bundled in the binary).
    pub fn from_static(
        id: &str,
        vocab_txt: &str,
        unk: u32,
        max_chars: usize,
        table: &'static [u8],
    ) -> Result<StaticModel> {
        let t = header_of_bytes(table)?;
        let vocab = parse_vocab(vocab_txt, t.rows)?;
        ensure!((unk as usize) < t.rows, "unk id out of range");
        Ok(StaticModel {
            id: id.to_string(),
            dim: t.cols,
            wp: WordPiece { vocab, unk, max_chars },
            table: t,
            store: Store::Static(table),
        })
    }

    /// Read the whole table into memory.
    pub fn into_loaded(mut self) -> Result<StaticModel> {
        if !matches!(self.store, Store::Loaded(_)) {
            self.store = Store::Loaded(self.all_rows()?);
        }
        Ok(self)
    }

    fn all_rows(&self) -> Result<Vec<f32>> {
        let (r, d) = (self.table.rows, self.dim);
        let mut v = vec![0f32; r * d];
        for (i, row) in v.chunks_mut(d).enumerate() {
            self.row(i as u32, row)?;
        }
        Ok(v)
    }

    pub fn vocab_size(&self) -> usize {
        self.table.rows
    }

    /// Whether the table is int8 (the bundled model).
    pub fn quantised(&self) -> bool {
        self.table.dtype == Dtype::I8
    }

    fn row(&self, id: u32, out: &mut [f32]) -> Result<()> {
        let t = &self.table;
        let d = self.dim;
        ensure!((id as usize) < t.rows, "token id out of range");
        let len = d as u64 * t.dtype.size();
        let at = t.offset + id as u64 * len;
        let scale_at = t.scales.map(|s| s + id as u64 * 4);
        match &self.store {
            Store::Loaded(v) => out.copy_from_slice(&v[id as usize * d..(id as usize + 1) * d]),
            Store::Static(b) => {
                let scale = match scale_at {
                    Some(s) => f32::from_le_bytes(b[s as usize..s as usize + 4].try_into()?),
                    None => 1.0,
                };
                decode(t.dtype, &b[at as usize..(at + len) as usize], scale, out);
            }
            Store::File(file) => {
                let mut buf = vec![0u8; len as usize];
                read_at(file, &mut buf, at)?;
                let scale = match scale_at {
                    Some(s) => {
                        let mut sb = [0u8; 4];
                        read_at(file, &mut sb, s)?;
                        f32::from_le_bytes(sb)
                    }
                    None => 1.0,
                };
                decode(t.dtype, &buf, scale, out);
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

/// A checked tokenizer: the vocabulary as `vocab.txt` text, the unknown token's id, and the
/// longest word WordPiece splits.
pub struct Vocab {
    pub text: String,
    pub unk: u32,
    pub max_chars: usize,
}

/// Read a BERT-uncased WordPiece `tokenizer.json` (the only kind the in-process tokenizer
/// implements).
pub fn vocab_of(tokenizer_json: &[u8]) -> Result<Vocab> {
    let tok: TokJson = serde_json::from_slice(tokenizer_json)?;
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
    let text: String = by_id.iter().map(|t| t.unwrap_or_default()).collect::<Vec<_>>().join("\n");
    let unk = *tok.model.vocab.get(&tok.model.unk_token).context("unk token missing")?;
    Ok(Vocab { text, unk, max_chars: tok.model.max_input_chars_per_word })
}

/// Where a pinned file is in `src`: `src/<name>`, or where the model's npm package keeps it
/// (`src/<path in the tarball without package/>`), so an unpacked npm package installs as is.
fn source_file(spec: &Spec, src: &Path, name: &str) -> PathBuf {
    let flat = src.join(name);
    if flat.exists() {
        return flat;
    }
    spec.npm
        .paths
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, p)| {
            let rel = p.strip_prefix("package/").unwrap_or(p);
            let inside = src.join(rel);
            if inside.exists() {
                inside
            } else {
                src.join(p)
            }
        })
        .unwrap_or(flat)
}

/// Check every pinned file of `spec` in `src` (size, then SHA-256).
pub fn verify(spec: &Spec, src: &Path) -> Result<()> {
    for f in spec.files {
        let p = source_file(spec, src, f.name);
        let len = std::fs::metadata(&p).with_context(|| format!("{} is missing", p.display()))?.len();
        ensure!(len == f.bytes, "{} is {len} bytes; the pinned file is {}", f.name, f.bytes);
        let got = sha256_file(&p)?;
        ensure!(got == f.sha256, "{}: sha256 {got} does not match the pinned {}", f.name, f.sha256);
    }
    Ok(())
}

/// Verify the pinned upstream files of potion-base-8M in `src` and install them under
/// `models_root`.
pub fn install(src: &Path, models_root: &Path) -> Result<Manifest> {
    install_spec(&BASE_8M, src, models_root)
}

/// Verify a model's pinned files in `src` (a directory of the upstream files, or an unpacked npm
/// package) and install them under `models_root/<key>`. The new directory is written beside the
/// old one and renamed into place.
pub fn install_spec(spec: &Spec, src: &Path, models_root: &Path) -> Result<Manifest> {
    verify(spec, src)?;
    let v = vocab_of(&std::fs::read(source_file(spec, src, "tokenizer.json"))?)?;
    let table = std::fs::read(source_file(spec, src, "model.safetensors"))?;
    let t = header_of_bytes(&table)?;
    if spec.files.iter().any(|f| f.name == "config.json") {
        let cfg: serde_json::Value = serde_json::from_slice(&std::fs::read(source_file(spec, src, "config.json"))?)?;
        ensure!(cfg["hidden_dim"].as_u64() == Some(t.cols as u64), "config.json's hidden_dim is not the table's");
        ensure!(cfg["normalize"] == true, "model does not normalise");
    }
    write_installed(&models_root.join(spec.key), &spec.id(), t.cols, v.unk, v.max_chars, &v.text, &table)
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

/// Quantise an f32 or f16 table to int8 with one f32 scale per row (`max |x| / 127`, values
/// rounded half away from zero): a quarter of f32's size. Deterministic: the same upstream file
/// gives the same bytes on every machine, which is what [`BUNDLED_TABLE_SHA256`] pins.
pub fn derive_i8(safetensors: &[u8]) -> Result<Vec<u8>> {
    let t = header_of_bytes(safetensors)?;
    ensure!(t.dtype != Dtype::I8, "already int8");
    let (r, c) = (t.rows, t.cols);
    let len = c * t.dtype.size() as usize;
    let mut q = Vec::with_capacity(r * c);
    let mut scales = Vec::with_capacity(r * 4);
    let mut row = vec![0f32; c];
    for i in 0..r {
        let at = t.offset as usize + i * len;
        decode(t.dtype, &safetensors[at..at + len], 1.0, &mut row);
        let max = row.iter().fold(0f32, |m, x| m.max(x.abs()));
        let scale = if max > 0.0 { max / 127.0 } else { 1.0 };
        for x in &row {
            q.push(((x / scale).round().clamp(-127.0, 127.0) as i8) as u8);
        }
        scales.extend_from_slice(&scale.to_le_bytes());
    }
    let header = format!(
        r#"{{"embeddings":{{"dtype":"I8","shape":[{r},{c}],"data_offsets":[0,{}]}},"scales":{{"dtype":"F32","shape":[{r}],"data_offsets":[{},{}]}}}}"#,
        r * c,
        r * c,
        r * c + r * 4
    );
    let mut out = Vec::with_capacity(8 + header.len() + q.len() + scales.len());
    out.extend_from_slice(&(header.len() as u64).to_le_bytes());
    out.extend_from_slice(header.as_bytes());
    out.extend_from_slice(&q);
    out.extend_from_slice(&scales);
    Ok(out)
}

/// Installed-model status for `adage models status`: `Ok(manifest)` or why it cannot be used.
pub fn status(models_root: &Path) -> Result<Manifest> {
    status_of(&dir_in(models_root))
}

/// The status of one installed model directory, every digest checked.
pub fn status_of(dir: &Path) -> Result<Manifest> {
    if !dir.join("manifest.json").exists() {
        bail!("not installed (run `adage models fetch`)");
    }
    let m: Manifest = serde_json::from_slice(&read_limited(&dir.join("manifest.json"), 64 << 10)?)?;
    for (name, e) in &m.files {
        let got = sha256_file(&dir.join(name))?;
        ensure!(got == e.sha256, "{name} does not match its recorded digest; reinstall");
    }
    StaticModel::open(dir)?;
    Ok(m)
}

// ---------------------------------------------------------------------------------------------
// The bundled model.
// ---------------------------------------------------------------------------------------------

/// The spec of the model bundled in the binary.
pub const BUNDLED_SPEC: &Spec = &BASE_8M;
/// SHA-256 of the bundled int8 table ([`derive_i8`] of the pinned `model.safetensors`).
pub const BUNDLED_TABLE_SHA256: &str = "b9c0e329a2ce282fc66f2e5ffd0ebcca701abff1840af8f397371e948a18ed05";
/// SHA-256 of the bundled vocabulary ([`vocab_of`] of the pinned `tokenizer.json`).
pub const BUNDLED_VOCAB_SHA256: &str = "7039169b5d11e3b0a132984a485013b5d4a686d3179e78a992066b16bf9caa23";

#[cfg(feature = "bundled")]
mod bundled {
    pub static TABLE: &[u8] = include_bytes!("../bundled/potion-base-8m-i8.safetensors");
    pub static VOCAB: &str = include_str!("../bundled/potion-base-8m.vocab.txt");
    pub const UNK: u32 = 1;
    pub const MAX_CHARS: usize = 100;
}

/// The bundled model's id: the upstream id marked `+i8` (its vectors differ from the f32 model's
/// in the fourth decimal, so what one learned is not matched against the other).
pub fn bundled_id() -> Option<String> {
    cfg!(feature = "bundled").then(|| format!("{}+i8", BUNDLED_SPEC.id()))
}

/// The bundled bytes: (table, vocabulary), when this build has them.
pub fn bundled_bytes() -> Option<(&'static [u8], &'static str)> {
    #[cfg(feature = "bundled")]
    {
        Some((bundled::TABLE, bundled::VOCAB))
    }
    #[cfg(not(feature = "bundled"))]
    {
        None
    }
}

/// The model bundled in the binary, if this build has one.
pub fn bundled() -> Option<StaticModel> {
    #[cfg(feature = "bundled")]
    {
        StaticModel::from_static(&bundled_id()?, bundled::VOCAB, bundled::UNK, bundled::MAX_CHARS, bundled::TABLE).ok()
    }
    #[cfg(not(feature = "bundled"))]
    {
        None
    }
}

/// Check the bundled bytes against their pins (`adage doctor`; a test in CI).
pub fn bundled_verified() -> Result<()> {
    let (t, v) = bundled_bytes().context("this build has no bundled model")?;
    ensure!(sha256_bytes(t) == BUNDLED_TABLE_SHA256, "the bundled table does not match its pinned SHA-256");
    ensure!(sha256_bytes(v.as_bytes()) == BUNDLED_VOCAB_SHA256, "the bundled vocabulary does not match its pin");
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// The hashed fallback, and choosing what to embed with.
// ---------------------------------------------------------------------------------------------

/// Dimensions of the hashed embedding.
pub const HASHED_DIM: usize = 256;
/// Its id. Changing the features changes every vector: bump the version.
pub const HASHED_ID: &str = "hashed-256@1";

/// Signed feature hashing of an already-normalised text: words (1.0), word bigrams (0.5) and the
/// character 4-grams of each word (0.25, so `hydrate` and `hydration` overlap), each feature's
/// index and sign from its SHA-256. Unit length; `None` when there is no word.
pub fn hashed_embed(normalized: &str) -> Option<Vec<f32>> {
    let words: Vec<String> = crate::tokenize::pre_tokenize(normalized)
        .into_iter()
        .filter(|w| w.chars().any(|c| c.is_alphanumeric()))
        .take(MAX_TOKENS)
        .collect();
    if words.is_empty() {
        return None;
    }
    let mut v = vec![0f32; HASHED_DIM];
    let mut add = |f: &str, w: f32| {
        let h = Sha256::digest(f.as_bytes());
        let i = u32::from_le_bytes([h[0], h[1], h[2], h[3]]) as usize % HASHED_DIM;
        v[i] += if h[4] & 1 == 0 { w } else { -w };
    };
    for (i, w) in words.iter().enumerate() {
        add(&format!("w:{w}"), 1.0);
        if let Some(n) = words.get(i + 1) {
            add(&format!("b:{w} {n}"), 0.5);
        }
        let cs: Vec<char> = format!("<{w}>").chars().collect();
        for g in cs.windows(4) {
            add(&format!("c:{}", g.iter().collect::<String>()), 0.25);
        }
    }
    if v.iter().all(|x| *x == 0.0) {
        return None;
    }
    Some(crate::unit(v))
}

/// What a process embeds with: a static model, or the hashed fallback.
pub enum Embedder {
    Model(StaticModel),
    Hashed,
}

impl Embedder {
    pub fn id(&self) -> String {
        match self {
            Embedder::Model(m) => m.id.clone(),
            Embedder::Hashed => HASHED_ID.to_string(),
        }
    }

    pub fn model(&self) -> Option<&StaticModel> {
        match self {
            Embedder::Model(m) => Some(m),
            Embedder::Hashed => None,
        }
    }

    pub fn into_model(self) -> Option<StaticModel> {
        match self {
            Embedder::Model(m) => Some(m),
            Embedder::Hashed => None,
        }
    }

    /// Embed an already-normalised text; a model that knows no token of it falls back to nothing
    /// (never to hashing: vectors of two schemes are not comparable).
    pub fn embed(&self, normalized: &str) -> Option<Vec<f32>> {
        match self {
            Embedder::Model(m) => m.embed(normalized),
            Embedder::Hashed => hashed_embed(normalized),
        }
    }
}

/// Where the embedder came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Source {
    /// Installed under `$ADAGE_HOME/models/<key>`.
    Installed { dir: PathBuf },
    /// Bundled in the binary.
    Bundled,
    /// The hashed fallback, and why no model is in use.
    Hashed { why: String },
}

pub struct Resolved {
    pub embedder: Embedder,
    pub source: Source,
    /// What was asked for: `auto`, a key, or `hashed`.
    pub selected: String,
}

impl Resolved {
    /// One line for `adage doctor` and the status card.
    pub fn describe(&self) -> String {
        match &self.source {
            Source::Installed { dir } => format!("{} (installed in {})", self.embedder.id(), dir.display()),
            Source::Bundled => format!("{} (bundled in this binary)", self.embedder.id()),
            Source::Hashed { why } => format!("hashed fallback ({HASHED_ID}): {why}"),
        }
    }
}

/// The environment variable that selects the embedder (`auto`, a model key, `hashed`).
pub const SELECT_ENV: &str = "ADAGE_EMBED_MODEL";
/// The file under the models directory `adage models use` writes.
pub const ACTIVE_FILE: &str = "active";

/// What is selected: `ADAGE_EMBED_MODEL`, else `models/active`, else `auto`.
pub fn selection(models_root: &Path) -> String {
    let pick = |s: String| {
        let s = s.trim().to_lowercase();
        (!s.is_empty()).then_some(s)
    };
    std::env::var(SELECT_ENV)
        .ok()
        .and_then(pick)
        .or_else(|| std::fs::read_to_string(models_root.join(ACTIVE_FILE)).ok().and_then(pick))
        .unwrap_or_else(|| "auto".into())
}

/// Choose the embedder (see the module documentation). `load_all` reads an installed table into
/// memory (the learner); the hook reads rows on demand.
pub fn resolve(models_root: &Path, load_all: bool) -> Resolved {
    let selected = selection(models_root);
    let open = |dir: &Path| if load_all { StaticModel::load(dir) } else { StaticModel::open(dir) };
    let key = match selected.as_str() {
        "hashed" | "none" | "off" => {
            return Resolved {
                embedder: Embedder::Hashed,
                source: Source::Hashed { why: format!("selected ({SELECT_ENV} or `adage models use hashed`)") },
                selected,
            }
        }
        "auto" => BASE_8M.key,
        k => match spec(k) {
            Some(s) => s.key,
            None => {
                return Resolved {
                    embedder: Embedder::Hashed,
                    source: Source::Hashed { why: format!("unknown model {k:?} selected") },
                    selected,
                }
            }
        },
    };
    let dir = models_root.join(key);
    let installed_err = match open(&dir) {
        Ok(m) => return Resolved { embedder: Embedder::Model(m), source: Source::Installed { dir }, selected },
        Err(e) if dir.join("manifest.json").exists() => Some(format!("{key} is installed but unusable ({e:#})")),
        Err(_) => None,
    };
    if key == BUNDLED_SPEC.key {
        if let Some(m) = bundled() {
            let m = if load_all { m.into_loaded().ok() } else { Some(m) };
            if let Some(m) = m {
                return Resolved { embedder: Embedder::Model(m), source: Source::Bundled, selected };
            }
        }
    }
    let why = installed_err.unwrap_or_else(|| {
        if key == BUNDLED_SPEC.key {
            format!("no model installed and none bundled in this build; `adage models fetch` installs {key}")
        } else {
            format!("{key} is selected but not installed; `adage models fetch {key}` installs it")
        }
    });
    Resolved { embedder: Embedder::Hashed, source: Source::Hashed { why }, selected }
}

/// The static model [`resolve`] picks, if any: what intent families are learned and matched with.
pub fn resolve_static(models_root: &Path, load_all: bool) -> Option<StaticModel> {
    resolve(models_root, load_all).embedder.into_model()
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
    fn int8_tables_embed_like_their_source() {
        let d = tempfile::tempdir().unwrap();
        let m = fixture(&d.path().join("m"));
        let src = std::fs::read(d.path().join("m/model.safetensors")).unwrap();
        let q = derive_i8(&src).unwrap();
        assert_eq!(derive_i8(&src).unwrap(), q, "deterministic");
        let vocab = std::fs::read_to_string(d.path().join("m/vocab.txt")).unwrap();
        let q: &'static [u8] = Box::leak(q.into_boxed_slice());
        let s = StaticModel::from_static("fixture+i8", &vocab, 0, 100, q).unwrap();
        assert!(s.quantised());
        for text in ["fix the failing tests", "write the readme docs"] {
            let (a, b) = (m.embed(text).unwrap(), s.embed(text).unwrap());
            assert!(crate::cosine(&a, &b) > 0.9999, "{text}");
        }
        // Installed int8 tables read the same from a file.
        write_installed(&d.path().join("q"), "fixture+i8", 4, 0, 100, &vocab, q).unwrap();
        let f = StaticModel::open(&d.path().join("q")).unwrap();
        assert_eq!(f.embed("fix tests").unwrap(), s.embed("fix tests").unwrap());
    }

    #[test]
    fn f16_decodes_exactly() {
        assert_eq!(f16_to_f32(0x3c00), 1.0);
        assert_eq!(f16_to_f32(0xc000), -2.0);
        assert_eq!(f16_to_f32(0x0000), 0.0);
        assert_eq!(f16_to_f32(0x7bff), 65504.0);
        assert_eq!(f16_to_f32(0x0001), 2f32.powi(-24), "smallest subnormal");
        assert_eq!(f16_to_f32(0x0200), 2f32.powi(-15), "a subnormal");
        assert!(f16_to_f32(0x7c00).is_infinite());
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

    #[test]
    fn hashed_embedding_is_lexical_and_deterministic() {
        let a = hashed_embed("fix the hydration mismatch in runtime-core").unwrap();
        let b = hashed_embed("hydration mismatch: fix it").unwrap();
        let c = hashed_embed("update the changelog for the release").unwrap();
        assert_eq!(a.len(), HASHED_DIM);
        assert!(crate::cosine(&a, &b) > crate::cosine(&a, &c) + 0.2);
        assert_eq!(hashed_embed("fix the hydration mismatch in runtime-core").unwrap(), a);
        assert!(hashed_embed(" ,.;").is_none());
    }

    #[test]
    fn resolution_is_explicit() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join(ACTIVE_FILE), "hashed\n").unwrap();
        // The environment variable wins over the file; unset here (tests run in one process, so
        // only the file is exercised).
        if std::env::var_os(SELECT_ENV).is_none() {
            let r = resolve(d.path(), false);
            assert!(matches!(r.embedder, Embedder::Hashed));
            assert!(matches!(r.source, Source::Hashed { .. }));
            assert!(r.describe().starts_with("hashed fallback"));
            std::fs::write(d.path().join(ACTIVE_FILE), "potion-code-16m-v2").unwrap();
            let r = resolve(d.path(), false);
            assert!(r.describe().contains("adage models fetch potion-code-16m-v2"), "{}", r.describe());
            std::fs::remove_file(d.path().join(ACTIVE_FILE)).unwrap();
            let r = resolve(d.path(), false);
            match bundled() {
                Some(_) => assert_eq!(r.source, Source::Bundled),
                None => assert!(matches!(r.source, Source::Hashed { .. })),
            }
        }
    }

    #[test]
    fn specs_are_pinned() {
        for s in SPECS {
            assert_eq!(s.files[0].name, "model.safetensors");
            assert!(s.files.iter().all(|f| f.sha256.len() == 64));
            assert_eq!(s.licence, "MIT");
            assert!(s.npm.paths.iter().all(|(n, _)| s.files.iter().any(|f| f.name == *n)));
        }
        assert_eq!(pinned_id(), "potion-base-8m@f65d0f325faa");
        assert_eq!(spec("potion-code-16m").unwrap().key, "potion-code-16m-v2");
    }

    #[cfg(feature = "bundled")]
    #[test]
    fn the_bundled_model_matches_its_pins() {
        bundled_verified().unwrap();
        let m = bundled().unwrap();
        assert_eq!(m.id, format!("{}+i8", pinned_id()));
        assert_eq!(m.vocab_size(), 29_528);
        assert_eq!(m.dim, 256);
        let a = m.embed("fix the failing tests in the parser").unwrap();
        let b = m.embed("the parser tests fail, please fix").unwrap();
        let c = m.embed("write release notes for the changelog").unwrap();
        assert!(crate::cosine(&a, &b) > crate::cosine(&a, &c));
        assert!(known_id(&m.id));
    }
}
