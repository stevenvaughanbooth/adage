//! `adage train …`: compile a skill into reflexes (docs/training.md).
//!
//! Compiling replays commands in adage-exec's sandbox and training sessions drive an agent
//! harness, and the `adage` binary never starts either itself (Q4a). So `adage train` hands over
//! to the separate `adage-train` binary, with the same arguments: on Unix it replaces this process
//! (`exec`), elsewhere it runs it and exits with its status. It looks for `adage-train` beside this
//! binary, then on `PATH`.
//!
//! `adage train accept <name>` is the exception: it installs the pack a compile staged, which is
//! what `adage` does for packs ([`crate::packs`]), and it runs nothing.

use crate::packs;
use adage_pack::Pack;
use adage_store::Paths;
use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};

fn exe() -> &'static str {
    if cfg!(windows) {
        "adage-train.exe"
    } else {
        "adage-train"
    }
}

/// `adage-train` beside the running `adage`, else the bare name for a `PATH` lookup.
fn locate() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join(exe())))
        .filter(|p| p.is_file())
        .unwrap_or_else(|| PathBuf::from(exe()))
}

pub fn train(args: &[String], paths: &Paths) -> Result<()> {
    if args.first().map(String::as_str) == Some("accept") {
        return accept(&args[1..], paths);
    }
    let bin = locate();
    let mut cmd = std::process::Command::new(&bin);
    cmd.args(args);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let e = cmd.exec();
        Err(e).with_context(|| {
            format!("cannot run {}: install adage-train beside adage or on PATH (docs/training.md)", bin.display())
        })
    }
    #[cfg(not(unix))]
    {
        let status = cmd.status().with_context(|| {
            format!("cannot run {}: install adage-train beside adage or on PATH (docs/training.md)", bin.display())
        })?;
        std::process::exit(status.code().unwrap_or(1));
    }
}

const ACCEPT_USAGE: &str = "usage: adage train accept <name> [--key <private key file>]";

/// The staged review's fields `accept` needs (the rest is for people).
#[derive(serde::Deserialize)]
struct Review {
    schema: String,
    pack: String,
    digest: String,
    root: String,
    text: String,
}

/// `adage train accept <name>`: install the pack `adage train <skill>` staged under
/// `$ADAGE_HOME/train/skills/<name>/`, exactly as reviewed (its digest must be the one the review
/// recorded), signed when a key is at hand, and bound to the repository it was compiled against,
/// at the strengths the review showed (a verified reflex may fire; the rest are prime).
fn accept(args: &[String], paths: &Paths) -> Result<()> {
    crate::share::require_person("train accept")?;
    let name = args.first().filter(|a| !a.starts_with("--")).context(ACCEPT_USAGE)?;
    let key = args.iter().position(|a| a == "--key").and_then(|i| args.get(i + 1)).map(PathBuf::from);
    if !name.bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-') || name.is_empty() {
        bail!("{name:?} is not a compiled skill's name (the one `adage train <skill>` printed)");
    }
    let dir = paths.root.join("train").join("skills").join(name);
    let review_path = dir.join("review.json");
    let review: Review = serde_json::from_slice(
        &std::fs::read(&review_path)
            .with_context(|| format!("no compiled skill {name} (run `adage train <SKILL.md>` first)"))?,
    )
    .with_context(|| format!("{} is not a review", review_path.display()))?;
    if review.schema != "adage-train-review/1" {
        bail!("{} has schema {:?}; compile the skill again", review_path.display(), review.schema);
    }
    if review.digest.is_empty() {
        bail!("nothing of {name} compiled, so there is no pack to accept");
    }
    // Exactly the files that were reviewed.
    let files =
        adage_pack::validate::read_dir_files(&dir.join("pack")).map_err(|e| anyhow::anyhow!("refusing: {e:#}"))?;
    let digest = adage_pack::pack_digest(&files);
    if digest != review.digest {
        bail!("refusing: the staged pack changed after its review; compile the skill again and review it");
    }
    let pack = Pack::from_files(&files).map_err(|e| anyhow::anyhow!("refusing: the pack fails validation: {e:#}"))?;
    if pack.manifest.pack.origin != adage_pack::PackOrigin::Trained || pack.manifest.pack.name != review.pack {
        bail!("refusing: {} is not the trained pack its review names", dir.join("pack").display());
    }
    pack.procedure.as_ref().context("refusing: a compiled skill's pack carries its procedure")?;
    let _tracks = adage_store::lock::exclusive(&paths.tracks_lock())?;
    let dest = packs::pack_dir(paths, &pack.manifest.pack.name);
    let stage = paths.root.join("staging").join(format!("accept-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&stage);
    pack.write(&stage)?;
    if adage_pack::digest_dir(&stage)? != digest {
        let _ = std::fs::remove_dir_all(&stage);
        bail!("the staged copy of the pack differs from what was reviewed");
    }
    // Signed when a key is at hand (ssh-agent, or --key), so it is ready to share; unsigned
    // otherwise: it was compiled and reviewed here, and `adage share` signs it when it travels.
    let sock = std::env::var_os("SSH_AUTH_SOCK").filter(|s| !s.is_empty()).map(PathBuf::from);
    let signed = if sock.is_some() || key.is_some() {
        let ask = |p: &Path| crate::tty::secret(&format!("Enter passphrase for {}: ", p.display()));
        match adage_sign::sign_with(
            &adage_sign::message(&digest)?,
            adage_sign::PACK_NAMESPACE,
            sock.as_deref(),
            key.as_deref(),
            &ask,
        ) {
            Ok(s) => {
                std::fs::write(stage.join(adage_pack::validate::SIG), &s.pem)?;
                Some(adage_sign::fingerprint(&s.key))
            }
            Err(e) if key.is_some() => {
                let _ = std::fs::remove_dir_all(&stage);
                return Err(e.context("signing the pack"));
            }
            Err(_) => None,
        }
    } else {
        None
    };
    packs::policy_check(paths, &pack, signed.as_deref())?;
    if dest.exists() {
        std::fs::remove_dir_all(&dest)?;
    }
    std::fs::create_dir_all(dest.parent().context("an install directory has a parent")?)?;
    std::fs::rename(&stage, &dest)?;
    packs::record_accepted(paths, &pack.manifest.pack.name, &digest)?;
    let root = PathBuf::from(&review.root);
    let cwd_root = std::env::current_dir().ok().and_then(|d| adage_core::repo::find_root(&d));
    let bind_root = if root.is_dir() { Some(root) } else { cwd_root };
    let verified = match &signed {
        Some(fp) => format!("compiled and reviewed on this machine (adage train accept), signed by {fp}"),
        None => "compiled and reviewed on this machine (adage train accept), unsigned".to_string(),
    };
    let bound = packs::bind_installed(paths, &pack, &digest, &verified, &[], bind_root.as_deref())?;
    let all = adage_tracks::io::read_all(&paths.tracks())?;
    let (index, _) = adage_tracks::Index::compile_root(&paths.tracks(), &all);
    index.write_atomic(&paths.index())?;

    let m = &pack.manifest;
    println!("adage train accept: {} {}  (sha256 {})", m.pack.name, m.pack.version, &digest[..16]);
    println!("  installed  {}", dest.display());
    match &signed {
        Some(fp) => println!("  signed     {fp}, namespace {}", adage_sign::PACK_NAMESPACE),
        None => println!("  signed     no (no ssh-agent key and no --key); `adage share` signs it when you share it"),
    }
    for b in &bound {
        if let adage_pack::bind::Applies::No(why) = &b.binding.applies {
            println!("  repo       {}: not applicable ({why})", b.binding.repo);
            continue;
        }
        let routes = b.track.as_ref().map(|t| t.routes.as_slice()).unwrap_or_default();
        println!(
            "  repo       {}: {} reflexes live, {} inert",
            b.binding.repo,
            b.binding.live().count(),
            b.binding.inert().count()
        );
        for r in routes {
            let strength = format!("{:?}", r.strength).to_lowercase();
            let nascent = if r.state == adage_tracks::RouteState::Nascent { " (nascent until approved)" } else { "" };
            println!("    {:<8} {}  {}{nascent}", strength, r.id, crate::share::describe_step(&r.step));
        }
        for r in b.binding.inert() {
            println!("    inert    {}: {}", r.id, r.status.reason().unwrap_or(""));
        }
    }
    println!("index generation {}, {} reflexes", index.generation, index.routes.len());
    let _ = review.text;
    Ok(())
}
