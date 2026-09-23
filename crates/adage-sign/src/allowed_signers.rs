//! The OpenSSH `allowed_signers` format (ssh-keygen(1), "ALLOWED SIGNERS"), which `ssh-key` does
//! not implement. Ported from spike S6.
//!
//! ```text
//! principals [options] keytype base64key [comment]
//! ```
//!
//! Options honoured: `namespaces="pattern,…"` (with `*`, `?` and `!` negation, as ssh-keygen),
//! `valid-after=`, `valid-before=` (`YYYYMMDD[Z]` or `YYYYMMDDHHMM[SS][Z]`, read as UTC).
//! `cert-authority` entries are skipped: certificate signers are not supported, and a CA key must
//! never be accepted as a plain signing key.

use anyhow::{anyhow, bail, Context, Result};
use ssh_key::PublicKey;
use std::time::{Duration, SystemTime};

#[derive(Debug, Clone)]
pub struct AllowedSigner {
    pub principals: Vec<String>,
    pub namespaces: Option<String>,
    pub valid_after: Option<SystemTime>,
    pub valid_before: Option<SystemTime>,
    pub key: PublicKey,
}

impl AllowedSigner {
    pub fn allows_namespace(&self, ns: &str) -> bool {
        self.namespaces.as_deref().is_none_or(|list| match_pattern_list(ns, list))
    }

    pub fn allows_time(&self, at: SystemTime) -> bool {
        self.valid_after.is_none_or(|a| at >= a) && self.valid_before.is_none_or(|b| at <= b)
    }
}

/// Parse an allowed_signers file. A malformed line fails the whole file: a trust anchor that
/// cannot be read completely is not trusted at all.
pub fn parse(text: &str) -> Result<Vec<AllowedSigner>> {
    let mut out = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(s) = parse_line(line).with_context(|| format!("allowed_signers line {}", n + 1))? {
            out.push(s);
        }
    }
    Ok(out)
}

fn is_keytype(tok: &str) -> bool {
    tok.starts_with("ssh-") || tok.starts_with("ecdsa-sha2-") || tok.starts_with("sk-")
}

fn parse_line(line: &str) -> Result<Option<AllowedSigner>> {
    let tokens: Vec<&str> = line.split_whitespace().collect();
    if tokens.len() < 3 {
        bail!("malformed line");
    }
    let principals = tokens[0].split(',').map(str::to_string).collect();
    let (opts, keytype, keydata) = if is_keytype(tokens[1]) {
        (None, tokens[1], tokens[2])
    } else if tokens.len() >= 4 {
        (Some(tokens[1]), tokens[2], tokens[3])
    } else {
        bail!("malformed line (missing key)");
    };
    let (mut namespaces, mut valid_after, mut valid_before) = (None, None, None);
    for part in opts.map(split_options).unwrap_or_default() {
        let (name, value) = match part.split_once('=') {
            Some((n, v)) => (n.to_ascii_lowercase(), Some(v.trim_matches('"').to_string())),
            None => (part.to_ascii_lowercase(), None),
        };
        let need = |v: Option<String>| v.ok_or_else(|| anyhow!("option {name} needs a value"));
        match name.as_str() {
            "namespaces" => namespaces = Some(need(value)?),
            "valid-after" => valid_after = Some(parse_timestamp(&need(value)?)?),
            "valid-before" => valid_before = Some(parse_timestamp(&need(value)?)?),
            "cert-authority" => return Ok(None),
            _ => {}
        }
    }
    let key = PublicKey::from_openssh(&format!("{keytype} {keydata}")).context("parsing public key")?;
    Ok(Some(AllowedSigner { principals, namespaces, valid_after, valid_before, key }))
}

fn split_options(s: &str) -> Vec<String> {
    let (mut parts, mut cur, mut quoted) = (Vec::new(), String::new(), false);
    for c in s.chars() {
        match c {
            '"' => quoted = !quoted,
            ',' if !quoted => {
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

/// ssh_config(5) PATTERNS: a comma-separated list; a negated match refuses outright.
fn match_pattern_list(s: &str, list: &str) -> bool {
    let mut matched = false;
    for p in list.split(',').map(str::trim) {
        if let Some(neg) = p.strip_prefix('!') {
            if glob(neg.as_bytes(), s.as_bytes()) {
                return false;
            }
        } else if glob(p.as_bytes(), s.as_bytes()) {
            matched = true;
        }
    }
    matched
}

fn glob(p: &[u8], s: &[u8]) -> bool {
    match (p.first(), s.first()) {
        (None, None) => true,
        (Some(b'*'), _) => glob(&p[1..], s) || (!s.is_empty() && glob(p, &s[1..])),
        (Some(b'?'), Some(_)) => glob(&p[1..], &s[1..]),
        (Some(a), Some(b)) if a == b => glob(&p[1..], &s[1..]),
        _ => false,
    }
}

/// `YYYYMMDD[Z]` or `YYYYMMDDHHMM[SS][Z]`, read as UTC. (ssh-keygen reads unsuffixed values in
/// local time; a documented simplification from spike S6.)
pub fn parse_timestamp(raw: &str) -> Result<SystemTime> {
    let d = raw.strip_suffix('Z').unwrap_or(raw);
    if !matches!(d.len(), 8 | 12 | 14) || !d.bytes().all(|c| c.is_ascii_digit()) {
        bail!("unsupported timestamp {raw:?}");
    }
    let n = |r: std::ops::Range<usize>| -> i64 { d.get(r).map_or(0, |x| x.parse().unwrap_or(0)) };
    let (y, mo, day, h, mi, s) = (n(0..4), n(4..6), n(6..8), n(8..10), n(10..12), n(12..14));
    if !(1..=12).contains(&mo) || !(1..=31).contains(&day) || h > 23 || mi > 59 || s > 59 {
        bail!("invalid timestamp {raw:?}");
    }
    // Days from civil (Howard Hinnant), proleptic Gregorian.
    let y2 = if mo <= 2 { y - 1 } else { y };
    let era = if y2 >= 0 { y2 } else { y2 - 399 } / 400;
    let yoe = y2 - era * 400;
    let doy = (153 * ((mo + 9) % 12) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let secs = (era * 146_097 + doe - 719_468) * 86_400 + h * 3600 + mi * 60 + s;
    if secs < 0 {
        bail!("timestamp before 1970: {raw:?}");
    }
    Ok(SystemTime::UNIX_EPOCH + Duration::from_secs(secs as u64))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps_and_patterns() {
        let t = parse_timestamp("20200101").unwrap();
        assert_eq!(t.duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs(), 1_577_836_800);
        assert_eq!(parse_timestamp("20200101000000Z").unwrap(), t);
        assert!(parse_timestamp("2020-01-01").is_err());
        assert!(parse_timestamp("20201301").is_err());
        assert!(match_pattern_list("adage-track", "git,adage-*"));
        assert!(!match_pattern_list("adage-track", "git"));
        assert!(!match_pattern_list("adage-track", "*,!adage-track"));
    }
}
