//! `adage models fetch | install <dir> | status` — the embedding model for intent families.
//!
//! The model is never bundled: `fetch` downloads the pinned upstream files with `curl` (argv, no
//! shell) into a scratch directory, and `install` verifies every file's SHA-256 against the pinned
//! digest before anything is written under `$ADAGE_HOME/models`. Offline machines download the
//! three files any other way and run `adage models install <dir>`. Without a model, everything
//! else works: `adage condition` skips intent families and hooks never fire intent routes.
//!
//! **Mirrors and blocked networks** (docs/zero-config.md §5). `ADAGE_MODEL_BASE_URL` points the
//! download at a mirror holding the same three files (`<base>/model.safetensors`, …); that is safe
//! because every file is still checked against its pinned SHA-256. A failed download is never
//! retried by itself: it is classified from curl's exit code and message ([`classify`]) and
//! recorded in `models/.fetch-failed.json` (`{class, host, at, detail}`), which `adage doctor` and
//! the status card read to say, once, why prompt grouping is off and how to fix it.

use adage_intent::model;
use adage_store::Paths;
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
            match model::status(&paths.models()) {
                Ok(m) => println!(
                    "intent model: {} ({} x {}) in {}",
                    m.id,
                    m.vocab_size,
                    m.dim,
                    model::dir_in(&paths.models()).display()
                ),
                Err(e) => println!("intent model: {e:#}"),
            }
            Ok(())
        }
        Some(other) => bail!("unknown `adage models {other}`; use fetch, install <dir>, or status"),
    }
}

fn install(dir: &Path, paths: &Paths) -> Result<()> {
    let m = model::install(dir, &paths.models())?;
    let _ = std::fs::remove_file(paths.models().join(FETCH_FAILED));
    println!("installed {} into {}", m.id, model::dir_in(&paths.models()).display());
    Ok(())
}

fn fetch(paths: &Paths) -> Result<()> {
    let m = fetch_with(paths, &mut |p| {
        if p.file_done == 0 {
            println!("fetching {} ({} bytes)", p.url, p.file_bytes);
        }
    })?;
    println!("installed {} into {}", m, model::dir_in(&paths.models()).display());
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
                .args(["-fsSL", "--proto", "=https", "--connect-timeout", "20", "--max-filesize"])
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
                let err = err.trim().to_string();
                let class = classify(status.code(), &err);
                let host = model::host(&url);
                let record = serde_json::json!({ "class": class, "host": host, "at": crate::learnlog::now_ms(), "detail": err.chars().take(200).collect::<String>() });
                let _ = adage_store::config::write_atomic(
                    &paths.models().join(FETCH_FAILED),
                    record.to_string().as_bytes(),
                );
                bail!(
                    "downloading {} failed ({status}){}. {}",
                    f.name,
                    if err.is_empty() { String::new() } else { format!(": {err}") },
                    explain(class, &host, &err)
                );
            }
            report(progress, f.bytes);
            before += f.bytes;
        }
        let id = model::install(&tmp, &paths.models())?.id;
        let _ = std::fs::remove_file(paths.models().join(FETCH_FAILED));
        Ok(id)
    })();
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
