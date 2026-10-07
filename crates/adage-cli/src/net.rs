//! The one place `adage` reaches the network: `curl`, run as an argv (no shell), by `adage models
//! fetch`, `adage init`'s Binaries step and `adage update --check`. Nothing on a hook path calls it,
//! and `ADAGE_OFFLINE=1` stops every fetch before curl starts.
//!
//! Only `https` is followed, redirects included. Two exceptions serve tests and local mirrors: a
//! `file://` URL, and plain `http` to this machine (`127.0.0.1`, `localhost`), each allowed only for
//! the URL it was asked for, never for a redirect. A failure is classified from curl's exit code
//! and message ([`classify`]) so every caller can say what went wrong and how to fix it.
//!
//! The release a binary came from ([`fetch_companion`]): `adage-exec` and `adage-train` sit beside
//! `adage` in every release channel but `cargo install adage-cli` and the shell installer, which
//! install `adage` alone. `adage init` fetches them from the release of its own version, each
//! archive verified against the `.sha256` the release publishes beside it (as
//! `scripts/stage-plugin.mjs` does), then unpacked with `tar`.

use anyhow::{Context, Result};
use sha2::Digest;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

/// Where releases are downloaded from: `ADAGE_RELEASE_URL` (a mirror, or a test's fixture), else
/// GitHub's release downloads for this repository.
pub(crate) fn release_base() -> String {
    std::env::var("ADAGE_RELEASE_URL")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .map(|v| v.trim().trim_end_matches('/').to_string())
        .unwrap_or_else(|| "https://github.com/stevenvaughanbooth/adage/releases/download".into())
}

/// `ADAGE_OFFLINE=1`: no fetch at all.
pub(crate) fn offline() -> bool {
    std::env::var("ADAGE_OFFLINE").as_deref() == Ok("1")
}

/// The release target for this machine, one of the five every release builds (`npm/config.mjs`
/// `PLATFORMS`). Linux releases are static musl binaries, which run on glibc systems too.
pub(crate) fn release_target() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => Some("aarch64-apple-darwin"),
        ("macos", "x86_64") => Some("x86_64-apple-darwin"),
        ("linux", "aarch64") => Some("aarch64-unknown-linux-musl"),
        ("linux", "x86_64") => Some("x86_64-unknown-linux-musl"),
        ("windows", "x86_64") => Some("x86_64-pc-windows-msvc"),
        _ => None,
    }
}

/// The host a URL names, for messages.
pub(crate) fn host(url: &str) -> String {
    let rest = url.split("://").nth(1).unwrap_or(url);
    rest.split(['/', ':']).next().unwrap_or(rest).to_string()
}

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

/// A failed fetch: its class ([`classify`]), the host, and curl's message.
#[derive(Debug, Clone)]
pub(crate) struct Failed {
    pub(crate) class: &'static str,
    pub(crate) host: String,
    pub(crate) detail: String,
    /// curl's exit code, when it ran.
    pub(crate) code: Option<i32>,
}

impl Failed {
    /// The server answered that there is no such file (an HTTP 404, or a `file://` path that does
    /// not exist): for a release, not published.
    pub(crate) fn not_found(&self) -> bool {
        (self.code == Some(22) && self.detail.contains(" 404")) || self.code == Some(37)
    }
}

/// The protocols curl may use for `url`: `https`; a `file://` URL; plain `http` to this machine.
fn protocols(url: &str) -> &'static str {
    let local_http = ["http://127.0.0.1", "http://localhost", "http://[::1]"]
        .iter()
        .any(|p| url.starts_with(p) && url[p.len()..].starts_with([':', '/']));
    if url.starts_with("file://") {
        "=file"
    } else if local_http {
        "=http"
    } else {
        "=https"
    }
}

/// Run curl on `url` into `out` (a file, or stdout captured when `None`), at most `max` bytes and
/// `timeout` long, calling `report` with the bytes written so far about five times a second.
fn curl(
    url: &str,
    out: Option<&Path>,
    max: u64,
    timeout: Option<Duration>,
    report: &mut dyn FnMut(u64),
) -> Result<Result<Vec<u8>, Failed>> {
    if offline() {
        return Ok(Err(Failed { class: "offline", host: host(url), detail: "ADAGE_OFFLINE=1".into(), code: None }));
    }
    report(0);
    let mut c = std::process::Command::new("curl");
    c.args(["-fsSL", "--proto", protocols(url), "--proto-redir", "=https", "--connect-timeout", "20"])
        .arg("--max-filesize")
        .arg((max + 4096).to_string())
        .arg("--user-agent")
        .arg(concat!("adage/", env!("CARGO_PKG_VERSION")));
    if let Some(t) = timeout {
        c.arg("--max-time").arg(t.as_secs().max(1).to_string());
    }
    match out {
        Some(p) => c.arg("-o").arg(p).stdout(Stdio::null()),
        None => c.stdout(Stdio::piped()),
    };
    let mut child = c
        .arg(url)
        .stdin(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .context("could not run curl; it is needed to download")?;
    // Both pipes drain on their own threads, so a chatty curl never blocks on a full pipe.
    let drain = |r: Option<Box<dyn std::io::Read + Send>>| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(mut r) = r {
                let _ = r.read_to_end(&mut buf);
            }
            buf
        })
    };
    let stdout = drain(child.stdout.take().map(|s| Box::new(s) as Box<dyn std::io::Read + Send>));
    let stderr = drain(child.stderr.take().map(|s| Box::new(s) as Box<dyn std::io::Read + Send>));
    let status = loop {
        if let Some(s) = child.try_wait()? {
            break s;
        }
        let done = out.and_then(|p| std::fs::metadata(p).ok()).map(|m| m.len()).unwrap_or(0);
        report(done.max(1).min(max));
        std::thread::sleep(Duration::from_millis(200));
    };
    let body = stdout.join().unwrap_or_default();
    let err = String::from_utf8_lossy(&stderr.join().unwrap_or_default()).trim().to_string();
    if status.success() {
        report(max);
        return Ok(Ok(body));
    }
    Ok(Err(Failed {
        class: classify(status.code(), &err),
        host: host(url),
        detail: if err.is_empty() { format!("{status}") } else { err },
        code: status.code(),
    }))
}

/// Download `url` to `out` (at most `max` bytes), reporting progress.
pub(crate) fn download(url: &str, out: &Path, max: u64, report: &mut dyn FnMut(u64)) -> Result<Result<(), Failed>> {
    Ok(curl(url, Some(out), max, None, report)?.map(|_| ()))
}

/// The body at `url` (at most `max` bytes), giving up after `timeout` (`adage update --check`).
pub(crate) fn get(url: &str, timeout: Duration, max: u64) -> Result<Result<Vec<u8>, Failed>> {
    curl(url, None, max, Some(timeout), &mut |_| {})
}

/// The SHA-256 of a file, as lowercase hex.
pub(crate) fn sha256_file(p: &Path) -> std::io::Result<String> {
    use std::io::Read;
    let mut f = std::fs::File::open(p)?;
    let mut h = sha2::Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex(&h.finalize()))
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// What fetching a companion binary came to.
#[derive(Debug)]
pub(crate) enum Companion {
    /// Verified and placed at this path.
    Placed(PathBuf),
    /// This machine is not one a release builds for.
    NoTarget,
    /// The release has no such archive (not published, or not for this target): the URL asked.
    NotPublished(String),
    /// The network failed (offline, blocked, TLS, a timeout).
    Failed(Failed),
    /// The archive's SHA-256 is not the one the release publishes: nothing was placed.
    Mismatch { archive: String, want: String, got: String },
}

/// The largest release archive accepted (a release binary is tens of megabytes).
const MAX_ARCHIVE: u64 = 512 << 20;

/// Fetch `name` (`adage-exec`, `adage-train`) from the release `v<version>` for this machine into
/// `into`, verified against the release's `.sha256` before it is unpacked; `scratch` is an empty
/// directory to download and unpack in (removed by the caller). The binary is written beside a
/// temporary name and renamed, so `into/<name>` is either the old file or the whole new one.
pub(crate) fn fetch_companion(name: &str, version: &str, into: &Path, scratch: &Path) -> Result<Companion> {
    let Some(target) = release_target() else { return Ok(Companion::NoTarget) };
    let ext = if cfg!(windows) { ".zip" } else { ".tar.xz" };
    let archive = format!("{name}-{target}{ext}");
    let url = format!("{}/v{version}/{archive}", release_base());
    std::fs::create_dir_all(scratch).with_context(|| format!("cannot create {}", scratch.display()))?;
    let file = scratch.join(&archive);
    let sums = scratch.join(format!("{archive}.sha256"));
    // The checksum first: a release without it is not one to unpack.
    for (u, out, max) in [(format!("{url}.sha256"), &sums, 4096), (url.clone(), &file, MAX_ARCHIVE)] {
        match download(&u, out, max, &mut |_| {})? {
            Ok(()) => {}
            Err(f) if f.not_found() => return Ok(Companion::NotPublished(u)),
            Err(f) => return Ok(Companion::Failed(f)),
        }
    }
    let want = std::fs::read_to_string(&sums)?.split_whitespace().next().unwrap_or("").to_lowercase();
    let got = sha256_file(&file)?;
    if want.len() != 64 || want != got {
        return Ok(Companion::Mismatch { archive, want, got });
    }
    let unpacked = scratch.join("unpacked");
    std::fs::create_dir_all(&unpacked)?;
    // bsdtar (macOS, Windows) and GNU tar (with xz) both unpack the release's archives.
    let status = std::process::Command::new("tar")
        .arg("-xf")
        .arg(&file)
        .arg("-C")
        .arg(&unpacked)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .context("could not run tar to unpack the release archive")?;
    if !status.status.success() {
        anyhow::bail!("tar could not unpack {archive}: {}", String::from_utf8_lossy(&status.stderr).trim());
    }
    let exe = format!("{name}{}", std::env::consts::EXE_SUFFIX);
    let found = find(&unpacked, &exe).with_context(|| format!("{archive} holds no {exe}"))?;
    std::fs::create_dir_all(into).with_context(|| format!("cannot create {}", into.display()))?;
    let dest = into.join(&exe);
    let tmp = into.join(format!(".{exe}.adage-{}", std::process::id()));
    std::fs::copy(&found, &tmp).with_context(|| format!("cannot write in {}", into.display()))?;
    make_executable(&tmp)?;
    std::fs::rename(&tmp, &dest).with_context(|| format!("cannot replace {}", dest.display()))?;
    Ok(Companion::Placed(dest))
}

/// The first file named `name` under `dir`.
fn find(dir: &Path, name: &str) -> Option<PathBuf> {
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_dir() {
                stack.push(p);
            } else if ft.is_file() && e.file_name() == name {
                return Some(p);
            }
        }
    }
    None
}

#[cfg(unix)]
pub(crate) fn make_executable(p: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755))
}

#[cfg(not(unix))]
pub(crate) fn make_executable(_: &Path) -> std::io::Result<()> {
    Ok(())
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
        let f = |code, detail: &str| Failed { class: "http", host: "h".into(), detail: detail.into(), code };
        assert!(f(Some(22), "curl: (22) The requested URL returned error: 404").not_found());
        assert!(f(Some(37), "curl: (37) Couldn't open file").not_found());
        assert!(!f(Some(22), "curl: (22) The requested URL returned error: 500").not_found());
    }

    #[test]
    fn only_https_reaches_other_hosts() {
        assert_eq!(protocols("https://github.com/x"), "=https");
        assert_eq!(protocols("file:///tmp/release"), "=file");
        assert_eq!(protocols("http://127.0.0.1:8123/v0.1.0/x"), "=http");
        assert_eq!(protocols("http://localhost/x"), "=http");
        assert_eq!(protocols("http://localhost.evil.example/x"), "=https", "only this machine");
        assert_eq!(protocols("http://example.com/x"), "=https", "curl then refuses it");
        assert_eq!(host("https://github.com/stevenvaughanbooth/adage"), "github.com");
    }

    #[test]
    fn every_release_target_is_one_npm_ships() {
        let config = include_str!("../../../npm/config.mjs");
        let t = release_target();
        if let Some(t) = t {
            assert!(config.contains(&format!("target: \"{t}\"")), "{t} is not in npm/config.mjs");
        }
    }
}
