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

/// One placeholder rule: its pattern, compiled the first time a prompt could match it, and a
/// literal it cannot match without. A hook process normalises one prompt, and compiling every
/// pattern (Unicode `\w` and `\b`) was a quarter of the prompt hook; a rule whose literal is not
/// in the text cannot match it, so skipping it changes nothing.
struct Rule {
    pattern: &'static str,
    token: &'static str,
    can_match: fn(&str) -> bool,
    re: std::sync::OnceLock<Regex>,
}

impl Rule {
    const fn new(pattern: &'static str, token: &'static str, can_match: fn(&str) -> bool) -> Rule {
        Rule { pattern, token, can_match, re: std::sync::OnceLock::new() }
    }

    fn re(&self) -> &Regex {
        self.re.get_or_init(|| Regex::new(self.pattern).unwrap())
    }
}

/// Seven lowercase hexadecimal characters in a row: what `{sha}` needs.
fn has_hex_run(text: &str) -> bool {
    let mut run = 0;
    for b in text.bytes() {
        run = if b.is_ascii_digit() || (b'a'..=b'f').contains(&b) { run + 1 } else { 0 };
        if run >= 7 {
            return true;
        }
    }
    false
}

// Order matters (as in S2): URLs and tickets before the generic path pattern. Each rule's literal
// is checked on the text as the rules before it left it; their placeholders contain none of the
// later rules' literals.
static RULES: [Rule; 7] = [
    Rule::new(r"https?://\S+", "{url}", |t| t.contains("://")),
    // S2 used `[A-Z]{2,6}-\d{1,6}`, which misses one-letter prefixes (`W-4821`); this is the
    // ticket span pattern adage-core binds slots with.
    Rule::new(r"\b[A-Z][A-Z0-9]{0,9}-\d{1,7}\b", "{ticket}", |t| t.contains('-')),
    Rule::new(r"#\d{1,6}\b", "{pr}", |t| t.contains('#')),
    Rule::new(r"\b\w[\w\-./]*/[\w\-./]+\b", "{path}", |t| t.contains('/')),
    Rule::new(r"\b[0-9a-f]{7,40}\b", "{sha}", has_hex_run),
    Rule::new(r"\b\d+\.\d+(?:\.\d+)?\b", "{version}", |t| t.contains('.')),
    Rule::new(r"\b[A-Z][A-Z0-9]*(?:_[A-Z0-9]+)+\b", "{env_var}", |t| t.contains('_')),
];

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
    let mut text = if has_fence { FENCE.replace_all(raw, " [code] ").into_owned() } else { raw.to_string() };
    let mut placeholders = 0;
    for rule in &RULES {
        if !(rule.can_match)(&text) {
            continue;
        }
        let re = rule.re();
        placeholders += re.find_iter(&text).count();
        text = re.replace_all(&text, rule.token).into_owned();
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

    /// Every rule applied to every prompt, as before the literal checks: the embeddings learned
    /// intents were built from depend on this text.
    fn reference(raw: &str) -> Normalized {
        let mut cut = raw.len().min(MAX_INPUT);
        while !raw.is_char_boundary(cut) {
            cut -= 1;
        }
        let raw = &raw[..cut];
        let has_fence = raw.contains("```");
        let mut text = FENCE.replace_all(raw, " [code] ").into_owned();
        let mut placeholders = 0;
        for rule in &RULES {
            let re = Regex::new(rule.pattern).unwrap();
            placeholders += re.find_iter(&text).count();
            text = re.replace_all(&text, rule.token).into_owned();
        }
        let text = text.split_whitespace().take(HEAD_TOKENS).collect::<Vec<_>>().join(" ");
        Normalized { text, has_fence, placeholders }
    }

    #[test]
    fn skipping_rules_that_cannot_match_changes_nothing() {
        let prompts = [
            "",
            "fix the failing test",
            "pick up W-4821 and ENG-12 then PROJ-1234567",
            "see https://example.com/a/b?c=1#frag and http://x.dev",
            "review #482, #1 and #1234567 at https://x.dev/pr/1",
            "fix src/auth/token.py and ./a-b/c.d/e_f please",
            "revert a3f9c21 and deadbeefcafe0123456789 but not abcdef or ABCDEF1",
            "bump react to 18.3.1 and node 22.1, python 3.12.0.1",
            "set DATABASE_URL and A_B_C but not _X or Y_ or snake_case",
            "unicode: café/naïve Ünïcode-12 été_2024 ü3.4 日本語/パス",
            "why?\n```\nTraceback (most recent call last):\n  File \"a/b.py\", line 3\n```\nthanks",
            "half a fence ``` and nothing after",
            "dashes - and dots . and slashes / and hashes # and underscores _ alone",
            "the 1234567 digits and 0000000 zeros, a-b-c, x.y.z, p/q/r",
            "tabs\tand\nnewlines\r\nin W-1 then 1.2 then {placeholder}",
        ];
        for p in prompts {
            assert_eq!(normalize_full(p), reference(p), "{p:?}");
        }
        let long: String = (0..2000).map(|i| format!("w{i}-{i}/x.{i}_y ")).collect();
        assert_eq!(normalize_full(&long), reference(&long));
    }

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
