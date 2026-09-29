//! The mandatory safety constraints on a learned type (Track G §4.2). A type that fails any of
//! them is rejected, never weakened into something that passes.
//!
//! - anchored `^…$` (by construction: [`adage_core::vtype::to_regex`]);
//! - its alphabet stays inside the runtime's `plain_token` (ASCII alphanumerics and `_./:@=+,-`),
//!   and no accepted value can start with `-`;
//! - no accepted value is longer than 128 bytes;
//! - it is not **word-like**: no branch is a lone letter-capable class run (a bare slug, a bare
//!   code), and the language has at most 2^64 strings. One refinement of the design's cap: a
//!   **rigid** type (every run a fixed-length digit or hex class or a literal, at least two
//!   punctuation literals, e.g. a UUID at 2^122) is exempt from the size cap, because it cannot
//!   spell a word or a sentence fragment. Without it no UUID could ever be a type;
//! - it rejects every negative.

use adage_core::vtype::{log2_size, matches, max_len, Class, Run, Runs, MAX_LEN};
use serde::{Deserialize, Serialize};

/// Characters a value may contain besides ASCII alphanumerics (the runtime's `plain_token`).
pub const PLAIN_PUNCT: &str = "_./:@=+,-";
/// The language-size cap, in bits.
pub const MAX_LOG2_SIZE: f64 = 64.0;

/// Why no type was learned for a slot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "reason", content = "detail", rename_all = "snake_case")]
pub enum Reject {
    /// Fewer than two distinct safe positive values.
    TooFew,
    /// More than three skeletons that do not fold.
    NoShape,
    /// A character outside `plain_token`, or a possible leading `-`.
    Alphabet(String),
    TooLong,
    WordLike(String),
    /// A negative the least general type still accepts.
    AcceptsNegative(String),
}

/// A value the runtime could bind at all: `plain_token`, at most 128 bytes, no leading `-`.
pub fn plain_value(v: &str) -> bool {
    !v.is_empty()
        && v.len() <= MAX_LEN
        && !v.starts_with('-')
        && v.chars().all(|c| c.is_ascii_alphanumeric() || PLAIN_PUNCT.contains(c))
}

fn first_chars(b: &[Run]) -> Vec<char> {
    let mut out = Vec::new();
    for r in b {
        match r {
            Run::Lit { text } => {
                out.extend(text.chars().next());
                return out;
            }
            Run::Set { .. } | Run::Class { .. } => {
                out.push('a');
                return out;
            }
            Run::Star { sep, min, .. } => {
                out.push(*sep);
                if *min > 0 {
                    return out;
                }
            }
        }
    }
    out
}

fn rigid(b: &[Run]) -> bool {
    let punct =
        b.iter().filter(|r| matches!(r, Run::Lit { text } if !text.chars().all(|c| c.is_ascii_alphanumeric()))).count();
    punct >= 2
        && b.iter().all(|r| match r {
            Run::Lit { .. } => true,
            Run::Class { class, lo, hi } => lo == hi && matches!(class, Class::D | Class::H),
            _ => false,
        })
}

fn word_like(b: &[Run]) -> bool {
    match b {
        [Run::Class { class, lo, .. }] => match class {
            Class::D => false,
            Class::H => *lo < 7,
            _ => true,
        },
        [Run::Star { class, .. }] => class.has_letters(),
        _ => false,
    }
}

/// Check every constraint except the negatives.
pub fn check_shape(runs: &Runs) -> Result<(), Reject> {
    for b in &runs.branches {
        for r in b {
            let ok = match r {
                Run::Lit { text } => text.chars().all(|c| c.is_ascii_alphanumeric() || PLAIN_PUNCT.contains(c)),
                Run::Set { texts } => texts.iter().all(|t| t.chars().all(|c| c.is_ascii_alphanumeric())),
                Run::Class { .. } => true,
                Run::Star { sep, .. } => PLAIN_PUNCT.contains(*sep),
            };
            if !ok {
                return Err(Reject::Alphabet(format!("{r:?}")));
            }
        }
        if first_chars(b).contains(&'-') {
            return Err(Reject::Alphabet("a value could start with '-'".into()));
        }
        if word_like(b) {
            return Err(Reject::WordLike("a bare letter run".into()));
        }
    }
    if max_len(runs) > MAX_LEN {
        return Err(Reject::TooLong);
    }
    let size = log2_size(runs);
    if size > MAX_LOG2_SIZE && !runs.branches.iter().all(|b| rigid(b)) {
        return Err(Reject::WordLike(format!("language of 2^{size:.1} strings")));
    }
    Ok(())
}

/// The first negative the type accepts, if any.
pub fn accepted_negative<'a>(runs: &Runs, negatives: impl IntoIterator<Item = &'a String>) -> Option<&'a String> {
    negatives.into_iter().find(|n| matches(runs, n))
}
