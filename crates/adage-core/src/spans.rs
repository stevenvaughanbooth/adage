//! Typed spans: values found in prompts and tool output that a route may bind into a step.
//!
//! "Propose with intelligence, validate with types" (Design §8): spans are only *candidates*.
//! A value reaches a directive only after passing its slot's type contract.

use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SpanKind {
    Ticket,
    Path,
    Sha,
    Url,
}

impl SpanKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            SpanKind::Ticket => "ticket",
            SpanKind::Path => "path",
            SpanKind::Sha => "sha",
            SpanKind::Url => "url",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Span {
    pub kind: SpanKind,
    pub value: String,
}

static TICKET: Lazy<Regex> = Lazy::new(|| Regex::new(r"\b[A-Z][A-Z0-9]{0,9}-\d{1,7}\b").unwrap());
static URL: Lazy<Regex> = Lazy::new(|| Regex::new(r#"\bhttps?://[^\s)>\]"'`]+"#).unwrap());
static SHA: Lazy<Regex> = Lazy::new(|| Regex::new(r"\b[0-9a-f]{7,40}\b").unwrap());
static PATH: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?:^|[\s`'(])((?:\.{0,2}/)?[A-Za-z0-9_.-]+(?:/[A-Za-z0-9_.-]+)*\.[A-Za-z0-9]{1,6})\b").unwrap()
});

/// Only the first part of a text is scanned, and at most this many spans are returned: the hot
/// path must stay bounded whatever an agent pastes or a tool prints.
pub const MAX_SCAN: usize = 64 * 1024;
pub const MAX_SPANS: usize = 256;

/// Extract spans in order of appearance, de-duplicated per kind.
pub fn extract(text: &str) -> Vec<Span> {
    let mut cut = text.len().min(MAX_SCAN);
    while !text.is_char_boundary(cut) {
        cut -= 1;
    }
    let text = &text[..cut];
    let mut out: Vec<(usize, Span)> = Vec::new();
    let mut seen: std::collections::HashSet<(SpanKind, String)> = std::collections::HashSet::new();
    let mut push = |pos: usize, kind: SpanKind, value: &str| {
        if out.len() < MAX_SPANS * 4 && seen.insert((kind, value.to_string())) {
            out.push((pos, Span { kind, value: value.to_string() }));
        }
    };
    for m in URL.find_iter(text) {
        push(m.start(), SpanKind::Url, m.as_str());
    }
    for m in TICKET.find_iter(text) {
        push(m.start(), SpanKind::Ticket, m.as_str());
    }
    for m in SHA.find_iter(text) {
        if m.as_str().chars().any(|c| c.is_ascii_digit()) && m.as_str().chars().any(|c| c.is_ascii_alphabetic()) {
            push(m.start(), SpanKind::Sha, m.as_str());
        }
    }
    for c in PATH.captures_iter(text) {
        let g = c.get(1).unwrap();
        if !g.as_str().contains("://") {
            push(g.start(), SpanKind::Path, g.as_str());
        }
    }
    out.sort_by_key(|(p, _)| *p);
    out.into_iter().take(MAX_SPANS).map(|(_, s)| s).collect()
}

/// Derive a strict, anchored contract regex from observed values of one ticket-like slot, e.g.
/// `W-1041`, `W-1046` → `^W-\d{4}$`. Returns `None` when values share no common shape.
pub fn ticket_contract(values: &[String]) -> Option<String> {
    let mut prefix: Option<String> = None;
    let (mut lo, mut hi) = (usize::MAX, 0usize);
    for v in values {
        let (p, digits) = v.rsplit_once('-')?;
        if !digits.chars().all(|c| c.is_ascii_digit()) || digits.is_empty() {
            return None;
        }
        match &prefix {
            None => prefix = Some(p.to_string()),
            Some(q) if q != p => return None,
            _ => {}
        }
        lo = lo.min(digits.len());
        hi = hi.max(digits.len());
    }
    let p = prefix?;
    let n = if lo == hi { format!("{{{lo}}}") } else { format!("{{{lo},{hi}}}") };
    Some(format!("^{}-\\d{}$", regex::escape(&p), n))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_ticket_and_path() {
        let s = extract("Pick up W-2042; see acme/billing.py and https://x.dev/a");
        assert!(s.contains(&Span { kind: SpanKind::Ticket, value: "W-2042".into() }));
        assert!(s.contains(&Span { kind: SpanKind::Path, value: "acme/billing.py".into() }));
        assert!(s.iter().any(|x| x.kind == SpanKind::Url));
    }

    #[test]
    fn extraction_is_bounded_and_fast() {
        let big: String = (0..80_000).map(|i| format!("W-{i} ")).collect();
        let t = std::time::Instant::now();
        let s = extract(&big);
        assert!(s.len() <= MAX_SPANS);
        assert!(t.elapsed().as_millis() < 500, "took {:?}", t.elapsed());
    }

    #[test]
    fn contract_from_values() {
        let v = vec!["W-1041".to_string(), "W-1046".to_string()];
        assert_eq!(ticket_contract(&v).unwrap(), r"^W-\d{4}$");
        let re = Regex::new(&ticket_contract(&v).unwrap()).unwrap();
        assert!(re.is_match("W-2042"));
        assert!(!re.is_match("W-2042; rm -rf /"));
        assert!(!re.is_match("--config=/x"));
    }
}
