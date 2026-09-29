//! Track packs: format `adage-pack/1` (Track C, milestone C0; `docs/packs.md`).
//!
//! A **pack** is the unit of distribution and approval for compiled skills: one track namespace
//! (`owner/name`) in route format v2, closed over everything it references, content-addressed and
//! signed. It is a directory:
//!
//! ```text
//! pack.toml              manifest (strict TOML)
//! routes/<id>.yaml       route v2
//! types/<id>.yaml        named contract types not built in
//! scripts/sha256-<hex>   vendored script bytes, named by their own SHA-256
//! evidence.json          aggregate evidence per route, by origin
//! attestations/<id>.json optional bench results (informational)
//! CLOSURE                canonical closure listing (generated; re-derived and compared on read)
//! pack.sig               sshsig over the pack digest, namespace "adage-pack"
//! ```
//!
//! A pack is inert: nothing in it grants execution. [`Pack::load`] reads one as untrusted input
//! (strict files, strict schema, semantic validation, then the closure, which must equal
//! `CLOSURE` byte for byte); [`Pack::write`] writes the canonical form; [`from_track_dir`] turns a
//! local track directory into a pack.

pub mod attest;
pub mod bind;
pub mod closure;
pub mod convert;
pub mod digest;
pub mod instincts;
pub mod model;
pub mod procedure;
pub mod types;
pub mod validate;
pub mod vendor;

pub use bind::{bind, BindEnv, Binding, BoundRoute, Status, Target};
pub use closure::Closure;
pub use convert::{from_track_dir, from_track_dir_with, Converted};
pub use digest::{digest_dir, pack_digest, route_exec_digest, sign_dir, verify_dir, NAMESPACE};
pub use model::*;

use anyhow::{bail, Context, Result};
use std::collections::BTreeMap;
use std::path::Path;
use validate::{Parsed, CLOSURE, EVIDENCE, MANIFEST, SIG};

/// A validated pack, closed over everything it references.
#[derive(Debug, Clone, PartialEq)]
pub struct Pack {
    pub manifest: PackManifest,
    /// Sorted by id.
    pub routes: Vec<RouteV2>,
    pub types: BTreeMap<String, TypeDef>,
    /// Vendored script bytes by their SHA-256 (hex).
    pub scripts: BTreeMap<String, Vec<u8>>,
    pub evidence: Option<PackEvidence>,
    pub attestations: BTreeMap<String, serde_json::Value>,
    /// `procedure.json`: the procedure a trained pack was compiled from ([`procedure`]).
    pub procedure: Option<procedure::Procedure>,
}

impl Pack {
    /// Read and validate a pack directory: files, schema, semantics, and the closure, which must
    /// equal the pack's `CLOSURE` (line endings aside) byte for byte. A signature is not checked
    /// here ([`verify_dir`]).
    pub fn load(dir: &Path) -> Result<Pack> {
        let files = validate::read_dir_files(dir)?;
        Pack::from_files(&files).with_context(|| format!("{} is not a valid pack", dir.display()))
    }

    /// [`Pack::load`] over files in memory (relative `/`-separated paths → bytes).
    pub fn from_files(files: &BTreeMap<String, Vec<u8>>) -> Result<Pack> {
        let parsed = validate::parse(files)?;
        let Some(found) = parsed.closure.clone() else { bail!("the pack has no {CLOSURE}") };
        let pack = Pack::from_parsed(parsed)?;
        let derived = pack.closure()?.render();
        let found = String::from_utf8(digest::normalise_line_endings(found.as_bytes())).unwrap_or_default();
        if found != derived {
            bail!("{CLOSURE} does not match the closure re-derived from the pack:\n{}", diff(&found, &derived));
        }
        Ok(pack)
    }

    /// Validate a pack built in memory (without a `CLOSURE`: it is derived).
    pub fn new(
        manifest: PackManifest,
        routes: Vec<RouteV2>,
        types: BTreeMap<String, TypeDef>,
        scripts: BTreeMap<String, Vec<u8>>,
    ) -> Result<Pack> {
        let mut p =
            Pack { manifest, routes, types, scripts, evidence: None, attestations: BTreeMap::new(), procedure: None };
        p.routes.sort_by(|a, b| a.id.cmp(&b.id));
        Pack::from_parsed(p.parsed())
    }

    fn from_parsed(mut parsed: Parsed) -> Result<Pack> {
        parsed.routes.sort_by(|a, b| a.id.cmp(&b.id));
        for w in parsed.routes.windows(2) {
            if w[0].id == w[1].id {
                bail!("route id {} is repeated", w[0].id);
            }
        }
        validate::validate(&parsed)?;
        closure::compute(&parsed)?;
        Ok(Pack {
            manifest: parsed.manifest,
            routes: parsed.routes,
            types: parsed.types,
            scripts: parsed.scripts,
            evidence: parsed.evidence,
            attestations: parsed.attestations,
            procedure: parsed.procedure,
        })
    }

    fn parsed(&self) -> Parsed {
        Parsed {
            manifest: self.manifest.clone(),
            routes: self.routes.clone(),
            types: self.types.clone(),
            scripts: self.scripts.clone(),
            evidence: self.evidence.clone(),
            attestations: self.attestations.clone(),
            procedure: self.procedure.clone(),
            closure: None,
        }
    }

    /// Re-validate after changing fields in place.
    pub fn check(&self) -> Result<()> {
        Pack::from_parsed(self.parsed()).map(|_| ())
    }

    /// The closure (re-derived; never read from the pack).
    pub fn closure(&self) -> Result<Closure> {
        closure::compute(&self.parsed())
    }

    /// The canonical files of this pack (without `pack.sig`).
    pub fn files(&self) -> Result<BTreeMap<String, Vec<u8>>> {
        let mut f = BTreeMap::new();
        f.insert(MANIFEST.to_string(), toml::to_string_pretty(&self.manifest)?.into_bytes());
        for r in &self.routes {
            f.insert(format!("routes/{}.yaml", r.id), serde_yaml::to_string(r)?.into_bytes());
        }
        for (id, t) in &self.types {
            f.insert(format!("types/{id}.yaml"), serde_yaml::to_string(t)?.into_bytes());
        }
        for (h, b) in &self.scripts {
            f.insert(format!("scripts/sha256-{h}"), b.clone());
        }
        if let Some(e) = &self.evidence {
            f.insert(EVIDENCE.to_string(), json_file(&serde_json::to_value(e)?));
        }
        for (id, a) in &self.attestations {
            f.insert(format!("attestations/{id}.json"), json_file(a));
        }
        if let Some(p) = &self.procedure {
            f.insert(procedure::PROCEDURE.to_string(), json_file(&serde_json::to_value(p)?));
        }
        f.insert(CLOSURE.to_string(), self.closure()?.render().into_bytes());
        Ok(f)
    }

    /// The pack digest of the canonical files ([`Pack::files`]): what [`Pack::write`] writes and
    /// a signature over the written directory covers. (For a pack as received, sign and verify the
    /// directory's own bytes with [`digest_dir`].)
    pub fn digest(&self) -> Result<String> {
        Ok(pack_digest(&self.files()?))
    }

    /// The route exec digest of route `id`.
    pub fn route_exec_digest(&self, id: &str) -> Option<String> {
        self.routes.iter().find(|r| r.id == id).map(|r| route_exec_digest(r, &self.types))
    }

    /// Write the canonical files to `dir`, mirroring the pack exactly: stale routes, types,
    /// scripts and attestations are removed, and so is `pack.sig`, which no longer matches. The
    /// files are re-read and must give back this pack.
    pub fn write(&self, dir: &Path) -> Result<()> {
        let files = self.files()?;
        if &Pack::from_files(&files)? != self {
            bail!("the pack does not survive its own serialisation");
        }
        std::fs::create_dir_all(dir)?;
        for sub in ["routes", "types", "scripts", "attestations"] {
            let d = dir.join(sub);
            if d.is_dir() {
                for e in std::fs::read_dir(&d)? {
                    let e = e?;
                    let rel = format!("{sub}/{}", e.file_name().to_string_lossy());
                    if e.file_type()?.is_file() && !files.contains_key(&rel) {
                        std::fs::remove_file(e.path())?;
                    }
                }
            }
        }
        for stale in [SIG, EVIDENCE, procedure::PROCEDURE] {
            if !files.contains_key(stale) && dir.join(stale).is_file() {
                std::fs::remove_file(dir.join(stale))?;
            }
        }
        for (rel, bytes) in &files {
            let p = dir.join(rel);
            if let Some(parent) = p.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(p, bytes)?;
        }
        Ok(())
    }
}

fn json_file(v: &serde_json::Value) -> Vec<u8> {
    let mut s = digest::canonical_json(v);
    s.push('\n');
    s.into_bytes()
}

fn diff(found: &str, derived: &str) -> String {
    let a: std::collections::BTreeSet<&str> = found.lines().collect();
    let b: std::collections::BTreeSet<&str> = derived.lines().collect();
    let mut out: Vec<String> = a.difference(&b).map(|l| format!("  declared, not derived: {l}")).collect();
    out.extend(b.difference(&a).map(|l| format!("  derived, not declared: {l}")));
    if out.is_empty() {
        out.push("  (same lines, different order or spacing)".into());
    }
    out.truncate(20);
    out.join("\n")
}
