//! Track directories on disk: `track.toml`, `routes/<id>.yaml`, validation, canonical digest.

use crate::model::*;
use anyhow::{bail, Context, Result};
use regex::Regex;
use sha2::{Digest, Sha256};
use std::path::Path;

pub fn write(dir: &Path, track: &Track) -> Result<()> {
    std::fs::create_dir_all(dir.join("routes"))?;
    // Remove routes that no longer exist so the directory mirrors the track exactly.
    for e in std::fs::read_dir(dir.join("routes"))? {
        let p = e?.path();
        if p.extension().is_some_and(|x| x == "yaml") {
            std::fs::remove_file(p)?;
        }
    }
    std::fs::write(dir.join("track.toml"), toml::to_string_pretty(&track.manifest)?)?;
    for r in &track.routes {
        std::fs::write(dir.join("routes").join(format!("{}.yaml", r.id)), serde_yaml::to_string(r)?)?;
    }
    Ok(())
}

pub fn read(dir: &Path) -> Result<Track> {
    let manifest: Manifest = toml::from_str(
        &std::fs::read_to_string(dir.join("track.toml")).with_context(|| format!("{}/track.toml", dir.display()))?,
    )?;
    let mut routes = Vec::new();
    let rdir = dir.join("routes");
    if rdir.exists() {
        let mut files: Vec<_> = std::fs::read_dir(&rdir)?.filter_map(|e| e.ok().map(|e| e.path())).collect();
        files.sort();
        for p in files {
            if p.extension().is_some_and(|x| x == "yaml") {
                let r: Route = serde_yaml::from_str(&std::fs::read_to_string(&p)?)
                    .with_context(|| format!("{}", p.display()))?;
                routes.push(r);
            }
        }
    }
    let t = Track { manifest, routes };
    validate(&t)?;
    Ok(t)
}

/// Read every track under `root` (one directory per track, possibly nested as `owner/name`).
pub fn read_all(root: &Path) -> Result<Vec<Track>> {
    let mut out = Vec::new();
    if !root.exists() {
        return Ok(out);
    }
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        if d.join("track.toml").exists() {
            out.push(read(&d)?);
            continue;
        }
        for e in std::fs::read_dir(&d)? {
            let p = e?.path();
            if p.is_dir() {
                stack.push(p);
            }
        }
    }
    out.sort_by(|a, b| a.manifest.track.name.cmp(&b.manifest.track.name));
    Ok(out)
}

pub fn slot_refs(command: &str) -> Vec<String> {
    let re = Regex::new(r"\{([a-z_][a-z0-9_]*)\}").unwrap();
    re.captures_iter(command).map(|c| c[1].to_string()).collect()
}

/// Structural and safety validation. A track that fails is refused at install and at index time.
pub fn validate(t: &Track) -> Result<()> {
    for r in &t.routes {
        let ctx = || format!("route {} in {}", r.id, t.manifest.track.name);
        let by_edit = matches!(r.trigger, Trigger::AfterEdit { .. });
        match ceiling(r.effect, by_edit) {
            None => bail!("{}: effect {:?} may never be directed", ctx(), r.effect),
            Some(max) if r.aspect == Aspect::Green && max != Aspect::Green => {
                bail!("{}: aspect green exceeds the ceiling for {:?}", ctx(), r.effect)
            }
            _ => {}
        }
        for s in &r.slots {
            if let Contract::Pattern { regex } = &s.contract {
                if !regex.starts_with('^') || !regex.ends_with('$') {
                    bail!("{}: slot {} contract must be anchored", ctx(), s.name);
                }
                Regex::new(regex).with_context(ctx)?;
            }
        }
        let declared = |n: &str| r.slots.iter().any(|s| s.name == n);
        match &r.step {
            Step::Run { command } => {
                for n in slot_refs(command) {
                    if !declared(&n) {
                        bail!("{}: command references undeclared slot {{{n}}}", ctx());
                    }
                }
                if r.aspect == Aspect::Red {
                    bail!("{}: a run step cannot be red", ctx());
                }
            }
            Step::ReadBatch { slot, .. } => {
                let s = r.slots.iter().find(|s| &s.name == slot);
                match s {
                    Some(s) if s.contract == Contract::RepoPath => {}
                    _ => bail!("{}: read_batch slot must be declared with a repo_path contract", ctx()),
                }
            }
            Step::Avoid { .. } => {
                if r.aspect != Aspect::Red {
                    bail!("{}: an avoid step must be red", ctx());
                }
            }
        }
        if let Trigger::AfterEdit { paths } = &r.trigger {
            glob::Pattern::new(paths).with_context(ctx)?;
        }
    }
    Ok(())
}

/// Canonical digest of a track directory (spike S6): every file, sorted by relative path, line
/// endings normalised, hashed; then the hash of that manifest. Signatures refer to this digest.
pub fn digest(dir: &Path) -> Result<String> {
    let mut files = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d)? {
            let p = e?.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.file_name().is_some_and(|n| n != "track.sig") {
                files.push(p);
            }
        }
    }
    let mut entries: Vec<(String, String)> = files
        .iter()
        .map(|p| {
            let rel = p.strip_prefix(dir).unwrap().to_string_lossy().replace('\\', "/");
            let bytes = std::fs::read(p)?;
            let norm: Vec<u8> = if rel.starts_with("scripts/") {
                bytes
            } else {
                String::from_utf8_lossy(&bytes).replace("\r\n", "\n").into_bytes()
            };
            Ok((rel, hex(&Sha256::digest(&norm))))
        })
        .collect::<Result<_>>()?;
    entries.sort();
    let manifest: String = entries.iter().map(|(p, h)| format!("{h}  {p}\n")).collect();
    Ok(hex(&Sha256::digest(manifest.as_bytes())))
}

pub fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}
