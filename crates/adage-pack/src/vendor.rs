//! L-vendor (Track C §1.7, decision D6): carry a script's bytes in the pack, at
//! `scripts/sha256-<hex>`, instead of pinning a repository file.
//!
//! [`vendor`] turns every L-ref script of a pack into an L-vendor one, reading the bytes from a
//! checkout and checking them against the pin. A vendored script:
//!
//! - is run by an interpreter **named in argv** (`python3`, `node`, `sh`); its shebang is ignored,
//!   so a route whose argv runs the script directly cannot be vendored;
//! - has no `deps`: its bytes are its whole closure, so a script that reads other repository files
//!   through declared deps cannot be vendored;
//! - is UTF-8 with no NUL, at most 64 KiB (256 KiB for all of a pack's scripts), as every pack
//!   file is.
//!
//! On a recipient's machine the bytes are installed read-only in `$ADAGE_HOME/cas/<hex>` by
//! `adage add`, and only `adage-exec` runs them, from there, after a person approves the reflex on
//! that machine (never automatically), with the full text shown.

use crate::model::*;
use crate::validate::{program, Program, MAX_SCRIPT_BYTES, VENDOR_INTERPRETERS};
use crate::Pack;
use adage_core::CommandTable;
use anyhow::{bail, Context, Result};
use std::path::Path;

/// The pack path of a vendored script.
pub fn script_path(sha256: &str) -> String {
    format!("scripts/sha256-{sha256}")
}

/// What [`vendor`] did.
#[derive(Debug, Clone, PartialEq)]
pub struct Vendored {
    pub pack: Pack,
    /// (route id, repository path, sha256) of each script now carried in the pack.
    pub scripts: Vec<(String, String, String)>,
}

/// Vendor every L-ref script of `pack`, reading its bytes under `root`.
pub fn vendor(pack: &Pack, root: &Path) -> Result<Vendored> {
    let table = CommandTable::load();
    let mut p = pack.clone();
    let mut done = Vec::new();
    for r in &mut p.routes {
        let rid = r.id.clone();
        for s in &mut r.steps {
            let label = format!("reflex {rid} step {}", s.id);
            let ctx = || label.clone();
            let (Some(run), Some(sc)) = (s.run.as_mut(), s.script.as_mut()) else { continue };
            if sc.closure != ClosureLevel::Ref {
                continue;
            }
            let interpreter = match program(&run.argv, &table)? {
                Program::Script { interpreter, .. } => interpreter,
                Program::Tool(_) => None,
            };
            if !interpreter.as_deref().is_some_and(|i| VENDOR_INTERPRETERS.contains(&i)) {
                bail!(
                    "{}: {} is run without an interpreter named in argv ({VENDOR_INTERPRETERS:?}); a vendored script's shebang is ignored, so it cannot be vendored",
                    ctx(),
                    sc.path
                );
            }
            if !sc.deps.is_empty() {
                bail!(
                    "{}: {} declares deps; a vendored script carries no other files, so it cannot be vendored",
                    ctx(),
                    sc.path
                );
            }
            let bytes = read_script(root, &sc.path).with_context(ctx)?;
            let got = crate::digest::sha256_hex(&bytes);
            if got != sc.sha256 {
                bail!("{}: {} has changed since it was pinned (sha256 {got}, pinned {})", ctx(), sc.path, sc.sha256);
            }
            let vendored = script_path(&got);
            for a in run.argv.iter_mut() {
                if a.strip_prefix("./").unwrap_or(a) == sc.path {
                    *a = vendored.clone();
                }
            }
            done.push((rid.clone(), sc.path.clone(), got.clone()));
            *sc = ScriptRef { path: vendored, closure: ClosureLevel::Vendor, sha256: got.clone(), deps: Vec::new() };
            p.scripts.insert(got, bytes);
        }
    }
    // A repository file no route runs any more is no longer required.
    let still: Vec<String> = p
        .routes
        .iter()
        .flat_map(|r| r.steps.iter())
        .filter_map(|s| s.script.as_ref().filter(|sc| sc.closure == ClosureLevel::Ref).map(|sc| sc.path.clone()))
        .collect();
    p.manifest.requires.repo_files.retain(|f| still.contains(&f.path));
    p.check().context("the vendored pack fails validation")?;
    Ok(Vendored { pack: p, scripts: done })
}

/// A script's bytes under `root`: a regular file with no symlink on the way, inside the root,
/// within the script limit, UTF-8 without NUL.
fn read_script(root: &Path, rel: &str) -> Result<Vec<u8>> {
    crate::convert::hash_repo_file(root, rel)?; // the same path checks as an L-ref pin
    let bytes = std::fs::read(root.join(rel))?;
    if bytes.len() > MAX_SCRIPT_BYTES {
        bail!("{rel} is larger than {MAX_SCRIPT_BYTES} bytes (the script limit)");
    }
    let text = std::str::from_utf8(&bytes).with_context(|| format!("{rel} is not UTF-8 text"))?;
    if text.contains('\0') {
        bail!("{rel} contains a NUL byte");
    }
    Ok(bytes)
}
