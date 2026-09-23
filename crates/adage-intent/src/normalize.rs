//! Prompt normalisation before embedding (Design §8.2.1 step 2), ported from spike S2's
//! `normalize.py`, whose measured effect (+13.6 points of family top-1 for static embeddings) is
//! why it exists. Spans become typed placeholders so the vector carries the intent, not the
//! identifier: "pick up W-4821" → "pick up {ticket}". Fenced code is replaced by a marker, and only
//! the head of the prompt is kept.

use once_cell::sync::Lazy;
use regex::Regex;

/// Whitespace tokens kept from the head of the prompt.
pub const HEAD_TOKENS: usize = 128;
/// Bytes of the raw prompt ever looked at: the hot path stays bounded whatever is pasted.
pub const MAX_INPUT: usize = 16 * 1024;

static FENCE: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?s)```.*?```").unwrap());
// Order matters (as in S2): URLs and tickets before the generic path pattern.
static RULES: Lazy<Vec<(Regex, &'static str)>> = Lazy::new(|| {
    [
        (r"https?://\S+", "{url}"),
        // S2 used `[A-Z]{2,6}-\d{1,6}`, which misses one-letter prefixes (`W-4821`); this is the
        // ticket span pattern adage-core binds slots with.
        (r"\b[A-Z][A-Z0-9]{0,9}-\d{1,7}\b", "{ticket}"),
        (r"#\d{1,6}\b", "{pr}"),
        (r"\b\w[\w\-./]*/[\w\-./]+\b", "{path}"),
        (r"\b[0-9a-f]{7,40}\b", "{sha}"),
        (r"\b\d+\.\d+(?:\.\d+)?\b", "{version}"),
        (r"\b[A-Z][A-Z0-9]*(?:_[A-Z0-9]+)+\b", "{env_var}"),
    ]
    .into_iter()
    .map(|(r, t)| (Regex::new(r).unwrap(), t))
    .collect()
});

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Normalized {
    pub text: String,
    pub has_fence: bool,
    pub placeholders: usize,
}

pub fn normalize_full(raw: &str) -> Normalized {
    let mut cut = raw.len().min(MAX_INPUT);
    while !raw.is_char_boundary(cut) {
        cut -= 1;
    }
    let raw = &raw[..cut];
    let has_fence = raw.contains("```");
    let mut text = FENCE.replace_all(raw, " [code] ").into_owned();
    let mut placeholders = 0;
    for (re, token) in RULES.iter() {
        placeholders += re.find_iter(&text).count();
        text = re.replace_all(&text, *token).into_owned();
    }
    let text = text.split_whitespace().take(HEAD_TOKENS).collect::<Vec<_>>().join(" ");
    Normalized { text, has_fence, placeholders }
}

/// The normalised text that is embedded.
pub fn normalize(raw: &str) -> String {
    normalize_full(raw).text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spans_become_placeholders() {
        assert_eq!(normalize("pick up W-4821"), "pick up {ticket}");
        assert_eq!(normalize("pick up ENG-4821"), "pick up {ticket}");
        assert_eq!(normalize("fix src/auth/token.py please"), "fix {path} please");
        assert_eq!(normalize("review #482 at https://x.dev/pr/1"), "review {pr} at {url}");
        assert_eq!(normalize("bump react to 18.3.1"), "bump react to {version}");
        assert_eq!(normalize("set DATABASE_URL now"), "set {env_var} now");
        assert_eq!(normalize("revert a3f9c21"), "revert {sha}");
    }

    #[test]
    fn code_is_stripped_and_head_kept() {
        let n = normalize_full("why?\n```\nTraceback\n  boom\n```\nthanks");
        assert!(n.has_fence);
        assert_eq!(n.text, "why? [code] thanks");
        let long: String = (0..500).map(|i| format!("w{i} ")).collect();
        assert_eq!(normalize(&long).split(' ').count(), HEAD_TOKENS);
    }

    #[test]
    fn bounded_on_huge_input() {
        let big = "é".repeat(1 << 20);
        let t = std::time::Instant::now();
        let _ = normalize(&big);
        assert!(t.elapsed().as_millis() < 500);
    }
}
