//! The BERT uncased WordPiece tokenizer potion-base-8M ships with (`BertNormalizer` with
//! lowercasing, `BertPreTokenizer`, `WordPiece` with `##` continuations), in plain std Rust.
//!
//! Differences from Hugging Face `tokenizers`, all confined to non-ASCII text: accents are stripped
//! for Latin-1 letters and combining marks only (no full NFD table), and Unicode punctuation is
//! recognised for the common blocks only. Learning and matching both use this implementation, so
//! the two sides always agree; only similarity to the reference model's vectors differs on such
//! text.

use std::borrow::Cow;
use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};

/// The vocabulary: token → id. Keys borrow the text when it lives for the process (the model
/// bundled in the binary), so building it allocates nothing on the hook path.
pub type Vocab = HashMap<Cow<'static, str>, u32, BuildHasherDefault<Fnv>>;

/// FNV-1a. The vocabulary is built once per process and looked up a few dozen times per prompt;
/// its keys come from a table compiled into the binary or pinned by digest, so SipHash's
/// resistance to chosen keys buys nothing here.
pub struct Fnv(u64);

impl Default for Fnv {
    fn default() -> Self {
        Fnv(0xcbf2_9ce4_8422_2325)
    }
}

impl Hasher for Fnv {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 ^= u64::from(b);
            self.0 = self.0.wrapping_mul(0x100_0000_01b3);
        }
    }
}

pub struct WordPiece {
    pub vocab: Vocab,
    pub unk: u32,
    pub max_chars: usize,
}

fn is_cjk(c: char) -> bool {
    matches!(c as u32,
        0x4E00..=0x9FFF | 0x3400..=0x4DBF | 0x20000..=0x2A6DF | 0x2A700..=0x2B73F
        | 0x2B740..=0x2B81F | 0x2B820..=0x2CEAF | 0xF900..=0xFAFF | 0x2F800..=0x2FA1F)
}

fn is_punct(c: char) -> bool {
    let u = c as u32;
    matches!(u, 33..=47 | 58..=64 | 91..=96 | 123..=126)
        || matches!(u, 0x00A1..=0x00BF | 0x2010..=0x2027 | 0x2030..=0x205E | 0x3000..=0x303F | 0xFF01..=0xFF0F
            | 0xFF1A..=0xFF20 | 0xFF3B..=0xFF40 | 0xFF5B..=0xFF65)
}

fn fold(c: char) -> char {
    match c {
        'à'..='å' => 'a',
        'ç' => 'c',
        'è'..='ë' => 'e',
        'ì'..='ï' => 'i',
        'ñ' => 'n',
        'ò'..='ö' | 'ø' => 'o',
        'ù'..='ü' => 'u',
        'ý' | 'ÿ' => 'y',
        _ => c,
    }
}

/// `BertNormalizer` (clean text, pad CJK, lowercase, strip accents) + `BertPreTokenizer`
/// (whitespace split, punctuation isolated): the words WordPiece sees.
pub fn pre_tokenize(text: &str) -> Vec<String> {
    let mut words: Vec<String> = Vec::new();
    let mut cur = String::new();
    let flush = |cur: &mut String, words: &mut Vec<String>| {
        if !cur.is_empty() {
            words.push(std::mem::take(cur));
        }
    };
    for c in text.chars() {
        if c == '\0' || c == '\u{FFFD}' || (c.is_control() && !c.is_whitespace()) {
            continue;
        }
        if c.is_whitespace() {
            flush(&mut cur, &mut words);
            continue;
        }
        if is_cjk(c) || is_punct(c) {
            flush(&mut cur, &mut words);
            words.push(c.to_string());
            continue;
        }
        for l in c.to_lowercase() {
            if matches!(l as u32, 0x0300..=0x036F) {
                continue; // combining marks
            }
            cur.push(fold(l));
        }
    }
    flush(&mut cur, &mut words);
    words
}

impl WordPiece {
    /// Token ids for `text`, `[UNK]` included (callers drop it, as model2vec does).
    pub fn encode(&self, text: &str, max_tokens: usize) -> Vec<u32> {
        let mut out = Vec::new();
        for w in pre_tokenize(text) {
            if out.len() >= max_tokens {
                break;
            }
            self.word(&w, &mut out);
        }
        out.truncate(max_tokens);
        out
    }

    fn word(&self, w: &str, out: &mut Vec<u32>) {
        let chars: Vec<char> = w.chars().collect();
        if chars.len() > self.max_chars {
            out.push(self.unk);
            return;
        }
        let mut pieces = Vec::new();
        let mut start = 0;
        while start < chars.len() {
            let mut end = chars.len();
            let mut found = None;
            while start < end {
                let mut s: String = chars[start..end].iter().collect();
                if start > 0 {
                    s.insert_str(0, "##");
                }
                if let Some(&id) = self.vocab.get(s.as_str()) {
                    found = Some(id);
                    break;
                }
                end -= 1;
            }
            match found {
                Some(id) => pieces.push(id),
                None => {
                    out.push(self.unk);
                    return;
                }
            }
            start = end;
        }
        out.extend(pieces);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bert_pre_tokenization() {
        assert_eq!(pre_tokenize("Fix {path}, please!"), vec!["fix", "{", "path", "}", ",", "please", "!"]);
        assert_eq!(pre_tokenize("Café  déjà\tvu"), vec!["cafe", "deja", "vu"]);
    }

    #[test]
    fn greedy_longest_match() {
        let vocab: Vocab =
            [("[UNK]", 0), ("test", 1), ("##ing", 2), ("tests", 3), ("un", 4), ("##believ", 5), ("##able", 6)]
                .into_iter()
                .map(|(k, v)| (Cow::Borrowed(k), v))
                .collect();
        let wp = WordPiece { vocab, unk: 0, max_chars: 100 };
        assert_eq!(wp.encode("testing tests unbelievable xyz", 512), vec![1, 2, 3, 4, 5, 6, 0]);
        assert_eq!(wp.encode("testing tests", 2), vec![1, 2]);
    }
}
