//! The release check (onboarding plan O5): is a newer Adage out?
//!
//! **One request, nothing sent but the request.** The npm registry's answer for the package
//! (`registry.npmjs.org/@adage-sh/cli/latest`, field `version`) first; only when it answers 404
//! (the package is not published) GitHub's latest release
//! (`api.github.com/repos/stevenvaughanbooth/adage/releases/latest`, `tag_name` without its `v`).
//! A failure that is not a 404 (offline, blocked by a proxy, TLS, a timeout) is the answer: GitHub
//! is not tried. Each request gives up after [`TIMEOUT`], carries `User-Agent: adage/<version>`
//! (curl's own arguments, through [`crate::net`]) and nothing else: no identifier, no token, no
//! repository or path. `ADAGE_OFFLINE=1` fetches nothing.
//!
//! **Where it runs.** `adage update --check [--now]` and `adage update`, by a person; and the
//! detached learner after it conditions a repository ([`check_if_due`]), at most once a day per
//! data directory, only with `update.check = daily` (the default). Never on a hook: the hook and
//! the card read the cache ([`adage_store::release::Latest`], `$ADAGE_HOME/cache/latest.json`),
//! which only this module writes. A failed check is cached like an answer and not retried before
//! the next day (the `models fetch` rule, docs/zero-config.md).
//!
//! **Mirrors and tests.** With `ADAGE_RELEASE_URL=<base>` (the release downloads' base,
//! [`crate::net::release_base`]) the two answers are read from `<base>/npm/@adage-sh/cli/latest`
//! and `<base>/github/repos/stevenvaughanbooth/adage/releases/latest` instead.

use super::channel::NPM_PACKAGE;
use crate::net;
use adage_store::release::{Failure, Latest, Release, Version, VERSION};
use adage_store::{config, lock, Paths};
use serde_json::Value;
use std::time::Duration;

/// How long each request may take.
pub(crate) const TIMEOUT: Duration = Duration::from_secs(3);
/// The largest answer read (npm's is about 2 KB, GitHub's release a few KB with its notes).
const MAX_ANSWER: u64 = 256 << 10;
const NPM: &str = "https://registry.npmjs.org";
const GITHUB_API: &str = "https://api.github.com";
const GITHUB_REPO: &str = "stevenvaughanbooth/adage";
/// Lines of the release notes kept for the surfaces.
const NOTES_LINES: usize = 3;
const NOTES_WIDTH: usize = 120;

/// The two places a release is read from, in order.
pub(crate) fn endpoints() -> [(String, &'static str); 2] {
    let mirror = std::env::var("ADAGE_RELEASE_URL").ok().filter(|v| !v.trim().is_empty());
    match mirror {
        Some(_) => {
            let base = net::release_base();
            [
                (format!("{base}/npm/{NPM_PACKAGE}/latest"), "npm"),
                (format!("{base}/github/repos/{GITHUB_REPO}/releases/latest"), "github"),
            ]
        }
        None => [
            (format!("{NPM}/{NPM_PACKAGE}/latest"), "npm"),
            (format!("{GITHUB_API}/repos/{GITHUB_REPO}/releases/latest"), "github"),
        ],
    }
}

/// Ask now and cache the answer (or the failure, keeping the last known release). Returns what
/// was cached. Never fails: a cache that cannot be written leaves the answer in memory only.
pub(crate) fn refresh(paths: &Paths, now: u64) -> Latest {
    let before = Latest::read(paths);
    let mut l =
        Latest { checked_at: now, have: VERSION.to_string(), latest: before.and_then(|b| b.latest), failure: None };
    match fetch() {
        Ok(r) => l.latest = Some(r),
        Err((class, host)) => l.failure = Some(Failure { class, host, at: now }),
    }
    let _ = l.write(paths);
    l
}

/// The newest release, or why it could not be read: `(class, host)`.
fn fetch() -> Result<Release, (String, String)> {
    let [(npm, _), (github, _)] = endpoints();
    let answer = |url: &str| -> Result<Vec<u8>, net::Failed> {
        match net::get(url, TIMEOUT, MAX_ANSWER) {
            Ok(r) => r,
            Err(e) => Err(net::Failed { class: "error", host: net::host(url), detail: format!("{e:#}"), code: None }),
        }
    };
    let failed = |f: net::Failed| (f.class.to_string(), f.host);
    match answer(&npm) {
        Ok(body) => from_npm(&body).ok_or_else(|| ("error".into(), net::host(&npm))),
        // Not published on npm: the GitHub release is the answer.
        Err(f) if f.not_found() => match answer(&github) {
            Ok(body) => from_github(&body).ok_or_else(|| ("error".into(), net::host(&github))),
            // No release yet, or a repository this machine cannot see.
            Err(f) if f.not_found() => Err(("unavailable".into(), f.host)),
            Err(f) => Err(failed(f)),
        },
        Err(f) => Err(failed(f)),
    }
}

/// The npm registry's `<package>/latest` document.
fn from_npm(body: &[u8]) -> Option<Release> {
    let v: Value = serde_json::from_slice(body).ok()?;
    let version = clean_version(v.get("version")?.as_str()?)?;
    Some(Release {
        url: Some(format!("https://github.com/{GITHUB_REPO}/releases/tag/v{version}")),
        version,
        source: "npm".into(),
        ..Release::default()
    })
}

/// GitHub's `releases/latest` document (drafts and pre-releases are never "latest" there).
fn from_github(body: &[u8]) -> Option<Release> {
    let v: Value = serde_json::from_slice(body).ok()?;
    let version = clean_version(v.get("tag_name")?.as_str()?)?;
    let text = |k: &str, max: usize| v.get(k).and_then(Value::as_str).map(|s| plain(s, max)).filter(|s| !s.is_empty());
    Some(Release {
        version,
        published_at: text("published_at", 40).filter(|t| days_since_epoch(t).is_some()),
        notes: v.get("body").and_then(Value::as_str).and_then(notes),
        url: text("html_url", 256).filter(|u| u.starts_with("https://")),
        source: "github".into(),
    })
}

/// A version as a release names it, checked (a tag's `v` dropped): never anything a surface would
/// print that is not a version.
fn clean_version(s: &str) -> Option<String> {
    let s = s.trim();
    let s = s.strip_prefix('v').unwrap_or(s);
    Version::parse(s).map(|_| s.to_string())
}

/// Text from the network, one line, without control characters, at most `max` characters.
fn plain(s: &str, max: usize) -> String {
    s.chars().filter(|c| !c.is_control()).take(max).collect::<String>().trim().to_string()
}

/// The release notes' first bullets (the release body's first lines when it has none), as plain
/// lines.
fn notes(body: &str) -> Option<String> {
    let strip = |l: &str| plain(&l.replace("**", "").replace('`', ""), NOTES_WIDTH);
    let bullets: Vec<String> = body
        .lines()
        .map(str::trim)
        .filter_map(|l| l.strip_prefix("- ").or_else(|| l.strip_prefix("* ")))
        .map(strip)
        .filter(|l| !l.is_empty())
        .take(NOTES_LINES)
        .collect();
    let lines = if bullets.is_empty() {
        body.lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .map(strip)
            .take(NOTES_LINES)
            .collect()
    } else {
        bullets
    };
    (!lines.is_empty()).then(|| lines.join("\n"))
}

/// Days since the Unix epoch of an RFC 3339 date (`2026-10-03T12:00:00Z`); only its date is read.
pub(crate) fn days_since_epoch(t: &str) -> Option<i64> {
    let date = t.get(..10)?;
    let mut it = date.split('-');
    let (y, m, d): (i64, i64, i64) = (it.next()?.parse().ok()?, it.next()?.parse().ok()?, it.next()?.parse().ok()?);
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) || it.next().is_some() {
        return None;
    }
    // Howard Hinnant's days_from_civil.
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

/// The cache, refreshed when it is older than a day (or `now`, with `--now`): what `adage update`
/// and `adage update --check` read.
pub(crate) fn current(paths: &Paths, now: u64, force: bool) -> Latest {
    match Latest::read(paths) {
        Some(l) if !force && !l.due(now) => l,
        _ => refresh(paths, now),
    }
}

/// The learner's daily check (`autolearn::run_auto`, after a learn): only with `update.check =
/// daily` and not `ADAGE_OFFLINE=1`, at most once a day per data directory (one learner checks
/// while the others skip), and never a failure: anything wrong is cached or forgotten.
pub(crate) fn check_if_due(paths: &Paths) {
    if net::offline() || !config::load(paths).config.update_check {
        return;
    }
    let now = adage_core::time::now_ms();
    if Latest::read(paths).is_some_and(|l| !l.due(now)) {
        return;
    }
    let _ = std::fs::create_dir_all(paths.cache());
    let Ok(Some(_held)) = lock::try_exclusive(&paths.cache().join("latest.lock")) else { return };
    // Another learner may have checked between the read and the lock.
    if Latest::read(paths).is_some_and(|l| !l.due(now)) {
        return;
    }
    refresh(paths, now);
}

/// One line for a check's answer: `adage 0.1.0 · 0.2.0 is out (released 3 days ago)`, `adage
/// 0.1.0 is the latest`, or `cannot check now: offline (registry.npmjs.org)`.
pub(crate) fn sentence(l: &Latest, now: u64) -> String {
    if let Some(r) = l.newer_than(VERSION) {
        let when = r
            .published_at
            .as_deref()
            .and_then(days_since_epoch)
            .map(|day| (now / 86_400_000) as i64 - day)
            .map(|ago| match ago {
                ..=0 => " (released today)".to_string(),
                1 => " (released yesterday)".to_string(),
                n => format!(" (released {n} days ago)"),
            })
            .unwrap_or_default();
        return format!("adage {VERSION} · {} is out{when}", r.version);
    }
    match &l.failure {
        Some(f) => format!("cannot check now: {} ({})", f.class, f.host),
        None => format!("adage {VERSION} is the latest"),
    }
}

/// What to do about a failed check, in one sentence.
pub(crate) fn explain(f: &Failure) -> String {
    let host = &f.host;
    match f.class.as_str() {
        "unavailable" => format!("{host} has no published release this machine can see yet"),
        "offline" => format!("{host} is not reachable (no network, or ADAGE_OFFLINE=1)"),
        "blocked" => format!("{host} is blocked by a proxy or policy"),
        "tls" => format!("TLS to {host} was intercepted: set CURL_CA_BUNDLE or SSL_CERT_FILE to your proxy's CA"),
        "timeout" => format!("{host} did not answer within {} seconds", TIMEOUT.as_secs()),
        _ => format!("{host} gave an answer that is not a release"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answers_are_read_and_checked() {
        let npm = from_npm(br#"{"name":"@adage-sh/cli","version":"0.2.0","dist":{}}"#).unwrap();
        assert_eq!((npm.version.as_str(), npm.source.as_str()), ("0.2.0", "npm"));
        assert_eq!(npm.url.as_deref(), Some("https://github.com/stevenvaughanbooth/adage/releases/tag/v0.2.0"));
        let gh = from_github(
            br###"{"tag_name":"v0.2.0","published_at":"2026-10-03T12:00:00Z","html_url":"https://github.com/stevenvaughanbooth/adage/releases/tag/v0.2.0",
                "body":"## What changed\n\n- **Hooks** run faster on large repos\n- `adage update` keeps the previous binaries\n- three\n- four\n"}"###,
        )
        .unwrap();
        assert_eq!(gh.version, "0.2.0");
        assert_eq!(
            gh.notes.as_deref(),
            Some("Hooks run faster on large repos\nadage update keeps the previous binaries\nthree")
        );
        assert_eq!(gh.published_at.as_deref(), Some("2026-10-03T12:00:00Z"));
        for bad in [&br#"{"version":"latest"}"#[..], br#"{"version":"0.2.0\u001b[31m"}"#, b"<html>", br#"{"v":1}"#] {
            assert!(from_npm(bad).is_none(), "{}", String::from_utf8_lossy(bad));
        }
        let hostile =
            from_github(br#"{"tag_name":"v0.2.0","html_url":"javascript:alert(1)","body":"- a\u0007b"}"#).unwrap();
        assert_eq!((hostile.url, hostile.notes.as_deref()), (None, Some("ab")));
    }

    #[test]
    fn the_check_says_one_line() {
        let now = (days_since_epoch("2026-10-06T00:00:00Z").unwrap() as u64) * 86_400_000 + 3_600_000;
        let rel = |v: &str, at: Option<&str>| Release {
            version: v.into(),
            published_at: at.map(str::to_string),
            ..Release::default()
        };
        let l = |latest: Option<Release>, failure: Option<Failure>| Latest {
            checked_at: now,
            have: VERSION.into(),
            latest,
            failure,
        };
        let newer = adage_store::release::Version::parse(VERSION)
            .map(|v| format!("{}.{}.{}", v.major, v.minor + 1, 0))
            .unwrap();
        assert_eq!(
            sentence(&l(Some(rel(&newer, Some("2026-10-03T12:00:00Z"))), None), now),
            format!("adage {VERSION} · {newer} is out (released 3 days ago)")
        );
        assert_eq!(sentence(&l(Some(rel(&newer, None)), None), now), format!("adage {VERSION} · {newer} is out"));
        assert_eq!(sentence(&l(Some(rel(VERSION, None)), None), now), format!("adage {VERSION} is the latest"));
        let off = Failure { class: "offline".into(), host: "registry.npmjs.org".into(), at: now };
        assert_eq!(sentence(&l(None, Some(off.clone())), now), "cannot check now: offline (registry.npmjs.org)");
        // A failure today does not hide a newer release known from before.
        assert!(sentence(&l(Some(rel(&newer, None)), Some(off)), now).contains("is out"));
    }

    #[test]
    fn dates_count_days() {
        assert_eq!(days_since_epoch("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(days_since_epoch("2000-03-01"), Some(11_017));
        assert_eq!(days_since_epoch("2026-13-01T00:00:00Z"), None);
        assert_eq!(days_since_epoch("soon"), None);
    }
}
