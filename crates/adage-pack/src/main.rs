//! `adage-pack`: a small debugging tool for track packs. It never executes anything a pack names.
//!
//! ```text
//! adage-pack inspect <pack-dir>                 validate and summarise; print CLOSURE
//! adage-pack verify <pack-dir> <allowed_signers>  validate, then check pack.sig (namespace adage-pack)
//! adage-pack digest <pack-dir>                  the pack digest and every route exec digest
//! ```

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

const USAGE: &str = "usage: adage-pack inspect <pack> | verify <pack> <allowed_signers> | digest <pack>";

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
        _ => bail!(USAGE),
    }
    Ok(())
}
