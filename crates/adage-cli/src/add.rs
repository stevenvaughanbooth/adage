//! `adage add <dir> [--trust]` (Design §10.7, §15.2; review finding F9; Track C §2.2): verify a
//! shared pack's signature against trusted signers, validate it, install it, bind it to
//! repositories at forming, recompile the index.
//!
//! A **pack** (`pack.toml`, format `adage-pack/1`) is read strictly (no symlinks, no smuggled
//! files), its digest checked against `pack.sig` in namespace `adage-pack`, and it is validated in
//! full (schema, semantics, and the closure re-derived and compared with `CLOSURE`); local policy
//! runs; the pack is installed under `tracks/@imported/<owner>/<name>/` exactly as verified, its
//! vendored scripts are copied read-only into `$ADAGE_HOME/cas/`, and it is bound to repositories
//! ([`crate::packs`]): routes whose closure is unsatisfied here become inert, with a reason. No
//! execution approval is created.
//!
//! Trust anchors, in order:
//! 1. `$ADAGE_HOME/trusted-signers` — the person's own `allowed_signers` file.
//! 2. `<repo>/.adage/trusted-signers` of the repository the track is scoped to (when run inside
//!    it). A hostile repository could plant this file, so it counts only once approved on this
//!    machine with `--trust`, pinned by its SHA-256 in `$ADAGE_HOME/trust.json` (trust on first
//!    use). Any change to the file needs approval again.
//! 3. A signer approved with `--trust`, pinned in `trust.json` to the repository key.
//!
//! An `allowed_signers` entry restricted to another namespace (`git`, say) never authorises a pack.

use crate::packs::{self, Bound};
use crate::share::require_person;
use adage_core::repo::{find_root, repo_key};
use adage_pack::bind::{Applies, Status};
use adage_pack::Pack;
use adage_sign::PublicKey;
use adage_sign::{allowed_signers, fingerprint, key_line, PACK_NAMESPACE};
use adage_store::Paths;
use adage_term::{Glyph, Line, Out, Role};
use adage_tracks::{io, Index, RouteState, Strength, Track};
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
    crate::writable_data_dir(paths)?;
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

fn decide(
    paths: &Paths,
    repo: &str,
    msg: &[u8],
    pem: &str,
    signer: &PublicKey,
    trust: bool,
    namespace: &str,
) -> Result<Decision> {
    let now = SystemTime::now();
    let fp = fingerprint(signer);

    let personal = paths.root.join("trusted-signers");
    if personal.is_file() {
        let s = allowed_signers::parse(&std::fs::read_to_string(&personal)?)
            .with_context(|| personal.display().to_string())?;
        match adage_sign::verify(msg, namespace, pem, &s, now) {
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
        match adage_sign::verify(msg, namespace, pem, s, now) {
            Ok(e) if pin_matches => {
                return Ok(Decision {
                    how: format!("{} in {} (approved, sha256 {})", e.principals.join(","), p.display(), &hash[..16]),
                    record: None,
                })
            }
            Ok(e) if trust => {
                approve(&mut store);
                return Ok(Decision {
                    how: format!(
                        "{} in {} — file approved now with --trust (sha256 {})",
                        e.principals.join(","),
                        p.display(),
                        &hash[..16]
                    ),
                    record: Some(store),
                });
            }
            Ok(_) => {
                return Err(crate::hint(
                    format!(
                        "refusing: {} lists this signer, but that file {} on this machine",
                        p.display(),
                        if store.repos.contains_key(repo) {
                            "has changed since it was approved"
                        } else {
                            "has not been approved"
                        }
                    ),
                    format!("review it, then run the same `adage add` with --trust to approve it (sha256 {hash})"),
                ))
            }
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
    Err(crate::hint(
        format!("refusing: unknown signer {fp} for {repo}"),
        format!(
            "confirm the fingerprint with the pack's author, then run the same `adage add` with --trust, or add their key to {}",
            personal.display()
        ),
    ))
}

pub fn add(args: &[String], paths: &Paths) -> Result<()> {
    require_person("add")?;
    let usage = "usage: adage add <pack-dir> [--trust]";
    let src = PathBuf::from(args.first().filter(|a| !a.starts_with("--")).context(usage)?);
    let trust = args.iter().any(|a| a == "--trust");
    let cwd_root = std::env::current_dir().ok().and_then(|d| find_root(&d));
    if src.join(adage_pack::validate::MANIFEST).is_file() {
        return add_pack(&src, paths, trust, cwd_root.as_deref());
    }
    if !src.is_dir() {
        return Err(crate::hint(
            format!("{} does not exist, or is not a directory", src.display()),
            "pass the directory `adage share --out <dir>` wrote",
        ));
    }
    if src.join("track.toml").is_file() {
        return Err(crate::hint(
            format!("{} is a track directory, not a pack", src.display()),
            "packs are the only format `adage add` installs: ask its author to run `adage share <track> --out <dir>` and send you that directory",
        ));
    }
    Err(crate::hint(
        format!("{} is not a pack (no {})", src.display(), adage_pack::validate::MANIFEST),
        "point `adage add` at the directory `adage share --out` wrote",
    ))
}

/// The repository key trust decisions are pinned to: the pack's repository this `add` runs in, or
/// its first; for a pack that applies by fingerprint, detector or anywhere, the repository `add`
/// runs in (`*` outside one).
fn trust_scope(pack: &Pack, cwd_root: Option<&Path>) -> String {
    match &pack.manifest.applicability {
        adage_pack::Applicability::Repo { repo_keys } => {
            let here = cwd_root.map(|r| (repo_key(r), adage_pack::bind::local_keys(r)));
            match here {
                Some((key, keys)) if repo_keys.iter().any(|k| keys.contains(k)) => key,
                _ => repo_keys.first().cloned().unwrap_or_default(),
            }
        }
        _ => cwd_root.map(repo_key).unwrap_or_else(|| "*".into()),
    }
}

/// Each route's effect as this machine classifies it (Track C §2.2): run steps by the local
/// command table, and a repository or vendored script as environment whatever it declares.
/// (Validation has already refused any step declaring less than its classified effect.)
fn local_effects(pack: &Pack) -> BTreeMap<String, adage_core::Effect> {
    let table = adage_core::CommandTable::load();
    let rank = adage_pack::validate::rank;
    pack.routes
        .iter()
        .map(|r| {
            let e = r
                .steps
                .iter()
                .map(|s| match (&s.run, &s.script) {
                    (Some(_), Some(_)) => adage_core::Effect::Environment,
                    (Some(run), None) => {
                        let c = io::classify(&run.argv.join(" "), &table);
                        if rank(c) > rank(s.effect) {
                            c
                        } else {
                            s.effect
                        }
                    }
                    _ => s.effect,
                })
                .max_by_key(|e| rank(*e))
                .unwrap_or(adage_core::Effect::Observe);
            (r.id.clone(), e)
        })
        .collect()
}

fn add_pack(src: &Path, paths: &Paths, trust: bool, cwd_root: Option<&Path>) -> Result<()> {
    // Exactly the listed files, read once: what is checked is what is installed.
    let files = adage_pack::validate::read_dir_files(src).map_err(|e| anyhow!("refusing: {e:#}"))?;
    let Some(sig) = files.get(adage_pack::validate::SIG) else {
        bail!("refusing an unsigned pack: {} has no {}", src.display(), adage_pack::validate::SIG)
    };
    let digest = adage_pack::pack_digest(&files);
    let msg = adage_sign::message(&digest)?;
    let pem = std::str::from_utf8(sig).context("refusing: pack.sig is not text")?.to_string();
    let signer = adage_sign::verify_embedded(&msg, PACK_NAMESPACE, &pem).map_err(|e| {
        anyhow!(
            "refusing: the signature does not match this pack — it changed after signing, or was not signed as a pack (namespace {PACK_NAMESPACE}) ({e:#})"
        )
    })?;
    let pack = Pack::from_files(&files).map_err(|e| anyhow!("refusing: the pack fails validation: {e:#}"))?;
    packs::policy_check(paths, &pack, Some(&fingerprint(&signer)))?;
    let scope = trust_scope(&pack, cwd_root);
    // One writer of the tracks and index at a time (`adage condition`, automatic learners).
    let _tracks = adage_store::lock::exclusive(&paths.tracks_lock())?;
    let decision = decide(paths, &scope, &msg, &pem, &signer, trust, PACK_NAMESPACE)?;

    let name = pack.manifest.pack.name.clone();
    let dest = packs::pack_dir(paths, &name);
    let stage = paths.root.join("staging").join(format!("add-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&stage);
    let staged = (|| -> Result<()> {
        for (rel, bytes) in &files {
            let p = stage.join(rel);
            std::fs::create_dir_all(p.parent().context("a pack file has a parent directory")?)?;
            std::fs::write(p, bytes)?;
        }
        // Re-read what was written: it must be the pack that was verified.
        if adage_pack::digest_dir(&stage)? != digest {
            bail!("the staged copy of the pack differs from what was verified");
        }
        Ok(())
    })();
    if let Err(e) = staged {
        let _ = std::fs::remove_dir_all(&stage);
        return Err(e);
    }
    if let Some(store) = &decision.record {
        save_store(paths, store)?;
    }
    replace_dir(&stage, &dest)?;
    let cas = packs::install_cas(paths, &pack)?;
    let verified = format!("signature by {}, namespace {PACK_NAMESPACE}", fingerprint(&signer));
    let bound = packs::bind_installed(paths, &pack, &digest, &verified, &[], cwd_root)?;
    let index = recompile(paths)?;

    let m = &pack.manifest;
    let mut o = Out::stdout();
    o.header("add", &[&format!("pack {}@{}", m.pack.name, m.pack.version), &format!("from {}", src.display())])?;
    o.blank()?;
    o.kv("signature", Line::new().text(format!("good, namespace {PACK_NAMESPACE}, key {}", fingerprint(&signer))))?;
    o.kv("trusted", Line::new().text(decision.how.clone()))?;
    o.kv("digest", Line::new().quiet(format!("sha256 {digest}")))?;
    o.kv("installed", Line::new().text(dest.display().to_string()))?;
    if !pack.scripts.is_empty() {
        o.kv(
            "scripts",
            Line::new().text(format!(
                "{} in {} (read-only; {} new); nothing runs them without an approval on this machine",
                crate::words::count(pack.scripts.len(), "vendored script"),
                packs::cas_dir(paths).display(),
                cas.len()
            )),
        )?;
    }
    print_bound(&mut o, &pack, &bound, &local_effects(&pack))?;
    finish(&mut o, &index)
}

/// The index row and what to run next, after an install.
fn finish<W: std::io::Write>(o: &mut Out<W>, index: &Index) -> Result<()> {
    o.kv(
        "index",
        Line::new()
            .text(format!("gen {}", &index.generation[..index.generation.len().min(8)]))
            .sep()
            .quiet(crate::words::count(index.routes.len(), "reflex")),
    )?;
    o.blank()?;
    o.next("adage review", "imported reflexes wait for your decision")?;
    o.also("adage status", "every reflex in effect")?;
    Ok(())
}

/// Replace `dest` with the directory `from` (on the same filesystem).
fn replace_dir(from: &Path, dest: &Path) -> Result<()> {
    if dest.exists() {
        std::fs::remove_dir_all(dest)?;
    }
    std::fs::create_dir_all(dest.parent().context("an install directory has a parent")?)?;
    std::fs::rename(from, dest)?;
    Ok(())
}

/// Recompile the index exactly as `adage condition` does.
fn recompile(paths: &Paths) -> Result<Index> {
    let all = io::read_all(&paths.tracks())?;
    let (index, _) = Index::compile_root(&paths.tracks(), &all);
    index.write_atomic(&paths.index())?;
    Ok(index)
}

fn print_bound<W: std::io::Write>(
    o: &mut Out<W>,
    pack: &Pack,
    bound: &[Bound],
    effects: &BTreeMap<String, adage_core::Effect>,
) -> Result<()> {
    if bound.is_empty() {
        o.kv(
            "bound",
            Line::new().text(format!(
                "nowhere yet: run `adage add` again inside a repository this pack applies to ({})",
                describe_applicability(&pack.manifest.applicability)
            )),
        )?;
        return Ok(());
    }
    for b in bound {
        let binding = &b.binding;
        if let Applies::No(why) = &binding.applies {
            o.kv("repo", Line::new().text(format!("{}: not applicable ({why})", binding.repo)))?;
            continue;
        }
        if let Some(d) = &b.dir {
            o.kv("bound", Line::new().text(d.display().to_string()))?;
        }
        o.kv(
            "repo",
            Line::new()
                .text(binding.repo.clone())
                .sep()
                .quiet(format!("{} live, {} inert", binding.live().count(), binding.inert().count()))
                .sep()
                .quiet("imported as forming: nothing fires on import"),
        )?;
        let routes = b.track.as_ref().map(|t| t.routes.as_slice()).unwrap_or_default();
        // `  ● prime  ◠ read-linked-files  read up to 5 files … · effect observe · exec up to E0`,
        // aligned as `adage status` aligns its rows.
        let id_w = binding.routes.iter().map(|r| r.id.chars().count()).max().unwrap_or(16).clamp(16, 34);
        let row = |glyph: Glyph, id: &str| {
            Line::new().pad_to(13).glyph(Role::Accent, glyph).text(" ").text(id.to_string()).pad_to(17 + id_w)
        };
        for r in &binding.routes {
            match &r.status {
                Status::Live { max_level } => {
                    let Some(rt) = routes.iter().find(|x| x.id == r.id) else { continue };
                    let before = b.was.get(&r.id).copied().unwrap_or(rt.strength);
                    let effect = effects.get(&r.id).map(|e| e.as_str()).unwrap_or("?");
                    let mut l = Line::new()
                        .text("  ")
                        .strength(crate::words::strength(rt.strength))
                        .extend(row(Glyph::Reflex, &r.id))
                        .text(crate::share::describe_step(&rt.step))
                        .sep()
                        .faint(format!("effect {effect}, exec up to {max_level:?}"));
                    if before != rt.strength {
                        l = l.sep().quiet(format!("was {}", crate::words::strength(before).word()));
                    }
                    if rt.state == RouteState::Nascent {
                        l = l.sep().quiet("nascent until approved");
                    }
                    o.line(&l)?;
                }
                Status::Inert { reason } => o.line(
                    &Line::new()
                        .text("  ")
                        .glyph(Role::Ink3, Glyph::Off)
                        .faint(" inert")
                        .extend(row(Glyph::Reflex, &r.id))
                        .faint(reason.clone()),
                )?,
            }
        }
        if !b.kept.is_empty() {
            let kept: Vec<String> =
                b.kept.iter().map(|(id, d)| format!("{id} ({})", crate::words::prose(d.as_str()))).collect();
            o.kv("kept", Line::new().text(format!("your review decisions: {}", kept.join(", "))))?;
        }
        for id in &b.proposed {
            o.kv(
                "proposed",
                Line::new().text(format!("{id} is pinned here and the new version differs; see `adage review`")),
            )?;
        }
    }
    o.kv("approvals", Line::new().text("none: a pack can only request execution; nothing here grants it"))?;
    Ok(())
}

fn describe_applicability(a: &adage_pack::Applicability) -> String {
    match a {
        adage_pack::Applicability::Repo { repo_keys } => format!("repositories {}", repo_keys.join(", ")),
        adage_pack::Applicability::Fingerprint { .. } => "by fingerprint".into(),
        adage_pack::Applicability::Detector { detector, catalogue } => format!("detector {detector} of {catalogue}"),
        adage_pack::Applicability::Any => "any repository".into(),
    }
}

/// Design O2: imported routes start at forming — prime for signed, trusted sources, never fire.
/// Routes the author extinguished or weakened stay out of service. The author's `review.yaml` travels in
/// the signed package but is not installed: the recipient reviews imports for themselves, so an
/// environment step waits as nascent until the recipient approves it (O11).
pub(crate) fn at_forming(mut t: Track) -> (Track, Vec<Strength>) {
    let was = t.routes.iter().map(|r| r.strength).collect();
    for r in &mut t.routes {
        if matches!(r.state, RouteState::Firm | RouteState::Forming) {
            r.state = match adage_tracks::initial_state(r.effect) {
                RouteState::Nascent => RouteState::Nascent,
                _ => RouteState::Forming,
            };
        }
        if r.strength == Strength::Fire {
            r.strength = Strength::Prime;
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
    fn forming_never_revives_extinguished_routes() {
        let route = |id: &str, strength, state| Route {
            id: id.into(),
            summary: "s".into(),
            trigger: Trigger::SessionStart,
            step: Step::Run { command: "make test".into() },
            slots: vec![],
            effect: adage_core::Effect::Verify,
            strength,
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
                route("live", Strength::Fire, RouteState::Firm),
                route("gone", Strength::Prime, RouteState::Extinguished),
                Route { effect: adage_core::Effect::Environment, ..route("setup", Strength::Prime, RouteState::Firm) },
            ],
        };
        let (t, was) = at_forming(t);
        assert_eq!(was, vec![Strength::Fire, Strength::Prime, Strength::Prime]);
        assert_eq!(
            t.routes[2].state,
            RouteState::Nascent,
            "an imported environment step waits for the recipient (O11)"
        );
        assert_eq!((t.routes[0].state, t.routes[0].strength), (RouteState::Forming, Strength::Prime));
        assert_eq!(t.routes[1].state, RouteState::Extinguished, "a route its author rejected stays extinguished");
        assert_eq!(t.manifest.track.provenance, "imported:desire-path");
    }
}
