//! `adage share` in the pack format (Track C §1.6): resolve the source, build the pack, run the
//! redaction check over every byte and the script checks over vendored scripts, write, and sign
//! the pack digest in namespace `adage-pack`.

use crate::redact;
use crate::share::list_files;
use adage_pack::Pack;
use adage_store::Paths;
use adage_term::{Line, Out};
use adage_tracks::{index, io, RouteState};
use anyhow::{bail, Context, Result};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// What `adage share` reads.
enum Source {
    /// A track directory (upgraded to a pack).
    Track(PathBuf),
    /// A pack directory (an installed pack, or one on disk).
    Pack(PathBuf),
}

/// A learned track wins; then an installed pack of that name; then a directory.
fn resolve(paths: &Paths, name: &str) -> Result<Source> {
    let root = paths.tracks();
    let mut stack = vec![root.clone()];
    while let Some(d) = stack.pop() {
        if d.join("track.toml").is_file() {
            if io::read(&d).is_ok_and(|t| t.manifest.track.name == name && !index::is_imported(&t)) {
                return Ok(Source::Track(d));
            }
            continue;
        }
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            if e.file_type().is_ok_and(|t| t.is_dir()) {
                stack.push(e.path());
            }
        }
    }
    let installed = crate::packs::pack_dir(paths, name);
    if adage_pack::validate::safe_pack_name(name) && installed.join(adage_pack::validate::MANIFEST).is_file() {
        return Ok(Source::Pack(installed));
    }
    let p = Path::new(name);
    if p.join(adage_pack::validate::MANIFEST).is_file() {
        return Ok(Source::Pack(p.to_path_buf()));
    }
    Ok(Source::Track(crate::share::find_track(&root, name)?))
}

/// The 64-hex digests a pack holds by construction (route exec digests, type and script hashes,
/// pins), which the redaction check's "long hex string" rule must not count as secrets.
fn known_digests(pack: &Pack) -> Result<BTreeSet<String>> {
    let mut d: BTreeSet<String> = pack.closure()?.nodes.values().filter(|v| v.len() == 64).cloned().collect();
    d.extend(pack.scripts.keys().cloned());
    d.extend(pack.manifest.requires.repo_files.iter().map(|f| f.sha256.clone()));
    for r in &pack.routes {
        d.extend(r.steps.iter().filter_map(|s| s.script.as_ref().map(|sc| sc.sha256.clone())));
    }
    Ok(d)
}

pub fn share(name: &str, out: &Path, key: Option<&Path>, args: &[String], paths: &Paths) -> Result<()> {
    let ack = args.iter().any(|a| a == "--ack-scripts");
    let cwd_root = std::env::current_dir().ok().and_then(|d| adage_core::repo::find_root(&d));
    let mut skipped: Vec<(String, String)> = Vec::new();
    let mut not_included: Vec<(String, &'static str)> = Vec::new();
    let (pack, from) = match resolve(paths, name)? {
        Source::Pack(dir) => (Pack::load(&dir).with_context(|| format!("{} fails validation", dir.display()))?, dir),
        Source::Track(dir) => {
            let mut track = io::read(&dir).with_context(|| format!("{} fails validation", dir.display()))?;
            // What a person retired or demoted stays here; a pack carries no per-machine state.
            track.routes.retain(|r| {
                let keep = r.state.is_live() || r.state == RouteState::Draft;
                if !keep {
                    skipped.push((r.id.clone(), format!("{:?} here", r.state).to_lowercase()));
                }
                keep
            });
            // Repository scripts are pinned against the repository `share` runs in, if it is the
            // track's.
            let root =
                cwd_root.as_deref().filter(|r| adage_pack::bind::local_keys(r).contains(&track.manifest.scope.repo));
            let up = adage_pack::upgrade::upgrade(&track, root)
                .with_context(|| format!("{} cannot be carried into a pack", track.manifest.track.name))?;
            skipped.extend(up.skipped);
            if let Ok(l) = list_files(&dir) {
                not_included.extend(l.other);
            }
            for f in [adage_tracks::review::FILE, adage_tracks::proposals::FILE] {
                if dir.join(f).is_file() {
                    not_included.push((f.to_string(), "local decisions: never shared"));
                }
            }
            (up.pack, dir)
        }
    };
    let files = pack.files()?;

    // Redaction over every byte, vendored scripts included; script checks over the scripts.
    let home = adage_store::home_dir().map(|h| h.to_string_lossy().into_owned());
    let digests = known_digests(&pack)?;
    let hex64 = regex::Regex::new(r"[0-9a-f]{64}").expect("regex");
    let mut findings = Vec::new();
    let mut flags: Vec<(String, crate::packs::ScriptFlag)> = Vec::new();
    for (rel, bytes) in &files {
        let Ok(text) = std::str::from_utf8(bytes) else {
            findings.push(redact::Finding {
                file: rel.clone(),
                line: 0,
                kind: "not UTF-8 text",
                excerpt: String::new(),
            });
            continue;
        };
        if rel.starts_with("scripts/") {
            for f in redact::scan(rel, text, home.as_deref()) {
                if f.kind == "absolute path" {
                    // In a script an absolute path is a flag to acknowledge, not a leak by itself.
                    let first_line_shebang = f.line == 1 && text.starts_with("#!");
                    if !first_line_shebang {
                        flags.push((
                            rel.clone(),
                            crate::packs::ScriptFlag { line: f.line, kind: f.kind, excerpt: f.excerpt },
                        ));
                    }
                } else {
                    findings.push(f);
                }
            }
            for f in crate::packs::script_flags(text) {
                if !flags.iter().any(|(r, x)| r == rel && x.line == f.line && x.kind == f.kind) {
                    flags.push((rel.clone(), f));
                }
            }
        } else {
            let masked =
                hex64.replace_all(
                    text,
                    |c: &regex::Captures| {
                        if digests.contains(&c[0]) {
                            "digest".to_string()
                        } else {
                            c[0].to_string()
                        }
                    },
                );
            findings.extend(redact::scan(rel, &masked, home.as_deref()));
        }
    }
    if !findings.is_empty() {
        return Err(crate::share::redaction_refusal(name, &findings));
    }
    if !flags.is_empty() && !ack {
        return Err(crate::refusal(
            format!(
                "refusing to share {name}: {} reach beyond the repository; nothing was written",
                crate::words::count(flags.len(), "vendored script line")
            ),
            flags.iter().map(|(rel, f)| format!("{rel}:{} · {} · {}", f.line, f.kind, f.excerpt)).collect(),
            "review them, then run the same `adage share` with --ack-scripts",
        ));
    }

    if out.exists() && (!out.is_dir() || std::fs::read_dir(out)?.next().is_some()) {
        bail!("{} already exists and is not an empty directory", out.display());
    }
    let leaf = out.file_name().with_context(|| format!("--out {} needs a directory name", out.display()))?;
    let stage = out.with_file_name(format!(".{}.adage-{}", leaf.to_string_lossy(), std::process::id()));
    let _ = std::fs::remove_dir_all(&stage);
    let signed = write_and_sign(&stage, &pack, key);
    let (digest, signed) = match signed {
        Ok(v) => v,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&stage);
            return Err(e);
        }
    };
    if out.exists() {
        std::fs::remove_dir(out)?;
    }
    std::fs::rename(&stage, out)?;

    let m = &pack.manifest;
    let mut o = Out::stdout();
    o.header("share", &[&format!("pack {}@{}", m.pack.name, m.pack.version), &format!("from {}", from.display())])?;
    o.blank()?;
    o.section(&format!("Included: exactly these files, plus {}", adage_pack::validate::SIG))?;
    for (rel, bytes) in &files {
        o.line(&Line::new().text(format!("  {rel:<48} ")).faint(format!("{:>7} bytes", bytes.len())))?;
    }
    o.blank()?;
    o.section("Reflexes: templates, types and aggregate statistics only")?;
    for r in &pack.routes {
        let exec = r.exec.as_ref().map(|x| format!("{:?}", x.level)).unwrap_or_else(|| "E0".into());
        let mut l = Line::new()
            .text(format!("  {:<24} {:<10} ", r.id, r.shape.as_str()))
            .strength(crate::words::strength(r.aspect))
            .pad_to(48)
            .quiet(format!("exec {exec}"));
        if let Some(e) = r.evidence.as_ref() {
            l = l.sep().quiet(format!("{}/{} traces", e.support, e.opportunities));
        }
        o.line(&l)?;
    }
    if !skipped.is_empty() {
        o.blank()?;
        o.section("Not carried")?;
        for (id, why) in &skipped {
            o.line(&Line::new().text(format!("  {id:<24} ")).quiet(crate::words::prose(why)))?;
        }
    }
    if !not_included.is_empty() {
        o.blank()?;
        o.section("Not included")?;
        for (rel, why) in &not_included {
            o.line(&Line::new().text(format!("  {rel:<40} ")).quiet(why.to_string()))?;
        }
    }
    if !flags.is_empty() {
        o.blank()?;
        o.section("Acknowledged (--ack-scripts)")?;
        for (rel, f) in &flags {
            o.line(&Line::new().text(format!("  {rel}:{}  {}  ", f.line, f.kind)).quiet(f.excerpt.clone()))?;
        }
    }
    o.blank()?;
    o.kv(
        "checked",
        Line::new().text("no absolute paths, home-directory paths, or secret-looking strings in any file"),
    )?;
    o.kv("never sent", Line::new().quiet("approvals, review.yaml, proposals, trust.json, session state"))?;
    o.kv("digest", Line::new().quiet(format!("sha256 {digest}")))?;
    let how = match signed.method {
        adage_sign::Method::Agent => "via ssh-agent",
        adage_sign::Method::KeyFile => "with key file",
    };
    o.kv(
        "signed",
        Line::new().text(format!(
            "{} ({}, {how}), namespace {}",
            adage_sign::fingerprint(&signed.key),
            signed.key.algorithm(),
            adage_sign::PACK_NAMESPACE
        )),
    )?;
    o.kv("wrote", Line::new().text(out.display().to_string()))?;
    let principal = Some(signed.key.comment())
        .filter(|c| !c.is_empty() && !c.contains(char::is_whitespace))
        .unwrap_or("adage-signer");
    o.blank()?;
    o.note("Recipients trust this signer for packs with a line in <repo>/.adage/trusted-signers or $ADAGE_HOME/trusted-signers:")?;
    o.line(&Line::new().text(format!(
        "  {principal} namespaces=\"{}\" {}",
        adage_sign::PACK_NAMESPACE,
        adage_sign::key_line(&signed.key)
    )))?;
    o.blank()?;
    o.next(&format!("adage add {}", out.display()), "on the recipient's machine, once they trust the signer")?;
    Ok(())
}

fn write_and_sign(stage: &Path, pack: &Pack, key: Option<&Path>) -> Result<(String, adage_sign::Signed)> {
    pack.write(stage)?;
    // What is signed is exactly what was written, re-read as a recipient reads it.
    let back = Pack::load(stage).context("the written pack fails validation")?;
    if &back != pack {
        bail!("the written pack differs from the source");
    }
    let digest = adage_pack::digest_dir(stage)?;
    let sock = std::env::var_os("SSH_AUTH_SOCK").filter(|s| !s.is_empty()).map(PathBuf::from);
    // A passphrase-protected key file is decrypted with a passphrase asked for at the terminal.
    let ask = |p: &Path| crate::tty::secret(&format!("Enter passphrase for {}: ", p.display()));
    let signed =
        adage_sign::sign_with(&adage_sign::message(&digest)?, adage_sign::PACK_NAMESPACE, sock.as_deref(), key, &ask)
            .context("signing the pack")?;
    std::fs::write(stage.join(adage_pack::validate::SIG), &signed.pem)?;
    Ok((digest, signed))
}
