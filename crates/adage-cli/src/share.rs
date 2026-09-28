//! `adage share <track-or-pack> --out <dir> [--key <path>] [--vendor] [--attest <results.jsonl>]...
//! [--ack-scripts] [--repo-root <dir>] [--format pack|v1]`
//! (Design §10.7, §10.8; Track C §1.6): emit a signed **pack** (format `adage-pack/1`).
//!
//! The source is a learned track (upgraded to a pack: its live routes, repository scripts pinned
//! L-ref against `--repo-root <dir>` or else the repository `share` runs in), an installed pack by name, or a pack or track
//! directory. Every byte of every pack file, vendored scripts included, goes through the
//! redaction check; vendored scripts that name URLs or hosts, run network tools, use absolute
//! paths or read secret environment variables are listed and need `--ack-scripts`. Approvals,
//! `review.yaml`, proposals and `trust.json` are never part of a pack. The pack digest is signed
//! into `pack.sig` in namespace `adage-pack`.
//!
//! `--format v1` writes a v1 track as before (`track.toml` + `routes/`, `track.sig` in namespace
//! `adage-track`), for recipients on an older Adage.
//!
//! Also the pieces `adage add` shares: the person-at-a-terminal guard (review finding F11) and the
//! file layout a shared track may have.

use crate::redact;
use adage_store::Paths;
use adage_tracks::{index, io, Step, Track};
use anyhow::{bail, Context, Result};
use std::io::IsTerminal;
use std::path::{Path, PathBuf};

pub const SIG: &str = "track.sig";
const MAX_FILES: usize = 256;
const MAX_FILE_BYTES: u64 = 1 << 20;

/// Review finding F11: commands that publish or change trust refuse to run inside an agent session
/// or without a terminal. `ADAGE_ALLOW_NONINTERACTIVE=1` lifts only the terminal requirement (for
/// CI and tests); nothing lifts the agent-session refusal. Environment detection is a guard against
/// an agent being steered into these commands, not a sandbox.
pub fn require_person(cmd: &str) -> Result<()> {
    // Every harness's marker, not only Claude Code's: an OpenCode or Codex agent must not be able
    // to share, trust, or review either.
    if let Some(var) = crate::install::AGENT_ENV.iter().find(|v| std::env::var_os(v).is_some_and(|x| !x.is_empty())) {
        bail!(
            "`adage {cmd}` is a decision for a person (it changes what is shared, trusted or \
             configured), so it will not run inside an agent session ({var} is set). Run it \
             yourself, in a terminal."
        );
    }
    if std::env::var("ADAGE_ALLOW_NONINTERACTIVE").as_deref() == Ok("1") {
        return Ok(());
    }
    if !std::io::stdin().is_terminal() {
        bail!(
            "`adage {cmd}` needs a person at a terminal (stdin is not a terminal). For CI, set \
             ADAGE_ALLOW_NONINTERACTIVE=1."
        );
    }
    Ok(())
}

/// The files a shared track may contain (Design §10.1). `scripts/` travels only with
/// repository-committed tracks, never in a direct share.
fn allowed(rel: &str) -> bool {
    matches!(rel, "track.toml" | "evidence.json" | "CHANGELOG.md" | "review.yaml")
        || ["routes/", "intents/"].iter().any(|d| {
            rel.strip_prefix(d).is_some_and(|f| !f.contains('/') && !f.starts_with('.') && f.ends_with(".yaml"))
        })
}

pub struct Listing {
    /// Track files, sorted, relative to the track directory.
    pub files: Vec<String>,
    /// Everything else, with the reason it is not part of the track.
    pub other: Vec<(String, &'static str)>,
}

/// List a track directory without following anything: a symlink anywhere refuses the track.
pub fn list_files(dir: &Path) -> Result<Listing> {
    let mut l = Listing { files: Vec::new(), other: Vec::new() };
    let mut stack = vec![(dir.to_path_buf(), 0)];
    while let Some((d, depth)) = stack.pop() {
        for e in std::fs::read_dir(&d).with_context(|| format!("reading {}", d.display()))? {
            let p = e?.path();
            let rel = p.strip_prefix(dir)?.to_string_lossy().replace('\\', "/");
            let meta = std::fs::symlink_metadata(&p)?;
            if meta.file_type().is_symlink() {
                bail!("{rel} is a symlink; a shared track may not contain symlinks");
            } else if meta.is_dir() && depth == 0 && (rel == "routes" || rel == "intents") {
                stack.push((p, depth + 1));
            } else if meta.is_dir() {
                l.other.push((format!("{rel}/"), why_not(&rel)));
            } else if rel == SIG {
                l.other.push((rel, "the signature, written by adage share"));
            } else if !meta.is_file() {
                bail!("{rel} is not a regular file");
            } else if allowed(&rel) {
                if meta.len() > MAX_FILE_BYTES {
                    bail!("{rel} is larger than {MAX_FILE_BYTES} bytes");
                }
                l.files.push(rel);
            } else {
                let why = why_not(&rel);
                l.other.push((rel, why));
            }
            if l.files.len() + l.other.len() > MAX_FILES {
                bail!("{} holds more than {MAX_FILES} files", dir.display());
            }
        }
    }
    l.files.sort();
    l.other.sort();
    Ok(l)
}

fn why_not(rel: &str) -> &'static str {
    if rel.split('/').any(|c| c.starts_with('.')) {
        "hidden"
    } else if rel == "scripts" || rel.starts_with("scripts/") {
        "scripts travel only with repository-committed tracks"
    } else {
        "not part of the track format"
    }
}

/// An installed track by name, or a track directory by path.
/// A learned track wins over an import of the same name.
pub(crate) fn find_track(root: &Path, name: &str) -> Result<PathBuf> {
    let mut stack = vec![root.to_path_buf()];
    let mut import = None;
    while let Some(d) = stack.pop() {
        if d.join("track.toml").is_file() {
            match io::read(&d) {
                Ok(t) if t.manifest.track.name == name && index::is_imported(&t) => import = Some(d),
                Ok(t) if t.manifest.track.name == name => return Ok(d),
                _ => {}
            }
            continue;
        }
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            if e.file_type().is_ok_and(|t| t.is_dir()) {
                stack.push(e.path());
            }
        }
    }
    if let Some(d) = import {
        return Ok(d);
    }
    let p = Path::new(name);
    if p.join("track.toml").is_file() {
        return Ok(p.to_path_buf());
    }
    bail!("no installed track named {name:?} in {} (see `adage status`)", root.display())
}

pub fn describe_step(step: &Step) -> String {
    match step {
        Step::Run { command } => format!("run `{command}`"),
        Step::ReadBatch { slot, max } => format!("read up to {max} files listed in {{{slot}}}"),
        Step::Avoid { avoid, instead, .. } => format!("avoid `{avoid}`, use `{instead}`"),
        Step::Advise { template, .. } => format!("advise ({template})"),
        Step::Sequence { commands } => {
            format!("run in order: {}", commands.iter().map(|c| format!("`{c}`")).collect::<Vec<_>>().join(", "))
        }
    }
}

const USAGE: &str = "usage: adage share <track-or-pack> --out <dir> [--key <path>] [--vendor] [--attest <results.jsonl>]... [--ack-scripts] [--repo-root <dir>] [--format pack|v1]";

pub fn share(args: &[String], paths: &Paths) -> Result<()> {
    require_person("share")?;
    let name = args.first().filter(|a| !a.starts_with("--")).context(USAGE)?;
    let out = PathBuf::from(crate::flag(args, "--out").context(USAGE)?);
    let key = crate::flag(args, "--key").map(PathBuf::from);
    match crate::flag(args, "--format").as_deref() {
        None | Some("pack") => crate::share_pack::share(name, &out, key.as_deref(), args, paths),
        Some("v1") => share_v1(name, &out, key, paths),
        Some(other) => bail!("--format {other}: expected pack or v1\n{USAGE}"),
    }
}

fn share_v1(name: &str, out: &Path, key: Option<PathBuf>, paths: &Paths) -> Result<()> {
    let out = out.to_path_buf();
    let src = find_track(&paths.tracks(), name)?;
    let track = io::read(&src).with_context(|| format!("{} fails validation", src.display()))?;
    let listing = list_files(&src)?;

    // Redaction and safety check, over exactly the bytes that will be written, before any are.
    let home = adage_store::home_dir().map(|h| h.to_string_lossy().into_owned());
    let mut contents = Vec::new();
    let mut findings = Vec::new();
    for rel in &listing.files {
        let bytes = std::fs::read(src.join(rel))?;
        match std::str::from_utf8(&bytes) {
            Ok(text) => findings.extend(redact::scan(rel, text, home.as_deref())),
            Err(_) => findings.push(redact::Finding {
                file: rel.clone(),
                line: 0,
                kind: "not UTF-8 text",
                excerpt: String::new(),
            }),
        }
        contents.push((rel.clone(), bytes));
    }
    if !findings.is_empty() {
        eprintln!("adage share: the redaction check found {} problem(s); nothing was written:", findings.len());
        for f in &findings {
            eprintln!("  {}:{}  {}  {}", f.file, f.line, f.kind, f.excerpt);
        }
        bail!("refusing to share {name}: generalise or remove the flagged text, then share again");
    }

    if out.exists() && (!out.is_dir() || std::fs::read_dir(&out)?.next().is_some()) {
        bail!("{} already exists and is not an empty directory", out.display());
    }
    let leaf = out.file_name().with_context(|| format!("--out {} needs a directory name", out.display()))?;
    let stage = out.with_file_name(format!(".{}.adage-{}", leaf.to_string_lossy(), std::process::id()));
    let _ = std::fs::remove_dir_all(&stage);
    let signed = write_and_sign(&stage, &contents, &track, key.as_deref());
    let (digest, signed) = match signed {
        Ok(v) => v,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&stage);
            return Err(e);
        }
    };
    if out.exists() {
        std::fs::remove_dir(&out)?;
    }
    std::fs::rename(&stage, &out)?;
    print_summary(&track, &out, &contents, &listing, &digest, &signed);
    Ok(())
}

fn write_and_sign(
    stage: &Path,
    contents: &[(String, Vec<u8>)],
    track: &Track,
    key: Option<&Path>,
) -> Result<(String, adage_sign::Signed)> {
    for (rel, bytes) in contents {
        let p = stage.join(rel);
        std::fs::create_dir_all(p.parent().unwrap())?;
        std::fs::write(p, bytes)?;
    }
    let copy = io::read(stage).context("the copied track fails validation")?;
    if &copy != track {
        bail!("the copied track differs from the source");
    }
    let digest = io::digest(stage)?;
    let sock = std::env::var_os("SSH_AUTH_SOCK").filter(|s| !s.is_empty()).map(PathBuf::from);
    // A passphrase-protected key file is decrypted with a passphrase asked for at the terminal.
    let ask = |p: &Path| crate::tty::secret(&format!("Enter passphrase for {}: ", p.display()));
    let signed =
        adage_sign::sign_with(&adage_sign::message(&digest)?, adage_sign::NAMESPACE, sock.as_deref(), key, &ask)
            .context("signing the track")?;
    std::fs::write(stage.join(SIG), &signed.pem)?;
    Ok((digest, signed))
}

fn print_summary(
    t: &Track,
    out: &Path,
    contents: &[(String, Vec<u8>)],
    l: &Listing,
    digest: &str,
    s: &adage_sign::Signed,
) {
    let m = &t.manifest;
    println!("adage share: {} @ {}  (repo {})\n", m.track.name, m.track.version, m.scope.repo);
    println!("Included — exactly these files, plus {SIG}:");
    for (rel, bytes) in contents {
        println!("  {rel:<40} {:>7} bytes", bytes.len());
    }
    println!("\nRoutes (templates and aggregate statistics only):");
    for r in &t.routes {
        let e = &r.evidence;
        println!(
            "  {:<22} {:<6} {:<11} {}  [{}/{} sessions]",
            r.id,
            format!("{:?}", r.strength).to_lowercase(),
            format!("{:?}", r.effect).to_lowercase(),
            describe_step(&r.step),
            e.support,
            e.opportunities
        );
    }
    if !l.other.is_empty() {
        println!("\nNot included:");
        for (rel, why) in l.other.iter().filter(|(r, _)| r != SIG) {
            println!("  {rel:<40} {why}");
        }
    }
    println!("\nChecked: no absolute paths, home-directory paths, or secret-looking strings in any file.");
    println!("Digest:  sha256 {digest}");
    let how = match s.method {
        adage_sign::Method::Agent => "via ssh-agent",
        adage_sign::Method::KeyFile => "with key file",
    };
    println!(
        "Signed:  {} ({}, {how}), namespace {}",
        adage_sign::fingerprint(&s.key),
        s.key.algorithm(),
        adage_sign::NAMESPACE
    );
    println!("Wrote:   {}", out.display());
    let principal =
        Some(s.key.comment()).filter(|c| !c.is_empty() && !c.contains(char::is_whitespace)).unwrap_or("adage-signer");
    println!(
        "\nRecipients trust this signer with a line in <repo>/.adage/trusted-signers or $ADAGE_HOME/trusted-signers:\n  {principal} namespaces=\"{}\" {}",
        adage_sign::NAMESPACE,
        adage_sign::key_line(&s.key)
    );
}
