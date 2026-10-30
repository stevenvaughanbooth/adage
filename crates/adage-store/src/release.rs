//! What this machine knows about Adage's own releases: `$ADAGE_HOME/cache/latest.json`
//! ([`Paths::update_cache`]), the answer of the last release check.
//!
//! The check itself (one request to the npm registry, then GitHub's releases when the package is
//! not published) lives in `adage-cli` (`update::latest`), and runs only from `adage update
//! --check` and from the detached learner at most once a day. This crate holds the file's shape so
//! every reader agrees on it, the hook included: the hook reads this file and never reaches the
//! network (docs/zero-config.md). A missing or malformed file reads as "nothing known".
//!
//! ```json
//! {"checked_at": 1759800000000, "have": "0.1.0",
//!  "latest": {"version": "0.2.0", "published_at": "2026-10-03T12:00:00Z", "notes": "…",
//!             "url": "https://github.com/stevenvaughanbooth/adage/releases/tag/v0.2.0", "source": "github"},
//!  "failure": null}
//! ```
//!
//! `checked_at` is when the last check ran (milliseconds since the Unix epoch), whether it
//! answered or failed; `have` the version of the binary that checked. A failed check keeps the
//! last known `latest` and records `failure: {class, host, at}` (`offline`, `blocked`, `tls`,
//! `timeout`, `http`, `error`); either way the next check waits [`CHECK_EVERY_MS`].

use crate::Paths;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::io::Read;
use std::path::Path;

/// This build's version: every crate of the workspace carries the same one.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// At most one release check per data directory in this many milliseconds (a day), answered or
/// not.
pub const CHECK_EVERY_MS: u64 = 86_400_000;
const MAX_CACHE_BYTES: usize = 64 << 10;

/// The answer of the last release check.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Latest {
    pub checked_at: u64,
    /// The version of the binary that checked.
    pub have: String,
    /// The newest release known, kept across a failed check.
    #[serde(default)]
    pub latest: Option<Release>,
    /// Why the last check failed; `None` when it answered.
    #[serde(default)]
    pub failure: Option<Failure>,
}

/// One published release.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Release {
    pub version: String,
    #[serde(default)]
    pub published_at: Option<String>,
    /// The release notes' first lines, as plain text.
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    /// Which source answered: `npm` or `github`.
    #[serde(default)]
    pub source: String,
}

/// A check that did not answer.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Failure {
    pub class: String,
    pub host: String,
    pub at: u64,
}

impl Latest {
    /// The cache, if there is one that parses.
    pub fn read(paths: &Paths) -> Option<Latest> {
        read_cache(std::fs::File::open(paths.update_cache()).ok()?)
    }

    /// Replace the cache atomically.
    pub fn write(&self, paths: &Paths) -> std::io::Result<()> {
        let body = serde_json::to_vec(self).map_err(std::io::Error::other)?;
        crate::config::write_atomic(&paths.update_cache(), &body)
    }

    /// Is a check due at `now`: a day since the last one, or a clock that moved back past it.
    pub fn due(&self, now: u64) -> bool {
        self.checked_at > now || now - self.checked_at >= CHECK_EVERY_MS
    }

    /// The known release, when it is newer than `have`; never one whose version does not parse.
    pub fn newer_than(&self, have: &str) -> Option<&Release> {
        self.latest.as_ref().filter(|r| is_newer(&r.version, have))
    }
}

/// A release cache is usually a few hundred bytes and is read on the hook path. Bound the I/O
/// before allocating; one extra byte distinguishes a full valid cache from an oversized file.
fn read_cache(reader: impl Read) -> Option<Latest> {
    let mut bytes = Vec::new();
    reader.take((MAX_CACHE_BYTES + 1) as u64).read_to_end(&mut bytes).ok()?;
    if bytes.len() > MAX_CACHE_BYTES {
        return None;
    }
    serde_json::from_slice(&bytes).ok()
}

/// A release version: `major.minor.patch`, an optional pre-release (`-rc.1`), build metadata
/// (`+…`) ignored, a leading `v` (a tag) allowed. Ordered as semantic versions are: a pre-release
/// comes before its release, and its identifiers compare numerically when both are numbers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    pre: Vec<String>,
}

impl Version {
    pub fn parse(s: &str) -> Option<Version> {
        let s = s.trim();
        let s = s.strip_prefix('v').unwrap_or(s);
        if s.is_empty() || s.len() > 64 {
            return None;
        }
        let s = s.split_once('+').map_or(s, |(v, _)| v);
        let (core, pre) = match s.split_once('-') {
            Some((c, p)) => (c, Some(p)),
            None => (s, None),
        };
        let nums: Vec<u64> = core
            .split('.')
            .map(|n| (!n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())).then(|| n.parse().ok()).flatten())
            .collect::<Option<_>>()?;
        let [major, minor, patch] = nums[..] else { return None };
        let pre: Vec<String> = match pre {
            Some(p) => {
                let ids: Vec<String> = p.split('.').map(str::to_string).collect();
                let ok = |i: &String| !i.is_empty() && i.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-');
                if !ids.iter().all(ok) {
                    return None;
                }
                ids
            }
            None => Vec::new(),
        };
        Some(Version { major, minor, patch, pre })
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.major, self.minor, self.patch).cmp(&(other.major, other.minor, other.patch)).then_with(|| {
            match (self.pre.is_empty(), other.pre.is_empty()) {
                (true, true) => Ordering::Equal,
                (true, false) => Ordering::Greater,
                (false, true) => Ordering::Less,
                (false, false) => {
                    for (a, b) in self.pre.iter().zip(&other.pre) {
                        let o = match (a.parse::<u64>(), b.parse::<u64>()) {
                            (Ok(x), Ok(y)) => x.cmp(&y),
                            (Ok(_), Err(_)) => Ordering::Less,
                            (Err(_), Ok(_)) => Ordering::Greater,
                            (Err(_), Err(_)) => a.cmp(b),
                        };
                        if o != Ordering::Equal {
                            return o;
                        }
                    }
                    self.pre.len().cmp(&other.pre.len())
                }
            }
        })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Is `candidate` a newer version than `have`? `false` when either does not parse.
pub fn is_newer(candidate: &str, have: &str) -> bool {
    matches!((Version::parse(candidate), Version::parse(have)), (Some(c), Some(h)) if c > h)
}

/// Does `p` lie inside Claude Code's plugin directories (`…/plugins/cache/…` where installed
/// plugins live, `…/plugins/data/…` where a plugin stages its binaries), whatever
/// `CLAUDE_CONFIG_DIR` is? Such a binary is the plugin's: Claude Code updates it.
pub fn plugin_binary(p: &Path) -> bool {
    let parts: Vec<String> = p.components().map(|c| c.as_os_str().to_string_lossy().to_lowercase()).collect();
    parts.windows(2).any(|w| w[0] == "plugins" && (w[1] == "cache" || w[1] == "data"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_order_as_releases_do() {
        let v = |s: &str| Version::parse(s).unwrap_or_else(|| panic!("{s} parses"));
        assert!(v("0.2.0") > v("0.1.0"));
        assert!(v("0.10.0") > v("0.9.9"), "numeric, not lexical");
        assert!(v("1.0.0") > v("1.0.0-rc.2"), "a release comes after its pre-releases");
        assert!(v("1.0.0-rc.10") > v("1.0.0-rc.2"));
        assert!(v("1.0.0-rc.1") > v("1.0.0-beta.9"));
        assert!(v("1.0.0-alpha.1") > v("1.0.0-alpha"));
        assert_eq!(v("v0.2.0"), v("0.2.0"), "a tag's v is dropped");
        assert_eq!(v("0.2.0+build.5"), v("0.2.0"), "build metadata is ignored");
        for bad in ["", "0.2", "0.2.0.1", "x.y.z", "0.2.0-", "0.2.0-rc..1", "-1.0.0", "1.0.0-rc$", " "] {
            assert!(Version::parse(bad).is_none(), "{bad:?} is not a version");
        }
        assert!(is_newer("0.2.0", "0.1.0"));
        assert!(!is_newer("0.1.0", "0.1.0"));
        assert!(!is_newer("0.0.9", "0.1.0"));
        assert!(!is_newer("banana", "0.1.0"), "a malformed answer is never newer");
    }

    #[test]
    fn the_cache_reads_back_and_a_failure_keeps_what_was_known() {
        let d = tempfile::tempdir().unwrap();
        let paths = Paths::at(d.path());
        assert_eq!(Latest::read(&paths), None, "no cache: nothing known");
        let release = Release { version: "0.2.0".into(), source: "npm".into(), ..Release::default() };
        let l = Latest { checked_at: 5, have: "0.1.0".into(), latest: Some(release.clone()), failure: None };
        l.write(&paths).unwrap();
        assert_eq!(Latest::read(&paths), Some(l.clone()));
        assert_eq!(l.newer_than("0.1.0"), Some(&release));
        assert_eq!(l.newer_than("0.2.0"), None);
        // Fields a newer binary adds are ignored; a missing one has its default.
        std::fs::write(paths.update_cache(), r#"{"checked_at":1,"have":"0.1.0","later":true}"#).unwrap();
        assert_eq!(Latest::read(&paths).map(|l| l.latest), Some(None));
        std::fs::write(paths.update_cache(), "not json").unwrap();
        assert_eq!(Latest::read(&paths), None, "a malformed cache is nothing known");
    }

    #[test]
    fn cache_reads_are_bounded_before_parsing_or_allocating_the_whole_file() {
        let mut oversized = std::io::Cursor::new(vec![b' '; MAX_CACHE_BYTES * 4]);
        assert_eq!(read_cache(&mut oversized), None);
        assert_eq!(oversized.position(), (MAX_CACHE_BYTES + 1) as u64, "do not read the rest of a large cache");

        let mut bytes = br#"{"checked_at":1,"have":"0.1.0"}"#.to_vec();
        bytes.resize(MAX_CACHE_BYTES, b' ');
        assert_eq!(read_cache(bytes.as_slice()).unwrap().checked_at, 1, "the size limit itself is valid");
        bytes.push(b' ');
        assert_eq!(read_cache(bytes.as_slice()), None, "valid JSON over the limit is still refused");
    }

    #[test]
    fn a_check_is_due_daily() {
        let l = Latest { checked_at: 1_000, ..Latest::default() };
        assert!(!l.due(1_000 + CHECK_EVERY_MS - 1));
        assert!(l.due(1_000 + CHECK_EVERY_MS));
        assert!(l.due(999), "a clock that moved back does not silence the check for good");
    }

    #[test]
    fn plugin_binaries_are_known_by_where_they_live() {
        let p = |s: &str| plugin_binary(Path::new(s));
        assert!(p("/x/cfg/plugins/cache/adage/adage/0.1.0/bin/adage"));
        assert!(p("/home/a/.claude/plugins/data/adage-adage/bin/adage-0123abcd"));
        assert!(p("/Users/a/Library/Claude/Plugins/Data/adage/bin/adage"), "case does not matter");
        assert!(!p("/home/a/.cargo/bin/adage"));
        assert!(!p("/home/a/plugins/adage/bin/adage"));
    }
}
