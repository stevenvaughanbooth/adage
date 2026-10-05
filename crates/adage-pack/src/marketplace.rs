//! Marketplaces (format `adage-marketplace/1`): the packs a repository offers, each pinned by its
//! digest, as a Claude Code marketplace lists plugins (`docs/marketplace.md`).
//!
//! A marketplace is a repository with [`MANIFEST`] at its root:
//!
//! ```toml
//! format = "adage-marketplace/1"
//!
//! [marketplace]
//! name = "adage"
//! repository = "github.com/stevenvaughanbooth/adage"   # its home: where it is fetched from
//! description = "Adage's base instincts"
//!
//! [[pack]]
//! name = "adage/python-env"     # the pack's own name (pack.toml)
//! source = "packs/python-env"   # its directory, relative to the repository's root
//! version = "0.1.0"             # its version (pack.toml)
//! digest = "…"                  # its pack digest: exactly these bytes
//! default = "detected"          # detected: binds wherever it applies; off: waits to be turned on
//! ```
//!
//! The pin is the trust: a pack a trusted marketplace pins needs no signature of its own, and other
//! bytes under its name are not that pack. Adage's own marketplace is the Adage repository's, built
//! into the binary ([`crate::builtin`]). A marketplace read and verified ([`Loaded`]) is synced to
//! this machine the one way every marketplace is ([`Loaded::sync`]): its packs installed where
//! `adage add` installs a pack, each recorded as from it, and the ones it no longer lists removed.

use crate::install::{self, Record, Via};
use crate::validate::{people_text_ok, safe_pack_name, safe_path, safe_repo_key, semver_ok, sha256_hex_ok};
use crate::{pack_digest, Pack};
use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// The `format` of every marketplace manifest this build reads.
pub const FORMAT: &str = "adage-marketplace/1";
/// Where a repository keeps its marketplace manifest.
pub const MANIFEST: &str = ".adage/marketplace.toml";
/// Most packs one marketplace lists.
pub const MAX_PACKS: usize = 256;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Marketplace {
    /// Always [`FORMAT`].
    pub format: String,
    pub marketplace: Info,
    #[serde(default, rename = "pack")]
    pub packs: Vec<Entry>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Info {
    /// How people name it (`adage`): lowercase letters, digits and `-`.
    pub name: String,
    /// Its home: the repository it lives in, as a repository key (`github.com/owner/repo`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,
    /// For people only.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
}

/// One pack the marketplace offers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    /// The pack's name, as its `pack.toml` gives it.
    pub name: String,
    /// The pack's directory, relative to the repository's root.
    pub source: String,
    /// The pack's version, as its `pack.toml` gives it.
    pub version: String,
    /// The pack digest of its files ([`crate::pack_digest`]).
    pub digest: String,
    /// What a first run does with it.
    #[serde(default)]
    pub default: Initially,
}

/// What a first run does with a marketplace's pack.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Initially {
    /// Binds wherever its detectors find what it needs (an instinct pack), until turned off.
    #[default]
    Detected,
    /// Installed, but bound only once a person turns it on.
    Off,
}

impl Marketplace {
    /// Parse a manifest strictly (unknown fields refused) and check it.
    pub fn parse(text: &str) -> Result<Marketplace> {
        let m: Marketplace = toml::from_str(text).with_context(|| format!("{MANIFEST} is not a valid manifest"))?;
        m.check()?;
        Ok(m)
    }

    fn check(&self) -> Result<()> {
        if self.format != FORMAT {
            bail!("{MANIFEST}: format {:?} is not {FORMAT:?}", self.format);
        }
        let n = &self.marketplace.name;
        let name_ok = !n.is_empty()
            && n.len() <= 32
            && n.as_bytes()[0].is_ascii_alphanumeric()
            && n.bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-');
        if !name_ok {
            bail!("{MANIFEST}: marketplace name {n:?} is not allowed (lowercase letters, digits and -)");
        }
        if let Some(r) = self.marketplace.repository.as_deref().filter(|r| !safe_repo_key(r) || r.contains('@')) {
            bail!("{MANIFEST}: repository {r:?} is not a repository key such as github.com/owner/repo");
        }
        if !people_text_ok(&self.marketplace.description, 1000) {
            bail!("{MANIFEST}: the description contains characters that may not be shown");
        }
        if self.packs.is_empty() || self.packs.len() > MAX_PACKS {
            bail!("{MANIFEST}: a marketplace lists 1..={MAX_PACKS} packs");
        }
        let (mut names, mut sources) = (BTreeSet::new(), BTreeSet::new());
        for e in &self.packs {
            if !safe_pack_name(&e.name) {
                bail!("{MANIFEST}: pack name {:?} is not allowed (owner/name, lowercase)", e.name);
            }
            if !safe_path(&e.source) {
                bail!("{MANIFEST}: {}: source {:?} is not a repository-relative directory", e.name, e.source);
            }
            if !semver_ok(&e.version) {
                bail!("{MANIFEST}: {}: version {:?} is not semver", e.name, e.version);
            }
            if !sha256_hex_ok(&e.digest) {
                bail!("{MANIFEST}: {}: digest {:?} is not a lowercase hex SHA-256", e.name, e.digest);
            }
            if !names.insert(&e.name) {
                bail!("{MANIFEST}: {} is listed twice", e.name);
            }
            if !sources.insert(&e.source) {
                bail!("{MANIFEST}: source {} is listed twice", e.source);
            }
        }
        Ok(())
    }

    /// The entry for a pack name.
    pub fn get(&self, name: &str) -> Option<&Entry> {
        self.packs.iter().find(|e| e.name == name)
    }

    /// Every pack the marketplace lists, read from `files` (repository-relative paths → bytes) and
    /// checked against its entry: valid as any pack, with the entry's name and version, and the
    /// pinned digest. The first that fails refuses the marketplace.
    pub fn load(&self, files: &BTreeMap<String, Vec<u8>>) -> Result<Vec<(Entry, Pack)>> {
        let mut out = Vec::new();
        for e in &self.packs {
            let own = pack_files(files, &e.source);
            if own.is_empty() {
                bail!("{}: {} has no files", e.name, e.source);
            }
            let pack =
                Pack::from_files(&own).with_context(|| format!("{} ({}) is not a valid pack", e.name, e.source))?;
            check_entry(e, &pack, &pack_digest(&own))?;
            out.push((e.clone(), pack));
        }
        Ok(out)
    }
}

/// The files under `source/` in `files`, relative to it.
pub fn pack_files(files: &BTreeMap<String, Vec<u8>>, source: &str) -> BTreeMap<String, Vec<u8>> {
    let prefix = format!("{source}/");
    files.iter().filter_map(|(rel, b)| rel.strip_prefix(&prefix).map(|r| (r.to_string(), b.clone()))).collect()
}

/// A pack is the one its entry pins: its name, its version and its digest.
pub fn check_entry(e: &Entry, pack: &Pack, digest: &str) -> Result<()> {
    let m = &pack.manifest.pack;
    if m.name != e.name || m.version != e.version {
        bail!("{} lists {}@{}, but its pack.toml says {}@{}", MANIFEST, e.name, e.version, m.name, m.version);
    }
    if digest != e.digest {
        bail!(
            "{}@{} has digest {digest}, not the {} its marketplace pins: it changed since it was pinned (bump its version, then pin the new digest)",
            e.name,
            e.version,
            e.digest
        );
    }
    Ok(())
}

/// A marketplace in a directory (a checkout): its manifest, and every pack it lists, each read
/// strictly from its source directory and checked against its pin.
pub fn load_dir(root: &Path) -> Result<Loaded> {
    let text = std::fs::read_to_string(root.join(MANIFEST))
        .with_context(|| format!("{} has no {MANIFEST}", root.display()))?;
    let m = Marketplace::parse(&text)?;
    let mut files = BTreeMap::from([(MANIFEST.to_string(), text.into_bytes())]);
    for e in &m.packs {
        for (rel, b) in crate::validate::read_dir_files(&root.join(&e.source))? {
            files.insert(format!("{}/{rel}", e.source), b);
        }
    }
    Loaded::from_files(files)
}

/// A marketplace read and verified: its manifest, and every pack it lists, each the one it pins.
#[derive(Debug, Clone)]
pub struct Loaded {
    pub manifest: Marketplace,
    /// In the manifest's order.
    pub packs: Vec<(Entry, Pack)>,
    files: BTreeMap<String, Vec<u8>>,
}

/// A marketplace's pack on this machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Installed {
    /// Not installed yet: the next sync installs it.
    No,
    /// Installed at exactly the bytes the marketplace pins.
    Current,
    /// Installed at other bytes (an older version, or damaged): the next sync replaces it.
    Differs,
}

/// What a sync changed, by pack name.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Synced {
    pub installed: Vec<String>,
    pub updated: Vec<String>,
    pub removed: Vec<String>,
}

impl Synced {
    pub fn is_empty(&self) -> bool {
        self.installed.is_empty() && self.updated.is_empty() && self.removed.is_empty()
    }
}

impl Loaded {
    /// A marketplace from its files (repository-relative paths → bytes): [`MANIFEST`] and the
    /// files of every pack it lists, each verified against its pin ([`Marketplace::load`]).
    pub fn from_files(files: BTreeMap<String, Vec<u8>>) -> Result<Loaded> {
        let text = files.get(MANIFEST).ok_or_else(|| anyhow!("the marketplace has no {MANIFEST}"))?;
        let manifest = Marketplace::parse(std::str::from_utf8(text).context("the manifest is not UTF-8")?)?;
        let packs = manifest.load(&files)?;
        Ok(Loaded { manifest, packs, files })
    }

    /// Its name (`adage`).
    pub fn name(&self) -> &str {
        &self.manifest.marketplace.name
    }

    /// Its manifest's text, exactly as published.
    pub fn manifest_text(&self) -> String {
        String::from_utf8_lossy(self.files.get(MANIFEST).map(Vec::as_slice).unwrap_or_default()).into_owned()
    }

    /// A pack it lists, by name.
    pub fn get(&self, name: &str) -> Option<&(Entry, Pack)> {
        self.packs.iter().find(|(e, _)| e.name == name)
    }

    /// A pack's files, relative to the pack (as a pack directory holds them).
    pub fn files(&self, name: &str) -> Option<BTreeMap<String, Vec<u8>>> {
        self.get(name).map(|(e, _)| pack_files(&self.files, &e.source))
    }

    /// Each pack it lists, and how it stands on this machine.
    pub fn status(&self, tracks_root: &Path) -> Vec<(&Entry, Installed)> {
        self.packs
            .iter()
            .map(|(e, _)| {
                let dir = install::pack_dir(tracks_root, &e.name);
                let state = match crate::digest_dir(&dir) {
                    Ok(d) if d == e.digest => Installed::Current,
                    Ok(_) => Installed::Differs,
                    Err(_) if dir.exists() => Installed::Differs,
                    Err(_) => Installed::No,
                };
                (e, state)
            })
            .collect()
    }

    /// Sync this marketplace to the machine whose tracks are at `tracks_root`: every pack it
    /// lists that is not installed at its pinned digest is written as `adage add` writes a pack
    /// ([`install::write_verified`]) and recorded as from this marketplace, and every pack recorded
    /// from it that it no longer lists is uninstalled. The caller holds the tracks lock.
    pub fn sync(&self, tracks_root: &Path) -> Result<Synced> {
        let mut out = Synced::default();
        let mut records = install::records(tracks_root);
        let before = records.clone();
        let me = self.name();
        for (e, state) in self.status(tracks_root) {
            if state != Installed::Current {
                let files = self.files(&e.name).ok_or_else(|| anyhow!("{} has no files", e.name))?;
                install::write_verified(&files, &e.digest, &install::pack_dir(tracks_root, &e.name))?;
                match state {
                    Installed::No => out.installed.push(e.name.clone()),
                    _ => out.updated.push(e.name.clone()),
                }
            }
            let via = Via::Marketplace { name: me.to_string() };
            records.insert(e.name.clone(), Record { version: e.version.clone(), digest: e.digest.clone(), via });
        }
        let delisted: Vec<String> = records
            .iter()
            .filter(|(n, r)| matches!(&r.via, Via::Marketplace { name } if name == me) && self.get(n).is_none())
            .map(|(n, _)| n.clone())
            .collect();
        for n in delisted {
            install::uninstall(tracks_root, &n)?;
            records.remove(&n);
            out.removed.push(n);
        }
        if records != before {
            install::save_records(tracks_root, &records)?;
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(extra: &str) -> String {
        format!(
            "format = \"adage-marketplace/1\"\n\n[marketplace]\nname = \"acme\"\n\n[[pack]]\nname = \"acme/lint\"\nsource = \"packs/lint\"\nversion = \"1.0.0\"\ndigest = \"{}\"\n{extra}",
            "a".repeat(64)
        )
    }

    #[test]
    fn a_manifest_is_read_strictly() {
        let m = Marketplace::parse(&manifest("")).unwrap();
        assert_eq!(m.packs[0].default, Initially::Detected);
        assert_eq!(Marketplace::parse(&manifest("default = \"off\"\n")).unwrap().packs[0].default, Initially::Off);
        for bad in [
            manifest("homepage = \"https://example.com\"\n"),
            manifest("default = \"on\"\n"),
            manifest("").replace("packs/lint", "/etc/lint"),
            manifest("").replace("packs/lint", "../lint"),
            manifest("").replace("acme/lint", "Acme/Lint"),
            manifest("").replace("1.0.0", "1.0"),
            manifest("").replace(&"a".repeat(64), "abc"),
            manifest("").replace("adage-marketplace/1", "adage-marketplace/2"),
            manifest("[[pack]]\nname = \"acme/lint\"\nsource = \"packs/other\"\nversion = \"1.0.0\"\n")
                + &format!("digest = \"{}\"\n", "b".repeat(64)),
        ] {
            assert!(Marketplace::parse(&bad).is_err(), "{bad}");
        }
    }
}
