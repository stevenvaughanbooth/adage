//! Canonical digest of a track directory.
//!
//! Spec (see FINDINGS.md "Canonical-digest spec" for the prose version):
//!
//! 1. Walk the track directory recursively, following no symlinks.
//! 2. Exclude `track.sig` itself (the signature covers everything else) and any
//!    dotfiles at the root (`.git`, `.DS_Store`, ...).
//! 3. For every remaining regular file, read its raw bytes.
//!    - Files under `scripts/` are hashed **verbatim** (they may be binaries or
//!      have semantically meaningful CR bytes).
//!    - Every other file has line endings normalised first: `\r\n` and lone `\r`
//!      become `\n`. Tracks are declared plain text (Design Spec §10.1) so this
//!      is safe, and it makes the digest immune to a git `core.autocrlf` checkout
//!      difference between machines.
//! 4. Hash each (normalised) file with SHA-256 -> lowercase hex.
//! 5. Build a manifest: one line per file, `"<sha256_hex>  <posix_relative_path>\n"`,
//!    sorted by relative path in byte order. Sorting makes the digest independent
//!    of directory-enumeration order or the order files were added on disk.
//! 6. The canonical digest is SHA-256 of the manifest bytes (32 raw bytes).

use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub struct FileEntry {
    pub rel_path: String,
    pub sha256_hex: String,
}

pub struct TrackDigest {
    pub manifest_text: String,
    pub digest: [u8; 32],
    pub files: Vec<FileEntry>,
}

fn normalise_line_endings(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\r' => {
                out.push(b'\n');
                if i + 1 < bytes.len() && bytes[i + 1] == b'\n' {
                    i += 1; // swallow the \n half of \r\n
                }
            }
            b => out.push(b),
        }
        i += 1;
    }
    out
}

fn is_verbatim(rel_path: &str) -> bool {
    rel_path == "scripts" || rel_path.starts_with("scripts/")
}

/// Compute the canonical digest of a track directory rooted at `root`.
pub fn compute(root: &Path) -> Result<TrackDigest> {
    if !root.is_dir() {
        bail!("track path {:?} is not a directory", root);
    }

    let mut entries: Vec<(PathBuf, String)> = Vec::new();

    for entry in WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if !entry.file_type().is_file() {
            continue;
        }
        let rel = path
            .strip_prefix(root)
            .expect("walkdir always yields children of root");
        let rel_str = rel.to_string_lossy().replace('\\', "/");

        // Exclusions: the signature file itself, and dotfiles anywhere in the tree
        // (.git, .DS_Store, editor swap files).
        if rel_str == "track.sig" {
            continue;
        }
        if rel
            .components()
            .any(|c| c.as_os_str().to_string_lossy().starts_with('.'))
        {
            continue;
        }

        entries.push((path.to_path_buf(), rel_str));
    }

    entries.sort_by(|a, b| a.1.cmp(&b.1));

    let mut files = Vec::with_capacity(entries.len());
    let mut manifest = String::new();

    for (abs_path, rel_str) in &entries {
        let raw = std::fs::read(abs_path)
            .with_context(|| format!("reading track file {:?}", abs_path))?;
        let normalised = if is_verbatim(rel_str) {
            raw
        } else {
            normalise_line_endings(&raw)
        };
        let sha256_hex = hex::encode(Sha256::digest(&normalised));
        manifest.push_str(&sha256_hex);
        manifest.push_str("  ");
        manifest.push_str(rel_str);
        manifest.push('\n');
        files.push(FileEntry {
            rel_path: rel_str.clone(),
            sha256_hex,
        });
    }

    let digest: [u8; 32] = Sha256::digest(manifest.as_bytes()).into();

    Ok(TrackDigest {
        manifest_text: manifest,
        digest,
        files,
    })
}
