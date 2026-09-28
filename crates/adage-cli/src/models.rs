//! `adage models fetch | install <dir> | status` — the embedding model for intent families.
//!
//! The model is never bundled: `fetch` downloads the pinned upstream files with `curl` (argv, no
//! shell) into a scratch directory, and `install` verifies every file's SHA-256 against the pinned
//! digest before anything is written under `$ADAGE_HOME/models`. Offline machines download the
//! three files any other way and run `adage models install <dir>`. Without a model, everything
//! else works: `adage condition` skips intent families and hooks never fire intent routes.

use adage_intent::model;
use adage_store::Paths;
use adage_term::{Line, Out, Role};
use anyhow::{bail, Context, Result};
use std::path::Path;

pub fn run(args: &[String], paths: &Paths) -> Result<()> {
    match args.first().map(String::as_str) {
        Some("fetch") => fetch(paths),
        Some("install") => {
            let dir = args
                .get(1)
                .context("usage: adage models install <dir with model.safetensors, tokenizer.json, config.json>")?;
            install(Path::new(dir), paths)
        }
        Some("status") | None => {
            let mut o = Out::stdout();
            match model::status(&paths.models()) {
                Ok(m) => {
                    o.header("models", &["installed"])?;
                    o.blank()?;
                    o.kv("model", Line::new().text(m.id).sep().quiet(format!("{} x {}", m.vocab_size, m.dim)))?;
                    o.kv("in", Line::new().text(crate::detect::tilde(&model::dir_in(&paths.models()))))?;
                    o.kv("enables", Line::new().quiet("intent families: similar prompts share a reflex"))?;
                }
                Err(e) => {
                    o.header("models", &["not installed"])?;
                    o.blank()?;
                    o.kv("model", Line::new().push(Role::Prime, format!("{e:#}")))?;
                    o.kv("without it", Line::new().quiet("everything works except intent families"))?;
                    o.blank()?;
                    o.next("adage models fetch", "downloads and verifies it (31 MB, SHA-256 checked)")?;
                    o.also("adage models install <dir>", "offline: from files you downloaded yourself")?;
                }
            }
            Ok(())
        }
        Some(other) => bail!("unknown `adage models {other}`; use fetch, install <dir>, or status"),
    }
}

fn install(dir: &Path, paths: &Paths) -> Result<()> {
    if !dir.is_dir() {
        return Err(crate::hint(
            format!("{} is not a directory", dir.display()),
            "download model.safetensors, tokenizer.json and config.json into one directory, or run `adage models fetch`",
        ));
    }
    let m = model::install(dir, &paths.models())?;
    let mut o = Out::stdout();
    o.header("models install", &["SHA-256 verified"])?;
    o.blank()?;
    installed(&mut o, &m.id, paths)
}

fn installed<W: std::io::Write>(o: &mut Out<W>, id: &str, paths: &Paths) -> Result<()> {
    o.kv("installed", Line::new().text(id))?;
    o.kv("in", Line::new().text(crate::detect::tilde(&model::dir_in(&paths.models()))))?;
    o.blank()?;
    o.next("adage condition", "forms intent families from your traces")?;
    Ok(())
}

fn fetch(paths: &Paths) -> Result<()> {
    let mut o = Out::stdout();
    let _ = o.header("models fetch", &["pinned model, SHA-256 verified"]);
    let _ = o.blank();
    let m = fetch_with(paths, &mut |p| {
        if p.file_done == 0 {
            let _ = o.kv("fetching", Line::new().text(p.url).sep().quiet(format!("{} bytes", p.file_bytes)));
        }
    })?;
    installed(&mut o, &m, paths)
}

/// Download progress, reported about five times a second while `curl` runs.
pub(crate) struct Progress<'a> {
    pub(crate) url: &'a str,
    pub(crate) file_done: u64,
    pub(crate) file_bytes: u64,
    /// Bytes of every file so far, and of all of them.
    pub(crate) done: u64,
    pub(crate) total: u64,
}

/// Download and install the pinned model, reporting progress; returns its id. `curl`'s own output
/// is captured, so a terminal UI can call this.
pub(crate) fn fetch_with(paths: &Paths, progress: &mut dyn FnMut(&Progress)) -> Result<String> {
    use std::process::Stdio;
    let tmp = paths.models().join(format!(".download{}", std::process::id()));
    std::fs::create_dir_all(&tmp)?;
    let total: u64 = model::FILES.iter().map(|f| f.bytes).sum();
    let result = (|| -> Result<String> {
        let mut before = 0u64;
        for f in &model::FILES {
            let url = model::url(f.name);
            let out = tmp.join(f.name);
            let report = |progress: &mut dyn FnMut(&Progress), done: u64| {
                progress(&Progress { url: &url, file_done: done, file_bytes: f.bytes, done: before + done, total })
            };
            report(progress, 0);
            let mut child = std::process::Command::new("curl")
                .args(["-fsSL", "--proto", "=https", "--max-filesize"])
                .arg((f.bytes + 4096).to_string())
                .arg("-o")
                .arg(&out)
                .arg(&url)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .spawn()
                .context("could not run curl; download the files yourself and use `adage models install <dir>`")?;
            let status = loop {
                if let Some(s) = child.try_wait()? {
                    break s;
                }
                let done = std::fs::metadata(&out).map(|m| m.len()).unwrap_or(0);
                report(progress, done.max(1).min(f.bytes));
                std::thread::sleep(std::time::Duration::from_millis(200));
            };
            if !status.success() {
                let mut err = String::new();
                if let Some(mut e) = child.stderr.take() {
                    use std::io::Read;
                    let _ = e.read_to_string(&mut err);
                }
                let err = err.trim();
                bail!(
                    "downloading {} failed ({status}){}",
                    f.name,
                    if err.is_empty() { String::new() } else { format!(": {err}") }
                );
            }
            report(progress, f.bytes);
            before += f.bytes;
        }
        Ok(model::install(&tmp, &paths.models())?.id)
    })();
    let _ = std::fs::remove_dir_all(&tmp);
    result
}
