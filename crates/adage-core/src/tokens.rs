//! The shared tokeniser (Track G §4.1): one definition of "a token" for the learner (value-flow
//! induction, `adage-graph`) and the runtime (binding a prompt value to a learned value type).
//!
//! - A text is split on whitespace and on every ASCII character that is neither alphanumeric nor
//!   one of `-_./#:@+=` (so on ``,;()[]<>"'` `` and also on `!?$&|*{}\~^%`). Those seven stay
//!   inside tokens, and so does any non-ASCII character (it is part of a word; no learned type can
//!   accept it anyway).
//! - Sentence punctuation that ends a token (`.` and `:`) is trimmed: `BIL-1041.` is `BIL-1041`.
//! - Every token is followed by its **sub-tokens**: the non-empty parts between `/`, `#`, `:` and
//!   `=`. So `ops/412` yields `ops/412`, `ops`, `412`; `#123` yields `#123`, `123`; and
//!   `--issue=123` yields `--issue=123`, `--issue`, `123`. A sub-token equal to its token is not
//!   repeated.
//!
//! Bounded: only the first [`MAX_BYTES`] of a text are read, a token longer than
//! [`MAX_TOKEN_LEN`] bytes is skipped (with its sub-tokens), and at most `max` tokens are returned.
//! Deterministic: a pure function of (text, max), in order of appearance.

/// Only this much of a text is tokenised.
pub const MAX_BYTES: usize = 64 * 1024;
/// Tokens longer than this are skipped: no learned value type accepts more than 128 bytes.
pub const MAX_TOKEN_LEN: usize = 256;
/// The runtime's bound for one prompt (Track G §7.4).
pub const PROMPT_TOKENS: usize = 256;

/// One token of a text.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Token {
    pub text: String,
    /// Byte offset of the token (or sub-token) in the text.
    pub start: usize,
    /// True for a sub-token (a part of the token before it).
    pub sub: bool,
}

/// Characters kept inside a token besides ASCII alphanumerics and non-ASCII characters.
pub const KEEP: &str = "-_./#:@+=";
/// Characters sub-tokens are split on.
pub const SUB_SPLIT: &str = "/#:=";

fn is_token_char(c: char) -> bool {
    !c.is_ascii() || c.is_ascii_alphanumeric() || KEEP.contains(c)
}

/// Cut `text` to at most `max` bytes, on a character boundary.
pub fn bounded(text: &str, max: usize) -> &str {
    let mut cut = text.len().min(max);
    while !text.is_char_boundary(cut) {
        cut -= 1;
    }
    &text[..cut]
}

/// The sub-tokens of one token, with their byte offsets inside it: the non-empty parts between
/// the characters of [`SUB_SPLIT`]. Empty when the token has no such separator.
pub fn subs(token: &str) -> Vec<(usize, &str)> {
    if !token.contains(|c| SUB_SPLIT.contains(c)) {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut start = 0;
    for (i, c) in token.char_indices() {
        if SUB_SPLIT.contains(c) {
            if i > start {
                out.push((start, &token[start..i]));
            }
            start = i + c.len_utf8();
        }
    }
    if token.len() > start {
        out.push((start, &token[start..]));
    }
    out
}

/// Tokenise `text` (see the module documentation). At most `max` tokens (sub-tokens included).
pub fn tokens(text: &str, max: usize) -> Vec<Token> {
    let text = bounded(text, MAX_BYTES);
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    let emit = |s: usize, e: usize, out: &mut Vec<Token>| {
        let raw = &text[s..e];
        let t = raw.trim_end_matches(['.', ':']);
        if t.is_empty() || t.len() > MAX_TOKEN_LEN {
            return;
        }
        if out.len() < max {
            out.push(Token { text: t.to_string(), start: s, sub: false });
        }
        let mut seen: Vec<&str> = Vec::new();
        for (off, sub) in subs(t) {
            let sub = sub.trim_end_matches(['.', ':']);
            if sub.is_empty() || sub == t || seen.contains(&sub) {
                continue;
            }
            seen.push(sub);
            if out.len() < max {
                out.push(Token { text: sub.to_string(), start: s + off, sub: true });
            }
        }
    };
    for (i, c) in text.char_indices() {
        if out.len() >= max {
            break;
        }
        if is_token_char(c) {
            start.get_or_insert(i);
        } else if let Some(s) = start.take() {
            emit(s, i, &mut out);
        }
    }
    if let Some(s) = start {
        if out.len() < max {
            emit(s, text.len(), &mut out);
        }
    }
    out.truncate(max);
    out
}

/// A token's **mention**: the literal text around it inside the token it was split from (`#` in
/// `#482` for the sub-token `482`); empty for a whole token.
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize)]
pub struct Mention {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub prefix: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub suffix: String,
}

/// [`tokens`], each with its [`Mention`].
pub fn with_mentions(text: &str, max: usize) -> Vec<(Token, Mention)> {
    let toks = tokens(text, max);
    let mut parent: Option<&Token> = None;
    let mut out = Vec::with_capacity(toks.len());
    for t in &toks {
        let m = match (t.sub, parent) {
            (true, Some(p)) if t.start >= p.start && t.start + t.text.len() <= p.start + p.text.len() => {
                let off = t.start - p.start;
                Mention { prefix: p.text[..off].to_string(), suffix: p.text[off + t.text.len()..].to_string() }
            }
            _ => Mention::default(),
        };
        if !t.sub {
            parent = Some(t);
        }
        out.push((t.clone(), m));
    }
    out
}

/// The distinct token texts (tokens and sub-tokens) of `text`, in order of first appearance.
pub fn words(text: &str, max: usize) -> Vec<String> {
    let mut seen = std::collections::BTreeSet::new();
    tokens(text, max).into_iter().filter(|t| seen.insert(t.text.clone())).map(|t| t.text).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(s: &str) -> Vec<String> {
        tokens(s, 1000).into_iter().map(|t| t.text).collect()
    }

    #[test]
    fn splits_and_keeps() {
        assert_eq!(texts("Pick up BIL-1041, now."), ["Pick", "up", "BIL-1041", "now"]);
        assert_eq!(texts("see (a_b.c) [x] <y> \"q\" 'r' `s`;t"), ["see", "a_b.c", "x", "y", "q", "r", "s", "t"]);
        assert_eq!(texts("rm $(id) | sh && x"), ["rm", "id", "sh", "x"]);
    }

    #[test]
    fn sub_tokens() {
        assert_eq!(texts("ops/412"), ["ops/412", "ops", "412"]);
        assert_eq!(texts("fix #123."), ["fix", "#123", "123"]);
        assert_eq!(texts("--issue=123"), ["--issue=123", "--issue", "123"]);
        assert_eq!(texts("a:b:a"), ["a:b:a", "a", "b"]);
        assert_eq!(texts("fix/login-42"), ["fix/login-42", "fix", "login-42"]);
        assert_eq!(subs("#482"), vec![(1, "482")]);
        assert!(subs("BIL-1041").is_empty());
    }

    #[test]
    fn mentions() {
        let m: Vec<(String, String, String)> =
            with_mentions("fix #123 and ops/4.", 100).into_iter().map(|(t, m)| (t.text, m.prefix, m.suffix)).collect();
        let s = |a: &str, b: &str, c: &str| (a.to_string(), b.to_string(), c.to_string());
        assert_eq!(
            m,
            [
                s("fix", "", ""),
                s("#123", "", ""),
                s("123", "#", ""),
                s("and", "", ""),
                s("ops/4", "", ""),
                s("ops", "", "/4"),
                s("4", "ops/", "")
            ]
        );
    }

    #[test]
    fn bounded_and_deterministic() {
        let big: String = (0..100_000).map(|i| format!("W-{i} ")).collect();
        let t = tokens(&big, PROMPT_TOKENS);
        assert_eq!(t.len(), PROMPT_TOKENS);
        assert_eq!(t, tokens(&big, PROMPT_TOKENS));
        let long = "a".repeat(MAX_TOKEN_LEN + 1);
        assert_eq!(texts(&format!("x {long} y")), ["x", "y"]);
        // Unicode stays inside a token and never splits a character.
        assert_eq!(texts("café-12 ünï"), ["café-12", "ünï"]);
        let s = "é".repeat(MAX_BYTES);
        assert!(tokens(&s, 10).len() <= 10);
    }
}
