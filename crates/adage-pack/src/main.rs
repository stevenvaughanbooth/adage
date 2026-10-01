//! `adage-pack`: a small debugging tool for track packs. It never executes anything a pack names.
//!
//! ```text
//! adage-pack inspect <pack-dir>                 validate and summarise; print CLOSURE
//! adage-pack verify <pack-dir> <allowed_signers>  validate, then check pack.sig (namespace adage-pack)
//! adage-pack digest <pack-dir>                  the pack digest and every route exec digest
//! adage-pack seal <pack-dir>                    write a hand-written pack in canonical form, CLOSURE derived
//! adage-pack marketplace <repo-dir> [--pin]     check a marketplace's pins (--pin: rewrite them)
//! ```

use adage_pack::marketplace::{self, Marketplace};
use adage_pack::{digest_dir, verify_dir, Pack, NAMESPACE};
use anyhow::{bail, Context, Result};
use std::path::PathBuf;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Err(e) = run(&args) {
        eprintln!("adage-pack: {e:#}");
        std::process::exit(1);
    }
}

const USAGE: &str = "usage: adage-pack inspect <pack> | verify <pack> <allowed_signers> | digest <pack> | seal <pack> | marketplace <repo> [--pin]";

fn run(args: &[String]) -> Result<()> {
    let arg = |i: usize| args.get(i).map(PathBuf::from).context(USAGE);
    match args.first().map(String::as_str) {
        Some("inspect") => {
            let dir = arg(1)?;
            let p = Pack::load(&dir)?;
            let m = &p.manifest;
            println!(
                "{} @ {}  ({:?}, exec requested {:?})",
                m.pack.name, m.pack.version, m.pack.origin, m.exec.requested
            );
            println!("applicability: {:?}", m.applicability);
            for r in &p.routes {
                println!(
                    "  {:<24} {:<10} {:<7} exec {}",
                    r.id,
                    r.shape.as_str(),
                    format!("{:?}", r.strength).to_lowercase(),
                    &p.route_exec_digest(&r.id).unwrap_or_default()[..16]
                );
            }
            println!("digest: {}\n", digest_dir(&dir)?);
            print!("{}", p.closure()?.render());
        }
        Some("verify") => {
            let dir = arg(1)?;
            Pack::load(&dir)?;
            let text = std::fs::read_to_string(arg(2)?)?;
            let signers = adage_sign::allowed_signers::parse(&text)?;
            let who = verify_dir(&dir, &signers, std::time::SystemTime::now())?;
            println!(
                "ok: signed by {:?} ({}), namespace {NAMESPACE}",
                who.principals,
                adage_sign::fingerprint(&who.key)
            );
        }
        Some("digest") => {
            let dir = arg(1)?;
            let p = Pack::load(&dir)?;
            println!("pack {}", digest_dir(&dir)?);
            for r in &p.routes {
                println!("route {} {}", r.id, p.route_exec_digest(&r.id).unwrap_or_default());
            }
        }
        Some("seal") => {
            let dir = arg(1)?;
            let p = Pack::seal(&dir)?;
            println!("sealed {}@{}: digest {}", p.manifest.pack.name, p.manifest.pack.version, digest_dir(&dir)?);
        }
        Some("marketplace") => {
            let root = arg(1)?;
            let path = root.join(marketplace::MANIFEST);
            let mut m =
                Marketplace::parse(&std::fs::read_to_string(&path).with_context(|| path.display().to_string())?)?;
            let pin = args.iter().any(|a| a == "--pin");
            let mut stale = 0;
            for e in &mut m.packs {
                let dir = root.join(&e.source);
                let p = Pack::load(&dir)?;
                let (version, digest) = (p.manifest.pack.version.clone(), digest_dir(&dir)?);
                let ok = p.manifest.pack.name == e.name && version == e.version && digest == e.digest;
                println!("{} {}@{} {digest}", if ok { "ok   " } else { "stale" }, e.name, version);
                if !ok {
                    stale += 1;
                    if p.manifest.pack.name != e.name {
                        bail!(
                            "{} lists {} at {}, whose pack.toml names {}",
                            path.display(),
                            e.name,
                            e.source,
                            p.manifest.pack.name
                        );
                    }
                    (e.version, e.digest) = (version, digest);
                }
            }
            if pin && stale > 0 {
                std::fs::write(&path, toml::to_string_pretty(&m)?)?;
                println!("pinned {stale} pack(s) in {}", path.display());
            } else if stale > 0 {
                bail!("{stale} pack(s) differ from their pins: bump each changed pack's version, then run with --pin");
            }
        }
        _ => bail!(USAGE),
    }
    Ok(())
}
