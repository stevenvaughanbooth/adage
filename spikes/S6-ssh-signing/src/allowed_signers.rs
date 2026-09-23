//! Minimal parser for the OpenSSH `allowed_signers` file format, since the
//! `ssh-key` crate (0.6.7) implements `sshsig` but has no allowed_signers or
//! namespace-restriction support at all -- this is Adage-side code, not
//! something the crate gives us for free.
//!
//! Line format (see ssh-keygen(1), "ALLOWED SIGNERS"):
//!
//!   principals [options] keytype base64key [comment]
//!
//! options is a comma-separated list of `name` or `name="value"`, where a
//! quoted value may itself not contain a comma outside quotes. Recognised
//! options here: `namespaces="ns1,ns2"`, `valid-after=timestamp`,
//! `valid-before=timestamp`, `cert-authority` (parsed but unused: certificate
//! signers are out of scope for this spike).

use anyhow::{anyhow, bail, Context, Result};
use ssh_key::PublicKey;
use std::path::Path;
use std::time::{Duration, SystemTime};

pub struct AllowedSigner {
    pub principals: Vec<String>,
    pub namespaces: Option<Vec<String>>,
    pub valid_after: Option<SystemTime>,
    pub valid_before: Option<SystemTime>,
    pub key: PublicKey,
    #[allow(dead_code)] // kept for debugging / future error messages
    pub raw_line: String,
}

impl AllowedSigner {
    pub fn allows_namespace(&self, ns: &str) -> bool {
        match &self.namespaces {
            None => true,
            Some(list) => list.iter().any(|n| n == ns),
        }
    }

    pub fn allows_time(&self, at: SystemTime) -> bool {
        if let Some(after) = self.valid_after {
            if at < after {
                return false;
            }
        }
        if let Some(before) = self.valid_before {
            if at > before {
                return false;
            }
        }
        true
    }
}

fn split_options(s: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;
    for c in s.chars() {
        match c {
            '"' => in_quotes = !in_quotes,
            ',' if !in_quotes => {
                parts.push(std::mem::take(&mut cur));
                continue;
            }
            _ => {}
        }
        cur.push(c);
    }
    if !cur.is_empty() {
        parts.push(cur);
    }
    parts
}

/// Parse an OpenSSH-style timestamp: YYYYMMDD[Z] or YYYYMMDDHHMM[SS][Z].
/// A trailing 'Z' means UTC; otherwise we treat it as UTC too (a documented
/// simplification -- see FINDINGS.md limitations. ssh-keygen uses the local
/// timezone for unsuffixed timestamps).
pub fn parse_timestamp(raw: &str) -> Result<SystemTime> {
    let (digits, _forced_utc) = if let Some(stripped) = raw.strip_suffix('Z') {
        (stripped, true)
    } else {
        (raw, true) // simplification: always UTC
    };

    let (y, mo, d, h, mi, s) = match digits.len() {
        8 => (
            &digits[0..4],
            &digits[4..6],
            &digits[6..8],
            "00",
            "00",
            "00",
        ),
        12 => (
            &digits[0..4],
            &digits[4..6],
            &digits[6..8],
            &digits[8..10],
            &digits[10..12],
            "00",
        ),
        14 => (
            &digits[0..4],
            &digits[4..6],
            &digits[6..8],
            &digits[8..10],
            &digits[10..12],
            &digits[12..14],
        ),
        _ => bail!("unsupported timestamp format: {raw:?}"),
    };
    let (y, mo, d, h, mi, s): (i64, i64, i64, i64, i64, i64) = (
        y.parse()?,
        mo.parse()?,
        d.parse()?,
        h.parse()?,
        mi.parse()?,
        s.parse()?,
    );

    // Days-from-civil algorithm (Howard Hinnant), proleptic Gregorian, UTC.
    let y2 = if mo <= 2 { y - 1 } else { y };
    let era = if y2 >= 0 { y2 } else { y2 - 399 } / 400;
    let yoe = (y2 - era * 400) as i64;
    let mp = (mo + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days_since_epoch = era * 146097 + doe - 719468;

    let secs = days_since_epoch * 86400 + h * 3600 + mi * 60 + s;
    if secs < 0 {
        bail!("timestamp before unix epoch: {raw:?}");
    }
    Ok(SystemTime::UNIX_EPOCH + Duration::from_secs(secs as u64))
}

pub fn parse_file(path: &Path) -> Result<Vec<AllowedSigner>> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("reading allowed_signers file {:?}", path))?;
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        out.push(parse_line(line)?);
    }
    Ok(out)
}

fn looks_like_keytype(tok: &str) -> bool {
    matches!(
        tok,
        "ssh-ed25519"
            | "ssh-rsa"
            | "ssh-dss"
            | "ecdsa-sha2-nistp256"
            | "ecdsa-sha2-nistp384"
            | "ecdsa-sha2-nistp521"
            | "sk-ssh-ed25519@openssh.com"
            | "sk-ecdsa-sha2-nistp256@openssh.com"
    )
}

fn parse_line(line: &str) -> Result<AllowedSigner> {
    let tokens: Vec<&str> = line.split_whitespace().collect();
    if tokens.len() < 3 {
        bail!("malformed allowed_signers line: {line:?}");
    }
    let principals: Vec<String> = tokens[0].split(',').map(|s| s.to_string()).collect();

    let (opts_tok, keytype, keydata) = if looks_like_keytype(tokens[1]) {
        (None, tokens[1], tokens[2])
    } else {
        if tokens.len() < 4 {
            bail!("malformed allowed_signers line (missing key): {line:?}");
        }
        (Some(tokens[1]), tokens[2], tokens[3])
    };

    let mut namespaces = None;
    let mut valid_after = None;
    let mut valid_before = None;

    if let Some(opts) = opts_tok {
        for part in split_options(opts) {
            let (name, value) = match part.split_once('=') {
                Some((n, v)) => (n, Some(v.trim_matches('"'))),
                None => (part.as_str(), None),
            };
            match name {
                "namespaces" => {
                    let v = value.ok_or_else(|| anyhow!("namespaces= needs a value"))?;
                    namespaces = Some(v.split(',').map(|s| s.to_string()).collect());
                }
                "valid-after" => {
                    valid_after =
                        Some(parse_timestamp(value.ok_or_else(|| {
                            anyhow!("valid-after= needs a value")
                        })?)?);
                }
                "valid-before" => {
                    valid_before =
                        Some(parse_timestamp(value.ok_or_else(|| {
                            anyhow!("valid-before= needs a value")
                        })?)?);
                }
                "cert-authority" => { /* parsed, not used: no certificate support here */ }
                _ => { /* ignore unknown options, same as ssh-keygen's forward-compat stance */ }
            }
        }
    }

    let full_key = format!("{keytype} {keydata}");
    let key = full_key
        .parse::<PublicKey>()
        .with_context(|| format!("parsing public key in allowed_signers line: {line:?}"))?;

    Ok(AllowedSigner {
        principals,
        namespaces,
        valid_after,
        valid_before,
        key,
        raw_line: line.to_string(),
    })
}
