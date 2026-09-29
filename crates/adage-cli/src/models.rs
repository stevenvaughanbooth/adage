//! `adage models [status] | fetch [<key>] | install <dir> [<key>] | use <key|auto|hashed>` — the
//! static embedding models.
//!
//! **Nothing to do by default.** The `adage` binary carries potion-base-8M quantised to int8
//! (`adage_intent::model`, "The bundled model"), so a fresh install embeds with no download. `fetch`
//! installs a model's pinned upstream files instead (full precision, or another model:
//! `potion-code-16m-v2`), and `use` selects which one hooks and the learner embed with.
//!
//! **Where `fetch` downloads from.** The pinned Hugging Face revision (or `ADAGE_MODEL_BASE_URL`, a
//! mirror holding the same files), file by file with `curl` (argv, no shell); when that fails, the
//! model's npm package, whose tarball is pinned by SHA-256 and holds the same files byte for byte
//! (`registry.npmjs.org` is reachable where Hugging Face is often blocked). Every file's SHA-256 is
//! checked against its pin before anything is written under `$ADAGE_HOME/models`. Offline machines
//! download the files any other way (or unpack the npm package) and run `adage models install
//! <dir>`.
//!
//! **Failures are never silent** (docs/zero-config.md §5). A failed download is not retried by
//! itself: it is classified from curl's exit code and message ([`classify`]) and recorded in
//! `models/.fetch-failed.json` (`{class, host, at, detail}`), which `adage doctor` and the status
//! card read, next to what the process embeds with instead (the bundled model, or the explicit
//! hashed fallback).

use adage_intent::model::{self, Spec};
use adage_store::Paths;
use anyhow::{bail, Context, Result};
use std::path::Path;

pub fn run(args: &[String], paths: &Paths) -> Result<()> {
    match args.first().map(String::as_str) {
        Some("fetch") => fetch(args.get(1).map(String::as_str), paths),
        Some("install") => {
            let dir = args.get(1).context(
                "usage: adage models install <dir with model.safetensors and tokenizer.json, or an unpacked npm package> [<key>]",
            )?;
            install(Path::new(dir), args.get(2).map(String::as_str), paths)
        }
        Some("use") => {
            let which = args.get(1).context("usage: adage models use <auto|hashed|potion-base-8m|potion-code-16m-v2>")?;
            let which = which.trim().to_lowercase();
            if !matches!(which.as_str(), "auto" | "hashed") && model::spec(&which).is_none() {
                bail!("unknown model {which:?}; use auto, hashed, or one of: {}", keys());
            }
            std::fs::create_dir_all(paths.models())?;
            adage_store::config::write_atomic(&paths.models().join(model::ACTIVE_FILE), which.as_bytes())?;
            let r = model::resolve(&paths.models(), false);
            println!("embedding with {}", r.describe());
            println!("run `adage condition` so what was learned with another model is learned again with this one.");
            Ok(())
        }
        Some("status") | None => {
            print!("{}", status_text(paths));
            Ok(())
        }
        Some(other) => bail!("unknown `adage models {other}`; use fetch [<key>], install <dir> [<key>], use <key>, or status"),
    }
}

fn keys() -> String {
    model::SPECS.iter().map(|s| s.key).collect::<Vec<_>>().join(", ")
}

/// What `adage models status` prints: what this process embeds with, then every known model.
pub fn status_text(paths: &Paths) -> String {
    let root = paths.models();
    let r = model::resolve(&root, false);
    let mut s = format!("embeddings: {}\n", r.describe());
    if matches!(r.source, model::Source::Bundled) {
        match model::bundled_verified() {
            Ok(()) => s.push_str("  bundled table and vocabulary match their pinned SHA-256\n"),
            Err(e) => s.push_str(&format!("  {e:#}\n")),
        }
    }
    for spec in model::SPECS {
        let dir = root.join(spec.key);
        let line = match model::status_of(&dir) {
            Ok(m) => format!("installed, {} x {}, checksums verified", m.vocab_size, m.dim),
            Err(_) if !dir.join("manifest.json").exists() => {
                format!("not installed (`adage models fetch {}`, {:.0} MB)", spec.key, spec.bytes() as f64 / 1e6)
            }
            Err(e) => format!("installed but unusable: {e:#}"),
        };
        s.push_str(&format!("  {:<20} {} ({}): {line}\n", spec.key, spec.id(), spec.licence));
    }
    if let Some(f) = fetch_failed(paths) {
        s.push_str(&format!(
            "  last download failed: {}\n",
            explain(
                f["class"].as_str().unwrap_or("error"),
                f["host"].as_str().unwrap_or("the model host"),
                f["detail"].as_str().unwrap_or("")
            )
        ));
    }
    s
}

/// The spec of a key given on the command line (the default model when none is).
fn spec_arg(key: Option<&str>) -> Result<&'static Spec> {
    match key {
        None => Ok(&model::BASE_8M),
        Some(k) => model::spec(k).with_context(|| format!("unknown model {k:?}; known: {}", keys())),
    }
}

fn install(dir: &Path, key: Option<&str>, paths: &Paths) -> Result<()> {
    // Without a key: the known model whose pinned table is in `dir`.
    let spec = match key {
        Some(_) => spec_arg(key)?,
        None => model::SPECS
            .iter()
            .copied()
            .find(|s| model::verify(s, dir).is_ok())
            .with_context(|| format!("{} holds none of the pinned models ({})", dir.display(), keys()))?,
    };
    let m = model::install_spec(spec, dir, &paths.models())?;
    let _ = std::fs::remove_file(paths.models().join(FETCH_FAILED));
    println!("installed {} into {}", m.id, paths.models().join(spec.key).display());
    Ok(())
}

fn fetch(key: Option<&str>, paths: &Paths) -> Result<()> {
    let spec = spec_arg(key)?;
    let m = fetch_spec(spec, paths, &mut |p| {
        if p.file_done == 0 {
            println!("fetching {} ({} bytes)", p.url, p.file_bytes);
        }
    })?;
    println!("installed {} into {}", m, paths.models().join(spec.key).display());
    if spec.key != model::BUNDLED_SPEC.key && model::selection(&paths.models()) != spec.key {
        println!("`adage models use {}` makes hooks and the learner embed with it.", spec.key);
    }
    Ok(())
}

/// Written under `$ADAGE_HOME/models` when a download fails; removed when one succeeds.
pub const FETCH_FAILED: &str = ".fetch-failed.json";

/// Why a download failed, from curl's exit code and message: `offline` (6, 7: no network or
/// name resolution), `blocked` (a 403 or 407 from a proxy or policy: 22 or 56), `tls` (35, 60:
/// TLS interception; set `CURL_CA_BUNDLE` or `SSL_CERT_FILE`), `timeout` (28), `http` (another
/// HTTP error), or `error`.
pub fn classify(code: Option<i32>, stderr: &str) -> &'static str {
    let blocked = stderr.contains(" 403") || stderr.contains(" 407");
    match code {
        Some(6 | 7) => "offline",
        Some(22 | 56) if blocked => "blocked",
        Some(35 | 60) => "tls",
        Some(28) => "timeout",
        Some(22) => "http",
        _ => "error",
    }
}

/// One sentence for a classified failure, with its fix.
pub fn explain(class: &str, host: &str, detail: &str) -> String {
    let status = if detail.contains("407") { "407" } else { "403" };
    match class {
        "blocked" => format!("{host} blocked by proxy ({status}). Use a mirror (ADAGE_MODEL_BASE_URL) or adage models install <dir>."),
        "offline" => format!("{host} not reachable (no network). Run adage models fetch when online, or adage models install <dir>."),
        "tls" => format!("TLS to {host} was intercepted. Set CURL_CA_BUNDLE or SSL_CERT_FILE to your proxy's CA and run adage models fetch again."),
        "timeout" => format!("{host} timed out. Run adage models fetch again later, or adage models install <dir>."),
        _ => format!("downloading from {host} failed ({detail}). Use a mirror (ADAGE_MODEL_BASE_URL) or adage models install <dir>."),
    }
}

/// The recorded failure, if the last download failed and no model has been installed since.
pub fn fetch_failed(paths: &Paths) -> Option<serde_json::Value> {
    let v: serde_json::Value = serde_json::from_slice(&std::fs::read(paths.models().join(FETCH_FAILED)).ok()?).ok()?;
    Some(v)
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

/// A failed download: (class, host, curl's message).
struct Failed {
    class: &'static str,
    host: String,
    detail: String,
}

/// Download `url` to `out` with curl (at most `max` bytes), reporting progress. The one place
/// `adage` starts a download.
fn curl(url: &str, out: &Path, max: u64, report: &mut dyn FnMut(u64)) -> Result<Result<(), Failed>> {
    use std::process::Stdio;
    report(0);
    let mut child = std::process::Command::new("curl")
        .args(["-fsSL", "--proto", "=https", "--connect-timeout", "20", "--max-filesize"])
        .arg((max + 4096).to_string())
        .arg("-o")
        .arg(out)
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .context("could not run curl; download the files yourself and use `adage models install <dir>`")?;
    let status = loop {
        if let Some(s) = child.try_wait()? {
            break s;
        }
        let done = std::fs::metadata(out).map(|m| m.len()).unwrap_or(0);
        report(done.max(1).min(max));
        std::thread::sleep(std::time::Duration::from_millis(200));
    };
    if status.success() {
        report(max);
        return Ok(Ok(()));
    }
    let mut err = String::new();
    if let Some(mut e) = child.stderr.take() {
        use std::io::Read;
        let _ = e.read_to_string(&mut err);
    }
    let err = err.trim().to_string();
    Ok(Err(Failed {
        class: classify(status.code(), &err),
        host: model::host(url),
        detail: if err.is_empty() { format!("{status}") } else { err },
    }))
}

/// Download and install the default model, reporting progress; returns its id.
pub(crate) fn fetch_with(paths: &Paths, progress: &mut dyn FnMut(&Progress)) -> Result<String> {
    fetch_spec(&model::BASE_8M, paths, progress)
}

/// Download and install a model (Hugging Face or the mirror file by file, else its npm tarball),
/// reporting progress; returns its id. `curl`'s own output is captured, so a terminal UI can call
/// this.
pub(crate) fn fetch_spec(spec: &Spec, paths: &Paths, progress: &mut dyn FnMut(&Progress)) -> Result<String> {
    let tmp = paths.models().join(format!(".download{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp)?;
    let result = (|| -> Result<String> {
        // 1. File by file from the pinned revision (or the mirror).
        let total = spec.bytes();
        let mut before = 0u64;
        let mut first: Option<Failed> = None;
        for f in spec.files {
            let url = spec.url(f.name);
            let got = curl(&url, &tmp.join(f.name), f.bytes, &mut |done| {
                progress(&Progress { url: &url, file_done: done, file_bytes: f.bytes, done: before + done, total })
            })?;
            if let Err(e) = got {
                first = Some(e);
                break;
            }
            before += f.bytes;
        }
        let first = match first {
            None => {
                let id = model::install_spec(spec, &tmp, &paths.models())?.id;
                return Ok(id);
            }
            Some(e) => e,
        };
        // 2. The npm package: its pinned tarball holds the same files.
        let npm = &spec.npm;
        let tgz = tmp.join("package.tgz");
        let got = curl(npm.tarball, &tgz, npm.tarball_bytes, &mut |done| {
            progress(&Progress {
                url: npm.tarball,
                file_done: done,
                file_bytes: npm.tarball_bytes,
                done,
                total: npm.tarball_bytes,
            })
        })?;
        match got {
            Ok(()) => {
                let bytes = std::fs::read(&tgz)?;
                let digest: String = <sha2::Sha256 as sha2::Digest>::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect();
                if digest != npm.tarball_sha256 {
                    bail!("{} {}: tarball sha256 {digest} does not match the pinned {}", npm.package, npm.version, npm.tarball_sha256);
                }
                let files = tmp.join("npm");
                adage_intent::npm::extract(spec, &bytes, &files)?;
                Ok(model::install_spec(spec, &files, &paths.models())?.id)
            }
            Err(second) => {
                let record = serde_json::json!({
                    "class": first.class,
                    "host": first.host,
                    "at": crate::learnlog::now_ms(),
                    "detail": first.detail.chars().take(200).collect::<String>(),
                    "fallback": { "class": second.class, "host": second.host, "detail": second.detail.chars().take(200).collect::<String>() },
                });
                let _ = adage_store::config::write_atomic(
                    &paths.models().join(FETCH_FAILED),
                    record.to_string().as_bytes(),
                );
                bail!(
                    "downloading {} failed. {} Fallback: {} Still embedding with {}.",
                    spec.key,
                    explain(first.class, &first.host, &first.detail),
                    explain(second.class, &second.host, &second.detail),
                    model::resolve(&paths.models(), false).describe()
                );
            }
        }
    })();
    if result.is_ok() {
        let _ = std::fs::remove_file(paths.models().join(FETCH_FAILED));
    }
    let _ = std::fs::remove_dir_all(&tmp);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curl_failures_are_classified() {
        assert_eq!(classify(Some(22), "curl: (22) The requested URL returned error: 403"), "blocked");
        assert_eq!(classify(Some(56), "curl: (56) CONNECT tunnel failed, response 407"), "blocked");
        assert_eq!(classify(Some(22), "curl: (22) The requested URL returned error: 404"), "http");
        assert_eq!(classify(Some(6), "Could not resolve host"), "offline");
        assert_eq!(classify(Some(7), ""), "offline");
        assert_eq!(classify(Some(60), "SSL certificate problem"), "tls");
        assert_eq!(classify(Some(35), ""), "tls");
        assert_eq!(classify(None, ""), "error");
        assert!(explain("blocked", "huggingface.co", "403").contains("huggingface.co blocked by proxy (403)"));
    }
}
