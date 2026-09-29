//! Secret-looking text and secret-holding files: the one set of rules for what Adage never
//! copies out of a trace. `adage share`'s export check (`adage-cli/src/redact.rs`) scans packs
//! with [`token_rules`]; procedure cards (`adage_learn::cards`, `adage_tracks::card::check`) drop
//! any line, command or error that [`has_secret`] flags, and never take content from a file
//! [`sensitive_path`] names.

use regex::Regex;
use std::sync::OnceLock;

/// Known token formats, by kind.
const TOKENS: &[(&str, &str)] = &[
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
    (
        "credential assignment",
        r"(?i)\b(?:password|passwd|secret|api[_-]?key|access[_-]?token|auth[_-]?token)\s*[:=]\s*\S{8,}",
    ),
];

/// The token rules, compiled once: `(kind, pattern)`.
pub fn token_rules() -> &'static [(&'static str, Regex)] {
    static R: OnceLock<Vec<(&'static str, Regex)>> = OnceLock::new();
    R.get_or_init(|| TOKENS.iter().map(|(k, r)| (*k, Regex::new(r).unwrap())).collect())
}

/// Shannon entropy in bits per character.
pub fn entropy(s: &str) -> f64 {
    let mut counts = [0u32; 256];
    for b in s.bytes() {
        counts[b as usize] += 1;
    }
    let n = s.len() as f64;
    counts.iter().filter(|&&c| c > 0).map(|&c| -(c as f64 / n) * (c as f64 / n).log2()).sum()
}

/// Random-looking: long, mixed upper, lower and digits, and near the entropy of random base64.
pub fn high_entropy(t: &str) -> bool {
    t.bytes().any(|c| c.is_ascii_uppercase())
        && t.bytes().any(|c| c.is_ascii_lowercase())
        && t.bytes().any(|c| c.is_ascii_digit())
        && entropy(t) >= 4.0
}

struct Extra {
    /// A quoted literal given to a name that says it is a credential (`api_key = "…"`).
    quoted: Regex,
    /// An environment-style assignment to such a name (`GITHUB_TOKEN=…`, `DB_PASSWORD: …`).
    env: Regex,
    candidate: Regex,
    hex: Regex,
}

fn extra() -> &'static Extra {
    static R: OnceLock<Extra> = OnceLock::new();
    R.get_or_init(|| Extra {
        quoted: Regex::new(
            r#"(?i)[a-z0-9_]*(?:token|secret|passw(?:or)?d|api[_-]?key|private[_-]?key|access[_-]?key|credentials?)[a-z0-9_]*["']?\s*[:=]\s*["'][^\s"']{8,}"#,
        )
        .unwrap(),
        env: Regex::new(
            r#"\b[A-Z0-9_]*(?:TOKEN|SECRET|PASSWORD|PASSWD|API_?KEY|PRIVATE_?KEY|ACCESS_?KEY|CREDENTIALS?)[A-Z0-9_]*\s*[:=]\s*[^\s"'$]{8,}"#,
        )
        .unwrap(),
        candidate: Regex::new(r"[A-Za-z0-9+_=-]{24,}").unwrap(),
        hex: Regex::new(r"(?:^|[^A-Za-z0-9])([0-9a-fA-F]{32,})\b").unwrap(),
    })
}

/// Whether text looks like it holds a secret: a known token, a credential assignment
/// (`GITHUB_TOKEN=…`, `password = "…"`), a random-looking string of 24 or more characters, or a
/// long hex string (not a `sha256:` pin). Deliberately broad: what it flags is dropped, never
/// shown.
pub fn has_secret(text: &str) -> bool {
    let x = extra();
    token_rules().iter().any(|(_, re)| re.is_match(text))
        || x.quoted.is_match(text)
        || x.env.is_match(text)
        || x.candidate.find_iter(text).any(|m| high_entropy(m.as_str()))
        || x.hex.captures_iter(text).any(|c| !text[..c.get(1).map_or(0, |m| m.start())].ends_with("sha256:"))
}

/// Whether a path names a file whose content is a secret by its kind: environment files
/// (`.env`, `.env.local`, `prod.env`, `.envrc`), keys and certificates' private halves (`*.pem`,
/// `*.key`, `id_rsa`, …), credential stores (`.netrc`, `.npmrc`, `.pypirc`, `.git-credentials`,
/// …), anything named for secrets, credentials or passwords, and anything under `.ssh`, `.aws`,
/// `.gnupg` or `.docker`.
pub fn sensitive_path(path: &str) -> bool {
    let p = path.replace('\\', "/").to_ascii_lowercase();
    let mut parts = p.split('/').filter(|s| !s.is_empty());
    let name = p.rsplit('/').next().unwrap_or("");
    if parts.any(|d| matches!(d, ".ssh" | ".aws" | ".gnupg" | ".docker" | ".kube")) {
        return true;
    }
    let ext = name.rsplit_once('.').map_or("", |(_, e)| e);
    name.starts_with(".env")
        || name.ends_with(".env")
        || ["secret", "credential", "password", "passwd", "private_key", "privatekey"].iter().any(|w| name.contains(w))
        || matches!(ext, "pem" | "key" | "p12" | "pfx" | "jks" | "keystore" | "gpg" | "asc" | "ppk" | "kdbx")
        || matches!(
            name,
            "id_rsa"
                | "id_dsa"
                | "id_ecdsa"
                | "id_ed25519"
                | ".netrc"
                | "_netrc"
                | ".npmrc"
                | ".pypirc"
                | ".pgpass"
                | ".htpasswd"
                | ".git-credentials"
                | ".dockercfg"
                | "kubeconfig"
                | "known_hosts"
                | "authorized_keys"
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_are_flagged() {
        for s in [
            &format!("GITHUB_TOKEN=ghp_{} gh pr create", "A1b2C3d4E5".repeat(4)),
            &format!("OPENAI_API_KEY = \"sk-{}\"", "abcDEF123_".repeat(3)),
            "export GITHUB_TOKEN=abcdefgh12345678",
            "DB_PASSWORD: hunter2hunter2",
            "  \"api_key\": \"q8Zr2LwP9vKx3NmT7bYc\",",
            "key q8Zr2LwP9vKx3NmT7bYc5HdJ1fGs here",
            "-----BEGIN OPENSSH PRIVATE KEY-----",
            "AKIAABCDEFGHIJKLMNOP",
            "secret = 9f2c4e1a7b3d5f6e8a0c2b4d6f8e0a1c",
        ] {
            assert!(has_secret(s), "{s}");
        }
        for s in [
            "def test_rounding():",
            "    assert round_half(2.5) == 2",
            "python -m pytest tests/test_v.py -xvs",
            "go test ./...",
            "flag.StringVar(&cfg.Name, \"name\", \"\", \"the name\")",
            "pin: sha256:9f2c4e1a7b3d5f6e8a0c2b4d6f8e0a1c9f2c4e1a7b3d5f6e8a0c2b4d6f8e0a1c",
            "No module named pytest",
            "token := lexer.Next()",
            "tokens = tokenize(source_text)",
        ] {
            assert!(!has_secret(s), "{s}");
        }
    }

    #[test]
    fn secret_holding_files_are_named() {
        for p in [
            ".env",
            ".env.local",
            "config/.env.production",
            "deploy/prod.env",
            ".envrc",
            "certs/server.pem",
            "tls/server.key",
            "home/.ssh/config",
            "config/credentials.yml",
            "config/secrets.json",
            ".npmrc",
            "id_ed25519",
            ".aws/config",
        ] {
            assert!(sensitive_path(p), "{p}");
        }
        for p in ["src/env.py", "src/click/core.py", "tests/test_keys.py", "Cargo.toml", ".github/workflows/ci.yml"] {
            assert!(!sensitive_path(p), "{p}");
        }
    }
}
