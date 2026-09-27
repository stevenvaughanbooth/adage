//! Learned value types (Track G §4.2–4.3): run skeletons, their anchored regex, and the
//! hand-written O(len) matcher the runtime uses instead of compiling a regex in the hook.
//!
//! A value is read as **segments**: maximal runs of ASCII alphanumerics, and single characters of
//! anything else. A type is a union of at most [`MAX_BRANCHES`] **branches**; a branch is a
//! sequence of [`Run`]s, each of which consumes whole segments:
//!
//! - [`Run::Lit`]: one exact segment (an alphanumeric literal such as `BIL`, or one punctuation
//!   character such as `-`);
//! - [`Run::Set`]: one alphanumeric segment out of a literal set (at most 4);
//! - [`Run::Class`]: one alphanumeric segment of a character class, with a length range;
//! - [`Run::Star`]: `(sep class{lo,hi}){min,max}`, a repeated group.
//!
//! Alphanumeric runs in a branch are always separated by punctuation, so "one run consumes one
//! whole alphanumeric segment" is also exactly what the emitted regex means ([`to_regex`]). The
//! regex uses explicit ASCII classes (`[0-9]`, never `\d`, which is Unicode in Rust), so the
//! matcher and the regex agree on every string (property-tested in `adage-graph`).

use serde::{Deserialize, Serialize};

/// A type has at most this many branches.
pub const MAX_BRANCHES: usize = 3;
/// No value longer than this is ever accepted.
pub const MAX_LEN: usize = 128;

/// Character classes of alphanumeric runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Class {
    /// `[A-Z]`
    U,
    /// `[a-z]`
    L,
    /// `[0-9]`
    D,
    /// `[0-9a-f]`: a run that mixes digits and lowercase hex letters.
    H,
    /// `[A-Za-z0-9]`
    W,
}

impl Class {
    pub fn contains(self, c: char) -> bool {
        match self {
            Class::U => c.is_ascii_uppercase(),
            Class::L => c.is_ascii_lowercase(),
            Class::D => c.is_ascii_digit(),
            Class::H => c.is_ascii_digit() || ('a'..='f').contains(&c),
            Class::W => c.is_ascii_alphanumeric(),
        }
    }

    /// The number of characters in the class.
    pub fn size(self) -> u32 {
        match self {
            Class::U | Class::L => 26,
            Class::D => 10,
            Class::H => 16,
            Class::W => 62,
        }
    }

    pub fn regex(self) -> &'static str {
        match self {
            Class::U => "[A-Z]",
            Class::L => "[a-z]",
            Class::D => "[0-9]",
            Class::H => "[0-9a-f]",
            Class::W => "[A-Za-z0-9]",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Class::U => "U",
            Class::L => "L",
            Class::D => "D",
            Class::H => "H",
            Class::W => "W",
        }
    }

    /// Whether every character of this class can be a letter (so a run of it can spell a word).
    pub fn has_letters(self) -> bool {
        !matches!(self, Class::D)
    }
}

/// One element of a branch (see the module documentation).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "run", rename_all = "lowercase")]
pub enum Run {
    Lit { text: String },
    Set { texts: Vec<String> },
    Class { class: Class, lo: u8, hi: u8 },
    Star { sep: char, class: Class, lo: u8, hi: u8, min: u8, max: u8 },
}

impl Run {
    /// The largest number of bytes this run can consume.
    pub fn max_len(&self) -> usize {
        match self {
            Run::Lit { text } => text.len(),
            Run::Set { texts } => texts.iter().map(String::len).max().unwrap_or(0),
            Run::Class { hi, .. } => *hi as usize,
            Run::Star { sep, hi, max, .. } => (*max as usize) * (sep.len_utf8() + *hi as usize),
        }
    }

    /// log2 of the number of distinct strings this run matches.
    pub fn log2_size(&self) -> f64 {
        fn class_len(class: Class, lo: u8, hi: u8) -> f64 {
            // log2( sum_{n=lo..hi} |c|^n ), computed stably in the log domain.
            let b = (class.size() as f64).log2();
            let top = hi as f64 * b;
            let s: f64 = (lo..=hi).map(|n| ((n as f64 * b) - top).exp2()).sum();
            top + s.log2()
        }
        match self {
            Run::Lit { .. } => 0.0,
            Run::Set { texts } => (texts.len().max(1) as f64).log2(),
            Run::Class { class, lo, hi } => class_len(*class, *lo, *hi),
            Run::Star { class, lo, hi, min, max, .. } => {
                let g = class_len(*class, *lo, *hi);
                let top = *max as f64 * g;
                let s: f64 = (*min..=*max).map(|r| ((r as f64 * g) - top).exp2()).sum();
                top + s.log2()
            }
        }
    }
}

/// A learned type's structure: a union of branches.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default)]
pub struct Runs {
    pub branches: Vec<Vec<Run>>,
}

/// The anchored regex of a type. Always `^(?:…)$`.
pub fn to_regex(runs: &Runs) -> String {
    let branch = |b: &[Run]| -> String {
        let mut s = String::new();
        for r in b {
            match r {
                Run::Lit { text } => s.push_str(&regex_escape(text)),
                Run::Set { texts } => {
                    let alts: Vec<String> = texts.iter().map(|t| regex_escape(t)).collect();
                    s.push_str(&format!("(?:{})", alts.join("|")));
                }
                Run::Class { class, lo, hi } => s.push_str(&format!("{}{}", class.regex(), range(*lo, *hi))),
                Run::Star { sep, class, lo, hi, min, max } => s.push_str(&format!(
                    "(?:{}{}{}){}",
                    regex_escape(&sep.to_string()),
                    class.regex(),
                    range(*lo, *hi),
                    range(*min, *max)
                )),
            }
        }
        s
    };
    let alts: Vec<String> = runs.branches.iter().map(|b| branch(b)).collect();
    format!("^(?:{})$", alts.join("|"))
}

fn range(lo: u8, hi: u8) -> String {
    if lo == hi {
        format!("{{{lo}}}")
    } else {
        format!("{{{lo},{hi}}}")
    }
}

/// Escape regex metacharacters (the same set `regex::escape` escapes).
pub fn regex_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if "\\.+*?()|[]{}^$#&-~".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// A value's segments: maximal ASCII-alphanumeric runs, and single other characters.
pub fn segments(v: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    for (i, c) in v.char_indices() {
        if c.is_ascii_alphanumeric() {
            start.get_or_insert(i);
        } else {
            if let Some(s) = start.take() {
                out.push(&v[s..i]);
            }
            out.push(&v[i..i + c.len_utf8()]);
        }
    }
    if let Some(s) = start {
        out.push(&v[s..]);
    }
    out
}

fn is_alnum_seg(s: &str) -> bool {
    s.as_bytes().first().is_some_and(|b| b.is_ascii_alphanumeric())
}

fn class_seg(class: Class, lo: u8, hi: u8, s: &str) -> bool {
    is_alnum_seg(s) && (lo as usize..=hi as usize).contains(&s.len()) && s.chars().all(|c| class.contains(c))
}

/// Whether one branch matches the segments exactly: an NFA simulation over whole segments. A
/// state is (run index, repeats done by the star at that index, inside a star group after its
/// separator). The state set is bounded by the type's size, so the cost is O(segments × runs),
/// linear in the value's length for a bounded type.
fn branch_matches(b: &[Run], segs: &[&str]) -> bool {
    use std::collections::BTreeSet;
    type State = (usize, u8, bool);
    // Epsilon closure: a star that has done at least `min` repeats may be left.
    let close = |set: &mut BTreeSet<State>| {
        let mut todo: Vec<State> = set.iter().copied().collect();
        while let Some((i, k, mid)) = todo.pop() {
            if let (false, Some(Run::Star { min, .. })) = (mid, b.get(i)) {
                if k >= *min && set.insert((i + 1, 0, false)) {
                    todo.push((i + 1, 0, false));
                }
            }
        }
    };
    let mut cur: BTreeSet<State> = BTreeSet::from([(0, 0, false)]);
    close(&mut cur);
    for seg in segs {
        let mut next: BTreeSet<State> = BTreeSet::new();
        for &(i, k, mid) in &cur {
            let ok = match b.get(i) {
                None => None,
                Some(Run::Lit { text }) => (seg == text).then_some((i + 1, 0, false)),
                Some(Run::Set { texts }) => {
                    (is_alnum_seg(seg) && texts.iter().any(|t| t == seg)).then_some((i + 1, 0, false))
                }
                Some(Run::Class { class, lo, hi }) => class_seg(*class, *lo, *hi, seg).then_some((i + 1, 0, false)),
                Some(Run::Star { sep, class, lo, hi, max, .. }) => {
                    if mid {
                        class_seg(*class, *lo, *hi, seg).then_some((i, k + 1, false))
                    } else {
                        (k < *max && seg.len() == sep.len_utf8() && seg.starts_with(*sep)).then_some((i, k, true))
                    }
                }
            };
            if let Some(s) = ok {
                next.insert(s);
            }
        }
        if next.is_empty() {
            return false;
        }
        close(&mut next);
        cur = next;
    }
    cur.contains(&(b.len(), 0, false))
}

/// Whether the type accepts `v`. No regex is compiled: O(len) for a bounded type.
pub fn matches(runs: &Runs, v: &str) -> bool {
    if v.is_empty() || v.len() > MAX_LEN || runs.branches.is_empty() {
        return false;
    }
    let segs = segments(v);
    runs.branches.iter().any(|b| branch_matches(b, &segs))
}

/// log2 of the number of strings the type accepts (an upper bound for a union).
pub fn log2_size(runs: &Runs) -> f64 {
    let sizes: Vec<f64> = runs.branches.iter().map(|b| b.iter().map(Run::log2_size).sum()).collect();
    let top = sizes.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    if !top.is_finite() {
        return 0.0;
    }
    top + sizes.iter().map(|s| (s - top).exp2()).sum::<f64>().log2()
}

/// The longest string the type accepts.
pub fn max_len(runs: &Runs) -> usize {
    runs.branches.iter().map(|b| b.iter().map(Run::max_len).sum()).max().unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lit(s: &str) -> Run {
        Run::Lit { text: s.into() }
    }

    #[test]
    fn prefix_dash() {
        let t = Runs { branches: vec![vec![lit("BIL"), lit("-"), Run::Class { class: Class::D, lo: 3, hi: 4 }]] };
        assert_eq!(to_regex(&t), r"^(?:BIL\-[0-9]{3,4})$");
        assert!(matches(&t, "BIL-104"));
        assert!(matches(&t, "BIL-1041"));
        assert!(!matches(&t, "BIL-10411"));
        assert!(!matches(&t, "BIL-1041;"));
        assert!(!matches(&t, "-BIL-1041"));
        assert!(!matches(&t, "BIM-1041"));
        assert!(!matches(&t, "BIL-١٢٣"));
    }

    #[test]
    fn stars_and_sets() {
        // fix/<word>(-<word>){0,1}-<n>
        let t = Runs {
            branches: vec![vec![
                Run::Set { texts: vec!["feat".into(), "fix".into()] },
                lit("/"),
                Run::Class { class: Class::L, lo: 3, hi: 6 },
                Run::Star { sep: '-', class: Class::L, lo: 3, hi: 6, min: 0, max: 1 },
                lit("-"),
                Run::Class { class: Class::D, lo: 1, hi: 3 },
            ]],
        };
        for ok in ["fix/login-42", "feat/cache-retry-7", "fix/abc-1"] {
            assert!(matches(&t, ok), "{ok}");
        }
        for bad in ["fix/login", "fix/a-b-c-1", "fix/cache-retry-queue-7", "fox/login-42", "fix/login-42-"] {
            assert!(!matches(&t, bad), "{bad}");
        }
        // 2 · 26^6 · (1 + 26^6) · 1110 strings: a two-word slug is past the 2^64 cap.
        assert!(log2_size(&t) > 64.0 && log2_size(&t) < 68.0);
    }

    #[test]
    fn segments_split() {
        assert_eq!(segments("ab-12/x"), ["ab", "-", "12", "/", "x"]);
        assert_eq!(segments("é1"), ["é", "1"]);
    }
}
