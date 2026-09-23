//! `adage add <dir> [--trust]` (Design §10.7, §15.2; review finding F9): verify a shared track's
//! signature against trusted signers, validate it, install it at probation, recompile the index.
//!
//! Trust anchors, in order:
//! 1. `$ADAGE_HOME/trusted-signers` — the person's own `allowed_signers` file.
//! 2. `<repo>/.adage/trusted-signers` of the repository the track is scoped to (when run inside
//!    it). A hostile repository could plant this file, so it counts only once approved on this
//!    machine with `--trust`, pinned by its SHA-256 in `$ADAGE_HOME/trust.json` (trust on first
//!    use). Any change to the file needs approval again.
//! 3. A signer approved with `--trust`, pinned in `trust.json` to the repository key.

use crate::share::{describe_step, list_files, require_person, SIG};
use adage_core::repo::{find_root, repo_key};
use adage_sign::PublicKey;
use adage_sign::{allowed_signers, fingerprint, key_line, NAMESPACE};
use adage_store::Paths;
use adage_tracks::{index::IMPORTED_DIR, io, Aspect, Index, RouteState, Track};
use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Debug, Default, Serialize, Deserialize)]
struct TrustStore {
    #[serde(default)]
    repos: BTreeMap<String, RepoTrust>,
}

/// Approvals for one repository key.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
struct RepoTrust {
    /// The repository's trusted-signers file when approved, and its SHA-256 (`None`: no file).
    trusted_signers_file: Option<String>,
    trusted_signers_sha256: Option<String>,
    signers: Vec<PinnedSigner>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct PinnedSigner {
    key: String,
    fingerprint: String,
    approved_unix: u64,
}

fn trust_path(paths: &Paths) -> PathBuf {
    paths.root.join("trust.json")
}

fn load_store(paths: &Paths) -> Result<TrustStore> {
    match std::fs::read(trust_path(paths)) {
        Ok(b) => serde_json::from_slice(&b).with_context(|| format!("{} is corrupt", trust_path(paths).display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(TrustStore::default()),
        Err(e) => Err(e.into()),
    }
}

fn save_store(paths: &Paths, store: &TrustStore) -> Result<()> {
    let p = trust_path(paths);
    std::fs::create_dir_all(&paths.root)?;
    let tmp = p.with_extension(format!("tmp{}", std::process::id()));
    std::fs::write(&tmp, serde_json::to_vec_pretty(store)?)?;
    std::fs::rename(tmp, p)?;
    Ok(())
}

/// Why the signer is trusted, and the trust change to commit once the track installs.
struct Decision {
    how: String,
    record: Option<TrustStore>,
}

fn decide(paths: &Paths, repo: &str, msg: &[u8], pem: &str, signer: &PublicKey, trust: bool) -> Result<Decision> {
    let now = SystemTime::now();
    let fp = fingerprint(signer);

    let personal = paths.root.join("trusted-signers");
    if personal.is_file() {
        let s = allowed_signers::parse(&std::fs::read_to_string(&personal)?)
            .with_context(|| personal.display().to_string())?;
        match adage_sign::verify(msg, NAMESPACE, pem, &s, now) {
            Ok(e) => {
                return Ok(Decision {
                    how: format!("{} in {}", e.principals.join(","), personal.display()),
                    record: None,
                })
            }
            Err(e) if adage_sign::lists_key(&s, signer) => bail!("refusing: {e:#} ({})", personal.display()),
            Err(_) => {}
        }
    }

    // The repository's own file counts only for tracks scoped to that repository.
    let repo_file = std::env::current_dir()
        .ok()
        .and_then(|d| find_root(&d))
        .filter(|r| repo_key(r) == repo)
        .map(|r| r.join(".adage/trusted-signers"))
        .filter(|p| p.is_file());
    let (file_hash, file_signers) = match &repo_file {
        Some(p) => {
            let bytes = std::fs::read(p)?;
            let s =
                allowed_signers::parse(&String::from_utf8_lossy(&bytes)).with_context(|| p.display().to_string())?;
            (Some(io::hex(&Sha256::digest(&bytes))), Some(s))
        }
        None => (None, None),
    };
    let mut store = load_store(paths)?;
    let pinned = store.repos.get(repo).cloned().unwrap_or_default();
    let pin_matches = store.repos.contains_key(repo) && pinned.trusted_signers_sha256 == file_hash;
    let approve = |store: &mut TrustStore| {
        let entry = store.repos.entry(repo.to_string()).or_default();
        if !pin_matches {
            *entry = RepoTrust {
                trusted_signers_file: repo_file.as_ref().map(|p| p.display().to_string()),
                trusted_signers_sha256: file_hash.clone(),
                signers: vec![],
            };
        }
        if !entry.signers.iter().any(|s| s.key == key_line(signer)) {
            let secs = now.duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
            entry.signers.push(PinnedSigner { key: key_line(signer), fingerprint: fp.clone(), approved_unix: secs });
        }
    };

    if let (Some(p), Some(s)) = (&repo_file, &file_signers) {
        let hash = file_hash.as_deref().unwrap_or_default();
        match adage_sign::verify(msg, NAMESPACE, pem, s, now) {
            Ok(e) if pin_matches => {
                return Ok(Decision { how: format!("{} in {} (approved, sha256 {})", e.principals.join(","), p.display(), &hash[..16]), record: None })
            }
            Ok(e) if trust => {
                approve(&mut store);
                return Ok(Decision {
                    how: format!("{} in {} — file approved now with --trust (sha256 {})", e.principals.join(","), p.display(), &hash[..16]),
                    record: Some(store),
                });
            }
            Ok(_) => bail!(
                "refusing: {} lists this signer, but that file {} on this machine. Review it, then re-run with --trust to approve it (sha256 {hash}).",
                p.display(),
                if store.repos.contains_key(repo) { "has changed since it was approved" } else { "has not been approved" }
            ),
            Err(e) if adage_sign::lists_key(s, signer) => bail!("refusing: {e:#} ({})", p.display()),
            Err(_) => {}
        }
    }

    if pin_matches && pinned.signers.iter().any(|s| s.key == key_line(signer)) {
        return Ok(Decision {
            how: format!("approved earlier on this machine for {repo} ({})", trust_path(paths).display()),
            record: None,
        });
    }
    if trust {
        approve(&mut store);
        return Ok(Decision { how: format!("approved now with --trust for {repo}"), record: Some(store) });
    }
    bail!(
        "refusing: unknown signer {fp} for {repo}. Confirm the fingerprint with the track's author, then re-run \
         with --trust, or add their key to {}.",
        personal.display()
    )
}

pub fn add(args: &[String], paths: &Paths) -> Result<()> {
    require_person("add")?;
    let src = PathBuf::from(args.first().filter(|a| !a.starts_with("--")).context("usage: adage add <dir> [--trust]")?);
    let trust = args.iter().any(|a| a == "--trust");
    if !src.join("track.toml").is_file() {
        bail!("{} is not a track directory (no track.toml)", src.display());
    }
    if !src.join(SIG).is_file() {
        bail!("refusing an unsigned track: {} has no {SIG}", src.display());
    }
    let listing = list_files(&src)?;
    if let Some((rel, why)) = listing.other.iter().find(|(r, why)| r != SIG && *why != "hidden") {
        bail!("refusing: {rel} does not belong in a shared track ({why})");
    }
    // Verify and install a private copy, so what is checked is exactly what is installed.
    let stage = paths.root.join("staging").join(format!("add-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&stage);
    let result = (|| {
        for rel in listing.files.iter().chain([&SIG.to_string()]) {
            let to = stage.join("in").join(rel);
            std::fs::create_dir_all(to.parent().unwrap())?;
            std::fs::copy(src.join(rel), to)?;
        }
        install(&stage, &src, paths, trust)
    })();
    let _ = std::fs::remove_dir_all(&stage);
    result
}

fn install(stage: &Path, src: &Path, paths: &Paths, trust: bool) -> Result<()> {
    let dir = stage.join("in");
    let digest = io::digest(&dir)?;
    let msg = adage_sign::message(&digest)?;
    let pem = std::fs::read_to_string(dir.join(SIG))?;
    let signer = adage_sign::verify_embedded(&msg, NAMESPACE, &pem).map_err(|e| {
        anyhow!("refusing: the signature does not match this track — it changed after signing, or was not signed as a track ({e:#})")
    })?;
    let track = io::read(&dir).context("refusing: the track fails validation")?;
    let repo = track.manifest.scope.repo.clone();
    let decision = decide(paths, &repo, &msg, &pem, &signer, trust)?;

    let (imported, was) = at_probation(track);
    io::validate(&imported)?;
    let name = imported.manifest.track.name.clone();
    // Imports live apart from learned tracks, so `adage learn` never overwrites one, and an import
    // never replaces a learned track (where both hold a route, the index keeps the learned one).
    let dest = paths.tracks().join(IMPORTED_DIR).join(&name);
    // A re-import keeps the recipient's own review decisions for routes whose ids survive.
    let carried = crate::reimport::carry(&dest, imported, &paths.tracks())?;
    let imported = carried.track;
    io::validate(&imported)?;
    let fresh = stage.join("out");
    io::write(&fresh, &imported)?;
    adage_tracks::review::save(&fresh, &carried.reviews)?;
    adage_tracks::proposals::save(&fresh, &carried.proposals)?;
    for rel in ["evidence.json", "CHANGELOG.md"] {
        if dir.join(rel).is_file() {
            std::fs::copy(dir.join(rel), fresh.join(rel))?;
        }
    }
    if dir.join("intents").is_dir() {
        std::fs::create_dir_all(fresh.join("intents"))?;
        for e in std::fs::read_dir(dir.join("intents"))? {
            let e = e?;
            std::fs::copy(e.path(), fresh.join("intents").join(e.file_name()))?;
        }
    }

    if let Some(store) = &decision.record {
        save_store(paths, store)?;
    }
    if dest.exists() {
        std::fs::remove_dir_all(&dest)?;
    }
    std::fs::create_dir_all(dest.parent().unwrap())?;
    std::fs::rename(&fresh, &dest)?;

    // Recompile the index exactly as `adage learn` does.
    let all = io::read_all(&paths.tracks())?;
    let index = Index::compile(&all);
    index.write_atomic(&paths.index())?;

    let m = &imported.manifest;
    println!("adage add: {} @ {}  (repo {})  from {}", m.track.name, m.track.version, repo, src.display());
    println!("  signature  good, namespace {NAMESPACE}, key {}", fingerprint(&signer));
    println!("  trusted    {}", decision.how);
    println!("  digest     sha256 {digest}");
    println!("  installed  {}", dest.display());
    println!("  routes     imported at probation; never green on import (O2):");
    for (r, before) in imported.routes.iter().zip(was) {
        let aspect = format!("{:?}", r.aspect).to_lowercase();
        let note =
            if before != r.aspect { format!(" (was {})", format!("{before:?}").to_lowercase()) } else { String::new() };
        println!("    {:<22} {aspect}{note:<12} {}", r.id, describe_step(&r.step));
    }
    if !carried.kept.is_empty() {
        let kept: Vec<String> = carried.kept.iter().map(|(id, d)| format!("{id} ({})", d.as_str())).collect();
        println!("  kept       your review decisions: {}", kept.join(", "));
    }
    for id in carried.proposals.routes.keys() {
        println!("  proposed   {id} is pinned here and the new version differs; see `adage review`");
    }
    println!("index generation {}, {} routes", index.generation, index.routes.len());
    Ok(())
}

/// Design O2: imported routes start at probation — yellow for signed, trusted sources, never green.
/// Routes the author retired or demoted stay out of service. The author's `review.yaml` travels in
/// the signed package but is not installed: the recipient reviews imports for themselves.
fn at_probation(mut t: Track) -> (Track, Vec<Aspect>) {
    let was = t.routes.iter().map(|r| r.aspect).collect();
    for r in &mut t.routes {
        if matches!(r.state, RouteState::Active | RouteState::Probation) {
            r.state = RouteState::Probation;
        }
        if r.aspect == Aspect::Green {
            r.aspect = Aspect::Yellow;
        }
    }
    let p = &mut t.manifest.track.provenance;
    if !p.starts_with("imported") {
        *p = format!("imported:{p}");
    }
    (t, was)
}

#[cfg(test)]
mod tests {
    use super::*;
    use adage_tracks::{Evidence, Gauge, Manifest, Route, Scope, Step, TrackInfo, Trigger};

    #[test]
    fn probation_never_revives_retired_routes() {
        let route = |id: &str, aspect, state| Route {
            id: id.into(),
            summary: "s".into(),
            trigger: Trigger::SessionStart,
            step: Step::Run { command: "make test".into() },
            slots: vec![],
            effect: adage_core::Effect::Verify,
            aspect,
            state,
            exit: vec![],
            evidence: Evidence::default(),
            authored: false,
        };
        let t = Track {
            manifest: Manifest {
                track: TrackInfo {
                    name: "a/b".into(),
                    version: "1.0.0".into(),
                    description: "d".into(),
                    provenance: "desire-path".into(),
                },
                scope: Scope { repo: "a".into() },
                gauge: Gauge::default(),
            },
            routes: vec![
                route("live", Aspect::Green, RouteState::Active),
                route("gone", Aspect::Yellow, RouteState::Retired),
            ],
        };
        let (t, was) = at_probation(t);
        assert_eq!(was, vec![Aspect::Green, Aspect::Yellow]);
        assert_eq!((t.routes[0].state, t.routes[0].aspect), (RouteState::Probation, Aspect::Yellow));
        assert_eq!(t.routes[1].state, RouteState::Retired, "a route its author rejected stays retired");
        assert_eq!(t.manifest.track.provenance, "imported:desire-path");
    }
}
