//! Installing a verified pack (Track C §2.2): where installed packs live, the one way their files
//! are written, and the record of where each came from. `adage add` writes a pack this way once its
//! signature (or a marketplace's pin) and its contents are verified, `adage train accept` once its
//! review holds, and a marketplace's sync ([`crate::marketplace::Loaded::sync`]) for every pack it
//! pins.
//!
//! ```text
//! $ADAGE_HOME/tracks/@imported/<owner>/<name>/    the pack, exactly as verified
//! $ADAGE_HOME/tracks/@imported/installed.json     where each installed pack came from (Record)
//! ```

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The install records, beside the installed packs.
pub const RECORDS: &str = "installed.json";

/// Where an installed pack lives under the tracks root.
pub fn pack_dir(tracks_root: &Path, name: &str) -> PathBuf {
    tracks_root.join(adage_tracks::index::IMPORTED_DIR).join(name)
}

/// Write a verified pack's `files` to `dest`, exactly: staged in a hidden directory beside it,
/// re-read and compared with `digest` (what was checked is what is installed), then renamed into
/// place, replacing what was there. The caller holds the tracks lock.
pub fn write_verified(files: &BTreeMap<String, Vec<u8>>, digest: &str, dest: &Path) -> Result<()> {
    let parent = dest.parent().context("an installed pack has a parent directory")?;
    let leaf = dest.file_name().context("an installed pack has a name")?.to_string_lossy();
    std::fs::create_dir_all(parent)?;
    let stage = parent.join(format!(".{leaf}.staging-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&stage);
    let staged = (|| -> Result<()> {
        for (rel, bytes) in files {
            let p = stage.join(rel);
            std::fs::create_dir_all(p.parent().context("a pack file has a parent directory")?)?;
            std::fs::write(p, bytes)?;
        }
        if crate::digest_dir(&stage)? != digest {
            bail!("the staged copy of the pack differs from what was verified");
        }
        if dest.exists() {
            std::fs::remove_dir_all(dest)?;
        }
        std::fs::rename(&stage, dest)?;
        Ok(())
    })();
    if staged.is_err() {
        let _ = std::fs::remove_dir_all(&stage);
    }
    staged
}

/// Where an installed pack came from, and the bytes it was installed at.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    pub version: String,
    /// Its pack digest when installed.
    pub digest: String,
    pub via: Via,
}

/// How an installed pack was trusted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Via {
    /// Pinned by a marketplace (`adage`: the Adage repository's, built into this binary).
    Marketplace { name: String },
    /// `adage add`: signed by this key (its fingerprint).
    Signature { signer: String },
    /// `adage train accept`: compiled from a skill and reviewed on this machine.
    Review,
}

/// Installed packs by name.
pub type Records = BTreeMap<String, Record>;

fn records_path(tracks_root: &Path) -> PathBuf {
    tracks_root.join(adage_tracks::index::IMPORTED_DIR).join(RECORDS)
}

/// The install records. None is an empty set; an unreadable file reads as none, and the next
/// install writes it again.
pub fn records(tracks_root: &Path) -> Records {
    std::fs::read(records_path(tracks_root)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

/// Replace the install records (atomically). The caller holds the tracks lock.
pub fn save_records(tracks_root: &Path, records: &Records) -> Result<()> {
    let p = records_path(tracks_root);
    std::fs::create_dir_all(p.parent().context("the install records have a parent directory")?)?;
    let tmp = p.with_extension(format!("tmp{}", std::process::id()));
    std::fs::write(&tmp, serde_json::to_vec_pretty(records)?)?;
    std::fs::rename(tmp, p)?;
    Ok(())
}

/// Record that `name` is installed, and how. The caller holds the tracks lock.
pub fn record(tracks_root: &Path, name: &str, record: Record) -> Result<()> {
    let mut all = records(tracks_root);
    if all.get(name) == Some(&record) {
        return Ok(());
    }
    all.insert(name.to_string(), record);
    save_records(tracks_root, &all)
}

/// Remove an installed pack: its files, its bound tracks (`@imported/@bound/<name>/`) and its
/// record. The caller holds the tracks lock and recompiles the index.
pub fn uninstall(tracks_root: &Path, name: &str) -> Result<()> {
    let imported = tracks_root.join(adage_tracks::index::IMPORTED_DIR);
    for dir in [imported.join(name), imported.join(adage_tracks::index::BOUND_DIR).join(name)] {
        if dir.exists() {
            std::fs::remove_dir_all(&dir).with_context(|| format!("removing {}", dir.display()))?;
        }
    }
    let mut all = records(tracks_root);
    if all.remove(name).is_some() {
        save_records(tracks_root, &all)?;
    }
    Ok(())
}
