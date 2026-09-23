//! The export check `adage share` runs before anything leaves the machine (Design §10.8, §15.2):
//! absolute paths, home-directory paths, and secret-looking strings, in every file of the package.

use regex::Regex;
use std::sync::OnceLock;

#[derive(Debug, Clone, PartialEq)]
pub struct Finding {
    pub file: String,
    pub line: usize,
    pub kind: &'static str,
    pub excerpt: String,
}

struct Rules {
    home: Regex,
    absolute: Regex,
    tokens: Vec<(&'static str, Regex)>,
    candidate: Regex,
    hex: Regex,
}

fn rules() -> &'static Rules {
    static R: OnceLock<Rules> = OnceLock::new();
    R.get_or_init(|| {
        let tokens = [
            ("GitHub token", r"\b(?:gh[pousr]_[A-Za-z0-9]{30,}|github_pat_[A-Za-z0-9_]{20,})"),
            ("GitLab token", r"\bglpat-[A-Za-z0-9_-]{20,}"),
            ("API key (sk-)", r"\bsk-(?:ant-|proj-)?[A-Za-z0-9_-]{20,}"),
            ("Stripe key", r"\b[rsp]k_(?:live|test)_[A-Za-z0-9]{16,}"),
            ("Slack token", r"\bxox[abposr]-[A-Za-z0-9-]{10,}"),
            ("AWS access key", r"\b(?:AKIA|ASIA)[0-9A-Z]{16}\b"),
            ("Google API key", r"\bAIza[0-9A-Za-z_-]{35}"),
            ("npm token", r"\bnpm_[A-Za-z0-9]{36}"),
            ("PyPI token", r"\bpypi-[A-Za-z0-9_-]{50,}"),
            ("private key", r"-----BEGIN [A-Z ]*PRIVATE KEY"),
            ("JSON web token", r"\beyJ[A-Za-z0-9_-]{10,}\.eyJ[A-Za-z0-9_-]{10,}"),
            ("credential assignment", r"(?i)\b(?:password|passwd|secret|api[_-]?key|access[_-]?token|auth[_-]?token)\s*[:=]\s*\S{8,}"),
        ];
        Rules {
            home: Regex::new(r#"(?:^|[^A-Za-z0-9_.])~/|\$HOME\b|\$\{HOME\}|/(?:Users|home)/[^/\s"']+"#).unwrap(),
            absolute: Regex::new(r#"(?:^|[\s"'`=:,(\[{])(/[A-Za-z0-9._@+~-]+/[^\s"'`]*|[A-Za-z]:\\[^\s"'`]*|file://\S*)"#).unwrap(),
            tokens: tokens.into_iter().map(|(k, r)| (k, Regex::new(r).unwrap())).collect(),
            candidate: Regex::new(r"[A-Za-z0-9+_=-]{24,}").unwrap(),
            hex: Regex::new(r"(?:^|[^A-Za-z0-9])([0-9a-fA-F]{32,})\b").unwrap(),
        }
    })
}

/// Shannon entropy in bits per character.
fn entropy(s: &str) -> f64 {
    let mut counts = [0u32; 256];
    for b in s.bytes() {
        counts[b as usize] += 1;
    }
    let n = s.len() as f64;
    counts.iter().filter(|&&c| c > 0).map(|&c| -(c as f64 / n) * (c as f64 / n).log2()).sum()
}

/// Random-looking: long, mixed upper, lower and digits, and near the entropy of random base64.
fn high_entropy(t: &str) -> bool {
    t.bytes().any(|c| c.is_ascii_uppercase())
        && t.bytes().any(|c| c.is_ascii_lowercase())
        && t.bytes().any(|c| c.is_ascii_digit())
        && entropy(t) >= 4.0
}

/// Show enough to find a secret, never the secret itself.
fn mask(s: &str) -> String {
    let head: String = s.chars().take(6).collect();
    format!("{head}… ({} chars)", s.chars().count())
}

/// Scan one file. `home` is the sharer's home directory, flagged wherever it appears.
pub fn scan(file: &str, text: &str, home: Option<&str>) -> Vec<Finding> {
    let r = rules();
    let home = home.filter(|h| h.len() > 1 && *h != "/");
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let mut push = |kind: &'static str, excerpt: String| out.push(Finding { file: file.into(), line: i + 1, kind, excerpt });
        let home_hit = home.and_then(|h| line.find(h).map(|_| h.to_string())).or_else(|| r.home.find(line).map(|m| m.as_str().trim().to_string()));
        if let Some(h) = home_hit {
            push("home-directory path", h);
        } else if let Some(c) = r.absolute.captures(line) {
            push("absolute path", c[1].to_string());
        }
        let mut secret = false;
        for (kind, re) in &r.tokens {
            if let Some(m) = re.find(line) {
                push(kind, mask(m.as_str()));
                secret = true;
            }
        }
        if secret {
            continue;
        }
        if let Some(m) = r.candidate.find_iter(line).find(|m| high_entropy(m.as_str())) {
            push("high-entropy string", mask(m.as_str()));
        } else if let Some(c) = r.hex.captures(line) {
            if !line[..c.get(1).unwrap().start()].ends_with("sha256:") {
                push("long hex string", mask(&c[1]));
            }
        }
    }
    out
}

/// Text safe to print in a report (`adage survey`): the same rules as [`scan`], applied as
/// replacements. Home-directory paths become `~` (another user's home, `~user`), and tokens,
/// high-entropy strings and long hex strings become `[redacted]`.
pub fn scrub(text: &str, home: Option<&str>) -> String {
    let r = rules();
    let mut s = text.to_string();
    if let Some(h) = home.filter(|h| h.len() > 1 && *h != "/") {
        s = s.replace(h.trim_end_matches('/'), "~");
    }
    static OTHER_HOME: OnceLock<Regex> = OnceLock::new();
    let other = OTHER_HOME.get_or_init(|| Regex::new(r#"/(?:Users|home)/([^/\s"']+)"#).unwrap());
    s = other.replace_all(&s, "~$1").into_owned();
    for (_, re) in &r.tokens {
        s = re.replace_all(&s, "[redacted]").into_owned();
    }
    s = r.candidate.replace_all(&s, |c: &regex::Captures| if high_entropy(&c[0]) { "[redacted]".to_string() } else { c[0].to_string() }).into_owned();
    r.hex.replace_all(&s, |c: &regex::Captures| c[0].replace(&c[1], "[redacted]")).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrub_redacts_what_scan_finds() {
        let home = Some("/Users/alice");
        assert_eq!(scrub("cd /Users/alice/src/billing && make", home), "cd ~/src/billing && make");
        assert_eq!(scrub("see /home/bob/x", home), "see ~bob/x");
        let token = format!("use ghp_{} now", "A1b2C3d4E5".repeat(4));
        assert_eq!(scrub(&token, home), "use [redacted] now");
        assert_eq!(scrub("key q8Zr2LwP9vKx3NmT7bYc5HdJ1fGs here", home), "key [redacted] here");
        assert_eq!(scrub("commit 9f2c4e1a7b3d5f6e8a0c2b4d6f8e0a1c", home), "commit [redacted]");
        assert_eq!(scrub("Pick up W-1042 and fix the tests", home), "Pick up W-1042 and fix the tests");
    }

    fn kinds(text: &str) -> Vec<&'static str> {
        scan("f", text, Some("/Users/alice")).into_iter().map(|f| f.kind).collect()
    }

    #[test]
    fn catches_paths_and_tokens() {
        assert_eq!(kinds("command: /opt/tools/bin/wi-lookup {item}"), vec!["absolute path"]);
        assert_eq!(kinds("summary: \"see /Users/alice/src/billing\""), vec!["home-directory path"]);
        assert_eq!(kinds("summary: read ~/notes.txt first"), vec!["home-directory path"]);
        assert_eq!(kinds("repo = \"/home/bob/work/repo\""), vec!["home-directory path"]);
        let fake = format!("token: ghp_{}", "A1b2C3d4E5".repeat(4));
        assert!(kinds(&fake).contains(&"GitHub token"));
        assert_eq!(kinds("AKIAABCDEFGHIJKLMNOP"), vec!["AWS access key"]);
        assert_eq!(kinds("x: q8Zr2LwP9vKx3NmT7bYc5HdJ1fGs"), vec!["high-entropy string"]);
        assert_eq!(kinds("x: 9f2c4e1a7b3d5f6e8a0c2b4d6f8e0a1c"), vec!["long hex string"]);
        // The finding never contains the secret itself.
        let f = &scan("f", &fake, None)[0];
        assert!(!f.excerpt.contains("A1b2C3d4E5A1b2"), "{}", f.excerpt);
    }

    #[test]
    fn ordinary_track_text_is_clean() {
        for ok in [
            "name = \"github.com-acme-billing/work-items\"",
            "repo = \"github.com/acme/billing\"",
            "  command: ./scripts/wi-lookup {item} --json",
            "  regex: ^W-\\d{4}$",
            "  paths: schema/*.json",
            "- claude-haiku-4-5-20251001",
            "summary: Context gathering for prompts that name a work item",
            "pin: sha256:9f2c4e1a7b3d5f6e8a0c2b4d6f8e0a1c9f2c4e1a7b3d5f6e8a0c2b4d6f8e0a1c",
            "url: https://github.com/acme/billing",
            "signature: No module named pytest",
        ] {
            assert!(kinds(ok).is_empty(), "{ok:?} flagged: {:?}", kinds(ok));
        }
    }
}
