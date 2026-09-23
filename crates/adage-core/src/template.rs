//! Argument templating: typed placeholders for paths, IDs, SHAs, numbers, URLs.
//!
//! Design Spec §11: "Arguments are generalised into typed slots. In the early sample this
//! doubled the repeat rate, from 37% to 73%." This module produces the "template" key; the
//! "exact" key is just the literal argv joined.

use once_cell::sync::Lazy;
use regex::Regex;

static SHA_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^[0-9a-fA-F]{7,40}$").unwrap());
static UUID_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$")
        .unwrap()
});
static TICKET_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^[A-Z]{1,6}-\d+$").unwrap());
static PR_ISSUE_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^#\d+$").unwrap());
static NUMBER_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^-?\d+(\.\d+)?$").unwrap());
static URL_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^[a-zA-Z][a-zA-Z0-9+.-]*://").unwrap());
static PATH_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^(/|\./|\.\./|~/|[A-Za-z0-9_.-]+/)").unwrap());
static ENV_ASSIGN_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^[A-Za-z_][A-Za-z0-9_]*=").unwrap());

/// Classify a single bare-word operand into a typed placeholder, or `None` to keep it literal
/// (flags, subcommand names, and other low-cardinality tokens are kept literal so templates
/// stay legible — `git status` should not become `git {arg}`).
pub fn placeholder(token: &str) -> Option<&'static str> {
    if token.is_empty() {
        return None;
    }
    if URL_RE.is_match(token) {
        return Some("{url}");
    }
    if UUID_RE.is_match(token) {
        return Some("{uuid}");
    }
    if SHA_RE.is_match(token) && token.chars().any(|c| c.is_ascii_digit()) {
        return Some("{sha}");
    }
    if TICKET_RE.is_match(token) || PR_ISSUE_RE.is_match(token) {
        return Some("{id}");
    }
    if NUMBER_RE.is_match(token) {
        return Some("{n}");
    }
    if ENV_ASSIGN_RE.is_match(token) {
        return Some("{env}");
    }
    if PATH_RE.is_match(token) || token.contains('/') {
        return Some("{path}");
    }
    None
}

pub fn template_argv(argv: &[String]) -> Vec<String> {
    argv.iter()
        .map(|t| placeholder(t).map(|p| p.to_string()).unwrap_or_else(|| t.clone()))
        .collect()
}
