//! `adage share <track-or-pack> --out <dir> [--key <path>] [--vendor] [--attest <results.jsonl>]...
//! [--ack-scripts] [--repo-root <dir>]`
//! (Design §10.7, §10.8; Track C §1.6): emit a signed **pack** (format `adage-pack/1`).
//!
//! The source is a learned track (converted to a pack: its live routes, repository scripts pinned
//! L-ref against `--repo-root <dir>` or else the repository `share` runs in), an installed pack by name, or a pack or track
//! directory. Every byte of every pack file, vendored scripts included, goes through the
//! redaction check; vendored scripts that name URLs or hosts, run network tools, use absolute
//! paths or read secret environment variables are listed and need `--ack-scripts`. Approvals,
//! `review.yaml`, proposals and `trust.json` are never part of a pack. The pack digest is signed
//! into `pack.sig` in namespace `adage-pack`.
//!
//! Also the pieces other commands share: the person-at-a-terminal guard (review finding F11), the
//! files of a track directory, and how a step is described to people.

use adage_store::Paths;
use adage_tracks::{index, io, Step};
use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};

const MAX_FILES: usize = 256;
const MAX_FILE_BYTES: u64 = 1 << 20;

/// Review finding F11: commands that publish or change trust refuse to run inside an agent session
/// or without a terminal. `ADAGE_ALLOW_NONINTERACTIVE=1` lifts only the terminal requirement (for
/// CI and tests); nothing lifts the agent-session refusal. Environment detection is a guard against
/// an agent being steered into these commands, not a sandbox.
pub fn require_person(cmd: &str) -> Result<()> {
    adage_app::person::require(cmd).map(drop)
}

/// The files of a track directory that go into a pack (Design §10.1). `scripts/` travels only
/// with repository-committed tracks, never in a direct share.
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
    Err(crate::hint(
        format!("no installed track named {name:?} in {}", root.display()),
        "use a track's name as `adage status` shows it (without @version), or pass a track directory",
    ))
}

pub fn describe_step(step: &Step) -> String {
    match step {
        Step::Run { command } => format!("run `{command}`"),
        Step::ReadBatch { slot, max } => format!("read up to {max} files listed in {{{slot}}}"),
        Step::Avoid { avoid, instead, .. } => format!("avoid `{avoid}`, use `{instead}`"),
        Step::Advise { template, slots } => match slots.get("path") {
            Some(p) => format!("advise ({template}: {p})"),
            None => format!("advise ({template})"),
        },
        Step::Sequence { commands } => {
            format!("run in order: {}", commands.iter().map(|c| format!("`{c}`")).collect::<Vec<_>>().join(", "))
        }
        Step::Card { card } => format!(
            "procedure card: {} file(s), {} command(s), from {} earlier session(s) of this family ({} passed)",
            card.files.len(),
            card.commands.len() + card.avoid.len(),
            card.sessions,
            card.passed
        ),
        Step::Skill { skill, args: Some(a) } => format!("use the `{skill}` skill with `{a}`"),
        Step::Skill { skill, args: None } => format!("use the `{skill}` skill"),
    }
}

/// The redaction check's findings as an error: one cause per finding; nothing was written.
pub(crate) fn redaction_refusal(name: &str, findings: &[crate::redact::Finding]) -> anyhow::Error {
    let causes = findings
        .iter()
        .map(|f| {
            let at = if f.line > 0 { format!("{}:{}", f.file, f.line) } else { f.file.clone() };
            if f.excerpt.is_empty() {
                format!("{at} · {}", f.kind)
            } else {
                format!("{at} · {} · {}", f.kind, f.excerpt)
            }
        })
        .collect();
    crate::refusal(
        format!(
            "refusing to share {name}: the redaction check found {}; nothing was written",
            crate::words::count(findings.len(), "problem")
        ),
        causes,
        "generalise or remove the flagged text, then share again",
    )
}

const USAGE: &str = "usage: adage share <track-or-pack> --out <dir> [--key <path>] [--vendor] [--attest <results.jsonl>]... [--ack-scripts] [--repo-root <dir>]";

pub fn share(args: &[String], paths: &Paths) -> Result<()> {
    require_person("share")?;
    let name = args.first().filter(|a| !a.starts_with("--")).context(USAGE)?;
    let out = PathBuf::from(crate::helpers::flag(args, "--out").context(USAGE)?);
    let key = crate::helpers::flag(args, "--key").map(PathBuf::from);
    if let Some(k) = key.as_deref().filter(|k| !k.is_file()) {
        return Err(crate::hint(
            format!("no signing key at {}", k.display()),
            "pass --key <private key file> (e.g. ~/.ssh/id_ed25519), or leave it out to sign with ssh-agent",
        ));
    }
    crate::share_pack::share(name, &out, key.as_deref(), args, paths)
}
