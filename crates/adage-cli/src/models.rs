//! `adage models fetch | install <dir> | status` — the embedding model for intent families.
//!
//! The model is never bundled: `fetch` downloads the pinned upstream files with `curl` (argv, no
//! shell) into a scratch directory, and `install` verifies every file's SHA-256 against the pinned
//! digest before anything is written under `$ADAGE_HOME/models`. Offline machines download the
//! three files any other way and run `adage models install <dir>`. Without a model, everything
//! else works: `adage learn` skips intent families and hooks never fire intent routes.

use adage_intent::model;
use adage_store::Paths;
use anyhow::{bail, Context, Result};
use std::path::Path;

pub fn run(args: &[String], paths: &Paths) -> Result<()> {
    match args.first().map(String::as_str) {
        Some("fetch") => fetch(paths),
        Some("install") => {
            let dir = args.get(1).context("usage: adage models install <dir with model.safetensors, tokenizer.json, config.json>")?;
            install(Path::new(dir), paths)
        }
        Some("status") | None => {
            match model::status(&paths.models()) {
                Ok(m) => println!("intent model: {} ({} x {}) in {}", m.id, m.vocab_size, m.dim, model::dir_in(&paths.models()).display()),
                Err(e) => println!("intent model: {e:#}"),
            }
            Ok(())
        }
        Some(other) => bail!("unknown `adage models {other}`; use fetch, install <dir>, or status"),
    }
}

fn install(dir: &Path, paths: &Paths) -> Result<()> {
    let m = model::install(dir, &paths.models())?;
    println!("installed {} into {}", m.id, model::dir_in(&paths.models()).display());
    Ok(())
}

fn fetch(paths: &Paths) -> Result<()> {
    let tmp = paths.models().join(format!(".download{}", std::process::id()));
    std::fs::create_dir_all(&tmp)?;
    let result = (|| -> Result<()> {
        for f in &model::FILES {
            let url = model::url(f.name);
            println!("fetching {} ({} bytes)", url, f.bytes);
            let status = std::process::Command::new("curl")
                .args(["-fsSL", "--proto", "=https", "--max-filesize"])
                .arg((f.bytes + 4096).to_string())
                .arg("-o")
                .arg(tmp.join(f.name))
                .arg(&url)
                .status()
                .context("could not run curl; download the files yourself and use `adage models install <dir>`")?;
            if !status.success() {
                bail!("downloading {} failed ({status})", f.name);
            }
        }
        install(&tmp, paths)
    })();
    let _ = std::fs::remove_dir_all(&tmp);
    result
}
