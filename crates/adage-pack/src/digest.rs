//! Digests and signatures (Track C §1.5).
//!
//! - The **route exec digest** ([`route_exec_digest`]) is the SHA-256 of the canonical JSON (keys
//!   sorted, no whitespace, as `EngineParams::canonical_json`) of the route minus `evidence` and
//!   `strength` (a pack never holds `state`), with every referenced type definition and every script
//!   SHA-256 inlined. Approvals are keyed by it: evidence updates and strength moves keep it; any
//!   change to a trigger, argv, input or output type, or script changes it.
//! - The **pack digest** ([`pack_digest`], [`digest_dir`]) is `adage_tracks::io::digest`'s
//!   algorithm: per-file SHA-256 over line-ending-normalised bytes (`scripts/` verbatim), sorted by
//!   path, then the SHA-256 of the `"<hash>  <path>\n"` manifest. `pack.sig` is excluded;
//!   `CLOSURE` is included, so the signature covers the declared closure.
//! - The **signature** is sshsig over the pack digest's 32 bytes in namespace [`NAMESPACE`].

use crate::model::{RouteV2, TypeDef};
use crate::types;
use crate::validate::{read_dir_files, SIG};
use anyhow::{Context, Result};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::Path;
use std::time::SystemTime;

/// The sshsig namespace for packs ([`adage_sign::PACK_NAMESPACE`]). An `allowed_signers` entry
/// restricted to `adage-track` does not authorise a pack, which can request execution.
pub const NAMESPACE: &str = adage_sign::PACK_NAMESPACE;

pub fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

pub fn sha256_hex(b: &[u8]) -> String {
    hex(&Sha256::digest(b))
}

/// `\r\n` and lone `\r` become `\n`; every other byte is kept as is.
pub fn normalise_line_endings(b: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\r' {
            out.push(b'\n');
            if b.get(i + 1) == Some(&b'\n') {
                i += 1;
            }
        } else {
            out.push(b[i]);
        }
        i += 1;
    }
    out
}

/// The pack digest over a set of files (relative, `/`-separated paths). `pack.sig` does not count.
pub fn pack_digest(files: &BTreeMap<String, Vec<u8>>) -> String {
    let manifest: String = files
        .iter()
        .filter(|(rel, _)| rel.as_str() != SIG)
        .map(|(rel, bytes)| {
            let h = if rel.starts_with("scripts/") {
                sha256_hex(bytes)
            } else {
                sha256_hex(&normalise_line_endings(bytes))
            };
            format!("{h}  {rel}\n")
        })
        .collect();
    sha256_hex(manifest.as_bytes())
}

/// The pack digest of a directory, as signatures cover it (the files are read with the pack's
/// strict listing: symlinks and unknown files refuse it).
pub fn digest_dir(dir: &Path) -> Result<String> {
    Ok(pack_digest(&read_dir_files(dir)?))
}

/// Serialise a JSON value with object keys sorted and no whitespace.
pub fn canonical_json(v: &Value) -> String {
    adage_tracks::params::canonical_json(v)
}

/// The canonical JSON of a type definition (what `CLOSURE` hashes, and the exec digest inlines).
pub fn type_json(t: &TypeDef) -> Value {
    serde_json::to_value(t).unwrap_or(Value::Null)
}

/// The route exec digest (see the module documentation). `types` are the pack's user types.
pub fn route_exec_digest(route: &RouteV2, types: &BTreeMap<String, TypeDef>) -> String {
    sha256_hex(exec_json(route, types).as_bytes())
}

/// The canonical JSON the route exec digest hashes.
pub fn exec_json(route: &RouteV2, user: &BTreeMap<String, TypeDef>) -> String {
    let mut v = serde_json::to_value(route).unwrap_or(Value::Null);
    if let Value::Object(m) = &mut v {
        m.remove("evidence");
        m.remove("strength");
        m.remove("state");
    }
    let mut referenced: BTreeMap<String, Value> = BTreeMap::new();
    let mut add = |ty: &str| {
        if types::is_builtin(ty) {
            referenced.insert(ty.to_string(), json!({ "builtin": ty }));
        }
        for id in types::user_refs(ty, user) {
            referenced.insert(id.clone(), type_json(&user[&id]));
        }
        // A list's built-in item type.
        if let Some(item) = user.get(ty).and_then(|t| t.item.as_deref()).filter(|i| types::is_builtin(i)) {
            referenced.insert(item.to_string(), json!({ "builtin": item }));
        }
    };
    for i in route.inputs.values() {
        add(&i.ty);
    }
    for s in &route.steps {
        for o in s.outputs.values() {
            add(&o.ty);
        }
    }
    let scripts: BTreeMap<String, String> =
        route.steps.iter().filter_map(|s| s.script.as_ref()).map(|s| (s.path.clone(), s.sha256.clone())).collect();
    let wrapped = json!({
        "route": v,
        "types": referenced,
        "scripts": scripts,
    });
    canonical_json(&wrapped)
}

/// The message a pack signature covers: the digest's 32 raw bytes.
pub fn message(digest_hex: &str) -> Result<Vec<u8>> {
    adage_sign::message(digest_hex)
}

/// Sign a pack directory's digest into `pack.sig` (ssh-agent first, then `key`). The directory
/// must already be a valid pack ([`crate::Pack::load`] it first).
pub fn sign_dir(dir: &Path, agent_sock: Option<&Path>, key: Option<&Path>) -> Result<adage_sign::Signed> {
    let digest = digest_dir(dir)?;
    let signed = adage_sign::sign(&message(&digest)?, NAMESPACE, agent_sock, key).context("signing the pack")?;
    std::fs::write(dir.join(SIG), &signed.pem)?;
    Ok(signed)
}

/// Verify a pack directory's `pack.sig` against allowed signers, in namespace [`NAMESPACE`].
pub fn verify_dir(
    dir: &Path,
    signers: &[adage_sign::AllowedSigner],
    now: SystemTime,
) -> Result<adage_sign::AllowedSigner> {
    let files = read_dir_files(dir)?;
    let sig = files.get(SIG).context("the pack has no pack.sig")?;
    let pem = std::str::from_utf8(sig).context("pack.sig is not text")?;
    adage_sign::verify(&message(&pack_digest(&files))?, NAMESPACE, pem, signers, now)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn follows_the_track_digest_algorithm() {
        let mut a = BTreeMap::new();
        a.insert("pack.toml".to_string(), b"a\r\nb\n".to_vec());
        a.insert("scripts/sha256-x".to_string(), b"x\r\n".to_vec());
        let mut b = a.clone();
        b.insert("pack.toml".to_string(), b"a\nb\n".to_vec());
        b.insert(SIG.to_string(), b"sig".to_vec());
        assert_eq!(pack_digest(&a), pack_digest(&b));
        // Scripts are hashed verbatim.
        b.insert("scripts/sha256-x".to_string(), b"x\n".to_vec());
        assert_ne!(pack_digest(&a), pack_digest(&b));
        // The same files digest the same way as a v1 track directory.
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("pack.toml"), "a\nb\n").unwrap();
        let mut one = BTreeMap::new();
        one.insert("pack.toml".to_string(), b"a\nb\n".to_vec());
        assert_eq!(pack_digest(&one), adage_tracks::io::digest(d.path()).unwrap());
    }
}
