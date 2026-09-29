//! Reading a model's files out of its npm package tarball (`.tgz`: gzip over ustar), in process.
//!
//! The npm registry is the fallback source of `adage models fetch` where Hugging Face is blocked
//! ([`crate::model::Npm`]). Only the files a spec names are extracted, each bounded by its pinned
//! size; the caller then checks every SHA-256 pin, so the tarball is never trusted for content.

use crate::model::Spec;
use anyhow::{bail, ensure, Context, Result};
use std::path::Path;

/// The deflate stream of a gzip member (RFC 1952 header skipped).
fn gzip_body(b: &[u8]) -> Result<&[u8]> {
    ensure!(b.len() >= 18 && b[0] == 0x1f && b[1] == 0x8b && b[2] == 8, "not a gzip file");
    let flags = b[3];
    let mut at = 10usize;
    if flags & 4 != 0 {
        ensure!(b.len() >= at + 2, "truncated gzip header");
        at += 2 + u16::from_le_bytes([b[at], b[at + 1]]) as usize;
    }
    for bit in [8u8, 16] {
        if flags & bit != 0 {
            let nul = b.get(at..).and_then(|r| r.iter().position(|&c| c == 0)).context("truncated gzip header")?;
            at += nul + 1;
        }
    }
    if flags & 2 != 0 {
        at += 2;
    }
    ensure!(at < b.len(), "truncated gzip header");
    Ok(&b[at..])
}

/// Decompress a `.tgz` to its tar bytes, at most `limit` bytes.
pub fn gunzip(tgz: &[u8], limit: usize) -> Result<Vec<u8>> {
    miniz_oxide::inflate::decompress_to_vec_with_limit(gzip_body(tgz)?, limit)
        .map_err(|e| anyhow::anyhow!("gzip: {:?}", e.status))
}

fn field(h: &[u8]) -> String {
    let end = h.iter().position(|&c| c == 0).unwrap_or(h.len());
    String::from_utf8_lossy(&h[..end]).to_string()
}

fn octal(h: &[u8]) -> Result<u64> {
    let s = field(h);
    let s = s.trim();
    if s.is_empty() {
        return Ok(0);
    }
    u64::from_str_radix(s, 8).with_context(|| format!("bad tar size {s:?}"))
}

/// The regular files of a tar archive: (path, contents).
pub fn entries(tar: &[u8]) -> Result<Vec<(String, &[u8])>> {
    let mut out = Vec::new();
    let mut at = 0usize;
    while at + 512 <= tar.len() {
        let h = &tar[at..at + 512];
        if h.iter().all(|&c| c == 0) {
            break;
        }
        let size = octal(&h[124..136])? as usize;
        let kind = h[156];
        let name = field(&h[0..100]);
        let prefix = if &h[257..262] == b"ustar" { field(&h[345..500]) } else { String::new() };
        let path = if prefix.is_empty() { name } else { format!("{prefix}/{name}") };
        let body = at + 512;
        ensure!(body + size <= tar.len(), "tar entry {path} is truncated");
        if kind == b'0' || kind == 0 {
            out.push((path, &tar[body..body + size]));
        }
        at = body + size.div_ceil(512) * 512;
    }
    Ok(out)
}

/// Write the files `spec` needs from its npm tarball into `dir` (flat: `dir/<name>`). Sizes are
/// checked here; the caller checks the SHA-256 pins ([`crate::model::verify`]).
pub fn extract(spec: &Spec, tgz: &[u8], dir: &Path) -> Result<()> {
    let limit = (spec.npm.tarball_bytes as usize).saturating_mul(4).max(spec.bytes() as usize * 2);
    let tar = gunzip(tgz, limit)?;
    let files = entries(&tar)?;
    std::fs::create_dir_all(dir)?;
    for (name, path) in spec.npm.paths {
        let pinned = spec.files.iter().find(|f| f.name == *name).context("a path for an unpinned file")?;
        let Some((_, body)) = files.iter().find(|(p, _)| p == path) else {
            bail!("{} {} has no {path}", spec.npm.package, spec.npm.version);
        };
        ensure!(body.len() as u64 == pinned.bytes, "{path} is {} bytes; the pinned file is {}", body.len(), pinned.bytes);
        std::fs::write(dir.join(name), body)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tar_of(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut out = Vec::new();
        for (name, body) in files {
            let mut h = [0u8; 512];
            h[..name.len()].copy_from_slice(name.as_bytes());
            let size = format!("{:011o}\0", body.len());
            h[124..136].copy_from_slice(size.as_bytes());
            h[156] = b'0';
            h[257..263].copy_from_slice(b"ustar\0");
            out.extend_from_slice(&h);
            out.extend_from_slice(body);
            out.resize(out.len().div_ceil(512) * 512, 0);
        }
        out.extend_from_slice(&[0u8; 1024]);
        out
    }

    fn gzip_of(data: &[u8]) -> Vec<u8> {
        let mut out = vec![0x1f, 0x8b, 8, 8, 0, 0, 0, 0, 0, 3];
        out.extend_from_slice(b"x.tar\0");
        out.extend(miniz_oxide::deflate::compress_to_vec(data, 6));
        out.extend_from_slice(&[0u8; 8]);
        out
    }

    #[test]
    fn reads_files_out_of_a_tgz() {
        let tar = tar_of(&[("package/a.txt", b"hello"), ("package/models/b.bin", &[7u8; 700])]);
        let tgz = gzip_of(&tar);
        let back = gunzip(&tgz, 1 << 20).unwrap();
        assert_eq!(back, tar);
        let e = entries(&back).unwrap();
        assert_eq!(e.len(), 2);
        assert_eq!(e[0], ("package/a.txt".to_string(), &b"hello"[..]));
        assert_eq!(e[1].1.len(), 700);
        assert!(gunzip(b"not gzip at all, no", 100).is_err());
        assert!(gunzip(&tgz, 100).is_err(), "over the limit");
    }
}
